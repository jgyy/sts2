"""What the supervisor needs to know about the first Slay the Spire: ModTheSpire with BaseMod and CommunicationMod (from the
Workshop), which starts relay.py for the bot (`write_comm_config`, from `python3 -m sts2bot sts1-setup`)."""

from __future__ import annotations

import json
import logging
import os
import re
import shlex
import subprocess
import sys
import time
from pathlib import Path

from ... import supervisor
from . import relay, speedmod
from .screens import CHARACTERS

log = logging.getLogger("sts2bot.supervisor")

STEAM_APP_ID = 646570
STEAM_LAUNCH = f"steam -applaunch {STEAM_APP_ID}"
MTS_WORKSHOP_ID = 1605060445  # ModTheSpire on the Steam Workshop
MODS = "basemod,CommunicationMod"
PROCESS_MARKS = ("ModTheSpire", "desktop-1.0.jar")  # what the game's java process has on its command line
RELAY = Path(__file__).resolve().parent / "relay.py"


def steam_root() -> Path:
    return Path.home() / ".local/share/Steam/steamapps"


def game_dir() -> Path:
    return Path(os.environ.get("STS1_GAME_DIR") or steam_root() / "common" / "SlayTheSpire")


def mts_jar() -> Path | None:
    for p in (os.environ.get("STS1_MTS_JAR"), steam_root() / "workshop/content" / str(STEAM_APP_ID) / str(MTS_WORKSHOP_ID) / "ModTheSpire.jar", game_dir() / "ModTheSpire.jar"):
        if p and Path(p).is_file():
            return Path(p)
    return None


def java() -> str:
    bundled = game_dir() / "jre" / "bin" / "java"
    return str(bundled) if os.access(bundled, os.X_OK) else "java"


def default_launch() -> str:
    """ModTheSpire straight into the game with the two mods, else Steam (which shows ModTheSpire's launcher)."""
    jar = mts_jar()
    return STEAM_LAUNCH if jar is None else " ".join(shlex.quote(a) for a in (java(), "-jar", str(jar), "--skip-launcher", "--skip-intro", "--mods", MODS))


def prefs_dir() -> Path:
    return Path(os.environ.get("STS1_PREFS_DIR") or game_dir() / "preferences")


# Each character's record in the first save slot (the game's CharStat files: the Ironclad's still has its old name).
ACT4 = ("IRONCLAD", "THE_SILENT", "DEFECT")  # the characters whose wins open act 4 (keys.py)
STAT_FILE = {"IRONCLAD": "STSDataVagabond", "THE_SILENT": "STSDataTheSilent", "DEFECT": "STSDataDefect", "WATCHER": "STSDataWatcher"}
# Where the game notes that a character has been unlocked ("STSUnlocks": 0 or missing is locked; the Ironclad always is not).
UNLOCK_KEY = {"THE_SILENT": "The Silent", "DEFECT": "Defect", "WATCHER": "Watcher"}


def _json(path: Path) -> dict | None:
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def read_prefs(folder: Path | None = None) -> dict:
    """Each character's highest ascension (open once it has won), the wins and who is locked, from the game's save files."""
    folder = folder or prefs_dir()
    out: dict = {"max_ascension": {}, "wins": 0, "locked": []}
    flags = _json(folder / "STSPlayer") or {}
    for who in CHARACTERS:
        data = _json(folder / STAT_FILE[who])
        if data is None:
            continue
        try:
            wins = int(data.get("WIN_COUNT") or 0)
            # After a win the game reads a missing ASCENSION_LEVEL as 1 (Prefs.getInteger(..., 1)).
            level = int(data.get("ASCENSION_LEVEL") or (1 if wins > 0 else 0))
        except (TypeError, ValueError):
            continue
        out["wins"] += wins
        out["max_ascension"][who] = min(20, level) if wins > 0 or level > 1 else 0
        if who in ACT4 and str(flags.get(f"{who}_WIN", "")).lower() != "true":
            # Act 4 opens on the three CLASS_WIN flags, which only a win the bot's mod saw sets: until then the character
            # counts as unclimbed, so the rotation plays it (2026-11-25: the Defect's one win predates the mod; DEFECT_WIN
            # was all act 4 lacked, and the Watcher alone, the only one on 0, played 35 runs in a row).
            out["max_ascension"][who] = 0
    unlocks = _json(folder / "STSUnlocks") or {}
    for who, key in UNLOCK_KEY.items():
        try:
            if int(unlocks.get(key) or 0) <= 0:
                out["locked"].append(who)
        except (TypeError, ValueError):
            out["locked"].append(who)
    return out


# What a game started by the bot is set to. A higher frame cap shortens every wait the game measures in frames.
FRAME_CAP = 240
# Sound at full while the game has the focus, none while it is behind another window (the game's own Mute in Bg).
SOUND = {"Master Volume": "1.0", "Music Volume": "1.0", "Sound Volume": "1.0", "Ambience On": "true", "Mute in Bg": "true"}
# The first-run tips marked seen, and NO_FTUE (no tutorial): a tip's pop-up waits for a click CommunicationMod cannot give.
TIPS_SEEN = ("NO_FTUE", "NEOW_SKIP", "NEOW_INTRO", "COMBAT_TIP", "SHUFFLE_TIP", "POWER_TIP", "M_POWER_TIP", "INTENT_TIP", "POTION_TIP", "RELIC_TIP", "CARD_REWARD_TIP", "ENERGY_USE_TIP")
# A profile for the first save slot when there is none (the game makes one only on a name screen CommunicationMod never shows).
PROFILE_NAME = "sts2bot"


def prepare_game(fullscreen: bool = True, folder: Path | None = None, game: Path | None = None) -> list[str]:
    """Set the display, Fast Mode, the sound, and on a new profile the first-run tips as seen. Returns what was changed."""
    game, folder, changed = game or game_dir(), folder or prefs_dir(), []
    display = game / "info.displayconfig"  # width, height, frame cap, full screen, windowed full screen (borderless), vsync
    try:
        lines = display.read_text().split()
    except OSError:
        lines = []
    width, height = (lines[0], lines[1]) if len(lines) >= 2 and lines[0].isdigit() and lines[1].isdigit() else ("1920", "1080")
    wanted = [width, height, str(FRAME_CAP), "false", "true" if fullscreen else "false", "false"]
    if lines != wanted:
        try:
            display.write_text("\n".join(wanted) + "\n")
            changed.append("display " + ("full screen" if fullscreen else "windowed") + f", {FRAME_CAP} fps")
        except OSError as e:
            log.warning("could not write %s: %s", display, e)
    try:
        folder.mkdir(parents=True, exist_ok=True)
        settings_file = folder / "STSGameplaySettings"
        settings = _json(settings_file) or {}
        if settings.get("Fast Mode") != "true" or settings.get("Particle Effects") != "false":
            # (Particle effects off: less to draw, so busy fights keep the frame cap.)
            settings.update({"Fast Mode": "true", "Particle Effects": "false", "LANGUAGE": settings.get("LANGUAGE", "ENG")})
            settings_file.write_text(json.dumps(settings, indent=2))
            changed.append("Fast Mode on, particle effects off")
        sound_file = folder / "STSSound"
        sound = _json(sound_file) or {}
        if any(sound.get(k) != v for k, v in SOUND.items()):
            sound_file.write_text(json.dumps({**sound, **SOUND}, indent=2))
            changed.append("volume at full")
        tips_file = folder / "STSTips"
        tips = _json(tips_file) or {}
        if any(tips.get(k) != "true" for k in TIPS_SEEN):
            tips_file.write_text(json.dumps({**tips, **{k: "true" for k in TIPS_SEEN}}, indent=2))
            changed.append("first-run tips off")
        slots_file = folder / "STSSaveSlots"
        slots = _json(slots_file) or {}
        if str(slots.get("DEFAULT_SLOT", "-1")) in ("-1", "0") and not slots.get("PROFILE_NAME"):
            slots_file.write_text(json.dumps({**slots, "DEFAULT_SLOT": "0", "PROFILE_NAME": PROFILE_NAME, "COMPLETION": slots.get("COMPLETION", "0.0"),
                                              "PLAYTIME": slots.get("PLAYTIME", "0")}, indent=2))
            changed.append(f"profile {PROFILE_NAME!r} in the first save slot")
    except OSError as e:
        log.warning("could not write the game's settings in %s: %s", folder, e)
    return changed


def comm_config_path() -> Path:
    base = os.environ.get("XDG_CONFIG_HOME") or str(Path.home() / ".config")
    return Path(base) / "ModTheSpire" / "CommunicationMod" / "config.properties"


def write_comm_config(port: int, path: Path | None = None) -> Path:
    """Have CommunicationMod start the relay with the game (it reads this file at start-up)."""
    path = path or comm_config_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    command = f"{sys.executable} {RELAY} --port {port}"
    # (Java properties: a POSIX path needs no escaping. verbose=false, or every message both ways goes to game.log, 40 MB an hour.)
    path.write_text(f"command={command}\nrunAtGameStart=true\nverbose=false\n")
    return path


def comm_command(path: Path | None = None) -> str | None:
    """The program CommunicationMod starts with the game (None if it has no config yet)."""
    try:
        text = (path or comm_config_path()).read_text()
    except OSError:
        return None
    return next((line[len("command="):].strip() for line in text.splitlines() if line.startswith("command=")), None)


def comm_config_problem(port: int, path: Path | None = None) -> str | None:
    """Why CommunicationMod will not start the relay this bot listens to on `port` (None when it will): without it the game never answers."""
    command = comm_command(path)
    if command is None:
        return f"CommunicationMod has no command to start ({path or comm_config_path()})"
    if RELAY.name not in command:
        return f"CommunicationMod starts another program with the game: {command}"
    found = re.search(r"--port[=\s]+(\d+)", command)
    relay_port = int(found.group(1)) if found else relay.DEFAULT_PORT
    if relay_port != port:
        return f"CommunicationMod starts the relay on another port than {port}: {command}"
    return None


def _game_processes() -> list[str]:
    rows = [line.split(None, 2) for line in subprocess.run(["ps", "-eo", "pid=,stat=,args="], capture_output=True, text=True).stdout.splitlines()]
    return [p[0] for p in rows if len(p) == 3 and not p[1].startswith("Z") and any(m in p[2] for m in PROCESS_MARKS) and "relay.py" not in p[2]]


class Sts1Host:
    name = "Slay the Spire"
    heals_mod = False  # CommunicationMod comes built from the Workshop

    def __init__(self, log_dir: Path | None = None, fullscreen: bool = True, time_scale: float = 1.0):
        # The game's output goes to game.log in log_dir (the one before to game.log.1); time_scale is its clock's speed (speedmod.py).
        self.log_dir, self.fullscreen, self.time_scale = log_dir, fullscreen, time_scale

    def running(self) -> bool:
        supervisor._reap()
        return bool(_game_processes())

    def launch(self, cmd: str) -> None:
        changed = prepare_game(self.fullscreen)
        if changed:
            log.info("game settings: %s", ", ".join(changed))
        env = None
        if f"--mods {MODS}" in cmd and speedmod.ensure(game_dir(), mts_jar()):
            # Our own mod: the faster game clock (Fast Mode alone left a run at 12-15 minutes), the unlocks and more state.
            cmd = cmd.replace(f"--mods {MODS}", f"--mods {MODS},{speedmod.MOD_ID}")
            env = {**os.environ, speedmod.ENV: f"{self.time_scale or 1.0:g}"}
            log.info("game clock: x%g", self.time_scale or 1.0)
        log.info("launching game: %s", cmd)
        cwd = game_dir() if game_dir().is_dir() else None
        out = None
        if self.log_dir is not None:  # (so that a crash leaves a trace)
            path = Path(self.log_dir) / "game.log"
            try:
                if path.exists():
                    path.replace(path.with_name("game.log.1"))
                out = open(path, "w")
            except OSError as e:
                log.warning("could not open %s: %s", path, e)
        try:
            supervisor._spawn(shlex.split(cmd), cwd=cwd, **({"stdout": out, "stderr": out} if out else {}), **({"env": env} if env else {}))
        finally:
            if out:
                out.close()  # (the game holds its own copy of the file)

    def kill(self, grace: float = 10.0) -> None:
        for sig in ("-TERM", "-KILL"):
            for pid in _game_processes():
                subprocess.run(["kill", sig, pid], capture_output=True)
            deadline = time.monotonic() + grace
            while time.monotonic() < deadline and self.running():
                time.sleep(0.5)
            if not self.running():
                return

    def mod_is_stale(self) -> bool:
        return False

    def rebuild_mod(self) -> bool:
        return False

    def mod_load_error(self, since: float = 0.0) -> str | None:
        return None

    def missing_steam(self, since: float = 0.0) -> bool:
        return False

    def no_api_hint(self) -> str:
        return "Are BaseMod and CommunicationMod enabled in ModTheSpire, and does CommunicationMod start the relay (run `python3 -m sts2bot sts1-setup`)?"

    def configure(self, sup) -> None:
        """Nothing to set: the first game has no bridge for animation speed or graphics (its own Fast Mode setting is the one)."""
