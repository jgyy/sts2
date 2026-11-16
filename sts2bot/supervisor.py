"""Keeps the game and the bot alive around the clock.

Launches Slay the Spire 2 when it is not running, waits for the bridge mod to
answer, plays, and restarts the game if it crashes, freezes, or the bot gets
stuck. A run that hangs twice in the same place is abandoned.
"""

from __future__ import annotations

import logging
import os
import shlex
import subprocess
import time
from pathlib import Path

from .bot import Bot, Stuck
from .client import Client, GameUnavailable

log = logging.getLogger("sts2bot.supervisor")

STEAM_APP_ID = 2868840
STEAM_LAUNCH = f"steam -applaunch {STEAM_APP_ID}"
PROCESS_NAME = "SlayTheSpire2"

# After the first run on a fresh profile the game hides "Singleplayer" until
# the Timeline screen has been visited, and the bridge mod refuses to open the
# Timeline from automation, so the bot would sit at the main menu forever.
# STS2_DEV_SKIP is the game's own switch for skipping that requirement (and
# the intro logo). It only reaches the game when we start the game binary
# ourselves: `steam -applaunch` hands the launch to the running Steam client.
# SteamAppId lets the directly started game attach to Steam (cloud saves etc.).
GAME_ENV = {
    "STS2_DEV_SKIP": "1",
    "SteamAppId": str(STEAM_APP_ID),
    "SteamGameId": str(STEAM_APP_ID),
}


def game_binary() -> Path | None:
    base = os.environ.get("STS2_GAME_DIR") or str(
        Path.home() / ".local/share/Steam/steamapps/common/Slay the Spire 2"
    )
    exe = Path(base) / PROCESS_NAME
    return exe if os.access(exe, os.X_OK) else None


def default_launch() -> str:
    """Start the game binary directly when we can find it, else go through Steam."""
    exe = game_binary()
    return shlex.quote(str(exe)) if exe else STEAM_LAUNCH


DEFAULT_LAUNCH = default_launch()


def game_running() -> bool:
    return subprocess.run(["pgrep", "-x", PROCESS_NAME], capture_output=True).returncode == 0


def kill_game(grace: float = 10.0) -> None:
    subprocess.run(["pkill", "-x", PROCESS_NAME], capture_output=True)
    deadline = time.monotonic() + grace
    while time.monotonic() < deadline and game_running():
        time.sleep(0.5)
    if game_running():
        subprocess.run(["pkill", "-9", "-x", PROCESS_NAME], capture_output=True)
        time.sleep(1.0)


def launch_game(cmd: str) -> None:
    log.info("launching game: %s", cmd)
    argv = shlex.split(cmd)
    exe = Path(argv[0])
    subprocess.Popen(
        argv,
        cwd=exe.parent if exe.is_absolute() and exe.parent.is_dir() else None,
        env={**os.environ, **GAME_ENV},
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )


class Supervisor:
    def __init__(
        self,
        bot: Bot,
        client: Client,
        *,
        launch_cmd: str = DEFAULT_LAUNCH,
        manage_game: bool = True,
        boot_timeout: float = 240.0,
        outage_grace: float = 45.0,
    ):
        self.bot = bot
        self.client = client
        self.launch_cmd = launch_cmd
        self.manage_game = manage_game
        self.boot_timeout = boot_timeout
        self.outage_grace = outage_grace
        self.restarts = 0
        self._last_stuck_at: tuple | None = None

    def _wait_for_api(self, timeout: float) -> bool:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.client.ping():
                return True
            time.sleep(2.0)
        return False

    def ensure_game(self) -> None:
        """Block until the bridge mod answers, (re)launching the game as needed."""
        backoff = 10.0
        while True:
            if self.client.ping():
                return
            if not self.manage_game:
                log.info("waiting for the game (start it yourself; --no-launch is set)")
                self._wait_for_api(60.0)
                continue
            if not game_running():
                launch_game(self.launch_cmd)
            if self._wait_for_api(self.boot_timeout):
                log.info("game is up")
                return
            log.error(
                "game did not expose the bot API within %.0fs. Is the STS2_MCP mod installed "
                "and are mods enabled in the game's settings? Restarting the game.",
                self.boot_timeout,
            )
            kill_game()
            time.sleep(backoff)
            backoff = min(backoff * 2, 300.0)

    def restart_game(self, why: str) -> None:
        self.restarts += 1
        log.warning("restart #%d: %s", self.restarts, why)
        if self.manage_game:
            kill_game()
            time.sleep(3.0)

    def run(self, max_runs: int | None = None) -> None:
        while max_runs is None or self.bot.runs_finished < max_runs:
            self.ensure_game()
            try:
                self.bot.run(max_runs)
            except GameUnavailable as e:
                log.warning("lost contact with the game: %s", e)
                if self._wait_for_api(self.outage_grace):
                    continue
                self.restart_game("game stopped answering")
            except Stuck as e:
                where = self.bot.progress()
                if where == self._last_stuck_at:
                    log.warning("stuck twice at %s; the run will be abandoned", where)
                    self.bot.abandon_next = True
                self._last_stuck_at = where
                if not self.manage_game:
                    log.error("%s (not restarting: --no-launch is set)", e)
                    time.sleep(30.0)
                self.restart_game(str(e))
