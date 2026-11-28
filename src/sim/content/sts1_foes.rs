//! The first game's monsters, encounters, elites and bosses (Ascension 0 numbers, written from the game's rules).
//!
//! (the original's patterns reach the fight through `foe.mem["fight"]`; here every rule is given the fight and the
//! monster's index: see sim/monsters.rs.)

#![allow(non_snake_case)]

use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::base::Random;
use crate::sim::effects::{debuff_me, Powers};
use crate::sim::engine::{Fight, Foe, Fx, Move as M, Spec};
use crate::sim::monsters::{self, counter, cycle, opening, weighted, weighted_pick, Hook, Pattern};

pub fn _set(foe: &mut Foe, r#move: &str, dmg: Option<i64>, hits: Option<i64>) {
    if let Some(dmg) = dmg {
        foe.mem.dmg.insert(r#move.to_string(), dmg);
    }
    if let Some(hits) = hits {
        foe.mem.hits.insert(r#move.to_string(), hits);
    }
}

/// `weighted(weights, most)(foe, rng)` for the fight's i-th monster.
fn w(f: &mut Fight, i: usize, weights: &[(&str, f64)], most: &[(&str, i64)]) -> String {
    weighted_pick(&f.foes[i], &mut f.run.rng, weights, most)
}

fn pat(rule: impl Fn(&mut Fight, usize) -> String + 'static) -> Pattern<Fight> {
    Rc::new(rule)
}

/// A hook that returns nothing (the original's None).
fn hook(rule: impl Fn(&mut Fight, usize, i64, bool) + 'static) -> Hook<Fight> {
    Rc::new(move |f, i, amount, attack| {
        rule(f, i, amount, attack);
        false
    })
}

// ------------------------------------------------------------------ hooks

pub fn _louse_start(f: &mut Fight, i: usize) {
    let curl = f.run.rng.randint(3, 7);
    f.foes[i].powers.put("CURL_UP", curl);
    let bite = f.run.rng.randint(5, 7);
    _set(&mut f.foes[i], "bite", Some(bite), None);
}

/// `_split_when_half(into, count)`: `into` is the list of monsters it splits into.
pub fn _split_when_half(into: &[&str]) -> Hook<Fight> {
    let into: Vec<String> = into.iter().map(|k| k.to_string()).collect();
    hook(move |f, i, _amount, _attack| {
        let foe = &mut f.foes[i];
        if foe.alive() && foe.hp <= foe.max_hp / 2 && !foe.mem.truthy("splitting") {
            foe.mem.set_flag("splitting", true);
            foe.mem.lists.insert("split_into".to_string(), into.clone());
            foe.move_name = "split".to_string();
        }
    })
}

pub fn _split(f: &mut Fight, i: usize) {
    let into = f.foes[i].mem.lists.get("split_into").cloned().unwrap_or_default();
    let hp = f.foes[i].hp;
    f.foes[i].gone = true;
    for key in into {
        let new = (f.game.spawn)(&key, f, false, Some(hp));
        f.foes.push(new);
        monsters::choose(f, f.foes.len() - 1);
    }
}

pub fn _give_back_gold(f: &mut Fight, i: usize) {
    f.run.gold += f.foes[i].mem.get("stolen", 0);
}

pub fn _lagavulin_wake(f: &mut Fight, i: usize) {
    let foe = &mut f.foes[i];
    foe.powers.shift_remove("METALLICIZE");
    foe.mem.set_flag("awake", true);
    foe.move_name = "stunned".to_string();
}

pub fn _lagavulin(f: &mut Fight, i: usize) -> String {
    let foe = &mut f.foes[i];
    if !foe.mem.truthy("awake") {
        let slept = foe.mem.add("slept", 1);
        if slept <= 3 {
            foe.powers.put("ASLEEP", 4 - slept);
            return "sleep".to_string();
        }
        foe.mem.set_flag("awake", true);
        foe.powers.shift_remove("ASLEEP");
        foe.powers.shift_remove("METALLICIZE");
    }
    ["attack", "attack", "siphon"][(counter(foe, "i", 0) % 3) as usize].to_string()
}

pub fn _hexa(f: &mut Fight, i: usize) -> String {
    let n = f.foes[i].history.len();
    if n == 0 {
        return "activate".to_string();
    }
    if n == 1 {
        let dmg = f.run.hp / 12 + 1;
        _set(&mut f.foes[i], "divider", Some(dmg), None);
        return "divider".to_string();
    }
    ["sear", "tackle", "sear", "inflame", "tackle", "sear", "inferno"][(n - 2) % 7].to_string()
}

pub fn _hexa_inferno(f: &mut Fight, i: usize) {
    f.foes[i].mem.set_flag("burn_up", true);
    for c in f.draw.iter_mut().chain(&mut f.discard).chain(&mut f.hand) {
        if c.key() == "BURN" {
            c.up = true;
        }
    }
}

pub fn _guardian(foe: &mut Foe) -> String {
    let step = foe.mem.get("defensive", 0);
    if step != 0 {
        // (shifted to its defensive mode: two moves, then its Twin Slam)
        foe.mem.set("defensive", step + 1);
        if step < 3 {
            return ["close_up", "roll"][(step - 1) as usize].to_string();
        }
        foe.mem.set("defensive", 0);
        foe.powers.shift_remove("SHARP_HIDE");
        foe.mem.set("i", 3);
        return "twin_slam".to_string();
    }
    ["charge_up", "fierce_bash", "vent_steam", "whirlwind"][(counter(foe, "i", 0) % 4) as usize].to_string()
}

pub fn _guardian_hit(f: &mut Fight, i: usize, amount: i64, _attack: bool) {
    let foe = &mut f.foes[i];
    if !foe.mem.truthy("defensive") && foe.mem.add("taken", amount) >= foe.mem.get("shift", 30) {
        foe.mem.set("taken", 0);
        let shift = foe.mem.get("shift", 30) + 10;
        foe.mem.set("shift", shift);
        foe.mem.set("defensive", 1);
        foe.block += 20;
        foe.move_name = _guardian(foe);
    }
}

pub fn _chosen(f: &mut Fight, i: usize) -> String {
    let n = f.foes[i].history.len();
    let rng = &mut f.run.rng;
    if n < 2 {
        return ["poke", "hex"][n].to_string();
    }
    if n % 2 == 0 {
        rng.choice(&["debilitate", "drain"]).to_string()
    } else if rng.random() < 0.4 {
        "zap".to_string()
    } else {
        "poke".to_string()
    }
}

pub fn _red_slaver(f: &mut Fight, i: usize) -> String {
    if f.foes[i].history.is_empty() {
        return "stab".to_string();
    }
    if !f.foes[i].mem.truthy("entangled") && f.run.rng.random() < 0.25 {
        f.foes[i].mem.set_flag("entangled", true);
        return "entangle".to_string();
    }
    w(f, i, &[("scrape", 55.0), ("stab", 45.0)], &[("scrape", 2), ("stab", 2)])
}

pub fn _byrd(f: &mut Fight, i: usize) -> String {
    let g = f.foes[i].mem.get("grounded", 0);
    if g != 0 {
        // (knocked down: stunned, then a headbutt, then up again)
        f.foes[i].mem.set("grounded", g - 1);
        return if g == 2 { "headbutt" } else { "fly" }.to_string();
    }
    w(f, i, &[("peck", 50.0), ("swoop", 20.0), ("caw", 30.0)], &[("peck", 2), ("swoop", 1), ("caw", 1)])
}

pub fn _book(f: &mut Fight, i: usize) -> String {
    let m = w(f, i, &[("multi", 85.0), ("single", 15.0)], &[("multi", 2), ("single", 1)]);
    if m == "multi" {
        let foe = &mut f.foes[i];
        let stabs = foe.mem.get("stabs", 1) + 1;
        foe.mem.set("stabs", stabs);
        _set(foe, "multi", None, Some(stabs));
    }
    m
}

pub fn _leader(f: &mut Fight, i: usize) -> String {
    let minions = f.foes.iter().filter(|x| x.alive() && x.minion).count();
    if minions < 2 && f.foes[i].last(1) != ["rally"] {
        return "rally".to_string();
    }
    w(f, i, &[("encourage", 50.0), ("stab", 50.0)], &[("encourage", 1), ("stab", 2)])
}

pub fn _collector(f: &mut Fight, i: usize) -> String {
    let n = f.foes[i].history.len();
    if n == 0 {
        return "spawn".to_string();
    }
    if n == 3 {
        return "mega_debuff".to_string();
    }
    if !f.foes.iter().any(|x| x.alive() && x.minion) && f.run.rng.random() < 0.25 {
        return "spawn".to_string();
    }
    w(f, i, &[("fireball", 70.0), ("buff", 30.0)], &[("fireball", 2), ("buff", 1)])
}

pub fn _champ(f: &mut Fight, i: usize) -> String {
    let foe = &mut f.foes[i];
    if foe.hp < foe.max_hp / 2 && !foe.mem.truthy("angry") {
        foe.mem.set_flag("angry", true);
        return "anger".to_string();
    }
    let n = foe.history.len();
    if foe.mem.truthy("angry") {
        return if foe.last(1) != ["execute"] { "execute".to_string() } else { w(f, i, &[("heavy_slash", 50.0), ("face_slap", 50.0)], &[]) };
    }
    if n % 4 == 3 {
        return "taunt".to_string();
    }
    let mut table = vec![("face_slap", 30.0), ("heavy_slash", 30.0), ("gloat", 20.0)];
    if foe.history.iter().filter(|m| *m == "defensive").count() < 2 {
        table.push(("defensive", 20.0));
    }
    w(f, i, &table, &[("defensive", 1), ("gloat", 1)])
}

pub fn _darkling_death(f: &mut Fight, i: usize) -> bool {
    if !f.foes.iter().enumerate().any(|(n, x)| n != i && x.key == "DARKLING" && !x.gone && !x.half_dead) {
        for x in f.foes.iter_mut() {
            if x.key == "DARKLING" {
                x.gone = true;
            }
        }
        return false;
    }
    let foe = &mut f.foes[i];
    foe.half_dead = true;
    foe.hp = 0;
    foe.mem.set("revive", 2);
    foe.move_name = "regrow".to_string();
    true
}

/// The half-dead rise as the enemies' turn ends: Darklings at half HP two rounds on, the Awakened One reborn at full HP.
pub fn revive_round(f: &mut Fight) {
    for i in 0..f.foes.len() {
        let x = &mut f.foes[i];
        if x.half_dead && x.key == "DARKLING" {
            let left = x.mem.get("revive", 0) - 1;
            x.mem.set("revive", left);
            if left <= 0 {
                x.half_dead = false;
                x.hp = x.max_hp / 2;
                monsters::choose(f, i);
            }
        } else if x.half_dead && x.key == "AWAKENED_ONE" {
            x.half_dead = false;
            x.hp = x.max_hp;
            x.mem.set_flag("risen", true);
        }
    }
}

/// Fading 5: it loses a stack at the start of each of its turns and dies instead of acting on the fifth.
pub fn _transient_end(f: &mut Fight, i: usize) {
    let foe = &mut f.foes[i];
    let fading = foe.powers.get("FADING").copied().unwrap_or(5) - 1;
    foe.powers.put("FADING", fading);
    if fading <= 1 {
        f.kill(i);
    }
}

pub fn _giant_head(f: &mut Fight, i: usize) -> String {
    let n = f.foes[i].history.len() as i64;
    if n < 4 {
        return w(f, i, &[("count", 50.0), ("glare", 50.0)], &[("count", 2), ("glare", 2)]);
    }
    _set(&mut f.foes[i], "it_is_time", Some(60.min(30 + 5 * (n - 4))), None);
    "it_is_time".to_string()
}

pub fn _nemesis(f: &mut Fight, i: usize) -> String {
    let foe = &f.foes[i];
    if foe.last(1) != ["scythe"] && !foe.last(2).iter().any(|m| m == "scythe") && f.run.rng.random() < 0.35 {
        return "scythe".to_string();
    }
    w(f, i, &[("attack", 50.0), ("debuff", 50.0)], &[("attack", 2), ("debuff", 1)])
}

pub fn _awakened_death(f: &mut Fight, i: usize) -> bool {
    if f.foes[i].mem.truthy("reborn") {
        for x in f.foes.iter_mut() {
            // beaten for good: the Cultists flee
            x.gone = true;
        }
        return false;
    }
    let foe = &mut f.foes[i];
    foe.mem.set_flag("reborn", true);
    foe.half_dead = true;
    foe.hp = 0;
    // Its debuffs, Curiosity and Unawakened go; Strength and Regenerate stay. It rises as the enemies' turn ends (revive_round).
    foe.powers.retain(|k, v| k == "REGEN" || k == "STRENGTH" && *v > 0);
    foe.move_name = "rebirth".to_string();
    true
}

pub fn _awakened(f: &mut Fight, i: usize) -> String {
    let foe = &mut f.foes[i];
    if foe.mem.pop("risen", 0) != 0 {
        return "dark_echo".to_string();
    }
    if foe.mem.truthy("reborn") {
        return w(f, i, &[("sludge", 50.0), ("tackle", 50.0)], &[("sludge", 2), ("tackle", 2)]);
    }
    if foe.history.is_empty() {
        return "slash".to_string();
    }
    w(f, i, &[("slash", 75.0), ("soul_strike", 25.0)], &[("slash", 2), ("soul_strike", 1)])
}

pub fn _time_eater(f: &mut Fight, i: usize) -> String {
    let foe = &mut f.foes[i];
    if foe.hp < foe.max_hp / 2 && !foe.mem.truthy("hasted") {
        foe.mem.set_flag("hasted", true);
        return "haste".to_string();
    }
    w(f, i, &[("reverberate", 45.0), ("head_slam", 35.0), ("ripple", 20.0)], &[("reverberate", 2), ("head_slam", 1), ("ripple", 1)])
}

pub fn _haste(f: &mut Fight, i: usize) {
    let foe = &mut f.foes[i];
    foe.hp = foe.hp.max(foe.max_hp / 2);
    for k in ["WEAK", "VULNERABLE", "POISON"] {
        foe.powers.shift_remove(k);
    }
}

pub fn _spire_growth(f: &mut Fight, i: usize) -> String {
    if !f.pw.on("CONSTRICT") && f.foes[i].last(1) != ["constrict"] {
        return "constrict".to_string();
    }
    w(f, i, &[("quick", 50.0), ("smash", 50.0)], &[("quick", 2), ("smash", 2)])
}

pub fn _maw(f: &mut Fight, i: usize) -> String {
    if f.foes[i].history.is_empty() {
        return "roar".to_string();
    }
    let m = w(f, i, &[("slam", 30.0), ("nom", 40.0), ("drool", 30.0)], &[("slam", 1), ("nom", 1), ("drool", 1)]);
    if m == "nom" {
        let foe = &mut f.foes[i];
        let hits = 1 + foe.mem.add("noms", 1);
        _set(foe, "nom", None, Some(hits));
    }
    m
}

// ------------------------------------------------------------------ the table

pub type Table = IndexMap<String, Rc<Spec>>;

thread_local! {
    static SPECS: Rc<Table> = Rc::new(_specs());
    static ENCOUNTER_TABLE: Rc<HashMap<i64, HashMap<&'static str, Vec<Encounter>>>> = Rc::new(_encounters());
}

/// The table of monsters (the original's `S`).
pub fn S() -> Rc<Table> {
    SPECS.with(|s| s.clone())
}

/// What a monster of the table is given beyond its moves and pattern.
#[derive(Default)]
struct Kw {
    powers: Vec<(&'static str, i64)>,
    block: i64,
    minion: bool,
    hooks: Vec<(&'static str, Hook<Fight>)>,
}

/// A monster of the table S. (the original's monsters each keep their fight in mind from the start, then do their own
/// "start" hook, if any: here there is nothing to keep.)
fn spec(s: &mut Table, key: &str, name: &str, hp: (i64, i64), moves: Vec<(&str, M)>, pattern: Pattern<Fight>, kw: Kw) {
    let mut sp = Spec::new(key, name, hp, moves.into_iter().map(|(k, m)| (k.to_string(), m)), pattern);
    sp.powers = kw.powers.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    sp.block = kw.block;
    sp.minion = kw.minion;
    sp.hooks = kw.hooks.into_iter().map(|(k, h)| (k.to_string(), h)).collect();
    s.insert(key.to_string(), Rc::new(sp));
}

/// `M(dmg)`.
fn m(dmg: i64) -> M {
    M { dmg, ..M::default() }
}

/// `M(dmg, hits=hits)`.
fn mh(dmg: i64, hits: i64) -> M {
    M { dmg, hits, ..M::default() }
}

/// `M(dmg, block=block)`.
fn mb(dmg: i64, block: i64) -> M {
    M { dmg, block, ..M::default() }
}

/// `M(dmg, fx=fx)`.
fn mx(dmg: i64, fx: Vec<Fx>) -> M {
    M { dmg, fx, ..M::default() }
}

/// `M(kind=kind)`.
fn mk(kind: &str) -> M {
    M { kind: kind.to_string(), ..M::default() }
}

fn powers(rows: &[(&'static str, i64)]) -> Kw {
    Kw { powers: rows.to_vec(), ..Kw::default() }
}

fn hooks(rows: Vec<(&'static str, Hook<Fight>)>) -> Kw {
    Kw { hooks: rows, ..Kw::default() }
}

fn _slimed(n: i64) -> Fx {
    Fx::status("SLIMED", n, "discard")
}

fn _specs() -> Table {
    let mut s = Table::new();
    let s = &mut s;
    let none = Kw::default;
    let split = || M { kind: "Unknown".to_string(), fx: vec![Fx::call(_split)], ..M::default() };

    // ------------------------------------------------------------------ act 1

    spec(s, "JAW_WORM", "Jaw Worm", (40, 44), vec![("chomp", m(11)), ("thrash", mb(7, 5)), ("bellow", M { block: 6, fx: vec![Fx::n("str", 3)], ..M::default() })],
         opening(&["chomp"], weighted(&[("bellow", 45.0), ("thrash", 30.0), ("chomp", 25.0)], &[("chomp", 1), ("bellow", 1), ("thrash", 2)])), none());
    spec(s, "CULTIST", "Cultist", (48, 54), vec![("incantation", mx(0, vec![Fx::n("ritual", 3)])), ("dark_strike", m(6))], opening(&["incantation"], cycle(&["dark_strike"], 0)), none());
    spec(s, "RED_LOUSE", "Red Louse", (10, 15), vec![("bite", m(6)), ("grow", mx(0, vec![Fx::n("str", 3)]))], weighted(&[("bite", 75.0), ("grow", 25.0)], &[("bite", 2), ("grow", 1)]),
         hooks(vec![("start", hook(|f, i, _, _| _louse_start(f, i)))]));
    spec(s, "GREEN_LOUSE", "Green Louse", (11, 17), vec![("bite", m(6)), ("spit", mx(0, vec![Fx::n("weak", 2)]))], weighted(&[("bite", 75.0), ("spit", 25.0)], &[("bite", 2), ("spit", 1)]),
         hooks(vec![("start", hook(|f, i, _, _| _louse_start(f, i)))]));
    spec(s, "ACID_SLIME_S", "Acid Slime (S)", (8, 12), vec![("tackle", m(3)), ("lick", mx(0, vec![Fx::n("weak", 1)]))], cycle(&["lick", "tackle"], 0), none());
    spec(s, "SPIKE_SLIME_S", "Spike Slime (S)", (10, 14), vec![("tackle", m(5))], cycle(&["tackle"], 0), none());
    spec(s, "ACID_SLIME_M", "Acid Slime (M)", (28, 32), vec![("spit", mx(7, vec![_slimed(1)])), ("tackle", m(10)), ("lick", mx(0, vec![Fx::n("weak", 1)]))],
         weighted(&[("spit", 30.0), ("tackle", 40.0), ("lick", 30.0)], &[("spit", 2), ("tackle", 1), ("lick", 1)]), none());
    spec(s, "SPIKE_SLIME_M", "Spike Slime (M)", (28, 32), vec![("flame_tackle", mx(8, vec![_slimed(1)])), ("lick", mx(0, vec![Fx::n("frail", 1)]))],
         weighted(&[("flame_tackle", 30.0), ("lick", 70.0)], &[("flame_tackle", 2), ("lick", 2)]), none());
    spec(s, "ACID_SLIME_L", "Acid Slime (L)", (65, 69), vec![("spit", mx(11, vec![_slimed(2)])), ("tackle", m(16)), ("lick", mx(0, vec![Fx::n("weak", 2)])), ("split", split())],
         pat(|f, i| if f.foes[i].mem.truthy("splitting") { "split".to_string() } else { w(f, i, &[("spit", 30.0), ("tackle", 40.0), ("lick", 30.0)], &[("spit", 2), ("tackle", 1), ("lick", 1)]) }),
         hooks(vec![("hit", _split_when_half(&["ACID_SLIME_M", "ACID_SLIME_M"]))]));
    spec(s, "SPIKE_SLIME_L", "Spike Slime (L)", (64, 70), vec![("flame_tackle", mx(16, vec![_slimed(2)])), ("lick", mx(0, vec![Fx::n("frail", 2)])), ("split", split())],
         pat(|f, i| if f.foes[i].mem.truthy("splitting") { "split".to_string() } else { w(f, i, &[("flame_tackle", 30.0), ("lick", 70.0)], &[("flame_tackle", 2), ("lick", 2)]) }),
         hooks(vec![("hit", _split_when_half(&["SPIKE_SLIME_M", "SPIKE_SLIME_M"]))]));
    spec(s, "FUNGI_BEAST", "Fungi Beast", (22, 28), vec![("bite", m(6)), ("grow", mx(0, vec![Fx::n("str", 3)]))], weighted(&[("bite", 60.0), ("grow", 40.0)], &[("bite", 2), ("grow", 1)]),
         hooks(vec![("death", hook(|f, _i, _, _| debuff_me(f, "VULNERABLE", 2)))]));
    spec(s, "LOOTER", "Looter", (44, 48), vec![("mug", mx(10, vec![Fx::n("steal_gold", 15)])), ("lunge", mx(12, vec![Fx::n("steal_gold", 15)])), ("smoke", mb(0, 6)), ("escape", mk("Escape"))],
         opening(&["mug", "mug", "lunge", "smoke"], cycle(&["escape"], 0)), hooks(vec![("death", hook(|f, i, _, _| _give_back_gold(f, i)))]));
    spec(s, "MUGGER", "Mugger", (48, 52), vec![("mug", mx(10, vec![Fx::n("steal_gold", 15)])), ("lunge", mx(16, vec![Fx::n("steal_gold", 15)])), ("smoke", mb(0, 11)), ("escape", mk("Escape"))],
         opening(&["mug", "mug", "lunge", "smoke"], cycle(&["escape"], 0)), hooks(vec![("death", hook(|f, i, _, _| _give_back_gold(f, i)))]));
    spec(s, "MAD_GREMLIN", "Mad Gremlin", (20, 24), vec![("scratch", m(4))], cycle(&["scratch"], 0), powers(&[("ANGRY", 1)]));
    spec(s, "SNEAKY_GREMLIN", "Sneaky Gremlin", (10, 14), vec![("puncture", m(9))], cycle(&["puncture"], 0), none());
    spec(s, "FAT_GREMLIN", "Fat Gremlin", (13, 17), vec![("smash", mx(4, vec![Fx::n("weak", 1)]))], cycle(&["smash"], 0), none());
    spec(s, "SHIELD_GREMLIN", "Shield Gremlin", (12, 15), vec![("protect", mx(0, vec![Fx::n("block_ally", 7)])), ("bash", m(6))],
         pat(|f, _i| if f.foes.iter().filter(|x| x.alive()).count() == 1 { "bash" } else { "protect" }.to_string()), none());
    spec(s, "GREMLIN_WIZARD", "Gremlin Wizard", (21, 25), vec![("charge", mk("Unknown")), ("blast", m(25))],
         pat(|f, i| if counter(&mut f.foes[i], "i", 0) % 4 == 3 { "blast" } else { "charge" }.to_string()), none());
    spec(s, "BLUE_SLAVER", "Blue Slaver", (46, 50), vec![("stab", m(12)), ("rake", mx(7, vec![Fx::n("weak", 1)]))], weighted(&[("stab", 60.0), ("rake", 40.0)], &[("stab", 2), ("rake", 2)]), none());
    spec(s, "RED_SLAVER", "Red Slaver", (46, 50), vec![("stab", m(13)), ("scrape", mx(8, vec![Fx::n("vuln", 1)])), ("entangle", mx(0, vec![Fx::of("entangle")]))], pat(_red_slaver), none());
    spec(s, "GREMLIN_NOB", "Gremlin Nob", (82, 86), vec![("bellow", mx(0, vec![Fx::n("enrage", 2)])), ("rush", m(14)), ("skull_bash", mx(6, vec![Fx::n("vuln", 2)]))],
         opening(&["bellow"], weighted(&[("skull_bash", 33.0), ("rush", 67.0)], &[("rush", 2), ("skull_bash", 1)])), none());
    spec(s, "LAGAVULIN", "Lagavulin", (109, 111), vec![("sleep", mk("Sleep")), ("stunned", mk("Stun")), ("attack", m(18)), ("siphon", mx(0, vec![Fx::n("str_down", 1), Fx::n("dex_down", 1)]))],
         pat(_lagavulin), Kw { powers: vec![("METALLICIZE", 8), ("ASLEEP", 3)], hooks: vec![("wake", hook(|f, i, _, _| _lagavulin_wake(f, i)))], ..Kw::default() });
    spec(s, "SENTRY", "Sentry", (38, 42), vec![("beam", m(9)), ("bolt", mx(0, vec![Fx::status("DAZED", 2, "discard")]))], cycle(&["bolt", "beam"], 0), powers(&[("ARTIFACT", 1)]));
    spec(s, "SENTRY_MID", "Sentry", (38, 42), vec![("beam", m(9)), ("bolt", mx(0, vec![Fx::status("DAZED", 2, "discard")]))], cycle(&["beam", "bolt"], 0), powers(&[("ARTIFACT", 1)]));
    spec(s, "SLIME_BOSS", "Slime Boss", (140, 140), vec![("goop", mx(0, vec![_slimed(3)])), ("prep", mk("Unknown")), ("slam", m(35)), ("split", split())],
         // (the cycle is built anew at every call, as in the original: its count starts over each time)
         pat(|f, i| if f.foes[i].mem.truthy("splitting") { "split".to_string() } else { cycle::<Fight>(&["goop", "prep", "slam"], 0)(f, i) }),
         hooks(vec![("hit", _split_when_half(&["ACID_SLIME_L", "SPIKE_SLIME_L"]))]));
    spec(s, "HEXAGHOST", "Hexaghost", (250, 250),
         vec![("activate", mk("Unknown")), ("divider", mh(1, 6)), ("sear", mx(6, vec![Fx::status("BURN", 1, "discard")])), ("tackle", mh(5, 2)),
              ("inflame", M { block: 12, fx: vec![Fx::n("str", 2)], ..M::default() }),
              ("inferno", M { dmg: 2, hits: 6, fx: vec![Fx::status("BURN", 3, "discard"), Fx::call(_hexa_inferno)], ..M::default() })],
         pat(_hexa), none());
    spec(s, "THE_GUARDIAN", "The Guardian", (240, 240),
         vec![("charge_up", mb(0, 9)), ("fierce_bash", m(32)), ("vent_steam", mx(0, vec![Fx::n("weak", 2), Fx::n("vuln", 2)])), ("whirlwind", mh(5, 4)),
              ("close_up", M { fx: vec![Fx::call(|f: &mut Fight, x| f.foes[x].powers.put("SHARP_HIDE", 3))], kind: "Buff".to_string(), ..M::default() }),
              ("roll", m(9)), ("twin_slam", mh(8, 2))],
         pat(|f, i| _guardian(&mut f.foes[i])), hooks(vec![("hit", hook(_guardian_hit))]));

    // ------------------------------------------------------------------ act 2

    spec(s, "SPHERIC_GUARDIAN", "Spheric Guardian", (20, 20), vec![("activate", mb(0, 25)), ("attack_frail", mx(10, vec![Fx::n("frail", 5)])), ("slam", mh(10, 2)), ("harden", mb(10, 15))],
         opening(&["activate", "attack_frail"], cycle(&["slam", "harden"], 0)), Kw { powers: vec![("BARRICADE", 1), ("ARTIFACT", 3)], block: 40, ..Kw::default() });
    spec(s, "CHOSEN", "Chosen", (95, 99),
         vec![("poke", mh(5, 2)), ("hex", mx(0, vec![Fx::of("hex")])), ("debilitate", mx(10, vec![Fx::n("vuln", 2)])), ("drain", mx(0, vec![Fx::n("weak", 3), Fx::n("str", 3)])), ("zap", m(18))],
         pat(_chosen), none());
    spec(s, "SHELLED_PARASITE", "Shelled Parasite", (68, 72), vec![("double", mh(6, 2)), ("suck", mx(10, vec![Fx::n("heal", 10)])), ("fell", mx(18, vec![Fx::n("frail", 2)]))],
         weighted(&[("double", 40.0), ("suck", 40.0), ("fell", 20.0)], &[("double", 2), ("suck", 2), ("fell", 1)]),
         Kw { powers: vec![("PLATING", 14)], hooks: vec![("hit", hook(|f, i, _n, attack| {
             if attack {
                 let left = (f.foes[i].powers.val("PLATING") - 1).max(0);
                 f.foes[i].powers.put("PLATING", left);
             }
         }))], ..Kw::default() });
    spec(s, "BYRD", "Byrd", (25, 31),
         vec![("peck", mh(1, 5)), ("swoop", m(12)), ("caw", mx(0, vec![Fx::n("str", 1)])), ("stunned", mk("Stun")), ("headbutt", m(3)),
              ("fly", M { fx: vec![Fx::call(|f: &mut Fight, x| f.foes[x].powers.put("FLUTTER", 3))], kind: "Buff".to_string(), ..M::default() })],
         pat(_byrd),
         Kw { powers: vec![("FLUTTER", 3)], hooks: vec![("grounded", Rc::new(|f: &mut Fight, i: usize, _, _| {
             f.foes[i].mem.set("grounded", 2);
             f.foes[i].move_name = "stunned".to_string();
             true // (the original lambda's tuple)
         }) as Hook<Fight>)], ..Kw::default() });
    spec(s, "SNAKE_PLANT", "Snake Plant", (75, 79), vec![("chomp", mh(7, 3)), ("spores", mx(0, vec![Fx::n("weak", 2), Fx::n("frail", 2)]))],
         weighted(&[("chomp", 65.0), ("spores", 35.0)], &[("chomp", 2), ("spores", 1)]), powers(&[("MALLEABLE", 3)]));
    spec(s, "SNECKO", "Snecko", (114, 120), vec![("glare", mx(0, vec![Fx::of("confuse")])), ("bite", m(15)), ("tail_whip", mx(8, vec![Fx::n("vuln", 2)]))],
         opening(&["glare"], weighted(&[("bite", 60.0), ("tail_whip", 40.0)], &[("bite", 2)])), none());
    spec(s, "CENTURION", "Centurion", (76, 80), vec![("slash", m(12)), ("fury", mh(6, 3)), ("defend", mx(0, vec![Fx::n("block_ally", 15)]))],
         weighted(&[("slash", 40.0), ("fury", 30.0), ("defend", 30.0)], &[("fury", 1), ("defend", 1)]), none());
    spec(s, "MYSTIC", "Mystic", (48, 56), vec![("heal", mx(0, vec![Fx::n("heal_all", 16)])), ("buff", mx(0, vec![Fx::n("str_all", 2)])), ("attack", mx(8, vec![Fx::n("frail", 2)]))],
         weighted(&[("attack", 50.0), ("buff", 30.0), ("heal", 20.0)], &[("buff", 1), ("heal", 1)]), none());
    spec(s, "BOOK_OF_STABBING", "Book of Stabbing", (160, 164), vec![("multi", mh(6, 2)), ("single", m(21))], pat(_book),
         hooks(vec![("start", hook(|f, i, _, _| f.foes[i].mem.set_flag("painful_stabs", true)))]));
    spec(s, "GREMLIN_LEADER", "Gremlin Leader", (140, 148),
         vec![("rally", mx(0, vec![Fx::named("summon", "GREMLIN", 2)])), ("encourage", mx(0, vec![Fx::n("str_all", 3), Fx::n("block_all", 6)])), ("stab", mh(6, 3))], pat(_leader), none());
    spec(s, "TASKMASTER", "Taskmaster", (54, 60), vec![("whip", mx(7, vec![Fx::status("WOUND", 1, "discard")]))], cycle(&["whip"], 0), none());
    spec(s, "TORCH_HEAD", "Torch Head", (38, 40), vec![("tackle", m(7))], cycle(&["tackle"], 0), Kw { minion: true, ..Kw::default() });
    spec(s, "BRONZE_ORB", "Bronze Orb", (52, 58), vec![("beam", m(8)), ("support", mx(0, vec![Fx::n("block_ally", 12)])), ("stasis", mx(0, vec![Fx::n("draw_down", 1)]))],
         opening(&["stasis"], weighted(&[("beam", 60.0), ("support", 40.0)], &[("beam", 2), ("support", 1)])), Kw { minion: true, ..Kw::default() });
    spec(s, "BRONZE_AUTOMATON", "Bronze Automaton", (300, 300),
         vec![("spawn", mx(0, vec![Fx::named("summon", "BRONZE_ORB", 2)])), ("flail", mh(7, 2)), ("boost", M { block: 9, fx: vec![Fx::n("str", 3)], ..M::default() }), ("hyper_beam", m(45)),
              ("stunned", mk("Stun"))],
         opening(&["spawn"], cycle(&["flail", "boost", "flail", "boost", "hyper_beam", "stunned"], 0)), powers(&[("ARTIFACT", 3)]));
    spec(s, "THE_COLLECTOR", "The Collector", (282, 282),
         vec![("spawn", mx(0, vec![Fx::named("summon", "TORCH_HEAD", 2)])), ("fireball", m(18)), ("buff", M { block: 15, fx: vec![Fx::n("str_all", 3)], ..M::default() }),
              ("mega_debuff", mx(0, vec![Fx::n("weak", 3), Fx::n("vuln", 3), Fx::n("frail", 3)]))],
         pat(_collector), none());
    spec(s, "THE_CHAMP", "The Champ", (420, 420),
         vec![("defensive", M { block: 15, fx: vec![Fx::n("metallicize", 5)], ..M::default() }), ("face_slap", mx(12, vec![Fx::n("frail", 2), Fx::n("vuln", 2)])), ("heavy_slash", m(16)),
              ("gloat", mx(0, vec![Fx::n("str", 2)])), ("taunt", mx(0, vec![Fx::n("weak", 2), Fx::n("vuln", 2)])), ("anger", mx(0, vec![Fx::of("remove_debuffs"), Fx::n("str", 6)])),
              ("execute", mh(10, 2))],
         pat(_champ), none());

    // ------------------------------------------------------------------ act 3

    spec(s, "DARKLING", "Darkling", (48, 56), vec![("nip", m(9)), ("chomp", mh(8, 2)), ("harden", mb(0, 12)), ("regrow", mk("Unknown"))],
         weighted(&[("nip", 40.0), ("chomp", 30.0), ("harden", 30.0)], &[("chomp", 1), ("harden", 1)]),
         Kw { powers: vec![("REATTACH", 25)], hooks: vec![("death", Rc::new(|f: &mut Fight, i, _, _| _darkling_death(f, i)) as Hook<Fight>), ("start", hook(|f, i, _, _| {
             let nip = f.run.rng.randint(7, 11);
             _set(&mut f.foes[i], "nip", Some(nip), None);
         }))], ..Kw::default() });
    spec(s, "ORB_WALKER", "Orb Walker", (90, 96), vec![("laser", mx(10, vec![Fx::status("BURN", 1, "discard")])), ("claw", m(15))],
         weighted(&[("laser", 60.0), ("claw", 40.0)], &[("laser", 2), ("claw", 2)]),
         Kw { powers: vec![("RITUAL", 3)], hooks: vec![("start", hook(|f, i, _, _| f.foes[i].mem.set_flag("ritual_ready", true)))], ..Kw::default() });
    spec(s, "SPIKER", "Spiker", (42, 56), vec![("cut", m(7)), ("spike", mx(0, vec![Fx::n("thorns", 2)]))], weighted(&[("cut", 50.0), ("spike", 50.0)], &[("spike", 2), ("cut", 2)]), powers(&[("THORNS", 3)]));
    spec(s, "REPULSOR", "Repulsor", (29, 35), vec![("repulse", mx(0, vec![Fx::status("DAZED", 2, "draw")])), ("bash", m(11))], weighted(&[("repulse", 80.0), ("bash", 20.0)], &[("bash", 1)]), none());
    spec(s, "EXPLODER", "Exploder", (30, 30), vec![("slam", m(9)), ("explode", mx(30, vec![Fx::of("die")]))], opening(&["slam", "slam"], cycle(&["explode"], 0)), none());
    spec(s, "SPIRE_GROWTH", "Spire Growth", (170, 170), vec![("quick", m(16)), ("smash", m(22)), ("constrict", mx(0, vec![Fx::n("constrict", 10)]))], pat(_spire_growth), none());
    spec(s, "TRANSIENT", "Transient", (999, 999), vec![("attack", m(30))],
         pat(|f, i| {
             let dmg = 30 + 10 * f.foes[i].history.len() as i64;
             _set(&mut f.foes[i], "attack", Some(dmg), None);
             "attack".to_string()
         }),
         Kw { powers: vec![("FADING", 5), ("SHIFTING", 1)], hooks: vec![("end", hook(|f, i, _, _| _transient_end(f, i)))], ..Kw::default() });
    spec(s, "MAW", "The Maw", (300, 300), vec![("roar", mx(0, vec![Fx::n("weak", 3), Fx::n("frail", 3)])), ("slam", m(25)), ("nom", mh(5, 1)), ("drool", mx(0, vec![Fx::n("str", 3)]))], pat(_maw), none());
    // (the Writhing Mass picks again whenever an attack hits it)
    const WRITHE: [(&str, f64); 4] = [("strong", 20.0), ("multi", 25.0), ("flail", 25.0), ("wither", 30.0)];
    spec(s, "WRITHING_MASS", "Writhing Mass", (160, 160), vec![("strong", m(32)), ("multi", mh(7, 3)), ("flail", mb(15, 16)), ("wither", mx(10, vec![Fx::n("weak", 2), Fx::n("vuln", 2)]))],
         pat(|f, i| w(f, i, &WRITHE, &[])),
         Kw { powers: vec![("REACTIVE", 1), ("MALLEABLE", 3)], hooks: vec![("hit", hook(|f, i, _n, attack| {
             if attack && f.foes[i].alive() {
                 f.foes[i].move_name = w(f, i, &WRITHE, &[]);
             }
         }))], ..Kw::default() });
    spec(s, "JAW_WORM_HORDE", "Jaw Worm", (40, 44), vec![("chomp", m(11)), ("thrash", mb(7, 5)), ("bellow", M { block: 6, fx: vec![Fx::n("str", 3)], ..M::default() })],
         opening(&["bellow"], weighted(&[("bellow", 45.0), ("thrash", 30.0), ("chomp", 25.0)], &[("chomp", 1), ("bellow", 1), ("thrash", 2)])), none());
    spec(s, "GIANT_HEAD", "Giant Head", (500, 500), vec![("count", m(13)), ("glare", mx(0, vec![Fx::n("weak", 1)])), ("it_is_time", m(30))], pat(_giant_head), powers(&[("SLOW", 1)]));
    spec(s, "NEMESIS", "Nemesis", (185, 185), vec![("debuff", mx(0, vec![Fx::status("BURN", 5, "discard")])), ("attack", mh(6, 3)), ("scythe", m(45))], pat(_nemesis),
         hooks(vec![("end", hook(|f, i, _, _| {
             if !f.foes[i].powers.on("INTANGIBLE") {
                 f.foes[i].powers.put("INTANGIBLE", 1);
             }
         }))]));
    spec(s, "SNAKE_DAGGER", "Snake Dagger", (20, 25), vec![("stab", mx(9, vec![Fx::status("WOUND", 1, "discard")])), ("explode", mx(25, vec![Fx::of("die")]))],
         pat(|f, i| if f.foes[i].history.is_empty() { "stab" } else { "explode" }.to_string()), Kw { minion: true, ..Kw::default() });
    spec(s, "REPTOMANCER", "Reptomancer", (180, 190),
         vec![("summon", mx(0, vec![Fx::named("summon", "SNAKE_DAGGER", 1)])), ("snake_strike", M { dmg: 13, hits: 2, fx: vec![Fx::n("weak", 1)], ..M::default() }), ("big_bite", m(30))],
         opening(&["snake_strike"], weighted(&[("summon", 33.0), ("snake_strike", 33.0), ("big_bite", 34.0)], &[("summon", 1), ("big_bite", 1), ("snake_strike", 1)])), none());
    spec(s, "AWAKENED_ONE", "Awakened One", (300, 300),
         vec![("slash", m(20)), ("soul_strike", mh(6, 4)), ("rebirth", mk("Unknown")), ("dark_echo", m(40)), ("sludge", mx(18, vec![Fx::status("VOID", 1, "draw")])), ("tackle", mh(10, 3))],
         pat(_awakened),
         Kw { powers: vec![("CURIOSITY", 1), ("REGEN", 10), ("STOCK", 1)], hooks: vec![("death", Rc::new(|f: &mut Fight, i, _, _| _awakened_death(f, i)) as Hook<Fight>)], ..Kw::default() });
    spec(s, "TIME_EATER", "Time Eater", (456, 456),
         vec![("reverberate", mh(7, 3)), ("head_slam", mx(26, vec![Fx::n("draw_down", 1)])), ("ripple", M { block: 20, fx: vec![Fx::n("vuln", 1), Fx::n("weak", 1), Fx::n("frail", 1)], ..M::default() }),
              ("haste", M { fx: vec![Fx::call(_haste)], kind: "Buff".to_string(), ..M::default() })],
         pat(_time_eater), powers(&[("TIME_WARP", 0)]));
    spec(s, "DONU", "Donu", (250, 250), vec![("circle", mx(0, vec![Fx::n("str_all", 3)])), ("beam", mh(10, 2))], cycle(&["circle", "beam"], 0), powers(&[("ARTIFACT", 2)]));
    spec(s, "DECA", "Deca", (250, 250), vec![("beam", M { dmg: 10, hits: 2, fx: vec![Fx::status("DAZED", 2, "discard")], ..M::default() }), ("square", mx(0, vec![Fx::n("block_all", 16)]))],
         cycle(&["beam", "square"], 0), powers(&[("ARTIFACT", 2)]));
    std::mem::take(s)
}

pub const GREMLINS: [&str; 8] = ["MAD_GREMLIN", "MAD_GREMLIN", "SNEAKY_GREMLIN", "SNEAKY_GREMLIN", "FAT_GREMLIN", "FAT_GREMLIN", "SHIELD_GREMLIN", "GREMLIN_WIZARD"];
pub const SHAPES: [&str; 3] = ["SPIKER", "REPULSOR", "EXPLODER"];

/// An encounter: a list of keys, or a function of the random generator giving one (`name`: what tells it from
/// the others for the "not the last two again" rule, where the original has the function's id).
#[derive(Clone)]
pub enum Encounter {
    Keys(Vec<&'static str>),
    Drawn(String, Rc<dyn Fn(&mut Random) -> Vec<String>>),
}

fn _encounters() -> HashMap<i64, HashMap<&'static str, Vec<Encounter>>> {
    let mut drawn_n = 0;
    let mut drawn = |rule: Rc<dyn Fn(&mut Random) -> Vec<String>>| {
        drawn_n += 1;
        Encounter::Drawn(format!("drawn{drawn_n}"), rule)
    };
    let keys = |k: &[&'static str]| Encounter::Keys(k.to_vec());
    fn pick(rng: &mut Random, of: &[&str]) -> String {
        rng.choice(of).to_string()
    }
    fn _louses(n: usize) -> Rc<dyn Fn(&mut Random) -> Vec<String>> {
        Rc::new(move |rng| (0..n).map(|_| pick(rng, &["RED_LOUSE", "GREEN_LOUSE"])).collect())
    }
    fn _small_slimes(rng: &mut Random) -> Vec<String> {
        vec![pick(rng, &["ACID_SLIME_M", "SPIKE_SLIME_M"]), pick(rng, &["ACID_SLIME_S", "SPIKE_SLIME_S"])]
    }
    fn shapes(n: usize) -> Rc<dyn Fn(&mut Random) -> Vec<String>> {
        Rc::new(move |rng| (0..n).map(|_| pick(rng, &SHAPES)).collect())
    }
    // act -> pool -> list of encounters
    let act1 = HashMap::from([
        ("weak", vec![keys(&["CULTIST"]), keys(&["JAW_WORM"]), drawn(_louses(2)), drawn(Rc::new(_small_slimes))]),
        ("strong", vec![
            drawn(Rc::new(|rng| rng.sample(&GREMLINS, 4).into_iter().map(String::from).collect())),
            drawn(Rc::new(|rng| vec![pick(rng, &["ACID_SLIME_L", "SPIKE_SLIME_L"])])),
            keys(&["SPIKE_SLIME_S", "SPIKE_SLIME_S", "SPIKE_SLIME_S", "ACID_SLIME_S", "ACID_SLIME_S"]), keys(&["BLUE_SLAVER"]), keys(&["RED_SLAVER"]), drawn(_louses(3)),
            keys(&["FUNGI_BEAST", "FUNGI_BEAST"]),
            drawn(Rc::new(|rng| vec!["LOOTER".to_string(), pick(rng, &["BLUE_SLAVER", "RED_LOUSE", "ACID_SLIME_M"])])),
            drawn(Rc::new(|rng| vec!["FUNGI_BEAST".to_string(), pick(rng, &["JAW_WORM", "RED_LOUSE"])])),
            keys(&["LOOTER"]),
        ]),
        ("elite", vec![keys(&["GREMLIN_NOB"]), keys(&["LAGAVULIN"]), keys(&["SENTRY", "SENTRY_MID", "SENTRY"])]),
        ("boss", vec![keys(&["SLIME_BOSS"]), keys(&["HEXAGHOST"]), keys(&["THE_GUARDIAN"])]),
    ]);
    let act2 = HashMap::from([
        ("weak", vec![keys(&["SPHERIC_GUARDIAN"]), keys(&["CHOSEN"]), keys(&["SHELLED_PARASITE"]), keys(&["BYRD", "BYRD", "BYRD"]), keys(&["LOOTER", "MUGGER"])]),
        ("strong", vec![keys(&["CHOSEN", "BYRD"]), keys(&["CULTIST", "CHOSEN"]), keys(&["SENTRY", "SPHERIC_GUARDIAN"]), keys(&["SNAKE_PLANT"]), keys(&["SNECKO"]), keys(&["CENTURION", "MYSTIC"]),
                        keys(&["CULTIST", "CULTIST", "CULTIST"]), keys(&["SHELLED_PARASITE", "FUNGI_BEAST"])]),
        ("elite", vec![
            drawn(Rc::new(|rng| {
                let mut out = vec!["GREMLIN_LEADER".to_string()];
                out.extend(rng.sample(&GREMLINS, 2).into_iter().map(String::from));
                out
            })),
            keys(&["BLUE_SLAVER", "TASKMASTER", "RED_SLAVER"]), keys(&["BOOK_OF_STABBING"]),
        ]),
        ("boss", vec![keys(&["BRONZE_AUTOMATON"]), keys(&["THE_COLLECTOR"]), keys(&["THE_CHAMP"])]),
    ]);
    let act3 = HashMap::from([
        ("weak", vec![keys(&["DARKLING", "DARKLING", "DARKLING"]), keys(&["ORB_WALKER"]), drawn(shapes(3))]),
        ("strong", vec![
            keys(&["SPIRE_GROWTH"]), keys(&["TRANSIENT"]), drawn(shapes(4)), keys(&["MAW"]),
            drawn(Rc::new(|rng| vec!["SPHERIC_GUARDIAN".to_string(), pick(rng, &SHAPES), pick(rng, &SHAPES)])),
            keys(&["JAW_WORM_HORDE", "JAW_WORM_HORDE", "JAW_WORM_HORDE"]), keys(&["DARKLING", "DARKLING", "DARKLING"]), keys(&["WRITHING_MASS"]),
        ]),
        ("elite", vec![keys(&["GIANT_HEAD"]), keys(&["NEMESIS"]), keys(&["REPTOMANCER", "SNAKE_DAGGER", "SNAKE_DAGGER"])]),
        ("boss", vec![keys(&["AWAKENED_ONE", "CULTIST", "CULTIST"]), keys(&["TIME_EATER"]), keys(&["DONU", "DECA"])]),
    ]);
    HashMap::from([(1, act1), (2, act2), (3, act3)])
}

/// act -> pool -> list of encounters.
pub fn ENCOUNTERS() -> Rc<HashMap<i64, HashMap<&'static str, Vec<Encounter>>>> {
    ENCOUNTER_TABLE.with(|e| e.clone())
}

/// The first fights of an act are drawn from its weak pool.
pub fn WEAK_FIGHTS(act: i64) -> i64 {
    match act {
        1 => 3,
        2 => 2,
        3 => 2,
        _ => panic!("KeyError: {act}"),
    }
}

/// `MINIONS[key]`: the leaders whose company are minions.
pub fn MINIONS(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "GREMLIN_LEADER" => Some(&GREMLINS),
        "REPTOMANCER" => Some(&["SNAKE_DAGGER"]),
        _ => None,
    }
}
