//! The decision trace: every seeded model run of tests/golden/trace.jsonl (played by the Python) must come out the
//! same here, decision for decision, and end the same way.

use indexmap::IndexMap;
use serde_json::Value;
use sts2bot::py::{self, JsonExt};
use sts2bot::sim::cli::trace_run;

const GOLDEN: &str = include_str!("../golden/trace.jsonl");

/// What differs between the run played here and the golden one, if anything.
fn compare(want: &Value) -> Option<String> {
    let (game, character, seed, asc) = (want["game"].s(), want["character"].s(), want["seed"].int(), want["ascension"].int());
    let who = format!("{game} {character} seed {seed} ascension {asc}");
    let (digest, result) = trace_run(game, character, seed, asc, &IndexMap::new());
    let golden: Vec<&str> = want["digest"].list().iter().map(|d| d.s()).collect();
    if golden.len() < 10 || digest.is_empty() {
        return Some(format!("{who}: {} decisions here, {} in the golden run", digest.len(), golden.len()));
    }
    for i in 0..digest.len().max(golden.len()) {
        let (got, wanted) = (digest.get(i).map(|d| d.as_str()), golden.get(i).copied());
        if got != wanted {
            let from = i.saturating_sub(4);
            let window = |d: &[&str]| d[from.min(d.len())..(i + 3).min(d.len())].join(" | ");
            let got_all: Vec<&str> = digest.iter().map(|d| d.as_str()).collect();
            return Some(format!("{who}: decision {i} of {} differs\n   got: {:?}\n  want: {:?}\n   got around: {}\n  want around: {}", golden.len(), got, wanted, window(&got_all), window(&golden)));
        }
    }
    // (everything a run returns but the wall-clock time it took)
    for (key, value) in want["result"].entries() {
        if key == "secs" {
            continue;
        }
        if py::json_dumps(&result[key.as_str()]) != py::json_dumps(value) {
            return Some(format!("{who}: result[{key}] differs\n   got: {}\n  want: {}", py::json_dumps(&result[key.as_str()]), py::json_dumps(value)));
        }
    }
    let keys = |v: &Value| v.entries().map(|(k, _)| k.clone()).collect::<Vec<_>>();
    if keys(&result) != keys(&want["result"]) {
        return Some(format!("{who}: the result's keys differ\n   got: {:?}\n  want: {:?}", keys(&result), keys(&want["result"])));
    }
    None
}

/// Every golden run of `game`, on a few threads; the runs that differ.
fn check(game: &str) {
    let lines: Vec<Value> = GOLDEN.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str::<Value>(l).unwrap()).filter(|v| v["game"].s() == game).collect();
    assert!(!lines.is_empty(), "no golden runs of {game}");
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(8);
    let per = lines.len().div_ceil(threads);
    let failures: Vec<String> = std::thread::scope(|s| {
        let handles: Vec<_> = lines.chunks(per).map(|chunk| s.spawn(move || chunk.iter().filter_map(compare).collect::<Vec<String>>())).collect();
        handles.into_iter().flat_map(|h| h.join().expect("a run panicked")).collect()
    });
    assert!(failures.is_empty(), "{} of {} runs differ:\n{}", failures.len(), lines.len(), failures.join("\n"));
}

#[test]
fn test_the_first_games_runs_are_the_pythons() {
    check("sts1");
}

/// More seeded runs of the first game than trace.jsonl holds (seeds 5..44 at Ascension 0 and at Ascension 15), kept as
/// the start of the SHA-256 of each run's digest and of its result.
fn more(every: usize) {
    use sha2::{Digest, Sha256};
    let hash = |text: &str| Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>()[..16].to_string();
    let lines: Vec<Value> = include_str!("../golden/trace_sts1_more.jsonl").lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str::<Value>(l).unwrap()).collect();
    assert!(lines.len() >= 300, "{} golden runs", lines.len());
    let lines: Vec<Value> = lines.into_iter().step_by(every).collect();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(8);
    let per = lines.len().div_ceil(threads);
    let one = |want: &Value| -> Option<String> {
        let (digest, mut result) = trace_run(want["game"].s(), want["character"].s(), want["seed"].int(), want["ascension"].int(), &IndexMap::new());
        result.as_object_mut().unwrap().shift_remove("secs");
        let got = (digest.len() as i64, hash(&digest.join("|")), hash(&py::json_dumps(&result)));
        let wanted = (want["n"].int(), want["digest"].s().to_string(), want["result"].s().to_string());
        (got != wanted).then(|| format!("{} {} seed {} ascension {}: got {:?}, want {:?}", want["game"].s(), want["character"].s(), want["seed"], want["ascension"], got, wanted))
    };
    let failures: Vec<String> = std::thread::scope(|s| {
        let handles: Vec<_> = lines.chunks(per).map(|chunk| s.spawn(move || chunk.iter().filter_map(one).collect::<Vec<String>>())).collect();
        handles.into_iter().flat_map(|h| h.join().expect("a run panicked")).collect()
    });
    assert!(failures.is_empty(), "{} of {} runs differ:\n{}", failures.len(), lines.len(), failures.join("\n"));
}

/// (One run in four, so that the tests stay quick: the next test plays them all.)
#[test]
fn test_more_of_the_first_games_runs_are_the_pythons() {
    more(4);
}

#[test]
#[ignore = "all 320 runs: half a minute or more (cargo test -- --ignored test_all_the_more_runs)"]
fn test_all_the_more_runs_of_the_first_game_are_the_pythons() {
    more(1);
}

#[test]
fn test_slay_the_spire_2s_runs_are_the_pythons() {
    check("sts2");
}
