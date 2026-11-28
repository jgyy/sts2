//! Every card, power, relic and monster of both games, and how the bot handles each: a rule of its own (its id named in the
//! planner, deck or relic code: rating tables do not count), read from its text by the generic parser, or not handled at all.
//!
//! ```text
//! scripts audit/coverage [sts2|sts1] [cards|powers|relics|monsters|all] [N]   (N: the unhandled to list, by use)
//! ```
//!
//! The game's own lists: ~/.local/state/sts2bot/loc/<game>/ (STS2's localization/eng from SlayTheSpire2.pck, extracted with
//! the game's pack file; the first game's from desktop-1.0.jar). How often each is met: fights.jsonl decks and relics, bot.log power lines.
//!
//! Where the Rust differs from the first version of this script: that one walked the bot's sources for their
//! string constants (docstrings among them). This one reads the Rust sources of the checkout the binary was built from
//! ([`literals`]): the string literals and the doc comments (the docstrings as ported), an item under `#[cfg(test)]` left out
//! (the original's tests were other files), and in a rating table (`static _TIER ... IndexMap::from([("KEY", ..), ..])`) the key
//! that opens each row, as the original left out the keys of the dict.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_json::{json, Value};

use super::fightlog::{io_error, read_jsonl, read_text, state_home, upto, Counter};
use super::modelfight::{exit_code, out};
use crate::base::{self, JsonExt};
use crate::re;

/// Where a named id is behaviour (not a rating): the planner, deck and relic code, events, the first game's screens and stances.
pub const BEHAVIOUR: [&str; 11] =
    ["fight", "run/decks.rs", "run/strategy.rs", "run/potions.rs", "run/relicworth.rs", "run/events", "games/sts1/screens.rs", "games/sts1/stance.rs", "games/sts1/keys.rs", "cards.rs", "bot.rs"];
pub const RATING_TABLES: [&str; 8] = ["_TIER", "_AFFLICTION", "_RARITY", "RATING", "FOOL_RATING", "_ANCIENT", "BOSS_WORTH", "_WORTH"];

fn partial(text: &str) -> bool {
    re!(r"(?i)\bwhenever\b|\bif\b|\bnext turn\b|\bfor each\b|\bthis turn\b|\bat the (?:start|end) of\b|\bretain\b|\bwhen\b|\bexhaust (?:all|your|\d)").is_match(text)
}

/// The string constants of a Rust source: its string literals and doc comments, without the items under `#[cfg(test)]` and
/// the keys of the rating tables.
pub fn literals(source: &str) -> Vec<String> {
    let c: Vec<char> = source.chars().collect();
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    let mut found: Vec<String> = Vec::new();
    let (mut i, mut depth) = (0usize, 0i64);
    let (mut prev, mut before) = (' ', ' '); // the last two characters that were not blank or comment
    let mut ident_before = String::new();
    let mut skip: Option<(i64, bool)> = None; // inside an item under #[cfg(test)]: (the depth it is at, its braces opened)
    let mut table: Option<i64> = None; // inside a rating table: the depth its `static` is at
    let attr: Vec<char> = "#[cfg(test)]".chars().collect();
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '/' && at(i + 1) == '/' {
            let end = (i..c.len()).find(|j| c[*j] == '\n').unwrap_or(c.len());
            let doc = (at(i + 2) == '/' && at(i + 3) != '/') || at(i + 2) == '!';
            if doc && skip.is_none() {
                found.push(c[i + 3..end].iter().collect());
            }
            i = end;
            continue;
        }
        if ch == '/' && at(i + 1) == '*' {
            let mut nested = 1;
            i += 2;
            while i < c.len() && nested > 0 {
                if c[i] == '/' && at(i + 1) == '*' {
                    nested += 1;
                    i += 2;
                } else if c[i] == '*' && at(i + 1) == '/' {
                    nested -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if c[i..].starts_with(&attr) {
            if skip.is_none() {
                skip = Some((depth, false));
            }
            i += attr.len();
            (before, prev) = (prev, ']');
            continue;
        }
        // a string: "..", b"..", r".." and r#".."# (with any number of #)
        let mut j = i;
        let mut raw = false;
        if (ch == 'b' || ch == 'r' || ch == 'c') && !(prev.is_alphanumeric() || prev == '_') {
            if ch != 'r' && at(j + 1) == 'r' {
                j += 1;
            }
            if c[j] == 'r' {
                let hashes = (j + 1..c.len()).take_while(|k| c[*k] == '#').count();
                if at(j + 1 + hashes) == '"' {
                    raw = true;
                    let open = j + 1 + hashes;
                    let mut k = open + 1;
                    let closes = |k: usize| c[k] == '"' && (0..hashes).all(|h| at(k + 1 + h) == '#');
                    while k < c.len() && !closes(k) {
                        k += 1;
                    }
                    let text: String = c[open + 1..k.min(c.len())].iter().collect();
                    let key = table.is_some() && prev == '(' && matches!(before, '[' | ',');
                    if skip.is_none() && !key {
                        found.push(text);
                    }
                    i = (k + 1 + hashes).min(c.len());
                    (before, prev) = (prev, '"');
                }
            } else if at(j + 1) == '"' {
                j += 1;
            } else {
                j = i;
            }
        }
        if raw {
            continue;
        }
        if c[j] == '"' {
            // (in a rating table, the string that opens a row is its key)
            let key = table.is_some() && prev == '(' && matches!(before, '[' | ',');
            let mut text = String::new();
            let mut k = j + 1;
            while k < c.len() && c[k] != '"' {
                if c[k] == '\\' {
                    k += 1;
                    match at(k) {
                        'n' => text.push('\n'),
                        't' => text.push('\t'),
                        'r' => text.push('\r'),
                        '0' => text.push('\0'),
                        '\n' => {
                            while at(k + 1).is_whitespace() {
                                k += 1;
                            }
                        }
                        'x' => {
                            let code: String = c[(k + 1).min(c.len())..(k + 3).min(c.len())].iter().collect();
                            text.push(u8::from_str_radix(&code, 16).map(|b| b as char).unwrap_or(' '));
                            k += 2;
                        }
                        'u' => {
                            let close = (k..c.len()).find(|x| c[*x] == '}').unwrap_or(k);
                            let code: String = c[(k + 2).min(close)..close].iter().collect();
                            text.push(u32::from_str_radix(&code, 16).ok().and_then(char::from_u32).unwrap_or(' '));
                            k = close;
                        }
                        other => text.push(other),
                    }
                    k += 1;
                } else {
                    text.push(c[k]);
                    k += 1;
                }
            }
            if skip.is_none() && !key {
                found.push(text);
            }
            i = k + 1;
            (before, prev) = (prev, '"');
            continue;
        }
        if ch == '\'' {
            // a character, or a lifetime
            if at(i + 1) == '\\' {
                let mut k = i + 3;
                while k < c.len() && c[k] != '\'' {
                    k += 1;
                }
                i = k + 1;
            } else if at(i + 2) == '\'' {
                i += 3;
            } else {
                i += 1;
            }
            (before, prev) = (prev, '\'');
            continue;
        }
        if ch.is_alphanumeric() || ch == '_' {
            let end = (i..c.len()).find(|k| !(c[*k].is_alphanumeric() || c[*k] == '_')).unwrap_or(c.len());
            let ident: String = c[i..end].iter().collect();
            if ident_before == "static" && RATING_TABLES.contains(&ident.as_str()) && table.is_none() {
                table = Some(depth);
            }
            ident_before = ident;
            (before, prev) = (prev, c[end - 1]);
            i = end;
            continue;
        }
        match ch {
            '(' | '[' | '{' => {
                if let (Some((d, opened)), '{') = (&mut skip, ch) {
                    if *d == depth {
                        *opened = true;
                    }
                }
                depth += 1;
            }
            ')' | ']' | '}' => {
                depth -= 1;
                if ch == '}' && skip == Some((depth, true)) {
                    skip = None;
                }
            }
            ';' => {
                if skip == Some((depth, false)) {
                    skip = None;
                }
                if table == Some(depth) {
                    table = None;
                }
            }
            _ => {}
        }
        (before, prev) = (prev, ch);
        i += 1;
    }
    found
}

/// The Rust files of the behaviour code, in order.
pub fn behaviour_files() -> Result<Vec<PathBuf>, String> {
    fn rglob(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        let shown = dir.to_string_lossy().into_owned();
        for entry in std::fs::read_dir(dir).map_err(|e| io_error(&e, &shown))? {
            let path = entry.map_err(|e| io_error(&e, &shown))?.path();
            if path.is_dir() {
                rglob(&path, files)?;
            } else if path.extension().is_some_and(|x| x == "rs") {
                files.push(path);
            }
        }
        Ok(())
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    for p in BEHAVIOUR {
        let path = root.join(p);
        if path.is_dir() {
            let mut found = Vec::new();
            rglob(&path, &mut found)?;
            found.sort();
            files.extend(found);
        } else {
            files.push(path);
        }
    }
    Ok(files)
}

/// Upper-case string literals in the behaviour code, the keys of rating tables left out.
pub fn named_ids() -> Result<HashSet<String>, String> {
    let mut found: HashSet<String> = HashSet::new();
    for f in behaviour_files()? {
        let shown = f.to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&f).map_err(|e| io_error(&e, &shown))?;
        for literal in literals(&text) {
            for word in re!(r"\b[A-Z][A-Z0-9_]{2,}\b").find_iter(&literal) {
                found.insert(word.as_str().to_string());
            }
        }
    }
    Ok(found)
}

fn state_file(parts: &str) -> String {
    state_home().join(parts).to_string_lossy().into_owned()
}

pub fn loc(game: &str, name: &str) -> Result<IndexMap<String, Value>, String> {
    let value: Value = serde_json::from_str(&read_text(&state_file(&format!("loc/{game}/{name}.json")))?).map_err(|e| format!("json.decoder.JSONDecodeError: {e}"))?;
    Ok(value.as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default())
}

/// (decks that held each card, runs' fights that held each relic)
pub fn deck_counts(game: &str) -> Result<(Counter, Counter), String> {
    let path = state_file(if game == "sts2" { "fights.jsonl" } else { "sts1/fights.jsonl" });
    let (mut c, mut relics) = (Counter::default(), Counter::default());
    for f in read_jsonl(&path)? {
        let names: BTreeSet<String> = f["deck"].list().iter().map(base::str_of).collect();
        for name in names {
            c.add(&re!(r"\+\d*$").replace(&name, ""));
        }
        let held: BTreeSet<String> = f["relics"].list().iter().map(base::str_of).collect();
        for r in held {
            relics.add(&r);
        }
    }
    Ok((c, relics))
}

pub fn power_mentions(game: &str) -> Result<Counter, String> {
    let log = read_text(&state_file(if game == "sts2" { "bot.log" } else { "sts1/bot.log" }))?;
    let mut c = Counter::default();
    for m in re!(r"\[([A-Z0-9_:,\-]+)\]").captures_iter(&log) {
        for part in m[1].split(',') {
            c.add(part.split(':').next().unwrap_or(""));
        }
    }
    Ok(c)
}

/// A row of a report: (id, name, how it is handled, how often it is met, its text).
pub type Row = (String, String, &'static str, i64, String);

fn text_of(v: Option<&Value>, default: &str) -> String {
    v.map_or_else(|| default.to_string(), base::str_of)
}

/// The ids of a table of "ID.field" keys (only the keys `keep` takes), in order.
fn ids(data: &IndexMap<String, Value>, keep: impl Fn(&str) -> bool) -> BTreeSet<String> {
    data.keys().filter(|k| keep(k)).map(|k| k.split('.').next().unwrap_or("").to_string()).collect()
}

pub fn sts2_cards(named: &HashSet<String>) -> Result<Vec<Row>, String> {
    let names = loc("sts2", "cards")?;
    let wiki: Value = serde_json::from_str(&read_text(&state_file("wiki.json"))?).map_err(|e| format!("json.decoder.JSONDecodeError: {e}"))?;
    let wiki = &wiki["cards"];
    let (decks, _) = deck_counts("sts2")?;
    let mut rows = Vec::new();
    for cid in ids(&names, |k| k.ends_with(".title")) {
        let title = text_of(names.get(&format!("{cid}.title")), &cid);
        let entry = wiki.get(&cid);
        let mut text = entry.map_or(String::new(), |e| e["base"]["description"].text());
        if text.is_empty() {
            text = re!(r"\{[^}]*\}").replace_all(&text_of(names.get(&format!("{cid}.description")), ""), "N").into_owned();
        }
        let status = if named.contains(&cid) {
            "rule"
        } else if let Some(entry) = entry {
            let info = crate::cards::parse(&json!({"id": cid, "name": title, "type": entry["type"], "cost": entry["base"]["cost"], "description": text, "target_type": "AnyEnemy"}));
            if info.known {
                if partial(&text) {
                    "partial"
                } else {
                    "read"
                }
            } else if entry["type"] == "Power" {
                "power"
            } else {
                "unread"
            }
        } else {
            "unseen"
        };
        let used = decks.get(&title);
        rows.push((cid, title, status, used, text));
    }
    Ok(rows)
}

pub fn sts1_cards(named: &HashSet<String>) -> Result<Vec<Row>, String> {
    use crate::games::sts1::text;
    let (decks, _) = deck_counts("sts1")?;
    let mut tables: IndexMap<&str, &str> = IndexMap::new();
    for table in [text::ironclad::CARDS(), text::silent::CARDS(), text::defect::CARDS(), text::watcher::CARDS(), text::colorless::CARDS()] {
        tables.extend(table.iter().map(|(k, v)| (*k, *v)));
    }
    let names: IndexMap<String, String> = loc("sts1", "cards")?.iter().map(|(k, v)| (k.to_uppercase().replace(' ', "_"), text_of(v.get("NAME"), k))).collect();
    let mut sorted: Vec<(&str, &str)> = tables.into_iter().collect();
    sorted.sort();
    let mut rows = Vec::new();
    for (cid, text) in sorted {
        let plain = re!(r"\(\d+\)").replace_all(text, "").into_owned();
        let name = names.get(cid).cloned().unwrap_or_else(|| cid.to_string());
        let info = crate::cards::parse(&json!({"id": cid, "name": name, "type": "Skill", "cost": "1", "description": plain, "target_type": "AnyEnemy"}));
        let status = if named.contains(cid) {
            "rule"
        } else if info.known {
            if partial(&plain) {
                "partial"
            } else {
                "read"
            }
        } else {
            "unread"
        };
        let used = decks.get(&name);
        rows.push((cid.to_string(), name, status, used, plain));
    }
    Ok(rows)
}

fn words(v: &Value) -> String {
    upto(&v["DESCRIPTIONS"].list().iter().map(base::str_of).collect::<Vec<_>>().join(" "), 160).to_string()
}

pub fn powers(game: &str, named: &HashSet<String>) -> Result<Vec<Row>, String> {
    let seen = power_mentions(game)?;
    let data = loc(game, "powers")?;
    let mut rows = Vec::new();
    if game == "sts2" {
        for pid in ids(&data, |_| true) {
            let short = pid.strip_suffix("_POWER").unwrap_or(&pid).to_string();
            let mut described = text_of(data.get(&format!("{pid}.description")), "");
            if described.is_empty() {
                described = text_of(data.get(&format!("{pid}.smartDescription")), "");
            }
            let text = re!(r"\[/?\w+\]").replace_all(&described, "").into_owned();
            let status = if named.contains(&short) || named.contains(&pid) { "rule" } else { "unnamed" };
            let used = seen.get(&short);
            rows.push((short.clone(), text_of(data.get(&format!("{pid}.title")), &short), status, used, text));
        }
    } else {
        let mut sorted: Vec<(&String, &Value)> = data.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        for (pid, v) in sorted {
            let key = re!(r"[^A-Z0-9]+").replace_all(&pid.to_uppercase(), "_").trim_matches('_').to_string();
            let squeezed = key.replace('_', "");
            let mapped = crate::games::sts1::screens::POWER_ID.iter().find(|(k, _)| *k == squeezed).map_or(key.clone(), |(_, to)| to.to_string());
            let status = if named.contains(&mapped) || named.contains(&key) { "rule" } else { "unnamed" };
            let used = seen.get(&mapped) + seen.get(&key);
            rows.push((mapped, text_of(v.get("NAME"), pid), status, used, words(v)));
        }
    }
    Ok(rows)
}

pub fn relics(game: &str, named: &HashSet<String>) -> Result<Vec<Row>, String> {
    use crate::run::{events::_ANCIENT, relicworth};
    let (_, held) = deck_counts(game)?;
    // (the original asked relicworth for a `_WORTH` it does not have: the Ancients' ratings and the deck-dependent ones are the rated)
    let rated: HashSet<&str> = _ANCIENT.keys().copied().chain(relicworth::_DECK.keys().copied()).collect();
    let mut rows = Vec::new();
    if game == "sts2" {
        let data = loc("sts2", "relics")?;
        for rid in ids(&data, |k| k.ends_with(".title")) {
            let status = if named.contains(&rid) {
                "rule"
            } else if rated.contains(rid.as_str()) {
                "rated"
            } else {
                "text"
            };
            let text = re!(r"\[/?\w+\]").replace_all(&text_of(data.get(&format!("{rid}.description")), ""), "").into_owned();
            let used = held.get(&rid);
            rows.push((rid.clone(), text_of(data.get(&format!("{rid}.title")), &rid), status, used, text));
        }
    } else {
        let data = loc("sts1", "relics")?;
        let mut sorted: Vec<(&String, &Value)> = data.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        for (rid, v) in sorted {
            let key = re!(r"[^A-Z0-9]+").replace_all(&rid.to_uppercase(), "_").trim_matches('_').to_string();
            let status = if named.contains(&key) {
                "rule"
            } else if rated.contains(key.as_str()) {
                "rated"
            } else {
                "text"
            };
            let used = held.get(&key) + held.get(rid);
            rows.push((key, text_of(v.get("NAME"), rid), status, used, words(v)));
        }
    }
    Ok(rows)
}

pub fn monsters(game: &str, named: &HashSet<String>) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    if game == "sts2" {
        let data = loc("sts2", "monsters")?;
        for mid in ids(&data, |k| k.ends_with(".name")) {
            let prefix = format!("{mid}.moves.");
            let moves: BTreeSet<String> = data.iter().filter(|(k, _)| k.starts_with(&prefix)).map(|(_, v)| base::str_of(v)).collect();
            let text = upto(&moves.into_iter().collect::<Vec<_>>().join(", "), 160).to_string();
            let status = if named.contains(&mid) { "rule" } else { "intents" };
            rows.push((mid.clone(), base::str_of(&data[&format!("{mid}.name")]), status, 0, text));
        }
    } else {
        let data = loc("sts1", "monsters")?;
        let mut sorted: Vec<(&String, &Value)> = data.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        for (mid, v) in sorted {
            let key = re!(r"[^A-Z0-9]+").replace_all(&mid.to_uppercase(), "").into_owned();
            let status = if named.contains(&key) { "rule" } else { "intents" };
            rows.push((key, text_of(v.get("NAME"), mid), status, 0, String::new()));
        }
    }
    Ok(rows)
}

pub fn report(title: &str, rows: &[Row], n: usize, todo: &[&str]) {
    let mut c = Counter::default();
    for r in rows {
        c.add(r.2);
    }
    out!("== {}: {}  {}", title, rows.len(), c.most_common(None).iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join("  "));
    let mut listed: Vec<&Row> = rows.iter().filter(|r| todo.contains(&r.2)).collect();
    listed.sort_by_key(|r| -r.3);
    for (_cid, name, status, used, text) in listed.into_iter().take(n) {
        out!("   {:8} {:5}  {:26} {}", status, used, upto(name, 26), upto(text, 150));
    }
}

fn script(args: &[String]) -> Result<(), String> {
    let game = args.first().map_or("sts2", String::as_str);
    let what = args.get(1).map_or("all", String::as_str);
    let n = match args.get(2) {
        Some(text) => super::modelfight::int(text)?,
        None => 25,
    };
    // (`rows[:n]` with a negative n leaves that many off the end: no audit asks for that)
    let n = n.max(0) as usize;
    crate::games::select(game)?;
    let named = named_ids()?;
    if matches!(what, "cards" | "all") {
        report(&format!("{game} cards"), &if game == "sts2" { sts2_cards(&named)? } else { sts1_cards(&named)? }, n, &["unread", "partial"]);
    }
    if matches!(what, "powers" | "all") {
        report(&format!("{game} powers"), &powers(game, &named)?, n, &["unnamed"]);
    }
    if matches!(what, "relics" | "all") {
        report(&format!("{game} relics"), &relics(game, &named)?, n, &["text"]);
    }
    if matches!(what, "monsters" | "all") {
        report(&format!("{game} monsters"), &monsters(game, &named)?, 0, &[]);
    }
    Ok(())
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(script(&args))
}

#[cfg(test)]
mod tests {
    use super::super::fightlog::tests::state;
    use super::super::modelfight::golden;
    use super::*;

    macro_rules! as_recorded {
        ($($test:ident: $file:literal, $args:expr;)*) => {$(
            #[test]
            fn $test() {
                let _state = state();
                let args: &[&str] = &$args;
                golden(main, args, include_str!(concat!("../../tests/golden/logs/audit_coverage", $file, ".txt")));
            }
        )*};
    }

    as_recorded! {
        sts2_as_recorded: "", [];
        sts2_cards_as_recorded: "_sts2_cards", ["sts2", "cards", "400"];
        sts2_powers_as_recorded: "_sts2_powers", ["sts2", "powers", "5"];
        sts2_all_as_recorded: "_sts2_all", ["sts2", "all", "300"];
        sts1_as_recorded: "_sts1", ["sts1"];
        sts1_all_as_recorded: "_sts1_all", ["sts1", "all", "300"];
    }

    #[test]
    fn the_string_constants_of_a_source() {
        let source = r##"
//! A module that names WHOLE_MODULE.
// a comment that names NOT_THIS
/// Says "DOC_WORD" of it.
pub static _TIER: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([("RATED_KEY", 1.0), ("OTHER_KEY", f("IN_A_ROW"))]));
pub static WORTH: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([("WORTH_KEY", 1.0)]));
fn f<'a>(x: &'a str) -> bool {
    let c = '"'; let d = '\''; let e = b'x';
    x == "PLAIN" || x == r"RAW_ONE" || x == r#"RAW "TWO""# || x == b"BYTES" /* "COMMENTED" */ || x == "two\nLINES and \"QUOTED\""
}
#[cfg(test)]
pub(crate) mod testing {
    pub fn g() -> &'static str { "TEST_ONLY" }
}
#[cfg(test)]
use x::{y, z};
const AFTER: &str = "AFTER_TESTS";
#[cfg(test)]
mod tests {
    const T: &str = "IN_TESTS";
}
"##;
        let found = literals(source);
        let all = found.join("|");
        for word in ["WHOLE_MODULE", "DOC_WORD", "IN_A_ROW", "WORTH_KEY", "PLAIN", "RAW_ONE", "RAW \"TWO\"", "BYTES", "two\nLINES and \"QUOTED\"", "AFTER_TESTS"] {
            assert!(all.contains(word), "{word}: {found:?}");
        }
        for word in ["NOT_THIS", "RATED_KEY", "OTHER_KEY", "COMMENTED", "TEST_ONLY", "IN_TESTS"] {
            assert!(!all.contains(word), "{word}: {found:?}");
        }
    }
}
