"""The Kin (act 1 boss): the two Followers are brought down before the Priest (battle.MAIN_MINIONS)."""

import unittest
from unittest import mock

from sts2bot.fight import battle, combat

from ..helpers import STRIKE, combat_state, enemy

MINION = [{"id": "MINION_POWER", "amount": 1}]


def kin(f0=63, f1=62, priest=199, hand=None):
    """Round 1 of the fight as logged on Ascension 8."""
    out = [
        {**enemy("KIN_FOLLOWER_0", f0), "status": MINION},
        {**enemy("KIN_FOLLOWER_1", f1, attack=5), "status": MINION},
        {**enemy("KIN_PRIEST_0", priest, attack=8), "intents": [{"type": "Attack", "label": "8"}, {"type": "Debuff", "label": ""}]},
    ]
    return combat_state(hand or [STRIKE(0)], [e for e in out if e["hp"] > 0], hp=70, max_hp=70, kind="boss")


def aimed(state):
    return combat.choose_card(state)[1].get("target")


class KinFollowersFirst(unittest.TestCase):
    def test_the_followers_are_hit_before_the_priest(self):
        # 13:17 on 2026-11-22: a Silent put every attack on the Priest (199) and died on round 7 with him on 89,
        # the Followers on 57 and 38 at 4 Strength each.
        self.assertEqual(aimed(kin()), "KIN_FOLLOWER_1")  # (the one with fewer HP)
        self.assertEqual(aimed(kin(f1=0)), "KIN_FOLLOWER_0")

    def test_a_priest_within_reach_is_still_the_one_to_finish(self):
        self.assertEqual(aimed(kin(priest=20)), "KIN_PRIEST_0")  # (his death takes the Followers along)
        self.assertEqual(aimed(kin(f0=0, f1=0, priest=150)), "KIN_PRIEST_0")

    def test_without_the_table_the_priest_comes_first_as_before(self):
        with mock.patch.object(battle, "MAIN_MINIONS", set()):
            self.assertEqual(aimed(kin()), "KIN_PRIEST_0")

    def test_other_minions_keep_the_per_turn_rule(self):
        # The Queen's Amalgam is the main threat only on turns it hits hard (15+, twice the Queen's, 60% of her HP).
        queen = {**enemy("QUEEN_0", 400), "intents": [{"type": "Buff", "label": ""}]}
        soft = combat_state([STRIKE(0)], [{**enemy("TORCH_HEAD_AMALGAM_0", 199, attack=8), "status": MINION}, queen], kind="boss")
        hard = combat_state([STRIKE(0)], [{**enemy("TORCH_HEAD_AMALGAM_0", 199, attack=30), "status": MINION}, queen], kind="boss")
        self.assertEqual([e.main_threat for e in battle.read_enemies(soft)], [False, False])
        self.assertEqual([e.main_threat for e in battle.read_enemies(hard)], [True, False])


if __name__ == "__main__":
    unittest.main()
