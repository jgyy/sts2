"""A ModTheSpire mod of our own for the first game: its clock run faster (its "time scale"), and its character unlocks.

Slay the Spire 2 has one through the bridge mod (`set_time_scale`); the first game has only its Fast Mode setting,
under which an enemy turn still took 2.6 s and a card 0.5 s (first live runs, 2026-11-24). Everything the game
animates or waits on is counted down by libGDX's frame time, so the mod multiplies what `getDeltaTime()` and
`getRawDeltaTime()` answer by the factor in the environment variable STS2BOT_TIME_SCALE (set by the bot when it
launches the game; unset, nothing changes, so the game started by hand plays as usual).

The death screen unlocks the next character (the Silent after any run, the Defect after a Silent run, the Watcher once the
Defect is unlocked and an act 3 boss has been beaten) only when its button is clicked, through the "unlock" screen that
follows; CommunicationMod's "proceed" goes straight to the menu, so on a new profile (2026-11-24) four runs went by with the
Silent still locked. The mod grants the same unlock when the death screen is built, by the game's own rules and calls
(UnlockTracker.hardUnlockOverride, read from desktop-1.0.jar's DeathScreen.update).

`ensure(game_dir)` compiles it with the system's javac against the game's own jars (Java 8 bytecode, the game's
JRE) and puts `sts2botspeed.jar` in the game's mods folder, again only when the source has changed.
"""

from __future__ import annotations

import hashlib
import logging
import shutil
import subprocess
import tempfile
from pathlib import Path

log = logging.getLogger("sts2bot.supervisor")

MOD_ID = "sts2botspeed"
ENV = "STS2BOT_TIME_SCALE"

JAVA = """package sts2botspeed;

import com.badlogic.gdx.backends.lwjgl.LwjglGraphics;
import com.evacipated.cardcrawl.modthespire.lib.SpireInitializer;
import com.evacipated.cardcrawl.modthespire.lib.SpirePatch;
import com.evacipated.cardcrawl.modthespire.lib.SpirePostfixPatch;
import com.megacrit.cardcrawl.characters.AbstractPlayer;
import com.megacrit.cardcrawl.core.Settings;
import com.megacrit.cardcrawl.dungeons.AbstractDungeon;
import com.megacrit.cardcrawl.monsters.MonsterGroup;
import com.megacrit.cardcrawl.screens.DeathScreen;
import com.megacrit.cardcrawl.unlock.UnlockTracker;

@SpireInitializer
public class TimeScale {
    public static float factor = 1.0f;

    public static void initialize() {
        String raw = System.getenv("STS2BOT_TIME_SCALE");
        if (raw != null) {
            try {
                factor = Math.max(1.0f, Math.min(10.0f, Float.parseFloat(raw.trim())));
            } catch (NumberFormatException e) {
                factor = 1.0f;
            }
        }
        System.out.println("sts2botspeed: game clock x" + factor);
    }

    @SpirePatch(clz = LwjglGraphics.class, method = "getDeltaTime")
    public static class Delta {
        @SpirePostfixPatch
        public static float Postfix(float __result, LwjglGraphics __instance) {
            return __result * factor;
        }
    }

    @SpirePatch(clz = LwjglGraphics.class, method = "getRawDeltaTime")
    public static class RawDelta {
        @SpirePostfixPatch
        public static float Postfix(float __result, LwjglGraphics __instance) {
            return __result * factor;
        }
    }

    @SpirePatch(clz = DeathScreen.class, method = SpirePatch.CONSTRUCTOR, paramtypez = {MonsterGroup.class})
    public static class Unlocks {
        @SpirePostfixPatch
        public static void Postfix(DeathScreen __instance, MonsterGroup m) {
            if (Settings.isDemo || Settings.isDailyRun || Settings.isTrial || Settings.seedSet || AbstractDungeon.player == null) {
                return;
            }
            String key = null;
            if (UnlockTracker.isCharacterLocked("The Silent")) {
                key = "The Silent";
            } else if (UnlockTracker.isCharacterLocked("Defect") && AbstractDungeon.player.chosenClass == AbstractPlayer.PlayerClass.THE_SILENT) {
                key = "Defect";
            } else if (UnlockTracker.isCharacterLocked("Watcher") && !UnlockTracker.isCharacterLocked("Defect")
                    && (UnlockTracker.isAchievementUnlocked("RUBY") || UnlockTracker.isAchievementUnlocked("EMERALD")
                        || UnlockTracker.isAchievementUnlocked("SAPPHIRE"))) {
                key = "Watcher";
            }
            if (key != null) {
                UnlockTracker.hardUnlockOverride(key);
                UnlockTracker.lockedCharacters.remove(key);
                System.out.println("sts2botspeed: unlocked " + key);
            }
        }
    }
}
"""

MANIFEST = """{
  "modid": "sts2botspeed",
  "name": "sts2bot speed",
  "author_list": ["sts2bot"],
  "description": "For the bot: runs the game clock faster (STS2BOT_TIME_SCALE) and unlocks characters as the death screen would.",
  "version": "1.0.0",
  "sts_version": "12-18-2022",
  "mts_version": "3.18.0",
  "dependencies": []
}
"""


def _stamp() -> str:
    return hashlib.sha256((JAVA + MANIFEST).encode()).hexdigest()[:16]


def jar_path(game_dir: Path) -> Path:
    return Path(game_dir) / "mods" / f"{MOD_ID}.jar"


def ensure(game_dir: Path, mts_jar: Path | None) -> bool:
    """Build and install the mod if it is missing or out of date. True if it is in place."""
    target = jar_path(game_dir)
    stamp = target.with_suffix(".stamp")
    if target.is_file() and stamp.is_file() and stamp.read_text().strip() == _stamp():
        return True
    javac, jar = shutil.which("javac"), shutil.which("jar")
    game_jar = Path(game_dir) / "desktop-1.0.jar"
    if not (javac and jar and mts_jar and Path(mts_jar).is_file() and game_jar.is_file()):
        log.warning("cannot build the first game's speed mod (javac %s, jar %s, ModTheSpire %s, game %s)", javac, jar, mts_jar, game_jar.is_file())
        return False
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp, "src", "sts2botspeed")
        src.mkdir(parents=True)
        (src / "TimeScale.java").write_text(JAVA)
        out = Path(tmp, "out")
        out.mkdir()
        (out / "ModTheSpire.json").write_text(MANIFEST)
        built = subprocess.run([javac, "--release", "8", "-nowarn", "-cp", f"{game_jar}:{mts_jar}", "-d", str(out), str(src / "TimeScale.java")],
                               capture_output=True, text=True, timeout=300)
        if built.returncode != 0:
            log.warning("the first game's speed mod did not compile: %s", (built.stderr or built.stdout).strip()[-500:])
            return False
        target.parent.mkdir(parents=True, exist_ok=True)
        packed = subprocess.run([jar, "cf", str(target), "-C", str(out), "."], capture_output=True, text=True, timeout=120)
        if packed.returncode != 0:
            log.warning("the first game's speed mod could not be packed: %s", packed.stderr.strip()[-300:])
            return False
    stamp.write_text(_stamp() + "\n")
    log.info("built %s", target)
    return True
