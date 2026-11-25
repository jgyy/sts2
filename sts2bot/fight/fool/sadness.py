"""The Fool's Sadness: "Unplayable. Retain." Sorrowful Strike, Mourning Blow, Jest, Vitriol, Rummage and Brighten Up put it in the
hand, Ennui one more every turn, and each stays there for the rest of the fight; only a Joy takes one away (Punch of Joy, Ecstasy).
A hand holds 10, so once the Sadness and a turn's draw no longer fit, every Sadness is a card not drawn, every turn after.

2026-11-25: of the Fool's boss fights with any Sadness, 18 had 5 or more in the hand and 5 had 8-10. At 18:06 it played Ennui on
round 2 against the Kin: from round 11 the hand was ten Sadness, nothing was drawn, and the Priest won from 75 HP against our 65.
The planner read Ennui as a Power it could not read (worth 7 a turn of the fight) and Sorrowful Strike as 13 damage and nothing else.

A line is scored by the cards the turns to come will draw: the first `PLAYED` of a hand at full worth (about what a turn's energy
plays), the rest at `SPARE`; with the Sadness the line leaves in the hand, and what Ennui and Ecstasy add or take away each turn.
"""

from __future__ import annotations

from ...cards import key_of, parse
from ..battle import _stacks

HAND_CAP = 10
TURN_DRAW = 5  # cards a turn draws
PLAYED, SPARE = 4, 0.4  # a hand's first 4 cards count whole (a turn's energy plays about that many), each one beyond at this share
MAX_TURNS = 15
PACE = {"boss": 28.0, "elite": 22.0}  # enemy HP a round we take off, before the fight has shown its own (an ordinary fight: 18)
LATER = 0.7  # what a card drawn on a later turn is worth against one in the hand now (the fight may be over first)


def held(player: dict) -> int:
    """Sadness in the hand now."""
    return sum(1 for c in player.get("hand") or [] if key_of(c.get("id") or c.get("name")) == "SADNESS")


def turn_rate(player: dict) -> tuple:
    """(Sadness a turn, extra cards drawn a turn) from the Powers up: Ennui's amount is its draw (2, 3 upgraded), a Sadness a copy."""
    ennui, ecstasy = max(0, _stacks(player, "ENNUI")), max(0, _stacks(player, "ECSTASY"))
    return ((max(1, round(ennui / 2.5)) if ennui else 0) - ecstasy, ennui)


def turns_left(state: dict) -> float:
    """Turns the fight still has, from the enemies' HP and the pace so far (or a usual pace before it has shown one)."""
    battle = state.get("battle") or {}
    foes = [e for e in battle.get("enemies") or [] if int(e.get("max_hp") or 0) < 100_000]
    left = sum(int(e.get("hp") or 0) + int(e.get("block") or 0) for e in foes if int(e.get("hp") or 0) > 0)
    rounds = int(battle.get("round") or 1) - 1
    dealt = sum(max(0, int(e.get("max_hp") or 0) - int(e.get("hp") or 0)) for e in foes)
    pace = dealt / rounds if rounds >= 2 and dealt > 0 else PACE.get(str(state.get("state_type") or ""), 18.0)
    return max(1.0, min(float(MAX_TURNS), left / max(10.0, pace)))


def outlook(state: dict, draw_worth: tuple) -> tuple:
    """(Sadness held, Sadness a turn, extra draw a turn, turns left, a card's worth) when a card in the hand changes any of them, else ()."""
    player = state.get("player") or {}
    if not any((info := parse(c)).sadness or info.sad_turn for c in player.get("hand") or []):
        return ()
    free, paid_share, per_energy = draw_worth
    rate, extra = turn_rate(player)
    return (held(player), rate, extra, turns_left(state), max(2.0, free + paid_share * per_energy))


def _drawn(sad: int, rate: int, extra: int, turns: float) -> float:
    """Worth of what the turns to come draw, in cards: `sad` Sadness in the hand now and `rate` more each turn."""
    total, turn = 0.0, 1
    while turn - 1 < turns:
        cards = max(0, min(TURN_DRAW + extra, HAND_CAP - max(0, sad + rate * turn)))
        total += (min(cards, PLAYED) + SPARE * max(0, cards - PLAYED)) * min(1.0, turns - (turn - 1))
        turn += 1
    return total


def worth(view: tuple, added: int, rate_added: int, extra_added: int) -> float:
    """What a line's Sadness (and Ennui's, Ecstasy's) does to the turns to come, in planner points (negative: cards not drawn)."""
    sad, rate, extra, turns, card = view
    gain = _drawn(sad + added, rate + rate_added, extra + extra_added, turns) - _drawn(sad, rate, extra, turns)
    return gain * card * LATER
