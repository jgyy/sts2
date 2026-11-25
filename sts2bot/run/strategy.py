"""Out-of-combat decisions: map, rewards, shop, rest sites, events, selections. Every handler returns an ordered list of
candidate actions; the main loop takes the first one that the game accepts, so a handler can list fallbacks."""

from __future__ import annotations

import logging
import random
import re
from dataclasses import dataclass, field

from ..learn import knob
from .potions import boss_floors, boss_worth, saving
from .relicworth import worth as relic_worth
from .events import _event_option_value, event  # noqa: F401  (events live in their own module)
from ..cards import _card_key, deck_value, is_basic, is_junk, kind as _kind, parse as _parse
from .decks import BIG_DECK, SKIP_THRESHOLD, _blocks, _hitters, _punch, _stars, deck_fit, reward_bar  # noqa: F401  (what a card is worth in this deck: its own module)
from ..fight import afflictions, handpick

Action = tuple[str, dict]
log = logging.getLogger("sts2bot.strategy")
POTION_SWAP = 6.0  # how much more a potion on offer must be worth at a boss than the least of a full belt to take its place


@dataclass
class Memory:
    """The few things the game state does not tell us."""
    deck_size: int = 10
    deck: list = field(default_factory=list)  # our cards as last seen at the start of a fight, plus picks since
    max_energy: int = 3  # energy a turn, as the last fight began with
    picks: list = field(default_factory=list)  # (floor, card name) for every card reward taken this run
    card_stats: object = None  # learn.CardStats: how cards have done in our own runs
    card_skips: dict = field(default_factory=dict)  # (act, floor) -> skipped card rewards
    select_key: str = ""
    logged_select: str = ""  # the "choose a card" screen last written to the log
    logged_shop: str = ""  # the shop shelf last written to the log
    selected: set = field(default_factory=set)
    runs_finished: int = 0
    character_selected: bool = False
    boss_next: bool = False  # the room we are walking into leads straight to the boss
    event_id: str = ""  # the event on screen (its id), to tell one event from the next
    event_hp0: int = 0  # our HP when that event began: what it has cost us so far
    map_nodes: dict = field(default_factory=dict)  # the act's map as last seen: (col, row) -> node
    map_at: tuple = ()  # the node we walked to
    upgrade_gain: object = None  # callable(card) -> what its upgrade adds, or None if unknown
    over_waits: int = 0  # looks at an end-of-run summary that is still animating
    max_ascension: dict = field(default_factory=dict)  # character id -> highest unlocked ascension
    ascension_waits: int = 0  # times we held off embarking for the ascension level to show
    epochs: dict = field(default_factory=dict)  # epoch id -> state, from the game's profile
    last_result: str = ""  # "win" or "loss": how the run on the end screen went (set when it is recorded)
    recent: list = field(default_factory=list)  # the characters of the last runs played, oldest first (for the rotation)
    timeline_done: bool = False  # the Timeline has nothing more to reveal this visit
    timeline_stuck: set = field(default_factory=set)  # epochs it could not reveal (not retried)
    parasol: tuple = ()  # the shop Lord's Parasol is emptying, and how often we have looked at it: (act, floor, looks)
    potion_swaps: dict = field(default_factory=dict)  # (act, floor) -> potions poured away there for a better one

    def reset_run(self) -> None:
        self.deck_size, self.deck, self.max_energy, self.picks, self.map_nodes, self.map_at, self.over_waits = 10, [], 3, [], {}, (), 0
        self.select_key, self.character_selected, self.ascension_waits = "", False, 0
        self.card_skips.clear()
        self.potion_swaps.clear()
        self.selected.clear()


def _where(state: dict) -> tuple:
    run = state.get("run") or {}
    return (run.get("act"), run.get("floor"))


def _vitals(state: dict) -> tuple[dict, int, int]:
    """(the player, HP, Max HP)."""
    p = state.get("player") or {}
    return p, int(p.get("hp") or 0), max(1, int(p.get("max_hp") or 1))


def _option_names(options) -> list[str]:
    """Menu options come either as strings or as {name, enabled} objects."""
    return [str(o if isinstance(o, str) else o["name"]) for o in options or [] if isinstance(o, str) or (isinstance(o, dict) and o.get("enabled", True) and o.get("name"))]


# --------------------------------------------------------------------- menu
def menu(state: dict, mem: Memory, character: str) -> list[Action]:
    screen = state.get("menu_screen") or "main"
    names = _option_names(state.get("options"))
    lower = {n.lower(): n for n in names}

    def pick(*wanted: str) -> list[Action]:
        return [("menu_select", {"option": lower[w]}) for w in wanted if w in lower]

    if screen == "main":
        mem.timeline_done = False
        # Unlocks only count once revealed on the Timeline (until then the game may not even offer Singleplayer): reveal them first.
        pending = set(state.get("pending_epoch_ids") or [])
        if "timeline" in lower and pending - mem.timeline_stuck and "continue" not in lower:
            return pick("timeline", "singleplayer")
        return pick("continue", "singleplayer") or pick("timeline")
    if screen == "timeline":
        return pick("back", "advance") if mem.timeline_done else pick("advance", "back")
    if screen == "singleplayer":
        return pick("standard")
    if screen == "character_select":
        ids = [str(c.get("id")) for c in state.get("characters") or [] if not c.get("locked")]
        want = choose_character(character, ids, mem.runs_finished, mem.max_ascension, mem.epochs, mem.recent)
        select = [("menu_select", {"option": want})] if want else []
        embark = pick("confirm", "embark")
        if not mem.character_selected:
            return select + embark
        # Every run is played at the highest ascension the character has unlocked: a win there unlocks the next.
        top, now = state.get("max_ascension"), state.get("ascension")
        raise_asc: list[Action] = [("set_ascension", {"level": top})] if isinstance(top, int) and isinstance(now, int) and now != top else []
        # The levels of a character just picked show a moment later (embarking before then starts at ascension 0): wait, not for ever.
        known = {str(k).upper(): int(v) for k, v in mem.max_ascension.items()}.get(str(want).upper(), 0)
        patience = 4 if isinstance(top, int) else 20
        if known > 0 and not raise_asc and now != known and mem.ascension_waits < patience:
            mem.ascension_waits += 1
            return select
        return raise_asc + embark + select
    if screen == "tutorial_prompt":
        return pick("no")
    if screen == "tutorial":
        return pick("advance", "proceed")
    if screen == "popup":
        return pick("yes", "confirm", "ok", "continue") or [("menu_select", {"option": n}) for n in names]
    # Timeline, settings, compendium, multiplayer screens: get out.
    return pick("back", "advance")


_EARNED = ("Obtained", "ObtainedNoSlot", "Revealed")


def timeline_complete(epochs: dict | None) -> bool:
    """Every epoch the profile lists has been earned (False when it lists none: a game without a Timeline, or not asked yet)."""
    return bool(epochs) and all(state in _EARNED for state in epochs.values())


def missing_epochs(character: str, epochs: dict | None) -> int:
    """How many of this character's own epochs (IRONCLAD2_EPOCH, ...) are still to be earned."""
    prefix = re.compile(rf"^{re.escape(character.upper())}\d+_EPOCH$")
    return sum(1 for eid, state in (epochs or {}).items() if prefix.match(str(eid).upper()) and state not in _EARNED)


_SESSION = random.SystemRandom().getrandbits(32)  # so a restart does not replay the same draws


def _who(name) -> str:
    """A character's name as the menu lists it ("THE_SILENT", "SILENT") and as runs.jsonl keeps it ("The Silent"), alike."""
    key = re.sub(r"[^A-Z]+", "_", str(name).upper()).strip("_")
    return key[4:] if key.startswith("THE_") else key


def choose_character(setting: str, unlocked: list[str], runs_finished: int, max_ascension: dict | None = None, epochs: dict | None = None,
                     recent: list | None = None) -> str | None:
    """`setting` is a character id, or "auto" / "cycle" / "random" over the unlocked ones.
    "auto" first plays the characters with a Timeline epoch of their own still to earn; among those (or everyone) one on
    the lowest ascension unlocked: drawn at random once a run, or with `recent` (oldest first) the one played longest ago."""
    # The game's own "Random" tile is listed like a character, on ascension 0 for ever: never pick it.
    unlocked = [u for u in unlocked if not u.upper().startswith("RANDOM")]
    if not unlocked:
        return None
    by_upper = {u.upper(): u for u in unlocked}
    s = setting.strip().upper()
    if s in by_upper:
        return by_upper[s]
    if s.startswith("AUTO:"):
        # "auto:IRONCLAD": that character first, for as long as it has epochs of its own to earn.
        first = by_upper.get(s.split(":", 1)[1].strip())
        if first and missing_epochs(first, epochs) > 0:
            return first
        s = "AUTO"
    levels = {str(k).upper(): int(v) for k, v in (max_ascension or {}).items()}
    if s in ("AUTO", "RANDOM"):
        if s == "AUTO":
            # While any character still has an epoch of its own to earn, only those play.
            owed = [u for u in unlocked if missing_epochs(u, epochs) > 0]
            unlocked = owed or unlocked
        low = min(levels.get(u.upper(), 0) for u in unlocked)
        lowest = [u for u in unlocked if levels.get(u.upper(), 0) == low]
        if s == "AUTO" and recent:
            last = {_who(c): n for n, c in enumerate(recent)}
            longest = min(last.get(_who(u), -1) for u in lowest)
            lowest = [u for u in lowest if last.get(_who(u), -1) == longest]
        return random.Random(_SESSION + runs_finished).choice(lowest)
    if s == "CYCLE":
        return unlocked[runs_finished % len(unlocked)]
    return unlocked[0]


# ---------------------------------------------------------------------- map
_ROOM_BASE = {"Unknown": 2.2, "Treasure": 4.0, "Boss": 0.0, "Ancient": 3.0}  # (the other kinds are worked out below)
EARLY_ROWS = 8  # act 1 rows where the deck is still close to the starter deck


def _room_value(kind: str, hp: float, gold: int, early: bool = False, act: int | None = None) -> float:
    v = _ROOM_BASE.get(kind, 1.0)
    if kind == "Elite" and early:
        v = -5.0  # a starter deck loses about 35 HP to an act 1 elite: only when there is no other way
    elif kind == "Elite":
        v = 3.0 if hp >= 0.9 else (0.5 if hp >= 0.7 else -6.0)  # only from a strong position
        if act == 1 and hp >= 0.7:
            v = 3.5  # act 1 elites are cheap once the deck has a few cards, and their relics carry the run
    elif kind == "Monster":
        v = 1.5 if hp >= 0.45 else -1.0
    elif kind == "RestSite":
        v = 3.0 + (1.0 - hp) * 5.0
    elif kind == "Shop":
        v = 4.5 if gold >= 300 else 3.0 if gold >= 150 else (1.5 if gold >= 80 else 0.3)  # (gold is worth nothing at the end of a run)
    return v


FIGHT_COST = {1: {"Monster": 5, "Elite": 21}, 2: {"Monster": 12, "Elite": 30}, 3: {"Monster": 14, "Elite": 38}}  # HP a room costs on average (our runs)
LATE_ROW = 5  # from this row of acts 2 and 3 an ordinary fight costs more than the act's average, before it less
LATE_COST = {2: (0.85, 1.2), 3: (0.93, 1.08)}
BAD_LUCK = 1.7  # how much more than average a fight costs when the draws go wrong
BOSS_WORTH = 16.0  # what getting past the boss is worth, in the units of the room rewards below
# The first game's burning elite, which holds the Emerald Key for act 4 (games/sts1/keys.py): the key comes first (user, 2026-11-25:
# "focus more collecting all keys"), and in acts 1-2 its elite kills seldom (3 of 58 fights) where act 3's kill one in six.
EMERALD_WORTH = {1: 16.0, 2: 18.0, 3: 16.0}
MAX_PATHS = 1500
ELITE_FLOOR = 0.35  # share of Max HP under which an elite is never chosen while there is another way on
# The act 2 boss win chance by the HP it is met on, as a share of that from full HP (even HP above 85% counts there).
ACT2_ODDS = ((0.0, 0.0), (0.3, 0.05), (0.5, 0.15), (0.6, 0.23), (0.7, 0.42), (0.8, 0.68), (0.9, 0.86), (1.0, 1.0))
UNKNOWN_COST = {2: 0.03}  # what an Unknown room costs on average, as a share of Max HP (in act 2 a fight one time in five)


def _between(points, x: float) -> float:
    """Straight lines between (x, y) points, flat beyond the ends."""
    if x <= points[0][0]:
        return points[0][1]
    for (x0, y0), (x1, y1) in zip(points, points[1:]):
        if x <= x1:
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    return points[-1][1]


def boss_odds(hp_ratio: float, act: int = 1) -> float:
    """Chance of beating the boss given how healthy we arrive, next to arriving at full HP. Act 1: no difference from 85%
    up, hardly a win below 60%. Act 2: ACT2_ODDS. Act 3: the last boss is beaten from (nearly) full HP only."""
    if act == 2:
        return _between(ACT2_ODDS, hp_ratio)
    if act >= 3:
        return 1.0 if hp_ratio >= 0.95 else 0.3 + 0.7 * (hp_ratio - 0.8) / 0.15 if hp_ratio >= 0.8 else max(0.0, 0.3 * (hp_ratio - 0.5) / 0.3)
    return 1.0 if hp_ratio >= 0.85 else 0.45 + 0.55 * (hp_ratio - 0.6) / 0.25 if hp_ratio >= 0.6 else max(0.0, 0.45 * (hp_ratio - 0.3) / 0.3)


REST_HEAL = 0.3  # of Max HP
HEAL_BELOW = {1: 0.85, 2: 0.95}  # the rest site just before a boss heals below this share of Max HP (act 2 bosses lose ground even from 85-97%)
LIFT_HITTERS = 5  # cards that deal damage a deck needs before Girya's Strength beats an upgrade
UPGRADE_EARLY = 4.0  # what an upgrade counts for against resting in act 1 (later acts: the `upgrade_worth` setting, see learn.CHOICES)


def _upgrade_worth(act) -> float:
    return UPGRADE_EARLY if act == 1 else knob("upgrade_worth")


def path_value(rooms: list[tuple[str, int]], hp: float, max_hp: int, gold: int, act: int, harsh: float = 1.0) -> float:
    """Expected worth of walking through `rooms` (type, row) in order, starting at `hp`: rewards along the way weighed
    against the state we reach the boss in, and at each rest site whichever of resting and upgrading comes out ahead."""
    costs = FIGHT_COST.get(act, FIGHT_COST[3])
    score = 0.0
    for n, (kind, row) in enumerate(rooms):
        if kind == "EmeraldElite":
            score, kind = score + EMERALD_WORTH.get(act, EMERALD_WORTH[3]), "Elite"
        if kind in ("Monster", "Elite"):
            cost = costs[kind]
            if kind == "Elite" and act == 1 and isinstance(row, int) and row < EARLY_ROWS:
                cost = 32  # a starter deck against an elite
            if kind == "Elite" and hp < 0.75 * max_hp:
                # Elites met below three quarters HP end runs far more often than their average cost says.
                score -= 2.5 if hp >= 0.65 * max_hp else 6.0 if hp >= 0.5 * max_hp else 14.0
            if kind == "Elite" and act >= 3:
                score -= 6.0  # an act 3 elite ends the run one time in five whatever the HP, and its relic has one boss left
            if kind == "Monster" and act in LATE_COST and isinstance(row, int):
                cost *= LATE_COST[act][row >= LATE_ROW]
            if kind == "Monster" and act >= 2 and hp < 0.3 * max_hp:
                score -= 14.0 if act >= 3 else 10.0  # the same for an ordinary fight met badly hurt (a run ended is 30 here)
            elif kind == "Monster" and act >= 2 and hp < 0.6 * max_hp:
                score -= 3.0 if act >= 3 else 1.5
            hp -= cost * harsh
            score += knob("elite_worth") if kind == "Elite" else 1.0
            gold += 30 if kind == "Elite" else 15
            if hp <= 0:
                score -= 30.0  # this is where the run ends, on average
                hp = 1.0
            elif hp < 0.2 * max_hp:
                score -= 6.0  # one bad draw from dead
            elif hp < 0.35 * max_hp:
                score -= 2.0
            hp = min(hp, max_hp)
        elif kind == "RestSite":
            onward = rooms[n + 1 :]
            smith = _upgrade_worth(act) + path_value(onward, hp, max_hp, gold, act, harsh)
            if hp >= 0.95 * max_hp:
                return score + smith
            rest = path_value(onward, min(float(max_hp), hp + REST_HEAL * max_hp), max_hp, gold, act, harsh)
            return score + max(smith, rest)
        elif kind == "Treasure":
            score += 2.0
        elif kind == "Shop":
            score += 0.3 + min(8.0, gold / 120.0)  # gold is only worth what it buys
            gold = gold % 75
        elif kind == "Unknown":
            score += 0.9
            hp = max(1.0, hp - UNKNOWN_COST.get(act, 0.0) * max_hp * harsh)
        elif kind == "Ancient":
            score += 3.0
        elif kind == "Boss":
            score += BOSS_WORTH * boss_odds(hp / max(1, max_hp), act)
    return score


def best_onward(nodes: dict, key, rooms: list, hp: float, max_hp: int, gold: int, act: int, budget: list) -> float:
    """Best value over every way on from node `key` (`rooms` is the way so far, that node included)."""
    kids = [tuple(c) for c in (nodes.get(key) or {}).get("children") or [] if tuple(c) in nodes]
    if not kids or budget[0] <= 0:
        budget[0] -= 1
        # A route is also judged by how it goes when fights cost a good deal more than average.
        return 0.6 * path_value(rooms, hp, max_hp, gold, act) + 0.4 * path_value(rooms, hp, max_hp, gold, act, BAD_LUCK)
    return max(best_onward(nodes, k, rooms + [(str(nodes[k].get("type")), k[1])], hp, max_hp, gold, act, budget) for k in kids)


def map_screen(state: dict, mem: Memory) -> list[Action]:
    m = state.get("map") or {}
    options = m.get("next_options") or []
    if not options:
        return []
    player, hp_now, max_hp = _vitals(state)
    hp = hp_now / max_hp
    gold = int(player.get("gold") or 0)
    nodes = {(n.get("col"), n.get("row")): n for n in m.get("nodes") or []}
    act = (state.get("run") or {}).get("act")
    budget = [MAX_PATHS]

    def is_early(row) -> bool:
        return act == 1 and isinstance(row, int) and row < EARLY_ROWS

    def value(opt: dict) -> float:
        key = (opt.get("col"), opt.get("row"))
        if key in nodes:
            return best_onward(nodes, key, [(str(nodes[key].get("type")), key[1])], hp_now, max_hp, gold, act if isinstance(act, int) else 1, budget)
        kids = opt.get("leads_to") or []
        ahead = max((_room_value(str(k.get("type")), hp, gold, is_early(k.get("row")), act) for k in kids), default=0.0)
        return _room_value(str(opt.get("type")), hp, gold, is_early(opt.get("row")), act) + 0.9 * ahead

    scored = [(value(o), o) for o in options]
    if hp < ELITE_FLOOR and any("Elite" not in str(o.get("type")) for _, o in scored):
        # Badly hurt, every road "dies" on paper and the values no longer tell them apart: not an elite.
        scored = [(v - (1000.0 if "Elite" in str(o.get("type")) else 0.0), o) for v, o in scored]
    ranked = [o for _, o in sorted(scored, key=lambda t: t[0], reverse=True)]
    if log.isEnabledFor(logging.DEBUG) and len(scored) > 1:
        log.debug("map: %s", ", ".join(f"{o.get('type')}@{o.get('col')} {v:.1f}" for v, o in scored))
    best = ranked[0]
    kids = best.get("leads_to") or [{"type": (nodes.get(tuple(c)) or {}).get("type")} for c in (nodes.get((best.get("col"), best.get("row"))) or {}).get("children") or []]
    mem.boss_next = any(str(k.get("type")) == "Boss" for k in kids)
    mem.map_nodes, mem.map_at = nodes, (best.get("col"), best.get("row"))
    return [("choose_map_node", {"index": int(o.get("index", 0))}) for o in ranked]


# ------------------------------------------------------------------ rewards
def _belt_full(player: dict) -> bool:
    """No room for a potion: the belt is full, or a relic (Sozu) forbids potions (and the shop would sell one for nothing)."""
    for relic in player.get("relics") or []:
        if "no longer obtain potions" in str(relic.get("description") or "").lower():
            return True
    return len(player.get("potions") or []) >= int(player.get("max_potion_slots") or 3)


def rewards(state: dict, mem: Memory) -> list[Action]:
    r = state.get("rewards") or {}
    player = state.get("player") or {}
    belt_full = _belt_full(player)
    skips_left = mem.card_skips.get(_where(state), 0)
    out: list[Action] = []
    # Highest index first so earlier indices stay valid.
    for item in sorted(r.get("items") or [], key=lambda i: -int(i.get("index", 0))):
        kind = str(item.get("type"))
        if kind == "potion" and belt_full:
            # A potion better than the least on the belt: pour that one away (twice a room at most, so a failed discard cannot loop).
            held = player.get("potions") or []
            worst = min(held, key=boss_worth, default=None)
            full = len(held) >= int(player.get("max_potion_slots") or 3)  # (and not Sozu, which lets none be taken)
            if full and worst is not None and boss_worth(item) >= boss_worth(worst) + POTION_SWAP and mem.potion_swaps.get(_where(state), 0) < 2:
                log.debug("potion: %s poured away for %s", worst.get("name"), item.get("potion_name"))
                out.append(("discard_potion", {"slot": int(worst.get("slot", 0))}))
            continue
        if kind == "card" and skips_left > 0:
            skips_left -= 1
            continue
        out.append(("claim_reward", {"index": int(item.get("index", 0))}))
    return out + [("proceed", {})]


def card_reward(state: dict, mem: Memory) -> list[Action]:
    cr = state.get("card_reward") or {}
    cards = cr.get("cards") or []
    act = (state.get("run") or {}).get("act")
    who = str((state.get("player") or {}).get("character") or "")

    def worth(c: dict) -> float:
        v = deck_value(c) + deck_fit(c, mem.deck, act if isinstance(act, int) else None, mem.max_energy)
        if mem.card_stats is not None:
            v += mem.card_stats.shift(who, str(c.get("name")), _kind(c))
        # The starter deck cannot kill anything quickly: an attack first (deck_fit sees to it once the deck is known).
        if not mem.deck and act == 1 and mem.deck_size < 18 and str(c.get("type")) == "Attack" and v > 0:
            v += 0.8
        return v

    ranked = sorted(cards, key=worth, reverse=True)
    if cr.get("in_combat"):
        # A card a potion (or Discovery) offers in a fight is for this fight only: the one the turn makes most of, never none (2026-11-25:
        # the Silent drank an Attack Potion on round 1 of Donu and Deca and skipped all three for "need 4.0").
        ranked = afflictions.offered(ranked, state.get("_fight_state"))
        log.debug("card offer in a fight: %s", ", ".join(str(c.get("name")) for c in ranked))
        return [("select_card_reward", {"card_index": int(c.get("index", 0))}) for c in ranked] + [("skip_card_reward", {})]
    threshold = reward_bar(mem.deck_size, act)
    if log.isEnabledFor(logging.DEBUG):
        log.debug("card reward (deck %d, need %.1f): %s", mem.deck_size, threshold, ", ".join(f"{c.get('name')} {worth(c):.1f}" for c in ranked))
    take = [("select_card_reward", {"card_index": int(c.get("index", 0))}) for c in ranked]
    skip: list[Action] = [("skip_card_reward", {})]
    if ranked and worth(ranked[0]) >= threshold:
        return take + (skip if cr.get("can_skip", True) else [])
    return skip + take


# ---------------------------------------------------------------- rest site
def rest_site(state: dict, mem: Memory) -> list[Action]:
    rs = state.get("rest_site") or {}
    opts = [o for o in rs.get("options") or [] if o.get("is_enabled", True)]
    player, hp_now, max_hp = _vitals(state)
    hp = hp_now / max_hp
    # Rest or upgrade: whichever leaves the way to the boss better off (without the map, a rule of thumb; HEAL_BELOW before a boss).
    act = (state.get("run") or {}).get("act")
    full = HEAL_BELOW.get(act, 0.95) if isinstance(act, int) else 0.95
    heal_first = hp < (full if mem.boss_next else 0.6)
    node = mem.map_nodes.get(mem.map_at) if mem.map_nodes else None
    if node is not None and str(node.get("type")) == "RestSite" and (node.get("children") or []):
        args = (max_hp, int(player.get("gold") or 0), act if isinstance(act, int) else 1)
        smith = _upgrade_worth(args[2]) + best_onward(mem.map_nodes, mem.map_at, [], hp_now, *args, [MAX_PATHS])
        healed = min(float(max_hp), hp_now + REST_HEAL * max_hp)
        rest = best_onward(mem.map_nodes, mem.map_at, [], healed, *args, [MAX_PATHS])
        # Below half HP the path values hardly tell 6 HP from 40 (no boss is won under 30%): heal then, and before a boss.
        heal_first = hp < 0.5 or (rest > smith and hp < 0.95) or (mem.boss_next and hp < full)
        log.debug("rest site at %d/%d: rest %.1f, upgrade %.1f", hp_now, max_hp, rest, smith)

    def rank(o: dict) -> float:
        oid = str(o.get("id") or o.get("name") or "").lower()
        if "rest" in oid or "heal" in oid:
            return 10.0 if heal_first else 1.0
        if "smith" in oid or "upgrade" in oid:
            return 5.0
        if oid == "recall":
            return 5.5  # the first game's Ruby Key, offered only while it is missing (games/sts1/keys.py): over an upgrade, not over a needed rest
        # A relic's option beats the upgrade: Girya's Lift (Strength, for a deck that hits), Shovel's Dig, Pumpkin Candle's Kindle.
        if "lift" in oid:
            hitters = sum(1 for c in mem.deck if _parse(c).damage > 0 and not _parse(c).osty_attack)
            return 6.0 if not mem.deck or hitters >= LIFT_HITTERS else 4.0
        if "dig" in oid or "kindle" in oid:
            return 6.0
        return 3.0

    out = [("choose_rest_option", {"index": int(o.get("index", 0))}) for o in sorted(opts, key=rank, reverse=True)]
    return [("proceed", {})] + out if rs.get("can_proceed") else out + [("proceed", {})]


# --------------------------------------------------------------------- shop
REMOVAL_EARLY = {1: 3.5, 2: 1.5}  # what a card removal is worth less in acts 1 and 2 under the `cards_first` setting
RELIC_BAR = 5.0  # a shop relic wanted less than this (a fair card) is bought only with 200 Gold to spare
SEAL_RESERVE = 45  # Gold a Merchant does not get while Seal of Gold is held (3 a turn for an energy: a boss fight is 10 to 15 turns)
# What the Fake Merchant's relics are worth next to a real one at 7, by what their text does over a run (Snecko Eye???: confusion for nothing).
_FAKES = {"FAKE_ANCHOR": 5.0, "FAKE_ORICHALCUM": 4.0, "FAKE_HAPPY_FLOWER": 3.5, "FAKE_VENERABLE_TEA_SET": 3.0, "FAKE_BLOOD_VIAL": 3.0, "FAKE_LEES_WAFFLE": 3.0,
          "FAKE_MANGO": 2.5, "FAKE_STRIKE_DUMMY": 1.5, "FAKE_SNECKO_EYE": -1.0}


def _shop_card(item: dict) -> dict:
    return {k: item.get("card_" + k) for k in ("id", "name", "type", "cost", "star_cost", "rarity", "description")}


def shop(state: dict, mem: Memory, key: str = "shop") -> list[Action]:
    s = state.get(key) or {}
    if key == "fake_merchant":
        s = s.get("shop") or {}
    if s.get("error"):
        return []
    player = state.get("player") or {}
    gold = int(player.get("gold") or 0)
    belt_full = _belt_full(player)
    act, floor = _where(state)
    if any("everything he sells" in str(r.get("description") or "").lower() for r in player.get("relics") or []):
        # Lord's Parasol empties the shelf by itself and a purchase of ours meanwhile crashes the game: buy nothing, leave when it is done.
        looks = mem.parasol[2] + 1 if mem.parasol[:2] == _where(state) else 1
        mem.parasol = (*_where(state), looks)
        return [("proceed", {})] if looks > 8 or not any(i.get("is_stocked", True) for i in s.get("items") or []) else []
    reserve = SEAL_RESERVE if any(str(r.get("id")) == "SEAL_OF_GOLD" for r in player.get("relics") or []) else 0
    scored: list[tuple[float, dict]] = []
    shelf: list[str] = []  # (for the log: what was on offer, what it cost and what we made of it)
    for item in s.get("items") or []:
        price = int(item.get("price", item.get("cost", 0)) or 0)
        if not item.get("is_stocked", True):
            continue
        cat = str(item.get("category"))
        rated = None  # (a card's rating, for the log, when it is under the bar and so not wanted)
        if cat == "card_removal":
            # From 150 Gold a removal has to beat the relics on the shelf, and early in a run the cards too (`cards_first`).
            want = 9.0 if price <= 125 else 7.6 - (price - 100) / 100.0
            want -= knob("cards_first") * REMOVAL_EARLY.get(act, 0.0)
        elif cat == "relic":
            # By what the relic does for this deck (relicworth.py); one worth less than a fair card only out of plenty.
            want = 8.0 - price / 200.0 + relic_worth(str(item.get("relic_id") or ""), mem.deck, act, int(player.get("hp") or 0), int(player.get("max_hp") or 0))
            want = want if want >= RELIC_BAR or gold - price >= 200 else -1.0
            if str(item.get("relic_id") or "").startswith("FAKE_"):
                # The Fake Merchant's copies do a fraction of what their names promise, for the same gold a real Merchant takes.
                want = _FAKES.get(str(item.get("relic_id")), 2.0)
                want = want if want >= 3.5 or gold - price >= 200 else -1.0
        elif cat == "card":
            v = deck_value(_shop_card(item)) + deck_fit(_shop_card(item), mem.deck, act if isinstance(act, int) else None, mem.max_energy)
            if mem.card_stats is not None:  # (what our own runs say of the card and of its kind, as at a card reward)
                v += mem.card_stats.shift(str(player.get("character") or ""), str(item.get("card_name")), _kind(_shop_card(item)))
            # Only cards that make the deck better, and more freely when rich.
            bar = 4.0 if gold - price >= 200 else 4.5 if mem.deck_size < BIG_DECK else 5.5
            want, rated = (v if v >= bar else -1.0), v
        elif cat == "potion":
            # Within reach of a boss (potions.SAVE_FLOORS, and all of act 3) a potion is the best thing gold buys.
            boss_next = saving(floor)
            boss_next = boss_next or (isinstance(floor, int) and floor > boss_floors()[1])
            if belt_full:
                want = -1.0
            elif boss_next:
                # Before a removal while the belt holds fewer than two, and the one worth most at that boss first.
                want = 9.5 if len(player.get("potions") or []) < 2 else 7.0
                want += min(1.0, boss_worth(item) / 60.0)
            else:
                want = 2.0 if gold - price >= 60 else -1.0
        else:
            want = -1.0
        afford = bool(item.get("can_afford", price <= gold)) and (not reserve or gold - price >= reserve)
        shelf.append(f"{cat} {item.get('card_name') or item.get('relic_name') or item.get('potion_name') or ''} {price}{'' if afford else '!'} [{want if rated is None else rated:.1f}]".replace("  ", " "))
        if want > 0 and afford:
            scored.append((want, item))
    shown = f"{_where(state)} {gold}: " + ", ".join(shelf)
    if log.isEnabledFor(logging.DEBUG) and shown != mem.logged_shop:
        mem.logged_shop = shown
        log.debug("shop (gold %d, potions %d of %d): %s", gold, len(player.get("potions") or []), int(player.get("max_potion_slots") or 3), ", ".join(shelf))
    scored.sort(key=lambda t: -t[0])
    return [("shop_purchase", {"index": int(i.get("index", 0))}) for _, i in scored] + [("proceed", {})]


# --------------------------------------------------------------- selections
_WORST_FIRST = re.compile(r"remove|transform|exhaust|discard|lose|purge|sacrifice|destroy", re.I)
KEEP_AT_ALL_COSTS = {"FRANTIC_ESCAPE"}  # the only way out of the Sandpit


def _removal_rank(card: dict) -> float:
    """Lower = get rid of it first."""
    if _card_key(card) in KEEP_AT_ALL_COSTS:
        return 1000.0  # (a Status card, so it went first when a card or relic asked for one)
    if is_junk(card):
        return -100.0
    v = deck_value(card)
    if is_basic(card):
        v -= 5.5 if _card_key(card).startswith("STRIKE") else 5.0  # a Strike before a Defend (the two rate alike)
    return v


def _pick_order(cards: list[dict], prompt: str, screen_type: str = "", gain=None) -> list[dict]:
    if _WORST_FIRST.search(prompt) or screen_type == "transform":
        return sorted(cards, key=_removal_rank)
    if "upgrade" in prompt.lower() or screen_type == "upgrade":
        fresh = [c for c in cards if not c.get("is_upgraded")]

        def rank(c: dict) -> float:
            # A good card we play often, and an upgrade that does a lot for it (capped, and little for a card we hardly play).
            g = gain(c) if gain is not None else None
            v = deck_value(c)
            if g is None:
                return v
            return 0.6 * v + min(g, 12.0) * (1.0 if v >= 2.5 else 0.3)

        ranked = sorted(fresh or cards, key=rank, reverse=True)
        if gain is not None and log.isEnabledFor(logging.DEBUG):
            log.debug("upgrade: %s", ", ".join(f"{c.get('name')} {rank(c):.1f}" for c in ranked[:5]))
        return ranked
    return sorted(cards, key=deck_value, reverse=True)


def card_select(state: dict, mem: Memory) -> list[Action]:
    cs = state.get("card_select") or {}
    prompt = str(cs.get("prompt") or "")
    kind = str(cs.get("screen_type") or "")
    cards = cs.get("cards") or []
    key = f"{_where(state)}|{kind}|{prompt}|{len(cards)}"
    if key != mem.select_key:
        mem.select_key, mem.selected = key, set()
    ordered = afflictions.order(_pick_order(cards, prompt, kind, mem.upgrade_gain), state)
    if kind == "choose":
        ordered = afflictions.offered(ordered, state.get("_fight_state"))  # (a card a potion offers mid-fight: by the turn)
        shown = key + "|" + ",".join(str(c.get("name")) for c in cards)
        if log.isEnabledFor(logging.DEBUG) and shown != mem.logged_select:
            mem.logged_select = shown
            log.debug("choose (%s): %s", prompt, " | ".join(f"{c.get('name')}: {c.get('description')}" for c in ordered))
        # One click picks and closes the screen; a click while it is still opening is ignored, so always offer it again.
        out = [("select_card", {"index": int(c.get("index", 0))}) for c in ordered]
        if cs.get("can_skip") or cs.get("can_cancel"):
            out.append(("cancel_selection", {}))
        return out

    indices = [int(c.get("index", 0)) for c in ordered]
    if not cs.get("preview_showing") and indices and all(i in mem.selected for i in indices):
        mem.selected = set()  # our clicks did not register; start over
    # When the prompt says how many, tick exactly that many before confirming (some screens report confirm ready too early).
    wanted = re.search(r"choose\s+(\d+)\s+card", prompt, re.I)
    ready = len(mem.selected) >= int(wanted.group(1)) if wanted and not cs.get("preview_showing") else bool(cs.get("preview_showing") or cs.get("can_confirm"))
    out: list[Action] = [("confirm_selection", {})] if ready else []
    out += [("select_card", {"index": i}) for i in indices if i not in mem.selected]
    if cs.get("can_skip") or cs.get("can_cancel"):
        out.append(("cancel_selection", {}))
    out.append(("confirm_selection", {}))
    return out


_PUT_BACK = re.compile(r"on top of your draw pile", re.I)


def _put_back_order(cards: list[dict], energy: int | None) -> list[dict]:
    """Which card of the hand to put on top of the draw pile (Photon Cut, Glimmer, Thinking Ahead): one that would not be
    played this turn anyway, the best of what we cannot pay for or of what is left over once the energy is spent."""
    def cost(c: dict) -> int:
        try:
            return int(c.get("cost"))
        except (TypeError, ValueError):
            return 0  # X-cost: playable with anything

    real = [c for c in cards if not is_junk(c)]
    if energy is None:
        return sorted(real, key=deck_value) + [c for c in cards if is_junk(c)]
    spare: list[dict] = [c for c in real if cost(c) > energy]
    left = energy
    for c in sorted((c for c in real if cost(c) <= energy), key=lambda c: -deck_value(c) / max(1, cost(c))):
        if cost(c) <= left:
            left -= cost(c)
        else:
            spare.append(c)
    spare.sort(key=deck_value, reverse=True)
    # Nothing to spare: a dead card costs a draw next turn, a live one costs its play now.
    rest = [c for c in cards if is_junk(c)] + sorted((c for c in real if c not in spare), key=deck_value)
    return spare + [c for c in rest if c not in spare]


def hand_select(state: dict, mem: Memory) -> list[Action]:
    hs = state.get("hand_select") or {}
    prompt = str(hs.get("prompt") or "")
    kind = "upgrade" if hs.get("mode") == "upgrade_select" else ""
    if _PUT_BACK.search(prompt) and not _WORST_FIRST.search(prompt):
        energy = (state.get("player") or {}).get("energy")
        ordered = _put_back_order(hs.get("cards") or [], int(energy) if isinstance(energy, (int, float)) else None)
        out = [("combat_confirm_selection", {})] if hs.get("can_confirm") else []
        if not hs.get("can_confirm") and log.isEnabledFor(logging.DEBUG):
            log.debug("hand select (%s): %s", prompt, ", ".join(str(c.get("name")) for c in ordered))
        out += [("combat_select_card", {"card_index": int(c.get("index", 0))}) for c in ordered]
        return out + [("combat_confirm_selection", {})]
    out: list[Action] = []
    if hs.get("can_confirm"):
        out.append(("combat_confirm_selection", {}))
    elif log.isEnabledFor(logging.DEBUG):
        log.debug("hand select (%s): %s", prompt, ", ".join(str(c.get("name")) for c in hs.get("cards") or []))
    ordered = _pick_order(hs.get("cards") or [], prompt, kind, mem.upgrade_gain)
    if re.search(r"\bmake free\b", prompt, re.I):
        # Touch of Insanity ("It is free to play this combat"): the dearest card, the best of those.
        ordered = sorted(ordered, key=lambda c: (str(c.get("type")) in ("Status", "Curse"), -_parse(c).cost))
    if re.search(r"\badd ethereal\b", prompt, re.I):
        # Sculpting Strike ("Add Ethereal to a card in your Hand"): on what the deck is better without, not on one Ethereal already.
        ethereal = lambda c: "ethereal" in str(c.get("description") or "").lower() or any(str(k.get("name")) == "Ethereal" for k in c.get("keywords") or [])
        ordered = sorted(ordered, key=lambda c: (ethereal(c), _removal_rank(c)))
    discarding = bool(re.search(r"\bdiscard\b", prompt, re.I))
    # A Sly card discarded from the hand is played for free: the best thing to throw away.
    sly = lambda c: discarding and any(str(k.get("name")) == "Sly" for k in c.get("keywords") or [])
    ordered = sorted((c for c in ordered if sly(c)), key=deck_value, reverse=True) + [c for c in ordered if not sly(c)]
    # Then the card this turn's plan misses least (unless only to be confirmed): a discard by the turn, an Exhaust unless it costs a kill or our life.
    costs = handpick.turn_costs(state, ordered, 1e-9 if discarding else handpick.RUINOUS / 2) if _WORST_FIRST.search(prompt) and not kind and not hs.get("can_confirm") else None
    if costs:
        def price(c: dict) -> float:
            cost = costs[int(c.get("index", 0))]
            if sly(c):
                return float("-inf")
            if discarding:
                return float("inf") if _card_key(c) in KEEP_AT_ALL_COSTS else float(round(cost / 2.0))
            return float(cost >= handpick.RUINOUS)  # (a Frantic Escape too, when every other card is the kill)

        ordered = sorted(ordered, key=price)  # (stable: equal prices keep the order above)
    return out + [("combat_select_card", {"card_index": int(c.get("index", 0))}) for c in ordered] + [("combat_confirm_selection", {})]


def bundle_select(state: dict, mem: Memory) -> list[Action]:
    bs = state.get("bundle_select") or {}
    out: list[Action] = []
    if bs.get("preview_showing") or bs.get("can_confirm"):
        out.append(("confirm_bundle_selection", {}))
    bundles = sorted(bs.get("bundles") or [], key=lambda b: sum(deck_value(c) for c in b.get("cards") or []), reverse=True)
    return out + [("select_bundle", {"index": int(b.get("index", 0))}) for b in bundles] + [("confirm_bundle_selection", {})]


def relic_select(state: dict, mem: Memory) -> list[Action]:
    rs = state.get("relic_select") or {}
    out: list[Action] = [("select_relic", {"index": int(r.get("index", 0))}) for r in rs.get("relics") or []]
    return out + [("skip_relic_selection", {})] if rs.get("can_skip") else out


def treasure(state: dict, mem: Memory) -> list[Action]:
    t = state.get("treasure") or {}
    relics = t.get("relics") or []
    if not relics and not t.get("can_proceed"):
        return []  # chest still opening
    return [("claim_treasure_relic", {"index": int(r.get("index", 0))}) for r in relics] + [("proceed", {})]


def crystal_sphere(state: dict, mem: Memory) -> list[Action]:
    cs = state.get("crystal_sphere") or {}
    if cs.get("can_proceed"):
        return [("crystal_sphere_proceed", {})]
    w, h = int(cs.get("grid_width") or 9), int(cs.get("grid_height") or 11)
    cells = sorted(cs.get("clickable_cells") or [], key=lambda c: abs(c.get("x", 0) - w // 2) + abs(c.get("y", 0) - h // 2))  # from the middle: big tools reveal most there
    return [("crystal_sphere_click_cell", {"x": c.get("x"), "y": c.get("y")}) for c in cells[:6]] + [("crystal_sphere_proceed", {})]


OVER_PATIENCE = 60  # looks at the end-of-run summary before giving up on it


def game_over(state: dict, mem: Memory) -> list[Action]:
    """Leave the end-of-run screen, by way of the run summary: that is where the game adds the run's score to the
    profile, and the epochs that unlock "by accumulating score" only ever come from there."""
    options = (state.get("game_over") or {}).get("options") or []
    if mem.last_result == "loss" and timeline_complete(mem.epochs) and "force_main_menu" in options:
        # With every epoch revealed the score has nothing left to buy: a death goes straight back to the menu.
        return [("menu_select", {"option": "force_main_menu"})]
    if "continue" in options:
        mem.over_waits = 0
        return [("menu_select", {"option": "continue"})]
    if "main_menu" in options:
        return [("menu_select", {"option": "main_menu"})]
    if "force_main_menu" in options:
        # The summary is still counting up: wait for its Main Menu button, but not for ever.
        mem.over_waits += 1
        return [("menu_select", {"option": "force_main_menu"})] if mem.over_waits > OVER_PATIENCE else []
    return [("menu_select", {"option": "main_menu"})]  # a mod without the summary patch


def overlay(state: dict, mem: Memory) -> list[Action]:
    return [("proceed", {}), ("confirm_selection", {}), ("cancel_selection", {})]
