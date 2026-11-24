"""Which game the bot is playing: Slay the Spire 2 (the default) or the first Slay the Spire.

The planner, the map and reward logic, the learning and the supervisor are the same for both. What
differs is how the bot talks to the game and a few tables that only fit one of them:

- sts2: the STS2_MCP bridge mod answers HTTP on localhost (sts2bot.client.Client).
- sts1: CommunicationMod starts a small relay (games/sts1/relay.py) that the bot reaches over
  localhost; games/sts1/client.py turns that game's state into the shape the bot already reads,
  and the bot's actions back into CommunicationMod commands.

A bot process plays one game, so the choice is a process-wide setting made once at start-up
(`select`); `--game both` plays the two at once with a process for each (games/both.py). Code that
holds a table for one game only asks `uses_sts2_tables()`.
"""

from __future__ import annotations

GAMES = ("sts2", "sts1")
TITLES = {"sts2": "Slay the Spire 2", "sts1": "Slay the Spire"}
DEFAULT_PORT = {"sts2": 15526, "sts1": 15527}

_active = "sts2"


def select(game: str) -> str:
    """Make `game` the one being played (for the rest of the process)."""
    global _active
    game = str(game).lower()
    if game not in GAMES:
        raise ValueError(f"unknown game {game!r} (expected one of {', '.join(GAMES)})")
    _active = game
    return game


def active() -> str:
    return _active


def uses_sts2_tables() -> bool:
    """True while playing Slay the Spire 2: the hand-made card ratings, the crowd's win rates and the
    Ancients' relic ratings were all made for its cards. The first game's cards share many names
    (Strike, Bash, Inflame ...) with different numbers, so there they are judged from their text."""
    return _active == "sts2"
