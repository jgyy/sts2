//! Artifact on an enemy: a debuff it swallows is worth the later one that lands in its place (src/fight/artifact.rs).

use serde_json::{json, Value};
use sts2bot::cards::parse;
use sts2bot::fight::{artifact, combat};

use super::{combat_state, pick, power};
use crate::helpers::{card, enemy, STRIKE};

fn SCOURGE(i: i64) -> Value {
    card(i, "SCOURGE", "Scourge", "Skill", 1, "Apply 13 Doom.", "AnyEnemy")
}

fn PILE(name: &str, text: &str, n: usize) -> Vec<Value> {
    vec![json!({"name": name, "cost": "1", "description": text}); n]
}

fn DOOM_DECK() -> Vec<Value> {
    [PILE("Scourge", "Apply 13 Doom.", 3), PILE("No Escape", "Apply 10 Doom.", 2), PILE("Strike", "Deal 6 damage.", 20)].concat()
}

/// (The Python's default: 3 charges; its other keywords: the energy.)
fn state(hand: Vec<Value>, pile: Vec<Value>, charges: i64, energy: i64) -> Value {
    combat_state!(hand, [enemy!("BOSS", 480, status = [power("ARTIFACT", charges)])], kind = "boss", draw_pile = pile, discard_pile = [], energy = energy)
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 5e-8, "{a} != {b}"); // (assertAlmostEqual: 7 places)
}

// StrippingArtifact

#[test]
fn test_a_doom_deck_spends_spare_energy_on_the_charges() {
    // Round 2 of Aeonglass, a Scourge left in the hand and Artifact 3 on the boss until round 9: the Doom is swallowed, and the next one is a charge nearer to landing.
    let choice = combat::choose_card(&state(vec![SCOURGE(0)], DOOM_DECK(), 3, 1), None);
    assert!(choice.is_some());
    assert_eq!(choice.unwrap().1["card_index"], 0);
}

#[test]
fn test_a_deck_that_cannot_wear_them_off_does_not_try() {
    // One debuff card in thirty and three charges: nothing would ever land, so the strip buys nothing.
    let trip = card(0, "TRIP", "Trip", "Skill", 1, "Apply 2 Vulnerable.", "AnyEnemy");
    assert!(combat::choose_card(&state(vec![trip], PILE("Strike", "Deal 6 damage.", 29), 3, 1), None).is_none());
}

#[test]
fn test_a_strike_is_not_given_up_for_a_charge() {
    // A charge is a debuff later, at a discount: with one energy the damage now still goes first, and with two both are played.
    let weak = card(0, "INTIMIDATE", "Intimidate", "Skill", 1, "Apply 1 Weak.", "AnyEnemy");
    let deck = [PILE("Intimidate", "Apply 1 Weak.", 8), PILE("Strike", "Deal 6 damage.", 12)].concat();
    assert_eq!(pick(&state(vec![weak.clone(), STRIKE(1)], deck.clone(), 3, 1)), 1);
    assert!(combat::choose_card(&state(vec![weak, STRIKE(1)], deck, 3, 2), None).is_some());
}

#[test]
fn test_no_artifact_no_change() {
    assert_eq!(combat::_context(&combat_state!([SCOURGE(0), STRIKE(1)], [enemy!("BOSS", 480)], kind = "boss", energy = 1)).strip, (0.0, 0.0));
}

// WhatAChargeIsWorth

#[test]
fn test_the_deck_s_own_debuffs_set_the_price() {
    let (each, coming) = artifact::outlook(&json!({"hand": [], "draw_pile": DOOM_DECK(), "discard_pile": []}), "boss", 2);
    approx(each, artifact::LATER * 0.8 * (3 * 13 + 2 * 10) as f64 / 5.0);
    approx(coming, (5 * 7 * 5) as f64 / 25.0); // five Doom cards in 25, seven rounds to go, five cards a round
}

#[test]
fn test_only_charges_the_deck_will_get_past_count() {
    assert_eq!(artifact::gain(3, 2, (10.0, 7.0)), 10.0);
    assert_eq!(artifact::gain(3, 1, (10.0, 7.0)), 20.0);
    assert_eq!(artifact::gain(3, 2, (10.0, 2.0)), 0.0); // two more debuffs to come, two charges still in the way
    assert_eq!(artifact::gain(3, 2, (10.0, 2.5)), 5.0);
    assert_eq!(artifact::gain(1, 0, (10.0, 0.4)), 4.0);
}

#[test]
fn test_a_card_with_two_debuffs_uses_two_charges() {
    let both = parse(&card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage. Apply 1 Weak. Apply 1 Vulnerable.", "AnyEnemy"));
    assert_eq!(artifact::landed(&both, "boss").0, 2);
    assert_eq!(artifact::landed(&parse(&STRIKE(0)), "boss"), (0, 0.0));
}
