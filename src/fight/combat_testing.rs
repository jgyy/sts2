//! What the golden tests of the planner and of run/potions.rs share: the cases of tests/golden/combat, read back.
//! (combat.rs's `testing` module, in a file of its own: the planner is long enough.)

use super::*;
use crate::fight::battle::testing::card_from_json;
use crate::games;

/// The tables the cases refer to: whole cards, relics and potions of the states (`atoms`), parsed cards, and the settings in force.
pub struct Golden {
    pub atoms: Vec<Value>,
    pub cards: Vec<CardInfo>,
    pub knobs: Vec<Vec<(String, f64)>>,
}

/// Every list under `key` in these files, one after the other.
pub fn rows(texts: &[&str], key: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for text in texts {
        let v: Value = serde_json::from_str(text).unwrap();
        out.extend(v[key].list().iter().cloned());
    }
    out
}

pub fn golden() -> Golden {
    let meta = [include_str!("../../tests/golden/combat/meta.json")];
    let atoms = rows(&[include_str!("../../tests/golden/combat/atoms0.json"), include_str!("../../tests/golden/combat/atoms1.json")], "atoms");
    let knobs = rows(&meta, "knobs").iter().map(|k| k.entries().map(|(name, x)| (name.clone(), x.float())).collect()).collect();
    Golden { atoms, cards: rows(&meta, "cards").iter().map(card_from_json).collect(), knobs }
}

/// The game and settings a case was recorded under, in force until dropped.
pub struct Env {
    _game: games::Playing,
    _knobs: crate::learn::KnobGuard,
    /// (the recordings are older than these rules: they are off for them)
    _newer: [crate::tunable::Restore; 6],
}

impl Golden {
    pub fn env(&self, v: &Value) -> Env {
        let knobs: Vec<(&str, f64)> = self.knobs[v["k"].int() as usize].iter().map(|(name, x)| (name.as_str(), *x)).collect();
        Env { _game: games::playing(v["g"].s()), _knobs: crate::learn::with_knobs(&knobs), _newer: [eruption::ERUPTION.set(false), crate::fight::battle::SHIFTING.set(false), crate::fight::battle::FOOL_READS.set(false), crate::fight::battle::CHAMP_HOLD.set(0.0), crate::fight::battle::SELF_VULN.set(false), crate::fight::battle::BARRICADE_HOLDS.set(false)] }
    }

    fn item(&self, x: &Value) -> Value {
        match x {
            Value::Number(id) => self.atoms[id.as_u64().unwrap() as usize].clone(),
            Value::Array(pair) => {
                let mut card = serde_json::Map::new();
                card.insert("index".to_string(), pair[1].clone());
                card.extend(self.atoms[pair[0].int() as usize].as_object().unwrap().clone());
                Value::Object(card)
            }
            other => other["="].clone(),
        }
    }

    /// The player as recorded: the cards, relics and potions of its piles put back.
    pub fn player(&self, packed: &Value) -> Value {
        let mut player = packed.clone();
        for pile in ["hand", "draw_pile", "discard_pile", "exhaust_pile", "relics", "potions"] {
            if packed[pile].is_array() {
                player[pile] = packed[pile].list().iter().map(|x| self.item(x)).collect();
            }
        }
        player
    }

    pub fn state(&self, packed: &Value) -> Value {
        let mut state = packed.clone();
        if packed["player"].is_object() {
            state["player"] = self.player(&packed["player"]);
        }
        if packed["_costs"].has("@") {
            state["_costs"] = self.atoms[packed["_costs"]["@"].int() as usize].clone();
        }
        state
    }

    /// Parsed cards by their place in the table (None for null).
    pub fn line(&self, ids: &Value) -> Option<Vec<CardInfo>> {
        ids.as_array().map(|ids| ids.iter().map(|id| self.cards[id.int() as usize].clone()).collect())
    }
}

pub fn wrath(v: &Value) -> Option<Wrath> {
    if v.is_null() {
        return None;
    }
    Some((v[0].float(), v[1].entries().map(|(id, seen)| (id.clone(), (seen[0].float(), seen[1].float()))).collect(), v[2].float()))
}

/// `simulate`'s keywords from the ones a case gives (the rest: the defaults).
pub fn args(v: &Value) -> SimulateArgs {
    let mut a = SimulateArgs::default();
    let two = |x: &Value| (x[0].float(), x[1].float());
    let whole = |x: &Value| {
        assert!(x.is_int() || x.is_boolean(), "not an int: {x}");
        x.int()
    };
    for (k, x) in v.entries() {
        match k.as_str() {
            "energy" => a.energy = whole(x),
            "stars" => a.stars = whole(x),
            "hp" => a.hp = whole(x),
            "max_hp" => a.max_hp = whole(x),
            "block" => a.block = whole(x),
            "fight" => a.fight = x.s().to_string(),
            "round_no" => a.round_no = whole(x),
            "card_limit" => a.card_limit = if x.is_null() { None } else { Some(whole(x)) },
            "can_block" => a.can_block = x.truthy(),
            "sandpit" => a.sandpit = if x.is_null() { None } else { Some(whole(x)) },
            "end_loss" => a.end_loss = whole(x),
            "burns" => a.burns = x.list().iter().map(|b| (whole(&b[0]), whole(&b[1]), b[2].truthy())).collect(),
            "hand_size" => a.hand_size = whole(x),
            "focus" => {
                a.focus = if x.is_null() {
                    None
                } else if x.has("last") {
                    Some(Focus::Last(x["last"].s().to_string()))
                } else {
                    Some(Focus::At(x["at"].s().to_string()))
                }
            }
            "orbs" => a.orbs = x.list().iter().map(|o| (o[0].s().to_string(), whole(&o[1]), whole(&o[2]))).collect(),
            "orb_slots" => a.orb_slots = whole(x),
            "shiv_damage" => a.shiv_damage = whole(x),
            "tender" => a.tender = x.truthy(),
            "vigor" => a.vigor = whole(x),
            "giant" => a.giant = x.truthy(),
            "buffer" => a.buffer = whole(x),
            "loss_cap" => a.loss_cap = whole(x),
            "taint" => a.taint = whole(x),
            "draw_worth" => a.draw_worth = (x[0].float(), x[1].float(), x[2].float()),
            "bare_block" => a.bare_block = whole(x),
            "regen" => a.regen = whole(x),
            "focus_power" => a.focus_power = whole(x),
            "escape_odds" => a.escape_odds = two(x),
            "wither_in" => a.wither_in = whole(x),
            "wither_hp" => a.wither_hp = whole(x),
            "wither_bill" => a.wither_bill = x.float(),
            "trig" => {
                let table = |t: &Value| t.entries().map(|(id, n)| (id.clone(), n.int())).collect::<IndexMap<String, i64>>();
                a.trig = if x.is_null() {
                    None
                } else {
                    Some(triggers::Triggers {
                        per_debuff: x["per_debuff"].int(),
                        per_card: x["per_card"].int(),
                        per_soul: x["per_soul"].int(),
                        card_block: x["card_block"].int(),
                        attack_block: x["attack_block"].int(),
                        costly_block: x["costly_block"].int(),
                        doom_block: x["doom_block"].int(),
                        strangle: table(&x["strangle"]),
                        oblivion: table(&x["oblivion"]),
                    })
                }
            }
            "pet" => a.pet = whole(x),
            "hail" => a.hail = whole(x),
            "deck" => a.deck = (whole(&x[0]), whole(&x[1])),
            "mastery" => a.mastery = whole(x),
            "might0" => a.might0 = whole(x),
            "holds" => a.holds = x.truthy(),
            "strip" => a.strip = two(x),
            "friends" => a.friends = whole(x),
            "drained" => a.drained = whole(x),
            "free" => a.free = whole(x),
            "doubled" => a.doubled = x.truthy(),
            "fortify" => a.fortify = whole(x),
            "pace" => a.pace = x.float(),
            "room" => a.room = two(x),
            "arrows" => a.arrows = whole(x),
            "aiming" => a.aiming = whole(x),
            "stretch" => a.stretch = whole(x),
            "tune" => a.tune = x.list().iter().map(|m| m.s().to_string()).collect(),
            "chord" => a.chord = whole(x),
            "boom" => a.boom = whole(x),
            "fool" => a.fool = if x.is_null() { None } else { Some((whole(&x[0]), whole(&x[1]), whole(&x[2]), whole(&x[3]), whole(&x[4]))) },
            "stance0" => a.stance0 = x.s().to_string(),
            "wrath" => a.wrath = wrath(x),
            "sadness" => a.sadness = if x.is_null() { None } else { Some((whole(&x[0]), whole(&x[1]), whole(&x[2]), x[3].float(), x[4].float())) },
            "mantra0" => a.mantra0 = whole(x),
            other => panic!("no keyword {other}"),
        }
    }
    a
}

pub fn action(a: &Option<Action>) -> Value {
    match a {
        Some((name, params)) => json!([name, params]),
        None => Value::Null,
    }
}

pub fn actions_json(all: &[Action]) -> Value {
    all.iter().map(|(name, params)| json!([name, params])).collect()
}

pub const CALLS: &str = include_str!("../../tests/golden/combat/calls.json");
