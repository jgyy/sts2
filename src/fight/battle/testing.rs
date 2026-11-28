//! What the tests of the fight modules share: the states of tests/golden/fight, and the original's objects read back from JSON.

use super::*;
use crate::games;

/// A CardInfo from the fields `v` names (`{"C": {...}}` or the fields themselves), the rest as `CardInfo::default()` has them.
pub fn card_from_json(v: &Value) -> CardInfo {
    let v = if v.has("C") { &v["C"] } else { v };
    let mut c = CardInfo::default();
    for (k, x) in v.entries() {
        match k.as_str() {
            "index" => c.index = x.int(),
            "name" => c.name = x.s().to_string(),
            "type" => c.r#type = x.s().to_string(),
            "cost" => c.cost = x.int(),
            "star_cost" => c.star_cost = x.int(),
            "damage" => c.damage = x.int(),
            "hits" => c.hits = x.int(),
            "double_if_vulnerable" => c.double_if_vulnerable = x.truthy(),
            "aoe" => c.aoe = x.truthy(),
            "random_target" => c.random_target = x.truthy(),
            "block" => c.block = x.int(),
            "vulnerable" => c.vulnerable = x.int(),
            "weak" => c.weak = x.int(),
            "draw" => c.draw = x.int(),
            "energy_gain" => c.energy_gain = x.int(),
            "hp_loss" => c.hp_loss = x.int(),
            "needs_target" => c.needs_target = x.truthy(),
            "playable" => c.playable = x.truthy(),
            "deadly" => c.deadly = x.truthy(),
            "block_lock" => c.block_lock = x.truthy(),
            "known" => c.known = x.truthy(),
            "strength" => c.strength = x.int(),
            "strength_lasts" => c.strength_lasts = x.truthy(),
            "enemy_strength_loss" => c.enemy_strength_loss = x.int(),
            "enemy_strength_lasts" => c.enemy_strength_lasts = x.truthy(),
            "key" => c.key = x.s().to_string(),
            "bound" => c.bound = x.truthy(),
            "poison" => c.poison = x.int(),
            "poison_needs" => c.poison_needs = x.truthy(),
            "doom" => c.doom = x.int(),
            "shivs" => c.shivs = x.int(),
            "star_gain" => c.star_gain = x.int(),
            "forge" => c.forge = x.int(),
            "summon" => c.summon = x.int(),
            "dexterity" => c.dexterity = x.int(),
            "dexterity_lasts" => c.dexterity_lasts = x.truthy(),
            "channel" => c.channel = x.list().iter().map(|s| s.s().to_string()).collect(),
            "evoke" => c.evoke = x.int(),
            "next_energy" => c.next_energy = x.int(),
            "next_draw" => c.next_draw = x.int(),
            "next_block" => c.next_block = x.int(),
            "keeps_hand" => c.keeps_hand = x.list().iter().map(|s| s.s().to_string()).collect(),
            "enemy_hp_loss" => c.enemy_hp_loss = x.int(),
            "souls" => c.souls = x.int(),
            "evoke_all" => c.evoke_all = x.int(),
            "short" => c.short = x.truthy(),
            "osty_attack" => c.osty_attack = x.truthy(),
            "osty_dies" => c.osty_dies = x.truthy(),
            "doom_kills" => c.doom_kills = x.truthy(),
            "debris" => c.debris = x.int(),
            "junk" => c.junk = x.int(),
            "enemy_strength_gain" => c.enemy_strength_gain = x.int(),
            "heal" => c.heal = x.int(),
            "ethereal" => c.ethereal = x.truthy(),
            "ends_turn" => c.ends_turn = x.truthy(),
            "replay" => c.replay = x.int(),
            "play_limit" => c.play_limit = x.int(),
            "shoots" => c.shoots = x.truthy(),
            "aim_times" => c.aim_times = x.int(),
            "aim" => c.aim = x.int(),
            "stretch" => c.stretch = x.int(),
            "impaled_only" => c.impaled_only = x.truthy(),
            "on_impaled" => c.on_impaled = if x.list().len() == 2 { Some((x[0].int(), x[1].int())) } else { None },
            "splash" => c.splash = x.truthy(),
            "stretch_dealt" => c.stretch_dealt = x.truthy(),
            "dislodge" => c.dislodge = x.truthy(),
            "impaled_double" => c.impaled_double = x.truthy(),
            "arrow_block" => c.arrow_block = x.truthy(),
            "melody" => c.melody = x.list().iter().map(|s| s.s().to_string()).collect(),
            "melody_hits" => c.melody_hits = x.truthy(),
            "melody_block" => c.melody_block = x.int(),
            "melody_twice" => c.melody_twice = x.list().iter().map(|s| s.s().to_string()).collect(),
            "verse" => c.verse = x.int(),
            "stance" => c.stance = x.s().to_string(),
            "wrath_next" => c.wrath_next = x.truthy(),
            "energy_drain" => c.energy_drain = x.int(),
            "mantra" => c.mantra = x.int(),
            "sadness" => c.sadness = x.int(),
            "sad_turn" => c.sad_turn = if x.list().len() == 2 { Some((x[0].int(), x[1].int())) } else { None },
            other => panic!("no CardInfo field {other}"),
        }
    }
    c
}

pub fn cards_from_json(v: &Value) -> Vec<CardInfo> {
    v.list().iter().map(card_from_json).collect()
}

/// An Enemy from `{"E": {...}}`.
pub fn foe(v: &Value) -> Enemy {
    assert!(v.has("E"), "not an enemy: {v}");
    Enemy::from_json(&v["E"])
}

pub fn foes(v: &Value) -> Vec<Enemy> {
    v.list().iter().map(foe).collect()
}

/// A float as the golden files hold it (an infinite one as "inf").
pub fn fj(x: f64) -> Value {
    if x == f64::INFINITY {
        json!("inf")
    } else if x == f64::NEG_INFINITY {
        json!("-inf")
    } else {
        json!(x)
    }
}

/// The game, constants, tables and settings a case was recorded under, in force until dropped.
pub struct Env {
    _game: games::Playing,
    _constants: Vec<crate::tunable::Restore>,
    _threat: Option<KnownThreatGuard>,
    _minions: Option<MainMinionsGuard>,
    _knobs: crate::learn::KnobGuard,
}

pub fn env(v: &Value) -> Env {
    let mut constants = vec![crate::fight::eruption::ERUPTION.set(false), SHIFTING.set(false), FOOL_READS.set(false), CHAMP_HOLD.set(0.0), SELF_VULN.set(false), BARRICADE_HOLDS.set(false)]; // (the recordings are older than these rules)
    for (name, x) in v["tun"].entries() {
        constants.push(match name.as_str() {
            "RAGE_BLOCK" => RAGE_BLOCK.set(x.int()),
            "ETHEREAL_KEEP" => crate::fight::triggers::ETHEREAL_KEEP.set(x.float()),
            other => panic!("no constant {other} to set"),
        });
    }
    Env {
        _game: games::playing(v["g"].as_str().unwrap_or("sts2")),
        _constants: constants,
        _threat: v.get("kt").map(|t| with_known_threat(t.entries().map(|(k, x)| (k.clone(), x.float())).collect())),
        _minions: v.get("mm").map(|t| with_main_minions(&t.list().iter().map(|x| x.s()).collect::<Vec<_>>())),
        _knobs: crate::learn::with_knobs(&[("debuff_worth", v["dw"].float_or(2.0))]),
    }
}

/// The states of tests/golden/fight/states*.json: {"g": game, "dw": the debuff_worth setting, "s": state, "r": what the original made of it}.
pub fn states() -> Vec<Value> {
    let mut out = Vec::new();
    for text in [include_str!("../../../tests/golden/fight/states.json"), include_str!("../../../tests/golden/fight/states2.json")] {
        let v: Value = serde_json::from_str(text).unwrap();
        out.extend(v["states"].list().iter().cloned());
    }
    assert!(out.len() > 100, "{} states", out.len());
    out
}

/// The recorded calls of one function: {"a": arguments, "r": result, "m": arguments as it left them, "env": ...}.
pub fn calls<'a>(golden: &'a Value, name: &str, at_least: usize) -> &'a [Value] {
    let got = golden[name].list();
    assert!(got.len() >= at_least, "{name}: {} cases", got.len());
    got
}

pub const TARGETS: [(i64, i64, bool); 7] = [(0, 0, false), (6, 1, false), (12, 1, false), (5, 3, false), (30, 1, false), (9, 2, true), (60, 1, false)];
