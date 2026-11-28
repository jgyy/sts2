//! Cards whose text names an amount that `cards::parse` read as nothing, or a condition it ignored.
//!
//! ```text
//! scripts audit/parse_vs_text
//! ```
//!
//! Reads the wiki dump (`scripts fightlog wiki` writes it). Powers are mostly priced by other means: look at the Skills and
//! Attacks. The second list is every card with an "if", "unless" or "only" in its text, to check against what `parse` gives.

use std::collections::HashSet;

use serde_json::Value;

use super::fightlog::{read_text, state_home, upto};
use super::modelfight::{exit_code, out};
use crate::cards::{self, CardInfo};
use crate::base::{self, JsonExt};

type Check = (&'static str, &'static str, fn(&CardInfo) -> i64);

pub const CHECKS: [Check; 8] = [
    ("damage", r"(?i)deal\s+(\d+)\s+damage", |i| if i.damage != 0 { i.damage } else { i.total_damage() }),
    ("block", r"(?i)gain\s+(\d+)\s+block", |i| i.block),
    ("weak", r"(?i)apply\s+(\d+)\s+weak", |i| i.weak),
    ("vulnerable", r"(?i)apply\s+(\d+)\s+vulnerable", |i| i.vulnerable),
    ("poison", r"(?i)apply\s+(\d+)\s+poison", |i| i.poison),
    ("doom", r"(?i)apply\s+(\d+)\s+doom", |i| i.doom),
    ("draw", r"(?i)draw\s+(\d+)\s+card", |i| i.draw),
    ("strength", r"(?i)gain\s+(\d+)\s+strength", |i| i.strength),
];

fn script() -> Result<(), String> {
    let path = state_home().join("wiki.json").to_string_lossy().into_owned();
    let wiki: Value = serde_json::from_str(&read_text(&path)?).map_err(|e| format!("json.decoder.JSONDecodeError: {e}"))?;
    let mut wiki: Vec<(&String, &Value)> = wiki["cards"].as_object().ok_or("KeyError: 'cards'")?.iter().collect();
    wiki.sort_by(|a, b| a.0.cmp(b.0));
    let kind = |v: &Value| v["type"].as_str().map(str::to_string).ok_or("TypeError: unsupported format string passed to NoneType.__format__");
    out!("-- named in the text, read as 0");
    let mut seen: HashSet<(&String, &str)> = HashSet::new();
    for (cid, v) in &wiki {
        for c in [&v["base"], &v["upgraded"]] {
            let text = base::str_of(c["description"].or(&Value::String(String::new())));
            for (name, rx, get) in CHECKS {
                if c.truthy() && base::re::cached(rx).is_match(&text) && get(&cards::parse(c)) == 0 && !seen.contains(&(*cid, name)) {
                    seen.insert((*cid, name));
                    out!("{:22} {:6} {:10} {}", cid, kind(v)?, name, upto(&text.replace('\n', " "), 110));
                }
            }
        }
    }
    out!("-- conditions in the text (the card's own effect, not a Power's trigger)");
    for (cid, v) in &wiki {
        let text = base::str_of(v["base"]["description"].or(&Value::String(String::new())));
        if matches!(v["type"].as_str(), Some("Attack" | "Skill")) && crate::re!(r"(?i)\bif\b|\bunless\b|\bonly\b").is_match(&text) {
            let i = cards::parse(&v["base"]);
            out!("{:22} {:100} d{} b{} p{} dr{}", cid, upto(&text.replace('\n', " "), 100), i.damage, i.block, i.poison, i.draw);
        }
    }
    Ok(())
}

pub fn main(_args: Vec<String>) -> i32 {
    exit_code(script())
}

#[cfg(test)]
mod tests {
    use super::super::fightlog::tests::state;
    use super::super::modelfight::golden;
    use super::main;

    #[test]
    fn as_recorded() {
        let _state = state();
        golden(main, &[], include_str!("../../tests/golden/logs/audit_parse_vs_text.txt"));
    }
}
