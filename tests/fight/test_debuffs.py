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
