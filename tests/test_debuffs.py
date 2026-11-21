"""What a turn of Weak or Vulnerable that outlasts this one counts for (the `debuff_worth` setting)."""

import unittest

from sts2bot import combat, learn

from .helpers import STRIKE, card, combat_state, enemy


class DebuffWorth(unittest.TestCase):
    def choice(self, hand, worth, **kw):
        saved, learn.KNOB["debuff_worth"] = learn.KNOB.get("debuff_worth", 1.0), worth
        try:
            return combat.choose_card(combat_state(hand, [enemy("BOSS", 300)], energy=1, kind="boss", **kw))[1]["card_index"]
        finally:
            learn.KNOB["debuff_worth"] = saved

    def test_two_turns_of_weak_on_a_boss_that_is_not_attacking(self):
        # One energy, the boss buffing: a Strike is 9 points, two turns of Weak to come were 6.5.
        weak = card(0, "INTIMIDATE", "Intimidate", "Skill", 1, "Apply 2 Weak.", "AnyEnemy")
        self.assertEqual(self.choice([weak, STRIKE(1)], 1.0), 1)
        self.assertEqual(self.choice([weak, STRIKE(1)], 2.0), 0)  # at 13 they go first

    def test_a_single_turn_of_vulnerable_is_not_made_dearer_than_the_hit_it_would_help(self):
        # One Vulnerable is gone when the boss has moved: with the last energy it helps nothing, at either value.
        once = card(0, "TRIP", "Trip", "Skill", 1, "Apply 1 Vulnerable.", "AnyEnemy")
        self.assertEqual(self.choice([once, STRIKE(1)], 1.0), 1)
        self.assertEqual(self.choice([once, STRIKE(1)], 2.0), 1)


if __name__ == "__main__":
    unittest.main()
