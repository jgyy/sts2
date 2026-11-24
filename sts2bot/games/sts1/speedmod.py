"""A ModTheSpire mod of our own for the first game (Fast Mode alone left an enemy turn at 2.6 s). It multiplies libGDX's
frame time by STS2BOT_TIME_SCALE (set when the bot launches the game), does what the death screen's button would and
CommunicationMod's "proceed" skips (unlocks, ascension, act 4), and adds card numbers, keys, the stance and power text to
CommunicationMod's state. `ensure` builds it with javac against the game's jars when the source has changed."""

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
import com.megacrit.cardcrawl.cards.AbstractCard;
import com.megacrit.cardcrawl.characters.AbstractPlayer;
import com.megacrit.cardcrawl.core.AbstractCreature;
import com.megacrit.cardcrawl.core.CardCrawlGame;
import com.megacrit.cardcrawl.core.Settings;
import com.megacrit.cardcrawl.dungeons.AbstractDungeon;
import com.megacrit.cardcrawl.map.MapRoomNode;
import com.megacrit.cardcrawl.monsters.MonsterGroup;
import com.megacrit.cardcrawl.powers.AbstractPower;
import com.megacrit.cardcrawl.screens.DeathScreen;
import com.megacrit.cardcrawl.screens.GameOverScreen;
import com.megacrit.cardcrawl.screens.VictoryScreen;
import com.megacrit.cardcrawl.unlock.UnlockTracker;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.HashMap;

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
                        || UnlockTracker.isAchievementUnlocked("SAPPHIRE") || GameOverScreen.isVictory || everWon())) {
                key = "Watcher";
            }
            if (key != null) {
                UnlockTracker.hardUnlockOverride(key);
                UnlockTracker.lockedCharacters.remove(key);
                System.out.println("sts2botspeed: unlocked " + key);
            }
        }
    }

    // The game's Watcher rule asks for an act 3 achievement (RUBY, EMERALD, SAPPHIRE), and grants none while it is modded
    // (UnlockTracker.unlockAchievement returns at once under Settings.isModded: the first win, 2026-11-24 21:39, left
    // STSAchievements empty and the Watcher locked). A win now, or a CLASS_WIN flag from an earlier one, stands in for it.
    static boolean everWon() {
        return CardCrawlGame.playerPref.getBoolean("IRONCLAD_WIN", false) || CardCrawlGame.playerPref.getBoolean("THE_SILENT_WIN", false)
            || CardCrawlGame.playerPref.getBoolean("DEFECT_WIN", false);
    }

    // What the death screen's button does for a finished run besides the character unlock: the next ascension level
    // (updateAscensionProgress: a win, or a death in act 4, at the highest level unlocked) and, for a win, the
    // CLASS_WIN flag that opens act 4 once the Ironclad, the Silent and the Defect all have it
    // (CardCrawlGame.doorUnlockScreenCheck, without its door screen). Then the cards and relics the unlock bar has
    // earned (the game grants them at its next launch otherwise: UnlockTracker.retroactiveUnlock in create()).
    @SpirePatch(clz = DeathScreen.class, method = SpirePatch.CONSTRUCTOR, paramtypez = {MonsterGroup.class})
    public static class Progress {
        @SpirePostfixPatch
        public static void Postfix(DeathScreen __instance, MonsterGroup m) {
            if (AbstractDungeon.player == null || !Settings.isStandardRun()) {
                return;
            }
            ascend(__instance, "updateAscensionProgress");
            AbstractPlayer.PlayerClass c = AbstractDungeon.player.chosenClass;
            if (GameOverScreen.isVictory && (c == AbstractPlayer.PlayerClass.IRONCLAD || c == AbstractPlayer.PlayerClass.THE_SILENT
                    || c == AbstractPlayer.PlayerClass.DEFECT)) {
                String key = c.name() + "_WIN";
                if (!CardCrawlGame.playerPref.getBoolean(key, false)) {
                    CardCrawlGame.playerPref.putBoolean(key, true);
                    CardCrawlGame.playerPref.flush();
                    System.out.println("sts2botspeed: " + key);
                }
            }
            unlocks();
        }
    }

    // The same after the Heart (VictoryScreen's button: updateAscensionAndBetaArtProgress).
    @SpirePatch(clz = VictoryScreen.class, method = SpirePatch.CONSTRUCTOR, paramtypez = {MonsterGroup.class})
    public static class HeartProgress {
        @SpirePostfixPatch
        public static void Postfix(VictoryScreen __instance, MonsterGroup m) {
            if (AbstractDungeon.player != null && Settings.isStandardRun()) {
                ascend(__instance, "updateAscensionAndBetaArtProgress");
                unlocks();
            }
        }
    }

    static void ascend(Object screen, String method) {
        try {
            Method up = screen.getClass().getDeclaredMethod(method);
            up.setAccessible(true);
            up.invoke(screen);
            System.out.println("sts2botspeed: " + method + " done");
        } catch (Exception e) {
            System.out.println("sts2botspeed: " + method + " failed: " + e);
        }
    }

    static void unlocks() {
        try {
            UnlockTracker.retroactiveUnlock();
        } catch (Exception e) {
            System.out.println("sts2botspeed: retroactiveUnlock failed: " + e);
        }
    }

    // What CommunicationMod leaves out of its state, added to it. A card's damage and Block as the game has worked them
    // out (Strength, Weak, Dexterity, Frail, stances, Heavy Blade, Perfected Strike ...: the numbers on the card in hand).
    @SpirePatch(cls = "communicationmod.GameStateConverter", method = "convertCardToJson", requiredModId = "CommunicationMod", optional = true)
    public static class CardNumbers {
        @SpirePostfixPatch
        public static HashMap<String, Object> Postfix(HashMap<String, Object> __result, AbstractCard card) {
            __result.put("damage", card.damage);
            __result.put("base_damage", card.baseDamage);
            __result.put("block", card.block);
            __result.put("base_block", card.baseBlock);
            __result.put("magic", card.magicNumber);
            __result.put("base_magic", card.baseMagicNumber);
            return __result;
        }
    }

    // The keys for act 4 and whether act 4 is open at all.
    @SpirePatch(cls = "communicationmod.GameStateConverter", method = "getGameState", requiredModId = "CommunicationMod", optional = true)
    public static class Keys {
        @SpirePostfixPatch
        public static HashMap<String, Object> Postfix(HashMap<String, Object> __result) {
            HashMap<String, Object> keys = new HashMap<>();
            keys.put("ruby", Settings.hasRubyKey);
            keys.put("emerald", Settings.hasEmeraldKey);
            keys.put("sapphire", Settings.hasSapphireKey);
            __result.put("keys", keys);
            __result.put("final_act_available", Settings.isFinalActAvailable);
            return __result;
        }
    }

    // Which elite on the map carries the Emerald Key (the burning one).
    @SpirePatch(cls = "communicationmod.GameStateConverter", method = "convertMapRoomNodeToJson", requiredModId = "CommunicationMod", optional = true)
    public static class EmeraldNode {
        @SpirePostfixPatch
        public static HashMap<String, Object> Postfix(HashMap<String, Object> __result, MapRoomNode node) {
            if (node != null && node.hasEmeraldKey) {
                __result.put("emerald_key", true);
            }
            return __result;
        }
    }

    // The Watcher's stance.
    @SpirePatch(cls = "communicationmod.GameStateConverter", method = "convertPlayerToJson", requiredModId = "CommunicationMod", optional = true)
    public static class Stance {
        @SpirePostfixPatch
        public static HashMap<String, Object> Postfix(HashMap<String, Object> __result, AbstractPlayer p) {
            if (p != null && p.stance != null) {
                __result.put("stance", p.stance.ID);
            }
            return __result;
        }
    }

    // What each power says (CommunicationMod sends its id, name and amount only).
    @SpirePatch(cls = "communicationmod.GameStateConverter", method = "convertCreaturePowersToJson", requiredModId = "CommunicationMod", optional = true)
    public static class PowerText {
        @SpirePostfixPatch
        public static ArrayList<Object> Postfix(ArrayList<Object> __result, AbstractCreature creature) {
            if (creature == null || creature.powers == null) {
                return __result;
            }
            for (int i = 0; i < __result.size() && i < creature.powers.size(); i++) {
                Object o = __result.get(i);
                AbstractPower p = creature.powers.get(i);
                if (o instanceof HashMap && p != null && p.description != null) {
                    ((HashMap<String, Object>) o).put("description", p.description);
                }
            }
            return __result;
        }
    }
}
"""

MANIFEST = """{
  "modid": "sts2botspeed",
  "name": "sts2bot speed",
  "author_list": ["sts2bot"],
  "description": "For the bot: runs the game clock faster (STS2BOT_TIME_SCALE), does what the death screen's button would (unlocks, ascension, act 4) and adds card numbers, keys and stances to CommunicationMod's state.",
  "version": "1.0.0",
  "sts_version": "12-18-2022",
  "mts_version": "3.18.0",
  "dependencies": []
}
"""


def _stamp() -> str:
    return hashlib.sha256((JAVA + MANIFEST).encode()).hexdigest()[:16]


def ensure(game_dir: Path, mts_jar: Path | None) -> bool:
    """Build and install the mod if it is missing or out of date. True if it is in place."""
    target = Path(game_dir) / "mods" / f"{MOD_ID}.jar"
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
