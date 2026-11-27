//! The Fool's powers that pay out on a new Melody (Song Path, Motif, Lullaby), Ki Verse, and Yah!'s energy when an Arrow comes out of an enemy. As read live: the first Cho
//! card of a turn gave Song Path's Block and the ones after it nothing more; Motif drew on the turn's first Wah, Ki and Cho card only; Yah!+ gave 2 energy on the Arrow's third hit and on the death of the enemy holding it.

use std::collections::BTreeSet;

use serde_json::{json, Value};
use sts2bot::cards::parse;
use sts2bot::fight::combat::{self, Plan, SimulateArgs};
use sts2bot::fight::fool::turn;

use super::{combat_state, ctx, pick, power, read_foes};
use crate::fake_game::{card, with};
use crate::helpers::{enemy, DEFEND, STRIKE};

fn IMPALED(n: i64) -> Value {
    power("IMPALED", n)
}

fn CHO(i: i64) -> Value {
    card(i, "CHO_GUARD", "Cho Guard", "Skill", 1, "Gain 6 Block.", "Self")
}

/// (The Python's default: 6 damage.)
fn WAH(i: i64, dmg: i64) -> Value {
    card(i, "WAH_JAB", "Wah Jab", "Attack", 1, &format!("Deal {dmg} damage."), "AnyEnemy")
}

fn KI(i: i64) -> Value {
    card(i, "KI_JAB", "Ki Jab", "Attack", 1, "Deal 7 damage.", "AnyEnemy")
}

const VERSE_TEXT: &str = "The next “Wah”, “Cho”, or “Ki” card you play this turn is played an additional time.";
const VERSE_PLUS: &str = "The next 2 “Wah”, “Cho”, or “Ki” cards you play this turn are played an additional time.";

/// (The Python's default: not the upgraded one.)
fn VERSE(i: i64, plus: bool) -> Value {
    card(i, "KI_VERSE", if plus { "Ki Verse+" } else { "Ki Verse" }, "Skill", 1, if plus { VERSE_PLUS } else { VERSE_TEXT }, "None")
}

fn KW() -> SimulateArgs {
    ctx!(fight = "boss")
}

fn tune(melodies: &[&str]) -> BTreeSet<String> {
    melodies.iter().map(|m| m.to_string()).collect()
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 5e-8, "{a} != {b}"); // (assertAlmostEqual: 7 places)
}

// Reading

#[test]
fn test_ki_verse_says_how_many_cards_it_plays_again() {
    assert_eq!(parse(&VERSE(0, false)).verse, 1);
    assert_eq!(parse(&VERSE(0, true)).verse, 2);
    assert_eq!(parse(&STRIKE(0)).verse, 0);
    assert_eq!(parse(&VERSE(0, false)).melody, ["KI"]); // (a Ki card itself)
}

#[test]
fn test_the_powers_are_read_off_us_and_yah_goes_once_an_arrow_is_out() {
    let player = json!({"status": [power("SONG_PATH", 7), power("MOTIF", 1), power("LULLABY", 20), power("KI_VERSE", 2), power("YAH", 2)]});
    assert_eq!(turn::fool_powers(&player, false), Some((7, 1, 20, 2, 2)));
    assert_eq!(turn::fool_powers(&player, true), Some((7, 1, 20, 2, 0)));
    assert_eq!(turn::fool_powers(&json!({"status": []}), false), None);
    let state = combat_state!([STRIKE(0)], [enemy!("A", 30, attack = 5)], status = [power("YAH", 1)]);
    assert_eq!(combat::_context(&state).fool, Some((0, 0, 0, 0, 1)));
    assert_eq!(combat::_context(&with(state, json!({"_dislodged": true}))).fool, None);
}

// FoolPowers

#[test]
fn test_song_path_blocks_on_the_first_cho_card_of_the_turn() {
    let (cho, hit) = (parse(&CHO(0)), read_foes(&[enemy!("A", 300, attack = 20)]));
    let song = SimulateArgs { fool: Some((5, 0, 0, 0, 0)), ..KW() };
    assert_eq!(combat::simulate(&[cho.clone()], &hit, KW()).unwrap().lost, 14);
    assert_eq!(combat::simulate(&[cho.clone()], &hit, &song).unwrap().lost, 9);
    assert_eq!(combat::simulate(&[cho.clone(), parse(&CHO(1))], &hit, &song).unwrap().lost, 3); // (once a turn)
    assert_eq!(combat::simulate(&[cho], &hit, SimulateArgs { tune: tune(&["CHO"]), ..song.clone() }).unwrap().lost, 14);
}

#[test]
fn test_song_path_puts_the_cho_card_before_a_bigger_defend() {
    let mut state = combat_state!([card(0, "DEFEND_R", "Defend", "Skill", 1, "Gain 7 Block.", "Self"), CHO(1)], [enemy!("A", 300, attack = 11)], energy = 1);
    assert_eq!(pick(&state), 0);
    state["player"]["status"] = json!([power("SONG_PATH", 5)]);
    assert_eq!(pick(&state), 1);
}

#[test]
fn test_motif_draws_on_each_new_melody() {
    let (line, foes) = ([parse(&WAH(0, 6)), parse(&CHO(1)), parse(&WAH(2, 6))], read_foes(&[enemy!("A", 300)]));
    let motif = SimulateArgs { fool: Some((0, 1, 0, 0, 0)), ..KW() };
    let plain = combat::simulate(&line, &foes, KW()).unwrap().score;
    let drew = combat::simulate(&line, &foes, &motif).unwrap().score;
    approx(drew - plain, 2.0 * 2.0); // Wah and Cho: two cards (the second Wah draws none)
    let held = combat::simulate(&line, &foes, SimulateArgs { tune: tune(&["WAH", "CHO"]), ..motif.clone() }).unwrap().score;
    approx(held, combat::simulate(&line, &foes, SimulateArgs { tune: tune(&["WAH", "CHO"]), ..KW() }).unwrap().score);
}

#[test]
fn test_lullaby_blocks_once_all_three_melodies_are_in() {
    let (hit, two) = (read_foes(&[enemy!("A", 300, attack = 30)]), [parse(&WAH(0, 6)), parse(&CHO(1))]);
    let lullaby = SimulateArgs { fool: Some((0, 0, 20, 0, 0)), ..KW() };
    assert_eq!(combat::simulate(&two, &hit, &lullaby).unwrap().lost, 24);
    assert_eq!(combat::simulate(&[two[0].clone(), two[1].clone(), parse(&KI(2))], &hit, &lullaby).unwrap().lost, 4);
    assert_eq!(combat::simulate(&[parse(&KI(2))], &hit, SimulateArgs { tune: tune(&["WAH", "CHO"]), ..lullaby.clone() }).unwrap().lost, 10);
}

#[test]
fn test_a_waiting_ki_verse_plays_the_next_melody_card_twice() {
    // Ki Jab (7) into an enemy on 14 that is about to hit for 10: a kill only when it is played twice.
    let (hit, ki) = (read_foes(&[enemy!("A", 14, attack = 10)]), parse(&KI(0)));
    let verse = SimulateArgs { fool: Some((0, 0, 0, 1, 0)), ..KW() };
    assert_eq!(combat::simulate(&[ki.clone()], &hit, KW()).unwrap().lost, 10);
    assert_eq!(combat::simulate(&[ki], &hit, &verse).unwrap().lost, 0);
    assert_eq!(combat::simulate(&[parse(&STRIKE(1))], &read_foes(&[enemy!("A", 12, attack = 10)]), &verse).unwrap().lost, 10); // (not a Strike)
}

#[test]
fn test_ki_verse_goes_before_the_cards_it_plays_twice_and_not_last() {
    let (verse, ki, foes) = (parse(&VERSE(0, false)), parse(&KI(1)), read_foes(&[enemy!("A", 300)]));
    assert!(combat::simulate(&[verse.clone(), ki.clone()], &foes, KW()).unwrap().score > combat::simulate(&[ki, verse], &foes, KW()).unwrap().score + 5.0);
    assert_eq!(pick(&combat_state!([VERSE(0, false), KI(1), STRIKE(2)], [enemy!("A", 300, attack = 5)], energy = 3)), 0);
    let mut plan = Plan::default();
    combat::choose_card(&combat_state!([VERSE(0, false), STRIKE(1)], [enemy!("A", 300, attack = 5)], energy = 2), Some(&mut plan));
    assert!(!plan.line.unwrap().iter().any(|c| c.key == "KI_VERSE")); // nothing to play twice: it was played last, for nothing
}

#[test]
fn test_the_arrows_third_hit_pays_for_the_next_card() {
    let (strike, defend, one) = (parse(&STRIKE(0)), parse(&DEFEND(1)), SimulateArgs { energy: 1, ..KW() });
    let yah = SimulateArgs { fool: Some((0, 0, 0, 0, 1)), ..one.clone() };
    let stuck = read_foes(&[enemy!("A", 300, attack = 10, status = [IMPALED(1)])]);
    assert!(combat::simulate(&[strike.clone(), defend.clone()], &stuck, &one).is_none());
    assert!(combat::simulate(&[strike.clone(), defend.clone()], &stuck, &yah).is_some());
    assert!(combat::simulate(&[strike.clone(), defend.clone()], &read_foes(&[enemy!("A", 300, attack = 10, status = [IMPALED(2)])]), &yah).is_none());
    assert!(combat::simulate(&[defend, strike], &stuck, &yah).is_none()); // (the hit must come first)
}

#[test]
fn test_the_death_of_the_enemy_holding_the_arrow_counts_and_only_once_a_turn() {
    let line: Vec<_> = (0..3).map(|i| parse(&STRIKE(i))).collect();
    let two = read_foes(&[enemy!("A", 6, attack = 10, status = [IMPALED(3)]), enemy!("B", 300, attack = 10, status = [IMPALED(1)])]);
    let yah = SimulateArgs { energy: 1, fool: Some((0, 0, 0, 0, 1)), ..KW() };
    assert!(combat::simulate(&line[..2], &two, &yah).is_some());
    assert!(combat::simulate(&line, &two, &yah).is_none()); // (the second Arrow out pays nothing)
}

#[test]
fn test_the_bot_notes_an_arrow_coming_out_this_turn() {
    let mut bot = crate::helpers::make_bot_none();
    bot._combat(&combat_state!([STRIKE(0), STRIKE(1)], [enemy!("A", 300, attack = 5, status = [IMPALED(1)])], energy = 2));
    assert!(!bot._dislodged);
    let out = combat_state!([STRIKE(1)], [enemy!("A", 294, attack = 5)], energy = 1);
    bot._combat(&out);
    assert!(bot._dislodged);
    bot._combat(&out);
    assert!(bot._dislodged); // (for the rest of the turn)
    bot._combat(&combat_state!([STRIKE(0)], [enemy!("A", 294, attack = 5, status = [IMPALED(3)])], energy = 3, round_no = 2));
    assert!(!bot._dislodged);
}

fn ENNUI(i: i64) -> Value {
    card(i, "ENNUI", "Ennui", "Power", 1, "At the start of your turn, add 1 Sadness to your Hand and draw 2 additional cards.", "Self")
}

fn SORROW(i: i64) -> Value {
    card(i, "SORROWFUL_STRIKE", "Sorrowful Strike", "Attack", 1, "Deal 13 damage. Add 2 Sadness into your Hand.", "AnyEnemy")
}

fn SADNESS(i: i64) -> Value {
    with(card(i, "SADNESS", "Sadness", "Status", 0, "Unplayable. Retain.", "Self"), json!({"can_play": false}))
}

fn KIN() -> Vec<Value> {
    vec![enemy!("KIN_PRIEST_0", 190, attack = 8), enemy!("KIN_FOLLOWER_0", 59, attack = 7), enemy!("KIN_FOLLOWER_1", 59, attack = 4)]
}

// Sadness
// Sadness stays in the hand all fight (fight/fool/sadness.rs); 2026-11-25 18:06 the Kin won on a hand of ten of them.

#[test]
fn test_cards_say_what_sadness_they_make_and_take_away() {
    assert_eq!(parse(&SORROW(0)).sadness, 2);
    assert_eq!(parse(&card(0, "MOURNING_BLOW", "Mourning Blow", "Attack", 0, "Deal 10 damage. Transform this into Sadness and return it to your Hand.", "AnyEnemy")).sadness, 1);
    assert_eq!(parse(&card(0, "VITRIOL", "Vitriol", "Skill", 0, "Apply 2 Weak. Add a Sadness into your Hand.", "AnyEnemy")).sadness, 1);
    assert_eq!(parse(&card(0, "PUNCH_OF_JOY", "Punch of Joy", "Attack", 2, "Deal 13 damage. Transform 1 random Sadness in your Hand into Joy.", "AnyEnemy")).sadness, -1);
    assert_eq!(parse(&ENNUI(0)).sad_turn, Some((1, 2)));
    assert_eq!(parse(&card(0, "ECSTASY", "Ecstasy", "Power", 2, "At the start of your turn, Transform 1 random Sadness in your Hand into Joy.", "Self")).sad_turn, Some((-1, 0)));
}

#[test]
fn test_no_ennui_at_the_start_of_a_long_fight() {
    let state = combat_state!([DEFEND(0), WAH(1, 12), STRIKE(2), STRIKE(3), ENNUI(4)], KIN(), round_no = 2, kind = "boss", hp = 84, max_hp = 84);
    assert_ne!(pick(&state), 4);
}

#[test]
fn test_ennui_in_a_short_fight() {
    let state = combat_state!([STRIKE(0), DEFEND(1), ENNUI(2)], [enemy!("TOADPOLE_0", 20, attack = 5)]);
    assert_eq!(pick(&state), 2);
}

#[test]
fn test_a_crowded_hand_takes_no_more_sadness_in_a_long_fight() {
    let mut hand: Vec<Value> = (0..6).map(SADNESS).collect();
    hand.extend([SORROW(6), STRIKE(7), DEFEND(8)]);
    let state = combat_state!(hand, KIN(), round_no = 5, kind = "boss", hp = 84, max_hp = 84);
    assert_eq!(pick(&state), 7);
    let state = combat_state!([SADNESS(0), SORROW(1), STRIKE(2), DEFEND(3)], KIN(), round_no = 3, kind = "boss", hp = 84, max_hp = 84);
    assert_eq!(pick(&state), 1); // (one in the hand: room for two more)
}
