//! The Ancients' relics: rated by how far the runs that held them went (relics.rs), from the table's own numbers.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{json, Value};
use sts2bot::py;
use sts2bot::run::events::{self, EventOptionValueArgs};
use sts2bot::run::strategy::{self, Memory};
use sts2bot::run::relics;
use sts2bot::Action;

/// (The Python's defaults: result "loss", who "The Regent", ascension 7.)
fn run_(floor: i64, held: &[&str], result: &str, who: &str, ascension: i64) -> Value {
    json!({"character": who, "ascension": ascension, "floor": floor, "result": result, "relics": held})
}

fn run(floor: i64, held: &[&str]) -> Value {
    run_(floor, held, "loss", "The Regent", 7)
}

/// `setUp`: no runs on record. (The record is the thread's, so none is left behind for the other tests.)
fn set_up() {
    relics::load(None, events::_ANCIENT.keys());
}

/// `events._event_option_value({"title": title}, 60, 75)`.
fn value(title: &str) -> f64 {
    events::_event_option_value(&json!({"title": title}), 60, 75, EventOptionValueArgs::default())
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 5e-8, "{a} != {b}"); // (assertAlmostEqual: 7 places)
}

fn held(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

// WhatTheAncientsRelicsDid

#[test]
fn test_with_no_runs_on_record_the_table_stands() {
    set_up();
    assert_eq!(relics::shift("SEAL_OF_GOLD"), 0.0);
    assert_eq!(value("Seal of Gold"), events::_ANCIENT["SEAL_OF_GOLD"]);
}

#[test]
fn test_a_relic_whose_runs_go_further_is_rated_up_and_the_other_down() {
    set_up();
    for _ in 0..30 {
        relics::note(&run(40, &["STONE_HUMIDIFIER"]), false);
        relics::note(&run(20, &["BOOMING_CONCH"]), false);
    }
    relics::note(&run(30, &["FISHING_ROD"]), true);
    assert!(relics::shift("STONE_HUMIDIFIER") > 2.0);
    assert!(relics::shift("BOOMING_CONCH") < -2.0);
    assert!(relics::shift("FISHING_ROD").abs() <= 0.1);
    // ... and never by more than MAX_SHIFT, whatever the runs did
    assert!(relics::shift("STONE_HUMIDIFIER") <= relics::MAX_SHIFT);
    assert!(relics::shift("BOOMING_CONCH") >= -relics::MAX_SHIFT);
    approx(value("Stone Humidifier"), events::_ANCIENT["STONE_HUMIDIFIER"] + relics::shift("STONE_HUMIDIFIER"));
}

#[test]
fn test_a_few_runs_move_a_rating_little() {
    set_up();
    for _ in 0..20 {
        relics::note(&run(30, &["FISHING_ROD"]), false);
    }
    relics::note(&run_(48, &["LAVA_ROCK"], "win", "The Regent", 7), true); // one run, 31 floors over the usual with the win
    assert!(relics::shift("LAVA_ROCK") < 1.5);
}

#[test]
fn test_a_relic_is_judged_from_the_floor_it_is_taken_on() {
    set_up();
    // Every run that holds an act 3 relic has reached act 3: against all runs it would look 15 floors good.
    for _ in 0..20 {
        relics::note(&run(25, &["FISHING_ROD"]), false);
        relics::note(&run(45, &["FISHING_ROD", "PAELS_BLOOD", "SAI"]), false);
        relics::note(&run(45, &["FISHING_ROD", "PAELS_BLOOD", "FUR_COAT"]), false);
    }
    relics::note(&run(45, &["FISHING_ROD", "PAELS_BLOOD", "SAI"]), true);
    assert!(relics::shift("SAI").abs() <= 0.1);
    assert!(relics::shift("FUR_COAT").abs() <= 0.1);
}

#[test]
fn test_darv_is_met_in_act_two_or_three() {
    set_up();
    assert_eq!(relics::picked_at("FISHING_ROD", &held(&["FISHING_ROD"])), 1);
    assert_eq!(relics::picked_at("PAELS_BLOOD", &held(&["PAELS_BLOOD", "SOZU"])), 18);
    assert_eq!(relics::picked_at("SOZU", &held(&["FISHING_ROD", "SOZU", "SAI"])), 18);
    assert_eq!(relics::picked_at("SOZU", &held(&["FISHING_ROD", "PAELS_BLOOD", "SOZU"])), 34); // Pael opened act 2, so Darv act 3
    assert_eq!(relics::picked_at("SAI", &held(&["SAI"])), 34);
}

#[test]
fn test_the_usual_is_the_same_characters_on_the_same_ascensions() {
    set_up();
    // A character that goes further by itself is not a better relic.
    for _ in 0..20 {
        relics::note(&run_(40, &["STONE_HUMIDIFIER"], "loss", "The Defect", 7), false);
        relics::note(&run_(20, &["FISHING_ROD"], "loss", "The Necrobinder", 7), false);
        relics::note(&run_(20, &["LAVA_ROCK"], "loss", "The Regent", 2), false);
    }
    relics::note(&run_(40, &["STONE_HUMIDIFIER"], "loss", "The Defect", 7), true);
    for name in ["STONE_HUMIDIFIER", "FISHING_ROD", "LAVA_ROCK"] {
        assert!(relics::shift(name).abs() <= 0.01);
    }
}

#[test]
fn test_the_record_is_read_from_the_run_history() {
    set_up();
    {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("runs.jsonl");
        let mut lines: Vec<String> = (0..30).map(|_| py::json_dumps(&run(40, &["STONE_HUMIDIFIER"]))).chain((0..30).map(|_| py::json_dumps(&run(20, &["BOOMING_CONCH"])))).collect();
        lines.extend(["not json".to_string(), py::json_dumps(&json!({"character": "The Regent", "floor": 33, "result": "loss"}))]); // (an old line: no relics)
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        relics::load(Some(&path), events::_ANCIENT.keys());
    }
    assert_eq!(relics::record()["STONE_HUMIDIFIER"].0, 30);
    assert!(relics::shift("STONE_HUMIDIFIER") > 2.0);
    assert_eq!(relics::shift("WAX_CHOKER"), 0.0); // not an Ancient's: nothing to move
}

#[test]
fn test_a_missing_history_is_an_empty_record() {
    set_up();
    relics::load(Some(Path::new("/nonexistent/runs.jsonl")), events::_ANCIENT.keys());
    assert!(relics::record().is_empty());
}

// AncientRatings

#[test]
fn test_every_relic_with_an_act_has_a_rating_or_is_left_to_its_text() {
    // (A relic the table does not know is read word by word, as before: nothing breaks, it just is not learned.)
    for name in ["ASTROLABE", "EMPTY_CAGE", "SEAL_OF_GOLD", "PAELS_BLOOD", "SAI"] {
        assert!(events::_ANCIENT.contains_key(name));
    }
}

#[test]
fn test_relics_once_read_word_by_word_no_longer_outrank_an_energy_every_turn() {
    assert!(value("Philosopher's Stone") > value("Astrolabe")); // (13.5 by its text)
    assert!(value("Sozu") >= value("Empty Cage")); // (11.5)
    assert!(value("Seal of Gold") > value("Biiig Hug"));
}

// SealOfGold

fn _shop(held: &[&str], gold: i64) -> Vec<Action> {
    let items = json!([{"index": 0, "category": "relic", "price": 150, "is_stocked": true, "can_afford": true}, {"index": 1, "category": "card_removal", "price": 100, "is_stocked": true, "can_afford": true}]);
    let player = json!({"gold": gold, "relics": held.iter().map(|r| json!({"id": r})).collect::<Vec<_>>(), "potions": [], "max_potion_slots": 3});
    strategy::shop(&json!({"state_type": "shop", "run": {"act": 2, "floor": 25}, "player": player, "shop": {"items": items, "can_proceed": true}}), &mut Memory::new(), "shop")
}

#[test]
fn test_a_merchant_does_not_get_the_gold_the_seal_turns_into_energy() {
    let buy = |i: i64| ("shop_purchase".to_string(), json!({"index": i}));
    let proceed = ("proceed".to_string(), json!({}));
    // 3 Gold a turn for an energy: with 170 the relic at 150 would leave 20, seven turns of a boss fight.
    assert_eq!(_shop(&["BURNING_BLOOD"], 170)[0], buy(1));
    assert_eq!(_shop(&["SEAL_OF_GOLD"], 170), [buy(1), proceed.clone()]);
    assert_eq!(_shop(&["SEAL_OF_GOLD"], 140), [proceed]); // nothing leaves 45
    assert_eq!(_shop(&["SEAL_OF_GOLD"], 400).len(), 3); // with plenty, everything as before
}
