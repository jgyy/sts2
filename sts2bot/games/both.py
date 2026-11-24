"""`python3 -m sts2bot --game both`: a bot process for each game (`--game sts2`, `--game sts1`, other options passed on), kept going
as the systemd unit would: one that fails is restarted after a pause, one that finished its --runs stays finished, and Ctrl+C or
SIGTERM stops both. (Processes, not threads: the game is a process-wide switch, and two processes plan on two cores.)"""

from __future__ import annotations

import logging
import signal
import subprocess
import sys
import time
from pathlib import Path

from . import GAMES

log = logging.getLogger("sts2bot.both")

ROOT = Path(__file__).resolve().parent.parent.parent  # where `python3 -m sts2bot` imports from
RESTART_DELAY = 15.0  # as the unit's RestartSec
MAX_DELAY = 300.0  # a bot that keeps failing straight away is tried less and less often, down to this
HEALTHY = 600.0  # a bot that ran this long before it failed is tried again after the short pause
STOP_GRACE = 10.0
GAME_OPTIONS = ("--port", "--log-dir", "--launch-cmd")  # each game needs its own value: not allowed with both


def without_game(argv: list[str]) -> list[str]:
    """The command line with its --game option taken out (`--game both`, `--game=both`, `--ga both` ...)."""
    out: list[str] = []
    skip = False
    for word in argv:
        if skip:
            skip = False
            continue
        name = word.split("=", 1)[0]
        if len(name) > 2 and "--game".startswith(name):
            skip = "=" not in word
            continue
        out.append(word)
    return out


def command(game: str, rest: list[str]) -> list[str]:
    return [sys.executable, "-m", "sts2bot", "--game", game, *rest]


def run(argv: list[str], *, games: tuple[str, ...] = GAMES, spawn=subprocess.Popen, clock=time.monotonic, sleep=time.sleep) -> int:
    """Play every game in `games` at once until each bot finishes (or forever, without --runs). Returns 0."""
    rest = without_game(argv)
    procs: dict[str, subprocess.Popen] = {}
    started: dict[str, float] = {}
    due = {g: 0.0 for g in games}  # when each bot is to be (re)started
    delay = {g: RESTART_DELAY for g in games}
    done: set[str] = set()

    def stop(signum, frame):
        raise KeyboardInterrupt

    previous = signal.signal(signal.SIGTERM, stop)
    try:
        while len(done) < len(games):
            now = clock()
            for game in games:
                if game in done:
                    continue
                proc = procs.get(game)
                if proc is None:
                    if now >= due[game]:
                        procs[game] = spawn(command(game, rest), cwd=ROOT, stdin=subprocess.DEVNULL)
                        started[game] = now
                        log.info("started the %s bot (pid %s)", game, procs[game].pid)
                    continue
                code = proc.poll()
                if code is None:
                    continue
                del procs[game]
                if code == 0:
                    done.add(game)
                    log.info("the %s bot finished", game)
                    continue
                if now - started[game] >= HEALTHY:
                    delay[game] = RESTART_DELAY
                due[game] = now + delay[game]
                log.warning("the %s bot stopped (exit code %s); starting it again in %.0f s", game, code, delay[game])
                delay[game] = min(delay[game] * 2, MAX_DELAY)
            sleep(1.0)
    except KeyboardInterrupt:
        pass
    finally:
        signal.signal(signal.SIGTERM, previous)
        _stop(list(procs.values()), clock=clock, sleep=sleep)
    return 0


def _wait(procs: list, seconds: float, clock, sleep) -> None:
    deadline = clock() + seconds
    while clock() < deadline and any(p.poll() is None for p in procs):
        sleep(0.2)


def _stop(procs: list, *, clock=time.monotonic, sleep=time.sleep) -> None:
    """Stop the bots still running (the games stay up): a moment to stop by themselves (Ctrl+C and SIGTERM reach them too), then
    a Ctrl+C of their own, then a kill."""
    _wait(procs, STOP_GRACE / 4, clock, sleep)
    for proc in procs:
        if proc.poll() is None:
            proc.send_signal(signal.SIGINT)
    _wait(procs, STOP_GRACE, clock, sleep)
    for proc in procs:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
