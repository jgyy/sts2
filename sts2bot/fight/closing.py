"""The end of a boss fight is a race: what our HP is worth once the boss is about to die.

A boss that a turn of our usual damage would kill is dying next turn, and HP we still hold when it dies is
worth nothing: so once a line leaves it within CLOSE_REACH turns of the damage we have dealt it a round so
far, the HP that line costs (short of all of it) is priced at CLOSE_PRICE of the usual. The usual price
rises as our HP falls, which is backwards at the end of a race: of 42 act 2 bosses lost in the logs of
2026-11-21, 7 ended with the boss within one turn of that damage and 5 more within two (a Knowledge
Demon on 6, a Crusher on 8, an Insatiable on 16). Tried on a model of the
Knowledge Demon whose best play is known exactly (scripts/demonfight.py, 2,400 fights an arm, the same
hands): the planner gave away 3.6 points of its chance of winning a fight without this, 2.9 with it (won
45.2% and 46.3% where perfect play wins 49.0%); a reach of 0.7 gave away 3.3 and 1.3 gave 3.1, and a price
of 0.2 did what 0.4 does. Bosses only: after an elite the HP has a run to pay for.
"""

from __future__ import annotations

CLOSE_REACH = 1.0  # turns of our usual damage within which a boss counts as dying next turn (0: the rule is off)
CLOSE_PRICE = 0.4  # ... and what our HP costs then, as a share of the usual price


def pace(state: dict) -> float:
    """HP the enemies still standing have lost a round so far (0 before round 3, and outside boss fights)."""
    battle = state.get("battle") or {}
    round_no = int(battle.get("round") or 1)
    if round_no < 3 or str(state.get("state_type") or "") != "boss":
        return 0.0
    gone = sum(max(0, int(e.get("max_hp") or 0) - int(e.get("hp") or 0)) for e in battle.get("enemies") or [] if int(e.get("max_hp") or 0) < 100_000)
    return gone / (round_no - 1)


def discount(pace: float, lost: int, hp: int, survivors: list) -> float:
    """What to multiply the price of the HP a line loses by: CLOSE_PRICE when the line leaves every leader within reach, else 1."""
    leaders = [e for e in survivors if not e.minion]
    if not pace or not CLOSE_REACH or lost >= hp or not leaders or any(e.adapts or e.spent or e.respawns for e in survivors):
        return 1.0  # (the Test Subject comes back, so does the first game's Awakened One, and all of our HP is never cheap)
    return CLOSE_PRICE if sum(e.hp + e.block for e in leaders) <= CLOSE_REACH * pace else 1.0
