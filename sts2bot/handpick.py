"""Which card to give up when a card or a relic asks for one from the hand, judged by the turn's plan.

The choice used to go by the cards' deck ratings alone: Dagger Throw's
discard took a Strike from a Silent on 9 HP that had the Insatiable two
Strikes from dead (36 Poison on 72 HP), and the two cards it kept instead
could not finish it.
"""

from __future__ import annotations

import re

from . import combat

FIGHTS = ("monster", "elite", "boss")
RUINOUS = 500.0  # a plan this much worse has lost a kill or our life, not a few points of damage


def turn_costs(state: dict, cards: list[dict]) -> dict[int, float] | None:
    """For each card on offer (by its index): how much worse this turn's best plan is without it.

    Negative for a card the hand is better off without (one that burns at the
    end of the turn). None when there is no fight to plan for.
    """
    battle, player = state.get("battle") or {}, state.get("player") or {}
    if len(cards) < 2 or not any(int(e.get("hp") or 0) > 0 for e in battle.get("enemies") or []):
        return None
    hand = player.get("hand") or []

    def whole(c: dict) -> dict:
        """The card as the planner needs it: the selection screen leaves out who it is aimed at."""
        twin = next((h for h in hand if h.get("name") == c.get("name") and h.get("description") == c.get("description")), None)
        target = (twin or {}).get("target_type") or ("AnyEnemy" if str(c.get("type")) == "Attack" else "Self")
        # (Whether it "can be played" is not asked of the game: during a selection nothing can.)
        return {**c, "target_type": target, "can_play": not re.search(r"\bunplayable\b", str(c.get("description") or ""), re.I)}

    pool = [whole(c) for c in cards]
    kind = state.get("_fight") if state.get("_fight") in FIGHTS else "monster"
    # Toasty Mittens ("Exhaust 1 card from your Hand and gain 1 Strength") asks before it gives: the
    # texts on show are a point short on every attack, and one kill was missed by exactly that.
    gift = sum(int(m.group(1)) for r in player.get("relics") or [] if (m := re.search(r"exhaust 1 card from your hand and gain (\d+) strength", str(r.get("description") or ""), re.I)))
    scores: list[float] = []
    for skip in range(len(pool)):
        rest = [{**c, "index": n} for n, c in enumerate(c for i, c in enumerate(pool) if i != skip)]
        plan: dict = {}
        combat.choose_card({**state, "state_type": kind, "_might": gift, "player": {**player, "hand": rest}}, plan)
        if "score" not in plan:
            return None
        scores.append(plan["score"])
    best = max(scores)
    return {int(c.get("index", i)): best - scores[i] for i, c in enumerate(cards)}
