"""Keeps the game and the bot alive around the clock: launches the game, waits for its bridge mod, plays, and restarts it
when it crashes, freezes or the bot gets stuck. What is particular to a game is its host (`Sts2Host` here, `games.sts1.host.Sts1Host`)."""

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
STEAM_SETTLE = 10.0  # after Steam's UI is up: logging in takes a moment longer
PROCESS_NAME = "SlayTheSpire2"
# SteamAppId lets the directly started game attach to Steam. (No STS2_DEV_SKIP: it kept what the Timeline unlocks locked.)
GAME_ENV = {"SteamAppId": str(STEAM_APP_ID), "SteamGameId": str(STEAM_APP_ID)}


def game_dir() -> Path:
    return Path(os.environ.get("STS2_GAME_DIR") or Path.home() / ".local/share/Steam/steamapps/common/Slay the Spire 2")


# The bridge mod is built against the game's code: rebuilt when a game update changed it, or the game's log says it failed to load.
MOD_NAME = "STS2_MCP"
INSTALL_SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "install_mod.sh"
MOD_STAMP = INSTALL_SCRIPT.parent.parent / ".build" / "mod_built_for"
GAME_LOG = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share") / "SlayTheSpire2" / "logs" / "godot.log"


def game_fingerprint() -> str | None:
    """Changes whenever a game update replaces the code the mod is built against."""
    stats = [(dll.parent.name, dll.stat()) for dll in sorted(game_dir().glob("data_sts2_*/sts2.dll"))]
    return ";".join(f"{name}:{st.st_size}:{st.st_mtime_ns}" for name, st in stats) or None


def mod_is_stale() -> bool:
    """True if the game files changed since the mod was last built."""
    now = game_fingerprint()
    if now is None or not INSTALL_SCRIPT.exists():
        return False
    try:
        return MOD_STAMP.read_text().strip() != now
    except OSError:
        return True


def _game_log(since: float) -> str:
    """The game's log ("" if there is none, or it was last written before `since`)."""
    try:
        return GAME_LOG.read_text(errors="replace") if GAME_LOG.stat().st_mtime >= since else ""
    except OSError:
        return ""


def mod_load_error(since: float = 0.0) -> str | None:
    """Why the game refused to load the mod, from its log (None if it did not). `since` (a time.time()) skips an older log."""
    text = _game_log(since)
    at = text.find(f"Exception thrown while loading mod {MOD_NAME}")
    if at < 0:
        return None
    for line in text[at:].splitlines()[1:12]:
        line = line.strip()
        if line and not line.startswith("at "):
            return line
    return "the game log does not say why"


def rebuild_mod() -> bool:
    """Build the mod against the installed game and install it. True on success."""
    if not INSTALL_SCRIPT.exists():
        log.error("cannot rebuild the bridge mod: %s is missing", INSTALL_SCRIPT)
        return False
    log.warning("rebuilding the bridge mod against the current game files")
    fingerprint = game_fingerprint()
    try:
        done = subprocess.run(["bash", str(INSTALL_SCRIPT)], capture_output=True, text=True, timeout=900, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.TimeoutExpired) as e:
        log.error("rebuilding the bridge mod failed: %s", e)
        return False
    if done.returncode != 0:
        output = (done.stdout + "\n" + done.stderr).splitlines()
        errors = sorted({re.sub(r"\s*\[/.*?\]$", "", ln.strip()) for ln in output if "error" in ln.lower()})
        log.error("the bridge mod does not build against this game version; scripts/mod_patches needs a fix for the game's changed code. Compiler said:\n  %s",
                  "\n  ".join((errors or output[-15:])[:15]))
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
    exe = game_dir() / PROCESS_NAME
    return shlex.quote(str(exe)) if os.access(exe, os.X_OK) else STEAM_LAUNCH


DEFAULT_LAUNCH = default_launch()

_children: list[subprocess.Popen] = []  # games we started; must be reaped when they die


def _reap() -> None:
    """Collect the children that have exited (a crashed game stays a zombie until then, and would count as running)."""
    for proc in list(_children):
        if proc.poll() is not None:
            _children.remove(proc)


def game_running() -> bool:
    """True if a live game process exists."""
    _reap()
    return _process_running(PROCESS_NAME)


def _process_running(name: str) -> bool:
    out = subprocess.run(["ps", "-C", name, "-o", "stat="], capture_output=True, text=True).stdout
    return any(line.strip() and not line.strip().startswith("Z") for line in out.splitlines())


def _failed(result: dict) -> bool:
    return result.get("status") == "error" or "error" in result


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
    """Ask the game for an animation speed through the mod, until it exits (Steam cloud sync restores an edited prefs.save)."""
    result = client.act("set_fast_mode", mode=mode)
    # Straight after a launch the game has not read its preferences yet; it is ready a few seconds later.
    for _ in range(20):
        if "not loaded" not in str(result.get("error") or ""):
            break
        time.sleep(1.0)
        result = client.act("set_fast_mode", mode=mode)
    if _failed(result):
        log.warning("could not set fast mode %r (%s); re-run scripts/install_mod.sh to update the mod", mode, result.get("error"))
        return False
    log.info("fast mode: %s", mode)
    return True


def set_light_graphics(client: Client, width: int = 960, height: int = 540, maximize: bool = True, fullscreen: bool = False) -> bool:
    """Full screen (or a maximised or `width` x `height` window), no anti-aliasing, uncapped frame rate, for this session (cheaper frames, faster answers)."""
    result = client.act("set_light_graphics", width=width, height=height, maximize=maximize, fullscreen=fullscreen)
    if _failed(result):
        log.warning("could not switch to light graphics (%s); re-run scripts/install_mod.sh", result.get("error"))
        return False
    if fullscreen and not result.get("fullscreen"):
        log.warning("the installed mod cannot go full screen; re-run scripts/install_mod.sh")
    elif maximize and not result.get("maximized"):
        log.warning("the installed mod cannot maximise the window; re-run scripts/install_mod.sh")
    size = "full screen" if fullscreen and result.get("fullscreen") else "maximised window" if maximize else f"{width}x{height}"
    log.info("light graphics: %s (was %.0f fps)", size, float(result.get("fps_before") or 0))
    return True


def set_time_scale(client: Client, scale: float) -> bool:
    """Run the game's own clock (its timers between moves, rooms and rewards) `scale` times faster, for this session."""
    result = client.act("set_time_scale", scale=scale)
    if _failed(result):
        log.warning("could not set the time scale (%s); re-run scripts/install_mod.sh to update the mod", result.get("error"))
        return False
    log.info("time scale: x%g", scale)
    return True


def _detached(argv: list[str]) -> list[str]:
    """Under systemd, Steam and the game get a scope of their own: restarting the bot's service leaves them running."""
    if os.environ.get("INVOCATION_ID") and shutil.which("systemd-run"):
        return ["systemd-run", "--user", "--scope", "--quiet", "--collect", "--", *argv]
    return argv


def _spawn(argv: list[str], **kw) -> None:
    """Start a process of its own (Steam, the game) and remember it, to reap it when it dies."""
    quiet = dict(stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    quiet.update(kw)  # (a caller may keep the output: the first game's goes to its game.log)
    _children.append(subprocess.Popen(_detached(argv), start_new_session=True, **quiet))


def ensure_steam(timeout: float = 180.0) -> bool:
    """Start the Steam client if it is not running and wait until it is ready (without it the game never loads the mod)."""
    if _process_running("steam"):
        return True
    steam = shutil.which("steam")
    if not steam:
        log.error("Steam is not running and no `steam` command was found to start it")
        return False
    log.info("Steam is not running; starting it")
    try:
        _spawn([steam, "-silent"])
    except OSError as e:
        log.error("could not start Steam: %s", e)
        return False
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if _process_running("steam") and _process_running("steamwebhelper"):  # (its UI: up once Steam has updated itself)
            time.sleep(STEAM_SETTLE)
            log.info("Steam is up")
            return True
        time.sleep(2.0)
    log.error("Steam did not come up within %.0fs (is it logged in?)", timeout)
    return False


def launch_game(cmd: str) -> None:
    argv = shlex.split(cmd)
    exe = Path(argv[0])
    if exe.name == PROCESS_NAME:
        ensure_steam()  # `steam -applaunch ...` starts Steam by itself
    log.info("launching game: %s", cmd)
    env = {**os.environ, **GAME_ENV}
    env.pop("STS2_DEV_SKIP", None)
    _spawn(argv, cwd=exe.parent if exe.is_absolute() and exe.parent.is_dir() else None, env=env)


class Sts2Host:
    """What the supervisor needs to know about Slay the Spire 2 (calls look the module's functions up, so tests can replace them)."""

    name = "Slay the Spire 2"
    heals_mod = True  # the bridge mod can be rebuilt by the supervisor

    def running(self) -> bool:
        return game_running()

    def launch(self, cmd: str) -> None:
        launch_game(cmd)

    def kill(self) -> None:
        kill_game()

    def mod_is_stale(self) -> bool:
        return mod_is_stale()

    def rebuild_mod(self) -> bool:
        return rebuild_mod()

    def mod_load_error(self, since: float = 0.0) -> str | None:
        return mod_load_error(since)

    def missing_steam(self, since: float = 0.0) -> bool:
        """True if the game came up without the Steam client it needs (its log says so)."""
        return not _process_running("steam") and "Steam is probably not running" in _game_log(since)

    def no_api_hint(self) -> str:
        return "Is the STS2_MCP mod installed and are mods enabled in the game's settings?"

    def configure(self, sup: "Supervisor") -> None:
        """Session settings, sent each time the bot (re)connects."""
        if sup.fast_mode:
            set_fast_mode(sup.client, sup.fast_mode)
        if sup.light_graphics:
            set_light_graphics(sup.client, maximize=not sup.small_window, fullscreen=sup.fullscreen and not sup.small_window)
        if sup.time_scale and sup.time_scale != 1.0 and set_time_scale(sup.client, sup.time_scale):
            sup.bot.time_scale = sup.time_scale  # the bot keeps it set: the game resets it


class Supervisor:
    def __init__(self, bot: Bot, client: Client, *, host=None, launch_cmd: str = DEFAULT_LAUNCH, manage_game: bool = True, fast_mode: str | None = "instant",
                 light_graphics: bool = True, small_window: bool = False, fullscreen: bool = True, time_scale: float = 1.0, boot_timeout: float = 240.0,
                 outage_grace: float = 45.0, heal_mod: bool = True):
        self.bot, self.client, self.host = bot, client, host or Sts2Host()
        self.launch_cmd, self.manage_game, self.fast_mode, self.light_graphics = launch_cmd, manage_game, fast_mode, light_graphics
        self.small_window, self.fullscreen, self.time_scale = small_window, fullscreen, time_scale
        self.boot_timeout, self.outage_grace = boot_timeout, outage_grace
        self.heal_mod = heal_mod and manage_game and self.host.heals_mod
        self.restarts = 0
        self._last_stuck_at: tuple | None = None
        self._mod_retry_at = 0.0  # no rebuild attempts before this (after a failed build)
        self._crash_at: tuple | None = None  # where the bot was when the game last crashed
        self._crash_repeats = 0
        self._mod_backoff = 300.0

    def _wait_for_api(self, timeout: float, launched_at: float | None = None) -> bool:
        """Wait for the mod to answer. With `launched_at`, give up as soon as the game's log shows it refused the mod."""
        started = time.monotonic()
        deadline = started + timeout
        while time.monotonic() < deadline:
            if self.client.ping():
                return True
            if launched_at is not None and self.host.mod_load_error(launched_at):
                return False
            if self.manage_game and time.monotonic() - started > 10.0:
                if not self.host.running():
                    return False  # it died: no point waiting out the timeout
                if self.host.missing_steam(launched_at or 0.0):
                    return False
            time.sleep(2.0)
        return False

    def _rebuild_mod(self) -> bool:
        """Rebuild the mod, at most once per back-off period while it keeps failing."""
        if time.monotonic() < self._mod_retry_at:
            return False
        if self.host.rebuild_mod():
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
            if not self.host.running():
                if self.heal_mod and self.host.mod_is_stale():
                    log.warning("the game files changed since the bridge mod was built (game update?)")
                    self._rebuild_mod()
                launched_at = time.time() - 1.0
                self.host.launch(self.launch_cmd)
            if self._wait_for_api(self.boot_timeout, launched_at if self.heal_mod else None):
                log.info("game is up")
                return
            why = self.host.mod_load_error(launched_at or 0.0)
            if why:
                log.error("the game did not load the bridge mod: %s", why)
                self.host.kill()
                if self.heal_mod and self._rebuild_mod():
                    backoff = 10.0
                    continue  # relaunch straight away with the fresh build
                # Nothing more to try until the mod is fixed or the game updates again.
                wait = max(backoff, self._mod_retry_at - time.monotonic()) if self.heal_mod else backoff
                time.sleep(max(1.0, min(wait, 3600.0)))
                backoff = min(backoff * 2, 300.0)
                continue
            if not self.host.running():
                log.error("the game exited before the bot API came up; starting it again")
            elif self.host.missing_steam(launched_at or 0.0):
                log.error("the game was started without Steam; restarting it with Steam running")
            else:
                log.error("game did not expose the bot API within %.0fs. %s Restarting the game.", self.boot_timeout, self.host.no_api_hint())
            self.host.kill()
            time.sleep(backoff)
            backoff = min(backoff * 2, 300.0)

    def _note_crash(self) -> None:
        """Break a crash loop: the game reloads the room it crashed in, so the bot avoids what it asked last (slowly, the second time),
        and the run is abandoned at the third crash in one place with nothing else to try, at the fifth in any case."""
        where = self.bot.progress()
        self._crash_repeats = self._crash_repeats + 1 if where == self._crash_at else 0
        self._crash_at = where
        another_way = self.bot.note_crash() is True
        if self._crash_repeats >= 4 or (self._crash_repeats >= 2 and not another_way):
            log.warning("the game keeps crashing at %s; the run will be abandoned", where)
            self.bot.abandon_next = True
            self._crash_repeats = 0
        elif self._crash_repeats >= 1:
            log.warning("the game crashed again at %s; playing that room slowly", where)
            self.bot.careful_until, self.bot.careful_at = self.bot.clock() + 120.0, where

    def restart_game(self, why: str) -> None:
        self.restarts += 1
        log.warning("restart #%d: %s", self.restarts, why)
        if self.manage_game:
            self.host.kill()
            time.sleep(3.0)

    def run(self, max_runs: int | None = None) -> None:
        while max_runs is None or self.bot.runs_finished < max_runs:
            self.ensure_game()
            try:
                self.host.configure(self)
                self.bot.run(max_runs)
            except GameUnavailable as e:
                log.warning("lost contact with the game: %s", e)
                if self.manage_game and not self.host.running():
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
