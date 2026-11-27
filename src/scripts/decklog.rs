//! What the decks that fought cheaply held: fights.jsonl (each fight's deck, relics and HP in and out) against wiki.json
//! (what each card is; `scripts fightlog wiki` writes it, and a card it lacks counts in the deck's size only).
//!
//! ```text
//! scripts decklog [ACT] ['YYYY-MM-DD HH:MM']          ordinary fights: what one costs (a share of Max HP) against how
//!                                                     many cards of each sort the deck held, the character taken out;
//!                                                     then the cards and relics themselves
//! scripts decklog bosses [ACT] ['YYYY-MM-DD HH:MM']   the same for boss fights, won or lost
//! ```
//!
//! A death costs the HP the fight began on and a tenth of Max HP more; fights begun under half HP or joined half way are left
//! out. t is the slope over its standard error (with twenty sorts of card on the table, one passes 2 by chance).

use std::collections::BTreeSet;
use std::path::Path;

use indexmap::IndexMap;
use serde_json::{json, Value};

use super::fightlog::{log_path, read_jsonl, read_text};
use super::modelfight::{exit_code, out};
use crate::cards;
use crate::py::{self, JsonExt};
use crate::run::decks;

pub const DOC: &str = "What the decks that fought cheaply held: fights.jsonl (each fight's deck, relics and HP in and out) against wiki.json
(what each card is; `scripts fightlog wiki` writes it, and a card it lacks counts in the deck's size only).

    scripts decklog [ACT] ['YYYY-MM-DD HH:MM']          ordinary fights: what one costs (a share of Max HP) against how
                                                        many cards of each sort the deck held, the character taken out;
                                                        then the cards and relics themselves
    scripts decklog bosses [ACT] ['YYYY-MM-DD HH:MM']   the same for boss fights, won or lost

A death costs the HP the fight began on and a tenth of Max HP more; fights begun under half HP or joined half way are left
out. t is the slope over its standard error (with twenty sorts of card on the table, one passes 2 by chance).
";

/// Card name (as a deck lists it, upgraded or not) -> the card as the planner reads one.
pub fn _wiki() -> Result<IndexMap<String, Value>, String> {
    let wiki: Value = serde_json::from_str(&read_text(&log_path("wiki.json"))?).map_err(|e| format!("json.decoder.JSONDecodeError: {e}"))?;
    let mut out = IndexMap::new();
    for (cid, entry) in wiki["cards"].as_object().ok_or("KeyError: 'cards'")? {
        for v in [&entry["base"], &entry["upgraded"]] {
            if v.truthy() {
                let name = v["name"].as_str().ok_or("KeyError: 'name'")?;
                out.insert(name.to_string(), json!({"id": cid, "name": name, "type": v["type"], "cost": v["cost"], "description": v["description"], "rarity": v["rarity"]}));
            }
        }
    }
    Ok(out)
}

pub fn load(since: &str) -> Result<Vec<Value>, String> {
    let mut fights = Vec::new();
    for path in [log_path("fights.jsonl.1"), log_path("fights.jsonl")] {
        if Path::new(&path).exists() {
            for r in read_jsonl(&path)? {
                if r["whole"].truthy() && r["time"].as_str().ok_or("KeyError: 'time'")?.replace('T', " ").as_str() >= since {
                    fights.push(r);
                }
            }
        }
    }
    Ok(fights)
}

pub fn features(fight: &Value, wiki: &IndexMap<String, Value>) -> IndexMap<&'static str, i64> {
    let names: Vec<&str> = fight["deck"].list().iter().map(|n| n.s()).collect();
    let known: Vec<&Value> = names.iter().filter_map(|n| wiki.get(*n)).collect();
    let infos: Vec<cards::CardInfo> = known
        .iter()
        .map(|c| {
            let mut card = (*c).clone();
            card["index"] = json!(0);
            cards::parse(&card)
        })
        .collect();
    let own: Vec<(&Value, &cards::CardInfo)> = known.iter().copied().zip(infos.iter()).filter(|(c, _)| c["rarity"] != "Basic" && c["type"] != "Curse" && c["type"] != "Status").collect();
    let count = |test: &dyn Fn(&Value, &cards::CardInfo) -> bool| own.iter().filter(|(c, i)| test(c, i)).count() as i64;
    IndexMap::from([
        ("Block cards", count(&|c, _| decks::_blocks(c))),
        ("cards for damage", count(&|c, _| decks::_punch(c) >= 8.0)),
        ("other attacks", count(&|c, _| c["type"] == "Attack" && decks::_punch(c) < 8.0)),
        ("hit ALL", count(&|_, i| i.aoe && i.damage != 0)),
        ("Powers", count(&|c, _| c["type"] == "Power")),
        ("other Skills", count(&|c, _| c["type"] == "Skill" && !decks::_blocks(c))),
        ("Weak", count(&|_, i| i.weak != 0)),
        ("Vulnerable", count(&|_, i| i.vulnerable != 0)),
        ("cards drawn", own.iter().map(|(_, i)| i.draw).sum()),
        ("cost 0", count(&|_, i| i.cost == 0)),
        ("cost 2+", count(&|_, i| i.cost >= 2)),
        ("upgraded", names.iter().filter(|n| n.ends_with('+')).count() as i64),
        ("Strikes", known.iter().filter(|c| c["id"].s().starts_with("STRIKE")).count() as i64),
        ("Defends", known.iter().filter(|c| c["id"].s().starts_with("DEFEND")).count() as i64),
        ("Curses", known.iter().filter(|c| c["type"] == "Curse" || c["type"] == "Status").count() as i64),
        ("deck size", names.len() as i64),
        ("relics", fight["relics"].list().len() as i64),
        ("potions", fight["potions"].list().len() as i64),
    ])
}

/// (slope, its standard error) of y on x with a constant for every group: rows are (group, x, y).
pub fn _slope(rows: &[(String, i64, f64)]) -> (f64, f64) {
    let mut by: IndexMap<&str, Vec<(i64, f64)>> = IndexMap::new();
    for (g, x, y) in rows {
        by.entry(g.as_str()).or_default().push((*x, *y));
    }
    let mut centred: Vec<(f64, f64)> = Vec::new();
    for pts in by.values() {
        let mx = pts.iter().map(|p| p.0).sum::<i64>() as f64 / pts.len() as f64;
        let my = py::fsum(pts.iter().map(|p| p.1)) / pts.len() as f64;
        centred.extend(pts.iter().map(|(x, y)| (*x as f64 - mx, y - my)));
    }
    let sxx = py::fsum(centred.iter().map(|(x, _)| x * x));
    let sxy = py::fsum(centred.iter().map(|(x, y)| x * y));
    let free = centred.len() as i64 - by.len() as i64 - 1;
    if sxx <= 0.0 || free <= 0 {
        return (0.0, 0.0);
    }
    let b = sxy / sxx;
    (b, (py::fsum(centred.iter().map(|(x, y)| (y - b * x).powf(2.0))) / free as f64 / sxx).powf(0.5))
}

pub fn report(fights: &[Value], bosses: bool) -> Result<(), String> {
    let wiki = _wiki()?;
    let acts: BTreeSet<i64> = fights.iter().map(|f| f["act"].int()).collect();
    for act in acts {
        let sub: Vec<&Value> = if bosses {
            fights.iter().filter(|f| f["act"].int() == act && f["kind"] == "boss").collect()
        } else {
            fights.iter().filter(|f| f["act"].int() == act && f["kind"] == "monster" && f["max_hp"].truthy() && f["hp_in"].float() >= 0.5 * f["max_hp"].float()).collect()
        };
        let outcome = |f: &Value| -> f64 {
            if bosses {
                100.0 * (!f["died"].truthy()) as i64 as f64
            } else if f["died"].truthy() {
                100.0 * (f["hp_in"].float() / f["max_hp"].float() + 0.1)
            } else {
                100.0 * ((f["hp_in"].int() - f["hp_out"].int()) as f64 / f["max_hp"].float())
            }
        };
        let (what, unit) = if bosses { ("won", "points of win rate") } else { ("cost, % of Max HP", "% of Max HP") };
        if sub.len() < 30 {
            out!("act {}: {} fights, too few", act, sub.len());
            continue;
        }
        let feats: Vec<IndexMap<&'static str, i64>> = sub.iter().map(|f| features(f, &wiki)).collect();
        let ys: Vec<f64> = sub.iter().map(|f| outcome(f)).collect();
        let who = |f: &Value| py::str_of(&f["character"]);
        out!("act {}: {} {} fights; {} {:.1} on average", act, sub.len(), if bosses { "boss" } else { "ordinary" }, what, py::fsum(ys.iter().copied()) / ys.len() as f64);
        for name in feats[0].keys() {
            let xs: Vec<i64> = feats.iter().map(|ft| ft[name]).collect();
            let rows: Vec<(String, i64, f64)> = sub.iter().zip(&xs).zip(&ys).map(|((f, x), y)| (who(f), *x, *y)).collect();
            let (b, se) = _slope(&rows);
            // (sorted by the text of each (count, outcomes) pair: the counts are 0 to 3, so by the count)
            let mut dose: std::collections::BTreeMap<i64, Vec<f64>> = std::collections::BTreeMap::new();
            if !["deck size", "relics", "upgraded", "cards drawn"].contains(name) {
                for (x, y) in xs.iter().zip(&ys) {
                    dose.entry((*x).min(3)).or_default().push(*y);
                }
            }
            let table = dose.iter().map(|(k, v)| format!("{}{}: {:4.1} ({})", k, if *k == 3 { "+" } else { "" }, py::fsum(v.iter().copied()) / v.len() as f64, v.len())).collect::<Vec<_>>().join("  ");
            out!(
                "  {:17} held {:4.1}   one more: {:+5.2} {} (t {:+.1})   {}",
                name,
                xs.iter().sum::<i64>() as f64 / xs.len() as f64,
                b,
                unit,
                if se != 0.0 { b / se } else { 0.0 },
                table
            );
        }
        let mut lists: IndexMap<String, Vec<f64>> = IndexMap::new();
        for (f, y) in sub.iter().zip(&ys) {
            lists.entry(who(f)).or_default().push(*y);
        }
        let par: IndexMap<String, f64> = lists.into_iter().map(|(k, v)| (k, py::fsum(v.iter().copied()) / v.len() as f64)).collect();
        for (label, key) in [("cards", "deck"), ("relics", "relics")] {
            let mut seen: IndexMap<String, Vec<f64>> = IndexMap::new();
            for (f, y) in sub.iter().zip(&ys) {
                let items: BTreeSet<&str> = f[key].list().iter().map(|n| n.s().trim_end_matches('+')).collect();
                for item in items {
                    seen.entry(item.to_string()).or_default().push(y - par[&who(f)]);
                }
            }
            let mut ranked: Vec<(f64, usize, &String)> = seen.iter().filter(|(_, v)| v.len() >= 25).map(|(k, v)| (py::fsum(v.iter().copied()) / v.len() as f64, v.len(), k)).collect();
            py::sort_by_key(&mut ranked, |r| (r.0, r.1, r.2.clone()));
            let ends: Vec<&(f64, usize, &String)> = if ranked.len() > 20 { ranked[..10].iter().chain(&ranked[ranked.len() - 10..]).collect() } else { ranked.iter().collect() };
            out!(
                "  {} against the character's usual ({}; 25+ fights; the ten at each end): {}",
                label,
                unit,
                ends.iter().map(|(m, n, k)| format!("{} {:+.1} ({})", k, m, n)).collect::<Vec<_>>().join(", ")
            );
        }
    }
    Ok(())
}

fn script(args: &[String]) -> Result<(), String> {
    let bosses = args.first().map(String::as_str) == Some("bosses");
    let mut args: &[String] = if bosses { &args[1..] } else { args };
    let mut act: Option<i64> = None;
    if let Some(first) = args.first().filter(|a| !a.is_empty() && a.chars().all(|c| c.is_numeric())) {
        act = Some(super::modelfight::int(first)?);
        args = &args[1..];
    }
    let fights: Vec<Value> = load(args.first().map_or("", String::as_str))?.into_iter().filter(|f| act.is_none() || f["act"].as_i64() == act).collect();
    if !fights.is_empty() {
        report(&fights, bosses)
    } else {
        out!("{DOC}\n(no fights on record yet)");
        Ok(())
    }
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(script(&args))
}

#[cfg(test)]
mod tests {
    use super::super::fightlog::tests::state;
    use super::super::modelfight::golden;
    use super::main;

    #[test]
    fn as_python() {
        let _state = state();
        golden(main, &[], include_str!("../../tests/golden/logs/decklog.txt"));
        golden(main, &["1"], include_str!("../../tests/golden/logs/decklog_1.txt"));
        golden(main, &["2026-11-21 00:00"], include_str!("../../tests/golden/logs/decklog_since.txt"));
    }

    #[test]
    fn bosses_as_python() {
        let _state = state();
        golden(main, &["bosses"], include_str!("../../tests/golden/logs/decklog_bosses.txt"));
        golden(main, &["bosses", "1", "2026-11-20 09:30"], include_str!("../../tests/golden/logs/decklog_bosses_since.txt"));
    }
}
