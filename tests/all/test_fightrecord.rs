use serde_json::{json, Value};
use sts2bot::fight::bossfight::BossWatch;
use sts2bot::fightrecord::FightWatch;

use crate::fake_game::with;

/// `fight_state(foes, round_no, hp=70, kind="monster", act=2, floor=20, **player)`: the keywords on the player as a dict.
fn fight_state(foes: &[&str], round_no: i64, hp: i64, kind: &str, act: i64, floor: i64, player: Value) -> Value {
    let enemies: Vec<Value> = foes.iter().enumerate().map(|(i, name)| json!({"entity_id": format!("{name}_{i}"), "hp": 40, "block": 0})).collect();
    json!({"state_type": kind, "run": {"act": act, "floor": floor, "ascension": 7}, "battle": {"round": round_no, "turn": "player", "enemies": enemies},
           "player": with(json!({"hp": hp, "max_hp": 80, "character": "The Silent"}), player)})
}

// FightWatchTest

#[test]
fn test_a_fight_won_is_written_with_the_deck_that_fought_it() {
    let d = tempfile::tempdir().unwrap();
    let mut watch = FightWatch::new(Some(d.path().join("fights.jsonl")));
    let first = fight_state(
        &["MYTE", "MYTE"],
        1,
        70,
        "monster",
        2,
        20,
        json!({"relics": [{"id": "RING_OF_THE_SNAKE"}], "potions": [{"name": "Fire Potion"}], "hand": [{"name": "Strike"}, {"name": "Backflip+"}], "draw_pile": [{"name": "Defend"}], "discard_pile": []}),
    );
    assert_eq!(watch.update(&first, Some("monster")), None);
    assert_eq!(watch.update(&json!({"state_type": "hand_select", "run": first["run"]}), Some("hand_select")), None); // (a discard, mid-fight)
    assert_eq!(watch.update(&fight_state(&["MYTE"], 4, 52, "monster", 2, 20, json!({"potions": []})), Some("monster")), None);
    let after = json!({"state_type": "rewards", "run": first["run"], "player": {"hp": 58, "max_hp": 80}}); // (6 healed as it ended)
    let record = watch.update(&after, Some("rewards")).unwrap();
    assert_eq!(watch.update(&after, Some("rewards")), None); // once
    let text = std::fs::read_to_string(d.path().join("fights.jsonl")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(serde_json::from_str::<Value>(lines[0]).unwrap(), record);
    assert_eq!(json!([record["act"], record["floor"], record["kind"], record["enemies"], record["count"]]), json!([2, 20, "monster", ["MYTE"], 2]));
    assert_eq!(json!([record["hp_in"], record["hp_out"], record["max_hp"], record["rounds"], record["died"]]), json!([70, 58, 80, 4, false]));
    assert_eq!(json!([record["deck"], record["relics"], record["potions"], record["potions_left"]]), json!([["Backflip+", "Defend", "Strike"], ["RING_OF_THE_SNAKE"], ["Fire Potion"], 0]));
    assert_eq!(record["whole"], true);
    assert_eq!(json!([record["character"], record["ascension"]]), json!(["The Silent", 7]));
}

#[test]
fn test_a_fight_lost_and_a_fight_joined_half_way() {
    let mut watch = FightWatch::new(None); // (nowhere to write: the record still comes back)
    watch.update(&fight_state(&["QUEEN"], 3, 30, "boss", 3, 48, json!({"discard_pile": [{"name": "Strike"}]})), Some("boss"));
    let record = watch.update(&json!({"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 0}}), Some("game_over")).unwrap();
    assert_eq!(json!([record["died"], record["hp_out"], record["kind"], record["whole"]]), json!([true, 0, "boss", false]));
}

#[test]
fn test_one_fight_straight_into_the_next_is_two_records() {
    let mut watch = FightWatch::new(None);
    watch.update(&fight_state(&["NIBBIT"], 1, 70, "monster", 1, 5, json!({})), Some("monster"));
    let first = watch.update(&fight_state(&["BYRDONIS"], 1, 61, "elite", 1, 6, json!({})), Some("elite")).unwrap();
    assert_eq!(json!([first["floor"], first["hp_out"], first["died"]]), json!([5, 61, false]));
    let second = watch.update(&json!({"state_type": "map", "run": {"act": 1, "floor": 6}, "player": {"hp": 40}}), Some("map")).unwrap();
    assert_eq!(json!([second["floor"], second["kind"], second["hp_in"], second["hp_out"]]), json!([6, "elite", 61, 40]));
}

#[test]
fn test_a_card_chosen_mid_fight_does_not_end_it() {
    // The first game's Attack Potion opens a card reward inside the fight (each such boss fight was "won" after round 1).
    let (mut watch, mut bosses) = (FightWatch::new(None), BossWatch::new());
    let start = fight_state(&["THEGUARDIAN"], 1, 70, "boss", 1, 16, json!({}));
    let pick = json!({"state_type": "card_reward", "run": start["run"], "card_reward": {"cards": [], "in_combat": true}});
    let later = fight_state(&["THEGUARDIAN"], 2, 60, "boss", 1, 16, json!({}));
    watch.update(&start, Some("boss"));
    assert_eq!(watch.update(&pick, Some("card_reward")), None);
    assert_eq!(watch.update(&later, Some("boss")), None);
    bosses.update(&start, Some("boss"));
    assert_eq!(bosses.update(&pick, Some("card_reward")), None);
    assert_eq!(bosses.update(&later, Some("boss")), None);
    let record = watch.update(&json!({"state_type": "rewards", "run": start["run"], "player": {"hp": 60}}), Some("rewards")).unwrap();
    assert_eq!(json!([record["rounds"], record["hp_out"]]), json!([2, 60]));
}
