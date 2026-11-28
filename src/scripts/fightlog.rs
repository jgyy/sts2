//! Read the bot's logs fight by fight (needs the bot to have run with -v).
//!
//! ```text
//! scripts fightlog bosses [ASCENSION]            win/loss per boss and character
//! scripts fightlog costs                         HP cost and deaths per ordinary encounter
//! scripts fightlog show 'YYYY-MM-DD HH:MM' [boss,elite,monster]   the first such fight from then on: a line per round, then the plays
//! scripts fightlog deaths ['YYYY-MM-DD HH:MM']   the fight each run ended in, from then on
//! scripts fightlog knobs ['YYYY-MM-DDTHH:MM']    how runs went under each value of each setting (runs.jsonl only)
//! scripts fightlog pace ['YYYY-MM-DD HH:MM']     rounds and enemy HP removed a round in ordinary fights, by act
//! scripts fightlog reads                         per card: the damage and Block the planner read against what the play did
//! scripts fightlog unplayed [boss,elite,monster] per card: played against left in hand at the end of a turn that could pay for it
//! scripts fightlog rooms SETTING ['YYYY-MM-DD HH:MM']   ordinary fights under each value of one setting, by character and act
//! scripts fightlog surprises ['YYYY-MM-DD']      turns that cost 3 HP or more beyond the plan, by enemy
//! scripts fightlog idle ['YYYY-MM-DD']           turns ended with energy and a payable card while 8 HP or more was about to go
//! scripts fightlog pit ['YYYY-MM-DD HH:MM']      the Insatiable's fights, and the Sandpit round by round (E: a Frantic Escape in hand)
//! scripts fightlog artifact ['YYYY-MM-DD HH:MM'] [boss,elite,monster]   Artifact charges on the enemies each round, and when gone
//! scripts fightlog crabs ['YYYY-MM-DD HH:MM']    Crusher and Rocket: which died first and when, then both crabs' HP by round
//! scripts fightlog bosspace ['YYYY-MM-DD HH:MM'] boss HP lost a round, won against lost, and under each value of each setting
//! scripts fightlog powers                        powers no module of the bot names, with their rules text
//! scripts fightlog events                        every event met: how often each option was taken, with the last scores
//! scripts fightlog ancients                      the Ancients' relics: rating, how the runs that held them went, the shift
//! scripts fightlog wiki                          save the game's text for every discovered card and relic to wiki.json
//! ```
//!
//! `load()` returns one record per fight joined to its run in runs.jsonl, for one-off questions.
//!
//! Where the Rust differs in shape from scripts/fightlog.rs:
//! - A fight is a [`Fight`]: the original's `"died" in f` is `f.died.is_some()`; a regex match of a `combat r` line is a [`Combat`]
//!   (`group(n)`); the generators `_turns` and `_endings` return lists.
//! - `powers` looked for `"NAME"` in the bot's the original sources beside the script; here in the Rust sources of the checkout the
//!   binary was built from (src/, without the model runs, the scripts and each file's tests).
//! - The log directory is `STS2BOT_STATE` or ~/.local/state/sts2bot/ as before; tests set it per thread with [`with_state`].

use std::cell::RefCell;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{Map, Value};

use super::modelfight::{exit_code, out, Datum};
use crate::base::{self, JsonExt};
use crate::re;

pub const DOC: &str = "Read the bot's logs fight by fight (needs the bot to have run with -v).

    scripts fightlog bosses [ASCENSION]            win/loss per boss and character
    scripts fightlog costs                         HP cost and deaths per ordinary encounter
    scripts fightlog show 'YYYY-MM-DD HH:MM' [boss,elite,monster]   the first such fight from then on: a line per round, then the plays
    scripts fightlog deaths ['YYYY-MM-DD HH:MM']   the fight each run ended in, from then on
    scripts fightlog knobs ['YYYY-MM-DDTHH:MM']    how runs went under each value of each setting (runs.jsonl only)
    scripts fightlog pace ['YYYY-MM-DD HH:MM']     rounds and enemy HP removed a round in ordinary fights, by act
    scripts fightlog reads                         per card: the damage and Block the planner read against what the play did
    scripts fightlog unplayed [boss,elite,monster] per card: played against left in hand at the end of a turn that could pay for it
    scripts fightlog rooms SETTING ['YYYY-MM-DD HH:MM']   ordinary fights under each value of one setting, by character and act
    scripts fightlog surprises ['YYYY-MM-DD']      turns that cost 3 HP or more beyond the plan, by enemy
    scripts fightlog idle ['YYYY-MM-DD']           turns ended with energy and a payable card while 8 HP or more was about to go
    scripts fightlog pit ['YYYY-MM-DD HH:MM']      the Insatiable's fights, and the Sandpit round by round (E: a Frantic Escape in hand)
    scripts fightlog artifact ['YYYY-MM-DD HH:MM'] [boss,elite,monster]   Artifact charges on the enemies each round, and when gone
    scripts fightlog crabs ['YYYY-MM-DD HH:MM']    Crusher and Rocket: which died first and when, then both crabs' HP by round
    scripts fightlog bosspace ['YYYY-MM-DD HH:MM'] boss HP lost a round, won against lost, and under each value of each setting
    scripts fightlog powers                        powers no module of the bot names, with their rules text
    scripts fightlog events                        every event met: how often each option was taken, with the last scores
    scripts fightlog ancients                      the Ancients' relics: rating, how the runs that held them went, the shift
    scripts fightlog wiki                          save the game's text for every discovered card and relic to wiki.json

`load()` returns one record per fight joined to its run in runs.jsonl, for one-off questions.
";

// ---------------------------------------------------------------------------------------------------------------------
// Where the logs are (shared with decklog, relearn and the audits)

thread_local! {
    static STATE_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Puts the earlier directory back when dropped.
pub struct StateGuard(Option<PathBuf>);

impl Drop for StateGuard {
    fn drop(&mut self) {
        STATE_DIR.with(|s| *s.borrow_mut() = self.0.take());
    }
}

/// On this thread, the bot's state directory is `dir` whatever `STS2BOT_STATE` and the home directory say (tests).
pub fn with_state(dir: impl Into<PathBuf>) -> StateGuard {
    StateGuard(STATE_DIR.with(|s| s.replace(Some(dir.into()))))
}

/// `Path.home()`.
pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// `os.path.expanduser(path)` ("~" and "~/..."; another user's "~name" is left as it is).
pub fn expanduser(path: &str) -> String {
    if path == "~" || path.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            let home = home.to_string_lossy();
            let home = if home.len() > 1 { home.trim_end_matches('/') } else { &home };
            return format!("{}{}", home, &path[1..]);
        }
    }
    path.to_string()
}

/// `Path.home() / ".local/state/sts2bot"`: where the scripts that take no `STS2BOT_STATE` look.
pub fn state_home() -> PathBuf {
    match STATE_DIR.with(|s| s.borrow().clone()) {
        Some(dir) => dir,
        None => home().join(".local/state/sts2bot"),
    }
}

/// the original's `LOGS`: `STS2BOT_STATE`, or ~/.local/state/sts2bot/.
#[allow(non_snake_case)]
pub fn LOGS() -> String {
    match STATE_DIR.with(|s| s.borrow().clone()) {
        Some(dir) => dir.to_string_lossy().into_owned(),
        None => expanduser(&std::env::var("STS2BOT_STATE").unwrap_or_else(|_| "~/.local/state/sts2bot/".to_string())),
    }
}

/// `os.path.join(a, b)`.
pub fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `os.path.join(LOGS, name)`.
pub fn log_path(name: &str) -> String {
    join(&LOGS(), name)
}

/// What the original's exception said, for a file that could not be read.
pub fn io_error(e: &std::io::Error, path: &str) -> String {
    let kind = match e.kind() {
        std::io::ErrorKind::NotFound => "FileNotFoundError",
        std::io::ErrorKind::PermissionDenied => "PermissionError",
        _ => "OSError",
    };
    format!("{kind}: {}: {}", crate::client::os_error_text(e), base::str_repr(path))
}

/// A text file as the original reads it with `errors="replace"`: bytes that are no UTF-8 replaced, every line end a "\n".
pub fn read_text(path: &str) -> Result<String, String> {
    let raw = std::fs::read(path).map_err(|e| io_error(&e, path))?;
    let text = String::from_utf8_lossy(&raw);
    Ok(if text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n") } else { text.into_owned() })
}

/// `for line in f: line.rstrip("\n")` over a text read with [`read_text`].
pub fn file_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

/// `[json.loads(line) for line in f]`.
pub fn read_jsonl(path: &str) -> Result<Vec<Value>, String> {
    let text = read_text(path)?;
    file_lines(&text).into_iter().map(|line| serde_json::from_str(line).map_err(|e| format!("json.decoder.JSONDecodeError: {e}"))).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// the original's strings, counters and dates

/// `s[:n]`, by code points.
pub fn upto(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// `s[n:]`, by code points.
pub fn from(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

/// `s[a:b]`, by code points.
pub fn slice(s: &str, a: usize, b: usize) -> &str {
    upto(from(s, a), b.saturating_sub(a))
}

/// `collections.Counter`, in the order its keys were first counted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Counter(pub IndexMap<String, i64>);

impl Counter {
    pub fn add(&mut self, key: &str) {
        *self.0.entry(key.to_string()).or_insert(0) += 1;
    }

    pub fn get(&self, key: &str) -> i64 {
        self.0.get(key).copied().unwrap_or(0)
    }

    /// `most_common(n)`: the highest counts first, equal counts in the order first counted.
    pub fn most_common(&self, n: Option<usize>) -> Vec<(String, i64)> {
        let mut items: Vec<(String, i64)> = self.0.iter().map(|(k, v)| (k.clone(), *v)).collect();
        items.sort_by(|a, b| b.1.cmp(&a.1));
        if let Some(n) = n {
            items.truncate(n);
        }
        items
    }
}

/// Days from 1970-01-01 to a date of the (proleptic Gregorian) calendar.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = base::floordiv(y, 400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = base::floordiv(z, 146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `datetime.datetime.strptime(text, "%Y-%m-%d %H:%M:%S")`, as seconds on a clock with no zone.
pub fn strptime(text: &str) -> Result<i64, String> {
    let bad = || format!("ValueError: time data {} does not match format '%Y-%m-%d %H:%M:%S'", base::str_repr(text));
    let m = re!(r"^(\d{4})-(\d{1,2})-(\d{1,2}) (\d{1,2}):(\d{1,2}):(\d{1,2})$").captures(text).ok_or_else(bad)?;
    let n = |i: usize| m[i].parse::<i64>().map_err(|_| bad());
    let (y, mo, d, h, mi, s) = (n(1)?, n(2)?, n(3)?, n(4)?, n(5)?, n(6)?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&mo) || d < 1 || d > days[(mo - 1) as usize] || h > 23 || mi > 59 || s > 61 || y < 1 {
        return Err(bad());
    }
    Ok(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + s)
}

/// `.strftime("%Y-%m-%d %H:%M:%S")` of such a time.
pub fn strftime(seconds: i64) -> String {
    let (days, rest) = (base::floordiv(seconds, 86400), base::modulo(seconds, 86400));
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, rest / 3600, rest % 3600 / 60, rest % 60)
}

/// A number of the settings (a JSON value) as a dict key: 1 and 1.0 are one key.
fn same_value(a: &Value, b: &Value) -> bool {
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x == y,
        (None, None) => a == b,
        _ => false,
    }
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(*b as i64 as f64),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// The order `sorted` puts two such keys in.
fn value_cmp(a: &Value, b: &Value) -> Ordering {
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => base::str_of(a).cmp(&base::str_of(b)),
    }
}

/// `f"{value:{width}}"` of a setting's value: a number to the right, a string to the left.
pub fn fmt_value(v: &Value, width: usize) -> String {
    match v {
        Value::String(s) => format!("{s:width$}"),
        Value::Bool(b) => format!("{:>width$}", *b as i64),
        other => format!("{:>width$}", base::str_of(other)),
    }
}

/// `table[key].append(item)` for a `defaultdict(list)` keyed by a setting's value.
fn push_by_value<T>(table: &mut Vec<(Value, Vec<T>)>, key: &Value, item: T) {
    match table.iter_mut().find(|(k, _)| same_value(k, key)) {
        Some((_, list)) => list.push(item),
        None => table.push((key.clone(), vec![item])),
    }
}

fn int(text: &str) -> i64 {
    text.parse().unwrap_or_else(|_| panic!("ValueError: invalid literal for int() with base 10: {}", base::str_repr(text)))
}

fn mean_i(items: impl Iterator<Item = i64>, n: usize) -> f64 {
    items.sum::<i64>() as f64 / n as f64
}

fn mean_f(items: impl Iterator<Item = f64>, n: usize) -> f64 {
    base::fsum(items) / n as f64
}

// ---------------------------------------------------------------------------------------------------------------------

macro_rules! ACT {
    () => {
        re!(r"^(\S+ \S+) INFO act (\d) floor (\d+)  (\w+)  hp (\d+)/(\d+)  gold (\d+)")
    };
}
macro_rules! OVER {
    () => {
        re!(r"^(\S+ \S+) INFO run \d+ over: (\w+) on act (\d) floor (\d+)")
    };
}
macro_rules! COMBAT {
    () => {
        re!(r"^(\S+ \S+) DEBUG combat r(\d+) hp (\d+)/(\d+) blk (\d+) en (\d+)(.*?) \| (.*?) \| hand (.*)$")
    };
}
macro_rules! PLAYED {
    () => {
        re!(r"DEBUG play_card (\{.*?\}) -> Playing '(.+?)'(?: targeting .*)?$")
    };
}
macro_rules! IN_HAND {
    () => {
        re!(r"(\d+):(.+?)\((\w+)(!?)((?: [db]\d+(?:x\d+)?)*)\)(?:, |$)")
    };
}
macro_rules! ENDING {
    () => {
        re!(r"^(\S+ \S+) DEBUG ending r(\d+) on (\d+) HP, expecting to lose (\d+)")
    };
}

/// A `combat r` line as COMBAT matched it: `group(1)` the time ... `group(9)` the hand.
#[derive(Clone, Debug, PartialEq)]
pub struct Combat(pub Vec<String>);

impl Combat {
    pub fn of(line: &str) -> Option<Combat> {
        COMBAT!().captures(line).map(|m| Combat((0..10).map(|i| m.get(i).map_or("", |g| g.as_str()).to_string()).collect()))
    }

    pub fn group(&self, n: usize) -> &str {
        &self.0[n]
    }
}

/// Every log line, oldest first.
pub fn lines() -> Result<impl Iterator<Item = String>, String> {
    let logs = LOGS();
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(if logs.is_empty() { "." } else { &logs }).map_err(|e| io_error(&e, &logs))? {
        let name = entry.map_err(|e| io_error(&e, &logs))?.file_name().to_string_lossy().into_owned();
        if name.starts_with("bot.log") {
            names.push(name);
        }
    }
    let key = |n: &String| -> i64 {
        if n.chars().last().is_some_and(|c| c.is_numeric()) {
            -int(n.rsplit_once('.').map_or("", |x| x.1))
        } else {
            1
        }
    };
    names.sort_by_key(key);
    Ok(names.into_iter().flat_map(move |name| {
        let path = join(&logs, &name);
        let text = read_text(&path).unwrap_or_else(|e| panic!("{e}"));
        file_lines(&text).into_iter().map(str::to_string).collect::<Vec<_>>()
    }))
}

/// The enemies of a combat line by name, e.g. CRUSHER+ROCKET.
pub fn _names(enemies: &str) -> String {
    let names: std::collections::BTreeSet<String> = enemies.split(" ; ").map(|e| re!(r"_\d+$").replace(e.split(' ').next().unwrap_or(""), "").into_owned()).collect();
    names.into_iter().collect::<Vec<_>>().join("+")
}

/// `(f.get("run") or {}).get("knobs") or {}`.
pub fn _knobs(f: &Fight) -> Map<String, Value> {
    match f.run.as_deref().map(|r| &r["knobs"]) {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    }
}

/// HP of everything on the other side (an enemy "on 999999999" is one already beaten).
pub fn _enemy_hp(enemies: &str) -> i64 {
    re!(r" (\d+)hp/").captures_iter(enemies).map(|m| int(&m[1])).filter(|x| *x < 100_000).sum()
}

/// Enemy HP removed in a fight: all of it, unless it was our death.
pub fn _dealt(f: &Fight) -> i64 {
    _enemy_hp(&f.turns[0].enemies) - if f.died == Some(true) { _enemy_hp(&f.turns[f.turns.len() - 1].enemies) } else { 0 }
}

/// read(enemies) at the first look of each round of a fight, in round order.
pub fn _by_round<T>(f: &Fight, read: impl Fn(&str) -> T) -> Vec<T> {
    let mut first: std::collections::BTreeMap<i64, T> = std::collections::BTreeMap::new();
    for t in &f.turns {
        first.entry(t.rnd).or_insert_with(|| read(&t.enemies));
    }
    first.into_values().collect()
}

/// A look at the fight: the original's `(round, hp, block, energy, powers, enemies, hand, time)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub rnd: i64,
    pub hp: i64,
    pub blk: i64,
    pub en: i64,
    pub powers: String,
    pub enemies: String,
    pub hand: String,
    pub t: String,
}

/// One fight of the log. `died` is None while the original's record had no "died" key (a fight still going when the log ends, or
/// when its run did); `run`, `char`, `asc`, `result` are those of the run it was in (None: the run is not over, or not on record).
#[derive(Clone, Debug, PartialEq)]
pub struct Fight {
    pub t: String,
    pub act: i64,
    pub floor: i64,
    pub kind: String,
    pub hp_in: i64,
    pub max_hp: i64,
    pub enemies: Option<String>,
    pub rounds: i64,
    pub turns: Vec<Turn>,
    pub hp_out: Option<i64>,
    pub died: Option<bool>,
    pub run: Option<Rc<Value>>,
    pub char: Option<String>,
    pub asc: Value,
    pub result: Option<String>,
}

/// (fights, runs). A fight that was still going when the log ends has no "died".
pub fn load() -> Result<(Vec<Fight>, Vec<Rc<Value>>), String> {
    let runs: Vec<Rc<Value>> = read_jsonl(&log_path("runs.jsonl"))?.into_iter().map(Rc::new).collect();
    let mut by_time: IndexMap<String, Rc<Value>> = IndexMap::new();
    for r in &runs {
        let time = r["time"].as_str().ok_or("KeyError: 'time'")?;
        by_time.insert(upto(time, 19).replace('T', " "), r.clone());
    }
    let mut fights: Vec<Fight> = Vec::new();
    let mut cur: Option<usize> = None;
    let mut in_run: Vec<usize> = Vec::new();
    for line in lines()? {
        if let Some(m) = ACT!().captures(&line) {
            let (t, act, floor, kind, hp, mx) = (&m[1], int(&m[2]), int(&m[3]), &m[4], int(&m[5]), int(&m[6]));
            if let Some(c) = cur {
                if (floor, act) != (fights[c].floor, fights[c].act) {
                    fights[c].hp_out = Some(hp);
                    fights[c].died = Some(false);
                    cur = None;
                }
            }
            if matches!(kind, "monster" | "elite" | "boss") && cur.is_none() {
                fights.push(Fight {
                    t: t.to_string(),
                    act,
                    floor,
                    kind: kind.to_string(),
                    hp_in: hp,
                    max_hp: mx,
                    enemies: None,
                    rounds: 0,
                    turns: Vec::new(),
                    hp_out: None,
                    died: None,
                    run: None,
                    char: None,
                    asc: Value::Null,
                    result: None,
                });
                cur = Some(fights.len() - 1);
                in_run.push(fights.len() - 1);
            }
        } else if let (Some(c), Some(m)) = (cur, if cur.is_some() { COMBAT!().captures(&line) } else { None }) {
            let f = &mut fights[c];
            let (rnd, hp) = (int(&m[2]), int(&m[3]));
            if f.enemies.is_none() {
                f.enemies = Some(_names(&m[8]));
                f.hp_in = hp;
            }
            f.rounds = f.rounds.max(rnd);
            f.turns.push(Turn { rnd, hp, blk: int(&m[5]), en: int(&m[6]), powers: m[7].to_string(), enemies: m[8].to_string(), hand: m[9].to_string(), t: m[1].to_string() });
        } else if let Some(m) = OVER!().captures(&line) {
            let result = m[2].to_string();
            let at = strptime(&m[1])?;
            let run = (-2..8).find_map(|d| by_time.get(&strftime(at + d)).cloned());
            if let Some(c) = cur {
                fights[c].died = Some(result == "loss");
                fights[c].hp_out = if result == "loss" { Some(0) } else { None };
            }
            let char = match &run {
                Some(r) => Some(r["character"].as_str().ok_or("AttributeError: 'NoneType' object has no attribute 'replace'")?.replace("The ", "")),
                None => None,
            };
            for i in in_run.drain(..) {
                let f = &mut fights[i];
                f.run = run.clone();
                f.char = char.clone();
                f.asc = run.as_ref().map_or(Value::Null, |r| r["ascension"].clone());
                f.result = Some(result.clone());
            }
            cur = None;
        }
    }
    Ok((fights, runs))
}

pub fn bosses(min_ascension: i64) -> Result<(), String> {
    let mut table: IndexMap<(i64, String, Option<String>), [i64; 2]> = IndexMap::new();
    for f in load()?.0 {
        if f.kind == "boss" && f.died.is_some() && f.asc.int() >= min_ascension {
            // A boss room left before its first combat line (the game restarted there) has no enemies: not a fight.
            let Some(enemies) = f.enemies.clone() else { continue };
            for who in [Some("all".to_string()), f.char.clone()] {
                let row = table.entry((f.act, enemies.clone(), who)).or_insert([0, 0]);
                row[0] += (f.died == Some(false)) as i64;
                row[1] += 1;
            }
        }
    }
    let mut rows: Vec<(&(i64, String, Option<String>), &[i64; 2])> = table.iter().collect();
    base::sort_by_key(&mut rows, |(k, _)| (k.0, k.1.clone(), k.2.as_deref() != Some("all"), k.2.clone().unwrap_or_else(|| "None".to_string())));
    for ((act, boss, who), [won, n]) in rows {
        let who = who.as_deref().filter(|w| !w.is_empty()).unwrap_or("?");
        out!("act {} {:28} {:12} {}/{}", act, upto(boss, 28), who, won, n);
    }
    Ok(())
}

pub fn costs() -> Result<(), String> {
    // (act, kind, enemies) -> [fights, deaths, costs, rounds]
    let mut table: IndexMap<(i64, String, String), (i64, i64, Vec<f64>, Vec<i64>)> = IndexMap::new();
    for f in load()?.0 {
        let (Some(died), Some(enemies)) = (f.died, f.enemies.as_deref().filter(|e| !e.is_empty())) else {
            continue;
        };
        if f.kind != "boss" {
            let row = table.entry((f.act, f.kind.clone(), upto(enemies, 56).to_string())).or_default();
            row.0 += 1;
            row.1 += died as i64;
            row.3.push(f.rounds);
            if let (false, Some(hp_out)) = (died, f.hp_out) {
                row.2.push((f.hp_in - hp_out) as f64 / f.max_hp as f64);
            }
        }
    }
    let mut rows: Vec<(&(i64, String, String), &(i64, i64, Vec<f64>, Vec<i64>), f64)> =
        table.iter().filter(|(_, v)| v.0 >= 4).map(|(k, v)| (k, v, base::fsum(v.2.iter().copied()) / v.2.len().max(1) as f64)).collect();
    base::sort_by_key(&mut rows, |r| (r.0 .0, -r.2));
    for ((act, kind, name), v, cost) in rows {
        out!("act {} {:7} {:56} fights {:3}  deaths {}  cost {:3.0}% of Max HP  rounds {:.1}", act, kind, name, v.0, v.1, 100.0 * cost, mean_i(v.3.iter().copied(), v.3.len()));
    }
    Ok(())
}

pub fn show(start: &str, kinds: &[String]) -> Result<(), String> {
    let mut text: Vec<String> = Vec::new();
    let mut plays: Vec<String> = Vec::new();
    let mut rnd: Option<String> = None;
    let mut on = false;

    fn flush(text: &mut Vec<String>, plays: &mut Vec<String>) {
        if !plays.is_empty() {
            text.push(format!("      plays: {}", plays.join(" > ")));
            plays.clear();
        }
    }

    for line in lines()? {
        if !on {
            if let Some(m) = ACT!().captures(&line) {
                if upto(&line, start.chars().count()) >= start && kinds.iter().any(|k| *k == m[4]) {
                    on = true;
                    text.push(from(&line, 11).to_string());
                }
            }
        } else if ACT!().is_match(&line) || OVER!().is_match(&line) || line.contains("boss fight over") {
            flush(&mut text, &mut plays);
            text.push(from(&line, 11).to_string());
            if !line.contains("boss fight over") {
                break;
            }
        } else if let Some(m) = COMBAT!().captures(&line) {
            if Some(&m[2]) != rnd.as_deref() {
                flush(&mut text, &mut plays);
                rnd = Some(m[2].to_string());
                text.push(format!("r{} hp {}/{} blk {} en {}{} | {} | {}", &m[2], &m[3], &m[4], &m[5], &m[6], &m[7], &m[8], &m[9]));
            }
        } else if let Some(m) = re!(r"DEBUG play_card .*-> Playing '([^']+)'").captures(&line) {
            plays.push(m[1].to_string());
        } else if let Some(m) = re!(r"Using potion '([^']+)'").captures(&line) {
            plays.push(format!("POTION {}", &m[1]));
        } else if line.contains("DEBUG ending r") {
            plays.push(format!("[{}]", line.split("DEBUG ").nth(1).unwrap_or("")));
        } else if line.contains("hand select") || line.contains("DEBUG choose") {
            let rest = line.split("DEBUG ").nth(1).ok_or("IndexError: list index out of range")?;
            plays.push(format!("{{{}}}", upto(rest, 160)));
        } else if let Some(m) = re!(r"Selecting card from hand: (.*)|Choosing card: (.*)").captures(&line) {
            let picked = m.get(1).map(|g| g.as_str()).filter(|s| !s.is_empty()).or_else(|| m.get(2).map(|g| g.as_str()));
            plays.push(format!("<pick {}>", picked.unwrap_or("None")));
        }
    }
    flush(&mut text, &mut plays);
    out!("{}", text.join("\n"));
    Ok(())
}

pub fn deaths(since: &str) -> Result<(), String> {
    for f in load()?.0 {
        if f.died == Some(true) && f.t.as_str() >= since {
            let settings = _knobs(&f).iter().map(|(k, v)| format!("{}={}", k, base::str_of(v))).collect::<Vec<_>>().join(" ");
            let who = f.char.clone().unwrap_or_else(|| "None".to_string());
            let enemies = f.enemies.clone().unwrap_or_else(|| "None".to_string());
            out!(
                "{} {:11} a{} act {} floor {:2} {:7} {:34} rounds {:2}  entered on {}/{}  {}",
                slice(&f.t, 5, 16),
                upto(&who, 11),
                base::str_of(&f.asc),
                f.act,
                f.floor,
                f.kind,
                upto(&enemies, 34),
                f.rounds,
                f.hp_in,
                f.max_hp,
                settings
            );
        }
    }
    Ok(())
}

/// Per setting and value: runs, where they ended, and the bosses they got past (a run that died at a boss met it).
pub fn knobs(since: &str) -> Result<(), String> {
    let mut runs: Vec<Value> = Vec::new();
    for r in read_jsonl(&log_path("runs.jsonl"))? {
        if r["knobs"].truthy() && r["time"].as_str().ok_or("KeyError: 'time'")? >= since {
            runs.push(r);
        }
    }
    let names: std::collections::BTreeSet<String> = runs.iter().flat_map(|r| r["knobs"].entries().map(|(k, _)| k.clone())).collect();
    for name in names {
        let mut table: Vec<(Value, Vec<&Value>)> = Vec::new();
        for r in &runs {
            if r["knobs"].has(&name) {
                push_by_value(&mut table, &r["knobs"][&name], r);
            }
        }
        table.sort_by(|a, b| value_cmp(&a.0, &b.0));
        for (value, rs) in table {
            let count = |test: &dyn Fn(&Value) -> bool| rs.iter().filter(|r| test(r)).count();
            let won = count(&|r| r["result"] == "win");
            let met2 = count(&|r| (r["act"].int() == 2 && r["floor"].int() == 33) || r["act"].int() >= 3);
            let met3 = count(&|r| (r["act"].int() == 3 && r["floor"].int() == 48) || r["result"] == "win");
            out!(
                "{:14} {}  runs {:3}  mean floor {:4.1}  past act 1 {:3}/{:<3}  past the act 2 boss {:2}/{:<2}  last boss {}/{}",
                name,
                fmt_value(&value, 5),
                rs.len(),
                mean_i(rs.iter().map(|r| r["floor"].int()), rs.len()),
                count(&|r| r["act"].int() >= 2),
                rs.len(),
                count(&|r| r["act"].int() >= 3),
                met2,
                won,
                met3
            );
        }
    }
    Ok(())
}

pub fn pace(since: &str) -> Result<(), String> {
    let mut table: std::collections::BTreeMap<i64, Vec<(i64, f64, bool)>> = std::collections::BTreeMap::new();
    for f in load()?.0 {
        if let (true, false, Some(died), true) = (f.kind == "monster", f.turns.is_empty(), f.died, f.t.as_str() >= since) {
            table.entry(f.act).or_default().push((f.rounds, _dealt(&f) as f64 / f.rounds.max(1) as f64, died));
        }
    }
    for (act, rows) in table {
        out!(
            "act {}: ordinary fights {:4}  rounds {:.2}  enemy HP removed a round {:.1}  deaths {}",
            act,
            rows.len(),
            mean_i(rows.iter().map(|r| r.0), rows.len()),
            mean_f(rows.iter().map(|r| r.1), rows.len()),
            rows.iter().filter(|r| r.2).count()
        );
    }
    Ok(())
}

/// A turn ended: (kind of fight, first state of the turn, [(state before a play, card name, state after)], last state).
pub type TurnEnded = (Option<String>, Rc<Combat>, Vec<(Rc<Combat>, String, Rc<Combat>)>, Rc<Combat>);

/// (kind of fight, first state of a turn, [(state before a play, card name, state after)], last state) for every turn ended.
pub fn _turns() -> Result<Vec<TurnEnded>, String> {
    let mut ended: Vec<TurnEnded> = Vec::new();
    let mut kind: Option<String> = None;
    let (mut first, mut last): (Option<Rc<Combat>>, Option<Rc<Combat>>) = (None, None);
    let mut pending: Option<(Rc<Combat>, String)> = None;
    let mut plays: Vec<(Rc<Combat>, String, Rc<Combat>)> = Vec::new();
    for line in lines()? {
        if let Some(m) = ACT!().captures(&line) {
            kind = Some(m[4].to_string());
            (first, last, pending) = (None, None, None);
            plays = Vec::new();
        } else if let Some(m) = Combat::of(&line).map(Rc::new) {
            if first.as_ref().is_none_or(|f| f.group(2) != m.group(2)) {
                first = Some(m.clone());
                plays = Vec::new();
            }
            if let Some((before, name)) = pending.take() {
                plays.push((before, name, m.clone()));
            }
            last = Some(m);
        } else if let (Some(m), Some(l)) = (PLAYED!().captures(&line), &last) {
            pending = Some((l.clone(), m[2].to_string()));
        } else if line.contains("DEBUG end_turn") && line.contains("->") && first.is_some() {
            // (a turn with a first state has a last one)
            ended.push((kind.clone(), first.take().unwrap(), plays.clone(), last.take().unwrap()));
            pending = None;
        }
    }
    Ok(ended)
}

/// Single-enemy plays with nothing on the enemy that bends damage, read against done. X-cost cards are left out (the hand
/// shows their damage for one energy); orb cards read 0 (their damage and Block come from the orbs, in `combat::simulate`).
pub fn reads() -> Result<(), String> {
    let odd = ["SLIPPERY", "INTANGIBLE", "HARD_TO_KILL", "FLUTTER", "SOAR", "SKITTISH", "BURROWED", "PLATING", "ASLEEP", "ADAPTABLE", "SLOW", "VULNERABLE", "HANG"];
    let mut table: IndexMap<String, Vec<[i64; 4]>> = IndexMap::new();
    let took = |g: &Combat| _enemy_hp(g.group(8)) + re!(r"hp/(\d+)blk").captures_iter(g.group(8)).map(|m| int(&m[1])).sum::<i64>();
    for (_kind, _first, plays, _last) in _turns()? {
        for (before, name, after) in plays {
            if before.group(8).contains(" ; ") || odd.iter().any(|o| before.group(8).contains(o)) || _enemy_hp(after.group(8)) == 0 {
                continue;
            }
            let Some(card) = IN_HAND!().captures_iter(before.group(9)).find(|c| c[2].trim() == name) else {
                continue;
            };
            if &card[3] == "X" {
                continue;
            }
            let d = re!(r"d(\d+)(?:x(\d+))?").captures(&card[5]);
            let b = re!(r"b(\d+)").captures(&card[5]);
            table.entry(name.trim_end_matches('+').to_string()).or_default().push([
                d.map_or(0, |d| int(&d[1]) * d.get(2).map_or(1, |x| int(x.as_str()))),
                took(&before) - took(&after),
                b.map_or(0, |b| int(&b[1])),
                int(after.group(5)) - int(before.group(5)),
            ]);
        }
    }
    let mut rows: Vec<(&String, usize, [f64; 4])> =
        table.iter().filter(|(_, v)| v.len() >= 5).map(|(n, v)| (n, v.len(), [0, 1, 2, 3].map(|i| mean_i(v.iter().map(|x| x[i]), v.len())))).collect();
    base::sort_by_key(&mut rows, |r| -((r.2[0] - r.2[1]).abs() + (r.2[2] - r.2[3]).abs()));
    for (n, k, [rd, dd, rb, gb]) in rows.into_iter().take(50) {
        out!("{:24} n {:4}  damage read {:5.1} did {:5.1}   Block read {:5.1} got {:5.1}", n, k, rd, dd, rb, gb);
    }
    Ok(())
}

pub fn unplayed(kinds: &[String]) -> Result<(), String> {
    let (mut played, mut left) = (Counter::default(), Counter::default());
    for (kind, first, plays, last) in _turns()? {
        if kind.is_some_and(|k| kinds.contains(&k)) {
            for (_before, name, _after) in &plays {
                played.add(name.trim_end_matches('+'));
            }
            let energy = int(first.group(6));
            for c in IN_HAND!().captures_iter(last.group(9)) {
                if c[4].is_empty() && c[3].chars().all(|x| x.is_numeric()) && int(&c[3]) <= energy {
                    left.add(c[2].trim().trim_end_matches('+'));
                }
            }
        }
    }
    let names: std::collections::BTreeSet<&String> = left.0.keys().chain(played.0.keys()).collect();
    let mut rows: Vec<(f64, &String)> = names.into_iter().filter(|n| left.get(n) + played.get(n) >= 12).map(|n| (left.get(n) as f64 / (left.get(n) + played.get(n)) as f64, n)).collect();
    rows.sort_by(|a, b| b.partial_cmp(a).unwrap_or(Ordering::Equal));
    for (share, n) in rows.into_iter().take(45) {
        out!("{:24} played {:4}  left {:4}  ({:.0}% left)", n, played.get(n), left.get(n), 100.0 * share);
    }
    Ok(())
}

pub fn rooms(name: &str, since: &str) -> Result<(), String> {
    // (who, act, value) -> [(rounds, enemy HP a round, cost, died)]
    let mut table: Vec<((Option<String>, i64, Value), Vec<(i64, f64, f64, bool)>)> = Vec::new();
    for f in load()?.0 {
        let value = _knobs(&f).get(name).cloned().unwrap_or(Value::Null);
        let Some(died) = f.died else {
            continue;
        };
        if f.kind != "monster" || f.turns.is_empty() || value.is_null() || f.t.as_str() < since {
            continue;
        }
        let out = if died { Some(0) } else { f.hp_out };
        if let Some(out) = out {
            for who in [f.char.clone(), Some("all".to_string())] {
                let row = (f.rounds, _dealt(&f) as f64 / f.rounds.max(1) as f64, (f.hp_in - out) as f64 / f.max_hp as f64, died);
                match table.iter_mut().find(|(k, _)| k.0 == who && k.1 == f.act && same_value(&k.2, &value)) {
                    Some((_, list)) => list.push(row),
                    None => table.push(((who, f.act, value.clone()), vec![row])),
                }
            }
        }
    }
    let text = |who: &Option<String>| who.clone().unwrap_or_else(|| "None".to_string());
    table.sort_by(|(a, _), (b, _)| {
        (a.0.as_deref() == Some("all"), text(&a.0), a.1).cmp(&(b.0.as_deref() == Some("all"), text(&b.0), b.1)).then_with(|| value_cmp(&a.2, &b.2))
    });
    for ((who, act, value), v) in table {
        if v.len() >= 8 {
            let who = who.ok_or("TypeError: unsupported format string passed to NoneType.__format__")?;
            out!(
                "{:12} act {} {} {}: fights {:3}  rounds {:.2}  enemy HP a round {:5.1}  cost {:4.1}% of Max HP  deaths {}",
                who,
                act,
                name,
                fmt_value(&value, 4),
                v.len(),
                mean_i(v.iter().map(|x| x.0), v.len()),
                mean_f(v.iter().map(|x| x.1), v.len()),
                100.0 * base::fsum(v.iter().map(|x| x.2)) / v.len() as f64,
                v.iter().filter(|x| x.3).count()
            );
        }
    }
    Ok(())
}

/// The end of a turn: (time, kind of fight, enemy names, the last combat state of the turn, HP expected to be lost, HP the
/// next round began on or None).
pub type Ending = (String, Option<String>, String, Rc<Combat>, i64, Option<i64>);

/// (time, kind of fight, enemy names, the last combat state of the turn, HP expected to be lost, HP the next round began on or None).
pub fn _endings() -> Result<Vec<Ending>, String> {
    let mut found: Vec<Ending> = Vec::new();
    let mut kind: Option<String> = None;
    let mut last: Option<Rc<Combat>> = None;
    let mut pending: Option<(String, Option<String>, String, Rc<Combat>, i64)> = None;
    for line in lines()? {
        let act = ACT!().captures(&line);
        if act.is_some() || OVER!().is_match(&line) {
            if let Some((t, k, names, l, expected)) = pending.take() {
                found.push((t, k, names, l, expected, None));
            }
            if let Some(m) = act {
                kind = Some(m[4].to_string());
            }
            last = None;
        } else if let Some(m) = Combat::of(&line).map(Rc::new) {
            if pending.as_ref().is_some_and(|p| int(m.group(2)) == int(p.3.group(2)) + 1) {
                let (t, k, names, l, expected) = pending.take().unwrap();
                found.push((t, k, names, l, expected, Some(int(m.group(3)))));
            }
            last = Some(m);
        } else if let (Some(m), Some(l)) = (ENDING!().captures(&line), &last) {
            pending = Some((m[1].to_string(), kind.clone(), _names(l.group(8)), l.clone(), int(&m[4])));
        }
    }
    Ok(found)
}

fn kind_text(kind: &Option<String>) -> &str {
    kind.as_deref().unwrap_or("None")
}

pub fn surprises(since: &str) -> Result<(), String> {
    // (time, kind, names, last, expected, lost)
    let rows: Vec<(String, Option<String>, String, Rc<Combat>, i64, i64)> = _endings()?
        .into_iter()
        .filter_map(|(t, kind, names, last, expected, after)| match after {
            Some(after) if t.as_str() >= since => {
                let lost = int(last.group(3)) - after;
                Some((t, kind, names, last, expected, lost))
            }
            _ => None,
        })
        .collect();
    let under: Vec<&(String, Option<String>, String, Rc<Combat>, i64, i64)> = rows.iter().filter(|r| r.5 - r.4 >= 3).collect();
    out!("turns {}; lost 3 HP or more beyond the plan: {}; 3 or more less: {}", rows.len(), under.len(), rows.iter().filter(|r| r.4 - r.5 >= 3).count());
    let (mut turns, mut worst) = (Counter::default(), Counter::default());
    for r in &rows {
        turns.add(upto(&r.2, 44));
    }
    for r in &under {
        worst.add(upto(&r.2, 44));
    }
    for (name, n) in worst.most_common(Some(15)) {
        out!("  {:44} {:3} of {:4} turns", name, n, turns.get(&name));
    }
    for (t, kind, _names, last, expected, lost) in under.iter().skip(under.len().saturating_sub(12)).map(|r| (&r.0, &r.1, &r.2, &r.3, r.4, r.5)) {
        out!(
            "{} {} expected {} lost {} | r{} hp {} blk {} en {}{} | {} | {}",
            from(t, 5),
            kind_text(kind),
            expected,
            lost,
            last.group(2),
            last.group(3),
            last.group(5),
            last.group(6),
            last.group(7),
            upto(last.group(8), 150),
            upto(last.group(9), 160)
        );
    }
    Ok(())
}

pub fn idle(since: &str) -> Result<(), String> {
    let mut rows: Vec<(String, Option<String>, String, Rc<Combat>, i64)> = Vec::new();
    for (t, kind, names, last, expected, _after) in _endings()? {
        let energy = int(last.group(6));
        let spare = IN_HAND!().captures_iter(last.group(9)).any(|c| c[4].is_empty() && c[3].chars().all(|x| x.is_numeric()) && int(&c[3]) <= energy && !c[5].trim().is_empty());
        // (Under Vital Spark every Skill played makes each hit on us harder: holding a Defend there is no idle turn.)
        if t.as_str() >= since && energy >= 1 && spare && expected >= 8 && !last.group(8).contains("VITAL_SPARK") {
            rows.push((t, kind, names, last, expected));
        }
    }
    let mut tally = Counter::default();
    for r in &rows {
        tally.add(upto(&r.2, 40));
    }
    out!("{}", Datum::List(tally.most_common(Some(15)).into_iter().map(|(k, n)| Datum::Tuple(vec![Datum::Str(k), Datum::Int(n)])).collect()).repr());
    for (t, kind, _names, last, expected) in rows.iter().skip(rows.len().saturating_sub(25)) {
        out!(
            "{} {} energy {} expects to lose {} | hp {}{} | {} | {}",
            from(t, 5),
            kind_text(kind),
            last.group(6),
            expected,
            last.group(3),
            last.group(7),
            upto(last.group(8), 150),
            upto(last.group(9), 170)
        );
    }
    Ok(())
}

pub fn pit(since: &str) -> Result<(), String> {
    let mut fight: Vec<String> = Vec::new();
    let mut tally = Counter::default();
    for line in lines()? {
        if line.contains(" combat r") && line.contains("THE_INSATIABLE_0") || (!fight.is_empty() && line.contains(" ending r")) {
            fight.push(line);
        } else if line.contains("boss fight over") && line.contains("INSATIABLE") && !fight.is_empty() {
            let states: Vec<&String> = fight.iter().filter(|x| x.contains(" combat r")).collect();
            let last = states[states.len() - 1];
            let end = fight.iter().rfind(|x| x.contains(" ending r"));
            let mut shown: IndexMap<String, String> = IndexMap::new();
            for x in &states {
                // the first state of each round: the Sandpit as the turn began
                let rnd = re!(r"combat r(\d+) ").captures(x).ok_or("AttributeError: 'NoneType' object has no attribute 'group'")?[1].to_string();
                let sand = re!(r"SANDPIT:(\d+)").captures(x);
                let hand = x.rsplit_once("| hand").map_or(x.as_str(), |p| p.1);
                shown.entry(rnd).or_insert_with(|| format!("{}{}", sand.map_or("-".to_string(), |s| s[1].to_string()), if hand.contains("Frantic Escape") { "E" } else { "" }));
            }
            let sand = re!(r"SANDPIT:(\d+)").captures(last).map(|s| s[1].to_string());
            let hp = int(&re!(r" hp (\d+)/").captures(last).ok_or("AttributeError: 'NoneType' object has no attribute 'group'")?[1]);
            let expect = match end {
                Some(e) => int(&re!(r"expecting to lose (\d+)").captures(e).ok_or("AttributeError: 'NoneType' object has no attribute 'group'")?[1]),
                None => 0,
            };
            let how = if line.contains(" won;") {
                "won"
            } else if sand.as_deref().is_some_and(|s| int(s) <= 1) && hp > expect {
                "eaten"
            } else {
                "lost"
            };
            if upto(&line, 16) >= since {
                tally.add(how);
                let boss = re!(r"boss hp \d+ -> (\d+)").captures(&line).ok_or("AttributeError: 'NoneType' object has no attribute 'group'")?[1].to_string();
                out!(
                    "{} {:5} rounds {:2}  boss left {:>3}  hp {:2}  Sandpit at the end {}  | {}",
                    slice(&line, 5, 16),
                    how,
                    shown.len(),
                    boss,
                    hp,
                    sand.as_deref().unwrap_or("-"),
                    shown.values().cloned().collect::<Vec<_>>().join(" ")
                );
            }
            fight = Vec::new();
        }
    }
    out!("{}", Datum::Dict(tally.0.iter().map(|(k, n)| (Datum::str(k), Datum::Int(*n))).collect()).repr());
    Ok(())
}

pub fn artifact(since: &str, kinds: &[String]) -> Result<(), String> {
    let mut gone: std::collections::BTreeMap<String, Vec<(i64, i64)>> = std::collections::BTreeMap::new();
    for f in load()?.0 {
        let left = _by_round(&f, |e| re!(r"ARTIFACT:(\d+)").captures_iter(e).map(|m| int(&m[1])).sum::<i64>());
        let Some(died) = f.died else {
            continue;
        };
        if left.is_empty() || left[0] == 0 || f.t.as_str() < since {
            continue;
        }
        let by = left.iter().position(|a| *a == 0).map(|n| n as i64 + 1);
        gone.entry(f.kind.clone()).or_default().push((by.unwrap_or(left.len() as i64 + 1) - 1, left.len() as i64));
        if kinds.contains(&f.kind) {
            out!(
                "{} {:7} {:24} {:11} {:4} rounds {:2}  gone by {:>2}  | {}",
                slice(&f.t, 5, 16),
                f.kind,
                upto(f.enemies.as_deref().unwrap_or(""), 24),
                f.char.as_deref().unwrap_or("None"),
                if died { "lost" } else { "won " },
                left.len(),
                by.map_or("-".to_string(), |b| b.to_string()),
                left.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" ")
            );
        }
    }
    for (kind, rows) in gone {
        out!("{:7} {:3} fights: Artifact stood for {:.1} of {:.1} rounds", kind, rows.len(), mean_i(rows.iter().map(|r| r.0), rows.len()), mean_i(rows.iter().map(|r| r.1), rows.len()));
    }
    Ok(())
}

pub fn crabs(since: &str) -> Result<(), String> {
    let mut tally: std::collections::BTreeMap<(String, String), i64> = std::collections::BTreeMap::new();
    for f in load()?.0 {
        let Some(died) = f.died else {
            continue;
        };
        if f.enemies.as_deref() != Some("CRUSHER+ROCKET") || f.t.as_str() < since {
            continue;
        }
        let path = _by_round(&f, |e| {
            let mut hp = [0i64, 0];
            for m in re!(r"(CRUSHER|ROCKET)_\d+ (\d+)hp").captures_iter(e) {
                hp[(&m[1] == "ROCKET") as usize] = int(&m[2]);
            }
            hp
        });
        let at = path.iter().position(|[c, r]| *c == 0 || *r == 0);
        let first = match at {
            None => "neither",
            Some(at) if path[at][0] == 0 => "Crusher",
            Some(_) => "Rocket",
        };
        *tally.entry((first.to_string(), if died { "lost" } else { "won" }.to_string())).or_insert(0) += 1;
        let when = match at {
            Some(at) => format!("by round {:2}, the other on {:3}", at + 1, path[at][0].max(path[at][1])),
            None => " ".repeat(31),
        };
        out!(
            "{} {:11} {:4} rounds {:2}  first to die: {:7} {} | {}",
            slice(&f.t, 5, 16),
            f.char.as_deref().unwrap_or("None"),
            if died { "lost" } else { "won " },
            path.len(),
            first,
            when,
            path.iter().map(|[c, r]| format!("{c}/{r}")).collect::<Vec<_>>().join(" ")
        );
    }
    out!("{}", Datum::Dict(tally.into_iter().map(|((a, b), n)| (Datum::Str(format!("{a} {b}")), Datum::Int(n))).collect()).repr());
    Ok(())
}

/// The bot's own sources, each file's tests left out: what the original read from the sts2bot/ directory beside the script.
pub fn bot_sources() -> Result<String, String> {
    fn walk(dir: &Path, top: bool, code: &mut String) -> Result<(), String> {
        let shown = dir.to_string_lossy().into_owned();
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).map_err(|e| io_error(&e, &shown))?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if path.is_dir() {
                if !(top && matches!(name.as_str(), "sim" | "scripts")) {
                    walk(&path, false, code)?;
                }
            } else if name.ends_with(".rs") {
                let text = std::fs::read_to_string(&path).map_err(|e| io_error(&e, &path.to_string_lossy()))?;
                code.push_str(re!(r"\n#\[cfg\(test\)\]\n(?:pub(?:\(crate\))? )?mod tests").split(&text).next().unwrap_or(""));
            }
        }
        Ok(())
    }
    let mut code = String::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), true, &mut code)?;
    Ok(code)
}

pub fn powers() -> Result<(), String> {
    let code = bot_sources()?;
    let mut seen: IndexMap<(bool, String), (String, String)> = IndexMap::new();
    for line in lines()? {
        if let Some(m) = re!(r" INFO power (\w+?)(?:_POWER)? \(on (.+?)\): (.*)$").captures(&line) {
            if !code.contains(&format!("\"{}\"", &m[1])) {
                let ours = m[2].starts_with("The ") && m[2].split_whitespace().last().is_some_and(|w| ["Ironclad", "Silent", "Regent", "Necrobinder", "Defect", "Fool"].contains(&w));
                seen.entry((ours, m[1].to_string())).or_insert_with(|| (m[2].to_string(), m[3].to_string()));
            }
        }
    }
    seen.sort_keys();
    for ((ours, name), (who, text)) in seen {
        out!("{} {:28} {:22} {}", if ours { "on us    " } else { "on enemy " }, name, upto(&who, 22), upto(&text, 150));
    }
    Ok(())
}

pub fn events() -> Result<(), String> {
    let mut met: IndexMap<String, (Counter, String)> = IndexMap::new();
    let mut last: Option<(String, String)> = None;
    for line in lines()? {
        if let Some(m) = re!(r" DEBUG event (\w+): (.*)$").captures(&line) {
            last = Some((m[1].to_string(), m[2].to_string()));
        } else if let (Some(m), true) = (re!(r"Choosing event option: (.*)$").captures(&line), last.is_some()) {
            if m[1].trim() != "Proceed" {
                let (name, text) = last.take().unwrap();
                let entry = met.entry(name).or_default();
                entry.0.add(upto(m[1].trim(), 40));
                entry.1 = text;
            }
        }
    }
    let mut rows: Vec<(String, (Counter, String))> = met.into_iter().collect();
    base::sort_by_key(&mut rows, |(_, (taken, _))| -taken.0.values().sum::<i64>());
    for (name, (taken, text)) in rows {
        out!("{:28} {}\n      {}", name, Datum::Dict(taken.0.iter().map(|(k, n)| (Datum::str(k), Datum::Int(*n))).collect()).repr(), upto(&text, 400));
    }
    Ok(())
}

pub fn wiki() -> Result<(), String> {
    wiki_at(15526)
}

/// `wiki`, from the mod listening on this port of localhost (the game's is 15526).
pub fn wiki_at(port: u16) -> Result<(), String> {
    let client = crate::client::Client::new("localhost", port, 20.0);
    // (urllib raised on an HTTP error; the bot's client hands the error's JSON back: it has no "results" and nothing is kept)
    let get = |path: &str| -> Result<Value, String> { client._request(&format!("http://localhost:{port}/api/v1{path}"), None).map_err(|e| format!("urllib.error.URLError: {e}")) };

    let profile = get("/profile")?;
    let mut found_all: IndexMap<&str, Map<String, Value>> = IndexMap::from([("cards", Map::new()), ("relics", Map::new())]);
    for kind in ["card", "relic"] {
        for item in profile[format!("discovered_{kind}s")].list() {
            let key = if item.is_string() { item.clone() } else { item["id"].clone() };
            let found = get(&format!("/wiki?q={}&type={}", base::url_quote(&base::str_of(&key)), kind))?;
            for hit in found["results"].list() {
                if hit["id"] == key {
                    found_all[format!("{kind}s").as_str()].insert(base::str_of(&key), hit.clone());
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50)); // one request at a time: a pool of them stalls the game while the bot plays
        }
    }
    let path = log_path("wiki.json");
    let (cards, relics) = (found_all["cards"].len(), found_all["relics"].len());
    let data = Value::Object(found_all.into_iter().map(|(k, v)| (k.to_string(), Value::Object(v))).collect());
    std::fs::write(&path, base::json_dumps(&data)).map_err(|e| io_error(&e, &path))?;
    out!("{} cards and {} relics in {}", cards, relics, path);
    Ok(())
}

pub fn ancients() -> Result<(), String> {
    use crate::run::events as ev;
    use crate::run::relics;

    relics::load(Some(Path::new(&log_path("runs.jsonl"))), ev::_ANCIENT.keys());
    let rec = relics::record();
    let later = |k: &str| relics::ACT2.contains(k) || relics::ACT3.contains(k) || relics::DARV.contains(k);
    let pools: [(&str, Option<&std::collections::HashSet<&'static str>>); 4] =
        [("Neow (floor 1)", None), ("act 2: Orobas, Pael, Tezcatara", Some(&relics::ACT2)), ("Darv (act 2 or 3)", Some(&relics::DARV)), ("act 3: Nonupeipe, Tanx, Vakuu", Some(&relics::ACT3))];
    for (title, pool) in pools {
        out!("{title}");
        let mut names: Vec<&str> = ev::_ANCIENT.keys().copied().filter(|k| pool.map_or_else(|| !later(k), |p| p.contains(k))).collect();
        base::sort_by_key(&mut names, |k| -(ev::_ANCIENT[*k] + relics::shift(k)));
        for k in names {
            let (n, gain) = rec.get(k).copied().unwrap_or((0, 0.0));
            let rated = ev::_ANCIENT[k];
            let held = if n != 0 { format!("  {:+5.1} floors a run", gain) } else { String::new() };
            out!("   {:20} {:5.1} {:+5.1} = {:5.1}   held {:3}{}", k, rated, relics::shift(k), rated + relics::shift(k), n, held);
        }
    }
    Ok(())
}

/// HP the other side lost a round in this fight, from one round's first look to the next (a heal or a new form is not a loss).
pub fn boss_pace(f: &Fight) -> f64 {
    let hp = _by_round(f, _enemy_hp);
    let lost: i64 = hp.iter().zip(hp.iter().skip(1)).map(|(a, b)| (a - b).max(0)).sum();
    lost as f64 / (hp.len() as i64 - 1).max(1) as f64
}

pub fn bosspace(since: &str) -> Result<(), String> {
    let rows: Vec<(Fight, f64)> = load()?
        .0
        .into_iter()
        .filter(|f| f.kind == "boss" && f.died.is_some() && f.rounds >= 3 && f.char.as_deref().is_some_and(|c| !c.is_empty()) && f.t.as_str() >= since)
        .map(|f| {
            let pace = boss_pace(&f);
            (f, pace)
        })
        .collect();
    let acts: std::collections::BTreeSet<i64> = rows.iter().map(|(f, _)| f.act).collect();
    for act in acts {
        let sub: Vec<&(Fight, f64)> = rows.iter().filter(|(f, _)| f.act == act).collect();
        let bands = [(0, 20), (20, 25), (25, 30), (30, 35), (35, 45), (45, 999)];
        let won = |lo: i64, hi: i64| {
            let v: Vec<&&(Fight, f64)> = sub.iter().filter(|x| lo as f64 <= x.1 && x.1 < hi as f64).collect();
            format!("{}/{}", v.iter().filter(|(f, _)| f.died == Some(false)).count(), v.len())
        };
        out!(
            "act {}: {} boss fights, {:.1} a round; won by HP a round: {}",
            act,
            sub.len(),
            mean_f(sub.iter().map(|(_, p)| *p), sub.len()),
            bands.iter().map(|(lo, hi)| format!("{}-{}: {}", lo, if *hi < 999 { hi.to_string() } else { String::new() }, won(*lo, *hi))).collect::<Vec<_>>().join("  ")
        );
        let key = |f: &Fight| (f.char.clone().unwrap_or_default(), f.enemies.clone());
        let mut lists: IndexMap<(String, Option<String>), Vec<f64>> = IndexMap::new();
        for (f, p) in &sub {
            lists.entry(key(f)).or_default().push(*p);
        }
        let usual: IndexMap<(String, Option<String>), f64> = lists.into_iter().map(|(k, v)| (k, base::fsum(v.iter().copied()) / v.len() as f64)).collect();
        let names: std::collections::BTreeSet<String> = sub.iter().flat_map(|(f, _)| _knobs(f).keys().cloned().collect::<Vec<_>>()).collect();
        for name in names {
            let mut arms: Vec<(Value, Vec<(f64, bool)>)> = Vec::new();
            for (f, p) in &sub {
                if let Some(value) = _knobs(f).get(&name).filter(|v| !v.is_null()) {
                    push_by_value(&mut arms, value, (p - usual[&key(f)], f.died == Some(false)));
                }
            }
            if arms.len() < 2 {
                continue;
            }
            arms.sort_by(|a, b| value_cmp(&a.0, &b.0));
            let mut cells: Vec<String> = Vec::new();
            for (value, v) in arms {
                let mean = mean_f(v.iter().map(|(d, _)| *d), v.len());
                let se = (base::fsum(v.iter().map(|(d, _)| (d - mean).powf(2.0))) / (v.len() as i64 - 1).max(1) as f64 / v.len() as f64).powf(0.5);
                cells.push(format!("{}: {:+5.1} a round (se {:.1}), won {}/{}", base::str_of(&value), mean, se, v.iter().filter(|(_, w)| *w).count(), v.len()));
            }
            out!("    {:14} {}", name, cells.join("   "));
        }
    }
    Ok(())
}

fn script(args: &[String]) -> Result<(), String> {
    let rest: &[String] = if args.is_empty() { &[] } else { &args[1..] };
    let since = rest.first().map_or("", String::as_str);
    let kinds = |i: usize, default: &[&str]| -> Vec<String> {
        match rest.get(i) {
            Some(text) => text.split(',').map(str::to_string).collect(),
            None => default.iter().map(|k| k.to_string()).collect(),
        }
    };
    let usage = || {
        out!("{DOC}");
        Ok(())
    };
    match args.first().map_or("", String::as_str) {
        "bosses" => bosses(match rest.first() {
            Some(text) => super::modelfight::int(text)?,
            None => 0,
        }),
        "unplayed" => unplayed(&kinds(0, &["boss", "elite"])),
        "artifact" => artifact(since, &kinds(1, &["boss", "elite"])),
        "rooms" if !rest.is_empty() => rooms(&rest[0], rest.get(1).map_or("", String::as_str)),
        "show" if !rest.is_empty() => show(&rest[0], &kinds(1, &["boss"])),
        "costs" => costs(),
        "reads" => reads(),
        "powers" => powers(),
        "events" => events(),
        "wiki" => wiki(),
        "ancients" => ancients(),
        "deaths" => deaths(since),
        "knobs" => knobs(since),
        "pace" => pace(since),
        "surprises" => surprises(since),
        "idle" => idle(since),
        "pit" => pit(since),
        "bosspace" => bosspace(since),
        "crabs" => crabs(since),
        _ => usage(),
    }
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(script(&args))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::modelfight::{capture, golden};
    use super::*;

    /// The synthetic state directory the goldens were made from (tests/golden/logs/state).
    pub(crate) fn state() -> StateGuard {
        with_state(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/logs/state"))
    }

    macro_rules! as_recorded {
        ($($test:ident: $file:literal, $args:expr;)*) => {$(
            #[test]
            fn $test() {
                let _state = state();
                golden(main, &$args, include_str!(concat!("../../tests/golden/logs/fightlog_", $file, ".txt")));
            }
        )*};
    }

    as_recorded! {
        bosses_as_recorded: "bosses", ["bosses"];
        bosses_from_an_ascension: "bosses_2", ["bosses", "2"];
        costs_as_recorded: "costs", ["costs"];
        show_as_recorded: "show", ["show", "2026-11-21 13:4"];
        show_other_kinds: "show_elite", ["show", "2026-11-22 12:0", "elite,monster"];
        deaths_as_recorded: "deaths", ["deaths"];
        deaths_since: "deaths_since", ["deaths", "2026-11-22"];
        knobs_as_recorded: "knobs", ["knobs"];
        knobs_since: "knobs_since", ["knobs", "2026-11-21T00:00"];
        pace_as_recorded: "pace", ["pace"];
        pace_since: "pace_since", ["pace", "2026-11-22 12:00"];
        reads_as_recorded: "reads", ["reads"];
        unplayed_as_recorded: "unplayed", ["unplayed"];
        unplayed_in_ordinary_fights: "unplayed_monster", ["unplayed", "monster"];
        rooms_as_recorded: "rooms", ["rooms", "caution"];
        rooms_since: "rooms_since", ["rooms", "punch", "2026-11-21 13:55"];
        surprises_as_recorded: "surprises", ["surprises"];
        surprises_since: "surprises_since", ["surprises", "2026-11-22"];
        idle_as_recorded: "idle", ["idle"];
        idle_since: "idle_since", ["idle", "2026-11-22"];
        pit_as_recorded: "pit", ["pit"];
        pit_since: "pit_since", ["pit", "2026-11-21 13:55"];
        artifact_as_recorded: "artifact", ["artifact"];
        artifact_of_every_kind: "artifact_all", ["artifact", "2026-11-21 13:50", "boss,elite,monster"];
        crabs_as_recorded: "crabs", ["crabs"];
        crabs_since: "crabs_since", ["crabs", "2026-11-21 13:53"];
        bosspace_as_recorded: "bosspace", ["bosspace"];
        bosspace_since: "bosspace_since", ["bosspace", "2026-11-22"];
        powers_as_recorded: "powers", ["powers"];
        events_as_recorded: "events", ["events"];
        ancients_as_recorded: "ancients", ["ancients"];
    }

    #[test]
    fn no_command_prints_the_usage() {
        for args in [vec![], vec!["nothing"], vec!["rooms"], vec!["show"]] {
            let (code, text) = capture(|| main(args.iter().map(|a| a.to_string()).collect()));
            assert_eq!((code, text), (0, format!("{DOC}\n")));
        }
    }

    /// The made-up game the original `wiki` golden was made against (a small stand-in server): the profile, and a search that
    /// answers with near misses beside the hit.
    fn serve(listener: std::net::TcpListener, profile: Value, cards: Value) {
        use std::io::{Read, Write};
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            let target = head.split(' ').nth(1).unwrap_or("").to_string();
            if target == "/stop" {
                break;
            }
            let (path, query) = target.split_once('?').unwrap_or((&target, ""));
            let unquote = |text: &str| {
                let (bytes, mut raw, mut i) = (text.as_bytes(), Vec::new(), 0);
                while i < bytes.len() {
                    if bytes[i] == b'%' {
                        raw.push(u8::from_str_radix(&text[i + 1..i + 3], 16).unwrap());
                        i += 3;
                    } else {
                        raw.push(bytes[i]);
                        i += 1;
                    }
                }
                String::from_utf8(raw).unwrap()
            };
            let arg = |name: &str| query.split('&').find_map(|pair| pair.strip_prefix(&format!("{name}="))).map(unquote).unwrap_or_default();
            let body = if path == "/api/v1/profile" {
                profile.clone()
            } else {
                let (key, kind) = (arg("q"), arg("type"));
                let hits: Vec<Value> = if kind == "card" {
                    if cards.has(&key) {
                        cards.entries().filter(|(k, _)| k.starts_with(upto(&key, 5))).take(3).map(|(_, v)| v.clone()).collect()
                    } else {
                        Vec::new()
                    }
                } else if key == "LOST" {
                    Vec::new()
                } else {
                    vec![serde_json::json!({"id": format!("{key}_X"), "name": "near"}), serde_json::json!({"id": key, "name": base::title(&key), "description": "A relic \u{2014} \u{e9}."})]
                };
                serde_json::json!({ "results": hits })
            };
            let raw = serde_json::to_vec(&body).unwrap();
            let _ = stream.write_all(format!("HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", raw.len()).as_bytes());
            let _ = stream.write_all(&raw);
        }
    }

    #[test]
    fn wiki_as_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let _state = with_state(dir.path());
        let profile: Value = serde_json::from_str(include_str!("../../tests/golden/logs/wiki_profile.json")).unwrap();
        let cards: Value = serde_json::from_str::<Value>(include_str!("../../tests/golden/logs/state/wiki.json")).unwrap()["cards"].clone();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || serve(listener, profile, cards));
        let (result, text) = capture(|| wiki_at(port));
        let _ = crate::client::Client::new("127.0.0.1", port, 5.0)._request(&format!("http://127.0.0.1:{port}/stop"), None);
        server.join().unwrap();
        assert_eq!(result, Ok(()));
        assert_eq!(text.replace(&*dir.path().to_string_lossy(), "STATE"), include_str!("../../tests/golden/logs/fightlog_wiki.txt"));
        assert_eq!(std::fs::read_to_string(dir.path().join("wiki.json")).unwrap(), include_str!("../../tests/golden/logs/wiki_written.json"));
    }

    /// The patterns read what the bot writes: every "act", "run over" and "power" line of the bot's whole-session goldens
    /// (tests/golden/bot: the messages the Rust bot is held to), as bot.log would hold it.
    #[test]
    fn patterns_match_the_bots_lines() {
        let (mut acts, mut overs, mut powers) = (0, 0, 0);
        for name in ["sim_a", "sim_c", "screens", "fake_lose"] {
            let text = std::fs::read_to_string(format!("{}/tests/golden/bot/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
            let session: Value = serde_json::from_str(&text).unwrap();
            for entry in session["logs"].list() {
                let (level, message) = (entry[0].s(), entry[2].s());
                let line = format!("2026-11-27 10:00:00 {level} {message}");
                let line = line.split('\n').next().unwrap();
                if message.starts_with("act ") {
                    let m = ACT!().captures(line).unwrap_or_else(|| panic!("{line}"));
                    assert!(message.starts_with(&format!("act {} floor {}  {}  hp {}/{}  gold {}", &m[2], &m[3], &m[4], &m[5], &m[6], &m[7])), "{line}");
                    acts += 1;
                } else if message.starts_with("run ") && message.contains(" over: ") {
                    let m = OVER!().captures(line).unwrap_or_else(|| panic!("{line}"));
                    assert!(matches!(&m[2], "win" | "loss"), "{line}");
                    assert_eq!(strftime(strptime(&m[1]).unwrap()), &m[1]);
                    overs += 1;
                } else if message.starts_with("power ") {
                    let m = re!(r" INFO power (\w+?)(?:_POWER)? \(on (.+?)\): (.*)$").captures(line).unwrap_or_else(|| panic!("{line}"));
                    assert!(!m[1].ends_with("_POWER"), "{line}");
                    powers += 1;
                } else if message.starts_with("boss fight over") {
                    assert!(re!(r"boss hp \d+ -> (\d+)").is_match(line) && (line.contains(" won;") || line.contains(" lost;")), "{line}");
                }
            }
        }
        assert!(acts > 50 && overs >= 4 && powers > 20, "{acts} {overs} {powers}");
    }

    #[test]
    fn dates() {
        assert_eq!(strftime(strptime("2026-11-21 13:49:57").unwrap() + 7), "2026-11-21 13:50:04");
        assert_eq!(strftime(strptime("2024-02-28 23:59:59").unwrap() + 1), "2024-02-29 00:00:00");
        assert_eq!(strftime(strptime("2026-01-01 00:00:01").unwrap() - 2), "2025-12-31 23:59:59");
        assert_eq!(strftime(strptime("2000-3-1 0:0:0").unwrap() - 1), "2000-02-29 23:59:59");
        assert!(strptime("2026-02-30 00:00:00").is_err());
        assert!(strptime("yesterday").is_err());
    }
}
