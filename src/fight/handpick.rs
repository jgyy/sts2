//! Which card to give up when a card or a relic asks for one from the hand, judged by the turn's plan (not deck ratings).

use std::cell::RefCell;
use std::collections::HashMap;

use indexmap::IndexMap;
use serde_json::{json, Value};

use super::combat;
use crate::py::{self, JsonExt};

pub const FIGHTS: [&str; 3] = ["monster", "elite", "boss"];
/// a plan this much worse has lost a kill or our life, not a few points of damage
pub const RUINOUS: f64 = 500.0;
/// the cost given a card that was not planned for: after the one found that costs nothing
pub const UNPLANNED: f64 = 1e6;
/// selections whose costs are remembered (the same one is asked again after each click)
pub const ANSWERS_KEPT: usize = 4;

thread_local! {
    pub static _answers: RefCell<IndexMap<String, IndexMap<i64, f64>>> = RefCell::new(IndexMap::new());
}

/// `handpick._answers.clear()`.
pub fn _answers_clear() {
    _answers.with(|a| a.borrow_mut().clear());
}

/// `json.dumps(v, sort_keys=True, default=str)`: what tells one question from another.
fn _dumps(v: &Value) -> String {
    py::Dumps { sort_keys: true, ..Default::default() }.dumps(v)
}

/// `{**d}`: the dict itself, or an empty one for anything else.
fn _dict(d: &Value) -> Value {
    if d.is_object() {
        d.clone()
    } else {
        json!({})
    }
}

/// For each card on offer (by its index): how much worse this turn's best plan is without it (None: no fight). Cards are planned
/// for in the order given, until a plain one costs no more than `tolerance`: the rest come back as `UNPLANNED`.
/// (The Python's default: `tolerance` 1e-9.)
pub fn turn_costs(state: &Value, cards: &[Value], tolerance: f64) -> Option<IndexMap<i64, f64>> {
    let (battle, player) = (&state["battle"], &state["player"]);
    if cards.len() < 2 || !battle["enemies"].list().iter().any(|e| e["hp"].int() > 0) {
        return None;
    }
    let hand = player["hand"].list();

    // The card as the planner needs it: the selection screen leaves out who it is aimed at.
    let whole = |c: &Value| -> Value {
        let twin = hand.iter().find(|h| h["name"] == c["name"] && h["description"] == c["description"]);
        let target = match twin.map(|t| &t["target_type"]).filter(|t| t.truthy()) {
            Some(t) => t.clone(),
            None => json!(if py::str_of(&c["type"]) == "Attack" { "AnyEnemy" } else { "Self" }),
        };
        // (Not the game's "can play": during a selection nothing can be played.)
        let mut out = _dict(c);
        out["target_type"] = target;
        out["can_play"] = json!(!crate::re!(r"(?i)\bunplayable\b").is_match(&c["description"].text()));
        out
    };

    let pool: Vec<Value> = cards.iter().map(whole).collect();
    let kind: Value = if state["_fight"].as_str().is_some_and(|k| FIGHTS.contains(&k)) { state["_fight"].clone() } else { json!("monster") };
    // The same question comes again after each click, and a plan per card on offer is dear: answers are remembered.
    let mut asked = _dict(state);
    if let Some(m) = asked.as_object_mut() {
        m.shift_remove("hand_select");
    }
    let key = _dumps(&json!([asked, cards.iter().map(|c| json!([c["index"], c])).collect::<Vec<_>>(), tolerance]));
    if let Some(known) = _answers.with(|a| a.borrow().get(&key).cloned()) {
        return Some(known);
    }
    // Toasty Mittens ("Exhaust 1 card from your Hand and gain 1 Strength") gives after it asks: the texts on show lack it.
    let gift: i64 = player["relics"]
        .list()
        .iter()
        .filter_map(|r| crate::re!(r"(?i)exhaust 1 card from your hand and gain (\d+) strength").captures(&r["description"].text()).map(|m| m[1].parse::<i64>().unwrap_or(0)))
        .sum();

    let plan_without = |skip: Option<usize>| -> Option<f64> {
        let rest: Vec<Value> = pool
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != skip)
            .map(|(_, c)| c)
            .enumerate()
            .map(|(n, c)| {
                let mut c = c.clone();
                c["index"] = json!(n);
                c
            })
            .collect();
        let mut plan = combat::Plan::default();
        let mut without = _dict(state);
        without["state_type"] = kind.clone();
        without["_might"] = json!(gift);
        let mut holding = _dict(player);
        holding["hand"] = Value::Array(rest);
        without["player"] = holding;
        combat::choose_card(&without, Some(&mut plan));
        plan.score
    };

    let whole_hand = plan_without(None)?;
    // Plain cards (only their own play counts) are tried in order until one the plan does not miss: later ones could only tie.
    let plain: Vec<bool> = pool.iter().map(_plain).collect();
    let order: Vec<usize> = (0..pool.len()).filter(|&i| !plain[i]).chain((0..pool.len()).filter(|&i| plain[i])).collect();
    let mut scores: IndexMap<usize, f64> = IndexMap::new();
    let mut planned: HashMap<String, Option<f64>> = HashMap::new();
    for skip in order {
        // Copies of a card leave the same hand behind: each kind is planned once.
        let mut kind_of = pool[skip].clone();
        if let Some(m) = kind_of.as_object_mut() {
            m.shift_remove("index");
        }
        let same = _dumps(&kind_of);
        let score = *planned.entry(same).or_insert_with(|| plan_without(Some(skip)));
        let score = score?;
        scores.insert(skip, score);
        if plain[skip] && whole_hand - score <= tolerance {
            break;
        }
    }
    let best = py::max_of(scores.values().copied()).expect("a card was planned for");
    let mut costs: IndexMap<i64, f64> = IndexMap::new();
    for (i, c) in cards.iter().enumerate() {
        let index = if c.has("index") { c["index"].int() } else { i as i64 };
        costs.insert(index, match scores.get(&i) {
            Some(score) => best - score,
            None => UNPLANNED,
        });
    }
    _answers.with(|a| {
        let mut a = a.borrow_mut();
        a.insert(key, costs.clone());
        while a.len() > ANSWERS_KEPT {
            a.shift_remove_index(0);
        }
    });
    Some(costs)
}

pub fn _HAND_BOUND() -> &'static regex::Regex {
    crate::re!(r"(?i)ethereal|hexed|retain|unplayable|innate|\bhands?\b|\beach\b|\bevery\b|\bburn|lose \d+ hp|\bcurse|\bstatus|\bdeck\b|\bpile")
}

pub fn _INERT() -> &'static regex::Regex {
    crate::re!(r"(?i)unplayable|ethereal|eternal|innate|exhaust|[\W_]+")
}

/// A card the rest of the hand cannot feel: an Attack, Skill or Power whose text says nothing of the hand,
/// or a Curse or Status that does nothing but sit there (Ascender's Bane: "Unplayable. Ethereal.").
pub fn _plain(c: &Value) -> bool {
    let text = c["description"].text();
    let ctype = py::str_of(&c["type"]);
    if ["Curse", "Status"].contains(&ctype.as_str()) && _INERT().replace_all(&text, "").is_empty() {
        return true;
    }
    let mut words = vec![text];
    words.extend(c["keywords"].list().iter().filter(|k| k.is_object()).map(|k| py::str_of(&k["name"])));
    ["Attack", "Skill", "Power"].contains(&ctype.as_str()) && !_HAND_BOUND().is_match(&words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fight::combat::testing::{rows, Golden};
    use std::cell::Cell;
    use std::rc::Rc;

    /// A cost as the cases carry it ("inf" for what JSON cannot).
    fn cost(v: &Value) -> f64 {
        match v.as_str() {
            Some("inf") => f64::INFINITY,
            Some("-inf") => f64::NEG_INFINITY,
            Some("nan") => f64::NAN,
            _ => v.as_f64().expect("a float"),
        }
    }

    #[test]
    fn golden_turn_costs() {
        let texts = [include_str!("../../tests/golden/fight2/costs0.json"), include_str!("../../tests/golden/fight2/costs1.json")];
        let (mut total, mut bad, mut none, mut cut) = (0, Vec::new(), 0, 0);
        for text in texts {
            let g = Golden {
                atoms: rows(&[text], "atoms"),
                cards: Vec::new(),
                knobs: rows(&[text], "knobs").iter().map(|k| k.entries().map(|(name, x)| (name.clone(), x.float())).collect()).collect(),
            };
            for (n, rec) in rows(&[text], "costs").iter().enumerate() {
                total += 1;
                let _env = g.env(&rec["env"]);
                let state = g.state(&rec["s"]);
                let calls = Rc::new(Cell::new(0));
                let spy = calls.clone();
                let _hook = combat::with_choose_card_hook(move |s, plan| {
                    spy.set(spy.get() + 1);
                    combat::_choose_card(s, plan)
                });
                _answers_clear();
                let got = turn_costs(&state, rec["cards"].list(), rec["tol"].float());
                let want: Option<Vec<(i64, f64)>> = rec["r"].as_array().map(|r| r.iter().map(|kv| (kv[0].int(), cost(&kv[1]))).collect());
                let made = calls.get();
                // (the same question again: the answer is remembered)
                let again = turn_costs(&state, rec["cards"].list(), rec["tol"].float());
                let shown = got.as_ref().map(|c| c.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>());
                if shown != want || made != rec["calls"].int() || again != got || (got.is_some() && calls.get() != made) {
                    if bad.len() < 5 {
                        eprintln!("case {n}: got {shown:?} in {made} plans\n  want {} in {}", rec["r"], rec["calls"]);
                    }
                    bad.push(n);
                }
                none += got.is_none() as usize;
                cut += got.is_some_and(|c| c.values().any(|v| *v == UNPLANNED)) as usize;
            }
        }
        assert!(total >= 500, "{total} cases");
        assert!(bad.is_empty(), "{} of {total} cases differ: {:?}", bad.len(), &bad[..bad.len().min(40)]);
        assert!(none > 5 && cut > 100, "{none} without a fight, {cut} cut short");
    }

    #[test]
    fn golden_plain() {
        let cases = rows(&[include_str!("../../tests/golden/fight2/cards0.json"), include_str!("../../tests/golden/fight2/cards1.json")], "card");
        assert!(cases.len() >= 6000);
        let bad: Vec<usize> = (0..cases.len()).filter(|&n| json!(_plain(&cases[n]["c"])) != cases[n]["plain"]).collect();
        assert!(bad.is_empty(), "{} of {} cards differ: {:?}", bad.len(), cases.len(), &bad[..bad.len().min(20)]);
    }

    #[test]
    fn answers_are_kept_four_deep() {
        _answers_clear();
        let foe = json!({"entity_id": "A", "hp": 30, "max_hp": 30, "block": 0, "status": [], "intents": [{"type": "Attack", "label": "5"}]});
        let cards = [
            json!({"index": 0, "name": "Strike", "type": "Attack", "cost": "1", "description": "Deal 6 damage."}),
            json!({"index": 1, "name": "Defend", "type": "Skill", "cost": "1", "description": "Gain 5 Block."}),
        ];
        for hp in 40..46 {
            let state = json!({"state_type": "hand_select", "battle": {"round": 1, "enemies": [foe]}, "player": {"hp": hp, "max_hp": 80, "energy": 1, "hand": cards}, "hand_select": {"cards": cards, "n": hp}});
            assert!(turn_costs(&state, &cards, 1e-9).is_some());
        }
        assert_eq!(_answers.with(|a| a.borrow().len()), ANSWERS_KEPT);
        assert_eq!(turn_costs(&json!({"player": {}}), &cards, 1e-9), None);
        assert_eq!((RUINOUS, UNPLANNED, FIGHTS), (500.0, 1e6, ["monster", "elite", "boss"]));
    }
}
