"""The act 3 knights: the Magi Knight, whose 35 comes on round 5, is brought down first (battle.KNOWN_THREAT)."""

import unittest
from unittest import mock

from sts2bot.cards import parse
from sts2bot.fight import combat, triggers

from ..fake_game import card
from ..fight import aimed, combat_state, power
from ..helpers import DEFEND, STRIKE, enemy

VULN = [power("VULNERABLE", 2)]
HEX = [{"name": "Hexed", "description": "Add Ethereal to this card."}]


def knights(flail=106, spectral=95, magi=87, hand=None):
    """Round 1 of the elite as logged on Ascension 8 (the Spectral Knight's Debuff makes every card Ethereal)."""
    out = [enemy("FLAIL_KNIGHT_0", flail, attack=15), {**enemy("SPECTRAL_KNIGHT_0", spectral), "intents": [{"type": "Debuff", "label": ""}]},
           {**enemy("MAGI_KNIGHT_0", magi), "intents": [{"type": "Attack", "label": "6"}, {"type": "Defend", "label": ""}]}]
    return combat_state(hand or [STRIKE(0)], [e for e in out if e["hp"] > 0], hp=66, max_hp=66, kind="elite")


class KnightsInOrder(unittest.TestCase):
    def test_the_magi_knight_is_hit_first_even_when_another_knight_is_vulnerable(self):
        # Round 1's Putrefy+ put Weak and Vulnerable on the Flail Knight, and from then on that knight (counted as 62 HP) and the Spectral one took the attacks; the Magi Knight, untouched at 66, hit 35 on round 5.
        state = knights()
        state["battle"]["enemies"][0]["status"] = VULN
        self.assertEqual(aimed(state), "MAGI_KNIGHT_0")
        state["battle"]["enemies"][1]["status"] = VULN  # (the Spectral Knight too)
        self.assertEqual(aimed(state), "MAGI_KNIGHT_0")

    def test_whom_the_first_card_goes_at(self):
        # The Flail Knight gains 3 Strength every fourth turn: in the model fight, Spectral Knight second won 54% of one deck's fights and Flail Knight second 70%, whatever the Hex costs.
        for why, state, target in (("Vulnerable goes on the knight to kill first", knights(hand=[card(0, "EXPOSE", "Expose", "Skill", 1, "Apply 2 Vulnerable.", "AnyEnemy")]), "MAGI_KNIGHT_0"),
                                   ("a knight nearly dead is finished: 30 HP of 12 a turn, against 87 of 22", knights(spectral=30), "SPECTRAL_KNIGHT_0"),
                                   ("a kill on offer", knights(flail=6), "FLAIL_KNIGHT_0"), ("then the Flail Knight before the Spectral one", knights(magi=0), "FLAIL_KNIGHT_0")):
            with self.subTest(why):
                self.assertEqual(aimed(state), target)

    def test_other_fights_are_untouched(self):
        state = combat_state([STRIKE(0)], [enemy("A", 50, attack=10), {**enemy("B", 70, attack=10), "status": VULN}], hp=80, kind="elite")
        self.assertEqual([e.focus for e in combat.read_enemies(state)], [1.0, 1.0])
        self.assertEqual(aimed(state), "B")  # (lowest health first, Vulnerable counting as less: 47 against 50)


class HexedCards(unittest.TestCase):
    def test_the_hexed_keyword_makes_a_card_ethereal(self):
        self.assertTrue(parse({**STRIKE(0), "keywords": HEX}).ethereal)
        self.assertFalse(parse(STRIKE(0)).ethereal)

    def test_under_hex_a_strike_is_played_rather_than_left_to_be_exhausted(self):
        # Round 2 of a model fight (scripts/knightfight.py, Strikes of 18 and Defends of 12): 66 of 75 HP, 18 and 15 coming, the Magi Knight on 51. Counting nothing lost, the planner kept to its Defends and let the Strikes be Exhausted; turns like it
        # left the deck with nothing to finish the Flail Knight, and the fight was lost.
        hand = [{**c, "description": c["description"].replace("6", "18").replace("5", "12"), "keywords": HEX} for c in (DEFEND(0), STRIKE(1), STRIKE(2), DEFEND(3), DEFEND(4))]
        magi = {**enemy("MAGI_KNIGHT_0", 51, block=6), "max_hp": 87, "intents": [{"type": "Debuff", "label": ""}]}
        pile = lambda *cs: [{"name": n, "cost": k, "description": d} for n, k, d in cs]
        strike, defend, heavy = ("Strike", "1", "Deal 18 damage."), ("Defend", "1", "Gain 12 Block."), ("Heavy Blow", "2", "Deal 39 damage.")
        state = combat_state(hand, [enemy("FLAIL_KNIGHT_0", 106, attack="9x2"), enemy("SPECTRAL_KNIGHT_0", 95, attack=15), magi], hp=66, max_hp=75, kind="elite", round_no=2,
                             status=[power("HEX", 2)], draw_pile=pile(("Bash", "2", "Deal 8 damage. Apply 2 Vulnerable."), heavy, strike, heavy, strike),
                             discard_pile=pile(strike, strike, defend, ("Clumsy", "-1", "Unplayable."), defend), discard_pile_count=5)
        self.assertEqual(combat.choose_card(state)[1], {"card_index": 1, "target": "MAGI_KNIGHT_0"})
        with mock.patch.object(triggers, "ETHEREAL_KEEP", 0.0):
            self.assertEqual(combat.choose_card(state)[1], {"card_index": 0})  # (what it did)
