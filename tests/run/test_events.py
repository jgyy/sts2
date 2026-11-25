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


class KnowingSkull(unittest.TestCase):
    """Every choice but the way out comes back dearer, and the way out costs 6 HP whenever it is taken."""

    def walk(self, hp, max_hp=85, act=2):
        """(HP lost, things bought) when the bot plays the event through: each thing costs 1 HP more every time it is taken."""
        from sts2bot.run import strategy

        mem, start, bought = strategy.Memory(), hp, 0
        costs = {"Riches?": 6, "A Pick Me Up?": 6, "Success?": 6}
        gains = {"Riches?": "Gain 90 Gold.", "A Pick Me Up?": "Get a Potion.", "Success?": "Get a Colorless Card."}
        for _ in range(20):
            opts = [{"index": i, "title": t, "description": f"{gains[t]} Lose {c} HP."} for i, (t, c) in enumerate(costs.items())]
            opts.append({"index": 3, "title": "How do I leave?", "description": "Lose 6 HP."})
            state = {"state_type": "event", "run": {"act": act, "floor": 22}, "player": {"hp": hp, "max_hp": max_hp, "potions": [], "relics": []},
                     "event": {"event_id": "KNOWING_SKULL", "options": opts}}
            pick = opts[events.event(state, mem)[0][1]["index"]]
            if pick["title"] == "How do I leave?":
                return start - (hp - 6), bought
            hp, bought = hp - costs[pick["title"]], bought + 1
            costs[pick["title"]] += 1
        self.fail("never left")

    def test_it_takes_a_thing_or_two_and_leaves(self):
        # 2026-11-25: 47 -> 1 HP at 19:07 and 66 -> 5 at 19:19, the way out always scoring a little under the next purchase.
        for hp in (47, 66, 85):
            lost, bought = self.walk(hp)
            self.assertLessEqual(lost, 20, hp)
            self.assertLessEqual(bought, 2, hp)

    def test_other_events_whose_options_all_cost_hp_still_pay_for_a_relic(self):
        # The Cursed Tome after its pages (6 HP bled): the Book for 10 HP beats stopping for 3, as before the Knowing Skull's rule.
        from sts2bot.run import strategy

        mem = strategy.Memory()
        mem.event_id, mem.event_hp0 = "CURSED_TOME@30", 66
        opts = [{"index": 0, "title": "Take", "description": "Obtain the Book. Lose 10 HP."}, {"index": 1, "title": "Stop", "description": "Lose 3 HP."}]
        state = {"state_type": "event", "run": {"act": 2, "floor": 30}, "player": {"hp": 60, "max_hp": 80, "potions": [], "relics": []},
                 "event": {"event_id": "CURSED_TOME", "options": opts}}
        self.assertEqual(events.event(state, mem)[0], ("choose_event_option", {"index": 0}))


class SecretPortal(unittest.TestCase):
    portal = {"title": "Enter the Portal", "description": "IMMEDIATELY travel to the boss."}
    leave = {"title": "Leave", "description": ""}

    def test_only_from_near_full_health_with_no_key_still_wanted(self):
        # 2026-11-25: entered at 39/75, 42/77 and 42/82 on a 0.5-0.5 tie; all three bosses lost.
        self.assertLess(value(self.portal, 42, 82, act=3), value(self.leave, 42, 82, act=3))
        self.assertGreaterEqual(value(self.portal, 80, 82, act=3), value(self.leave, 80, 82, act=3))
        self.assertLess(value(self.portal, 80, 82, act=3, keys_wanted=True), value(self.leave, 80, 82, act=3, keys_wanted=True))


class MindBloom(unittest.TestCase):
    def test_upgrading_everything_is_not_worth_never_healing_again(self):
        awake = {"title": "I am Awake", "description": "Upgrade all Cards. You can no longer heal."}
        rich = {"title": "I am Rich", "description": "Gain 999 Gold. Cursed - 2 Normality."}
        war = {"title": "I am War", "description": "Fight a Boss from Act 1. Obtain a Rare Relic."}
        for hp in (40, 79):
            self.assertLess(value(awake, hp, 79, act=3), max(value(rich, hp, 79, act=3), value(war, hp, 79, act=3)), hp)
