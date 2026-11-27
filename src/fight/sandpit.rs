//! The Insatiable's Sandpit: what the margin left on it is worth. It counts down every turn and at 0 we are eaten; each Frantic
//! Escape played pushes it back by one. What matters is how many rounds the fight still needs, and whether Escapes come in time.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::py;

pub const DEATH: f64 = 300.0; // what being eaten costs, in points
pub const ESCAPE_COST: f64 = 7.0; // what an Escape still to be played costs (half an energy)
pub const PACE: f64 = 32.0; // damage a round the Insatiable usually takes, until the fight says otherwise
pub const SLACK: f64 = 0.8; // the rounds still needed are counted at this share of the pace: a slow hand is what kills

thread_local! {
    static _EATEN: RefCell<HashMap<(i64, i64, u64, u64), f64>> = RefCell::new(HashMap::new());
}

/// Chance of the pit over `turns` more turns of ours, this one ending on `left`, every Escape drawn played.
pub fn _eaten(left: i64, turns: i64, soon: f64, later: f64) -> f64 {
    if left <= 1 {
        return 1.0;
    }
    if turns <= 1 {
        return 0.0; // the boss dies on our next turn: nothing more is asked of the margin
    }
    let key = (left, turns, soon.to_bits(), later.to_bits());
    if let Some(known) = _EATEN.with(|m| m.borrow().get(&key).copied()) {
        return known;
    }
    let (one, two) = ((1.0 - soon) * 0.64, (1.0 - soon) * 0.36); // six Escapes in thirty cards: one in the hand, or more
    let top = turns + 1; // more margin than the fight has turns left is worth nothing
    let out = py::fsum([(soon, 0), (one, 1), (two, 2)].into_iter().map(|(p, k)| p * _eaten((left - 1 + k).min(top), turns - 1, later, later)));
    _EATEN.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() >= 200_000 {
            m.clear();
        }
        m.insert(key, out);
    });
    out
}

/// Points lost by ending the turn with `left` on the Sandpit and the boss on `boss_hp`: the chance of being eaten first, plus
/// the Escapes the rest of the fight will ask for. `dealt`: what the boss had lost before this turn; `odds`: as battle._escape_odds
/// (the Python's default: (0.3, 0.3)).
pub fn price(left: i64, boss_hp: i64, dealt: i64, round_no: i64, odds: (f64, f64)) -> f64 {
    if left <= 1 {
        return DEATH * 10.0;
    }
    // The pace so far, the usual one counted as two rounds; not this turn's damage, which is in `boss_hp` already.
    let pace = (dealt.max(0) as f64 + 2.0 * PACE) / (round_no.max(1) as f64 + 1.0);
    let slow = SLACK * pace;
    let turns = 1.0 + boss_hp as f64 / (if slow > 1.0 { slow } else { 1.0 }); // our turns after this one, the kill among them
    let clip = |p: f64| {
        let some = if p > 0.0 { p } else { 0.0 };
        py::round_to(if some < 1.0 { some } else { 1.0 }, 2)
    };
    let (soon, later) = (clip(odds.0), clip(odds.1));

    let cost = |t: i64| -> f64 { DEATH * _eaten(left.min(t + 1), t, soon, later) + ESCAPE_COST * 0.max(t - (left - 1)) as f64 };

    let low = py::trunc(turns);
    cost(low) + (turns - low as f64) * (cost(low + 1) - cost(low))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::py::JsonExt;
    use serde_json::{json, Value};

    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/fight/sandpit.json")).unwrap();
        let (eaten, prices) = (g["sandpit._eaten"].list(), g["sandpit.price"].list());
        assert!(eaten.len() >= 30 && prices.len() >= 700, "{} and {} cases", eaten.len(), prices.len());
        for rec in eaten {
            let a = &rec["a"];
            assert_eq!(json!(_eaten(a[0].int(), a[1].int(), a[2].float(), a[3].float())), rec["r"], "_eaten {a}");
        }
        for rec in prices {
            let a = &rec["a"];
            assert_eq!(json!(price(a[0].int(), a[1].int(), a[2].int(), a[3].int(), (a[4][0].float(), a[4][1].float()))), rec["r"], "price {a}");
        }
        assert_eq!(price(1, 280, 40, 2, (0.3, 0.3)), DEATH * 10.0);
    }
}
