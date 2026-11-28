//! Cards played for nothing: each play in bot.log*, the combat state before it against the state just before the next action.
//!
//! ```text
//! scripts audit/noeffect ['YYYY-MM-DD HH:MM']
//! ```
//!
//! A play that changed nothing the log shows (our side, every enemy, the hand beyond the card itself) is listed by card with
//! the share of its plays that did so: often a card the parser misreads, which the planner scans cannot see. Status cards
//! played to be rid of them, and energy spent at once, are expected on the list.

use indexmap::IndexMap;

use crate::scripts::fightlog::{io_error, read_text, state_home, upto};
use crate::scripts::modelfight::{exit_code, out};
use crate::re;

/// (our side, the enemies, the cards in hand) of a `combat r` line.
pub fn split(s: &str) -> (String, String, Vec<String>) {
    let parts: Vec<&str> = s.split(" | ").collect();
    let hand = if parts[parts.len() - 1].starts_with("hand") { parts[parts.len() - 1] } else { "" };
    let foes = if !hand.is_empty() { parts[1..parts.len() - 1].join(" | ") } else { parts[1..].join(" | ") };
    (parts[0].to_string(), foes, re!(r"\d+:([^,(]+)\(").captures_iter(hand).map(|c| c[1].trim().to_string()).collect())
}

/// `str.splitlines()`.
fn splitlines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split(['\n', '\x0b', '\x0c', '\x1c', '\x1d', '\x1e', '\u{85}', '\u{2028}', '\u{2029}']).collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

/// card -> [plays, plays that changed nothing, examples]
pub type Stats = IndexMap<String, (i64, i64, Vec<String>)>;

pub fn scan(since: &str) -> Result<Stats, String> {
    let mut stats: Stats = IndexMap::new();
    let dir = state_home();
    let shown = dir.to_string_lossy().into_owned();
    let mut names: Vec<String> = Vec::new();
    // (a directory that is not there has no logs, as a glob finds none)
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries {
            let name = entry.map_err(|e| io_error(&e, &shown))?.file_name().to_string_lossy().into_owned();
            if name.starts_with("bot.log") {
                names.push(name);
            }
        }
    }
    // oldest first
    names.sort_by_key(|name| {
        let suffix = name.rfind('.').filter(|i| *i > 0 && *i + 1 < name.len()).map_or("", |i| &name[i + 1..]);
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_numeric()) {
            -suffix.parse::<i64>().unwrap_or(0)
        } else {
            0
        }
    });
    for name in names {
        let text = read_text(&dir.join(&name).to_string_lossy())?;
        let mut last: Option<(i64, String, String)> = None;
        let mut prev: Option<(i64, String, String)> = None;
        let mut pending: Option<String> = None;
        for line in splitlines(&text) {
            if !since.is_empty() && upto(line, 16) < since {
                continue;
            }
            if let Some(m) = re!(r"DEBUG combat r(\d+) (.*)$").captures(line) {
                last = Some((m[1].parse().map_err(|_| format!("ValueError: invalid literal for int() with base 10: '{}'", &m[1]))?, m[2].to_string(), upto(line, 19).to_string()));
                continue;
            }
            if !line.contains("->") || !["play_card", "end_turn", "use_potion"].iter().any(|k| line.contains(k)) {
                continue;
            }
            if let (Some(l), Some(card), Some(p)) = (&last, &pending, &prev) {
                if l.0 == p.0 {
                    let ((me0, foes0, hand0), (me1, foes1, mut hand1)) = (split(&p.1), split(&l.1));
                    let mut rest = hand0;
                    if let Some(i) = rest.iter().position(|c| c == card) {
                        rest.remove(i);
                    }
                    let row = stats.entry(card.clone()).or_default();
                    row.0 += 1;
                    rest.sort();
                    hand1.sort();
                    if me0 == me1 && foes0 == foes1 && rest == hand1 {
                        row.1 += 1;
                        if row.2.len() < 2 {
                            row.2.push(format!("{} | {}", p.2, upto(&p.1, 230)));
                        }
                    }
                }
            }
            prev = last.clone();
            pending = re!(r"Playing '([^']+)'").captures(line).map(|m| m[1].to_string());
        }
    }
    Ok(stats)
}

fn script(args: &[String]) -> Result<(), String> {
    let stats = scan(args.first().map_or("", String::as_str))?;
    let mut rows: Vec<(&String, i64, i64, &Vec<String>)> = stats.iter().filter(|(_, (a, b, _))| *b >= 2 && *b as f64 / *a as f64 >= 0.25).map(|(n, (a, b, ex))| (n, *a, *b, ex)).collect();
    rows.sort_by_key(|r| -r.2);
    for (n, a, b, ex) in rows {
        out!("{:28} played {:4}  no visible effect {:4} ({:.0}%)", n, a, b, (100 * b) as f64 / a as f64);
        for e in ex {
            out!("      {e}");
        }
    }
    Ok(())
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(script(&args))
}

#[cfg(test)]
mod tests {
    use crate::scripts::fightlog::tests::state;
    use crate::scripts::modelfight::golden;
    use super::main;

    #[test]
    fn as_recorded() {
        let _state = state();
        golden(main, &[], include_str!("../../../tests/golden/logs/audit_noeffect.txt"));
        golden(main, &["2026-11-22 12:00"], include_str!("../../../tests/golden/logs/audit_noeffect_since.txt"));
    }
}
