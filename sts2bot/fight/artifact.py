"""Artifact on an enemy: a charge taken off is a later debuff that lands in its place, if the deck will apply enough of them."""

from __future__ import annotations

from ..cards import CardInfo, parse
from ..learn import knob

LATER = 0.7  # a debuff that lands later in the fight, against the same one landing now
FIGHT_ROUNDS = {"boss": 9, "elite": 6}  # how long such a fight usually runs (an ordinary one: 4)
HAND = 5.0  # cards seen in a turn


def landed(c: CardInfo, fight: str) -> tuple[int, float]:
    """(charges a card uses up on one enemy, what its debuffs are worth there once they land), in the planner's points."""
    vuln = {"boss": 7.0, "elite": 5.0}.get(fight, 3.0) * knob("debuff_worth")
    weak = {"boss": 5.0, "elite": 3.5}.get(fight, 2.0) * knob("debuff_worth")
    venom = {"boss": 3.0, "elite": 2.5}.get(fight, 2.0)  # a point of Poison over the turns it lasts
    parts = (min(c.vulnerable, 3) * vuln, min(c.weak, 3) * weak, 0.8 * c.doom, venom * c.poison, 1.0 * c.enemy_strength_loss)
    return sum(1 for p in parts if p > 0), float(sum(parts))


def outlook(player: dict, fight: str, round_no: int) -> tuple[float, float]:
    """(what a debuff of this deck is worth once it lands, how many it will still apply before the fight ends)."""
    piles = [c for pile in ("hand", "draw_pile", "discard_pile") for c in (player.get(pile) if isinstance(player.get(pile), list) else [])]
    uses, worth = 0, 0.0
    for c in piles:
        n, v = landed(parse(c), fight)
        uses, worth = uses + n, worth + v
    if not uses:
        return (0.0, 0.0)
    turns = max(1, FIGHT_ROUNDS.get(fight, 4) - round_no)
    return (LATER * worth / uses, uses * turns * HAND / max(HAND, len(piles)))


def gain(before: int, after: int, view: tuple) -> float:
    """What a line that took an enemy's Artifact from `before` to `after` earned by it."""
    each, coming = view
    # (The charge on top is the last to pay: the deck has to get through the ones under it first.)
    return each * sum(min(1.0, max(0.0, coming - (left - 1))) for left in range(after + 1, before + 1))
