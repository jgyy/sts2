"""The Kin (act 1 boss): the two Followers are brought down before the Priest (battle.MAIN_MINIONS)."""

import unittest
from unittest import mock

from sts2bot.fight import battle

from ..fight import aimed, combat_state, power
from ..helpers import STRIKE, enemy

MINION = [power("MINION")]


def kin(f0=63, f1=62, priest=199):
    """Round 1 of the fight as logged on Ascension 8."""
    out = [{**enemy("KIN_FOLLOWER_0", f0), "status": MINION}, {**enemy("KIN_FOLLOWER_1", f1, attack=5), "status": MINION},
           {**enemy("KIN_PRIEST_0", priest, attack=8), "intents": [{"type": "Attack", "label": "8"}, {"type": "Debuff", "label": ""}]}]
    return combat_state([STRIKE(0)], [e for e in out if e["hp"] > 0], hp=70, max_hp=70, kind="boss")


class KinFollowersFirst(unittest.TestCase):
    def test_the_followers_are_hit_before_the_priest_unless_he_can_be_finished(self):
        # A Silent put every attack on the Priest (199) and died on round 7 with him on 89, the Followers on 57 and 38 at 4 Strength each.
        for why, state, target in (("the Follower with fewer HP first", kin(), "KIN_FOLLOWER_1"), ("then the other", kin(f1=0), "KIN_FOLLOWER_0"),
                                   ("a Priest within reach is the one to finish: his death takes the Followers along", kin(priest=20), "KIN_PRIEST_0"),
                                   ("a Priest alone", kin(f0=0, f1=0, priest=150), "KIN_PRIEST_0")):
            with self.subTest(why):
                self.assertEqual(aimed(state), target)

    def test_without_the_table_the_priest_comes_first_as_before(self):
        with mock.patch.object(battle, "MAIN_MINIONS", set()):
            self.assertEqual(aimed(kin()), "KIN_PRIEST_0")

    def test_other_minions_keep_the_per_turn_rule(self):
        # The Queen's Amalgam is the main threat only on turns it hits hard (15+, twice the Queen's, 60% of her HP).
        queen = {**enemy("QUEEN_0", 400), "intents": [{"type": "Buff", "label": ""}]}
        amalgam = lambda attack: combat_state([STRIKE(0)], [{**enemy("TORCH_HEAD_AMALGAM_0", 199, attack=attack), "status": MINION}, queen], kind="boss")
        self.assertEqual([e.main_threat for e in battle.read_enemies(amalgam(8))], [False, False])
        self.assertEqual([e.main_threat for e in battle.read_enemies(amalgam(30))], [True, False])

    def test_weak_on_the_amalgam_does_not_make_it_harmless(self):
        # Deathbringer's Weak made the Amalgam's 18 show as 13, and Reap+ went into the Queen's 381 HP: what counts is the hit it will make once the Weak is gone.
        queen = {**enemy("QUEEN_0", 381), "intents": [{"type": "Debuff", "label": ""}]}
        state = combat_state([STRIKE(0)], [{**enemy("TORCH_HEAD_AMALGAM_0", 180, attack=13), "status": [*MINION, power("WEAK")]}, queen], kind="boss")
        self.assertEqual([e.main_threat for e in battle.read_enemies(state)], [True, False])
        self.assertEqual(aimed(state), "TORCH_HEAD_AMALGAM_0")
