"""What a turn of Weak or Vulnerable that outlasts this one counts for (the `debuff_worth` setting)."""

import unittest
from unittest import mock

from sts2bot import learn

from ..helpers import STRIKE, card, enemy
from . import combat_state, pick


class DebuffWorth(unittest.TestCase):
    def test_a_debuff_goes_first_only_when_its_later_turns_outweigh_a_strike(self):
        # One energy against a boss that is buffing: (why, the debuff, debuff_worth, the card played: 0 the debuff, 1 the Strike).
        weak = card(0, "INTIMIDATE", "Intimidate", "Skill", 1, "Apply 2 Weak.", "AnyEnemy")
        once = card(0, "TRIP", "Trip", "Skill", 1, "Apply 1 Vulnerable.", "AnyEnemy")
        for why, debuff, worth, first in (("two turns of Weak to come are 6.5 points, a Strike 9", weak, 1.0, 1), ("... at 13 they go first", weak, 2.0, 0),
                                          ("one Vulnerable is gone when the boss has moved: it helps nothing", once, 1.0, 1), ("... at either value", once, 2.0, 1)):
            with self.subTest(why), mock.patch.dict(learn.KNOB, {"debuff_worth": worth}):
                self.assertEqual(pick(combat_state([debuff, STRIKE(1)], [enemy("BOSS", 300)], energy=1, kind="boss")), first)


class PricesOfAPlay(unittest.TestCase):
    """What a card gives away for good (until 2026-11-26 the planner saw neither): Strength to the enemy, dead cards in our piles."""

    def test_strength_given_to_the_enemy_costs_every_hit_it_has_left(self):
        hand = [card(0, "FIGHT_ME", "Fight Me!", "Attack", 1, "Deal 9 damage. The enemy gains 1 Strength.", "AnyEnemy"), STRIKE(1)]
        boss = enemy("BOSS", 300, attack="5x3")
        self.assertEqual(pick(combat_state(hand, [boss], energy=1, kind="boss")), 1)  # (a point of damage is not three hits a turn)

    def test_a_status_put_in_our_piles_costs_a_draw_every_shuffle(self):
        # A Wound in a deck of 25 over a boss fight: about 8 points (battle.junk_cost), a Strike's 6 damage 9.
        wild = lambda dmg: card(0, "WILD_STRIKE", "Wild Strike", "Attack", 1, f"Deal {dmg} damage. Shuffle a Wound into your draw pile.", "AnyEnemy")
        played = lambda dmg: pick(combat_state([wild(dmg), STRIKE(1)], [enemy("BOSS", 300)], energy=1, kind="boss", draw_pile_count=23))
        self.assertEqual((played(7), played(14)), (1, 0))  # (a point more is not worth a Wound; eight more are)
