"""The Knowledge Demon's offers: which of two afflictions to carry for the rest of the fight.

Three times it makes us choose: Mind Rot (a card less every turn) or Disintegration 6 (that much damage
at the end of every turn), then Sloth (three cards a turn) or Disintegration 7, then Waste Away (an
energy less) or Disintegration 8. The burns add up, and Block stops them.

For a deck on three energy that draws four cards the limits cost little, and the burn is the dear one.
Not for every deck: a Necrobinder on 5 to 7 energy that had played 4, 6, 6 and 8 cards a turn took Sloth,
dealt 19 a round where it had dealt 40, and ended every turn with 3 energy unspent; then, with energy it
could not use, it took 8 damage a turn rather than give one up. So each offer is priced by what this
fight has shown so far: the cards we played each turn, and the energy left over when it ended.
"""

from __future__ import annotations

import logging
import re

from .battle import power_amount
from ..cards import _card_key

log = logging.getLogger("sts2bot.afflictions")

CARD_LOST = 7.5  # HP a turn that one card a turn we may not play comes to (the fight is that much longer)
# Cards a turn Sloth has to cost before the burn is the cheaper price. It was 1.25, set from one fight that
# scored 3.0. Perfect play at the Demon (scripts/demon/offers.py: 207 decks of 3 to 7 energy and hands of 5 to 7
# cards, each solved with Mind Rot taken and then Sloth or the burn) puts the crossover at 1.9 whatever the
# energy: from 1.25 to 1.75 Sloth wins in 48 decks of 49 and the burn gives away 7 points of win chance on average
# (9 in decks perfect play wins 10-90% of), at 1.75 to 2.0 they tie, from 2.0 the burn takes over (+2 points, +10
# from 2.25). It sat on the wrong side of the line in the four fights of 2026-11-22 that took it at 1.5 to 1.75.
SLOTH_MARGIN = 2.0
IDLE_ENOUGH = 0.5  # share of recent turns that ended with energy unspent, from which an energy less costs next to nothing


def lost_to_sloth(turns: list) -> float:
    """Cards a turn that three a turn would have stopped, over the turns after the first (a turn counts for three at most)."""
    later = [n for n, _left in turns[1:]]
    return sum(min(3, max(0, n - 3)) for n in later) / len(later) if len(later) >= 2 else 0.0


def idle_share(turns: list) -> float | None:
    """Share of the last four turns that ended with energy we could not spend (None: too few turns seen)."""
    recent = [left for _n, left in turns[-4:]]
    return sum(left >= 1 for left in recent) / len(recent) if len(recent) >= 2 else None


def order(ordered: list[dict], state: dict) -> list[dict]:
    """`ordered` with the affliction to take first. `state["_turns"]` is (cards played, energy left) for each turn so far."""
    keys = {_card_key(c): c for c in ordered}
    burn, drain, sloth = keys.get("DISINTEGRATION"), keys.get("WASTE_AWAY"), keys.get("SLOTH")
    if burn is None or (drain is None and sloth is None):
        return ordered
    # (The choice screen comes without the enemies: a rule that asked for the
    # Demon's HP never applied, and the energy went 38 times out of 38.)
    player = state.get("player") or {}
    hp = int(player.get("hp") or 0)
    turns = list(state.get("_turns") or [])
    m = re.search(r"take (\d+) damage", str(burn.get("description") or ""), re.I)
    per_turn = int(m.group(1)) if m else 8
    burning = max(0, power_amount(player, "DISINTEGRATION"))  # a burn taken earlier: the new one comes on top
    first = lambda c: [c] + [o for o in ordered if o is not c]
    if sloth is not None:
        # Three cards a turn against 7 damage a turn, with six turns or more to go. The burn only for a deck
        # that has been playing well over three, and with the HP to pay for it.
        lost = lost_to_sloth(turns)
        take_burn = lost >= SLOTH_MARGIN and CARD_LOST * lost > per_turn and hp > 5 * per_turn and not burning
        log.debug("affliction: cards a turn %s, Sloth would stop %.2f a turn: %s", [n for n, _ in turns], lost, "the burn" if take_burn else "Sloth")
        return first(burn if take_burn else sloth)
    # An energy less against 8 damage a turn. The damage is the cheaper price for a deck that spends all
    # its energy: an energy is about 8 Block or 10 damage every turn, so going without it costs as much
    # HP as the burn does, and the fight takes half as long again on top. Of 22 fights where the bot gave
    # up the energy (never the HP, under an older and stricter rule), it won 2; four more ended with the
    # Demon on 17 to 42 HP after three or four turns played on 2 energy.
    # ... but under Sloth half the turns end with energy nothing can be played for (90 of 192), and an
    # energy that would not have been spent costs nothing: of 21 burns taken here, 12 were taken by decks
    # that had left energy over on two or more of their last four turns. And a second burn on top of the
    # first is 15 a turn.
    idle = idle_share(turns)
    spare = burning > 0 or (idle is not None and idle >= IDLE_ENOUGH)
    # (Not when the burn alone would finish us within two turns: then every turn counts.)
    take_burn = hp > 2 * (per_turn + burning) and not spare
    log.debug("affliction: energy left at the end of each turn %s: %s", [left for _, left in turns], "the burn" if take_burn else "Waste Away")
    return first(burn if take_burn else drain)
