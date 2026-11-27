//! The Watcher's Wrath carried into the next turn: what the planner charges for ending a turn in it.

use std::rc::Rc;

use serde_json::{json, Value};
use sts2bot::cards::parse;
use sts2bot::fight::combat::{self, Plan};
use sts2bot::games::sts1::screens::Screens;
use sts2bot::games::sts1::stance;
use sts2bot::run::strategy::{self, Memory};

use super::helpers::sts1_case;
use crate::fake_game::with;

/// id -> (type, cost, aimed)
const CARDS: [(&str, (&str, i64, bool)); 6] = [("Eruption", ("ATTACK", 1, true)), ("Strike_P", ("ATTACK", 1, true)), ("Defend_P", ("SKILL", 1, false)), ("Vigilance", ("SKILL", 2, false)),
                                               ("ClearTheMind", ("SKILL", 1, false)), ("Blasphemy", ("SKILL", 1, false))];
const NAMES: [(&str, &str); 3] = [("Strike_P", "Strike"), ("Defend_P", "Defend"), ("ClearTheMind", "Tranquility")];

/// (The Python's default: index None.)
pub fn card(cid: &str, index: Option<i64>) -> Value {
    let (kind, cost, aimed) = CARDS.iter().find(|(k, _)| *k == cid).unwrap_or_else(|| panic!("KeyError: '{cid}'")).1;
    let name = NAMES.iter().find(|(k, _)| *k == cid).map_or(cid, |(_, name)| *name);
    Screens::new().card(&json!({"id": cid, "name": name, "type": kind, "cost": cost, "has_target": aimed}), index, index.is_some())
}

/// `watcher`'s keywords, with the Python's defaults.
pub struct Kw {
    pub hp: i64,
    pub blow: i64,
    pub stance_now: &'static str,
    pub intent: Option<i64>,
    pub round_no: i64,
}

impl Default for Kw {
    fn default() -> Self {
        Kw { hp: 31, blow: 40, stance_now: "Neutral", intent: None, round_no: 5 }
    }
}

pub fn watcher_kw(hand: &[&str], draw: &[&str], kw: Kw) -> Value {
    let Kw { hp, blow, stance_now, intent, round_no } = kw;
    let intents = match intent {
        Some(intent) if intent != 0 => json!([{"type": "Attack", "label": intent.to_string()}]),
        _ => json!([{"type": "Defend", "label": ""}, {"type": "Buff", "label": ""}]),
    };
    let enemy = json!({"entity_id": "BRONZEAUTOMATON_0", "hp": 200, "max_hp": 300, "block": 0, "status": [], "intents": intents});
    let in_hand: Vec<Value> = hand.iter().enumerate().map(|(n, c)| card(c, Some(n as i64))).collect();
    let in_draw: Vec<Value> = draw.iter().map(|c| card(c, None)).collect();
    let player = json!({"hp": hp, "max_hp": 72, "block": 0, "energy": 2, "hand": in_hand, "draw_pile": in_draw,
                        "discard_pile": [], "draw_pile_count": draw.len(), "discard_pile_count": 0, "exhaust_pile_count": 0, "potions": [], "stance": stance_now, "orbs": []});
    json!({"state_type": "boss", "run": {"act": 2, "floor": 33, "ascension": 0}, "player": player, "_blow": {"BRONZEAUTOMATON_0": [blow, 0.6]},
           "battle": {"round": round_no, "turn": "player", "is_play_phase": true, "enemies": [enemy]}})
}

pub fn watcher(hand: &[&str], draw: &[&str]) -> Value {
    watcher_kw(hand, draw, Kw::default())
}

/// `[a] * n + [b] * m`.
fn pile(parts: &[(&'static str, usize)]) -> Vec<&'static str> {
    parts.iter().flat_map(|(c, n)| std::iter::repeat_n(*c, *n)).collect()
}

fn play(index: i64) -> sts2bot::Action {
    ("play_card".to_string(), json!({"card_index": index, "target": "BRONZEAUTOMATON_0"}))
}

// class WrathCarry

#[test]
fn test_the_odds_of_no_way_out_come_from_the_next_draw() {
    let _case = sts1_case();
    let player = watcher(&[], &pile(&[("Strike_P", 5), ("Defend_P", 5)]))["player"].clone();
    assert_eq!(stance::stuck_odds(&player, 0), 1.0);
    let player = watcher(&[], &pile(&[("Strike_P", 4), ("Vigilance", 1)]))["player"].clone();
    assert_eq!(stance::stuck_odds(&player, 0), 0.0); // (all five are drawn)
    let player = watcher(&[], &pile(&[("Strike_P", 9), ("Vigilance", 1)]))["player"].clone();
    assert!((stance::stuck_odds(&player, 0) - 0.5).abs() < 5e-8);
    let player = watcher(&["ClearTheMind"], &pile(&[("Strike_P", 10)]))["player"].clone();
    assert_eq!(stance::stuck_odds(&player, 0), 0.0); // (Tranquility is Retained)
    let player = watcher(&[], &pile(&[("Strike_P", 9), ("Blasphemy", 1)]))["player"].clone();
    assert_eq!(stance::stuck_odds(&player, 0), 1.0); // (Divinity, then death)
}

#[test]
fn test_no_wrath_before_a_blow_she_could_not_leave_it_for() {
    let _case = sts1_case();
    // A Boost turn, 31 HP, a 40 blow seen already and no Calm card in the next five: Eruption would leave her in Wrath for it.
    let state = watcher(&["Eruption", "Strike_P", "Strike_P"], &pile(&[("Strike_P", 5), ("Defend_P", 5)]));
    assert_eq!(combat::actions(&state)[0], play(1));
}

#[test]
fn test_wrath_when_the_next_hand_holds_a_way_out() {
    let _case = sts1_case();
    let state = watcher(&["Eruption", "Strike_P", "Strike_P"], &pile(&[("Strike_P", 4), ("Vigilance", 1)]));
    assert_eq!(combat::actions(&state)[0], play(0));
}

#[test]
fn test_wrath_when_the_blows_are_small() {
    let _case = sts1_case();
    let state = watcher_kw(&["Eruption", "Strike_P", "Strike_P"], &pile(&[("Strike_P", 5), ("Defend_P", 5)]), Kw { hp: 72, blow: 4, round_no: 3, ..Default::default() });
    assert_eq!(combat::actions(&state)[0], play(0));
}

// class SimmeringFury
// "At the start of your next turn, enter Wrath and draw 2 cards": next turn begins in Wrath (2026-11-26 20:02: the Hyper
// Beam came to 102 on 70 HP), which the planner had read as a card that does nothing.

fn fury(round_no: i64, draw: &[&str]) -> Value {
    let mut state = watcher_kw(&["Strike_P", "Strike_P"], draw, Kw { hp: 70, blow: 8, round_no, ..Default::default() });
    let fury = Screens::new().card(&json!({"id": "SimmeringFury", "name": "Simmering Fury", "type": "SKILL", "cost": 1}), Some(2), true);
    state["player"]["hand"].as_array_mut().unwrap().push(fury);
    state["player"]["energy"] = json!(3);
    state
}

/// `(combat.choose_card(state, plan := {}), plan)[1]["line"]`, as the cards' names.
fn planned(state: &Value) -> Vec<String> {
    let mut plan = Plan::default();
    combat::choose_card(state, Some(&mut plan));
    plan.line.expect("KeyError: 'line'").iter().map(|c| c.name.to_string()).collect()
}

#[test]
fn test_it_is_read_as_next_turn_wrath_and_two_cards() {
    let _case = sts1_case();
    let info = parse(&fury(3, &[])["player"]["hand"][2]);
    assert!(info.wrath_next);
    assert_eq!((info.next_draw, &*info.stance), (2, ""));
}

#[test]
fn test_no_fury_before_a_blow_it_could_not_leave_wrath_for() {
    let _case = sts1_case();
    let mut state = fury(5, &pile(&[("Strike_P", 5), ("Defend_P", 5)]));
    state["_blow"] = json!({"BRONZEAUTOMATON_0": [45, 1.0]}); // (a 45 blow seen: doubled, it kills her on 70)
    let line = planned(&state);
    assert!(!line.iter().any(|name| name == "Simmering Fury"), "{line:?}");
    // With a way out in the seven cards it draws, it is worth its 2 cards and the Calm it is not in.
    let mut state = fury(5, &pile(&[("Strike_P", 6), ("Vigilance", 1)]));
    state["_blow"] = json!({"BRONZEAUTOMATON_0": [45, 1.0]});
    let line = planned(&state);
    assert!(line.iter().any(|name| name == "Simmering Fury"), "{line:?}");
}

#[test]
fn test_the_stance_potion_is_a_choice_the_planner_weighs() {
    let _case = sts1_case();
    let mut s = Screens::new();
    s.raw = Rc::new(json!({"available_commands": ["choose"], "game_state": {"screen_type": "CARD_REWARD", "room_phase": "COMBAT", "screen_state": {"cards": [
        {"id": "Wrath", "name": "Wrath", "type": "POWER", "cost": -2}, {"id": "Calm", "name": "Calm", "type": "POWER", "cost": -2}]}}}));
    let offer = s._card_reward();
    assert_eq!(offer["card_reward"]["cards"].as_array().unwrap().iter().map(|c| &c["description"]).collect::<Vec<_>>(), ["Enter Wrath.", "Enter Calm."]);
    // On 31 HP with a 40 blow seen and no way out of Wrath in the next five: Calm.
    let fight = watcher(&["Strike_P", "Strike_P"], &pile(&[("Strike_P", 5), ("Defend_P", 5)]));
    let state = with(offer, json!({"state_type": "card_reward", "_fight_state": fight}));
    assert_eq!(strategy::card_reward(&state, &mut Memory::new())[0], ("select_card_reward".to_string(), json!({"card_index": 1})));
}

// class Mantra
// Mantra: at 10 she enters Divinity (Attacks tripled, 3 energy) at once.

#[test]
fn test_mantra_that_reaches_ten_is_divinity_now() {
    let _case = sts1_case();
    let prostrate = Screens::new().card(&json!({"id": "Prostrate", "name": "Prostrate", "type": "SKILL", "cost": 0}), Some(0), true);
    let mut state = watcher_kw(&["Strike_P", "Strike_P"], &pile(&[("Defend_P", 5)]), Kw { hp: 72, blow: 4, ..Default::default() });
    state["player"]["hand"] = json!([prostrate.clone(), card("Strike_P", Some(1)), card("Strike_P", Some(2))]);
    state["player"]["status"] = json!([{"id": "MANTRA_POWER", "amount": 8}]);
    state["battle"]["enemies"][0]["hp"] = json!(30); // (two Strikes tripled kill it: 6 x 3 x 2 = 36)
    assert_eq!(parse(&prostrate).mantra, 2);
    let (ctx, foes) = (combat::_context(&state), combat::read_enemies(&state));
    let line: Vec<_> = state["player"]["hand"].as_array().unwrap().iter().map(parse).collect();
    assert_eq!(ctx.mantra0, 8);
    assert!(combat::simulate(&line, &foes, &ctx).unwrap().score > combat::WIN_BONUS.get()); // (Prostrate first: Divinity, then the kill)
    let last: Vec<_> = line[1..].iter().chain(&line[..1]).cloned().collect();
    assert!(combat::simulate(&last, &foes, &ctx).unwrap().score < combat::WIN_BONUS.get()); // (Prostrate last: too late)
}
