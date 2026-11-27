//! A tiny stand-in for the game that speaks the bridge mod's state/action shapes. It is not Slay the Spire: just enough
//! screens (menu, map, combat, rewards, card reward, rest site, card select, game over) to drive the bot loop for whole runs.
//!
//! So far only the card builder the planner's tests share is here.
// TODO(bot): `FakeGame` (and `_options`) drive the bot loop: they are ported with src/bot.rs and tests/all/test_bot.rs.

use serde_json::{json, Value};

/// A card as the game sends it. (The Python's defaults: target "None", rarity "Common"; its `**extra` and another rarity: `with`.)
pub fn card(index: i64, cid: &str, name: &str, ctype: &str, cost: impl ToString, desc: &str, target: &str) -> Value {
    json!({"index": index, "id": cid, "name": name, "type": ctype, "cost": cost.to_string(), "star_cost": null, "description": desc,
           "target_type": target, "can_play": true, "is_upgraded": false, "rarity": "Common"})
}

/// `{**base, **extra}`: `base` with the keys of `extra` set (new ones at the end).
pub fn with(mut base: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().expect("a dict of keys to set") {
        base[k.as_str()] = v.clone();
    }
    base
}

/// (id, name, type, cost, description, target)
pub const STRIKE: (&str, &str, &str, i64, &str, &str) = ("STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy");
pub const DEFEND: (&str, &str, &str, i64, &str, &str) = ("DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self");
pub const BASH: (&str, &str, &str, i64, &str, &str) = ("BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy");
