"""Which game the bot plays: Slay the Spire 2 (the default) or the first Slay the Spire (games/sts1: CommunicationMod and a relay).
A process-wide choice made once at start-up (`select`; `--game both` runs a process for each); one game's tables ask `uses_sts2_tables()`."""

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
    """True while playing Slay the Spire 2, whose cards the hand-made ratings, crowd win rates and relic ratings were made for."""
    return _active == "sts2"
