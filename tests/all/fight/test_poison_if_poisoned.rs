//! Cards whose Poison depends on what is on the enemy already.

use serde_json::Value;
use sts2bot::cards::parse;

use super::{combat_state, ctx, gain, pick, power, read_foes};
use crate::fake_game::card;
use crate::helpers::{enemy, DEFEND};

fn BUBBLE() -> Value {
    card(0, "BUBBLE_BUBBLE", "Bubble Bubble", "Skill", 1, "If the enemy has Poison, apply 9 Poison.", "AnyEnemy")
}

// BubbleBubble

#[test]
fn test_bubble_bubble_poisons_only_what_is_poisoned_already() {
    // Read as plain Poison, a Silent with no other source played it six times in one lost fight (1 energy each, nothing on her).
    let info = parse(&BUBBLE());
    assert_eq!((info.poison, info.poison_needs), (9, true));
    assert!(!parse(&card(0, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy")).poison_needs);
    let on = |stack: i64| {
        let status = if stack != 0 { vec![power("POISON", stack)] } else { vec![] };
        gain(&[info.clone()], &read_foes(&[enemy!("A", 300, status = status)]), &ctx!(energy = 1, fight = "boss"))
    };
    assert!(on(0) <= 0.0); // nothing to build on: a wasted energy
    assert!(on(5) > 5.0);
}

#[test]
fn test_bubble_bubble_waits_for_the_poison_that_comes_first() {
    let deadly = card(1, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy");
    assert_eq!(pick(&combat_state!([BUBBLE(), deadly], [enemy!("A", 200, attack = 8)], energy = 2, hp = 60, max_hp = 80, kind = "boss")), 1); // the source first, then Bubble Bubble
    // With no source in the hand, the energy goes to a card that does something.
    assert_eq!(pick(&combat_state!([BUBBLE(), DEFEND(1)], [enemy!("A", 200, attack = 12)], energy = 1, hp = 60, max_hp = 80, kind = "boss")), 1);
}
