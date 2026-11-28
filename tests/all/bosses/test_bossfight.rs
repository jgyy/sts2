use serde_json::{json, Value};
use sts2bot::fight::bossfight::BossWatch;

/// (the original's defaults: player_hp 70, max_hp 80, act 1, floor 17.)
fn boss_state(foes: &[(&str, i64)], round_no: i64, player_hp: i64, floor: i64) -> Value {
    let enemies: Vec<Value> = foes.iter().enumerate().map(|(i, (name, hp))| json!({"entity_id": format!("{name}_{i}"), "hp": hp, "block": 0})).collect();
    json!({"state_type": "boss", "run": {"act": 1, "floor": floor}, "battle": {"round": round_no, "turn": "player", "enemies": enemies}, "player": {"hp": player_hp, "max_hp": 80}})
}

/// What the watch says when the game moves on to a screen of this kind. (the original's default: floor 17.)
fn shown(watch: &mut BossWatch, kind: &str, floor: i64) -> Option<String> {
    watch.update(&json!({"state_type": kind, "run": {"act": 1, "floor": floor}}), Some(kind))
}

// BossWatchTest

#[test]
fn test_won_fight_reports_boss_and_player_hp() {
    let mut watch = BossWatch::new();
    assert_eq!(watch.update(&boss_state(&[("VANTOM", 170)], 1, 70, 17), Some("boss")), None);
    assert_eq!(watch.update(&boss_state(&[("VANTOM", 60)], 2, 50, 17), Some("boss")), None);
    assert_eq!(
        shown(&mut watch, "rewards", 17).as_deref(),
        Some("boss fight over: act 1 floor 17 VANTOM won; boss hp 170 -> 60; player hp 70 -> 50/80; rounds 2; brought relics 0, potions 0, deck 0")
    );
}

#[test]
fn test_what_the_bot_brings_is_written_down_as_the_fight_begins() {
    let (mut watch, mut state) = (BossWatch::new(), boss_state(&[("VANTOM", 170)], 1, 70, 17));
    state["player"]["relics"] = json!([{"id": "BURNING_BLOOD", "name": "Burning Blood"}]);
    state["player"]["potions"] = json!([{"name": "Fire Potion"}]);
    state["player"]["hand"] = json!([{"name": "Strike"}]);
    state["player"]["draw_pile"] = json!([{"name": "Bash+"}]);
    state["player"]["discard_pile"] = json!([]);
    watch.update(&state, Some("boss"));
    assert_eq!(watch.brought.as_deref(), Some("boss fight begins: act 1 floor 17 VANTOM; relics BURNING_BLOOD; potions Fire Potion; cards Bash+, Strike"));
}

#[test]
fn test_lost_fight_is_reported_once() {
    let mut watch = BossWatch::new();
    watch.update(&boss_state(&[("SOUL_FYSH", 200)], 1, 60, 17), Some("boss"));
    watch.update(&boss_state(&[("SOUL_FYSH", 150)], 3, 5, 17), Some("boss"));
    assert!(shown(&mut watch, "game_over", 17).unwrap().contains("SOUL_FYSH lost; boss hp 200 -> 150; player hp 60 -> 5/80; rounds 2"));
    assert_eq!(shown(&mut watch, "game_over", 17), None);
}

#[test]
fn test_a_card_picked_mid_fight_does_not_end_it() {
    // A Survivor's discard opens a hand-select screen in the middle of the fight.
    let mut watch = BossWatch::new();
    watch.update(&boss_state(&[("WATERFALL_GIANT", 240)], 5, 70, 17), Some("boss"));
    for mid in ["hand_select", "card_select", "bundle_select"] {
        assert_eq!(shown(&mut watch, mid, 17), None);
    }
    watch.update(&boss_state(&[("WATERFALL_GIANT", 87)], 5, 70, 17), Some("boss"));
    assert!(shown(&mut watch, "rewards", 17).unwrap().contains("won; boss hp 240 -> 87"));
    assert_eq!(shown(&mut watch, "map", 18), None);
}

#[test]
fn test_several_enemies_are_named_once_each() {
    let mut watch = BossWatch::new();
    watch.update(&boss_state(&[("KIN_FOLLOWER", 90), ("KIN_PRIEST", 80)], 1, 70, 17), Some("boss"));
    assert!(shown(&mut watch, "map", 18).unwrap().contains("act 1 floor 17 KIN_FOLLOWER+KIN_PRIEST won"));
}

#[test]
fn test_what_is_not_a_boss_fight_is_ignored() {
    let mut ordinary = boss_state(&[("SLIME", 20)], 1, 70, 3);
    ordinary["state_type"] = json!("monster");
    for (why, state) in [("an ordinary fight", ordinary), ("a boss state with no living enemies", boss_state(&[("VANTOM", 0)], 1, 70, 17))] {
        let mut watch = BossWatch::new();
        assert_eq!(watch.update(&state, state["state_type"].as_str()), None, "{why}");
        assert_eq!(watch.update(&json!({"state_type": "map", "run": {}}), Some("map")), None, "{why}");
    }
}
