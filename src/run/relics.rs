//! What the Ancients' relics have done in our own runs, kept as learn::CardStats keeps cards: how much further the runs that
//! held a relic got than runs of the same character and ascension usually get from the floor it was taken on moves its
//! events::_ANCIENT rating (`shift`). Nothing is kept on disk: the record is rebuilt from runs.jsonl at start, and each
//! finished run is added. When a relic was taken is read off the relic itself (each act opens with one Ancient).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use indexmap::IndexMap;
use serde_json::Value;

use crate::learn::history;
use crate::logging::Logger;
use crate::base::{self, JsonExt};

static LOG: Logger = Logger::new("sts2bot.relics");
pub const WIN_BONUS: i64 = 15; // as learn::WIN_BONUS: a win counts as this many floors beyond the last one
pub const PRIOR_RUNS: f64 = 25.0; // runs a relic needs before its record counts for half
pub const POINTS_PER_FLOOR: f64 = 1.0; // a floor further than usual, in the points of events::_ANCIENT (its ratings spread about as wide as the floors do)
pub const MAX_SHIFT: f64 = 3.0; // a rating never moves further than this
pub const FEW: i64 = 5; // runs of a character on an ascension band before "the usual" means anything
pub const BAND: i64 = 2; // ascension levels taken together for "the usual"

// The relics of the Ancients that open act 2 (Orobas, Pael, Tezcatara) and act 3 (Nonupeipe, Tanx, Vakuu), and Darv's (either); any other is Neow's.
pub static ACT2: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "ALCHEMICAL_COFFER", "ARCHAIC_TOOTH", "DRIFTWOOD", "ELECTRIC_SHRYMP", "GEAR_GLASS", "GLASS_EYE", "LICH_GLASS", "NOBLE_GLASS", "VENOM_GLASS", "PRISMATIC_GEM", "RADIANT_PEARL", "SAND_CASTLE", "PAELS_BLOOD", "PAELS_CLAW", "PAELS_EYE",
        "PAELS_FLESH", "PAELS_GROWTH", "PAELS_HORN", "PAELS_LEGION", "PAELS_TEARS", "PAELS_TOOTH", "PAELS_WING", "BIIIG_HUG", "GOLDEN_COMPASS", "NUTRITIOUS_SOUP", "PUMPKIN_CANDLE", "SEAL_OF_GOLD", "STORYBOOK", "TOASTY_MITTENS", "TOY_BOX",
        "VERY_HOT_COCOA", "YUMMY_COOKIE",
    ])
});
pub static ACT3: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "BEAUTIFUL_BRACELET", "BLESSED_ANTLER", "BRILLIANT_SCARF", "DELICATE_FROND", "DIAMOND_DIADEM", "FUR_COAT", "GLITTER", "JEWELRY_BOX", "LOOMING_FRUIT", "SIGNET_RING", "CLAWS", "CROSSBOW", "IRON_CLUB", "MEAT_CLEAVER", "SAI",
        "SPIKED_GAUNTLETS", "TANXS_WHISTLE", "THROWING_AXE", "TRI_BOOMERANG", "WAR_HAMMER", "BLOOD_SOAKED_ROSE", "CHOICES_PARADOX", "DISTINGUISHED_CAPE", "FIDDLE", "JEWELED_MASK", "LORDS_PARASOL", "MUSIC_BOX", "PRESERVED_FOG",
        "WHISPERING_EARRING",
    ])
});
pub static DARV: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "ASTROLABE", "BLACK_STAR", "CALLING_BELL", "DUSTY_TOME", "ECTOPLASM", "EMPTY_CAGE", "PHILOSOPHERS_STONE", "RUNIC_PYRAMID", "SNECKO_EYE", "SOZU", "VELVET_CHOKER",
    ])
});
pub const FLOOR2: i64 = 18; // the floors acts 2 and 3 open on
pub const FLOOR3: i64 = 34;

/// One finished run of the record: (character, ascension band, floors reached with the win bonus, relics held).
pub type Run = (String, i64, i64, BTreeSet<String>);

thread_local! {
    /// every relic an Ancient offers that we rate (events::_ANCIENT)
    pub static _known: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// (character, ascension band, floors reached with the win bonus, relics held)
    pub static _runs: RefCell<Vec<Run>> = const { RefCell::new(Vec::new()) };
    /// relic -> [runs that held it, floors beyond the usual in total]
    pub static _table: RefCell<IndexMap<String, [f64; 2]>> = RefCell::new(IndexMap::new());
}

/// The floor a run holding `held` took this Ancient's relic on.
pub fn picked_at(relic: &str, held: &BTreeSet<String>) -> i64 {
    if ACT3.contains(relic) {
        return FLOOR3;
    }
    if ACT2.contains(relic) {
        return FLOOR2;
    }
    if DARV.contains(relic) {
        return if held.iter().any(|r| ACT2.contains(r.as_str())) { FLOOR3 } else { FLOOR2 }; // (one Ancient opens each act: with another act 2 relic held, Darv was the third)
    }
    1
}

pub fn _rebuild() {
    _runs.with_borrow(|runs| {
        let mut by_group: IndexMap<(&str, i64), Vec<i64>> = IndexMap::new();
        for (who, band, r#final, _held) in runs {
            by_group.entry((who.as_str(), *band)).or_default().push(*r#final);
        }
        let mut usual: IndexMap<(&str, i64, i64), f64> = IndexMap::new(); // (character, band, floor) -> what such a run that got to that floor usually ends on
        for ((who, band), finals) in &by_group {
            for floor in [1, FLOOR2, FLOOR3] {
                let reached: Vec<i64> = finals.iter().copied().filter(|f| *f >= floor).collect();
                if reached.len() as i64 >= FEW {
                    usual.insert((who, *band, floor), reached.iter().sum::<i64>() as f64 / reached.len() as f64);
                }
            }
        }
        _known.with_borrow(|known| {
            _table.with_borrow_mut(|table| {
                table.clear();
                for (who, band, r#final, held) in runs {
                    // (the original walks `held & _known` in a set's order, which changes from one process to the next: here by name.)
                    for relic in held.iter().filter(|r| known.contains(r.as_str())) {
                        let floor = picked_at(relic, held);
                        let par = match usual.get(&(who.as_str(), *band, floor)) {
                            Some(par) if *r#final >= floor => *par,
                            _ => continue,
                        };
                        let entry = table.entry(relic.clone()).or_insert([0.0, 0.0]);
                        entry[0] += 1.0;
                        entry[1] += *r#final as f64 - par;
                    }
                }
            })
        })
    })
}

/// `int(x or 0)` as the original does it, with its ValueError and TypeError as an Err.
fn _int(x: &Value) -> Result<i64, String> {
    if !x.truthy() {
        return Ok(0);
    }
    match x {
        Value::Bool(b) => Ok(*b as i64),
        Value::Number(n) => Ok(n.as_i64().unwrap_or_else(|| n.as_f64().map(base::trunc).unwrap_or(0))),
        Value::String(s) => s.trim().parse::<i64>().map_err(|_| format!("invalid literal for int() with base 10: {}", base::str_repr(s))),
        other => Err(format!("int() argument must be a string, a bytes-like object or a real number, not {}", if other.is_array() { "'list'" } else { "'dict'" })),
    }
}

/// `note`, with what the original raises on a line it cannot read (a ValueError or TypeError) as an Err.
pub fn try_note(run: &Value, rebuild: bool) -> Result<(), String> {
    let (relics, floor) = (&run["relics"], &run["floor"]);
    if !crate::run::modes::standard(run) || !run["character"].truthy() || relics.is_null() || !(floor.is_int() || floor.is_boolean()) {
        return Ok(()); // (the first hundred runs were written down without their relics)
    }
    let r#final = floor.int() + if run["result"].as_str() == Some("win") { WIN_BONUS } else { 0 };
    // (Only the relics the record is about are kept, one shared copy of each name.)
    let names: Vec<String> = match relics {
        Value::Array(items) => items.iter().map(base::str_of).collect(),
        Value::String(s) => s.chars().map(|c| c.to_string()).collect(),
        Value::Object(m) => m.keys().cloned().collect(),
        other => return Err(format!("'{}' object is not iterable", if other.is_boolean() { "bool" } else if other.is_int() { "int" } else { "float" })),
    };
    let held: BTreeSet<String> = _known.with_borrow(|known| names.into_iter().filter(|r| known.contains(r) || ACT2.contains(r.as_str())).collect());
    let band = base::floordiv(_int(&run["ascension"])?, BAND);
    _runs.with_borrow_mut(|runs| runs.push((base::str_of(&run["character"]), band, r#final, held)));
    if rebuild {
        _rebuild();
    }
    Ok(())
}

/// Add a finished run (a line of runs.jsonl) to the record. (`rebuild`: true in the original when left out.)
pub fn note(run: &Value, rebuild: bool) {
    if let Err(e) = try_note(run, rebuild) {
        panic!("{e}");
    }
}

/// Start the record afresh from the run history (`known`: the relics that have a rating to shift).
pub fn load<I, S>(runs_file: Option<&Path>, known: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    _known.with_borrow_mut(|k| {
        k.clear();
        k.extend(known.into_iter().map(|s| s.as_ref().to_string()));
    });
    _runs.with_borrow_mut(|r| r.clear());
    _table.with_borrow_mut(|t| t.clear());
    for run in history(runs_file) {
        if try_note(&run, false).is_err() {
            continue;
        }
    }
    _rebuild();
    let keys: Vec<String> = _table.with_borrow(|t| t.keys().cloned().collect());
    if !keys.is_empty() {
        let mut moved: Vec<(f64, String)> = keys.into_iter().map(|k| (shift(&k), k)).collect();
        base::sort_by_key_rev(&mut moved, |m| m.clone());
        let show = |rows: &[(f64, String)]| rows.iter().map(|(s, k)| format!("{} {:+.1}", k, s)).collect::<Vec<_>>().join(", ");
        crate::log_info!(
            LOG,
            "Ancient relics by our own record ({} runs): up {}; down {}",
            _runs.with_borrow(|r| r.len()),
            show(&moved[..moved.len().min(5)]),
            show(&moved[moved.len().saturating_sub(5)..])
        );
    }
}

/// How far the record moves this relic's rating (0 for one never held).
pub fn shift(relic: &str) -> f64 {
    let [runs, gained] = _table.with_borrow(|t| t.get(relic).copied().unwrap_or([0.0, 0.0]));
    let moved = POINTS_PER_FLOOR * gained / (runs + PRIOR_RUNS);
    let capped = if MAX_SHIFT < moved { MAX_SHIFT } else { moved }; // min(MAX_SHIFT, moved)
    if capped > -MAX_SHIFT {
        capped
    } else {
        -MAX_SHIFT
    }
}

/// relic -> (runs that held it, floors beyond the usual a run), for scripts.
pub fn record() -> IndexMap<String, (i64, f64)> {
    _table.with_borrow(|t| t.iter().filter(|(_, [n, _])| *n != 0.0).map(|(k, [n, g])| (k.clone(), (base::trunc(*n), g / n))).collect())
}

#[cfg(test)]
mod tests {
    //! Replays tests/golden/relics.json: what the original tests and model runs did with the record, one stretch from each
    //! `load` to the next (recorded from relics.rs).
    use super::*;
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/relics.json")).unwrap();
        let sorted = |set: &HashSet<&'static str>| {
            let mut names: Vec<&str> = set.iter().copied().collect();
            names.sort();
            json!(names)
        };
        assert_eq!(sorted(&ACT2), g["ACT2"]);
        assert_eq!(sorted(&ACT3), g["ACT3"]);
        assert_eq!(sorted(&DARV), g["DARV"]);
        assert_eq!(json!([WIN_BONUS, PRIOR_RUNS, POINTS_PER_FLOOR, MAX_SHIFT, FEW, BAND, FLOOR2, FLOOR3]), g["consts"]);
        let mut counts = [0; 5];
        for (s, stretch) in g["stretches"].as_array().unwrap().iter().enumerate() {
            for (n, e) in stretch.as_array().unwrap().iter().enumerate() {
                let why = format!("stretch {s} event {n} {e}");
                match e[0].as_str().unwrap() {
                    "load" => {
                        let known: Vec<&str> = e[2].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
                        match e[1].as_str() {
                            Some(content) => {
                                let mut file = tempfile::NamedTempFile::new().unwrap();
                                file.write_all(content.as_bytes()).unwrap();
                                file.flush().unwrap();
                                load(Some(file.path()), known);
                            }
                            None => load(Some(Path::new("/nonexistent/runs.jsonl")), known),
                        }
                        counts[0] += 1;
                    }
                    "note" => {
                        assert_eq!(try_note(&e[1], e[2].as_bool().unwrap()).is_err(), !e[3].is_null(), "{why}");
                        counts[1] += 1;
                    }
                    "shift" => {
                        assert_eq!(shift(e[1].as_str().unwrap()), e[2].as_f64().unwrap(), "{why}");
                        counts[2] += 1;
                    }
                    "record" => {
                        let got = record();
                        let want = e[1].as_object().unwrap();
                        assert_eq!(got.len(), want.len(), "{why}");
                        for (k, (runs, gained)) in &got {
                            assert_eq!(json!([runs, gained]), want[k], "{why}");
                        }
                        counts[3] += 1;
                    }
                    "picked_at" => {
                        let held: BTreeSet<String> = e[2].as_array().unwrap().iter().map(|k| k.as_str().unwrap().to_string()).collect();
                        assert_eq!(picked_at(e[1].as_str().unwrap(), &held), e[3].as_i64().unwrap(), "{why}");
                        counts[4] += 1;
                    }
                    other => panic!("no replay for {other}"),
                }
            }
        }
        load(None, Vec::<String>::new());
        assert!(counts[0] > 20 && counts[1] > 200 && counts[2] > 90 && counts[3] >= 2 && counts[4] > 300, "{counts:?}");
    }

    #[test]
    fn the_record_is_told_when_it_moves_something() {
        let run = |floor: i64, relic: &str| json!({"character": "The Ironclad", "ascension": 0, "floor": floor, "result": "loss", "relics": [relic]});
        let mut lines = String::new();
        for _ in 0..30 {
            lines += &(base::json_dumps(&run(40, "STONE_HUMIDIFIER")) + "\n" + &base::json_dumps(&run(20, "BOOMING_CONCH")) + "\n");
        }
        lines += "not json\n[1, 2]\n";
        lines += &(base::json_dumps(&json!({"character": "The Ironclad", "floor": 3, "relics": 7})) + "\n");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(lines.as_bytes()).unwrap();
        file.flush().unwrap();
        let ((), logged) = crate::logging::capture(|| load(Some(file.path()), ["STONE_HUMIDIFIER", "BOOMING_CONCH"]));
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].message, "Ancient relics by our own record (60 runs): up STONE_HUMIDIFIER +3.0, BOOMING_CONCH -3.0; down STONE_HUMIDIFIER +3.0, BOOMING_CONCH -3.0");
        assert_eq!(record()["STONE_HUMIDIFIER"], (30, 10.0));
        load(None, Vec::<String>::new());
        assert!(record().is_empty());
    }
}
