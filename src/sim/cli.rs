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

use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::Value;

use crate::games;
use crate::sim::content::{self, Content};
use crate::sim::run::Run;

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

// TODO(sts2 content): the rest of the command line (GRID, knobs_for, fixed, cmd_run, cmd_trace, cmd_prior, cmd_report, the arguments).
pub fn main(_args: Vec<String>) -> i32 {
    unimplemented!("not ported yet")
}
