"""Cards a potion offers mid-fight ("Choose 1 of 3 ... It's free to play this turn") are chosen for the turn they come in, by the planner, not by what each would be worth in the deck."""

import unittest
from unittest import mock

from sts2bot.fight import afflictions

from ..helpers import STRIKE, enemy, make_bot
from . import combat_state

# As the choose screen lists them: no target, no "can_play".
GUARD = {"index": 0, "id": "ULTIMATE_DEFEND", "name": "Ultimate Defend", "type": "Skill", "cost": "1", "rarity": "Uncommon", "description": "Gain 11 Block."}
PUNCH = {"index": 1, "id": "SUCKER_PUNCH", "name": "Sucker Punch", "type": "Attack", "cost": "1", "rarity": "Common", "description": "Deal 8 damage. Apply 1 Weak."}
BLADE = {"index": 2, "id": "BLADE_RUSH", "name": "Blade Rush", "type": "Attack", "cost": "2", "rarity": "Rare", "description": "Deal 14 damage."}


def fight(attack, hp=60, foe_hp=300):
    return combat_state([STRIKE(0)], [enemy("A", foe_hp, attack=attack)], energy=1, hp=hp, kind="boss")


class Offers(unittest.TestCase):
    def setUp(self):
        afflictions._OFFER[:] = [None, 0]

    def test_the_card_taken_is_the_one_for_this_turn(self):
        for why, offer, state, taken in (
            ("no Block card on a turn nothing attacks (the Knowledge Demon's first turn is a Debuff)", [GUARD, PUNCH], fight(attack=None), "Sucker Punch"),
            ("the Block card when the hit would kill: 20 on 12 HP is 15 and dead behind Sucker Punch's Weak, 9 behind Ultimate Defend", [PUNCH, GUARD], fight(attack=20, hp=12), "Ultimate Defend"),
            ("the offer is free this turn: Blade Rush and the Strike are 20, the kill on 15 HP (Sucker Punch and Strike 14)", [PUNCH, BLADE], fight(attack=10, foe_hp=15), "Blade Rush"),
        ):
            with self.subTest(why):
                afflictions._OFFER[:] = [None, 0]  # (each one a screen of its own)
                self.assertEqual(afflictions.offered(offer, state)[0]["name"], taken)

    def test_afflictions_and_screens_outside_a_fight_are_left_alone(self):
        sloth = {"index": 0, "name": "Sloth", "type": "Status", "description": "You cannot play more than 3 cards each turn."}
        burn = {"index": 1, "name": "Disintegration", "type": "Status", "description": "At the end of your turn, take 7 damage."}
        self.assertEqual(afflictions.offered([sloth, burn], fight(attack=10)), [sloth, burn])
        self.assertEqual(afflictions.offered([GUARD, PUNCH], None), [GUARD, PUNCH])

    def test_the_bot_hands_the_screen_the_fight_it_came_in(self):
        bot, state = make_bot(None), fight(attack=None)
        bot._combat(state)
        screen = {"state_type": "card_select", "run": state["run"], "player": state["player"], "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": [GUARD, PUNCH]}}
        self.assertEqual(bot.candidates(screen)[0], ("select_card", {"index": 1}))  # Sucker Punch
        elsewhere = {**screen, "run": {**state["run"], "floor": state["run"]["floor"] + 1}}  # (an event's offer, out of the fight)
        with mock.patch.object(afflictions, "offered", wraps=afflictions.offered) as ranked:
            bot.candidates(elsewhere)
        self.assertIsNone(ranked.call_args.args[1])
