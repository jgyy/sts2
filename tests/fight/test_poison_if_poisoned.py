"""Cards whose Poison depends on what is on the enemy already."""

import unittest

from sts2bot.fight import combat
from sts2bot.cards import parse

from ..fake_game import card
from ..helpers import DEFEND, combat_state, enemy


class BubbleBubble(unittest.TestCase):
    def test_bubble_bubble_poisons_only_what_is_poisoned_already(self):
        # "If the enemy has Poison, apply 9 Poison." Read as plain Poison, a Silent with no other source played it
        # six times in one lost Lagavulin Matriarch fight (1 energy each, nothing on her).
        bubble = card(0, "BUBBLE_BUBBLE", "Bubble Bubble", "Skill", 1, "If the enemy has Poison, apply 9 Poison.", "AnyEnemy")
        info = parse(bubble)
        self.assertEqual((info.poison, info.poison_needs), (9, True))
        self.assertFalse(parse(card(0, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy")).poison_needs)
        kw = dict(energy=1, stars=0, hp=60, max_hp=80, block=0, fight="boss")

        def gain(stack):
            status = [{"id": "POISON_POWER", "amount": stack}] if stack else []
            foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 300, status=status)]}})
            return combat.simulate([info], foes, **kw).score - combat.simulate([], foes, **kw).score

        self.assertLessEqual(gain(0), 0)  # nothing to build on: a wasted energy
        self.assertGreater(gain(5), 5)

    def test_bubble_bubble_waits_for_the_poison_that_comes_first(self):
        bubble = card(0, "BUBBLE_BUBBLE", "Bubble Bubble", "Skill", 1, "If the enemy has Poison, apply 9 Poison.", "AnyEnemy")
        deadly = card(1, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy")
        state = combat_state([bubble, deadly], [enemy("A", 200, attack=8)], energy=2, hp=60, max_hp=80, kind="boss")
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)  # the source first, then Bubble Bubble
        # With no source in the hand, the energy goes to a card that does something.
        state = combat_state([bubble, DEFEND(1)], [enemy("A", 200, attack=12)], energy=1, hp=60, max_hp=80, kind="boss")
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)
