"""Tests of the turn planner and what it reads from cards and enemies; the builders below are shared with tests/bosses (a module of their own would be an eleventh file here)."""

import re

from sts2bot.cards import parse
from sts2bot.fight import combat

from .. import helpers


def combat_state(hand, enemies, *, round_no=1, floor=None, **kw):
    """helpers.combat_state in one call: also the round, the floor, "_" keys on the state and any other keyword on the player."""
    s = helpers.combat_state(hand, enemies, **{k: kw.pop(k) for k in ("energy", "hp", "max_hp", "block", "kind", "potions") if k in kw})
    s["battle"]["round"] = round_no
    if floor is not None:
        s["run"]["floor"] = floor
    for k, v in kw.items():
        (s if k.startswith("_") else s["player"])[k] = v
    return s


def power(name, amount=1):
    """A status entry as the game writes it."""
    return {"id": f"{name}_POWER", "amount": amount}


def potion(slot, name, text, target="Self"):
    return {"slot": slot, "id": name.upper().replace(" ", "_"), "name": name, "description": text, "target_type": target, "can_use_in_combat": True}


def read_foes(*enemies):
    """helpers.enemy dicts as the planner reads them."""
    return combat.read_enemies(combat_state([], list(enemies)))


def ctx(energy=3, hp=60, max_hp=80, block=0, **kw):
    """combat.simulate's keywords for a turn with no Stars, and whatever else is given."""
    return dict(energy=energy, stars=0, hp=hp, max_hp=max_hp, block=block, **kw)


def gain(line, foes, **kw):
    """What a line of parsed cards scores over playing nothing."""
    return combat.simulate(line, foes, **kw).score - combat.simulate([], foes, **kw).score


def outcome(state, cards=(), **kw):
    """combat.simulate of these card dicts in this state, set up as choose_card sets it up (keywords on top)."""
    return combat.simulate([parse(c) for c in cards], combat.read_enemies(state), **{**combat._context(state), **kw})


def worth(state, cards, **kw):
    return outcome(state, cards, **kw).score - outcome(state, **kw).score


def pick(state, plan=None):
    """The hand index of the card the planner plays first."""
    return combat.choose_card(state, plan)[1]["card_index"]


def aimed(state):
    return combat.choose_card(state)[1].get("target")


def play_turn(state, limit=3, block=False, after=None, choose=None):
    """Play the planner's cards as the game would, at most `limit`: each leaves the hand and costs its energy, block=True adds its "Gain N Block", after(card, params) does the rest (true: stop); returns the cards played."""
    played = []
    while len(played) < limit and (act := (choose or combat.choose_card)(state)) and act[0] == "play_card":
        c = next(h for h in state["player"]["hand"] if h["index"] == act[1]["card_index"])
        played.append(c)
        state["player"]["hand"] = [h for h in state["player"]["hand"] if h is not c]
        state["player"]["energy"] -= int(c["cost"])
        state["player"]["block"] += int(m[1]) if block and (m := re.search(r"Gain (\d+) Block", c["description"])) else 0
        if after and after(c, act[1]):
            break
    return played
