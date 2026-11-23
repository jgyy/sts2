"""What the first game's tests share: the client wired straight to the fake game, and the game switch."""

import unittest

from sts2bot import games
from sts2bot.games.sts1.client import Sts1Client


class Direct(Sts1Client):
    """Sts1Client with the relay taken out: the fake game answers in the same words the relay passes on."""

    def __init__(self, fake, **kw):
        super().__init__(**kw)
        self.fake = fake

    def ping(self):
        return True

    def _message(self):
        return self.fake.message()

    def send(self, command):
        answer = self.fake.command(command)
        if "error" in answer:
            return {"status": "error", "error": answer["error"]}
        return {"status": "ok", "message": answer}


class Sts1Case(unittest.TestCase):
    """Plays the first game for the length of each test."""

    def setUp(self):
        games.select("sts1")
        self.addCleanup(games.select, "sts2")
