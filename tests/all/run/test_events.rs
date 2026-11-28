//! Events priced by the HP they cost: a fight an event starts, and relics that come only after five more fights.

use indexmap::IndexMap;
use serde_json::{json, Value};
use sts2bot::run::events::{self, EventOptionValueArgs};
use sts2bot::run::strategy;

/// `events._event_option_value(opt, hp, max_hp, act=act)`.
fn value(opt: &Value, hp: i64, max_hp: i64, act: i64) -> f64 {
    events::_event_option_value(opt, hp, max_hp, EventOptionValueArgs { act: Some(act), ..Default::default() })
}

// EventFights

fn punch() -> Value {
    json!({"title": "I Can Take Them", "description": "Fight them for Greater Rewards."})
}

fn nab() -> Value {
    json!({"title": "Nab", "description": "Add Injury to your Deck. Obtain a random Relic."})
}

#[test]
fn test_a_fight_an_event_starts_costs_its_hp_so_punch_off_loses_to_nab() {
    // Taken at 55/73 (the fight took 44 HP, the walk on left 2) and at 37/66 (a death); rated 4.5 at any HP, Nab 3.5.
    assert!(value(&punch(), 55, 73, 1) < value(&nab(), 55, 73, 1));
    assert!(value(&punch(), 37, 66, 1) < value(&nab(), 37, 66, 1));
}

#[test]
fn test_a_fight_that_could_kill_is_never_taken() {
    assert!(value(&punch(), 30, 66, 1) < -900.0);
}

#[test]
fn test_the_same_fight_costs_more_in_act_3() {
    // Priced as an HP loss is, by ACT3_HP: the last acts' HP is worth more.
    assert!(value(&punch(), 73, 73, 3) < value(&punch(), 73, 73, 1));
}

// DelayedRelics

fn mystery_box() -> Value {
    json!({"title": "Wongo's Mystery Box", "description": "Pay 300 Gold. Obtain 3 random Relics after 5 combats."})
}

fn bargain() -> Value {
    json!({"title": "Wongo's Bargain Bin", "description": "Pay 100 Gold. Obtain 1 random Common Relic."})
}

#[test]
fn test_three_relics_after_five_combats_lose_to_one_now_at_low_hp() {
    // Taken at 8/75 (16.5 against the Bin's 5.8): the run died three fights later holding no box relics.
    assert!(value(&mystery_box(), 8, 75, 2) < value(&bargain(), 8, 75, 2));
    assert!(value(&mystery_box(), 75, 75, 2) > value(&bargain(), 75, 75, 2)); // at full health the box still wins
    assert_eq!(value(&bargain(), 8, 75, 2), value(&bargain(), 75, 75, 2)); // (a relic now is not discounted)
}

#[test]
fn test_surviving_five_fights_is_likelier_above_half_hp() {
    assert!(events::_lives_through(5, 40, 75) > events::_lives_through(5, 30, 75));
    assert!(events::_lives_through(5, 75, 75) > 0.9);
}

// KnowingSkull: every choice but the way out comes back dearer, and the way out costs 6 HP whenever it is taken.

/// (HP lost, things bought) when the bot plays the event through: each thing costs 1 HP more every time it is taken.
/// (the original's defaults: max_hp 85, act 2.)
fn walk(mut hp: i64, max_hp: i64, act: i64) -> (i64, i64) {
    let (mut mem, start, mut bought) = (strategy::Memory::new(), hp, 0);
    let mut costs: IndexMap<&str, i64> = IndexMap::from([("Riches?", 6), ("A Pick Me Up?", 6), ("Success?", 6)]);
    let gains: IndexMap<&str, &str> = IndexMap::from([("Riches?", "Gain 90 Gold."), ("A Pick Me Up?", "Get a Potion."), ("Success?", "Get a Colorless Card.")]);
    for _ in 0..20 {
        let mut opts: Vec<Value> = costs.iter().enumerate().map(|(i, (t, c))| json!({"index": i, "title": t, "description": format!("{} Lose {} HP.", gains[t], c)})).collect();
        opts.push(json!({"index": 3, "title": "How do I leave?", "description": "Lose 6 HP."}));
        let state = json!({"state_type": "event", "run": {"act": act, "floor": 22}, "player": {"hp": hp, "max_hp": max_hp, "potions": [], "relics": []},
                           "event": {"event_id": "KNOWING_SKULL", "options": opts}});
        let pick = &opts[events::event(&state, &mut mem)[0].1["index"].as_u64().unwrap() as usize];
        let title = pick["title"].as_str().unwrap();
        if title == "How do I leave?" {
            return (start - (hp - 6), bought);
        }
        (hp, bought) = (hp - costs[title], bought + 1);
        costs[title] += 1;
    }
    panic!("never left");
}

#[test]
fn test_it_takes_a_thing_or_two_and_leaves() {
    // 2026-11-25: 47 -> 1 HP at 19:07 and 66 -> 5 at 19:19, the way out always scoring a little under the next purchase.
    for hp in [47, 66, 85] {
        let (lost, bought) = walk(hp, 85, 2);
        assert!(lost <= 20, "{hp}");
        assert!(bought <= 2, "{hp}");
    }
}

#[test]
fn test_other_events_whose_options_all_cost_hp_still_pay_for_a_relic() {
    // The Cursed Tome after its pages (6 HP bled): the Book for 10 HP beats stopping for 3, as before the Knowing Skull's rule.
    let mut mem = strategy::Memory::new();
    (mem.event_id, mem.event_hp0) = ("CURSED_TOME@30".to_string(), 66);
    let opts = json!([{"index": 0, "title": "Take", "description": "Obtain the Book. Lose 10 HP."}, {"index": 1, "title": "Stop", "description": "Lose 3 HP."}]);
    let state = json!({"state_type": "event", "run": {"act": 2, "floor": 30}, "player": {"hp": 60, "max_hp": 80, "potions": [], "relics": []},
                       "event": {"event_id": "CURSED_TOME", "options": opts}});
    assert_eq!(events::event(&state, &mut mem)[0], ("choose_event_option".to_string(), json!({"index": 0})));
}

// SecretPortal

fn portal() -> Value {
    json!({"title": "Enter the Portal", "description": "IMMEDIATELY travel to the boss."})
}

fn leave() -> Value {
    json!({"title": "Leave", "description": ""})
}

#[test]
fn test_only_from_near_full_health_with_no_key_still_wanted() {
    // 2026-11-25: entered at 39/75, 42/77 and 42/82 on a 0.5-0.5 tie; all three bosses lost.
    assert!(value(&portal(), 42, 82, 3) < value(&leave(), 42, 82, 3));
    assert!(value(&portal(), 80, 82, 3) >= value(&leave(), 80, 82, 3));
    let wanted = |opt: &Value| events::_event_option_value(opt, 80, 82, EventOptionValueArgs { act: Some(3), keys_wanted: true, ..Default::default() });
    assert!(wanted(&portal()) < wanted(&leave()));
}

// MindBloom

#[test]
fn test_upgrading_everything_is_not_worth_never_healing_again() {
    let awake = json!({"title": "I am Awake", "description": "Upgrade all Cards. You can no longer heal."});
    let rich = json!({"title": "I am Rich", "description": "Gain 999 Gold. Cursed - 2 Normality."});
    let war = json!({"title": "I am War", "description": "Fight a Boss from Act 1. Obtain a Rare Relic."});
    for hp in [40, 79] {
        assert!(value(&awake, hp, 79, 3) < value(&rich, hp, 79, 3).max(value(&war, hp, 79, 3)), "{hp}");
    }
}
