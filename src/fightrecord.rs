//! One record per fight, kept for good in fights.jsonl (the log rotates away, and the run history only has a run's last deck): the
//! fight as it began (deck, relics, potions, HP) and as it ended. `scripts fightlog decks` reads it.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::fight::bossfight::mid_fight;
use crate::logging::Logger;
use crate::py::{self, JsonExt};

static LOG: Logger = Logger::new("sts2bot");

pub const COMBAT: [&str; 3] = ["monster", "elite", "boss"];
/// bytes before the file is set aside as fights.jsonl.1 (about ten days of play)
pub const LIMIT: i64 = 64 << 20;
pub const _PILES: [&str; 4] = ["hand", "draw_pile", "discard_pile", "exhaust_pile"];

/// (The Python's default: `field` "name".)
pub fn _names(player: &Value, key: &str, field: &str) -> Vec<String> {
    player[key].list().iter().map(|x| py::str_of(x[field].or(&x["name"]).or(&Value::String("?".into())))).collect()
}

/// `time.strftime("%Y-%m-%dT%H:%M:%S")`: the local time now.
pub fn _now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// An I/O error as Python words an OSError: "[Errno 2] No such file or directory: 'fights.jsonl'" (`named`: the path, as `repr` has it).
fn _os_error(e: &std::io::Error, named: &str) -> String {
    let said = e.to_string();
    match e.raw_os_error() {
        Some(code) => format!("[Errno {}] {}: {}", code, said.strip_suffix(&format!(" (os error {code})")).unwrap_or(&said), named),
        None => said,
    }
}

pub struct FightWatch {
    pub path: Option<PathBuf>,
    /// The fight being watched (the Python's dict, `where` as `[act, floor]`).
    pub fight: Option<Value>,
    /// The time a fight is stamped with as it begins (`_now`; a test's or a replay's own clock otherwise).
    pub now: Box<dyn Fn() -> String + Send>,
}

impl Default for FightWatch {
    fn default() -> Self {
        FightWatch::new(None)
    }
}

impl FightWatch {
    /// (The Python's default: no path, nothing written.)
    pub fn new(path: Option<PathBuf>) -> Self {
        FightWatch::with_clock(path, _now)
    }

    /// `new` with the clock that stamps the records.
    pub fn with_clock(path: Option<PathBuf>, now: impl Fn() -> String + Send + 'static) -> Self {
        FightWatch { path, fight: None, now: Box::new(now) }
    }

    /// Feed every state the bot reads. Returns the record of a fight that has just ended.
    pub fn update(&mut self, state: &Value, kind: Option<&str>) -> Option<Value> {
        if kind.is_some_and(|k| COMBAT.contains(&k)) && state["battle"].truthy() {
            let run = &state["run"];
            let r#where = json!([run["act"], run["floor"]]);
            let mut done = None;
            if self.fight.as_ref().is_some_and(|f| f["where"] != r#where) {
                done = Some(self._close(state, false)); // (one fight straight into the next: it was won)
            }
            self._seen(state, kind.unwrap_or(""), &r#where);
            return done;
        }
        if self.fight.is_none() || mid_fight(state, kind) {
            return None;
        }
        Some(self._close(state, kind == Some("game_over") && !state["game_over"]["victory"].truthy()))
    }

    pub fn _seen(&mut self, state: &Value, kind: &str, r#where: &Value) {
        let (battle, player, run) = (&state["battle"], &state["player"], &state["run"]);
        let foes: BTreeSet<String> = battle["enemies"]
            .list()
            .iter()
            .map(|e| crate::re!(r"_\d+(\n?)$").replace(&py::str_of(e["entity_id"].or(&e["name"]).or(&Value::String("?".into()))), "${1}").into_owned())
            .collect();
        let rnd = battle["round"].int();
        if self.fight.is_none() {
            if foes.is_empty() {
                return; // nothing on the field yet
            }
            let mut deck: Vec<String> = _PILES.iter().flat_map(|pile| _names(player, pile, "name")).collect();
            deck.sort();
            let mut f = Map::new();
            f.insert("where".into(), r#where.clone());
            f.insert("time".into(), json!((self.now)()));
            f.insert("character".into(), player["character"].clone());
            f.insert("ascension".into(), run["ascension"].clone());
            f.insert("act".into(), r#where[0].clone());
            f.insert("floor".into(), r#where[1].clone());
            f.insert("kind".into(), json!(kind));
            f.insert("enemies".into(), json!(foes));
            f.insert("count".into(), json!(battle["enemies"].list().len()));
            // False for a fight the bot came into half way (a restart): its HP and piles are not the fight's own.
            f.insert("whole".into(), json!(rnd <= 1 && !player["discard_pile"].truthy() && !player["exhaust_pile"].truthy()));
            f.insert("hp_in".into(), json!(player["hp"].int()));
            f.insert("max_hp".into(), json!(player["max_hp"].int()));
            f.insert("relics".into(), json!(_names(player, "relics", "id")));
            f.insert("potions".into(), json!(_names(player, "potions", "name")));
            f.insert("deck".into(), json!(deck));
            f.insert("rounds".into(), json!(0));
            self.fight = Some(Value::Object(f));
        }
        let f = self.fight.as_mut().expect("a fight is being watched");
        let mut enemies: BTreeSet<String> = f["enemies"].list().iter().map(|n| n.s().to_string()).collect();
        enemies.extend(foes);
        f["enemies"] = json!(enemies);
        f["rounds"] = json!(f["rounds"].int().max(rnd));
        f["hp"] = json!(player["hp"].int());
        f["potions_left"] = json!(player["potions"].list().len());
    }

    pub fn _close(&mut self, state: &Value, lost: bool) -> Value {
        let mut record = self.fight.take().expect("a fight is being watched");
        let f = record.as_object_mut().expect("the fight is a dict");
        let player = &state["player"];
        f.shift_remove("where");
        f.insert("died".into(), json!(lost));
        // After a fight won the next screen has the HP it left us on, what the fight's end gave back included.
        let hp_out = if lost {
            0
        } else if !player["hp"].is_null() {
            player["hp"].int()
        } else {
            f.shift_remove("hp").map(|hp| hp.int()).unwrap_or(0)
        };
        f.insert("hp_out".into(), json!(hp_out));
        f.shift_remove("hp");
        self._write(&record);
        record
    }

    pub fn _write(&self, record: &Value) {
        let Some(path) = &self.path else { return };
        // (each step with the path Python would name in its OSError)
        let written = (|| -> Result<(), (std::io::Error, String)> {
            let named = |at: &Path| py::str_repr(&at.to_string_lossy());
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| (e, named(parent)))?;
            }
            if path.exists() && std::fs::metadata(path).map_err(|e| (e, named(path)))?.len() as i64 > LIMIT {
                let mut aside = path.file_name().unwrap_or_default().to_os_string();
                aside.push(".1");
                let aside = path.with_file_name(aside);
                std::fs::rename(path, &aside).map_err(|e| (e, format!("{} -> {}", named(path), named(&aside))))?;
            }
            let mut out = std::fs::OpenOptions::new().append(true).create(true).open(path).map_err(|e| (e, named(path)))?;
            out.write_all((py::Dumps { separators: Some((",", ":")), ..Default::default() }.dumps(record) + "\n").as_bytes()).map_err(|e| (e, named(path)))
        })();
        if let Err((e, named)) = written {
            // a full disk must not stop the bot playing
            crate::log_warning!(LOG, "could not write {}: {}", path.display(), _os_error(&e, &named));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fight::bossfight::testing::sequences;
    use crate::fight::combat::testing::rows;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    fn compact(v: &Value) -> String {
        py::Dumps { separators: Some((",", ":")), ..Default::default() }.dumps(v)
    }

    #[test]
    fn golden_names() {
        let cases = rows(&[include_str!("../tests/golden/fight2/small.json")], "names");
        assert!(cases.len() >= 300);
        for rec in &cases {
            assert_eq!(json!(_names(&rec["p"], rec["key"].s(), rec["field"].s())), rec["r"], "_names {rec}");
        }
        assert_eq!(_names(&json!({"relics": [{"id": "", "name": 0}, {"id": 3}, {"name": "N"}]}), "relics", "id"), ["?", "3", "N"]);
    }

    #[test]
    fn golden_fight_watch() {
        let (g, seqs) = sequences();
        assert!(seqs.len() >= 45);
        let (mut calls, mut records) = (0, 0);
        for (n, seq) in seqs.iter().enumerate() {
            let _env = g.env(&seq["env"]);
            let d = tempfile::tempdir().unwrap();
            let path = d.path().join("logs").join("fights.jsonl");
            // The cases' clock: a second on for every time it is asked.
            let ticks = Arc::new(AtomicI64::new(0));
            let clock = ticks.clone();
            let mut watch = FightWatch::with_clock(Some(path.clone()), move || {
                let t = clock.fetch_add(1, Ordering::SeqCst) + 1;
                format!("2026-11-27T{:02}:{:02}:{:02}", t / 3600 % 24, t / 60 % 60, t % 60)
            });
            let lines = seq["lines"].list();
            for (i, call) in seq["calls"].list().iter().enumerate() {
                let (got, logged) = crate::logging::capture(|| watch.update(&call[0], call[1].as_str()));
                let want = &seq["r"][i][2];
                assert_eq!(got.as_ref().map(compact), want.as_i64().map(|at| lines[at as usize].s().to_string()), "sequence {n} ({}), call {i}: {}", seq["src"], call[1]);
                assert!(logged.is_empty(), "sequence {n}, call {i}: {logged:?}");
                calls += 1;
                records += got.is_some() as usize;
            }
            let written = std::fs::read_to_string(&path).unwrap_or_default();
            assert_eq!(written.lines().collect::<Vec<_>>(), lines.iter().map(|l| l.s()).collect::<Vec<_>>(), "sequence {n}");
            assert_eq!(ticks.load(Ordering::SeqCst) as usize, lines.len() + watch.fight.is_some() as usize, "sequence {n}: the clock is read once a fight");
        }
        assert!(calls > 4000 && records > 300, "{calls} calls, {records} records");
    }

    #[test]
    fn the_file_is_set_aside_when_full_and_a_failure_is_only_logged() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("fights.jsonl");
        std::fs::File::create(&path).unwrap().set_len(LIMIT as u64 + 1).unwrap();
        let watch = FightWatch::new(Some(path.clone()));
        watch._write(&json!({"a": [1, 2.5, "é"], "b": null}));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\":[1,2.5,\"\\u00e9\"],\"b\":null}\n");
        assert_eq!(std::fs::metadata(d.path().join("fights.jsonl.1")).unwrap().len() as i64, LIMIT + 1);
        watch._write(&json!({}));
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
        // A file where the directory should be: Python's "[Errno 17] File exists: '...'", and the bot plays on.
        let blocked = FightWatch::new(Some(path.join("logs").join("fights.jsonl")));
        let ((), logged) = crate::logging::capture(|| blocked._write(&json!({})));
        assert_eq!(logged.len(), 1);
        assert_eq!((logged[0].logger, logged[0].level.name()), ("sts2bot", "WARNING"));
        let below = path.join("logs");
        assert_eq!(logged[0].message, format!("could not write {}: [Errno 20] Not a directory: {}", below.join("fights.jsonl").display(), py::str_repr(&below.to_string_lossy())));
        assert_eq!(_now().len(), 19);
        assert_eq!((COMBAT, LIMIT, _PILES), (["monster", "elite", "boss"], 67108864, ["hand", "draw_pile", "discard_pile", "exhaust_pile"]));
    }
}
