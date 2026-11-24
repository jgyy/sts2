"""The model-run simulator (sim/): every first-game card plays without error, and a fight of each game runs to its end."""

import importlib
import logging
import unittest

from sts2bot import games, learn
from sim.cards import Card
from sim.engine import Fight
from sim.run import Run


def _content(game: str):
    games.select(game)
    return importlib.import_module(f"sim.content.{game}")


class Sim(unittest.TestCase):
    def setUp(self):
        logging.disable(logging.CRITICAL)
        learn.KNOB.update(learn.DEFAULTS)

    def tearDown(self):
        logging.disable(logging.NOTSET)
        games.select("sts2")

    def test_every_first_game_card_plays(self):
        sts1 = _content("sts1")
        for d in sts1.CARDS.values():
            for up in (False, True):
                run = Run(sts1, "IRONCLAD", 1)
                f = Fight(run, [sts1._new("JAW_WORM", 0, run, "monster"), sts1._new("CULTIST", 1, run, "monster")], "monster")
                f.start()
                f.player_turn_start()
                f.hand.append(Card(d, up))
                f.energy = 5
                if f.playable(f.hand[-1]):
                    f.play(len(f.hand) - 1, f.foes[0].eid)
                f.state()
                f.player_turn_end()
                f.enemies_turn()

    def test_a_fight_of_each_game_ends(self):
        for game, character, foes, kind in (("sts1", "WATCHER", ["CULTIST"], "monster"), ("sts2", "REGENT", ["NIBBIT"], 0)):
            mod = _content(game)
            run = Run(mod, character, 3)
            run.act_id = "OVERGROWTH"
            run.note_deck()
            f = Fight(run, [mod._new(k, n, run, kind) for n, k in enumerate(foes)], "monster")
            f.run_fight()
            self.assertTrue(f.won or f.lost)
            self.assertGreater(f.round, 0)
