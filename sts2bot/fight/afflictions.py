"""The Knowledge Demon's offers: which affliction to carry for the rest of the fight, a limit (Mind Rot, Sloth, Waste Away) or a burn
(Disintegration). Which costs more depends on the deck: priced by the cards played each turn so far, and the energy left unspent."""

from __future__ import annotations

import logging
import re

from . import combat, triggers
from .battle import power_amount
from ..cards import _card_key, parse

log = logging.getLogger("sts2bot.afflictions")

CARD_LOST = 7.5  # HP a turn that one card a turn we may not play comes to (the fight is that much longer)
SLOTH_MARGIN = 2.0  # cards a turn Sloth has to cost before the burn is the cheaper price (perfect play: scripts/demon/offers.py)
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
    # (The choice screen comes without the enemies: no rule here can ask for the Demon's HP.)
    player = state.get("player") or {}
    hp = int(player.get("hp") or 0)
    turns = list(state.get("_turns") or [])
    m = re.search(r"take (\d+) damage", str(burn.get("description") or ""), re.I)
    per_turn = int(m.group(1)) if m else 8
    burning = max(0, power_amount(player, "DISINTEGRATION"))  # a burn taken earlier: the new one comes on top
    first = lambda c: [c] + [o for o in ordered if o is not c]
    if sloth is not None:
        # The burn only for a deck that has been playing well over three cards a turn, with the HP to pay for it.
        lost = lost_to_sloth(turns)
        take_burn = lost >= SLOTH_MARGIN and CARD_LOST * lost > per_turn and hp > 5 * per_turn and not burning
        log.debug("affliction: cards a turn %s, Sloth would stop %.2f a turn: %s", [n for n, _ in turns], lost, "the burn" if take_burn else "Sloth")
        return first(burn if take_burn else sloth)
    # An energy less against the burn: a deck that spends all its energy takes the burn, unless it is a second one or would finish us in two turns.
    idle = idle_share(turns)
    spare = burning > 0 or (idle is not None and idle >= IDLE_ENOUGH)
    take_burn = hp > 2 * (per_turn + burning) and not spare
    log.debug("affliction: energy left at the end of each turn %s: %s", [left for _, left in turns], "the burn" if take_burn else "Waste Away")
    return first(burn if take_burn else drain)


# Cards a fight offers ("Choose 1 of 3 ... It's free to play this turn."): ranked by what each does for this turn's plan.
def _aimed(card: dict) -> str:
    """The target a card offered on this screen would have in the hand (the screen does not say)."""
    text = str(card.get("description") or "")
    if re.search(r"\ball\s+enemies\b", text, re.I):
        return "AllEnemies"
    if re.search(r"random\s+enemy", text, re.I):
        return "RandomEnemy"
    return "AnyEnemy" if str(card.get("type")) == "Attack" or re.search(r"\bapply\b|\benemy\b", text, re.I) else "Self"


_OFFER: list = [None, 0]  # the last offer screen ranked, and the card put first
OFFER_MARGIN = 5.0  # planner points by which an offered card has to beat the one the ratings put first


def _read(card: dict) -> bool:
    """A card offered here that the planner can read."""
    info = parse({**card, "target_type": _aimed(card)})
    return info.known or info.type == "Power" or info.key in triggers.READ


def _iffy(card: dict) -> bool:
    """A card that may not be playable at all this turn ("Can only be played if ...")."""
    return bool(re.search(r"can\s+only\s+be\s+played", str(card.get("description") or ""), re.I))


def offered(ordered: list[dict], fight: dict | None) -> list[dict]:
    """`ordered` with the card this turn makes most of first, by the planner's best line with it free in the hand (`fight`: the state before)."""
    if not fight or len(ordered) < 2 or any(str(c.get("type")) not in ("Attack", "Skill", "Power") for c in ordered):
        return ordered
    player = fight.get("player") or {}
    hand = list(player.get("hand") or [])
    seen = (tuple(str(c.get("name")) for c in ordered), tuple(str(c.get("name")) for c in hand), player.get("energy"), (fight.get("battle") or {}).get("round"))
    if _OFFER[0] == seen:  # (the screen is asked about on every poll while it is open)
        return [ordered[_OFFER[1]]] + [c for n, c in enumerate(ordered) if n != _OFFER[1]]
    scores = []
    for c in ordered:
        plan: dict = {}
        free = {**c, "index": len(hand), "cost": "0", "target_type": _aimed(c), "can_play": True, "unplayable_reason": None}
        combat.choose_card({**fight, "player": {**player, "hand": [*hand, free]}}, plan)
        scores.append(float(plan.get("score", float("-inf"))))
    # Only a card the planner can read and surely play, by a clear margin: the rating behind the order also counts later draws of it.
    fit = [n for n, c in enumerate(ordered) if _read(c) and not _iffy(c)]
    top = float("-inf") if _iffy(ordered[0]) else scores[0] if _read(ordered[0]) else float("inf")  # (nothing to set an unread card against)
    best = max(fit, key=lambda n: (scores[n], -n), default=0)
    best = best if scores[best] >= top + OFFER_MARGIN else 0
    _OFFER[:] = [seen, best]
    log.debug("offer: %s", ", ".join(f"{c.get('name')} {s:.1f}" for c, s in zip(ordered, scores)))
    return [ordered[best]] + [c for n, c in enumerate(ordered) if n != best]
