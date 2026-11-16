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
from .supervisor import DEFAULT_LAUNCH, Supervisor


def default_log_dir() -> Path:
    base = os.environ.get("XDG_STATE_HOME") or str(Path.home() / ".local" / "state")
    return Path(base) / "sts2bot"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="sts2bot", description="Plays Slay the Spire 2 nonstop through the STS2_MCP mod.")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=15526)
    ap.add_argument(
        "--character",
        default="IRONCLAD",
        help='character id (e.g. IRONCLAD, SILENT), or "cycle" / "random" over unlocked ones (default: IRONCLAD)',
    )
    ap.add_argument("--runs", type=int, default=None, help="stop after this many runs (default: never stop)")
    ap.add_argument("--no-launch", action="store_true", help="never start or kill the game; only play when it is up")
    ap.add_argument("--launch-cmd", default=DEFAULT_LAUNCH, help=f"command that starts the game (default: {DEFAULT_LAUNCH!r})")
    ap.add_argument("--stuck-timeout", type=float, default=300.0, help="seconds without any change before the game is restarted")
    ap.add_argument("--poll", type=float, default=0.4, help="seconds between state polls")
    ap.add_argument("--log-dir", type=Path, default=default_log_dir())
    ap.add_argument("-v", "--verbose", action="store_true", help="log every action")
    args = ap.parse_args(argv)

    args.log_dir.mkdir(parents=True, exist_ok=True)
    fmt = logging.Formatter("%(asctime)s %(levelname)s %(message)s", "%Y-%m-%d %H:%M:%S")
    handlers: list[logging.Handler] = [
        logging.StreamHandler(sys.stderr),
        logging.handlers.RotatingFileHandler(args.log_dir / "bot.log", maxBytes=5_000_000, backupCount=3),
    ]
    for h in handlers:
        h.setFormatter(fmt)
    logging.basicConfig(level=logging.DEBUG if args.verbose else logging.INFO, handlers=handlers)

    client = Client(args.host, args.port)
    bot = Bot(client, character=args.character, log_dir=args.log_dir, poll=args.poll, stuck_timeout=args.stuck_timeout)
    sup = Supervisor(bot, client, launch_cmd=args.launch_cmd, manage_game=not args.no_launch)
    logging.getLogger("sts2bot").info("logs and run history in %s", args.log_dir)
    try:
        sup.run(args.runs)
    except KeyboardInterrupt:
        pass
    logging.getLogger("sts2bot").info("stopped after %d runs (%d wins)", bot.runs_finished, bot.wins)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
