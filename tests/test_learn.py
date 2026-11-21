"""The card record: what a pick is measured against, and the kinds a card starts from."""

import tempfile
import unittest
from pathlib import Path

from sts2bot import learn
from sts2bot.cards import kind

from .fake_game import card as _card


def card(name, ctype, cost, text):
    return _card(0, name.upper().replace(" ", "_"), name, ctype, cost, text)


class Kinds(unittest.TestCase):
    def test_a_card_is_free_gives_energy_or_only_brings_cards(self):
        self.assertEqual(kind(card("Poke", "Attack", 0, "Osty deals 6 damage.")), "free")
        self.assertEqual(kind(card("Wisp", "Skill", 0, "Gain [necrobinder_energy_icon.png]. Exhaust.")), "energy")
        self.assertEqual(kind(card("Invoke", "Skill", 1, "Next turn, Summon 2 and gain [necrobinder_energy_icon.png][necrobinder_energy_icon.png].")), "energy")
        self.assertEqual(kind(card("Parse", "Skill", 1, "Ethereal. Draw 3 cards.")), "draw")
        self.assertEqual(kind(card("Capture Spirit", "Skill", 1, "Enemy loses 3 HP. Add 3 Souls into your Draw Pile.")), "draw")
        self.assertEqual(kind(card("Dredge", "Skill", 1, "Put 3 cards from your Discard Pile into your Hand. Exhaust.")), "draw")

    def test_most_cards_are_no_kind(self):
        self.assertEqual(kind(card("Strike", "Attack", 1, "Deal 6 damage.")), "")
        self.assertEqual(kind(card("Scourge", "Skill", 1, "Apply 13 Doom. Draw 1 card.")), "")  # it does its own work as well
        self.assertEqual(kind(card("Grave Warden", "Skill", 1, "Gain 8 Block. Add a Soul into your Draw Pile.")), "")
        self.assertEqual(kind(card("Shared Fate", "Skill", 0, "Lose 2 Strength. Enemy loses 2 Strength. Exhaust.")), "")  # a free Skill
        self.assertEqual(kind(card("Friendship", "Power", 1, "Lose 2 Strength. Gain [necrobinder_energy_icon.png] at the start of each turn.")), "")
        self.assertEqual(kind(card("Graveblast", "Attack", 1, "Deal 4 damage. Put a card from your Discard Pile into your Hand. Exhaust.")), "")


class Record(unittest.TestCase):
    def test_a_boss_reward_is_not_credited_with_beating_the_boss(self):
        stats = learn.CardStats(None)
        for floor in [17] * 6 + [30] * 6:  # half the runs end at the act 1 boss
            stats.record("The Regent", floor, False, [])
        # A run that beat the boss, took a card from its reward and another on the next floor, and ended on 30:
        # no further than the runs that left floor 17 alive usually get, whichever card is asked.
        stats.record("The Regent", 30, False, [[17, "Big Bang"], [18, "Glow"]])
        table = stats.cards["The Regent"]
        self.assertEqual(table["BIG_BANG"], [1.0, 0.0])
        self.assertEqual(table["GLOW"], [1.0, 0.0])

    def test_a_card_starts_where_its_kind_stands(self):
        stats = learn.CardStats(None)
        stats.finals["The Necrobinder"] = [25] * 20
        # Free attacks have run 4 floors a pick over the other picks, cards that only draw 4 under.
        stats.cards["The Necrobinder"] = {"POKE": [200.0, 800.0], "PARSE": [200.0, -800.0], "DEFILE": [400.0, 0.0]}
        stats.kinds.update(POKE="free", PARSE="draw", DEFILE="")
        plain = stats.shift("The Necrobinder", "Reave", "")
        self.assertGreater(stats.shift("The Necrobinder", "Fetch", "free"), plain + 0.5)
        self.assertLess(stats.shift("The Necrobinder", "Dredge", "draw"), plain - 0.5)
        # ... for every character (the kinds are pooled), and never further than MAX_KIND.
        stats.finals["The Defect"] = [25] * 20
        self.assertGreater(stats.shift("The Defect", "Claw", "free"), stats.shift("The Defect", "Zap", "") + 0.5)
        self.assertLessEqual(stats.kind_shift("free"), learn.MAX_KIND)

    def test_the_free_lean_setting_takes_the_start_off_free_attacks_only(self):
        stats = learn.CardStats(None)
        stats.finals["The Necrobinder"] = [25] * 20
        # Par is zero here, so a free attack's kind is worth its whole record: 800 floors over 200 picks.
        stats.cards["The Necrobinder"] = {"POKE": [200.0, 800.0], "DEFILE": [200.0, -800.0]}
        stats.kinds.update(POKE="free", DEFILE="draw")
        saved = learn.KNOB["free_lean"]
        try:
            learn.KNOB["free_lean"] = 1.0
            on = stats.shift("The Necrobinder", "Fetch", "free")
            learn.KNOB["free_lean"] = 0.0
            off = stats.shift("The Necrobinder", "Fetch", "free")
            # Only free attacks are affected: a draw card starts from its kind at either setting.
            learn.KNOB["free_lean"] = 1.0
            draw_on = stats.shift("The Necrobinder", "Dredge", "draw")
            learn.KNOB["free_lean"] = 0.0
            draw_off = stats.shift("The Necrobinder", "Dredge", "draw")
        finally:
            learn.KNOB["free_lean"] = saved
        self.assertGreater(on, off + 1.0)  # about 1.5 floors of kind start, less at 0.0
        self.assertEqual(draw_on, draw_off)
        self.assertIn("free_lean", learn.CHOICES)
        self.assertEqual(learn.DEFAULTS["free_lean"], 1.0)  # the arm that is today's behaviour

    def test_a_cards_own_record_outweighs_its_kind_in_the_end(self):
        stats = learn.CardStats(None)
        stats.finals["The Necrobinder"] = [25] * 20
        stats.cards["The Necrobinder"] = {"POKE": [300.0, 900.0], "DEFILE": [1500.0, 2700.0], "RIGHT_HAND_HAND": [1200.0, -3600.0]}
        stats.kinds.update(POKE="free", RIGHT_HAND_HAND="free", DEFILE="")
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
        from sts2bot import strategy

        poke = {"index": 0, "category": "card", "price": 40, "is_stocked": True, "can_afford": True, "card_id": "POKE", "card_name": "Poke",
                "card_type": "Attack", "card_cost": "0", "card_rarity": "Common", "card_description": "Osty deals 6 damage."}
        state = {"state_type": "shop", "run": {"act": 2, "floor": 20}, "player": {"character": "The Necrobinder", "hp": 60, "max_hp": 66, "gold": 60, "relics": [], "potions": []},
                 "shop": {"items": [poke], "can_proceed": True}}
        mem = strategy.Memory()
        self.assertEqual(strategy.shop(state, mem)[0][0], "proceed")  # rated 3.6 by the tables: under the bar
        stats = learn.CardStats(None)
        stats.finals["The Necrobinder"] = [25] * 20
        stats.cards["The Necrobinder"] = {"POKE": [800.0, 2400.0], "DEFILE": [800.0, -2400.0]}  # ... and 3 floors a pick over par in our runs
        mem = strategy.Memory(card_stats=stats)
        self.assertEqual(strategy.shop(state, mem)[0], ("shop_purchase", {"index": 0}))


if __name__ == "__main__":
    unittest.main()
