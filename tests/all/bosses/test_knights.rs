//! The act 3 knights: the Magi Knight, whose 35 comes on round 5, is brought down first (battle.KNOWN_THREAT).

use serde_json::{json, Value};
use sts2bot::cards::parse;
use sts2bot::fight::{combat, triggers};
use sts2bot::py::JsonExt;

use crate::fake_game::{card, with};
use crate::fight::{aimed, combat_state, power};
use crate::helpers::{enemy, DEFEND, STRIKE};

fn VULN() -> Value {
    json!([power("VULNERABLE", 2)])
}

fn HEX() -> Value {
    json!([{"name": "Hexed", "description": "Add Ethereal to this card."}])
}

/// Round 1 of the elite as logged on Ascension 8 (the Spectral Knight's Debuff makes every card Ethereal).
/// (The Python's defaults: flail 106, spectral 95, magi 87, hand a Strike.)
fn knights(flail: i64, spectral: i64, magi: i64, hand: Option<Vec<Value>>) -> Value {
    let out = [
        enemy!("FLAIL_KNIGHT_0", flail, attack = 15),
        with(enemy!("SPECTRAL_KNIGHT_0", spectral), json!({"intents": [{"type": "Debuff", "label": ""}]})),
        with(enemy!("MAGI_KNIGHT_0", magi), json!({"intents": [{"type": "Attack", "label": "6"}, {"type": "Defend", "label": ""}]})),
    ];
    combat_state!(hand.unwrap_or_else(|| vec![STRIKE(0)]), out.into_iter().filter(|e| e["hp"].int() > 0).collect::<Vec<_>>(), hp = 66, max_hp = 66, kind = "elite")
}

// KnightsInOrder

#[test]
fn test_the_magi_knight_is_hit_first_even_when_another_knight_is_vulnerable() {
    // Round 1's Putrefy+ put Weak and Vulnerable on the Flail Knight, and from then on that knight (counted as 62 HP) and the Spectral one took the attacks; the Magi Knight, untouched at 66, hit 35 on round 5.
    let mut state = knights(106, 95, 87, None);
    state["battle"]["enemies"][0]["status"] = VULN();
    assert_eq!(aimed(&state).as_deref(), Some("MAGI_KNIGHT_0"));
    state["battle"]["enemies"][1]["status"] = VULN(); // (the Spectral Knight too)
    assert_eq!(aimed(&state).as_deref(), Some("MAGI_KNIGHT_0"));
}

#[test]
fn test_whom_the_first_card_goes_at() {
    // The Flail Knight gains 3 Strength every fourth turn: in the model fight, Spectral Knight second won 54% of one deck's fights and Flail Knight second 70%, whatever the Hex costs.
    for (why, state, target) in [
        ("Vulnerable goes on the knight to kill first", knights(106, 95, 87, Some(vec![card(0, "EXPOSE", "Expose", "Skill", 1, "Apply 2 Vulnerable.", "AnyEnemy")])), "MAGI_KNIGHT_0"),
        ("a knight nearly dead is finished: 30 HP of 12 a turn, against 87 of 22", knights(106, 30, 87, None), "SPECTRAL_KNIGHT_0"),
        ("a kill on offer", knights(6, 95, 87, None), "FLAIL_KNIGHT_0"),
        ("then the Flail Knight before the Spectral one", knights(106, 95, 0, None), "FLAIL_KNIGHT_0"),
    ] {
        assert_eq!(aimed(&state).as_deref(), Some(target), "{why}");
    }
}

#[test]
fn test_other_fights_are_untouched() {
    let state = combat_state!([STRIKE(0)], [enemy!("A", 50, attack = 10), with(enemy!("B", 70, attack = 10), json!({"status": VULN()}))], hp = 80, kind = "elite");
    assert_eq!(combat::read_enemies(&state).iter().map(|e| e.focus).collect::<Vec<_>>(), [1.0, 1.0]);
    assert_eq!(aimed(&state).as_deref(), Some("B")); // (lowest health first, Vulnerable counting as less: 47 against 50)
}

// HexedCards

#[test]
fn test_the_hexed_keyword_makes_a_card_ethereal() {
    assert!(parse(&with(STRIKE(0), json!({"keywords": HEX()}))).ethereal);
    assert!(!parse(&STRIKE(0)).ethereal);
}

#[test]
fn test_under_hex_a_strike_is_played_rather_than_left_to_be_exhausted() {
    // Round 2 of a model fight (scripts/knightfight.rs, Strikes of 18 and Defends of 12): 66 of 75 HP, 18 and 15 coming, the Magi Knight on 51. Counting nothing lost, the planner kept to its Defends and let the Strikes be Exhausted; turns like it
    // left the deck with nothing to finish the Flail Knight, and the fight was lost.
    let hand: Vec<Value> = [DEFEND(0), STRIKE(1), STRIKE(2), DEFEND(3), DEFEND(4)]
        .into_iter()
        .map(|c| {
            let text = c["description"].s().replace('6', "18").replace('5', "12");
            with(c, json!({"description": text, "keywords": HEX()}))
        })
        .collect();
    let magi = with(enemy!("MAGI_KNIGHT_0", 51, block = 6), json!({"max_hp": 87, "intents": [{"type": "Debuff", "label": ""}]}));
    let pile = |cs: &[(&str, &str, &str)]| -> Vec<Value> { cs.iter().map(|(n, k, d)| json!({"name": n, "cost": k, "description": d})).collect() };
    let (strike, defend, heavy) = (("Strike", "1", "Deal 18 damage."), ("Defend", "1", "Gain 12 Block."), ("Heavy Blow", "2", "Deal 39 damage."));
    let state = combat_state!(
        hand,
        [enemy!("FLAIL_KNIGHT_0", 106, attack = "9x2"), enemy!("SPECTRAL_KNIGHT_0", 95, attack = 15), magi],
        hp = 66,
        max_hp = 75,
        kind = "elite",
        round_no = 2,
        status = [power("HEX", 2)],
        draw_pile = pile(&[("Bash", "2", "Deal 8 damage. Apply 2 Vulnerable."), heavy, strike, heavy, strike]),
        discard_pile = pile(&[strike, strike, defend, ("Clumsy", "-1", "Unplayable."), defend]),
        discard_pile_count = 5
    );
    assert_eq!(combat::choose_card(&state, None).unwrap().1, json!({"card_index": 1, "target": "MAGI_KNIGHT_0"}));
    let _kept = triggers::ETHEREAL_KEEP.set(0.0);
    assert_eq!(combat::choose_card(&state, None).unwrap().1, json!({"card_index": 0})); // (what it did)
}
