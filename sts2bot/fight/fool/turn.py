"""The Fool's Arrows, Aim, Stretch and Melodies, as a line of cards the planner tries plays out.

A Shot puts our Arrow in its target (IMPALED 3) until three more hits land on it or it dies; with no Arrow to hand it does nothing.
Aim and Stretch show in every Shot's and Block card's text, and only the first such card gets them. A Wah, Cho or Ki card gives that
Melody for the turn; powers "whenever you gain a Melody" pay once a Melody a turn. Yah! pays the first time an Arrow comes out.
"""

from __future__ import annotations

from dataclasses import replace

from ...cards import MELODIES, CardInfo, melodies
from ..battle import _stacks

IMPALE_HITS = 3
AIM_LATER = 0.6  # what a point of Aim kept for a later turn is worth, in damage (the next Shot drawn takes it)
STRETCH_LATER = 0.5  # ... and a point of Stretch, in Block


def arrows_owned(player: dict) -> int:
    """Arrows we have: one, and one more for each Quiver ("Gain 1 additional Arrow")."""
    return 1 + _stacks(player, "QUIVER")


def free_arrows(arrows: int, foes: list) -> int:
    """Arrows that can still be shot: those owned less those standing in a living enemy (-1 owned: not counted)."""
    return 1 if arrows < 0 else arrows - sum(1 for e in foes if e.alive and e.impaled > 0)


def played_melodies(names) -> frozenset:
    """The Melodies the cards played so far this turn have given."""
    return frozenset(m for name in names or () for m in melodies(name))


def fool_powers(player: dict, dislodged: bool = False) -> tuple:
    """(Song Path's Block, Motif's draw, Lullaby's Block, Ki Verse's cards, Yah!'s energy), () with none up (Yah!: 0 once `dislodged`)."""
    got = tuple(_stacks(player, p) for p in ("SONG_PATH", "MOTIF", "LULLABY", "KI_VERSE")) + (0 if dislodged else _stacks(player, "YAH"),)
    return got if any(got) else ()


def impaled_pool(c: CardInfo, foes: list) -> list:
    """Whom a card that wants an Impaled target can go at: the Impaled ones, if there are any."""
    stuck = [e for e in foes if e.alive and e.impaled > 0] if c.impaled_only or c.on_impaled or c.impaled_double or c.dislodge else []
    return stuck or foes


def concerns(c: CardInfo) -> bool:
    """Whether a card needs FoolTurn.before at all (most of every other character's do not)."""
    return bool(c.shoots or c.impaled_only or c.on_impaled or c.impaled_double or c.arrow_block or c.melody_hits or c.melody_block or c.melody_twice)


class FoolTurn:
    """What a line has done so far to our Arrows, Aim, Stretch and Melodies."""

    def __init__(self, arrows: int = -1, aim: int = 0, stretch: int = 0, tune=frozenset(), chord: int = 0, boom: int = 0, powers: tuple = (), foes=()):
        self.arrows = arrows  # owned (-1: not counted)
        self.aim = self.aim0 = aim  # Aim on us now, and as the card texts were written
        self.stretch = self.stretch0 = stretch
        self.aim_got = self.stretch_got = 0  # what this line has added and not spent
        self.tune, self.tune0 = set(tune), len(tune)  # Melodies this turn (and how many the texts counted)
        self.chord = chord  # Violent Chord: damage to ALL enemies whenever we gain a Wah Melody
        self.boom = boom  # Explosive Arrows: damage to ALL enemies whenever we Impale one
        self.live = False  # the card being played Shoots an Arrow it has
        # Song Path (Block on a new Cho Melody), Motif (draws on each new Melody), Lullaby (Block once all three are in), Ki Verse (cards to play again), Yah! (energy)
        self.song, self.motif, self.lullaby, self.verse, self.yah = powers or (0, 0, 0, 0, 0)
        self.stuck = sum(1 for e in foes if e.alive and e.impaled > 0) if self.yah else 0  # enemies holding one of our Arrows
        self.got = (0, 0, 0)  # (Block, cards drawn, energy) the last card booked set off through those powers

    def encore(self, c: CardInfo) -> int:
        """Ki Verse: 1 if this card is played an additional time (it is a Wah, Cho or Ki card and a Ki Verse waits)."""
        if self.verse and c.melody:
            self.verse -= 1
            return 1
        return 0

    def before(self, c: CardInfo, foes: list, aimed_at: str | None) -> CardInfo:
        """The card as it plays out at this point of the line."""
        self.live = c.shoots and free_arrows(self.arrows, foes) > 0
        if c.shoots and not self.live:
            c = replace(c, damage=0, block=0, weak=0, vulnerable=0)  # (Hobbling Shot's Weak and Vulnerable ride on the Arrow)
        elif c.shoots and self.aim != self.aim0:
            c = replace(c, damage=max(0, c.damage + (self.aim - self.aim0) * c.aim_times))
        if c.impaled_only or c.on_impaled or c.impaled_double:
            stuck = [e for e in foes if e.alive and e.impaled > 0 and (aimed_at is None or e.entity_id == aimed_at)]
            if not stuck and c.impaled_only:  # (its draw and energy too)
                c = replace(c, damage=0, weak=0, vulnerable=0, draw=0, energy_gain=0, dislodge=False)
            elif stuck and c.on_impaled:
                c = replace(c, energy_gain=c.energy_gain + c.on_impaled[0], draw=c.draw + c.on_impaled[1])
            elif stuck and c.impaled_double:
                c = replace(c, damage=c.damage * 2)
        if c.melody_hits and len(self.tune) != self.tune0:
            c = replace(c, hits=max(1, c.hits + len(self.tune) - self.tune0))  # (its text counted the Melodies there were)
        if c.melody_block:
            c = replace(c, block=c.melody_block * len(self.tune))
        if c.melody_twice and self.tune & set(c.melody_twice):
            c = replace(c, hits=c.hits * 2)
        if c.arrow_block and free_arrows(self.arrows, foes) > 0:
            c = replace(c, block=c.block * 2)
        if c.block and (self.stretch or self.stretch0):  # every Block card's text shows the Stretch, the first one played takes it
            c, self.stretch, self.stretch_got = replace(c, block=max(0, c.block - self.stretch0 + self.stretch)), 0, 0
        return c

    def after(self, c: CardInfo, target, xs: int = 1, dealt: int = 0, foes=()) -> int:
        """Book what the card did (`dealt`: what its hits did to the target); return the damage to ALL it sets off, leave the rest in `got`."""
        boom = energy = 0
        # (Yah!: the third hit on the Impaled enemy and its death are counted by now; a Dislodge is about to be.)
        if self.yah and (sum(1 for e in foes if e.alive and e.impaled > 0) < self.stuck or (c.dislodge and target is not None and target.alive and target.impaled > 0)):
            energy, self.yah = self.yah, 0
        if self.live and target is not None and target.alive:
            target.impaled, boom = IMPALE_HITS, self.boom
        if c.dislodge and target is not None:
            target.impaled = 0  # (Uproot, Recall: the Arrow comes back)
        if self.yah:
            self.stuck = sum(1 for e in foes if e.alive and e.impaled > 0)
        if self.live:
            self.aim = self.aim_got = 0
        self.aim, self.aim_got = self.aim + c.aim * xs, self.aim_got + c.aim * xs
        gained = c.stretch + (dealt if c.stretch_dealt else 0)
        self.stretch, self.stretch_got = self.stretch + gained * xs, self.stretch_got + gained * xs
        new = set(c.melody) - self.tune
        full = len(self.tune) < len(MELODIES) <= len(self.tune | new)  # (Lullaby: "When you gain every Melody")
        self.tune |= new
        self.verse += c.verse
        self.got = (self.song * ("CHO" in new) + self.lullaby * full, self.motif * len(new), energy)
        self.live = False  # (only this card Shoots)
        return (self.chord if "WAH" in new else 0) + boom

    def books(self, c: CardInfo) -> bool:
        """Whether `after` has anything to do for this card (the planner skips it for the rest, most of every other character's)."""
        return bool(self.live or c.melody or c.aim or c.stretch or c.stretch_dealt or c.dislodge or c.verse or self.yah)

    def later(self, dmg_value: float) -> float:
        """What the Aim and Stretch this line adds and leaves unspent are worth next turn."""
        return min(self.aim, self.aim_got) * AIM_LATER * dmg_value + min(self.stretch, self.stretch_got) * STRETCH_LATER
