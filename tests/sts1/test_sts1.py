"""The first Slay the Spire: the state as the bot reads it, the commands its actions become, whole runs."""

import json
from unittest import mock
import tempfile
from pathlib import Path

from sts2bot import cards, games
from sts2bot.fight import combat
from sts2bot.games.sts1 import host, text
from sts2bot.games.sts1.screens import Screens
from sts2bot.run import crowd, potions, strategy

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

    def test_a_skipped_card_reward_is_not_opened_again(self):
        # The first live run (2026-11-24): CommunicationMod names a reward's kind "reward_type", the game keeps a skipped card
        # reward on the list, and the bot opened it, skipped it and opened it again some hundreds of times a second.
        game = FakeComm(seed=1)
        client = Direct(game)
        game._start("IRONCLAD", 0)
        game.screen, game.phase, game.rewards = "COMBAT_REWARD", "COMPLETE", [{"reward_type": "GOLD", "gold": 15}, {"reward_type": "CARD"}]
        rewards = client.state()["rewards"]["items"]
        self.assertEqual([r["type"] for r in rewards], ["gold", "card"])
        bot = make_bot(client, character="IRONCLAD")
        with mock.patch.object(strategy, "reward_bar", lambda size, act: 99.0):  # (nothing on offer is good enough)
            for _ in range(8):
                bot.step()
        self.assertEqual(sum(1 for c in game.sent if c == "skip"), 1)
        self.assertIn("proceed", game.sent)  # (and on to the map)

    def test_a_fights_first_look_waits_for_the_intents(self):
        game = FakeComm(seed=1)
        client = Direct(game)
        game._start("IRONCLAD", 0)
        game._fight("MonsterRoom")
        game.intent_pending = 2  # (the game sets them a few frames in)
        enemy = client.state()["battle"]["enemies"][0]
        self.assertEqual(enemy["intents"][0]["type"], "Attack")
        self.assertEqual(game.sent.count("wait 10"), 2)
        client.state()
        self.assertEqual(game.sent.count("wait 10"), 2)  # (none once they are there)

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
        from sts2bot.games.sts1 import tiers

        self.assertEqual(crowd.rating("INFLAME"), tiers.TIER["INFLAME"])  # (the first game's own tiers: games/sts1/tiers.py)
        self.assertIsNone(crowd.rating("CRIMSON_MANTLE"))  # (a Slay the Spire 2 card)
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
            Path(d, "STSDataVagabond").write_text(json.dumps({"WIN_COUNT": "3", "ASCENSION_LEVEL": "6"}))  # (the Ironclad's)
            Path(d, "STSDataTheSilent").write_text(json.dumps({"WIN_COUNT": "0", "ASCENSION_LEVEL": "1"}))
            Path(d, "STSDataDefect").write_text("not json")
            Path(d, "STSUnlocks").write_text(json.dumps({"The Silent": "2", "Defect": "1", "GUARDIAN": "2"}))
            prefs = host.read_prefs(Path(d))
        self.assertEqual(prefs["wins"], 3)
        self.assertEqual(prefs["max_ascension"], {"IRONCLAD": 6, "THE_SILENT": 0})
        self.assertEqual(prefs["locked"], ["WATCHER"])

    def test_a_first_win_opens_ascension_1(self):
        # The game keeps no ASCENSION_LEVEL until the first level-up and reads it as 1 once a character has won.
        with tempfile.TemporaryDirectory() as d:
            Path(d, "STSDataVagabond").write_text(json.dumps({"WIN_COUNT": "1", "WIN_STREAK": "0"}))
            Path(d, "STSDataTheSilent").write_text(json.dumps({"WIN_STREAK": "0"}))
            prefs = host.read_prefs(Path(d))
        self.assertEqual(prefs["max_ascension"], {"IRONCLAD": 1, "THE_SILENT": 0})

    def test_a_new_profile_has_only_the_ironclad(self):
        with tempfile.TemporaryDirectory() as d:
            prefs = host.read_prefs(Path(d))
        self.assertEqual(prefs, {"max_ascension": {}, "wins": 0, "locked": ["THE_SILENT", "DEFECT", "WATCHER"]})
        client = Direct(FakeComm(), prefs=prefs)
        self.assertEqual([c["id"] for c in client.profile()["characters"]], ["IRONCLAD"])
        client.act("menu_select", option="singleplayer")
        self.assertEqual(client.act("menu_select", option="DEFECT")["status"], "error")  # (a locked one is not picked)

    def test_the_save_files_are_read_again_before_each_run(self):
        seen = {"max_ascension": {"IRONCLAD": 0}, "wins": 0, "locked": ["THE_SILENT", "DEFECT", "WATCHER"]}
        client = Direct(FakeComm(), prefs=lambda: seen)
        self.assertEqual(client.profile()["characters"], [{"id": "IRONCLAD", "max_ascension": 0}])
        seen = {"max_ascension": {"IRONCLAD": 1}, "wins": 1, "locked": ["DEFECT", "WATCHER"]}  # a win: A1, and the Silent
        self.assertEqual(client.profile()["characters"], [{"id": "IRONCLAD", "max_ascension": 1}, {"id": "THE_SILENT", "max_ascension": 0}])
        self.assertEqual(client.profile()["total_wins"], 1)

    def test_the_game_is_set_to_full_screen_and_fast_mode_before_it_opens(self):
        with tempfile.TemporaryDirectory() as d:
            game, prefs = Path(d), Path(d) / "preferences"
            (game / "info.displayconfig").write_text("2560\n1600\n60\nfalse\nfalse\ntrue\n")
            changed = host.prepare_game(True, prefs, game)
            self.assertEqual((game / "info.displayconfig").read_text().split(), ["2560", "1600", str(host.FRAME_CAP), "false", "true", "false"])
            self.assertEqual(json.loads((prefs / "STSGameplaySettings").read_text())["Fast Mode"], "true")
            tips = json.loads((prefs / "STSTips").read_text())
            self.assertEqual((tips["INTENT_TIP"], tips["CARD_REWARD_TIP"], tips["NO_FTUE"]), ("true", "true", "true"))
            slots = json.loads((prefs / "STSSaveSlots").read_text())
            self.assertEqual((slots["DEFAULT_SLOT"], slots["PROFILE_NAME"]), ("0", host.PROFILE_NAME))  # (a profile, as the game's name screen makes)
            sound = json.loads((prefs / "STSSound").read_text())
            self.assertEqual((sound["Master Volume"], sound["Music Volume"], sound["Sound Volume"], sound["Mute in Bg"]), ("1.0", "1.0", "1.0", "true"))
            self.assertEqual(len(changed), 5)
            self.assertEqual(host.prepare_game(True, prefs, game), [])  # (once set, left alone)
            host.prepare_game(False, prefs, game)
            self.assertEqual((game / "info.displayconfig").read_text().split()[3:5], ["false", "false"])  # --windowed

    def test_the_game_is_launched_with_its_clock_sped_up_and_its_output_kept(self):
        from sts2bot import supervisor
        from sts2bot.games.sts1 import speedmod

        with tempfile.TemporaryDirectory() as d, mock.patch.dict("os.environ", {"STS1_GAME_DIR": d, "STS1_PREFS_DIR": str(Path(d) / "preferences")}):
            spawned = {}
            fake_spawn = lambda argv, **kw: spawned.update(argv=argv, **kw)
            with mock.patch.object(supervisor, "_spawn", fake_spawn), mock.patch.object(speedmod, "ensure", lambda *a: True):
                host.Sts1Host(log_dir=Path(d), time_scale=3.0).launch(f"java -jar mts.jar --skip-launcher --mods {host.MODS}")
            self.assertIn(f"{host.MODS},{speedmod.MOD_ID}", spawned["argv"])
            self.assertEqual(spawned["env"][speedmod.ENV], "3")
            self.assertTrue(Path(d, "game.log").exists())
            spawned.clear()
            with mock.patch.object(supervisor, "_spawn", fake_spawn), mock.patch.object(speedmod, "ensure", lambda *a: False):
                host.Sts1Host(log_dir=Path(d), time_scale=3.0).launch(f"java -jar mts.jar --mods {host.MODS}")
            self.assertNotIn("env", spawned)  # (no mod: the game as it is)
            self.assertTrue(Path(d, "game.log.1").exists())  # (the launch before)

    def test_communication_mod_is_pointed_at_the_relay(self):
        with tempfile.TemporaryDirectory() as d:
            path = host.write_comm_config(15599, Path(d) / "CommunicationMod" / "config.properties")
            written = path.read_text()
        self.assertIn("relay.py --port 15599", written)
        self.assertIn("runAtGameStart=true", written)
        self.assertIn("verbose=false", written)  # (every message echoed into game.log otherwise)
