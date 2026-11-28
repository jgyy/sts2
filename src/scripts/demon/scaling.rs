//! What one scaling Power is worth at the Knowledge Demon, by perfect play (demonfight.rs's exact model).
//!
//! ```text
//! scripts demon/scaling      six decks holding no Power, each with one card more of each sort (about 3 min)
//! ```
//!
//! Behind decks::SCALE_POWER. The Power slot gains `pd` damage (Doom, counted as damage) a turn per Power in play, a cost
//! `pc`, and `pm`: attacks times (1 + pm) once it is in play (Reaper Form).
//!
//! (This script's `plays`, which it put in demonfight's place, is demonfight's own here: that one reads the deck's `pc`
//! and `pd`, which are 1 and 0 in any deck but a `Deck2`.)

use crate::scripts::demonfight::{self as D, Deck, DeckArgs};
use crate::scripts::modelfight::{exit_code, on_all_cores, out, quiet};
use crate::base::trunc;

/// The keyword arguments of `Deck2` beyond `Deck`'s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deck2Args {
    pub pd: i64,
    pub pc: i64,
    pub pm: f64,
}

impl Default for Deck2Args {
    fn default() -> Self {
        Deck2Args { pd: 0, pc: 1, pm: 0.0 }
    }
}

/// `Deck2.hit`.
pub fn hit(deck: &Deck, kind: char, s: i64) -> i64 {
    trunc(deck._hit(kind, s) as f64 * (1.0 + deck.pm * s as f64))
}

/// `Deck2(counts, pd, pc, pm, **kw)`.
pub fn Deck2(counts: [i64; 6], more: Deck2Args, args: DeckArgs) -> Deck {
    Deck { pd: more.pd, pc: more.pc, pm: more.pm, hit_fn: Some(hit), ..Deck::new(counts, args) }
}

pub use crate::scripts::demonfight::plays;

pub fn BASES() -> Vec<([i64; 6], DeckArgs)> {
    vec![
        ([6, 4, 2, 6, 2, 0], DeckArgs { da: 15, db: 11, ..Default::default() }),
        ([6, 3, 3, 6, 2, 0], DeckArgs { da: 19, db: 12, ..Default::default() }),
        ([7, 3, 2, 7, 3, 0], DeckArgs { da: 17, db: 12, maxhp: 66, ..Default::default() }),
        ([6, 4, 2, 6, 2, 0], DeckArgs { da: 15, db: 11, energy: 4, ..Default::default() }),
        ([5, 4, 3, 6, 2, 0], DeckArgs { da: 15, db: 11, maxhp: 90, ..Default::default() }),
        ([8, 2, 2, 7, 3, 0], DeckArgs { da: 13, db: 10, energy: 4, maxhp: 66, ..Default::default() }),
    ]
}
pub const P1: [i64; 6] = [0, 0, 0, 0, 0, 1];

/// What a card of EXTRA changes in the deck's keyword arguments (the original's dict; `Lethality`: its None, worked out in `cell`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Power {
    /// `{}`, or `pe`, `ps`, `pd`, `pm`, `pc` where given
    Is { pe: Option<i64>, ps: Option<i64>, pd: i64, pm: f64, pc: i64 },
    Lethality,
}

const PLAIN: Power = Power::Is { pe: None, ps: None, pd: 0, pm: 0.0, pc: 1 };

pub fn EXTRA() -> Vec<(&'static str, [i64; 6], Power)> {
    vec![
        ("nothing", [0; 6], PLAIN),
        ("Strike-like attack", [1, 0, 0, 0, 0, 0], PLAIN),
        ("two-cost attack", [0, 1, 0, 0, 0, 0], PLAIN),
        ("Inflame (+3 Str, 1)", P1, PLAIN),
        ("energy Power", P1, Power::Is { pe: Some(1), ps: Some(0), pd: 0, pm: 0.0, pc: 1 }),
        ("Countdown (6/turn, 1)", P1, Power::Is { pe: None, ps: Some(0), pd: 6, pm: 0.0, pc: 1 }),
        ("Countdown+ (9/turn?, 1)", P1, Power::Is { pe: None, ps: Some(0), pd: 9, pm: 0.0, pc: 1 }),
        ("Lethality (~+half a hit, 1)", P1, Power::Lethality),
        ("Reaper Form (x2, 3)", P1, Power::Is { pe: None, ps: Some(0), pd: 0, pm: 1.0, pc: 3 }),
        ("Reaper Form (x2, 2)", P1, Power::Is { pe: None, ps: Some(0), pd: 0, pm: 1.0, pc: 2 }),
    ]
}

/// (the deck, the card's name, what perfect play wins with it)
pub fn cell(args: &([i64; 6], DeckArgs, &'static str, [i64; 6], Power)) -> (String, &'static str, f64) {
    let (counts, kw, name, add, pw) = *args;
    let pw = match pw {
        Power::Lethality => Power::Is { pe: None, ps: Some(0), pd: trunc(0.5 * kw.da as f64), pm: 0.0, pc: 1 },
        other => other,
    };
    let Power::Is { pe, ps, pd, pm, pc } = pw else { unreachable!() };
    let deck = Deck2(std::array::from_fn(|k| counts[k] + add[k]), Deck2Args { pd, pc, pm }, DeckArgs { pe: pe.unwrap_or(kw.pe), ps: ps.unwrap_or(kw.ps), ..kw });
    let V = D::solve(&deck, D::OFFERS[0]);
    (Deck::new(counts, kw).repr(), name, V[1][0].at(deck.maxhp, D::BOSS))
}

fn script(_args: &[String]) -> Result<(), String> {
    quiet();
    let jobs: Vec<([i64; 6], DeckArgs, &'static str, [i64; 6], Power)> = BASES().into_iter().flat_map(|(c, kw)| EXTRA().into_iter().map(move |(n, a, p)| (c, kw, n, a, p))).collect();
    let mut last: Option<String> = None;
    for (deck, name, v) in on_all_cores(&|| {}, &cell, &jobs, 8) {
        if last.as_ref() != Some(&deck) {
            out!("{deck}");
            last = Some(deck);
        }
        out!("    + {name:30} perfect {v:.3}");
    }
    Ok(())
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(script(&args))
}

#[cfg(test)]
mod tests {
    use crate::scripts::modelfight::golden;
    use super::main;

    #[test]
    fn as_recorded() {
        golden(main, &[], include_str!("../../../tests/golden/scripts/demon_scaling.txt"));
    }
}
