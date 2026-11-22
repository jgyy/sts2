"""The act 3 knights: the Magi Knight, whose 35 comes on round 5, is brought down first (battle.KNOWN_THREAT)."""

import unittest

from sts2bot import combat
from tests.fake_game import card

from .helpers import STRIKE, combat_state, enemy

VULN = [{"id": "VULNERABLE_POWER", "amount": 2}]


def knights(flail=106, spectral=95, magi=87, hand=None, **kw):
    """Round 1 of the elite as logged on Ascension 8 (the Spectral Knight's Debuff makes every card Ethereal)."""
    out = [
        enemy("FLAIL_KNIGHT_0", flail, attack=15),
        {**enemy("SPECTRAL_KNIGHT_0", spectral), "intents": [{"type": "Debuff", "label": ""}]},
        {**enemy("MAGI_KNIGHT_0", magi), "intents": [{"type": "Attack", "label": "6"}, {"type": "Defend", "label": ""}]},
    ]
    return combat_state(hand or [STRIKE(0)], [e for e in out if e["hp"] > 0], hp=66, max_hp=66, kind="elite", **kw)


def aimed(state):
    return combat.choose_card(state)[1].get("target")


class KnightsInOrder(unittest.TestCase):
    def test_the_magi_knight_is_hit_first_even_when_another_knight_is_vulnerable(self):
        # 11:47 on 2026-11-22: round 1's Putrefy+ put Weak and Vulnerable on the Flail Knight, and from then on that knight
        # (counted as 62 HP) and the Spectral one took the attacks; the Magi Knight, untouched at 66, hit 35 on round 5.
        state = knights()
        state["battle"]["enemies"][0]["status"] = VULN
        self.assertEqual(aimed(state), "MAGI_KNIGHT_0")
        state["battle"]["enemies"][1]["status"] = VULN  # (the Spectral Knight too)
        self.assertEqual(aimed(state), "MAGI_KNIGHT_0")

    def test_vulnerable_goes_on_the_knight_to_kill_first(self):
        expose = card(0, "EXPOSE", "Expose", "Skill", 1, "Apply 2 Vulnerable.", "AnyEnemy")
        self.assertEqual(aimed(knights(hand=[expose])), "MAGI_KNIGHT_0")

    def test_a_knight_nearly_dead_is_finished_and_then_the_flail_knight_comes_before_the_spectral(self):
        self.assertEqual(aimed(knights(spectral=30)), "SPECTRAL_KNIGHT_0")  # (30 HP of 12 a turn, against 87 of 22)
        self.assertEqual(aimed(knights(flail=6)), "FLAIL_KNIGHT_0")  # a kill on offer
        # The Flail Knight gains 3 Strength every fourth turn: in the model fight, Spectral Knight second won 54% of
        # one deck's fights and Flail Knight second 70%, whatever the Hex costs.
        self.assertEqual(aimed(knights(magi=0)), "FLAIL_KNIGHT_0")

    def test_other_fights_are_untouched(self):
        state = combat_state([STRIKE(0)], [enemy("A", 50, attack=10), {**enemy("B", 70, attack=10), "status": VULN}], hp=80, kind="elite")
        self.assertEqual([e.focus for e in combat.read_enemies(state)], [1.0, 1.0])
        self.assertEqual(aimed(state), "B")  # (lowest health first, Vulnerable counting as less: 47 against 50)


class HexedCards(unittest.TestCase):
    def test_the_hexed_keyword_makes_a_card_ethereal(self):
        from sts2bot.cards import parse

        hexed = {**STRIKE(0), "keywords": [{"name": "Hexed", "description": "Add Ethereal to this card."}]}
        self.assertTrue(parse(hexed).ethereal)
        self.assertFalse(parse(STRIKE(0)).ethereal)

    def test_under_hex_a_strike_is_played_rather_than_left_to_be_exhausted(self):
        # Round 2 of a model fight (scripts/knightfight.py, Strikes of 18 and Defends of 12): 66 of 75 HP, 18 and 15
        # coming, the Magi Knight on 51. Counting nothing lost, the planner kept to its Defends and let the Strikes be
        # Exhausted; turns like it left the deck with nothing to finish the Flail Knight, and the fight was lost.
        from unittest import mock

        from sts2bot import triggers

        from .helpers import DEFEND

        hex_kw = [{"name": "Hexed", "description": "Add Ethereal to this card."}]
        hand = [DEFEND(0), STRIKE(1), STRIKE(2), DEFEND(3), DEFEND(4)]
        for c in hand:
            c["description"] = c["description"].replace("6", "18").replace("5", "12")
            c["keywords"] = hex_kw
        magi = {**enemy("MAGI_KNIGHT_0", 51, block=6), "max_hp": 87, "intents": [{"type": "Debuff", "label": ""}]}
        state = combat_state(hand, [enemy("FLAIL_KNIGHT_0", 106, attack="9x2"), enemy("SPECTRAL_KNIGHT_0", 95, attack=15), magi], hp=66, max_hp=75, kind="elite")
        state["battle"]["round"] = 2
        pile = lambda *cs: [{"name": n, "cost": k, "description": d} for n, k, d in cs]
        strike, defend, heavy = ("Strike", "1", "Deal 18 damage."), ("Defend", "1", "Gain 12 Block."), ("Heavy Blow", "2", "Deal 39 damage.")
        state["player"].update(status=[{"id": "HEX_POWER", "amount": 2}], draw_pile=pile(("Bash", "2", "Deal 8 damage. Apply 2 Vulnerable."), heavy, strike, heavy, strike),
                               discard_pile=pile(strike, strike, defend, ("Clumsy", "-1", "Unplayable."), defend), discard_pile_count=5)
        self.assertEqual(combat.choose_card(state)[1], {"card_index": 1, "target": "MAGI_KNIGHT_0"})
        with mock.patch.object(triggers, "ETHEREAL_KEEP", 0.0):
            self.assertEqual(combat.choose_card(state)[1], {"card_index": 0})  # (what it did)
