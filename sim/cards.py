"""Cards as the simulator holds them: a definition from a game's tables, and a copy in a deck or a fight."""

from __future__ import annotations

import re
from dataclasses import dataclass
from functools import lru_cache

from sts2bot.cards import CardInfo, parse

_KEYWORD = {k: re.compile(rf"(?:^|[.!]\s*){k}(?:\.|$)", re.I) for k in ("Exhaust", "Ethereal", "Retain", "Innate", "Unplayable")}
_DEAL = re.compile(r"\b(deal)\s+\d+\s+damage", re.I)
_DEAL_EQUAL = re.compile(r"\bdeal\s+damage\s+equal\s+to\s+[^.]*", re.I)
_BLOCK = re.compile(r"\b(gain)\s+\d+\s+block", re.I)


@dataclass(frozen=True)
class CardDef:
    key: str  # the tables' key, also the card's id: POMMEL_STRIKE
    name: str
    type: str  # Attack, Skill, Power, Status, Curse
    cost: int  # -1: X; -2: unplayable
    rarity: str  # Basic, Common, Uncommon, Rare, Event, Status, Curse
    targeted: bool
    text: str
    text_up: str
    cost_up: int
    color: str  # the character whose pool it is in, or COLORLESS, STATUS, CURSE
    star_cost: int | None = None
    star_cost_up: int | None = None
    keywords: tuple = ()  # (Slay the Spire 2 lists Exhaust, Ethereal, Retain, Innate, Sly ... apart from the text)
    keywords_up: tuple = ()


@lru_cache(maxsize=None)
def info_of(d: CardDef, up: bool) -> CardInfo:
    """What the card's printed text says (base numbers: the engine adds Strength and the rest itself)."""
    kws = d.keywords_up if up else d.keywords
    return parse({"id": d.key, "name": d.name, "type": d.type, "cost": "X" if d.cost == -1 else str(max(0, d.cost_up if up else d.cost)),
                  "description": d.text_up if up else d.text, "target_type": target_type(d, up),
                  "star_cost": d.star_cost_up if up and d.star_cost_up is not None else d.star_cost,
                  "keywords": [{"name": k, "description": ""} for k in kws]})


def target_type(d: CardDef, up: bool = False) -> str:
    if d.targeted:
        return "AnyEnemy"
    return "AllEnemies" if re.search(r"\ball enemies\b", d.text_up if up else d.text, re.I) else "Self"


class Card:
    """A copy of a card. `cost_fight`/`cost_turn` override the printed cost (Madness, Infernal Blade, Swivel)."""

    __slots__ = ("d", "up", "cost_fight", "cost_turn", "bonus", "retained")

    def __init__(self, d: CardDef, up: bool = False):
        self.d, self.up = d, up
        self.cost_fight: int | None = None
        self.cost_turn: int | None = None
        self.bonus = 0  # damage or Block a card has grown by this fight (Rampage, Windmill Strike, Perseverance)
        self.retained = 0

    @property
    def key(self) -> str:
        return self.d.key

    @property
    def type(self) -> str:
        return self.d.type

    @property
    def name(self) -> str:
        return self.d.name + ("+" if self.up else "")

    @property
    def text(self) -> str:
        return self.d.text_up if self.up else self.d.text

    @property
    def info(self) -> CardInfo:
        return info_of(self.d, self.up)

    @property
    def printed_cost(self) -> int:
        return self.d.cost_up if self.up else self.d.cost

    def cost(self) -> int:
        base = self.printed_cost
        if base < 0:
            return base
        for c in (self.cost_turn, self.cost_fight):
            if c is not None:
                return c
        return base

    @property
    def star_cost(self) -> int | None:
        return self.d.star_cost_up if self.up and self.d.star_cost_up is not None else self.d.star_cost

    @property
    def keywords(self) -> tuple:
        return self.d.keywords_up if self.up else self.d.keywords

    def has(self, keyword: str) -> bool:
        return keyword in self.keywords or keyword in _KEYWORD and bool(_KEYWORD[keyword].search(self.text))

    def copy(self) -> Card:
        c = Card(self.d, self.up)
        c.cost_fight, c.bonus = self.cost_fight, self.bonus
        return c

    def view(self, index: int | None = None, damage: int | None = None, block: int | None = None, playable: bool = True) -> dict:
        """The card as the bot's state lists it; `damage`/`block`: the numbers the game shows for this moment of the fight."""
        cost = self.cost()
        desc = self.text
        if damage is not None and self.type == "Attack":
            desc = _DEAL.sub(lambda m: f"{m.group(1)} {max(0, damage)} damage", _DEAL_EQUAL.sub(f"Deal {max(0, damage)} damage", desc))
        if block is not None:
            desc = _BLOCK.sub(lambda m: f"{m.group(1)} {max(0, block)} Block", desc)
        out = {
            "id": self.d.key, "name": self.name, "type": self.type, "cost": "X" if cost == -1 else str(max(0, cost)),
            "star_cost": self.star_cost, "description": desc, "target_type": target_type(self.d, self.up),
            "can_play": playable and cost != -2, "is_upgraded": self.up, "rarity": self.d.rarity,
        }
        if cost == -2:
            out["unplayable_reason"] = "Unplayable"
        if self.keywords:
            out["keywords"] = [{"name": k, "description": ""} for k in self.keywords]
        if index is not None:
            out["index"] = index
        return out
