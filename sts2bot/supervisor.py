"""Keeps the game and the bot alive around the clock.

Launches Slay the Spire 2 when it is not running, waits for the bridge mod to
answer, plays, and restarts the game if it crashes, freezes, or the bot gets
stuck. A run that hangs twice in the same place is abandoned.
"""

from __future__ import annotations

import logging
import os
import re
import shlex
import shutil
import subprocess
import time
from pathlib import Path

from .bot import Bot, Stuck
from .client import Client, GameUnavailable

log = logging.getLogger("sts2bot.supervisor")

STEAM_APP_ID = 2868840
STEAM_LAUNCH = f"steam -applaunch {STEAM_APP_ID}"
PROCESS_NAME = "SlayTheSpire2"

# SteamAppId lets the directly started game attach to Steam (cloud saves etc.).
#
# The game used to be started with STS2_DEV_SKIP=1, its own switch for
# skipping the Timeline screen. That also kept everything the Timeline unlocks
# (ascension levels, new characters, cards) locked for good, so the game now
# runs the normal way and the bot clicks through the Timeline itself.
GAME_ENV = {
    "SteamAppId": str(STEAM_APP_ID),
    "SteamGameId": str(STEAM_APP_ID),
}


def game_dir() -> Path:
    return Path(os.environ.get("STS2_GAME_DIR") or Path.home() / ".local/share/Steam/steamapps/common/Slay the Spire 2")


def game_binary() -> Path | None:
    exe = game_dir() / PROCESS_NAME
    return exe if os.access(exe, os.X_OK) else None


# ----------------------------------------------------------- bridge mod upkeep
#
# The game updates itself through Steam, and an update regularly breaks the
# bridge mod: it is compiled against the game's own code, so when a class it
# touches is renamed or removed the game refuses to load it ("Loaded 0 mods
# WITH ERRORS") and the bot has nothing to talk to. Rebuilding the mod against
# the new game files fixes most of these, so the supervisor does that itself:
# before a launch when the game files changed, and after a launch when the
# game's log says the mod failed to load.

MOD_NAME = "STS2_MCP"
INSTALL_SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "install_mod.sh"
MOD_STAMP = INSTALL_SCRIPT.parent.parent / ".build" / "mod_built_for"
GAME_LOG = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share") / "SlayTheSpire2" / "logs" / "godot.log"


def game_fingerprint() -> str | None:
    """Changes whenever a game update replaces the code the mod is built against."""
    parts = []
    for dll in sorted(game_dir().glob("data_sts2_*/sts2.dll")):
        st = dll.stat()
        parts.append(f"{dll.parent.name}:{st.st_size}:{st.st_mtime_ns}")
    return ";".join(parts) or None


def mod_is_stale() -> bool:
    """True if the game files changed since the mod was last built."""
    now = game_fingerprint()
    if now is None or not INSTALL_SCRIPT.exists():
        return False
    try:
        return MOD_STAMP.read_text().strip() != now
    except OSError:
        return True


def mod_load_error(since: float = 0.0) -> str | None:
    """Why the game refused to load the mod, read from the game's log (None if it did not).

    `since` (a time.time() value) skips a log left over from an earlier launch.
    """
    try:
        if GAME_LOG.stat().st_mtime < since:
            return None
        text = GAME_LOG.read_text(errors="replace")
    except OSError:
        return None
    marker = f"Exception thrown while loading mod {MOD_NAME}"
    at = text.find(marker)
    if at < 0:
        return None
    for line in text[at:].splitlines()[1:12]:
        line = line.strip()
        if line and not line.startswith("at "):
            return line
    return "the game log does not say why"


def started_without_steam(since: float = 0.0) -> bool:
    """True if the game's log says it came up with no Steam client to talk to."""
    try:
        if GAME_LOG.stat().st_mtime < since:
            return False
        return "Steam is probably not running" in GAME_LOG.read_text(errors="replace")
    except OSError:
        return False


def rebuild_mod() -> bool:
    """Build the mod against the installed game and install it. True on success."""
    if not INSTALL_SCRIPT.exists():
        log.error("cannot rebuild the bridge mod: %s is missing", INSTALL_SCRIPT)
        return False
    log.warning("rebuilding the bridge mod against the current game files")
    fingerprint = game_fingerprint()
    try:
        done = subprocess.run(
            ["bash", str(INSTALL_SCRIPT)], capture_output=True, text=True, timeout=900, stdin=subprocess.DEVNULL
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        log.error("rebuilding the bridge mod failed: %s", e)
        return False
    if done.returncode != 0:
        output = (done.stdout + "\n" + done.stderr).splitlines()
        errors = sorted({re.sub(r"\s*\[/.*?\]$", "", ln.strip()) for ln in output if "error" in ln.lower()})
        log.error(
            "the bridge mod does not build against this game version; scripts/mod_patches needs a fix "
            "for the game's changed code. Compiler said:\n  %s",
            "\n  ".join((errors or output[-15:])[:15]),
        )
        return False
    if fingerprint:
        try:
            MOD_STAMP.parent.mkdir(parents=True, exist_ok=True)
            MOD_STAMP.write_text(fingerprint + "\n")
        except OSError as e:
            log.debug("could not record the mod build: %s", e)
    log.info("bridge mod rebuilt and installed")
    return True


def default_launch() -> str:
    """Start the game binary directly when we can find it, else go through Steam."""
    exe = game_binary()
    return shlex.quote(str(exe)) if exe else STEAM_LAUNCH


DEFAULT_LAUNCH = default_launch()


_children: list[subprocess.Popen] = []  # games we started; must be reaped when they die


def game_running() -> bool:
    """True if a live game process exists.

    A game we launched that then crashes stays in the process table as a
    zombie until we collect it. Counting that as "running" made the bot wait
    forever for a dead game instead of starting a new one.
    """
    for proc in list(_children):
        if proc.poll() is not None:  # reaps it if it has exited
            _children.remove(proc)
    out = subprocess.run(["ps", "-C", PROCESS_NAME, "-o", "stat="], capture_output=True, text=True).stdout
    return any(line.strip() and not line.strip().startswith("Z") for line in out.splitlines())


def kill_game(grace: float = 10.0) -> None:
    subprocess.run(["pkill", "-x", PROCESS_NAME], capture_output=True)
    deadline = time.monotonic() + grace
    while time.monotonic() < deadline and game_running():
        time.sleep(0.5)
    if game_running():
        subprocess.run(["pkill", "-9", "-x", PROCESS_NAME], capture_output=True)
        time.sleep(1.0)


FAST_MODES = ("instant", "fast", "normal")


def set_fast_mode(client: Client, mode: str) -> bool:
    """Ask the game for an animation speed ("instant" is the fastest).

    Goes through the bridge mod (scripts/mod_patches/sts2bot.patch) and
    lasts until the game exits, so it is sent after every launch. Editing the
    game's prefs.save instead does not work: Steam cloud sync restores the old
    file when the game starts.
    """
    result = client.act("set_fast_mode", mode=mode)
    if result.get("status") == "error" or "error" in result:
        log.warning(
            "could not set fast mode %r (%s); re-run scripts/install_mod.sh to update the mod",
            mode,
            result.get("error"),
        )
        return False
    log.info("fast mode: %s", mode)
    return True


def set_light_graphics(client: Client, width: int = 960, height: int = 540) -> bool:
    """Small window, no anti-aliasing, uncapped frame rate, for this session only.

    The mod answers each request once per rendered frame, so cheaper frames
    make the whole bot faster. The saved graphics settings are left alone, so
    the game looks normal when it is started by hand.
    """
    result = client.act("set_light_graphics", width=width, height=height)
    if result.get("status") == "error" or "error" in result:
        log.warning("could not switch to light graphics (%s); re-run scripts/install_mod.sh", result.get("error"))
        return False
    log.info("light graphics: %dx%d (was %.0f fps)", width, height, float(result.get("fps_before") or 0))
    return True


def _detached(argv: list[str]) -> list[str]:
    """Under systemd, start Steam and the game in a scope of their own.

    A service's children live in its control group and are killed with it, so
    restarting the bot's service (to pick up new code, say) would take the
    game and Steam down too. In their own scope they keep running.
    """
    if os.environ.get("INVOCATION_ID") and shutil.which("systemd-run"):
        return ["systemd-run", "--user", "--scope", "--quiet", "--collect", "--", *argv]
    return argv


def _process_running(name: str) -> bool:
    out = subprocess.run(["ps", "-C", name, "-o", "stat="], capture_output=True, text=True).stdout
    return any(line.strip() and not line.strip().startswith("Z") for line in out.splitlines())


def steam_running() -> bool:
    return _process_running("steam")


def ensure_steam(timeout: float = 180.0) -> bool:
    """Start the Steam client if it is not running, and wait until it is ready.

    The game started directly needs a running Steam client: without one its
    Steamworks start-up fails ("Steam is probably not running") and it never
    gets as far as loading the bridge mod.
    """
    if steam_running():
        return True
    steam = shutil.which("steam")
    if not steam:
        log.error("Steam is not running and no `steam` command was found to start it")
        return False
    log.info("Steam is not running; starting it")
    try:
        proc = subprocess.Popen(
            _detached([steam, "-silent"]),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError as e:
        log.error("could not start Steam: %s", e)
        return False
    _children.append(proc)
    # The client is usable once its web helper (the UI process) is up; it
    # updates itself first, which can take a while.
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if steam_running() and _process_running("steamwebhelper"):
            time.sleep(STEAM_SETTLE)  # logging in takes a moment longer
            log.info("Steam is up")
            return True
        time.sleep(2.0)
    log.error("Steam did not come up within %.0fs (is it logged in?)", timeout)
    return False


STEAM_SETTLE = 10.0


def launch_game(cmd: str) -> None:
    argv = shlex.split(cmd)
    exe = Path(argv[0])
    if exe.name == PROCESS_NAME:
        ensure_steam()  # `steam -applaunch ...` starts Steam by itself
    log.info("launching game: %s", cmd)
    env = {**os.environ, **GAME_ENV}
    env.pop("STS2_DEV_SKIP", None)
    proc = subprocess.Popen(
        _detached(argv),
        cwd=exe.parent if exe.is_absolute() and exe.parent.is_dir() else None,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    _children.append(proc)


class Supervisor:
    def __init__(
        self,
        bot: Bot,
        client: Client,
        *,
        launch_cmd: str = DEFAULT_LAUNCH,
        manage_game: bool = True,
        fast_mode: str | None = "instant",
        light_graphics: bool = True,
        boot_timeout: float = 240.0,
        outage_grace: float = 45.0,
        heal_mod: bool = True,
    ):
        self.bot = bot
        self.client = client
        self.launch_cmd = launch_cmd
        self.manage_game = manage_game
        self.fast_mode = fast_mode
        self.light_graphics = light_graphics
        self.boot_timeout = boot_timeout
        self.outage_grace = outage_grace
        self.heal_mod = heal_mod and manage_game
        self.restarts = 0
        self._last_stuck_at: tuple | None = None
        self._mod_retry_at = 0.0  # no rebuild attempts before this (after a failed build)
        self._crash_at: tuple | None = None  # where the bot was when the game last crashed
        self._crash_repeats = 0
        self._mod_backoff = 300.0

    def _wait_for_api(self, timeout: float, launched_at: float | None = None) -> bool:
        """Wait for the mod to answer. With `launched_at`, give up as soon as the
        game's log shows that it refused to load the mod."""
        started = time.monotonic()
        deadline = started + timeout
        while time.monotonic() < deadline:
            if self.client.ping():
                return True
            if launched_at is not None and mod_load_error(launched_at):
                return False
            if self.manage_game and time.monotonic() - started > 10.0:
                if not game_running():
                    return False  # it died: no point waiting out the timeout
                if not steam_running() and started_without_steam(launched_at or 0.0):
                    return False
            time.sleep(2.0)
        return False

    def _rebuild_mod(self) -> bool:
        """Rebuild the mod, at most once per back-off period while it keeps failing."""
        if time.monotonic() < self._mod_retry_at:
            return False
        if rebuild_mod():
            self._mod_retry_at, self._mod_backoff = 0.0, 300.0
            return True
        self._mod_retry_at = time.monotonic() + self._mod_backoff
        log.error("will try to rebuild the mod again in %.0f minutes", self._mod_backoff / 60)
        self._mod_backoff = min(self._mod_backoff * 2, 3600.0)
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
            launched_at = None
            if not game_running():
                if self.heal_mod and mod_is_stale():
                    log.warning("the game files changed since the bridge mod was built (game update?)")
                    self._rebuild_mod()
                launched_at = time.time() - 1.0
                launch_game(self.launch_cmd)
            if self._wait_for_api(self.boot_timeout, launched_at if self.heal_mod else None):
                log.info("game is up")
                return
            why = mod_load_error(launched_at or 0.0)
            if why:
                log.error("the game did not load the bridge mod: %s", why)
                kill_game()
                if self.heal_mod and self._rebuild_mod():
                    backoff = 10.0
                    continue  # relaunch straight away with the fresh build
                # Nothing more to try until the mod is fixed or the game updates again.
                wait = max(backoff, self._mod_retry_at - time.monotonic()) if self.heal_mod else backoff
                time.sleep(max(1.0, min(wait, 3600.0)))
                backoff = min(backoff * 2, 300.0)
                continue
            if not game_running():
                log.error("the game exited before the bot API came up; starting it again")
            elif not steam_running() and started_without_steam(launched_at or 0.0):
                log.error("the game was started without Steam; restarting it with Steam running")
            else:
                log.error(
                    "game did not expose the bot API within %.0fs. Is the STS2_MCP mod installed "
                    "and are mods enabled in the game's settings? Restarting the game.",
                    self.boot_timeout,
                )
            kill_game()
            time.sleep(backoff)
            backoff = min(backoff * 2, 300.0)

    def _note_crash(self) -> None:
        """Break a crash loop.

        The game saves at the start of each room, so after a crash the bot
        comes back to the same room and does the same things. If that is what
        crashed the game, it crashes again, for ever. The second time, play
        that stretch slowly; if it still crashes, give the run up.
        """
        where = self.bot.progress()
        self._crash_repeats = self._crash_repeats + 1 if where == self._crash_at else 0
        self._crash_at = where
        if self._crash_repeats >= 3:
            log.warning("the game keeps crashing at %s; the run will be abandoned", where)
            self.bot.abandon_next = True
            self._crash_repeats = 0
        elif self._crash_repeats >= 1:
            log.warning("the game crashed again at %s; playing slowly for a while", where)
            self.bot.careful_until = self.bot.clock() + 120.0

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
                if self.fast_mode:
                    set_fast_mode(self.client, self.fast_mode)
                if self.light_graphics:
                    set_light_graphics(self.client)
                self.bot.run(max_runs)
            except GameUnavailable as e:
                log.warning("lost contact with the game: %s", e)
                if self.manage_game and not game_running():
                    self._note_crash()
                    self.restart_game("the game crashed")  # no point waiting for a dead process
                    continue
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
