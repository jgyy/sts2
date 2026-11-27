//! Cards whose numbers depend on the hand they are played from.

use serde_json::Value;

use super::{combat_state, outcome, pick, power, worth};
use crate::fake_game::{card, with};
use crate::helpers::{enemy, DEFEND, STRIKE};

fn FIRE(i: i64) -> Value {
    card(i, "FIEND_FIRE", "Fiend Fire", "Attack", 2, "Exhaust your Hand. Deal 7 damage for each card Exhausted. Exhaust.", "AnyEnemy")
}

fn LETTING(i: i64) -> Value {
    card(i, "BLOODLETTING", "Bloodletting+", "Skill", 0, "Lose 3 HP. Gain [ironclad_energy_icon.png][ironclad_energy_icon.png][ironclad_energy_icon.png].", "Self")
}

// FiendFire

#[test]
fn test_it_hits_once_for_every_card_left_in_the_hand() {
    let s = combat_state!([STRIKE(0), FIRE(1), DEFEND(2), STRIKE(3), DEFEND(4)], [enemy!("BEAST_0", 200)], energy = 3);
    assert_eq!(worth(&s, &[FIRE(1)]), 7.0 * 4.0);
    assert_eq!(worth(&s, &[STRIKE(0), FIRE(1)]), 6.0 + 7.0 * 3.0); // the Strike played first is not there to burn
    assert!(outcome(&s, &[FIRE(1), STRIKE(0)]).is_none()); // nothing of this hand comes after it
}

#[test]
fn test_with_one_play_allowed_it_is_the_play() {
    // Ringing, the Ceremonial Beast on 44 and Vulnerable with 18 coming, four cards beside the Fiend Fire: the bot played a Defend, and next turn its 42 damage left the boss on 2.
    let beast = with(enemy!("CEREMONIAL_BEAST_0", 44, attack = 18, status = [power("VULNERABLE", 1), power("STRENGTH", 3)]), serde_json::json!({"max_hp": 262}));
    let s = combat_state!([STRIKE(0), LETTING(1), FIRE(2), DEFEND(3), STRIKE(4)], [beast], energy = 3, hp = 27, max_hp = 87, kind = "boss", round_no = 8, status = [power("RINGING", 1)]);
    assert_eq!(pick(&s), 2);
}

#[test]
fn test_it_does_not_burn_a_power_it_could_play_next_turn() {
    let form = card(1, "DEMON_FORM", "Demon Form", "Power", 3, "At the start of your turn, gain 2 Strength.", "Self");
    let mut s = combat_state!([FIRE(0), form, STRIKE(2), STRIKE(3), DEFEND(4)], [enemy!("BEAST_0", 200, attack = 10)], energy = 2, kind = "boss");
    assert_ne!(pick(&s), 0); // 28 damage for a Demon Form: no
    s["player"]["hand"][1] = STRIKE(1);
    assert_eq!(pick(&s), 0); // with a Strike in its place: yes
}

// SkillsInTheHand

#[test]
fn test_one_skill_a_turn_is_planned_as_one() {
    // The Living Fog's "You can only play 1 Skill per turn": a plan of two Defends cannot happen, so the Strike goes too.
    let shrug = card(0, "SHRUG_IT_OFF", "Shrug It Off", "Skill", 1, "Gain 8 Block.", "Self");
    let mut s = combat_state!([shrug, DEFEND(1), STRIKE(2)], [enemy!("LIVING_FOG_0", 40, attack = 13)], energy = 2);
    assert_eq!(pick(&s), 0); // free to: 13 Block for the 13
    s["player"]["status"] = serde_json::json!([power("SMOGGY", 1)]);
    assert_eq!(pick(&s), 2); // one Skill: the Strike, then Shrug It Off
}

#[test]
fn test_a_hit_for_each_skill_in_the_hand() {
    let darts = card(0, "FLECHETTES", "Flechettes", "Attack", 1, "Deal 5 damage for each Skill in your Hand.", "AnyEnemy");
    assert_eq!(pick(&combat_state!([darts.clone(), DEFEND(1), DEFEND(2), STRIKE(3)], [enemy!("SLUG_0", 200)], energy = 1)), 0); // 10 damage, where a Strike is 6
    assert_eq!(pick(&combat_state!([darts, STRIKE(1), STRIKE(2)], [enemy!("SLUG_0", 200)], energy = 1)), 1); // no Skill beside it: nothing
}
