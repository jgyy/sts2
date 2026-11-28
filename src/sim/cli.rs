//! `sim`: play model runs, report on them, or print a digest of every decision.
//!
//! ```text
//!     sim run --game sts1 --character IRONCLAD --runs 1000 --jobs 4 --out sim-runs.jsonl
//!     sim run --game sts2 --character all --runs 1000 --vary      # each run draws its own value of every setting
//!     sim report sim-runs.jsonl [--md docs/strategy/simulation.md]
//!     sim trace --game sts1 --runs 2                             # one hash per character: changes iff a decision does
//!     sim prior sts2.jsonl sts1.jsonl --out src/run/model_cards.json   # the card table `model_lean` weighs in
//!     sim prior runs.jsonl --onto src/run/model_cards.json --out new.json  # ... plus what runs that played it still say
//!     sim run --game sts2 --vary --only model_table,caution --alt-cards new.json   # two tables (and caution) drawn per run
//!     sim run --game sts1 --vary --only caution,belt_spare --grid caution=1,1.35       # ... caution from these two values only
//! ```
//!
//! How this differs from the original: the worker processes of `sim run` (`multiprocessing.Pool`) are threads, each
//! with its own game, settings and card table; a setting's value is kept as the JSON number it was given as
//! (`--settings` and `model_table` may be whole numbers, and a run's line prints them as the original does).

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::cards::key_of;
use crate::games;
use crate::learn;
use crate::logging;
use crate::base::{self, JsonExt, Random};
use crate::sim::content::{self, Content};
use crate::sim::report;
use crate::sim::run::Run;

/// The settings tried with --vary: the bot's own choices (learn.CHOICES) and a value on either side where that is cheap.
pub fn GRID_DEFAULT() -> IndexMap<String, Vec<Value>> {
    let rows: [(&str, &[f64]); 14] = [
        ("caution", &[0.75, 1.0, 1.35]),
        ("elite_worth", &[0.8, 2.5, 6.0]),
        ("upgrade_worth", &[1.3, 2.5, 4.0]),
        ("pickiness", &[-1.0, 0.0, 1.0]),
        ("crowd", &[0.0, 0.3, 0.6]),
        ("punch", &[0.9, 2.7]),
        ("weak_want", &[0.0, 1.0]),
        ("free_lean", &[0.0, 1.0]),
        ("attack_room", &[3.0, 99.0]),
        ("block_want", &[0.0, 1.5]),
        ("cards_first", &[0.0, 1.0]),
        ("debuff_worth", &[1.0, 2.0, 3.0]),
        ("model_lean", &[0.0, 1.0, 2.0]),
        ("belt_spare", &[0.0, 1.0]), // a full belt is a reason to drink in a hallway fight (round 5: 0 in Slay the Spire 2, 1 in the first game)
    ];
    rows.iter().map(|(k, v)| (k.to_string(), v.iter().map(|x| json!(x)).collect())).collect()
}

/// model_table setting -> the card table learn::model_shift reads (1, 2, ...: --alt-cards).
pub type Tables = IndexMap<i64, Arc<Value>>;

thread_local! {
    // (the original's module-level GRID and TABLES, which `sim run` changes: per thread here)
    static GRID_NOW: RefCell<IndexMap<String, Vec<Value>>> = RefCell::new(GRID_DEFAULT());
    static TABLES_NOW: RefCell<Tables> = RefCell::new(IndexMap::new());
    static PANIC: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

/// GRID as it stands on this thread (a copy).
pub fn GRID() -> IndexMap<String, Vec<Value>> {
    GRID_NOW.with(|g| g.borrow().clone())
}

/// TABLES as it stands on this thread (a copy; the tables themselves are shared).
pub fn TABLES() -> Tables {
    TABLES_NOW.with(|t| t.borrow().clone())
}

/// The game's content, with the game made the one being played (on this thread).
pub fn content(game: &str) -> Rc<Content> {
    games::select(game).unwrap_or_else(|e| panic!("{e}"));
    content::content(game)
}

/// One seeded run as `sim trace` plays it (`Run(content(game), character, seed, knobs, ascension).play()`):
/// the digest of every decision, and the run's result.
pub fn trace_run(game: &str, character: &str, seed: i64, ascension: i64, knobs: &IndexMap<String, f64>) -> (Vec<String>, Value) {
    crate::logging::disable(Some(crate::logging::Level::Critical));
    let mut run = Run::new(&content(game), character, seed, Some(knobs), ascension);
    let result = run.play();
    crate::logging::disable(None);
    (std::mem::take(&mut run.digest), result)
}

/// One run of `sim run`: (game, character, seed, knobs, ascension).
pub type Job = (String, String, i64, Map<String, Value>, i64);

/// A run's settings as the bot reads them (numbers).
pub fn floats(knobs: &Map<String, Value>) -> IndexMap<String, f64> {
    knobs.iter().map(|(k, v)| (k.clone(), v.float())).collect()
}

/// `_job(args)`: the run's result, or what went wrong in it (`tables`: the original's TABLES, which its workers inherit).
pub fn _job(args: &Job, tables: &Tables) -> Value {
    let (game, character, seed, knobs, asc) = args;
    logging::disable(Some(logging::Level::Critical));
    if !tables.is_empty() {
        let n = knobs.get("model_table").map_or(0, |v| base::trunc(v.float()));
        learn::set_model_shared(Some(tables.get(&n).unwrap_or_else(|| panic!("KeyError: {n}")).clone()));
    }
    let played = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Run::new(&content(game), character, *seed, Some(&floats(knobs)), *asc).play()));
    match played {
        Ok(mut result) => {
            // (the settings as they were given: a whole number stays one)
            result["knobs"] = Value::Object(knobs.clone());
            if let Some(lean) = knobs.get("model_lean") {
                result["model_lean"] = lean.clone();
            }
            result
        }
        Err(payload) => {
            // a model bug must not end a thousand-run batch: it is counted and reported instead
            let (message, place) = PANIC.with(|p| p.borrow_mut().take()).unwrap_or_else(|| {
                let text = payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                (text, String::new())
            });
            json!({"game": game, "character": character, "seed": seed, "knobs": knobs, "error": message, "trace": place})
        }
    }
}

/// `knobs_for(seed, vary)`: the run's own value of every setting in GRID.
pub fn knobs_for(seed: i64, vary: bool) -> Map<String, Value> {
    if !vary {
        return Map::new();
    }
    let mut rng = Random::new(seed as i128 * 7919 + 17);
    GRID_NOW.with(|g| g.borrow().iter().map(|(k, v)| (k.clone(), rng.choice(v).clone())).collect())
}

/// Settings given on the command line (--set NAME=VALUE) and, per character, in a JSON file (--settings).
pub fn fixed(a: &Args, character: &str) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    if let Some(path) = &a.settings {
        let text = std::fs::read_to_string(path).map_err(|e| format!("FileNotFoundError: {path}: {e}"))?;
        let table: Value = serde_json::from_str(&text).map_err(|e| format!("json.decoder.JSONDecodeError: {path}: {e}"))?;
        if let Some(own) = table.dict().and_then(|t| t.get(character)).and_then(|own| own.dict()) {
            for (k, v) in own {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    for item in &a.set {
        let (name, value) = item.split_once('=').ok_or_else(|| format!("ValueError: not enough values to unpack (expected 2, got 1): {item}"))?;
        out.insert(name.to_string(), json!(float(value)?));
    }
    Ok(out)
}

/// `float(text)`.
fn float(text: &str) -> Result<f64, String> {
    let t = text.trim();
    if t.is_empty() || t.contains('_') {
        return Err(format!("ValueError: could not convert string to float: {}", base::str_repr(text)));
    }
    t.parse().map_err(|_| format!("ValueError: could not convert string to float: {}", base::str_repr(text)))
}

fn read_json(path: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("FileNotFoundError: {path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("json.decoder.JSONDecodeError: {path}: {e}"))
}

pub fn cmd_run(a: &Args) -> Result<i32, String> {
    let game = content(&a.game);
    if let Some(alt) = &a.alt_cards {
        // a second card table to set against the shipped one: `--set model_table=1`, or drawn per run with --vary
        let mut tables: Tables = IndexMap::new();
        tables.insert(0, Arc::new(Value::Object(learn::load_model_cards())));
        for (n, path) in alt.split(',').enumerate() {
            tables.insert(n as i64 + 1, Arc::new(read_json(path)?));
        }
        let mut numbers: Vec<i64> = tables.keys().copied().collect();
        numbers.sort();
        TABLES_NOW.with(|t| t.borrow_mut().extend(tables));
        GRID_NOW.with(|g| g.borrow_mut().insert("model_table".to_string(), numbers.into_iter().map(|n| json!(n)).collect()));
    }
    if let Some(only) = a.only.as_deref().filter(|o| !o.is_empty()) {
        // --vary over these settings alone, the rest at each game's defaults
        let keep: Vec<&str> = only.split(',').collect();
        GRID_NOW.with(|g| g.borrow_mut().retain(|name, _| keep.contains(&name.as_str())));
    }
    for item in &a.grid {
        // --grid NAME=V1,V2: the values --vary draws this setting from
        let (name, values) = item.split_once('=').ok_or_else(|| format!("ValueError: not enough values to unpack (expected 2, got 1): {item}"))?;
        let values = values.split(',').map(|v| float(v).map(|x| json!(x))).collect::<Result<Vec<Value>, String>>()?;
        GRID_NOW.with(|g| g.borrow_mut().insert(name.to_string(), values));
    }
    let chars: Vec<String> = if a.character == "all" { game.CHARACTERS.keys().cloned().collect() } else { a.character.split(',').map(|c| c.trim().to_uppercase()).collect() };
    // (Interleaved, so that every character's numbers grow together; seeds are shared by characters and arms.)
    let mut jobs: Vec<Job> = Vec::new();
    for n in 0..a.runs.max(0) {
        for ch in &chars {
            let mut knobs = knobs_for(a.seed + n, a.vary);
            for (k, v) in fixed(a, ch)? {
                knobs.insert(k, v);
            }
            jobs.push((a.game.clone(), ch.clone(), a.seed + n, knobs, a.ascension));
        }
    }
    let mut out: Box<dyn Write> = match &a.out {
        Some(path) => Box::new(std::fs::OpenOptions::new().append(true).create(true).open(path).map_err(|e| format!("{path}: {e}"))?),
        None => Box::new(std::io::stdout()),
    };
    if a.jobs < 1 {
        return Err("ValueError: Number of processes must be at least 1".to_string());
    }
    let (t0, mut done, mut errors) = (Instant::now(), 0, 0);
    let tables = TABLES();
    let next = AtomicUsize::new(0);
    // (A panic in a run is that run's error line, not a message on stderr.)
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        let message = info.payload().downcast_ref::<String>().cloned().or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        let place = info.location().map_or(String::new(), |l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        PANIC.with(|p| *p.borrow_mut() = Some((message, place)));
    }));
    let failed: Option<String> = std::thread::scope(|scope| {
        let (send, receive) = mpsc::channel::<Value>();
        for _ in 0..(a.jobs as usize).min(jobs.len().max(1)) {
            let (send, jobs, tables, next) = (send.clone(), &jobs, &tables, &next);
            let worker = move || {
                learn::update_knobs(&learn::DEFAULTS.iter().map(|(k, v)| (k.to_string(), *v)).collect());
                loop {
                    let at = next.fetch_add(1, Ordering::SeqCst);
                    if at >= jobs.len() || send.send(_job(&jobs[at], tables)).is_err() {
                        return;
                    }
                }
            };
            std::thread::Builder::new().stack_size(64 << 20).spawn_scoped(scope, worker).expect("a worker thread");
        }
        drop(send);
        for r in receive {
            done += 1;
            errors += r.has("error") as i64;
            if let Err(e) = out.write_all((base::json_dumps(&r) + "\n").as_bytes()).and_then(|_| out.flush()) {
                next.store(jobs.len(), Ordering::SeqCst);
                return Some(format!("{e}"));
            }
            if a.out.is_some() && (done % 50 == 0 || done == jobs.len()) {
                eprintln!("{done}/{} runs, {errors} errors, {:.0}s", jobs.len(), t0.elapsed().as_secs_f64());
            }
        }
        None
    });
    std::panic::set_hook(hook);
    match failed {
        Some(error) => Err(error),
        None => Ok(0),
    }
}

/// A hash of every decision in a few seeded runs per character: equal before and after a change iff the bot chose alike.
pub fn cmd_trace(a: &Args) -> Result<i32, String> {
    logging::disable(Some(logging::Level::Critical));
    for game in a.game.split(',') {
        let g = content(game);
        for ch in g.CHARACTERS.keys() {
            let mut h = Sha256::new();
            for n in 0..a.runs {
                let mut r = Run::new(&g, ch, a.seed + n, Some(&IndexMap::new()), a.ascension);
                let result = r.play();
                h.update((r.digest.join("|") + &base::json_dumps(&result["deck"])).as_bytes());
            }
            let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
            println!("{game} {ch:12} {}", &hex[..16]);
        }
    }
    Ok(0)
}

/// learn::MODEL_CARDS from run files: per game and character, the floors each card's picks gained over the runs that left the same
/// floor alive (report::card_table's measure), less the character's average pick, shrunk by `--prior` picks and capped. Runs that
/// played with the table (model_lean) are left out, so that it never learns from itself.
pub fn cmd_prior(a: &Args) -> Result<i32, String> {
    let mut groups: IndexMap<(String, String), Vec<Value>> = IndexMap::new();
    let base: Option<Value> = match &a.onto {
        Some(path) => Some(read_json(path)?),
        None => None,
    };
    let defaults = learn::game_defaults();
    for r in report::load(&a.files) {
        let lean = match r.dict().and_then(|d| d.get("model_lean")) {
            Some(lean) => lean.clone(),
            None => match r["knobs"].dict().and_then(|k| k.get("model_lean")) {
                Some(lean) => lean.clone(),
                None => json!(0.0),
            },
        };
        let keep = if base.is_some() {
            // --onto: only runs that played the base table at its game's own lean (learn::GAME_DEFAULTS)
            lean.truthy()
                && defaults.get(r["game"].s()).and_then(|d| d.get("model_lean")).is_some_and(|own| lean.as_f64() == Some(*own))
                && r["knobs"].dict().and_then(|k| k.get("model_table")).map_or(Some(0.0), |t| t.as_f64()) == Some(a.table as f64)
        } else {
            !lean.truthy() // (not runs that leaned on the table)
        };
        if !r.has("error") && keep {
            groups.entry((r["game"].s().to_string(), r["character"].s().to_string())).or_default().push(r);
        }
    }
    let mut out: Map<String, Value> = match &base {
        Some(Value::Object(base)) => base.clone(),
        _ => Map::new(),
    };
    groups.sort_keys();
    for ((game, ch), runs) in &groups {
        let g = content(game);
        let mut usual: HashMap<u64, f64> = HashMap::new();
        let mut gained: IndexMap<String, Vec<f64>> = IndexMap::new();
        for r in runs {
            for pick in r["picks"].list() {
                let (floor, key) = (pick[0].float(), base::str_of(&pick[1]));
                let at = (floor + 0.0).to_bits();
                let par = *usual.entry(at).or_insert_with(|| {
                    let alive: Vec<i64> = runs.iter().filter(|x| x["floor"].float() > floor || x["won"].truthy()).map(report::score).collect();
                    alive.iter().sum::<i64>() as f64 / alive.len().max(1) as f64
                });
                gained.entry(key).or_default().push(report::score(r) as f64 - par);
            }
        }
        let picks: usize = gained.values().map(|g| g.len()).sum();
        let par = base::fsum(gained.values().map(|g| base::fsum(g.iter().copied()))) / picks.max(1) as f64;
        let who = learn::who_key(&g.CHARACTERS.get(ch).unwrap_or_else(|| panic!("KeyError: {ch}")).title);
        // On top of a base table (--onto), what the runs that leaned on it still say: a card whose picks still gained moves up.
        let mut table: IndexMap<String, Value> = match base.as_ref().map(|b| &b[game.as_str()][who.as_str()]) {
            Some(Value::Object(table)) => table.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            _ => IndexMap::new(),
        };
        for (key, gains) in &gained {
            let name = key_of(g.CARDS.get(key).map_or(key.as_str(), |d| d.name.as_str()));
            let said = table.get(&name).map_or(0.0, |v| v.float()) + (base::fsum(gains.iter().copied()) - par * gains.len() as f64) / (gains.len() as f64 + a.prior);
            // (`max(-most, min(most, said))`)
            let capped = if said < a.most { said } else { a.most };
            let shift = if capped > -a.most { capped } else { -a.most };
            table.insert(name, json!(base::round_to(shift, 2)));
        }
        let mut kept: Vec<(&String, &Value)> = table.iter().filter(|(_, v)| v.float().abs() >= 0.05).collect();
        kept.sort_by(|x, y| x.0.cmp(y.0));
        let kept: Map<String, Value> = kept.into_iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let slot = out.entry(game.clone()).or_insert_with(|| json!({}));
        match slot.as_object_mut() {
            Some(by_character) => {
                by_character.insert(who, Value::Object(kept));
            }
            None => return Err(format!("TypeError: the table's {game} is not a dict")),
        }
        eprintln!("{game} {ch}: {} runs, {picks} picks, {} cards", runs.len(), table.len());
    }
    let text = base::json_dumps_pretty(&Value::Object(out), 1, true) + "\n";
    match &a.out {
        Some(path) => std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?,
        None => {
            let mut stdout = std::io::stdout();
            stdout.write_all(text.as_bytes()).and_then(|_| stdout.flush()).map_err(|e| format!("{e}"))?;
        }
    }
    Ok(0)
}

pub fn cmd_report(a: &Args) -> Result<i32, String> {
    let text = report::report(&a.files, a.min_runs.max(0) as usize);
    if let Some(md) = &a.md {
        std::fs::write(md, &text).map_err(|e| format!("{md}: {e}"))?;
    }
    println!("{text}");
    Ok(0)
}

// ---------------------------------------------------------------- the arguments

/// What the command line says (the original's argparse namespace: every command's arguments, each at its default).
#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub cmd: String,
    pub game: String,
    pub character: String,
    pub runs: i64,
    pub seed: i64,
    pub ascension: i64,
    pub vary: bool,
    pub only: Option<String>,
    pub grid: Vec<String>,
    pub set: Vec<String>,
    pub settings: Option<String>,
    pub alt_cards: Option<String>,
    pub jobs: i64,
    pub out: Option<String>,
    pub files: Vec<String>,
    pub md: Option<String>,
    pub min_runs: i64,
    pub prior: f64,
    pub most: f64,
    pub onto: Option<String>,
    pub table: i64,
}

impl Args {
    /// The defaults of a command's arguments.
    pub fn new(cmd: &str) -> Args {
        Args {
            cmd: cmd.to_string(),
            game: if cmd == "trace" { "sts1,sts2" } else { "sts1" }.to_string(),
            character: "all".to_string(),
            runs: if cmd == "trace" { 2 } else { 100 },
            seed: 1,
            ascension: 0,
            vary: false,
            only: None,
            grid: Vec::new(),
            set: Vec::new(),
            settings: None,
            alt_cards: None,
            jobs: std::thread::available_parallelism().map(|n| n.get() as i64).unwrap_or(2),
            out: None,
            files: Vec::new(),
            md: None,
            min_runs: 30,
            prior: 40.0,
            most: 3.0,
            onto: None,
            table: 0,
        }
    }
}

const USAGE: &str = "usage: sim [-h] {run,trace,report,prior} ...";

const DESCRIPTION: &str = "`sim`: play model runs, report on them, or print a digest of every decision.

sim run --game sts1 --character IRONCLAD --runs 1000 --jobs 4 --out sim-runs.jsonl
sim run --game sts2 --character all --runs 1000 --vary      # each run draws its own value of every setting
sim report sim-runs.jsonl [--md docs/strategy/simulation.md]
sim trace --game sts1 --runs 2                             # one hash per character: changes iff a decision does
sim prior sts2.jsonl sts1.jsonl --out src/run/model_cards.json   # the card table `model_lean` weighs in
sim prior runs.jsonl --onto src/run/model_cards.json --out new.json  # ... plus what runs that played it still say
sim run --game sts2 --vary --only model_table,caution --alt-cards new.json   # two tables (and caution) drawn per run
sim run --game sts1 --vary --only caution,belt_spare --grid caution=1,1.35       # ... caution from these two values only

positional arguments:
  {run,trace,report,prior}
    run                 play model runs and write one JSON line per run
    trace               hash every decision of a few seeded runs (a refactor
                        must leave these unchanged)
    report              summarise run files
    prior               the model's card table the bot reads
                        (learn::MODEL_CARDS) from run files

options:
  -h, --help            show this help message and exit";

/// A command's usage line and the rest of its help.
fn help_of(cmd: &str) -> (&'static str, &'static str) {
    match cmd {
        "run" => (
            "usage: sim run [-h] [--game {sts2,sts1}] [--character CHARACTER] [--runs RUNS]
               [--seed SEED] [--ascension ASCENSION] [--vary] [--only NAMES]
               [--grid NAME=V1,V2] [--set NAME=VALUE] [--settings FILE]
               [--alt-cards FILE] [--jobs JOBS] [--out OUT]",
            "options:
  -h, --help            show this help message and exit
  --game {sts2,sts1}
  --character CHARACTER
                        a character id, several joined by commas, or all
  --runs RUNS           runs per character
  --seed SEED
  --ascension ASCENSION
  --vary                each run draws its own value of every setting in GRID
  --only NAMES          with --vary: vary only these settings (comma-
                        separated)
  --grid NAME=V1,V2     with --vary: draw this setting from these values
                        (repeatable)
  --set NAME=VALUE      play every run with this setting (repeatable)
  --settings FILE       JSON {character: {setting: value}} to play each
                        character with
  --alt-cards FILE      another card table (sim prior) for runs with
                        model_table 1 (several, comma-separated: 1, 2, ...)
  --jobs JOBS
  --out OUT",
        ),
        "trace" => (
            "usage: sim trace [-h] [--game GAME] [--runs RUNS] [--seed SEED]
                 [--ascension ASCENSION]",
            "options:
  -h, --help            show this help message and exit
  --game GAME
  --runs RUNS
  --seed SEED
  --ascension ASCENSION",
        ),
        "report" => (
            "usage: sim report [-h] [--md MD] [--min-runs MIN_RUNS] files [files ...]",
            "positional arguments:
  files

options:
  -h, --help           show this help message and exit
  --md MD
  --min-runs MIN_RUNS",
        ),
        _ => (
            "usage: sim prior [-h] [--out OUT] [--prior PRIOR] [--most MOST] [--onto TABLE]
                 [--table TABLE]
                 files [files ...]",
            "positional arguments:
  files

options:
  -h, --help     show this help message and exit
  --out OUT
  --prior PRIOR  picks before a card's own record counts for half
  --most MOST    no card moves further than this
  --onto TABLE   add to this table what the runs that played it (at the game's
                 default model_lean) still say
  --table TABLE  with --onto: the runs whose model_table was this (1: run with
                 --alt-cards TABLE)",
        ),
    }
}

/// A command's options: (name, takes a value).
fn options_of(cmd: &str) -> &'static [(&'static str, bool)] {
    match cmd {
        "run" => &[
            ("--game", true), ("--character", true), ("--runs", true), ("--seed", true), ("--ascension", true), ("--vary", false), ("--only", true),
            ("--grid", true), ("--set", true), ("--settings", true), ("--alt-cards", true), ("--jobs", true), ("--out", true), ("--help", false),
        ],
        "trace" => &[("--game", true), ("--runs", true), ("--seed", true), ("--ascension", true), ("--help", false)],
        "report" => &[("--md", true), ("--min-runs", true), ("--help", false)],
        _ => &[("--out", true), ("--prior", true), ("--most", true), ("--onto", true), ("--table", true), ("--help", false)],
    }
}

/// What argparse does with the command line: the arguments, or the exit code after it printed help (0) or an error (2).
pub fn parse_args(argv: &[String]) -> Result<Args, i32> {
    let wrong = |usage: &str, prog: &str, message: String| -> Result<Args, i32> {
        eprintln!("{usage}\n{prog}: error: {message}");
        Err(2)
    };
    const COMMANDS: [&str; 4] = ["run", "trace", "report", "prior"];
    // (the main parser: its own options, then the command)
    let mut at = 0;
    let mut unknown: Vec<String> = Vec::new();
    while at < argv.len() && argv[at].starts_with('-') && argv[at].len() > 1 {
        if argv[at] == "-h" || (argv[at].len() > 2 && "--help".starts_with(argv[at].as_str())) {
            println!("{USAGE}\n\n{DESCRIPTION}");
            return Err(0);
        }
        unknown.push(argv[at].clone());
        at += 1;
    }
    let Some(cmd) = argv.get(at) else {
        return wrong(USAGE, "sim", "the following arguments are required: cmd".to_string());
    };
    if !COMMANDS.contains(&cmd.as_str()) {
        return wrong(USAGE, "sim", format!("argument cmd: invalid choice: {} (choose from 'run', 'trace', 'report', 'prior')", base::str_repr(cmd)));
    }
    let prog = format!("sim {cmd}");
    let (usage, rest) = help_of(cmd);
    let options = options_of(cmd);
    let takes_files = matches!(cmd.as_str(), "report" | "prior");
    let mut a = Args::new(cmd);
    let negative = |s: &str| s.len() > 1 && s.starts_with('-') && s[1..].chars().all(|c| c.is_ascii_digit() || c == '.');
    let int = |name: &str, text: &str| -> Result<i64, String> { text.trim().parse::<i64>().map_err(|_| format!("argument {name}: invalid int value: {}", base::str_repr(text))) };
    let mut i = at + 1;
    let mut only_files = false;
    while i < argv.len() {
        let arg = argv[i].clone();
        i += 1;
        if !only_files && arg == "--" {
            only_files = true;
            continue;
        }
        if only_files || !arg.starts_with('-') || arg == "-" || negative(&arg) {
            if takes_files {
                a.files.push(arg);
            } else {
                unknown.push(arg);
            }
            continue;
        }
        if arg == "-h" {
            println!("{usage}\n\n{rest}");
            return Err(0);
        }
        let (flag, mut inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
            None => (arg.clone(), None),
        };
        // (argparse takes any prefix of an option that names one of them alone)
        let found: Vec<&(&str, bool)> = match options.iter().find(|(name, _)| *name == flag) {
            Some(exact) => vec![exact],
            None if flag.starts_with("--") && flag.len() > 2 => options.iter().filter(|(name, _)| name.starts_with(flag.as_str())).collect(),
            None => vec![],
        };
        if found.len() > 1 {
            let names: Vec<&str> = found.iter().map(|(name, _)| *name).collect();
            return wrong(usage, &prog, format!("ambiguous option: {flag} could match {}", names.join(", ")));
        }
        let Some((name, takes_value)) = found.first().map(|o| **o) else {
            unknown.push(arg);
            continue;
        };
        if name == "--help" {
            println!("{usage}\n\n{rest}");
            return Err(0);
        }
        if !takes_value {
            if inline.is_some() {
                return wrong(usage, &prog, format!("argument {name}: ignored explicit argument {}", base::str_repr(&inline.take().unwrap_or_default())));
            }
            a.vary = true;
            continue;
        }
        let value = match inline {
            Some(value) => value,
            None => match argv.get(i) {
                Some(next) if !next.starts_with('-') || next == "-" || negative(next) => {
                    i += 1;
                    next.clone()
                }
                _ => return wrong(usage, &prog, format!("argument {name}: expected one argument")),
            },
        };
        let set = (|| -> Result<(), String> {
            match name {
                "--game" if cmd == "run" => {
                    if !games::GAMES.contains(&value.as_str()) {
                        return Err(format!("argument --game: invalid choice: {} (choose from {})", base::str_repr(&value), games::GAMES.iter().map(|g| base::str_repr(g)).collect::<Vec<_>>().join(", ")));
                    }
                    a.game = value;
                }
                "--game" => a.game = value,
                "--character" => a.character = value,
                "--runs" => a.runs = int(name, &value)?,
                "--seed" => a.seed = int(name, &value)?,
                "--ascension" => a.ascension = int(name, &value)?,
                "--only" => a.only = Some(value),
                "--grid" => a.grid.push(value),
                "--set" => a.set.push(value),
                "--settings" => a.settings = Some(value),
                "--alt-cards" => a.alt_cards = Some(value),
                "--jobs" => a.jobs = int(name, &value)?,
                "--out" => a.out = Some(value),
                "--md" => a.md = Some(value),
                "--min-runs" => a.min_runs = int(name, &value)?,
                "--prior" | "--most" => {
                    let x = float(&value).map_err(|_| format!("argument {name}: invalid float value: {}", base::str_repr(&value)))?;
                    if name == "--prior" {
                        a.prior = x;
                    } else {
                        a.most = x;
                    }
                }
                "--onto" => a.onto = Some(value),
                "--table" => a.table = int(name, &value)?,
                _ => unreachable!("an option without a rule: {name}"),
            }
            Ok(())
        })();
        if let Err(message) = set {
            return wrong(usage, &prog, message);
        }
    }
    if takes_files && a.files.is_empty() {
        return wrong(usage, &prog, "the following arguments are required: files".to_string());
    }
    if !unknown.is_empty() {
        return wrong(USAGE, "sim", format!("unrecognized arguments: {}", unknown.join(" ")));
    }
    Ok(a)
}

pub fn main(args: Vec<String>) -> i32 {
    let a = match parse_args(&args) {
        Ok(a) => a,
        Err(code) => return code,
    };
    learn::update_knobs(&learn::DEFAULTS.iter().map(|(k, v)| (k.to_string(), *v)).collect());
    let done = match a.cmd.as_str() {
        "run" => cmd_run(&a),
        "trace" => cmd_trace(&a),
        "report" => cmd_report(&a),
        _ => cmd_prior(&a),
    };
    match done {
        Ok(code) => code,
        Err(error) => {
            // (the original ends with the exception's traceback and 1)
            eprintln!("{error}");
            1
        }
    }
}
