//! Keeps the game and the bot alive around the clock: launches the game, waits for its bridge mod, plays, and restarts it
//! when it crashes, freezes or the bot gets stuck. What is particular to a game is its host (`Sts2Host` here, `games::sts1::host::Sts1Host`).
//!
//! Everything this module asks of the machine (programs run and started, the clocks, sleeping, the environment, a few paths)
//! and the functions the original tests replaced by name go through one set of seams, per thread: `patch(|s| ...)` puts
//! stand-ins in place until its guard is dropped (the original's `mock.patch.object(supervisor, ...)`).

#![allow(non_snake_case, non_upper_case_globals)]

use std::cell::RefCell;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;

use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::bot::{Bot, BotError, Progress, SharedGame};
use crate::client::{os_error_text, Game};
use crate::games::both::{self, Proc};
use crate::logging::Logger;
use crate::base::{self, splitlines, strip, JsonExt};
use crate::{log_debug, log_error, log_info, log_warning};

pub use crate::games::both::KeyboardInterrupt;

pub static log: Logger = Logger::new("sts2bot.supervisor");

pub const STEAM_APP_ID: i64 = 2868840;
pub const STEAM_LAUNCH: &str = "steam -applaunch 2868840";
pub const STEAM_SETTLE: f64 = 10.0; // after Steam's UI is up: logging in takes a moment longer
pub const PROCESS_NAME: &str = "SlayTheSpire2";
// SteamAppId lets the directly started game attach to Steam. (No STS2_DEV_SKIP: it kept what the Timeline unlocks locked.)
pub const GAME_ENV: [(&str, &str); 2] = [("SteamAppId", "2868840"), ("SteamGameId", "2868840")];

// ---------------------------------------------------------------------------------------------------------------------
// The seams.

/// What a finished program left (`subprocess.run(..., capture_output=True, text=True)`): its exit code (minus the signal
/// that ended it), stdout and stderr.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Completed {
    pub returncode: i64,
    pub stdout: String,
    pub stderr: String,
}

/// `_spawn`'s keyword arguments: where the program starts, its environment (None: ours) and the file its output goes
/// to (stdout and stderr both; None: nowhere).
#[derive(Debug, Default)]
pub struct SpawnArgs {
    pub cwd: Option<PathBuf>,
    pub env: Option<IndexMap<String, String>>,
    pub out: Option<File>,
}

pub type Seam<F> = Option<Rc<RefCell<F>>>;

macro_rules! seams {
    ($( $(#[$doc:meta])* $name:ident, $set:ident : ($($arg:ty),*) $(-> $ret:ty)? ;)*) => {
        /// The stand-ins in force on this thread (None: the real thing). See `patch`.
        #[derive(Clone, Default)]
        pub struct Seams {
            $( $(#[$doc])* pub $name: Seam<dyn FnMut($($arg),*) $(-> $ret)?>, )*
            /// `os.environ` (None: the process's own).
            pub environ: Option<IndexMap<String, String>>,
            pub GAME_LOG: Option<PathBuf>,
            pub MOD_STAMP: Option<PathBuf>,
            pub INSTALL_SCRIPT: Option<PathBuf>,
            pub DEFAULT_LAUNCH: Option<String>,
        }

        impl Seams {
            $( pub fn $set(&mut self, f: impl FnMut($($arg),*) $(-> $ret)? + 'static) {
                self.$name = Some(Rc::new(RefCell::new(f)));
            } )*
        }
    };
}

seams! {
    /// `subprocess.run(argv, capture_output=True, text=True, timeout=...)`. Err: what the original's exception said
    /// (the program could not be started, or was still going after the timeout).
    run, set_run: (&[String], Option<f64>) -> Result<Completed, String>;
    /// `subprocess.Popen(argv, start_new_session=True, ...)`.
    popen, set_popen: (&[String], &SpawnArgs) -> std::io::Result<Box<dyn Proc>>;
    /// `shutil.which(name)`.
    which, set_which: (&str) -> Option<String>;
    /// `time.sleep(seconds)` (Err: Ctrl+C came meanwhile).
    sleep, set_sleep: (f64) -> Result<(), KeyboardInterrupt>;
    /// `time.monotonic()`.
    monotonic, set_monotonic: () -> f64;
    /// `time.time()`.
    time, set_time: () -> f64;
    _process_running, set_process_running: (&str) -> bool;
    game_running, set_game_running: () -> bool;
    game_fingerprint, set_game_fingerprint: () -> Option<String>;
    mod_is_stale, set_mod_is_stale: () -> bool;
    mod_load_error, set_mod_load_error: (f64) -> Option<String>;
    rebuild_mod, set_rebuild_mod: () -> bool;
    launch_game, set_launch_game: (&str);
    kill_game, set_kill_game: (f64);
    _spawn, set_spawn: (&[String], SpawnArgs) -> std::io::Result<()>;
    /// `self.bot.run(max_runs)` in `Supervisor.run` (a bot that does as scripted: the original tests' mock bot).
    bot_run, set_bot_run: (&mut Bot, Option<i64>) -> Result<(), BotError>;
}

thread_local! {
    static SEAMS: RefCell<Seams> = RefCell::new(Seams::default());
    // games we started; must be reaped when they die
    static CHILDREN: RefCell<Vec<Box<dyn Proc>>> = const { RefCell::new(Vec::new()) };
}

/// Puts the earlier seams back when dropped.
#[must_use = "the stand-ins end when this is dropped"]
pub struct Patched(Option<Seams>);

impl Drop for Patched {
    fn drop(&mut self) {
        if let Some(before) = self.0.take() {
            SEAMS.with(|s| *s.borrow_mut() = before);
        }
    }
}

/// Stand-ins on this thread until the guard is dropped: `let _p = supervisor::patch(|s| { s.set_sleep(|_| Ok(())); s.GAME_LOG = Some(path); });`
pub fn patch(change: impl FnOnce(&mut Seams)) -> Patched {
    let before = SEAMS.with(|s| s.borrow().clone());
    SEAMS.with(|s| change(&mut s.borrow_mut()));
    Patched(Some(before))
}

macro_rules! seam {
    ($name:ident) => {
        SEAMS.with(|s| s.borrow().$name.clone())
    };
}

/// Returns what the stand-in for `name` returns, when one is in place (`=> value`: returns that after calling it).
macro_rules! stand_in {
    ($name:ident($($arg:expr),*)) => {
        if let Some(f) = seam!($name) { return (f.borrow_mut())($($arg),*); }
    };
    ($name:ident($($arg:expr),*) => $ret:expr) => {
        if let Some(f) = seam!($name) { (f.borrow_mut())($($arg),*); return $ret; }
    };
}

/// `os.environ.get(name)`.
pub fn environ_get(name: &str) -> Option<String> {
    match SEAMS.with(|s| s.borrow().environ.as_ref().map(|e| e.get(name).cloned())) {
        Some(found) => found,
        None => std::env::var_os(name).map(|v| v.to_string_lossy().into_owned()),
    }
}

/// `{**os.environ}`.
pub fn environ() -> IndexMap<String, String> {
    match SEAMS.with(|s| s.borrow().environ.clone()) {
        Some(env) => env,
        None => std::env::vars_os().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect(),
    }
}

/// `os.environ.get(name) or None`: set and not empty.
pub fn environ_or(name: &str) -> Option<String> {
    environ_get(name).filter(|v| !v.is_empty())
}

/// `Path.home()`.
pub fn home() -> PathBuf {
    if let Some(home) = environ_get("HOME") {
        return PathBuf::from(home);
    }
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if !pw.is_null() && !(*pw).pw_dir.is_null() {
            return PathBuf::from(std::ffi::CStr::from_ptr((*pw).pw_dir).to_string_lossy().into_owned());
        }
    }
    PathBuf::from("/")
}

/// `shutil.which(name)`.
pub fn which(name: &str) -> Option<String> {
    stand_in!(which(name));
    use std::os::unix::fs::PermissionsExt;
    let path = environ_get("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|p| p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)).map(|p| p.to_string_lossy().into_owned())
}

/// `time.sleep(seconds)`; Err when we are told to stop meanwhile (the original's KeyboardInterrupt came out of the sleep).
pub fn sleep(seconds: f64) -> Result<(), KeyboardInterrupt> {
    stand_in!(sleep(seconds));
    both::sleep(seconds)
}

/// `time.monotonic()`.
pub fn monotonic() -> f64 {
    stand_in!(monotonic());
    both::monotonic()
}

/// `time.time()`.
pub fn time() -> f64 {
    stand_in!(time());
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// `str(e)` of the OSError the original raised over `name`: "[Errno 2] No such file or directory: 'steam'".
pub fn os_error(e: &std::io::Error, name: &str) -> String {
    format!("{}: {}", os_error_text(e), base::str_repr(name))
}

/// `subprocess.run(argv, capture_output=True, text=True, timeout=timeout)`, through the seam.
pub fn run(argv: &[String], timeout: Option<f64>) -> Result<Completed, String> {
    stand_in!(run(argv, timeout));
    use std::io::Read;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    let mut child = Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| os_error(&e, &argv[0]))?;
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
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if timeout.is_none_or(|t| started.elapsed().as_secs_f64() < t) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let words: Vec<String> = argv.iter().map(|a| base::str_repr(a)).collect();
                return Err(format!("Command '[{}]' timed out after {} seconds", words.join(", "), base::float_g(timeout.unwrap_or(0.0))));
            }
            Err(e) => return Err(os_error_text(&e)),
        }
    };
    let returncode = status.code().map(|c| c as i64).unwrap_or_else(|| -(status.signal().unwrap_or(0) as i64));
    Ok(Completed { returncode, stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() })
}

/// (Where the original's `subprocess.run` raised and nothing caught it.)
fn ran(argv: &[&str]) -> Completed {
    let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
    run(&argv, None).unwrap_or_else(|e| panic!("{e}"))
}

/// `subprocess.Popen(argv, start_new_session=True, stdin=DEVNULL, stdout=..., stderr=..., cwd=..., env=...)`, through the seam.
pub fn popen(argv: &[String], kw: &SpawnArgs) -> std::io::Result<Box<dyn Proc>> {
    stand_in!(popen(argv, kw));
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).stdin(Stdio::null());
    match &kw.out {
        Some(out) => {
            cmd.stdout(Stdio::from(out.try_clone()?)).stderr(Stdio::from(out.try_clone()?));
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    if let Some(cwd) = &kw.cwd {
        cmd.current_dir(cwd);
    }
    if let Some(env) = &kw.env {
        cmd.env_clear().envs(env);
    }
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    Ok(Box::new(both::Popen { child: cmd.spawn()?, returncode: None }))
}

/// `shlex.quote(s)`.
pub fn shlex_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// `shlex.split(s)` (Err: what its ValueError said).
pub fn shlex_split(s: &str) -> Result<Vec<String>, String> {
    // (shlex in posix mode with whitespace_split and no comments: a quote, even an empty one, makes a word)
    let (mut out, mut chars, mut word, mut quoted) = (Vec::new(), s.chars(), String::new(), false);
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\r' | '\n' => {
                if quoted || !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                    quoted = false;
                }
            }
            '\\' => word.push(chars.next().ok_or("No escaped character")?),
            '\'' | '"' => {
                quoted = true;
                loop {
                    match chars.next().ok_or("No closing quotation")? {
                        q if q == c => break,
                        '\\' if c == '"' => {
                            let e = chars.next().ok_or("No escaped character")?;
                            if e != '\\' && e != '"' {
                                word.push('\\');
                            }
                            word.push(e);
                        }
                        other => word.push(other),
                    }
                }
            }
            _ => word.push(c),
        }
    }
    if quoted || !word.is_empty() {
        out.push(word);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------------

pub fn game_dir() -> PathBuf {
    match environ_or("STS2_GAME_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => home().join(".local/share/Steam/steamapps/common/Slay the Spire 2"),
    }
}

// The bridge mod is built against the game's code: rebuilt when a game update changed it, or the game's log says it failed to load.
pub const MOD_NAME: &str = "STS2_MCP";

/// (the original's: beside the package it was imported from; here: in the directory the program was built in.)
pub fn INSTALL_SCRIPT() -> PathBuf {
    SEAMS.with(|s| s.borrow().INSTALL_SCRIPT.clone()).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts").join("install_mod.sh"))
}

pub fn MOD_STAMP() -> PathBuf {
    SEAMS.with(|s| s.borrow().MOD_STAMP.clone()).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".build").join("mod_built_for"))
}

/// (As the environment was when it was first asked for: the original's was set at import.)
pub fn GAME_LOG() -> PathBuf {
    static REAL: LazyLock<PathBuf> = LazyLock::new(|| {
        let base = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| {
            let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| home());
            home.join(".local/share")
        });
        base.join("SlayTheSpire2").join("logs").join("godot.log")
    });
    SEAMS.with(|s| s.borrow().GAME_LOG.clone()).unwrap_or_else(|| REAL.clone())
}

/// Changes whenever a game update replaces the code the mod is built against.
pub fn game_fingerprint() -> Option<String> {
    stand_in!(game_fingerprint());
    use std::os::unix::fs::MetadataExt;
    let mut names: Vec<String> = match std::fs::read_dir(game_dir()) {
        Ok(entries) => entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("data_sts2_")).collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    let mut parts = Vec::new();
    for name in names {
        if let Ok(st) = std::fs::metadata(game_dir().join(&name).join("sts2.dll")) {
            parts.push(format!("{}:{}:{}", name, st.size(), st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128));
        }
    }
    let joined = parts.join(";");
    (!joined.is_empty()).then_some(joined)
}

/// True if the game files changed since the mod was last built.
pub fn mod_is_stale() -> bool {
    stand_in!(mod_is_stale());
    let Some(now) = game_fingerprint() else { return false };
    if !INSTALL_SCRIPT().exists() {
        return false;
    }
    match std::fs::read_to_string(MOD_STAMP()) {
        Ok(text) => strip(&text) != now,
        Err(_) => true,
    }
}

/// The game's log ("" if there is none, or it was last written before `since`).
pub fn _game_log(since: f64) -> String {
    use std::os::unix::fs::MetadataExt;
    let path = GAME_LOG();
    let Ok(st) = std::fs::metadata(&path) else { return String::new() };
    if st.mtime() as f64 + 1e-9 * st.mtime_nsec() as f64 >= since {
        std::fs::read(&path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default()
    } else {
        String::new()
    }
}

/// Why the game refused to load the mod, from its log (None if it did not). `since` (a `time()`) skips an older log.
/// (the original's default: `since=0.0`.)
pub fn mod_load_error(since: f64) -> Option<String> {
    stand_in!(mod_load_error(since));
    let text = _game_log(since);
    let at = text.find(&format!("Exception thrown while loading mod {MOD_NAME}"))?;
    for line in splitlines(&text[at..]).into_iter().skip(1).take(11) {
        let line = strip(line);
        if !line.is_empty() && !line.starts_with("at ") {
            return Some(line.to_string());
        }
    }
    Some("the game log does not say why".to_string())
}

/// Build the mod against the installed game and install it. True on success.
pub fn rebuild_mod() -> bool {
    stand_in!(rebuild_mod());
    let script = INSTALL_SCRIPT();
    if !script.exists() {
        log_error!(log, "cannot rebuild the bridge mod: {} is missing", script.display());
        return false;
    }
    log_warning!(log, "rebuilding the bridge mod against the current game files");
    let fingerprint = game_fingerprint();
    let done = match run(&["bash".to_string(), script.to_string_lossy().into_owned()], Some(900.0)) {
        Ok(done) => done,
        Err(e) => {
            log_error!(log, "rebuilding the bridge mod failed: {}", e);
            return false;
        }
    };
    if done.returncode != 0 {
        let text = format!("{}\n{}", done.stdout, done.stderr);
        let output = splitlines(&text);
        let tail = crate::re!(r"\s*\[/.*?\]$");
        let mut errors: Vec<String> = output.iter().filter(|ln| ln.to_lowercase().contains("error")).map(|ln| tail.replace_all(strip(ln), "").into_owned()).collect();
        errors.sort();
        errors.dedup();
        let shown: Vec<String> = if errors.is_empty() { output[output.len().saturating_sub(15)..].iter().map(|s| s.to_string()).collect() } else { errors };
        log_error!(
            log,
            "the bridge mod does not build against this game version; scripts/mod_patches needs a fix for the game's changed code. Compiler said:\n  {}",
            shown.iter().take(15).cloned().collect::<Vec<_>>().join("\n  ")
        );
        return false;
    }
    if let Some(fingerprint) = fingerprint.filter(|f| !f.is_empty()) {
        let stamp = MOD_STAMP();
        let made = match stamp.parent() {
            Some(parent) => std::fs::create_dir_all(parent).map_err(|e| os_error(&e, &parent.to_string_lossy())),
            None => Ok(()),
        };
        let wrote = made.and_then(|_| std::fs::write(&stamp, format!("{fingerprint}\n")).map_err(|e| os_error(&e, &stamp.to_string_lossy())));
        if let Err(e) = wrote {
            log_debug!(log, "could not record the mod build: {}", e);
        }
    }
    log_info!(log, "bridge mod rebuilt and installed");
    true
}

/// `os.access(path, os.X_OK)`.
pub fn executable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    match std::ffi::CString::new(path.as_os_str().as_bytes()) {
        Ok(c) => unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 },
        Err(_) => false,
    }
}

/// Start the game binary directly when we can find it, else go through Steam.
pub fn default_launch() -> String {
    let exe = game_dir().join(PROCESS_NAME);
    if executable(&exe) {
        shlex_quote(&exe.to_string_lossy())
    } else {
        STEAM_LAUNCH.to_string()
    }
}

/// (the original's module constant, `default_launch()` at import: here as things were when it was first asked for.)
pub fn DEFAULT_LAUNCH() -> String {
    static REAL: LazyLock<String> = LazyLock::new(|| {
        let saved = SEAMS.with(|s| s.borrow_mut().environ.take());
        let found = default_launch();
        SEAMS.with(|s| s.borrow_mut().environ = saved);
        found
    });
    SEAMS.with(|s| s.borrow().DEFAULT_LAUNCH.clone()).unwrap_or_else(|| REAL.clone())
}

/// The games we started (`_children`): `supervisor::_children(|c| c.push(child))`.
pub fn _children<R>(f: impl FnOnce(&mut Vec<Box<dyn Proc>>) -> R) -> R {
    CHILDREN.with(|c| f(&mut c.borrow_mut()))
}

/// Collect the children that have exited (a crashed game stays a zombie until then, and would count as running).
pub fn _reap() {
    _children(|children| children.retain_mut(|proc| proc.poll().is_none()));
}

/// True if a live game process exists.
pub fn game_running() -> bool {
    stand_in!(game_running());
    _reap();
    _process_running(PROCESS_NAME)
}

pub fn _process_running(name: &str) -> bool {
    stand_in!(_process_running(name));
    let out = ran(&["ps", "-C", name, "-o", "stat="]).stdout;
    splitlines(&out).iter().any(|line| !strip(line).is_empty() && !strip(line).starts_with('Z'))
}

pub fn _failed(result: &Value) -> bool {
    result["status"] == "error" || result.has("error")
}

/// (the original's default: `grace=10.0`.)
pub fn kill_game(grace: f64) -> Result<(), KeyboardInterrupt> {
    stand_in!(kill_game(grace) => Ok(()));
    ran(&["pkill", "-x", PROCESS_NAME]);
    let deadline = monotonic() + grace;
    while monotonic() < deadline && game_running() {
        sleep(0.5)?;
    }
    if game_running() {
        ran(&["pkill", "-9", "-x", PROCESS_NAME]);
        sleep(1.0)?;
    }
    Ok(())
}

pub const FAST_MODES: [&str; 3] = ["instant", "fast", "normal"];

/// Ask the game for an animation speed through the mod, until it exits (Steam cloud sync restores an edited prefs.save).
pub fn set_fast_mode(client: &mut dyn Game, mode: &str) -> Result<bool, BotError> {
    let mut result = client.act("set_fast_mode", &json!({"mode": mode}))?;
    // Straight after a launch the game has not read its preferences yet; it is ready a few seconds later.
    for _ in 0..20 {
        if !result["error"].text().contains("not loaded") {
            break;
        }
        sleep(1.0)?;
        result = client.act("set_fast_mode", &json!({"mode": mode}))?;
    }
    if _failed(&result) {
        log_warning!(log, "could not set fast mode {} ({}); re-run scripts/install_mod.sh to update the mod", base::str_repr(mode), base::str_of(&result["error"]));
        return Ok(false);
    }
    log_info!(log, "fast mode: {}", mode);
    Ok(true)
}

crate::defaults! {
    /// `set_light_graphics`'s keyword arguments, with the original's defaults.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct SetLightGraphicsArgs { pub width: i64 = 960, pub height: i64 = 540, pub maximize: bool = true, pub fullscreen: bool }
}

/// Full screen (or a maximised or `width` x `height` window), no anti-aliasing, uncapped frame rate, for this session (cheaper frames, faster answers).
pub fn set_light_graphics(client: &mut dyn Game, args: SetLightGraphicsArgs) -> Result<bool, BotError> {
    let SetLightGraphicsArgs { width, height, maximize, fullscreen } = args;
    let result = client.act("set_light_graphics", &json!({"width": width, "height": height, "maximize": maximize, "fullscreen": fullscreen}))?;
    if _failed(&result) {
        log_warning!(log, "could not switch to light graphics ({}); re-run scripts/install_mod.sh", base::str_of(&result["error"]));
        return Ok(false);
    }
    if fullscreen && !result["fullscreen"].truthy() {
        log_warning!(log, "the installed mod cannot go full screen; re-run scripts/install_mod.sh");
    } else if maximize && !result["maximized"].truthy() {
        log_warning!(log, "the installed mod cannot maximise the window; re-run scripts/install_mod.sh");
    }
    let size = if fullscreen && result["fullscreen"].truthy() {
        "full screen".to_string()
    } else if maximize {
        "maximised window".to_string()
    } else {
        format!("{width}x{height}")
    };
    log_info!(log, "light graphics: {} (was {:.0} fps)", size, result["fps_before"].float());
    Ok(true)
}

/// Run the game's own clock (its timers between moves, rooms and rewards) `scale` times faster, for this session.
pub fn set_time_scale(client: &mut dyn Game, scale: f64) -> Result<bool, BotError> {
    let result = client.act("set_time_scale", &json!({"scale": scale}))?;
    if _failed(&result) {
        log_warning!(log, "could not set the time scale ({}); re-run scripts/install_mod.sh to update the mod", base::str_of(&result["error"]));
        return Ok(false);
    }
    log_info!(log, "time scale: x{}", base::float_g(scale));
    Ok(true)
}

/// Under systemd, Steam and the game get a scope of their own: restarting the bot's service leaves them running.
pub fn _detached(argv: &[String]) -> Vec<String> {
    if environ_or("INVOCATION_ID").is_some() && which("systemd-run").is_some_and(|w| !w.is_empty()) {
        let mut out: Vec<String> = ["systemd-run", "--user", "--scope", "--quiet", "--collect", "--"].iter().map(|w| w.to_string()).collect();
        out.extend(argv.iter().cloned());
        return out;
    }
    argv.to_vec()
}

/// Start a process of its own (Steam, the game) and remember it, to reap it when it dies.
/// (Its output goes nowhere unless the caller keeps it: the first game's goes to its game.log.)
pub fn _spawn(argv: &[String], kw: SpawnArgs) -> std::io::Result<()> {
    stand_in!(_spawn(argv, kw));
    let child = popen(&_detached(argv), &kw)?;
    _children(|children| children.push(child));
    Ok(())
}

/// Start the Steam client if it is not running and wait until it is ready (without it the game never loads the mod).
/// (the original's default: `timeout=180.0`.)
pub fn ensure_steam(timeout: f64) -> Result<bool, KeyboardInterrupt> {
    if _process_running("steam") {
        return Ok(true);
    }
    let Some(steam) = which("steam").filter(|s| !s.is_empty()) else {
        log_error!(log, "Steam is not running and no `steam` command was found to start it");
        return Ok(false);
    };
    log_info!(log, "Steam is not running; starting it");
    if let Err(e) = _spawn(&[steam.clone(), "-silent".to_string()], SpawnArgs::default()) {
        log_error!(log, "could not start Steam: {}", os_error(&e, &steam));
        return Ok(false);
    }
    let deadline = monotonic() + timeout;
    while monotonic() < deadline {
        if _process_running("steam") && _process_running("steamwebhelper") {
            // (its UI: up once Steam has updated itself)
            sleep(STEAM_SETTLE)?;
            log_info!(log, "Steam is up");
            return Ok(true);
        }
        sleep(2.0)?;
    }
    log_error!(log, "Steam did not come up within {:.0}s (is it logged in?)", timeout);
    Ok(false)
}

/// (A launch command that cannot be read, or a program that cannot be started, ends the program here as the original's
/// uncaught ValueError or OSError did.)
pub fn launch_game(cmd: &str) -> Result<(), KeyboardInterrupt> {
    stand_in!(launch_game(cmd) => Ok(()));
    let argv = shlex_split(cmd).unwrap_or_else(|e| panic!("{e}"));
    let exe = PathBuf::from(argv.first().unwrap_or_else(|| panic!("list index out of range")));
    if exe.file_name().is_some_and(|n| n == PROCESS_NAME) {
        ensure_steam(180.0)?; // `steam -applaunch ...` starts Steam by itself
    }
    log_info!(log, "launching game: {}", cmd);
    let mut env = environ();
    for (k, v) in GAME_ENV {
        env.insert(k.to_string(), v.to_string());
    }
    env.shift_remove("STS2_DEV_SKIP");
    let cwd = exe.parent().filter(|p| exe.is_absolute() && p.is_dir()).map(|p| p.to_path_buf());
    _spawn(&argv, SpawnArgs { cwd, env: Some(env), out: None }).unwrap_or_else(|e| panic!("{}", os_error(&e, &argv[0])));
    Ok(())
}

/// What the supervisor needs to know about a game (the original's hosts were two classes with the same methods).
pub trait Host {
    fn name(&self) -> &'static str;
    /// the bridge mod can be rebuilt by the supervisor
    fn heals_mod(&self) -> bool;
    fn running(&self) -> bool;
    fn launch(&self, cmd: &str) -> Result<(), BotError>;
    fn kill(&self) -> Result<(), BotError>;
    fn mod_is_stale(&self) -> bool;
    fn rebuild_mod(&self) -> bool;
    /// (the original's default: `since=0.0`.)
    fn mod_load_error(&self, since: f64) -> Option<String>;
    /// (the original's default: `since=0.0`.)
    fn missing_steam(&self, since: f64) -> bool;
    fn no_api_hint(&self) -> String;
    /// Session settings, sent each time the bot (re)connects.
    fn configure(&self, sup: &mut Supervisor) -> Result<(), BotError>;
}

/// What the supervisor needs to know about Slay the Spire 2 (calls look the module's functions up, so tests can replace them).
#[derive(Clone, Copy, Debug, Default)]
pub struct Sts2Host;

impl Host for Sts2Host {
    fn name(&self) -> &'static str {
        "Slay the Spire 2"
    }

    fn heals_mod(&self) -> bool {
        true // the bridge mod can be rebuilt by the supervisor
    }

    fn running(&self) -> bool {
        game_running()
    }

    fn launch(&self, cmd: &str) -> Result<(), BotError> {
        Ok(launch_game(cmd)?)
    }

    fn kill(&self) -> Result<(), BotError> {
        Ok(kill_game(10.0)?)
    }

    fn mod_is_stale(&self) -> bool {
        mod_is_stale()
    }

    fn rebuild_mod(&self) -> bool {
        rebuild_mod()
    }

    fn mod_load_error(&self, since: f64) -> Option<String> {
        mod_load_error(since)
    }

    /// True if the game came up without the Steam client it needs (its log says so).
    fn missing_steam(&self, since: f64) -> bool {
        !_process_running("steam") && _game_log(since).contains("Steam is probably not running")
    }

    fn no_api_hint(&self) -> String {
        "Is the STS2_MCP mod installed and are mods enabled in the game's settings?".to_string()
    }

    /// Session settings, sent each time the bot (re)connects.
    fn configure(&self, sup: &mut Supervisor) -> Result<(), BotError> {
        let client = Rc::clone(&sup.client);
        if let Some(mode) = sup.fast_mode.clone().filter(|m| !m.is_empty()) {
            set_fast_mode(&mut *client.borrow_mut(), &mode)?;
        }
        if sup.light_graphics {
            set_light_graphics(&mut *client.borrow_mut(), SetLightGraphicsArgs { maximize: !sup.small_window, fullscreen: sup.fullscreen && !sup.small_window, ..Default::default() })?;
        }
        if sup.time_scale != 0.0 && sup.time_scale != 1.0 && set_time_scale(&mut *client.borrow_mut(), sup.time_scale)? {
            sup.bot.time_scale = sup.time_scale; // the bot keeps it set: the game resets it
        }
        Ok(())
    }
}

crate::defaults! {
    /// The keyword arguments of `Supervisor(bot, client, ...)`, with the original's defaults.
    pub struct SupervisorArgs {
        /// (None: `Sts2Host`)
        pub host: Option<Rc<dyn Host>>,
        pub launch_cmd: String = DEFAULT_LAUNCH(),
        pub manage_game: bool = true,
        /// (None: left as the game has it)
        pub fast_mode: Option<String> = Some("instant".to_string()),
        pub light_graphics: bool = true,
        pub small_window: bool,
        pub fullscreen: bool = true,
        pub time_scale: f64 = 1.0,
        pub boot_timeout: f64 = 240.0,
        pub outage_grace: f64 = 45.0,
        pub heal_mod: bool = true,
    }
}

pub struct Supervisor {
    pub bot: Bot,
    pub client: SharedGame,
    pub host: Rc<dyn Host>,
    pub launch_cmd: String,
    pub manage_game: bool,
    pub fast_mode: Option<String>,
    pub light_graphics: bool,
    pub small_window: bool,
    pub fullscreen: bool,
    pub time_scale: f64,
    pub boot_timeout: f64,
    pub outage_grace: f64,
    pub heal_mod: bool,
    pub restarts: i64,
    pub _last_stuck_at: Option<Progress>,
    /// outages during which the ping still answered: (when the first of them began, when the last one ended)
    pub _deaf: Option<(f64, f64)>,
    pub _mod_retry_at: f64, // no rebuild attempts before this (after a failed build)
    pub _crash_at: Option<Progress>, // where the bot was when the game last crashed
    pub _crash_repeats: i64,
    pub _mod_backoff: f64,
}

impl Supervisor {
    /// `Supervisor(bot, client, *, host=None, launch_cmd=DEFAULT_LAUNCH, manage_game=True, fast_mode="instant", light_graphics=True,
    /// small_window=False, fullscreen=True, time_scale=1.0, boot_timeout=240.0, outage_grace=45.0, heal_mod=True)`.
    pub fn new(bot: Bot, client: SharedGame, args: SupervisorArgs) -> Supervisor {
        let SupervisorArgs { host, launch_cmd, manage_game, fast_mode, light_graphics, small_window, fullscreen, time_scale, boot_timeout, outage_grace, heal_mod } = args;
        let host: Rc<dyn Host> = host.unwrap_or_else(|| Rc::new(Sts2Host));
        let heal_mod = heal_mod && manage_game && host.heals_mod();
        Supervisor {
            bot,
            client,
            host,
            launch_cmd,
            manage_game,
            fast_mode,
            light_graphics,
            small_window,
            fullscreen,
            time_scale,
            boot_timeout,
            outage_grace,
            heal_mod,
            restarts: 0,
            _last_stuck_at: None,
            _deaf: None,
            _mod_retry_at: 0.0,
            _crash_at: None,
            _crash_repeats: 0,
            _mod_backoff: 300.0,
        }
    }

    fn ping(&self) -> bool {
        self.client.borrow_mut().ping()
    }

    /// Wait for the mod to answer. With `launched_at`, give up as soon as the game's log shows it refused the mod.
    pub fn _wait_for_api(&mut self, timeout: f64, launched_at: Option<f64>) -> Result<bool, KeyboardInterrupt> {
        let started = monotonic();
        let deadline = started + timeout;
        while monotonic() < deadline {
            if self.ping() {
                return Ok(true);
            }
            if let Some(at) = launched_at {
                if self.host.mod_load_error(at).is_some_and(|why| !why.is_empty()) {
                    return Ok(false);
                }
            }
            if self.manage_game && monotonic() - started > 10.0 {
                if !self.host.running() {
                    return Ok(false); // it died: no point waiting out the timeout
                }
                if self.host.missing_steam(launched_at.unwrap_or(0.0)) {
                    return Ok(false);
                }
            }
            sleep(2.0)?;
        }
        Ok(false)
    }

    /// Rebuild the mod, at most once per back-off period while it keeps failing.
    pub fn _rebuild_mod(&mut self) -> bool {
        if monotonic() < self._mod_retry_at {
            return false;
        }
        if self.host.rebuild_mod() {
            (self._mod_retry_at, self._mod_backoff) = (0.0, 300.0);
            return true;
        }
        self._mod_retry_at = monotonic() + self._mod_backoff;
        log_error!(log, "will try to rebuild the mod again in {:.0} minutes", self._mod_backoff / 60.0);
        self._mod_backoff = (self._mod_backoff * 2.0).min(3600.0);
        false
    }

    /// Block until the bridge mod answers, (re)launching the game as needed.
    pub fn ensure_game(&mut self) -> Result<(), BotError> {
        let mut backoff = 10.0;
        loop {
            if self.ping() {
                return Ok(());
            }
            if !self.manage_game {
                log_info!(log, "waiting for the game (start it yourself; --no-launch is set)");
                self._wait_for_api(60.0, None)?;
                continue;
            }
            let mut launched_at: Option<f64> = None;
            if !self.host.running() {
                if self.heal_mod && self.host.mod_is_stale() {
                    log_warning!(log, "the game files changed since the bridge mod was built (game update?)");
                    self._rebuild_mod();
                }
                launched_at = Some(time() - 1.0);
                self.host.launch(&self.launch_cmd)?;
            }
            if self._wait_for_api(self.boot_timeout, if self.heal_mod { launched_at } else { None })? {
                log_info!(log, "game is up");
                return Ok(());
            }
            let since = launched_at.unwrap_or(0.0);
            if let Some(why) = self.host.mod_load_error(since).filter(|why| !why.is_empty()) {
                log_error!(log, "the game did not load the bridge mod: {}", why);
                self.host.kill()?;
                if self.heal_mod && self._rebuild_mod() {
                    backoff = 10.0;
                    continue; // relaunch straight away with the fresh build
                }
                // Nothing more to try until the mod is fixed or the game updates again.
                let wait = if self.heal_mod {
                    let until = self._mod_retry_at - monotonic();
                    if until > backoff { until } else { backoff }
                } else {
                    backoff
                };
                let capped = if 3600.0 < wait { 3600.0 } else { wait };
                sleep(if capped > 1.0 { capped } else { 1.0 })?;
                backoff = (backoff * 2.0_f64).min(300.0);
                continue;
            }
            if !self.host.running() {
                log_error!(log, "the game exited before the bot API came up; starting it again");
            } else if self.host.missing_steam(since) {
                log_error!(log, "the game was started without Steam; restarting it with Steam running");
            } else {
                log_error!(log, "game did not expose the bot API within {:.0}s. {} Restarting the game.", self.boot_timeout, self.host.no_api_hint());
            }
            self.host.kill()?;
            sleep(backoff)?;
            backoff = (backoff * 2.0_f64).min(300.0);
        }
    }

    /// Break a crash loop: the game reloads the room it crashed in, so the bot avoids what it asked last (slowly, the second time),
    /// and the run is abandoned at the third crash in one place with nothing else to try, at the fifth in any case.
    pub fn _note_crash(&mut self) {
        let at = self.bot.progress();
        self._crash_repeats = if Some(&at) == self._crash_at.as_ref() { self._crash_repeats + 1 } else { 0 };
        self._crash_at = Some(at.clone());
        let another_way = self.bot.note_crash();
        if self._crash_repeats >= 4 || (self._crash_repeats >= 2 && !another_way) {
            log_warning!(log, "the game keeps crashing at {}; the run will be abandoned", at);
            self.bot.abandon_next = true;
            self._crash_repeats = 0;
        } else if self._crash_repeats >= 1 {
            log_warning!(log, "the game crashed again at {}; playing that room slowly", at);
            (self.bot.careful_until, self.bot.careful_at) = (self.bot.clock() + 120.0, Some(at));
        }
    }

    pub fn restart_game(&mut self, why: &str) -> Result<(), BotError> {
        self.restarts += 1;
        log_warning!(log, "restart #{}: {}", self.restarts, why);
        if self.manage_game {
            self.host.kill()?;
            sleep(3.0)?;
        }
        Ok(())
    }

    /// Play until `max_runs` runs have finished (forever if None). Err: we were told to stop (the original's KeyboardInterrupt
    /// came out of here).
    pub fn run(&mut self, max_runs: Option<i64>) -> Result<(), KeyboardInterrupt> {
        let interrupted = |e: BotError| match e {
            BotError::Interrupted(stop) => stop,
            // (only a game that cannot be reached while it is being set up or killed: nothing here asks it anything)
            other => panic!("{other}"),
        };
        while max_runs.is_none_or(|max_runs| self.bot.runs_finished < max_runs) {
            self.ensure_game().map_err(interrupted)?;
            let host = Rc::clone(&self.host);
            let played = match host.configure(self) {
                Ok(()) => match seam!(bot_run) {
                    Some(f) => (f.borrow_mut())(&mut self.bot, max_runs),
                    None => self.bot.run(max_runs),
                },
                Err(e) => Err(e),
            };
            match played {
                Ok(()) => {}
                Err(BotError::Interrupted(stop)) => return Err(stop),
                Err(BotError::GameUnavailable(e)) => {
                    log_warning!(log, "lost contact with the game: {}", e);
                    if self.manage_game && !self.host.running() {
                        self._note_crash();
                        self.restart_game("the game crashed").map_err(interrupted)?; // no point waiting for a dead process
                        continue;
                    }
                    if self._wait_for_api(self.outage_grace, None)? {
                        // The mod's ping is answered off the game's main thread: a game spinning in a loop still answers it and
                        // nothing else (2026-11-28: 25 minutes of this at one card of a Vantom fight). Outages that follow one
                        // another for longer than the grace are a hang: restart as after a crash, and avoid what was asked last.
                        let now = monotonic();
                        let since = match self._deaf {
                            Some((since, last)) if now - last < self.outage_grace + 30.0 => since,
                            _ => now,
                        };
                        self._deaf = Some((since, now));
                        if !self.manage_game || now - since < self.outage_grace {
                            continue;
                        }
                        self._deaf = None;
                        self._note_crash();
                        self.restart_game("the game answers pings but nothing else (hung)").map_err(interrupted)?;
                        continue;
                    }
                    self.restart_game("game stopped answering").map_err(interrupted)?;
                }
                Err(BotError::Stuck(e)) => {
                    let at = self.bot.progress();
                    if Some(&at) == self._last_stuck_at.as_ref() {
                        log_warning!(log, "stuck twice at {}; the run will be abandoned", at);
                        self.bot.abandon_next = true;
                    }
                    self._last_stuck_at = Some(at);
                    if !self.manage_game {
                        log_error!(log, "{} (not restarting: --no-launch is set)", e);
                        sleep(30.0)?;
                    }
                    self.restart_game(&e.to_string()).map_err(interrupted)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests;
