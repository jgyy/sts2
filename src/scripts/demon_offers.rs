//! The Knowledge Demon's afflictions, priced by perfect play across a grid of decks (what fixed afflictions::SLOTH_MARGIN).
//!
//! ```text
//! scripts demon/offers [2|3]
//! ```
//!
//! Offer 2 (default) sets Sloth against the burn after Mind Rot; offer 3 Waste Away against the burn after Mind Rot and
//! Sloth. Decks perfect play wins under 4% or over 97% either way are left out; beside each is the bot's own measure,
//! `afflictions::lost_to_sloth`: cards a turn beyond three the best play with no limit would use.

use std::cell::Cell;
use std::collections::HashMap;

use super::demonfight::{self as d, Burns, Deck, DeckArgs, Rules};
use super::modelfight::{exit_code, int, on_all_cores, out, quiet};
use crate::py::{sort_by_key, trunc};

thread_local! {
    /// cards drawn beyond the model's five, set in each worker
    pub static BONUS: Cell<i64> = const { Cell::new(0) };
}

/// demonfight's rules with the bonus cards (`demonfight::set_rules(Some(rules))` in each worker).
pub fn rules(r: i64, burns: Burns) -> Rules {
    let mut out = d::_rules(r, burns);
    out.draw += BONUS.with(|b| b.get());
    out
}

/// Cards a turn beyond three the unlimited best play would use (the bot's `lost_to_sloth`, at most 3 a turn).
pub fn lost(deck: &Deck, draw: i64) -> f64 {
    let mut total = 0.0;
    for (hand, p) in d::hands(deck, draw, 0) {
        let [a, h, z, b, _f, _pw] = hand;
        let mut best: Option<((i64, i64), i64)> = None;
        for i in 0..=a {
            for j in 0..=h {
                for k in 0..=b {
                    if i + 2 * j + k > deck.energy {
                        continue;
                    }
                    let n = i + j + k + z;
                    let key = (i * deck.hit('A', 0) + j * deck.hit('H', 0) + z * deck.hit('Z', 0) + k * deck.db, -n);
                    if best.is_none_or(|(top, _)| key > top) {
                        best = Some((key, n));
                    }
                }
            }
        }
        let best = best.expect("TypeError: 'NoneType' object is not subscriptable");
        total += p * 3.min(0.max(best.1 - 3)) as f64;
    }
    total
}

/// (On a thread where `demonfight::set_rules(Some(rules))` has been called: `setup`.)
pub fn job(args: &(usize, i64, Deck, Burns)) -> (usize, Burns, f64) {
    let (i, bonus, deck, burns) = args;
    BONUS.with(|b| b.set(*bonus));
    (*i, *burns, d::solve(deck, *burns)[1][0].at(deck.maxhp, d::BOSS))
}

/// What every worker starts with: this script's rules in place of demonfight's (`d.rules = rules`).
pub fn setup() {
    d::set_rules(Some(rules));
}

/// (the mix of cards, energy, bonus cards, a Strike's damage, the deck)
pub type Meta = ([i64; 6], i64, i64, i64, Deck);

/// The grid of decks.
pub fn metas() -> Vec<Meta> {
    let mixes = [[4, 3, 5, 6, 3, 0], [3, 3, 7, 5, 3, 0], [5, 2, 4, 7, 3, 0], [3, 2, 9, 4, 3, 0]];
    let mut meta = Vec::new();
    for mix in mixes {
        for e in [3, 4, 5, 6, 7] {
            for bonus in [0, 1, 2, 3] {
                for da in [8, 10, 12, 14, 17, 20] {
                    meta.push((mix, e, bonus, da, Deck::new(mix, DeckArgs { da, db: trunc(da as f64 * 0.8), energy: e, maxhp: 66, ..Default::default() })));
                }
            }
        }
    }
    meta
}

/// The script's table for these decks of the grid (the script itself: all of them).
pub fn report(offer: i64, meta: &[Meta]) {
    let base = (false, false, false); // Mind Rot, Sloth, Waste Away
    let other = if offer == 2 { (false, true, false) } else { (false, false, true) };
    let jobs: Vec<(usize, i64, Deck, Burns)> = meta.iter().enumerate().flat_map(|(i, m)| [base, other].map(|b| (i, m.2, m.4.clone(), b))).collect();
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let out = on_all_cores(&setup, &job, &jobs, workers);
    let mut res: HashMap<(usize, Burns), f64> = HashMap::new();
    for (i, b, v) in out {
        res.insert((i, b), v);
    }
    out!("offer {offer}: MSW (Mind Rot, Sloth, Waste Away) against {}", if offer == 2 { "MbW (the burn at offer 2)" } else { "MSb (the burn at offer 3)" });
    let mut rows = Vec::new();
    for (i, (mix, e, bonus, da, deck)) in meta.iter().enumerate() {
        let (a, b) = (res[&(i, base)], res[&(i, other)]);
        if !(a.max(b) < 0.04 || a.min(b) > 0.97) {
            rows.push((lost(deck, 4 + bonus), *mix, *e, *bonus, *da, a, b));
        }
    }
    sort_by_key(&mut rows, |row| *row);
    for (l, mix, e, bonus, da, a, b) in rows {
        out!("{l:5.2}  {} e{e} +{bonus} da{da:2} | MSW {a:.3}  {} {b:.3}", mix.iter().map(|n| n.to_string()).collect::<String>(), if offer == 2 { "MbW" } else { "MSb" });
    }
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let offer = if !args.is_empty() { int(&args[0])? } else { 2 };
    report(offer, &metas());
    Ok(())
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(script(&args))
}

#[cfg(test)]
mod tests {
    use super::super::modelfight::{capture, golden};
    use super::*;

    const OFFER2: &str = include_str!("../../tests/golden/scripts/demon_offers.txt");
    const OFFER3: &str = include_str!("../../tests/golden/scripts/demon_offers_3.txt");

    /// The rows of the Python's table for every 19th deck of the grid: the same decks priced here.
    #[test]
    fn as_python_in_part() {
        for (offer, expected) in [(2, OFFER2), (3, OFFER3)] {
            let meta: Vec<Meta> = metas().into_iter().step_by(19).collect();
            let ((), text) = capture(|| report(offer, &meta));
            let mut lines = text.lines();
            assert_eq!(lines.next(), expected.lines().next());
            let (rows, all): (Vec<&str>, Vec<&str>) = (lines.collect(), expected.lines().collect());
            assert!(rows.len() >= 8, "{} rows", rows.len());
            // (the same order as in the whole table: the rows are sorted)
            let mut at = 0;
            for row in rows {
                at += all[at..].iter().position(|line| *line == row).unwrap_or_else(|| panic!("offer {offer}: not in the Python's table: {row}")) + 1;
            }
        }
    }

    /// The whole tables (minutes): `cargo test --release -- --ignored demon_offers`.
    #[test]
    #[ignore]
    fn as_python() {
        golden(main, &[], OFFER2);
        golden(main, &["3"], OFFER3);
    }
}
