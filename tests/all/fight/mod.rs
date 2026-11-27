//! Tests of the turn planner and what it reads from cards and enemies; the builders below are shared with tests/all/bosses (a module of their own would be an eleventh file here).

use serde_json::{json, Value};
use sts2bot::cards::{parse, CardInfo};
use sts2bot::fight::combat::{self, Enemy, Outcome, Plan, SimulateArgs};
use sts2bot::py::JsonExt;
use sts2bot::Action;

use crate::helpers;

mod test_arrows;
mod test_artifact;
mod test_closing;
mod test_combat;
mod test_counted;
mod test_debuffs;
mod test_melodies;
mod test_offers;
mod test_poison_if_poisoned;

/// helpers.combat_state in one call: also the round (`round_no`), the floor, "_" keys on the state and any other keyword on the player.
/// The keywords as a dict (`combat_state!(hand, enemies, energy = 1, round_no = 3, status = [power("VIGOR", 8)])` writes it).
pub fn combat_state_kw(hand: Vec<Value>, enemies: Vec<Value>, kw: Value) -> Value {
    let mut own = serde_json::Map::new();
    let mut rest = Vec::new();
    for (k, v) in kw.as_object().expect("keywords") {
        if ["energy", "hp", "max_hp", "block", "kind", "potions"].contains(&k.as_str()) {
            own.insert(k.clone(), v.clone());
        } else {
            rest.push((k, v));
        }
    }
    let mut s = helpers::combat_state_kw(hand, enemies, Value::Object(own));
    s["battle"]["round"] = json!(1);
    for (k, v) in rest {
        if k == "round_no" {
            s["battle"]["round"] = v.clone();
        } else if k == "floor" {
            if !v.is_null() {
                s["run"]["floor"] = v.clone();
            }
        } else if k.starts_with('_') {
            s[k.as_str()] = v.clone();
        } else {
            s["player"][k.as_str()] = v.clone();
        }
    }
    s
}

/// `combat_state(hand, enemies, energy=1, kind="boss", ...)`.
macro_rules! combat_state {
    ($hand:expr, $enemies:expr $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::fight::combat_state_kw(Vec::from($hand), Vec::from($enemies), serde_json::json!({$((stringify!($k)): $v),*}))
    };
}
pub(crate) use combat_state;

/// A status entry as the game writes it. (The Python's default: amount 1.)
pub fn power(name: &str, amount: i64) -> Value {
    json!({"id": format!("{name}_POWER"), "amount": amount})
}

/// (The Python's default: target "Self".)
pub fn potion(slot: i64, name: &str, text: &str, target: &str) -> Value {
    json!({"slot": slot, "id": name.to_uppercase().replace(' ', "_"), "name": name, "description": text, "target_type": target, "can_use_in_combat": true})
}

/// helpers.enemy dicts as the planner reads them.
pub fn read_foes(enemies: &[Value]) -> Vec<Enemy> {
    combat::read_enemies(&combat_state_kw(vec![], enemies.to_vec(), json!({})))
}

/// combat.simulate's keywords for a turn with no Stars (energy 3, hp 60, max_hp 80, block 0), and whatever else is given:
/// `ctx!(energy = 1, arrows = 1, fight = "boss")`.
macro_rules! ctx {
    ($($k:ident = $v:expr),* $(,)?) => {
        { sts2bot::fight::combat::SimulateArgs { $($k: ($v).into(),)* ..$crate::fight::ctx_base() } }
    };
}
pub(crate) use ctx;

pub fn ctx_base() -> SimulateArgs {
    SimulateArgs { energy: 3, stars: 0, hp: 60, max_hp: 80, block: 0, ..Default::default() }
}

/// What a line of parsed cards scores over playing nothing.
pub fn gain(line: &[CardInfo], foes: &[Enemy], kw: &SimulateArgs) -> f64 {
    combat::simulate(line, foes, kw).unwrap().score - combat::simulate::<CardInfo, _>(&[], foes, kw).unwrap().score
}

/// combat.simulate of these card dicts in this state, set up as choose_card sets it up, the keywords `set` changes on top.
pub fn outcome_with(state: &Value, cards: &[Value], set: impl FnOnce(&mut SimulateArgs)) -> Option<Outcome> {
    let mut kw = combat::_context(state);
    set(&mut kw);
    combat::simulate(&cards.iter().map(parse).collect::<Vec<_>>(), &combat::read_enemies(state), kw)
}

/// `outcome(state, cards)` (no cards: `&[]`).
pub fn outcome(state: &Value, cards: &[Value]) -> Option<Outcome> {
    outcome_with(state, cards, |_| {})
}

pub fn worth(state: &Value, cards: &[Value]) -> f64 {
    outcome(state, cards).unwrap().score - outcome(state, &[]).unwrap().score
}

pub fn worth_with(state: &Value, cards: &[Value], set: impl Fn(&mut SimulateArgs)) -> f64 {
    outcome_with(state, cards, &set).unwrap().score - outcome_with(state, &[], &set).unwrap().score
}

/// The hand index of the card the planner plays first.
pub fn pick(state: &Value) -> i64 {
    combat::choose_card(state, None).expect("a card to play").1["card_index"].as_i64().unwrap()
}

/// `pick(state, plan)`.
pub fn pick_plan(state: &Value, plan: &mut Plan) -> i64 {
    combat::choose_card(state, Some(plan)).expect("a card to play").1["card_index"].as_i64().unwrap()
}

pub fn aimed(state: &Value) -> Option<String> {
    combat::choose_card(state, None).expect("a card to play").1["target"].as_str().map(str::to_string)
}

/// Play the planner's cards as the game would, at most `limit`: each leaves the hand and costs its energy, block=true adds its "Gain N Block",
/// after(card, params) does the rest (true: stop); returns the cards played. (The Python's defaults: limit 3, block false, no after, combat.choose_card.)
pub fn play_turn(state: &mut Value, limit: usize, block: bool, mut after: Option<&mut dyn FnMut(&mut Value, &Value, &Value) -> bool>, choose: Option<&dyn Fn(&Value) -> Option<Action>>) -> Vec<Value> {
    let mut played = Vec::new();
    while played.len() < limit {
        let act = match choose {
            Some(choose) => choose(state),
            None => combat::choose_card(state, None),
        };
        let Some(act) = act.filter(|a| a.0 == "play_card") else { break };
        let hand = state["player"]["hand"].as_array_mut().unwrap();
        let at = hand.iter().position(|h| h["index"] == act.1["card_index"]).expect("the card played is in the hand");
        let c = hand.remove(at);
        played.push(c.clone());
        state["player"]["energy"] = json!(state["player"]["energy"].int() - c["cost"].int());
        let gained = match sts2bot::re!(r"Gain (\d+) Block").captures(c["description"].s()) {
            Some(m) if block => m[1].parse::<i64>().unwrap(),
            _ => 0,
        };
        state["player"]["block"] = json!(state["player"]["block"].int() + gained);
        if let Some(after) = after.as_mut() {
            if after(state, &c, &act.1) {
                break;
            }
        }
    }
    played
}

/// `play_turn(state)`.
pub fn play(state: &mut Value) -> Vec<Value> {
    play_turn(state, 3, false, None, None)
}
