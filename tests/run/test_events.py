"""Events priced by the HP they cost: a fight an event starts, and relics that come only after five more fights."""

import unittest

from sts2bot.run import events

value = events._event_option_value


class EventFights(unittest.TestCase):
    punch = {"title": "I Can Take Them", "description": "Fight them for Greater Rewards."}
    nab = {"title": "Nab", "description": "Add Injury to your Deck. Obtain a random Relic."}

    def test_a_fight_an_event_starts_costs_its_hp_so_punch_off_loses_to_nab(self):
        # Taken at 55/73 (the fight took 44 HP, the walk on left 2) and at 37/66 (a death); rated 4.5 at any HP, Nab 3.5.
        self.assertLess(value(self.punch, 55, 73, act=1), value(self.nab, 55, 73, act=1))
        self.assertLess(value(self.punch, 37, 66, act=1), value(self.nab, 37, 66, act=1))

    def test_a_fight_that_could_kill_is_never_taken(self):
        self.assertLess(value(self.punch, 30, 66, act=1), -900.0)

    def test_the_same_fight_costs_more_in_act_3(self):
        # Priced as an HP loss is, by ACT3_HP: the last acts' HP is worth more.
        self.assertLess(value(self.punch, 73, 73, act=3), value(self.punch, 73, 73, act=1))


class DelayedRelics(unittest.TestCase):
    box = {"title": "Wongo's Mystery Box", "description": "Pay 300 Gold. Obtain 3 random Relics after 5 combats."}
    bargain = {"title": "Wongo's Bargain Bin", "description": "Pay 100 Gold. Obtain 1 random Common Relic."}

    def test_three_relics_after_five_combats_lose_to_one_now_at_low_hp(self):
        # Taken at 8/75 (16.5 against the Bin's 5.8): the run died three fights later holding no box relics.
        self.assertLess(value(self.box, 8, 75, act=2), value(self.bargain, 8, 75, act=2))
        self.assertGreater(value(self.box, 75, 75, act=2), value(self.bargain, 75, 75, act=2))  # at full health the box still wins
        self.assertEqual(value(self.bargain, 8, 75, act=2), value(self.bargain, 75, 75, act=2))  # (a relic now is not discounted)

    def test_surviving_five_fights_is_likelier_above_half_hp(self):
        self.assertGreater(events._lives_through(5, 40, 75), events._lives_through(5, 30, 75))
        self.assertGreater(events._lives_through(5, 75, 75), 0.9)
