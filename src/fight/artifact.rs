//! Artifact on an enemy: a charge taken off is a later debuff that lands in its place, if the deck will apply enough of them.

use serde_json::Value;

use crate::cards::{parse, CardInfo};
use crate::learn::knob;
use crate::py::{self, JsonExt};

pub const LATER: f64 = 0.7; // a debuff that lands later in the fight, against the same one landing now
pub const FIGHT_ROUNDS: [(&str, i64); 2] = [("boss", 9), ("elite", 6)]; // how long such a fight usually runs (an ordinary one: 4)
pub const HAND: f64 = 5.0; // cards seen in a turn

/// (charges a card uses up on one enemy, what its debuffs are worth there once they land), in the planner's points.
pub fn landed(c: &CardInfo, fight: &str) -> (i64, f64) {
    let vuln = (match fight {
        "boss" => 7.0,
        "elite" => 5.0,
        _ => 3.0,
    }) * knob("debuff_worth");
    let weak = (match fight {
        "boss" => 5.0,
        "elite" => 3.5,
        _ => 2.0,
    }) * knob("debuff_worth");
    // a point of Poison over the turns it lasts
    let venom = match fight {
        "boss" => 3.0,
        "elite" => 2.5,
        _ => 2.0,
    };
    let parts = [c.vulnerable.min(3) as f64 * vuln, c.weak.min(3) as f64 * weak, 0.8 * c.doom as f64, venom * c.poison as f64, 1.0 * c.enemy_strength_loss as f64];
    (parts.iter().filter(|p| **p > 0.0).count() as i64, py::fsum(parts))
}

/// (what a debuff of this deck is worth once it lands, how many it will still apply before the fight ends).
pub fn outlook(player: &Value, fight: &str, round_no: i64) -> (f64, f64) {
    let piles: Vec<&Value> = ["hand", "draw_pile", "discard_pile"].iter().flat_map(|pile| player[*pile].list()).collect();
    let (mut uses, mut worth) = (0, 0.0);
    for c in &piles {
        let (n, v) = landed(&parse(c), fight);
        uses += n;
        worth += v;
    }
    if uses == 0 {
        return (0.0, 0.0);
    }
    let rounds = FIGHT_ROUNDS.iter().find(|(kind, _)| *kind == fight).map(|(_, n)| *n).unwrap_or(4);
    let turns = 1.max(rounds - round_no);
    let most = if piles.len() as f64 > HAND { piles.len() as f64 } else { HAND };
    (LATER * worth / uses as f64, (uses * turns) as f64 * HAND / most)
}

/// What a line that took an enemy's Artifact from `before` to `after` earned by it (`view`: `outlook`'s pair).
pub fn gain(before: i64, after: i64, view: (f64, f64)) -> f64 {
    let (each, coming) = view;
    // (The charge on top is the last to pay: the deck has to get through the ones under it first.)
    let paid = |left: i64| {
        let short = coming - (left - 1) as f64;
        let some = if short > 0.0 { short } else { 0.0 };
        if some < 1.0 {
            some
        } else {
            1.0
        }
    };
    each * py::fsum((after + 1..before + 1).map(paid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fight::battle::testing::*;
    use serde_json::json;

    #[test]
    fn golden() {
        let mut checked = 0;
        for (n, row) in states().iter().enumerate() {
            let _env = env(row);
            let (state, r) = (&row["s"], &row["r"]);
            let player = &state["player"];
            let fight = if state["state_type"].truthy() { py::str_of(&state["state_type"]) } else { "monster".to_string() };
            let round_no = if state["battle"]["round"].truthy() { state["battle"]["round"].int() } else { 1 };
            let (here, boss) = (outlook(player, &fight, round_no), outlook(player, "boss", 2));
            assert_eq!(json!([[here.0, here.1], [boss.0, boss.1]]), r["art"], "state {n}");
            for (c, want) in player["hand"].list().iter().zip(r["hand"].list()) {
                let (a, b) = (landed(&parse(c), &fight), landed(&parse(c), "elite"));
                assert_eq!(json!([[a.0, a.1], [b.0, b.1]]), json!([want[3], want[4]]), "state {n}: {c}");
                checked += 2;
            }
            checked += 2;
        }
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/fight/cards.json")).unwrap();
        for row in calls(&g, "cards", 1200) {
            let c = card_from_json(&row["c"]);
            for (knob, field, fights) in [(1.0, "landed", &["monster", "elite", "boss"][..]), (2.0, "landed2", &["boss"][..])] {
                let _env = env(&json!({"g": "sts2", "dw": knob}));
                let got: Vec<Value> = fights.iter().map(|f| landed(&c, f)).map(|(k, v)| json!([k, v])).collect();
                assert_eq!(if got.len() == 1 { got[0].clone() } else { json!(got) }, row[field], "{} at {knob}", c.name);
                checked += got.len();
            }
        }
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/fight/artifact.json")).unwrap();
        for rec in calls(&g, "artifact.landed", 100) {
            let _env = env(&rec["env"]);
            let (a, got) = (&rec["a"], landed(&card_from_json(&rec["a"][0]), rec["a"][1].s()));
            assert_eq!(json!([got.0, got.1]), rec["r"], "landed {a}");
            checked += 1;
        }
        for rec in calls(&g, "artifact.gain", 120) {
            let a = &rec["a"];
            assert_eq!(json!(gain(a[0].int(), a[1].int(), (a[2][0].float(), a[2][1].float()))), rec["r"], "gain {a}");
            checked += 1;
        }
        for rec in calls(&g, "artifact.outlook", 5) {
            let _env = env(&rec["env"]);
            let got = outlook(&rec["a"][0], rec["a"][1].s(), rec["a"][2].int());
            assert_eq!(json!([got.0, got.1]), rec["r"], "outlook");
            checked += 1;
        }
        assert!(checked > 7000, "{checked} checks");
    }
}
