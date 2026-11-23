"""Command line entry point: `python -m sts2bot`."""

from __future__ import annotations

import argparse
import logging
import logging.handlers
import os
import sys
from pathlib import Path

from .bot import Bot
from .client import Client
from . import games, service
from .supervisor import DEFAULT_LAUNCH, FAST_MODES, Supervisor

DEFAULT_TIME_SCALE = 3.0


def default_log_dir(game: str = "sts2") -> Path:
    """Each game keeps its own history (what the bot learns of one game's cards says nothing of the other's)."""
    base = os.environ.get("XDG_STATE_HOME") or str(Path.home() / ".local" / "state")
    return Path(base) / "sts2bot" if game == "sts2" else Path(base) / "sts2bot" / game


def sts1_setup(argv: list[str]) -> int:
    """`python3 -m sts2bot sts1-setup`: have CommunicationMod start the relay the bot talks to."""
    from .games.sts1 import host

    ap = argparse.ArgumentParser(prog="sts2bot sts1-setup", description="Point CommunicationMod (Slay the Spire) at the bot's relay.")
    ap.add_argument("--port", type=int, default=games.DEFAULT_PORT["sts1"])
    args = ap.parse_args(argv)
    path = host.write_comm_config(args.port)
    print(f"wrote {path}")
    print("CommunicationMod now starts the relay with the game. Start the bot with: python3 -m sts2bot --game sts1")
    if host.mts_jar() is None:
        print("ModTheSpire.jar was not found: subscribe to ModTheSpire, BaseMod and CommunicationMod on the Steam Workshop, or set STS1_MTS_JAR.")
    return 0


def main(argv: list[str] | None = None) -> int:
    raw = sys.argv[1:] if argv is None else list(argv)
    if raw[:1] == ["service"]:  # `python3 -m sts2bot service disable` and friends: no game, no bot
        return service.main(raw[1:])
    if raw[:1] == ["sts1-setup"]:
        return sts1_setup(raw[1:])
    ap = argparse.ArgumentParser(
        prog="sts2bot",
        description="Plays Slay the Spire 2 nonstop through the STS2_MCP mod, or the first Slay the Spire through CommunicationMod (--game sts1).",
    )
    ap.add_argument("--game", choices=games.GAMES, default="sts2", help="which game to play: sts2 (default) or sts1, the first Slay the Spire")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=None, help="where the bridge listens (default: 15526 for sts2, 15527 for sts1's relay)")
    ap.add_argument(
        "--character",
        default="auto",
        help='"auto" and "random" play a character with the lowest ascension unlocked (drawn at random among those '
        'on that level), always at the highest ascension it has unlocked; or a character id '
        '(e.g. IRONCLAD; THE_SILENT, DEFECT or WATCHER in sts1), or "cycle" over unlocked ones (default: auto)',
    )
    ap.add_argument("--runs", type=int, default=None, help="stop after this many runs (default: never stop)")
    ap.add_argument("--first", metavar="CHARACTER", help="with --character auto: play this character until it has earned all of its own epochs, then carry on as usual")
    ap.add_argument("--no-tuning", action="store_true", help="always play with the default settings instead of trying variations and learning from the results")
    ap.add_argument("--no-launch", action="store_true", help="never start or kill the game; only play when it is up")
    ap.add_argument("--launch-cmd", default=None, help=f"command that starts the game (default for sts2: {DEFAULT_LAUNCH!r}; for sts1: ModTheSpire with BaseMod and CommunicationMod)")
    ap.add_argument(
        "--fast-mode",
        choices=(*FAST_MODES, "keep"),
        default="instant",
        help="game animation speed, set through the mod each time the bot connects; "
        '"keep" leaves the setting alone (default: instant, the fastest)',
    )
    ap.add_argument(
        "--full-graphics",
        action="store_true",
        help="keep the game's normal window and graphics (default: maximised window, no anti-aliasing, uncapped frame rate)",
    )
    ap.add_argument("--small-window", action="store_true", help="shrink the game window to 960x540 instead of maximising it")
    ap.add_argument(
        "--time-scale",
        type=float,
        default=DEFAULT_TIME_SCALE,
        help=f"how many times faster the game's clock runs, set through the mod for the session (default: {DEFAULT_TIME_SCALE:g}; 1 leaves it alone)",
    )
    ap.add_argument("--stuck-timeout", type=float, default=300.0, help="seconds without any change before the game is restarted")
    ap.add_argument("--poll", type=float, default=0.2, help="seconds between state polls")
    ap.add_argument("--log-dir", type=Path, default=None, help="logs and run history (default: ~/.local/state/sts2bot, and its sts1/ for the first game)")
    ap.add_argument("-v", "--verbose", action="store_true", help="log every action")
    args = ap.parse_args(argv)
    games.select(args.game)
    args.port = args.port or games.DEFAULT_PORT[args.game]
    args.log_dir = args.log_dir or default_log_dir(args.game)

    args.log_dir.mkdir(parents=True, exist_ok=True)
    fmt = logging.Formatter("%(asctime)s %(levelname)s %(message)s", "%Y-%m-%d %H:%M:%S")
    handlers: list[logging.Handler] = [
        logging.StreamHandler(sys.stderr),
        logging.handlers.RotatingFileHandler(args.log_dir / "bot.log", maxBytes=5_000_000, backupCount=3),
    ]
    for h in handlers:
        h.setFormatter(fmt)
    logging.basicConfig(level=logging.DEBUG if args.verbose else logging.INFO, handlers=handlers)

    host = None
    if args.game == "sts1":
        from .games.sts1 import host as sts1_host
        from .games.sts1.client import Sts1Client

        client: Client = Sts1Client(args.host, args.port, prefs=sts1_host.read_prefs())
        host = sts1_host.Sts1Host()
        launch_cmd = args.launch_cmd or sts1_host.default_launch()
    else:
        client = Client(args.host, args.port)
        launch_cmd = args.launch_cmd or DEFAULT_LAUNCH
    bot = Bot(
        client,
        character=f"auto:{args.first}" if args.first and args.character.lower() == "auto" else args.character,
        log_dir=args.log_dir,
        poll=args.poll,
        stuck_timeout=args.stuck_timeout,
        tune=not args.no_tuning,
    )
    sup = Supervisor(bot, client, host=host, launch_cmd=launch_cmd, manage_game=not args.no_launch, fast_mode=None if args.fast_mode == "keep" else args.fast_mode, light_graphics=not args.full_graphics, small_window=args.small_window, time_scale=args.time_scale)
    logging.getLogger("sts2bot").info("playing %s; logs and run history in %s", games.TITLES[args.game], args.log_dir)
    try:
        sup.run(args.runs)
    except KeyboardInterrupt:
        pass
    logging.getLogger("sts2bot").info("stopped after %d runs (%d wins)", bot.runs_finished, bot.wins)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
