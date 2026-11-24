"""Potions kept for the boss: on the belt, at the Merchant and in events."""

import unittest

from sts2bot.fight import combat
from sts2bot.run import events, strategy
from sts2bot.run.strategy import Memory

from ..helpers import STRIKE, combat_state, enemy


def potion(slot, name, text, target="AnyPlayer"):
    return {"slot": slot, "name": name, "description": text, "target_type": target, "can_use_in_combat": True}


def bought(items, floor, gold, held):
    """What a shop of these items sells, in order, to a belt of two slots holding `held` Fire Potions."""
    player = {"gold": gold, "hp": 60, "max_hp": 75, "max_potion_slots": 2, "potions": [potion(i, "Fire Potion", "Deal 20 damage.") for i in range(held)], "relics": []}
    state = {"state_type": "shop", "run": {"act": 3 if floor > 33 else 2, "floor": floor}, "player": player, "shop": {"items": items}}
    return [a[1].get("index") for a in strategy.shop(state, Memory()) if a[0] == "shop_purchase"]


class KeptForTheBoss(unittest.TestCase):
    def drunk(self, floor, hp=75):
        belt = [potion(0, "Strength Potion", "Gain 2 Strength."), potion(1, "Swift Potion", "Draw 3 cards.")]
        state = combat_state([STRIKE(0)], [enemy("A", 60, attack=10)], hp=hp, max_hp=75, potions=belt)
        state["run"]["floor"] = floor
        state["player"]["max_potion_slots"] = 2
        return len(combat.potion_actions(state))

    def test_a_full_belt_is_no_reason_to_drink_anywhere_in_act_3(self):
        # The last boss was never beaten with fewer than two potions, and a slot emptied in act 3 stays empty half the time.
        self.assertEqual([self.drunk(f) for f in (34, 36, 40, 46)], [0, 0, 0, 0])
        self.assertEqual(self.drunk(20), 1)  # early in act 2 the spare one still goes
        self.assertEqual(self.drunk(5), 1)
        self.assertGreater(self.drunk(36, hp=12), 0)  # ... and a fight gone wrong gets them in any act

    def test_an_act_3_merchant_fills_the_belt_first(self):
        shelf = lambda: [{"index": 0, "category": "relic", "relic_id": "ANCHOR", "relic_name": "Anchor", "price": 150, "is_stocked": True},
                         {"index": 1, "category": "potion", "potion_name": "Fire Potion", "potion_description": "Deal 20 damage.", "price": 50, "is_stocked": True}]
        self.assertEqual(bought(shelf(), 38, 160, 1)[0], 1)  # the potion before the relic, ten floors from the last boss
        self.assertEqual(bought(shelf(), 22, 160, 1)[0], 0)  # in act 2, far from the boss, the relic

    def test_a_merchant_before_the_boss_sells_the_best_potion_first(self):
        # (All three stood at 9.5, and the first listed, Liquid Bronze, was bought.)
        shelf = [(0, "Liquid Bronze", 76), (1, "Heart of Iron", 78), (2, "Radiant Tincture", 76)]
        items = [{"index": i, "category": "potion", "potion_name": n, "potion_description": "", "price": p, "is_stocked": True, "can_afford": True} for i, n, p in shelf]
        for held in (0, 1):
            self.assertEqual(bought(items, 31, 100, held), [2, 1, 0])

    def test_an_event_that_takes_a_potion_is_charged_for_it(self):
        belt = [potion(0, "Attack Potion", "Add 1 of 3 random Attack cards into your Hand."), potion(1, "Strength Potion", "Gain 2 Strength.")]
        value = lambda title, text="Obtain a random Relic.", act=3, held=belt: events._event_option_value({"title": title, "description": text}, 60, 80, act=act, potions=held)
        self.assertGreater(value("Give 100 Gold"), value("Give Attack Potion"))
        self.assertGreater(value("Give Attack Potion"), value("Give Strength Potion"))  # the better potion costs more
        self.assertEqual(value("Give Fire Potion"), value("Take It"))  # a potion we do not hold: nothing to charge
        lose = "Lose Strength Potion. Obtain an Upgraded Common Skill."
        self.assertLess(value("Insert Common Potion", lose), value("Insert Common Potion", lose, act=1))  # dearer in act 3
        # A potion on offer is worth more to a belt with room for it.
        self.assertGreater(value("Bottle", "Procure 1 random Potion.", held=[]), value("Bottle", "Procure 1 random Potion."))
