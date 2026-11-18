import json
import tempfile
import unittest
from pathlib import Path

from sts2bot import combat, learn, strategy
from sts2bot.bot import Bot, Stuck
from sts2bot.cards import deck_value, parse
from sts2bot.strategy import Memory

from .fake_game import FakeGame, card
from .helpers import DEFEND, STRIKE, FakeClock, combat_state, enemy, make_bot  # noqa: F401


class FullLoop(unittest.TestCase):
    def test_map_travel_is_not_clicked_twice(self):
        """While the travel animation plays the map still answers; wait it out."""

        class TravellingMap:
            def __init__(self):
                self.clicks = 0
                self.polls_left = 0

            def state(self):
                if self.clicks and self.polls_left <= 0:
                    return {"state_type": "event", "event": {"in_dialogue": True}}
                self.polls_left -= 1
                opts = [{"index": 0, "col": self.clicks, "row": 1, "type": "Monster"}]
                return {"state_type": "map", "map": {"next_options": opts, "nodes": []}, "player": {"hp": 50, "max_hp": 80}}

            def act(self, action, **params):
                if action == "choose_map_node":
                    self.clicks += 1
                    self.polls_left = 5
                return {"status": "ok"}

        game = TravellingMap()
        make_bot(game).step()
        self.assertEqual(game.clicks, 1)
        self.assertEqual(game.state()["state_type"], "event")

    def test_plays_several_runs_back_to_back(self):
        game = FakeGame(seed=1, floors=4)
        with tempfile.TemporaryDirectory() as d:
            bot = make_bot(game, log_dir=Path(d))
            bot.run(max_runs=3)
            lines = [json.loads(l) for l in (Path(d) / "runs.jsonl").read_text().splitlines()]
        self.assertEqual(bot.runs_finished, 3)
        self.assertEqual(game.runs_started, 3)
        self.assertEqual(game.actions[-1], ("menu_select", {"option": "main_menu"}))
        self.assertEqual(len(lines), 3)
        self.assertTrue(all(l["result"] == "win" and l["floor"] >= 4 for l in lines))
        self.assertEqual(game.screen, "main")
        names = {a for a, _ in game.actions}
        self.assertLessEqual({"play_card", "end_turn", "claim_reward", "select_card_reward", "choose_map_node", "choose_rest_option"}, names)

    def test_skipped_card_reward_does_not_loop(self):
        game = FakeGame(seed=2, floors=3)
        bot = make_bot(game)
        import sts2bot.strategy as st

        old = st.SKIP_THRESHOLD
        st.SKIP_THRESHOLD = 99.0  # skip everything
        try:
            bot.run(max_runs=1)
        finally:
            st.SKIP_THRESHOLD = old
        skips = sum(1 for a, _ in game.actions if a == "skip_card_reward")
        offered = sum(1 for a, p in game.actions if (a, p) == ("claim_reward", {"index": 1}))
        self.assertGreaterEqual(skips, 1)
        self.assertEqual(skips, offered)  # each card reward opened once, never re-opened
        self.assertEqual(len(game.deck), 10)

    def test_loses_a_run_and_starts_again(self):
        game = FakeGame(seed=3, floors=50, enemy_hp=70, enemy_attack=22)
        bot = make_bot(game)
        bot.run(max_runs=2)
        self.assertEqual((bot.runs_finished, bot.wins), (2, 0))

    def test_resumes_a_saved_run_from_the_menu(self):
        game = FakeGame(seed=4, floors=3)
        bot = make_bot(game)
        while game.screen != "combat":
            bot.step()
        game.saved_screen, game.screen = "combat", "main"  # as after a game restart
        bot.run(max_runs=1)
        self.assertEqual(game.runs_started, 1)
        self.assertIn(("menu_select", {"option": "continue"}), game.actions)

    def test_abandons_when_told_to(self):
        game = FakeGame(seed=5)
        bot = make_bot(game)
        while game.screen != "combat":
            bot.step()
        game.saved_screen, game.screen = "combat", "main"
        bot.abandon_next = True
        for _ in range(6):
            bot.step()
        self.assertFalse(bot.abandon_next)
        self.assertEqual(game.runs_started, 2)

    def test_raises_stuck_when_nothing_works(self):
        class Frozen(FakeGame):
            def state(self):
                return {"state_type": "overlay", "run": {"act": 1, "floor": 2}, "player": {"hp": 5, "max_hp": 80}, "overlay": {"screen_type": "NWeird"}}

            def act(self, action, **p):
                self.actions.append((action, p))
                return {"status": "error", "error": "nope"}

        bot = make_bot(Frozen(), stuck_timeout=120)
        with self.assertRaises(Stuck):
            for _ in range(10_000):
                bot.step()


class FastMode(unittest.TestCase):
    def test_sent_through_the_mod_and_tolerates_an_old_mod(self):
        from sts2bot.supervisor import set_fast_mode

        class Mod:
            def __init__(self, reply):
                self.reply, self.calls = reply, []

            def act(self, action, **params):
                self.calls.append((action, params))
                return self.reply

        new = Mod({"status": "ok", "fast_mode": "instant"})
        self.assertTrue(set_fast_mode(new, "instant"))
        self.assertEqual(new.calls, [("set_fast_mode", {"mode": "instant"})])
        with self.assertLogs("sts2bot.supervisor", level="WARNING"):
            self.assertFalse(set_fast_mode(Mod({"status": "error", "error": "Unknown action"}), "instant"))


class CrashRecovery(unittest.TestCase):
    def test_a_crashed_child_is_not_counted_as_running(self):
        import subprocess, sys, time
        from unittest import mock
        from sts2bot import supervisor

        child = subprocess.Popen([sys.executable, "-c", "pass"])
        time.sleep(0.3)  # exited, not yet collected: a zombie
        supervisor._children.append(child)
        zombie = mock.Mock(stdout="Z\n")
        with mock.patch.object(supervisor.subprocess, "run", return_value=zombie):
            self.assertFalse(supervisor.game_running())
        self.assertNotIn(child, supervisor._children)
        alive = mock.Mock(stdout="Sl\n")
        with mock.patch.object(supervisor.subprocess, "run", return_value=alive):
            self.assertTrue(supervisor.game_running())


class LiveGameLessons(unittest.TestCase):
    """Things the first sessions against the real game got wrong."""

    ICON = "[ironclad_energy_icon.png]"

    def test_energy_is_written_as_icons(self):
        c = parse(card(0, "OFFERING", "Offering", "Skill", 0, f"Lose 6 HP. Gain {self.ICON}{self.ICON}. Draw 3 cards. Exhaust."))
        self.assertEqual((c.energy_gain, c.draw, c.hp_loss), (2, 3, 6))

    def test_later_effects_are_not_read_as_immediate(self):
        jug = parse(card(0, "JUGGERNAUT", "Juggernaut", "Power", 2, "Whenever you gain Block, deal 6 damage to a random enemy."))
        self.assertEqual(jug.damage, 0)
        barrier = parse(card(0, "FLAME_BARRIER", "Flame Barrier", "Skill", 2, "Gain 12 Block. Whenever you are attacked this turn, deal 4 damage back."))
        self.assertEqual((barrier.block, barrier.damage), (12, 0))
        bomb = parse(card(0, "THE_BOMB", "The Bomb", "Skill", 2, "At the end of 3 turns, deal 40 damage to ALL enemies."))
        self.assertEqual(bomb.damage, 0)

    def test_weak_and_vulnerable_together(self):
        c = parse(card(0, "SHOCKWAVE", "Shockwave", "Skill", 2, "Apply 3 Weak and Vulnerable to ALL enemies. Exhaust."))
        self.assertEqual((c.weak, c.vulnerable, c.aoe), (3, 3, True))

    def test_strength_this_turn_adds_to_later_attacks(self):
        setup = card(0, "SETUP_STRIKE", "Setup Strike", "Attack", 1, "Deal 7 damage. Gain 3 Strength this turn.", "AnyEnemy")
        hand = [parse(setup), parse(STRIKE(1)), parse(STRIKE(2))]
        foes = combat.read_enemies(combat_state([], [enemy("A", 100)]))
        ctx = dict(energy=3, stars=0, hp=80, max_hp=80, block=0)
        with_setup = combat.simulate(hand, foes, **ctx).score
        plain = combat.simulate([parse(STRIKE(0)), parse(STRIKE(1)), parse(STRIKE(2))], foes, **ctx).score
        self.assertAlmostEqual(with_setup - plain, 1 + 3 + 3)

    def test_demon_form_is_played_early_in_a_boss_fight(self):
        demon = card(0, "DEMON_FORM", "Demon Form", "Power", 3, "At the start of your turn, gain 3 Strength.")
        state = combat_state([demon, STRIKE(1), STRIKE(2), STRIKE(3)], [enemy("BOSS", 250)], kind="boss")
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))

    def test_sapping_strength_counts_as_defence(self):
        shackles = card(0, "DARK_SHACKLES", "Dark Shackles", "Skill", 0, "Enemy loses 9 Strength this turn. Exhaust.", "AnyEnemy")
        foe = enemy("A", 100)
        foe["intents"] = [{"type": "Attack", "label": "10x3"}]
        state = combat_state([shackles], [foe], energy=0, hp=20)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "A"}))

    def test_never_trades_max_hp_away(self):
        def best(*options):
            opts = [{"index": i, "title": t, "description": d} for i, (t, d) in enumerate(options)]
            state = {"state_type": "event", "event": {"options": opts}, "player": {"hp": 70, "max_hp": 80}}
            return strategy.event(state, Memory())[0][1]["index"]

        self.assertEqual(best(("Lose Everything", "???"), ("Give Up", "Stop reading the text and leave.")), 1)
        self.assertEqual(best(("Continue", "Lose 24 Max HP. Upgrade 2 random cards."), ("Give Up", "Leave.")), 1)
        self.assertEqual(best(("Decipher", "Lose 3 Max HP. Upgrade a random card."), ("Give Up", "Leave.")), 0)

    def test_an_event_that_keeps_costing_hp_is_left_after_a_few_bites(self):
        # Slippery Bridge: "Hold On" costs 3, 4, 5, ... HP each time. Priced one bite at a
        # time it always looked cheaper than losing a card, and one run lost 42 HP there.
        mem = Memory()
        hp, picks = 71, []
        for loss in (3, 4, 5):
            opts = [
                {"index": 0, "title": "Overcome", "description": "Shining Strike is removed from your Deck."},
                {"index": 1, "title": "Hold On", "description": f"Lose {loss} HP. The card in the above option is randomized."},
            ]
            state = {"state_type": "event", "event": {"event_id": "SLIPPERY_BRIDGE", "options": opts},
                     "player": {"hp": hp, "max_hp": 75}}
            picks.append(strategy.event(state, mem)[0][1]["index"])
            if picks[-1] == 1:
                hp -= loss
        self.assertEqual(picks, [1, 1, 0])

    def test_bridge_event_gives_up_a_starter_card_not_a_good_one(self):
        def value(title, text, hp=60):
            return strategy._event_option_value({"title": title, "description": text}, hp, 80)

        hold = value("Hold On", "Lose 5 HP. The card in the above option is randomized.")
        self.assertGreater(value("Overcome", "Defend is removed from your Deck."), hold)
        self.assertLess(value("Overcome", "Demon Form+ is removed from your Deck."), hold)

    def test_an_event_never_doubles_the_deck_or_hands_out_a_curse_as_a_relic(self):
        # The Reflections event: "Receive Bad Luck" was scored as a relic, and
        # "Duplicate your Deck" was not penalised. One run doubled its deck to 71.
        def value(title, text, hp=69):
            return strategy._event_option_value({"title": title, "description": text}, hp, 105)

        shatter = value("Shatter", "Duplicate your Deck. Receive Bad Luck.")
        mirror = value("Touch a Mirror", "Downgrade 2 random cards. Upgrade 4 random cards.")
        self.assertLess(shatter, 0)
        self.assertGreater(mirror, shatter)
        self.assertGreater(value("Obtain the Spoils Map", "A relic."), 3)  # a real relic by name still counts

    def test_picks_the_mildest_boss_affliction(self):
        cards_ = [
            card(0, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 6 damage."),
            card(1, "SLOTH", "Sloth", "Status", 0, "You cannot play more than 3 cards each turn."),
            card(2, "WASTE_AWAY", "Waste Away", "Status", 0, "Gain 1 less energy per turn."),
        ]
        state = {"state_type": "card_select", "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": cards_}}
        self.assertEqual(strategy.card_select(state, Memory())[0], ("select_card", {"index": 1}))

    def test_unreadable_cards_are_not_taken_for_their_rarity(self):
        odd = card(0, "NEW_THING", "New Thing", "Skill", 1, "Do something the bot has never seen.", rarity="Rare")
        state = {"state_type": "card_reward", "run": {"act": 2}, "card_reward": {"cards": [odd], "can_skip": True}}
        self.assertEqual(strategy.card_reward(state, Memory())[0][0], "skip_card_reward")

    def test_a_burrowed_enemy_is_dug_out_instead_of_blocked_forever(self):
        burrowed = [{"id": "BURROWED_POWER", "amount": 1}]
        bash = card(3, "BASH", "Bash", "Attack", 2, "Deal 11 damage. Apply 2 Vulnerable.", "AnyEnemy")
        hand = [DEFEND(0), DEFEND(1), DEFEND(2), bash]
        state = combat_state(hand, [enemy("T", 62, attack=24, block=20, status=burrowed)], hp=57)
        state["battle"]["round"] = 25
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 3, "target": "T"}))
        # Breaking the last of its Block stuns it, so that comes before blocking.
        state = combat_state(hand, [enemy("T", 62, attack=24, block=9, status=burrowed)], hp=57)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 3, "target": "T"}))

    def test_escapes_the_sandpit_before_it_closes(self):
        escape = card(0, "FRANTIC_ESCAPE", "Frantic Escape", "Status", 1, "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.")
        boss = enemy("INS", 200, attack=20, status=[{"id": "SANDPIT_POWER", "amount": 1}])
        state = combat_state([escape, DEFEND(1), DEFEND(2), DEFEND(3)], [boss], kind="boss")
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))

    def test_kills_the_minion_that_does_all_the_damage(self):
        minion = enemy("TORCH", 199, attack=28, status=[{"id": "MINION_POWER", "amount": 1}])
        queen = enemy("QUEEN", 400)
        state = combat_state([STRIKE(0)], [minion, queen], kind="boss", energy=1)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "TORCH"}))
        # Small fry beside a leader that fights for itself are still ignored.
        weak = enemy("FOLLOWER", 59, attack=5, status=[{"id": "MINION_POWER", "amount": 1}])
        state = combat_state([STRIKE(0)], [weak, enemy("PRIEST", 190, attack=8)], kind="boss", energy=1)
        self.assertEqual(combat.choose_card(state)[1]["target"], "PRIEST")

    def test_turns_to_face_the_enemy_about_to_hit_hard(self):
        """Surrounded: the enemy behind us hits 50% harder; aiming a card at it turns us round."""
        crusher = enemy("CRUSHER", 200, attack=6, status=[{"id": "BACK_ATTACK_LEFT_POWER", "amount": 1}])
        rocket = enemy("ROCKET", 150, attack=27, status=[{"id": "BACK_ATTACK_RIGHT_POWER", "amount": 1}])
        state = combat_state([STRIKE(0)], [crusher, rocket], kind="boss", energy=1, hp=40)
        state["player"]["status"] = [{"id": "SURROUNDED_POWER", "amount": 1}]
        state["_facing"] = "CRUSHER"  # so Rocket's 27 is its attack from behind
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "ROCKET"}))
        foes = combat.read_enemies(state)
        self.assertEqual([(e.entity_id, e.behind) for e in foes], [("CRUSHER", False), ("ROCKET", True)])

    def test_hard_to_kill_caps_each_hit(self):
        shell = [{"id": "HARD_TO_KILL_POWER", "amount": 9}]
        big = card(0, "BLUDGEON", "Bludgeon", "Attack", 3, "Deal 32 damage.", "AnyEnemy")
        twin = card(1, "TWIN_STRIKE", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy")
        state = combat_state([big, twin, STRIKE(2), STRIKE(3)], [enemy("EXO", 28, status=shell)])
        self.assertNotEqual(combat.choose_card(state)[1]["card_index"], 0)

    def test_block_potion_waits_for_a_big_hit(self):
        potion = {"slot": 0, "id": "BLOCK_POTION", "name": "Block Potion", "description": "Gain 12 Block.", "target_type": "Self"}
        calm = combat_state([STRIKE(0)], [enemy("BOSS", 300)], kind="boss", potions=[potion])
        self.assertNotIn("use_potion", [a for a, _ in combat.actions(calm)])
        storm = combat_state([STRIKE(0)], [enemy("BOSS", 300, attack=30)], kind="boss", potions=[potion])
        self.assertEqual(combat.actions(storm)[0], ("use_potion", {"slot": 0}))

    def test_only_one_bound_card_is_planned_per_turn(self):
        bound = {"keywords": [{"name": "Bound", "description": "Only 1 Bound card can be played each turn."}]}
        hand = [
            card(0, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self", **bound),
            card(1, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self", **bound),
            card(2, "IMPERVIOUS", "Impervious", "Skill", 2, "Gain 30 Block. Exhaust.", "Self", **bound),
        ]
        # Without the rule the plan would be Defend + Impervious; the first
        # Defend played would then lock Impervious out and the hit would land.
        state = combat_state(hand, [enemy("A", 200, attack=30)], hp=40)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 2}))

    def test_two_cards_together_kill_the_reviving_attacker(self):
        ghost = enemy("PARAFRIGHT", 21, attack=16, status=[{"id": "ILLUSION_POWER", "amount": 1}, {"id": "MINION_POWER", "amount": 1}])
        leader = enemy("OBSCURA", 101, attack=10)
        bash = card(0, "BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy")
        twin = card(1, "TWIN_STRIKE", "Twin Strike+", "Attack", 1, "Deal 7 damage twice.", "AnyEnemy")
        state = combat_state([bash, twin, DEFEND(2)], [ghost, leader], hp=18)
        action, params = combat.choose_card(state)
        self.assertEqual((action, params.get("target")), ("play_card", "PARAFRIGHT"))

    def test_a_win_is_read_from_the_games_own_counter(self):
        class Game:
            wins = 4

            def profile(self):
                return {"total_wins": self.wins, "characters": []}

        game = Game()
        bot = Bot(game, clock=FakeClock().now, sleep=lambda s: None)
        bot._refresh_ascensions()  # run start: remembers the counter
        over = {"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 0, "max_hp": 80}}
        game.wins = 5
        with self.assertLogs("sts2bot", level="INFO") as logs:
            bot._record_run(over)
        self.assertIn("win", logs.output[0])
        self.assertEqual(bot.wins, 1)
        with self.assertLogs("sts2bot", level="INFO") as logs:
            bot._record_run({**over, "player": {"hp": 12, "max_hp": 80}})  # counter unchanged: a loss
        self.assertIn("loss", logs.output[0])

    def test_reveals_unlocks_on_the_timeline_before_starting_a_run(self):
        mem = Memory()
        main = {"state_type": "menu", "menu_screen": "main", "options": ["timeline", "settings", "quit"],
                "pending_epoch_ids": ["NEOW_EPOCH"]}
        self.assertEqual(strategy.menu(main, mem, "auto")[0], ("menu_select", {"option": "timeline"}))
        timeline = {"state_type": "menu", "menu_screen": "timeline",
                    "options": [{"name": "advance", "enabled": True}, {"name": "back", "enabled": True}]}
        self.assertEqual(strategy.menu(timeline, mem, "auto")[0], ("menu_select", {"option": "advance"}))

        class Game:
            def state(self):
                return timeline

            def act(self, action, **params):
                return {"status": "ok", "message": "No more epochs to advance", "done": True}

        clock = FakeClock()
        bot = Bot(Game(), clock=clock.now, sleep=clock.sleep)
        for _ in range(3):
            bot.step()  # the mod says nothing is left, three times running ...
        self.assertEqual(strategy.menu(timeline, bot.mem, "auto")[0], ("menu_select", {"option": "back"}))
        # ... so back at the menu the bot plays instead of going round again.
        main = {"state_type": "menu", "menu_screen": "main", "options": ["singleplayer", "timeline", "quit"]}
        self.assertEqual(strategy.menu(main, bot.mem, "auto")[0], ("menu_select", {"option": "singleplayer"}))
        # A run in progress comes first; the Timeline can wait until it is over.
        busy = {"state_type": "menu", "menu_screen": "main", "options": ["continue", "abandon_run", "timeline"],
                "pending_epoch_ids": ["SILENT2_EPOCH"]}
        self.assertEqual(strategy.menu(busy, Memory(), "auto")[0], ("menu_select", {"option": "continue"}))

    def test_choose_screen_click_is_retried_quickly(self):
        """The first click on a "choose 1 of 3" screen is often ignored by the game."""

        class Game:
            def __init__(self):
                self.clicks = 0

            def state(self):
                done = self.clicks >= 2
                cs = {"screen_type": "choose", "prompt": "Choose a card.", "cards": [] if done else [STRIKE(0)]}
                return {"state_type": "map" if done else "card_select", "card_select": cs}

            def act(self, action, **params):
                self.clicks += 1
                return {"status": "ok", "message": "Choosing card"}

        game = Game()
        clock = FakeClock()
        bot = Bot(game, clock=clock.now, sleep=clock.sleep)
        bot.step()
        bot.step()
        self.assertEqual(game.clicks, 2)
        self.assertLess(clock.t, 1.5)


class CrashLoop(unittest.TestCase):
    def test_repeated_crashes_in_one_place_slow_the_bot_then_abandon_the_run(self):
        from unittest import mock
        from sts2bot import supervisor

        bot = mock.Mock()
        bot.progress.return_value = (3, 1, 6)
        bot.clock.return_value = 100.0
        bot.abandon_next = False
        bot.careful_until = 0.0
        sup = supervisor.Supervisor(bot, mock.Mock(), launch_cmd="game")
        sup._note_crash()  # a first crash can be chance
        self.assertEqual((bot.careful_until, bot.abandon_next), (0.0, False))
        with self.assertLogs("sts2bot.supervisor", level="WARNING"):
            sup._note_crash()
        self.assertEqual((bot.careful_until, bot.abandon_next), (220.0, False))
        with self.assertLogs("sts2bot.supervisor", level="WARNING"):
            sup._note_crash()  # the third crash in the same place: the run is given up
        self.assertTrue(bot.abandon_next)
        # Getting somewhere new starts the count again.
        bot.abandon_next = False
        bot.progress.return_value = (3, 1, 7)
        sup._note_crash()
        self.assertFalse(bot.abandon_next)


class SteamUpkeep(unittest.TestCase):
    """The game started directly needs the Steam client; the supervisor starts it."""

    def test_steam_is_started_before_the_game_when_it_is_not_running(self):
        from unittest import mock
        from sts2bot import supervisor

        events = []
        up = {"steam": False}

        def running(name):
            return up["steam"]

        def popen(argv, **kw):
            events.append(Path(argv[0]).name)
            if Path(argv[0]).name == "steam":
                up["steam"] = True
            return mock.Mock(poll=lambda: None)

        with mock.patch.object(supervisor, "_process_running", running), mock.patch.object(
            supervisor.shutil, "which", return_value="/usr/bin/steam"
        ), mock.patch.object(supervisor.subprocess, "Popen", popen), mock.patch.object(
            supervisor.time, "sleep"
        ), mock.patch.object(supervisor, "_children", []), mock.patch.dict(supervisor.os.environ):
            supervisor.os.environ.pop("INVOCATION_ID", None)  # not under systemd: started directly
            supervisor.launch_game("/games/sts2/SlayTheSpire2")
            self.assertEqual(events, ["steam", "SlayTheSpire2"])
            supervisor.launch_game("/games/sts2/SlayTheSpire2")  # Steam is up now: not started twice
            self.assertEqual(events, ["steam", "SlayTheSpire2", "SlayTheSpire2"])


    def test_under_systemd_the_game_gets_a_scope_of_its_own(self):
        from unittest import mock
        from sts2bot import supervisor

        with mock.patch.dict(supervisor.os.environ, {"INVOCATION_ID": "abc"}), mock.patch.object(
            supervisor.shutil, "which", return_value="/usr/bin/systemd-run"
        ):
            self.assertEqual(supervisor._detached(["game", "-x"])[:3], ["systemd-run", "--user", "--scope"])
            self.assertEqual(supervisor._detached(["game", "-x"])[-2:], ["game", "-x"])
        with mock.patch.dict(supervisor.os.environ):
            supervisor.os.environ.pop("INVOCATION_ID", None)
            self.assertEqual(supervisor._detached(["game"]), ["game"])


class ModUpkeep(unittest.TestCase):
    """A game update breaks the bridge mod; the supervisor rebuilds it by itself."""

    def test_load_failure_is_read_from_the_game_log(self):
        import os, time
        from unittest import mock
        from sts2bot import supervisor

        with tempfile.TemporaryDirectory() as d:
            logf = Path(d) / "godot.log"
            with mock.patch.object(supervisor, "GAME_LOG", logf):
                self.assertIsNone(supervisor.mod_load_error())
                logf.write_text("[INFO] Loading assembly DLL mods/STS2_MCP.dll\n[INFO]  --- RUNNING MODDED! --- Loaded 1 mods\n")
                self.assertIsNone(supervisor.mod_load_error())
                logf.write_text(
                    "[ERROR] Exception thrown while loading mod STS2_MCP: System.Reflection.ReflectionTypeLoadException: x\n"
                    "   at System.Reflection.RuntimeModule.GetTypes()\n"
                    "System.TypeLoadException: Could not load type 'A.LobbyPlayer' from assembly 'sts2'.\n"
                )
                self.assertIn("LobbyPlayer", supervisor.mod_load_error())
                # A log from before this launch says nothing about it.
                os.utime(logf, (time.time() - 100, time.time() - 100))
                self.assertIsNone(supervisor.mod_load_error(since=time.time() - 10))

    def test_mod_is_stale_when_the_game_files_change(self):
        from unittest import mock
        from sts2bot import supervisor

        with tempfile.TemporaryDirectory() as d:
            stamp = Path(d) / "stamp"
            with mock.patch.object(supervisor, "MOD_STAMP", stamp), mock.patch.object(
                supervisor, "game_fingerprint", return_value="v1"
            ):
                self.assertTrue(supervisor.mod_is_stale())  # never built by the bot
                stamp.write_text("v1\n")
                self.assertFalse(supervisor.mod_is_stale())
            with mock.patch.object(supervisor, "MOD_STAMP", stamp), mock.patch.object(
                supervisor, "game_fingerprint", return_value="v2"
            ):
                self.assertTrue(supervisor.mod_is_stale())

    def test_rebuilds_and_relaunches_when_the_game_rejects_the_mod(self):
        from unittest import mock
        from sts2bot import supervisor

        class Api:
            up = False

            def ping(self):
                return self.up

        api = Api()
        events = []
        state = {"error": "Could not load type X"}

        def rebuild():
            events.append("rebuild")
            state["error"] = None
            return True

        def launch(cmd):
            events.append("launch")
            api.up = state["error"] is None

        sup = supervisor.Supervisor(mock.Mock(), api, launch_cmd="game", boot_timeout=0.05)
        with mock.patch.multiple(
            supervisor,
            game_running=lambda: False,
            mod_is_stale=lambda: False,
            mod_load_error=lambda since=0.0: state["error"],
            rebuild_mod=rebuild,
            launch_game=launch,
            kill_game=lambda grace=10.0: events.append("kill"),
        ), mock.patch.object(supervisor.time, "sleep"), self.assertLogs("sts2bot.supervisor", level="ERROR"):
            sup.ensure_game()
        self.assertEqual(events, ["launch", "kill", "rebuild", "launch"])

    def test_rebuilds_before_launch_after_a_game_update(self):
        from unittest import mock
        from sts2bot import supervisor

        api = mock.Mock()
        api.ping.side_effect = [False, True]
        events = []
        sup = supervisor.Supervisor(mock.Mock(), api, launch_cmd="game")
        with mock.patch.multiple(
            supervisor,
            game_running=lambda: False,
            mod_is_stale=lambda: True,
            mod_load_error=lambda since=0.0: None,
            rebuild_mod=lambda: events.append("rebuild") or True,
            launch_game=lambda cmd: events.append("launch"),
        ), self.assertLogs("sts2bot.supervisor", level="WARNING"):
            sup.ensure_game()
        self.assertEqual(events, ["rebuild", "launch"])


class HttpClient(unittest.TestCase):
    """The real Client against a local HTTP server that fronts the fake game."""

    def test_round_trip_and_errors(self):
        import threading
        from http.server import BaseHTTPRequestHandler, HTTPServer

        from sts2bot.client import Client, GameUnavailable

        game = FakeGame(seed=6, floors=2)

        class Handler(BaseHTTPRequestHandler):
            def _send(self, code, body):
                raw = json.dumps(body).encode()
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def do_GET(self):
                self._send(200, game.state() if self.path.startswith("/api") else {"status": "ok"})

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                result = game.act(body.pop("action"), **body)
                self._send(200 if result["status"] == "ok" else 400, result)

            def log_message(self, *a):
                pass

        server = HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            client = Client(port=server.server_address[1])
            self.assertTrue(client.ping())
            self.assertEqual(client.state()["state_type"], "menu")
            self.assertEqual(client.act("end_turn")["status"], "error")
            bot = Bot(client, poll=0.0, settle=0.05)
            bot.run(max_runs=1)
            self.assertEqual(bot.runs_finished, 1)
        finally:
            server.shutdown()
            server.server_close()
        with self.assertRaises(GameUnavailable):
            client.state()
        self.assertFalse(client.ping())


if __name__ == "__main__":
    unittest.main()
