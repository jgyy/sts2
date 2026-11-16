import json
import tempfile
import unittest
from pathlib import Path

from sts2bot import combat, strategy
from sts2bot.bot import Bot, Stuck
from sts2bot.cards import deck_value, parse
from sts2bot.strategy import Memory

from .fake_game import FakeGame, card


class FakeClock:
    def __init__(self):
        self.t = 0.0

    def now(self):
        return self.t

    def sleep(self, s):
        self.t += s


def make_bot(game, **kw):
    clock = FakeClock()
    return Bot(game, clock=clock.now, sleep=clock.sleep, **kw)


def combat_state(hand, enemies, *, energy=3, hp=80, max_hp=80, block=0, kind="monster", potions=()):
    return {
        "state_type": kind,
        "run": {"act": 1, "floor": 3, "ascension": 0},
        "battle": {"round": 1, "turn": "player", "is_play_phase": True, "enemies": enemies},
        "player": {
            "hp": hp,
            "max_hp": max_hp,
            "block": block,
            "energy": energy,
            "hand": hand,
            "potions": list(potions),
            "draw_pile_count": 5,
            "discard_pile_count": 0,
            "exhaust_pile_count": 0,
        },
    }


def enemy(eid, hp, attack=None, block=0, status=()):
    intents = [{"type": "Attack", "label": str(attack)}] if attack else [{"type": "Buff", "label": ""}]
    return {"entity_id": eid, "hp": hp, "max_hp": hp, "block": block, "status": list(status), "intents": intents}


STRIKE = lambda i: card(i, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy")
DEFEND = lambda i: card(i, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self")


class CardParsing(unittest.TestCase):
    def test_multi_hit_and_debuffs(self):
        c = parse(card(0, "X", "X", "Attack", 2, "Deal 5 damage 3 times. Apply 2 Vulnerable.", "AnyEnemy"))
        self.assertEqual((c.damage, c.hits, c.vulnerable, c.cost, c.needs_target), (5, 3, 2, 2, True))

    def test_x_cost_and_block(self):
        c = parse({"index": 1, "type": "Skill", "cost": "X", "description": "Gain 7 Block. Draw 2 cards."})
        self.assertTrue(c.is_x)
        self.assertEqual((c.block, c.draw), (7, 2))

    def test_intent_damage(self):
        self.assertEqual(combat.intent_damage({"type": "Attack", "label": "5x3"}), 15)
        self.assertEqual(combat.intent_damage({"type": "AttackDebuff", "label": "11"}), 11)
        self.assertEqual(combat.intent_damage({"type": "Buff", "label": "3"}), 0)

    def test_deck_value_ordering(self):
        strike = card(0, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", rarity="Basic")
        good = card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage. Apply 1 Weak. Apply 1 Vulnerable.", rarity="Uncommon")
        curse = card(0, "REGRET", "Regret", "Curse", 0, "Unplayable.")
        self.assertGreater(deck_value(good), deck_value(strike))
        self.assertGreater(deck_value(strike), deck_value(curse))


class CombatPlanning(unittest.TestCase):
    def test_takes_lethal_instead_of_blocking(self):
        s = combat_state([DEFEND(0), STRIKE(1), STRIKE(2)], [enemy("A_0", 12, attack=20)], hp=30)
        self.assertEqual(combat.choose_card(s)[1], {"card_index": 1, "target": "A_0"})

    def test_blocks_when_attack_would_kill(self):
        s = combat_state([STRIKE(0), DEFEND(1), DEFEND(2), DEFEND(3)], [enemy("A_0", 60, attack=14)], hp=12, max_hp=80)
        action, params = combat.choose_card(s)
        self.assertEqual(params["card_index"], 1)

    def test_attacks_when_enemy_is_not_attacking(self):
        s = combat_state([DEFEND(0), STRIKE(1)], [enemy("A_0", 60)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)

    def test_kills_the_enemy_it_can_kill(self):
        s = combat_state([STRIKE(0)], [enemy("BIG_0", 50, attack=5), enemy("SMALL_0", 6, attack=10)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["target"], "SMALL_0")

    def test_vulnerable_makes_lethal_reachable(self):
        # The real game names powers like VULNERABLE_POWER.
        vuln = [{"id": "VULNERABLE_POWER", "amount": 2}]
        s = combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 9, attack=30, status=vuln)], energy=1, hp=10)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 0)

    def test_conditional_double_hit_needs_vulnerable(self):
        dismantle = card(0, "DISMANTLE", "Dismantle", "Attack", 1, "Deal 8 damage. If the enemy is Vulnerable, hits twice.", "AnyEnemy")
        info = parse(dismantle)
        self.assertEqual((info.damage, info.hits, info.double_if_vulnerable), (8, 1, True))
        vuln = [{"id": "VULNERABLE_POWER", "amount": 1}]
        # 8 * 1.5 * 2 = 24 kills the vulnerable one; the plain one would survive.
        s = combat_state([dismantle], [enemy("PLAIN_0", 20, attack=5), enemy("VULN_0", 24, attack=5, status=vuln)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["target"], "VULN_0")

    def test_blocks_at_full_health_when_it_cannot_kill(self):
        s = combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 40, attack=10)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)

    def test_does_not_waste_energy_on_status_cards(self):
        slimed = card(0, "SLIMED", "Slimed", "Status", 1, "Exhaust.", "None")
        s = combat_state([slimed, STRIKE(1)], [enemy("A_0", 40)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)

    def test_plating_counts_as_block(self):
        s = combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 40, attack=5)], energy=1)
        self.assertEqual(combat.expected_loss(s), 5)
        s["player"]["status"] = [{"id": "PLATING_POWER", "amount": 5}]
        self.assertEqual(combat.expected_loss(s), 0)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 0)  # nothing to block: attack

    def test_weak_goes_on_the_attacker_and_saves_hp(self):
        weaken = card(0, "W", "Weaken", "Skill", 1, "Apply 2 Weak.", "AnyEnemy")
        s = combat_state([weaken], [enemy("IDLE_0", 30), enemy("HITTER_0", 60, attack=20)], energy=1, hp=30)
        self.assertEqual(combat.choose_card(s), ("play_card", {"card_index": 0, "target": "HITTER_0"}))

    def test_respects_energy_and_unplayable_cards(self):
        hand = [card(0, "BIG", "Big", "Attack", 3, "Deal 30 damage.", "AnyEnemy"), {**STRIKE(1), "can_play": False}]
        s = combat_state(hand, [enemy("A_0", 60)], energy=2)
        self.assertIsNone(combat.choose_card(s))
        self.assertEqual(combat.actions(s), [("end_turn", {})])

    def test_potions_only_when_it_matters(self):
        potion = {"slot": 1, "id": "FIRE_POTION", "can_use_in_combat": True, "target_type": "AnyEnemy"}
        calm = combat_state([STRIKE(0)], [enemy("A_0", 40, attack=5)], potions=[potion])
        self.assertEqual(combat.potion_actions(calm), [])
        boss = combat_state([STRIKE(0)], [enemy("A_0", 40, attack=5)], potions=[potion], kind="boss")
        self.assertEqual(combat.potion_actions(boss), [("use_potion", {"slot": 1, "target": "A_0"})])


class Strategy(unittest.TestCase):
    def base(self, kind, hp=80, gold=100, **body):
        return {"state_type": kind, "run": {"act": 1, "floor": 5}, "player": {"hp": hp, "max_hp": 80, "gold": gold, "potions": []}, **body}

    def test_map_avoids_elite_when_hurt_and_takes_it_when_healthy(self):
        m = {
            "next_options": [{"index": 0, "col": 0, "row": 1, "type": "Elite"}, {"index": 1, "col": 1, "row": 1, "type": "Monster"}],
            "nodes": [
                {"col": 0, "row": 1, "type": "Elite", "children": []},
                {"col": 1, "row": 1, "type": "Monster", "children": []},
            ],
        }
        self.assertEqual(strategy.map_screen(self.base("map", hp=30, map=m), Memory())[0][1], {"index": 1})
        self.assertEqual(strategy.map_screen(self.base("map", hp=80, map=m), Memory())[0][1], {"index": 0})

    def test_map_looks_ahead(self):
        m = {
            "next_options": [{"index": 0, "col": 0, "row": 1, "type": "Monster"}, {"index": 1, "col": 1, "row": 1, "type": "Monster"}],
            "nodes": [
                {"col": 0, "row": 1, "type": "Monster", "children": [[0, 2]]},
                {"col": 1, "row": 1, "type": "Monster", "children": [[1, 2]]},
                {"col": 0, "row": 2, "type": "Monster", "children": []},
                {"col": 1, "row": 2, "type": "Treasure", "children": []},
            ],
        }
        self.assertEqual(strategy.map_screen(self.base("map", map=m), Memory())[0][1], {"index": 1})

    def test_rest_heals_when_low_and_smiths_when_healthy(self):
        rs = {"options": [{"index": 0, "id": "rest", "is_enabled": True}, {"index": 1, "id": "smith", "is_enabled": True}]}
        self.assertEqual(strategy.rest_site(self.base("rest_site", hp=30, rest_site=rs), Memory())[0][1], {"index": 0})
        self.assertEqual(strategy.rest_site(self.base("rest_site", hp=78, rest_site=rs), Memory())[0][1], {"index": 1})

    def test_event_avoids_fatal_and_cursed_options(self):
        ev = {
            "options": [
                {"index": 0, "title": "Grab", "description": "Obtain a relic. Lose 50 HP."},
                {"index": 1, "title": "Pray", "description": "Gain 30 gold."},
                {"index": 2, "title": "Take", "description": "Obtain a relic. Gain a Curse.", "is_locked": True},
            ]
        }
        acts = strategy.event(self.base("event", hp=20, event=ev), Memory())
        self.assertEqual(acts[0][1], {"index": 1})
        self.assertNotIn(("choose_event_option", {"index": 2}), acts)

    def test_removal_picks_curse_then_basics(self):
        cs = {
            "screen_type": "select",
            "prompt": "Choose a card to Remove.",
            "cards": [
                card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage.", rarity="Uncommon"),
                card(1, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", rarity="Basic"),
                card(2, "REGRET", "Regret", "Curse", 0, "Unplayable."),
            ],
        }
        acts = strategy.card_select(self.base("card_select", card_select=cs), Memory())
        self.assertEqual([a[1].get("index") for a in acts[:3]], [2, 1, 0])

    def test_choose_a_card_is_offered_again_after_an_ignored_click(self):
        cards = [card(0, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 7 damage.", "None"),
                 card(1, "SLOTH", "Sloth", "Status", 0, "You cannot play more than 3 cards each turn.", "None")]
        s = self.base("card_select", card_select={"screen_type": "choose", "prompt": "Choose a card.", "cards": cards})
        mem = Memory()
        first = strategy.card_select(s, mem)
        self.assertEqual({a for a, _ in first}, {"select_card"})  # never confirm_selection here
        mem.selected.update({0, 1})  # both were clicked on earlier, identical screens
        self.assertEqual(strategy.card_select(s, mem), first)

    def test_grid_selection_starts_over_when_clicks_did_not_register(self):
        cards = [STRIKE(0), DEFEND(1)]
        s = self.base("card_select", card_select={"screen_type": "select", "prompt": "Choose 2 cards.", "cards": cards})
        mem = Memory()
        strategy.card_select(s, mem)
        mem.selected.update({0, 1})
        self.assertIn(("select_card", {"index": 0}), strategy.card_select(s, mem))

    def test_shop_buys_removal_and_relic_but_not_weak_cards(self):
        shop = {
            "items": [
                {"index": 0, "category": "card", "price": 50, "is_stocked": True, "can_afford": True, "card_id": "STRIKE_R", "card_type": "Attack", "card_cost": "1", "card_rarity": "Basic", "card_description": "Deal 6 damage."},
                {"index": 1, "category": "relic", "price": 150, "is_stocked": True, "can_afford": True},
                {"index": 2, "category": "card_removal", "price": 75, "is_stocked": True, "can_afford": True},
                {"index": 3, "category": "relic", "price": 300, "is_stocked": True, "can_afford": False},
            ],
            "can_proceed": True,
        }
        acts = strategy.shop(self.base("shop", gold=250, shop=shop), Memory())
        self.assertEqual(acts, [("shop_purchase", {"index": 2}), ("shop_purchase", {"index": 1}), ("proceed", {})])

    def test_character_choice(self):
        unlocked = ["IRONCLAD", "SILENT", "REGENT"]
        self.assertEqual(strategy.choose_character("silent", unlocked, 0), "SILENT")
        self.assertEqual(strategy.choose_character("DEFECT", unlocked, 0), "IRONCLAD")
        self.assertEqual([strategy.choose_character("cycle", unlocked, n) for n in range(4)], ["IRONCLAD", "SILENT", "REGENT", "IRONCLAD"])


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
