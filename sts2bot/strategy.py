"""Out-of-combat decisions: map, rewards, shop, rest sites, events, selections.

Every handler returns an ordered list of candidate actions. The main loop
takes the first one that the game accepts, so a handler can list fallbacks.
"""

from __future__ import annotations

import logging
import random
import re
from dataclasses import dataclass, field

from .learn import knob
from .potions import BOSS_FLOORS, boss_worth
from .events import _event_option_value, event  # noqa: F401  (events live in their own module)
from .cards import _card_key, deck_value, is_basic, is_junk, kind as _kind, parse as _parse
from .decks import BIG_DECK, SKIP_THRESHOLD, _blocks, _hitters, _punch, _stars, deck_fit, reward_bar  # noqa: F401  (what a card is worth in this deck: its own module)
from . import afflictions, handpick

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
    timeline_done: bool = False  # the Timeline has nothing more to reveal this visit
    timeline_stuck: set = field(default_factory=set)  # epochs it could not reveal (not retried)
    parasol: tuple = ()  # the shop Lord's Parasol is emptying, and how often we have looked at it: (act, floor, looks)
    potion_swaps: dict = field(default_factory=dict)  # (act, floor) -> potions poured away there for a better one

    def reset_run(self) -> None:
        self.deck_size = 10
        self.deck = []
        self.max_energy = 3
        self.picks = []
        self.map_nodes = {}
        self.map_at = ()
        self.over_waits = 0
        self.card_skips.clear()
        self.potion_swaps.clear()
        self.select_key = ""
        self.selected.clear()
        self.character_selected = False
        self.ascension_waits = 0


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
        # Unlocks the game has awarded (ascension levels, characters, cards)
        # only count once they are revealed on the Timeline, and until then
        # the game may not even offer Singleplayer. Go and reveal them.
        pending = set(state.get("pending_epoch_ids") or [])
        if "timeline" in lower and pending - mem.timeline_stuck and "continue" not in lower:
            return pick("timeline", "singleplayer")
        return pick("continue", "singleplayer") or pick("timeline")
    if screen == "timeline":
        return pick("back", "advance") if mem.timeline_done else pick("advance", "back")
    if screen == "singleplayer":
        return pick("standard")
    if screen == "character_select":
        chars = [c for c in state.get("characters") or [] if not c.get("locked")]
        ids = [str(c.get("id")) for c in chars]
        want = choose_character(character, ids, mem.runs_finished, mem.max_ascension, mem.epochs)
        select = [("menu_select", {"option": want})] if want else []
        embark = pick("confirm", "embark")
        if not mem.character_selected:
            return select + embark
        # Every run is played at the highest level the character has unlocked:
        # a win there is what unlocks the next one.
        top, now = state.get("max_ascension"), state.get("ascension")
        raise_asc: list[Action] = [("set_ascension", {"level": top})] if isinstance(top, int) and isinstance(now, int) and now != top else []
        # The screen takes a moment to show the levels of the character just
        # picked. Embarking before it does started the run at ascension 0 even
        # though a higher one was unlocked, so wait for it (but not for ever).
        known = {str(k).upper(): int(v) for k, v in mem.max_ascension.items()}.get(str(want).upper(), 0)
        # (When the screen itself says the top level is lower, the unlock has
        # not been revealed on the Timeline yet; a short wait is enough then.)
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


def missing_epochs(character: str, epochs: dict | None) -> int:
    """How many of this character's own epochs (IRONCLAD2_EPOCH, ...) are still to be earned."""
    prefix = re.compile(rf"^{re.escape(character.upper())}\d+_EPOCH$")
    return sum(1 for eid, state in (epochs or {}).items() if prefix.match(str(eid).upper()) and state not in _EARNED)


_SESSION = random.SystemRandom().getrandbits(32)  # so a restart does not replay the same draws


def choose_character(setting: str, unlocked: list[str], runs_finished: int, max_ascension: dict | None = None, epochs: dict | None = None) -> str | None:
    """`setting` is a character id, or "auto" / "cycle" / "random" over the unlocked ones.

    "auto" first plays the characters that still have a Timeline epoch of
    their own to earn. Among those (or among everyone, once no such epoch is
    left, and always for "random") it plays a character with the lowest
    ascension unlocked, so that all of them climb evenly, and draws at random
    among those on that level (all asked for on 2026-11-18). The draw is
    fixed for a run, since the menu asks again on every poll.
    """
    # The game's own "Random" tile is listed like a character, on ascension 0
    # for ever: counted as one, it was the lowest and got picked every time.
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
            # Epochs come first (asked for on 2026-11-18): while any character
            # still has one of its own to earn, only those characters play.
            # Everything else on the Timeline (score, Ancients, modes) comes
            # with playing and winning, whoever plays.
            owed = [u for u in unlocked if missing_epochs(u, epochs) > 0]
            unlocked = owed or unlocked
        low = min(levels.get(u.upper(), 0) for u in unlocked)
        lowest = [u for u in unlocked if levels.get(u.upper(), 0) == low]
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
        # A starter deck loses about 35 HP to an act 1 elite, and several runs
        # ended right there. Only take one when there is no other way.
        v = -5.0
    elif kind == "Elite":
        # Elites are worth it only from a strong position; a starter deck
        # walking into one at ~80% HP came out with 18 HP.
        v = 3.0 if hp >= 0.9 else (0.5 if hp >= 0.7 else -6.0)
        if act == 1 and hp >= 0.7:
            # Act 1 elites turned out cheap once the deck has a few cards
            # (about 12 HP each) and their relics carry the rest of the run.
            v = 3.5
    elif kind == "Monster":
        v = 1.5 if hp >= 0.45 else -1.0
    elif kind == "RestSite":
        v = 3.0 + (1.0 - hp) * 5.0
    elif kind == "Shop":
        # Gold is worth nothing at the end of a run; the bot died holding 1000.
        v = 4.5 if gold >= 300 else 3.0 if gold >= 150 else (1.5 if gold >= 80 else 0.3)
    return v


# What a room costs in HP on average, measured over the bot's own runs
# (act -> room -> HP). A boss costs 35 to 50 HP even when it is beaten.
# (Ascension 2, 2026-11-18 evening: act 1 elites cost 19.7 HP over 19 fights, act 2
# elites 27.0 over 15 and 0.8 potions each; monsters 3.0, 7.8 and 10.6.)
# (Ascension 4, 2026-11-19: act 3 monsters cost 13.9 HP over 104 fights; the rest of the table still held.)
# (Ascension 5-6, 2026-11-21: act 2 monsters cost 12.2 HP over 527 fights, 10.1 in the first five rooms of
# the act and 14.5 after them; act 2 elites 30.2 over 49; act 1 elites 21.6 over 80. At 9 a fight the routes
# and rest sites of act 2 counted on a quarter more HP than they had: 20 of those 527 fights ended the run.)
FIGHT_COST = {1: {"Monster": 5, "Elite": 21}, 2: {"Monster": 12, "Elite": 30}, 3: {"Monster": 14, "Elite": 38}}
LATE_ROW = 5  # from this row of acts 2 and 3 an ordinary fight costs more than the act's average, before it less
LATE_COST = {2: (0.85, 1.2), 3: (0.93, 1.08)}
BAD_LUCK = 1.7  # how much more than average a fight costs when the draws go wrong
BOSS_WORTH = 16.0  # what getting past the boss is worth, in the units of the room rewards below
MAX_PATHS = 1500
ELITE_FLOOR = 0.35  # share of Max HP under which an elite is never chosen while there is another way on


def boss_odds(hp_ratio: float, act: int = 1) -> float:
    """Chance of beating the boss given how healthy we arrive, next to arriving at full HP.

    Below 60% HP a boss is hardly ever beaten (1 in 12); from 80% up it makes
    no difference that the results show. From 246 boss fights on Ascension 3
    and 4, acts 1 and 2 together, as a share of the win rate at full HP:
    90-97% HP 0.95, 80-90% 1.00, 70-80% 0.72, 60-70% 0.85 (8 fights), below
    that 0.15. (Until 2026-11-20 this was ((hp - 0.3) / 0.7) ** 1.6, fitted
    when half the bosses were still met below 60% HP: it put 80% HP at 0.58
    and 90% at 0.78, so 20 HP at the boss weighed more than three relics and
    an elite on offer was taken 5 times in 78 at 85% HP or more.)

    The last boss is another matter: of 37 met on Ascension 4 (to 2026-11-20) all six beaten were
    entered on 95% HP or more (6 in 23; none in 14 below), and on Ascension 5 none in 8 below 80%.
    So in act 3 the curve is steep right up to full health, and a route there is worth what HP it
    keeps (elites are not taken in act 3 anyway, so this costs no relics).
    """
    if act >= 3:
        if hp_ratio >= 0.95:
            return 1.0
        if hp_ratio >= 0.8:
            return 0.3 + 0.7 * (hp_ratio - 0.8) / 0.15
        return max(0.0, 0.3 * (hp_ratio - 0.5) / 0.3)
    if hp_ratio >= 0.85:
        return 1.0
    if hp_ratio >= 0.6:
        return 0.45 + 0.55 * (hp_ratio - 0.6) / 0.25
    return max(0.0, 0.45 * (hp_ratio - 0.3) / 0.3)


REST_HEAL = 0.3  # of Max HP
UPGRADE_EARLY = 4.0  # what an upgrade counts for against resting in act 1 (later acts: the `upgrade_worth` setting, see learn.CHOICES)


def _upgrade_worth(act) -> float:
    return UPGRADE_EARLY if act == 1 else knob("upgrade_worth")


def path_value(rooms: list[tuple[str, int]], hp: float, max_hp: int, gold: int, act: int, harsh: float = 1.0) -> float:
    """Expected worth of walking through `rooms` (type, row) in order, starting at `hp`.

    Rewards along the way (relics, cards, upgrades) are weighed against the
    state we reach the boss in: the old room-by-room scoring took an elite at
    78% HP two rooms before the boss and arrived there with 26 HP. At each
    rest site it takes whichever of resting and upgrading comes out ahead.
    """
    costs = FIGHT_COST.get(act, FIGHT_COST[3])
    score = 0.0
    for n, (kind, row) in enumerate(rooms):
        if kind in ("Monster", "Elite"):
            cost = costs[kind]
            if kind == "Elite" and act == 1 and isinstance(row, int) and row < EARLY_ROWS:
                cost = 32  # a starter deck against an elite
            if kind == "Elite" and hp < 0.75 * max_hp:
                # Elites met below three quarters HP killed 9 runs in 27 (2 in 101 above
                # it), and the average cost says nothing of that: a third of what the
                # rest of the run is worth, more when already below half.
                # (By band on Ascension 3-4: 65-75% HP 1 death in 8, 50-65% 3 in 8, below
                # half 5 in 5. Runs start on 80% HP now, so the top band is where most
                # elites are met.)
                score -= 2.5 if hp >= 0.65 * max_hp else 6.0 if hp >= 0.5 * max_hp else 14.0
            if kind == "Elite" and act >= 3:
                # An act 3 elite ends the run one time in five whatever HP it is met on, and its relic has
                # one boss left to be of use at. Of 16 met on Ascension 5-6 (2026-11-21, twelve of them on
                # 85% HP or more) three killed the run there, two of those from over 90% HP, and two more
                # left it on 2 and 9 HP to die in the next room; the rest cost 38 HP on average, and of
                # the nine runs that went on to the boss the four that got back to full HP won twice, the
                # five that did not never. (A run ended is priced at 30 here.)
                score -= 6.0
            if kind == "Monster" and act in LATE_COST and isinstance(row, int):
                cost *= LATE_COST[act][row >= LATE_ROW]
            if kind == "Monster" and act >= 2 and hp < 0.3 * max_hp:
                # The same for an ordinary fight met badly hurt. On Ascension 5-6 below 30% HP they
                # ended 13 runs in 32 in act 2 and 3 in 5 in act 3; at 30-60% 5 in 99 and 5 in 39;
                # above that 2 in 396 and 1 in 109. (Until 2026-11-21: 4 points in act 2 below 35%,
                # 8 in act 3, when the end of a run is priced at 30.)
                score -= 14.0 if act >= 3 else 10.0
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
            # Gold is only worth what it buys: one run walked past every shop
            # in two acts with 600 gold in hand.
            score += 0.3 + min(8.0, gold / 120.0)  # (to 8: a bot walked past shops with 1,016 gold)
            gold = gold % 75
        elif kind == "Unknown":
            score += 0.9
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
        # Fights do not cost their average: a route is also judged by how it
        # goes when they cost a good deal more. A route that only works out on
        # average led through two forced elites at 18 HP.
        # (No healing after fights is added: the measured costs include the Ironclad's
        # Burning Blood, and without it the other characters lose about the same.)
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
    if hp < ELITE_FLOOR and any(str(o.get("type")) != "Elite" for _, o in scored):
        # Badly hurt, every road "dies" on paper and the values no longer tell them apart: an elite was
        # taken on 14 of 76 HP at -67.1 over an Unknown room at -72.2. Of 9 elites met under half HP on
        # Ascension 7, 6 ended the run; an Unknown room is a fight one time in five.
        scored = [(v - (1000.0 if str(o.get("type")) == "Elite" else 0.0), o) for v, o in scored]
    ranked = [o for _, o in sorted(scored, key=lambda t: t[0], reverse=True)]
    if log.isEnabledFor(logging.DEBUG) and len(scored) > 1:
        log.debug("map: %s", ", ".join(f"{o.get('type')}@{o.get('col')} {v:.1f}" for v, o in scored))
    best = ranked[0]
    kids = best.get("leads_to") or [
        {"type": (nodes.get(tuple(c)) or {}).get("type")}
        for c in (nodes.get((best.get("col"), best.get("row"))) or {}).get("children") or []
    ]
    mem.boss_next = any(str(k.get("type")) == "Boss" for k in kids)
    mem.map_nodes, mem.map_at = nodes, (best.get("col"), best.get("row"))
    return [("choose_map_node", {"index": int(o.get("index", 0))}) for o in ranked]


# ------------------------------------------------------------------ rewards


def _belt_full(player: dict) -> bool:
    """No room for a potion: the belt is full, or a relic (Sozu) forbids potions.

    With Sozu the shop accepts the purchase and nothing happens, which once
    kept the bot buying the same potion until it was stopped.
    """
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
            # A better potion than the least of the belt: that one is poured away, and on the next look
            # there is room. (Once or twice a room: a discard the game did not carry out must not loop.)
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
    out.append(("proceed", {}))
    return out


def card_reward(state: dict, mem: Memory) -> list[Action]:
    cr = state.get("card_reward") or {}
    cards = cr.get("cards") or []
    act = (state.get("run") or {}).get("act")

    who = str((state.get("player") or {}).get("character") or "")

    def worth(c: dict) -> float:
        v = deck_value(c) + deck_fit(c, mem.deck, act if isinstance(act, int) else None, mem.max_energy)
        if mem.card_stats is not None:
            v += mem.card_stats.shift(who, str(c.get("name")), _kind(c))
        # The starter deck cannot kill anything quickly; until that is fixed, an attack is worth
        # more than one more way to block. (deck_fit sees to it once the deck is known; this is
        # for a run picked up before its first fight.)
        if not mem.deck and act == 1 and mem.deck_size < 18 and str(c.get("type")) == "Attack" and v > 0:
            v += 0.8
        return v

    ranked = sorted(cards, key=worth, reverse=True)
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

    # Rest or upgrade: whichever leaves the rest of the way to the boss better
    # off. Without the map (a run picked up at a rest site) the old rule of
    # thumb decides.
    act = (state.get("run") or {}).get("act")
    # What counts as healthy enough to meet the boss without healing first. Acts 1 and 2: bosses
    # met on 80-97% HP were beaten as often as from full (41 of 60 against 94 of 117), so from
    # 85% up the rest site is an upgrade. The last boss is a longer fight, and its few results
    # do not say the same (0 of 6 on 80-97%, 6 of 23 from full).
    full = 0.95 if not isinstance(act, int) or act >= 3 else 0.85
    heal_first = hp < (full if mem.boss_next else 0.6)
    node = mem.map_nodes.get(mem.map_at) if mem.map_nodes else None
    if node is not None and str(node.get("type")) == "RestSite" and (node.get("children") or []):
        args = (max_hp, int(player.get("gold") or 0), act if isinstance(act, int) else 1)
        smith = _upgrade_worth(args[2]) + best_onward(mem.map_nodes, mem.map_at, [], hp_now, *args, [MAX_PATHS])
        healed = min(float(max_hp), hp_now + REST_HEAL * max_hp)
        rest = best_onward(mem.map_nodes, mem.map_at, [], healed, *args, [MAX_PATHS])
        # Below half HP the path values hardly see the difference between
        # 6 HP and 40 (boss odds are zero under 30%), so a flat upgrade bonus
        # won: one act 3 bot walked into the boss at 6 of 109 HP. Heal then.
        # Before a boss a heal is worth more than the flat upgrade bonus: the
        # bot skipped the rest 29 times below 90% HP and fought the boss weaker.
        heal_first = hp < 0.5 or (rest > smith and hp < 0.95) or (mem.boss_next and hp < full)
        log.debug("rest site at %d/%d: rest %.1f, upgrade %.1f", hp_now, max_hp, rest, smith)

    def rank(o: dict) -> float:
        oid = str(o.get("id") or o.get("name") or "").lower()
        if "rest" in oid or "heal" in oid:
            return 10.0 if heal_first else 1.0
        if "smith" in oid or "upgrade" in oid:
            return 5.0
        return 3.0

    out = [("choose_rest_option", {"index": int(o.get("index", 0))}) for o in sorted(opts, key=rank, reverse=True)]
    if rs.get("can_proceed"):
        out.insert(0, ("proceed", {}))
    else:
        out.append(("proceed", {}))
    return out


# --------------------------------------------------------------------- shop


REMOVAL_EARLY = {1: 3.5, 2: 1.5}  # what a card removal is worth less in acts 1 and 2 under the `cards_first` setting
SEAL_RESERVE = 45  # Gold a Merchant does not get while Seal of Gold is held (3 a turn for an energy: a boss fight is 10 to 15 turns)

# What the Fake Merchant's relics are worth next to a real one at 7 (by what their text does over a run).
_FAKES = {
    "FAKE_ANCHOR": 5.0,  # 4 Block as each combat starts
    "FAKE_ORICHALCUM": 4.0,  # 3 Block on a turn ended without any
    "FAKE_HAPPY_FLOWER": 3.5,  # an energy every 5 turns
    "FAKE_VENERABLE_TEA_SET": 3.0,  # an energy in the combat after a rest site
    "FAKE_BLOOD_VIAL": 3.0,  # 1 HP as each combat starts
    "FAKE_LEES_WAFFLE": 3.0,  # a tenth of our HP back, once
    "FAKE_MANGO": 2.5,  # 3 Max HP
    "FAKE_STRIKE_DUMMY": 1.5,  # 1 damage on each Strike
    "FAKE_SNECKO_EYE": -1.0,  # every card's cost at random, and nothing for it
}


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
    if any("everything he sells" in str(r.get("description") or "").lower() for r in player.get("relics") or []):
        # Lord's Parasol ("When you encounter the Merchant, immediately obtain EVERYTHING he sells") empties
        # the shelf by itself, one item after another, asking which card to remove on the way. A purchase of
        # our own in the middle of that left its loop at an item that was gone, and the game does not survive
        # it (FailureOutOfStock out of LordsParasol.PurchaseEverything: the same shop crashed the game three
        # times and the run was given up). Nothing is bought here; we leave once the shelf is empty, or after
        # a few looks if something on it cannot be taken (a potion with the belt full).
        looks = mem.parasol[2] + 1 if mem.parasol[:2] == _where(state) else 1
        mem.parasol = (*_where(state), looks)
        return [("proceed", {})] if looks > 8 or not any(i.get("is_stocked", True) for i in s.get("items") or []) else []

    # (The bot leaves a Merchant with 35 to 58 Gold on average, and with less than a boss fight's worth often enough.)
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
            # (From Ascension 6 the price is 100 and 50 more each time: at 150 and up a removal has to
            # beat the relics on the shelf instead of going before them.)
            want = 9.0 if price <= 125 else 7.6 - (price - 100) / 100.0
            # ... and early in a run it has to beat the cards on the shelf too (a setting under test, see
            # learn.CHOICES["cards_first"]): a Strike gone from sixteen cards is less than a good card added.
            want -= knob("cards_first") * REMOVAL_EARLY.get((state.get("run") or {}).get("act"), 0.0)
        elif cat == "relic":
            # (Snecko Eye was refused here until 2026-11-20, as costs the planner could not count on. It reads
            # the cost of every card in hand as drawn, and 3 of the 5 runs that held one were won.)
            want = 8.0 - price / 200.0
            if str(item.get("relic_id") or "").startswith("FAKE_"):
                # The Fake Merchant's copies do a fraction of what their names promise, for about 55 gold each,
                # and until 2026-11-21 all nine were bought like relics, cheapest first. One only costs:
                # "Snecko Eye???" is "Start each combat Confused" without the two cards the real one draws
                # (held by 14 of the 84 runs that met him).
                # ... and the gold is the same gold a real Merchant takes: one run paid 319 for six of them and
                # reached a shelf with a removal and three relics two floors on with 15. The ones worth a third
                # of a removal are bought, the rest only out of plenty.
                want = _FAKES.get(str(item.get("relic_id")), 2.0)
                want = want if want >= 3.5 or gold - price >= 200 else -1.0
        elif cat == "card":
            act = (state.get("run") or {}).get("act")
            v = deck_value(_shop_card(item)) + deck_fit(_shop_card(item), mem.deck, act if isinstance(act, int) else None, mem.max_energy)
            if mem.card_stats is not None:  # (what our own runs say of the card and of its kind, as at a card reward)
                v += mem.card_stats.shift(str(player.get("character") or ""), str(item.get("card_name")), _kind(_shop_card(item)))
            # Only cards that make the deck better, and more freely when rich.
            bar = 4.0 if gold - price >= 200 else 4.5 if mem.deck_size < BIG_DECK else 5.5
            want, rated = (v if v >= bar else -1.0), v
        elif cat == "potion":
            # With the boss a room or two away a potion is the best thing gold
            # buys: act 2 bosses met with two or three potions were beaten 19
            # times in 35, with one or none 5 in 24. And the gold kept back is
            # for a shop that comes after the boss, or never.
            floor = (state.get("run") or {}).get("floor")
            boss_next = isinstance(floor, int) and any(0 < boss - floor <= 3 for boss in BOSS_FLOORS)
            # ... and all through act 3: nothing a Merchant sells there has longer than the last boss to pay
            # for itself, and that boss was met with one potion or none 14 times in 31 (never beaten so; one
            # run reached it holding one potion and 240 Gold).
            boss_next = boss_next or (isinstance(floor, int) and floor > BOSS_FLOORS[1])
            if belt_full:
                want = -1.0
            elif boss_next:
                # ... and before a removal while the belt holds fewer than two: a shelf three floors from the
                # act 2 boss gave up a card for 100 with 137 in hand and two potions at 49 and 52 beside it.
                # (Bosses met with one potion or none: act 2 12 beaten in 36, the last 0 in 12; with two or
                # more 38 in 61 and 7 in 18. What a deck gains by one Strike fewer does not show in what its
                # fights cost.)
                want = 9.5 if len(player.get("potions") or []) < 2 else 7.0
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
        # (Until 2026-11-21 the log had the purchases as shelf numbers and prices only: what the bot passed over,
        # and whether a relic or a card was what the gold went on, could not be read back.)
        mem.logged_shop = shown
        log.debug("shop (gold %d, potions %d of %d): %s", gold, len(player.get("potions") or []), int(player.get("max_potion_slots") or 3), ", ".join(shelf))
    scored.sort(key=lambda t: -t[0])
    out: list[Action] = [("shop_purchase", {"index": int(i.get("index", 0))}) for _, i in scored]
    out.append(("proceed", {}))
    return out


# --------------------------------------------------------------- selections

_WORST_FIRST = re.compile(r"remove|transform|exhaust|discard|lose|purge|sacrifice|destroy", re.I)


KEEP_AT_ALL_COSTS = {"FRANTIC_ESCAPE"}  # the only way out of the Sandpit


def _removal_rank(card: dict) -> float:
    """Lower = get rid of it first."""
    if _card_key(card) in KEEP_AT_ALL_COSTS:
        # A Status card, so it used to be the first thing discarded or
        # exhausted when a card or relic asked for one: 23 times, and then
        # the Sandpit ate a bot on 58 HP with the boss on 4.
        return 1000.0
    if is_junk(card):
        return -100.0
    v = deck_value(card)
    if is_basic(card):
        # (A Strike before a Defend: the two rate alike, and it was the order on the screen that
        # decided. Every guide says Strikes first, and the Necrobinder's decks with none left got
        # past act 2 in 12 runs of 20, with Strikes in 17 of 60.)
        v -= 5.5 if _card_key(card).startswith("STRIKE") else 5.0
    return v


def _pick_order(cards: list[dict], prompt: str, screen_type: str = "", gain=None) -> list[dict]:
    if _WORST_FIRST.search(prompt) or screen_type == "transform":
        return sorted(cards, key=_removal_rank)
    if "upgrade" in prompt.lower() or screen_type == "upgrade":
        fresh = [c for c in cards if not c.get("is_upgraded")]

        def rank(c: dict) -> float:
            # A good card we play often, and an upgrade that does a lot for it.
            g = gain(c) if gain is not None else None
            v = deck_value(c)
            if g is None:
                return v
            # (Capped, and little for a card we hardly want to play at all.)
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

    ordered = _pick_order(cards, prompt, kind, mem.upgrade_gain)
    ordered = afflictions.order(ordered, state)
    shown = key + "|" + ",".join(str(c.get("name")) for c in cards)
    if kind == "choose" and log.isEnabledFor(logging.DEBUG) and shown != mem.logged_select:
        mem.logged_select = shown
        log.debug("choose (%s): %s", prompt, " | ".join(f"{c.get('name')}: {c.get('description')}" for c in ordered))
    if kind == "choose":
        # One click picks and closes the screen; there is nothing to confirm
        # and nothing to remember. A click that lands while the screen is
        # still opening is ignored by the game, so always offer it again.
        out = [("select_card", {"index": int(c.get("index", 0))}) for c in ordered]
        if cs.get("can_skip") or cs.get("can_cancel"):
            out.append(("cancel_selection", {}))
        return out

    confirmable = cs.get("preview_showing") or cs.get("can_confirm")
    indices = [int(c.get("index", 0)) for c in ordered]
    if not cs.get("preview_showing") and indices and all(i in mem.selected for i in indices):
        mem.selected = set()  # our clicks did not register; start over
    out: list[Action] = []
    wanted = re.search(r"choose\s+(\d+)\s+card", prompt, re.I)
    if wanted and not cs.get("preview_showing"):
        # The prompt says how many: tick exactly that many, then confirm. Some
        # screens report a confirm button as ready before anything is ticked.
        ready = len(mem.selected) >= int(wanted.group(1))
    else:
        ready = bool(confirmable)
    if ready:
        out.append(("confirm_selection", {}))
    out += [("select_card", {"index": i}) for i in indices if i not in mem.selected]
    if cs.get("can_skip") or cs.get("can_cancel"):
        out.append(("cancel_selection", {}))
    out.append(("confirm_selection", {}))
    return out


_PUT_BACK = re.compile(r"on top of your draw pile", re.I)


def _put_back_order(cards: list[dict], energy: int | None) -> list[dict]:
    """Which card of the hand to put on top of the draw pile (Photon Cut, Glimmer, Thinking Ahead).

    It comes back next turn, so the card to give is one that would not be
    played this turn anyway: the best of what we cannot pay for, or of what
    is left over once the energy is spent. The bot used to hand back its best
    card every time and play the weaker ones.
    """
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
        # Touch of Insanity ("It is free to play this combat"): the dearest card, the best of those. (By rating
        # alone a 0-cost Astral Pulse+ was made free next to a 2-cost Bulwark+.)
        ordered = sorted(ordered, key=lambda c: (str(c.get("type")) in ("Status", "Curse"), -_parse(c).cost))
    discarding = bool(re.search(r"\bdiscard\b", prompt, re.I))
    # A Sly card "discarded from your Hand before the end of your turn" is played
    # for free: the best thing to throw away. (Flick-Flack was kept 9 times in 10
    # while a Strike or a Defend went.)
    sly = lambda c: discarding and any(str(k.get("name")) == "Sly" for k in c.get("keywords") or [])
    ordered = sorted((c for c in ordered if sly(c)), key=deck_value, reverse=True) + [c for c in ordered if not sly(c)]
    # (With the choice made and only to be confirmed, the confirmation is the first thing sent and the order
    # below is only for when it is refused: not worth the plan per card, which took two seconds a click.)
    # (A card Exhausted only has to cost less than a kill or a life: the first such card in line will do.)
    costs = handpick.turn_costs(state, ordered, 1e-9 if discarding else handpick.RUINOUS / 2) if _WORST_FIRST.search(prompt) and not kind and not hs.get("can_confirm") else None
    if costs:
        # ... and otherwise the card this turn's plan misses least. A discard is back with the
        # next shuffle, so the turn is all that counts; a card Exhausted is gone for the fight,
        # so there the rating decides unless the choice would cost a kill or our life.
        def price(c: dict) -> float:
            cost = costs[int(c.get("index", 0))]
            if sly(c):
                return float("-inf")
            if discarding:
                return float("inf") if _card_key(c) in KEEP_AT_ALL_COSTS else float(round(cost / 2.0))
            # (A Frantic Escape too, when every other card is the kill: with the Insatiable on 59 the bot
            # Exhausted the Bone Shards that made 60 and kept an Escape it had no further use for.
            # Otherwise its rating keeps it last in line.)
            return float(cost >= handpick.RUINOUS)

        ordered = sorted(ordered, key=price)  # (stable: equal prices keep the order above)
    return out + [("combat_select_card", {"card_index": int(c.get("index", 0))}) for c in ordered] + [("combat_confirm_selection", {})]


def bundle_select(state: dict, mem: Memory) -> list[Action]:
    bs = state.get("bundle_select") or {}
    out: list[Action] = []
    if bs.get("preview_showing") or bs.get("can_confirm"):
        out.append(("confirm_bundle_selection", {}))
    bundles = sorted(bs.get("bundles") or [], key=lambda b: sum(deck_value(c) for c in b.get("cards") or []), reverse=True)
    out += [("select_bundle", {"index": int(b.get("index", 0))}) for b in bundles]
    out.append(("confirm_bundle_selection", {}))
    return out


def relic_select(state: dict, mem: Memory) -> list[Action]:
    rs = state.get("relic_select") or {}
    out: list[Action] = [("select_relic", {"index": int(r.get("index", 0))}) for r in rs.get("relics") or []]
    if rs.get("can_skip"):
        out.append(("skip_relic_selection", {}))
    return out


def treasure(state: dict, mem: Memory) -> list[Action]:
    t = state.get("treasure") or {}
    relics = t.get("relics") or []
    if not relics and not t.get("can_proceed"):
        return []  # chest still opening
    out: list[Action] = [("claim_treasure_relic", {"index": int(r.get("index", 0))}) for r in relics]
    out.append(("proceed", {}))
    return out


def crystal_sphere(state: dict, mem: Memory) -> list[Action]:
    cs = state.get("crystal_sphere") or {}
    if cs.get("can_proceed"):
        return [("crystal_sphere_proceed", {})]
    cells = cs.get("clickable_cells") or []
    w, h = int(cs.get("grid_width") or 9), int(cs.get("grid_height") or 11)
    # Start from the middle: big tools reveal the most there.
    cells = sorted(cells, key=lambda c: abs(c.get("x", 0) - w // 2) + abs(c.get("y", 0) - h // 2))
    out: list[Action] = [("crystal_sphere_click_cell", {"x": c.get("x"), "y": c.get("y")}) for c in cells[:6]]
    out.append(("crystal_sphere_proceed", {}))
    return out


OVER_PATIENCE = 60  # looks at the end-of-run summary before giving up on it


def game_over(state: dict, mem: Memory) -> list[Action]:
    """Leave the end-of-run screen, by way of the run summary.

    The summary is where the game adds the run's score to the profile; the
    epochs that unlock "by accumulating score" (about eighteen of them) only
    ever come from there. The bot used to go straight to the main menu, and
    after 196 runs the profile's score was still 0.
    """
    options = (state.get("game_over") or {}).get("options") or []
    if "continue" in options:
        mem.over_waits = 0
        return [("menu_select", {"option": "continue"})]
    if "main_menu" in options:
        return [("menu_select", {"option": "main_menu"})]
    if "force_main_menu" in options:
        # The summary is still counting up. Wait for its Main Menu button,
        # but not for ever.
        mem.over_waits += 1
        return [("menu_select", {"option": "force_main_menu"})] if mem.over_waits > OVER_PATIENCE else []
    return [("menu_select", {"option": "main_menu"})]  # a mod without the summary patch


def overlay(state: dict, mem: Memory) -> list[Action]:
    return [("proceed", {}), ("confirm_selection", {}), ("cancel_selection", {})]
