//! What a turn of Weak or Vulnerable that outlasts this one counts for (the `debuff_worth` setting).

use sts2bot::learn;

use super::{combat_state, pick};
use crate::helpers::{card, enemy, STRIKE};

// DebuffWorth

#[test]
fn test_a_debuff_goes_first_only_when_its_later_turns_outweigh_a_strike() {
    // One energy against a boss that is buffing: (why, the debuff, debuff_worth, the card played: 0 the debuff, 1 the Strike).
    let weak = card(0, "INTIMIDATE", "Intimidate", "Skill", 1, "Apply 2 Weak.", "AnyEnemy");
    let once = card(0, "TRIP", "Trip", "Skill", 1, "Apply 1 Vulnerable.", "AnyEnemy");
    for (why, debuff, worth, first) in [
        ("two turns of Weak to come are 6.5 points, a Strike 9", &weak, 1.0, 1),
        ("... at 13 they go first", &weak, 2.0, 0),
        ("one Vulnerable is gone when the boss has moved: it helps nothing", &once, 1.0, 1),
        ("... at either value", &once, 2.0, 1),
    ] {
        let _knob = learn::with_knobs(&[("debuff_worth", worth)]);
        assert_eq!(pick(&combat_state!([debuff.clone(), STRIKE(1)], [enemy!("BOSS", 300)], energy = 1, kind = "boss")), first, "{why}");
    }
}

// PricesOfAPlay
// What a card gives away for good (until 2026-11-26 the planner saw neither): Strength to the enemy, dead cards in our piles.

#[test]
fn test_strength_given_to_the_enemy_costs_every_hit_it_has_left() {
    let hand = [card(0, "FIGHT_ME", "Fight Me!", "Attack", 1, "Deal 9 damage. The enemy gains 1 Strength.", "AnyEnemy"), STRIKE(1)];
    let boss = enemy!("BOSS", 300, attack = "5x3");
    assert_eq!(pick(&combat_state!(hand, [boss], energy = 1, kind = "boss")), 1); // (a point of damage is not three hits a turn)
}

#[test]
fn test_a_status_put_in_our_piles_costs_a_draw_every_shuffle() {
    // A Wound in a deck of 25 over a boss fight: about 8 points (battle.junk_cost), a Strike's 6 damage 9.
    let wild = |dmg: i64| card(0, "WILD_STRIKE", "Wild Strike", "Attack", 1, &format!("Deal {dmg} damage. Shuffle a Wound into your draw pile."), "AnyEnemy");
    let played = |dmg: i64| pick(&combat_state!([wild(dmg), STRIKE(1)], [enemy!("BOSS", 300)], energy = 1, kind = "boss", draw_pile_count = 23));
    assert_eq!((played(7), played(14)), (1, 0)); // (a point more is not worth a Wound; eight more are)
}
