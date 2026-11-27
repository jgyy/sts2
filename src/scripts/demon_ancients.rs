//! The Ancients' every-turn relics at the Knowledge Demon, by perfect play (demonfight.rs's exact model).
//!
//! ```text
//! scripts demon/ancients      eight decks (six on 4 energy or more, two on 3) under each relic (about 10 s)
//! ```
//!
//! Each relic is a change to the fight's rules (RELICS); the Demon's afflictions are all taken. Behind relicworth::energy_lean.

use std::cell::Cell;
use std::collections::HashMap;

use super::demonfight::{self as D, Burns, Deck, DeckArgs, Rules};
use super::modelfight::{exit_code, on_all_cores, out, quiet};
use crate::py::{floordiv, fsum};

/// A relic, as changes to the rules (Python's dict: a key it does not hold is 0 here).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Relic {
    pub draw: i64,
    pub energy: i64,
    pub from3: i64,
    pub cocoa: i64,
    pub enemy_str: i64,
    pub str_per_round: i64,
}

thread_local! {
    /// the round being solved: `solve` asks for each round's rules before its plays
    pub static ROUND: Cell<i64> = const { Cell::new(1) };
    /// the relic in force, as changes to the rules (set in each worker)
    pub static RELIC: Cell<Relic> = const { Cell::new(Relic { draw: 0, energy: 0, from3: 0, cocoa: 0, enemy_str: 0, str_per_round: 0 }) };
}

/// demonfight's rules under the relic in force (`demonfight::set_rules(Some(rules))` in each worker).
pub fn rules(r: i64, burns: Burns) -> Rules {
    let mut out = D::_rules(r, burns);
    let relic = RELIC.with(|x| x.get());
    ROUND.with(|x| x.set(r));
    out.draw += relic.draw;
    out.drain -= relic.energy - (if relic.from3 != 0 && r < 3 { relic.energy } else { 0 }) + (if r == 1 { relic.cocoa } else { 0 });
    if relic.enemy_str != 0 && out.hits != 0 {
        out.inc += relic.enemy_str * out.hits;
        out.per = floordiv(out.inc, out.hits);
    }
    out
}

/// This script's `Deck.hit`: the relic's Strength a round on top.
pub fn hit(deck: &Deck, kind: char, s: i64) -> i64 {
    deck._hit(kind, s) + RELIC.with(|x| x.get()).str_per_round * ROUND.with(|x| x.get())
}

/// This script's kind of deck.
pub fn deck(counts: [i64; 6], args: DeckArgs) -> Deck {
    Deck { hit_fn: Some(hit), ..Deck::new(counts, args) }
}

pub fn BASES() -> Vec<([i64; 6], DeckArgs)> {
    vec![
        ([6, 4, 2, 6, 2, 0], DeckArgs { da: 15, db: 11, energy: 4, ..Default::default() }),
        ([6, 3, 3, 6, 2, 0], DeckArgs { da: 19, db: 12, ..Default::default() }),
        ([6, 4, 2, 6, 2, 2], DeckArgs { da: 13, db: 10, energy: 4, ..Default::default() }),
        ([6, 3, 3, 6, 2, 2], DeckArgs { da: 17, db: 12, ..Default::default() }),
        ([5, 4, 3, 6, 2, 3], DeckArgs { da: 15, db: 11, maxhp: 90, ..Default::default() }),
        ([4, 3, 5, 7, 3, 3], DeckArgs { da: 14, db: 11, energy: 4, maxhp: 66, ..Default::default() }),
        ([6, 4, 2, 6, 2, 0], DeckArgs { da: 15, db: 11, ..Default::default() }),
        ([7, 3, 2, 7, 3, 0], DeckArgs { da: 17, db: 12, maxhp: 66, ..Default::default() }),
    ]
}

/// (name, rule changes, dead cards added)
pub fn RELICS() -> Vec<(&'static str, Relic, i64)> {
    let none = Relic::default();
    vec![
        ("none", none, 0),
        ("+1 energy (Velvet Choker, Prismatic Gem, Sozu)", Relic { energy: 1, ..none }, 0),
        ("+1 card a turn (Pael's Blood)", Relic { draw: 1, ..none }, 0),
        ("+2 cards a turn (Snecko Eye unconfused, Fiddle)", Relic { draw: 2, ..none }, 0),
        ("+1 energy from turn 3 (Pael's Flesh)", Relic { energy: 1, from3: 1, ..none }, 0),
        ("+4 energy on turn 1 (Very Hot Cocoa)", Relic { cocoa: 4, ..none }, 0),
        ("+1 energy, Demon +1 Strength (Philosopher's Stone)", Relic { energy: 1, enemy_str: 1, ..none }, 0),
        ("+1 energy, a dead card (Blessed Antler, Blood-Soaked Rose)", Relic { energy: 1, ..none }, 1),
        ("+1 energy, two dead cards", Relic { energy: 1, ..none }, 2),
        ("+1 Strength a round (Toasty Mittens, no card lost)", Relic { str_per_round: 1, ..none }, 0),
        ("+1 Strength a round, a card less (Toasty Mittens)", Relic { str_per_round: 1, draw: -1, ..none }, 0),
    ]
}
/// every affliction taken, no burn
pub const TAKEN: Burns = (false, false, false);

/// (On a thread where `demonfight::set_rules(Some(rules))` has been called: `setup`.)
pub fn job(args: &(usize, usize)) -> (usize, usize, f64) {
    let (b, v) = *args;
    let (counts, kw) = BASES()[b];
    let (_name, changes, dead) = RELICS()[v];
    RELIC.with(|x| x.set(changes));
    let deck = deck(std::array::from_fn(|k| counts[k] + if D::KINDS.as_bytes()[k] == b'F' { dead } else { 0 }), kw);
    (b, v, D::solve(&deck, TAKEN)[1][0].at(deck.maxhp, D::BOSS))
}

/// What every worker starts with: this script's rules in place of demonfight's (`D.rules = rules`).
pub fn setup() {
    D::set_rules(Some(rules));
}

fn script(_args: &[String]) -> Result<(), String> {
    quiet();
    let (bases, relics) = (BASES(), RELICS());
    let jobs: Vec<(usize, usize)> = (0..bases.len()).flat_map(|b| (0..relics.len()).map(move |v| (b, v))).collect();
    let res: HashMap<(usize, usize), f64> = on_all_cores(&setup, &job, &jobs, 8).into_iter().map(|(b, v, p)| ((b, v), p)).collect();
    let on3: Vec<usize> = (0..bases.len()).filter(|b| bases[*b].1.energy == 3).collect();
    let on4: Vec<usize> = (0..bases.len()).filter(|b| !on3.contains(b)).collect();
    let gain = |v: usize, bs: &[usize]| -> f64 { 100.0 * fsum(bs.iter().map(|b| res[&(*b, v)] - res[&(*b, 0)])) / bs.len() as f64 };
    out!("perfect play's chance of beating the Demon from full HP, by deck; then the mean gain in points on 3 and on 4+ energy");
    for (v, (name, _c, _d)) in relics.iter().enumerate() {
        out!("{name:58}{}   3E {:+5.1}  4E+ {:+5.1}", (0..bases.len()).map(|b| format!(" {:.3}", res[&(b, v)])).collect::<String>(), gain(v, &on3), gain(v, &on4));
    }
    Ok(())
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(script(&args))
}

#[cfg(test)]
mod tests {
    use super::super::modelfight::golden;
    use super::main;

    #[test]
    fn as_python() {
        golden(main, &[], include_str!("../../tests/golden/scripts/demon_ancients.txt"));
    }
}
