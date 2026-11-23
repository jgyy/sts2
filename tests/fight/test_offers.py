"""Cards a potion offers mid-fight ("Choose 1 of 3 ... It's free to play this turn") are chosen for the turn they come
in, by the planner, not by what each would be worth in the deck."""

import unittest
from unittest import mock

from sts2bot.fight import afflictions

from ..helpers import STRIKE, combat_state, enemy, make_bot

# As the choose screen lists them: no target, no "can_play".
GUARD = {"index": 0, "id": "ULTIMATE_DEFEND", "name": "Ultimate Defend", "type": "Skill", "cost": "1", "rarity": "Uncommon", "description": "Gain 11 Block."}
PUNCH = {"index": 1, "id": "SUCKER_PUNCH", "name": "Sucker Punch", "type": "Attack", "cost": "1", "rarity": "Common", "description": "Deal 8 damage. Apply 1 Weak."}
BLADE = {"index": 2, "id": "BLADE_RUSH", "name": "Blade Rush", "type": "Attack", "cost": "2", "rarity": "Rare", "description": "Deal 14 damage."}


def fight(attack, hp=60, foe_hp=300):
    return combat_state([STRIKE(0)], [enemy("A", foe_hp, attack=attack)], energy=1, hp=hp, kind="boss")


class Offers(unittest.TestCase):
    def setUp(self):
        afflictions._OFFER[:] = [None, 0]

    def test_no_block_card_on_a_turn_nothing_attacks(self):
        # The Knowledge Demon's first turn is a Debuff: Ultimate Defend was taken there and never played.
        self.assertEqual(afflictions.offered([GUARD, PUNCH], fight(attack=None))[0]["name"], "Sucker Punch")

    def test_the_block_card_when_the_hit_would_kill(self):
        # (20 on 12 HP: Sucker Punch's Weak makes it 15 and dead, Ultimate Defend 9.)
        self.assertEqual(afflictions.offered([PUNCH, GUARD], fight(attack=20, hp=12))[0]["name"], "Ultimate Defend")

    def test_the_offer_is_free_this_turn(self):
        # Blade Rush costs 2 and only one energy is left: free, it and the Strike are 20, the kill on 15 HP (Punch and Strike 14).
        self.assertEqual(afflictions.offered([PUNCH, BLADE], fight(attack=10, foe_hp=15))[0]["name"], "Blade Rush")

    def test_afflictions_and_screens_outside_a_fight_are_left_alone(self):
        sloth = {"index": 0, "name": "Sloth", "type": "Status", "description": "You cannot play more than 3 cards each turn."}
        burn = {"index": 1, "name": "Disintegration", "type": "Status", "description": "At the end of your turn, take 7 damage."}
        self.assertEqual(afflictions.offered([sloth, burn], fight(attack=10)), [sloth, burn])
        self.assertEqual(afflictions.offered([GUARD, PUNCH], None), [GUARD, PUNCH])

    def test_the_bot_hands_the_screen_the_fight_it_came_in(self):
        bot = make_bot(None)
        state = fight(attack=None)
        bot._combat(state)
        screen = {"state_type": "card_select", "run": state["run"], "player": state["player"],
                  "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": [GUARD, PUNCH]}}
        first = bot.candidates(screen)[0]
        self.assertEqual(first, ("select_card", {"index": 1}))  # Sucker Punch
        elsewhere = {**screen, "run": {**state["run"], "floor": state["run"]["floor"] + 1}}  # (an event's offer, out of the fight)
        with mock.patch.object(afflictions, "offered", wraps=afflictions.offered) as ranked:
            bot.candidates(elsewhere)
        self.assertIsNone(ranked.call_args.args[1])


if __name__ == "__main__":
    unittest.main()
