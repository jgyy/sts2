"""The Insatiable's Sandpit: what the margin left on it is worth. It counts down every turn and at 0 we are eaten; each Frantic
Escape played pushes it back by one. What matters is how many rounds the fight still needs, and whether Escapes come in time."""

from __future__ import annotations

from functools import lru_cache

DEATH = 300.0  # what being eaten costs, in points
ESCAPE_COST = 7.0  # what an Escape still to be played costs (half an energy)
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
    """Points lost by ending the turn with `left` on the Sandpit and the boss on `boss_hp`: the chance of being eaten first, plus
    the Escapes the rest of the fight will ask for. `dealt`: what the boss had lost before this turn; `odds`: as battle._escape_odds."""
    if left <= 1:
        return DEATH * 10.0
    # The pace so far, the usual one counted as two rounds; not this turn's damage, which is in `boss_hp` already.
    pace = (max(0, dealt) + 2.0 * PACE) / (max(1, round_no) + 1.0)
    turns = 1.0 + boss_hp / max(1.0, SLACK * pace)  # our turns after this one, the kill among them
    soon, later = (round(min(1.0, max(0.0, p)), 2) for p in odds)

    def cost(t: int) -> float:
        return DEATH * _eaten(min(left, t + 1), t, soon, later) + ESCAPE_COST * max(0, t - (left - 1))

    low = int(turns)
    return cost(low) + (turns - low) * (cost(low + 1) - cost(low))
