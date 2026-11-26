"""Both games at once (`--game both`): a bot process for each, kept going; and the first game's CommunicationMod check."""

import os
import signal
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from sts2bot import __main__ as cli
from sts2bot import games
from sts2bot.games import both
from sts2bot.games.sts1 import host


class FakeProc:
    def __init__(self, argv, pid):
        self.argv, self.pid, self.code, self.signals = argv, pid, None, []

    @property
    def game(self):
        return self.argv[self.argv.index("--game") + 1]

    def poll(self):
        return self.code

    def send_signal(self, sig):
        self.signals.append(sig)
        self.code = 0  # a bot stops on Ctrl+C and says how many runs it played

    def kill(self):
        self.code = -9

    def wait(self):
        return self.code


class Rig:
    """A clock that moves only when `run` sleeps, and bots that exit when `script` says (those started from `until` on: at once)."""

    def __init__(self, script, until=float("inf")):
        self.t, self.procs, self.script, self.until = 0.0, [], script, until

    def spawn(self, argv, **kw):
        self.procs.append(FakeProc(argv, 1000 + len(self.procs)))
        if self.t >= self.until:
            self.procs[-1].code = 0
        return self.procs[-1]

    def clock(self):
        return self.t

    def sleep(self, dt):
        self.t += dt
        self.script(self)

    def of(self, game):
        return [p for p in self.procs if p.game == game]

    def run(self, argv, **kw):
        return both.run(argv, spawn=self.spawn, clock=self.clock, sleep=self.sleep, **kw)


class Both(unittest.TestCase):
    def setUp(self):
        self.enterContext(mock.patch.object(both, "log"))

    def test_the_game_option_is_taken_out_and_the_rest_passed_on(self):
        for argv in (["--game", "both", "-v"], ["--game=both", "-v"], ["-v", "--ga", "both"]):
            self.assertEqual(both.without_game(argv), ["-v"])
        self.assertEqual(both.command("sts1", ["--runs", "3"]), [sys.executable, "-m", "sts2bot", "--game", "sts1", "--runs", "3"])

    def test_each_game_gets_a_bot_and_one_that_fails_is_started_again(self):
        def script(rig):
            sts2, sts1 = rig.of("sts2"), rig.of("sts1")
            if rig.t >= 5 and len(sts1) == 1:
                sts1[0].code = 1  # crashed
            if rig.t >= 40 and len(sts1) == 2:
                sts1[1].code = 0  # played its runs
            if rig.t >= 50:
                sts2[0].code = 0

        rig = Rig(script)
        self.assertEqual(rig.run(["--game", "both", "--runs", "2", "-v"]), 0)
        self.assertEqual(len(rig.of("sts2")), 1)
        self.assertEqual(len(rig.of("sts1")), 2)
        self.assertEqual(rig.procs[0].argv[3:], ["--game", "sts2", "--runs", "2", "-v"])
        self.assertTrue(all(p.signals == [] for p in rig.procs))  # every one stopped by itself

    def test_a_bot_that_keeps_failing_is_tried_less_often(self):
        starts = []

        def script(rig):
            for p in rig.of("sts1"):
                if p.code is None:  # fails as soon as it starts
                    starts.append(rig.t)
                    p.code = 2

        rig = Rig(script, until=200)
        with mock.patch.object(both, "RESTART_DELAY", 10.0), mock.patch.object(both, "MAX_DELAY", 40.0):
            rig.run(["--game", "both"], games=("sts1",))
        gaps = [b - a for a, b in zip(starts, starts[1:])]
        self.assertEqual([round(g) for g in gaps[:4]], [11, 21, 41, 41])  # (1 s of polling on top of each pause)

    def test_a_bot_told_to_reload_is_back_at_once_whatever_its_back_off(self):
        starts = []

        def script(rig):
            for p in rig.of("sts1"):
                if p.code is None:  # reloaded (SIGUSR1) as soon as it starts
                    starts.append(rig.t)
                    p.code = -signal.SIGUSR1

        rig = Rig(script, until=30)
        rig.run(["--game", "both"], games=("sts1",))
        gaps = [b - a for a, b in zip(starts, starts[1:])]
        self.assertTrue(gaps and all(g <= 3 for g in gaps))  # (no doubling: a second or two each time)

    def test_stopping_stops_every_bot(self):
        def script(rig):
            if rig.t >= 10 and not getattr(rig, "stopped", False):
                rig.stopped = True
                raise KeyboardInterrupt  # Ctrl+C, or SIGTERM from systemd

        rig = Rig(script)
        self.assertEqual(rig.run(["--game", "both"]), 0)
        self.assertEqual(len(rig.procs), 2)
        self.assertTrue(all(p.signals == [signal.SIGINT] for p in rig.procs))
        self.assertIs(signal.getsignal(signal.SIGTERM), signal.SIG_DFL)


class CommandLine(unittest.TestCase):
    def setUp(self):
        self.tmp = self.enterContext(tempfile.TemporaryDirectory())
        self.enterContext(mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": self.tmp}))
        self.enterContext(mock.patch("logging.basicConfig"))
        self.addCleanup(games.select, "sts2")

    def test_options_that_belong_to_one_game_are_refused(self):
        for opt in (["--port", "1"], ["--log-dir", "x"], ["--launch-cmd", "x"]):
            with self.assertRaises(SystemExit), mock.patch("sys.stderr"):
                cli.main(["--game", "both", *opt])

    def test_both_starts_once_communicationmod_starts_the_relay(self):
        with mock.patch.object(both, "run", return_value=0) as run:
            with self.assertLogs("sts2bot", "ERROR") as said:
                self.assertEqual(cli.main(["--game", "both", "-v"]), 2)  # no config yet: nothing is started
            self.assertIn("sts1-setup", said.output[0])
            run.assert_not_called()
            host.write_comm_config(games.DEFAULT_PORT["sts1"])
            self.assertEqual(cli.main(["--game", "both", "-v"]), 0)
            run.assert_called_once_with(["--game", "both", "-v"])

    def test_the_first_game_is_not_launched_for_another_program(self):
        host.comm_config_path().parent.mkdir(parents=True)
        host.comm_config_path().write_text("command=python3 /elsewhere/main.py --ascension 0\nrunAtGameStart=true\n")
        with mock.patch("logging.handlers.RotatingFileHandler"), mock.patch.object(cli, "Supervisor") as sup, self.assertLogs("sts2bot", "ERROR") as said:
            self.assertEqual(cli.main(["--game", "sts1", "--log-dir", self.tmp]), 2)
        sup.assert_not_called()
        self.assertIn("/elsewhere/main.py", said.output[0])


class CommConfig(unittest.TestCase):
    def test_what_communicationmod_starts_is_checked(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "config.properties"
            self.assertIn("no command", host.comm_config_problem(15527, path))
            path.write_text("verbose=true\ncommand=python3 /x/slaythespire/main.py --ascension 0\n")
            self.assertIn("another program", host.comm_config_problem(15527, path))
            host.write_comm_config(15599, path)
            self.assertIn("another port", host.comm_config_problem(15527, path))
            self.assertIsNone(host.comm_config_problem(15599, path))
            path.write_text(f"command=python3 {host.RELAY}\n")  # no --port: the relay's own default
            self.assertIsNone(host.comm_config_problem(15527, path))
