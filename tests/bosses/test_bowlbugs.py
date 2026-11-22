"""Bowlbug Rock and Bowlbug Nectar (act 2 hallway): the Nectar is brought down first (battle.KNOWN_THREAT)."""

import unittest
from unittest import mock

from sts2bot.fight import battle, combat

from ..helpers import STRIKE, combat_state, enemy

ROCK = {"id": "IMBALANCED_POWER", "amount": 1}


def bowlbugs(rock=47, nectar=37, nectar_attack=3, rock_attack=16, threat=None, egg=None):
    """A round of the fight as logged on Ascension 9: the Rock first, then the Nectar (3, Buff, then 19 a turn)."""
    out = [{**enemy("BOWLBUG_ROCK_0", rock, attack=rock_attack), "status": [ROCK]}, enemy("BOWLBUG_NECTAR_0", nectar, attack=nectar_attack)]
    if egg:
        out.append(enemy("BOWLBUG_EGG_0", egg, attack=8))
    state = combat_state([STRIKE(0)], [e for e in out if e["hp"] > 0], hp=60, max_hp=75)
    # (What each has aimed at us a round so far, this round included, as bot.py keeps it for an ordinary fight.)
    state["_threat"] = threat or {"BOWLBUG_ROCK_0": float(rock_attack or 0), "BOWLBUG_NECTAR_0": float(nectar_attack or 0)}
    return state


def aimed(state):
    return combat.choose_card(state)[1].get("target")


class NectarFirst(unittest.TestCase):
    def test_the_nectar_is_hit_before_the_rock_from_round_one(self):
        # 2026-11-22 17:35: the Rock went 46 -> 11 -> 1 while the Nectar stood at 37, Buffed, and hit 19 a round.
        self.assertEqual(aimed(bowlbugs()), "BOWLBUG_NECTAR_0")

    def test_also_on_the_turn_it_buffs(self):
        # Round 2: the Nectar shows a Buff and has shown 1.5 a round; the Rock 16.
        state = bowlbugs(nectar_attack=None, threat={"BOWLBUG_ROCK_0": 16.0, "BOWLBUG_NECTAR_0": 1.5})
        self.assertEqual(aimed(state), "BOWLBUG_NECTAR_0")

    def test_a_rock_within_reach_is_still_finished(self):
        self.assertEqual(aimed(bowlbugs(rock=5)), "BOWLBUG_ROCK_0")

    def test_without_the_table_the_rock_came_first(self):
        known = {k: v for k, v in battle.KNOWN_THREAT.items() if k != "BOWLBUG_NECTAR"}
        state = bowlbugs(nectar_attack=None, threat={"BOWLBUG_ROCK_0": 16.0, "BOWLBUG_NECTAR_0": 1.5})
        with mock.patch.object(battle, "KNOWN_THREAT", known):
            self.assertEqual(aimed(state), "BOWLBUG_ROCK_0")

    def test_the_nectar_s_threat_is_what_it_will_do(self):
        nectar = next(e for e in battle.read_enemies(bowlbugs()) if e.entity_id == "BOWLBUG_NECTAR_0")
        self.assertEqual(nectar.threat, 19.0)


if __name__ == "__main__":
    unittest.main()
