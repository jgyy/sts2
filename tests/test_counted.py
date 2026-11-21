"""Cards whose numbers depend on the hand they are played from."""

import unittest

from sts2bot import combat
from sts2bot.cards import parse

from .fake_game import card
from .helpers import DEFEND, STRIKE, combat_state, enemy

FIRE = lambda i: card(i, "FIEND_FIRE", "Fiend Fire", "Attack", 2, "Exhaust your Hand. Deal 7 damage for each card Exhausted. Exhaust.", "AnyEnemy")
LETTING = lambda i: card(i, "BLOODLETTING", "Bloodletting+", "Skill", 0, "Lose 3 HP. Gain [ironclad_energy_icon.png][ironclad_energy_icon.png][ironclad_energy_icon.png].", "Self")


class FiendFire(unittest.TestCase):
    def test_it_hits_once_for_every_card_left_in_the_hand(self):
        s = combat_state([STRIKE(0), FIRE(1), DEFEND(2), STRIKE(3), DEFEND(4)], [enemy("BEAST_0", 200)], energy=3)
        foes, ctx = combat.read_enemies(s), combat._context(s)
        worth = lambda line: combat.simulate([parse(c) for c in line], foes, **ctx).score - combat.simulate([], foes, **ctx).score
        self.assertEqual(worth([FIRE(1)]), 7.0 * 4)
        self.assertEqual(worth([STRIKE(0), FIRE(1)]), 6.0 + 7.0 * 3)  # the Strike played first is not there to burn
        self.assertIsNone(combat.simulate([parse(FIRE(1)), parse(STRIKE(0))], foes, **ctx))  # nothing of this hand comes after it

    def test_with_one_play_allowed_it_is_the_play(self):
        # The fight it was found in (2026-11-21 23:14, Ceremonial Beast on Ascension 8): Ringing, the boss on 44 and
        # Vulnerable with 18 coming, four cards beside the Fiend Fire. The bot played a Defend, and next turn its
        # 42 damage left the boss on 2.
        beast = enemy("CEREMONIAL_BEAST_0", 44, attack=18, status=[{"id": "VULNERABLE_POWER", "amount": 1}, {"id": "STRENGTH_POWER", "amount": 3}])
        beast["max_hp"] = 262
        s = combat_state([STRIKE(0), LETTING(1), FIRE(2), DEFEND(3), STRIKE(4)], [beast], energy=3, hp=27, max_hp=87, kind="boss")
        s["battle"]["round"] = 8
        s["player"]["status"] = [{"id": "RINGING_POWER", "amount": 1}]
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 2)


    def test_it_does_not_burn_a_power_it_could_play_next_turn(self):
        form = card(1, "DEMON_FORM", "Demon Form", "Power", 3, "At the start of your turn, gain 2 Strength.", "Self")
        s = combat_state([FIRE(0), form, STRIKE(2), STRIKE(3), DEFEND(4)], [enemy("BEAST_0", 200, attack=10)], energy=2, kind="boss")
        self.assertNotEqual(combat.choose_card(s)[1]["card_index"], 0)  # 28 damage for a Demon Form: no
        s["player"]["hand"][1] = STRIKE(1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 0)  # with a Strike in its place: yes


class Flechettes(unittest.TestCase):
    def test_a_hit_for_each_skill_in_the_hand(self):
        darts = card(0, "FLECHETTES", "Flechettes", "Attack", 1, "Deal 5 damage for each Skill in your Hand.", "AnyEnemy")
        s = combat_state([darts, DEFEND(1), DEFEND(2), STRIKE(3)], [enemy("SLUG_0", 200)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 0)  # 10 damage, where a Strike is 6
        s = combat_state([darts, STRIKE(1), STRIKE(2)], [enemy("SLUG_0", 200)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)  # no Skill beside it: nothing


if __name__ == "__main__":
    unittest.main()
