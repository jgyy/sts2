//! A ModTheSpire mod of our own for the first game (Fast Mode alone left an enemy turn at 2.6 s). It multiplies libGDX's
//! frame time by STS2BOT_TIME_SCALE (set when the bot launches the game), does what the death screen's button would and
//! CommunicationMod's "proceed" skips (unlocks, ascension, act 4), and adds card numbers, keys, the stance and power text to
//! CommunicationMod's state. `ensure` builds it with javac against the game's jars when the source has changed.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::logging::Logger;
use crate::{log_info, log_warning};

#[allow(non_upper_case_globals)]
pub static log: Logger = Logger::new("sts2bot.supervisor");

pub const MOD_ID: &str = "sts2botspeed";
pub const ENV: &str = "STS2BOT_TIME_SCALE";

pub const JAVA: &str = r#"package sts2botspeed;

import com.badlogic.gdx.backends.lwjgl.LwjglGraphics;
import com.evacipated.cardcrawl.modthespire.lib.SpireInitializer;
import com.evacipated.cardcrawl.modthespire.lib.SpirePatch;
import com.evacipated.cardcrawl.modthespire.lib.SpirePostfixPatch;
import com.evacipated.cardcrawl.modthespire.lib.SpirePrefixPatch;
import com.megacrit.cardcrawl.cards.AbstractCard;
import com.megacrit.cardcrawl.characters.AbstractPlayer;
import com.megacrit.cardcrawl.core.AbstractCreature;
import com.megacrit.cardcrawl.core.CardCrawlGame;
import com.megacrit.cardcrawl.core.Settings;
import com.megacrit.cardcrawl.dungeons.AbstractDungeon;
import com.megacrit.cardcrawl.map.MapRoomNode;
import com.megacrit.cardcrawl.monsters.AbstractMonster;
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
                factor = Math.max(1.0f, Math.min(16.0f, Float.parseFloat(raw.trim())));
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

    // Surrounded (the Spire Shield and Spear): a card or potion aimed at an enemy turns us to face it, as the game's own click does
    // (AbstractPlayer.playCard, PotionPopUp). CommunicationMod's play and potion commands queue them without that turn, so the
    // Shield, behind us from the start, would hit 50% harder all fight (its Smash: 51 for 34).
    @SpirePatch(cls = "communicationmod.CommandExecutor", method = "executePlayCommand", requiredModId = "CommunicationMod", optional = true)
    public static class FacePlay {
        @SpirePrefixPatch
        public static void Prefix(String[] tokens) {
            try {
                AbstractPlayer p = AbstractDungeon.player;
                if (p == null || tokens == null || tokens.length < 3 || !p.hasPower("Surrounded")) {
                    return;
                }
                int n = Integer.parseInt(tokens[1]);
                n = n == 0 ? 10 : n;
                if (n < 1 || n > p.hand.size()) {
                    return;
                }
                AbstractCard c = p.hand.group.get(n - 1);
                if (c.target == AbstractCard.CardTarget.ENEMY || c.target == AbstractCard.CardTarget.SELF_AND_ENEMY) {
                    face(Integer.parseInt(tokens[2]));
                }
            } catch (Exception e) {
                System.out.println("sts2botspeed: facing failed: " + e);
            }
        }
    }

    @SpirePatch(cls = "communicationmod.CommandExecutor", method = "executePotionCommand", requiredModId = "CommunicationMod", optional = true)
    public static class FacePotion {
        @SpirePrefixPatch
        public static void Prefix(String[] tokens) {
            try {
                if (tokens != null && tokens.length >= 4 && "use".equals(tokens[1])) {
                    face(Integer.parseInt(tokens[3]));
                }
            } catch (Exception e) {
                System.out.println("sts2botspeed: facing failed: " + e);
            }
        }
    }

    static void face(int target) {
        AbstractPlayer p = AbstractDungeon.player;
        if (p == null || !p.hasPower("Surrounded") || AbstractDungeon.getCurrRoom() == null || AbstractDungeon.getCurrRoom().monsters == null) {
            return;
        }
        ArrayList<AbstractMonster> ms = AbstractDungeon.getCurrRoom().monsters.monsters;
        if (target < 0 || target >= ms.size()) {
            return;
        }
        boolean turn = ms.get(target).drawX < p.drawX;
        if (turn != p.flipHorizontal) {
            p.flipHorizontal = turn;
            System.out.println("sts2botspeed: facing " + ms.get(target).id);
        }
    }

    // Who is behind us while Surrounded: the game works it out from where we face (AbstractMonster.applyBackAttack), and the
    // BackAttack mark it shows stays on whoever got it first.
    @SpirePatch(cls = "communicationmod.GameStateConverter", method = "convertMonsterToJson", requiredModId = "CommunicationMod", optional = true)
    public static class Behind {
        @SpirePostfixPatch
        public static HashMap<String, Object> Postfix(HashMap<String, Object> __result, AbstractMonster m) {
            AbstractPlayer p = AbstractDungeon.player;
            if (m != null && p != null && p.hasPower("Surrounded")) {
                boolean left = m.drawX < p.drawX;
                __result.put("left", left);
                __result.put("behind", p.flipHorizontal ? !left : left);
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
"#;

pub const MANIFEST: &str = r#"{
  "modid": "sts2botspeed",
  "name": "sts2bot speed",
  "author_list": ["sts2bot"],
  "description": "For the bot: runs the game clock faster (STS2BOT_TIME_SCALE), does what the death screen's button would (unlocks, ascension, act 4), turns the player to face a target when surrounded and adds card numbers, keys, stances and who is behind to CommunicationMod's state.",
  "version": "1.0.0",
  "sts_version": "12-18-2022",
  "mts_version": "3.18.0",
  "dependencies": []
}
"#;

pub fn _stamp() -> String {
    let digest = Sha256::digest(format!("{JAVA}{MANIFEST}").as_bytes());
    digest.iter().map(|b| format!("{:02x}", b)).collect::<String>()[..16].to_string()
}

/// `shutil.which(name)`: the program of that name on PATH.
pub fn which(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|p| p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false))
}

/// What a finished program left: its exit code (minus the signal that ended it), stdout and stderr.
pub struct Completed {
    pub returncode: i64,
    pub stdout: String,
    pub stderr: String,
}

/// `subprocess.run(argv, capture_output=True, text=True, timeout=timeout)`. None: it could not be started, or it was
/// still going after `timeout` seconds (and was killed).
pub fn run(argv: &[String], timeout: f64) -> Option<Completed> {
    use std::os::unix::process::ExitStatusExt;
    let mut child = Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let out = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + Duration::from_secs_f64(timeout);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let returncode = status.code().map(|c| c as i64).unwrap_or_else(|| -(status.signal().unwrap_or(0) as i64));
    Some(Completed { returncode, stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() })
}

/// The last `n` characters of the text with the blanks around it taken off (`text.strip()[-n:]`).
fn last(text: &str, n: usize) -> String {
    let text = text.trim();
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(n)).collect()
}

fn shown<T: std::fmt::Display>(x: &Option<T>) -> String {
    x.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "None".to_string())
}

/// A directory of our own under the system's for temporary files, removed when dropped (`tempfile.TemporaryDirectory`).
struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> std::io::Result<Self> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        for n in 0..1000u32 {
            let path = std::env::temp_dir().join(format!("tmp-sts2bot-{}-{:08x}", std::process::id(), nanos.wrapping_add(n)));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(TemporaryDirectory(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::other("no usable temporary directory name found"))
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

thread_local! {
    static ENSURE: std::cell::RefCell<Option<std::rc::Rc<dyn Fn(&Path, Option<&Path>) -> bool>>> = const { std::cell::RefCell::new(None) };
}

/// Puts the earlier `ensure` back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct EnsureGuard(Option<std::rc::Rc<dyn Fn(&Path, Option<&Path>) -> bool>>);

impl Drop for EnsureGuard {
    fn drop(&mut self) {
        ENSURE.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(speedmod, "ensure", f)`: on this thread, until the guard is dropped.
pub fn with_ensure_hook(f: impl Fn(&Path, Option<&Path>) -> bool + 'static) -> EnsureGuard {
    EnsureGuard(ENSURE.with(|h| h.borrow_mut().replace(std::rc::Rc::new(f))))
}

/// Build and install the mod if it is missing or out of date. True if it is in place.
pub fn ensure(game_dir: &Path, mts_jar: Option<&Path>) -> bool {
    if let Some(hook) = ENSURE.with(|h| h.borrow().clone()) {
        return hook(game_dir, mts_jar);
    }
    let target = game_dir.join("mods").join(format!("{MOD_ID}.jar"));
    let stamp = target.with_extension("stamp");
    if target.is_file() && stamp.is_file() && std::fs::read_to_string(&stamp).map(|s| s.trim() == _stamp()).unwrap_or(false) {
        return true;
    }
    let (javac, jar) = (which("javac"), which("jar"));
    let game_jar = game_dir.join("desktop-1.0.jar");
    let (javac, jar, mts_jar) = match (&javac, &jar, mts_jar) {
        (Some(javac), Some(jar), Some(mts)) if mts.is_file() && game_jar.is_file() => (javac, jar, mts),
        _ => {
            log_warning!(
                log,
                "cannot build the first game's speed mod (javac {}, jar {}, ModTheSpire {}, game {})",
                shown(&javac.as_ref().map(|p| p.display())),
                shown(&jar.as_ref().map(|p| p.display())),
                shown(&mts_jar.map(|p| p.display())),
                if game_jar.is_file() { "True" } else { "False" }
            );
            return false;
        }
    };
    let text = |p: &Path| p.to_string_lossy().into_owned();
    {
        let tmp = TemporaryDirectory::new().expect("a temporary directory");
        let src = tmp.0.join("src").join("sts2botspeed");
        std::fs::create_dir_all(&src).expect("the mod's source directory");
        std::fs::write(src.join("TimeScale.java"), JAVA).expect("the mod's source");
        let out = tmp.0.join("out");
        std::fs::create_dir(&out).expect("the mod's build directory");
        std::fs::write(out.join("ModTheSpire.json"), MANIFEST).expect("the mod's manifest");
        let built = run(
            &[text(javac), "--release".into(), "8".into(), "-nowarn".into(), "-cp".into(), format!("{}:{}", game_jar.display(), mts_jar.display()), "-d".into(), text(&out), text(&src.join("TimeScale.java"))],
            300.0,
        );
        match built {
            Some(built) if built.returncode == 0 => {}
            Some(built) => {
                log_warning!(log, "the first game's speed mod did not compile: {}", last(if built.stderr.is_empty() { &built.stdout } else { &built.stderr }, 500));
                return false;
            }
            None => {
                log_warning!(log, "the first game's speed mod did not compile: {}", "javac did not run to its end");
                return false;
            }
        }
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).expect("the game's mods directory");
        }
        let packed = run(&[text(jar), "cf".into(), text(&target), "-C".into(), text(&out), ".".into()], 120.0);
        match packed {
            Some(packed) if packed.returncode == 0 => {}
            Some(packed) => {
                log_warning!(log, "the first game's speed mod could not be packed: {}", last(&packed.stderr, 300));
                return false;
            }
            None => {
                log_warning!(log, "the first game's speed mod could not be packed: {}", "jar did not run to its end");
                return false;
            }
        }
    }
    std::fs::write(&stamp, format!("{}\n", _stamp())).expect("the mod's stamp");
    log_info!(log, "built {}", target.display());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The source and manifest are Python's, byte for byte: `speedmod._stamp()` and the whole sha256 there.
    #[test]
    fn the_stamp_is_pythons() {
        assert_eq!(_stamp(), "8082b7a9993beaf9");
        let digest = Sha256::digest(format!("{JAVA}{MANIFEST}").as_bytes());
        assert_eq!(digest.iter().map(|b| format!("{:02x}", b)).collect::<String>(), "8082b7a9993beaf9b42e4d0b3ccb52f719fe5ecfea973dfe756ddc873dcc36cb");
        assert_eq!((JAVA.len(), MANIFEST.len()), (14159, 476));
        assert!(JAVA.starts_with("package sts2botspeed;\n\nimport") && JAVA.ends_with("}\n}\n") && MANIFEST.starts_with("{\n  \"modid\"") && MANIFEST.ends_with("]\n}\n"));
    }

    #[test]
    fn an_up_to_date_mod_is_left_alone_and_a_missing_toolchain_is_said() {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("mods");
        std::fs::create_dir(&mods).unwrap();
        // nothing to build with (no game jar): a warning, and no mod
        let (built, said) = crate::logging::capture(|| ensure(dir.path(), None));
        assert!(!built);
        assert_eq!(said.len(), 1);
        assert!(said[0].output().starts_with("WARNING:sts2bot.supervisor:cannot build the first game's speed mod (javac "), "{}", said[0].output());
        assert!(said[0].message.ends_with(", ModTheSpire None, game False)"), "{}", said[0].message);
        // in place with this source's stamp: nothing to do
        std::fs::write(mods.join("sts2botspeed.jar"), b"jar").unwrap();
        std::fs::write(mods.join("sts2botspeed.stamp"), format!("{}\n", _stamp())).unwrap();
        let (built, said) = crate::logging::capture(|| ensure(dir.path(), None));
        assert!(built && said.is_empty());
        // another source's stamp: to be built again (and it cannot be, here)
        std::fs::write(mods.join("sts2botspeed.stamp"), "0123456789abcdef\n").unwrap();
        assert!(!crate::logging::capture(|| ensure(dir.path(), None)).0);
    }

    #[test]
    fn programs_are_run_with_their_output_kept() {
        let words = |w: &[&str]| w.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let done = run(&words(&["/bin/sh", "-c", "echo out; echo err >&2; exit 3"]), 10.0).unwrap();
        assert_eq!((done.returncode, done.stdout.as_str(), done.stderr.as_str()), (3, "out\n", "err\n"));
        assert!(run(&words(&["/bin/sleep", "5"]), 0.1).is_none());
        assert!(run(&words(&["/nonexistent/program"]), 1.0).is_none());
        assert_eq!(which("sh").map(|p| p.file_name().unwrap().to_owned()), Some("sh".into()));
        assert_eq!(which("no-such-program-anywhere"), None);
        assert_eq!(last("  abcdé \n", 3), "cdé");
    }
}
