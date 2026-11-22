import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from sts2bot.fight import combat
from sts2bot import learn
from sts2bot.run import strategy
from sts2bot.bot import Bot, Stuck
from sts2bot.cards import deck_value, parse
from sts2bot.run.strategy import Memory

from ..fake_game import FakeGame, card
from ..helpers import DEFEND, STRIKE, FakeClock, combat_state, enemy, make_bot  # noqa: F401


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
            for _ in range(40):  # (a card's own record counts slowly: learn.PRIOR_PICKS)
                stats.record("The Silent", 45, False, [[3, "Footwork+"], [5, "Slice"]])
                stats.record("The Silent", 8, False, [[3, "Slice"]])
            self.assertGreater(stats.shift("The Silent", "Footwork"), fresh + 0.5)
            self.assertLess(stats.shift("The Silent", "Slice"), stats.shift("The Silent", "Footwork"))
            again = learn.CardStats(path)  # kept between sessions
            self.assertEqual(again.shift("The Silent", "Footwork"), stats.shift("The Silent", "Footwork"))
            self.assertLessEqual(len(again.finals["The Silent"]), learn.HISTORY)

    def test_an_elite_met_hurt_costs_more_than_its_average(self):
        from unittest import mock

        with mock.patch.dict(learn.KNOB, {"elite_worth": 2.5}):
            way = [("Elite", 5), ("Monster", 6)]
            healthy = strategy.path_value(way, 80, 80, 0, 2)
            sore = strategy.path_value(way, 56, 80, 0, 2)  # 70%: 1 lost in 8
            hurt = strategy.path_value(way, 48, 80, 0, 2)  # 60%: where a third of these fights were lost
            low = strategy.path_value(way, 38, 80, 0, 2)
        self.assertLess(sore, healthy - 2.0)
        self.assertLess(hurt, healthy - 5.0)
        self.assertLess(low, hurt - 5.0)

    def test_a_fight_met_badly_hurt_is_priced_as_the_risk_it_is(self):
        fight_first = [("Monster", 10), ("RestSite", 11), ("RestSite", 12), ("Boss", 13)]
        rest_first = [("RestSite", 10), ("Monster", 11), ("RestSite", 12), ("Boss", 13)]
        # On 23 of 80 HP in act 3 the fight that comes before any rest is the one that ends runs.
        self.assertLess(strategy.path_value(fight_first, 23, 80, 100, 3), strategy.path_value(rest_first, 23, 80, 100, 3) - 8.0)
        # Healthy, the order makes no difference; and act 1 fights are not priced this way at all.
        self.assertAlmostEqual(strategy.path_value(fight_first, 70, 80, 100, 3), strategy.path_value(rest_first, 70, 80, 100, 3), delta=0.5)
        self.assertGreater(strategy.path_value([("Monster", 10)], 23, 80, 100, 1), strategy.path_value([("Monster", 10)], 23, 80, 100, 3) + 7.0)

    def test_an_act_3_elite_is_a_fifth_of_a_run_and_its_relic_has_one_boss_left(self):
        # Sixteen met on Ascension 5-6: three ended the run there, two more left it to die in the next room.
        with_elite = [("Elite", 9), ("RestSite", 10), ("Monster", 11), ("RestSite", 12), ("Boss", 13)]
        without = [("Unknown", 9), ("RestSite", 10), ("Monster", 11), ("RestSite", 12), ("Boss", 13)]
        with mock.patch.dict(learn.KNOB, {"elite_worth": 6.0, "upgrade_worth": 2.5}):
            self.assertLess(strategy.path_value(with_elite, 98, 100, 0, 3), strategy.path_value(without, 98, 100, 0, 3))
            # ... while in act 2 the same choice still goes to the elite (its relic has an act and a half to pay).
            self.assertGreater(strategy.path_value(with_elite, 98, 100, 0, 2), strategy.path_value(without, 98, 100, 0, 2))

    def test_hp_given_up_in_an_act_3_event_costs_more(self):
        from sts2bot.run import events
        mushroom = {"title": "Fragrant Mushroom", "description": "Lose 15 HP. Upgrade 2 random cards."}
        early, late = events._event_option_value(mushroom, 60, 80, act=2), events._event_option_value(mushroom, 60, 80, act=3)
        self.assertAlmostEqual(early - late, 12.0 * 15 / 60 * (events.ACT3_HP - 1.0))
        self.assertEqual(events._event_option_value(mushroom, 60, 80), early)

    def test_star_makers_are_worth_less_once_nothing_spends_their_stars(self):
        venerate = lambda i: card(i, "VENERATE", "Venerate", "Skill", 1, "Gain [star_icon.png][star_icon.png].", "Self")
        deck = [venerate(0), venerate(1), STRIKE(2), STRIKE(3)]  # makes 4 Stars a cycle, spends none
        cache = card(9, "HIDDEN_CACHE", "Hidden Cache", "Skill", 1, "Gain [star_icon.png]. Next turn, gain [star_icon.png][star_icon.png][star_icon.png].", "Self")
        self.assertLess(strategy.deck_fit(cache, deck), -0.4)
        self.assertEqual(strategy.deck_fit(cache, [STRIKE(0)]), 0.0)  # an empty star bank still wants makers

    def test_a_deck_short_of_stars_wants_makers_not_one_more_card_that_spends_them(self):
        star = lambda i, name, need, text: {**card(i, name.upper().replace(" ", "_"), name, "Attack", 0, text, "AnyEnemy"), "star_cost": need}
        venerate = card(0, "VENERATE", "Venerate", "Skill", 1, "Gain [star_icon.png][star_icon.png].", "Self")
        blow = card(1, "KNOCKOUT_BLOW", "Knockout Blow", "Attack", 3, "Deal 30 damage. If this kills an enemy, gain " + "[star_icon.png]" * 5 + ".", "AnyEnemy")
        deck = [venerate, blow, star(2, "Falling Star", 2, "Deal 8 damage."), star(3, "Astral Pulse", 3, "Deal 6 damage to ALL enemies twice."), STRIKE(4)]
        # Needs 5 Stars a cycle and makes 3 (the Blow's five come with a kill: one), plus the one the fight starts with.
        seven = star(9, "Seven Stars", 7, "Deal 7 damage to ALL enemies 7 times.")
        self.assertLess(strategy.deck_fit(seven, deck), -3.5)
        glow = card(8, "GLOW", "Glow", "Skill", 1, "Gain [star_icon.png]. Draw 1 card. Next turn, draw 1 card.", "Self")
        self.assertGreater(strategy.deck_fit(glow, deck), 0.4)

    def test_a_strike_is_removed_before_a_defend(self):
        cards_ = [DEFEND(0), STRIKE(1), card(2, "POMMEL_STRIKE", "Pommel Strike", "Attack", 1, "Deal 9 damage. Draw 1 card.", "AnyEnemy")]
        order = strategy._pick_order(cards_, "Choose a card to Remove.")
        self.assertEqual([c["index"] for c in order], [1, 0, 2])

    def test_an_attack_is_worth_more_to_a_deck_that_cannot_kill(self):
        attack = card(0, "DEFILE", "Defile", "Attack", 1, "Ethereal. Deal 13 damage.", "AnyEnemy")
        block = card(1, "DEFY", "Defy", "Skill", 1, "Gain 6 Block. Apply 1 Weak.", "AnyEnemy")
        starter = [STRIKE(i) for i in range(4)] + [DEFEND(i) for i in range(4, 10)]
        first = strategy.deck_fit(attack, starter, 1)
        self.assertGreater(first, 1.0)  # the starter deck does 2.4 for an energy
        self.assertEqual(strategy.deck_fit(block, starter, 1), 0.0)
        self.assertEqual(strategy.deck_fit(attack, starter), 0.0)  # (no act: no opinion)
        # Each attack taken makes the next one matter less, and a deck with enough of them gets no nudge.
        second = strategy.deck_fit(attack, starter + [{**attack, "name": "Something Else", "id": "ELSE"}], 1)
        self.assertLess(second, first)
        armed = starter + [card(i, f"BIG_{i}", f"Big {i}", "Attack", 1, "Deal 14 damage.", "AnyEnemy") for i in range(10, 18)]
        self.assertEqual(strategy.deck_fit(attack, armed, 2), 0.0)
        # A deck of Block and Powers late in a run is as short of damage as the starter was.
        soft = starter + [{**block, "id": f"B{i}", "name": f"B{i}"} for i in range(10)]
        self.assertGreater(strategy.deck_fit(attack, soft, 2), 1.0)

    def test_the_first_attack_of_a_run_comes_before_what_pays_off_later(self):
        strike = {"id": "STRIKE_REGENT", "name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."}
        defend = {"id": "DEFEND_REGENT", "name": "Defend", "type": "Skill", "cost": 1, "description": "Gain 5 Block."}
        star = {"id": "FALLING_STAR", "name": "Falling Star", "type": "Attack", "cost": 0, "star_cost": 2, "description": "Deal 8 damage. Apply 1 Weak. Apply 1 Vulnerable."}
        attack = {"id": "KINGLY_PUNCH", "name": "Kingly Punch", "type": "Attack", "cost": 1, "description": "Deal 8 damage."}
        starter = [strike] * 4 + [defend] * 4 + [star]
        first = strategy.deck_fit(attack, starter, 1)
        self.assertGreaterEqual(first, 3.0)  # (the Regent's own Falling Star is not one it has taken)
        second = strategy.deck_fit(attack, starter + [{**attack, "id": "OTHER", "name": "Other"}], 1)
        self.assertLessEqual(second, 2.0)
        self.assertLessEqual(strategy.deck_fit(attack, starter, 2), 2.0)  # act 1 only
        # How hard a deck short of damage leans to attacks is a setting under test.
        from unittest import mock

        from sts2bot import learn

        soft = starter + [{**defend, "id": f"B{i}", "name": f"B{i}"} for i in range(8)] + [{**attack, "id": "OTHER", "name": "Other"}]
        with mock.patch.dict(learn.KNOB, {"punch": 1.8}):
            self.assertGreater(strategy.deck_fit(attack, soft, 2), 3.0)
        with mock.patch.dict(learn.KNOB, {"punch": 0.9}):
            self.assertLessEqual(strategy.deck_fit(attack, soft, 2), 2.0)

    def test_the_first_two_sources_of_weak_are_wanted_under_the_setting(self):
        from unittest import mock

        from sts2bot import learn

        skill = lambda name, text: {"id": name.upper(), "name": name, "type": "Skill", "cost": 1, "description": text}
        weak, other = skill("Sap", "Apply 2 Weak."), skill("Guard", "Gain 8 Block.")
        deck = [{"id": "STRIKE_IRONCLAD", "name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."}] * 9
        plain, plain_other = strategy.deck_fit(weak, deck, 2), strategy.deck_fit(other, deck, 2)
        with mock.patch.dict(learn.KNOB, {"weak_want": 1.0}):
            self.assertAlmostEqual(strategy.deck_fit(weak, deck, 2), plain + 1.0)
            self.assertAlmostEqual(strategy.deck_fit(other, deck, 2), plain_other)
            held = deck + [skill("Hex", "Apply 1 Weak."), skill("Curse Word", "Apply 1 Weak.")]
            self.assertAlmostEqual(strategy.deck_fit(weak, held, 2), strategy.deck_fit(other, held, 2))  # two held: no more than any card

    def test_the_first_block_cards_beyond_the_defends_are_wanted_under_the_setting(self):
        from unittest import mock

        from sts2bot import learn

        strike = {"id": "STRIKE_IRONCLAD", "name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."}
        defend = {"id": "DEFEND_IRONCLAD", "name": "Defend", "type": "Skill", "cost": 1, "description": "Gain 5 Block."}
        guard = lambda i: {"name": f"Guard {i}", "cost": 1, "description": "Gain 8 Block."}  # (as the draw pile lists a card)
        offer = {"id": "SHRUG_IT_OFF", "name": "Shrug It Off", "type": "Skill", "cost": 1, "description": "Gain 8 Block. Draw 1 card."}
        other = {"id": "SAP", "name": "Sap", "type": "Skill", "cost": 1, "description": "Draw 2 cards."}
        # The Defends are not Block cards the deck took, upgraded or not.
        base = [strike] * 5 + [defend] * 4 + [{"name": "Defend+", "cost": 1, "description": "Gain 8 Block."}]
        self.assertFalse(any(map(strategy._blocks, base)))
        plain = strategy.deck_fit(offer, base, 1)
        with mock.patch.dict(learn.KNOB, {"block_want": 1.5}):
            held = [strategy.deck_fit(offer, base + [guard(i) for i in range(n)], 1) for n in (0, 1, 2, 3)]
            self.assertAlmostEqual(strategy.deck_fit(other, base, 1), strategy.deck_fit(other, base + [guard(0)], 1))  # (no other card moves)
            # A deck of 24 has room for one more.
            big = base + [strike] * 12 + [guard(0), guard(1)]
            self.assertAlmostEqual(strategy.deck_fit(offer, big, 2) - strategy.deck_fit(other, big, 2), 1.5)
            # One that costs more than a turn's energy is not wanted for it.
            dear = {"id": "BULWARK", "name": "Bulwark", "type": "Skill", "cost": 4, "description": "Gain 30 Block."}
            self.assertLess(strategy.deck_fit(dear, base, 1), 0.0)
        self.assertAlmostEqual(held[0], plain + 1.5)
        self.assertAlmostEqual(held[1], held[0])  # the second as much as the first
        self.assertAlmostEqual(held[2], plain + 0.75)  # half for the third
        self.assertAlmostEqual(held[3], plain)  # and then a Block card is worth what it is rated

    def test_the_lean_to_attacks_lets_go_once_the_deck_has_its_share_under_the_setting(self):
        from unittest import mock

        from sts2bot import learn

        strike = {"id": "STRIKE_IRONCLAD", "name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."}
        defend = {"id": "DEFEND_IRONCLAD", "name": "Defend", "type": "Skill", "cost": 1, "description": "Gain 5 Block."}
        hit = lambda i: {"name": f"Hit {i}", "cost": 1, "description": "Deal 9 damage."}  # (as the draw pile lists a card: no id, no type)
        offer = {"id": "NEW_HIT", "name": "New Hit", "type": "Attack", "cost": 1, "description": "Deal 10 damage."}
        # Upgraded Strikes are not cards the deck took: they leave the room as it was.
        base = [strike] * 3 + [{"name": "Strike+", "cost": 1, "description": "Deal 9 damage."}] * 2 + [defend] * 9
        self.assertEqual(strategy._hitters(base + [hit(0), hit(1)]), 2)
        with mock.patch.dict(learn.KNOB, {"punch": 1.8}):
            always = [strategy.deck_fit(offer, base + [hit(i) for i in range(n)], 2) for n in (3, 6)]
            with mock.patch.dict(learn.KNOB, {"attack_room": 3.0}):
                capped = [strategy.deck_fit(offer, base + [hit(i) for i in range(n)], 2) for n in (3, 4, 5, 6)]
        self.assertGreater(always[1], 1.5)  # (without the setting a deck of six such attacks still leans to a seventh)
        self.assertAlmostEqual(capped[0], always[0])  # three held: the lean in full
        self.assertGreater(capped[0], capped[1])
        self.assertGreater(capped[1], capped[2])
        self.assertGreater(capped[2], capped[3])
        self.assertAlmostEqual(capped[3], 0.0)  # six held: an attack is worth what it is rated, no more

    def test_a_card_is_worth_more_beside_the_cards_other_players_win_with_it(self):
        def card(name):
            return {"id": name.upper().replace(" ", "_"), "name": name, "type": "Power", "cost": 1, "description": "Whenever a card is Exhausted, draw 1 card."}

        deck = [{"id": "STRIKE_IRONCLAD", "name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."}] * 9
        alone = strategy.deck_fit(card("Dark Embrace"), deck, 2)
        beside = strategy.deck_fit(card("Dark Embrace"), deck + [card("Feel No Pain")], 2)
        self.assertGreater(beside, alone + 1.0)
        # A card of no such group gains nothing from it.
        self.assertEqual(strategy.deck_fit(card("Barricade"), deck + [card("Feel No Pain")], 2), strategy.deck_fit(card("Barricade"), deck, 2))

    def test_a_card_dearer_than_a_turns_energy_is_not_the_damage_a_deck_is_short_of(self):
        strike = {"id": "STRIKE_NECROBINDER", "name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."}
        defend = {"id": "DEFEND_NECROBINDER", "name": "Defend", "type": "Skill", "cost": 1, "description": "Gain 5 Block."}
        bury = {"id": "BURY", "name": "Bury", "type": "Attack", "cost": 4, "description": "Deal 52 damage."}
        kick = {"id": "KINGLY_KICK", "name": "Kingly Kick", "type": "Attack", "cost": 4, "description": "Deal 27 damage. Whenever you draw this card, reduce its cost by 1."}
        wisp = {"id": "WISP", "name": "Wisp", "type": "Skill", "cost": 0, "description": "Gain [necrobinder_energy_icon.png]. Exhaust."}
        deck = [strike] * 4 + [defend] * 4 + [{**strike, "id": "REAVE", "name": "Reave", "description": "Deal 10 damage."}]
        self.assertLessEqual(strategy.deck_fit(bury, deck, 2), -3.0)  # 13 damage an energy on paper, and never played
        self.assertGreater(strategy.deck_fit(bury, deck, 2, 4), 0.0)  # with a fourth energy it is what the deck wants
        self.assertGreater(strategy.deck_fit(bury, deck + [wisp, wisp], 2), strategy.deck_fit(bury, deck, 2))
        self.assertGreaterEqual(strategy.deck_fit(kick, deck, 2), 0.0)  # one that gets cheaper is not held against

    def test_a_card_is_judged_against_the_characters_other_picks(self):
        from sts2bot import learn

        stats = learn.CardStats(None)
        # Every pick "beat the usual" (the bot got better meanwhile); Slice only by a little.
        stats.cards["The Silent"] = {"FOOTWORK": [120.0, 1200.0], "SLICE": [120.0, 240.0]}
        stats.finals["The Silent"] = [20] * 10
        self.assertGreater(stats.shift("The Silent", "Footwork"), 0.5)
        self.assertLess(stats.shift("The Silent", "Slice"), -0.5)


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

        # (Whatever is on trial just now: the sampler is what is tested, on a setting with two values.)
        with tempfile.TemporaryDirectory() as d, mock.patch.dict(learn.KNOB), mock.patch.dict(learn.CHOICES, {"caution": [0.75, 1.0]}):
            knobs = learn.Knobs(Path(d) / "tuning.json", rng=random.Random(1))
            for _ in range(300):
                knobs.draw()
                knobs.record(8.0 if learn.KNOB["caution"] == 0.75 else -4.0)  # bold runs get further
            used = [learn.Knobs(Path(d) / "tuning.json", rng=random.Random(n)).draw()["caution"] for n in range(40)]
            self.assertGreater(used.count(0.75), 30)
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

    def test_badly_hurt_an_elite_is_never_the_way_on_while_there_is_another(self):
        # 14 of 76 HP in act 2: both roads are dead on paper (-67.1 and -72.2), and the elite's relic and the
        # rest site behind it tipped it. The Unknown room is a fight one time in five; the elite always is.
        nodes = [{"col": 2, "row": 10, "type": "Elite", "children": [[2, 11]]}, {"col": 3, "row": 10, "type": "Unknown", "children": [[3, 11]]},
                 {"col": 2, "row": 11, "type": "RestSite", "children": [[2, 12]]}, {"col": 3, "row": 11, "type": "Monster", "children": [[2, 12]]},
                 {"col": 2, "row": 12, "type": "Monster", "children": [[2, 13]]}, {"col": 2, "row": 13, "type": "Boss", "children": []}]
        opts = [{"index": 0, "col": 2, "row": 10, "type": "Elite"}, {"index": 1, "col": 3, "row": 10, "type": "Unknown"}]
        state = self.base("map", hp=14, map={"next_options": opts, "nodes": nodes})
        state["player"]["max_hp"], state["run"]["act"] = 76, 2
        with mock.patch.dict(learn.KNOB, {"elite_worth": 6.0}):
            self.assertEqual(strategy.map_screen(state, Memory())[0][1], {"index": 1})
            with mock.patch.object(strategy, "ELITE_FLOOR", 0.0):
                self.assertEqual(strategy.map_screen(state, Memory())[0][1], {"index": 0})  # (what the values alone said)
            state["map"]["next_options"] = opts[:1]  # no other way: the elite it is
            self.assertEqual(strategy.map_screen(state, Memory())[0][1], {"index": 0})

    def test_route_keeps_enough_health_for_the_boss(self):
        # An elite right before the boss costs more than its relic is worth, even from
        # full health. With a rest site in between an act 1 elite (20 HP) is paid for:
        # from 80% HP up a boss is beaten as often as from full. An act 2 elite (27 HP)
        # needs two, and nothing pays for one met badly hurt.
        def route(mid):
            mids = mid.split("+")
            nodes = [{"col": 0, "row": 12, "type": "Elite", "children": [[0, 13]]}, {"col": 1, "row": 12, "type": "Monster", "children": [[0, 13]]}]
            nodes += [{"col": 0, "row": 13 + n, "type": kind, "children": [[0, 14 + n]]} for n, kind in enumerate(mids)]
            nodes += [{"col": 0, "row": 13 + len(mids), "type": "Boss", "children": []}]
            opts = [{"index": 0, "col": 0, "row": 12, "type": "Elite"}, {"index": 1, "col": 1, "row": 12, "type": "Monster"}]
            return {"next_options": opts, "nodes": nodes}

        def pick(hp, mid, act=1):
            state = self.base("map", hp=hp, map=route(mid))
            state["run"]["act"] = act
            return strategy.map_screen(state, Memory())[0][1]["index"]

        self.assertEqual(pick(80, "Unknown"), 1)
        with mock.patch.dict(learn.KNOB, {"elite_worth": 6.0}):  # (the worth in force; at 2.5 the monster and an upgrade win in act 1)
            self.assertEqual(pick(80, "RestSite"), 0)
        self.assertEqual(pick(60, "RestSite"), 1)
        self.assertEqual(pick(80, "RestSite", act=2), 1)
        with mock.patch.dict(learn.KNOB, {"elite_worth": 4.0}):  # (since 2026-11-22's act 2 curve, 18.2 against 18.3 at 2.5)
            self.assertEqual(pick(80, "RestSite+RestSite", act=2), 0)
        self.assertEqual(pick(40, "RestSite+RestSite"), 1)
        self.assertEqual(strategy.boss_odds(0.9), strategy.boss_odds(1.0))
        self.assertGreater(strategy.boss_odds(0.8), 2 * strategy.boss_odds(0.6) - 0.1)
        self.assertGreater(strategy.boss_odds(0.6), strategy.boss_odds(0.5))
        # The last boss has only been beaten from 95% HP up: there, 85% is a third of the way to nothing.
        self.assertEqual(strategy.boss_odds(0.96, 3), 1.0)
        self.assertLess(strategy.boss_odds(0.85, 3), 0.6)
        self.assertLess(strategy.boss_odds(0.7, 3), 0.25)
        fight_or_rest = [("Monster", 12), ("RestSite", 13), ("Boss", 14)], [("RestSite", 12), ("RestSite", 13), ("Boss", 14)]
        self.assertGreater(strategy.path_value(fight_or_rest[1], 50, 80, 0, 3), strategy.path_value(fight_or_rest[0], 50, 80, 0, 3) + 3.0)

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
        # Later on an elite is fine, as long as there are rest sites enough before the boss.
        nodes[2:] = [{"col": 0, "row": 3, "type": "RestSite", "children": [[0, 4]]}, {"col": 0, "row": 4, "type": "RestSite", "children": [[0, 5]]},
                     {"col": 0, "row": 5, "type": "Boss", "children": []}]
        s["run"]["act"] = 2
        from unittest import mock

        with mock.patch.dict(learn.KNOB, {"elite_worth": 4.0}):  # where the learner values a relic that much
            self.assertEqual(strategy.map_screen(s, mem)[0][1]["index"], 0)
        # At a low worth it is not: a bad fight there means meeting the boss short of full HP.
        with mock.patch.dict(learn.KNOB, {"elite_worth": 1.5}):
            self.assertEqual(strategy.map_screen(s, mem)[0][1]["index"], 1)

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

    def test_an_upgrade_counts_the_same_in_act_one_whatever_the_setting(self):
        # Act 1 lost 20 runs in 86 with the upgrade at 2.5 and 16 in 143 at 4.0; from act 2 on the lower
        # worth got past the boss oftener. So the setting only speaks from act 2.
        rooms = [("RestSite", 9), ("Monster", 10), ("Boss", 16)]
        values = {}
        for act in (1, 2):
            for worth in (2.5, 4.0):
                with mock.patch.dict(learn.KNOB, {"upgrade_worth": worth}):
                    values[act, worth] = strategy.path_value(rooms, 76.0, 80, 100, act)  # too healthy to rest: the upgrade is taken
        self.assertEqual(values[1, 2.5], values[1, 4.0])
        self.assertAlmostEqual(values[2, 4.0] - values[2, 2.5], 1.5)

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
            # From 85% HP up the act 1 boss is beaten as often as from full: upgrade then.
            s["player"]["hp"], s["run"]["act"] = 66, 1
            self.assertEqual(strategy.rest_site(s, mem)[0][1]["index"], 1)
            # ... but not before the act 2 boss (since 2026-11-22: 85-97% HP won 0.88 as often as 97%+) or the last.
            for act in (2, 3):
                s["run"]["act"] = act
                self.assertEqual(strategy.rest_site(s, mem)[0][1]["index"], 0, act)
            s["player"]["hp"], s["run"]["act"] = 72, 2  # 96%: upgrade
            self.assertEqual(strategy.rest_site(s, mem)[0][1]["index"], 1)
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

    def test_the_card_made_free_for_the_fight_is_the_dearest(self):
        cards_ = [
            card(0, "ASTRAL_PULSE", "Astral Pulse+", "Attack", 0, "Deal 18 damage to ALL enemies.", rarity="Rare"),
            card(1, "BULWARK", "Bulwark+", "Skill", 2, "Gain 15 Block."),
            card(2, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", rarity="Basic"),
        ]
        hs = {"prompt": "Choose a card to make free.", "cards": cards_}
        acts = strategy.hand_select(self.base("hand_select", hand_select=hs), Memory())
        self.assertEqual(acts[0], ("combat_select_card", {"card_index": 1}))

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
        mem = Memory()
        with self.assertLogs("sts2bot.strategy", level="DEBUG") as seen:
            acts = strategy.shop(self.base("shop", gold=250, shop=shop), mem)
            strategy.shop(self.base("shop", gold=250, shop=shop), mem)  # (looked at again: the shelf is written down once)
        self.assertEqual(acts, [("shop_purchase", {"index": 2}), ("shop_purchase", {"index": 1}), ("proceed", {})])
        # The log has the whole shelf: what each thing cost, what we made of it, and what we could not pay for.
        (line,) = [m for m in seen.output if "shop (gold 250, potions 0 of 3)" in m]
        self.assertIn("card_removal 75 [9.0]", line)
        self.assertIn("relic 300! [6.5]", line)
        self.assertRegex(line, r"card \S* ?50 \[-?\d")
        # Ascension 6: the second removal costs 150, and the relic at the same price comes first.
        shop["items"][2]["price"] = 150
        self.assertEqual(strategy.shop(self.base("shop", gold=250, shop=shop), Memory())[0], ("shop_purchase", {"index": 1}))

    def test_early_in_a_run_good_cards_come_before_the_removal_under_the_setting(self):
        from sts2bot import learn

        good = {"index": 0, "category": "card", "price": 24, "is_stocked": True, "can_afford": True, "card_id": "SHRUG_IT_OFF", "card_name": "Shrug It Off",
                "card_type": "Skill", "card_cost": "1", "card_rarity": "Common", "card_description": "Gain 8 Block. Draw 1 card."}
        removal = {"index": 1, "category": "card_removal", "price": 100, "is_stocked": True, "can_afford": True}
        state = self.base("shop", gold=113, shop={"items": [good, removal], "can_proceed": True})
        with mock.patch.object(strategy, "deck_value", lambda c: 7.7):  # (a card the ratings think well of, as on the shelf seen)
            self.assertEqual(strategy.shop(state, Memory())[0], ("shop_purchase", {"index": 1}))  # as it was: the removal first
            with mock.patch.dict(learn.KNOB, {"cards_first": 1.0}):
                self.assertEqual(strategy.shop(state, Memory())[0], ("shop_purchase", {"index": 0}))
                state["run"]["act"] = 3  # by act 3 a deck is thinned before it is fed
                self.assertEqual(strategy.shop(state, Memory())[0], ("shop_purchase", {"index": 1}))
        with mock.patch.dict(learn.KNOB, {"cards_first": 1.0}):
            state["run"]["act"] = 1
            with mock.patch.object(strategy, "deck_value", lambda c: 5.0):  # a middling card still waits for the removal
                self.assertEqual(strategy.shop(state, Memory())[0], ("shop_purchase", {"index": 1}))

    def test_the_fake_merchants_snecko_eye_is_left_on_the_shelf(self):
        fake = lambda i, rid, price: {"index": i, "category": "relic", "price": price, "is_stocked": True, "can_afford": True, "relic_id": rid, "relic_name": rid}
        shelf = {"items": [fake(0, "FAKE_STRIKE_DUMMY", 43), fake(1, "FAKE_SNECKO_EYE", 57), fake(2, "FAKE_ANCHOR", 57)], "can_proceed": True}
        state = self.base("fake_merchant", gold=319, fake_merchant={"shop": shelf})
        # "Start each combat Confused" with nothing for it is never bought; the others go by what they do, not by price.
        self.assertEqual(strategy.shop(state, Memory(), key="fake_merchant"), [("shop_purchase", {"index": 2}), ("shop_purchase", {"index": 0}), ("proceed", {})])
        # The gold is the same gold a real Merchant takes: short of plenty, only the ones that do something are bought.
        state["player"]["gold"] = 180
        self.assertEqual(strategy.shop(state, Memory(), key="fake_merchant"), [("shop_purchase", {"index": 2}), ("proceed", {})])

    def test_with_lords_parasol_the_shelf_empties_itself_and_nothing_is_bought(self):
        # The relic buys everything in turn; a purchase of ours in the middle crashed the game three times.
        item = lambda i, stocked: {"index": i, "category": "relic", "price": 150, "is_stocked": stocked, "can_afford": True}
        state = self.base("shop", gold=250, shop={"items": [item(0, True), item(1, False)], "can_proceed": True})
        state["player"]["relics"] = [{"id": "LORDS_PARASOL", "description": "When you encounter the Merchant, immediately obtain EVERYTHING he sells."}]
        mem = Memory()
        self.assertEqual(strategy.shop(state, mem), [])  # it is still at it: wait
        state["shop"]["items"][0]["is_stocked"] = False
        self.assertEqual(strategy.shop(state, mem), [("proceed", {})])
        # Something it cannot take (a potion with the belt full) does not keep us there for ever.
        state["shop"]["items"][0]["is_stocked"] = True
        looks = [strategy.shop(state, mem) for _ in range(9)]
        self.assertEqual(looks[-1], [("proceed", {})])
        self.assertTrue(all(a == [] or a == [("proceed", {})] for a in looks))

    def test_a_shop_before_the_boss_fills_the_potion_belt(self):
        items = [
            {"index": 0, "category": "potion", "price": 60, "is_stocked": True, "can_afford": True},
            {"index": 1, "category": "relic", "price": 250, "is_stocked": True, "can_afford": True},
        ]
        state = self.base("shop", gold=100, shop={"items": items, "can_proceed": True})
        state["player"]["max_potion_slots"] = 3
        # Mid-act the last gold is kept; two rooms from the boss it is not, and the potion comes first.
        self.assertEqual(strategy.shop(state, Memory())[0], ("shop_purchase", {"index": 1}))
        state["run"]["floor"] = 31
        self.assertEqual(strategy.shop(state, Memory())[0], ("shop_purchase", {"index": 0}))
        # ... before a card removal too, until the belt holds two.
        removal = {"index": 2, "category": "card_removal", "price": 100, "is_stocked": True, "can_afford": True}
        state["shop"]["items"].append(removal)
        self.assertEqual(strategy.shop(state, Memory())[0], ("shop_purchase", {"index": 0}))
        state["player"]["potions"] = [{}, {}]
        self.assertEqual(strategy.shop(state, Memory())[0], ("shop_purchase", {"index": 2}))
        state["shop"]["items"].remove(removal)
        state["player"]["potions"] = [{}, {}, {}]
        self.assertEqual(strategy.shop(state, Memory())[0], ("shop_purchase", {"index": 1}))

    def test_a_full_belt_gives_up_its_least_potion_for_a_better_one(self):
        belt = [{"slot": 0, "name": "Strength Potion", "description": "Gain 2 Strength."}, {"slot": 1, "name": "Swift Potion", "description": "Draw 3 cards."}]
        offer = lambda name, text: {"index": 1, "type": "potion", "potion_name": name, "potion_description": text}
        state = self.base("rewards", rewards={"items": [{"index": 0, "type": "gold"}, offer("Fire Potion", "Deal 20 damage.")]})
        state["player"].update(potions=belt, max_potion_slots=2)
        mem = Memory()
        self.assertEqual(strategy.rewards(state, mem), [("discard_potion", {"slot": 1}), ("claim_reward", {"index": 0}), ("proceed", {})])
        mem.potion_swaps[(1, 5)] = 2  # (the game said yes twice and the belt is still full: no third time)
        self.assertEqual(strategy.rewards(state, mem), [("claim_reward", {"index": 0}), ("proceed", {})])
        # One no better than what is held is left lying, as before.
        state["rewards"]["items"][1] = offer("Gambler's Brew", "Discard any number of cards, then draw that many.")
        self.assertEqual(strategy.rewards(state, Memory()), [("claim_reward", {"index": 0}), ("proceed", {})])
        # ... and with Sozu nothing is poured away for a potion that cannot be taken.
        state["rewards"]["items"][1] = offer("Fire Potion", "Deal 20 damage.")
        state["player"].update(potions=belt[1:], relics=[{"id": "SOZU", "description": "You can no longer obtain Potions."}])
        self.assertEqual(strategy.rewards(state, Memory()), [("claim_reward", {"index": 0}), ("proceed", {})])

    def test_no_potion_is_bought_with_a_relic_that_forbids_them(self):
        items = [{"index": 0, "category": "potion", "price": 60, "is_stocked": True, "can_afford": True}]
        state = self.base("shop", gold=100, shop={"items": items, "can_proceed": True})
        state["run"]["floor"] = 31
        state["player"]["relics"] = [{"id": "SOZU", "description": "Gain energy at the start of each turn. You can no longer obtain Potions."}]
        self.assertEqual(strategy.shop(state, Memory()), [("proceed", {})])

    def test_auto_plays_the_character_with_the_lowest_ascension(self):
        unlocked = ["IRONCLAD", "SILENT", "DEFECT"]
        pick = lambda asc: strategy.choose_character("auto", unlocked, 0, asc)
        self.assertEqual(pick({"IRONCLAD": 2, "SILENT": 1, "DEFECT": 2}), "SILENT")
        self.assertIn(pick({"IRONCLAD": 1}), ("SILENT", "DEFECT"))  # never played counts as 0

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

    def test_auto_and_random_draw_among_the_lowest_ascension_only(self):
        who = ["IRONCLAD", "SILENT", "REGENT", "NECROBINDER"]
        levels = {"IRONCLAD": 1, "SILENT": 2, "REGENT": 1, "NECROBINDER": 1}
        for setting in ("auto", "random"):
            seen = {strategy.choose_character(setting, who, run, levels) for run in range(60)}
            self.assertEqual(seen, {"IRONCLAD", "REGENT", "NECROBINDER"})  # never the Silent, everyone else in turn
            # The menu asks on every poll: the answer must not change during one run.
            self.assertEqual(len({strategy.choose_character(setting, who, 7, levels) for _ in range(20)}), 1)
            # The game's "Random" tile is not a character, whatever level it shows.
            with_tile = who + ["RANDOM_CHARACTER"]
            self.assertNotIn("RANDOM_CHARACTER", {strategy.choose_character(setting, with_tile, run, levels) for run in range(60)})
            levels_one_behind = dict(levels, REGENT=0)
            self.assertEqual({strategy.choose_character(setting, who, run, levels_one_behind) for run in range(20)}, {"REGENT"})

    def test_a_run_is_played_at_the_top_level_even_with_epochs_left(self):
        chars = [{"id": "IRONCLAD", "locked": False}]
        s = {"state_type": "menu", "menu_screen": "character_select", "characters": chars,
             "options": [{"name": "IRONCLAD", "enabled": True}, {"name": "confirm", "enabled": True}],
             "ascension": 0, "max_ascension": 3}
        epochs = {"IRONCLAD4_EPOCH": "Revealed", "IRONCLAD7_EPOCH": "NotObtained"}
        mem = Memory(max_ascension={"IRONCLAD": 3}, epochs=dict(epochs), character_selected=True)
        self.assertEqual(strategy.menu(s, mem, "IRONCLAD")[0], ("set_ascension", {"level": 3}))

    def test_auto_plays_the_characters_that_still_owe_an_epoch(self):
        who = ["IRONCLAD", "SILENT", "REGENT", "NECROBINDER", "DEFECT"]
        epochs = {f"{c}7_EPOCH": "Revealed" for c in who}
        epochs.update(DEFECT7_EPOCH="NotObtained", NECROBINDER7_EPOCH="NotObtained", RELIC1_EPOCH="NotObtained")
        # The Ironclad is a level behind, but has nothing of its own left to earn.
        levels = {"IRONCLAD": 1, "SILENT": 2, "REGENT": 2, "NECROBINDER": 2, "DEFECT": 2}
        seen = {strategy.choose_character("auto", who, run, levels, epochs) for run in range(60)}
        self.assertEqual(seen, {"NECROBINDER", "DEFECT"})
        # Among those that owe one, the lowest ascension still goes first.
        levels["DEFECT"] = 1
        self.assertEqual({strategy.choose_character("auto", who, run, levels, epochs) for run in range(20)}, {"DEFECT"})
        # All earned: back to evening out the levels.
        done = {k: "Revealed" for k in epochs}
        levels["DEFECT"] = 2
        self.assertEqual({strategy.choose_character("auto", who, run, levels, done) for run in range(20)}, {"IRONCLAD"})

    def test_one_character_can_be_put_first(self):
        epochs = {"IRONCLAD7_EPOCH": "NotObtained", "REGENT4_EPOCH": "NotObtained"}
        who = ["IRONCLAD", "REGENT"]
        levels = {"IRONCLAD": 1}
        self.assertEqual(strategy.choose_character("auto", who, 0, levels, epochs), "REGENT")
        self.assertEqual(strategy.choose_character("auto:ironclad", who, 0, levels, epochs), "IRONCLAD")
        epochs["IRONCLAD7_EPOCH"] = "Revealed"  # done: back to the usual order
        self.assertEqual(strategy.choose_character("auto:ironclad", who, 0, levels, epochs), "REGENT")

    def test_character_choice(self):
        unlocked = ["IRONCLAD", "SILENT", "REGENT"]
        self.assertEqual(strategy.choose_character("silent", unlocked, 0), "SILENT")
        self.assertEqual(strategy.choose_character("DEFECT", unlocked, 0), "IRONCLAD")
        self.assertEqual([strategy.choose_character("cycle", unlocked, n) for n in range(4)], ["IRONCLAD", "SILENT", "REGENT", "IRONCLAD"])


class Afflictions(unittest.TestCase):
    def offer(self, hp, turns=None, status=()):  # as the game sends it: no enemies on this screen
        waste = card(0, "WASTE_AWAY", "Waste Away", "Status", 0, "Gain 1 less [energy_icon.png] per turn.")
        burn = card(1, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 8 damage.")
        state = {
            "state_type": "card_select", "run": {"act": 2, "floor": 33}, "player": {"hp": hp, "max_hp": 80, "status": list(status)},
            "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": [waste, burn]}, "_turns": turns,
        }
        return strategy.card_select(state, Memory())[0][1]["index"]

    def test_damage_is_taken_over_lost_energy(self):
        self.assertEqual(self.offer(hp=60), 1)
        self.assertEqual(self.offer(hp=30), 1)  # an energy a turn costs as much, and for longer
        self.assertEqual(self.offer(hp=16), 0)  # two burns from dead: keep the HP

    def test_an_energy_that_was_not_being_spent_is_the_one_given_up(self):
        # Seen: a Necrobinder on 5 to 7 energy under Sloth, three cards a turn and 1 to 5 energy over every
        # turn, took 8 damage a turn on 21 HP rather than lose an energy it had no card for.
        spent = [(4, 0), (3, 0), (3, 0), (3, 0), (3, 0), (3, 0), (3, 0), (3, 0), (3, 0)]
        over = spent[:5] + [(3, 4), (3, 1), (3, 5), (3, 4)]
        self.assertEqual(self.offer(hp=60, turns=spent), 1)
        self.assertEqual(self.offer(hp=60, turns=over), 0)
        self.assertEqual(self.offer(hp=60, turns=spent[:5] + [(3, 0), (3, 1), (3, 0), (3, 2)]), 0)  # (two turns in four is enough)
        self.assertEqual(self.offer(hp=60, turns=spent[:5] + [(3, 0), (3, 0), (3, 0), (3, 2)]), 1)
        # ... and a second burn on top of the first is 15 a turn: the energy goes instead.
        self.assertEqual(self.offer(hp=60, turns=spent, status=[{"id": "DISINTEGRATION_POWER", "amount": 7}]), 0)

    def sloth(self, hp, turns):
        burn = card(0, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 7 damage.")
        sloth = card(1, "SLOTH", "Sloth", "Status", 0, "You cannot play more than 3 cards each turn.")
        state = {
            "state_type": "card_select", "run": {"act": 2, "floor": 33}, "player": {"hp": hp, "max_hp": 90}, "_turns": turns,
            "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": [burn, sloth]},
        }
        return strategy.card_select(state, Memory())[0][1]["index"]

    def test_three_cards_a_turn_is_dear_for_a_deck_that_plays_six(self):
        # The same Necrobinder had played 4, 6, 6 and 8 cards a turn; under Sloth it dealt 19 a round for 40.
        self.assertEqual(self.sloth(48, [(2, 2), (4, 0), (6, 0), (6, 0), (8, 0)]), 0)
        self.assertEqual(self.sloth(80, [(4, 0), (3, 0), (4, 0), (3, 0), (2, 1)]), 1)  # three energy, four cards: Sloth costs little
        self.assertEqual(self.sloth(80, [(4, 0), (3, 0), (9, 0), (3, 0), (2, 1)]), 1)  # (one long turn is not a deck)
        self.assertEqual(self.sloth(30, [(2, 2), (4, 0), (6, 0), (6, 0), (8, 0)]), 1)  # no HP for six turns of 7
        self.assertEqual(self.sloth(80, []), 1)  # (a bot started in the middle of the fight knows nothing: as before)

    def test_the_burn_is_the_cheaper_price_only_past_two_cards_a_turn_lost(self):  # (see afflictions.SLOTH_MARGIN; turn one is left out)
        self.assertEqual(self.sloth(80, [(2, 2), (6, 0), (4, 0), (5, 0), (3, 0)]), 1)  # 3, 1, 2, 0: 1.5 a turn
        self.assertEqual(self.sloth(80, [(2, 2), (6, 0), (7, 0), (3, 0), (4, 0)]), 1)  # 3, 3, 0, 1: 1.75 a turn
        self.assertEqual(self.sloth(80, [(2, 2), (6, 0), (5, 0), (5, 0), (5, 0)]), 0)  # 3, 2, 2, 2: 2.25 a turn


class HandSelections(unittest.TestCase):
    def test_a_sly_card_is_the_one_discarded(self):
        flick = card(1, "FLICK_FLACK", "Flick-Flack", "Attack", 1, "Deal 7 damage to ALL enemies.", "AllEnemies")
        flick["keywords"] = [{"name": "Sly", "description": "If this card is discarded from your Hand before the end of your turn, play it for free."}]
        state = {
            "state_type": "hand_select",
            "hand_select": {"prompt": "Choose a card to Discard.", "can_confirm": False, "cards": [STRIKE(0), flick, DEFEND(2)]},
            "player": {"energy": 1},
        }
        picks = [a[1]["card_index"] for a in strategy.hand_select(state, Memory()) if a[0] == "combat_select_card"]
        self.assertEqual(picks[0], 1)
        state["hand_select"]["prompt"] = "Choose a card to Exhaust."
        picks = [a[1]["card_index"] for a in strategy.hand_select(state, Memory()) if a[0] == "combat_select_card"]
        self.assertNotEqual(picks[0], 1)

    def insatiable(self, prompt):
        """A Silent on 9 HP, the Insatiable on 59 with 36 Poison and Serpent Form up: two Strikes and any third card kill it."""
        escape = card(0, "FRANTIC_ESCAPE", "Frantic Escape", "Status", 3, "Move 1 space away from the Sandpit.", "Self")
        snake = card(3, "SNAKEBITE", "Snakebite", "Skill", 2, "Retain. Apply 7 Poison.", "AnyEnemy")
        hand = [escape, STRIKE(1), STRIKE(2), snake, DEFEND(4)]
        for c in hand:  # the selection screen leaves these two out
            c.pop("target_type", None), c.pop("can_play", None)
        boss = {"entity_id": "THE_INSATIABLE_0", "hp": 59, "max_hp": 321, "block": 0, "intents": [{"type": "Attack", "label": "12x2"}],
                "status": [{"id": "POISON_POWER", "amount": 36}, {"id": "SANDPIT_POWER", "amount": 3}]}
        return {
            "state_type": "hand_select", "_fight": "boss",
            "hand_select": {"prompt": prompt, "can_confirm": False, "cards": hand},
            "battle": {"round": 9, "turn": "player", "is_play_phase": True, "enemies": [boss]},
            "player": {"hp": 9, "max_hp": 79, "block": 0, "energy": 3, "hand": [], "status": [{"id": "SERPENT_FORM_POWER", "amount": 4}]},
            "run": {"act": 2, "floor": 33},
        }

    def test_the_discard_is_a_card_the_turn_does_not_need(self):
        # By rating alone a Strike went: without it the 24 damage that leaves the boss to its Poison is not there.
        picks = [a[1]["card_index"] for a in strategy.hand_select(self.insatiable("Choose a card to Discard."), Memory()) if a[0] == "combat_select_card"]
        self.assertNotIn(picks[0], (1, 2))
        self.assertEqual(picks[-1], 0)  # and never the way out of the Sandpit

    def test_a_card_is_not_exhausted_when_that_loses_the_kill(self):
        picks = [a[1]["card_index"] for a in strategy.hand_select(self.insatiable("Choose a card to Exhaust."), Memory()) if a[0] == "combat_select_card"]
        self.assertNotIn(picks[0], (1, 2))

    def test_an_escape_is_exhausted_when_every_other_card_is_the_kill(self):
        """The Insatiable on 59 HP, the bot on 7 before 12x2: three attacks and the Bone Shards make 60 once the Mittens' Strength is in."""
        escape = card(0, "FRANTIC_ESCAPE", "Frantic Escape", "Status", 2, "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.", "Self")
        hand = [
            escape,
            card(1, "DEBILITATE", "Debilitate", "Attack", 1, "Deal 19 damage. Vulnerable and Weak are twice as effective against the enemy for the next 2 turns.", "AnyEnemy"),
            card(2, "VEILPIERCER", "Veilpiercer", "Attack", 1, "Deal 19 damage.", "AnyEnemy"),
            card(3, "BONE_SHARDS", "Bone Shards", "Attack", 1, "If Osty is alive, he deals 9 damage to ALL enemies and you gain 9 Block. Osty dies.", "AllEnemies"),
            card(4, "STRIKE_NECROBINDER", "Strike", "Attack", 1, "Deal 10 damage.", "AnyEnemy"),
        ]
        for c in hand:
            c.pop("target_type", None), c.pop("can_play", None)
        boss = {"entity_id": "THE_INSATIABLE_0", "hp": 59, "max_hp": 321, "block": 0, "intents": [{"type": "Attack", "label": "12x2"}],
                "status": [{"id": "SANDPIT_POWER", "amount": 2}]}
        state = {
            "state_type": "hand_select", "_fight": "boss",
            "hand_select": {"prompt": "Choose a card to Exhaust.", "can_confirm": False, "cards": hand},
            "battle": {"round": 10, "turn": "player", "is_play_phase": True, "enemies": [boss]},
            "player": {"hp": 7, "max_hp": 66, "block": 0, "energy": 5, "hand": [], "pets": [{"name": "Osty", "hp": 2, "alive": True}],
                       "relics": [{"id": "TOASTY_MITTENS", "description": "At the start of your turn, Exhaust 1 card from your Hand and gain 1 Strength."}]},
            "run": {"act": 2, "floor": 33},
        }
        first = lambda: [a[1]["card_index"] for a in strategy.hand_select(state, Memory()) if a[0] == "combat_select_card"][0]
        self.assertEqual(first(), 0)  # 20 + 20 + 9 + 11 = 60: the Escape is the card nothing is lost with
        # With the boss out of reach the Escape is kept as before, and the lowest-rated card goes.
        boss["hp"] = 200
        self.assertNotEqual(first(), 0)

    def test_out_of_a_fight_the_rating_decides_as_before(self):
        from sts2bot.fight import handpick
        state = self.insatiable("Choose a card to Discard.")
        state["battle"]["enemies"] = []
        self.assertIsNone(handpick.turn_costs(state, state["hand_select"]["cards"]))

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

    def big_hand(self):
        """Ten cards in hand, six of them copies, in a fight the plan can be made for."""
        hand = [STRIKE(0), STRIKE(1), STRIKE(2), DEFEND(3), DEFEND(4), DEFEND(5), DEFEND(6), DEFEND(7),
                card(8, "BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy"),
                card(9, "SHRUG", "Shrug It Off", "Skill", 1, "Gain 8 Block. Draw 1 card.", "Self")]
        state = combat_state(hand, [enemy("A_0", 60, 12), enemy("B_1", 40, 9)], energy=3, hp=60, max_hp=80)
        state["state_type"] = "hand_select"
        state["hand_select"] = {"prompt": "Choose a card to Discard.", "can_confirm": False, "cards": hand}
        return state

    def test_a_confirmation_does_not_wait_for_the_plan(self):
        # The plan per card took about two seconds with ten in hand, and the confirmation is first to go out anyway.
        state = self.big_hand()
        state["hand_select"]["can_confirm"] = True
        with mock.patch.object(combat, "choose_card", side_effect=AssertionError("planned")):
            acts = strategy.hand_select(state, Memory())
        self.assertEqual(acts[0], ("combat_confirm_selection", {}))
        self.assertIn("combat_select_card", [a[0] for a in acts])  # (the fall-back stays, by rating)

    def test_each_kind_of_card_is_planned_once(self):
        from sts2bot.fight import handpick

        state = self.big_hand()
        handpick._answers.clear()
        real = combat.choose_card
        with mock.patch.object(combat, "choose_card", side_effect=real) as spy:
            costs = handpick.turn_costs(state, state["hand_select"]["cards"], tolerance=-1e9)  # (never stopping early)
        self.assertEqual(spy.call_count, 5)  # the whole hand, then Strike, Defend, Bash, Shrug It Off: not ten
        self.assertEqual({costs[i] for i in (0, 1, 2)}, {costs[0]})  # (copies cost the same)
        self.assertEqual({costs[i] for i in (3, 4, 5, 6, 7)}, {costs[3]})

    def test_planning_stops_at_a_card_the_turn_does_not_miss(self):
        from sts2bot.fight import handpick

        state = self.big_hand()
        handpick._answers.clear()
        real = combat.choose_card
        with mock.patch.object(combat, "choose_card", side_effect=real) as spy:
            costs = handpick.turn_costs(state, state["hand_select"]["cards"])
        self.assertEqual(spy.call_count, 2)  # the whole hand and the hand without the first card: nothing lost
        self.assertEqual(costs[0], 0.0)
        self.assertEqual(costs[9], handpick.UNPLANNED)  # (the rest keep their order, after it)

    def test_a_card_that_weighs_on_the_hand_is_always_planned_for(self):
        from sts2bot.fight import handpick

        state = self.big_hand()
        burn = card(10, "BURN", "Burn", "Status", 0, "Unplayable. At the end of your turn, take 2 damage.")
        state["hand_select"]["cards"] = state["hand_select"]["cards"] + [burn]
        handpick._answers.clear()
        costs = handpick.turn_costs(state, state["hand_select"]["cards"])
        self.assertNotEqual(costs[10], handpick.UNPLANNED)  # (a Status card may be a burn: it is never skipped over)

    def test_the_same_selection_is_not_planned_again(self):
        from sts2bot.fight import handpick

        state = self.big_hand()
        handpick._answers.clear()
        first = handpick.turn_costs(state, state["hand_select"]["cards"])
        with mock.patch.object(combat, "choose_card", side_effect=AssertionError("planned again")):
            again = handpick.turn_costs(state, state["hand_select"]["cards"])
            state["hand_select"]["selected"] = [{"index": 3}]  # (a click made on the screen changes nothing the plan reads)
            after_click = handpick.turn_costs(state, state["hand_select"]["cards"])
        self.assertEqual(first, again)
        self.assertEqual(first, after_click)
        state["battle"]["enemies"][0]["hp"] = 5  # (another fight is another question)
        with mock.patch.object(combat, "choose_card", wraps=combat.choose_card) as spy:
            handpick.turn_costs(state, state["hand_select"]["cards"])
        self.assertGreater(spy.call_count, 0)


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


class Crowd(unittest.TestCase):
    """Other players' runs as a second opinion on a card (crowd.py)."""

    SOW = card(0, "SOW", "Sow", "Attack", 1, "Retain. Deal 8 damage to ALL enemies.", "AllEnemies")
    DEFY = card(1, "DEFY", "Defy", "Skill", 1, "Gain 6 Block. Apply 1 Weak.", "AnyEnemy")

    def test_every_card_in_the_tables_gets_a_rating(self):
        from sts2bot.run import crowd

        self.assertEqual(len(crowd.RATING), 5 * 80 + 33)
        self.assertTrue(all(1.5 <= v <= 8.0 for v in crowd.RATING.values()))
        self.assertGreater(crowd.rating("AFTERIMAGE"), crowd.rating("DAGGER_SPRAY") + 3.0)
        # A card known by its name only is spelt differently from its id.
        self.assertEqual(crowd.rating("TIME_S_UP"), crowd.rating("TIMES_UP"))
        self.assertIsNone(crowd.rating("STRIKE_NECROBINDER"))

    def test_the_crowd_moves_a_rating_only_as_far_as_the_setting_says(self):
        from unittest import mock

        with mock.patch.dict(learn.KNOB, {"crowd": 0.0}):
            ours = deck_value(self.SOW)
            self.assertGreater(ours, deck_value(self.DEFY))  # our own table: the attack first
            self.assertEqual(deck_value({**self.SOW, "is_upgraded": True}), ours + 0.7)
        with mock.patch.dict(learn.KNOB, {"crowd": 0.6}):
            self.assertLess(deck_value(self.SOW), deck_value(self.DEFY))  # a quarter of the runs with Sow were won
            self.assertLess(deck_value(self.SOW), ours)
            # A card our table does not list is read from its text, and that is mixed the same way.
            demesne = card(2, "DEMESNE", "Demesne", "Power", 3, "Ethereal. At the start of your turn, gain [necrobinder_energy_icon.png] and draw 1 additional card.", rarity="Rare")
            self.assertGreater(deck_value(demesne), 5.0)
            # ... but not for a card that is only good in hands that can do what the bot cannot.
            stoke = card(3, "STOKE", "Stoke", "Skill", 1, "Exhaust your Hand. Add 1 random card into your Hand for each card Exhausted.", rarity="Rare")
            self.assertEqual(deck_value(stoke), 2.0)

    def test_a_relic_can_limit_the_cards_played_in_a_turn(self):
        state = combat_state([STRIKE(i) for i in range(3)], [enemy("JAW_WORM_0", 40)])
        self.assertIsNone(combat.cards_left_this_turn(state))
        state["player"]["relics"] = [{"id": "VELVET_CHOKER", "description": "Gain [ironclad_energy_icon.png] at the start of each turn. You cannot play more than 6 cards per turn."}]
        self.assertEqual(combat.cards_left_this_turn({**state, "_plays_this_turn": 4}), 2)
        self.assertIsNone(combat.choose_card({**state, "_plays_this_turn": 6}))
        self.assertIsNotNone(combat.choose_card({**state, "_plays_this_turn": 5}))

    def test_snecko_eye_is_bought_like_any_relic(self):
        items = [{"index": 0, "category": "relic", "price": 150, "is_stocked": True, "can_afford": True, "relic_id": "SNECKO_EYE"}]
        state = {"state_type": "shop", "run": {"act": 1, "floor": 5}, "player": {"hp": 80, "max_hp": 80, "gold": 250, "potions": []}, "shop": {"items": items, "can_proceed": True}}
        self.assertEqual(strategy.shop(state, Memory())[0], ("shop_purchase", {"index": 0}))


if __name__ == "__main__":
    unittest.main()
