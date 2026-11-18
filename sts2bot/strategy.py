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
from .potions import BOSS_FLOORS
from .events import _ANCIENT, _event_option_value, event  # noqa: F401  (events live in their own module)
from .cards import _card_key, deck_value, is_basic, is_junk, is_known as _known_card, parse as _parse

Action = tuple[str, dict]

log = logging.getLogger("sts2bot.strategy")

SKIP_THRESHOLD = 3.0  # card rewards scoring below this are skipped
BIG_DECK = 24


@dataclass
class Memory:
    """The few things the game state does not tell us."""

    deck_size: int = 10
    deck: list = field(default_factory=list)  # our cards as last seen at the start of a fight, plus picks since
    picks: list = field(default_factory=list)  # (floor, card name) for every card reward taken this run
    card_stats: object = None  # learn.CardStats: how cards have done in our own runs
    card_skips: dict = field(default_factory=dict)  # (act, floor) -> skipped card rewards
    select_key: str = ""
    logged_select: str = ""  # the "choose a card" screen last written to the log
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
    epoch_drought: dict = field(default_factory=dict)  # character id -> runs in a row that earned no epoch
    boss_kills: dict | None = None  # character id -> bosses killed so far, from the game's records
    strength: dict = field(default_factory=dict)  # character id -> how far its recent runs got (0..1)
    current_character: str = ""  # who the run in progress is played with
    timeline_done: bool = False  # the Timeline has nothing more to reveal this visit
    timeline_stuck: set = field(default_factory=set)  # epochs it could not reveal (not retried)

    def reset_run(self) -> None:
        self.deck_size = 10
        self.deck = []
        self.picks = []
        self.map_nodes = {}
        self.map_at = ()
        self.over_waits = 0
        self.card_skips.clear()
        self.select_key = ""
        self.selected.clear()
        self.character_selected = False
        self.ascension_waits = 0


def _where(state: dict) -> tuple:
    run = state.get("run") or {}
    return (run.get("act"), run.get("floor"))


def _hp_ratio(state: dict) -> float:
    p = state.get("player") or {}
    return int(p.get("hp") or 0) / max(1, int(p.get("max_hp") or 1))


def _option_names(options) -> list[str]:
    """Menu options come either as strings or as {name, enabled} objects."""
    out = []
    for o in options or []:
        if isinstance(o, str):
            out.append(o)
        elif isinstance(o, dict) and o.get("enabled", True) and o.get("name"):
            out.append(str(o["name"]))
    return out


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
        want = choose_character(
            character, ids, mem.runs_finished, mem.max_ascension, mem.epochs, mem.epoch_drought, mem.boss_kills, mem.strength
        )
        select = [("menu_select", {"option": want})] if want else []
        embark = pick("confirm", "embark")
        if not mem.character_selected:
            return select + embark
        # Every run is played at the highest level the character has unlocked:
        # a win there is what unlocks the next one.
        raise_asc: list[Action] = []
        top, now = state.get("max_ascension"), state.get("ascension")
        aim = top
        if isinstance(aim, int) and isinstance(now, int) and now != aim:
            raise_asc = [("set_ascension", {"level": aim})]
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


BOSSES_FOR_EPOCH = 15
TOTAL_EPOCHS = 57  # the profile only lists the score epochs as they come up


def epochs_left(epochs: dict | None) -> bool:
    """True while the Timeline still has something to unlock (or we cannot tell)."""
    if not epochs:
        return False  # no profile to go by: behave as if there were no Timeline
    return len(epochs) < TOTAL_EPOCHS or any(state not in _EARNED for state in epochs.values())


_SESSION = random.SystemRandom().getrandbits(32)  # so a restart does not replay the same draws


def choose_character(
    setting: str,
    unlocked: list[str],
    runs_finished: int,
    max_ascension: dict | None = None,
    epochs: dict | None = None,
    drought: dict | None = None,
    boss_kills: dict | None = None,
    strength: dict | None = None,
) -> str | None:
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

_ROOM_BASE = {
    "Monster": 1.5,
    "Unknown": 2.2,
    "Treasure": 4.0,
    "RestSite": 3.0,
    "Shop": 1.0,
    "Elite": 2.5,
    "Boss": 0.0,
    "Ancient": 3.0,
}


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
FIGHT_COST = {1: {"Monster": 5, "Elite": 16}, 2: {"Monster": 10, "Elite": 21}, 3: {"Monster": 11, "Elite": 40}}
BAD_LUCK = 1.7  # how much more than average a fight costs when the draws go wrong
BOSS_WORTH = 16.0  # what getting past the boss is worth, in the units of the room rewards below
MAX_PATHS = 1500


def boss_odds(hp_ratio: float) -> float:
    """Chance of beating the boss given how healthy we arrive.

    From the run history: at 75% HP or more the act 1 boss was beaten four
    times in five, between 50% and 75% one time in four, below that never.
    The last stretch counts for the most: act 2 bosses met at 95% HP or more
    were beaten 27 times in 45 against 10 in 44 below, and every one of seven
    act 3 wins started at full HP (7 of 16; 0 of 24 below). So the curve
    bends: 0.26 at 60% HP, 0.58 at 80%, 0.78 at 90%.
    """
    return max(0.0, min(1.0, (hp_ratio - 0.30) / 0.70)) ** 1.6


REST_HEAL = 0.3  # of Max HP


def path_value(
    rooms: list[tuple[str, int]], hp: float, max_hp: int, gold: int, act: int, heal_after_fight: int = 0, harsh: float = 1.0
) -> float:
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
            hp += heal_after_fight - cost * harsh
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
            smith = knob("upgrade_worth") + path_value(onward, hp, max_hp, gold, act, heal_after_fight, harsh)
            if hp >= 0.95 * max_hp:
                return score + smith
            rest = path_value(onward, min(float(max_hp), hp + REST_HEAL * max_hp), max_hp, gold, act, heal_after_fight, harsh)
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
            score += BOSS_WORTH * boss_odds(hp / max(1, max_hp))
    return score


def best_onward(nodes: dict, key, rooms: list, hp: float, max_hp: int, gold: int, act: int, mend: int, budget: list) -> float:
    """Best value over every way on from node `key` (`rooms` is the way so far, that node included)."""
    kids = [tuple(c) for c in (nodes.get(key) or {}).get("children") or [] if tuple(c) in nodes]
    if not kids or budget[0] <= 0:
        budget[0] -= 1
        # Fights do not cost their average: a route is also judged by how it
        # goes when they cost a good deal more. A route that only works out on
        # average led through two forced elites at 18 HP.
        usual = path_value(rooms, hp, max_hp, gold, act, mend)
        unlucky = path_value(rooms, hp, max_hp, gold, act, mend, BAD_LUCK)
        return 0.6 * usual + 0.4 * unlucky
    return max(best_onward(nodes, k, rooms + [(str(nodes[k].get("type")), k[1])], hp, max_hp, gold, act, mend, budget) for k in kids)


def _mend(player: dict) -> int:
    """HP that comes back after every fight, beyond what FIGHT_COST already allows for.

    (Nothing at present: the measured costs include the Ironclad's Burning
    Blood, and without it the other characters lose about the same.)
    """
    return 0


def map_screen(state: dict, mem: Memory) -> list[Action]:
    m = state.get("map") or {}
    options = m.get("next_options") or []
    if not options:
        return []
    player = state.get("player") or {}
    hp_now, max_hp = int(player.get("hp") or 0), max(1, int(player.get("max_hp") or 1))
    hp = hp_now / max_hp
    gold = int(player.get("gold") or 0)
    nodes = {(n.get("col"), n.get("row")): n for n in m.get("nodes") or []}
    act = (state.get("run") or {}).get("act")
    mend = _mend(player)
    act_no = act if isinstance(act, int) else 1

    def is_early(row) -> bool:
        return act == 1 and isinstance(row, int) and row < EARLY_ROWS

    budget = [MAX_PATHS]

    def best_path(key, rooms: list) -> float:
        return best_onward(nodes, key, rooms, hp_now, max_hp, gold, act_no, mend, budget)

    def value(opt: dict) -> float:
        key = (opt.get("col"), opt.get("row"))
        if key in nodes:
            return best_path(key, [(str(nodes[key].get("type")), key[1])])
        kids = opt.get("leads_to") or []
        ahead = max((_room_value(str(k.get("type")), hp, gold, is_early(k.get("row")), act) for k in kids), default=0.0)
        return _room_value(str(opt.get("type")), hp, gold, is_early(opt.get("row")), act) + 0.9 * ahead

    scored = [(value(o), o) for o in options]
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


def rewards(state: dict, mem: Memory) -> list[Action]:
    r = state.get("rewards") or {}
    player = state.get("player") or {}
    belt_full = len(player.get("potions") or []) >= int(player.get("max_potion_slots") or 3)
    skips_left = mem.card_skips.get(_where(state), 0)
    out: list[Action] = []
    # Highest index first so earlier indices stay valid.
    for item in sorted(r.get("items") or [], key=lambda i: -int(i.get("index", 0))):
        kind = str(item.get("type"))
        if kind == "potion" and belt_full:
            continue
        if kind == "card" and skips_left > 0:
            skips_left -= 1
            continue
        out.append(("claim_reward", {"index": int(item.get("index", 0))}))
    out.append(("proceed", {}))
    return out


STARS_AT_START = 3  # the Regent's starting relic


def _stars(card: dict) -> tuple[int, int]:
    """(Stars the card needs, Stars it gives) each time it is played."""
    info = _parse(card)
    need = 2 if info.star_cost < 0 else info.star_cost
    text = str(card.get("description") or "")
    later = sum(len(re.findall(r"star_icon", m)) for m in re.findall(r"next turn, gain ((?:\[star_icon\.png\]\s*)+)", text, re.I))
    return need, info.star_gain + later


def deck_fit(card: dict, deck: list) -> float:
    """How much better or worse a card is in *this* deck than on its own.

    The ratings in cards.py judge a card alone. Two things they cannot see:
    the Regent's Stars (a 5-Star Comet is dead weight in a deck that makes
    two Stars a cycle; one run reached its boss holding five such cards it
    could never play) and copies (the third Astral Pulse is worth less than
    the first).
    """
    if not deck:
        return 0.0
    fit = 0.0
    need, give = _stars(card)
    if need or give:
        pairs = [_stars(c) for c in deck]
        demand = sum(n for n, _ in pairs)
        supply = STARS_AT_START + sum(g for _, g in pairs)
        if need:
            short = demand + need - 1.2 * supply
            if short > 0:
                fit -= min(4.0, 0.7 * short)
        if give and demand > supply:
            fit += min(2.0, 0.4 * (demand - supply))
    key = _card_key(card)
    copies = sum(1 for c in deck if _card_key(c) == key)
    fit -= 0.6 * copies
    return fit


def reward_bar(deck_size: int, act: int | None) -> float:
    """What a card reward has to be worth to be taken."""
    # Be pickier as the deck grows: every mediocre card makes the good ones
    # show up less often. But acts 2 and 3 are where the deck has to do the
    # damage: the bot skipped 37% of act 2 rewards and 56% of act 3 ones, most
    # of them 3-4 point attacks, and most act 3 bosses were still above half
    # their HP when the run ended. So there the growth is halved.
    growth = 0.0 if deck_size < 16 else 1.0 if deck_size < 20 else 2.0 if deck_size < BIG_DECK else 3.0
    if isinstance(act, int) and act >= 2:
        growth *= 0.5
    return SKIP_THRESHOLD + knob("pickiness") + growth


def card_reward(state: dict, mem: Memory) -> list[Action]:
    cr = state.get("card_reward") or {}
    cards = cr.get("cards") or []
    act = (state.get("run") or {}).get("act")

    who = str((state.get("player") or {}).get("character") or "")

    def worth(c: dict) -> float:
        v = deck_value(c) + deck_fit(c, mem.deck)
        if mem.card_stats is not None:
            v += mem.card_stats.shift(who, str(c.get("name")))
        # The starter deck cannot kill anything quickly; until that is fixed,
        # an attack is worth more than one more way to block.
        if act == 1 and mem.deck_size < 18 and str(c.get("type")) == "Attack" and v > 0:
            v += 0.8
        return v

    ranked = sorted(cards, key=worth, reverse=True)
    out: list[Action] = []
    # Be pickier as the deck grows: every mediocre card makes the good ones
    # show up less often.
    threshold = reward_bar(mem.deck_size, act)
    if log.isEnabledFor(logging.DEBUG):
        log.debug("card reward (deck %d, need %.1f): %s", mem.deck_size, threshold, ", ".join(f"{c.get('name')} {worth(c):.1f}" for c in ranked))
    take = [("select_card_reward", {"card_index": int(c.get("index", 0))}) for c in ranked]
    if ranked and worth(ranked[0]) >= threshold:
        out += take
        if cr.get("can_skip", True):
            out.append(("skip_card_reward", {}))
    else:
        out.append(("skip_card_reward", {}))
        out += take
    return out


# ---------------------------------------------------------------- rest site


def rest_site(state: dict, mem: Memory) -> list[Action]:
    rs = state.get("rest_site") or {}
    opts = [o for o in rs.get("options") or [] if o.get("is_enabled", True)]
    hp = _hp_ratio(state)
    player = state.get("player") or {}
    hp_now, max_hp = int(player.get("hp") or 0), max(1, int(player.get("max_hp") or 1))

    # Rest or upgrade: whichever leaves the rest of the way to the boss better
    # off. Without the map (a run picked up at a rest site) the old rule of
    # thumb decides.
    heal_first = hp < (0.95 if mem.boss_next else 0.6)
    node = mem.map_nodes.get(mem.map_at) if mem.map_nodes else None
    if node is not None and str(node.get("type")) == "RestSite" and (node.get("children") or []):
        act = (state.get("run") or {}).get("act")
        args = (max_hp, int(player.get("gold") or 0), act if isinstance(act, int) else 1, _mend(player))
        smith = knob("upgrade_worth") + best_onward(mem.map_nodes, mem.map_at, [], hp_now, *args, [MAX_PATHS])
        healed = min(float(max_hp), hp_now + REST_HEAL * max_hp)
        rest = best_onward(mem.map_nodes, mem.map_at, [], healed, *args, [MAX_PATHS])
        # Below half HP the path values hardly see the difference between
        # 6 HP and 40 (boss odds are zero under 30%), so a flat upgrade bonus
        # won: one act 3 bot walked into the boss at 6 of 109 HP. Heal then.
        # Before a boss a heal is worth more than the flat upgrade bonus: the
        # bot skipped the rest 29 times below 90% HP and fought the boss weaker.
        heal_first = hp < 0.5 or (rest > smith and hp < 0.95) or (mem.boss_next and hp < 0.95)
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


def _shop_card(item: dict) -> dict:
    return {
        "id": item.get("card_id"),
        "name": item.get("card_name"),
        "type": item.get("card_type"),
        "cost": item.get("card_cost"),
        "star_cost": item.get("card_star_cost"),
        "rarity": item.get("card_rarity"),
        "description": item.get("card_description"),
    }


def shop(state: dict, mem: Memory, key: str = "shop") -> list[Action]:
    s = state.get(key) or {}
    if key == "fake_merchant":
        s = s.get("shop") or {}
    if s.get("error"):
        return []
    player = state.get("player") or {}
    gold = int(player.get("gold") or 0)
    belt_full = len(player.get("potions") or []) >= int(player.get("max_potion_slots") or 3)

    scored: list[tuple[float, dict]] = []
    for item in s.get("items") or []:
        price = int(item.get("price", item.get("cost", 0)) or 0)
        if not item.get("is_stocked", True) or not item.get("can_afford", price <= gold):
            continue
        cat = str(item.get("category"))
        if cat == "card_removal":
            want = 9.0
        elif cat == "relic":
            # Snecko Eye randomises card costs, which the planner cannot work with.
            bad = "SNECKO" in str(item.get("relic_id") or item.get("relic_name") or "").upper()
            want = -1.0 if bad else 8.0 - price / 200.0
        elif cat == "card":
            v = deck_value(_shop_card(item)) + deck_fit(_shop_card(item), mem.deck)
            # Only cards that make the deck better, and more freely when rich.
            bar = 4.0 if gold - price >= 200 else 4.5 if mem.deck_size < BIG_DECK else 5.5
            want = v if v >= bar else -1.0
        elif cat == "potion":
            # With the boss a room or two away a potion is the best thing gold
            # buys: act 2 bosses met with two or three potions were beaten 19
            # times in 35, with one or none 5 in 24. And the gold kept back is
            # for a shop that comes after the boss, or never.
            floor = (state.get("run") or {}).get("floor")
            boss_next = isinstance(floor, int) and any(0 < boss - floor <= 3 for boss in BOSS_FLOORS)
            if belt_full:
                want = -1.0
            elif boss_next:
                want = 7.0
            else:
                want = 2.0 if gold - price >= 60 else -1.0
        else:
            want = -1.0
        if want > 0:
            scored.append((want, item))
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
        v -= 5.0
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


def _affliction_order(ordered: list[dict], state: dict) -> list[dict]:
    """The Knowledge Demon's last offer: one energy less every turn, or 8 damage every turn.

    The damage is the cheaper price nearly always. An energy is about 8 Block
    or 10 damage every turn, so going without it costs as much HP as the burn
    does, and the fight takes half as long again on top. Of 22 fights where
    the bot gave up the energy (never the HP, under an older and stricter
    rule), it won 2; four more ended with the Demon on 17 to 42 HP after three
    or four turns played on 2 energy.
    """
    keys = {_card_key(c): c for c in ordered}
    burn, drain = keys.get("DISINTEGRATION"), keys.get("WASTE_AWAY")
    if burn is None or drain is None:
        return ordered
    # (The choice screen comes without the enemies: a rule that asked for the
    # Demon's HP never applied, and the energy went 38 times out of 38.)
    player = state.get("player") or {}
    m = re.search(r"take (\d+) damage", str(burn.get("description") or ""), re.I)
    per_turn = int(m.group(1)) if m else 8
    # (Not when the burn alone would finish us within two turns: then every turn counts.)
    if int(player.get("hp") or 0) > 2 * per_turn:
        return [burn] + [c for c in ordered if c is not burn]
    return ordered


def card_select(state: dict, mem: Memory) -> list[Action]:
    cs = state.get("card_select") or {}
    prompt = str(cs.get("prompt") or "")
    kind = str(cs.get("screen_type") or "")
    cards = cs.get("cards") or []
    key = f"{_where(state)}|{kind}|{prompt}|{len(cards)}"
    if key != mem.select_key:
        mem.select_key, mem.selected = key, set()

    ordered = _pick_order(cards, prompt, kind, mem.upgrade_gain)
    ordered = _affliction_order(ordered, state)
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
    for c in _pick_order(hs.get("cards") or [], prompt, kind, mem.upgrade_gain):
        out.append(("combat_select_card", {"card_index": int(c.get("index", 0))}))
    out.append(("combat_confirm_selection", {}))
    return out


def bundle_select(state: dict, mem: Memory) -> list[Action]:
    bs = state.get("bundle_select") or {}
    out: list[Action] = []
    if bs.get("preview_showing") or bs.get("can_confirm"):
        out.append(("confirm_bundle_selection", {}))
    bundles = sorted(
        bs.get("bundles") or [],
        key=lambda b: sum(deck_value(c) for c in b.get("cards") or []),
        reverse=True,
    )
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
