//! What the model fights share: drawing, the combat state the planner reads, its turn, NAME=VALUE patches, the arms table.
//!
//! Where the Rust differs in shape from scripts/modelfight.rs:
//! - A fight's result, a row of a table, a deck: [`Datum`], a the original value that prints as the original printed it.
//! - `print` is [`out!`]: to stdout, or into the text a test is capturing ([`capture`]).
//! - `with patch(arm): body` is [`under`]: the settings are per thread, so the body runs on a thread of its own, which
//!   starts from the constants as written and ends with them. (No script patches inside a patch but for what `patched` adds.)
//! - `turn` was a generator; here it is one step of it (the planner's next play) and each fight writes the loop out.
//! - the original had two bindings of the constants combat.rs imported from battle.rs (`battle.RAGE_COST`, `combat.RAGE_COST`):
//!   patching one changed only what that module read. Here there is one constant, so [`set_battle`] and [`set_combat`] set it
//!   only when the module named is the one that reads it, and say so (on stderr) when they change nothing, as the original did silently.

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};

use crate::fight::fool::sadness;
use crate::fight::{battle, closing, combat, triggers};
use crate::logging;
use crate::base::{self, JsonExt, Random};
use crate::{learn, Action};

// ---------------------------------------------------------------------------------------------------------------------
// Printing

thread_local! {
    static SINK: RefCell<Option<Arc<Mutex<String>>>> = const { RefCell::new(None) };
}

/// A line of output: to stdout, or to the text being captured on this thread.
pub fn emit(line: String) {
    match SINK.with(|s| s.borrow().clone()) {
        Some(sink) => {
            let mut text = sink.lock().unwrap();
            text.push_str(&line);
            text.push('\n');
        }
        None => println!("{line}"),
    }
}

/// `print(f"...")`.
macro_rules! out {
    ($($arg:tt)*) => {
        $crate::scripts::modelfight::emit(format!($($arg)*))
    };
}
pub(crate) use out;

/// What `f` prints (on this thread and on those it starts with [`under`] and [`on_all_cores`]), beside what it returns.
pub fn capture<T>(f: impl FnOnce() -> T) -> (T, String) {
    let sink = Arc::new(Mutex::new(String::new()));
    let before = SINK.with(|s| s.replace(Some(sink.clone())));
    let result = f();
    SINK.with(|s| *s.borrow_mut() = before);
    let text = sink.lock().unwrap().clone();
    (result, text)
}

/// `logging.disable(logging.CRITICAL)`, on this thread.
pub fn quiet() {
    logging::disable(Some(logging::Level::Critical));
}

/// A script's `main`: what went wrong (the original's exception) on stderr and 1, else 0.
pub fn exit_code(result: Result<(), String>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

/// `int(text)` of a command line argument.
pub fn int(text: &str) -> Result<i64, String> {
    text.trim().parse().map_err(|_| format!("ValueError: invalid literal for int() with base 10: {}", base::str_repr(text)))
}

/// `float(text)` of a command line argument.
pub fn float(text: &str) -> Result<f64, String> {
    let t = text.trim();
    if t.is_empty() || t.contains('_') {
        return Err(format!("ValueError: could not convert string to float: {}", base::str_repr(text)));
    }
    t.parse().map_err(|_| format!("ValueError: could not convert string to float: {}", base::str_repr(text)))
}

// ---------------------------------------------------------------------------------------------------------------------
// the original values, as the scripts print them

/// A the original value a script returns, tallies or prints: `repr` and `str` are the original's.
#[derive(Clone, Debug, PartialEq)]
pub enum Datum {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Tuple(Vec<Datum>),
    List(Vec<Datum>),
    /// a dict (or a Counter), in insertion order
    Dict(Vec<(Datum, Datum)>),
}

impl Datum {
    pub fn str(s: &str) -> Datum {
        Datum::Str(s.to_string())
    }

    /// A tuple of ints (a deck as the scripts write it).
    pub fn ints(items: &[i64]) -> Datum {
        Datum::Tuple(items.iter().map(|n| Datum::Int(*n)).collect())
    }

    pub fn repr(&self) -> String {
        match self {
            Datum::None => "None".into(),
            Datum::Bool(b) => if *b { "True".into() } else { "False".into() },
            Datum::Int(n) => n.to_string(),
            Datum::Float(x) => base::float_repr(*x),
            Datum::Str(s) => base::str_repr(s),
            Datum::Tuple(items) if items.len() == 1 => format!("({},)", items[0].repr()),
            Datum::Tuple(items) => format!("({})", items.iter().map(Datum::repr).collect::<Vec<_>>().join(", ")),
            Datum::List(items) => format!("[{}]", items.iter().map(Datum::repr).collect::<Vec<_>>().join(", ")),
            Datum::Dict(items) => format!("{{{}}}", items.iter().map(|(k, v)| format!("{}: {}", k.repr(), v.repr())).collect::<Vec<_>>().join(", ")),
        }
    }

    /// `str(x)`: a string as it is, anything else its repr.
    pub fn text(&self) -> String {
        match self {
            Datum::Str(s) => s.clone(),
            other => other.repr(),
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Datum::None => false,
            Datum::Bool(b) => *b,
            Datum::Int(n) => *n != 0,
            Datum::Float(x) => *x != 0.0,
            Datum::Str(s) => !s.is_empty(),
            Datum::Tuple(items) | Datum::List(items) => !items.is_empty(),
            Datum::Dict(items) => !items.is_empty(),
        }
    }

    /// An int (a bool as 0 or 1).
    pub fn int(&self) -> i64 {
        match self {
            Datum::Bool(b) => *b as i64,
            Datum::Int(n) => *n,
            other => panic!("TypeError: not an int: {}", other.repr()),
        }
    }

    pub fn float(&self) -> f64 {
        match self {
            Datum::Float(x) => *x,
            other => other.int() as f64,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Datum::Str(s) => s,
            other => panic!("TypeError: not a str: {}", other.repr()),
        }
    }

    /// `x[i]` of a tuple or a list.
    pub fn at(&self, i: usize) -> &Datum {
        match self {
            Datum::Tuple(items) | Datum::List(items) => &items[i],
            other => panic!("TypeError: not a sequence: {}", other.repr()),
        }
    }

    /// `x[:keep]` of a tuple (None: all of it).
    pub fn head(&self, keep: Option<usize>) -> Datum {
        match (self, keep) {
            (Datum::Tuple(items), Some(n)) => Datum::Tuple(items[..n.min(items.len())].to_vec()),
            _ => self.clone(),
        }
    }

    /// `d[key]` of a dict.
    pub fn get(&self, key: &Datum) -> Option<&Datum> {
        match self {
            Datum::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// `collections.Counter(keys)`: how often each key came, in the order they first did.
pub fn counter(keys: impl IntoIterator<Item = Datum>) -> Datum {
    let mut rows: Vec<(Datum, Datum)> = Vec::new();
    for key in keys {
        match rows.iter_mut().find(|(k, _)| *k == key) {
            Some((_, Datum::Int(n))) => *n += 1,
            _ => rows.push((key, Datum::Int(1))),
        }
    }
    Datum::Dict(rows)
}

// ---------------------------------------------------------------------------------------------------------------------
// The fight's pieces

/// (id, name, type, cost, text, target) of a card.
pub type Face = (String, String, String, i64, String, String);

/// A [`Face`] from its parts.
pub fn face(id: &str, name: &str, ctype: &str, cost: i64, text: &str, target: &str) -> Face {
    (id.to_string(), name.to_string(), ctype.to_string(), cost, text.to_string(), target.to_string())
}

/// `{"can_play": False, "unplayable_reason": "HasUnplayableKeyword"}`
pub fn UNPLAYABLE() -> Vec<(&'static str, Value)> {
    vec![("can_play", json!(false)), ("unplayable_reason", json!("HasUnplayableKeyword"))]
}

/// tests/fake_game.rs's `card`: a card as the game lists it in the hand (`extra`: more keys, or other values for these).
pub fn card(index: i64, cid: &str, name: &str, ctype: &str, cost: &str, desc: &str, target: &str, extra: &[(&str, Value)]) -> Value {
    let mut c = Map::new();
    c.insert("index".into(), json!(index));
    c.insert("id".into(), json!(cid));
    c.insert("name".into(), json!(name));
    c.insert("type".into(), json!(ctype));
    c.insert("cost".into(), json!(cost));
    c.insert("star_cost".into(), Value::Null);
    c.insert("description".into(), json!(desc));
    c.insert("target_type".into(), json!(target));
    c.insert("can_play".into(), json!(true));
    c.insert("is_upgraded".into(), json!(false));
    c.insert("rarity".into(), json!("Common"));
    for (k, v) in extra {
        c.insert(k.to_string(), v.clone());
    }
    Value::Object(c)
}

/// `card(i, *face, **extra)`.
pub fn card_of(index: i64, f: &Face, extra: &[(&str, Value)]) -> Value {
    card(index, &f.0, &f.1, &f.2, &f.3.to_string(), &f.4, &f.5, extra)
}

/// A deck of counts[i] cards of kinds[i], shuffled.
pub fn shuffled(rng: &mut Random, kinds: &str, counts: &[i64]) -> Vec<char> {
    let mut deck: Vec<char> = kinds.chars().zip(counts).flat_map(|(k, n)| (0..*n).map(move |_| k)).collect();
    rng.shuffle(&mut deck);
    deck
}

/// Draw a hand, shuffling the discard pile into the (emptied) draw pile when it runs out. (the original's default size: 5.)
pub fn deal(draw: &mut Vec<char>, disc: &mut Vec<char>, rng: &mut Random, size: i64) -> Vec<char> {
    let mut hand = Vec::new();
    for _ in 0..size {
        if draw.is_empty() {
            *draw = std::mem::take(disc);
            rng.shuffle(draw);
        }
        if let Some(c) = draw.pop() {
            hand.push(c);
        }
    }
    hand
}

/// The hand as the game lists it; `face(kind)` is (id, name, type, cost, text, target) for a card kind. (the original's default `dead`: "F".)
pub fn hand_cards(hand: &[char], face: &dyn Fn(char) -> Face, dead: &str, extra: &[(&str, Value)]) -> Vec<Value> {
    hand.iter()
        .enumerate()
        .map(|(i, c)| {
            let mut more = if dead.contains(*c) { UNPLAYABLE() } else { Vec::new() };
            more.extend(extra.iter().map(|(k, v)| (*k, v.clone())));
            card_of(i as i64, &face(*c), &more)
        })
        .collect()
}

/// `pile(kinds)` for game_state, from `face` as in hand_cards.
pub fn piles<'a>(face: &'a dyn Fn(char) -> Face) -> impl Fn(&[char]) -> Vec<Value> + 'a {
    move |cs: &[char]| {
        cs.iter()
            .map(|c| {
                let f = face(*c);
                json!({"name": f.1, "cost": f.3.to_string(), "description": f.4})
            })
            .collect()
    }
}

/// The keyword arguments of [`game_state`].
#[derive(Clone, Debug, Default)]
pub struct GameStateArgs {
    pub hp: i64,
    pub maxhp: i64,
    pub block: i64,
    pub energy: i64,
    pub status: Vec<Value>,
    /// `**extra`: more keys of the state
    pub extra: Vec<(&'static str, Value)>,
}

/// The combat state the bot reads; `pile(kinds)` lists a pile's cards.
pub fn game_state(kind: &str, run: Value, r: i64, enemies: Vec<Value>, cards: Vec<Value>, pile: &dyn Fn(&[char]) -> Vec<Value>, draw: &[char], disc: &[char], args: GameStateArgs) -> Value {
    let mut state = json!({
        "state_type": kind, "run": run,
        "battle": {"round": r, "turn": "player", "is_play_phase": true, "enemies": enemies},
        "player": {"hp": args.hp, "max_hp": args.maxhp, "block": args.block, "energy": args.energy, "hand": cards, "potions": [], "status": args.status,
                   "draw_pile": pile(draw), "discard_pile": pile(disc), "draw_pile_count": draw.len(), "discard_pile_count": disc.len(), "exhaust_pile_count": 0},
    });
    for (k, v) in args.extra {
        state[k] = v;
    }
    state
}

/// Powers as the game lists them, from (id, amount) pairs; those at 0 are left out.
pub fn status(powers: &[(&str, i64)]) -> Vec<Value> {
    powers.iter().filter(|(_, n)| *n != 0).map(|(p, n)| json!({"id": p, "amount": n})).collect()
}

/// An attack intent, labelled as the game does. (the original's default `hits`: 1.)
pub fn attack(d: i64, hits: i64) -> Value {
    json!({"type": "Attack", "label": if hits > 1 { format!("{d}x{hits}") } else { d.to_string() }})
}

/// The planner's next play of a turn, (index, card kind, action), or None when it ends the turn.
/// (the original's `turn` went on until then, until `done()` or for `most` plays, 12 by default: the fights write that loop out.)
pub fn turn(state: &Value, hand: &[char]) -> Option<(usize, char, Action)> {
    let act = combat::choose_card(state, None)?;
    let i = act.1["card_index"].int() as usize;
    Some((i, hand[i], act))
}

/// d damage to an enemy, through its Block first.
pub fn hit(hp: &mut i64, block: &mut i64, d: i64) {
    let soak = d.min(*block);
    *block -= soak;
    *hp = 0.max(*hp - (d - soak));
}

/// (our Block left, damage through it) when d damage meets it.
pub fn through(block: i64, d: i64) -> (i64, i64) {
    let soak = block.min(d);
    (block - soak, d - soak)
}

/// `act[1].get("target")`: the enemy the planner named, if it named one.
pub fn aimed(act: &Action) -> Option<&str> {
    act.1["target"].as_str()
}

/// `print`'s text of `act[1].get("target")`.
pub fn aimed_text(act: &Action) -> String {
    aimed(act).map(str::to_string).unwrap_or_else(|| "None".to_string())
}

/// The enemy a planned card is aimed at, or the first one alive when it named none (or a dead one). `foes`: (id, HP) of each, in order.
pub fn target(act: &Action, foes: &[(&str, i64)]) -> String {
    if let Some(t) = aimed(act) {
        let hp = foes.iter().find(|(id, _)| *id == t).unwrap_or_else(|| panic!("KeyError: {}", base::str_repr(t))).1;
        if hp > 0 {
            return t.to_string();
        }
    }
    foes.iter().find(|(_, hp)| *hp > 0).expect("StopIteration").0.to_string()
}

// ---------------------------------------------------------------------------------------------------------------------
// NAME=VALUE

/// The constants combat.rs imported from battle.rs by name ...
pub const COMBAT_IMPORTS: [&str; 9] = ["BEYOND_REACH", "ENERGY_DRAIN", "LETHAL_PENALTY", "RAGE_BLOCK", "RAGE_COST", "REATTACH_REACH", "SLIP_WORTH", "WIN_BONUS", "THREAT_FLOOR"];
/// ... and those of them the planner (combat.rs) reads. (ENERGY_DRAIN is read by both modules: a patch of either sets it for both.)
pub const COMBAT_READS: [&str; 4] = ["ENERGY_DRAIN", "LETHAL_PENALTY", "RAGE_COST", "WIN_BONUS"];
/// ... and those battle.rs never reads itself: `battle.NAME = value` changed nothing in the original.
pub const BATTLE_READS_NOT: [&str; 3] = ["LETHAL_PENALTY", "RAGE_COST", "WIN_BONUS"];

fn nothing(name: &str, module: &str, why: &str) {
    eprintln!("note: {name} set on {module} changes nothing, as in the original ({why})");
}

/// `hasattr(battle, name)`, of its constants.
pub fn in_battle(name: &str) -> bool {
    battle::TUNABLES.contains(&name)
}

/// `hasattr(combat, name)`, of its constants (its own and those it imported from battle.rs).
pub fn in_combat(name: &str) -> bool {
    combat::TUNABLES.contains(&name) || COMBAT_IMPORTS.contains(&name)
}

/// `battle.NAME = value` on this thread from now on: what battle.rs reads changes, not what only the planner reads.
pub fn set_battle(name: &str, value: f64) -> Result<(), String> {
    if !in_battle(name) {
        return Err(format!("{name} is not a constant of battle.rs that can be set by name"));
    }
    if BATTLE_READS_NOT.contains(&name) {
        nothing(name, "battle", "only the planner reads it, by combat's own name for it");
    } else {
        battle::set_tunable(name, value);
    }
    Ok(())
}

/// `combat.NAME = value` on this thread from now on: what the planner (combat.rs) reads changes, not what battle.rs reads.
pub fn set_combat(name: &str, value: f64) -> Result<(), String> {
    if combat::TUNABLES.contains(&name) {
        combat::set_tunable(name, value);
    } else if COMBAT_READS.contains(&name) {
        battle::set_tunable(name, value);
    } else if in_battle(name) {
        nothing(name, "combat", "battle reads it, by its own name for it");
    } else {
        return Err(format!("{name} is not a constant of combat.rs that can be set by name"));
    }
    Ok(())
}

/// demonfight.rs's `setattr(closing if hasattr(closing, k) else combat, k, v)`.
pub fn set_closing_or_combat(name: &str, value: f64) -> Result<(), String> {
    if closing::set_tunable(name, value) {
        return Ok(());
    }
    set_combat(name, value).map_err(|_| format!("{name} is not a constant of closing.rs or combat.rs that can be set by name"))
}

/// Set one constant of battle.rs, combat.rs or triggers.rs (NAME=VALUE on the command line) on this thread from now on.
pub fn setting(name: &str, value: f64) -> Result<(), String> {
    if in_battle(name) {
        set_battle(name, value)
    } else if in_combat(name) {
        set_combat(name, value)
    } else if triggers::set_tunable(name, value) {
        Ok(())
    } else {
        Err(format!("{name} is not a constant of battle.rs, combat.rs or triggers.rs that can be set by name"))
    }
}

/// A constant as `setting`, or a setting of learn.rs's knobs (K.name=VALUE).
pub fn knob_setting(name: &str, value: f64) -> Result<(), String> {
    match name.strip_prefix("K.") {
        Some(knob) => {
            learn::set_knob(knob, value);
            Ok(())
        }
        None => setting(name, value),
    }
}

/// S.NAME=VALUE: a constant of fight/fool/sadness.rs.
pub fn set_sadness(name: &str, value: f64) -> Result<(), String> {
    if sadness::set_tunable(name, value) {
        Ok(())
    } else {
        Err(format!("{name} is not a constant of fight/fool/sadness.rs that can be set by name"))
    }
}

/// Every NAME=VALUE of an arm ("+" between them) set on this thread, on debuff_worth 2.0 (the live sampler plays 2.0 and 3.0).
/// (the original's default `setting`: knob_setting. To have them for a while only: `under(|| patched(arm, &knob_setting), || ...)`.)
pub fn patched(arm: &str, setting: &dyn Fn(&str, f64) -> Result<(), String>) -> Result<(), String> {
    let mut settings = Vec::new();
    for p in arm.split('+').filter(|p| p.contains('=')) {
        let parts: Vec<&str> = p.split('=').collect();
        settings.push((parts[0], float(parts[1])?));
    }
    learn::set_knob("debuff_worth", 2.0);
    for (name, value) in settings {
        setting(name, value)?;
    }
    Ok(())
}

/// `name, value = part.split("=")`.
pub fn name_value(part: &str) -> Result<(&str, &str), String> {
    let parts: Vec<&str> = part.split('=').collect();
    match parts[..] {
        [name, value] => Ok((name, value)),
        _ => Err(format!("ValueError: NAME=VALUE expected, not {}", base::str_repr(part))),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Threads

const STACK: usize = 256 << 20;

/// `with patch: body`: `body` under the settings `patch` makes, on a thread of its own, so that they end with it.
/// The thread starts from the constants, tables and knobs as written (not from this thread's), prints where this one does,
/// and logs nothing.
pub fn under<R: Send>(patch: impl FnOnce() -> Result<(), String> + Send, body: impl FnOnce() -> R + Send) -> Result<R, String> {
    let sink = SINK.with(|s| s.borrow().clone());
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new().stack_size(STACK).spawn_scoped(scope, move || {
            SINK.with(|s| *s.borrow_mut() = sink);
            quiet();
            patch()?;
            Ok(body())
        });
        match worker.expect("a thread for the fight").join() {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    })
}

/// `pool.map(job, jobs)` on at most `most` threads (the original's default: 12), `setup` first in each of them: the settings are
/// per thread, so whatever a job needs set (constants, tables, knobs) it sets itself or `setup` does. The threads print
/// where this one does and log nothing.
pub fn on_all_cores<J: Sync, R: Send>(setup: &(dyn Fn() + Sync), job: &(dyn Fn(&J) -> R + Sync), jobs: &[J], most: usize) -> Vec<R> {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    let workers = most.min(cores).min(jobs.len()).max(1);
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<R>>> = jobs.iter().map(|_| Mutex::new(None)).collect();
    let sink = SINK.with(|s| s.borrow().clone());
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let sink = sink.clone();
                let (next, results) = (&next, &results);
                std::thread::Builder::new()
                    .stack_size(STACK)
                    .spawn_scoped(scope, move || {
                        SINK.with(|s| *s.borrow_mut() = sink);
                        quiet();
                        setup();
                        loop {
                            let i = next.fetch_add(1, Ordering::SeqCst);
                            if i >= jobs.len() {
                                break;
                            }
                            let result = job(&jobs[i]);
                            *results[i].lock().unwrap() = Some(result);
                        }
                    })
                    .expect("a thread for the jobs")
            })
            .collect();
        for handle in handles {
            if let Err(panic) = handle.join() {
                std::panic::resume_unwind(panic);
            }
        }
    });
    results.into_iter().map(|r| r.into_inner().unwrap().expect("every job done")).collect()
}

/// The `setup` of scripts that need none.
pub fn no_setup() {}

// ---------------------------------------------------------------------------------------------------------------------
// Tables

/// n fights of a deck, a seed for each: every arm plays the same shuffles.
pub fn played<S: ?Sized>(fight: impl Fn(&S, &mut Random) -> Datum, spec: &S, n: i64, seed: i64) -> Vec<Datum> {
    quiet();
    (0..n).map(|j| fight(spec, &mut Random::new((seed * 10007 + j) as i128))).collect()
}

/// `sum(items) / n`, the items ints, bools or floats as the original adds them.
pub fn mean_of<'a>(items: impl IntoIterator<Item = &'a Datum>, n: usize) -> f64 {
    let mut total = base::Sum::new(0.0);
    for x in items {
        match x {
            Datum::Float(f) => total.add_f(*f),
            other => total.add_i(other.int()),
        }
    }
    total.get() / n as f64
}

pub fn mean(rows: &[Datum], i: usize) -> f64 {
    mean_of(rows.iter().map(|r| r.at(i)), rows.len())
}

/// The rows' tallies of what died first (column i), added up and sorted.
pub fn firsts(rows: &[Datum], i: usize) -> Datum {
    let mut total: Vec<(String, i64)> = Vec::new();
    for o in rows {
        let Datum::Dict(tally) = o.at(i) else { panic!("TypeError: not a Counter: {}", o.at(i).repr()) };
        for (k, n) in tally {
            match total.iter_mut().find(|(key, _)| key == k.as_str()) {
                Some((_, sum)) => *sum += n.int(),
                None => total.push((k.as_str().to_string(), n.int())),
            }
        }
    }
    total.retain(|(_, n)| *n > 0); // (Counter + Counter keeps the positive counts)
    total.sort_by(|a, b| a.0.cmp(&b.0));
    Datum::Dict(total.into_iter().map(|(k, n)| (Datum::Str(k), Datum::Int(n))).collect())
}

/// What `run(arm, deck, fights, seed)` gives: a row of the table (its first two columns the arm and the deck).
pub type Run<'a> = &'a (dyn Fn(&str, &[i64], i64, i64) -> Result<Datum, String> + Sync);

/// `[FIGHTS] [ARM ...]`: run(arm, deck, FIGHTS, 900 + its place) for every arm and deck: (the results, [(arm, its rows)]).
pub fn table(args: &[String], run: Run, decks: &[Vec<i64>], arms: &[&str]) -> Result<(Vec<Datum>, Vec<(String, Vec<Datum>)>), String> {
    let n = if !args.is_empty() { int(&args[0])? } else { 100 };
    let arms: Vec<String> = if args.len() > 1 { args[1..].to_vec() } else { arms.iter().map(|a| a.to_string()).collect() };
    let jobs: Vec<(String, Vec<i64>, i64, i64)> = arms.iter().flat_map(|arm| decks.iter().enumerate().map(move |(i, dk)| (arm.clone(), dk.clone(), n, 900 + i as i64))).collect();
    let out = on_all_cores(&no_setup, &|(arm, dk, n, seed): &(String, Vec<i64>, i64, i64)| run(arm, dk, *n, *seed), &jobs, 12);
    let out: Vec<Datum> = out.into_iter().collect::<Result<_, _>>()?;
    let by_arm = arms.iter().map(|arm| (arm.clone(), out.iter().filter(|o| o.at(0).as_str() == arm).cloned().collect())).collect();
    Ok((out, by_arm))
}

/// `trace [SEED] [DECK] [ARM]`: (seed, deck, arm), by default 1, decks[1] and "on". (`args`: the command line with `trace` first.)
pub fn trace_args(args: &[String], decks: &[Vec<i64>]) -> Result<(i64, Vec<i64>, String), String> {
    let a = &args[1..];
    let seed = if !a.is_empty() { int(&a[0])? } else { 1 };
    let deck = if a.len() > 1 { index(decks, int(&a[1])?)?.clone() } else { decks[1].clone() };
    Ok((seed, deck, if a.len() > 2 { a[2].clone() } else { "on".to_string() }))
}

/// `items[i]` as the original indexes (from the end when negative).
pub fn index<T>(items: &[T], i: i64) -> Result<&T, String> {
    let at = if i < 0 { i + items.len() as i64 } else { i };
    if at < 0 || at >= items.len() as i64 {
        return Err("IndexError: list index out of range".to_string());
    }
    Ok(&items[at as usize])
}

/// (seed, {arm: fight[:keep]}) for the 200 fights of deck i, every arm on the same shuffles: `fight(arm, rng)` plays one
/// under the arm's patches (with [`under`]).
pub fn flips(i: i64, arms: &[String], keep: Option<usize>, fight: &dyn Fn(&str, &mut Random) -> Result<Datum, String>) -> Result<Vec<(i64, Datum)>, String> {
    let mut out = Vec::new();
    for j in 0..200 {
        let seed = (900 + i) * 10007 + j;
        let mut res: Vec<(Datum, Datum)> = Vec::new();
        for arm in arms {
            let row = fight(arm, &mut Random::new(seed as i128))?.head(keep);
            match res.iter_mut().find(|(k, _)| k.as_str() == arm) {
                Some((_, v)) => *v = row,
                None => res.push((Datum::str(arm), row)),
            }
        }
        out.push((seed, Datum::Dict(res)));
    }
    Ok(out)
}

/// `len({v[0] for v in res.values()}) > 1`: the arms did not all end the same way.
pub fn differ(res: &Datum) -> bool {
    let Datum::Dict(rows) = res else { return false };
    rows.iter().any(|(_, v)| v.at(0).int() != rows[0].1.at(0).int())
}

/// The text a script printed, against the original's (tests).
#[cfg(test)]
pub fn golden(main: fn(Vec<String>) -> i32, args: &[&str], expected: &str) {
    let (code, text) = capture(|| main(args.iter().map(|a| a.to_string()).collect()));
    assert_eq!(code, 0, "{args:?}");
    if text != expected {
        for (n, (got, want)) in text.lines().zip(expected.lines()).enumerate() {
            assert_eq!(got, want, "{args:?}: line {}", n + 1);
        }
        assert_eq!(text.lines().count(), expected.lines().count(), "{args:?}: lines");
        assert_eq!(text, expected, "{args:?}");
    }
}

/// The part of a golden file under `$ name ARGS` (up to the next `$ ` line).
#[cfg(test)]
pub fn section<'a>(file: &'a str, command: &str) -> &'a str {
    let head = format!("$ {command}\n");
    let start = file.find(&head).unwrap_or_else(|| panic!("no {command:?} in the golden file")) + head.len();
    let rest = &file[start..];
    match rest.find("\n$ ") {
        Some(end) => &rest[..end + 1],
        None => rest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reprs() {
        let row = Datum::Tuple(vec![Datum::Bool(true), Datum::Int(3), Datum::None, Datum::Tuple(vec![Datum::str("ROCKET_0"), Datum::Float(2.0)]), Datum::Dict(vec![(Datum::Int(1), Datum::Float(0.5))])]);
        assert_eq!(row.repr(), "(True, 3, None, ('ROCKET_0', 2.0), {1: 0.5})");
        assert_eq!(format!("{:3.0} {:3.0} {:.0}", 0.5, 1.5, 2.5), "  0   2 2");
        assert_eq!(firsts(&[Datum::Tuple(vec![counter([Datum::str("b"), Datum::str("a"), Datum::str("b")])]), Datum::Tuple(vec![counter([Datum::str("a")])])], 0).repr(), "{'a': 2, 'b': 2}");
        assert_eq!(mean(&[Datum::Tuple(vec![Datum::Bool(true)]), Datum::Tuple(vec![Datum::Bool(false)]), Datum::Tuple(vec![Datum::Int(1)])], 0), 2.0 / 3.0);
    }

    #[test]
    fn two_bindings() {
        under(
            || {
                set_battle("RAGE_COST", 9.0)?; // (only combat reads it: nothing)
                set_combat("THREAT_FLOOR", 9.0)?; // (only battle reads it: nothing)
                set_combat("RAGE_COST", 0.5)?;
                setting("HP_BASE", 2.0)?;
                setting("ETHEREAL_KEEP", 0.5)?;
                assert!(setting("NO_SUCH", 1.0).is_err() && set_closing_or_combat("NO_SUCH", 1.0).is_err());
                set_closing_or_combat("CLOSE_REACH", 0.0)
            },
            || assert_eq!((battle::RAGE_COST.get(), battle::THREAT_FLOOR.get(), combat::HP_BASE.get(), triggers::ETHEREAL_KEEP.get(), closing::CLOSE_REACH.get()), (0.5, 6.0, 2.0, 0.5, 0.0)),
        )
        .unwrap();
        assert_eq!((battle::RAGE_COST.get(), combat::HP_BASE.get()), (0.3, 1.5));
    }

    #[test]
    fn workers() {
        let (out, text) = capture(|| {
            on_all_cores(
                &no_setup,
                &|j: &i64| {
                    out!("job");
                    j * 2
                },
                &[1, 2, 3, 4, 5],
                3,
            )
        });
        assert_eq!(out, vec![2, 4, 6, 8, 10]);
        assert_eq!(text, "job\n".repeat(5));
    }
}
