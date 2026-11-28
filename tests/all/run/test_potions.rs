//! Potions kept for the boss: on the belt, at the Merchant and in events.

use serde_json::{json, Value};
use sts2bot::fight::combat;
use sts2bot::learn;
use sts2bot::run::events::{self, EventOptionValueArgs};
use sts2bot::run::strategy::{self, Memory};

use crate::helpers::{combat_state, enemy, STRIKE};

/// (the original's default: target "AnyPlayer".)
fn potion(slot: i64, name: &str, text: &str) -> Value {
    json!({"slot": slot, "name": name, "description": text, "target_type": "AnyPlayer", "can_use_in_combat": true})
}

/// What a shop of these items sells, in order, to a belt of two slots holding `held` Fire Potions.
fn bought(items: &[Value], floor: i64, gold: i64, held: i64) -> Vec<Value> {
    let potions: Vec<Value> = (0..held).map(|i| potion(i, "Fire Potion", "Deal 20 damage.")).collect();
    let player = json!({"gold": gold, "hp": 60, "max_hp": 75, "max_potion_slots": 2, "potions": potions, "relics": []});
    let state = json!({"state_type": "shop", "run": {"act": if floor > 33 { 3 } else { 2 }, "floor": floor}, "player": player, "shop": {"items": items}});
    strategy::shop(&state, &mut Memory::new(), "shop").iter().filter(|a| a.0 == "shop_purchase").map(|a| a.1["index"].clone()).collect()
}

// KeptForTheBoss

/// (the original's default: hp 75.)
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

#[test]
fn test_an_act_3_merchant_fills_the_belt_first() {
    let shelf = || {
        [json!({"index": 0, "category": "relic", "relic_id": "ANCHOR", "relic_name": "Anchor", "price": 150, "is_stocked": true}),
         json!({"index": 1, "category": "potion", "potion_name": "Fire Potion", "potion_description": "Deal 20 damage.", "price": 50, "is_stocked": true})]
    };
    assert_eq!(bought(&shelf(), 38, 160, 1)[0], 1); // the potion before the relic, ten floors from the last boss
    assert_eq!(bought(&shelf(), 22, 160, 1)[0], 0); // in act 2, far from the boss, the relic
}

#[test]
fn test_a_merchant_before_the_boss_sells_the_best_potion_first() {
    // (All three stood at 9.5, and the first listed, Liquid Bronze, was bought.)
    let shelf = [(0, "Liquid Bronze", 76), (1, "Heart of Iron", 78), (2, "Radiant Tincture", 76)];
    let items: Vec<Value> = shelf.iter().map(|(i, n, p)| json!({"index": i, "category": "potion", "potion_name": n, "potion_description": "", "price": p, "is_stocked": true, "can_afford": true})).collect();
    for held in [0, 1] {
        assert_eq!(bought(&items, 31, 100, held), [json!(2), json!(1), json!(0)]);
    }
}

#[test]
fn test_an_event_that_takes_a_potion_is_charged_for_it() {
    let belt = [potion(0, "Attack Potion", "Add 1 of 3 random Attack cards into your Hand."), potion(1, "Strength Potion", "Gain 2 Strength.")];
    // (the original's defaults: text "Obtain a random Relic.", act 3, held the belt.)
    let value_ = |title: &str, text: &str, act: i64, held: &[Value]| {
        events::_event_option_value(&json!({"title": title, "description": text}), 60, 80, EventOptionValueArgs { act: Some(act), potions: Some(held), ..Default::default() })
    };
    let value = |title: &str| value_(title, "Obtain a random Relic.", 3, &belt);
    assert!(value("Give 100 Gold") > value("Give Attack Potion"));
    assert!(value("Give Attack Potion") > value("Give Strength Potion")); // the better potion costs more
    assert_eq!(value("Give Fire Potion"), value("Take It")); // a potion we do not hold: nothing to charge
    let lose = "Lose Strength Potion. Obtain an Upgraded Common Skill.";
    assert!(value_("Insert Common Potion", lose, 3, &belt) < value_("Insert Common Potion", lose, 1, &belt)); // dearer in act 3
    // A potion on offer is worth more to a belt with room for it.
    assert!(value_("Bottle", "Procure 1 random Potion.", 3, &[]) > value_("Bottle", "Procure 1 random Potion.", 3, &belt));
}
