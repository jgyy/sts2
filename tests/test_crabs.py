"""Crusher and Rocket: one crab is brought down first, and only the last card turns us to face the other."""

import unittest

from sts2bot import combat

from .helpers import DEFEND, STRIKE, combat_state, enemy


def crabs(crusher_hp, rocket_hp, crusher_hit=None, rocket_hit=None, facing="CRUSHER_0", hand=(), **kw):
    rage = {"id": "CRAB_RAGE_POWER", "amount": 1}
    crusher = enemy("CRUSHER_0", crusher_hp, attack=crusher_hit, status=[{"id": "BACK_ATTACK_LEFT_POWER", "amount": 1}, rage])
    rocket = enemy("ROCKET_0", rocket_hp, attack=rocket_hit, status=[{"id": "BACK_ATTACK_RIGHT_POWER", "amount": 1}, rage])
    crusher["max_hp"], rocket["max_hp"] = 206, 196
    state = combat_state(list(hand), [crusher, rocket], kind="boss", **kw)
    state["player"]["status"] = [{"id": "SURROUNDED_POWER", "amount": 1}]
    state["_facing"] = facing
    return state


class OneCrabFirst(unittest.TestCase):
    def test_a_crab_on_its_last_hp_is_killed_while_the_other_is_still_whole(self):
        # 17:43 on 2026-11-21: a Crusher on 5 HP stood for six rounds, hitting for 22, while the Rocket came down
        # from 168, because killing it cost 2 points for each HP of the Rocket. The 6 Strength is worth 0.3.
        state = crabs(5, 168, crusher_hit=22, facing="ROCKET_0", hand=[STRIKE(0), DEFEND(1)], energy=1, hp=40, max_hp=84)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "CRUSHER_0"}))

    def test_a_kill_still_waits_when_it_would_waste_the_turn_on_99_block(self):
        # Two Strikes, the Crusher on 5 and not attacking: the Rocket is hit first, then the Crusher dies.
        state = crabs(5, 168, rocket_hit=20, facing="ROCKET_0", hand=[STRIKE(0), STRIKE(1)], energy=2, hp=60, max_hp=84)
        self.assertEqual(combat.choose_card(state)[1]["target"], "ROCKET_0")

    def test_the_damage_stays_on_one_crab_and_the_last_card_turns_us(self):
        # The Rocket is about to hit for 49 from behind (33 from in front). Aiming the whole turn at it to turn
        # round is how both crabs came down side by side; one Strike turns us as well as three.
        hand = [STRIKE(0), STRIKE(1), STRIKE(2)]
        state = crabs(60, 150, rocket_hit=49, hand=hand, energy=3, hp=60, max_hp=84)
        plan: dict = {}
        first = combat.choose_card(state, plan)
        self.assertEqual(first[1]["target"], "CRUSHER_0")
        self.assertEqual(len(plan["line"]), 3)
        self.assertEqual(plan["lost"], 33)  # (it ends the turn facing the Rocket)
        # ... and with one card left it is the one that turns.
        state = crabs(48, 150, rocket_hit=49, hand=[STRIKE(0)], energy=1, hp=60, max_hp=84)
        self.assertEqual(combat.choose_card(state)[1]["target"], "ROCKET_0")


if __name__ == "__main__":
    unittest.main()
