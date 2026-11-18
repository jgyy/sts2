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
    selected: set = field(default_factory=set)
    runs_finished: int = 0
    character_selected: bool = False
    boss_next: bool = False  # the room we are walking into leads straight to the boss
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
        # Epochs first, ascension levels after: while the Timeline is not
        # complete the run is played at the level its epochs ask for, which is
        # the easiest one that counts. After that, at the highest unlocked.
        raise_asc: list[Action] = []
        top, now = state.get("max_ascension"), state.get("ascension")
        aim = top
        if isinstance(top, int):
            aim = min(top, epoch_ascension(str(want), mem.epochs, top))
        if isinstance(aim, int) and isinstance(now, int) and now != aim:
            raise_asc = [("set_ascension", {"level": aim})]
        # The screen takes a moment to show the levels of the character just
        # picked. Embarking before it does started the run at ascension 0 even
        # though a higher one was unlocked, so wait for it (but not for ever).
        known = {str(k).upper(): int(v) for k, v in mem.max_ascension.items()}.get(str(want).upper(), 0)
        known = min(known, epoch_ascension(str(want), mem.epochs, known))
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


def epoch_ascension(character: str, epochs: dict | None, top: int) -> int:
    """The ascension level to play this character at.

    While epochs are left: Ascension 1 if the character's own "win on
    Ascension 1" epoch is the one it is after (and the level is unlocked),
    otherwise 0, where wins, bosses and score all come easiest. With the
    Timeline complete: the highest level unlocked, to unlock the next.
    """
    if not epochs_left(epochs):
        return top
    c = character.upper()
    won_before = (epochs or {}).get(f"{c}4_EPOCH") in _EARNED
    wants_a1 = f"{c}7_EPOCH" in (epochs or {}) and (epochs or {}).get(f"{c}7_EPOCH") not in _EARNED
    return 1 if top >= 1 and won_before and wants_a1 else 0




def epoch_pull(character: str, epochs: dict | None, drought: int = 0, max_ascension: int = 0, boss_kills: int | None = None, strength: float | None = None) -> float:
    """How much playing this character is likely to add to the Timeline right now.

    Each character has its own line of epochs, and the game says what each
    one takes: 2, 3 and 4 are beating acts 1, 2 and 3; 5 and 6 are 15 elites
    and 15 bosses killed; 7 is a win on Ascension 1. Killing bosses adds up
    whatever happens, so a character still short of 15 is always worth a
    run. Epochs that need a win go to whoever is getting furthest
    (`strength`, how far its recent runs got as a share of the whole climb),
    with a discount for runs in a row that earned nothing so the others get
    their turn.
    """
    prefix = re.compile(rf"^{re.escape(character.upper())}(\d+)_EPOCH$")
    sure, chancy = 0.0, 0.0
    power = (strength if strength is not None else 0.4) ** 2
    for eid, state in (epochs or {}).items():
        m = prefix.match(str(eid).upper())
        if not m or state in _EARNED:
            continue
        n = int(m.group(1))
        if n == 6 and boss_kills is not None and boss_kills < BOSSES_FOR_EPOCH:
            sure += 1.5 + boss_kills / BOSSES_FOR_EPOCH
        elif n == 4:
            chancy += 3.0 * power
        elif n == 7:
            chancy += 1.8 * power if max_ascension >= 1 else 0.3  # needs Ascension 1 unlocked first
        else:
            chancy += 1.0
    return sure + chancy / (1.0 + drought / 4.0)


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

    "auto" works towards unlocking every epoch on the game's Timeline: it
    plays whoever is most likely to earn one next (see epoch_pull). Once
    nobody has epochs left, it plays the character with the lowest ascension
    so that all of them climb evenly.
    """
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
    if s == "AUTO":
        levels = {str(k).upper(): int(v) for k, v in (max_ascension or {}).items()}
        dry = {str(k).upper(): int(v) for k, v in (drought or {}).items()}
        kills = {str(k).upper(): int(v) for k, v in (boss_kills or {}).items()}
        strong = {str(k).upper(): float(v) for k, v in (strength or {}).items()}

        def pull(u: str) -> float:
            k = u.upper()
            return epoch_pull(u, epochs, dry.get(k, 0), levels.get(k, 0), kills.get(k) if boss_kills is not None else None, strong.get(k))

        # Most likely to earn an epoch first; then the lowest ascension; then the listed order.
        return min(unlocked, key=lambda u: (-pull(u), levels.get(u.upper(), 0)))
    if s == "CYCLE":
        return unlocked[runs_finished % len(unlocked)]
    if s == "RANDOM":
        return random.Random(runs_finished).choice(unlocked)
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
FIGHT_COST = {1: {"Monster": 5, "Elite": 16}, 2: {"Monster": 10, "Elite": 21}, 3: {"Monster": 11, "Elite": 32}}
BAD_LUCK = 1.7  # how much more than average a fight costs when the draws go wrong
BOSS_WORTH = 16.0  # what getting past the boss is worth, in the units of the room rewards below
MAX_PATHS = 1500


def boss_odds(hp_ratio: float) -> float:
    """Chance of beating the boss given how healthy we arrive.

    From the run history: at 75% HP or more the act 1 boss was beaten four
    times in five, between 50% and 75% one time in four, below that never.
    """
    return max(0.0, min(1.0, (hp_ratio - 0.30) / 0.60))


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
            score += 0.3 + min(5.0, gold / 120.0)
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
    threshold = SKIP_THRESHOLD + knob("pickiness") + (0.0 if mem.deck_size < 16 else 1.0 if mem.deck_size < 20 else 2.0 if mem.deck_size < BIG_DECK else 3.0)
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
    heal_first = hp < (0.9 if mem.boss_next else 0.6)
    node = mem.map_nodes.get(mem.map_at) if mem.map_nodes else None
    if node is not None and str(node.get("type")) == "RestSite" and (node.get("children") or []):
        act = (state.get("run") or {}).get("act")
        args = (max_hp, int(player.get("gold") or 0), act if isinstance(act, int) else 1, _mend(player))
        smith = knob("upgrade_worth") + best_onward(mem.map_nodes, mem.map_at, [], hp_now, *args, [MAX_PATHS])
        healed = min(float(max_hp), hp_now + REST_HEAL * max_hp)
        rest = best_onward(mem.map_nodes, mem.map_at, [], healed, *args, [MAX_PATHS])
        heal_first = rest > smith and hp < 0.95
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
            want = 2.0 if not belt_full and gold - price >= 60 else -1.0
        else:
            want = -1.0
        if want > 0:
            scored.append((want, item))
    scored.sort(key=lambda t: -t[0])
    out: list[Action] = [("shop_purchase", {"index": int(i.get("index", 0))}) for _, i in scored]
    out.append(("proceed", {}))
    return out


# ------------------------------------------------------------------- events

_GOOD = [
    (r"\brelics?\b", 6.0),
    (r"\bremove\b", 4.0),
    (r"\bupgrade\b", 4.0),
    (r"\bmax hp\b", 3.0),
    (r"\bheal\b", 2.5),
    (r"\bgold\b", 2.0),
    (r"\btransform\b", 2.0),
    (r"\bpotions?\b", 1.5),
    (r"\bcards?\b", 1.0),
    (r"\benchant\b", 3.0),
    (r"draw \d+ additional cards?", 5.0),  # every turn, for the rest of the run
    (r"energy_icon", 5.0),
]
_BAD = [(r"\bcurse\b", 6.0), (r"\bfight\b", 2.0), (r"draw \d+ fewer cards?", 4.0), (r"\bdowngrade\b", 2.0), (r"\bsacrifice\b", 5.0)]
# Options that end the run in all but name. One event offers ever larger
# rewards for Max HP until the last step, "Lose Everything", leaves 1 Max HP;
# the bot walked into it nine times.
_RUINOUS = re.compile(r"\blose (all|everything)\b|\bset your max hp\b|\bmax hp (to|becomes) \d\b", re.I)


_JUNK_NAMES = {
    "CLUMSY", "GUILTY", "NORMALITY", "BADLUCK", "REGRET", "POORSLEEP", "INJURY", "WOUND", "DAZED", "SOOT",
    "SLIMED", "INFECTION", "ENTHRALLED", "DOUBT", "SHAME", "DECAY", "PARASITE", "PAIN", "WRITHE",
}


# What the relics offered by the Ancients (the event that opens each act) are
# worth to this bot, on the scale of the scores below. Reading their text word
# by word got these badly wrong: energy every turn with a curse attached
# scored below "upgrade a card", and +31 Max HP below a random relic.
_ANCIENT = {
    # more energy or cards every turn
    "BLOOD_SOAKED_ROSE": 6.5, "SPIKED_GAUNTLETS": 13.5, "PHILOSOPHERS_STONE": 13.5, "SOZU": 12.5,
    "BLESSED_ANTLER": 12.5, "PAELS_FLESH": 12.5, "PAELS_BLOOD": 13.0, "FIDDLE": 11.5, "WHISPERING_EARRING": 11.0,
    "PAELS_TEARS": 9.0, "VERY_HOT_COCOA": 10.0, "PUMPKIN_CANDLE": 8.5, "SEAL_OF_GOLD": 8.0, "BOOMING_CONCH": 8.0,
    # something every turn or every fight
    "SAI": 13.0, "CROSSBOW": 12.0, "TOASTY_MITTENS": 11.0, "MUSIC_BOX": 10.0, "DELICATE_FROND": 10.0,
    "CHOICES_PARADOX": 9.0, "DIAMOND_DIADEM": 9.0, "PAELS_LEGION": 8.0, "RADIANT_PEARL": 7.5, "JEWELED_MASK": 7.0,
    "BRILLIANT_SCARF": 6.5, "FUR_COAT": 12.0,
    # health, gold, relics
    "LOOMING_FRUIT": 13.0, "NUTRITIOUS_OYSTER": 8.5, "STONE_HUMIDIFIER": 7.0, "LORDS_PARASOL": 12.0,
    "SIGNET_RING": 9.5, "LARGE_CAPSULE": 9.5, "SMALL_CAPSULE": 9.0, "TOY_BOX": 9.0, "LAVA_ROCK": 8.0,
    "NEOWS_BONES": 7.0, "GOLDEN_PEARL": 6.5, "CURSED_PEARL": 6.0,
    # the deck
    "YUMMY_COOKIE": 10.5, "PAELS_TOOTH": 10.0, "JEWELRY_BOX": 10.0, "WAR_HAMMER": 9.0, "FISHING_ROD": 9.0,
    "BIIIG_HUG": 9.0, "ARCHAIC_TOOTH": 8.5, "NUTRITIOUS_SOUP": 8.5, "PAELS_CLAW": 8.0, "BEAUTIFUL_BRACELET": 8.0,
    "TRI_BOOMERANG": 7.5, "PAELS_GROWTH": 7.5, "PHIAL_HOLSTER": 7.5, "PRECISE_SCISSORS": 7.0, "PRESERVED_FOG": 7.0,
    "PRECARIOUS_SHEARS": 5.0, "LOST_COFFER": 7.0, "ARCANE_SCROLL": 7.0, "CLAWS": 7.0, "SILVER_CRUCIBLE": 7.0,
    "GLITTER": 7.0, "POMANDER": 6.5, "KALEIDOSCOPE": 6.5, "SCROLL_BOXES": 6.5, "DEMON_GLASS": 6.5,
    "LEAD_PAPERWEIGHT": 6.5, "DUSTY_TOME": 6.5, "STORYBOOK": 6.5, "NEOWS_TALISMAN": 6.0, "NEW_LEAF": 5.5,
    "DOWSING_ROD": 5.5, "TANXS_WHISTLE": 5.5, "NEOWS_TORMENT": 5.5, "PAELS_HORN": 5.5, "GOLDEN_COMPASS": 5.0,
    # things the bot cannot make use of
    "WINGED_BOOTS": 4.0, "PAELS_WING": 4.0, "MEAT_CLEAVER": 3.5, "PAELS_EYE": 3.0, "DISTINGUISHED_CAPE": 3.0,
}


def _event_option_value(opt: dict, hp: int, max_hp: int) -> float:
    text = f"{opt.get('title') or ''}. {opt.get('description') or ''}".lower()
    known = _ANCIENT.get(re.sub(r"[^A-Z0-9]+", "_", re.sub(r"['\u2019]", "", str(opt.get("title") or "")).upper()).strip("_"))
    if known is not None:
        return known
    score = 0.5
    if opt.get("relic_name"):
        score += 6.0
    for pat, w in _GOOD:
        if re.search(pat, text) and not re.search(r"(?:lose|pay|give)[^.]*" + pat, text):
            score += w
    # "Obtain the Sword of Stone", "Receive the Spoils Map": a relic by name.
    if not opt.get("relic_name") and "relic" not in text and re.search(r"\b(?:obtain|receive) (?:the )?(?!a\b|an\b|\d)[a-z]", text):
        score += 5.0
    m = re.search(r"\bpay (\d+) gold", text)
    if m:
        score -= int(m.group(1)) / 150.0
    for pat, w in _BAD:
        if re.search(pat, text):
            score -= w
    if _RUINOUS.search(text):
        score -= 1000.0
    m = re.search(r"\.\s*(.+?) is removed from your deck", text)
    if m:
        # A named card goes: good riddance to a Strike, Defend or curse, a loss otherwise.
        gone = {"name": m.group(1).strip().title()}
        junk = is_basic(gone) or re.sub(r"\W", "", gone["name"]).upper() in _JUNK_NAMES
        score += 2.0 if junk else -0.8 * max(0.0, deck_value(gone) - 2.0)
    m = re.search(r"\badd (?:an? |1 )?([a-z' !-]+?) to your deck", text)
    if m:
        gained = {"name": m.group(1).strip().title()}
        if re.sub(r"\W", "", gained["name"]).upper() in _JUNK_NAMES:
            score -= 4.0
        else:
            score += 0.5 * (deck_value(gained) - 3.0) if _known_card(gained) else 0.0
    m = re.search(r"lose\s+(\d+)(%?)\s+max\s+hp", text)
    if m:
        # Max HP never comes back: a small bite is a fair price for an
        # upgrade or a relic, a big one is not worth anything on offer.
        share = int(m.group(1)) / 100.0 if m.group(2) else int(m.group(1)) / max(1, max_hp)
        score -= 1.5 + 60.0 * share + (1000.0 if share >= 0.25 else 0.0)
    m = re.search(r"(?:lose|take)\s+(\d+)\s+(?:hp|damage)", text)
    if m:
        loss = int(m.group(1))
        if loss >= hp:
            score -= 1000.0
        else:
            score -= 12.0 * loss / max(1, hp)
    if re.search(r"heal", text) and hp >= 0.9 * max_hp:
        score -= 2.0
    return score


def event(state: dict, mem: Memory) -> list[Action]:
    ev = state.get("event") or {}
    if ev.get("in_dialogue"):
        return [("advance_dialogue", {})]
    opts = [o for o in ev.get("options") or [] if not o.get("is_locked")]
    if not opts:
        return [("advance_dialogue", {}), ("proceed", {})]
    player = state.get("player") or {}
    hp, max_hp = int(player.get("hp") or 1), int(player.get("max_hp") or 1)
    proceeds = [o for o in opts if o.get("is_proceed")]
    rest = [o for o in opts if not o.get("is_proceed")]
    rest.sort(key=lambda o: _event_option_value(o, hp, max_hp), reverse=True)
    if log.isEnabledFor(logging.DEBUG) and rest:
        log.debug(
            "event %s: %s",
            ev.get("event_id"),
            " | ".join(f"[{_event_option_value(o, hp, max_hp):.1f}] {o.get('title')}: {o.get('description')}" for o in rest),
        )
    ordered = proceeds + rest if proceeds else rest
    return [("choose_event_option", {"index": int(o.get("index", 0))}) for o in ordered]


# --------------------------------------------------------------- selections

_WORST_FIRST = re.compile(r"remove|transform|exhaust|discard|lose|purge|sacrifice|destroy", re.I)


def _removal_rank(card: dict) -> float:
    """Lower = get rid of it first."""
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


def card_select(state: dict, mem: Memory) -> list[Action]:
    cs = state.get("card_select") or {}
    prompt = str(cs.get("prompt") or "")
    kind = str(cs.get("screen_type") or "")
    cards = cs.get("cards") or []
    key = f"{_where(state)}|{kind}|{prompt}|{len(cards)}"
    if key != mem.select_key:
        mem.select_key, mem.selected = key, set()

    ordered = _pick_order(cards, prompt, kind, mem.upgrade_gain)
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


def hand_select(state: dict, mem: Memory) -> list[Action]:
    hs = state.get("hand_select") or {}
    prompt = str(hs.get("prompt") or "")
    kind = "upgrade" if hs.get("mode") == "upgrade_select" else ""
    out: list[Action] = []
    if hs.get("can_confirm"):
        out.append(("combat_confirm_selection", {}))
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
