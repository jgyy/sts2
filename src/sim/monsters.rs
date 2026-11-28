//! Enemies: a move table per monster and the rule that picks the next move.
//!
//! A move is damage (times hits), Block, and a list of effects (`fx`), each a word the engine knows
//! (effects::do_) and its arguments: ("str", 3) Strength for itself, ("weak", 2) Weak on us,
//! ("status", "WOUND", 2, "discard") cards into one of our piles, ("summon", "TORCH_HEAD", 2) and so on. A pattern
//! is a function of the fight and the monster's place in it that names the next move; the helpers below cover
//! the usual ones.
//!
//! # How this differs from the original (the engine and the games' monster tables build on it)
//!
//! **The fight is a type parameter.** In the original a monster's rules are functions of the fight (`hooks`:
//! `fn(fight, foe, ...)`; a move's `("call", fn)`: `fn(fight, foe)`) and its pattern a function `(foe, rng)` that reaches
//! the fight through `foe.mem["fight"]`. Here `Move<F>`, `Spec<F>`, `Foe<F>` are generic over the fight `F`, and
//! every rule takes the fight and the INDEX of the monster in the fight's list of foes (the list only grows, so
//! the index is the monster's identity: the original's `x is not foe` is `n != i`):
//!
//! ```ignore
//! pub type Pattern<F> = Rc<dyn Fn(&mut F, usize) -> String>;           // (fight, foe index) -> move name
//! pub type Hook<F>    = Rc<dyn Fn(&mut F, usize, i64, bool) -> bool>;  // (fight, foe index, amount, attack) -> truth
//! pub type Call<F>    = Rc<dyn Fn(&mut F, usize)>;                     // a move's ("call", fn)
//! ```
//!
//! sim/engine.rs is expected to say `pub type Foe = monsters::Foe<Fight>;` (and the same for `Spec`, `Move`, `Fx`)
//! and to implement [`FightLike`] for its `Fight`: one method, `with_foe`, that lends a monster and the fight's
//! random generator together. That is all this module asks of a fight (the helpers `cycle`, `opening`, `weighted`
//! and [`choose`] need a monster's memory, its history and the generator at once).
//!
//! - `Foe.choose(rng)` is the free function [`choose`]`(fight, i)` (a pattern may look at the whole fight).
//!   A pattern written out in a monster table takes `(f, i)`; where the original calls a helper inline,
//!   `weighted({...}, {...})(foe, rng)`, call [`weighted_pick`]`(foe, rng, &[...], &[...])` (the same draw), or build the
//!   pattern and call it: `weighted(&[...], &[...])(f, i)`; `cycle(&[...], 0)(f, i)` built inline starts from a new
//!   count every time, as it does in the original (the Slime Boss's `cycle("goop", "prep", "slam")(f, r)`).
//! - A hook is called with `(fight, i, amount, attack)`; the events that pass no amount ("start", "death", "end",
//!   "acted", "wake", "grounded") get `0, false`. It returns the original's truth of what the function returned (only
//!   "death" is asked: true, it is not gone); a the original hook returning None returns false.
//! - `foe.mem["fight"] = f` has nothing to keep; the monster tables that ask `if fight` (sts2_foes._holds) can set the
//!   flag `foe.mem.set("fight", 1)` where the original stored the fight.
//!
//! **`Foe.mem`** is a dict of mixed keys and values in the original; here [`Mem`] keeps them by type: `ints` (counts, and
//! flags as 0/1: `mem.get("shell", 0)`, `mem.truthy("awake")`, `mem.add("slept", 1)`), `strs` ("state", "force"),
//! `dmg` and `hits` for the keys `("dmg", move)` and `("hits", move)`, `lists` (the slimes' "split_into").
//! The patterns' own keys (the original's `key = object()`) are names no table uses, from [`_key`].
//!
//! **A move's effects** are [`Fx`]: a word and its arguments ([`Arg`]: an int, a string or a function).
//! `Spec.moves` holds `Rc<Move>` so that `foe.r#move()` hands out the move without borrowing the monster.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::base::{self, Random};

/// What this module asks of a fight: its `i`-th monster and its random generator, lent together.
pub trait FightLike: Sized + 'static {
    fn with_foe<R>(&mut self, i: usize, body: impl FnOnce(&mut Foe<Self>, &mut Random) -> R) -> R;
}

/// (fight, the monster's index) -> the name of its next move.
pub type Pattern<F> = Rc<dyn Fn(&mut F, usize) -> String>;
/// A monster's own rule for an event: (fight, the monster's index, amount, attack) -> the truth of what the original returned.
pub type Hook<F> = Rc<dyn Fn(&mut F, usize, i64, bool) -> bool>;
/// What a move's `("call", fn)` does: (fight, the monster's index).
pub type Call<F> = Rc<dyn Fn(&mut F, usize)>;

/// An argument of a move's effect.
pub enum Arg<F> {
    Int(i64),
    Str(String),
    Call(Call<F>),
}

impl<F> Clone for Arg<F> {
    fn clone(&self) -> Self {
        match self {
            Arg::Int(n) => Arg::Int(*n),
            Arg::Str(s) => Arg::Str(s.clone()),
            Arg::Call(c) => Arg::Call(c.clone()),
        }
    }
}

impl<F> fmt::Debug for Arg<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Arg::Int(n) => write!(f, "{n}"),
            Arg::Str(s) => write!(f, "{s:?}"),
            Arg::Call(_) => write!(f, "<function>"),
        }
    }
}

/// (Functions are equal when they are the same one, as in the original.)
impl<F> PartialEq for Arg<F> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Arg::Int(a), Arg::Int(b)) => a == b,
            (Arg::Str(a), Arg::Str(b)) => a == b,
            (Arg::Call(a), Arg::Call(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl<F> From<i64> for Arg<F> {
    fn from(n: i64) -> Self {
        Arg::Int(n)
    }
}

impl<F> From<&str> for Arg<F> {
    fn from(s: &str) -> Self {
        Arg::Str(s.to_string())
    }
}

impl<F> From<String> for Arg<F> {
    fn from(s: String) -> Self {
        Arg::Str(s)
    }
}

/// One effect of a move: the original's tuple `(word, *args)`.
pub struct Fx<F> {
    pub word: String,
    pub args: Vec<Arg<F>>,
}

impl<F> Fx<F> {
    /// `(word, *args)`: `Fx::new("status", ["WOUND".into(), 2.into(), "discard".into()])`.
    pub fn new(word: &str, args: impl IntoIterator<Item = Arg<F>>) -> Fx<F> {
        Fx { word: word.to_string(), args: args.into_iter().collect() }
    }

    /// `(word,)`: ("entangle",).
    pub fn of(word: &str) -> Fx<F> {
        Fx::new(word, [])
    }

    /// `(word, n)`: ("str", 3).
    pub fn n(word: &str, n: i64) -> Fx<F> {
        Fx::new(word, [Arg::Int(n)])
    }

    /// `(word, name, n)`: ("summon", "TORCH_HEAD", 2), ("power", "RITUAL", 1).
    pub fn named(word: &str, name: &str, n: i64) -> Fx<F> {
        Fx::new(word, [Arg::Str(name.to_string()), Arg::Int(n)])
    }

    /// `("status", key, count, pile)`.
    pub fn status(key: &str, count: i64, pile: &str) -> Fx<F> {
        Fx::new("status", [Arg::Str(key.to_string()), Arg::Int(count), Arg::Str(pile.to_string())])
    }

    /// `("call", fn)`.
    pub fn call(rule: impl Fn(&mut F, usize) + 'static) -> Fx<F> {
        Fx::new("call", [Arg::Call(Rc::new(rule))])
    }

    /// `args[i]` as an int (0 when it is missing or not one: the engine's `n = args[0] if args else 0`).
    pub fn int(&self, i: usize) -> i64 {
        match self.args.get(i) {
            Some(Arg::Int(n)) => *n,
            _ => 0,
        }
    }

    /// `args[i]` when it is a string.
    pub fn str(&self, i: usize) -> Option<&str> {
        match self.args.get(i) {
            Some(Arg::Str(s)) => Some(s),
            _ => None,
        }
    }

    /// `args[i]` when it is a function.
    pub fn fun(&self, i: usize) -> Option<Call<F>> {
        match self.args.get(i) {
            Some(Arg::Call(c)) => Some(c.clone()),
            _ => None,
        }
    }
}

impl<F> Clone for Fx<F> {
    fn clone(&self) -> Self {
        Fx { word: self.word.clone(), args: self.args.clone() }
    }
}

impl<F> fmt::Debug for Fx<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:?}", self.word)?;
        for a in &self.args {
            write!(f, ", {a:?}")?;
        }
        write!(f, ")")
    }
}

impl<F> PartialEq for Fx<F> {
    fn eq(&self, other: &Self) -> bool {
        self.word == other.word && self.args == other.args
    }
}

pub struct Move<F> {
    pub dmg: i64,
    pub hits: i64,
    pub block: i64,
    pub fx: Vec<Fx<F>>,
    pub kind: String, // an intent type the move shows instead of the one worked out (Sleep, Stun, Escape, Unknown)
}

impl<F> Move<F> {
    /// `Move(dmg, hits, block, fx, kind)`.
    pub fn new(dmg: i64, hits: i64, block: i64, fx: Vec<Fx<F>>, kind: &str) -> Move<F> {
        Move { dmg, hits, block, fx, kind: kind.to_string() }
    }
}

impl<F> Default for Move<F> {
    fn default() -> Self {
        Move { dmg: 0, hits: 1, block: 0, fx: Vec::new(), kind: String::new() }
    }
}

impl<F> Clone for Move<F> {
    fn clone(&self) -> Self {
        Move { dmg: self.dmg, hits: self.hits, block: self.block, fx: self.fx.clone(), kind: self.kind.clone() }
    }
}

impl<F> fmt::Debug for Move<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Move").field("dmg", &self.dmg).field("hits", &self.hits).field("block", &self.block).field("fx", &self.fx).field("kind", &self.kind).finish()
    }
}

impl<F> PartialEq for Move<F> {
    fn eq(&self, other: &Self) -> bool {
        self.dmg == other.dmg && self.hits == other.hits && self.block == other.block && self.fx == other.fx && self.kind == other.kind
    }
}

pub struct Spec<F> {
    pub key: String,
    pub name: String,
    pub hp: (i64, i64),
    pub moves: IndexMap<String, Rc<Move<F>>>,
    pub pattern: Pattern<F>,
    pub powers: IndexMap<String, i64>, // powers it starts the fight with
    pub block: i64,                    // Block it starts the fight with
    pub minion: bool,
    pub hooks: IndexMap<String, Hook<F>>, // event -> fn(fight, foe, ...): "hit", "death", "end"
}

impl<F> Spec<F> {
    /// `Spec(key, name, hp, moves, pattern)`: no powers, no Block, not a minion, no hooks (set the fields for those:
    /// `Spec { minion: true, ..Spec::new(...) }`).
    pub fn new(key: &str, name: &str, hp: (i64, i64), moves: impl IntoIterator<Item = (String, Move<F>)>, pattern: Pattern<F>) -> Spec<F> {
        Spec {
            key: key.to_string(),
            name: name.to_string(),
            hp,
            moves: moves.into_iter().map(|(k, m)| (k, Rc::new(m))).collect(),
            pattern,
            powers: IndexMap::new(),
            block: 0,
            minion: false,
            hooks: IndexMap::new(),
        }
    }

    /// `spec.hooks.get(event)`.
    pub fn hook(&self, event: &str) -> Option<Hook<F>> {
        self.hooks.get(event).cloned()
    }
}

impl<F> Clone for Spec<F> {
    fn clone(&self) -> Self {
        Spec {
            key: self.key.clone(),
            name: self.name.clone(),
            hp: self.hp,
            moves: self.moves.clone(),
            pattern: self.pattern.clone(),
            powers: self.powers.clone(),
            block: self.block,
            minion: self.minion,
            hooks: self.hooks.clone(),
        }
    }
}

impl<F> fmt::Debug for Spec<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Spec")
            .field("key", &self.key)
            .field("name", &self.name)
            .field("hp", &self.hp)
            .field("moves", &self.moves)
            .field("powers", &self.powers)
            .field("block", &self.block)
            .field("minion", &self.minion)
            .field("hooks", &self.hooks.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// (The pattern and the hooks are equal when they are the same functions, as in the original.)
impl<F> PartialEq for Spec<F> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.name == other.name
            && self.hp == other.hp
            && self.moves == other.moves
            && Rc::ptr_eq(&self.pattern, &other.pattern)
            && self.powers == other.powers
            && self.block == other.block
            && self.minion == other.minion
            && self.hooks.len() == other.hooks.len()
            && self.hooks.iter().all(|(k, h)| other.hooks.get(k).is_some_and(|o| Rc::ptr_eq(h, o)))
    }
}

/// A monster's memory (the original's `Foe.mem`, a dict), by the type of what is kept.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Mem {
    /// Counts, and flags as 0 and 1.
    pub ints: HashMap<String, i64>,
    /// "state", "force": names of moves and states.
    pub strs: HashMap<String, String>,
    /// `mem[("dmg", move)]`: a move's damage where it is not the table's.
    pub dmg: HashMap<String, i64>,
    /// `mem[("hits", move)]`.
    pub hits: HashMap<String, i64>,
    /// Lists of names ("split_into": the monsters a slime splits into).
    pub lists: HashMap<String, Vec<String>>,
}

impl Mem {
    /// `mem.get(key, default)`.
    pub fn get(&self, key: &str, default: i64) -> i64 {
        self.ints.get(key).copied().unwrap_or(default)
    }

    /// `bool(mem.get(key))`.
    pub fn truthy(&self, key: &str) -> bool {
        self.ints.get(key).is_some_and(|v| *v != 0)
    }

    /// `key in mem` (among the ints).
    pub fn has(&self, key: &str) -> bool {
        self.ints.contains_key(key)
    }

    /// `mem[key] = value`.
    pub fn set(&mut self, key: &str, value: i64) {
        match self.ints.get_mut(key) {
            Some(v) => *v = value,
            None => {
                self.ints.insert(key.to_string(), value);
            }
        }
    }

    /// `mem[key] = True` or `False`.
    pub fn set_flag(&mut self, key: &str, value: bool) {
        self.set(key, value as i64);
    }

    /// `effects.add(mem, key, n)`: `mem[key] += n` (from 0); the new value.
    pub fn add(&mut self, key: &str, n: i64) -> i64 {
        let v = self.get(key, 0) + n;
        self.set(key, v);
        v
    }

    /// `mem.pop(key, default)`.
    pub fn pop(&mut self, key: &str, default: i64) -> i64 {
        self.ints.remove(key).unwrap_or(default)
    }

    /// `mem.setdefault(key, default)`.
    pub fn setdefault(&mut self, key: &str, default: i64) -> i64 {
        if let Some(v) = self.ints.get(key) {
            return *v;
        }
        self.ints.insert(key.to_string(), default);
        default
    }

    /// `mem.get(key)` for a name ("state", "force").
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.strs.get(key).map(|s| s.as_str())
    }

    pub fn set_str(&mut self, key: &str, value: &str) {
        self.strs.insert(key.to_string(), value.to_string());
    }

    /// `mem.pop(key, None)` for a name.
    pub fn pop_str(&mut self, key: &str) -> Option<String> {
        self.strs.remove(key)
    }
}

/// A monster in a fight.
pub struct Foe<F> {
    pub spec: Rc<Spec<F>>,
    pub key: String,
    pub name: String,
    pub eid: String,
    pub max_hp: i64,
    pub hp: i64,
    pub block: i64,
    pub powers: IndexMap<String, i64>,
    pub minion: bool,
    pub history: Vec<String>,
    pub move_name: String,
    pub mem: Mem,
    pub gone: bool,      // dead, escaped or eaten
    pub half_dead: bool, // Darklings, the Awakened One between forms: on the field, not hurt by anything
}

impl<F> Foe<F> {
    pub fn new(spec: &Rc<Spec<F>>, n: i64, hp: i64, hp_mult: f64) -> Foe<F> {
        let hp = base::round(hp as f64 * hp_mult).max(1);
        Foe {
            spec: spec.clone(),
            key: spec.key.clone(),
            name: spec.name.clone(),
            eid: format!("{}_{}", spec.key, n),
            max_hp: hp,
            hp,
            block: spec.block,
            powers: spec.powers.clone(),
            minion: spec.minion,
            history: Vec::new(),
            move_name: String::new(),
            mem: Mem::default(),
            gone: false,
            half_dead: false,
        }
    }

    pub fn alive(&self) -> bool {
        !self.gone && !self.half_dead
    }

    pub fn r#move(&mut self) -> Rc<Move<F>> {
        if let Some(m) = self.spec.moves.get(&self.move_name) {
            return m.clone();
        }
        // (a state machine that names a move the table lacks: its first move stands in)
        let (name, m) = self.spec.moves.first().expect("a monster with no moves");
        self.move_name = name.clone();
        m.clone()
    }

    /// `history[-n:]`.
    pub fn last(&self, n: usize) -> &[String] {
        &self.history[self.history.len() - if n == 0 { self.history.len() } else { n.min(self.history.len()) }..]
    }

    /// Its next move is its Stun move, if it has one.
    pub fn stun(&mut self) {
        if let Some((name, _)) = self.spec.moves.iter().find(|(_, mv)| mv.kind == "Stun") {
            self.move_name = name.clone();
        }
    }
}

impl<F> Clone for Foe<F> {
    fn clone(&self) -> Self {
        Foe {
            spec: self.spec.clone(),
            key: self.key.clone(),
            name: self.name.clone(),
            eid: self.eid.clone(),
            max_hp: self.max_hp,
            hp: self.hp,
            block: self.block,
            powers: self.powers.clone(),
            minion: self.minion,
            history: self.history.clone(),
            move_name: self.move_name.clone(),
            mem: self.mem.clone(),
            gone: self.gone,
            half_dead: self.half_dead,
        }
    }
}

impl<F> fmt::Debug for Foe<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Foe")
            .field("eid", &self.eid)
            .field("name", &self.name)
            .field("hp", &self.hp)
            .field("max_hp", &self.max_hp)
            .field("block", &self.block)
            .field("powers", &self.powers)
            .field("minion", &self.minion)
            .field("history", &self.history)
            .field("move_name", &self.move_name)
            .field("mem", &self.mem)
            .field("gone", &self.gone)
            .field("half_dead", &self.half_dead)
            .finish()
    }
}

/// `foe.choose(rng)`: the fight's `i`-th monster picks its next move (the generator is the fight's).
pub fn choose<F: FightLike>(f: &mut F, i: usize) {
    let pattern = f.with_foe(i, |foe, _| foe.spec.pattern.clone());
    let name = pattern(f, i);
    f.with_foe(i, |foe, _| foe.move_name = name);
}

// ----------------------------------------------------------------- patterns

thread_local! {
    static KEYS: Cell<u64> = const { Cell::new(0) };
}

/// A key for a pattern's own count in a monster's memory that nothing else uses (the original's `key = object()`).
pub fn _key() -> String {
    KEYS.with(|k| {
        k.set(k.get() + 1);
        format!("<object {}>", k.get())
    })
}

/// A count kept in the monster's memory: its value before this call adds one.
pub fn counter<F>(f: &mut Foe<F>, key: &str, start: i64) -> i64 {
    let i = f.mem.get(key, start);
    f.mem.set(key, i + 1);
    i
}

/// The moves in turn, round and round.
pub fn cycle<F: FightLike>(names: &[&str], start: i64) -> Pattern<F> {
    let key = _key();
    let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    Rc::new(move |f, i| f.with_foe(i, |foe, _| names[base::modulo(counter(foe, &key, start), names.len() as i64) as usize].clone()))
}

/// These moves first, in order, then the pattern `then`.
pub fn opening<F: FightLike>(names: &[&str], then: Pattern<F>) -> Pattern<F> {
    let key = _key();
    let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    Rc::new(move |f, i| {
        let first = f.with_foe(i, |foe, _| {
            let n = foe.mem.get(&key, 0);
            if n < names.len() as i64 {
                foe.mem.set(&key, n + 1);
                return Some(names[n as usize].clone());
            }
            None
        });
        match first {
            Some(name) => name,
            None => then(f, i),
        }
    })
}

/// A move at random by weight, never more than `most[move]` times in a row (default: no limit).
pub fn weighted<F: FightLike>(weights: &[(&str, f64)], most: &[(&str, i64)]) -> Pattern<F> {
    let weights: Vec<(String, f64)> = weights.iter().map(|(m, w)| (m.to_string(), *w)).collect();
    let most: Vec<(String, i64)> = most.iter().map(|(m, n)| (m.to_string(), *n)).collect();
    Rc::new(move |f, i| f.with_foe(i, |foe, rng| weighted_pick(foe, rng, &weights, &most)))
}

/// What the pattern `weighted(weights, most)` picks for this monster: the original's `weighted(weights, most)(foe, rng)`.
pub fn weighted_pick<F, S: AsRef<str>, T: AsRef<str>>(f: &Foe<F>, rng: &mut Random, weights: &[(S, f64)], most: &[(T, i64)]) -> String {
    let limit = |m: &str| most.iter().find(|(k, _)| k.as_ref() == m).map(|(_, n)| *n).unwrap_or(99);
    let ok: Vec<(&str, f64)> = weights.iter().map(|(m, w)| (m.as_ref(), *w)).filter(|(m, _)| limit(m) > _run_length(&f.history, m)).collect();
    let options: Vec<(&str, f64)> = if ok.is_empty() { weights.iter().map(|(m, w)| (m.as_ref(), *w)).collect() } else { ok };
    let mut r = rng.random() * base::fsum(options.iter().map(|(_, w)| *w));
    for (m, w) in &options {
        r -= w;
        if r <= 0.0 {
            return m.to_string();
        }
    }
    options.first().expect("a pattern with no moves").0.to_string()
}

pub fn _run_length(history: &[String], r#move: &str) -> i64 {
    history.iter().rev().position(|m| m != r#move).unwrap_or(history.len()) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::JsonExt;
    use serde_json::{json, Value};

    struct Scene {
        foes: Vec<Foe<Scene>>,
        rng: Random,
        calls: i64,
    }

    impl FightLike for Scene {
        fn with_foe<R>(&mut self, i: usize, body: impl FnOnce(&mut Foe<Self>, &mut Random) -> R) -> R {
            body(&mut self.foes[i], &mut self.rng)
        }
    }

    fn names(v: &Value) -> Vec<&str> {
        v.list().iter().map(|n| n.s()).collect()
    }

    /// A pattern from its description in the golden file: ["cycle", names, start] | ["opening", names, then] |
    /// ["weighted", [[move, weight] ...], [[move, most] ...]] | ["inline", ...] (the helper built anew at every call).
    fn pattern_of(v: &Value) -> Pattern<Scene> {
        match v[0].s() {
            "cycle" => cycle(&names(&v[1]), v[2].as_i64().unwrap()),
            "opening" => opening(&names(&v[1]), pattern_of(&v[2])),
            "weighted" => {
                let weights: Vec<(&str, f64)> = v[1].list().iter().map(|w| (w[0].s(), w[1].as_f64().unwrap())).collect();
                let most: Vec<(&str, i64)> = v[2].list().iter().map(|w| (w[0].s(), w[1].as_i64().unwrap())).collect();
                weighted(&weights, &most)
            }
            "inline" => {
                let inner = v[1].clone();
                Rc::new(move |f: &mut Scene, i| pattern_of(&inner)(f, i))
            }
            "counter" => Rc::new(|f: &mut Scene, i| f.with_foe(i, |foe, _| if counter(foe, "i", 0) % 4 == 3 { "blast" } else { "charge" }.to_string())),
            other => panic!("{other}"),
        }
    }

    fn spec_of(v: &Value) -> Rc<Spec<Scene>> {
        let moves = v["moves"].list().iter().map(|m| (m[0].s().to_string(), Move::new(m[1].as_i64().unwrap(), m[2].as_i64().unwrap(), m[3].as_i64().unwrap(), vec![], m[4].s())));
        let mut spec = Spec::new(v["key"].s(), v["name"].s(), (v["hp"][0].as_i64().unwrap(), v["hp"][1].as_i64().unwrap()), moves, pattern_of(&v["pattern"]));
        spec.powers = v["powers"].entries().map(|(k, n)| (k.clone(), n.as_i64().unwrap())).collect();
        spec.block = v["block"].as_i64().unwrap();
        spec.minion = v["minion"].as_bool().unwrap();
        Rc::new(spec)
    }

    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/sim_monsters.json")).unwrap();
        let mut checked = 0;
        for case in g["patterns"].list() {
            let spec = spec_of(&case["spec"]);
            for run in case["runs"].list() {
                let mut scene = Scene { foes: vec![Foe::new(&spec, 0, 30, 1.0), Foe::new(&spec, 1, 30, 1.0)], rng: Random::new(run["seed"].as_i64().unwrap() as i128), calls: 0 };
                let mut got = Vec::new();
                for _ in 0..run["moves"].list().len() {
                    choose(&mut scene, 1);
                    let foe = &mut scene.foes[1];
                    foe.history.push(foe.move_name.clone());
                    got.push(foe.move_name.clone());
                }
                assert_eq!(json!(got), run["moves"], "{} seed {}", case["spec"]["pattern"], run["seed"]);
                assert_eq!(scene.rng.random(), run["next"].as_f64().unwrap(), "{} seed {}", case["spec"]["pattern"], run["seed"]);
                assert!(scene.foes[0].history.is_empty() && scene.foes[0].mem == Mem::default());
                checked += got.len() + 1;
            }
        }
        for case in g["foes"].list() {
            let spec = spec_of(&case["spec"]);
            let mut foe = Foe::new(&spec, case["n"].as_i64().unwrap(), case["hp"].as_i64().unwrap(), case["mult"].as_f64().unwrap());
            let powers: serde_json::Map<String, Value> = foe.powers.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
            let seen = json!([foe.key, foe.name, foe.eid, foe.max_hp, foe.hp, foe.block, powers, foe.minion, foe.alive()]);
            assert_eq!(seen, case["made"], "{case}");
            for step in case["moves"].list() {
                foe.move_name = step[0].s().to_string();
                let m = foe.r#move();
                assert_eq!(json!([foe.move_name, m.dmg, m.hits, m.block, m.kind]), step[1], "{case}");
                foe.stun();
                assert_eq!(foe.move_name, step[2].s(), "{case}");
                checked += 2;
            }
            foe.history = names(&case["history"]).iter().map(|s| s.to_string()).collect();
            for (n, want) in case["last"].list().iter().enumerate() {
                assert_eq!(json!(foe.last(n)), *want, "last {n} {case}");
                checked += 1;
            }
            for row in case["runs"].list() {
                assert_eq!(_run_length(&foe.history, row[0].s()), row[1].as_i64().unwrap(), "{case}");
                checked += 1;
            }
            foe.gone = true;
            assert!(!foe.alive());
            checked += 1;
        }
        assert!(checked > 3000, "{checked} golden cases");
    }

    #[test]
    fn hooks_and_calls_reach_the_fight() {
        let mut spec: Spec<Scene> = Spec::new("X", "X", (5, 5), [("hit".to_string(), Move { dmg: 3, fx: vec![Fx::n("str", 2), Fx::call(|f: &mut Scene, _| f.calls += 10)], ..Move::default() })], cycle(&["hit"], 0));
        spec.hooks.insert("hit".into(), Rc::new(|f: &mut Scene, i, amount, attack| {
            f.calls += amount;
            f.foes[i].mem.add("taken", amount);
            attack
        }));
        let spec = Rc::new(spec);
        let mut scene = Scene { foes: vec![Foe::new(&spec, 0, 5, 1.0)], rng: Random::new(1), calls: 0 };
        let hook = scene.foes[0].spec.hook("hit").unwrap();
        assert!(hook(&mut scene, 0, 4, true));
        assert!(scene.foes[0].spec.hook("death").is_none());
        let m = scene.foes[0].r#move();
        assert_eq!((m.fx[0].word.as_str(), m.fx[0].int(0), m.fx[0].str(0)), ("str", 2, None));
        m.fx[1].fun(0).unwrap()(&mut scene, 0);
        assert_eq!((scene.calls, scene.foes[0].mem.get("taken", 0), scene.foes[0].move_name.as_str()), (14, 4, "hit"));
        assert_eq!(*m, Move { dmg: 3, fx: m.fx.clone(), ..Move::default() });
        assert_eq!(*spec, (*spec).clone());
        let mem = &mut scene.foes[0].mem;
        assert_eq!((mem.pop("taken", 0), mem.pop("taken", 7), mem.setdefault("m", 3), mem.setdefault("m", 5), mem.truthy("m"), mem.has("x")), (4, 7, 3, 3, true, false));
        mem.set_str("force", "STUN");
        assert_eq!((mem.get_str("force"), mem.get_str("state")), (Some("STUN"), None));
        assert_eq!((mem.pop_str("force"), mem.pop_str("force")), (Some("STUN".to_string()), None));
    }
}
