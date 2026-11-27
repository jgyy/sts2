//! Work the bot's card record (card_stats.json) out again from runs.jsonl, after a change to learn::CardStats::record.
//!
//! ```text
//! scripts relearn              each kind of card's record, and the cards whose rating moves most
//! scripts relearn --write      ... and put the new record in place (the old one is kept as card_stats.json.old)
//! ```
//!
//! Card kinds (cards::kind) come from wiki.json (`scripts fightlog wiki`). Stop the bot before --write: it saves its own.

use std::path::PathBuf;

use serde_json::Value;

use super::fightlog::{io_error, log_path, read_jsonl, read_text};
use super::modelfight::{exit_code, out};
use crate::py::{self, JsonExt};
use crate::{cards, learn};

pub fn rebuilt() -> Result<learn::CardStats, String> {
    let mut stats = learn::CardStats::new(None);
    for r in read_jsonl(&log_path("runs.jsonl"))? {
        let floor = &r["floor"];
        if !(floor.is_int() || floor.is_boolean()) {
            return Err(format!("TypeError: unsupported operand type(s) for +: '{}' and 'int'", if floor.is_null() { "NoneType" } else { "str" }));
        }
        let mut picks: Vec<(i64, String)> = Vec::new();
        for p in r["picks"].list() {
            picks.push((p[0].int(), py::str_of(&p[1])));
        }
        stats.record(&r["character"].text(), floor.int(), r["result"] == "win", &picks);
    }
    let wiki = read_text(&log_path("wiki.json")).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).and_then(|w| w.get("cards").and_then(Value::as_object).cloned());
    let wiki = wiki.unwrap_or_else(|| {
        out!("no wiki.json: the kinds will fill in as the bot is offered each card (scripts fightlog wiki writes one)");
        Default::default()
    });
    for entry in wiki.values() {
        let base = if entry["base"].truthy() { entry["base"].clone() } else { Value::Object(Default::default()) };
        stats.kinds.insert(learn::card_key(&base["name"].text()), cards::kind(&base).to_string());
    }
    stats._pool = None;
    Ok(stats)
}

pub fn run(write: bool) -> Result<(), String> {
    let path = log_path("card_stats.json");
    let mut old = learn::CardStats::new(Some(PathBuf::from(&path)));
    let mut new = rebuilt()?;
    for kind in ["free", "energy", "draw"] {
        let start = new.kind_shift(kind, "");
        let rows: Vec<[f64; 2]> = new._pool.as_ref().and_then(|p| p.get(kind)).map(|t| t.values().copied().collect()).unwrap_or_default();
        let (picks, gained) = (py::fsum(rows.iter().map(|x| x[0])), py::fsum(rows.iter().map(|x| x[1])));
        out!(
            "{:7} {:3} cards, {:5.0} picks, {:+5.2} floors a pick beyond par: its cards start at {:+5.2}",
            kind,
            rows.len(),
            picks,
            gained / if picks > 1.0 { picks } else { 1.0 },
            start
        );
    }
    let mut whos: Vec<String> = new.cards.keys().cloned().collect();
    whos.sort();
    for who in whos {
        // (how far it moved, the card, its picks, the rating before and after, its kind)
        let mut moved: Vec<(f64, String, f64, f64, f64, String)> = Vec::new();
        let rows: Vec<(String, f64)> = new.cards[&who].iter().map(|(key, x)| (key.clone(), x[0])).collect();
        for (key, picks) in rows {
            let (was, now) = (old.shift(&who, &key, None), new.shift(&who, &key, None));
            moved.push((now - was, key.clone(), picks, was, now, new.kinds.get(&key).cloned().unwrap_or_default()));
        }
        py::sort_by_key(&mut moved, |m| m.clone());
        out!("{}: {:.0} picks, par {:+.2} floors a pick", who, py::fsum(new.cards[&who].values().map(|x| x[0])), new._par(&who));
        let ends = moved.iter().take(6).chain(moved.iter().skip(moved.len().saturating_sub(6)));
        for (_diff, key, picks, was, now, kind) in ends {
            out!("   {:24} {:4.0} picks  {:+5.2} -> {:+5.2}  {}", key, picks, was, now, kind);
        }
    }
    if write {
        if std::path::Path::new(&path).exists() {
            std::fs::rename(&path, format!("{path}.old")).map_err(|e| io_error(&e, &path))?;
        }
        new.path = Some(PathBuf::from(&path));
        new.save();
        out!("written: {path}");
    }
    Ok(())
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(run(args.iter().any(|a| a == "--write")))
}

#[cfg(test)]
mod tests {
    use super::super::fightlog::with_state;
    use super::super::modelfight::{capture, golden};
    use super::main;

    const STATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/logs/state");

    /// A copy of the state directory's files that the script reads (it writes there).
    fn copy(names: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            std::fs::copy(format!("{STATE}/{name}"), dir.path().join(name)).unwrap();
        }
        dir
    }

    #[test]
    fn as_python() {
        let dir = copy(&["runs.jsonl", "wiki.json", "card_stats.json"]);
        let _state = with_state(dir.path());
        golden(main, &[], include_str!("../../tests/golden/logs/relearn.txt"));
        assert!(!dir.path().join("card_stats.json.old").exists());
    }

    #[test]
    fn write_as_python() {
        let dir = copy(&["runs.jsonl", "wiki.json", "card_stats.json"]);
        let _state = with_state(dir.path());
        let (code, text) = capture(|| main(vec!["--write".to_string()]));
        assert_eq!(code, 0);
        assert_eq!(text.replace(&*dir.path().to_string_lossy(), "STATE"), include_str!("../../tests/golden/logs/relearn_write.txt"));
        assert_eq!(std::fs::read_to_string(dir.path().join("card_stats.json")).unwrap(), include_str!("../../tests/golden/logs/relearn_written.json"));
        assert_eq!(std::fs::read_to_string(dir.path().join("card_stats.json.old")).unwrap(), include_str!("../../tests/golden/logs/state/card_stats.json"));
    }

    #[test]
    fn without_a_wiki_as_python() {
        let dir = copy(&["runs.jsonl", "card_stats.json"]);
        let _state = with_state(dir.path());
        // (the one line that names the command to run, which is another here)
        let python = include_str!("../../tests/golden/logs/relearn_nowiki.txt");
        assert!(python.contains("(python3 scripts/fightlog.py wiki writes one)"));
        golden(main, &[], &python.replace("python3 scripts/fightlog.py wiki", "scripts fightlog wiki"));
    }
}
