import json
import tempfile
import unittest
from pathlib import Path

from sts2bot.fightrecord import FightWatch


def fight_state(foes, round_no, hp=70, kind="monster", act=2, floor=20, **player):
    enemies = [{"entity_id": f"{name}_{i}", "hp": 40, "block": 0} for i, name in enumerate(foes)]
    return {
        "state_type": kind,
        "run": {"act": act, "floor": floor, "ascension": 7},
        "battle": {"round": round_no, "turn": "player", "enemies": enemies},
        "player": {"hp": hp, "max_hp": 80, "character": "The Silent", **player},
    }


class FightWatchTest(unittest.TestCase):
    def test_a_fight_won_is_written_with_the_deck_that_fought_it(self):
        with tempfile.TemporaryDirectory() as d:
            watch = FightWatch(Path(d) / "fights.jsonl")
            first = fight_state(["MYTE", "MYTE"], 1, relics=[{"id": "RING_OF_THE_SNAKE"}], potions=[{"name": "Fire Potion"}],
                                hand=[{"name": "Strike"}, {"name": "Backflip+"}], draw_pile=[{"name": "Defend"}], discard_pile=[])
            self.assertIsNone(watch.update(first, "monster"))
            self.assertIsNone(watch.update({"state_type": "hand_select", "run": first["run"]}, "hand_select"))  # (a discard, mid-fight)
            self.assertIsNone(watch.update(fight_state(["MYTE"], 4, hp=52, potions=[]), "monster"))
            after = {"state_type": "rewards", "run": first["run"], "player": {"hp": 58, "max_hp": 80}}  # (6 healed as it ended)
            record = watch.update(after, "rewards")
            self.assertIsNone(watch.update(after, "rewards"))  # once
            (line,) = (Path(d) / "fights.jsonl").read_text().splitlines()
        self.assertEqual(json.loads(line), record)
        self.assertEqual((record["act"], record["floor"], record["kind"], record["enemies"], record["count"]), (2, 20, "monster", ["MYTE"], 2))
        self.assertEqual((record["hp_in"], record["hp_out"], record["max_hp"], record["rounds"], record["died"]), (70, 58, 80, 4, False))
        self.assertEqual((record["deck"], record["relics"], record["potions"], record["potions_left"]), (["Backflip+", "Defend", "Strike"], ["RING_OF_THE_SNAKE"], ["Fire Potion"], 0))
        self.assertTrue(record["whole"])
        self.assertEqual((record["character"], record["ascension"]), ("The Silent", 7))

    def test_a_fight_lost_and_a_fight_joined_half_way(self):
        watch = FightWatch()  # (nowhere to write: the record still comes back)
        watch.update(fight_state(["QUEEN"], 3, hp=30, kind="boss", act=3, floor=48, discard_pile=[{"name": "Strike"}]), "boss")
        record = watch.update({"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 0}}, "game_over")
        self.assertEqual((record["died"], record["hp_out"], record["kind"], record["whole"]), (True, 0, "boss", False))

    def test_one_fight_straight_into_the_next_is_two_records(self):
        watch = FightWatch()
        watch.update(fight_state(["NIBBIT"], 1, floor=5, act=1), "monster")
        first = watch.update(fight_state(["BYRDONIS"], 1, hp=61, kind="elite", floor=6, act=1), "elite")
        self.assertEqual((first["floor"], first["hp_out"], first["died"]), (5, 61, False))
        second = watch.update({"state_type": "map", "run": {"act": 1, "floor": 6}, "player": {"hp": 40}}, "map")
        self.assertEqual((second["floor"], second["kind"], second["hp_in"], second["hp_out"]), (6, "elite", 61, 40))

    def test_a_card_chosen_mid_fight_does_not_end_it(self):
        # The first game's Attack Potion opens a card reward inside the fight: every such boss fight logged a "won" after
        # round 1 and began again (2026-11-24).
        from sts2bot.fight.bossfight import BossWatch

        watch, bosses = FightWatch(), BossWatch()
        start = fight_state(["THEGUARDIAN"], 1, kind="boss", act=1, floor=16)
        pick = {"state_type": "card_reward", "run": start["run"], "card_reward": {"cards": [], "in_combat": True}}
        for w in (watch, bosses):
            w.update(start, "boss")
            self.assertIsNone(w.update(pick, "card_reward"))
            self.assertIsNone(w.update(fight_state(["THEGUARDIAN"], 2, hp=60, kind="boss", act=1, floor=16), "boss"))
        record = watch.update({"state_type": "rewards", "run": start["run"], "player": {"hp": 60}}, "rewards")
        self.assertEqual((record["rounds"], record["hp_out"]), (2, 60))
