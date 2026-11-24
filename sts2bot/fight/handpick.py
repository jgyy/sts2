"""Which card to give up when a card or a relic asks for one from the hand, judged by the turn's plan (not deck ratings)."""

from __future__ import annotations

import json
import re

from . import combat

FIGHTS = ("monster", "elite", "boss")
RUINOUS = 500.0  # a plan this much worse has lost a kill or our life, not a few points of damage
UNPLANNED = 1e6  # the cost given a card that was not planned for: after the one found that costs nothing
ANSWERS_KEPT = 4  # selections whose costs are remembered (the same one is asked again after each click)
_answers: dict[str, dict[int, float]] = {}


def turn_costs(state: dict, cards: list[dict], tolerance: float = 1e-9) -> dict[int, float] | None:
    """For each card on offer (by its index): how much worse this turn's best plan is without it (None: no fight). Cards are planned
    for in the order given, until a plain one costs no more than `tolerance`: the rest come back as `UNPLANNED`."""
    battle, player = state.get("battle") or {}, state.get("player") or {}
    if len(cards) < 2 or not any(int(e.get("hp") or 0) > 0 for e in battle.get("enemies") or []):
        return None
    hand = player.get("hand") or []

    def whole(c: dict) -> dict:
        """The card as the planner needs it: the selection screen leaves out who it is aimed at."""
        twin = next((h for h in hand if h.get("name") == c.get("name") and h.get("description") == c.get("description")), None)
        target = (twin or {}).get("target_type") or ("AnyEnemy" if str(c.get("type")) == "Attack" else "Self")
        # (Not the game's "can play": during a selection nothing can be played.)
        return {**c, "target_type": target, "can_play": not re.search(r"\bunplayable\b", str(c.get("description") or ""), re.I)}

    pool = [whole(c) for c in cards]
    kind = state.get("_fight") if state.get("_fight") in FIGHTS else "monster"
    # The same question comes again after each click, and a plan per card on offer is dear: answers are remembered.
    key = json.dumps([{k: v for k, v in state.items() if k != "hand_select"}, [(c.get("index"), c) for c in cards], tolerance], sort_keys=True, default=str)
    if key in _answers:
        return _answers[key]
    # Toasty Mittens ("Exhaust 1 card from your Hand and gain 1 Strength") gives after it asks: the texts on show lack it.
    gift = sum(int(m.group(1)) for r in player.get("relics") or [] if (m := re.search(r"exhaust 1 card from your hand and gain (\d+) strength", str(r.get("description") or ""), re.I)))

    def plan_without(skip: int | None) -> float | None:
        rest = [{**c, "index": n} for n, c in enumerate(c for i, c in enumerate(pool) if i != skip)]
        plan: dict = {}
        combat.choose_card({**state, "state_type": kind, "_might": gift, "player": {**player, "hand": rest}}, plan)
        return plan.get("score")

    whole_hand = plan_without(None)
    if whole_hand is None:
        return None
    # Plain cards (only their own play counts) are tried in order until one the plan does not miss: later ones could only tie.
    plain = [_plain(c) for c in pool]
    order = [i for i in range(len(pool)) if not plain[i]] + [i for i in range(len(pool)) if plain[i]]
    scores: dict[int, float] = {}
    planned: dict[str, float | None] = {}
    for skip in order:
        # Copies of a card leave the same hand behind: each kind is planned once.
        same = json.dumps({k: v for k, v in pool[skip].items() if k != "index"}, sort_keys=True, default=str)
        if same not in planned:
            planned[same] = plan_without(skip)
        if planned[same] is None:
            return None
        scores[skip] = planned[same]
        if plain[skip] and whole_hand - scores[skip] <= tolerance:
            break
    best = max(scores.values())
    costs = {int(c.get("index", i)): (best - scores[i]) if i in scores else UNPLANNED for i, c in enumerate(cards)}
    _answers[key] = costs
    while len(_answers) > ANSWERS_KEPT:
        del _answers[next(iter(_answers))]
    return costs


_HAND_BOUND = re.compile(r"ethereal|hexed|retain|unplayable|innate|\bhands?\b|\beach\b|\bevery\b|\bburn|lose \d+ hp|\bcurse|\bstatus|\bdeck\b|\bpile", re.I)
_INERT = re.compile(r"unplayable|ethereal|eternal|innate|exhaust|[\W_]+", re.I)


def _plain(c: dict) -> bool:
    """A card the rest of the hand cannot feel: an Attack, Skill or Power whose text says nothing of the hand,
    or a Curse or Status that does nothing but sit there (Ascender's Bane: "Unplayable. Ethereal.")."""
    text = str(c.get("description") or "")
    if str(c.get("type")) in ("Curse", "Status") and not _INERT.sub("", text):
        return True
    words = " ".join([text] + [str(k.get("name")) for k in c.get("keywords") or [] if isinstance(k, dict)])
    return str(c.get("type")) in ("Attack", "Skill", "Power") and not _HAND_BOUND.search(words)
