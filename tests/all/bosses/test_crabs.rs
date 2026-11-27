//! Crusher and Rocket: one crab is brought down first, and only the last card turns us to face the other.

use serde_json::{json, Value};
use sts2bot::fight::combat::{self, Plan};

use crate::fake_game::{card, with};
use crate::fight::{aimed, combat_state_kw, power};
use crate::helpers::{enemy_kw, DEFEND, STRIKE};

/// `crabs(crusher_hp, rocket_hp, crusher_hit=None, rocket_hit=None, facing="CRUSHER_0", hand=(), **kw)`: the keywords as a dict.
fn crabs(crusher_hp: i64, rocket_hp: i64, crusher_hit: Option<i64>, rocket_hit: Option<i64>, facing: &str, hand: Vec<Value>, kw: Value) -> Value {
    let crusher = with(enemy_kw("CRUSHER_0", crusher_hp, json!({"attack": crusher_hit, "status": [power("BACK_ATTACK_LEFT", 1), power("CRAB_RAGE", 1)]})), json!({"max_hp": 206}));
    let rocket = with(enemy_kw("ROCKET_0", rocket_hp, json!({"attack": rocket_hit, "status": [power("BACK_ATTACK_RIGHT", 1), power("CRAB_RAGE", 1)]})), json!({"max_hp": 196}));
    combat_state_kw(hand, vec![crusher, rocket], with(json!({"kind": "boss", "status": [power("SURROUNDED", 1)], "_facing": facing}), kw))
}

// OneCrabFirst

#[test]
fn test_a_crab_on_its_last_hp_is_killed_while_the_other_is_still_whole() {
    // A Crusher on 5 HP stood for six rounds hitting for 22 while the Rocket came down from 168: killing it cost 2 points for each HP of the Rocket. The 6 Strength is worth 0.3.
    let state = crabs(5, 168, Some(22), None, "ROCKET_0", vec![STRIKE(0), DEFEND(1)], json!({"energy": 1, "hp": 40, "max_hp": 84}));
    assert_eq!(combat::choose_card(&state, None), Some(("play_card".to_string(), json!({"card_index": 0, "target": "CRUSHER_0"}))));
}

#[test]
fn test_a_kill_still_waits_when_it_would_waste_the_turn_on_99_block() {
    // Two Strikes, the Crusher on 5 and not attacking: the Rocket is hit first, then the Crusher dies.
    assert_eq!(aimed(&crabs(5, 168, None, Some(20), "ROCKET_0", vec![STRIKE(0), STRIKE(1)], json!({"energy": 2, "hp": 60, "max_hp": 84}))).as_deref(), Some("ROCKET_0"));
}

#[test]
fn test_the_damage_stays_on_one_crab_and_the_last_card_turns_us() {
    // The Rocket is about to hit for 49 from behind (33 from in front). Aiming the whole turn at it to turn round is how both crabs came down side by side; one Strike turns us as well as three.
    let mut plan = Plan::default();
    let first = combat::choose_card(&crabs(60, 150, None, Some(49), "CRUSHER_0", vec![STRIKE(0), STRIKE(1), STRIKE(2)], json!({"energy": 3, "hp": 60, "max_hp": 84})), Some(&mut plan)).unwrap();
    assert_eq!(first.1["target"], "CRUSHER_0");
    assert_eq!(plan.line.as_ref().unwrap().len(), 3);
    assert_eq!(plan.lost, Some(33)); // (it ends the turn facing the Rocket)
    assert_eq!(aimed(&crabs(48, 150, None, Some(49), "CRUSHER_0", vec![STRIKE(0)], json!({"energy": 1, "hp": 60, "max_hp": 84}))).as_deref(), Some("ROCKET_0")); // ... and the one card left turns us
}

#[test]
fn test_round_one_s_big_hit_goes_on_the_rocket_whatever_its_first_intent() {
    // Comet+ went on the Crusher, showing 22 against the Rocket's 5; the Crusher was then the one to kill, and the Rocket hit 31, 58 and 36 from 190 (battle.KNOWN_THREAT).
    let comet = card(0, "COMET", "Comet", "Attack", 1, "Deal 30 damage. Apply 3 Weak. Apply 3 Vulnerable.", "AnyEnemy");
    let state = crabs(219, 209, Some(21), Some(4), "ROCKET_0", vec![comet, STRIKE(1), DEFEND(2), DEFEND(3)], json!({"energy": 4, "hp": 70, "max_hp": 80}));
    assert_eq!(combat::choose_card(&state, None), Some(("play_card".to_string(), json!({"card_index": 0, "target": "ROCKET_0"}))));
    assert_eq!(combat::read_enemies(&state).iter().map(|e| e.threat).collect::<Vec<_>>(), [13.0, 20.0]);
}
