"""What the supervisor needs to know about the first Slay the Spire (see supervisor.Sts2Host for the second).

The game is started through ModTheSpire with BaseMod and CommunicationMod; CommunicationMod starts
relay.py, which is what the bot talks to. Nothing is built: the mods come from the Steam Workshop.
`write_comm_config` points CommunicationMod at the relay (`python3 -m sts2bot sts1-setup`).
"""

from __future__ import annotations

import json
import logging
import os
import shlex
import subprocess
import sys
import time
from pathlib import Path

from ... import supervisor
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
    for p in (os.environ.get("STS1_MTS_JAR"), steam_root() / "workshop/content" / str(STEAM_APP_ID) / str(MTS_WORKSHOP_ID) / "ModTheSpire.jar",
              game_dir() / "ModTheSpire.jar"):
        if p and Path(p).is_file():
            return Path(p)
    return None


def java() -> str:
    bundled = game_dir() / "jre" / "bin" / "java"
    return str(bundled) if os.access(bundled, os.X_OK) else "java"


def default_launch() -> str:
    """ModTheSpire straight into the game with the two mods, else Steam (which shows ModTheSpire's launcher)."""
    jar = mts_jar()
    if jar is None:
        return STEAM_LAUNCH
    return " ".join(shlex.quote(a) for a in (java(), "-jar", str(jar), "--skip-launcher", "--skip-intro", "--mods", MODS))


def prefs_dir() -> Path:
    return Path(os.environ.get("STS1_PREFS_DIR") or game_dir() / "preferences")


def read_prefs(folder: Path | None = None) -> dict:
    """Each character's highest ascension and the wins so far, from the game's own save files (first save slot).

    The files are JSON with string values ("ASCENSION_LEVEL": "5"). Ascension only opens once a
    character has won; before that a run is played on 0."""
    folder = folder or prefs_dir()
    out: dict = {"max_ascension": {}, "wins": 0}
    for who in CHARACTERS:
        try:
            data = json.loads((folder / who).read_text())
        except (OSError, ValueError):
            continue
        try:
            wins = int(data.get("WIN_COUNT") or 0)
            level = int(data.get("ASCENSION_LEVEL") or 0)
        except (TypeError, ValueError):
            continue
        out["wins"] += wins
        out["max_ascension"][who] = min(20, level) if wins > 0 or level > 1 else 0
    return out


def comm_config_path() -> Path:
    base = os.environ.get("XDG_CONFIG_HOME") or str(Path.home() / ".config")
    return Path(base) / "ModTheSpire" / "CommunicationMod" / "config.properties"


def write_comm_config(port: int, path: Path | None = None) -> Path:
    """Have CommunicationMod start the relay with the game (it reads this file at start-up)."""
    path = path or comm_config_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    command = f"{sys.executable} {RELAY} --port {port}"
    # (Java's properties format: backslashes and colons would need escaping; a POSIX path has neither.)
    path.write_text(f"command={command}\nrunAtGameStart=true\n")
    return path


def _game_processes() -> list[str]:
    out = subprocess.run(["ps", "-eo", "pid=,stat=,args="], capture_output=True, text=True).stdout
    found = []
    for line in out.splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3 and not parts[1].startswith("Z") and any(m in parts[2] for m in PROCESS_MARKS) and "relay.py" not in parts[2]:
            found.append(parts[0])
    return found


class Sts1Host:
    name = "Slay the Spire"
    heals_mod = False  # CommunicationMod comes built from the Workshop

    def running(self) -> bool:
        for proc in list(supervisor._children):
            if proc.poll() is not None:
                supervisor._children.remove(proc)
        return bool(_game_processes())

    def launch(self, cmd: str) -> None:
        log.info("launching game: %s", cmd)
        cwd = game_dir() if game_dir().is_dir() else None
        supervisor._spawn(shlex.split(cmd), cwd=cwd)

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
        return ("Are BaseMod and CommunicationMod enabled in ModTheSpire, and does CommunicationMod start the relay "
                "(run `python3 -m sts2bot sts1-setup`)?")

    def configure(self, sup) -> None:
        """Nothing to set: the first game has no bridge for animation speed or graphics (its own Fast Mode setting is the one)."""
