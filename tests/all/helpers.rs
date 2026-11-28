//! What the test files share: a clock that does not wait, and small builders for combat states.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use serde_json::{json, Value};
use sts2bot::bot::{Bot, BotArgs, SharedGame};
use sts2bot::base::JsonExt;

pub use crate::fake_game::{card, with};

/// A clock that does not wait: `sleep` moves it on. (A handle: its clones tell the same time.)
#[derive(Clone, Default)]
pub struct FakeClock {
    pub t: Rc<Cell<f64>>,
}

impl FakeClock {
    pub fn new() -> Self {
        FakeClock::default()
    }

    pub fn now(&self) -> f64 {
        self.t.get()
    }

    pub fn sleep(&self, s: f64) {
        self.t.set(self.t.get() + s);
    }

    /// `clock=clock.now`.
    pub fn clock_fn(&self) -> Option<Box<dyn Fn() -> f64>> {
        let clock = self.clone();
        Some(Box::new(move || clock.now()))
    }

    /// `sleep=clock.sleep`.
    pub fn sleep_fn(&self) -> Option<Box<dyn Fn(f64)>> {
        let clock = self.clone();
        Some(Box::new(move |s| clock.sleep(s)))
    }
}

/// `make_bot(game, **kw)`: the keywords in `kw` (its clock and sleep are a FakeClock's).
pub fn make_bot_kw(game: SharedGame, kw: BotArgs) -> Bot {
    let clock = FakeClock::new();
    Bot::new(game, BotArgs { clock: clock.clock_fn(), sleep: clock.sleep_fn(), ..kw })
}

/// `make_bot(game)`.
pub fn make_bot(game: SharedGame) -> Bot {
    make_bot_kw(game, BotArgs::default())
}

/// `make_bot(None)`.
pub fn make_bot_none() -> Bot {
    make_bot(Rc::new(RefCell::new(crate::fake_game::NoGame)))
}

/// `combat_state(hand, enemies, *, energy=3, hp=80, max_hp=80, block=0, kind="monster", potions=())`: the keywords as a dict
/// (`combat_state!(hand, enemies, energy = 1, kind = "boss")` writes it).
pub fn combat_state_kw(hand: Vec<Value>, enemies: Vec<Value>, kw: Value) -> Value {
    for k in kw.as_object().expect("keywords").keys() {
        assert!(["energy", "hp", "max_hp", "block", "kind", "potions"].contains(&k.as_str()), "combat_state() got an unexpected keyword argument '{k}'");
    }
    let or = |k: &str, default: Value| kw.get(k).cloned().unwrap_or(default);
    let player = json!({"hp": or("hp", json!(80)), "max_hp": or("max_hp", json!(80)), "block": or("block", json!(0)), "energy": or("energy", json!(3)), "hand": hand,
                        "potions": or("potions", json!([])), "draw_pile_count": 5, "discard_pile_count": 0, "exhaust_pile_count": 0});
    json!({"state_type": or("kind", json!("monster")), "run": {"act": 1, "floor": 3, "ascension": 0},
           "battle": {"round": 1, "turn": "player", "is_play_phase": true, "enemies": enemies}, "player": player})
}

/// `helpers.combat_state(hand, enemies, energy=1, ...)` (tests/all/fight has its own, with more keywords).
#[allow(unused_macros)]
macro_rules! combat_state {
    ($hand:expr, $enemies:expr $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::helpers::combat_state_kw(Vec::from($hand), Vec::from($enemies), serde_json::json!({$((stringify!($k)): $v),*}))
    };
}
#[allow(unused_imports)]
pub(crate) use combat_state;

/// `enemy(eid, hp, attack=None, block=0, status=())`: the keywords as a dict (`enemy!("A", 30, attack = 12)` writes it).
pub fn enemy_kw(eid: &str, hp: i64, kw: Value) -> Value {
    for k in kw.as_object().expect("keywords").keys() {
        assert!(["attack", "block", "status"].contains(&k.as_str()), "enemy() got an unexpected keyword argument '{k}'");
    }
    let attack = &kw["attack"];
    let intents = if attack.truthy() { json!([{"type": "Attack", "label": sts2bot::base::str_of(attack)}]) } else { json!([{"type": "Buff", "label": ""}]) };
    json!({"entity_id": eid, "hp": hp, "max_hp": hp, "block": kw.get("block").cloned().unwrap_or(json!(0)), "status": kw.get("status").cloned().unwrap_or(json!([])), "intents": intents})
}

/// `enemy("A", 30, attack=12, status=[...])`.
macro_rules! enemy {
    ($eid:expr, $hp:expr $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::helpers::enemy_kw($eid, $hp, serde_json::json!({$((stringify!($k)): $v),*}))
    };
}
pub(crate) use enemy;

pub fn STRIKE(i: i64) -> Value {
    card(i, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy")
}

pub fn DEFEND(i: i64) -> Value {
    card(i, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self")
}

pub fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 5e-8, "{a} != {b}"); // (assertAlmostEqual: 7 places)
}
