"""Artifact on an enemy: a debuff it swallows is worth the later one that lands in its place (sts2bot/fight/artifact.py)."""

import unittest

from sts2bot.fight import artifact, combat

from ..helpers import STRIKE, card, enemy
from . import combat_state, pick, power

SCOURGE = lambda i: card(i, "SCOURGE", "Scourge", "Skill", 1, "Apply 13 Doom.", "AnyEnemy")
PILE = lambda name, text, n: [{"name": name, "cost": "1", "description": text}] * n
DOOM_DECK = PILE("Scourge", "Apply 13 Doom.", 3) + PILE("No Escape", "Apply 10 Doom.", 2) + PILE("Strike", "Deal 6 damage.", 20)


def state(hand, pile, charges=3, **kw):
    return combat_state(hand, [enemy("BOSS", 480, status=[power("ARTIFACT", charges)])], kind="boss", draw_pile=pile, discard_pile=[], **kw)


class StrippingArtifact(unittest.TestCase):
    def test_a_doom_deck_spends_spare_energy_on_the_charges(self):
        # Round 2 of Aeonglass, a Scourge left in the hand and Artifact 3 on the boss until round 9: the Doom is swallowed, and the next one is a charge nearer to landing.
        choice = combat.choose_card(state([SCOURGE(0)], DOOM_DECK, energy=1))
        self.assertIsNotNone(choice)
        self.assertEqual(choice[1]["card_index"], 0)

    def test_a_deck_that_cannot_wear_them_off_does_not_try(self):
        # One debuff card in thirty and three charges: nothing would ever land, so the strip buys nothing.
        trip = card(0, "TRIP", "Trip", "Skill", 1, "Apply 2 Vulnerable.", "AnyEnemy")
        self.assertIsNone(combat.choose_card(state([trip], PILE("Strike", "Deal 6 damage.", 29), energy=1)))

    def test_a_strike_is_not_given_up_for_a_charge(self):
        # A charge is a debuff later, at a discount: with one energy the damage now still goes first, and with two both are played.
        weak = card(0, "INTIMIDATE", "Intimidate", "Skill", 1, "Apply 1 Weak.", "AnyEnemy")
        deck = PILE("Intimidate", "Apply 1 Weak.", 8) + PILE("Strike", "Deal 6 damage.", 12)
        self.assertEqual(pick(state([weak, STRIKE(1)], deck, energy=1)), 1)
        self.assertIsNotNone(combat.choose_card(state([weak, STRIKE(1)], deck, energy=2)))

    def test_no_artifact_no_change(self):
        self.assertEqual(combat._context(combat_state([SCOURGE(0), STRIKE(1)], [enemy("BOSS", 480)], kind="boss", energy=1))["strip"], (0.0, 0.0))


class WhatAChargeIsWorth(unittest.TestCase):
    def test_the_deck_s_own_debuffs_set_the_price(self):
        each, coming = artifact.outlook({"hand": [], "draw_pile": DOOM_DECK, "discard_pile": []}, "boss", 2)
        self.assertAlmostEqual(each, artifact.LATER * 0.8 * (3 * 13 + 2 * 10) / 5)
        self.assertAlmostEqual(coming, 5 * 7 * 5 / 25)  # five Doom cards in 25, seven rounds to go, five cards a round

    def test_only_charges_the_deck_will_get_past_count(self):
        self.assertEqual(artifact.gain(3, 2, (10.0, 7.0)), 10.0)
        self.assertEqual(artifact.gain(3, 1, (10.0, 7.0)), 20.0)
        self.assertEqual(artifact.gain(3, 2, (10.0, 2.0)), 0.0)  # two more debuffs to come, two charges still in the way
        self.assertEqual(artifact.gain(3, 2, (10.0, 2.5)), 5.0)
        self.assertEqual(artifact.gain(1, 0, (10.0, 0.4)), 4.0)

    def test_a_card_with_two_debuffs_uses_two_charges(self):
        both = combat.parse(card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage. Apply 1 Weak. Apply 1 Vulnerable.", "AnyEnemy"))
        self.assertEqual(artifact.landed(both, "boss")[0], 2)
        self.assertEqual(artifact.landed(combat.parse(STRIKE(0)), "boss"), (0, 0.0))
