import unittest

from sts2bot.fight import combat

from ..fake_game import card
from ..helpers import combat_state, enemy


class ClosingARace(unittest.TestCase):
    def _play(self, boss_hp):
        hand = [card(0, "STRIKE_R", "Strike", "Attack", 1, "Deal 9 damage.", "AnyEnemy"), card(1, "DEFEND_R", "Defend", "Skill", 1, "Gain 8 Block.", "Self")]
        state = combat_state(hand, [{**enemy("KNOWLEDGE_DEMON_0", boss_hp, attack=13), "max_hp": 379}], energy=1, hp=30, max_hp=75, kind="boss")
        state["battle"]["round"] = 8
        return combat.choose_card(state)[1]["card_index"]

    def test_a_boss_that_dies_next_turn_is_hit_and_not_blocked(self):
        # 379 -> 40 in seven rounds is 48 a round: one more turn kills it, and the 8 HP a Defend would save are worth little then.
        self.assertEqual(self._play(40), 0)

    def test_a_boss_with_turns_to_live_is_still_blocked(self):
        self.assertEqual(self._play(200), 1)

    def test_the_last_of_our_hp_is_never_the_cheap_kind(self):
        hand = [card(0, "STRIKE_R", "Strike", "Attack", 1, "Deal 9 damage.", "AnyEnemy"), card(1, "DEFEND_R", "Defend", "Skill", 1, "Gain 8 Block.", "Self")]
        state = combat_state(hand, [{**enemy("KNOWLEDGE_DEMON_0", 40, attack=13), "max_hp": 379}], energy=1, hp=10, max_hp=75, kind="boss")
        state["battle"]["round"] = 8
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)  # 13 coming on 10 HP: the Defend

    def test_an_elite_is_not_raced_this_way(self):
        hand = [card(0, "STRIKE_R", "Strike", "Attack", 1, "Deal 9 damage.", "AnyEnemy"), card(1, "DEFEND_R", "Defend", "Skill", 1, "Gain 8 Block.", "Self")]
        state = combat_state(hand, [{**enemy("BYRDONIS_0", 40, attack=13), "max_hp": 379}], energy=1, hp=30, max_hp=75, kind="elite")
        state["battle"]["round"] = 8
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)
