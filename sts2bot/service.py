"""`python3 -m sts2bot service ...`: turn the bot's systemd user service on or off (killing the bot does not: the unit restarts it)."""

from __future__ import annotations

import argparse
import subprocess
import sys

UNIT = "sts2bot"  # systemd/sts2bot.service, installed as the user unit of the same name

# What each action asks systemctl to do (`stop` lasts until the next login; `disable` also keeps it from starting then).
ACTIONS = {"status": ["status", "--no-pager", "--lines=0"], "start": ["start"], "stop": ["stop"], "enable": ["enable", "--now"], "disable": ["disable", "--now"]}
NOTE = {
    "stop": "The bot is stopped until you start it again or log in next time.",
    "disable": "The bot is stopped and will not start at login. Start it again with: python3 -m sts2bot service enable",
    "enable": "The bot is started now and at every login.",
}


def systemctl_args(action: str, unit: str = UNIT) -> list[str]:
    return ["systemctl", "--user", *ACTIONS[action], unit]


def main(argv: list[str] | None = None, runner=None) -> int:
    ap = argparse.ArgumentParser(prog="sts2bot service", description="Start, stop, enable or disable the bot's systemd user service.")
    ap.add_argument("action", choices=sorted(ACTIONS))
    ap.add_argument("--unit", default=UNIT, help=f"unit name (default: {UNIT})")
    args = ap.parse_args(argv)
    run = runner or subprocess.run  # looked up now, so a stand-in can replace it
    try:
        code = run(systemctl_args(args.action, args.unit)).returncode
    except FileNotFoundError:
        print("systemctl was not found: this only works on a systemd Linux desktop", file=sys.stderr)
        return 127
    if code == 0 and args.action in NOTE:
        print(NOTE[args.action])
        if args.action in ("stop", "disable"):
            print("The game keeps running; close it from the game or Steam if you want it gone too.")
    return code
