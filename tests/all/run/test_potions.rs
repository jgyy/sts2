//! Potions kept for the boss: on the belt, at the Merchant and in events.

use serde_json::{json, Value};
use sts2bot::fight::combat;
use sts2bot::learn;

use crate::helpers::{combat_state, enemy, STRIKE};

/// (The Python's default: target "AnyPlayer".)
fn potion(slot: i64, name: &str, text: &str) -> Value {
    json!({"slot": slot, "name": name, "description": text, "target_type": "AnyPlayer", "can_use_in_combat": true})
}

// TODO(strategy): `bought(items, floor, gold, held)` asks strategy.shop (src/run/strategy.rs): ported with it.

// KeptForTheBoss

/// (The Python's default: hp 75.)
fn drunk(floor: i64, hp: i64) -> usize {
    let belt = [potion(0, "Strength Potion", "Gain 2 Strength."), potion(1, "Swift Potion", "Draw 3 cards.")];
    let mut state = combat_state!([STRIKE(0)], [enemy!("A", 60, attack = 10)], hp = hp, max_hp = 75, potions = belt);
    state["run"]["floor"] = json!(floor);
    state["player"]["max_potion_slots"] = json!(2);
    combat::potion_actions(&state, None, None).len()
}

/// `mock.patch.dict(learn.KNOB, learn.defaults(game))`.
fn playing_with(game: &str) -> learn::KnobGuard {
    let table = learn::defaults(Some(game));
    learn::with_knobs(&table.iter().map(|(name, x)| (name.as_str(), *x)).collect::<Vec<_>>())
}

#[test]
fn test_a_full_belt_is_no_reason_to_drink_anywhere_in_act_3() {
    // The last boss was never beaten with fewer than two potions, and a slot emptied in act 3 stays empty half the time.
    assert_eq!([34, 36, 40, 46].map(|f| drunk(f, 75)), [0, 0, 0, 0]);
    assert_eq!(drunk(20, 75), 1); // early in act 2 the spare one still goes
    assert_eq!(drunk(5, 75), 1);
    assert!(drunk(36, 12) > 0); // ... and a fight gone wrong gets them in any act
}

#[test]
fn test_slay_the_spire_2_keeps_a_full_belt_from_hallway_fights() {
    // (Model runs 2026-11-26: 1.2 floors further at A8, the act 2 boss won 63% of the time instead of 54%; the first game wants the opposite.)
    {
        let _knobs = playing_with("sts2");
        assert_eq!(drunk(20, 75), 0);
        assert!(drunk(20, 12) > 0); // (danger still gets them)
    }
    let _knobs = playing_with("sts1");
    assert_eq!(drunk(20, 75), 1);
}

// TODO(strategy): test_an_act_3_merchant_fills_the_belt_first and test_a_merchant_before_the_boss_sells_the_best_potion_first need strategy.shop
// and strategy.Memory; test_an_event_that_takes_a_potion_is_charged_for_it needs events._event_option_value (src/run/events/mod.rs): ported with them.
