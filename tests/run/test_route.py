"""The act 2 boss curve and what an Unknown room costs on the way (strategy.ACT2_ODDS, strategy.UNKNOWN_COST)."""

import unittest
from unittest import mock

from sts2bot import learn
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
            free, free_act1 = strategy.path_value(way, 70, 80, 0, 2), strategy.path_value(way, 70, 80, 0, 1)
        self.assertLess(strategy.path_value(way, 70, 80, 0, 2), free - 0.5)
        self.assertEqual(strategy.path_value(way, 70, 80, 0, 1), free_act1)  # (act 1: under 1%, left out)


class OwnCost(unittest.TestCase):
    """`own_cost`: the route planner prices fights by what this run's own have cost (strategy.Memory.toll)."""

    def state(self, row, hp, kinds=("Monster", "Monster")):
        node = lambda col, r, kind, *kids: {"col": col, "row": r, "type": kind, "children": [list(k) for k in kids]}
        nodes = [node(c, r, kinds[c] if r == row else "Monster", (c, r + 1)) for c in (0, 1) for r in range(1, 6)] + [node(0, 6, "Boss"), node(1, 6, "Boss")]
        opts = [{"index": c, "col": c, "row": row, "type": kinds[c]} for c in (0, 1)]
        return {"state_type": "map", "run": {"act": 2, "floor": 17 + row}, "player": {"hp": hp, "max_hp": 80, "gold": 0}, "map": {"nodes": nodes, "next_options": opts}}

    def test_the_fights_of_this_run_set_the_price(self):
        mem = strategy.Memory()
        with mock.patch.dict(learn.KNOB, {"own_cost": 1.0}):
            self.assertEqual(mem.toll(), 1.0)
            strategy.map_screen(self.state(1, 80), mem)
            strategy.map_screen(self.state(1, 80), mem)  # (the same screen looked at again: nothing walked yet)
            self.assertEqual(mem.fight_toll, [0.0, 0.0])
            strategy.map_screen(self.state(2, 50), mem)  # an act 2 hallway fight (10.2 HP on average) that cost 30
            self.assertEqual(mem.fight_toll[0], 30.0)
            self.assertAlmostEqual(mem.toll(), (30 + strategy.TOLL_PRIOR) / (12 * 0.85 + strategy.TOLL_PRIOR))
            mem.fight_toll = [400.0, 100.0]
            self.assertEqual(mem.toll(), strategy.TOLL_RANGE[1])
        self.assertEqual(mem.toll(), 1.0)  # (the setting off: the average, whatever the run has seen)
        mem.reset_run()
        self.assertEqual((mem.walked, mem.fight_toll), ((), [0.0, 0.0]))

    def test_a_boss_and_its_heal_are_not_counted(self):
        mem = strategy.Memory()
        with mock.patch.dict(learn.KNOB, {"own_cost": 1.0}):
            strategy.map_screen(self.state(5, 60), mem)
            nxt = self.state(1, 80)
            nxt["run"]["act"] = 3
            strategy.map_screen(nxt, mem)
            self.assertEqual(mem.fight_toll, [0.0, 0.0])

    def test_a_deck_that_fights_cheaply_takes_the_elite(self):
        way = [("Elite", 3), ("Monster", 4), ("Monster", 5), ("Boss", 6)]
        safe = [("Monster", 3), ("Monster", 4), ("Monster", 5), ("Boss", 6)]
        gain = lambda toll: strategy.path_value(way, 70, 80, 0, 2, 1.0, toll) - strategy.path_value(safe, 70, 80, 0, 2, 1.0, toll)
        self.assertGreater(gain(0.6), gain(1.0))
        self.assertGreater(gain(1.0), gain(1.6))


class SpoilsMap(unittest.TestCase):
    """The Spoils Map's 600 Gold lies at a node of the next act's map (the mod's `quests`): the route walks through it."""

    def state(self, quests):
        node = lambda col, row, kind, *kids: {"col": col, "row": row, "type": kind, "children": [list(k) for k in kids]}
        nodes = [node(0, 1, "Monster", (0, 2)), node(1, 1, "Monster", (1, 2)), node(0, 2, "Monster", (0, 3)), node(1, 2, "Monster", (0, 3)),
                 node(0, 3, "Shop", (0, 4)), node(0, 4, "Boss")]
        opts = [{"index": 0, "col": 0, "row": 1, "type": "Monster"}, {"index": 1, "col": 1, "row": 1, "type": "Monster"}]
        return {"state_type": "map", "run": {"act": 2, "floor": 18}, "player": {"hp": 70, "max_hp": 80, "gold": 50},
                "map": {"nodes": nodes, "next_options": opts, "quests": quests}}

    def test_the_route_goes_through_the_spoils(self):
        mem = strategy.Memory()
        first = lambda quests: strategy.map_screen(self.state(quests), mem)[0][1]["index"]
        self.assertEqual(first([{"kind": "spoils", "act": 2, "col": 1, "row": 2}]), 1)
        self.assertEqual(first([{"kind": "spoils", "act": 2, "col": 0, "row": 2}]), 0)
        self.assertEqual(first([{"kind": "spoils", "act": 3, "col": 1, "row": 2}]), 0)  # (another act's: nothing here; ties go first)
