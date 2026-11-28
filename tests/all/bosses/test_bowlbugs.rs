//! Bowlbug Rock and Bowlbug Nectar (act 2 hallway): the Nectar is brought down first (battle.KNOWN_THREAT).

use serde_json::{json, Value};
use sts2bot::fight::battle;
use sts2bot::base::JsonExt;

use crate::fake_game::with;
use crate::fight::{aimed, combat_state, power};
use crate::helpers::{enemy_kw, STRIKE};

/// round 2: the Nectar shows a Buff and has shown 1.5 a round
/// (`bowlbugs(**BUFFING)`: `bowlbugs(47, 37, None, Some(16), BUFFING())`.)
fn BUFFING() -> Option<Value> {
    Some(json!({"BOWLBUG_ROCK_0": 16.0, "BOWLBUG_NECTAR_0": 1.5}))
}

/// A round of the fight as logged on Ascension 9 (the Nectar: 3, Buff, then 19 a turn), with what each has aimed at us a round so far, this round included (bot.rs's _threat).
/// (the original's defaults: rock 47, nectar 37, nectar_attack 3, rock_attack 16, no threat.)
fn bowlbugs(rock: i64, nectar: i64, nectar_attack: Option<i64>, rock_attack: Option<i64>, threat: Option<Value>) -> Value {
    let out = [with(enemy_kw("BOWLBUG_ROCK_0", rock, json!({"attack": rock_attack})), json!({"status": [power("IMBALANCED", 1)]})), enemy_kw("BOWLBUG_NECTAR_0", nectar, json!({"attack": nectar_attack}))];
    let seen = threat.unwrap_or_else(|| json!({"BOWLBUG_ROCK_0": rock_attack.unwrap_or(0) as f64, "BOWLBUG_NECTAR_0": nectar_attack.unwrap_or(0) as f64}));
    combat_state!([STRIKE(0)], out.into_iter().filter(|e| e["hp"].int() > 0).collect::<Vec<_>>(), hp = 60, max_hp = 75, _threat = seen)
}

// NectarFirst

#[test]
fn test_whom_the_strike_goes_at() {
    for (why, state, target) in [
        ("the Nectar before the Rock from round one (the Rock went 46 -> 11 -> 1 while the Nectar hit 19 a round)", bowlbugs(47, 37, Some(3), Some(16), None), "BOWLBUG_NECTAR_0"),
        ("also on the turn it buffs", bowlbugs(47, 37, None, Some(16), BUFFING()), "BOWLBUG_NECTAR_0"),
        ("a Rock within reach is still finished", bowlbugs(5, 37, Some(3), Some(16), None), "BOWLBUG_ROCK_0"),
    ] {
        assert_eq!(aimed(&state).as_deref(), Some(target), "{why}");
    }
}

#[test]
fn test_without_the_table_the_rock_came_first() {
    let _without = battle::with_known_threat(battle::known_threat().into_iter().filter(|(k, _)| k != "BOWLBUG_NECTAR").collect());
    assert_eq!(aimed(&bowlbugs(47, 37, None, Some(16), BUFFING())).as_deref(), Some("BOWLBUG_ROCK_0"));
}

#[test]
fn test_the_nectar_s_threat_is_what_it_will_do() {
    assert_eq!(battle::read_enemies(&bowlbugs(47, 37, Some(3), Some(16), None)).iter().find(|e| &*e.entity_id == "BOWLBUG_NECTAR_0").unwrap().threat, 19.0);
}
