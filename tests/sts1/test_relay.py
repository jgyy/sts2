"""The relay CommunicationMod starts: commands out on stdout, states in on stdin, the bot on localhost HTTP."""

import contextlib
import io
import json
import subprocess
import sys
import threading

from sts2bot.games.sts1 import relay as relay_mod
from sts2bot.games.sts1.client import Sts1Client

from ..helpers import make_bot
from .fake_comm import FakeComm
from .helpers import Sts1Case


class GamePipe(io.StringIO):
    """The relay's stdout, read by a fake game that answers on the relay's stdin a moment later."""

    def __init__(self, game: FakeComm):
        super().__init__()
        self.game, self.relay = game, None

    def write(self, s):
        for line in s.splitlines():
            if line.strip() and line.strip() != "ready":
                answer = json.dumps(self.game.command(line.strip()))
                threading.Thread(target=self.relay.feed, args=(answer,), daemon=True).start()
        return len(s)


def relay_to(game, timeout):
    pipe = GamePipe(game)
    pipe.relay = relay_mod.Relay(pipe, answer_timeout=timeout)
    return pipe.relay


class RelayTests(Sts1Case):
    def test_a_command_is_answered_with_the_games_next_message(self):
        game = FakeComm()
        relay = relay_to(game, 2.0)
        relay.feed(json.dumps(game.message()))
        self.assertEqual(relay.snapshot()["message"]["in_game"], False)
        answer = relay.send("start IRONCLAD 0")
        self.assertEqual(answer["status"], "ok")
        self.assertTrue(answer["message"]["in_game"])
        bad = relay.send("end")  # (not on the map)
        self.assertEqual(bad["status"], "error")
        self.assertIn("Invalid command", bad["error"])
        self.assertTrue(relay.snapshot()["message"]["in_game"])  # an error does not replace the state

    def test_a_game_that_does_not_answer_leaves_the_command_pending(self):
        relay = relay_mod.Relay(io.StringIO(), answer_timeout=0.05)
        self.assertTrue(relay.send("state").get("pending"))
        with contextlib.redirect_stderr(io.StringIO()):
            relay.feed("not json")  # (ignored)
        relay.feed(json.dumps({"in_game": False, "available_commands": ["start"]}))
        self.assertEqual(relay.snapshot()["seq"], 1)

    def test_the_bot_plays_a_run_through_the_relay_over_http(self):
        game = FakeComm(seed=5)
        relay = relay_to(game, 5.0)
        server = relay_mod.serve(0, relay)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        client = Sts1Client(port=server.server_address[1])
        self.assertFalse(client.ping())  # up, but the game has not said anything yet
        relay.feed(json.dumps(game.message()))
        self.assertTrue(client.ping())
        bot = make_bot(client, character="WATCHER")
        bot.run(max_runs=1)
        self.assertEqual(bot.runs_finished, 1)
        self.assertIn("start WATCHER 0", game.sent)

    def test_the_relay_says_ready_and_leaves_with_the_game(self):
        proc = subprocess.Popen([sys.executable, relay_mod.__file__, "--port", "0"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.addCleanup(proc.kill)
        self.assertEqual(proc.stdout.readline().strip(), "ready")  # what CommunicationMod waits for
        proc.stdin.close()  # the game is gone
        self.assertEqual(proc.wait(timeout=10), 0)
