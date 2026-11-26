"""What the things an event gives and takes are worth, on the scale its options are scored on (a relic is 6, an upgrade or a
card removed 4): the shared prices the per-event rules (sts2.py, sts1.py) are built from, in one place so that every event
weighs gold, HP, curses and cards alike."""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from ...cards import _card_key, deck_value, is_basic, parse

RELIC = 6.0
POTION = 1.5
CARD_REWARD = 1.2  # a card reward of three, the best one taken (or none)
UPGRADE = 4.0  # an upgrade of our choosing (the best card for it)
RANDOM_UPGRADE = 2.0  # one at random (as often a card we hardly play)
ACT3_HP = 1.6  # a point of HP given up in act 3 is dearer (the last boss is won from full HP)
GOLD_RATE = {1: 1 / 40, 2: 1 / 45, 3: 1 / 70}  # points a gold: shops left to spend it in (a relic is about 200 gold, a removal 100)
GOLD_FULL = 300  # gold past this is worth half as much: there is only so much on a shelf
FIGHTS_PER_FLOOR = 0.55  # combats a floor, on the way to the last boss
LAST_FLOOR = {"sts2": 48, "sts1": 50}

# Curses and the like by what they cost in the deck (the generic scorer's table, events._CURSES, uses the same numbers).
CURSE = {
    "ascenders bane": 1.5, "clumsy": 1.5, "guilty": 1.0, "poor sleep": 6.0, "folly": 3.0, "spore mind": 2.5, "injury": 3.0, "debt": 4.0, "decay": 4.5,
    "doubt": 4.5, "shame": 4.5, "greed": 5.0, "curse of the bell": 5.0, "regret": 5.0, "writhe": 5.0, "normality": 8.0, "enthralled": 8.0,
    "bad luck": 10.0, "pain": 5.0, "parasite": 4.0, "necronomicurse": 6.0, "pride": 4.0, "wound": 2.0, "dazed": 1.5, "burn": 2.5, "void": 3.0,
    "slimed": 1.0, "soot": 3.0, "infection": 3.5,
}
ETERNAL = {"ascenders bane", "greed", "curse of the bell", "folly", "bad luck", "enthralled", "necronomicurse"}  # never removed
RANDOM_CURSE = 4.5  # "Add a random Curse": the mean of what can come


@dataclass
class Ctx:
    """The run as an event sees it."""
    hp: int
    max_hp: int
    gold: int = 0
    act: int = 1
    floor: int = 1
    deck: list = field(default_factory=list)  # card dicts (strategy.Memory.deck)
    relics: list = field(default_factory=list)
    potions: list = field(default_factory=list)
    slots: int = 3  # potion slots
    character: str = ""
    game: str = "sts2"
    bled: int = 0  # HP this event has taken already
    keys: list | None = None  # the first game's keys, once act 4 is open

    @property
    def free_slots(self) -> int:
        return max(0, self.slots - len(self.potions))

    @property
    def fights_left(self) -> float:
        return max(1.0, (LAST_FLOOR.get(self.game, 48) - self.floor) * FIGHTS_PER_FLOOR)


def num(pattern: str, text: str, default: int = 0) -> int:
    m = re.search(pattern, text, re.I)
    return int(m.group(1)) if m else default


def name_of(card: dict | str) -> str:
    raw = card if isinstance(card, str) else str(card.get("name") or card.get("id") or "")
    return re.sub(r"[^a-z' ]", "", raw.lower().replace("_", " ")).replace("'", "").strip()


def is_curse(card: dict) -> bool:
    return str(card.get("type")) in ("Curse", "Status") or name_of(card) in CURSE


def hp_cost(loss: int, ctx: Ctx) -> float:
    """HP given up: dear as a share of what is left, dearer in act 3, the end of the run if it is all of it."""
    if loss <= 0:
        return 0.0
    if loss >= ctx.hp:
        return 1000.0
    return 12.0 * loss / max(1, ctx.hp) * (ACT3_HP if ctx.act >= 3 else 1.0)


def heal(amount: int, ctx: Ctx) -> float:
    return 12.0 * min(max(0, amount), max(0, ctx.max_hp - ctx.hp)) / max(1, ctx.max_hp)


def max_hp_gain(n: int) -> float:
    return 0.35 * min(n, 17) + 0.2 * max(0, n - 17)


def max_hp_loss(n: int, ctx: Ctx) -> float:
    share = n / max(1, ctx.max_hp)
    return 1.5 + 60.0 * share + (1000.0 if share >= 0.25 else 0.0)


def _gold_curve(g: float, rate: float) -> float:
    return rate * min(g, GOLD_FULL) + rate / 2 * max(0.0, g - GOLD_FULL)


def gold(change: int, ctx: Ctx) -> float:
    """Gold gained (or paid, negative): what it adds to what we hold, by the shops left to spend it in."""
    rate = GOLD_RATE.get(min(ctx.act, 3), GOLD_RATE[3])
    have = max(0, ctx.gold)
    return _gold_curve(max(0, have + change), rate) - _gold_curve(have, rate)


def curse(name: str) -> float:
    return CURSE.get(name_of(name), RANDOM_CURSE)


def card_worth(card: dict | str) -> float:
    """What a card adds to the deck (0 for an average one): its rating less what an average pick is worth."""
    c = {"name": card} if isinstance(card, str) else card
    return 0.5 * (deck_value(c) - 3.0)


def _removal_value(card: dict) -> float:
    if is_curse(card):
        return 0.0 if name_of(card) in ETERNAL else curse(name_of(card)) + 1.0
    if is_basic(card) or is_basic({"name": str(card.get("name") or "").rstrip("+")}):
        up = str(card.get("name") or "").endswith("+") or card.get("is_upgraded")
        strike = "strike" in name_of(card)
        return (4.0 if strike else 3.5) - (1.0 if up else 0.0)
    return max(0.5, 3.0 - 0.5 * max(0.0, deck_value(card) - 2.0))


def removals(ctx: Ctx, n: int = 1) -> float:
    """Removing the n cards the deck is best without (a curse first, then Strikes, then Defends)."""
    worths = sorted((_removal_value(c) for c in ctx.deck), reverse=True)
    if not worths:
        return 3.5 * n  # (nothing known of the deck: a Strike's worth)
    return sum(worths[:n]) + 0.5 * max(0, n - len(worths))


def transforms(ctx: Ctx, n: int = 1) -> float:
    """Transforming the n cards we choose: each goes (as a removal) and a random card of ours comes."""
    return sum(max(1.0, w - 1.5) for w in sorted((_removal_value(c) for c in ctx.deck), reverse=True)[:n]) if ctx.deck else 2.5 * n


def upgrades_left(ctx: Ctx) -> int:
    return sum(1 for c in ctx.deck if not is_curse(c) and not str(c.get("name") or "").endswith("+") and not c.get("is_upgraded"))


def random_upgrades(n: int, ctx: Ctx) -> float:
    left = upgrades_left(ctx) if ctx.deck else 10
    return RANDOM_UPGRADE * min(n, left)


def potions_gained(n: int, ctx: Ctx, each: float = POTION) -> float:
    """Potions on a belt with room for them (the rest are mostly lost: at best one pushes out a weaker one we hold)."""
    return each * min(n, ctx.free_slots) + 0.1 * max(0, n - ctx.free_slots)


def counts(ctx: Ctx) -> dict:
    """Card sorts in the deck: Strikes, Defends (basic), Powers, attacks, Block cards, big attacks."""
    out = {"strikes": 0, "defends": 0, "powers": 0, "attacks": 0, "blocks": 0, "big_attacks": 0, "basics": 0}
    for c in ctx.deck:
        n, info = name_of(c), parse(c)
        basic = is_basic(c) or is_basic({"name": str(c.get("name") or "").rstrip("+")})
        out["basics"] += basic and not is_curse(c)
        out["strikes"] += basic and "strike" in n
        out["defends"] += basic and "defend" in n
        out["powers"] += str(c.get("type")) == "Power"
        out["attacks"] += bool(info.damage)
        out["blocks"] += bool(info.block)
        out["big_attacks"] += info.total_damage >= 10 and info.cost >= 1
    return out


def card_key(card: dict | str) -> str:
    return _card_key({"name": card} if isinstance(card, str) else card)
