"""The first Slay the Spire: the state as the bot reads it, the commands its actions become, whole runs."""

import json
import tempfile
from pathlib import Path

from sts2bot import cards, games
from sts2bot.fight import combat
from sts2bot.games.sts1 import host, text
from sts2bot.games.sts1.screens import Screens
from sts2bot.run import crowd, potions

from ..helpers import make_bot
from .fake_comm import FakeComm
from .helpers import Direct, Sts1Case


class WholeRuns(Sts1Case):
    def test_the_bot_plays_runs_start_to_finish(self):
        game = FakeComm(seed=3)
        bot = make_bot(Direct(game, prefs={"max_ascension": {"IRONCLAD": 2}}), character="IRONCLAD")
        bot.run(max_runs=2)
        self.assertEqual(bot.runs_finished, 2)
        self.assertEqual(game.runs, 2)
        self.assertEqual(bot.wins, 2)  # (the boss is beaten: the fake's game over says victory)
        sent = game.sent
        self.assertIn("start IRONCLAD 2", sent)  # at the highest ascension the save files open
        self.assertTrue(any(s.startswith("play ") for s in sent))
        self.assertIn("end", sent)
        self.assertIn("choose shop", sent)  # the merchant's shelf is opened ...
        self.assertIn("leave", sent)  # ... and left
        self.assertTrue(all(not s.startswith("play ") or len(s.split()) in (2, 3) for s in sent))
        self.assertEqual(bot.mem.character_selected, False)  # (reset for the next run)

    def test_attacks_are_aimed_at_the_monster_by_its_place_in_the_list(self):
        game = FakeComm(seed=1)
        client = Direct(game)
        game._start("IRONCLAD", 0)
        game._fight("MonsterRoom")
        game.hand = [("Strike_R", "Strike", "ATTACK", 1, True)]
        state = client.state()
        self.assertEqual(state["state_type"], "monster")
        enemy = state["battle"]["enemies"][0]
        self.assertEqual(enemy["entity_id"], "JAWWORM_0")
        self.assertEqual(enemy["intents"], [{"type": "Attack", "label": "7"}])
        wanted = combat.actions(state)
        self.assertEqual(wanted[0], ("play_card", {"card_index": 0, "target": "JAWWORM_0"}))
        self.assertEqual(client.command(*wanted[0]), "play 1 0")
        self.assertEqual(client.act("end_turn")["status"], "ok")


class Translation(Sts1Case):
    def test_cards_read_like_the_second_games(self):
        s = Screens()
        bash = s.card({"id": "Bash", "name": "Bash", "type": "ATTACK", "cost": 2, "has_target": True, "rarity": "BASIC", "upgrades": 0})
        info = cards.parse(bash)
        self.assertEqual((info.damage, info.vulnerable, info.cost), (8, 2, 2))
        plus = cards.parse(s.card({"id": "Bash", "name": "Bash+", "type": "ATTACK", "cost": 2, "has_target": True, "upgrades": 1}))
        self.assertEqual((plus.damage, plus.vulnerable), (10, 3))
        # Ids that are older names find their text, and energy is read.
        sneaky = s.card({"id": "Underhanded Strike", "name": "Sneaky Strike", "type": "ATTACK", "cost": 2, "has_target": True})
        self.assertEqual(cards.parse(sneaky).damage, 12)
        seeing = cards.parse(s.card({"id": "Seeing Red", "name": "Seeing Red", "type": "SKILL", "cost": 1}))
        self.assertEqual(seeing.energy_gain, 2)
        whirl = s.card({"id": "Whirlwind", "name": "Whirlwind", "type": "ATTACK", "cost": -1})
        self.assertEqual((whirl["cost"], whirl["target_type"]), ("X", "AllEnemies"))

    def test_every_table_entry_expands_to_plain_text(self):
        for key, raw in text.CARDS.items():
            for up in (False, True):
                out = text.expand(raw, up)
                self.assertNotIn("||", out, key)
                self.assertNotRegex(out, r"\d+\(\d+\)", key)

    def test_the_boss_closes_every_road_on_the_map(self):
        s = Screens()
        msg = {"in_game": True, "available_commands": ["choose"], "game_state": {
            "screen_type": "MAP", "act": 1, "floor": 14, "current_hp": 50, "max_hp": 80, "gold": 0,
            "map": [{"x": 1, "y": 13, "symbol": "R", "children": [{"x": 1, "y": 14}]}, {"x": 1, "y": 14, "symbol": "M", "children": [{"x": 3, "y": 16}]}],
            "screen_state": {"next_nodes": [{"x": 1, "y": 14, "symbol": "M"}], "boss_available": False}}}
        st = s.translate(msg)
        self.assertEqual(st["map"]["next_options"][0]["leads_to"][0]["type"], "Boss")
        boss = [n for n in st["map"]["nodes"] if n["type"] == "Boss"]
        self.assertEqual(len(boss), 1)

    def test_potions_that_must_stay_on_the_belt_are_not_offered(self):
        s = Screens()
        s.raw = {"game_state": {"potions": [{"id": "FairyPotion", "name": "Fairy in a Bottle", "can_use": True, "requires_target": False},
                                            {"id": "Potion Slot", "name": "Potion Slot"},
                                            {"id": "Fire Potion", "name": "Fire Potion", "can_use": True, "requires_target": True}]}}
        belt = s.potions()
        self.assertEqual([p["slot"] for p in belt], [0, 2])
        self.assertFalse(belt[0]["can_use_in_combat"])
        self.assertEqual((belt[1]["target_type"], belt[1]["description"]), ("AnyEnemy", "Deal 20 damage."))

    def test_powers_take_the_names_the_planner_knows(self):
        m = {"powers": [{"id": "Weakened", "amount": 2}, {"id": "Plated Armor", "amount": 4}, {"id": "Vulnerable", "amount": 1}]}
        ids = [p["id"] for p in Screens.powers(m)]
        self.assertEqual(ids, ["WEAK_POWER", "PLATING_POWER", "VULNERABLE_POWER"])

    def test_menu_is_the_bots_own_until_the_run_starts(self):
        game = FakeComm()
        client = Direct(game, prefs={"max_ascension": {"DEFECT": 4}})
        self.assertEqual(client.state()["options"], ["singleplayer"])
        self.assertEqual(client.act("menu_select", option="singleplayer")["status"], "ok")
        self.assertEqual(client.act("menu_select", option="DEFECT")["status"], "ok")
        self.assertEqual(client.state()["ascension"], 4)
        self.assertEqual(client.act("menu_select", option="confirm")["status"], "ok")
        self.assertEqual(game.sent[-1], "start DEFECT 4")
        self.assertEqual(client.state()["state_type"], "map")


class GameSwitch(Sts1Case):
    def test_the_second_games_ratings_stay_out_of_the_first(self):
        self.assertIsNone(crowd.rating("INFLAME"))
        self.assertFalse(cards.is_known({"id": "INFLAME", "name": "Inflame"}))
        self.assertEqual(potions.boss_floors(), potions.STS1_BOSS_FLOORS)
        games.select("sts2")
        self.assertIsNotNone(crowd.rating("INFLAME"))
        self.assertEqual(potions.boss_floors(), potions.BOSS_FLOORS)

    def test_unknown_game_is_refused(self):
        with self.assertRaises(ValueError):
            games.select("sts3")


class SaveFilesAndSetup(Sts1Case):
    def test_ascension_and_wins_come_from_the_save_files(self):
        with tempfile.TemporaryDirectory() as d:
            Path(d, "IRONCLAD").write_text(json.dumps({"WIN_COUNT": "3", "ASCENSION_LEVEL": "6"}))
            Path(d, "THE_SILENT").write_text(json.dumps({"WIN_COUNT": "0", "ASCENSION_LEVEL": "1"}))
            Path(d, "DEFECT").write_text("not json")
            prefs = host.read_prefs(Path(d))
        self.assertEqual(prefs["wins"], 3)
        self.assertEqual(prefs["max_ascension"], {"IRONCLAD": 6, "THE_SILENT": 0})

    def test_communication_mod_is_pointed_at_the_relay(self):
        with tempfile.TemporaryDirectory() as d:
            path = host.write_comm_config(15599, Path(d) / "CommunicationMod" / "config.properties")
            written = path.read_text()
        self.assertIn("relay.py --port 15599", written)
        self.assertIn("runAtGameStart=true", written)
