//! The keys to the first game's act 4 (open once three characters have won: speedmod.rs sets CLASS_WIN), and when the bot
//! goes for each: Ruby (a rest site's Recall), Emerald (the burning elite), Sapphire (a chest, instead of its relic). The mod
//! adds them to CommunicationMod's state; screens.rs shows the bot only the choice wanted.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::py::{self, JsonExt};

pub const KEYS: [&str; 3] = ["ruby", "emerald", "sapphire"];
pub const RUBY_HP: f64 = 0.70; // an act 3 rest site is given up for the Ruby Key from this share of Max HP up ...
pub const RUBY_HP_LAST: f64 = 0.45; // ... and the last one before the boss from this share (no other rest site comes after it)
// The keys come first (user, 2026-11-25 19:20: "for sts1 make it a priority to get key"): before act 3, Recall is offered at every
// rest site and `strategy::rest_site` takes it wherever it would have smithed (`ruby_open`); act 3 then only has to catch the rest.
pub const BOSS_ROW_FLOOR: i64 = 15; // floor % 17 of the row of rest sites before every boss (floors 15, 32, 49)

/// The keys this run still needs (none while act 4 is shut: then a key is worth nothing).
pub fn missing(game: &Value) -> BTreeSet<String> {
    if !game["final_act_available"].truthy() {
        return BTreeSet::new();
    }
    let held = &game["keys"];
    KEYS.iter().filter(|k| !held[**k].truthy()).map(|k| k.to_string()).collect()
}

/// `game.get("act") == 3` (a number that equals 3; nothing else does).
fn _act_3(game: &Value) -> bool {
    matches!(&game["act"], Value::Number(n) if n.as_f64() == Some(3.0))
}

/// The Emerald Key is held, or can still be had: before act 3 a later map brings another burning elite, in act 3 one has to be
/// on a row of this map still to come (map rows 0-14 are floors 1-15, 18-32, 35-49). Without it act 4 stays shut whatever else
/// is held, and the act 3 chest's relic and its rest sites are worth more than the other keys.
pub fn emerald_ahead(game: &Value) -> bool {
    if game["keys"]["emerald"].truthy() || game["act"].int() < 3 {
        return true;
    }
    let row = py::modulo(game["floor"].int() - 1, 17);
    game["map"].list().iter().any(|n| n["emerald_key"].truthy() && n["y"].int() > row)
}

/// Recall is on offer here in place of an upgrade (the rest-site rule still rests first when hurt).
pub fn ruby_open(game: &Value) -> bool {
    missing(game).contains("ruby") && emerald_ahead(game)
}

/// Recall at this rest site whatever else it offers: only in act 3 (the last chance), with HP to spare for the act 3 boss.
pub fn wants_ruby(game: &Value) -> bool {
    if !missing(game).contains("ruby") || !_act_3(game) || !emerald_ahead(game) {
        return false;
    }
    let hp = game["current_hp"].float();
    let top = if game["max_hp"].truthy() { game["max_hp"].float() } else { 1.0 };
    let last = py::modulo(game["floor"].int(), 17) == BOSS_ROW_FLOOR;
    hp >= RUBY_HP * top || (last && hp >= RUBY_HP_LAST * top)
}

/// The key over the chest's relic, in act 3 only (its middle row is a treasure room on every road: a sure chance).
pub fn wants_sapphire(game: &Value) -> bool {
    missing(game).contains("sapphire") && _act_3(game) && emerald_ahead(game)
}

/// The burning elite is marked for the route model (strategy::path_value: an "EmeraldElite" is an elite worth more).
pub fn wants_emerald(game: &Value) -> bool {
    missing(game).contains("emerald")
}

#[cfg(test)]
mod tests {
    //! Replays tests/golden/sts1_text.json (written by CPython 3.13 from keys.py over the games of tests/sts1/test_keys.py and more).
    use super::*;
    use serde_json::json;

    #[test]
    fn the_keys_are_wanted_when_python_wants_them() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/sts1_text.json")).unwrap();
        assert_eq!(json!([KEYS, RUBY_HP, RUBY_HP_LAST, BOSS_ROW_FLOOR]), json!(g["keys_consts"].list()[..4]));
        let cases = g["keys"].list();
        assert!(cases.len() > 1000);
        for c in cases {
            let game = &c[0];
            let got = json!([missing(game).into_iter().collect::<Vec<_>>(), emerald_ahead(game), ruby_open(game), wants_ruby(game), wants_sapphire(game), wants_emerald(game)]);
            assert_eq!(got, json!(c.list()[1..]), "{game}");
        }
    }
}
