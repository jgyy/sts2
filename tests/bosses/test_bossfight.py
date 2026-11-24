import unittest

from sts2bot.fight.bossfight import BossWatch


def boss_state(foes, round_no, player_hp=70, max_hp=80, act=1, floor=17):
    enemies = [{"entity_id": f"{name}_{i}", "hp": hp, "block": 0} for i, (name, hp) in enumerate(foes)]
    return {"state_type": "boss", "run": {"act": act, "floor": floor}, "battle": {"round": round_no, "turn": "player", "enemies": enemies}, "player": {"hp": player_hp, "max_hp": max_hp}}


def shown(watch, kind, floor=17):
    """What the watch says when the game moves on to a screen of this kind."""
    return watch.update({"state_type": kind, "run": {"act": 1, "floor": floor}}, kind)


class BossWatchTest(unittest.TestCase):
    def test_won_fight_reports_boss_and_player_hp(self):
        watch = BossWatch()
        self.assertIsNone(watch.update(boss_state([("VANTOM", 170)], 1), "boss"))
        self.assertIsNone(watch.update(boss_state([("VANTOM", 60)], 2, player_hp=50), "boss"))
        self.assertEqual(shown(watch, "rewards"), "boss fight over: act 1 floor 17 VANTOM won; boss hp 170 -> 60; player hp 70 -> 50/80; rounds 2; brought relics 0, potions 0, deck 0")

    def test_what_the_bot_brings_is_written_down_as_the_fight_begins(self):
        watch, state = BossWatch(), boss_state([("VANTOM", 170)], 1)
        state["player"].update(relics=[{"id": "BURNING_BLOOD", "name": "Burning Blood"}], potions=[{"name": "Fire Potion"}], hand=[{"name": "Strike"}], draw_pile=[{"name": "Bash+"}], discard_pile=[])
        watch.update(state, "boss")
        self.assertEqual(watch.brought, "boss fight begins: act 1 floor 17 VANTOM; relics BURNING_BLOOD; potions Fire Potion; cards Bash+, Strike")

    def test_lost_fight_is_reported_once(self):
        watch = BossWatch()
        watch.update(boss_state([("SOUL_FYSH", 200)], 1, player_hp=60), "boss")
        watch.update(boss_state([("SOUL_FYSH", 150)], 3, player_hp=5), "boss")
        self.assertIn("SOUL_FYSH lost; boss hp 200 -> 150; player hp 60 -> 5/80; rounds 2", shown(watch, "game_over"))
        self.assertIsNone(shown(watch, "game_over"))

    def test_a_card_picked_mid_fight_does_not_end_it(self):
        # A Survivor's discard opens a hand-select screen in the middle of the fight.
        watch = BossWatch()
        watch.update(boss_state([("WATERFALL_GIANT", 240)], 5), "boss")
        for mid in ("hand_select", "card_select", "bundle_select"):
            self.assertIsNone(shown(watch, mid))
        watch.update(boss_state([("WATERFALL_GIANT", 87)], 5), "boss")
        self.assertIn("won; boss hp 240 -> 87", shown(watch, "rewards"))
        self.assertIsNone(shown(watch, "map", floor=18))

    def test_several_enemies_are_named_once_each(self):
        watch = BossWatch()
        watch.update(boss_state([("KIN_FOLLOWER", 90), ("KIN_PRIEST", 80)], 1), "boss")
        self.assertIn("act 1 floor 17 KIN_FOLLOWER+KIN_PRIEST won", shown(watch, "map", floor=18))

    def test_what_is_not_a_boss_fight_is_ignored(self):
        for why, state in (("an ordinary fight", {**boss_state([("SLIME", 20)], 1, floor=3), "state_type": "monster"}), ("a boss state with no living enemies", boss_state([("VANTOM", 0)], 1))):
            with self.subTest(why):
                watch = BossWatch()
                self.assertIsNone(watch.update(state, state["state_type"]))
                self.assertIsNone(watch.update({"state_type": "map", "run": {}}, "map"))
