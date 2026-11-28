//! Slay the Spire 2's monsters, built from sts2_data.json: each one's moves and the state machine that picks them.
//!
//! The data (Spire Codex's numbers, spire-mentor's state machines) names powers the engine has to act out; the
//! ones that change a fight are handled in `HOOKS` and `effects`, the rest are shown to the planner and do nothing.
//!
//! (Python's patterns reach the fight through `foe.mem["fight"]`; here every rule is given the fight and the
//! monster's index, and `mem["fight"]` is a flag: see sim/monsters.rs.)

#![allow(non_snake_case)]

use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::py::{self, JsonExt};
use crate::sim::effects::Powers;
use crate::sim::engine::{Fight, Foe, Fx, Move, Spec};
use crate::sim::monsters::{self, Hook, Pattern};

pub fn PLAYER_DEBUFF(name: &str) -> Option<&'static str> {
    Some(match name {
        "WEAK" => "weak",
        "FRAIL" => "frail",
        "VULNERABLE" => "vuln",
        _ => return None,
    })
}

pub fn INTENT_KIND(intent: &str) -> Option<&'static str> {
    Some(match intent {
        "stun" => "Stun",
        "sleep" => "Sleep",
        "escape" => "Escape",
        _ => return None,
    })
}

/// `d.get(key, default)`: the default only when the key is missing (a null that is there stays).
fn get<'a>(d: &'a Value, key: &str, default: &'a Value) -> &'a Value {
    match d.as_object().and_then(|m| m.get(key)) {
        Some(v) => v,
        None => default,
    }
}

/// `str(x)` of a value of the data.
fn text_of(v: &Value) -> String {
    py::str_of(v)
}

pub fn move_of(mv: &Value, asc: i64) -> Move {
    if mv["id"].as_str() == Some("EXPLODE") {
        return Move::new(1, 1, 0, vec![], "");
    }
    let one = json!(1);
    let dmg = if asc >= 9 {
        get(mv, "damage_a9", &mv["damage"])
    } else if asc >= 8 {
        get(mv, "damage_a8", &mv["damage"])
    } else {
        &mv["damage"]
    };
    let hits = if asc >= 9 { get(mv, "hits_a9", get(mv, "hits", &one)) } else { get(mv, "hits", &one) };
    let block = if asc >= 9 {
        get(mv, "block_a9", get(mv, "block_a8", &mv["block"]))
    } else if asc >= 8 {
        get(mv, "block_a8", &mv["block"])
    } else {
        &mv["block"]
    };
    let mut fx: Vec<Fx> = Vec::new();
    for p in mv["powers"].list() {
        let name = text_of(&p["power"]).to_uppercase();
        let n = if asc >= 9 { get(p, "amount_a9", get(p, "amount", &one)) } else { get(p, "amount", &one) };
        let n = n.as_i64().unwrap_or_else(|| panic!("TypeError: a power's amount is {n}"));
        let target = if p["target"].truthy() { text_of(&p["target"]) } else { "self".to_string() };
        if target.starts_with("player") {
            fx.push(match PLAYER_DEBUFF(&name) {
                Some(word) => Fx::n(word, n),
                None => Fx::named("player_power", &name, n),
            });
        } else if target.starts_with("all") || target.starts_with("Torch") {
            fx.push(if name == "STRENGTH" { Fx::n("str_all", n) } else { Fx::named("power", &name, n) });
        } else {
            fx.push(if name == "STRENGTH" { Fx::n("str", n) } else { Fx::named("power", &name, n) });
        }
    }
    for s in mv["status_cards"].list() {
        let card = if s["card"].truthy() { text_of(&s["card"]) } else { String::new() };
        if card == "choice" {
            fx.push(Fx::call(_curse_of_knowledge));
            continue;
        }
        let pile_text = text_of(&s["pile"]);
        let pile = if pile_text.starts_with("hand") {
            "hand"
        } else if pile_text.starts_with("draw") {
            "draw"
        } else {
            "discard"
        };
        let count = if asc >= 9 { get(s, "count_a9", get(s, "count", &one)) } else { get(s, "count", &one) };
        fx.push(Fx::status(&card, count.int(), pile));
    }
    if mv["heal"].truthy() {
        let heal = text_of(&mv["heal"]);
        let digits = !heal.is_empty() && heal.chars().all(|c| c.is_ascii_digit());
        fx.push(Fx::n("heal", if digits { mv["heal"].int() } else { 10 }));
    }
    let intents: Vec<String> = mv["intent"].list().iter().map(|i| text_of(i).to_lowercase()).collect();
    if intents.iter().any(|i| i == "summon") {
        fx.push(Fx::call(_summon));
    }
    let mut kind = intents.iter().find_map(|i| INTENT_KIND(i)).unwrap_or("");
    if !dmg.truthy() && !block.truthy() && fx.is_empty() && kind.is_empty() {
        kind = "Unknown";
    }
    Move::new(dmg.int(), if hits.truthy() { hits.int() } else { 1 }, block.int(), fx, kind)
}

/// A monster's state machine as the data has it.
struct Machine {
    m: Value,
    states: IndexMap<String, Value>,
    moves: Vec<String>, // (a set in Python: its first member here is the table's first move)
}

impl Machine {
    /// `next(iter(moves))`.
    fn any_move(&self) -> String {
        self.moves.first().expect("StopIteration: a monster with no moves").clone()
    }

    /// `name in states`, for a name that may be None.
    fn state(&self, name: &Value) -> Option<&Value> {
        name.as_str().and_then(|n| self.states.get(n))
    }

    fn resolve(&self, f: &mut Fight, i: usize, state_id: &Value, depth: i64) -> String {
        let st = match self.state(state_id) {
            Some(st) if depth <= 8 => st,
            _ => {
                return match state_id.as_str() {
                    Some(id) if self.moves.iter().any(|m| m == id) => id.to_string(),
                    _ => self.any_move(),
                }
            }
        };
        if st["type"].s() == "move" {
            f.foes[i].mem.set_str("state", st["id"].s());
            return if st["move"].truthy() { st["move"].s().to_string() } else { st["id"].s().to_string() };
        }
        if st["type"].s() == "conditional" {
            for b in st["branches"].list() {
                if _holds(f, i, &b["if"]) {
                    return if self.state(&b["move"]).is_some() { self.resolve(f, i, &b["move"], depth + 1) } else { _as_move(&mut f.foes[i], &b["move"], &self.states) };
                }
            }
            return self.any_move();
        }
        let mut branches: Vec<&Value> = st["branches"].list().iter().filter(|b| _allowed(&f.foes[i], b)).collect();
        if branches.is_empty() {
            branches = st["branches"].list().iter().collect();
        }
        let weight = |b: &Value| get(b, "weight", &Value::Null).float_or(1.0);
        let total = py::fsum(branches.iter().map(|b| weight(b)));
        let mut r = f.run.rng.random() * total;
        for b in &branches {
            r -= weight(b);
            if r <= 0.0 {
                return match self.state(&b["move"]) {
                    Some(to) if to["type"].s() != "move" => self.resolve(f, i, &b["move"], depth + 1),
                    _ => _as_move(&mut f.foes[i], &b["move"], &self.states),
                };
            }
        }
        _as_move(&mut f.foes[i], &branches.last().expect("IndexError: a state with no branches")["move"], &self.states)
    }

    fn pick(&self, f: &mut Fight, i: usize) -> String {
        if let Some(forced) = f.foes[i].mem.pop_str("force") {
            if !forced.is_empty() {
                return self.resolve(f, i, &json!(forced), 0);
            }
        }
        if !f.foes[i].mem.strs.contains_key("state") {
            let first = _initial(f, i, &self.m, &self.states);
            return self.resolve(f, i, &json!(first), 0);
        }
        let now = f.foes[i].mem.get_str("state").unwrap_or("").to_string();
        let nxt = self.states.get(&now).map(|st| &st["next"]).filter(|n| n.truthy()).cloned().unwrap_or_else(|| json!(now));
        self.resolve(f, i, &nxt, 0)
    }
}

pub fn pattern_of(m: &Value) -> Pattern<Fight> {
    let machine = Machine {
        m: m.clone(),
        states: m["states"].list().iter().map(|s| (s["id"].s().to_string(), s.clone())).collect(),
        moves: m["moves"].list().iter().map(|mv| mv["id"].s().to_string()).collect(),
    };
    Rc::new(move |f, i| machine.pick(f, i))
}

pub fn _as_move(f: &mut Foe, name: &Value, states: &IndexMap<String, Value>) -> String {
    let name = name.s();
    if let Some(st) = states.get(name) {
        if st["type"].s() == "move" {
            f.mem.set_str("state", st["id"].s());
            return if st["move"].truthy() { st["move"].s().to_string() } else { name.to_string() };
        }
    }
    f.mem.set_str("state", name);
    name.to_string()
}

pub fn _allowed(f: &Foe, b: &Value) -> bool {
    let (r#move, rule) = (b["move"].s(), b["repeat"].s());
    let hist = &f.history;
    if rule == "CannotRepeat" && hist.last().is_some_and(|h| h == r#move) {
        return false;
    }
    if rule == "CanRepeatXTimes" {
        let most = if b["max_times"].truthy() { b["max_times"].int() } else { 2 }.max(0) as usize;
        if hist.len() >= most && f.last(most).iter().all(|h| h == r#move) {
            return false;
        }
    }
    if rule == "UseOnlyOnce" && hist.iter().any(|h| h == r#move) {
        return false;
    }
    true
}

pub fn _holds(f: &mut Fight, i: usize, cond: &Value) -> bool {
    let fight = f.foes[i].mem.truthy("fight");
    let cond = match cond {
        Value::Null => return true,
        other => other.s(),
    };
    match cond {
        "always_true" => true,
        "lagavulin_asleep" => f.foes[i].powers.val("ASLEEP") > 0,
        "bowlbug_rock_is_off_balance" => f.foes[i].mem.pop("off_balance", 0) != 0,
        "queen_has_amalgam_died" => fight && f.foes.iter().any(|x| x.key == "TORCH_HEAD_AMALGAM" && x.gone),
        "living_shield_has_allies" | "living_shield_alone" => {
            let allies = if fight { f.foes.iter().enumerate().filter(|(n, x)| x.alive() && *n != i).count() } else { 0 };
            (allies > 0) == (cond == "living_shield_has_allies")
        }
        "fabricator_can_fabricate" | "ovicopter_can_lay" | "two_tailed_rat_can_summon" => (if fight { f.foes.iter().filter(|x| x.alive()).count() } else { 5 }) < 4,
        "frog_knight_should_charge" => {
            let foe = &f.foes[i];
            foe.hp < foe.max_hp / 2 && !foe.mem.truthy("charged")
        }
        "knowledge_demon_curse_counter_lt_three" => f.foes[i].mem.get("curses", 0) < 3,
        "test_subject_respawns_lt_two" => f.foes[i].mem.get("respawns", 0) < 2,
        _ => false,
    }
}

pub fn _initial(f: &mut Fight, i: usize, m: &Value, states: &IndexMap<String, Value>) -> String {
    let init = if m["initial"].truthy() { text_of(&m["initial"]) } else { String::new() };
    if states.contains_key(&init) {
        return init;
    }
    let names: Vec<&str> = crate::re!(r#""([A-Z_]+)""#).captures_iter(&init).map(|c| c.get(1).expect("a name").as_str()).collect();
    if names.is_empty() {
        return m["states"].list().first().map_or(String::new(), |s| s["id"].s().to_string());
    }
    let foe = &f.foes[i];
    let slot = foe.mem.get("slot", 0);
    let last = names[names.len() - 1];
    if init.contains("is_alone") {
        return if foe.mem.truthy("alone") {
            names[0]
        } else if slot == 0 {
            names[1]
        } else {
            last
        }
        .to_string();
    }
    if init.contains("dread") {
        return last.to_string(); // (Dread mode is the beta's: the usual opener is the other one)
    }
    if init.starts_with("if ") {
        return if slot == 0 { names[0] } else { last }.to_string();
    }
    if init.contains("starter_idx") {
        return f.run.rng.choice(&names).to_string();
    }
    // (`names[min(slot, len(names) - 1)]`: a slot below zero would count from the end, and there is none)
    names[slot.min(names.len() as i64 - 1) as usize].to_string()
}

// what the data leaves out: the Test Subject's first form (the planner's notes put it at 100 HP), the Pressure Gun's 20 (23),
// the Aeonglass's Withering Presence (the bot's logs, scripts/bosses_aeonfight.rs)
pub fn FIXES(key: &str) -> Option<Value> {
    Some(match key {
        "TEST_SUBJECT" => json!({"hp": [100, 100], "hp_a8": [110, 110]}),
        "AEONGLASS" => json!({"powers": [{"power": "ARTIFACT", "amount": 3}, {"power": "WITHERING_PRESENCE", "amount": 6}]}),
        _ => return None,
    })
}

pub fn MOVE_FIXES(key: &str, r#move: &str) -> Option<Value> {
    match (key, r#move) {
        ("WATERFALL_GIANT", "PRESSURE_GUN") => Some(json!({"damage": 20, "damage_a9": 23})),
        _ => None,
    }
}

/// `{**d, **fixes}`.
fn merged(d: &Value, fixes: Option<Value>) -> Value {
    let mut out = d.clone();
    if let (Some(out), Some(Value::Object(fixes))) = (out.as_object_mut(), fixes) {
        for (k, v) in fixes {
            out.insert(k, v);
        }
    }
    out
}

pub fn spec_of(key: &str, m: &Value, asc: i64) -> Spec {
    let mut m = merged(m, FIXES(key));
    let mut moves: IndexMap<String, Move> = IndexMap::new();
    for mv in m["moves"].list() {
        let id = mv["id"].s();
        moves.insert(id.to_string(), move_of(&merged(mv, MOVE_FIXES(key, id)), asc));
    }
    if let Some(states) = m["states"].as_array_mut() {
        // (state ids that only name a move, "X_MOVE" for "X")
        for st in states {
            if st["type"].s() == "move" && !st["move"].as_str().is_some_and(|mv| moves.contains_key(mv)) && moves.contains_key(st["id"].s()) {
                st["move"] = st["id"].clone();
            }
        }
    }
    let mut powers: IndexMap<String, i64> = IndexMap::new();
    for p in m["powers"].list() {
        let amount = if asc >= 9 && !p["amount_a9"].is_null() {
            p["amount_a9"].int()
        } else if p["amount"].truthy() {
            p["amount"].int()
        } else {
            1
        };
        powers.insert(text_of(&p["power"]).to_uppercase(), amount);
    }
    let hp = if asc >= 8 { &m["hp_a8"] } else { &m["hp"] };
    let minion = powers.contains_key("MINION");
    Spec { powers, minion, hooks: HOOKS(key), ..Spec::new(key, m["name"].s(), (hp[0].int(), hp[1].int()), moves, pattern_of(&m)) }
}

// ------------------------------------------------------------------ special cases

pub fn _summon(f: &mut Fight, i: usize) {
    let minion = SUMMONS(&f.foes[i].key);
    if let Some(minion) = minion {
        if f.foes.iter().filter(|x| x.alive()).count() < 5 {
            let new = (f.game.spawn)(minion, f, true, None);
            f.foes.push(new);
            monsters::choose(f, f.foes.len() - 1);
        }
    }
}

/// The Knowledge Demon's offer: the bot takes the burn (Disintegration) every time, as its afflictions.rs mostly does.
pub fn _curse_of_knowledge(f: &mut Fight, i: usize) {
    let curses = f.foes[i].mem.add("curses", 1);
    let now = f.pw.val("DISINTEGRATION") + 5 + curses;
    f.pw.put("DISINTEGRATION", now);
}

pub fn _plow_hit(f: &mut Fight, i: usize, _amount: i64, _attack: bool) {
    let foe = &mut f.foes[i];
    if foe.powers.on("PLOW") && foe.hp <= foe.powers["PLOW"] {
        foe.powers.shift_remove("PLOW");
        foe.powers.shift_remove("STRENGTH");
        foe.mem.set_str("force", "STUN");
        foe.move_name = "STUN".to_string();
        foe.mem.set_str("state", "STUN");
    }
}

pub fn _shriek_hit(f: &mut Fight, i: usize, _amount: i64, _attack: bool) {
    let foe = &mut f.foes[i];
    if foe.powers.on("SHRIEK") && foe.hp <= foe.powers["SHRIEK"] {
        foe.powers.shift_remove("SHRIEK");
        foe.stun();
    }
}

/// The Test Subject as the bot's logs have it (scripts/bosses_subjectfight.rs): a form beaten does not attack, and the
/// next (100 HP more) acts the round after with its Strength and debuffs at nothing. Form 2 has Painful Stabs and a
/// Multi-Claw of 3 hits and one more each round; form 3 Nemesis, Intangible on its first round and every other one.
pub fn _adaptable_death(f: &mut Fight, i: usize) -> bool {
    let foe = &mut f.foes[i];
    let n = foe.mem.get("respawns", 0);
    if n >= 2 {
        return false;
    }
    foe.mem.set("respawns", n + 1);
    foe.half_dead = true;
    foe.hp = 0;
    foe.mem.set("revive", 1);
    foe.mem.set("form_hp", foe.max_hp + 100);
    foe.powers = if n == 0 { IndexMap::from([("ADAPTABLE".to_string(), 1), ("PAINFUL_STABS".to_string(), 1)]) } else { IndexMap::from([("NEMESIS".to_string(), 1)]) };
    foe.mem.set_flag("nemesis", false);
    foe.mem.hits.insert("MULTI_CLAW".to_string(), 3);
    foe.mem.set_str("force", ["MULTI_CLAW", "PHASE3_LACERATE"][n as usize]);
    true
}

/// Increasing Intensity: every Wither goes up a level (3 more damage), and the Strength it gains grows by 1 each time.
pub fn _intensity(f: &mut Fight, i: usize) {
    let foe = &mut f.foes[i];
    if foe.move_name != "INCREASING_INTENSITY" {
        return;
    }
    let grown = foe.powers.val("STRENGTH") + foe.mem.get("wither_level", 0);
    foe.powers.put("STRENGTH", grown);
    let level = foe.mem.get("wither_level", 0) + 1;
    foe.mem.set("wither_level", level);
    let wither = f.game.wither.expect("the game's Wither");
    for c in f.hand.iter_mut().chain(&mut f.draw).chain(&mut f.discard) {
        if c.key() == "WITHER" {
            c.d = wither(level);
        }
    }
}

/// The Slumbering Beetle wakes after its Slumber's count of turns, or once it has lost HP three times (`slumber(foe, turn=False)`).
pub fn slumber(foe: &mut Foe, turn: bool) {
    if turn {
        foe.powers["SLUMBER"] -= 1;
    } else {
        foe.mem.add("slumber_hits", 1);
    }
    if foe.powers.val("SLUMBER") <= 0 || foe.mem.get("slumber_hits", 0) >= 3 {
        foe.powers.shift_remove("SLUMBER");
        foe.mem.set_str("force", "WAKE_UP_MOVE");
    }
}

/// The Waterfall Giant: beaten, it stands one more turn out of reach and blows up for its Steam Eruption.
pub fn _giant_death(f: &mut Fight, i: usize) -> bool {
    let foe = &mut f.foes[i];
    if foe.mem.truthy("exploded") {
        return false;
    }
    foe.mem.set_flag("exploded", true);
    foe.max_hp = 999_999_999;
    foe.hp = 999_999_999;
    let blast = foe.powers.get("STEAM_ERUPTION").copied().unwrap_or(15).max(10);
    foe.mem.dmg.insert("EXPLODE".to_string(), blast);
    foe.move_name = "EXPLODE".to_string();
    foe.mem.set_str("state", "EXPLODE");
    true
}

pub fn _stock_death(f: &mut Fight, i: usize) -> bool {
    let foe = &mut f.foes[i];
    if foe.powers.val("STOCK") > 0 {
        // "When killed, a new Axebot replaces it": the old one's Strength (Boot Up), Block and debuffs go with it.
        let stock = foe.powers["STOCK"] - 1;
        let mut powers = foe.spec.powers.clone();
        powers.put("STOCK", stock);
        foe.powers = powers;
        foe.block = 0;
        foe.max_hp += 10;
        foe.hp = foe.max_hp;
        return true;
    }
    false
}

pub fn _illusion_death(f: &mut Fight, i: usize) -> bool {
    let foe = &mut f.foes[i];
    foe.half_dead = true;
    foe.hp = 0;
    foe.mem.set("revive", 1);
    true
}

pub fn _reattach_death(f: &mut Fight, i: usize) -> bool {
    let others = f.foes.iter().enumerate().any(|(n, x)| n != i && x.alive() && x.powers.on("REATTACH"));
    if !others {
        return false;
    }
    let foe = &mut f.foes[i];
    foe.half_dead = true;
    foe.hp = 0;
    foe.mem.set("revive", 2);
    true
}

/// A hook that returns nothing (Python's None).
fn hook(rule: impl Fn(&mut Fight, usize, i64, bool) + 'static) -> Hook<Fight> {
    Rc::new(move |f, i, amount, attack| {
        rule(f, i, amount, attack);
        false
    })
}

/// A "death" hook: the truth of what it returns is whether the monster stays.
fn death(rule: fn(&mut Fight, usize) -> bool) -> Hook<Fight> {
    Rc::new(move |f, i, _amount, _attack| rule(f, i))
}

/// `HOOKS.get(key, {})`.
pub fn HOOKS(key: &str) -> IndexMap<String, Hook<Fight>> {
    let rows: Vec<(&str, Hook<Fight>)> = match key {
        "CEREMONIAL_BEAST" => vec![("hit", hook(_plow_hit))],
        "WATERFALL_GIANT" => vec![
            ("death", death(_giant_death)),
            ("acted", hook(|f, i, _, _| {
                if f.foes[i].mem.truthy("exploded") && f.foes[i].move_name == "EXPLODE" {
                    f.kill(i);
                }
            })),
        ],
        "SPINY_TOAD" => vec![("acted", hook(|f, i, _, _| {
            if f.foes[i].move_name == "SPIKE_EXPLOSION" {
                f.foes[i].powers.shift_remove("THORNS");
            }
        }))],
        // Soar: "Receives 50% less attack damage until it lands" (Spire Codex): Judicial Flight takes off, Verdict comes down.
        "OWL_MAGISTRATE" => vec![("acted", hook(|f, i, _, _| {
            if f.foes[i].move_name == "VERDICT" {
                f.foes[i].powers.shift_remove("SOAR");
            }
        }))],
        "TERROR_EEL" => vec![("hit", hook(_shriek_hit))],
        "TEST_SUBJECT" => vec![
            ("death", death(_adaptable_death)),
            ("acted", hook(|f, i, _, _| {
                let foe = &mut f.foes[i];
                if foe.move_name == "MULTI_CLAW" {
                    *foe.mem.hits.get_mut("MULTI_CLAW").expect("KeyError: ('hits', 'MULTI_CLAW')") += 1;
                }
            })),
        ],
        "AEONGLASS" => vec![("acted", hook(|f, i, _, _| _intensity(f, i)))],
        "SLUMBERING_BEETLE" => vec![("hit", hook(|f, i, _, _| {
            if f.foes[i].powers.on("SLUMBER") {
                slumber(&mut f.foes[i], false);
            }
        }))],
        "AXEBOT" => vec![("death", death(_stock_death))],
        "EYE_WITH_TEETH" | "PARAFRIGHT" => vec![("death", death(_illusion_death))],
        "DECIMILLIPEDE_SEGMENT" | "DECIMILLIPEDE_SEGMENT_FRONT" | "DECIMILLIPEDE_SEGMENT_MIDDLE" | "DECIMILLIPEDE_SEGMENT_BACK" => vec![("death", death(_reattach_death))],
        _ => vec![],
    };
    rows.into_iter().map(|(k, h)| (k.to_string(), h)).collect()
}

/// Who a summoning move brings (the data says "summon" without naming it).
pub fn SUMMONS(key: &str) -> Option<&'static str> {
    Some(match key {
        "FABRICATOR" => "ZAPBOT",
        "OVICOPTER" => "TOUGH_EGG",
        "TWO_TAILED_RAT" => "TWO_TAILED_RAT",
        "PHROG_PARASITE" => "WRIGGLER",
        "QUEEN" => "TORCH_HEAD_AMALGAM",
        "THE_OBSCURA" => "PARAFRIGHT",
        "KIN_PRIEST" => "KIN_FOLLOWER",
        "SOUL_NEXUS" => "WRIGGLER",
        "LIVING_SHIELD" => "LIVING_SHIELD",
        _ => return None,
    })
}

/// The half-dead come back: Illusions at full HP, Decimillipede segments on their Reattach HP, the Test Subject in its next form
/// (the round's end then picks their move with everyone else's).
pub fn revive_round(f: &mut Fight) {
    for x in f.foes.iter_mut() {
        if x.half_dead && !x.gone {
            let left = x.mem.get("revive", 1) - 1;
            x.mem.set("revive", left);
            if left <= 0 {
                x.half_dead = false;
                x.hp = if x.powers.on("REATTACH") { x.powers["REATTACH"] } else { x.max_hp };
                if x.key == "TEST_SUBJECT" {
                    let form = x.mem.pop("form_hp", x.max_hp);
                    x.max_hp = form;
                    x.hp = form;
                }
            }
        }
    }
}

pub fn new_foe(spec: &Rc<Spec>, n: i64, hp: i64, slot: i64, alone: bool) -> Foe {
    let mut foe = Foe::new(spec, n, hp, 1.0);
    foe.mem.set("slot", slot);
    foe.mem.set_flag("alone", alone);
    foe
}
