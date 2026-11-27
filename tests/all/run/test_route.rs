//! The act 2 boss curve and what an Unknown room costs on the way (strategy.ACT2_ODDS, strategy.UNKNOWN_COST).

use indexmap::IndexMap;
use serde_json::{json, Value};
use sts2bot::run::strategy;

// Act2Boss

#[test]
fn test_hp_above_85_percent_counts_at_the_act_2_boss() {
    // Live (A5+, 85-97% HP won 0.88 as often as 97%+) and the Knowledge Demon solved exactly (90% HP: 0.82).
    assert!(strategy::boss_odds(0.9, 2) < 0.9);
    assert!(strategy::boss_odds(0.7, 2) < 0.5);
    assert_eq!(strategy::boss_odds(1.0, 2), 1.0);
    assert_eq!(strategy::boss_odds(0.0, 2), 0.0);
    assert!((0..20).all(|x| strategy::boss_odds(x as f64 / 20.0, 2) <= strategy::boss_odds((x + 1) as f64 / 20.0, 2)));
}

#[test]
fn test_act_1_keeps_its_flat_top() {
    assert_eq!(strategy::boss_odds(0.9, 1), 1.0);
    assert_eq!(strategy::boss_odds(0.85, 1), 1.0);
}

#[test]
fn test_an_unknown_room_in_act_2_costs_hp_on_paper() {
    // 375 act 2 Unknown rooms: an event 65% of the time, a fight 20% at 17% of Max HP; 3% on average.
    let way = [("Unknown", Some(13)), ("Boss", Some(14))];
    let (free, free_act1) = {
        let _none = strategy::with_unknown_cost(IndexMap::new());
        (strategy::path_value(&way, 70.0, 80, 0, 2, 1.0), strategy::path_value(&way, 70.0, 80, 0, 1, 1.0))
    };
    assert!(strategy::path_value(&way, 70.0, 80, 0, 2, 1.0) < free - 0.5);
    assert_eq!(strategy::path_value(&way, 70.0, 80, 0, 1, 1.0), free_act1); // (act 1: under 1%, left out)
}

// SpoilsMap: the Spoils Map's 600 Gold lies at a node of the next act's map (the mod's `quests`): the route walks through it.

fn state(quests: Value) -> Value {
    let node = |col: i64, row: i64, kind: &str, kids: &[(i64, i64)]| json!({"col": col, "row": row, "type": kind, "children": kids.iter().map(|(c, r)| json!([c, r])).collect::<Vec<_>>()});
    let nodes = [node(0, 1, "Monster", &[(0, 2)]), node(1, 1, "Monster", &[(1, 2)]), node(0, 2, "Monster", &[(0, 3)]), node(1, 2, "Monster", &[(0, 3)]),
                 node(0, 3, "Shop", &[(0, 4)]), node(0, 4, "Boss", &[])];
    let opts = json!([{"index": 0, "col": 0, "row": 1, "type": "Monster"}, {"index": 1, "col": 1, "row": 1, "type": "Monster"}]);
    json!({"state_type": "map", "run": {"act": 2, "floor": 18}, "player": {"hp": 70, "max_hp": 80, "gold": 50},
           "map": {"nodes": nodes, "next_options": opts, "quests": quests}})
}

#[test]
fn test_the_route_goes_through_the_spoils() {
    let mut mem = strategy::Memory::new();
    let mut first = |quests: Value| strategy::map_screen(&state(quests), &mut mem)[0].1["index"].as_i64().unwrap();
    assert_eq!(first(json!([{"kind": "spoils", "act": 2, "col": 1, "row": 2}])), 1);
    assert_eq!(first(json!([{"kind": "spoils", "act": 2, "col": 0, "row": 2}])), 0);
    assert_eq!(first(json!([{"kind": "spoils", "act": 3, "col": 1, "row": 2}])), 0); // (another act's: nothing here; ties go first)
}
