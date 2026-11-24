"""Bowlbug Rock and Bowlbug Nectar (act 2 hallway): the Nectar is brought down first (battle.KNOWN_THREAT)."""

import unittest
from unittest import mock

from sts2bot.fight import battle

from ..fight import aimed, combat_state, power
from ..helpers import STRIKE, enemy

BUFFING = dict(nectar_attack=None, threat={"BOWLBUG_ROCK_0": 16.0, "BOWLBUG_NECTAR_0": 1.5})  # round 2: the Nectar shows a Buff and has shown 1.5 a round


def bowlbugs(rock=47, nectar=37, nectar_attack=3, rock_attack=16, threat=None):
    """A round of the fight as logged on Ascension 9 (the Nectar: 3, Buff, then 19 a turn), with what each has aimed at us a round so far, this round included (bot.py's _threat)."""
    out = [{**enemy("BOWLBUG_ROCK_0", rock, attack=rock_attack), "status": [power("IMBALANCED")]}, enemy("BOWLBUG_NECTAR_0", nectar, attack=nectar_attack)]
    seen = threat or {"BOWLBUG_ROCK_0": float(rock_attack or 0), "BOWLBUG_NECTAR_0": float(nectar_attack or 0)}
    return combat_state([STRIKE(0)], [e for e in out if e["hp"] > 0], hp=60, max_hp=75, _threat=seen)


class NectarFirst(unittest.TestCase):
    def test_whom_the_strike_goes_at(self):
        for why, state, target in (("the Nectar before the Rock from round one (the Rock went 46 -> 11 -> 1 while the Nectar hit 19 a round)", bowlbugs(), "BOWLBUG_NECTAR_0"),
                                   ("also on the turn it buffs", bowlbugs(**BUFFING), "BOWLBUG_NECTAR_0"), ("a Rock within reach is still finished", bowlbugs(rock=5), "BOWLBUG_ROCK_0")):
            with self.subTest(why):
                self.assertEqual(aimed(state), target)

    def test_without_the_table_the_rock_came_first(self):
        with mock.patch.object(battle, "KNOWN_THREAT", {k: v for k, v in battle.KNOWN_THREAT.items() if k != "BOWLBUG_NECTAR"}):
            self.assertEqual(aimed(bowlbugs(**BUFFING)), "BOWLBUG_ROCK_0")

    def test_the_nectar_s_threat_is_what_it_will_do(self):
        self.assertEqual(next(e for e in battle.read_enemies(bowlbugs()) if e.entity_id == "BOWLBUG_NECTAR_0").threat, 19.0)
