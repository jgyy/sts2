//! What the supervisor needs to know about the first Slay the Spire: ModTheSpire with BaseMod and CommunicationMod (from the
//! Workshop), which starts the relay (relay.rs: `sts2bot relay`) for the bot (`write_comm_config`, from `sts2bot sts1-setup`).

#![allow(non_snake_case, non_upper_case_globals)]

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::screens::CHARACTERS;
use super::{relay, speedmod};
use crate::bot::BotError;
use crate::logging::Logger;
use crate::base::{self, JsonExt};
use crate::supervisor::{self, os_error, Host, KeyboardInterrupt, SpawnArgs, Supervisor};
use crate::{log_info, log_warning};

pub static log: Logger = Logger::new("sts2bot.supervisor");

pub const STEAM_APP_ID: i64 = 646570;
pub const STEAM_LAUNCH: &str = "steam -applaunch 646570";
pub const MTS_WORKSHOP_ID: i64 = 1605060445; // ModTheSpire on the Steam Workshop
pub const MODS: &str = "basemod,CommunicationMod";
pub const PROCESS_MARKS: [&str; 2] = ["ModTheSpire", "desktop-1.0.jar"]; // what the game's java process has on its command line

/// This program, which is the relay too (the original's `sys.executable`, there followed by the path of relay.rs).
pub fn executable() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "sts2bot".to_string())
}

/// What a command line that starts the relay has in it: this program's name and its `relay` subcommand
/// (the original's `RELAY.name`, "relay.rs": `RELAY` was the path of that file).
pub fn RELAY() -> String {
    let exe = executable();
    format!("{} relay", Path::new(&exe).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(exe.clone()))
}

pub fn steam_root() -> PathBuf {
    supervisor::home().join(".local/share/Steam/steamapps")
}

pub fn game_dir() -> PathBuf {
    match supervisor::environ_or("STS1_GAME_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => steam_root().join("common").join("SlayTheSpire"),
    }
}

pub fn mts_jar() -> Option<PathBuf> {
    let places = [
        supervisor::environ_or("STS1_MTS_JAR").map(PathBuf::from),
        Some(steam_root().join("workshop/content").join(STEAM_APP_ID.to_string()).join(MTS_WORKSHOP_ID.to_string()).join("ModTheSpire.jar")),
        Some(game_dir().join("ModTheSpire.jar")),
    ];
    places.into_iter().flatten().find(|p| p.is_file())
}

pub fn java() -> String {
    let bundled = game_dir().join("jre").join("bin").join("java");
    if supervisor::executable(&bundled) {
        bundled.to_string_lossy().into_owned()
    } else {
        "java".to_string()
    }
}

/// ModTheSpire straight into the game with the two mods, else Steam (which shows ModTheSpire's launcher).
pub fn default_launch() -> String {
    match mts_jar() {
        None => STEAM_LAUNCH.to_string(),
        Some(jar) => {
            let words = [java(), "-jar".to_string(), jar.to_string_lossy().into_owned(), "--skip-launcher".to_string(), "--skip-intro".to_string(), "--mods".to_string(), MODS.to_string()];
            words.iter().map(|a| supervisor::shlex_quote(a)).collect::<Vec<_>>().join(" ")
        }
    }
}

pub fn prefs_dir() -> PathBuf {
    match supervisor::environ_or("STS1_PREFS_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => game_dir().join("preferences"),
    }
}

// Each character's record in the first save slot (the game's CharStat files: the Ironclad's still has its old name).
pub const ACT4: [&str; 3] = ["IRONCLAD", "THE_SILENT", "DEFECT"]; // the characters whose wins open act 4 (keys.rs)
pub const STAT_FILE: [(&str, &str); 4] = [("IRONCLAD", "STSDataVagabond"), ("THE_SILENT", "STSDataTheSilent"), ("DEFECT", "STSDataDefect"), ("WATCHER", "STSDataWatcher")];
// Where the game notes that a character has been unlocked ("STSUnlocks": 0 or missing is locked; the Ironclad always is not).
pub const UNLOCK_KEY: [(&str, &str); 3] = [("THE_SILENT", "The Silent"), ("DEFECT", "Defect"), ("WATCHER", "Watcher")];

pub fn _json(path: &Path) -> Option<Map<String, Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Value>(&text).ok()? {
        Value::Object(data) => Some(data),
        _ => None,
    }
}

/// `int(x)` of what a save file holds (Err: the original's TypeError or ValueError).
fn int(x: &Value) -> Result<i64, ()> {
    match x {
        Value::Bool(b) => Ok(*b as i64),
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().filter(|f| f.is_finite()).map(|f| f.trunc() as i64)).ok_or(()),
        Value::String(s) => {
            let s = supervisor::strip(s);
            let (sign, digits) = match s.strip_prefix('-') {
                Some(rest) => (-1, rest),
                None => (1, s.strip_prefix('+').unwrap_or(s)),
            };
            let well_formed = !digits.is_empty() && digits.split('_').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
            if !well_formed {
                return Err(());
            }
            digits.replace('_', "").parse::<i64>().map(|n| sign * n).map_err(|_| ())
        }
        _ => Err(()),
    }
}

/// `int(x or default)`.
fn int_or(x: &Value, default: i64) -> Result<i64, ()> {
    if x.truthy() {
        int(x)
    } else {
        Ok(default)
    }
}

/// Each character's highest ascension (open once it has won), the wins and who is locked, from the game's save files.
/// (`folder`: None is `prefs_dir()`.)
pub fn read_prefs(folder: Option<&Path>) -> Value {
    let folder = folder.filter(|f| !f.as_os_str().is_empty()).map(|f| f.to_path_buf()).unwrap_or_else(prefs_dir);
    let mut max_ascension: Map<String, Value> = Map::new();
    let mut total: i64 = 0;
    let mut locked: Vec<Value> = Vec::new();
    let flags = _json(&folder.join("STSPlayer")).unwrap_or_default();
    for who in CHARACTERS {
        let stat_file = STAT_FILE.iter().find(|(c, _)| *c == who).expect("a stat file for each character").1;
        let Some(data) = _json(&folder.join(stat_file)) else {
            continue;
        };
        let null = Value::Null;
        let Ok(wins) = int_or(data.get("WIN_COUNT").unwrap_or(&null), 0) else {
            continue;
        };
        // After a win the game reads a missing ASCENSION_LEVEL as 1 (Prefs.getInteger(..., 1)).
        let Ok(level) = int_or(data.get("ASCENSION_LEVEL").unwrap_or(&null), if wins > 0 { 1 } else { 0 }) else {
            continue;
        };
        total += wins;
        max_ascension.insert(who.to_string(), json!(if wins > 0 || level > 1 { level.min(20) } else { 0 }));
        let won = flags.get(&format!("{who}_WIN")).map(base::str_of).unwrap_or_default();
        if ACT4.contains(&who) && won.to_lowercase() != "true" {
            // Act 4 opens on the three CLASS_WIN flags, which only a win the bot's mod saw sets: until then the character
            // counts as unclimbed, so the rotation plays it (2026-11-25: the Defect's one win predates the mod; DEFECT_WIN
            // was all act 4 lacked, and the Watcher alone, the only one on 0, played 35 runs in a row).
            max_ascension.insert(who.to_string(), json!(0));
        }
    }
    let unlocks = _json(&folder.join("STSUnlocks")).unwrap_or_default();
    for (who, key) in UNLOCK_KEY {
        match int_or(unlocks.get(key).unwrap_or(&Value::Null), 0) {
            Ok(n) if n > 0 => {}
            _ => locked.push(json!(who)),
        }
    }
    json!({"max_ascension": max_ascension, "wins": total, "locked": locked})
}

// What a game started by the bot is set to. A higher frame cap shortens every wait the game measures in frames.
pub const FRAME_CAP: i64 = 240;
// Sound at full while the game has the focus, none while it is behind another window (the game's own Mute in Bg).
pub const SOUND: [(&str, &str); 5] = [("Master Volume", "1.0"), ("Music Volume", "1.0"), ("Sound Volume", "1.0"), ("Ambience On", "true"), ("Mute in Bg", "true")];
// The first-run tips marked seen, and NO_FTUE (no tutorial): a tip's pop-up waits for a click CommunicationMod cannot give.
pub const TIPS_SEEN: [&str; 12] =
    ["NO_FTUE", "NEOW_SKIP", "NEOW_INTRO", "COMBAT_TIP", "SHUFFLE_TIP", "POWER_TIP", "M_POWER_TIP", "INTENT_TIP", "POTION_TIP", "RELIC_TIP", "CARD_REWARD_TIP", "ENERGY_USE_TIP"];
// A profile for the first save slot when there is none (the game makes one only on a name screen CommunicationMod never shows).
pub const PROFILE_NAME: &str = "jegoh"; // (the user's, 2026-11-26)
pub const OLD_PROFILE_NAMES: [&str; 1] = ["sts2bot"]; // names the bot gave the slot before: renamed at the next launch

/// (An OSError, and the file it was about.)
type Failed = (std::io::Error, PathBuf);

fn write_text(path: &Path, text: String) -> Result<(), Failed> {
    std::fs::write(path, text).map_err(|e| (e, path.to_path_buf()))
}

fn dumps(data: &Map<String, Value>) -> String {
    base::json_dumps_pretty(&Value::Object(data.clone()), 2, false)
}

/// Set the display, Fast Mode, the sound, and on a new profile the first-run tips as seen. Returns what was changed.
/// (the original's defaults: `fullscreen=True, folder=None, game=None`: the game's own folders.)
pub fn prepare_game(fullscreen: bool, folder: Option<&Path>, game: Option<&Path>) -> Vec<String> {
    let game = game.filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf()).unwrap_or_else(game_dir);
    let folder = folder.filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf()).unwrap_or_else(prefs_dir);
    let mut changed: Vec<String> = Vec::new();
    let display = game.join("info.displayconfig"); // width, height, frame cap, full screen, windowed full screen (borderless), vsync
    let lines: Vec<String> = match std::fs::read_to_string(&display) {
        Ok(text) => text.split(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)).filter(|w| !w.is_empty()).map(|w| w.to_string()).collect(),
        Err(_) => Vec::new(),
    };
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let (width, height) = if lines.len() >= 2 && digits(&lines[0]) && digits(&lines[1]) { (lines[0].clone(), lines[1].clone()) } else { ("1920".to_string(), "1080".to_string()) };
    let wanted = vec![width, height, FRAME_CAP.to_string(), "false".to_string(), if fullscreen { "true" } else { "false" }.to_string(), "false".to_string()];
    if lines != wanted {
        match std::fs::write(&display, wanted.join("\n") + "\n") {
            Ok(()) => changed.push(format!("display {}, {} fps", if fullscreen { "full screen" } else { "windowed" }, FRAME_CAP)),
            Err(e) => log_warning!(log, "could not write {}: {}", display.display(), os_error(&e, &display.to_string_lossy())),
        }
    }
    let text = |s: &str| Value::String(s.to_string());
    let settle = |changed: &mut Vec<String>| -> Result<(), Failed> {
        std::fs::create_dir_all(&folder).map_err(|e| (e, folder.clone()))?;
        let settings_file = folder.join("STSGameplaySettings");
        let mut settings = _json(&settings_file).unwrap_or_default();
        if settings.get("Fast Mode") != Some(&text("true")) || settings.get("Particle Effects") != Some(&text("false")) {
            // (Particle effects off: less to draw, so busy fights keep the frame cap.)
            let language = settings.get("LANGUAGE").cloned().unwrap_or(text("ENG"));
            settings.insert("Fast Mode".to_string(), text("true"));
            settings.insert("Particle Effects".to_string(), text("false"));
            settings.insert("LANGUAGE".to_string(), language);
            write_text(&settings_file, dumps(&settings))?;
            changed.push("Fast Mode on, particle effects off".to_string());
        }
        let sound_file = folder.join("STSSound");
        let mut sound = _json(&sound_file).unwrap_or_default();
        if SOUND.iter().any(|(k, v)| sound.get(*k) != Some(&text(v))) {
            for (k, v) in SOUND {
                sound.insert(k.to_string(), text(v));
            }
            write_text(&sound_file, dumps(&sound))?;
            changed.push("volume at full".to_string());
        }
        let tips_file = folder.join("STSTips");
        let mut tips = _json(&tips_file).unwrap_or_default();
        if TIPS_SEEN.iter().any(|k| tips.get(*k) != Some(&text("true"))) {
            for k in TIPS_SEEN {
                tips.insert(k.to_string(), text("true"));
            }
            write_text(&tips_file, dumps(&tips))?;
            changed.push("first-run tips off".to_string());
        }
        let slots_file = folder.join("STSSaveSlots");
        let slots = _json(&slots_file).unwrap_or_default();
        let slot = slots.get("DEFAULT_SLOT").map(base::str_of).unwrap_or("-1".to_string());
        let name = slots.get("PROFILE_NAME").cloned().unwrap_or(Value::Null);
        if (slot == "-1" || slot == "0") && !name.truthy() {
            let mut made = slots.clone();
            made.insert("DEFAULT_SLOT".to_string(), text("0"));
            made.insert("PROFILE_NAME".to_string(), text(PROFILE_NAME));
            made.insert("COMPLETION".to_string(), slots.get("COMPLETION").cloned().unwrap_or(text("0.0")));
            made.insert("PLAYTIME".to_string(), slots.get("PLAYTIME").cloned().unwrap_or(text("0")));
            write_text(&slots_file, dumps(&made))?;
            changed.push(format!("profile {} in the first save slot", base::str_repr(PROFILE_NAME)));
        } else if name.as_str().is_some_and(|n| OLD_PROFILE_NAMES.contains(&n)) {
            for f in [slots_file.clone(), folder.join("STSSaveSlots.backUp")] {
                // (the game falls back on its backup copy)
                if let Some(mut kept) = _json(&f) {
                    kept.insert("PROFILE_NAME".to_string(), text(PROFILE_NAME));
                    write_text(&f, dumps(&kept))?;
                }
            }
            changed.push(format!("profile renamed {} -> {}", base::repr_of(&name), base::str_repr(PROFILE_NAME)));
        }
        Ok(())
    };
    if let Err((e, path)) = settle(&mut changed) {
        log_warning!(log, "could not write the game's settings in {}: {}", folder.display(), os_error(&e, &path.to_string_lossy()));
    }
    changed
}

pub fn comm_config_path() -> PathBuf {
    let base = supervisor::environ_or("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| supervisor::home().join(".config"));
    base.join("ModTheSpire").join("CommunicationMod").join("config.properties")
}

/// Have CommunicationMod start the relay with the game (it reads this file at start-up).
/// (`path`: None is `comm_config_path()`. Err: the original's OSError.)
pub fn write_comm_config(port: i64, path: Option<&Path>) -> std::io::Result<PathBuf> {
    let path = path.filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf()).unwrap_or_else(comm_config_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let command = format!("{} relay --port {}", executable(), port);
    // (Java properties: a POSIX path needs no escaping. verbose=false, or every message both ways goes to game.log, 40 MB an hour.)
    std::fs::write(&path, format!("command={command}\nrunAtGameStart=true\nverbose=false\n"))?;
    Ok(path)
}

/// The program CommunicationMod starts with the game (None if it has no config yet).
pub fn comm_command(path: Option<&Path>) -> Option<String> {
    let path = path.filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf()).unwrap_or_else(comm_config_path);
    let text = std::fs::read_to_string(path).ok()?;
    supervisor::splitlines(&text).into_iter().find_map(|line| line.strip_prefix("command=")).map(|rest| supervisor::strip(rest).to_string())
}

/// Why CommunicationMod will not start the relay this bot listens to on `port` (None when it will): without it the game never answers.
pub fn comm_config_problem(port: i64, path: Option<&Path>) -> Option<String> {
    let path = path.filter(|p| !p.as_os_str().is_empty());
    let Some(command) = comm_command(path) else {
        return Some(format!("CommunicationMod has no command to start ({})", path.map(|p| p.to_path_buf()).unwrap_or_else(comm_config_path).display()));
    };
    if !command.contains(&RELAY()) {
        return Some(format!("CommunicationMod starts another program with the game: {command}"));
    }
    let relay_port = match crate::re!(r"--port[=\s]+(\d+)").captures(&command) {
        Some(found) => found[1].parse::<i64>().unwrap_or(-1),
        None => relay::DEFAULT_PORT as i64,
    };
    if relay_port != port {
        return Some(format!("CommunicationMod starts the relay on another port than {port}: {command}"));
    }
    None
}

pub fn _game_processes() -> Vec<String> {
    let argv: Vec<String> = ["ps", "-eo", "pid=,stat=,args="].iter().map(|a| a.to_string()).collect();
    let out = supervisor::run(&argv, None).unwrap_or_else(|e| panic!("{e}")).stdout;
    let relay = RELAY();
    let mut pids = Vec::new();
    for line in supervisor::splitlines(&out) {
        // (line.split(None, 2): the pid, the state, and the command line as it is)
        let line = line.trim_start_matches(base_space);
        let Some((pid, rest)) = line.split_once(base_space) else {
            continue;
        };
        let rest = rest.trim_start_matches(base_space);
        let Some((stat, args)) = rest.split_once(base_space) else {
            continue;
        };
        let args = args.trim_start_matches(base_space);
        if args.is_empty() {
            continue;
        }
        if !stat.starts_with('Z') && PROCESS_MARKS.iter().any(|m| args.contains(m)) && !args.contains(&relay) {
            pids.push(pid.to_string());
        }
    }
    pids
}

fn base_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

pub struct Sts1Host {
    // The game's output goes to game.log in log_dir (the one before to game.log.1); time_scale is its clock's speed (speedmod.rs).
    pub log_dir: Option<PathBuf>,
    pub fullscreen: bool,
    pub time_scale: f64,
}

impl Default for Sts1Host {
    fn default() -> Self {
        Sts1Host::new(None, true, 1.0)
    }
}

impl Sts1Host {
    pub const name: &'static str = "Slay the Spire";
    pub const heals_mod: bool = false; // CommunicationMod comes built from the Workshop

    /// `Sts1Host(log_dir=None, fullscreen=True, time_scale=1.0)`.
    pub fn new(log_dir: Option<PathBuf>, fullscreen: bool, time_scale: f64) -> Self {
        Sts1Host { log_dir, fullscreen, time_scale }
    }

    pub fn running(&self) -> bool {
        supervisor::_reap();
        !_game_processes().is_empty()
    }

    /// (A launch command that cannot be read, or a program that cannot be started, ends the program here as the original's
    /// uncaught ValueError or OSError did.)
    pub fn launch(&self, cmd: &str) {
        let changed = prepare_game(self.fullscreen, None, None);
        if !changed.is_empty() {
            log_info!(log, "game settings: {}", changed.join(", "));
        }
        let mut cmd = cmd.to_string();
        let mut env = None;
        let scale = if self.time_scale != 0.0 { self.time_scale } else { 1.0 };
        if cmd.contains(&format!("--mods {MODS}")) && speedmod::ensure(&game_dir(), mts_jar().as_deref()) {
            // Our own mod: the faster game clock (Fast Mode alone left a run at 12-15 minutes), the unlocks and more state.
            cmd = cmd.replace(&format!("--mods {MODS}"), &format!("--mods {MODS},{}", speedmod::MOD_ID));
            let mut with = supervisor::environ();
            with.insert(speedmod::ENV.to_string(), base::float_g(scale));
            env = Some(with);
            log_info!(log, "game clock: x{}", base::float_g(scale));
        }
        log_info!(log, "launching game: {}", cmd);
        let cwd = Some(game_dir()).filter(|d| d.is_dir());
        let mut out = None;
        if let Some(log_dir) = &self.log_dir {
            // (so that a crash leaves a trace)
            let path = log_dir.join("game.log");
            let opened = (|| {
                if path.exists() {
                    std::fs::rename(&path, path.with_file_name("game.log.1"))?;
                }
                std::fs::File::create(&path)
            })();
            match opened {
                Ok(file) => out = Some(file),
                Err(e) => log_warning!(log, "could not open {}: {}", path.display(), os_error(&e, &path.to_string_lossy())),
            }
        }
        let argv = supervisor::shlex_split(&cmd).unwrap_or_else(|e| panic!("{e}"));
        // (the game holds its own copy of the file: ours is closed when the call is over)
        supervisor::_spawn(&argv, SpawnArgs { cwd, env, out }).unwrap_or_else(|e| panic!("{}", os_error(&e, argv.first().map(|a| a.as_str()).unwrap_or(""))));
    }

    /// (the original's default: `grace=10.0`.)
    pub fn kill(&self, grace: f64) -> Result<(), KeyboardInterrupt> {
        for sig in ["-TERM", "-KILL"] {
            for pid in _game_processes() {
                let _ = supervisor::run(&["kill".to_string(), sig.to_string(), pid], None).unwrap_or_else(|e| panic!("{e}"));
            }
            let deadline = supervisor::monotonic() + grace;
            while supervisor::monotonic() < deadline && self.running() {
                supervisor::sleep(0.5)?;
            }
            if !self.running() {
                return Ok(());
            }
        }
        Ok(())
    }

    pub fn mod_is_stale(&self) -> bool {
        false
    }

    pub fn rebuild_mod(&self) -> bool {
        false
    }

    pub fn mod_load_error(&self, _since: f64) -> Option<String> {
        None
    }

    pub fn missing_steam(&self, _since: f64) -> bool {
        false
    }

    pub fn no_api_hint(&self) -> String {
        "Are BaseMod and CommunicationMod enabled in ModTheSpire, and does CommunicationMod start the relay (run `sts2bot sts1-setup`)?".to_string()
    }

    /// Nothing to set: the first game has no bridge for animation speed or graphics (its own Fast Mode setting is the one).
    pub fn configure(&self, _sup: &mut Supervisor) {}
}

impl Host for Sts1Host {
    fn name(&self) -> &'static str {
        Sts1Host::name
    }

    fn heals_mod(&self) -> bool {
        Sts1Host::heals_mod
    }

    fn running(&self) -> bool {
        Sts1Host::running(self)
    }

    fn launch(&self, cmd: &str) -> Result<(), BotError> {
        Sts1Host::launch(self, cmd);
        Ok(())
    }

    fn kill(&self) -> Result<(), BotError> {
        Ok(Sts1Host::kill(self, 10.0)?)
    }

    fn mod_is_stale(&self) -> bool {
        Sts1Host::mod_is_stale(self)
    }

    fn rebuild_mod(&self) -> bool {
        Sts1Host::rebuild_mod(self)
    }

    fn mod_load_error(&self, since: f64) -> Option<String> {
        Sts1Host::mod_load_error(self, since)
    }

    fn missing_steam(&self, since: f64) -> bool {
        Sts1Host::missing_steam(self, since)
    }

    fn no_api_hint(&self) -> String {
        Sts1Host::no_api_hint(self)
    }

    fn configure(&self, sup: &mut Supervisor) -> Result<(), BotError> {
        Sts1Host::configure(self, sup);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Against the original: tests/golden/supervisor/host.json holds what games/sts1/host.rs made of made-up save folders,
    //! game folders, CommunicationMod configs and process lists, the machine faked on both sides.

    use super::*;
    use crate::supervisor::testing::{env, logged, norm, relay, words};
    use crate::supervisor::Completed;
    use std::cell::RefCell;
    use std::rc::Rc;

    const GOLDEN: &str = include_str!("../../../tests/golden/supervisor/host.json");

    fn golden(part: &str) -> Vec<Value> {
        let all: Value = serde_json::from_str(GOLDEN).unwrap();
        all[part].as_array().unwrap().clone()
    }

    fn text(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    /// Every file under `root` and what it holds, by its path from there.
    fn tree(root: &Path) -> Map<String, Value> {
        fn walk(dir: &Path, root: &Path, out: &mut Map<String, Value>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    out.insert(text(path.strip_prefix(root).unwrap()), json!(std::fs::read_to_string(&path).unwrap()));
                }
            }
        }
        let mut out = Map::new();
        walk(root, root, &mut out);
        out
    }

    /// The files (a name ending in "/": a folder) in `dir`.
    fn fill(dir: &Path, files: &Value) {
        for (name, body) in files.as_object().unwrap() {
            match name.strip_suffix('/') {
                Some(folder) => std::fs::create_dir(dir.join(folder)).unwrap(),
                None => std::fs::write(dir.join(name), body.as_str().unwrap()).unwrap(),
            }
        }
    }

    #[test]
    fn the_save_files_are_read_as_the_record_read_them() {
        let cases = golden("read_prefs");
        assert!(cases.len() >= 26);
        for case in cases {
            let d = tempfile::tempdir().unwrap();
            fill(d.path(), &case["files"]);
            let got = if case["env"] == true {
                let _p = supervisor::patch(|s| s.environ = Some(env(&[("STS1_PREFS_DIR", json!(text(d.path())))])));
                read_prefs(None)
            } else {
                read_prefs(Some(d.path()))
            };
            assert_eq!(got, case["want"], "{}", case["files"]);
        }
    }

    #[test]
    fn the_game_is_prepared_as_the_record_prepared_it() {
        let cases = golden("prepare_game");
        assert!(cases.len() >= 150);
        for case in cases {
            let d = tempfile::tempdir().unwrap();
            let game = d.path().join("game");
            let prefs = game.join("preferences");
            std::fs::create_dir(&game).unwrap();
            fill(&game, &case["game"]);
            if case["prefs"] == "file" {
                std::fs::write(&prefs, "in the way").unwrap();
            } else if !case["prefs"].is_null() {
                std::fs::create_dir(&prefs).unwrap();
                fill(&prefs, &case["prefs"]);
            }
            let fullscreen = case["fullscreen"].as_bool().unwrap();
            let ((changed, again), logs) = logged(|| (prepare_game(fullscreen, Some(&prefs), Some(&game)), prepare_game(fullscreen, Some(&prefs), Some(&game))));
            let got = json!({"game": case["game"], "prefs": case["prefs"], "fullscreen": fullscreen, "changed": changed, "again": again, "logs": logs, "after": tree(d.path())});
            assert_eq!(norm(got, d.path()), case, "{} {}", case["game"], case["prefs"]);
        }
    }

    #[test]
    fn communicationmods_config_is_read_and_written_as_recorded() {
        let cases = golden("comm_config");
        assert!(cases.len() >= 21);
        for case in cases {
            let d = tempfile::tempdir().unwrap();
            let path = d.path().join("config.properties");
            if case["text"].is_null() && case["command"].is_null() && case["problems"].is_null() {
                let _p = supervisor::patch(|s| s.environ = Some(env(&[])));
                assert_eq!(json!({"path": text(&comm_config_path())}), case);
                continue;
            }
            if case["env"] == true {
                let _p = supervisor::patch(|s| s.environ = Some(env(&[("XDG_CONFIG_HOME", json!(text(d.path())))])));
                let got = json!({"text": null, "env": true, "command": comm_command(None), "problems": [comm_config_problem(15527, None)], "path": text(&comm_config_path())});
                assert_eq!(norm(got, d.path()), case);
                continue;
            }
            if let Some(config) = case["text"].as_str() {
                std::fs::write(&path, config.replace("{RELAY}", &relay())).unwrap();
            }
            let problems: Vec<Option<String>> = [15527, 15599, 15528, 0].iter().map(|port| comm_config_problem(*port, Some(&path))).collect();
            let got = json!({"text": case["text"], "command": comm_command(Some(&path)), "problems": problems});
            assert_eq!(norm(got, d.path()), case);
        }
        for case in golden("write_comm_config") {
            let d = tempfile::tempdir().unwrap();
            let port = case["port"].as_i64().unwrap();
            let path = write_comm_config(port, Some(&d.path().join("a").join("CommunicationMod").join("config.properties"))).unwrap();
            let other = {
                let _p = supervisor::patch(|s| s.environ = Some(env(&[("XDG_CONFIG_HOME", json!(text(d.path())))])));
                write_comm_config(port, None).unwrap()
            };
            let got = json!({"port": port, "path": text(&path), "text": std::fs::read_to_string(&path).unwrap(), "other": text(&other), "other_text": std::fs::read_to_string(&other).unwrap(),
                             "command": comm_command(Some(&path)), "problem": comm_config_problem(15527, Some(&path))});
            assert_eq!(norm(got, d.path()), case);
        }
    }

    #[test]
    fn the_first_game_is_found_as_recorded() {
        use std::os::unix::fs::PermissionsExt;
        let cases = golden("where");
        assert!(cases.len() >= 11);
        for case in cases {
            let d = tempfile::tempdir().unwrap();
            let sc = &case["sc"];
            for f in sc["files"].as_array().into_iter().flatten() {
                let path = d.path().join(f.as_str().unwrap());
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, "x").unwrap();
                if sc["exec"] == true && f.as_str().unwrap().ends_with("java") {
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
                } else {
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
                }
            }
            let mut more: Vec<(&str, Value)> = Vec::new();
            for (k, key) in [("STS1_GAME_DIR", "game"), ("STS1_MTS_JAR", "jar"), ("STS1_PREFS_DIR", "prefs")] {
                if let Some(v) = sc[key].as_str() {
                    more.push((k, json!(if v.is_empty() { String::new() } else { text(&d.path().join(v)) })));
                }
            }
            let _p = supervisor::patch(|s| {
                let mut e = env(&more);
                e.insert("HOME".to_string(), text(&d.path().join("home")));
                s.environ = Some(e);
            });
            let got = json!({"sc": sc, "steam_root": text(&steam_root()), "game_dir": text(&game_dir()), "prefs_dir": text(&prefs_dir()), "mts_jar": mts_jar().map(|j| text(&j)), "java": java(),
                             "default_launch": default_launch()});
            assert_eq!(norm(got, d.path()), case);
        }
    }

    #[test]
    fn the_games_processes_are_found_as_recorded() {
        let cases = golden("game_processes");
        assert!(cases.len() >= 7);
        for case in cases {
            let stdout = case["stdout"].as_str().unwrap().replace("{RELAY}", &relay());
            let _p = supervisor::patch(|s| s.set_run(move |_, _| Ok(Completed { stdout: stdout.clone(), ..Default::default() })));
            assert_eq!(json!({"stdout": case["stdout"], "want": _game_processes(), "running": Sts1Host::default().running()}), case);
        }
    }

    #[test]
    fn the_first_game_is_launched_as_recorded() {
        use std::io::Write;
        let cases = golden("launch");
        assert!(cases.len() >= 8);
        for case in cases {
            let d = tempfile::tempdir().unwrap();
            let sc = &case["sc"];
            let (game, logs_dir) = (d.path().join("game"), d.path().join("logs"));
            if sc["no_game_dir"] != true {
                std::fs::create_dir(&game).unwrap();
            }
            std::fs::create_dir(&logs_dir).unwrap();
            if sc["old_log"] == true {
                std::fs::write(logs_dir.join("game.log"), "the launch before\n").unwrap();
            }
            if sc["old_old"] == true {
                std::fs::write(logs_dir.join("game.log.1"), "the one before that\n").unwrap();
            }
            let spawned: Rc<RefCell<Vec<Value>>> = Rc::default();
            let asked: Rc<RefCell<Vec<Value>>> = Rc::default();
            let (seen, game_log) = (Rc::clone(&spawned), logs_dir.join("game.log"));
            let _p = supervisor::patch(|s| {
                s.environ = Some(env(&[("KEPT", json!("yes")), ("STS1_GAME_DIR", json!(text(&game))), ("STS1_PREFS_DIR", json!(text(&game.join("preferences"))))]));
                s.set_spawn(move |argv, kw| {
                    // (the file the output goes to is the game.log just opened: what is written to it lands there)
                    let mut out = None;
                    if let Some(mut file) = kw.out.as_ref() {
                        file.write_all(b"from the game").unwrap();
                        assert_eq!(std::fs::read_to_string(&game_log).unwrap(), "from the game");
                        file.set_len(0).unwrap();
                        out = Some(text(&game_log));
                    }
                    let mut keys = vec!["cwd"];
                    keys.extend(kw.env.as_ref().map(|_| "env"));
                    keys.extend(out.as_ref().map(|_| "stderr"));
                    keys.extend(out.as_ref().map(|_| "stdout"));
                    seen.borrow_mut().push(json!({"argv": argv, "cwd": kw.cwd.as_ref().map(|c| text(c)), "scale": kw.env.as_ref().map(|e| e.get(speedmod::ENV)), "kept": kw.env.as_ref().map(|e| e.get("KEPT")),
                                                  "out": out, "err": out, "keys": keys}));
                    Ok(())
                });
            });
            let (seen, ensure) = (Rc::clone(&asked), sc["ensure"].as_bool().unwrap());
            let _e = speedmod::with_ensure_hook(move |game_dir, mts_jar| {
                seen.borrow_mut().push(json!([text(game_dir), mts_jar.map(text)]));
                ensure
            });
            let log_dir = match &sc["log_dir"] {
                Value::Bool(true) => Some(logs_dir.clone()),
                Value::Bool(false) => None,
                _ => Some(d.path().join("nowhere")),
            };
            let h = Sts1Host::new(log_dir, sc["windowed"] != true, sc["scale"].as_f64().unwrap());
            if sc["prepared"] == true {
                logged(|| prepare_game(true, None, None));
            }
            let (_, logs) = logged(|| h.launch(sc["cmd"].as_str().unwrap()));
            let after: Map<String, Value> = tree(d.path()).into_iter().filter(|(k, _)| k.starts_with("logs/")).collect();
            let got = json!({"sc": sc, "spawned": *spawned.borrow(), "asked": *asked.borrow(), "logs": logs, "after": after, "display": std::fs::read_to_string(game.join("info.displayconfig")).ok(),
                             "hint": h.no_api_hint(), "flags": [json!(Sts1Host::name), json!(Sts1Host::heals_mod), json!(h.mod_is_stale()), json!(h.rebuild_mod()), json!(h.mod_load_error(0.0)), json!(h.missing_steam(0.0))]});
            assert_eq!(norm(got, d.path()), case, "{sc}");
        }
    }

    #[test]
    fn the_first_game_is_killed_as_recorded() {
        /// The first game's processes: gone `dies` seconds after the signal `on` ("-TERM", "-KILL"; none: never).
        #[derive(Default)]
        struct Game {
            sc: Value,
            t: f64,
            trace: Vec<Value>,
            sent: Vec<(String, f64)>,
        }
        impl Game {
            fn alive(&self) -> bool {
                let dies = self.sc["dies"].as_f64().unwrap_or(0.0);
                !self.sent.iter().any(|(sig, at)| self.sc["on"] == sig.as_str() && self.t >= at + dies)
            }
        }
        let cases = golden("kill");
        assert!(cases.len() >= 9);
        for case in cases {
            let world = Rc::new(RefCell::new(Game { sc: case["sc"].clone(), ..Default::default() }));
            let _p = supervisor::patch(|s| {
                let w = Rc::clone(&world);
                s.set_run(move |argv, _| {
                    let mut w = w.borrow_mut();
                    w.trace.push(json!(["run", argv]));
                    let mut stdout = String::new();
                    if argv[0] == "ps" {
                        stdout = if w.alive() { words(&w.sc["pids"]).iter().map(|pid| format!(" {pid} Sl java -jar ModTheSpire.jar\n")).collect() } else { "1 Sl init\n".to_string() };
                    } else if !w.sent.iter().any(|(sig, _)| *sig == argv[1]) {
                        let at = w.t;
                        w.sent.push((argv[1].clone(), at));
                    }
                    Ok(Completed { stdout, ..Default::default() })
                });
                let w = Rc::clone(&world);
                s.set_sleep(move |dt| {
                    let mut w = w.borrow_mut();
                    w.trace.push(json!(["sleep", dt]));
                    w.t += dt;
                    Ok(())
                });
                let w = Rc::clone(&world);
                s.set_monotonic(move || w.borrow().t);
            });
            Sts1Host::default().kill(case["sc"]["grace"].as_f64().unwrap_or(10.0)).unwrap();
            assert_eq!(json!({"sc": case["sc"], "trace": world.borrow().trace}), case);
        }
    }
}
