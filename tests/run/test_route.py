"""The act 2 boss curve and what an Unknown room costs on the way (strategy.ACT2_ODDS, strategy.UNKNOWN_COST)."""

import unittest
from unittest import mock

from sts2bot.run import strategy


class Act2Boss(unittest.TestCase):
    def test_hp_above_85_percent_counts_at_the_act_2_boss(self):
        # Live (A5+, 85-97% HP won 0.88 as often as 97%+) and the Knowledge Demon solved exactly (90% HP: 0.82).
        self.assertLess(strategy.boss_odds(0.9, 2), 0.9)
        self.assertLess(strategy.boss_odds(0.7, 2), 0.5)
        self.assertEqual(strategy.boss_odds(1.0, 2), 1.0)
        self.assertEqual(strategy.boss_odds(0.0, 2), 0.0)
        self.assertTrue(all(strategy.boss_odds(x / 20, 2) <= strategy.boss_odds((x + 1) / 20, 2) for x in range(20)))

    def test_act_1_keeps_its_flat_top(self):
        self.assertEqual(strategy.boss_odds(0.9, 1), 1.0)
        self.assertEqual(strategy.boss_odds(0.85, 1), 1.0)

    def test_an_unknown_room_in_act_2_costs_hp_on_paper(self):
        # 375 act 2 Unknown rooms: an event 65% of the time, a fight 20% at 17% of Max HP; 3% on average.
        way = [("Unknown", 13), ("Boss", 14)]
        with mock.patch.object(strategy, "UNKNOWN_COST", {}):
            free = strategy.path_value(way, 70, 80, 0, 2)
            free_act1 = strategy.path_value(way, 70, 80, 0, 1)
        self.assertLess(strategy.path_value(way, 70, 80, 0, 2), free - 0.5)
        self.assertEqual(strategy.path_value(way, 70, 80, 0, 1), free_act1)  # (act 1: under 1%, left out)


if __name__ == "__main__":
    unittest.main()
