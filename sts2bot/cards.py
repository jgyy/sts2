"""Card understanding from the text the game shows on the card.

There is no card database here on purpose: the bot reads the rendered
description, so it keeps working when the game adds or rebalances cards.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

_WORD_NUM = {"twice": 2, "thrice": 3}
_BASIC = re.compile(r"^(strike|defend)(_[a-z]+)?$", re.I)


@dataclass(frozen=True)
class CardInfo:
    index: int
    name: str
    type: str  # Attack, Skill, Power, Status, Curse
    cost: int  # energy; X-cost cards report -1
    star_cost: int
    damage: int  # per hit
    hits: int
    double_if_vulnerable: bool  # "If the enemy is Vulnerable, hits twice."
    aoe: bool
    random_target: bool
    block: int
    vulnerable: int
    weak: int
    draw: int
    energy_gain: int
    hp_loss: int
    needs_target: bool
    playable: bool
    deadly: bool  # the text says it can kill us outright ("... die.")
    block_lock: bool  # stops us gaining Block afterwards
    known: bool  # False when no effect could be parsed from the text

    @property
    def is_x(self) -> bool:
        return self.cost < 0

    @property
    def total_damage(self) -> int:
        return self.damage * self.hits


def _int(pattern: str, text: str) -> int:
    m = re.search(pattern, text, re.I)
    return int(m.group(1)) if m else 0


def _cost(raw) -> int:
    if raw is None:
        return 0
    s = str(raw).strip()
    if s.upper() == "X":
        return -1
    try:
        return max(0, int(s))
    except ValueError:
        return 0


def is_basic(card: dict) -> bool:
    return bool(_BASIC.match(str(card.get("id") or card.get("name") or "")))


def is_junk(card: dict) -> bool:
    return str(card.get("type")) in ("Status", "Curse")


def parse(card: dict) -> CardInfo:
    text = str(card.get("description") or "")
    ctype = str(card.get("type") or "")
    target = str(card.get("target_type") or "")

    damage = _int(r"deals?\s+(\d+)\s+damage", text)
    hits = 1
    double_if_vulnerable = False
    if damage:
        # A conditional second hit must not count as an unconditional one.
        plain = text
        m = re.search(r"if\s+the\s+enemy\s+is\s+vulnerable,?\s+hits\s+twice\.?", text, re.I)
        if m:
            double_if_vulnerable = True
            plain = text[: m.start()] + text[m.end() :]
        hits = _int(r"(\d+)\s+times", plain) or 1
        for word, n in _WORD_NUM.items():
            if re.search(rf"\b{word}\b", plain, re.I):
                hits = n
    block = _int(r"gain\s+(\d+)\s+block", text)
    vulnerable = _int(r"apply\s+(\d+)\s+vulnerable", text) or _int(r"(\d+)\s+vulnerable", text)
    weak = _int(r"apply\s+(\d+)\s+weak", text) or _int(r"(\d+)\s+weak", text)
    draw = _int(r"draw\s+(\d+)\s+card", text) or (1 if re.search(r"draw\s+a\s+card", text, re.I) else 0)
    energy_gain = _int(r"gain\s+(\d+)\s+energy", text)
    hp_loss = _int(r"lose\s+(\d+)\s+hp", text)

    aoe = target == "AllEnemies" or bool(re.search(r"all\s+enemies", text, re.I))
    random_target = bool(re.search(r"random\s+enemy", text, re.I))
    known = any((damage, block, vulnerable, weak, draw, energy_gain))

    return CardInfo(
        index=int(card.get("index", 0)),
        name=str(card.get("name") or card.get("id") or "?"),
        type=ctype,
        cost=_cost(card.get("cost")),
        star_cost=_cost(card.get("star_cost")) if card.get("star_cost") not in (None, "") else 0,
        damage=damage,
        hits=hits,
        double_if_vulnerable=double_if_vulnerable,
        aoe=aoe,
        random_target=random_target,
        block=block,
        vulnerable=vulnerable,
        weak=weak,
        draw=draw,
        energy_gain=energy_gain,
        hp_loss=hp_loss,
        needs_target=target == "AnyEnemy",
        playable=bool(card.get("can_play", True)),
        deadly=bool(re.search(r"\bdie\b", text, re.I)),
        block_lock=bool(re.search(r"(cannot|can't|can not)\s+gain\s+(any\s+)?block", text, re.I)),
        known=known,
    )


# A nudge for cards whose real strength the text does not show (or
# overstates). The text-based score below still does most of the work, so
# cards that are not listed are judged on what they say. Values are in the
# same units as deck_value: +1 is roughly one rarity step.
_KNOWN = {
    # strong for what they cost
    "OFFERING": 3.0, "SHOCKWAVE": 3.0, "IMMOLATE": 3.0, "IMPERVIOUS": 2.0, "FLAME_BARRIER": 2.0,
    "SHRUG_IT_OFF": 2.0, "UPPERCUT": 2.0, "DISARM": 2.0, "REAPER": 2.0, "FEED": 1.5,
    "BATTLE_TRANCE": 1.0, "INFLAME": 1.5, "DEMON_FORM": 1.5, "POMMEL_STRIKE": 1.5, "CARNAGE": 1.5,
    "SPOT_WEAKNESS": 1.5, "METALLICIZE": 1.5, "TWIN_STRIKE": 1.0, "THUNDERCLAP": 1.0, "HEMOKINESIS": 1.0,
    "WHIRLWIND": 1.0, "BLUDGEON": 1.0, "CLEAVE": 1.0, "IRON_WAVE": 1.0, "ARMAMENTS": 1.0,
    "POWER_THROUGH": 1.0, "GHOSTLY_ARMOR": 1.0, "SEEING_RED": 1.0, "INTIMIDATE": 1.0, "FIEND_FIRE": 1.0,
    "HEADBUTT": 0.5, "ANGER": 0.5, "TRUE_GRIT": 0.5, "BURNING_PACT": 0.5, "RAMPAGE": 0.5,
    "PERFECTED_STRIKE": 0.5, "COMBUST": 0.5, "DOUBLE_TAP": 0.5,
    # need a deck built around them, or are just weak
    "BARRICADE": -1.5, "ENTRENCH": -1.5, "BODY_SLAM": -1.0, "JUGGERNAUT": -1.0, "CLASH": -1.0,
    "RUPTURE": -1.0, "SEARING_BLOW": -1.0, "RECKLESS_CHARGE": -1.0, "FEEL_NO_PAIN": -1.0,
    "DARK_EMBRACE": -1.0, "FIRE_BREATHING": -1.0, "EVOLVE": -1.0, "BERSERK": -0.5, "WARCRY": -0.5,
    "HAVOC": -0.5, "WILD_STRIKE": -0.5, "DUAL_WIELD": -0.5, "RAGE": -0.5, "LIMIT_BREAK": -0.5,
}


def _known_nudge(card: dict) -> float:
    for field in ("id", "name"):
        key = re.sub(r"[^A-Z0-9]+", "_", str(card.get(field) or "").upper().rstrip("+")).strip("_")
        if key in _KNOWN:
            return _KNOWN[key]
    return 0.0


_RARITY = {"Basic": 0.0, "Common": 1.0, "Uncommon": 2.2, "Rare": 3.5, "Ancient": 4.5, "Event": 2.0}


def deck_value(card: dict) -> float:
    """How much we want this card in the deck (higher is better).

    Used for card rewards, shop buys, and upgrade / removal picks.
    """
    ctype = str(card.get("type") or "")
    if ctype == "Curse":
        return -10.0
    if ctype == "Status":
        return -8.0
    info = parse(card)
    if info.deadly:
        return -6.0  # e.g. "If you take unblocked attack damage this combat, die."
    cost = max(1, info.cost) if not info.is_x else 2
    score = _RARITY.get(str(card.get("rarity") or card.get("card_rarity") or ""), 1.0)

    dmg = info.total_damage * (1.6 if info.aoe else 1.0)
    score += max(0.0, dmg / cost - 6.0) * 0.45
    score += max(0.0, info.block / cost - 5.0) * 0.45
    score += min(_int(r"gain\s+(\d+)\s+strength", str(card.get("description") or "")), 4) * 1.0
    score += info.draw * 0.8 + info.energy_gain * 1.2
    score += min(info.vulnerable, 3) * 0.6 + min(info.weak, 3) * 0.4
    if info.cost == 0 and info.known:
        score += 0.5
    if ctype == "Power":
        # Powers we cannot read are a gamble; they also cost a turn of tempo.
        score += 1.5 if info.known else 0.5
    if info.cost >= 3:
        score -= 0.8
    if info.block_lock:
        score -= 3.0
    score -= info.hp_loss * 0.1
    if is_basic(card):
        score -= 2.5
    if card.get("is_upgraded"):
        score += 0.7
    return score + _known_nudge(card)
