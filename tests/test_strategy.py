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


class LearnedRatings(unittest.TestCase):
    def test_cards_that_went_far_rise_and_the_rest_fall(self):
        from sts2bot import learn

        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "card_stats.json"
            stats = learn.CardStats(path)
            for floor in (10, 17, 17, 20, 25, 33):  # what is usual for this character
                stats.record("The Silent", floor, False, [])
            self.assertEqual(stats.shift("The Regent", "Footwork"), 0.0)  # nothing known: no opinion
            fresh = stats.shift("The Silent", "Footwork")
            for _ in range(8):
                stats.record("The Silent", 45, False, [[3, "Footwork+"], [5, "Slice"]])
                stats.record("The Silent", 8, False, [[3, "Slice"]])
            self.assertGreater(stats.shift("The Silent", "Footwork"), fresh + 0.5)
            self.assertLess(stats.shift("The Silent", "Slice"), stats.shift("The Silent", "Footwork"))
            again = learn.CardStats(path)  # kept between sessions
            self.assertEqual(again.shift("The Silent", "Footwork"), stats.shift("The Silent", "Footwork"))
            self.assertLessEqual(len(again.finals["The Silent"]), learn.HISTORY)


class EndOfRun(unittest.TestCase):
    def test_goes_through_the_summary_so_the_score_is_banked(self):
        mem = Memory()
        over = lambda *options: {"state_type": "game_over", "game_over": {"options": list(options)}}
        self.assertEqual(strategy.game_over(over("continue", "force_main_menu"), mem), [("menu_select", {"option": "continue"})])
        self.assertEqual(strategy.game_over(over("force_main_menu"), mem), [])  # the score is still counting up
        self.assertEqual(strategy.game_over(over("main_menu", "force_main_menu"), mem), [("menu_select", {"option": "main_menu"})])
        for _ in range(strategy.OVER_PATIENCE):
            strategy.game_over(over("force_main_menu"), mem)
        self.assertEqual(strategy.game_over(over("force_main_menu"), mem), [("menu_select", {"option": "force_main_menu"})])
        # An older mod only knows the one option.
        self.assertEqual(strategy.game_over({"state_type": "game_over"}, Memory()), [("menu_select", {"option": "main_menu"})])


class Upgrades(unittest.TestCase):
    def test_the_upgrade_that_adds_most_is_taken(self):
        from sts2bot.cards import upgrade_gain

        strike = (card(0, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy"), {"type": "Attack", "cost": 1, "description": "Deal 9 damage."})
        big = (card(1, "DEVASTATE", "Devastate", "Attack", 1, "Deal 35 damage.", "AnyEnemy"), {"type": "Attack", "cost": 1, "description": "Deal 47 damage."})
        cheap = (card(2, "X", "Body Slam", "Attack", 1, "Deal damage equal to your Block.", "AnyEnemy"), {"type": "Attack", "cost": 0, "description": "Deal damage equal to your Block."})
        self.assertEqual(upgrade_gain(*strike), 3.0)
        self.assertEqual(upgrade_gain(*big), 12.0)
        self.assertEqual(upgrade_gain(*cheap), 6.0)  # a point of energy saved on every play
        table = {"STRIKE_R": 3.0, "DEVASTATE": 12.0, "SHRUG_IT_OFF": 3.0}
        shrug = card(3, "SHRUG_IT_OFF", "Shrug It Off", "Skill", 1, "Gain 8 Block. Draw 1 card.", "Self")
        mem = Memory(upgrade_gain=lambda c: table.get(c["id"]))
        state = {"state_type": "card_select", "run": {"act": 1, "floor": 6},
                 "card_select": {"screen_type": "upgrade", "prompt": "Choose a card to Upgrade.", "cards": [strike[0], shrug, big[0]]}}
        first = next(a for a in strategy.card_select(state, mem) if a[0] == "select_card")
        self.assertEqual(first[1]["index"], 1)  # Devastate, though Shrug It Off is the better-rated card


class Tuning(unittest.TestCase):
    def test_settings_that_do_well_get_used_more(self):
        import random
        from unittest import mock
        from sts2bot import learn

        with tempfile.TemporaryDirectory() as d, mock.patch.dict(learn.KNOB):
            knobs = learn.Knobs(Path(d) / "tuning.json", rng=random.Random(1))
            for _ in range(300):
                knobs.draw()
                knobs.record(8.0 if learn.KNOB["caution"] == 1.35 else -4.0)  # careful runs get further
            used = [learn.Knobs(Path(d) / "tuning.json", rng=random.Random(n)).draw()["caution"] for n in range(40)]
            self.assertGreater(used.count(1.35), 30)
        self.assertEqual(learn.knob("caution"), 1.0)  # the defaults are back


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
        hurt, healthy = self.base("map", hp=30, map=m), self.base("map", hp=80, map=m)
        hurt["run"]["act"] = healthy["run"]["act"] = 2  # early act 1 elites are avoided regardless
        self.assertEqual(strategy.map_screen(hurt, Memory())[0][1], {"index": 1})
        self.assertEqual(strategy.map_screen(healthy, Memory())[0][1], {"index": 0})

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

    def test_route_keeps_enough_health_for_the_boss(self):
        # An elite right before the boss costs more than its relic is worth, even from
        # full health; with a rest site in between it is fine, unless we are badly hurt.
        def route(mid):
            nodes = [{"col": 0, "row": 12, "type": "Elite", "children": [[0, 13]]}, {"col": 1, "row": 12, "type": "Monster", "children": [[0, 13]]},
                     {"col": 0, "row": 13, "type": mid, "children": [[0, 14]]}, {"col": 0, "row": 14, "type": "Boss", "children": []}]
            opts = [{"index": 0, "col": 0, "row": 12, "type": "Elite"}, {"index": 1, "col": 1, "row": 12, "type": "Monster"}]
            return {"next_options": opts, "nodes": nodes}

        pick = lambda hp, mid: strategy.map_screen(self.base("map", hp=hp, map=route(mid)), Memory())[0][1]["index"]
        self.assertEqual(pick(80, "Unknown"), 1)
        self.assertEqual(pick(80, "RestSite"), 0)
        self.assertEqual(pick(40, "RestSite"), 1)
        self.assertGreater(strategy.boss_odds(0.9), strategy.boss_odds(0.6))

    def test_rests_before_the_boss_unless_nearly_full(self):
        opts = [{"index": 0, "id": "REST", "is_enabled": True}, {"index": 1, "id": "SMITH", "is_enabled": True}]
        s = self.base("rest_site", hp=60, rest_site={"options": opts})  # 75%
        self.assertEqual(strategy.rest_site(s, Memory())[0][1]["index"], 1)
        self.assertEqual(strategy.rest_site(s, Memory(boss_next=True))[0][1]["index"], 0)

    def test_avoids_elites_early_in_act_one_and_notes_the_boss(self):
        nodes = [{"col": 0, "row": 2, "type": "Elite", "children": [[0, 3]]}, {"col": 1, "row": 2, "type": "Monster", "children": [[0, 3]]},
                 {"col": 0, "row": 3, "type": "Boss", "children": []}]
        opts = [{"index": 0, "col": 0, "row": 2, "type": "Elite"}, {"index": 1, "col": 1, "row": 2, "type": "Monster"}]
        s = self.base("map", hp=80, map={"next_options": opts, "nodes": nodes})
        s["run"]["act"] = 1
        mem = Memory()
        self.assertEqual(strategy.map_screen(s, mem)[0][1]["index"], 1)
        self.assertTrue(mem.boss_next)
        # Later on an elite is fine, as long as there is a rest site before the boss.
        nodes[2:] = [{"col": 0, "row": 3, "type": "RestSite", "children": [[0, 4]]}, {"col": 0, "row": 4, "type": "Boss", "children": []}]
        s["run"]["act"] = 2
        self.assertEqual(strategy.map_screen(s, mem)[0][1]["index"], 0)

    def test_a_badly_hurt_bot_rests_even_when_the_path_says_smith(self):
        # Act 3 at 6 of 109 HP, the boss next: the path values come out small
        # here, so the flat Smith bonus used to beat healing and the bot walked
        # into the boss at 6 HP.
        opts = [{"index": 0, "id": "REST", "is_enabled": True}, {"index": 1, "id": "SMITH", "is_enabled": True}]
        s = self.base("rest_site", hp=6, rest_site={"options": opts})
        s["player"]["max_hp"], s["run"]["act"] = 109, 3
        mem = Memory(boss_next=True)
        mem.map_nodes = {(1, 13): {"col": 1, "row": 13, "type": "RestSite", "children": [[3, 14]]},
                         (3, 14): {"col": 3, "row": 14, "type": "Boss", "children": []}}
        mem.map_at = (1, 13)
        saved, learn.KNOB["upgrade_worth"] = learn.KNOB["upgrade_worth"], 2.5  # the setting the learner usually picks
        try:
            self.assertEqual(strategy.rest_site(s, mem)[0][1]["index"], 0)
        finally:
            learn.KNOB["upgrade_worth"] = saved

    def test_heals_before_a_boss_when_below_ninety_percent(self):
        # Act 2 at 61 of 75 HP, the boss next: a flat upgrade bonus used to beat the heal
        # (the 19% of HP a rest gives is worth little to the path model), and the bot
        # entered the boss at 81%. Before a boss, rest.
        opts = [{"index": 0, "id": "REST", "is_enabled": True}, {"index": 1, "id": "SMITH", "is_enabled": True}]
        s = self.base("rest_site", hp=61, rest_site={"options": opts})
        s["player"]["max_hp"], s["run"]["act"] = 75, 2
        mem = Memory(boss_next=True)
        mem.map_nodes = {(0, 14): {"col": 0, "row": 14, "type": "RestSite", "children": [[3, 15]]},
                         (3, 15): {"col": 3, "row": 15, "type": "Boss", "children": []}}
        mem.map_at = (0, 14)
        saved, learn.KNOB["upgrade_worth"] = learn.KNOB["upgrade_worth"], 2.5
        try:
            self.assertEqual(strategy.rest_site(s, mem)[0][1]["index"], 0)
        finally:
            learn.KNOB["upgrade_worth"] = saved

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

    def test_stops_ticking_cards_once_enough_are_chosen(self):
        cards = [STRIKE(i) for i in range(5)]
        s = self.base("card_select", card_select={"screen_type": "enchant", "prompt": "Choose 3 cards to Enchant.", "cards": cards})
        mem = Memory()
        for _ in range(3):
            action, params = strategy.card_select(s, mem)[0]
            self.assertEqual(action, "select_card")
            mem.selected.add(params["index"])
        self.assertEqual(strategy.card_select(s, mem)[0], ("confirm_selection", {}))

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

    def test_auto_plays_the_character_with_the_lowest_ascension(self):
        unlocked = ["IRONCLAD", "SILENT", "DEFECT"]
        pick = lambda asc: strategy.choose_character("auto", unlocked, 0, asc)
        self.assertEqual(pick({}), "IRONCLAD")
        self.assertEqual(pick({"IRONCLAD": 2, "SILENT": 1, "DEFECT": 1}), "SILENT")
        self.assertEqual(pick({"IRONCLAD": 1}), "SILENT")  # never played counts as 0
        self.assertEqual(pick({"IRONCLAD": 3, "SILENT": 3, "DEFECT": 3}), "IRONCLAD")

    def test_embarks_at_the_highest_unlocked_ascension(self):
        chars = [{"id": "IRONCLAD", "locked": False}, {"id": "SILENT", "locked": False}, {"id": "DEFECT", "locked": True}]
        s = {"state_type": "menu", "menu_screen": "character_select", "characters": chars,
             "options": [{"name": "IRONCLAD", "enabled": True}, {"name": "SILENT", "enabled": True}, {"name": "confirm", "enabled": True}],
             "ascension": 0, "max_ascension": 3}
        mem = Memory(max_ascension={"IRONCLAD": 4, "SILENT": 3})
        self.assertEqual(strategy.menu(s, mem, "auto")[0], ("menu_select", {"option": "SILENT"}))
        mem.character_selected = True
        self.assertEqual(strategy.menu(s, mem, "auto")[0], ("set_ascension", {"level": 3}))
        s["ascension"] = 3
        self.assertEqual(strategy.menu(s, mem, "auto")[0], ("menu_select", {"option": "confirm"}))
        # The levels are not on screen yet (it happens just after picking the
        # character): hold off, do not embark at ascension 0.
        s["ascension"], s["max_ascension"] = 0, 0
        mem.ascension_waits = 0
        self.assertEqual(strategy.menu(s, mem, "auto")[0], ("menu_select", {"option": "SILENT"}))
        # A mod without the patch never shows them: give up waiting and embark.
        del s["ascension"], s["max_ascension"]
        first = [strategy.menu(s, mem, "auto")[0] for _ in range(25)]
        self.assertEqual(first[-1], ("menu_select", {"option": "confirm"}))

    def test_epochs_come_before_ascension_levels(self):
        chars = [{"id": "IRONCLAD", "locked": False}]
        s = {"state_type": "menu", "menu_screen": "character_select", "characters": chars,
             "options": [{"name": "IRONCLAD", "enabled": True}, {"name": "confirm", "enabled": True}],
             "ascension": 0, "max_ascension": 3}
        epochs = {"IRONCLAD4_EPOCH": "Revealed", "IRONCLAD7_EPOCH": "NotObtained"}
        mem = Memory(max_ascension={"IRONCLAD": 3}, epochs=dict(epochs), character_selected=True)
        # Its Ascension 1 epoch is still to earn: level 1, not the highest.
        self.assertEqual(strategy.menu(s, mem, "IRONCLAD")[0], ("set_ascension", {"level": 1}))
        # That done but the Timeline not complete (other epochs out there): the easiest level.
        mem.epochs["IRONCLAD7_EPOCH"] = "Revealed"
        s["ascension"] = 2
        self.assertEqual(strategy.menu(s, mem, "IRONCLAD")[0], ("set_ascension", {"level": 0}))
        # Every epoch in: climb.
        mem.epochs = {f"E{n}_EPOCH": "Revealed" for n in range(strategy.TOTAL_EPOCHS)}
        self.assertEqual(strategy.menu(s, mem, "IRONCLAD")[0], ("set_ascension", {"level": 3}))

    def test_auto_plays_towards_unlocking_every_epoch(self):
        epochs = {
            "IRONCLAD2_EPOCH": "Revealed", "IRONCLAD7_EPOCH": "NotObtained",
            "SILENT1_EPOCH": "Revealed", "SILENT2_EPOCH": "Revealed", "SILENT4_EPOCH": "NotObtained",
            "SILENT5_EPOCH": "NotObtained", "REGENT1_EPOCH": "Revealed", "REGENT2_EPOCH": "NotObtained",
            "REGENT3_EPOCH": "NotObtained", "REGENT4_EPOCH": "NotObtained", "NECROBINDER1_EPOCH": "NotObtained",
        }
        who = ["IRONCLAD", "SILENT", "REGENT"]
        pick = lambda drought=None: strategy.choose_character("auto", who, 0, {"IRONCLAD": 1}, epochs, drought)
        self.assertEqual(strategy.missing_epochs("SILENT", epochs), 2)
        self.assertEqual(pick(), "REGENT")  # three epochs to earn, the most
        # Twelve Regent runs that earned nothing: try someone else for now.
        self.assertEqual(pick({"REGENT": 12}), "SILENT")
        # Nobody has epochs left: back to evening out the ascension levels.
        done = {k: "Revealed" for k in epochs}
        self.assertEqual(strategy.choose_character("auto", who, 0, {"IRONCLAD": 1, "REGENT": 2}, done), "SILENT")

    def test_auto_knows_what_each_epoch_takes(self):
        epochs = {f"{c}{n}_EPOCH": "NotObtained" for c in ("DEFECT", "REGENT", "SILENT") for n in (4, 6, 7)}
        epochs["SILENT6_EPOCH"] = "Revealed"
        who = ["SILENT", "REGENT", "DEFECT"]
        kills = {"SILENT": 16, "REGENT": 8, "DEFECT": 13}
        strength = {"SILENT": 0.7, "REGENT": 0.55, "DEFECT": 0.85}
        pick = lambda **kw: strategy.choose_character("auto", who, 0, {}, epochs, kw.get("drought"), kw.get("kills", kills), strength)
        # Two bosses short of the 15-boss epoch, and the strongest: the Defect.
        self.assertEqual(pick(), "DEFECT")
        # Bosses still count when nothing else has come of the last runs.
        self.assertEqual(pick(drought={"DEFECT": 20}), "REGENT")  # also short of bosses; the Silent is not
        # With the boss epochs done, wins are what is left: the strongest goes first,
        # and the others get a turn when it keeps coming back with nothing.
        done = {"SILENT": 16, "REGENT": 15, "DEFECT": 15}
        epochs.update(REGENT6_EPOCH="Revealed", DEFECT6_EPOCH="Revealed")
        self.assertEqual(pick(kills=done), "DEFECT")
        self.assertEqual(pick(kills=done, drought={"DEFECT": 8}), "SILENT")

    def test_one_character_can_be_put_first(self):
        epochs = {"IRONCLAD7_EPOCH": "NotObtained", "REGENT4_EPOCH": "NotObtained", "REGENT6_EPOCH": "NotObtained", "REGENT7_EPOCH": "NotObtained"}
        who = ["IRONCLAD", "REGENT"]
        self.assertEqual(strategy.choose_character("auto", who, 0, {}, epochs), "REGENT")
        self.assertEqual(strategy.choose_character("auto:ironclad", who, 0, {}, epochs), "IRONCLAD")
        epochs["IRONCLAD7_EPOCH"] = "Revealed"  # done: back to the usual order
        self.assertEqual(strategy.choose_character("auto:ironclad", who, 0, {}, epochs), "REGENT")

    def test_character_choice(self):
        unlocked = ["IRONCLAD", "SILENT", "REGENT"]
        self.assertEqual(strategy.choose_character("silent", unlocked, 0), "SILENT")
        self.assertEqual(strategy.choose_character("DEFECT", unlocked, 0), "IRONCLAD")
        self.assertEqual([strategy.choose_character("cycle", unlocked, n) for n in range(4)], ["IRONCLAD", "SILENT", "REGENT", "IRONCLAD"])


class Afflictions(unittest.TestCase):
    def offer(self, hp, boss_hp):
        waste = card(0, "WASTE_AWAY", "Waste Away", "Status", 0, "Gain 1 less [energy_icon.png] per turn.")
        burn = card(1, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 8 damage.")
        state = {
            "state_type": "card_select", "run": {"act": 2, "floor": 33}, "player": {"hp": hp, "max_hp": 80},
            "battle": {"enemies": [enemy("KNOWLEDGE_DEMON_0", boss_hp)]},
            "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": [waste, burn]},
        }
        return strategy.card_select(state, Memory())[0][1]["index"]

    def test_damage_is_taken_over_lost_energy_when_the_fight_can_be_finished(self):
        self.assertEqual(self.offer(hp=60, boss_hp=100), 1)  # three turns of 8 with 20 to spare
        self.assertEqual(self.offer(hp=30, boss_hp=100), 0)  # cannot afford it
        self.assertEqual(self.offer(hp=60, boss_hp=250), 0)  # too far from the end


class HandSelections(unittest.TestCase):
    def test_the_card_put_back_on_the_deck_is_one_it_would_not_play(self):
        comet = card(0, "COMET", "Comet", "Attack", 3, "Deal 33 damage.", "AnyEnemy", rarity="Rare")
        pulse = card(1, "ASTRAL_PULSE", "Astral Pulse", "Attack", 0, "Deal 8 damage to ALL enemies twice.", "AllEnemies")
        wound = card(3, "WOUND", "Wound", "Status", 0, "Unplayable.")
        state = {
            "state_type": "hand_select",
            "player": {"energy": 1},
            "hand_select": {"prompt": "Choose a card to put on top of your Draw Pile.", "can_confirm": False,
                            "cards": [comet, pulse, DEFEND(2), wound]},
        }
        picks = lambda: [a[1]["card_index"] for a in strategy.hand_select(state, Memory()) if a[0] == "combat_select_card"]
        self.assertEqual(picks()[0], 0)  # 3 energy it does not have: Comet waits a turn, at no cost
        # With energy for everything, the dead card goes back before a card it would play.
        state["player"]["energy"] = 4
        self.assertEqual(picks()[0], 3)
        # Energy for one card of two: the weaker one is the one left over.
        state["player"]["energy"] = 1
        state["hand_select"]["cards"] = [STRIKE(0), card(1, "POMMEL_STRIKE", "Pommel Strike", "Attack", 1, "Deal 9 damage. Draw 1 card.", "AnyEnemy")]
        self.assertEqual(picks()[0], 0)


class RewardBar(unittest.TestCase):
    def test_acts_two_and_three_grow_the_bar_half_as_fast(self):
        # Growth from a 12-card deck to a 26-card one: 3 in act 1, 1.5 in acts 2 and 3.
        self.assertAlmostEqual(strategy.reward_bar(26, 1) - strategy.reward_bar(12, 1), 3.0)
        self.assertAlmostEqual(strategy.reward_bar(26, 2) - strategy.reward_bar(12, 2), 1.5)
        self.assertAlmostEqual(strategy.reward_bar(26, 3) - strategy.reward_bar(12, 3), 1.5)
        self.assertAlmostEqual(strategy.reward_bar(22, 1) - strategy.reward_bar(22, 2), 1.0)

    def test_small_decks_have_the_same_bar_in_every_act(self):
        self.assertEqual(strategy.reward_bar(10, 1), strategy.reward_bar(10, 3))
        self.assertEqual(strategy.reward_bar(10, None), strategy.reward_bar(10, 1))


if __name__ == "__main__":
    unittest.main()
