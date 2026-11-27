//! The Insatiable's Sandpit is priced by the rounds the fight still needs (src/fight/sandpit.rs).

use serde_json::{json, Value};
use sts2bot::fight::sandpit;
use sts2bot::py::JsonExt;

use crate::fake_game::{card, with};
use crate::fight::{combat_state, play_turn, power};
use crate::helpers::{enemy, STRIKE};

/// (The Python's default: cost 1.)
fn ESCAPE(i: i64, cost: i64) -> Value {
    card(i, "FRANTIC_ESCAPE", "Frantic Escape", "Status", cost, "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.", "None")
}

/// what is left to draw: an Escape in most hands
fn DECK() -> Vec<Value> {
    let mut deck = vec![json!({"name": "Strike"}); 20];
    deck.extend(vec![json!({"name": "Frantic Escape"}); 4]);
    deck
}

/// The hand indexes of the cards the planner plays this turn against the Insatiable, the Sandpit moving with each Escape.
/// (The Python's default: energy 3.)
fn plays(hand: Vec<Value>, hp: i64, pit: i64, round_no: i64, energy: i64) -> Vec<i64> {
    let boss = with(enemy!("INS", hp, attack = 0, status = [power("SANDPIT", pit)]), json!({"max_hp": 321}));
    let escapes = hand.iter().filter(|c| c["id"] == "FRANTIC_ESCAPE").count() as i64;
    let mut escaped = |state: &mut Value, c: &Value, _: &Value| -> bool {
        if c["index"].int() < 2 && escapes > c["index"].int() {
            let amount = &mut state["battle"]["enemies"][0]["status"][0]["amount"];
            *amount = json!(amount.int() + 1);
        }
        false
    };
    let mut state = combat_state!(hand, [boss], energy = energy, kind = "boss", round_no = round_no, draw_pile = DECK());
    play_turn(&mut state, 3, false, Some(&mut escaped), None).iter().map(|c| c["index"].int()).collect()
}

fn sorted(mut v: Vec<i64>) -> Vec<i64> {
    v.sort();
    v
}

// Sandpit

#[test]
fn test_margin_is_worth_nothing_past_the_end_of_the_fight() {
    let price = |left, boss_hp, dealt, round_no| sandpit::price(left, boss_hp, dealt, round_no, (0.3, 0.3));
    // The boss on 15 after this turn and dying at 35 a round: nothing is asked of a margin of 3.
    assert_eq!(price(3, 15, 300, 9), 0.0);
    assert!(price(2, 15, 300, 9) > 25.0); // ... but 2 still needs that kill
    // The same margin with most of the fight to come is a risk, and less of one for every point more.
    let early: Vec<f64> = [2, 3, 4, 5, 6].iter().map(|&left| price(left, 280, 40, 2)).collect();
    let mut falling = early.clone();
    falling.sort_by(|a, b| b.partial_cmp(a).unwrap());
    assert_eq!(early, falling);
    assert!(early[2] - early[3] > 25.0); // 4 to 5 was worth 11 points, whatever the boss had left
    assert_eq!(price(1, 280, 40, 2), sandpit::DEATH * 10.0);
}

#[test]
fn test_damage_that_shortens_the_fight_saves_an_escape() {
    assert!(sandpit::price(4, 250, 40, 2, (0.3, 0.3)) < sandpit::price(4, 280, 40, 2, (0.3, 0.3)) - 5.0);
}

#[test]
fn test_two_escapes_are_banked_while_the_fight_is_long() {
    // Round 2, Sandpit 4, the boss on 280: every Escape played now is one the fight will ask for anyway. (The second had been worth less than a Strike: 3 fights in 15 were lost in the pit after hands that held two Escapes and played one.)
    assert_eq!(sorted(plays(vec![ESCAPE(0, 1), ESCAPE(1, 1), STRIKE(2), STRIKE(3), STRIKE(4)], 280, 4, 2, 3)[..2].to_vec()), [0, 1]);
}

#[test]
fn test_no_escape_is_bought_for_a_fight_that_is_over() {
    // Round 9, the boss on 30 of 321, Sandpit 3: two Strikes leave it on 18 and a margin that outlasts it. An Escape for both energy is damage not dealt: six fights were lost on our HP with the boss on 13 to 49 and 3 to 5 on the Sandpit.
    assert_eq!(sorted(plays(vec![ESCAPE(0, 2), STRIKE(2), STRIKE(3)], 30, 3, 9, 2)), [2, 3]);
    assert_eq!(plays(vec![ESCAPE(0, 2), STRIKE(2), STRIKE(3)], 30, 1, 9, 2), [0]); // ... with the Sandpit on 1, the Escape or the pit
}
