"""The Insatiable's Sandpit: what the margin left on it is worth.

Sandpit counts down every turn and at 0 we are eaten, whatever our HP. Each
Frantic Escape played pushes it back by one and makes that copy a point
dearer. So the fight costs about one Escape for every round it lasts past
the fourth, whenever they are played, and the only questions are how many
rounds it still needs and whether the Escapes come to hand in time.

The margin used to be priced by itself (180 points on 2, 55 on 3, 16 on 4,
5 on 5), whatever was left of the boss. Of 15 fights lost on Ascension 6-7,
three ended in the pit on 18 to 38 HP with the boss on 26, 37 and 92 (two
or three hands running without an Escape, after turns that had let one go
or played one where two were in hand), and six ended on our HP with the
boss on 13 to 49 and a margin of 3 to 5 that was never used: one to three
Escapes, up to five energy, that should have been damage.

Two toys of the fight said how hard to lean on it (the real deck's draws and
Escape costs, the planner's own points). Without our HP in it, any price of
this shape beat the old one. With HP, Block cards and the boss's cycle in
it, the same price at the old weight of a death (600) lost more fights on
HP than it saved from the pit: it banked Escapes on turns that needed the
Block (0.79 of fights won against 0.82). At half that weight and half an
energy for an Escape still owed it won 0.84 and halved the fights lost to
the pit (0.017 against 0.038, 36 sorts of deck, 3,000 fights each). So the
weights below are the toy's, not an argument's.
"""

from __future__ import annotations

from functools import lru_cache

DEATH = 300.0  # what being eaten costs, in points (600 before: see above)
ESCAPE_COST = 7.0  # what an Escape still to be played costs (half an energy: at a whole one the toy won less)
PACE = 32.0  # damage a round the Insatiable usually takes, until the fight says otherwise
SLACK = 0.8  # the rounds still needed are counted at this share of the pace: a slow hand is what kills


@lru_cache(maxsize=4096)
def _eaten(left: int, turns: int, soon: float, later: float) -> float:
    """Chance of the pit over `turns` more turns of ours, this one ending on `left`, every Escape drawn played."""
    if left <= 1:
        return 1.0
    if turns <= 1:
        return 0.0  # the boss dies on our next turn: nothing more is asked of the margin
    one, two = (1.0 - soon) * 0.64, (1.0 - soon) * 0.36  # six Escapes in thirty cards: one in the hand, or more
    top = turns + 1  # more margin than the fight has turns left is worth nothing
    return sum(p * _eaten(min(left - 1 + k, top), turns - 1, later, later) for p, k in ((soon, 0), (one, 1), (two, 2)))


def price(left: int, boss_hp: int, dealt: int, round_no: int, odds: tuple = (0.3, 0.3)) -> float:
    """Points lost by ending the turn with `left` on the Sandpit and the boss on `boss_hp`.

    The chance of being eaten before the boss is dead, and the Escapes the rest
    of the fight will still ask for: with those in the price, an Escape played
    now is one not paid for later, and damage that shortens the fight saves one.
    `dealt` is what the boss had lost when this turn began, `odds` the chance of
    no Escape in the next hand and in a hand later on.
    """
    if left <= 1:
        return DEATH * 10.0
    # The pace so far, with the usual one counted as two rounds: round 1 and its potions say little about
    # round 8. (Not this turn's damage: it is in `boss_hp` already, and counted twice 12 damage on round 1
    # moved the price by 65 points.)
    pace = (max(0, dealt) + 2.0 * PACE) / (max(1, round_no) + 1.0)
    turns = 1.0 + boss_hp / max(1.0, SLACK * pace)  # our turns after this one, the kill among them
    soon, later = (round(min(1.0, max(0.0, p)), 2) for p in odds)

    def cost(t: int) -> float:
        return DEATH * _eaten(min(left, t + 1), t, soon, later) + ESCAPE_COST * max(0, t - (left - 1))

    low = int(turns)
    return cost(low) + (turns - low) * (cost(low + 1) - cost(low))
