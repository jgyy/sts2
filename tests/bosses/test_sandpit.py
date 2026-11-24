"""The Insatiable's Sandpit is priced by the rounds the fight still needs (sts2bot/fight/sandpit.py)."""

import unittest

from sts2bot.fight import sandpit

from ..fight import combat_state, play_turn, power
from ..helpers import STRIKE, card, enemy

ESCAPE = lambda i, cost=1: card(i, "FRANTIC_ESCAPE", "Frantic Escape", "Status", cost, "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.")
DECK = [{"name": "Strike"}] * 20 + [{"name": "Frantic Escape"}] * 4  # what is left to draw: an Escape in most hands


def plays(hand, hp, pit, round_no, energy=3):
    """The hand indexes of the cards the planner plays this turn against the Insatiable, the Sandpit moving with each Escape."""
    boss = {**enemy("INS", hp, attack=0, status=[power("SANDPIT", pit)]), "max_hp": 321}
    escapes = sum(1 for c in hand if c["id"] == "FRANTIC_ESCAPE")
    def escaped(c, _):
        if c["index"] < 2 and escapes > c["index"]:
            boss["status"][0]["amount"] += 1
    return [c["index"] for c in play_turn(combat_state(hand, [boss], energy=energy, kind="boss", round_no=round_no, draw_pile=list(DECK)), after=escaped)]


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
        # Round 2, Sandpit 4, the boss on 280: every Escape played now is one the fight will ask for anyway. (The second had been worth less than a Strike: 3 fights in 15 were lost in the pit after hands that held two Escapes and played one.)
        self.assertEqual(sorted(plays([ESCAPE(0), ESCAPE(1), STRIKE(2), STRIKE(3), STRIKE(4)], 280, 4, 2)[:2]), [0, 1])

    def test_no_escape_is_bought_for_a_fight_that_is_over(self):
        # Round 9, the boss on 30 of 321, Sandpit 3: two Strikes leave it on 18 and a margin that outlasts it. An Escape for both energy is damage not dealt: six fights were lost on our HP with the boss on 13 to 49 and 3 to 5 on the Sandpit.
        self.assertEqual(sorted(plays([ESCAPE(0, cost=2), STRIKE(2), STRIKE(3)], 30, 3, 9, energy=2)), [2, 3])
        self.assertEqual(plays([ESCAPE(0, cost=2), STRIKE(2), STRIKE(3)], 30, 1, 9, energy=2), [0])  # ... with the Sandpit on 1, the Escape or the pit
