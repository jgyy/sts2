"""Shop relics bought by what they do, not by price alone (relicworth.py)."""

import unittest

from sts2bot.run import events, relicworth, strategy
from sts2bot.run.strategy import Memory


def relic(i, rid, name, price):
    return {"index": i, "category": "relic", "relic_id": rid, "relic_name": name, "relic_description": "", "price": price, "is_stocked": True}


def bought(items, gold, act=2, deck=(), hp=60, max_hp=75):
    player = {"gold": gold, "hp": hp, "max_hp": max_hp, "max_potion_slots": 2, "potions": [], "relics": []}
    state = {"state_type": "shop", "run": {"act": act, "floor": 8 if act == 1 else 22 if act == 2 else 38}, "player": player, "shop": {"items": items}}
    mem = Memory()
    mem.deck = list(deck)
    return [a[1]["index"] for a in strategy.shop(state, mem) if a[0] == "shop_purchase"]


X_CARD = {"name": "Whirlwind", "type": "Attack", "cost": "X", "description": "Deal 5 damage to ALL enemies X times."}
STRIKE = {"name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."}


class ShopRelics(unittest.TestCase):
    def test_the_best_relic_before_the_cheapest(self):
        # (2026-11-22 00:00, a Necrobinder in act 2 with 272 gold: the old rule bought the Cauldron.)
        shelf = [relic(0, "PANTOGRAPH", "Pantograph", 233), relic(1, "MERCURY_HOURGLASS", "Mercury Hourglass", 210), relic(2, "CAULDRON", "Cauldron", 203)]
        self.assertEqual(bought(shelf, 272)[0], 0)

    def test_a_relic_the_bot_cannot_use_only_out_of_plenty(self):
        shelf = [relic(0, "MINIATURE_TENT", "Miniature Tent", 250)]
        self.assertEqual(bought(shelf, 300), [])
        self.assertEqual(bought(shelf, 460), [0])

    def test_chemical_x_needs_x_cost_cards(self):
        shelf = [relic(0, "CHEMICAL_X", "Chemical X", 195), relic(1, "PEN_NIB", "Pen Nib", 234)]
        self.assertEqual(bought(shelf, 237, deck=[STRIKE] * 5), [1])  # (2026-11-21 23:04: the old rule bought Chemical X)
        self.assertLess(relicworth.worth("CHEMICAL_X", [STRIKE]), relicworth.worth("CHEMICAL_X", [X_CARD]))
        self.assertGreaterEqual(relicworth.worth("CHEMICAL_X", [X_CARD]), 0.0)  # one X card: an ordinary relic, two: a good one

    def test_lees_waffle_is_worth_the_hp_it_gives_back(self):
        self.assertGreater(relicworth.worth("LEES_WAFFLE", hp=25, max_hp=75), relicworth.worth("LEES_WAFFLE", hp=70, max_hp=75))

    def test_what_pays_over_the_run_is_worth_less_late(self):
        self.assertGreater(relicworth.worth("MEMBERSHIP_CARD", act=1), relicworth.worth("MEMBERSHIP_CARD", act=3))
        self.assertEqual(relicworth.worth("ANCHOR", act=1), relicworth.worth("ANCHOR", act=3))

    def test_an_unknown_relic_is_an_ordinary_one(self):
        self.assertEqual(relicworth.worth("SOMETHING_NEW"), 0.0)
        shelf = [relic(0, "SOMETHING_NEW", "Something New", 200)]
        self.assertEqual(bought(shelf, 210), [0])  # (as before: 8.0 - price / 200)

    def test_an_energy_every_turn_is_bought_over_an_ordinary_relic(self):
        # (2026-11-22 14:40: the Demon model puts one energy a turn above Pantograph's 25 HP; the fight-cost table had it at 0.1-1.0.)
        shelf = [relic(0, "MERCURY_HOURGLASS", "Mercury Hourglass", 150), relic(1, "PAELS_FLESH", "Pael's Flesh", 190)]
        self.assertEqual(bought(shelf, 200)[0], 1)  # (the first purchase: the bot re-plans after each one)
        self.assertGreater(relicworth.worth("PAELS_FLESH", act=2), relicworth.worth("PANTOGRAPH", act=2))

    def test_a_pumpkin_candle_is_worth_less_late(self):
        # It goes out after five combats, so an act 3 boss may find it spent.
        self.assertGreater(relicworth.worth("PUMPKIN_CANDLE", act=1), relicworth.worth("PUMPKIN_CANDLE", act=3))

    def test_every_rated_relic_is_one_the_game_has(self):
        # (A misspelt id would be rated as ordinary without a word.)
        import json
        from pathlib import Path
        wiki = Path.home() / ".local/state/sts2bot/wiki.json"
        if not wiki.exists():
            self.skipTest("no wiki dump")
        known = set(json.loads(wiki.read_text())["relics"])
        missing = sorted(r for r in set(relicworth.WORTH) | relicworth.LONG_RUN | set(relicworth._DECK) | {"LEES_WAFFLE"} if r not in known)
        self.assertEqual(missing, [])


class RestSiteRelics(unittest.TestCase):
    OSTY = {"name": "Poke", "type": "Attack", "cost": 1, "description": "Osty deals 6 damage."}

    def chosen(self, hp, ids=("HEAL", "SMITH", "LIFT"), deck=None):
        options = [{"index": i, "id": oid, "name": oid.title(), "is_enabled": True} for i, oid in enumerate(ids)]
        state = {"state_type": "rest_site", "run": {"act": 2, "floor": 25}, "player": {"hp": hp, "max_hp": 80}, "rest_site": {"options": options}}
        mem = Memory()
        mem.deck = list(deck or [])
        first = [a for a in strategy.rest_site(state, mem) if a[0] == "choose_rest_option"][0]
        return ids[first[1]["index"]]

    def test_girya_and_shovel_are_used(self):
        # (Until 2026-11-22 they ranked under Smith: 31 runs held Girya and 26 Shovel, and none lifted or dug.)
        self.assertEqual(self.chosen(78, deck=[STRIKE] * 6), "LIFT")
        self.assertEqual(self.chosen(78, ("HEAL", "SMITH", "DIG")), "DIG")
        self.assertEqual(self.chosen(30, deck=[STRIKE] * 6), "HEAL")  # healing first when it is needed

    def test_no_lift_for_a_deck_that_does_not_hit(self):
        self.assertEqual(self.chosen(78, deck=[self.OSTY] * 6 + [STRIKE]), "SMITH")


class SculptingStrike(unittest.TestCase):
    def test_ethereal_goes_on_what_the_deck_is_better_without(self):
        # (Until 2026-11-22 the best card in hand was marked, and Exhausted when it was not played that turn.)
        hand = [{"index": 0, "name": "Grave Warden", "type": "Skill", "cost": 1, "description": "Gain 8 Block. Summon 3."},
                {"index": 1, "name": "Ascender's Bane", "type": "Curse", "cost": -1, "description": "Unplayable. Ethereal."},
                {"index": 2, "name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."},
                {"index": 3, "name": "Defend+", "type": "Skill", "cost": 1, "description": "Gain 8 Block."}]
        state = {"state_type": "hand_select", "player": {"energy": 3, "hand": hand}, "hand_select": {"prompt": "Choose a card to add Ethereal.", "cards": hand}}
        first = [a for a in strategy.hand_select(state, Memory()) if a[0] == "combat_select_card"][0]
        self.assertEqual(hand[first[1]["card_index"]]["name"], "Strike")


class RelicsTraded(unittest.TestCase):
    HELD = [{"id": r} for r in ("GORGET", "JOSS_PAPER", "BAG_OF_PREPARATION", "BEATING_REMNANT", "PAPER_KRANE")]

    def value(self, title, text):
        return events._event_option_value({"title": title, "description": text}, 60, 75, act=2, potions=[], held=self.HELD)

    def test_the_relic_trader_takes_the_best_trade(self):
        # (2026-11-21 23:52: all three scored 0.5 and the top one, Gorget for Akabeko, was taken.)
        top = self.value("Take the Top One", "Trade Gorget for Akabeko.")
        middle = self.value("Take the Middle One", "Trade Joss Paper for Gremlin Horn.")
        bottom = self.value("Take the Bottom One", "Trade Bag of Preparation for Lucky Fysh.")
        self.assertGreater(middle, top)
        self.assertGreater(top, bottom)

    def test_ranwid_keeps_a_good_relic(self):
        gold = self.value("Give 100 Gold", "Obtain a random Relic.")
        self.assertGreater(self.value("Give Paper Krane", "Obtain 2 random Relics."), gold)  # two relics for an ordinary one
        self.assertLess(self.value("Give Beating Remnant", "Obtain 2 random Relics."), gold)

    def test_a_relic_we_do_not_hold_is_not_charged(self):
        self.assertEqual(self.value("Give Anchor", "Obtain a random Relic."), events._event_option_value({"title": "Give Anchor", "description": "Obtain a random Relic."}, 60, 75, act=2, potions=[]))


if __name__ == "__main__":
    unittest.main()
