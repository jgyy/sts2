//! What the test files share: a clock that does not wait, and small builders for combat states.
// TODO(bot): `FakeClock` and `make_bot(game, **kw)` need src/bot.rs: they are ported with it.

use serde_json::{json, Value};
use sts2bot::py::JsonExt;

pub use crate::fake_game::{card, with};

/// `combat_state(hand, enemies, *, energy=3, hp=80, max_hp=80, block=0, kind="monster", potions=())`: the keywords as a dict
/// (`combat_state!(hand, enemies, energy = 1, kind = "boss")` writes it).
pub fn combat_state(hand: Vec<Value>, enemies: Vec<Value>, kw: Value) -> Value {
    for k in kw.as_object().expect("keywords").keys() {
        assert!(["energy", "hp", "max_hp", "block", "kind", "potions"].contains(&k.as_str()), "combat_state() got an unexpected keyword argument '{k}'");
    }
    let or = |k: &str, default: Value| kw.get(k).cloned().unwrap_or(default);
    let player = json!({"hp": or("hp", json!(80)), "max_hp": or("max_hp", json!(80)), "block": or("block", json!(0)), "energy": or("energy", json!(3)), "hand": hand,
                        "potions": or("potions", json!([])), "draw_pile_count": 5, "discard_pile_count": 0, "exhaust_pile_count": 0});
    json!({"state_type": or("kind", json!("monster")), "run": {"act": 1, "floor": 3, "ascension": 0},
           "battle": {"round": 1, "turn": "player", "is_play_phase": true, "enemies": enemies}, "player": player})
}

/// `helpers.combat_state(hand, enemies, energy=1, ...)`.
macro_rules! helpers_combat_state {
    ($hand:expr, $enemies:expr $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::helpers::combat_state(Vec::from($hand), Vec::from($enemies), serde_json::json!({$((stringify!($k)): $v),*}))
    };
}
pub(crate) use helpers_combat_state;

/// `enemy(eid, hp, attack=None, block=0, status=())`: the keywords as a dict (`enemy!("A", 30, attack = 12)` writes it).
pub fn enemy(eid: &str, hp: i64, kw: Value) -> Value {
    for k in kw.as_object().expect("keywords").keys() {
        assert!(["attack", "block", "status"].contains(&k.as_str()), "enemy() got an unexpected keyword argument '{k}'");
    }
    let attack = &kw["attack"];
    let intents = if attack.truthy() { json!([{"type": "Attack", "label": sts2bot::py::str_of(attack)}]) } else { json!([{"type": "Buff", "label": ""}]) };
    json!({"entity_id": eid, "hp": hp, "max_hp": hp, "block": kw.get("block").cloned().unwrap_or(json!(0)), "status": kw.get("status").cloned().unwrap_or(json!([])), "intents": intents})
}

/// `enemy("A", 30, attack=12, status=[...])`.
macro_rules! enemy {
    ($eid:expr, $hp:expr $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::helpers::enemy($eid, $hp, serde_json::json!({$((stringify!($k)): $v),*}))
    };
}
pub(crate) use enemy;

pub fn STRIKE(i: i64) -> Value {
    card(i, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy")
}

pub fn DEFEND(i: i64) -> Value {
    card(i, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self")
}
