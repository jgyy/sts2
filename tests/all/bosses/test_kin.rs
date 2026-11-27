//! The Kin (act 1 boss): the two Followers are brought down before the Priest (battle.MAIN_MINIONS).

use serde_json::{json, Value};
use sts2bot::fight::battle;
use sts2bot::py::JsonExt;

use crate::fake_game::with;
use crate::fight::{aimed, combat_state, power};
use crate::helpers::{enemy, STRIKE};

fn MINION() -> Value {
    json!([power("MINION", 1)])
}

/// Round 1 of the fight as logged on Ascension 8. (The Python's defaults: f0 63, f1 62, priest 199.)
fn kin(f0: i64, f1: i64, priest: i64) -> Value {
    let out = [
        with(enemy!("KIN_FOLLOWER_0", f0), json!({"status": MINION()})),
        with(enemy!("KIN_FOLLOWER_1", f1, attack = 5), json!({"status": MINION()})),
        with(enemy!("KIN_PRIEST_0", priest, attack = 8), json!({"intents": [{"type": "Attack", "label": "8"}, {"type": "Debuff", "label": ""}]})),
    ];
    combat_state!([STRIKE(0)], out.into_iter().filter(|e| e["hp"].int() > 0).collect::<Vec<_>>(), hp = 70, max_hp = 70, kind = "boss")
}

// KinFollowersFirst

#[test]
fn test_the_followers_are_hit_before_the_priest_unless_he_can_be_finished() {
    // A Silent put every attack on the Priest (199) and died on round 7 with him on 89, the Followers on 57 and 38 at 4 Strength each.
    for (why, state, target) in [
        ("the Follower with fewer HP first", kin(63, 62, 199), "KIN_FOLLOWER_1"),
        ("then the other", kin(63, 0, 199), "KIN_FOLLOWER_0"),
        ("a Priest within reach is the one to finish: his death takes the Followers along", kin(63, 62, 20), "KIN_PRIEST_0"),
        ("a Priest alone", kin(0, 0, 150), "KIN_PRIEST_0"),
    ] {
        assert_eq!(aimed(&state).as_deref(), Some(target), "{why}");
    }
}

#[test]
fn test_without_the_table_the_priest_comes_first_as_before() {
    let _none = battle::with_main_minions(&[]);
    assert_eq!(aimed(&kin(63, 62, 199)).as_deref(), Some("KIN_PRIEST_0"));
}

#[test]
fn test_other_minions_keep_the_per_turn_rule() {
    // The Queen's Amalgam is the main threat only on turns it hits hard (15+, twice the Queen's, 60% of her HP).
    let queen = with(enemy!("QUEEN_0", 400), json!({"intents": [{"type": "Buff", "label": ""}]}));
    let amalgam = |attack: i64| combat_state!([STRIKE(0)], [with(enemy!("TORCH_HEAD_AMALGAM_0", 199, attack = attack), json!({"status": MINION()})), queen.clone()], kind = "boss");
    assert_eq!(battle::read_enemies(&amalgam(8)).iter().map(|e| e.main_threat).collect::<Vec<_>>(), [false, false]);
    assert_eq!(battle::read_enemies(&amalgam(30)).iter().map(|e| e.main_threat).collect::<Vec<_>>(), [true, false]);
}

#[test]
fn test_weak_on_the_amalgam_does_not_make_it_harmless() {
    // Deathbringer's Weak made the Amalgam's 18 show as 13, and Reap+ went into the Queen's 381 HP: what counts is the hit it will make once the Weak is gone.
    let queen = with(enemy!("QUEEN_0", 381), json!({"intents": [{"type": "Debuff", "label": ""}]}));
    let state = combat_state!([STRIKE(0)], [with(enemy!("TORCH_HEAD_AMALGAM_0", 180, attack = 13), json!({"status": [power("MINION", 1), power("WEAK", 1)]})), queen], kind = "boss");
    assert_eq!(battle::read_enemies(&state).iter().map(|e| e.main_threat).collect::<Vec<_>>(), [true, false]);
    assert_eq!(aimed(&state).as_deref(), Some("TORCH_HEAD_AMALGAM_0"));
}
