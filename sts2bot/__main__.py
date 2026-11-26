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
from . import games, learn, service
from .supervisor import DEFAULT_LAUNCH, FAST_MODES, Supervisor

log = logging.getLogger("sts2bot")

# (x3 to x6 made no difference while the bot's 0.2 s waits and the mod's 27 ms state builds held up every turn; with both cut, 2026-11-25: x6;
# 2026-11-26, the user: "x16 the speed for both sts1 and sts2" (the mod allows up to 20).)
DEFAULT_TIME_SCALE = 16.0
# The first game's clock, through our own mod (games/sts1/speedmod.py). It runs at 240 frames a second there, so even x10
# leaves each frame the time step of a 24 fps game: the game's logic sees nothing unusual. (x8 until 2026-11-25: its enemy
# turns, 0.5-0.7 s, were then most of the time a round takes.)
DEFAULT_STS1_TIME_SCALE = 16.0  # (x16 from 2026-11-26, the user's ask; speedmod.py allows up to 16: a frame's step is then a 15 fps game's)
DATE_FORMAT = "%Y-%m-%d %H:%M:%S"
STS1_SETUP_HINT = "would wait for a relay that never starts and restart the game for ever. Run `python3 -m sts2bot sts1-setup` to point CommunicationMod at the bot's relay"


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


def play_both(ap: argparse.ArgumentParser, args: argparse.Namespace, raw: list[str]) -> int:
    """`--game both`: a bot process for each game (games/both.py), started and kept going from here."""
    from .games import both
    from .games.sts1 import host as sts1_host

    clash = [opt for opt in both.GAME_OPTIONS if getattr(args, opt[2:].replace("-", "_")) is not None]
    if clash:
        ap.error(f"{', '.join(clash)} cannot be used with --game both: each game needs its own (run the two bots separately instead)")
    logging.basicConfig(level=logging.INFO, format="%(asctime)s both %(levelname)s %(message)s", datefmt=DATE_FORMAT, handlers=[logging.StreamHandler(sys.stderr)])
    why = sts1_host.comm_config_problem(games.DEFAULT_PORT["sts1"])
    if why and not args.no_launch:
        log.error("%s. Neither game is started: the first game's bot %s", why, STS1_SETUP_HINT)
        return 2
    log.info("playing %s; logs in %s and %s", " and ".join(games.TITLES[g] for g in games.GAMES), *(default_log_dir(g) for g in games.GAMES))
    return both.run(raw)


def main(argv: list[str] | None = None) -> int:
    raw = sys.argv[1:] if argv is None else list(argv)
    if raw[:1] == ["service"]:  # `python3 -m sts2bot service disable` and friends: no game, no bot
        return service.main(raw[1:])
    if raw[:1] == ["sts1-setup"]:
        return sts1_setup(raw[1:])
    ap = argparse.ArgumentParser(prog="sts2bot", description="Plays Slay the Spire 2 nonstop through the STS2_MCP mod, the first Slay the Spire through CommunicationMod "
                                 "(--game sts1), or both at the same time (--game both).")
    ap.add_argument("--game", choices=(*games.GAMES, "both"), default="sts2", help="which game to play: sts2 (default), sts1 (the first Slay the Spire), or both at once "
                    "(a bot process for each, with the other options passed on to both)")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=None, help="where the bridge listens (default: 15526 for sts2, 15527 for sts1's relay)")
    ap.add_argument("--character", default="auto", help='"auto" and "random" play a character with the lowest ascension unlocked (drawn at random among those on that level), '
                    'always at the highest ascension it has unlocked; or a character id (e.g. IRONCLAD; THE_SILENT, DEFECT or WATCHER in sts1), or "cycle" over unlocked ones (default: auto)')
    ap.add_argument("--runs", type=int, default=None, help="stop after this many runs (default: never stop)")
    ap.add_argument("--first", metavar="CHARACTER", help="with --character auto: play this character until it has earned all of its own epochs, then carry on as usual")
    ap.add_argument("--no-tuning", action="store_true", help="always play with the default settings instead of trying variations and learning from the results")
    ap.add_argument("--no-launch", action="store_true", help="never start or kill the game; only play when it is up")
    ap.add_argument("--launch-cmd", default=None, help=f"command that starts the game (default for sts2: {DEFAULT_LAUNCH!r}; for sts1: ModTheSpire with BaseMod and CommunicationMod)")
    ap.add_argument("--fast-mode", choices=(*FAST_MODES, "keep"), default="instant",
                    help='game animation speed, set through the mod each time the bot connects; "keep" leaves the setting alone (default: instant, the fastest)')
    ap.add_argument("--full-graphics", action="store_true", help="keep the game's normal window and graphics (default: maximised window, no anti-aliasing, uncapped frame rate)")
    ap.add_argument("--small-window", action="store_true", help="shrink the game window to 960x540 instead of full screen")
    ap.add_argument("--windowed", action="store_true", help="a maximised window instead of full screen (both games)")
    ap.add_argument("--time-scale", type=float, default=DEFAULT_TIME_SCALE,
                    help=f"how many times faster the game's clock runs, set through the mod for the session (default: {DEFAULT_TIME_SCALE:g}; 1 leaves it alone)")
    ap.add_argument("--sts1-time-scale", type=float, default=DEFAULT_STS1_TIME_SCALE,
                    help=f"the same for the first Slay the Spire, through the bot's own ModTheSpire mod (default: {DEFAULT_STS1_TIME_SCALE:g}; 1 leaves it alone)")
    ap.add_argument("--stuck-timeout", type=float, default=300.0, help="seconds without any change before the game is restarted")
    ap.add_argument("--poll", type=float, default=0.08, help="seconds between state polls when there is nothing to do (0.2 until 2026-11-26)")
    ap.add_argument("--log-dir", type=Path, default=None, help="logs and run history (default: ~/.local/state/sts2bot, and its sts1/ for the first game)")
    ap.add_argument("-v", "--verbose", action="store_true", help="log every action")
    args = ap.parse_args(argv)
    if args.game == "both":
        return play_both(ap, args, raw)
    games.select(args.game)
    learn.KNOB.update(learn.defaults())  # (with tuning, each run then draws its own from learn.CHOICES)
    args.port = args.port or games.DEFAULT_PORT[args.game]
    args.log_dir = args.log_dir or default_log_dir(args.game)

    args.log_dir.mkdir(parents=True, exist_ok=True)
    # bot.log keeps its format (the analysis scripts read it); the terminal and the journal also say which game a line
    # is from (with --game both the two bots write to the same place).
    fmt = logging.Formatter("%(asctime)s %(levelname)s %(message)s", DATE_FORMAT)
    screen = logging.StreamHandler(sys.stderr)
    screen.setFormatter(logging.Formatter(f"%(asctime)s {args.game} %(levelname)s %(message)s", DATE_FORMAT))
    # (A run takes about three minutes now: 5 MB held under half an hour of play, and the log reports read whole sessions.)
    file = logging.handlers.RotatingFileHandler(args.log_dir / "bot.log", maxBytes=20_000_000, backupCount=8)
    file.setFormatter(fmt)
    logging.basicConfig(level=logging.DEBUG if args.verbose else logging.INFO, handlers=[screen, file])

    host = None
    if args.game == "sts1":
        from .games.sts1 import host as sts1_host
        from .games.sts1.client import Sts1Client

        why = sts1_host.comm_config_problem(args.port)
        if why:
            if not args.no_launch:
                log.error("%s. Not launching the game: the bot %s", why, STS1_SETUP_HINT)
                return 2
            log.warning("%s; the bot cannot reach the game until you run `python3 -m sts2bot sts1-setup`", why)
        client: Client = Sts1Client(args.host, args.port, prefs=sts1_host.read_prefs)
        host = sts1_host.Sts1Host(log_dir=args.log_dir, fullscreen=not args.windowed, time_scale=args.sts1_time_scale)
        launch_cmd = args.launch_cmd or sts1_host.default_launch()
    else:
        client = Client(args.host, args.port)
        launch_cmd = args.launch_cmd or DEFAULT_LAUNCH
    character = f"auto:{args.first}" if args.first and args.character.lower() == "auto" else args.character
    bot = Bot(client, character=character, log_dir=args.log_dir, poll=args.poll, stuck_timeout=args.stuck_timeout, tune=not args.no_tuning)
    if args.game == "sts1":
        bot.room_pause, bot.room_poll = 0.0, 0.03  # (see Bot.__init__: CommunicationMod already waits for the game)
    sup = Supervisor(bot, client, host=host, launch_cmd=launch_cmd, manage_game=not args.no_launch, fast_mode=None if args.fast_mode == "keep" else args.fast_mode, light_graphics=not args.full_graphics, small_window=args.small_window, fullscreen=not args.windowed, time_scale=args.time_scale)
    log.info("playing %s; logs and run history in %s", games.TITLES[args.game], args.log_dir)
    try:
        sup.run(args.runs)
    except KeyboardInterrupt:
        pass
    log.info("stopped after %d runs (%d wins)", bot.runs_finished, bot.wins)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
