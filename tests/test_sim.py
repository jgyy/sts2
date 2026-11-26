"""The model-run simulator (sim/): every first-game card plays without error, and a fight of each game runs to its end."""

import contextlib
import importlib
import io
import json
import logging
import tempfile
import unittest
from pathlib import Path

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

    def test_each_game_keeps_its_own_ascension_rules(self):
        sts2 = _content("sts2")
        run = Run(sts2, "IRONCLAD", 1, {}, 7)
        self.assertEqual(run.hp, run.max_hp)  # (a damaged start is the first game's A6)
        self.assertEqual(sts2.boss_heal(run), 0.0)  # (the next act's Ancient heals: act_start)
        rare = lambda odds: dict(odds["monster"])["Rare"]
        self.assertAlmostEqual(rare(sts2.SCARCE_ODDS), rare(sts2.CARD_ODDS) / 2)  # A7: half the rares ...
        self.assertAlmostEqual(sum(p for _r, p in sts2.SCARCE_ODDS["elite"]), 1.0)  # ... the rest commons
        sts1 = _content("sts1")
        run = Run(sts1, "IRONCLAD", 1, {}, 6)
        self.assertEqual(run.hp, int(run.max_hp * 0.9))
        self.assertEqual(sts1.boss_heal(run), 0.75)

    def test_the_card_table_learns_only_from_runs_that_did_not_use_it(self):
        from sim.__main__ import main

        def run(floor, picks, lean=0.0):
            return {"game": "sts2", "character": "SILENT", "floor": floor, "won": False, "picks": picks, "knobs": {}, "model_lean": lean}

        runs = [run(30, [[2, "FOOTWORK"]]) for _ in range(5)] + [run(20, [[2, "DEFLECT"]]) for _ in range(5)]
        runs += [run(48, [[2, "DEFLECT"]], lean=1.0) for _ in range(50)]  # (played with the table: left out)
        with tempfile.TemporaryDirectory() as d:
            src, out = Path(d) / "runs.jsonl", Path(d) / "cards.json"
            src.write_text("".join(json.dumps(r) + "\n" for r in runs))
            with contextlib.redirect_stderr(io.StringIO()):
                main(["prior", str(src), "--out", str(out), "--prior", "10"])
            table = json.loads(out.read_text())["sts2"]["SILENT"]
        # Par is 25: Footwork's five picks 5 floors over it, Deflect's 5 under, each shrunk to 5/15 of that.
        self.assertAlmostEqual(table["FOOTWORK"], round(25 / 15, 2))
        self.assertAlmostEqual(table["DEFLECT"], round(-25 / 15, 2))

    def test_the_card_table_can_learn_on_top_of_itself(self):
        from sim.__main__ import main

        def run(floor, picks, lean):
            return {"game": "sts2", "character": "SILENT", "floor": floor, "won": False, "picks": picks, "knobs": {}, "model_lean": lean}

        lean = learn.GAME_DEFAULTS["sts2"]["model_lean"]
        runs = [run(30, [[2, "FOOTWORK"]], lean) for _ in range(5)] + [run(20, [[2, "DEFLECT"]], lean) for _ in range(5)]
        runs += [run(48, [[2, "DEFLECT"]], 0.0) for _ in range(50)]  # (not played with the table at the game's lean: left out)
        with tempfile.TemporaryDirectory() as d:
            src, base, out = Path(d) / "runs.jsonl", Path(d) / "base.json", Path(d) / "cards.json"
            src.write_text("".join(json.dumps(r) + "\n" for r in runs))
            base.write_text(json.dumps({"sts2": {"SILENT": {"FOOTWORK": 1.0, "DEFLECT": -2.5, "NOXIOUS_FUMES": 0.5}}, "sts1": {"IRONCLAD": {"BASH": 1.0}}}))
            with contextlib.redirect_stderr(io.StringIO()):
                main(["prior", str(src), "--out", str(out), "--prior", "10", "--onto", str(base)])
            table = json.loads(out.read_text())
        # What the runs still say (as above, +-25/15) is added to the base, within --most; cards and games without runs keep theirs.
        self.assertEqual(table["sts2"]["SILENT"], {"FOOTWORK": round(1.0 + 25 / 15, 2), "DEFLECT": -3.0, "NOXIOUS_FUMES": 0.5})
        self.assertEqual(table["sts1"], {"IRONCLAD": {"BASH": 1.0}})

    def test_the_owl_magistrate_lands(self):
        sts2 = _content("sts2")
        run = Run(sts2, "IRONCLAD", 1, {}, 0)
        run.act_id = "GLORY"
        run.note_deck()
        f = Fight(run, [sts2._new("OWL_MAGISTRATE", 0, run, "monster")], "monster")
        f.start()
        owl, seen = f.foes[0], []
        for _ in range(4):
            f.player_turn_start()
            f.player_turn_end()
            f.enemies_turn()
            seen.append((owl.history[-1], bool(owl.powers.get("SOAR"))))
        # Half damage from attacks while it flies, the round after Judicial Flight; Verdict brings it down.
        self.assertEqual(seen, [("MAGISTRATE_SCRUTINY", False), ("PECK_ASSAULT", False), ("JUDICIAL_FLIGHT", True), ("VERDICT", False)])

    def test_a_new_axebot_starts_afresh(self):
        sts2 = _content("sts2")
        run = Run(sts2, "IRONCLAD", 1, {}, 0)
        run.act_id = "GLORY"
        run.note_deck()
        f = Fight(run, [sts2._new("AXEBOT", 0, run, "monster")], "monster")
        f.start()
        bot = f.foes[0]
        bot.powers.update({"STRENGTH": 6, "VULNERABLE": 2})
        hp = bot.max_hp
        f.kill(bot)
        self.assertTrue(bot.alive)
        self.assertEqual(bot.powers, {"STOCK": 1})  # (Stock 2 at the start: one replacement used)
        self.assertEqual(bot.max_hp, hp + 10)

    def test_grand_finale_waits_for_an_empty_draw_pile(self):
        # (Until 2026-11-26 no "Can only be played if" held in Slay the Spire 2's model: 60 damage to ALL enemies whenever drawn.)
        sts2 = _content("sts2")
        run = Run(sts2, "SILENT", 1, {}, 0)
        run.act_id = "OVERGROWTH"
        run.note_deck()
        f = Fight(run, [sts2._new("NIBBIT", 0, run, 0)], "monster")
        f.start()
        f.player_turn_start()
        f.hand.append(Card(sts2.CARDS["GRAND_FINALE"]))
        self.assertTrue(f.draw)
        self.assertFalse(f.playable(f.hand[-1]))
        f.draw.clear()
        self.assertTrue(f.playable(f.hand[-1]))

    def test_what_a_card_text_says_beyond_the_parse_happens(self):
        # (2026-11-26: Bone Shards left Osty alive, Fight Me gave the enemy nothing, statuses and copies were never added, Hyperbeam
        # cost no Focus; the first game's Reprogram gave its Strength and Dexterity twice.)
        def played(game, character, key, setup=lambda f: None):
            mod = _content(game)
            run = Run(mod, character, 1, {}, 0)
            run.act_id = "OVERGROWTH"
            run.note_deck()
            foe = mod._new("NIBBIT", 0, run, 0) if game == "sts2" else mod._new("JAW_WORM", 0, run, "monster")
            f = Fight(run, [foe], "monster")
            f.start()
            f.player_turn_start()
            f.energy = 6
            setup(f)
            f.hand.append(Card(mod.CARDS[key]))
            f.play(len(f.hand) - 1, f.foes[0].eid)
            return f

        osty = lambda f: setattr(f, "osty", 5)
        self.assertEqual(played("sts2", "NECROBINDER", "BONE_SHARDS", osty).osty, 0)
        self.assertEqual(played("sts2", "NECROBINDER", "BONE_SHARDS", lambda f: setattr(f, "osty", 0)).block, 0)  # ("If Osty is alive")
        self.assertEqual(played("sts2", "IRONCLAD", "FIGHT_ME").foes[0].powers.get("STRENGTH"), 1)
        self.assertEqual(sum(c.key == "WOUND" for c in played("sts2", "DEFECT", "FIGHT_THROUGH").discard), 2)
        twins = [c for c in played("sts2", "DEFECT", "ADAPTIVE_STRIKE").discard if c.key == "ADAPTIVE_STRIKE"]
        self.assertEqual(sorted(c.cost() for c in twins), [0, 2])  # (the card itself, and its copy for nothing)
        beam = played("sts2", "DEFECT", "HYPERBEAM", lambda f: f.channel("LIGHTNING"))
        self.assertEqual((beam.focus(), beam.orb_passive(beam.orbs[0])), (-3, 0))
        beam.player_turn_end()
        self.assertEqual(beam.focus(), 0)  # ("this turn")
        stocked = lambda f: f.discard.extend(Card(f.game.CARDS[k]) for k in ("STRIKE_NECROBINDER", "DEFEND_NECROBINDER", "UNLEASH"))
        dredged = played("sts2", "NECROBINDER", "DREDGE", stocked)
        self.assertEqual((len(dredged.hand), len(dredged.discard)), (8, 0))  # (Dredge itself is Exhausted)
        thrash = played("sts2", "IRONCLAD", "THRASH")
        self.assertEqual((len(thrash.hand), len(thrash.exhausted)), (4, 1))  # (a Strike of the five Exhausted)
        foregone = played("sts2", "REGENT", "FOREGONE_CONCLUSION")
        foregone.player_turn_end()
        foregone.enemies_turn()
        foregone.player_turn_start()
        self.assertEqual(len(foregone.hand), 7)
        reprogram = played("sts1", "DEFECT", "REPROGRAM")
        self.assertEqual((reprogram.strength(), reprogram.dexterity(), reprogram.focus()), (1, 1, -1))
