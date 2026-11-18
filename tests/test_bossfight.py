import unittest

from sts2bot.bossfight import BossWatch


def boss_state(foes, round_no, player_hp=70, max_hp=80, act=1, floor=17):
    enemies = [{"entity_id": f"{name}_{i}", "hp": hp, "block": 0} for i, (name, hp) in enumerate(foes)]
    return {
        "state_type": "boss",
        "run": {"act": act, "floor": floor},
        "battle": {"round": round_no, "turn": "player", "enemies": enemies},
        "player": {"hp": player_hp, "max_hp": max_hp},
    }


class BossWatchTest(unittest.TestCase):
    def test_won_fight_reports_boss_and_player_hp(self):
        watch = BossWatch()
        self.assertIsNone(watch.update(boss_state([("VANTOM", 170)], 1), "boss"))
        self.assertIsNone(watch.update(boss_state([("VANTOM", 60)], 2, player_hp=50), "boss"))
        line = watch.update({"state_type": "rewards", "run": {"act": 1, "floor": 17}}, "rewards")
        self.assertEqual(
            line,
            "boss fight over: act 1 floor 17 VANTOM won; boss hp 170 -> 60; player hp 70 -> 50/80; rounds 2",
        )

    def test_lost_fight_is_reported_once(self):
        watch = BossWatch()
        watch.update(boss_state([("SOUL_FYSH", 200)], 1, player_hp=60), "boss")
        watch.update(boss_state([("SOUL_FYSH", 150)], 3, player_hp=5), "boss")
        line = watch.update({"state_type": "game_over", "run": {"act": 1, "floor": 17}}, "game_over")
        self.assertIn("SOUL_FYSH lost; boss hp 200 -> 150; player hp 60 -> 5/80; rounds 2", line)
        self.assertIsNone(watch.update({"state_type": "game_over", "run": {"act": 1, "floor": 17}}, "game_over"))

    def test_several_enemies_are_named_once_each(self):
        watch = BossWatch()
        watch.update(boss_state([("KIN_FOLLOWER", 90), ("KIN_PRIEST", 80)], 1), "boss")
        line = watch.update({"state_type": "map", "run": {"act": 1, "floor": 18}}, "map")
        self.assertIn("act 1 floor 17 KIN_FOLLOWER+KIN_PRIEST won", line)

    def test_ordinary_fights_are_ignored(self):
        watch = BossWatch()
        normal = boss_state([("SLIME", 20)], 1, floor=3)
        normal["state_type"] = "monster"
        self.assertIsNone(watch.update(normal, "monster"))
        self.assertIsNone(watch.update({"state_type": "map", "run": {}}, "map"))

    def test_boss_state_with_no_living_enemies_is_not_a_fight(self):
        watch = BossWatch()
        self.assertIsNone(watch.update(boss_state([("VANTOM", 0)], 1), "boss"))
        self.assertIsNone(watch.update({"state_type": "map", "run": {}}, "map"))


if __name__ == "__main__":
    unittest.main()
