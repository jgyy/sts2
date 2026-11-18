import json
import tempfile
import unittest
from pathlib import Path

from sts2bot import combat, learn, strategy
from sts2bot.bot import Bot, Stuck
from sts2bot.cards import deck_value, parse
from sts2bot.strategy import Memory

from .fake_game import FakeGame, card
from .helpers import DEFEND, STRIKE, FakeClock, combat_state, enemy, make_bot  # noqa: F401


class BossMechanics(unittest.TestCase):
    """Things the bosses do that the planner got wrong (found in the fight logs of 2026-11-18)."""

    ESCAPE = staticmethod(lambda i, cost=1: card(
        i, "FRANTIC_ESCAPE", "Frantic Escape", "Status", cost,
        "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1."))

    def test_a_death_blow_is_an_attack(self):
        blow = {"type": "DeathBlow", "label": "39"}
        self.assertEqual((combat.intent_damage(blow), combat.intent_hits(blow)), (39, 1))

    def test_the_giants_last_blow_is_blocked_and_the_dead_giant_left_alone(self):
        # Killed, the Waterfall Giant stands on 999,999,999 HP until it blows up.
        giant = enemy("WATERFALL_GIANT_0", 999999999, status=[{"id": "STEAM_ERUPTION_POWER", "amount": 39}])
        giant["intents"] = [{"type": "DeathBlow", "label": "39"}]
        big = card(0, "KICK", "Kingly Kick", "Attack", 0, "Deal 35 damage.", "AnyEnemy")
        reflect = card(1, "REFLECT", "Reflect", "Skill", 1, "Gain 15 Block.", "Self")
        state = combat_state([big, reflect, DEFEND(2), DEFEND(3)], [giant], energy=3, hp=36, max_hp=75, kind="boss")
        played = []
        for _ in range(3):
            action = combat.choose_card(state)
            self.assertIsNotNone(action)
            index = action[1]["card_index"]
            played.append(index)
            state["player"]["hand"] = [c for c in state["player"]["hand"] if c["index"] != index]
            state["player"]["energy"] -= 1
            state["player"]["block"] += 15 if index == 1 else 5
        self.assertEqual(sorted(played), [1, 2, 3])  # every Block card, and not one hit on the giant
        self.assertIsNone(combat.choose_card(state))  # the free attack is not worth playing either

    def test_beckon_is_seen_despite_the_double_space(self):
        # The mod turns the line break in Beckon's text into a second space.
        beckon = card(0, "BECKON", "Beckon", "Status", 1, "At the end of your turn, if this is in your Hand,  lose 6 HP.", "Self")
        state = combat_state([beckon, DEFEND(1)], [enemy("FYSH", 200, attack=4)], energy=1, kind="boss")
        self.assertEqual(combat.hand_burns(state["player"]), ((0, 6, False),))
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))  # 6 saved beats 4 blocked

    def test_block_stops_cards_that_deal_damage_but_not_ones_that_take_hp(self):
        wither = card(0, "WITHER", "Wither+2", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 9 damage.")
        beckon = card(0, "BECKON", "Beckon", "Status", 1, "At the end of your turn, if this is in your Hand,  lose 6 HP.")
        kw = dict(energy=0, stars=0, hp=50, max_hp=80, fight="boss")
        foe = combat.read_enemies({"battle": {"enemies": [enemy("A", 300, attack=10)]}})
        behind = lambda c, block: combat.simulate([], foe, block=block, burns=combat.hand_burns({"hand": [c]}), **kw).score
        self.assertEqual(behind(wither, 19), behind(wither, 25))  # 19 Block takes the Wither and the attack
        self.assertLess(behind(wither, 10), behind(wither, 19))
        self.assertEqual(behind(beckon, 10), behind(beckon, 25))  # the 6 HP go whatever the Block

    def test_a_wither_costs_three_for_every_level(self):
        named = lambda name: {"hand": [{"name": name, "description": "At the end of your turn, if this is in your Hand, take 3 damage."}]}
        self.assertEqual(combat.hand_burns(named("Wither"))[0][1], 3)
        self.assertEqual(combat.hand_burns(named("Wither+2"))[0][1], 9)
        self.assertEqual(combat._next_wither({"discard_pile": [{"name": "Wither+1"}]}), 6)
        self.assertEqual(combat._next_wither({}, round_no=8), 9)  # two of Aeonglass's upgrades have gone by

    def test_dead_on_paper_it_plays_the_block_it_has(self):
        # 29 coming, 22 HP, one Defend+: nothing saves it for sure, but the Block comes closest.
        power = card(0, "DEFRAGMENT", "Defragment", "Power", 1, "Gain 1 Focus.", "Self")
        guard = card(1, "DEFEND", "Defend+", "Skill", 1, "Gain 8 Block.", "Self")
        skim = card(2, "SKIM", "Skim", "Skill", 1, "Draw 3 cards.", "Self")
        state = combat_state([power, guard], [enemy("A", 300, attack=40)], energy=1, hp=22, max_hp=90, kind="boss")
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))
        # With energy for both, the cards are drawn first: one of them may be the Block that is missing.
        state = combat_state([power, guard, skim], [enemy("A", 300, attack=40)], energy=2, hp=22, max_hp=90, kind="boss")
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 2}))

    def test_an_escape_is_played_before_the_margin_is_gone(self):
        # Sandpit 3 and one Escape in hand: ending on 2 needs an Escape in the very next hand.
        boss = enemy("INS", 200, attack=0, status=[{"id": "SANDPIT_POWER", "amount": 3}])
        state = combat_state([self.ESCAPE(0), STRIKE(1), STRIKE(2), STRIKE(3)], [boss], energy=3, kind="boss")
        state["player"]["draw_pile"] = [{"name": "Strike"}] * 20 + [{"name": "Frantic Escape"}] * 2
        seen = set()
        for _ in range(3):
            action = combat.choose_card(state)
            seen.add(action[1]["card_index"])
            state["player"]["hand"] = [c for c in state["player"]["hand"] if c["index"] != action[1]["card_index"]]
            state["player"]["energy"] -= 1
        self.assertIn(0, seen)
        # ... even at 2 energy, the whole of what is left.
        state = combat_state([self.ESCAPE(0, cost=2), STRIKE(1), STRIKE(2)], [boss], energy=2, kind="boss")
        state["player"]["draw_pile"] = [{"name": "Strike"}] * 20 + [{"name": "Frantic Escape"}] * 2
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))

    def test_escape_odds_follow_the_piles(self):
        none_left = {"draw_pile": [{"name": "Strike"}] * 10, "discard_pile": [{"name": "Frantic Escape"}] * 6, "hand": []}
        self.assertEqual(combat._escape_odds(none_left)[0], 1.0)  # the next five cards hold none for certain
        all_escapes = {"draw_pile": [{"name": "Frantic Escape"}] * 5, "discard_pile": [], "hand": []}
        self.assertEqual(combat._escape_odds(all_escapes)[0], 0.0)
        mixed = {"draw_pile": [{"name": "Strike"}] * 24 + [{"name": "Frantic Escape"}] * 6, "discard_pile": [], "hand": []}
        self.assertAlmostEqual(combat._escape_odds(mixed)[0], 0.298, places=2)

    def test_beating_a_form_of_the_test_subject_is_not_winning_the_fight(self):
        adaptable = [{"id": "ADAPTABLE_POWER", "amount": 1}]
        kw = dict(energy=2, stars=0, hp=60, max_hp=80, block=0, fight="boss")
        foes = lambda status: combat.read_enemies({"battle": {"enemies": [enemy("TEST_SUBJECT_0", 5, attack=15, status=status)]}})
        kill = [parse(STRIKE(0))]
        self.assertGreater(combat.simulate(kill, foes([]), **kw).score, combat.WIN_BONUS)
        beaten = combat.simulate(kill, foes(adaptable), **kw).score
        self.assertLess(beaten, combat.WIN_BONUS / 2)
        # ... but it is worth doing: its attack this turn does not happen.
        self.assertGreater(beaten, combat.simulate([], foes(adaptable), **kw).score + 40)
        # Once it is gone nothing more can be aimed at it this turn.
        self.assertAlmostEqual(combat.simulate(kill + [parse(STRIKE(1))], foes(adaptable), **kw).score, beaten)

    def test_set_up_cards_are_played_while_the_enemy_is_away(self):
        loop = card(0, "LOOP", "Loop", "Power", 1, "At the start of your turn, trigger the passive of your rightmost Orb.", "Self")
        zap = card(1, "ZAP", "Zap", "Skill", 1, "Channel 1 Lightning.", "Self")
        state = combat_state([loop, zap, STRIKE(2), DEFEND(3)], [], energy=2, kind="boss")
        self.assertIsNone(combat.choose_card(state))  # a fight that is simply over
        state["_revive_pending"] = True
        first = combat.choose_card(state)
        self.assertEqual(first, ("play_card", {"card_index": 0}))
        state["player"]["hand"] = [zap, STRIKE(2), DEFEND(3)]
        state["player"]["energy"] = 1
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))
        state["player"]["hand"] = [STRIKE(2), DEFEND(3)]
        self.assertIsNone(combat.choose_card(state))  # no use hitting or blocking thin air

    def test_an_escape_is_never_the_card_given_up(self):
        escape = self.ESCAPE(0)
        hand = [escape, card(1, "X", "Hyperbeam", "Attack", 2, "Deal 26 damage to ALL enemies.", rarity="Rare"), DEFEND(2)]
        state = {"state_type": "hand_select", "hand_select": {"prompt": "Choose a card to Exhaust.", "cards": hand, "can_confirm": False}}
        first = strategy.hand_select(state, Memory())[0]
        self.assertEqual(first, ("combat_select_card", {"card_index": 2}))
        order = [a[1]["card_index"] for a in strategy.hand_select(state, Memory()) if a[0] == "combat_select_card"]
        self.assertEqual(order[-1], 0)  # the Escape goes last, after even the best card


if __name__ == "__main__":
    unittest.main()
