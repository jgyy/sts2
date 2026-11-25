import json
import os
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from unittest import mock

from sts2bot import supervisor
from sts2bot.bot import Bot, Stuck
from sts2bot.cards import parse
from sts2bot.client import Client, GameUnavailable, _dechunk
from sts2bot.fight import combat
from sts2bot.run import decks, strategy
from sts2bot.run.strategy import Memory

from .fake_game import FakeGame, card
from .helpers import DEFEND, STRIKE, FakeClock, combat_state, enemy, make_bot

ICON = "[ironclad_energy_icon.png]"


def event_value(title, text, hp=60, max_hp=80):
    return strategy._event_option_value({"title": title, "description": text}, hp, max_hp)


def quiet_bot(game, sleep=lambda s: None):
    """A bot whose waits take no time and leave its clock where it is."""
    return Bot(game, clock=FakeClock().now, sleep=sleep)


def restarted(**kw):
    """A fake game and its bot, stopped at the first fight and back on the main menu, as after a game restart."""
    game = FakeGame(**kw)
    bot = make_bot(game)
    while game.screen != "combat":
        bot.step()
    game.saved_screen, game.screen = "combat", "main"
    return game, bot


def crashing(where, **kw):
    """A supervisor over a mock bot that stands at `where` (its progress())."""
    bot = mock.Mock(**kw)
    bot.progress.return_value, bot.clock.return_value, bot.abandon_next = where, 100.0, False
    return bot, supervisor.Supervisor(bot, mock.Mock(), launch_cmd="game")


class Wins:
    """A game that only shows its profile: `wins` on the win counter, one more from the `late`-th look on (0: never)."""

    def __init__(self, wins, late=0):
        self.wins, self.late, self.asked = wins, late, 0

    def profile(self):
        self.asked += 1
        return {"total_wins": self.wins + bool(self.late and self.asked >= self.late), "characters": []}


class FullLoop(unittest.TestCase):
    def test_map_travel_is_not_clicked_twice(self):
        """While the travel animation plays the map still answers; wait it out."""

        class TravellingMap:
            clicks, polls_left = 0, 0

            def state(self):
                if self.clicks and self.polls_left <= 0:
                    return {"state_type": "event", "event": {"in_dialogue": True}}
                self.polls_left -= 1
                opts = [{"index": 0, "col": self.clicks, "row": 1, "type": "Monster"}]
                return {"state_type": "map", "map": {"next_options": opts, "nodes": []}, "player": {"hp": 50, "max_hp": 80}}

            def act(self, action, **params):
                if action == "choose_map_node":
                    self.clicks, self.polls_left = self.clicks + 1, 5
                return {"status": "ok"}

        game = TravellingMap()
        make_bot(game).step()
        self.assertEqual(game.clicks, 1)
        self.assertEqual(game.state()["state_type"], "event")

    def test_a_potion_is_not_one_of_the_cards_a_turn_allows(self):
        bot = make_bot(FakeGame())
        state = combat_state([STRIKE(0), DEFEND(1)], [enemy("KNOWLEDGE_DEMON_0", 200, attack=30)], hp=38)
        state["player"]["status"] = [{"id": "SLOTH_POWER", "amount": 0}]  # (the number is the game's count of cards so far)
        bot._combat(state)
        bot._after_success(state, "use_potion", {"slot": 0})
        bot._after_success(state, "play_card", {"card_index": 0})
        bot._after_success(state, "play_card", {"card_index": 0})
        state["player"]["status"][0]["amount"] = 2
        bot._played_from = None  # (the hand as it was when the last card left it: not what this test is about)
        self.assertEqual(bot._combat(state)[0][0], "play_card")  # the third card of three, potion or no potion
        bot._after_success(state, "play_card", {"card_index": 0})
        state["player"]["status"][0]["amount"] = 3
        bot._played_from = None
        self.assertEqual(bot._combat(state), [])  # (one more look before ending a turn with energy left)
        self.assertEqual(bot._combat(state)[0][0], "end_turn")

    def test_a_choice_in_a_fight_is_told_how_its_turns_went(self):
        # (The Knowledge Demon's offers: what Sloth or an energy less would cost shows in the turns played so far.)
        bot = make_bot(FakeGame())
        state = combat_state([STRIKE(0), DEFEND(1)], [enemy("KNOWLEDGE_DEMON_0", 300, attack=17)], kind="boss")
        for rnd, (cards_played, left) in enumerate([(4, 0), (5, 2)], start=1):
            state["battle"]["round"], state["player"]["energy"] = rnd, 3
            bot._combat(state)
            for _ in range(cards_played):
                bot._after_success(state, "play_card", {"card_index": 0})
            state["player"]["energy"], bot._played_from = left, None
            bot._combat(state)
        seen = {}
        offer = {"state_type": "card_select", "run": state["run"], "player": state["player"], "card_select": {"screen_type": "choose", "cards": []}}
        with mock.patch.object(strategy, "card_select", lambda s, mem: seen.update(s) or []):
            bot.candidates(offer)
            self.assertEqual(seen["_turns"], [(4, 0), (5, 2)])
            seen.clear()
            bot.candidates({**offer, "run": {"act": 1, "floor": 9}})  # some other room's screen: no fight to speak of
            self.assertNotIn("_turns", seen)
        state["run"] = {"act": 1, "floor": 4, "ascension": 0}  # the next fight starts from nothing
        state["battle"]["round"] = 1
        bot._combat(state)
        self.assertEqual(bot._turns, [])

    def test_what_each_enemy_has_done_so_far_is_remembered_for_the_fight(self):
        bot = make_bot(FakeGame())
        state = combat_state([STRIKE(0)], [enemy("BUG_0", 46, attack=15), enemy("BEETLE_0", 86)])
        bot._combat(state)
        bot._combat(state)  # the same turn seen again counts once
        self.assertEqual(bot._threat, {"BUG_0": [15, 1, 15.0, 1], "BEETLE_0": [0, 1, 0.0, 0]})
        state["battle"]["round"] = 2
        state["battle"]["enemies"] = [enemy("BUG_0", 46), enemy("BEETLE_0", 86, attack=16)]
        bot._combat(state)
        self.assertEqual(bot._threat, {"BUG_0": [15, 2, 15.0, 1], "BEETLE_0": [16, 2, 16.0, 1]})  # (damage, rounds, blows, turns attacked)
        state["run"]["floor"] = 4  # the next fight starts from nothing
        state["battle"]["round"] = 1
        bot._combat(state)
        self.assertEqual(bot._threat, {"BUG_0": [0, 1, 0.0, 0], "BEETLE_0": [16, 1, 16.0, 1]})

    def test_a_purchase_that_never_happens_is_given_up(self):
        """The shop said yes to a potion a relic forbids; the bot must leave in the end."""

        class DeafShop:
            buys, left = 0, False

            def state(self):
                if self.left:
                    return {"state_type": "map", "map": {"next_options": [], "nodes": []}}
                items = [{"index": 0, "category": "relic", "price": 50, "is_stocked": True, "can_afford": True}]
                return {"state_type": "shop", "shop": {"items": items, "can_proceed": True}, "player": {"hp": 50, "max_hp": 80, "gold": 300}, "run": {"act": 1, "floor": 5}}

            def act(self, action, **params):
                self.buys += action == "shop_purchase"
                self.left = self.left or action == "proceed"
                return {"status": "ok"}

        game = DeafShop()
        bot = make_bot(game)
        for _ in range(40):
            bot.step()
            if game.left:
                break
        self.assertTrue(game.left)
        self.assertLessEqual(game.buys, 2)

    def test_the_game_speed_is_set_again_as_the_fight_goes_on(self):
        """The game resets its own time scale in boss fights; set once, x3 was gone for the session."""

        class Idle:
            scales = []

            def state(self):
                return {"state_type": "unknown"}

            def act(self, action, **params):
                if action == "set_time_scale":
                    self.scales.append(params["scale"])
                return {"status": "ok"}

        game = Idle()
        bot = make_bot(game)
        bot.time_scale = 3.0
        for _ in range(60):
            bot.step()
        self.assertGreater(len(game.scales), 1)
        self.assertEqual(set(game.scales), {3.0})

    def test_plays_several_runs_back_to_back(self):
        game = FakeGame(seed=1, floors=4)
        with tempfile.TemporaryDirectory() as d:
            bot = make_bot(game, log_dir=Path(d))
            bot.run(max_runs=3)
            lines = [json.loads(line) for line in (Path(d) / "runs.jsonl").read_text().splitlines()]
        self.assertEqual(bot.runs_finished, 3)
        self.assertEqual(game.runs_started, 3)
        self.assertEqual(game.actions[-1], ("menu_select", {"option": "main_menu"}))
        self.assertEqual(len(lines), 3)
        self.assertTrue(all(line["result"] == "win" and line["floor"] >= 4 for line in lines))
        self.assertEqual(game.screen, "main")
        names = {a for a, _ in game.actions}
        self.assertLessEqual({"play_card", "end_turn", "claim_reward", "select_card_reward", "choose_map_node", "choose_rest_option"}, names)

    def test_skipped_card_reward_does_not_loop(self):
        game = FakeGame(seed=2, floors=3)
        with mock.patch.object(decks, "SKIP_THRESHOLD", 99.0):  # (the bar for a card reward: skip everything)
            make_bot(game).run(max_runs=1)
        skips = sum(1 for a, _ in game.actions if a == "skip_card_reward")
        offered = sum(1 for a, p in game.actions if (a, p) == ("claim_reward", {"index": 1}))
        self.assertGreaterEqual(skips, 1)
        self.assertEqual(skips, offered)  # each card reward opened once, never re-opened
        self.assertEqual(len(game.deck), 10)

    def test_loses_a_run_and_starts_again(self):
        bot = make_bot(FakeGame(seed=3, floors=50, enemy_hp=70, enemy_attack=22))
        bot.run(max_runs=2)
        self.assertEqual((bot.runs_finished, bot.wins), (2, 0))

    def test_resumes_a_saved_run_from_the_menu(self):
        game, bot = restarted(seed=4, floors=3)
        bot.run(max_runs=1)
        self.assertEqual(game.runs_started, 1)
        self.assertIn(("menu_select", {"option": "continue"}), game.actions)

    def test_abandons_when_told_to(self):
        game, bot = restarted(seed=5)
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
        class Mod:
            def __init__(self, reply):
                self.reply, self.calls = reply, []

            def act(self, action, **params):
                self.calls.append((action, params))
                return self.reply

        new = Mod({"status": "ok", "fast_mode": "instant"})
        self.assertTrue(supervisor.set_fast_mode(new, "instant"))
        self.assertEqual(new.calls, [("set_fast_mode", {"mode": "instant"})])
        with self.assertLogs("sts2bot.supervisor", level="WARNING"):
            self.assertFalse(supervisor.set_fast_mode(Mod({"status": "error", "error": "Unknown action"}), "instant"))


class CrashRecovery(unittest.TestCase):
    def test_a_crashed_child_is_not_counted_as_running(self):
        child = subprocess.Popen([sys.executable, "-c", "pass"])
        time.sleep(0.3)  # exited, not yet collected: a zombie
        supervisor._children.append(child)
        with mock.patch.object(supervisor.subprocess, "run", return_value=mock.Mock(stdout="Z\n")):
            self.assertFalse(supervisor.game_running())
        self.assertNotIn(child, supervisor._children)
        with mock.patch.object(supervisor.subprocess, "run", return_value=mock.Mock(stdout="Sl\n")):
            self.assertTrue(supervisor.game_running())


class LiveGameLessons(unittest.TestCase):
    """Things the first sessions against the real game got wrong."""

    def test_card_text_is_read_as_the_game_means_it(self):
        # Energy is written as icons; what a card does later is not done now; Weak and Vulnerable can come together.
        for c, fields, expected in [
                (card(0, "OFFERING", "Offering", "Skill", 0, f"Lose 6 HP. Gain {ICON}{ICON}. Draw 3 cards. Exhaust."), ("energy_gain", "draw", "hp_loss"), (2, 3, 6)),
                (card(0, "JUGGERNAUT", "Juggernaut", "Power", 2, "Whenever you gain Block, deal 6 damage to a random enemy."), ("damage",), (0,)),
                (card(0, "FLAME_BARRIER", "Flame Barrier", "Skill", 2, "Gain 12 Block. Whenever you are attacked this turn, deal 4 damage back."), ("block", "damage"), (12, 0)),
                (card(0, "THE_BOMB", "The Bomb", "Skill", 2, "At the end of 3 turns, deal 40 damage to ALL enemies."), ("damage",), (0,)),
                (card(0, "SHOCKWAVE", "Shockwave", "Skill", 2, "Apply 3 Weak and Vulnerable to ALL enemies. Exhaust."), ("weak", "vulnerable", "aoe"), (3, 3, True))]:
            with self.subTest(c["name"]):
                info = parse(c)
                self.assertEqual(tuple(getattr(info, f) for f in fields), expected)

    def test_strength_this_turn_adds_to_later_attacks(self):
        setup = card(0, "SETUP_STRIKE", "Setup Strike", "Attack", 1, "Deal 7 damage. Gain 3 Strength this turn.", "AnyEnemy")
        foes = combat.read_enemies(combat_state([], [enemy("A", 100)]))
        ctx = dict(energy=3, stars=0, hp=80, max_hp=80, block=0)
        with_setup = combat.simulate([parse(setup), parse(STRIKE(1)), parse(STRIKE(2))], foes, **ctx).score
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
        self.assertEqual(combat.choose_card(combat_state([shackles], [foe], energy=0, hp=20)), ("play_card", {"card_index": 0, "target": "A"}))

    def test_never_trades_max_hp_away(self):
        def best(*options):
            state = {"state_type": "event", "event": {"options": [{"index": i, "title": t, "description": d} for i, (t, d) in enumerate(options)]}, "player": {"hp": 70, "max_hp": 80}}
            return strategy.event(state, Memory())[0][1]["index"]

        self.assertEqual(best(("Lose Everything", "???"), ("Give Up", "Stop reading the text and leave.")), 1)
        self.assertEqual(best(("Continue", "Lose 24 Max HP. Upgrade 2 random cards."), ("Give Up", "Leave.")), 1)
        self.assertEqual(best(("Decipher", "Lose 3 Max HP. Upgrade a random card."), ("Give Up", "Leave.")), 0)

    def test_an_event_that_keeps_costing_hp_is_left_after_a_few_bites(self):
        # Slippery Bridge: "Hold On" costs 3, 4, 5, ... HP each time. Priced one bite at a time it always looked cheaper than
        # losing a card, and one run lost 42 HP there.
        mem, hp, picks = Memory(), 71, []
        for loss in (3, 4, 5):
            opts = [{"index": 0, "title": "Overcome", "description": "Shining Strike is removed from your Deck."},
                    {"index": 1, "title": "Hold On", "description": f"Lose {loss} HP. The card in the above option is randomized."}]
            state = {"state_type": "event", "event": {"event_id": "SLIPPERY_BRIDGE", "options": opts}, "player": {"hp": hp, "max_hp": 75}}
            picks.append(strategy.event(state, mem)[0][1]["index"])
            if picks[-1] == 1:
                hp -= loss
        self.assertEqual(picks, [1, 1, 0])

    def test_bridge_event_gives_up_a_starter_card_not_a_good_one(self):
        hold = event_value("Hold On", "Lose 5 HP. The card in the above option is randomized.")
        self.assertGreater(event_value("Overcome", "Defend is removed from your Deck."), hold)
        self.assertLess(event_value("Overcome", "Demon Form+ is removed from your Deck."), hold)

    def test_one_good_potion_beats_three_foul_ones(self):
        self.assertGreater(event_value("Ransack", "Procure 1 random Uncommon Potion."), event_value("Grab Potions", "Procure 3 Foul Potions."))

    def test_the_lantern_key_is_given_back_for_gold(self):
        self.assertGreater(event_value("Return the Key", "Gain 100 Gold."), event_value("Keep the Key", "Fight to obtain the Key."))

    def test_an_event_never_doubles_the_deck_or_hands_out_a_curse_as_a_relic(self):
        # Reflections: "Receive Bad Luck" was scored as a relic and "Duplicate your Deck" went unpenalised; one run doubled its deck to 71.
        shatter = event_value("Shatter", "Duplicate your Deck. Receive Bad Luck.", 69, 105)
        mirror = event_value("Touch a Mirror", "Downgrade 2 random cards. Upgrade 4 random cards.", 69, 105)
        self.assertLess(shatter, 0)
        self.assertGreater(mirror, shatter)
        self.assertGreater(event_value("Obtain the Spoils Map", "A relic.", 69, 105), 3)  # a real relic by name still counts

    def test_a_curse_costs_what_it_does_and_a_relic_is_worth_what_it_gives(self):
        # Clumsy is one dead draw a fight: a relic is worth it (refused 45 times in 45 for 6 HP and 52 Gold).
        self.assertGreater(event_value("That", "Add Clumsy to your Deck. Obtain a random Relic."), event_value("This", "Lose 6 HP. Gain 52 Gold."))
        self.assertLess(event_value("Resist", "Remove 2 cards from your Deck. Add Normality to your Deck."), 0)
        # Royal Poison takes 4 HP at the start of every combat: not for a heal we hardly need, only for our life.
        tea = lambda hp: event_value("Enjoy Your Tea", "Obtain Royal Poison. Heal to full HP.", hp)
        fight = lambda hp: event_value("Pick a Fight", "Lose 11 HP. Obtain a random Relic.", hp)
        self.assertLess(tea(70), fight(70))
        self.assertGreater(tea(12), fight(12))
        # The Sword of Stone needs five elites killed, and the bot meets about one a run.
        self.assertLess(event_value("Grab the Sword", "Obtain the Sword of Stone.", 80), event_value("Dive into the Water", "Gain 115 Gold. Lose 7 HP.", 80))
        # The Punch Off fight (two constructs that took 16, 37 and 44 HP, one a death) costs its HP: a relic with an Injury (Nab)
        # is taken instead, even at 60 of 80.
        self.assertLess(event_value("I Can Take Them", "Fight them for Greater Rewards."), event_value("Nab", "Add Injury to your Deck. Obtain a random Relic."))

    def test_a_heal_gold_and_max_hp_are_worth_their_amounts(self):
        value = lambda text, hp=60: event_value("", text, hp)
        self.assertGreater(value("Heal 21 HP.", 50), value("Heal 21 HP.", 75))
        self.assertAlmostEqual(value("Heal to full HP.", 80), value("Leave."))
        self.assertGreater(value("Gain 311 Gold."), value("Gain 62 Gold.") + 2)
        self.assertGreater(value("Heal 10 HP.", 53), value("Gain 2 Max HP. Take 3 damage.", 53))
        self.assertGreater(value("Gain 10 Max HP."), value("Gain 2 Max HP."))

    def test_picks_the_mildest_boss_affliction(self):
        cards_ = [card(0, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 6 damage."),
                  card(1, "SLOTH", "Sloth", "Status", 0, "You cannot play more than 3 cards each turn."), card(2, "WASTE_AWAY", "Waste Away", "Status", 0, "Gain 1 less energy per turn.")]
        state = {"state_type": "card_select", "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": cards_}}
        self.assertEqual(strategy.card_select(state, Memory())[0], ("select_card", {"index": 1}))

    def test_unreadable_cards_are_not_taken_for_their_rarity(self):
        odd = card(0, "NEW_THING", "New Thing", "Skill", 1, "Do something the bot has never seen.", rarity="Rare")
        state = {"state_type": "card_reward", "run": {"act": 2}, "card_reward": {"cards": [odd], "can_skip": True}}
        self.assertEqual(strategy.card_reward(state, Memory())[0][0], "skip_card_reward")

    def test_a_card_offered_in_a_fight_is_always_taken_and_is_no_pick(self):
        # (2026-11-25: an Attack Potion on round 1 of Donu and Deca, its three cards skipped as a reward below the bar.)
        odd = card(0, "NEW_THING", "New Thing", "Skill", 1, "Do something the bot has never seen.", rarity="Rare")
        state = {"state_type": "card_reward", "run": {"act": 2}, "card_reward": {"cards": [odd], "can_skip": True, "in_combat": True}}
        self.assertEqual(strategy.card_reward(state, Memory())[0], ("select_card_reward", {"card_index": 0}))
        bot = make_bot(FakeGame())
        bot._after_success({**state, "card_reward": {**state["card_reward"], "cards": [dict(odd, index=0)]}}, "select_card_reward", {"card_index": 0})
        self.assertEqual((bot.mem.deck, bot.mem.picks), ([], []))

    def test_a_burrowed_enemy_is_dug_out_instead_of_blocked_forever(self):
        burrowed = [{"id": "BURROWED_POWER", "amount": 1}]
        hand = [DEFEND(0), DEFEND(1), DEFEND(2), card(3, "BASH", "Bash", "Attack", 2, "Deal 11 damage. Apply 2 Vulnerable.", "AnyEnemy")]
        state = combat_state(hand, [enemy("T", 62, attack=24, block=20, status=burrowed)], hp=57)
        state["battle"]["round"] = 25
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 3, "target": "T"}))
        # Breaking the last of its Block stuns it, so that comes before blocking.
        state = combat_state(hand, [enemy("T", 62, attack=24, block=9, status=burrowed)], hp=57)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 3, "target": "T"}))

    def test_escapes_the_sandpit_before_it_closes(self):
        escape = card(0, "FRANTIC_ESCAPE", "Frantic Escape", "Status", 1, "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.")
        boss = enemy("INS", 200, attack=20, status=[{"id": "SANDPIT_POWER", "amount": 1}])
        self.assertEqual(combat.choose_card(combat_state([escape, DEFEND(1), DEFEND(2), DEFEND(3)], [boss], kind="boss")), ("play_card", {"card_index": 0}))

    def test_kills_the_minion_that_does_all_the_damage(self):
        minion = enemy("TORCH", 199, attack=28, status=[{"id": "MINION_POWER", "amount": 1}])
        state = combat_state([STRIKE(0)], [minion, enemy("QUEEN", 400)], kind="boss", energy=1)
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
        self.assertEqual([(e.entity_id, e.behind) for e in combat.read_enemies(state)], [("CRUSHER", False), ("ROCKET", True)])

    def test_hard_to_kill_caps_each_hit(self):
        big = card(0, "BLUDGEON", "Bludgeon", "Attack", 3, "Deal 32 damage.", "AnyEnemy")
        twin = card(1, "TWIN_STRIKE", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy")
        state = combat_state([big, twin, STRIKE(2), STRIKE(3)], [enemy("EXO", 28, status=[{"id": "HARD_TO_KILL_POWER", "amount": 9}])])
        self.assertNotEqual(combat.choose_card(state)[1]["card_index"], 0)

    def test_block_potion_waits_for_a_big_hit(self):
        potion = {"slot": 0, "id": "BLOCK_POTION", "name": "Block Potion", "description": "Gain 12 Block.", "target_type": "Self"}
        calm = combat_state([STRIKE(0)], [enemy("BOSS", 300)], kind="boss", potions=[potion])
        self.assertNotIn("use_potion", [a for a, _ in combat.actions(calm)])
        storm = combat_state([STRIKE(0)], [enemy("BOSS", 300, attack=30)], kind="boss", potions=[potion])
        self.assertEqual(combat.actions(storm)[0], ("use_potion", {"slot": 0}))

    def test_only_one_bound_card_is_planned_per_turn(self):
        bound = {"keywords": [{"name": "Bound", "description": "Only 1 Bound card can be played each turn."}]}
        hand = [card(0, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self", **bound), card(1, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self", **bound),
                card(2, "IMPERVIOUS", "Impervious", "Skill", 2, "Gain 30 Block. Exhaust.", "Self", **bound)]
        # Without the rule the plan would be Defend + Impervious; the first Defend played would then lock Impervious out and the hit would land.
        self.assertEqual(combat.choose_card(combat_state(hand, [enemy("A", 200, attack=30)], hp=40)), ("play_card", {"card_index": 2}))

    def test_two_cards_together_kill_the_reviving_attacker(self):
        ghost = enemy("PARAFRIGHT", 21, attack=16, status=[{"id": "ILLUSION_POWER", "amount": 1}, {"id": "MINION_POWER", "amount": 1}])
        bash = card(0, "BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy")
        twin = card(1, "TWIN_STRIKE", "Twin Strike+", "Attack", 1, "Deal 7 damage twice.", "AnyEnemy")
        action, params = combat.choose_card(combat_state([bash, twin, DEFEND(2)], [ghost, enemy("OBSCURA", 101, attack=10)], hp=18))
        self.assertEqual((action, params.get("target")), ("play_card", "PARAFRIGHT"))

    def test_a_win_is_read_from_the_games_own_counter(self):
        game = Wins(4)
        bot = quiet_bot(game)
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

    def test_a_win_the_game_has_not_written_down_yet_is_waited_for(self):
        bot = quiet_bot(Wins(2, late=5))
        bot._refresh_ascensions()
        with self.assertLogs("sts2bot", level="INFO") as logs:
            bot._record_run({"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 3, "max_hp": 83}})
        self.assertIn("win", logs.output[-1])
        self.assertEqual(bot.wins, 1)

    def test_a_death_straight_out_of_a_fight_is_not_waited_on(self):
        # Every act 3 death waited 4 s for a win that could not come (4.1 s from the last play; wins take 1-2 s).
        game, slept = Wins(2), []
        bot = quiet_bot(game, slept.append)
        bot._refresh_ascensions()
        over = {"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 0, "max_hp": 80}}
        bot._note_state({"state_type": "boss", "run": {"act": 3, "floor": 48}, "player": {"hp": 4, "max_hp": 80}, "battle": {"round": 9, "enemies": []}})
        with self.assertLogs("sts2bot", level="INFO") as logs:
            bot._note_state(over)
        self.assertTrue(any("run 1 over: loss" in line for line in logs.output), logs.output)
        self.assertEqual(slept, [])  # no waiting on the counter
        # ... while one that leaves its fight first (a won last boss) is still waited for.
        bot = quiet_bot(game, slept.append)
        bot._refresh_ascensions()
        bot._note_state({"state_type": "rewards", "run": {"act": 3, "floor": 48}, "player": {"hp": 30, "max_hp": 80}})
        with self.assertLogs("sts2bot", level="INFO"):
            bot._note_state(over)
        self.assertEqual(len(slept), 8)

    def test_with_the_timeline_complete_a_death_skips_the_summary(self):
        over = {"state_type": "game_over", "game_over": {"options": ["continue", "force_main_menu"]}}
        mem = Memory(epochs={"NEOW_EPOCH": "Revealed", "FOOL1_EPOCH": "Revealed"}, last_result="loss")
        self.assertEqual(strategy.game_over(over, mem), [("menu_select", {"option": "force_main_menu"})])
        mem.last_result = "win"
        self.assertEqual(strategy.game_over(over, mem), [("menu_select", {"option": "continue"})])  # (a win: the summary)
        mem.last_result, mem.epochs["FOOL2_EPOCH"] = "loss", "NotObtained"
        self.assertEqual(strategy.game_over(over, mem), [("menu_select", {"option": "continue"})])  # (score still buys epochs)

    def test_a_bot_started_mid_run_still_sees_that_run_won(self):
        game = Wins(3)
        bot = quiet_bot(game)
        bot._note_state({"state_type": "map", "run": {"act": 3, "floor": 40}, "player": {"hp": 50, "max_hp": 80}})
        game.wins = 4
        with self.assertLogs("sts2bot", level="INFO") as logs:
            bot._note_state({"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 0, "max_hp": 80}})
        self.assertTrue(any("run 1 over: win" in line for line in logs.output), logs.output)

    def test_reveals_unlocks_on_the_timeline_before_starting_a_run(self):
        mem = Memory()
        main = {"state_type": "menu", "menu_screen": "main", "options": ["timeline", "settings", "quit"], "pending_epoch_ids": ["NEOW_EPOCH"]}
        self.assertEqual(strategy.menu(main, mem, "auto")[0], ("menu_select", {"option": "timeline"}))
        timeline = {"state_type": "menu", "menu_screen": "timeline", "options": [{"name": "advance", "enabled": True}, {"name": "back", "enabled": True}]}
        self.assertEqual(strategy.menu(timeline, mem, "auto")[0], ("menu_select", {"option": "advance"}))

        class Game:
            def state(self):
                return timeline

            def act(self, action, **params):
                return {"status": "ok", "message": "No more epochs to advance", "done": True}

        bot = make_bot(Game())
        for _ in range(3):
            bot.step()  # the mod says nothing is left, three times running ...
        self.assertEqual(strategy.menu(timeline, bot.mem, "auto")[0], ("menu_select", {"option": "back"}))
        # ... so back at the menu the bot plays instead of going round again.
        main = {"state_type": "menu", "menu_screen": "main", "options": ["singleplayer", "timeline", "quit"]}
        self.assertEqual(strategy.menu(main, bot.mem, "auto")[0], ("menu_select", {"option": "singleplayer"}))
        # A run in progress comes first; the Timeline can wait until it is over.
        busy = {"state_type": "menu", "menu_screen": "main", "options": ["continue", "abandon_run", "timeline"], "pending_epoch_ids": ["SILENT2_EPOCH"]}
        self.assertEqual(strategy.menu(busy, Memory(), "auto")[0], ("menu_select", {"option": "continue"}))

    def test_choose_screen_click_is_retried_quickly(self):
        """The first click on a "choose 1 of 3" screen is often ignored by the game."""

        class Game:
            clicks = 0

            def state(self):
                done = self.clicks >= 2
                return {"state_type": "map" if done else "card_select", "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": [] if done else [STRIKE(0)]}}

            def act(self, action, **params):
                self.clicks += 1
                return {"status": "ok", "message": "Choosing card"}

        game, clock = Game(), FakeClock()
        bot = Bot(game, clock=clock.now, sleep=clock.sleep)
        bot.step()
        bot.step()
        self.assertEqual(game.clicks, 2)
        self.assertLess(clock.t, 1.5)


class CrashLoop(unittest.TestCase):
    def test_repeated_crashes_in_one_place_slow_the_bot_then_abandon_the_run(self):
        bot, sup = crashing((3, 1, 6), careful_until=0.0)
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

    def test_while_there_is_something_else_to_try_the_run_is_not_given_up(self):
        bot, sup = crashing((3, 3, 46), **{"note_crash.return_value": True})  # each time the bot had asked for something it can now leave alone
        sup._note_crash()
        for _ in range(3):
            with self.assertLogs("sts2bot.supervisor", level="WARNING"):
                sup._note_crash()
            self.assertFalse(bot.abandon_next)
        self.assertEqual(bot.careful_at, (3, 3, 46))  # slowly, and only there
        with self.assertLogs("sts2bot.supervisor", level="WARNING"):
            sup._note_crash()  # the fifth crash in one room: whatever was tried, it is not the bot
        self.assertTrue(bot.abandon_next)

    def test_what_was_asked_when_the_game_died_is_not_asked_again_in_that_room(self):
        class Shop(FakeGame):
            """A shop with two things to buy, where buying the first kills the game."""

            def state(self):
                items = [{"index": i, "category": "relic", "price": 50, "is_stocked": True, "can_afford": True} for i in (0, 1)]
                return {"state_type": "shop", "run": {"act": 3, "floor": 46}, "player": {"hp": 50, "max_hp": 80, "gold": 300, "potions": []},
                        "shop": {"items": items, "can_proceed": True}}

            def act(self, action, **params):
                self.actions.append((action, params))
                return {"status": "ok", "message": "ok"}

        game = Shop()
        bot = make_bot(game)
        bot.step()
        first = game.actions[-1]
        self.assertEqual(first[0], "shop_purchase")
        with self.assertLogs("sts2bot", level="WARNING"):
            self.assertTrue(bot.note_crash())  # the game died: that purchase is the suspect
        self.assertFalse(bot.note_crash())  # (nothing more was asked since)
        bot._strikes.clear()
        bot.step()
        self.assertNotEqual(game.actions[-1], first)  # back in the same shop, it buys the other thing
        # Slow play is for the room that crashed, not for the ones after it.
        bot.careful_until, bot.careful_at = bot.clock() + 120.0, (0, 3, 46)
        bot._last_where = (3, 46)
        self.assertTrue(bot._careful())
        bot._last_where = (3, 47)
        self.assertFalse(bot._careful())


class SteamUpkeep(unittest.TestCase):
    """The game started directly needs the Steam client; the supervisor starts it."""

    def test_steam_is_started_before_the_game_when_it_is_not_running(self):
        events, up = [], {"steam": False}

        def popen(argv, **kw):
            events.append(Path(argv[0]).name)
            up["steam"] = up["steam"] or Path(argv[0]).name == "steam"
            return mock.Mock(poll=lambda: None)

        with mock.patch.object(supervisor, "_process_running", lambda name: up["steam"]), mock.patch.object(supervisor.shutil, "which", return_value="/usr/bin/steam"), \
                mock.patch.object(supervisor.subprocess, "Popen", popen), mock.patch.object(supervisor.time, "sleep"), mock.patch.object(supervisor, "_children", []), \
                mock.patch.dict(supervisor.os.environ):
            supervisor.os.environ.pop("INVOCATION_ID", None)  # not under systemd: started directly
            supervisor.launch_game("/games/sts2/SlayTheSpire2")
            self.assertEqual(events, ["steam", "SlayTheSpire2"])
            supervisor.launch_game("/games/sts2/SlayTheSpire2")  # Steam is up now: not started twice
            self.assertEqual(events, ["steam", "SlayTheSpire2", "SlayTheSpire2"])

    def test_under_systemd_the_game_gets_a_scope_of_its_own(self):
        with mock.patch.dict(supervisor.os.environ, {"INVOCATION_ID": "abc"}), mock.patch.object(supervisor.shutil, "which", return_value="/usr/bin/systemd-run"):
            self.assertEqual(supervisor._detached(["game", "-x"])[:3], ["systemd-run", "--user", "--scope"])
            self.assertEqual(supervisor._detached(["game", "-x"])[-2:], ["game", "-x"])
        with mock.patch.dict(supervisor.os.environ):
            supervisor.os.environ.pop("INVOCATION_ID", None)
            self.assertEqual(supervisor._detached(["game"]), ["game"])


class ModUpkeep(unittest.TestCase):
    """A game update breaks the bridge mod; the supervisor rebuilds it by itself."""

    def test_load_failure_is_read_from_the_game_log(self):
        with tempfile.TemporaryDirectory() as d, mock.patch.object(supervisor, "GAME_LOG", Path(d) / "godot.log"):
            logf = Path(d) / "godot.log"
            self.assertIsNone(supervisor.mod_load_error())
            logf.write_text("[INFO] Loading assembly DLL mods/STS2_MCP.dll\n[INFO]  --- RUNNING MODDED! --- Loaded 1 mods\n")
            self.assertIsNone(supervisor.mod_load_error())
            logf.write_text("[ERROR] Exception thrown while loading mod STS2_MCP: System.Reflection.ReflectionTypeLoadException: x\n"
                            "   at System.Reflection.RuntimeModule.GetTypes()\nSystem.TypeLoadException: Could not load type 'A.LobbyPlayer' from assembly 'sts2'.\n")
            self.assertIn("LobbyPlayer", supervisor.mod_load_error())
            # A log from before this launch says nothing about it.
            os.utime(logf, (time.time() - 100, time.time() - 100))
            self.assertIsNone(supervisor.mod_load_error(since=time.time() - 10))

    def test_mod_is_stale_when_the_game_files_change(self):
        with tempfile.TemporaryDirectory() as d:
            stamp = Path(d) / "stamp"
            with mock.patch.object(supervisor, "MOD_STAMP", stamp), mock.patch.object(supervisor, "game_fingerprint", return_value="v1"):
                self.assertTrue(supervisor.mod_is_stale())  # never built by the bot
                stamp.write_text("v1\n")
                self.assertFalse(supervisor.mod_is_stale())
            with mock.patch.object(supervisor, "MOD_STAMP", stamp), mock.patch.object(supervisor, "game_fingerprint", return_value="v2"):
                self.assertTrue(supervisor.mod_is_stale())

    def test_rebuilds_and_relaunches_when_the_game_rejects_the_mod(self):
        class Api:
            up = False

            def ping(self):
                return self.up

        api, events, state = Api(), [], {"error": "Could not load type X"}

        def rebuild():
            events.append("rebuild")
            state["error"] = None
            return True

        def launch(cmd):
            events.append("launch")
            api.up = state["error"] is None

        sup = supervisor.Supervisor(mock.Mock(), api, launch_cmd="game", boot_timeout=0.05)
        with mock.patch.multiple(supervisor, game_running=lambda: False, mod_is_stale=lambda: False, mod_load_error=lambda since=0.0: state["error"],
                                 rebuild_mod=rebuild, launch_game=launch, kill_game=lambda grace=10.0: events.append("kill")), \
                mock.patch.object(supervisor.time, "sleep"), self.assertLogs("sts2bot.supervisor", level="ERROR"):
            sup.ensure_game()
        self.assertEqual(events, ["launch", "kill", "rebuild", "launch"])

    def test_rebuilds_before_launch_after_a_game_update(self):
        api, events = mock.Mock(), []
        api.ping.side_effect = [False, True]
        sup = supervisor.Supervisor(mock.Mock(), api, launch_cmd="game")
        with mock.patch.multiple(supervisor, game_running=lambda: False, mod_is_stale=lambda: True, mod_load_error=lambda since=0.0: None,
                                 rebuild_mod=lambda: events.append("rebuild") or True, launch_game=lambda cmd: events.append("launch")), \
                self.assertLogs("sts2bot.supervisor", level="WARNING"):
            sup.ensure_game()
        self.assertEqual(events, ["rebuild", "launch"])


class HttpClient(unittest.TestCase):
    """The real Client against a local HTTP server that fronts the fake game."""

    def test_round_trip_and_errors(self):
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

    def test_chunked_answers_are_read_whole(self):
        self.assertEqual(_dechunk(b"4\r\n{\"a\"\r\n5;x=1\r\n: 12}\r\n0\r\n\r\n"), b'{"a": 12}')
