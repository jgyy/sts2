"""The Fool's Arrows, Aim, Stretch and Melodies, as a line of cards the planner tries plays out.

Arrows. A card that Shoots puts our Arrow in its target (the enemy shows IMPALED 3), and the Arrow stays there until
three more hits land on that enemy (Block or not: the Tunneler's Burrowed Block took the count from 3 to 2) or it dies.
The shot's own hits do not count (Rapid Shot, 3 hits, left 3 to go). With no Arrow to hand a Shot is paid for and does
nothing: in the Fool's first ten runs on the beta 40-odd Shots went into enemies already holding our only Arrow (an
Infested Prism fight lost five cards that way), and none of those runs got past floor 33.
Aim ("Your next Arrow deals more damage") is in every Shot's text and goes with the first one; it counts on each hit
(Rapid Shot 9 x 3 at Aim 6), and twice on Heavy Shot. Stretch ("the next time you gain Block from a card") is in every
Block card's text too, and only the first one gets it.
Melodies: playing a Wah, Cho or Ki card (the words of its name) gives that Melody for the rest of the turn.
The powers that pay out "whenever you gain a Melody" do so once a Melody a turn, when it is new (read live, 23 Nov
22:20, Song Path+ and Motif+ up: Wah Cho+ as the first Cho card of the turn gave its 14 Block and 7 more, the Wah Cho
and Cho Ready+ after it only their own; Motif drew a card on Wah Touch, Ki Whip and Wah Cho+, the turn's first Wah,
Ki and Cho, and none on the Cho cards after). Yah!'s energy comes the first time each turn an Arrow comes out of an
enemy, whatever takes it out: the third hit on it (Ki Whip, energy 5 -> 7 under Yah!+), its death (Bow Smack's kill,
+1 of its own and +2), or a card that Dislodges.
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
    """(Song Path's Block, Motif's draw, Lullaby's Block, Ki Verse's cards, Yah!'s energy) as the powers on us show them,
    () with none of them up. Yah! counts for nothing once an Arrow has come out this turn (`dislodged`)."""
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
        # Song Path (Block on a new Cho Melody), Motif (cards drawn on each new Melody), Lullaby (Block once all three are
        # in), Ki Verse (Wah, Cho and Ki cards still to be played an additional time), Yah! (energy, until an Arrow comes out)
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
            if not stuck and c.impaled_only:  # (Wrest's draw too: left in, 69 of 106 Wrests drew nothing, played before the Shot)
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
        """Book what the card did; the damage to ALL enemies it sets off (Violent Chord, Explosive Arrows), if any.
        `dealt` is the damage the card's hits did to its target (Blossom Shot gains that much Stretch). `foes` are all
        the enemies; what the card sets off through Song Path, Motif, Lullaby and Yah! is left in `got`."""
        boom = energy = 0
        if self.yah:  # (the third hit on the Impaled enemy and its death are counted by now; a Dislodge is about to be)
            if sum(1 for e in foes if e.alive and e.impaled > 0) < self.stuck or (c.dislodge and target is not None and target.alive and target.impaled > 0):
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
        self.live = False  # (left on, the Strike after a Shot put the Arrow back in at 3 hits to go)
        return (self.chord if "WAH" in new else 0) + boom

    def books(self, c: CardInfo) -> bool:
        """Whether `after` has anything to do for this card (the planner skips it for the rest, most of every other character's)."""
        return bool(self.live or c.melody or c.aim or c.stretch or c.stretch_dealt or c.dislodge or c.verse or self.yah)

    def later(self, dmg_value: float) -> float:
        """What the Aim and Stretch this line adds and leaves unspent are worth next turn."""
        return min(self.aim, self.aim_got) * AIM_LATER * dmg_value + min(self.stretch, self.stretch_got) * STRETCH_LATER
