"""Crusher and Rocket: one crab is brought down first, and only the last card turns us to face the other."""

import unittest

from sts2bot.fight import combat

from ..fake_game import card
from ..fight import aimed, combat_state, power
from ..helpers import DEFEND, STRIKE, enemy


def crabs(crusher_hp, rocket_hp, crusher_hit=None, rocket_hit=None, facing="CRUSHER_0", hand=(), **kw):
    crusher = {**enemy("CRUSHER_0", crusher_hp, attack=crusher_hit, status=[power("BACK_ATTACK_LEFT"), power("CRAB_RAGE")]), "max_hp": 206}
    rocket = {**enemy("ROCKET_0", rocket_hp, attack=rocket_hit, status=[power("BACK_ATTACK_RIGHT"), power("CRAB_RAGE")]), "max_hp": 196}
    return combat_state(list(hand), [crusher, rocket], kind="boss", status=[power("SURROUNDED")], _facing=facing, **kw)


class OneCrabFirst(unittest.TestCase):
    def test_a_crab_on_its_last_hp_is_killed_while_the_other_is_still_whole(self):
        # A Crusher on 5 HP stood for six rounds hitting for 22 while the Rocket came down from 168: killing it cost 2 points for each HP of the Rocket. The 6 Strength is worth 0.3.
        state = crabs(5, 168, crusher_hit=22, facing="ROCKET_0", hand=[STRIKE(0), DEFEND(1)], energy=1, hp=40, max_hp=84)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "CRUSHER_0"}))

    def test_a_kill_still_waits_when_it_would_waste_the_turn_on_99_block(self):
        # Two Strikes, the Crusher on 5 and not attacking: the Rocket is hit first, then the Crusher dies.
        self.assertEqual(aimed(crabs(5, 168, rocket_hit=20, facing="ROCKET_0", hand=[STRIKE(0), STRIKE(1)], energy=2, hp=60, max_hp=84)), "ROCKET_0")

    def test_the_damage_stays_on_one_crab_and_the_last_card_turns_us(self):
        # The Rocket is about to hit for 49 from behind (33 from in front). Aiming the whole turn at it to turn round is how both crabs came down side by side; one Strike turns us as well as three.
        plan: dict = {}
        first = combat.choose_card(crabs(60, 150, rocket_hit=49, hand=[STRIKE(0), STRIKE(1), STRIKE(2)], energy=3, hp=60, max_hp=84), plan)
        self.assertEqual(first[1]["target"], "CRUSHER_0")
        self.assertEqual(len(plan["line"]), 3)
        self.assertEqual(plan["lost"], 33)  # (it ends the turn facing the Rocket)
        self.assertEqual(aimed(crabs(48, 150, rocket_hit=49, hand=[STRIKE(0)], energy=1, hp=60, max_hp=84)), "ROCKET_0")  # ... and the one card left turns us

    def test_round_one_s_big_hit_goes_on_the_rocket_whatever_its_first_intent(self):
        # Comet+ went on the Crusher, showing 22 against the Rocket's 5; the Crusher was then the one to kill, and the Rocket hit 31, 58 and 36 from 190 (battle.KNOWN_THREAT).
        comet = card(0, "COMET", "Comet", "Attack", 1, "Deal 30 damage. Apply 3 Weak. Apply 3 Vulnerable.", "AnyEnemy")
        state = crabs(219, 209, crusher_hit=21, rocket_hit=4, facing="ROCKET_0", hand=[comet, STRIKE(1), DEFEND(2), DEFEND(3)], energy=4, hp=70, max_hp=80)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "ROCKET_0"}))
        self.assertEqual([e.threat for e in combat.read_enemies(state)], [13.0, 20.0])
