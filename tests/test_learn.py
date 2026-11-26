"""The card record: what a pick is measured against, and the kinds a card starts from."""

import tempfile
import unittest
from pathlib import Path
from unittest import mock

from sts2bot import learn
from sts2bot.cards import kind
from sts2bot.run import strategy

from .fake_game import card as _card

E = "[necrobinder_energy_icon.png]"


def card(name, ctype, cost, text):
    return _card(0, name.upper().replace(" ", "_"), name, ctype, cost, text)


def necro(cards, **kinds):
    """A record of Necrobinder runs that end on floor 25, with these cards' [picks, floors gained] and kinds."""
    stats = learn.CardStats(None)
    stats.finals["The Necrobinder"] = [25] * 20
    stats.cards["The Necrobinder"] = cards
    stats.kinds.update(kinds)
    return stats


class Kinds(unittest.TestCase):
    def test_a_card_is_free_gives_energy_or_only_brings_cards(self):
        self.assertEqual(kind(card("Poke", "Attack", 0, "Osty deals 6 damage.")), "free")
        self.assertEqual(kind(card("Wisp", "Skill", 0, f"Gain {E}. Exhaust.")), "energy")
        self.assertEqual(kind(card("Invoke", "Skill", 1, f"Next turn, Summon 2 and gain {E}{E}.")), "energy")
        self.assertEqual(kind(card("Parse", "Skill", 1, "Ethereal. Draw 3 cards.")), "draw")
        self.assertEqual(kind(card("Capture Spirit", "Skill", 1, "Enemy loses 3 HP. Add 3 Souls into your Draw Pile.")), "draw")
        self.assertEqual(kind(card("Dredge", "Skill", 1, "Put 3 cards from your Discard Pile into your Hand. Exhaust.")), "draw")

    def test_most_cards_are_no_kind(self):
        self.assertEqual(kind(card("Strike", "Attack", 1, "Deal 6 damage.")), "")
        self.assertEqual(kind(card("Scourge", "Skill", 1, "Apply 13 Doom. Draw 1 card.")), "")  # it does its own work as well
        self.assertEqual(kind(card("Grave Warden", "Skill", 1, "Gain 8 Block. Add a Soul into your Draw Pile.")), "")
        self.assertEqual(kind(card("Shared Fate", "Skill", 0, "Lose 2 Strength. Enemy loses 2 Strength. Exhaust.")), "")  # a free Skill
        self.assertEqual(kind(card("Friendship", "Power", 1, f"Lose 2 Strength. Gain {E} at the start of each turn.")), "")
        self.assertEqual(kind(card("Graveblast", "Attack", 1, "Deal 4 damage. Put a card from your Discard Pile into your Hand. Exhaust.")), "")


class Record(unittest.TestCase):
    def test_a_boss_reward_is_not_credited_with_beating_the_boss(self):
        stats = learn.CardStats(None)
        for floor in [17] * 6 + [30] * 6:  # half the runs end at the act 1 boss
            stats.record("The Regent", floor, False, [])
        # Cards from the boss's reward and the next floor, run ended on 30: no further than the runs that left floor 17 alive get.
        stats.record("The Regent", 30, False, [[17, "Big Bang"], [18, "Glow"]])
        self.assertEqual(stats.cards["The Regent"]["BIG_BANG"], [1.0, 0.0])
        self.assertEqual(stats.cards["The Regent"]["GLOW"], [1.0, 0.0])

    def test_a_card_starts_where_its_kind_stands(self):
        # Free attacks have run 4 floors a pick over the other picks, cards that only draw 4 under.
        stats = necro({"POKE": [200.0, 800.0], "PARSE": [200.0, -800.0], "DEFILE": [400.0, 0.0]}, POKE="free", PARSE="draw", DEFILE="")
        plain = stats.shift("The Necrobinder", "Reave", "")
        self.assertGreater(stats.shift("The Necrobinder", "Fetch", "free"), plain + 0.5)
        self.assertLess(stats.shift("The Necrobinder", "Dredge", "draw"), plain - 0.5)
        # ... for every character (the kinds are pooled), and never further than MAX_KIND.
        stats.finals["The Defect"] = [25] * 20
        self.assertGreater(stats.shift("The Defect", "Claw", "free"), stats.shift("The Defect", "Zap", "") + 0.5)
        self.assertLessEqual(stats.kind_shift("free"), learn.MAX_KIND)

    def test_the_free_lean_setting_takes_the_start_off_free_attacks_only(self):
        # Par is zero here, so a free attack's kind is worth its whole record: 800 floors over 200 picks.
        stats = necro({"POKE": [200.0, 800.0], "DEFILE": [200.0, -800.0]}, POKE="free", DEFILE="draw")

        def shift(lean, name, its_kind):
            with mock.patch.dict(learn.KNOB, {"free_lean": lean}):
                return stats.shift("The Necrobinder", name, its_kind)

        on, off = shift(1.0, "Fetch", "free"), shift(0.0, "Fetch", "free")
        self.assertGreater(on, off + 1.0)  # about 1.5 floors of kind start, less at 0.0
        self.assertEqual(shift(1.0, "Dredge", "draw"), shift(0.0, "Dredge", "draw"))  # a draw card starts from its kind at either setting
        self.assertIn("free_lean", learn.CHOICES)
        self.assertEqual(learn.DEFAULTS["free_lean"], 1.0)  # the arm that is today's behaviour

    def test_a_cards_own_record_outweighs_its_kind_in_the_end(self):
        stats = necro({"POKE": [300.0, 900.0], "DEFILE": [1500.0, 2700.0], "RIGHT_HAND_HAND": [1200.0, -3600.0]}, POKE="free", RIGHT_HAND_HAND="free", DEFILE="")
        # Its kind is 3 floors over par without it; its own 1,200 picks were 3 under.
        self.assertGreater(stats.kind_shift("free", "RIGHT_HAND_HAND"), 1.0)
        self.assertLess(stats.shift("The Necrobinder", "Right Hand Hand", "free"), -0.5)

    def test_the_kinds_are_kept_with_the_record(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "card_stats.json"
            stats = learn.CardStats(path)
            for _ in range(6):
                stats.record("The Necrobinder", 20, False, [])
            stats.shift("The Necrobinder", "Poke+", "free")
            stats.record("The Necrobinder", 30, False, [[3, "Poke+"]])
            again = learn.CardStats(path)
            self.assertEqual(again.kinds, {"POKE": "free"})
            self.assertEqual(again.shift("The Necrobinder", "Poke"), stats.shift("The Necrobinder", "Poke"))


class AtTheMerchant(unittest.TestCase):
    def test_a_card_on_the_shelf_is_rated_with_its_record_too(self):
        poke = {"index": 0, "category": "card", "price": 40, "is_stocked": True, "can_afford": True, "card_id": "POKE", "card_name": "Poke",
                "card_type": "Attack", "card_cost": "0", "card_rarity": "Common", "card_description": "Osty deals 6 damage."}
        player = {"character": "The Necrobinder", "hp": 60, "max_hp": 66, "gold": 60, "relics": [], "potions": []}
        state = {"state_type": "shop", "run": {"act": 2, "floor": 20}, "player": player, "shop": {"items": [poke], "can_proceed": True}}
        self.assertEqual(strategy.shop(state, strategy.Memory())[0][0], "proceed")  # rated 3.6 by the tables: under the bar
        stats = necro({"POKE": [800.0, 2400.0], "DEFILE": [800.0, -2400.0]})  # ... and 3 floors a pick over par in our runs
        self.assertEqual(strategy.shop(state, strategy.Memory(card_stats=stats))[0], ("shop_purchase", {"index": 0}))


class ModelTable(unittest.TestCase):
    """learn.model_shift: what the model runs say of a card (`python3 -m sim prior`), weighed by the `model_lean` setting."""

    def test_the_table_counts_only_as_far_as_the_setting_says(self):
        table = {"sts2": {"SILENT": {"FOOTWORK": 1.5, "DEFLECT": -2.0}}, "sts1": {"SILENT": {"FOOTWORK": -1.0}}}
        with mock.patch.object(learn, "_model", table):
            with mock.patch.dict(learn.KNOB, {"model_lean": 0.0}):
                self.assertEqual(learn.model_shift("The Silent", "Footwork"), 0.0)  # off: today's behaviour
            with mock.patch.dict(learn.KNOB, {"model_lean": 1.0}):
                self.assertEqual(learn.model_shift("The Silent", "Footwork+"), 1.5)  # (upgraded or not, by its id)
                self.assertEqual(learn.model_shift("THE_SILENT", "Deflect"), -2.0)  # (the menu's spelling of her)
                self.assertEqual(learn.model_shift("The Silent", "Neutralize"), 0.0)  # a card the table leaves out
                self.assertEqual(learn.model_shift("The Defect", "Footwork"), 0.0)  # ... and a character
                with mock.patch("sts2bot.games._active", "sts1"):
                    self.assertEqual(learn.model_shift("The Silent", "Footwork"), -1.0)  # each game its own table
            with mock.patch.dict(learn.KNOB, {"model_lean": 2.0}):
                self.assertEqual(learn.model_shift("The Silent", "Deflect"), -4.0)  # (twice as far: Slay the Spire 2's default)
        self.assertEqual(learn.DEFAULTS["model_lean"], 0.0)

    def test_the_shipped_table_reads(self):
        table = learn._load(learn.MODEL_CARDS, "model card table")
        self.assertTrue(table, "run `python3 -m sim prior` to make sts2bot/run/model_cards.json")
        for game, chars in table.items():
            self.assertIn(game, ("sts1", "sts2"))
            for who, cards in chars.items():
                self.assertEqual(who, learn.who_key(who))
                self.assertTrue(all(abs(v) <= 3.0 for v in cards.values()), who)


class GameDefaults(unittest.TestCase):
    def test_each_game_plays_its_own_best_settings_without_tuning(self):
        with mock.patch.dict(learn.GAME_DEFAULTS, {"sts2": {"crowd": 0.3}, "sts1": {"crowd": 0.6}}):
            self.assertEqual(learn.defaults("sts2")["crowd"], 0.3)
            self.assertEqual(learn.defaults("sts1")["crowd"], 0.6)
            with mock.patch("sts2bot.games._active", "sts1"):
                self.assertEqual(learn.defaults(), learn.defaults("sts1"))
            self.assertEqual(learn.defaults("sts2")["caution"], learn.DEFAULTS["caution"])  # (the rest as DEFAULTS)

    def test_every_game_default_is_a_setting(self):
        for game, table in learn.GAME_DEFAULTS.items():
            self.assertLessEqual(set(table), set(learn.DEFAULTS), game)
