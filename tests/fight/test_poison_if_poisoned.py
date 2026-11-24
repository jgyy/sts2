"""Cards whose Poison depends on what is on the enemy already."""

import unittest

from sts2bot.cards import parse

from ..fake_game import card
from ..helpers import DEFEND, enemy
from . import combat_state, ctx, gain, pick, power, read_foes

BUBBLE = card(0, "BUBBLE_BUBBLE", "Bubble Bubble", "Skill", 1, "If the enemy has Poison, apply 9 Poison.", "AnyEnemy")


class BubbleBubble(unittest.TestCase):
    def test_bubble_bubble_poisons_only_what_is_poisoned_already(self):
        # Read as plain Poison, a Silent with no other source played it six times in one lost fight (1 energy each, nothing on her).
        info = parse(BUBBLE)
        self.assertEqual((info.poison, info.poison_needs), (9, True))
        self.assertFalse(parse(card(0, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy")).poison_needs)
        on = lambda stack: gain([info], read_foes(enemy("A", 300, status=[power("POISON", stack)] if stack else [])), **ctx(energy=1, fight="boss"))
        self.assertLessEqual(on(0), 0)  # nothing to build on: a wasted energy
        self.assertGreater(on(5), 5)

    def test_bubble_bubble_waits_for_the_poison_that_comes_first(self):
        deadly = card(1, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy")
        self.assertEqual(pick(combat_state([BUBBLE, deadly], [enemy("A", 200, attack=8)], energy=2, hp=60, max_hp=80, kind="boss")), 1)  # the source first, then Bubble Bubble
        # With no source in the hand, the energy goes to a card that does something.
        self.assertEqual(pick(combat_state([BUBBLE, DEFEND(1)], [enemy("A", 200, attack=12)], energy=1, hp=60, max_hp=80, kind="boss")), 1)
