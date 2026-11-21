"""The Insatiable's Sandpit is priced by the rounds the fight still needs (sts2bot/sandpit.py)."""

import unittest

from sts2bot import combat, sandpit

from .helpers import STRIKE, card, combat_state, enemy

ESCAPE = lambda i, cost=1: card(  # noqa: E731
    i, "FRANTIC_ESCAPE", "Frantic Escape", "Status", cost,
    "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.")
DECK = [{"name": "Strike"}] * 20 + [{"name": "Frantic Escape"}] * 4  # what is left to draw: an Escape in most hands


def insatiable(hp, pit, attack=0):
    boss = enemy("INS", hp, attack=attack, status=[{"id": "SANDPIT_POWER", "amount": pit}])
    boss["max_hp"] = 321
    return boss


def plays(state, turns=3):
    """The cards the planner plays this turn, by hand index."""
    seen = []
    for _ in range(turns):
        action = combat.choose_card(state)
        if not action or action[0] != "play_card":
            break
        index = action[1]["card_index"]
        seen.append(index)
        cost = next(c["cost"] for c in state["player"]["hand"] if c["index"] == index)
        state["player"]["hand"] = [c for c in state["player"]["hand"] if c["index"] != index]
        state["player"]["energy"] -= int(cost)
        if index < 2 and state["_escapes"] > index:  # an Escape played: the Sandpit moves
            state["battle"]["enemies"][0]["status"][0]["amount"] += 1
    return seen


def fight(hand, hp, pit, round_no, energy=3):
    state = combat_state(hand, [insatiable(hp, pit)], energy=energy, kind="boss")
    state["battle"]["round"] = round_no
    state["player"]["draw_pile"] = list(DECK)
    state["_escapes"] = sum(1 for c in hand if c["id"] == "FRANTIC_ESCAPE")
    return state


class Sandpit(unittest.TestCase):
    def test_margin_is_worth_nothing_past_the_end_of_the_fight(self):
        # The boss on 15 after this turn and dying at 35 a round: nothing is asked of a margin of 3.
        self.assertEqual(sandpit.price(3, 15, 300, 9), 0.0)
        self.assertGreater(sandpit.price(2, 15, 300, 9), 25.0)  # ... but 2 still needs that kill
        # The same margin with most of the fight to come is a risk, and less of one for every point more.
        early = [sandpit.price(left, 280, 40, 2) for left in (2, 3, 4, 5, 6)]
        self.assertEqual(early, sorted(early, reverse=True))
        self.assertGreater(early[2] - early[3], 25.0)  # 4 to 5 was worth 11 points, whatever the boss had left
        self.assertEqual(sandpit.price(1, 280, 40, 2), sandpit.DEATH * 10.0)

    def test_damage_that_shortens_the_fight_saves_an_escape(self):
        self.assertLess(sandpit.price(4, 250, 40, 2), sandpit.price(4, 280, 40, 2) - 5.0)

    def test_two_escapes_are_banked_while_the_fight_is_long(self):
        # Round 2, Sandpit 4, the boss on 280: every Escape played now is one the fight will ask for anyway.
        # (The second had been worth 5 to 11 points, less than a Strike: 3 fights in 15 were lost in the pit
        # on 18 to 38 HP, after hands that held two Escapes and played one.)
        state = fight([ESCAPE(0), ESCAPE(1), STRIKE(2), STRIKE(3), STRIKE(4)], 280, 4, 2)
        self.assertEqual(sorted(plays(state)[:2]), [0, 1])

    def test_no_escape_is_bought_for_a_fight_that_is_over(self):
        # Round 9, the boss on 30 of 321, Sandpit 3: two Strikes leave it on 18 and a margin that outlasts it.
        # An Escape for both energy (worth 39 points before, whatever the boss had left) is damage not dealt:
        # six fights were lost on our HP with the boss on 13 to 49 and 3 to 5 on the Sandpit.
        state = fight([ESCAPE(0, cost=2), STRIKE(2), STRIKE(3)], 30, 3, 9, energy=2)
        self.assertEqual(sorted(plays(state)), [2, 3])
        # ... but with the Sandpit on 1 it is the Escape or the pit, as before.
        state = fight([ESCAPE(0, cost=2), STRIKE(2), STRIKE(3)], 30, 1, 9, energy=2)
        self.assertEqual(plays(state), [0])


if __name__ == "__main__":
    unittest.main()
