import json
import tempfile
import unittest
from pathlib import Path

from sts2bot import events, relics, strategy
from sts2bot.strategy import Memory


def run(floor, held, result="loss", who="The Regent", ascension=7):
    return {"character": who, "ascension": ascension, "floor": floor, "result": result, "relics": list(held)}


class WhatTheAncientsRelicsDid(unittest.TestCase):
    def setUp(self):
        relics.load(None, events._ANCIENT)

    def tearDown(self):
        relics.load(None, events._ANCIENT)  # (the record is the module's: leave none behind for the other tests)

    def test_with_no_runs_on_record_the_table_stands(self):
        self.assertEqual(relics.shift("SEAL_OF_GOLD"), 0.0)
        self.assertEqual(events._event_option_value({"title": "Seal of Gold"}, 60, 75), events._ANCIENT["SEAL_OF_GOLD"])

    def test_a_relic_whose_runs_go_further_is_rated_up_and_the_other_down(self):
        for _ in range(30):
            relics.note(run(40, ["STONE_HUMIDIFIER"]), rebuild=False)
            relics.note(run(20, ["BOOMING_CONCH"]), rebuild=False)
        relics.note(run(30, ["FISHING_ROD"]))
        self.assertGreater(relics.shift("STONE_HUMIDIFIER"), 2.0)
        self.assertLess(relics.shift("BOOMING_CONCH"), -2.0)
        self.assertAlmostEqual(relics.shift("FISHING_ROD"), 0.0, delta=0.1)
        # ... and never by more than MAX_SHIFT, whatever the runs did
        self.assertLessEqual(relics.shift("STONE_HUMIDIFIER"), relics.MAX_SHIFT)
        self.assertGreaterEqual(relics.shift("BOOMING_CONCH"), -relics.MAX_SHIFT)
        value = events._event_option_value({"title": "Stone Humidifier"}, 60, 75)
        self.assertAlmostEqual(value, events._ANCIENT["STONE_HUMIDIFIER"] + relics.shift("STONE_HUMIDIFIER"))

    def test_a_few_runs_move_a_rating_little(self):
        for _ in range(20):
            relics.note(run(30, ["FISHING_ROD"]), rebuild=False)
        relics.note(run(48, ["LAVA_ROCK"], "win"))  # one run, 31 floors over the usual with the win
        self.assertLess(relics.shift("LAVA_ROCK"), 1.5)

    def test_a_relic_is_judged_from_the_floor_it_is_taken_on(self):
        # Every run that holds an act 3 relic has reached act 3: against all runs it would look 15 floors good.
        for _ in range(20):
            relics.note(run(25, ["FISHING_ROD"]), rebuild=False)
            relics.note(run(45, ["FISHING_ROD", "PAELS_BLOOD", "SAI"]), rebuild=False)
            relics.note(run(45, ["FISHING_ROD", "PAELS_BLOOD", "FUR_COAT"]), rebuild=False)
        relics.note(run(45, ["FISHING_ROD", "PAELS_BLOOD", "SAI"]))
        self.assertAlmostEqual(relics.shift("SAI"), 0.0, delta=0.1)
        self.assertAlmostEqual(relics.shift("FUR_COAT"), 0.0, delta=0.1)

    def test_darv_is_met_in_act_two_or_three(self):
        self.assertEqual(relics.picked_at("FISHING_ROD", {"FISHING_ROD"}), 1)
        self.assertEqual(relics.picked_at("PAELS_BLOOD", {"PAELS_BLOOD", "SOZU"}), 18)
        self.assertEqual(relics.picked_at("SOZU", {"FISHING_ROD", "SOZU", "SAI"}), 18)
        self.assertEqual(relics.picked_at("SOZU", {"FISHING_ROD", "PAELS_BLOOD", "SOZU"}), 34)  # Pael opened act 2, so Darv act 3
        self.assertEqual(relics.picked_at("SAI", {"SAI"}), 34)

    def test_the_usual_is_the_same_characters_on_the_same_ascensions(self):
        # A character that goes further by itself is not a better relic.
        for _ in range(20):
            relics.note(run(40, ["STONE_HUMIDIFIER"], who="The Defect"), rebuild=False)
            relics.note(run(20, ["FISHING_ROD"], who="The Necrobinder"), rebuild=False)
            relics.note(run(20, ["LAVA_ROCK"], ascension=2), rebuild=False)
        relics.note(run(40, ["STONE_HUMIDIFIER"], who="The Defect"))
        for name in ("STONE_HUMIDIFIER", "FISHING_ROD", "LAVA_ROCK"):
            self.assertAlmostEqual(relics.shift(name), 0.0, delta=0.01)

    def test_the_record_is_read_from_the_run_history(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "runs.jsonl"
            lines = [json.dumps(run(40, ["STONE_HUMIDIFIER"])) for _ in range(30)] + [json.dumps(run(20, ["BOOMING_CONCH"])) for _ in range(30)]
            lines += ["not json", json.dumps({"character": "The Regent", "floor": 33, "result": "loss"})]  # (an old line: no relics)
            path.write_text("\n".join(lines) + "\n")
            relics.load(path, events._ANCIENT)
        self.assertEqual(relics.record()["STONE_HUMIDIFIER"][0], 30)
        self.assertGreater(relics.shift("STONE_HUMIDIFIER"), 2.0)
        self.assertEqual(relics.shift("WAX_CHOKER"), 0.0)  # not an Ancient's: nothing to move

    def test_a_missing_history_is_an_empty_record(self):
        relics.load(Path("/nonexistent/runs.jsonl"), events._ANCIENT)
        self.assertEqual(relics.record(), {})


class AncientRatings(unittest.TestCase):
    def test_every_relic_with_an_act_has_a_rating_or_is_left_to_its_text(self):
        # (A relic the table does not know is read word by word, as before: nothing breaks, it just is not learned.)
        for name in ("ASTROLABE", "EMPTY_CAGE", "SEAL_OF_GOLD", "PAELS_BLOOD", "SAI"):
            self.assertIn(name, events._ANCIENT)

    def test_relics_once_read_word_by_word_no_longer_outrank_an_energy_every_turn(self):
        value = lambda title: events._event_option_value({"title": title}, 60, 75)
        self.assertGreater(value("Philosopher's Stone"), value("Astrolabe"))  # (13.5 by its text until 2026-11-21)
        self.assertGreaterEqual(value("Sozu"), value("Empty Cage"))  # (11.5)
        self.assertGreater(value("Seal of Gold"), value("Biiig Hug"))


class SealOfGold(unittest.TestCase):
    def _shop(self, held, gold):
        items = [{"index": 0, "category": "relic", "price": 150, "is_stocked": True, "can_afford": True}, {"index": 1, "category": "card_removal", "price": 100, "is_stocked": True, "can_afford": True}]
        player = {"gold": gold, "relics": [{"id": r} for r in held], "potions": [], "max_potion_slots": 3}
        return strategy.shop({"state_type": "shop", "run": {"act": 2, "floor": 25}, "player": player, "shop": {"items": items, "can_proceed": True}}, Memory())

    def test_a_merchant_does_not_get_the_gold_the_seal_turns_into_energy(self):
        # 3 Gold a turn for an energy: with 170 the relic at 150 would leave 20, seven turns of a boss fight.
        self.assertEqual(self._shop(["BURNING_BLOOD"], 170)[0], ("shop_purchase", {"index": 1}))
        self.assertEqual(self._shop(["SEAL_OF_GOLD"], 170), [("shop_purchase", {"index": 1}), ("proceed", {})])
        self.assertEqual(self._shop(["SEAL_OF_GOLD"], 140), [("proceed", {})])  # nothing leaves 45
        self.assertEqual(len(self._shop(["SEAL_OF_GOLD"], 400)), 3)  # with plenty, everything as before


if __name__ == "__main__":
    unittest.main()
