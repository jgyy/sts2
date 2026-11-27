//! The Fool's Sadness: "Unplayable. Retain." Sorrowful Strike, Mourning Blow, Jest, Vitriol, Rummage and Brighten Up put it in the
//! hand, Ennui one more every turn, and each stays there for the rest of the fight; only a Joy takes one away (Punch of Joy, Ecstasy).
//! A hand holds 10, so once the Sadness and a turn's draw no longer fit, every Sadness is a card not drawn, every turn after.
//!
//! 2026-11-25: of the Fool's boss fights with any Sadness, 18 had 5 or more in the hand and 5 had 8-10. At 18:06 it played Ennui on
//! round 2 against the Kin: from round 11 the hand was ten Sadness, nothing was drawn, and the Priest won from 75 HP against our 65.
//! The planner read Ennui as a Power it could not read (worth 7 a turn of the fight) and Sorrowful Strike as 13 damage and nothing else.
//!
//! A line is scored by the cards the turns to come will draw: the first `PLAYED` of a hand at full worth (about what a turn's energy
//! plays), the rest at `SPARE`; with the Sadness the line leaves in the hand, and what Ennui and Ecstasy add or take away each turn.

use serde_json::Value;

use crate::cards::{key_of, parse};
use crate::fight::battle::_stacks;
use crate::py::{self, JsonExt};

crate::tunables! {
    pub HAND_CAP: i64 = 10;
    /// cards a turn draws
    pub TURN_DRAW: i64 = 5;
    /// a hand's first 4 cards count whole (a turn's energy plays about that many), each one beyond at this share
    pub PLAYED: i64 = 4;
    pub SPARE: f64 = 0.4;
    pub MAX_TURNS: i64 = 15;
    /// what a card drawn on a later turn is worth against one in the hand now (the fight may be over first)
    pub LATER: f64 = 0.7;
    // Fights that run long, where a full hand costs most: a share of the price is taken over this many times the turns left.
    // (2026-11-25: in boss fights won, 20-39% of rounds at Soul Fysh, the Ceremonial Beast, the Waterfall Giant and the Knowledge
    // Demon came 1.5x or more past the estimate; 19:03 the Fool lost to Soul Fysh on 25 HP from round 8 under ten Sadness.)
    /// (scripts/bosses/sadfight.rs: won 43.9% -> 48.8%; 0.5 x 2.5 and 1.0 x 2.0 the same)
    pub LONG: f64 = 1.8;
    pub TAIL: f64 = 0.4;
}

pub const PACE: [(&str, f64); 2] = [("boss", 28.0), ("elite", 22.0)]; // enemy HP a round we take off, before the fight has shown its own (an ordinary fight: 18)

/// `outlook`'s view: (Sadness held, Sadness a turn, extra draw a turn, turns left, a card's worth).
pub type View = (i64, i64, i64, f64, f64);

/// Sadness in the hand now.
pub fn held(player: &Value) -> i64 {
    player["hand"].list().iter().filter(|c| key_of(&c["id"].or(&c["name"]).text()) == "SADNESS").count() as i64
}

/// (Sadness a turn, extra cards drawn a turn) from the Powers up: Ennui's amount is its draw (2, 3 upgraded), a Sadness a copy.
pub fn turn_rate(player: &Value) -> (i64, i64) {
    let (ennui, ecstasy) = (0.max(_stacks(player, "ENNUI")), 0.max(_stacks(player, "ECSTASY")));
    ((if ennui != 0 { 1.max(py::round(ennui as f64 / 2.5)) } else { 0 }) - ecstasy, ennui)
}

/// Turns the fight still has, from the enemies' HP and the pace so far (or a usual pace before it has shown one).
pub fn turns_left(state: &Value) -> f64 {
    let battle = &state["battle"];
    let foes: Vec<&Value> = battle["enemies"].list().iter().filter(|e| e["max_hp"].int() < 100_000).collect();
    let left: i64 = foes.iter().filter(|e| e["hp"].int() > 0).map(|e| e["hp"].int() + e["block"].int()).sum();
    let rounds = (if battle["round"].truthy() { battle["round"].int() } else { 1 }) - 1;
    let dealt: i64 = foes.iter().map(|e| 0.max(e["max_hp"].int() - e["hp"].int())).sum();
    let pace = if rounds >= 2 && dealt > 0 {
        dealt as f64 / rounds as f64
    } else {
        let kind = state["state_type"].text();
        PACE.iter().find(|(k, _)| *k == kind).map(|(_, p)| *p).unwrap_or(18.0)
    };
    let most = MAX_TURNS.get() as f64;
    let needs = left as f64 / (if pace > 10.0 { pace } else { 10.0 });
    let capped = if needs < most { needs } else { most };
    if capped > 1.0 {
        capped
    } else {
        1.0
    }
}

/// (Sadness held, Sadness a turn, extra draw a turn, turns left, a card's worth) when a card in the hand changes any of them, else None.
/// `draw_worth`: battle._draw_worth's.
pub fn outlook(state: &Value, draw_worth: (f64, f64, f64)) -> Option<View> {
    let player = &state["player"];
    if !player["hand"].list().iter().any(|c| {
        let info = parse(c);
        info.sadness != 0 || info.sad_turn.is_some()
    }) {
        return None;
    }
    let (free, paid_share, per_energy) = draw_worth;
    let (rate, extra) = turn_rate(player);
    let card = free + paid_share * per_energy;
    Some((held(player), rate, extra, turns_left(state), if card > 2.0 { card } else { 2.0 }))
}

/// Worth of what the turns to come draw, in cards: `sad` Sadness in the hand now and `rate` more each turn.
pub fn _drawn(sad: i64, rate: i64, extra: i64, turns: f64) -> f64 {
    let (mut total, mut turn) = (0.0, 1);
    let (played, spare) = (PLAYED.get(), SPARE.get());
    while ((turn - 1) as f64) < turns {
        let cards = 0.max((TURN_DRAW.get() + extra).min(HAND_CAP.get() - 0.max(sad + rate * turn)));
        let share = turns - (turn - 1) as f64;
        total += (cards.min(played) as f64 + spare * 0.max(cards - played) as f64) * (if share < 1.0 { share } else { 1.0 });
        turn += 1;
    }
    total
}

/// What a line's Sadness (and Ennui's, Ecstasy's) does to the turns to come, in planner points (negative: cards not drawn).
pub fn worth(view: View, added: i64, rate_added: i64, extra_added: i64) -> f64 {
    let (sad, rate, extra, turns, card) = view;
    let gain = |t: f64| _drawn(sad + added, rate + rate_added, extra + extra_added, t) - _drawn(sad, rate, extra, t);
    let (most, long) = (MAX_TURNS.get() as f64, LONG.get() * turns);
    ((1.0 - TAIL.get()) * gain(turns) + TAIL.get() * gain(if long < most { long } else { most })) * card * LATER.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fight::battle::testing::*;
    use crate::fight::battle::_draw_worth;
    use serde_json::json;

    #[test]
    fn golden() {
        let (mut checked, mut views) = (0, 0);
        for (n, row) in states().iter().enumerate() {
            let _env = env(row);
            let (state, r) = (&row["s"], &row["r"]);
            let player = &state["player"];
            let rate = turn_rate(player);
            let view = outlook(state, _draw_worth(player));
            views += view.is_some() as usize;
            let view = view.map(|v| json!([v.0, v.1, v.2, v.3, v.4])).unwrap_or(Value::Null);
            assert_eq!(json!([held(player), [rate.0, rate.1], turns_left(state), view]), r["sad"], "state {n}");
            checked += 4;
        }
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/fight/sadness.json")).unwrap();
        for rec in calls(&g, "sadness._drawn", 360) {
            let a = &rec["a"];
            assert_eq!(json!(_drawn(a[0].int(), a[1].int(), a[2].int(), a[3].float())), rec["r"], "_drawn {a}");
            checked += 1;
        }
        for rec in calls(&g, "sadness.worth", 360) {
            let (a, v) = (&rec["a"], &rec["a"][0]);
            let view = (v[0].int(), v[1].int(), v[2].int(), v[3].float(), v[4].float());
            assert_eq!(json!(worth(view, a[1].int(), a[2].int(), a[3].int())), rec["r"], "worth {a}");
            checked += 1;
        }
        assert!(checked > 2000 && views > 3, "{checked} checks, {views} states with Sadness to come");
        let view = (2, 1, 2, 6.0, 3.0);
        let usual = worth(view, 2, 0, 0);
        {
            let _cap = HAND_CAP.set(30);
            assert_eq!(worth(view, 2, 0, 0), 0.0);
        }
        assert!(worth(view, 2, 0, 0) == usual && usual < 0.0);
    }
}
