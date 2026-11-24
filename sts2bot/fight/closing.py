"""The end of a boss fight is a race: HP still held when the boss dies buys nothing (not so after an elite: the run goes on)."""

from __future__ import annotations

CLOSE_REACH = 1.0  # turns of our usual damage within which a boss counts as dying next turn (0: the rule is off)
CLOSE_PRICE = 0.4  # ... and what our HP costs then, as a share of the usual price


def pace(state: dict) -> float:
    """HP the enemies have lost a round so far (0 before round 3, and outside boss fights)."""
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
        return 1.0  # (a boss that comes back is no race; losing all our HP is never cheap)
    return CLOSE_PRICE if sum(e.hp + e.block for e in leaders) <= CLOSE_REACH * pace else 1.0
