//! Logging as the original version did it: named loggers with levels, a line per record to the terminal and to a file
//! that is rotated by size, and a way for tests to read what was logged.
//!
//! ```ignore
//! static LOG: Logger = Logger::new("sts2bot.learn");
//! log_warning!(LOG, "could not read {} ({}); starting afresh", path.display(), e);
//! let (out, records) = logging::capture(|| risky());          // tests: what this thread logged meanwhile
//! ```
//! Level and capture are per thread where it matters (`disable`, `capture`); handlers are the process's.

use std::cell::{Cell, RefCell};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Debug = 10,
    Info = 20,
    Warning = 30,
    Error = 40,
    Critical = 50,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warning => "WARNING",
            Level::Error => "ERROR",
            Level::Critical => "CRITICAL",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Record {
    pub logger: &'static str,
    pub level: Level,
    pub message: String,
}

impl Record {
    /// As `assertLogs` showed a record: "WARNING:sts2bot.learn:message".
    pub fn output(&self) -> String {
        format!("{}:{}:{}", self.level.name(), self.logger, self.message)
    }
}

pub struct Logger {
    pub name: &'static str,
}

/// A file that is renamed to `.1` (older ones to `.2` ...) when it passes `max_bytes`, keeping `backups` old files.
pub struct RotatingFile {
    path: PathBuf,
    max_bytes: u64,
    backups: usize,
    file: Option<File>,
    size: u64,
}

impl RotatingFile {
    pub fn new(path: impl Into<PathBuf>, max_bytes: u64, backups: usize) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(RotatingFile { path, max_bytes, backups, file: Some(file), size })
    }

    fn numbered(&self, n: usize) -> PathBuf {
        PathBuf::from(format!("{}.{}", self.path.display(), n))
    }

    fn rotate(&mut self) {
        self.file = None;
        if self.backups > 0 {
            for n in (1..self.backups).rev() {
                let _ = std::fs::rename(self.numbered(n), self.numbered(n + 1));
            }
            let _ = std::fs::rename(&self.path, self.numbered(1));
        } else {
            let _ = std::fs::remove_file(&self.path);
        }
        self.file = OpenOptions::new().create(true).append(true).open(&self.path).ok();
        self.size = 0;
    }

    fn write_line(&mut self, line: &str) {
        if self.max_bytes > 0 && self.size > 0 && self.size + line.len() as u64 + 1 >= self.max_bytes {
            self.rotate();
        }
        if let Some(f) = self.file.as_mut() {
            if writeln!(f, "{line}").is_ok() {
                self.size += line.len() as u64 + 1;
            }
        }
    }
}

pub enum Sink {
    Stderr,
    Stdout,
    File(RotatingFile),
}

/// Where records go and how a line looks. `format` knows `%(asctime)s`, `%(levelname)s`, `%(name)s` and `%(message)s`,
/// and `%(datetime)s`: the time without its milliseconds (the original's `%(asctime)s` under `datefmt="%Y-%m-%d %H:%M:%S"`).
pub struct Handler {
    pub sink: Sink,
    pub level: Level,
    pub format: String,
}

static HANDLERS: Mutex<Vec<Handler>> = Mutex::new(Vec::new());
static ROOT_LEVEL: AtomicU8 = AtomicU8::new(Level::Warning as u8);

thread_local! {
    static DISABLED: Cell<u8> = const { Cell::new(0) };
    static CAPTURE: RefCell<Vec<Vec<Record>>> = const { RefCell::new(Vec::new()) };
}

pub const DEFAULT_FORMAT: &str = "%(asctime)s %(levelname)s %(name)s: %(message)s";

/// `logging.basicConfig`: the level below which nothing is logged, and the handlers (replacing any there were).
pub fn configure(level: Level, handlers: Vec<Handler>) {
    ROOT_LEVEL.store(level as u8, Ordering::Relaxed);
    *HANDLERS.lock().unwrap_or_else(|e| e.into_inner()) = handlers;
}

/// `logging.disable(level)` for this thread: records at `level` and below are dropped (None: nothing is).
pub fn disable(level: Option<Level>) {
    DISABLED.with(|d| d.set(level.map(|l| l as u8).unwrap_or(0)));
}

/// Would a record at `level` go anywhere? (So that the message need not be built otherwise.)
#[inline]
pub fn enabled(level: Level) -> bool {
    let l = level as u8;
    if l <= DISABLED.with(|d| d.get()) {
        return false;
    }
    l >= ROOT_LEVEL.load(Ordering::Relaxed) || CAPTURE.with(|c| !c.borrow().is_empty())
}

/// Seconds since the epoch as local "YYYY-MM-DD HH:MM:SS,mmm" (the original's `%(asctime)s`).
pub fn asctime() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02},{:03}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec, now.subsec_millis())
}

pub fn emit(logger: &'static str, level: Level, message: String) {
    if level as u8 <= DISABLED.with(|d| d.get()) {
        return;
    }
    let captured = CAPTURE.with(|c| {
        let mut c = c.borrow_mut();
        match c.last_mut() {
            Some(top) => {
                top.push(Record { logger, level, message: message.clone() });
                true
            }
            None => false,
        }
    });
    if captured || (level as u8) < ROOT_LEVEL.load(Ordering::Relaxed) {
        return;
    }
    let mut handlers = HANDLERS.lock().unwrap_or_else(|e| e.into_inner());
    if handlers.is_empty() {
        if level >= Level::Warning {
            eprintln!("{message}"); // (the original's handler of last resort)
        }
        return;
    }
    let mut when: Option<String> = None;
    for h in handlers.iter_mut() {
        if level < h.level {
            continue;
        }
        let line = h
            .format
            .replace("%(asctime)s", when.get_or_insert_with(asctime))
            .replace("%(datetime)s", when.get_or_insert_with(asctime).split(',').next().unwrap_or(""))
            .replace("%(levelname)s", level.name())
            .replace("%(name)s", logger)
            .replace("%(message)s", &message);
        match &mut h.sink {
            Sink::Stderr => eprintln!("{line}"),
            Sink::Stdout => println!("{line}"),
            Sink::File(f) => f.write_line(&line),
        }
    }
}

/// Run `f` and return what it returned with every record this thread logged meanwhile (at any level; nothing reaches the handlers).
pub fn capture<T>(f: impl FnOnce() -> T) -> (T, Vec<Record>) {
    CAPTURE.with(|c| c.borrow_mut().push(Vec::new()));
    struct Pop;
    impl Drop for Pop {
        fn drop(&mut self) {
            CAPTURE.with(|c| {
                let mut c = c.borrow_mut();
                if c.len() > 1 {
                    let top = c.pop().unwrap();
                    c.last_mut().unwrap().extend(top);
                }
            });
        }
    }
    let guard = Pop;
    let out = f();
    let records = CAPTURE.with(|c| {
        let mut c = c.borrow_mut();
        let top = c.pop().unwrap_or_default();
        if let Some(outer) = c.last_mut() {
            outer.extend(top.iter().cloned());
        }
        top
    });
    std::mem::forget(guard);
    (out, records)
}

impl Logger {
    pub const fn new(name: &'static str) -> Self {
        Logger { name }
    }

    #[inline]
    pub fn enabled(&self, level: Level) -> bool {
        enabled(level)
    }

    pub fn log(&self, level: Level, message: String) {
        emit(self.name, level, message);
    }

    pub fn debug(&self, message: impl Into<String>) {
        emit(self.name, Level::Debug, message.into());
    }

    pub fn info(&self, message: impl Into<String>) {
        emit(self.name, Level::Info, message.into());
    }

    pub fn warning(&self, message: impl Into<String>) {
        emit(self.name, Level::Warning, message.into());
    }

    pub fn error(&self, message: impl Into<String>) {
        emit(self.name, Level::Error, message.into());
    }
}

#[macro_export]
macro_rules! log_at {
    ($level:expr, $logger:expr, $($arg:tt)+) => {
        if $crate::logging::enabled($level) {
            $crate::logging::emit($logger.name, $level, format!($($arg)+));
        }
    };
}

#[macro_export]
macro_rules! log_debug {
    ($logger:expr, $($arg:tt)+) => { $crate::log_at!($crate::logging::Level::Debug, $logger, $($arg)+) };
}

#[macro_export]
macro_rules! log_info {
    ($logger:expr, $($arg:tt)+) => { $crate::log_at!($crate::logging::Level::Info, $logger, $($arg)+) };
}

#[macro_export]
macro_rules! log_warning {
    ($logger:expr, $($arg:tt)+) => { $crate::log_at!($crate::logging::Level::Warning, $logger, $($arg)+) };
}

#[macro_export]
macro_rules! log_error {
    ($logger:expr, $($arg:tt)+) => { $crate::log_at!($crate::logging::Level::Error, $logger, $($arg)+) };
}
