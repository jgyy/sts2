//! Tables from model-run files: how each character did, what each setting did to it, which cards went with deep runs,
//! and where runs ended. A run's score is the floor it ended on, plus 15 for a win (as learn.WIN_BONUS).

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use serde_json::Value;

use crate::py::{self, JsonExt};

pub const WIN_FLOORS: i64 = 15;

/// A setting's value as a dict key: a number (1, 1.0 and True are one key, as in Python).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Num(pub f64);

impl Num {
    pub fn of(v: &Value) -> Num {
        Num(v.float() + 0.0) // (+ 0.0: minus zero is the key zero)
    }
}

impl Eq for Num {}

impl std::hash::Hash for Num {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state)
    }
}

/// The runs of one (game, character, ascension).
pub type Key = (String, String, i64);
pub type Groups<'a> = IndexMap<Key, Vec<&'a Value>>;
/// value -> (score over the setting's average, its standard error, runs)
pub type Values = IndexMap<Num, (f64, f64, i64)>;
/// setting -> value -> ...
pub type Effects = IndexMap<String, Values>;
/// (score over the usual, card, picks, standard error)
pub type CardRow = (f64, String, i64, f64);

pub fn load(files: &[String]) -> Vec<Value> {
    let mut out = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
        for line in text.lines() {
            if !line.trim().is_empty() {
                out.push(serde_json::from_str(line).unwrap_or_else(|e| panic!("{path}: not JSON ({e}): {line}")));
            }
        }
    }
    out
}

pub fn score(r: &Value) -> i64 {
    r["floor"].int() + if r["won"].truthy() { WIN_FLOORS } else { 0 }
}

/// (The numbers are all ints or all floats in Python, never a mix: an int is passed as the float it equals.)
pub fn mean_se(xs: &[f64]) -> (f64, f64) {
    let n = xs.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let m = py::fsum(xs.iter().copied()) / n as f64;
    (m, (py::fsum(xs.iter().map(|x| (x - m).powf(2.0))) / 1.max(n as i64 - 1) as f64 / n as f64).sqrt())
}

pub fn pct(k: i64, n: i64) -> String {
    if n != 0 {
        format!("{:.1}%", 100.0 * k as f64 / n as f64)
    } else {
        "-".to_string()
    }
}

fn is_num(v: &Value, n: i64) -> bool {
    v.as_f64() == Some(n as f64)
}

fn strings(v: &Value) -> Vec<String> {
    v.list().iter().map(py::str_of).collect()
}

pub fn boss_record<R: Borrow<Value>>(runs: &[R]) -> Vec<String> {
    let mut cells = Vec::new();
    for act in [1, 2, 3] {
        let met: Vec<&Value> = runs.iter().flat_map(|r| r.borrow()["fights"].list()).filter(|f| f["kind"] == "boss" && is_num(&f["act"], act)).collect();
        cells.push(format!("{} of {}", pct(met.iter().map(|f| f["won"].int()).sum(), met.len() as i64), met.len()));
    }
    cells
}

fn sorted_keys<'k>(groups: &'k Groups) -> Vec<&'k Key> {
    let mut keys: Vec<&Key> = groups.keys().collect();
    keys.sort();
    keys
}

/// Each boss's win rate per character: how often a fight with it was won.
pub fn boss_table(groups: &Groups, game: &str) -> Vec<String> {
    let chars: Vec<&str> = sorted_keys(groups).into_iter().filter(|k| k.0 == game).map(|k| k.1.as_str()).collect();
    let mut rec: BTreeMap<(i64, String), IndexMap<String, [i64; 2]>> = BTreeMap::new();
    for ((g, ch, _a), rs) in groups {
        if g != game {
            continue;
        }
        for f in rs.iter().flat_map(|r| r["fights"].list()).filter(|f| f["kind"] == "boss") {
            let foes: BTreeSet<String> = strings(&f["foes"]).into_iter().collect();
            let row = rec.entry((f["act"].int(), foes.into_iter().collect::<Vec<_>>().join("+"))).or_default();
            for key in [ch.as_str(), "All"] {
                let cell = row.entry(key.to_string()).or_insert([0, 0]);
                cell[0] += f["won"].int();
                cell[1] += 1;
            }
        }
    }
    let mut lines = vec![format!("| Act | Boss | {} | All |", chars.join(" | ")), format!("|---|---|{}", "---|".repeat(chars.len() + 1))];
    for ((act, boss), cells) in &rec {
        let cell = |c: &str| cells.get(c).copied().unwrap_or([0, 0]);
        let row: Vec<String> = chars.iter().map(|c| if cell(c)[1] != 0 { format!("{:.0}%", (100 * cell(c)[0]) as f64 / cell(c)[1] as f64) } else { String::new() }).collect();
        let all = cell("All");
        lines.push(format!("| {} | {} | {} | {} of {} |", act, boss, row.join(" | "), pct(all[0], all[1]), all[1]));
    }
    lines
}

/// setting -> value -> (score over the setting's average in these runs, its standard error, runs).
pub fn effects<R: Borrow<Value>>(runs: &[R]) -> Effects {
    let mut out: Effects = IndexMap::new();
    let names: BTreeSet<&String> = runs.iter().flat_map(|r| r.borrow()["knobs"].entries().map(|(k, _)| k)).collect();
    for name in names {
        let mut by: IndexMap<Num, Vec<f64>> = IndexMap::new();
        for r in runs {
            let r = r.borrow();
            if r["knobs"].has(name) {
                by.entry(Num::of(&r["knobs"][name.as_str()])).or_default().push(score(r) as f64);
            }
        }
        if by.len() < 2 {
            continue;
        }
        let base = mean_se(&by.values().flatten().copied().collect::<Vec<f64>>()).0;
        let mut rows: Vec<(&Num, &Vec<f64>)> = by.iter().collect();
        py::sort_by_key(&mut rows, |(v, _)| **v);
        out.insert(name.clone(), rows.into_iter().map(|(v, s)| (*v, (mean_se(s).0 - base, mean_se(s).1, s.len() as i64))).collect());
    }
    out
}

/// Each character's effects, pooled over characters weighted by runs (every character's own average taken out).
pub fn pooled(groups: &Groups) -> Effects {
    let mut acc: IndexMap<String, IndexMap<Num, (f64, f64, i64)>> = IndexMap::new();
    for runs in groups.values() {
        for (name, values) in effects(runs) {
            for (v, (d, se, n)) in values {
                let a = acc.entry(name.clone()).or_default().entry(v).or_insert((0.0, 0.0, 0));
                a.0 += d * n as f64;
                a.1 += (se * n as f64).powf(2.0);
                a.2 += n;
            }
        }
    }
    acc.into_iter().map(|(name, vals)| (name, vals.into_iter().map(|(v, a)| (v, (a.0 / a.2 as f64, a.1.sqrt() / a.2 as f64, a.2))).collect())).collect()
}

/// (best value, its margin over the runner-up, that margin's standard error).
pub fn best(values: &Values) -> (Num, f64, f64) {
    let mut rows: Vec<(&Num, &(f64, f64, i64))> = values.iter().collect();
    py::sort_by_key(&mut rows, |(_, x)| -x.0);
    let ((v1, (d1, s1, _)), (_v2, (d2, s2, _))) = (rows[0], rows[1]);
    (*v1, d1 - d2, (s1 * s1 + s2 * s2).sqrt())
}

pub fn mark(d: f64, se: f64) -> &'static str {
    if se != 0.0 && d.abs() > 2.58 * se {
        "**"
    } else if se != 0.0 && d.abs() > 1.96 * se {
        "*"
    } else {
        ""
    }
}

pub fn settings_table(groups: &Groups, game: &str) -> Vec<String> {
    let chars: Vec<&str> = sorted_keys(groups).into_iter().filter(|k| k.0 == game).map(|k| k.1.as_str()).collect();
    let mut per: IndexMap<&str, Effects> = IndexMap::new();
    for ((g, ch, _a), rs) in groups {
        if g == game {
            per.insert(ch.as_str(), effects(rs));
        }
    }
    let of_game: Groups = groups.iter().filter(|(k, _)| k.0 == game).map(|(k, v)| (k.clone(), v.clone())).collect();
    let pool = pooled(&of_game);
    let mut lines = vec![format!("| Setting | Value | {} | All |", chars.join(" | ")), format!("|---|---|{}", "---|".repeat(chars.len() + 1))];
    for (name, values) in &pool {
        for (v, (d, se, _n)) in values {
            let mut cells: Vec<String> = Vec::new();
            for ch in &chars {
                let cd = per.get(ch).and_then(|e| e.get(name)).and_then(|vals| vals.get(v));
                cells.push(match cd {
                    Some(cd) => format!("{:+.1}{}", cd.0, mark(cd.0, cd.1)),
                    None => String::new(),
                });
            }
            lines.push(format!("| {} | {} | {} | {:+.1}{} |", name, py::float_g(v.0), cells.join(" | "), d, mark(*d, *se)));
        }
    }
    lines
}

pub fn recommendations(groups: &Groups, game: &str) -> Vec<String> {
    let mut out = Vec::new();
    for key in sorted_keys(groups) {
        let (g, ch, _a) = key;
        if g != game {
            continue;
        }
        let mut picks: Vec<String> = Vec::new();
        for (name, values) in effects(&groups[key]) {
            let (v, margin, se) = best(&values);
            if margin > 1.96 * se {
                picks.push(format!("{} {} (+{:.1} ± {:.1})", name, py::float_g(v.0), margin, se));
            }
        }
        out.push(format!("- **{}**: {}", ch, if picks.is_empty() { "no setting separates from the rest at 95%".to_string() } else { picks.join(", ") }));
    }
    out
}

/// Cards taken from rewards, by how much further the runs that took them got than the character's runs that left the
/// same floor alive (learn.CardStats' measure: a card taken late is not credited with the floors before it).
/// (Python's defaults: top 10, min_picks 30.)
pub fn card_table<R: Borrow<Value>>(runs: &[R], top: usize, min_picks: usize) -> (Vec<CardRow>, Vec<CardRow>) {
    let mut usual: IndexMap<i64, f64> = IndexMap::new();
    let mut gained: IndexMap<String, Vec<f64>> = IndexMap::new();
    for r in runs {
        let r = r.borrow();
        for pick in r["picks"].list() {
            let (floor, key) = (pick[0].int(), py::str_of(&pick[1]));
            if !usual.contains_key(&floor) {
                let alive: Vec<i64> = runs.iter().map(|x| x.borrow()).filter(|x| x["floor"].int() > floor || x["won"].truthy()).map(score).collect();
                usual.insert(floor, if alive.is_empty() { 0.0 } else { alive.iter().sum::<i64>() as f64 / alive.len() as f64 });
            }
            gained.entry(key).or_default().push(score(r) as f64 - usual[&floor]);
        }
    }
    let mut rows: Vec<CardRow> = gained.iter().filter(|(_, g)| g.len() >= min_picks).map(|(k, g)| (mean_se(g).0, k.clone(), g.len() as i64, mean_se(g).1)).collect();
    py::sort_by_key_rev(&mut rows, |row| row.clone());
    let good = rows[..top.min(rows.len())].to_vec();
    let mut bad = rows[if top == 0 { 0 } else { rows.len().saturating_sub(top) }..].to_vec();
    bad.reverse();
    (good, bad)
}

/// (Python's default: top 6.)
pub fn deaths<R: Borrow<Value>>(runs: &[R], top: usize) -> Vec<((i64, String, String), i64)> {
    let mut at: IndexMap<(i64, String, String), i64> = IndexMap::new();
    for r in runs {
        let r = r.borrow();
        if !r["won"].truthy() && r["fights"].truthy() {
            let last = r["fights"].list().last().expect("a fight");
            *at.entry((last["act"].int(), py::str_of(&last["kind"]), strings(&last["foes"]).join("+"))).or_insert(0) += 1;
        }
    }
    let mut rows: Vec<((i64, String, String), i64)> = at.into_iter().collect();
    py::sort_by_key_rev(&mut rows, |row| row.1);
    rows.truncate(top);
    rows
}

/// Won runs next to the runs that got as far and lost to the last boss (runs that end early do less of everything).
pub fn habits<R: Borrow<Value>>(runs: &[R]) -> String {
    let last_act = || py::max_of(runs.iter().map(|x| x.borrow()["act"].float())).unwrap_or(0.0);
    let won: Vec<&Value> = runs.iter().map(|r| r.borrow()).filter(|r| r["won"].truthy()).collect();
    let lost: Vec<&Value> = runs
        .iter()
        .map(|r| r.borrow())
        .filter(|r| !r["won"].truthy() && r["fights"].truthy() && r["fights"].list().last().is_some_and(|f| f["kind"] == "boss") && r["act"].float() == last_act())
        .collect();
    let avg = |rs: &[&Value], k: &str| -> f64 {
        let mut sum = py::Sum::new(0.0);
        for r in rs {
            match &r[k] {
                Value::Array(v) => sum.add_i(v.len() as i64),
                v if v.is_f64() => sum.add_f(v.float()),
                v => sum.add_i(v.int()),
            }
        }
        sum.get() / 1.max(rs.len()) as f64
    };
    ["elites", "rests", "smiths", "removals", "skips", "potions", "deck"].iter().map(|k| format!("{} {:.1} / {:.1}", k, avg(&won, k), avg(&lost, k))).collect::<Vec<_>>().join(", ")
}

/// (Python's default: min_runs 30.)
pub fn report(files: &[String], _min_runs: usize) -> String {
    let all = load(files);
    let errors = all.iter().filter(|r| r.has("error")).count();
    let runs: Vec<&Value> = all.iter().filter(|r| !r.has("error")).collect();
    let mut groups: Groups = IndexMap::new();
    for r in &runs {
        groups.entry((py::str_of(&r["game"]), py::str_of(&r["character"]), if r.has("ascension") { r["ascension"].int() } else { 0 })).or_default().push(r);
    }
    let mut out: Vec<String> = vec!["# Model runs".to_string(), String::new()];
    if errors != 0 {
        out.push(format!("{errors} runs ended in a model error and are left out."));
        out.push(String::new());
    }
    out.push("| Game | Character | Asc | Runs | Wins | Mean floor | Act 1 boss | Act 2 boss | Act 3 boss |".to_string());
    out.push("|---|---|---|---|---|---|---|---|---|".to_string());
    for key in sorted_keys(&groups) {
        let (game, ch, asc) = key;
        let rs = &groups[key];
        let (m, se) = mean_se(&rs.iter().map(|r| r["floor"].float()).collect::<Vec<f64>>());
        out.push(format!("| {} | {} | {} | {} | {} | {:.1} ± {:.1} | {} |", game, ch, asc, rs.len(), pct(rs.iter().map(|r| r["won"].int()).sum(), rs.len() as i64), m, se, boss_record(rs).join(" | ")));
    }
    let games: BTreeSet<&String> = groups.keys().map(|k| &k.0).collect();
    for game in games {
        out.extend([String::new(), format!("## {game}: settings"), String::new(), "Score over the setting's average, per character and pooled (* 95%, ** 99%):".to_string(), String::new()]);
        out.extend(settings_table(&groups, game));
        out.extend([String::new(), "Best value where it beats the runner-up at 95%:".to_string(), String::new()]);
        out.extend(recommendations(&groups, game));
        out.extend([String::new(), format!("## {game}: bosses"), String::new(), "Share of boss fights won:".to_string(), String::new()]);
        out.extend(boss_table(&groups, game));
    }
    for key in sorted_keys(&groups) {
        let (game, ch, asc) = key;
        let rs = &groups[key];
        out.extend([String::new(), format!("## {} {} (Ascension {}, {} runs)", game, ch, asc, rs.len()), String::new(), format!("Won runs / runs lost to the last boss, per run: {}", habits(rs))]);
        let (good, bad) = card_table(rs, 10, 30);
        if !good.is_empty() {
            let cards = |rows: &[CardRow]| rows.iter().map(|(d, k, n, se)| format!("{} {:+.1}{} ({})", k, d, mark(*d, *se), n)).collect::<Vec<_>>().join(", ");
            out.extend([
                String::new(),
                "Cards that went with the deepest runs (score over runs that left the same floor alive, * 95%; picks):".to_string(),
                cards(&good),
                String::new(),
                "... and the shallowest:".to_string(),
                cards(&bad),
            ]);
        }
        out.extend([String::new(), format!("Where runs ended: {}", deaths(rs, 6).iter().map(|((a, kind, foes), n)| format!("act {a} {kind} {foes} {n}")).collect::<Vec<_>>().join(", "))]);
    }
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const RUNS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/report_runs.jsonl");

    fn golden() -> Value {
        serde_json::from_str(include_str!("../../tests/golden/report.json")).unwrap()
    }

    fn rows(rows: &[CardRow]) -> Value {
        Value::Array(rows.iter().map(|(d, k, n, se)| json!([d, k, n, se])).collect())
    }

    fn effects_json(e: &Effects) -> Value {
        Value::Array(e.iter().map(|(name, vals)| json!([name, vals.iter().map(|(v, (d, se, n))| json!([v.0, d, se, n])).collect::<Vec<_>>()])).collect())
    }

    #[test]
    fn the_report_is_pythons() {
        let g = golden();
        assert_eq!(report(&[RUNS.to_string()], 30), g["report"].s());
        // (two files: the same runs twice over)
        assert_eq!(report(&[RUNS.to_string(), RUNS.to_string()], 30), g["report_twice"].s());
    }

    #[test]
    fn every_table_is_pythons() {
        let g = golden();
        let all = load(&[RUNS.to_string()]);
        let runs: Vec<&Value> = all.iter().filter(|r| !r.has("error")).collect();
        assert_eq!(all.len() as i64, g["loaded"].int());
        assert_eq!(Value::Array(runs.iter().map(|r| json!(score(r))).collect()), g["scores"]);
        let mut groups: Groups = IndexMap::new();
        for r in &runs {
            groups.entry((r["game"].s().to_string(), r["character"].s().to_string(), r["ascension"].int())).or_default().push(r);
        }
        assert_eq!(groups.len(), g["groups"].list().len());
        for (key, want) in groups.keys().zip(g["groups"].list()) {
            let rs = &groups[key];
            assert_eq!(json!([key.0, key.1, key.2]), want["key"]);
            for (top, min_picks, table) in want["card_tables"].list().iter().map(|t| (t[0].int() as usize, t[1].int() as usize, &t[2])) {
                let (good, bad) = card_table(rs, top, min_picks);
                assert_eq!(json!([rows(&good), rows(&bad)]), *table, "{key:?} {top} {min_picks}");
            }
            assert_eq!(effects_json(&effects(rs)), want["effects"], "{key:?}");
            assert_eq!(json!(boss_record(rs)), want["boss_record"]);
            assert_eq!(json!(habits(rs)), want["habits"]);
            assert_eq!(Value::Array(deaths(rs, 6).iter().map(|((a, k, f), n)| json!([[a, k, f], n])).collect()), want["deaths"]);
            assert_eq!(Value::Array(deaths(rs, 2).iter().map(|((a, k, f), n)| json!([[a, k, f], n])).collect()), want["deaths2"]);
            let (m, se) = mean_se(&rs.iter().map(|r| score(r) as f64).collect::<Vec<f64>>());
            assert_eq!(json!([m, se]), want["mean_se"]);
            assert_eq!(Value::Array(effects(rs).values().map(|v| { let (b, m, s) = best(v); json!([b.0, m, s]) }).collect()), want["best"]);
        }
        assert_eq!(effects_json(&pooled(&groups)), g["pooled"]);
        for game in ["sts1", "sts2", "none"] {
            assert_eq!(json!(settings_table(&groups, game)), g["settings_table"][game]);
            assert_eq!(json!(recommendations(&groups, game)), g["recommendations"][game]);
            assert_eq!(json!(boss_table(&groups, game)), g["boss_table"][game]);
        }
        let all_runs = card_table(&all[..0], 10, 30);
        assert_eq!(all_runs, (vec![], vec![]));
    }

    #[test]
    fn small_things_as_python() {
        let g = golden();
        for case in g["mean_se"].list() {
            let xs: Vec<f64> = case[0].list().iter().map(|x| x.float()).collect();
            let (m, se) = mean_se(&xs);
            assert_eq!(json!([m, se]), case[1], "{case}");
        }
        for case in g["pct"].list() {
            assert_eq!(pct(case[0].int(), case[1].int()), case[2].s(), "{case}");
        }
        for case in g["mark"].list() {
            assert_eq!(mark(case[0].float(), case[1].float()), case[2].s(), "{case}");
        }
    }
}
