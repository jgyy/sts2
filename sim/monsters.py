"""Enemies: a move table per monster and the rule that picks the next move.

A move is damage (times hits), Block, and a list of effects (`fx`), each a tuple whose first word the engine
knows (engine.Fight.do): ("str", 3) Strength for itself, ("weak", 2) Weak on us, ("status", "WOUND", 2, "discard")
cards into one of our piles, ("summon", "TORCH_HEAD", 2) and so on. A pattern is a function of the monster and
the fight's random generator that names the next move; the helpers below cover the usual ones.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable


@dataclass(frozen=True)
class Move:
    dmg: int = 0
    hits: int = 1
    block: int = 0
    fx: tuple = ()
    kind: str = ""  # an intent type the move shows instead of the one worked out (Sleep, Stun, Escape, Unknown)


@dataclass
class Spec:
    key: str
    name: str
    hp: tuple[int, int]
    moves: dict[str, Move]
    pattern: Callable
    powers: dict = field(default_factory=dict)  # powers it starts the fight with
    block: int = 0  # Block it starts the fight with
    minion: bool = False
    hooks: dict = field(default_factory=dict)  # event -> fn(fight, foe, ...): "hit", "death", "end"


class Foe:
    """A monster in a fight."""

    def __init__(self, spec: Spec, n: int, hp: int, hp_mult: float = 1.0):
        self.spec, self.key, self.name = spec, spec.key, spec.name
        self.eid = f"{spec.key}_{n}"
        self.max_hp = self.hp = max(1, round(hp * hp_mult))
        self.block = spec.block
        self.powers: dict[str, int] = dict(spec.powers)
        self.minion = spec.minion
        self.history: list[str] = []
        self.move_name = ""
        self.mem: dict = {}
        self.gone = False  # dead, escaped or eaten
        self.half_dead = False  # Darklings, the Awakened One between forms: on the field, not hurt by anything

    @property
    def alive(self) -> bool:
        return not self.gone and not self.half_dead

    @property
    def move(self) -> Move:
        if self.move_name not in self.spec.moves:  # (a state machine that names a move the table lacks: its first move stands in)
            self.move_name = next(iter(self.spec.moves))
        return self.spec.moves[self.move_name]

    def choose(self, rng) -> None:
        self.move_name = self.spec.pattern(self, rng)

    def last(self, n: int = 1) -> list[str]:
        return self.history[-n:]

    def stun(self) -> None:
        """Its next move is its Stun move, if it has one."""
        self.move_name = next((m for m, mv in self.spec.moves.items() if mv.kind == "Stun"), self.move_name)


# ----------------------------------------------------------------- patterns


def counter(f: Foe, key, start: int = 0) -> int:
    """A count kept in the monster's memory: its value before this call adds one."""
    i = f.mem.get(key, start)
    f.mem[key] = i + 1
    return i


def cycle(*names: str, start: int = 0) -> Callable:
    """The moves in turn, round and round."""
    key = object()
    return lambda f, rng: names[counter(f, key, start) % len(names)]


def opening(*names: str, then: Callable) -> Callable:
    """These moves first, in order, then the pattern `then`."""
    key = object()

    def pick(f, rng):
        i = f.mem.get(key, 0)
        if i < len(names):
            f.mem[key] = i + 1
            return names[i]
        return then(f, rng)
    return pick


def weighted(weights: dict[str, float], most: dict[str, int] | None = None) -> Callable:
    """A move at random by weight, never more than `most[move]` times in a row (default: no limit)."""
    most = most or {}

    def pick(f, rng):
        ok = {m: w for m, w in weights.items() if most.get(m, 99) > _run_length(f.history, m)}
        options = ok or weights
        r = rng.random() * sum(options.values())
        for m, w in options.items():
            r -= w
            if r <= 0:
                return m
        return next(iter(options))
    return pick


def _run_length(history: list[str], move: str) -> int:
    return next((i for i, m in enumerate(reversed(history)) if m != move), len(history))
