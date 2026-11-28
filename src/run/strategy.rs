//! Out-of-combat decisions: map, rewards, shop, rest sites, events, selections. Every handler returns an ordered list of
//! candidate actions; the main loop takes the first one that the game accepts, so a handler can list fallbacks.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::LazyLock;

use indexmap::IndexMap;
use serde_json::{json, Value};

pub use crate::cards::{_card_key, is_basic, is_junk, kind as _kind, parse as _parse};
pub use crate::fight::{afflictions, handpick};
use crate::learn::CardStats;
pub use crate::learn::{knob, model_shift};
use crate::logging::{Level, Logger};
use crate::base::{self, JsonExt};
pub use crate::run::decks::{_blocks, _hitters, _punch, _stars, deck_fit, BIG_DECK, SKIP_THRESHOLD}; // (what a card is worth in this deck: its own module)
pub use crate::run::events::{_event_option_value, event}; // (events live in their own module)
pub use crate::run::potions::{boss_floors, boss_worth, saving};
pub use crate::run::relicworth::worth as relic_worth;
pub use crate::Action;

static LOG: Logger = Logger::new("sts2bot.strategy");

pub const POTION_SWAP: f64 = 6.0; // how much more a potion on offer must be worth at a boss than the least of a full belt to take its place

crate::tunables! {
    pub ELITE_FLOOR: f64 = 0.35; // share of Max HP under which an elite is never chosen while there is another way on
    /// The first game's Emerald Key (user, 2026-11-28: "focus even more to get key"; 0: all of this off): from EMERALD_HOLD of
    /// Max HP up the route keeps the burning elite in reach, whatever the path values say, taking the best way through it, and
    /// steps into it from EMERALD_GO; below that the values decide. In act 3 (the last chance) from EMERALD_LAST_HOLD and
    /// EMERALD_LAST. Met from 85% up it had killed 2 of 71 in act 1 and 1 of 15 in act 2, yet the values left it behind from
    /// full HP: they count a death again at every fight that follows one.
    pub EMERALD_HOLD: f64 = 0.5;
    pub EMERALD_GO: f64 = 0.8;
    pub EMERALD_LAST_HOLD: f64 = 0.4;
    pub EMERALD_LAST: f64 = 0.5;
    /// ... a rest site with the burning elite next heals under this share
    pub EMERALD_REST: f64 = 0.9;
    /// ... and from this share up one Recalls (the Ruby Key) rather than rest, unless the boss or the burning elite is next
    /// (0: Recall only over an upgrade)
    pub RUBY_NOW: f64 = 0.7;
}

/// `_where(state)`: (the act, the floor) as the state gives them (None for one that is missing).
pub type Where = (Option<i64>, Option<i64>);

/// `Memory.upgrade_gain`: callable(card) -> what its upgrade adds, or None if unknown.
pub type UpgradeGain = Rc<dyn Fn(&Value) -> Option<f64>>;

/// The few things the game state does not tell us.
#[derive(Clone)]
pub struct Memory {
    pub deck_size: i64,
    pub deck: Vec<Value>,           // our cards as last seen at the start of a fight, plus picks since
    pub max_energy: i64,            // energy a turn, as the last fight began with
    pub picks: Vec<(i64, String)>,  // (floor, card name) for every card reward taken this run
    pub card_stats: Option<Rc<RefCell<CardStats>>>, // learn.CardStats: how cards have done in our own runs (the bot's own, shared)
    pub card_skips: HashMap<Where, i64>, // (act, floor) -> skipped card rewards
    pub select_key: String,
    pub logged_select: String,      // the "choose a card" screen last written to the log
    pub logged_shop: String,        // the shop shelf last written to the log
    pub selected: BTreeSet<i64>,
    pub runs_finished: i64,
    pub character_selected: bool,
    pub boss_next: bool,            // the room we are walking into leads straight to the boss
    pub event_id: String,           // the event on screen (its id), to tell one event from the next
    pub event_hp0: i64,             // our HP when that event began: what it has cost us so far
    pub map_nodes: IndexMap<(i64, i64), Value>, // the act's map as last seen: (col, row) -> node
    pub map_at: Option<(i64, i64)>, // the node we walked to (the original's `()`: None)
    pub upgrade_gain: Option<UpgradeGain>, // callable(card) -> what its upgrade adds, or None if unknown
    pub over_waits: i64,            // looks at an end-of-run summary that is still animating
    pub max_ascension: IndexMap<String, i64>, // character id -> highest unlocked ascension
    pub ascension_waits: i64,       // times we held off embarking for the ascension level to show
    pub epochs: IndexMap<String, String>, // epoch id -> state, from the game's profile
    pub last_result: String,        // "win" or "loss": how the run on the end screen went (set when it is recorded)
    pub recent: Vec<String>,        // the characters of the last runs played, oldest first (for the rotation)
    pub timeline_done: bool,        // the Timeline has nothing more to reveal this visit
    pub timeline_stuck: BTreeSet<String>, // epochs it could not reveal (not retried)
    pub parasol: Option<(Option<i64>, Option<i64>, i64)>, // the shop Lord's Parasol is emptying, and how often we have looked at it: (act, floor, looks)
    pub potion_swaps: HashMap<Where, i64>, // (act, floor) -> potions poured away there for a better one
}

impl Default for Memory {
    fn default() -> Self {
        Memory {
            deck_size: 10,
            deck: Vec::new(),
            max_energy: 3,
            picks: Vec::new(),
            card_stats: None,
            card_skips: HashMap::new(),
            select_key: String::new(),
            logged_select: String::new(),
            logged_shop: String::new(),
            selected: BTreeSet::new(),
            runs_finished: 0,
            character_selected: false,
            boss_next: false,
            event_id: String::new(),
            event_hp0: 0,
            map_nodes: IndexMap::new(),
            map_at: None,
            upgrade_gain: None,
            over_waits: 0,
            max_ascension: IndexMap::new(),
            ascension_waits: 0,
            epochs: IndexMap::new(),
            last_result: String::new(),
            recent: Vec::new(),
            timeline_done: false,
            timeline_stuck: BTreeSet::new(),
            parasol: None,
            potion_swaps: HashMap::new(),
        }
    }
}

impl Memory {
    /// `Memory()`.
    pub fn new() -> Self {
        Memory::default()
    }

    pub fn reset_run(&mut self) {
        (self.deck_size, self.deck, self.max_energy, self.picks, self.map_nodes, self.map_at, self.over_waits) = (10, Vec::new(), 3, Vec::new(), IndexMap::new(), None, 0);
        (self.select_key, self.character_selected, self.ascension_waits) = (String::new(), false, 0);
        self.card_skips.clear();
        self.potion_swaps.clear();
        self.selected.clear();
    }
}


// ------------------------------------------------------------- test hooks
thread_local! {
    static DECK_VALUE: RefCell<Option<Rc<dyn Fn(&Value) -> f64>>> = const { RefCell::new(None) };
    static REWARD_BAR: RefCell<Option<Rc<dyn Fn(i64, Option<i64>) -> f64>>> = const { RefCell::new(None) };
    static CARD_SELECT: RefCell<Option<Rc<dyn Fn(&Value, &mut Memory) -> Vec<Action>>>> = const { RefCell::new(None) };
    static UNKNOWN_COST_NOW: RefCell<Option<IndexMap<i64, f64>>> = const { RefCell::new(None) };
    static SESSION: Cell<Option<i64>> = const { Cell::new(None) };
}

/// Puts the earlier `deck_value` back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct DeckValueGuard(Option<Rc<dyn Fn(&Value) -> f64>>);

impl Drop for DeckValueGuard {
    fn drop(&mut self) {
        DECK_VALUE.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(strategy, "deck_value", f)`: `f` rates every card this module asks about (not those decks.rs or the events
/// ask about), on this thread, until the guard is dropped.
pub fn with_deck_value_hook(f: impl Fn(&Value) -> f64 + 'static) -> DeckValueGuard {
    DeckValueGuard(DECK_VALUE.with(|h| h.borrow_mut().replace(Rc::new(f))))
}

/// `cards::deck_value`, as this module reads it (see [`with_deck_value_hook`]).
pub fn deck_value(card: &Value) -> f64 {
    match DECK_VALUE.with(|h| h.borrow().clone()) {
        Some(f) => f(card),
        None => crate::cards::deck_value(card),
    }
}

/// Puts the earlier `reward_bar` back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct RewardBarGuard(Option<Rc<dyn Fn(i64, Option<i64>) -> f64>>);

impl Drop for RewardBarGuard {
    fn drop(&mut self) {
        REWARD_BAR.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(strategy, "reward_bar", f)`: on this thread, until the guard is dropped.
pub fn with_reward_bar_hook(f: impl Fn(i64, Option<i64>) -> f64 + 'static) -> RewardBarGuard {
    RewardBarGuard(REWARD_BAR.with(|h| h.borrow_mut().replace(Rc::new(f))))
}

/// `decks::reward_bar`, as this module reads it (see [`with_reward_bar_hook`]).
pub fn reward_bar(deck_size: i64, act: Option<i64>) -> f64 {
    match REWARD_BAR.with(|h| h.borrow().clone()) {
        Some(f) => f(deck_size, act),
        None => crate::run::decks::reward_bar(deck_size, act),
    }
}

/// Puts the earlier `card_select` back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct CardSelectGuard(Option<Rc<dyn Fn(&Value, &mut Memory) -> Vec<Action>>>);

impl Drop for CardSelectGuard {
    fn drop(&mut self) {
        CARD_SELECT.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(strategy, "card_select", f)`: `f` answers every `strategy::card_select` on this thread until the guard is
/// dropped (`strategy::_card_select` is the handler itself).
pub fn with_card_select_hook(f: impl Fn(&Value, &mut Memory) -> Vec<Action> + 'static) -> CardSelectGuard {
    CardSelectGuard(CARD_SELECT.with(|h| h.borrow_mut().replace(Rc::new(f))))
}

/// Puts the earlier `UNKNOWN_COST` back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct UnknownCostGuard(Option<IndexMap<i64, f64>>);

impl Drop for UnknownCostGuard {
    fn drop(&mut self) {
        UNKNOWN_COST_NOW.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(strategy, "UNKNOWN_COST", table)`: on this thread, until the guard is dropped.
pub fn with_unknown_cost(table: IndexMap<i64, f64>) -> UnknownCostGuard {
    UnknownCostGuard(UNKNOWN_COST_NOW.with(|h| h.borrow_mut().replace(table)))
}

/// `UNKNOWN_COST.get(act, 0.0)`, from the table in force.
pub fn unknown_cost(act: i64) -> f64 {
    UNKNOWN_COST_NOW.with(|h| match &*h.borrow() {
        Some(table) => table.get(&act).copied().unwrap_or(0.0),
        None => UNKNOWN_COST.get(&act).copied().unwrap_or(0.0),
    })
}

// ------------------------------------------------------------ small helpers
fn act_(name: &str, params: Value) -> Action {
    (name.to_string(), params)
}

/// `isinstance(x, int)` (a bool is one), and the int.
fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(*b as i64),
        other if other.is_int() => other.as_i64(),
        _ => None,
    }
}

/// `d.get(key, True)`, as a truth value.
fn get_or_true(d: &Value, key: &str) -> bool {
    d.get(key).is_none_or(|x| x.truthy())
}

/// `int(c.get("index", 0))`.
fn index_of(c: &Value) -> i64 {
    c["index"].int()
}

/// `(d.get("col"), d.get("row"))` where both are ints (the only keys a map's nodes have).
fn col_row(d: &Value) -> Option<(i64, i64)> {
    Some((d["col"].as_i64()?, d["row"].as_i64()?))
}

/// `tuple(c)` for a child `[col, row]`.
fn pair(c: &Value) -> Option<(i64, i64)> {
    match c.list() {
        [col, row] => Some((col.as_i64()?, row.as_i64()?)),
        _ => None,
    }
}

/// `a == b` for two JSON values as the original compares what they stand for (1 == 1.0).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// `sorted(items, key=key)` / `sorted(items, key=key, reverse=True)`: the key asked once for each item, in order; stable.
fn sorted_by<K: PartialOrd>(items: impl IntoIterator<Item = Value>, mut key: impl FnMut(&Value) -> K, reverse: bool) -> Vec<Value> {
    let mut keyed: Vec<(K, Value)> = items.into_iter().map(|c| (key(&c), c)).collect();
    let cmp = |a: &K, b: &K| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal);
    if reverse {
        keyed.sort_by(|a, b| cmp(&b.0, &a.0));
    } else {
        keyed.sort_by(|a, b| cmp(&a.0, &b.0));
    }
    keyed.into_iter().map(|t| t.1).collect()
}

/// `f"{_where(state)}"`: the tuple as the original prints it.
fn where_repr(state: &Value) -> String {
    let run = &state["run"];
    format!("({}, {})", base::repr_of(&run["act"]), base::repr_of(&run["floor"]))
}

pub fn _where(state: &Value) -> Where {
    let run = &state["run"];
    (run["act"].as_i64(), run["floor"].as_i64())
}

/// (the player, HP, Max HP).
pub fn _vitals(state: &Value) -> (&Value, i64, i64) {
    let p = &state["player"];
    (p, p["hp"].int(), 1.max(if p["max_hp"].truthy() { p["max_hp"].int() } else { 1 }))
}

/// Menu options come either as strings or as {name, enabled} objects.
pub fn _option_names(options: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for o in options.list() {
        if let Value::String(s) = o {
            out.push(s.clone());
        } else if o.is_object() && get_or_true(o, "enabled") && o["name"].truthy() {
            out.push(base::str_of(&o["name"]));
        }
    }
    out
}

// --------------------------------------------------------------------- menu
pub fn menu(state: &Value, mem: &mut Memory, character: &str) -> Vec<Action> {
    let screen = if state["menu_screen"].truthy() { state["menu_screen"].as_str().unwrap_or("?") } else { "main" };
    let names = _option_names(&state["options"]);
    let mut lower: IndexMap<String, String> = IndexMap::new();
    for n in &names {
        lower.insert(n.to_lowercase(), n.clone());
    }

    let pick = |wanted: &[&str]| -> Vec<Action> { wanted.iter().filter_map(|w| lower.get(*w)).map(|n| act_("menu_select", json!({"option": n}))).collect() };

    if screen == "main" {
        mem.timeline_done = false;
        // Unlocks only count once revealed on the Timeline (until then the game may not even offer Singleplayer): reveal them first.
        let pending = state["pending_epoch_ids"].list();
        let left = pending.iter().any(|e| !matches!(e, Value::String(s) if mem.timeline_stuck.contains(s)));
        if lower.contains_key("timeline") && left && !lower.contains_key("continue") {
            return pick(&["timeline", "singleplayer"]);
        }
        let first = pick(&["continue", "singleplayer"]);
        return if first.is_empty() { pick(&["timeline"]) } else { first };
    }
    if screen == "timeline" {
        return if mem.timeline_done { pick(&["back", "advance"]) } else { pick(&["advance", "back"]) };
    }
    if screen == "singleplayer" {
        return pick(&["standard"]);
    }
    if screen == "character_select" {
        let ids: Vec<String> = state["characters"].list().iter().filter(|c| !c["locked"].truthy()).map(|c| base::str_of(&c["id"])).collect();
        let want = choose_character(character, &ids, mem.runs_finished, Some(&mem.max_ascension), Some(&mem.epochs), Some(&mem.recent));
        let select: Vec<Action> = match &want {
            Some(w) if !w.is_empty() => vec![act_("menu_select", json!({"option": w}))],
            _ => vec![],
        };
        let embark = pick(&["confirm", "embark"]);
        if !mem.character_selected {
            return [select, embark].concat();
        }
        // Every run is played at the highest ascension the character has unlocked: a win there unlocks the next.
        let (top, now) = (as_int(&state["max_ascension"]), as_int(&state["ascension"]));
        let raise_asc: Vec<Action> = match (top, now) {
            (Some(top), Some(now)) if now != top => vec![act_("set_ascension", json!({"level": state["max_ascension"]}))],
            _ => vec![],
        };
        // The levels of a character just picked show a moment later (embarking before then starts at ascension 0): wait, not for ever.
        let wanted = want.as_deref().unwrap_or("None").to_uppercase();
        let mut known = 0;
        for (k, v) in &mem.max_ascension {
            if k.to_uppercase() == wanted {
                known = *v;
            }
        }
        let patience = if top.is_some() { 4 } else { 20 };
        let now_is_known = match &state["ascension"] {
            Value::Bool(b) => *b as i64 == known,
            Value::Number(n) => n.as_f64() == Some(known as f64),
            _ => false,
        };
        if known > 0 && raise_asc.is_empty() && !now_is_known && mem.ascension_waits < patience {
            mem.ascension_waits += 1;
            return select;
        }
        return [raise_asc, embark, select].concat();
    }
    if screen == "tutorial_prompt" {
        return pick(&["no"]);
    }
    if screen == "tutorial" {
        return pick(&["advance", "proceed"]);
    }
    if screen == "popup" {
        let first = pick(&["yes", "confirm", "ok", "continue"]);
        return if first.is_empty() { names.iter().map(|n| act_("menu_select", json!({"option": n}))).collect() } else { first };
    }
    // Timeline, settings, compendium, multiplayer screens: get out.
    pick(&["back", "advance"])
}

pub const _EARNED: [&str; 3] = ["Obtained", "ObtainedNoSlot", "Revealed"];

/// Every epoch the profile lists has been earned (False when it lists none: a game without a Timeline, or not asked yet).
pub fn timeline_complete(epochs: Option<&IndexMap<String, String>>) -> bool {
    epochs.is_some_and(|e| !e.is_empty() && e.values().all(|state| _EARNED.contains(&state.as_str())))
}

/// How many of this character's own epochs (IRONCLAD2_EPOCH, ...) are still to be earned.
pub fn missing_epochs(character: &str, epochs: Option<&IndexMap<String, String>>) -> i64 {
    let prefix = base::re::cached(&format!(r"^{}\d+_EPOCH\n?\z", base::re::escape(&character.to_uppercase())));
    epochs.map_or(0, |e| e.iter().filter(|(eid, state)| prefix.is_match(&eid.to_uppercase()) && !_EARNED.contains(&state.as_str())).count() as i64)
}

/// so a restart does not replay the same draws
static _SESSION_DRAWN: LazyLock<i64> = LazyLock::new(|| base::Random::unseeded().getrandbits(32) as i64);

/// `_SESSION`: drawn once a process; a test (or a replay) can set its own for this thread with [`set_session`].
pub fn _SESSION() -> i64 {
    SESSION.with(|s| s.get()).unwrap_or(*_SESSION_DRAWN)
}

/// `strategy._SESSION = n` on this thread (None: back to the one drawn for the process).
pub fn set_session(session: Option<i64>) {
    SESSION.with(|s| s.set(session));
}

/// A character's name as the menu lists it ("THE_SILENT", "SILENT") and as runs.jsonl keeps it ("The Silent"), alike.
pub fn _who(name: &str) -> String {
    let upper = name.to_uppercase();
    let key = crate::re!(r"[^A-Z]+").replace_all(&upper, "_");
    let key = key.trim_matches('_');
    key.strip_prefix("THE_").unwrap_or(key).to_string()
}

/// `setting` is a character id, or "auto" / "cycle" / "random" over the unlocked ones.
/// "auto" first plays the characters with a Timeline epoch of their own still to earn; among those (or everyone) one on
/// the lowest ascension unlocked: drawn at random once a run, or with `recent` (oldest first) the one played longest ago.
/// (the original's defaults: `max_ascension`, `epochs` and `recent` None.)
pub fn choose_character(
    setting: &str,
    unlocked: &[String],
    runs_finished: i64,
    max_ascension: Option<&IndexMap<String, i64>>,
    epochs: Option<&IndexMap<String, String>>,
    recent: Option<&[String]>,
) -> Option<String> {
    // The game's own "Random" tile is listed like a character, on ascension 0 for ever: never pick it.
    let mut unlocked: Vec<&String> = unlocked.iter().filter(|u| !u.to_uppercase().starts_with("RANDOM")).collect();
    if unlocked.is_empty() {
        return None;
    }
    let mut by_upper: HashMap<String, &String> = HashMap::new();
    for u in &unlocked {
        by_upper.insert(u.to_uppercase(), u);
    }
    let mut s = base_strip(setting).to_uppercase();
    if let Some(u) = by_upper.get(&s) {
        return Some((*u).clone());
    }
    if s.starts_with("AUTO:") {
        // "auto:IRONCLAD": that character first, for as long as it has epochs of its own to earn.
        let first = by_upper.get(base_strip(s.split_once(':').expect("a colon").1)).copied();
        if let Some(first) = first {
            if !first.is_empty() && missing_epochs(first, epochs) > 0 {
                return Some(first.clone());
            }
        }
        s = "AUTO".to_string();
    }
    let mut levels: HashMap<String, i64> = HashMap::new();
    for (k, v) in max_ascension.into_iter().flatten() {
        levels.insert(k.to_uppercase(), *v);
    }
    let level = |u: &String| levels.get(&u.to_uppercase()).copied().unwrap_or(0);
    if s == "AUTO" || s == "RANDOM" {
        if s == "AUTO" {
            // While any character still has an epoch of its own to earn, only those play.
            let owed: Vec<&String> = unlocked.iter().copied().filter(|u| missing_epochs(u, epochs) > 0).collect();
            if !owed.is_empty() {
                unlocked = owed;
            }
        }
        let low = unlocked.iter().map(|u| level(u)).min().expect("someone unlocked");
        let mut lowest: Vec<&String> = unlocked.iter().copied().filter(|u| level(u) == low).collect();
        if let (true, Some(recent)) = (s == "AUTO", recent.filter(|r| !r.is_empty())) {
            let mut last: HashMap<String, i64> = HashMap::new();
            for (n, c) in recent.iter().enumerate() {
                last.insert(_who(c), n as i64);
            }
            let ago = |u: &String| last.get(&_who(u)).copied().unwrap_or(-1);
            let longest = lowest.iter().map(|u| ago(u)).min().expect("someone lowest");
            lowest.retain(|u| ago(u) == longest);
        }
        let drawn = base::Random::new((_SESSION() + runs_finished) as i128).choice_index(lowest.len());
        return Some(lowest[drawn].clone());
    }
    if s == "CYCLE" {
        return Some(unlocked[base::modulo(runs_finished, unlocked.len() as i64) as usize].clone());
    }
    Some(unlocked[0].clone())
}

/// `str.strip()`: the original's blanks (those of Unicode and the four separators 0x1c-0x1f).
fn base_strip(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

// ---------------------------------------------------------------------- map
pub const _ROOM_BASE: [(&str, f64); 4] = [("Unknown", 2.2), ("Treasure", 4.0), ("Boss", 0.0), ("Ancient", 3.0)]; // (the other kinds are worked out below)
pub const EARLY_ROWS: i64 = 8; // act 1 rows where the deck is still close to the starter deck

/// (the original's defaults: `early` False, `act` None.)
pub fn _room_value(kind: &str, hp: f64, gold: i64, early: bool, act: Option<i64>) -> f64 {
    let mut v = _ROOM_BASE.iter().find(|(k, _)| *k == kind).map_or(1.0, |(_, v)| *v);
    if kind == "Elite" && early {
        v = -5.0; // a starter deck loses about 35 HP to an act 1 elite: only when there is no other way
    } else if kind == "Elite" {
        // only from a strong position
        v = if hp >= 0.9 {
            3.0
        } else if hp >= 0.7 {
            0.5
        } else {
            -6.0
        };
        if act == Some(1) && hp >= 0.7 {
            v = 3.5; // act 1 elites are cheap once the deck has a few cards, and their relics carry the run
        }
    } else if kind == "Monster" {
        v = if hp >= 0.45 { 1.5 } else { -1.0 };
    } else if kind == "RestSite" {
        v = 3.0 + (1.0 - hp) * 5.0;
    } else if kind == "Shop" {
        // (gold is worth nothing at the end of a run)
        v = if gold >= 300 {
            4.5
        } else if gold >= 150 {
            3.0
        } else if gold >= 80 {
            1.5
        } else {
            0.3
        };
    }
    v
}

/// HP a room costs on average (our runs): act -> (Monster, Elite)
pub const FIGHT_COST: [(i64, (i64, i64)); 3] = [(1, (5, 21)), (2, (12, 30)), (3, (14, 38))];
pub const LATE_ROW: i64 = 5; // from this row of acts 2 and 3 an ordinary fight costs more than the act's average, before it less
pub const LATE_COST: [(i64, (f64, f64)); 2] = [(2, (0.85, 1.2)), (3, (0.93, 1.08))];
pub const BAD_LUCK: f64 = 1.7; // how much more than average a fight costs when the draws go wrong
pub const BOSS_WORTH: f64 = 16.0; // what getting past the boss is worth, in the units of the room rewards below
// Slay the Spire 2's Spoils Map marks a node of the next act's map worth 600 Gold to whoever walks through it (map_screen tags it
// "Spoils:<type>"): the gold is spent at the shops after it (in the path's own shop value), and a little more for the rest of the run.
pub const SPOILS: &str = "Spoils:";
pub const SPOILS_GOLD: i64 = 600;
pub const SPOILS_BONUS: f64 = 3.0;
/// The first game's burning elite, which holds the Emerald Key for act 4 (games/sts1/keys.rs): the key comes first (user, 2026-11-25:
/// "focus more collecting all keys"), and in acts 1-2 its elite kills seldom (3 of 58 fights) where act 3's kill one in six. Raised
/// 2026-11-26 (user: "focus even more on getting keys"): of 87 runs since act 4 opened the Ruby came in 72, the Emerald in 44; of the
/// 53 map choices with the burning elite on offer it was taken 28 times, and some of the rest went to a hallway fight by 2-13 points.
/// Raised again the same evening ("sts1 help focus even more to collect key"): the route aims for it from each act's first row; the
/// elite HP penalties and ELITE_FLOOR still keep it from being fought badly hurt (a run that dies collects no key).
pub static EMERALD_WORTH: LazyLock<IndexMap<i64, f64>> = LazyLock::new(|| IndexMap::from([(1, 30.0), (2, 32.0), (3, 28.0)]));
pub const MAX_PATHS: i64 = 1500;
// The act 2 boss win chance by the HP it is met on, as a share of that from full HP (even HP above 85% counts there).
pub const ACT2_ODDS: [(f64, f64); 8] = [(0.0, 0.0), (0.3, 0.05), (0.5, 0.15), (0.6, 0.23), (0.7, 0.42), (0.8, 0.68), (0.9, 0.86), (1.0, 1.0)];
/// what an Unknown room costs on average, as a share of Max HP (in act 2 a fight one time in five); read with [`unknown_cost`]
pub static UNKNOWN_COST: LazyLock<IndexMap<i64, f64>> = LazyLock::new(|| IndexMap::from([(2, 0.03)]));

/// Straight lines between (x, y) points, flat beyond the ends.
pub fn _between(points: &[(f64, f64)], x: f64) -> f64 {
    if x <= points[0].0 {
        return points[0].1;
    }
    for (&(x0, y0), &(x1, y1)) in points.iter().zip(&points[1..]) {
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    points[points.len() - 1].1
}

/// `max(0.0, x)`.
fn at_least_zero(x: f64) -> f64 {
    if x > 0.0 {
        x
    } else {
        0.0
    }
}

/// Chance of beating the boss given how healthy we arrive, next to arriving at full HP. Act 1: no difference from 85%
/// up, hardly a win below 60%. Act 2: ACT2_ODDS. Act 3: the last boss is beaten from (nearly) full HP only.
/// (the original's default: `act` 1.)
pub fn boss_odds(hp_ratio: f64, act: i64) -> f64 {
    if act == 2 {
        return _between(&ACT2_ODDS, hp_ratio);
    }
    if act >= 3 {
        return if hp_ratio >= 0.95 {
            1.0
        } else if hp_ratio >= 0.8 {
            0.3 + 0.7 * (hp_ratio - 0.8) / 0.15
        } else {
            at_least_zero(0.3 * (hp_ratio - 0.5) / 0.3)
        };
    }
    if hp_ratio >= 0.85 {
        1.0
    } else if hp_ratio >= 0.6 {
        0.45 + 0.55 * (hp_ratio - 0.6) / 0.25
    } else {
        at_least_zero(0.45 * (hp_ratio - 0.3) / 0.3)
    }
}

pub const REST_HEAL: f64 = 0.3; // of Max HP
pub const HEAL_BELOW: [(i64, f64); 2] = [(1, 0.85), (2, 0.95)]; // the rest site just before a boss heals below this share of Max HP (act 2 bosses lose ground even from 85-97%)
pub const LIFT_HITTERS: i64 = 5; // cards that deal damage a deck needs before Girya's Strength beats an upgrade
pub const UPGRADE_EARLY: f64 = 4.0; // what an upgrade counts for against resting in act 1 (later acts: the `upgrade_worth` setting, see learn.CHOICES)

pub fn _upgrade_worth(act: Option<i64>) -> f64 {
    if act == Some(1) {
        UPGRADE_EARLY
    } else {
        knob("upgrade_worth")
    }
}

/// Expected worth of walking through `rooms` (type, row) in order, starting at `hp`: rewards along the way weighed
/// against the state we reach the boss in, and at each rest site whichever of resting and upgrading comes out ahead.
/// (the original's default: `harsh` 1.0. A row that is not an int is None.)
pub fn path_value<S: AsRef<str>>(rooms: &[(S, Option<i64>)], hp: f64, max_hp: i64, gold: i64, act: i64, harsh: f64) -> f64 {
    let (mut hp, mut gold) = (hp, gold);
    let max = max_hp as f64;
    let costs = FIGHT_COST.iter().find(|(a, _)| *a == act).map_or(FIGHT_COST[2].1, |(_, c)| *c);
    let late = LATE_COST.iter().find(|(a, _)| *a == act).map(|(_, c)| *c);
    let mut score = 0.0;
    for (n, (kind, row)) in rooms.iter().enumerate() {
        let (mut kind, row) = (kind.as_ref(), *row);
        if let Some(rest) = kind.strip_prefix(SPOILS) {
            (score, gold, kind) = (score + SPOILS_BONUS, gold + SPOILS_GOLD, rest);
        }
        if kind == "EmeraldElite" {
            (score, kind) = (score + EMERALD_WORTH.get(&act).copied().unwrap_or(EMERALD_WORTH[&3]), "Elite");
        }
        if kind == "Monster" || kind == "Elite" {
            let elite = kind == "Elite";
            let mut cost = if elite { costs.1 } else { costs.0 } as f64;
            if elite && act == 1 && row.is_some_and(|row| row < EARLY_ROWS) {
                cost = 32.0; // a starter deck against an elite
            }
            if elite && hp < 0.75 * max {
                // Elites met below three quarters HP end runs far more often than their average cost says.
                score -= if hp >= 0.65 * max {
                    2.5
                } else if hp >= 0.5 * max {
                    6.0
                } else {
                    14.0
                };
            }
            if elite && act >= 3 {
                score -= 6.0; // an act 3 elite ends the run one time in five whatever the HP, and its relic has one boss left
            }
            if let (false, Some(late), Some(row)) = (elite, late, row) {
                cost *= if row >= LATE_ROW { late.1 } else { late.0 };
            }
            if !elite && act >= 2 && hp < 0.3 * max {
                score -= if act >= 3 { 14.0 } else { 10.0 }; // the same for an ordinary fight met badly hurt (a run ended is 30 here)
            } else if !elite && act >= 2 && hp < 0.6 * max {
                score -= if act >= 3 { 3.0 } else { 1.5 };
            }
            hp -= cost * harsh;
            score += if elite { knob("elite_worth") } else { 1.0 };
            gold += if elite { 30 } else { 15 };
            if hp <= 0.0 {
                score -= 30.0; // this is where the run ends, on average
                hp = 1.0;
            } else if hp < 0.2 * max {
                score -= 6.0; // one bad draw from dead
            } else if hp < 0.35 * max {
                score -= 2.0;
            }
            if max < hp {
                hp = max;
            }
        } else if kind == "RestSite" {
            let onward = &rooms[n + 1..];
            let smith = _upgrade_worth(Some(act)) + path_value(onward, hp, max_hp, gold, act, harsh);
            if hp >= 0.95 * max {
                return score + smith;
            }
            let healed = hp + REST_HEAL * max;
            let rest = path_value(onward, if healed < max { healed } else { max }, max_hp, gold, act, harsh);
            return score + if rest > smith { rest } else { smith };
        } else if kind == "Treasure" {
            score += 2.0;
        } else if kind == "Shop" {
            let buys = gold as f64 / 120.0;
            score += 0.3 + if buys < 8.0 { buys } else { 8.0 }; // gold is only worth what it buys
            gold = base::modulo(gold, 75);
        } else if kind == "Unknown" {
            score += 0.9;
            let left = hp - unknown_cost(act) * max * harsh;
            hp = if left > 1.0 { left } else { 1.0 };
        } else if kind == "Ancient" {
            score += 3.0;
        } else if kind == "Boss" {
            score += BOSS_WORTH * boss_odds(hp / 1.max(max_hp) as f64, act);
        }
    }
    score
}

/// Best value over every way on from node `key` (`rooms` is the way so far, that node included).
/// (`key`: None for Memory's "no node"; `budget`: the original's one-item list, the paths still to be walked.)
pub fn best_onward(nodes: &IndexMap<(i64, i64), Value>, key: Option<(i64, i64)>, rooms: &[(String, Option<i64>)], hp: f64, max_hp: i64, gold: i64, act: i64, budget: &mut i64) -> f64 {
    let mut way: Vec<(Cow<str>, Option<i64>)> = rooms.iter().map(|(kind, row)| (Cow::Borrowed(kind.as_str()), *row)).collect();
    _onward(nodes, key, &mut way, hp, max_hp, gold, act, budget, false)
}

/// (`burning`: only ways through the burning elite: a way not yet through it goes on only to where it still lies ahead.)
fn _onward<'a>(nodes: &'a IndexMap<(i64, i64), Value>, key: Option<(i64, i64)>, rooms: &mut Vec<(Cow<'a, str>, Option<i64>)>, hp: f64, max_hp: i64, gold: i64, act: i64, budget: &mut i64, burning: bool) -> f64 {
    let node = key.and_then(|k| nodes.get(&k));
    let mut kids: Vec<(i64, i64)> = node.map_or(&[][..], |n| n["children"].list()).iter().filter_map(pair).filter(|k| nodes.contains_key(k)).collect();
    if burning && !rooms.iter().any(|(kind, _)| kind == "EmeraldElite") {
        kids.retain(|k| nodes[k]["type"] == "EmeraldElite" || _burning_after(nodes, *k, &mut HashSet::new()));
    }
    if kids.is_empty() || *budget <= 0 {
        *budget -= 1;
        // A route is also judged by how it goes when fights cost a good deal more than average.
        return 0.6 * path_value(rooms, hp, max_hp, gold, act, 1.0) + 0.4 * path_value(rooms, hp, max_hp, gold, act, BAD_LUCK);
    }
    let mut best: Option<f64> = None;
    for k in kids {
        let kind = &nodes[&k]["type"];
        rooms.push((kind.as_str().map_or_else(|| Cow::Owned(base::str_of(kind)), Cow::Borrowed), Some(k.1)));
        let v = _onward(nodes, Some(k), rooms, hp, max_hp, gold, act, budget, burning);
        rooms.pop();
        if best.is_none_or(|b| v > b) {
            best = Some(v);
        }
    }
    best.expect("a way on")
}

/// Whether the burning elite is on some way on from node `key` (`seen`: nodes already looked past).
fn _burning_after(nodes: &IndexMap<(i64, i64), Value>, key: (i64, i64), seen: &mut HashSet<(i64, i64)>) -> bool {
    nodes[&key]["children"].list().iter().filter_map(pair).any(|k| nodes.contains_key(&k) && seen.insert(k) && (nodes[&k]["type"] == "EmeraldElite" || _burning_after(nodes, k, seen)))
}

pub fn map_screen(state: &Value, mem: &mut Memory) -> Vec<Action> {
    let m = &state["map"];
    let options = m["next_options"].list();
    if options.is_empty() {
        return vec![];
    }
    let (player, hp_now, max_hp) = _vitals(state);
    let hp = hp_now as f64 / max_hp as f64;
    let gold = player["gold"].int();
    let mut nodes: IndexMap<(i64, i64), Value> = IndexMap::new();
    for n in m["nodes"].list() {
        if let Some(key) = col_row(n) {
            nodes.insert(key, n.clone());
        }
    }
    let act_raw = &state["run"]["act"];
    let act = as_int(act_raw);
    let mut budget = MAX_PATHS;
    for q in m["quests"].list() {
        // the Spoils Map's 600 Gold: a node of this act's map to walk through (the mod's `quests`)
        let Some(key) = col_row(q) else { continue };
        if q["kind"] == "spoils" && same(&q["act"], act_raw) && nodes.contains_key(&key) && !base::str_of(&nodes[&key]["type"]).starts_with(SPOILS) {
            let kind = format!("{SPOILS}{}", base::str_of(&nodes[&key]["type"]));
            nodes[&key]["type"] = json!(kind);
            crate::log_debug!(LOG, "map: the Spoils Map's gold at ({}, {})", key.0, key.1);
        }
    }

    let is_early = |row: &Value| -> bool { act == Some(1) && as_int(row).is_some_and(|row| row < EARLY_ROWS) };

    let mut scored: Vec<(f64, &Value)> = Vec::new();
    for opt in options {
        let v = match col_row(opt).filter(|key| nodes.contains_key(key)) {
            Some(key) => {
                let mut way = vec![(Cow::Owned(base::str_of(&nodes[&key]["type"])), Some(key.1))];
                _onward(&nodes, Some(key), &mut way, hp_now as f64, max_hp, gold, act.unwrap_or(1), &mut budget, false)
            }
            None => {
                let mut ahead: Option<f64> = None;
                for k in opt["leads_to"].list() {
                    let v = _room_value(&base::str_of(&k["type"]), hp, gold, is_early(&k["row"]), act);
                    if ahead.is_none_or(|a| v > a) {
                        ahead = Some(v);
                    }
                }
                _room_value(&base::str_of(&opt["type"]), hp, gold, is_early(&opt["row"]), act) + 0.9 * ahead.unwrap_or(0.0)
            }
        };
        scored.push((v, opt));
    }
    let is_elite = |o: &Value| base::str_of(&o["type"]).contains("Elite");
    if hp < ELITE_FLOOR.get() && scored.iter().any(|(_, o)| !is_elite(o)) {
        // Badly hurt, every road "dies" on paper and the values no longer tell them apart: not an elite.
        scored = scored.into_iter().map(|(v, o)| (v - if is_elite(o) { 1000.0 } else { 0.0 }, o)).collect();
    }
    if EMERALD_HOLD.get() > 0.0 {
        let (hold, go) = if act == Some(3) { (EMERALD_LAST_HOLD.get(), EMERALD_LAST.get()) } else { (EMERALD_HOLD.get(), EMERALD_GO.get()) };
        // each option's best value along a way through the burning elite, with HP enough to keep it in reach
        let through: Vec<Option<f64>> = scored
            .iter()
            .map(|(_, o)| {
                let key = col_row(o).filter(|key| nodes.contains_key(key))?;
                let burning = nodes[&key]["type"] == "EmeraldElite";
                if hp < if burning { go } else { hold } || !(burning || _burning_after(&nodes, key, &mut HashSet::new())) {
                    return None;
                }
                let mut way = vec![(Cow::Owned(base::str_of(&nodes[&key]["type"])), Some(key.1))];
                Some(_onward(&nodes, Some(key), &mut way, hp_now as f64, max_hp, gold, act.unwrap_or(1), &mut MAX_PATHS.clone(), true))
            })
            .collect();
        if through.iter().any(Option::is_some) {
            scored = scored.into_iter().zip(through).map(|((v, o), t)| (t.unwrap_or(v - 1e6), o)).collect();
        }
    }
    let mut ranked = scored.clone();
    base::sort_by_key_rev(&mut ranked, |t| t.0);
    let ranked: Vec<&Value> = ranked.into_iter().map(|(_, o)| o).collect();
    if LOG.enabled(Level::Debug) && scored.len() > 1 {
        let shown: Vec<String> = scored.iter().map(|(v, o)| format!("{}@{} {:.1}", base::str_of(&o["type"]), base::str_of(&o["col"]), v)).collect();
        crate::log_debug!(LOG, "map: {}", shown.join(", "));
    }
    let best = ranked[0];
    let leads = best["leads_to"].list();
    mem.boss_next = if !leads.is_empty() {
        leads.iter().any(|k| base::str_of(&k["type"]) == "Boss")
    } else {
        let here = col_row(best).and_then(|key| nodes.get(&key));
        here.map_or(&[][..], |n| n["children"].list()).iter().any(|c| pair(c).and_then(|k| nodes.get(&k)).is_some_and(|n| n["type"] == "Boss"))
    };
    let out = ranked.iter().map(|o| act_("choose_map_node", json!({"index": index_of(o)}))).collect();
    let at = col_row(best);
    (mem.map_nodes, mem.map_at) = (nodes, at);
    out
}

// ------------------------------------------------------------------ rewards
/// No room for a potion: the belt is full, or a relic (Sozu) forbids potions (and the shop would sell one for nothing).
pub fn _belt_full(player: &Value) -> bool {
    for relic in player["relics"].list() {
        if relic["description"].text().to_lowercase().contains("no longer obtain potions") {
            return true;
        }
    }
    player["potions"].list().len() as i64 >= slots(player)
}

/// `int(player.get("max_potion_slots") or 3)`.
fn slots(player: &Value) -> i64 {
    if player["max_potion_slots"].truthy() {
        player["max_potion_slots"].int()
    } else {
        3
    }
}

pub fn rewards(state: &Value, mem: &mut Memory) -> Vec<Action> {
    let r = &state["rewards"];
    let player = &state["player"];
    let belt_full = _belt_full(player);
    let mut skips_left = mem.card_skips.get(&_where(state)).copied().unwrap_or(0);
    let mut out: Vec<Action> = Vec::new();
    // Highest index first so earlier indices stay valid.
    for item in sorted_by(r["items"].list().iter().cloned(), |i| -index_of(i), false) {
        let kind = base::str_of(&item["type"]);
        if kind == "potion" && belt_full {
            // A potion better than the least on the belt: pour that one away (twice a room at most, so a failed discard cannot loop).
            let held = player["potions"].list();
            let worst = base::min_by_key(held.iter(), |p| boss_worth(p));
            let full = held.len() as i64 >= slots(player); // (and not Sozu, which lets none be taken)
            if let (true, Some(worst)) = (full, worst) {
                if boss_worth(&item) >= boss_worth(worst) + POTION_SWAP && mem.potion_swaps.get(&_where(state)).copied().unwrap_or(0) < 2 {
                    crate::log_debug!(LOG, "potion: {} poured away for {}", base::str_of(&worst["name"]), base::str_of(&item["potion_name"]));
                    out.push(act_("discard_potion", json!({"slot": worst["slot"].int()})));
                }
            }
            continue;
        }
        if kind == "card" && skips_left > 0 {
            skips_left -= 1;
            continue;
        }
        out.push(act_("claim_reward", json!({"index": index_of(&item)})));
    }
    out.push(act_("proceed", json!({})));
    out
}

pub fn card_reward(state: &Value, mem: &mut Memory) -> Vec<Action> {
    let cr = &state["card_reward"];
    let cards = cr["cards"].list();
    let act = as_int(&state["run"]["act"]);
    let who = state["player"]["character"].text();

    let worth = |c: &Value| -> f64 {
        let mut v = deck_value(c) + deck_fit(c, &mem.deck, act, mem.max_energy);
        if let Some(stats) = &mem.card_stats {
            v += stats.borrow_mut().shift(&who, &base::str_of(&c["name"]), Some(_kind(c)));
        }
        v += model_shift(&who, &c["name"].text());
        // The starter deck cannot kill anything quickly: an attack first (deck_fit sees to it once the deck is known).
        if mem.deck.is_empty() && act == Some(1) && mem.deck_size < 18 && base::str_of(&c["type"]) == "Attack" && v > 0.0 {
            v += 0.8;
        }
        v
    };

    let mut ranked = sorted_by(cards.iter().cloned(), worth, true);
    let select = |ranked: &[Value]| -> Vec<Action> { ranked.iter().map(|c| act_("select_card_reward", json!({"card_index": index_of(c)}))).collect() };
    if cr["in_combat"].truthy() {
        // A card a potion (or Discovery) offers in a fight is for this fight only: the one the turn makes most of, never none (2026-11-25:
        // the Silent drank an Attack Potion on round 1 of Donu and Deca and skipped all three for "need 4.0").
        ranked = afflictions::offered(&ranked, &state["_fight_state"]);
        if LOG.enabled(Level::Debug) {
            crate::log_debug!(LOG, "card offer in a fight: {}", ranked.iter().map(|c| base::str_of(&c["name"])).collect::<Vec<_>>().join(", "));
        }
        let mut out = select(&ranked);
        out.push(act_("skip_card_reward", json!({})));
        return out;
    }
    let threshold = reward_bar(mem.deck_size, act);
    if LOG.enabled(Level::Debug) {
        let shown: Vec<String> = ranked.iter().map(|c| format!("{} {:.1}", base::str_of(&c["name"]), worth(c))).collect();
        crate::log_debug!(LOG, "card reward (deck {}, need {:.1}): {}", mem.deck_size, threshold, shown.join(", "));
    }
    let take = select(&ranked);
    let skip: Vec<Action> = vec![act_("skip_card_reward", json!({}))];
    if !ranked.is_empty() && worth(&ranked[0]) >= threshold {
        return [take, if get_or_true(cr, "can_skip") { skip } else { vec![] }].concat();
    }
    [skip, take].concat()
}

// ---------------------------------------------------------------- rest site
pub fn rest_site(state: &Value, mem: &mut Memory) -> Vec<Action> {
    let rs = &state["rest_site"];
    let opts: Vec<Value> = rs["options"].list().iter().filter(|o| get_or_true(o, "is_enabled")).cloned().collect();
    let (player, hp_now, max_hp) = _vitals(state);
    let hp = hp_now as f64 / max_hp as f64;
    // Rest or upgrade: whichever leaves the way to the boss better off (without the map, a rule of thumb; HEAL_BELOW before a boss).
    let act = as_int(&state["run"]["act"]);
    let full = act.and_then(|act| HEAL_BELOW.iter().find(|(a, _)| *a == act)).map_or(0.95, |(_, share)| *share);
    let mut heal_first = hp < if mem.boss_next { full } else { 0.6 };
    let node = if mem.map_nodes.is_empty() { None } else { mem.map_at.and_then(|at| mem.map_nodes.get(&at)) };
    if node.is_some_and(|node| base::str_of(&node["type"]) == "RestSite" && node["children"].truthy()) {
        let args = (max_hp, player["gold"].int(), act.unwrap_or(1));
        let mut budget = MAX_PATHS;
        let smith = _upgrade_worth(Some(args.2)) + best_onward(&mem.map_nodes, mem.map_at, &[], hp_now as f64, args.0, args.1, args.2, &mut budget);
        let healed = hp_now as f64 + REST_HEAL * max_hp as f64;
        let healed = if healed < max_hp as f64 { healed } else { max_hp as f64 };
        let mut budget = MAX_PATHS;
        let rest = best_onward(&mem.map_nodes, mem.map_at, &[], healed, args.0, args.1, args.2, &mut budget);
        // Below half HP the path values hardly tell 6 HP from 40 (no boss is won under 30%): heal then, and before a boss.
        heal_first = hp < 0.5 || (rest > smith && hp < 0.95) || (mem.boss_next && hp < full);
        crate::log_debug!(LOG, "rest site at {}/{}: rest {:.1}, upgrade {:.1}", hp_now, max_hp, rest, smith);
    }
    let keys_first = EMERALD_HOLD.get() > 0.0;
    let burning_next = keys_first && node.is_some_and(|node| node["children"].list().iter().filter_map(pair).any(|k| mem.map_nodes.get(&k).is_some_and(|n| n["type"] == "EmeraldElite")));
    heal_first = heal_first || (burning_next && hp < EMERALD_REST.get());

    let rank = |o: &Value| -> f64 {
        let oid = o["id"].or(&o["name"]).text().to_lowercase();
        if oid.contains("rest") || oid.contains("heal") {
            return if heal_first { 10.0 } else { 1.0 };
        }
        if oid.contains("smith") || oid.contains("upgrade") {
            return 5.0;
        }
        if oid == "recall" {
            // The first game's Ruby Key, offered only while it is missing (games/sts1/keys.rs): over an upgrade, and over a rest
            // while healthy enough (not before the boss, nor the burning elite)
            return if keys_first && RUBY_NOW.get() > 0.0 && hp >= RUBY_NOW.get() && !mem.boss_next && !burning_next { 11.0 } else { 5.5 };
        }
        // A relic's option beats the upgrade: Girya's Lift (Strength, for a deck that hits), Shovel's Dig, Pumpkin Candle's Kindle.
        if oid.contains("lift") {
            let hitters = mem.deck.iter().map(_parse).filter(|c| c.damage > 0 && !c.osty_attack).count() as i64;
            return if mem.deck.is_empty() || hitters >= LIFT_HITTERS { 6.0 } else { 4.0 };
        }
        if oid.contains("dig") || oid.contains("kindle") {
            return 6.0;
        }
        3.0
    };

    let out: Vec<Action> = sorted_by(opts, rank, true).iter().map(|o| act_("choose_rest_option", json!({"index": index_of(o)}))).collect();
    let proceed = vec![act_("proceed", json!({}))];
    if rs["can_proceed"].truthy() {
        [proceed, out].concat()
    } else {
        [out, proceed].concat()
    }
}

// --------------------------------------------------------------------- shop
pub const REMOVAL_EARLY: [(i64, f64); 2] = [(1, 3.5), (2, 1.5)]; // what a card removal is worth less in acts 1 and 2 under the `cards_first` setting
pub const RELIC_BAR: f64 = 5.0; // a shop relic wanted less than this (a fair card) is bought only with 200 Gold to spare
pub const SEAL_RESERVE: i64 = 45; // Gold a Merchant does not get while Seal of Gold is held (3 a turn for an energy: a boss fight is 10 to 15 turns)
// What the Fake Merchant's relics are worth next to a real one at 7, by what their text does over a run (Snecko Eye???: confusion for nothing).
#[rustfmt::skip]
pub const _FAKES: [(&str, f64); 9] = [("FAKE_ANCHOR", 5.0), ("FAKE_ORICHALCUM", 4.0), ("FAKE_HAPPY_FLOWER", 3.5), ("FAKE_VENERABLE_TEA_SET", 3.0), ("FAKE_BLOOD_VIAL", 3.0), ("FAKE_LEES_WAFFLE", 3.0),
    ("FAKE_MANGO", 2.5), ("FAKE_STRIKE_DUMMY", 1.5), ("FAKE_SNECKO_EYE", -1.0)];

pub fn _shop_card(item: &Value) -> Value {
    let mut card = serde_json::Map::new();
    for k in ["id", "name", "type", "cost", "star_cost", "rarity", "description"] {
        card.insert(k.to_string(), item[format!("card_{k}").as_str()].clone());
    }
    Value::Object(card)
}

/// (the original's default: `key` "shop".)
pub fn shop(state: &Value, mem: &mut Memory, key: &str) -> Vec<Action> {
    let mut s = &state[key];
    if key == "fake_merchant" {
        s = &s["shop"];
    }
    if s["error"].truthy() {
        return vec![];
    }
    let player = &state["player"];
    let gold = player["gold"].int();
    let belt_full = _belt_full(player);
    let (act, floor) = _where(state);
    let relics = player["relics"].list();
    if relics.iter().any(|r| r["description"].text().to_lowercase().contains("everything he sells")) {
        // Lord's Parasol empties the shelf by itself and a purchase of ours meanwhile crashes the game: buy nothing, leave when it is done.
        let looks = match mem.parasol {
            Some((a, f, looks)) if (a, f) == (act, floor) => looks + 1,
            _ => 1,
        };
        mem.parasol = Some((act, floor, looks));
        return if looks > 8 || !s["items"].list().iter().any(|i| get_or_true(i, "is_stocked")) { vec![act_("proceed", json!({}))] } else { vec![] };
    }
    let reserve = if relics.iter().any(|r| base::str_of(&r["id"]) == "SEAL_OF_GOLD") { SEAL_RESERVE } else { 0 };
    let debug = LOG.enabled(Level::Debug);
    let mut scored: Vec<(f64, &Value)> = Vec::new();
    let mut shelf: Vec<String> = Vec::new(); // (for the log: what was on offer, what it cost and what we made of it)
    for item in s["items"].list() {
        let price = match item.get("price") {
            Some(price) => price.int(),
            None => item["cost"].int(),
        };
        if !get_or_true(item, "is_stocked") {
            continue;
        }
        let cat = base::str_of(&item["category"]);
        let mut rated: Option<f64> = None; // (a card's rating, for the log, when it is under the bar and so not wanted)
        let want: f64;
        if cat == "card_removal" {
            // From 150 Gold a removal has to beat the relics on the shelf, and early in a run the cards too (`cards_first`).
            let base = if price <= 125 { 9.0 } else { 7.6 - (price - 100) as f64 / 100.0 };
            want = base - knob("cards_first") * act.and_then(|act| REMOVAL_EARLY.iter().find(|(a, _)| *a == act)).map_or(0.0, |(_, less)| *less);
        } else if cat == "relic" {
            // By what the relic does for this deck (relicworth.rs); one worth less than a fair card only out of plenty.
            let relic_id = item["relic_id"].text();
            let mut w = 8.0 - price as f64 / 200.0 + relic_worth(&relic_id, &mem.deck, act, player["hp"].int(), player["max_hp"].int());
            w = if w >= RELIC_BAR || gold - price >= 200 { w } else { -1.0 };
            if relic_id.starts_with("FAKE_") {
                // The Fake Merchant's copies do a fraction of what their names promise, for the same gold a real Merchant takes.
                w = _FAKES.iter().find(|(k, _)| *k == relic_id).map_or(2.0, |(_, v)| *v);
                w = if w >= 3.5 || gold - price >= 200 { w } else { -1.0 };
            }
            want = w;
        } else if cat == "card" {
            let card = _shop_card(item);
            let mut v = deck_value(&card) + deck_fit(&card, &mem.deck, act, mem.max_energy);
            if let Some(stats) = &mem.card_stats {
                // (what our own runs say of the card and of its kind, as at a card reward)
                v += stats.borrow_mut().shift(&player["character"].text(), &base::str_of(&item["card_name"]), Some(_kind(&card)));
            }
            v += model_shift(&player["character"].text(), &item["card_name"].text());
            // Only cards that make the deck better, and more freely when rich.
            let bar = if gold - price >= 200 {
                4.0
            } else if mem.deck_size < BIG_DECK {
                4.5
            } else {
                5.5
            };
            (want, rated) = (if v >= bar { v } else { -1.0 }, Some(v));
        } else if cat == "potion" {
            // Within reach of a boss (potions.SAVE_FLOORS, and all of act 3) a potion is the best thing gold buys.
            let boss_next = saving(&state["run"]["floor"]) || floor.is_some_and(|floor| floor > boss_floors()[1]);
            if belt_full {
                want = -1.0;
            } else if boss_next {
                // Before a removal while the belt holds fewer than two, and the one worth most at that boss first.
                let base = if player["potions"].list().len() < 2 { 9.5 } else { 7.0 };
                let more = boss_worth(item) / 60.0;
                want = base + if more < 1.0 { more } else { 1.0 };
            } else {
                want = if gold - price >= 60 { 2.0 } else { -1.0 };
            }
        } else {
            want = -1.0;
        }
        let can = match item.get("can_afford") {
            Some(x) => x.truthy(),
            None => price <= gold,
        };
        let afford = can && (reserve == 0 || gold - price >= reserve);
        if debug {
            let name = item["card_name"].or(&item["relic_name"]).or(&item["potion_name"]).text();
            shelf.push(format!("{} {} {}{} [{:.1}]", cat, name, price, if afford { "" } else { "!" }, rated.unwrap_or(want)).replace("  ", " "));
        }
        if want > 0.0 && afford {
            scored.push((want, item));
        }
    }
    if debug {
        let shown = format!("{} {}: {}", where_repr(state), gold, shelf.join(", "));
        if shown != mem.logged_shop {
            mem.logged_shop = shown;
            crate::log_debug!(LOG, "shop (gold {}, potions {} of {}): {}", gold, player["potions"].list().len(), slots(player), shelf.join(", "));
        }
    }
    base::sort_by_key(&mut scored, |t| -t.0);
    let mut out: Vec<Action> = scored.iter().map(|(_, i)| act_("shop_purchase", json!({"index": index_of(i)}))).collect();
    out.push(act_("proceed", json!({})));
    out
}

// --------------------------------------------------------------- selections
/// (Bonfire Spirits, A Note For Yourself)
pub fn _WORST_FIRST() -> &'static regex::Regex {
    crate::re!(r"(?i)remove|transform|exhaust|discard|lose|purge|sacrifice|destroy|\boffer\b|\bstore\b")
}
pub const KEEP_AT_ALL_COSTS: [&str; 1] = ["FRANTIC_ESCAPE"]; // the only way out of the Sandpit

/// Lower = get rid of it first.
pub fn _removal_rank(card: &Value) -> f64 {
    if KEEP_AT_ALL_COSTS.contains(&_card_key(card).as_str()) {
        return 1000.0; // (a Status card, so it went first when a card or relic asked for one)
    }
    if is_junk(card) {
        return -100.0;
    }
    let mut v = deck_value(card);
    if is_basic(card) {
        v -= if _card_key(card).starts_with("STRIKE") { 5.5 } else { 5.0 }; // a Strike before a Defend (the two rate alike)
    }
    v
}

/// (the original's defaults: `screen_type` "", `gain` None.)
pub fn _pick_order(cards: &[Value], prompt: &str, screen_type: &str, gain: Option<&UpgradeGain>) -> Vec<Value> {
    if _WORST_FIRST().is_match(prompt) || screen_type == "transform" {
        return sorted_by(cards.iter().cloned(), _removal_rank, false);
    }
    if prompt.to_lowercase().contains("upgrade") || screen_type == "upgrade" {
        let fresh: Vec<Value> = cards.iter().filter(|c| !c["is_upgraded"].truthy()).cloned().collect();

        let rank = |c: &Value| -> f64 {
            // A good card we play often, and an upgrade that does a lot for it (capped, and little for a card we hardly play).
            let g = gain.and_then(|gain| gain(c));
            let v = deck_value(c);
            match g {
                None => v,
                Some(g) => 0.6 * v + (if 12.0 < g { 12.0 } else { g }) * (if v >= 2.5 { 1.0 } else { 0.3 }),
            }
        };

        let ranked = sorted_by(if fresh.is_empty() { cards.to_vec() } else { fresh }, rank, true);
        if gain.is_some() && LOG.enabled(Level::Debug) {
            let shown: Vec<String> = ranked.iter().take(5).map(|c| format!("{} {:.1}", base::str_of(&c["name"]), rank(c))).collect();
            crate::log_debug!(LOG, "upgrade: {}", shown.join(", "));
        }
        return ranked;
    }
    sorted_by(cards.iter().cloned(), deck_value, true)
}

pub fn card_select(state: &Value, mem: &mut Memory) -> Vec<Action> {
    match CARD_SELECT.with(|h| h.borrow().clone()) {
        Some(f) => f(state, mem),
        None => _card_select(state, mem),
    }
}

/// `card_select` itself (see [`with_card_select_hook`]).
pub fn _card_select(state: &Value, mem: &mut Memory) -> Vec<Action> {
    let cs = &state["card_select"];
    let prompt = cs["prompt"].text();
    let kind = cs["screen_type"].text();
    let cards = cs["cards"].list();
    let key = format!("{}|{}|{}|{}", where_repr(state), kind, prompt, cards.len());
    if key != mem.select_key {
        (mem.select_key, mem.selected) = (key.clone(), BTreeSet::new());
    }
    let mut ordered = afflictions::order(&_pick_order(cards, &prompt, &kind, mem.upgrade_gain.as_ref()), state);
    let can_leave = cs["can_skip"].truthy() || cs["can_cancel"].truthy();
    if kind == "choose" {
        ordered = afflictions::offered(&ordered, &state["_fight_state"]); // (a card a potion offers mid-fight: by the turn)
        if LOG.enabled(Level::Debug) {
            let shown = format!("{}|{}", key, cards.iter().map(|c| base::str_of(&c["name"])).collect::<Vec<_>>().join(","));
            if shown != mem.logged_select {
                mem.logged_select = shown;
                let each: Vec<String> = ordered.iter().map(|c| format!("{}: {}", base::str_of(&c["name"]), base::str_of(&c["description"]))).collect();
                crate::log_debug!(LOG, "choose ({}): {}", prompt, each.join(" | "));
            }
        }
        // One click picks and closes the screen; a click while it is still opening is ignored, so always offer it again.
        let mut out: Vec<Action> = ordered.iter().map(|c| act_("select_card", json!({"index": index_of(c)}))).collect();
        if can_leave {
            out.push(act_("cancel_selection", json!({})));
        }
        return out;
    }

    let indices: Vec<i64> = ordered.iter().map(index_of).collect();
    let preview = cs["preview_showing"].truthy();
    if !preview && !indices.is_empty() && indices.iter().all(|i| mem.selected.contains(i)) {
        mem.selected = BTreeSet::new(); // our clicks did not register; start over
    }
    // When the prompt says how many, tick exactly that many before confirming (some screens report confirm ready too early).
    let wanted = crate::re!(r"(?i)choose\s+(\d+)\s+card").captures(&prompt);
    let ready = match wanted {
        Some(wanted) if !preview => mem.selected.len() as i64 >= int_of(&wanted[1]),
        _ => preview || cs["can_confirm"].truthy(),
    };
    let mut out: Vec<Action> = if ready { vec![act_("confirm_selection", json!({}))] } else { vec![] };
    out.extend(indices.iter().filter(|i| !mem.selected.contains(i)).map(|i| act_("select_card", json!({"index": i}))));
    if can_leave {
        out.push(act_("cancel_selection", json!({})));
    }
    out.push(act_("confirm_selection", json!({})));
    out
}

/// `int(digits)` of a number a pattern found (one too long for an i64, never seen, reads as a very large one).
fn int_of(digits: &str) -> i64 {
    digits.parse().unwrap_or(i64::MAX >> 16)
}

pub fn _PUT_BACK() -> &'static regex::Regex {
    crate::re!(r"(?i)on top of your draw pile")
}

/// Which card of the hand to put on top of the draw pile (Photon Cut, Glimmer, Thinking Ahead): one that would not be
/// played this turn anyway, the best of what we cannot pay for or of what is left over once the energy is spent.
pub fn _put_back_order(cards: &[Value], energy: Option<i64>) -> Vec<Value> {
    fn cost(c: &Value) -> i64 {
        match &c["cost"] {
            Value::Bool(b) => *b as i64,
            Value::Number(n) => n.as_i64().unwrap_or_else(|| n.as_f64().map_or(0, base::trunc)),
            Value::String(s) => base_strip(s).parse().unwrap_or(0), // X-cost: playable with anything
            _ => 0,
        }
    }

    let real: Vec<Value> = cards.iter().filter(|c| !is_junk(c)).cloned().collect();
    let Some(energy) = energy else {
        let mut out = sorted_by(real, deck_value, false);
        out.extend(cards.iter().filter(|c| is_junk(c)).cloned());
        return out;
    };
    let mut spare: Vec<Value> = real.iter().filter(|c| cost(c) > energy).cloned().collect();
    let mut left = energy;
    for c in sorted_by(real.iter().filter(|c| cost(c) <= energy).cloned(), |c| -deck_value(c) / 1.max(cost(c)) as f64, false) {
        if cost(&c) <= left {
            left -= cost(&c);
        } else {
            spare.push(c);
        }
    }
    let spare = sorted_by(spare, deck_value, true);
    // Nothing to spare: a dead card costs a draw next turn, a live one costs its play now.
    let mut rest: Vec<Value> = cards.iter().filter(|c| is_junk(c)).cloned().collect();
    rest.extend(sorted_by(real.iter().filter(|c| !spare.contains(c)).cloned(), deck_value, false));
    let mut out = spare.clone();
    out.extend(rest.into_iter().filter(|c| !spare.contains(c)));
    out
}

/// Ashwater, Gambler's Brew, Gambling Chip; Purity
pub fn _OPTIONAL() -> &'static regex::Regex {
    crate::re!(r"(?i)\bany number of cards\b|\bup to (\d+) cards?\b")
}
/// "Choose any number of cards to replace." (discarded, then as many drawn)
pub fn _REPLACE() -> &'static regex::Regex {
    crate::re!(r"(?i)\breplace\b")
}
pub fn _EXHAUST() -> &'static regex::Regex {
    crate::re!(r"(?i)\bexhaust\b")
}

/// The card is `word` itself ("Retain.", "Ethereal."): not a card that only names it (Salvo: "Retain your Hand this turn").
pub fn _keyword(c: &Value, word: &str) -> bool {
    if c["keywords"].list().iter().any(|k| k.is_object() && base::str_of(&k["name"]).to_lowercase() == word) {
        return true;
    }
    base::re::cached(&format!(r"(?i)(?:^|\.\s*){word}\.")).is_match(&c["description"].text())
}

/// Unplayed cards stay in the hand this turn: Runic Pyramid, Ringing Triangle on the first turn, "Retain your Hand this turn".
pub fn _hand_kept(state: &Value) -> bool {
    let player = &state["player"];
    let relics: BTreeSet<String> = player["relics"].list().iter().filter(|r| r.is_object()).map(|r| r["id"].or(&r["name"]).text().to_uppercase().replace(' ', "_")).collect();
    let round = &state["battle"]["round"];
    let first = (if round.truthy() { round.int() } else { 1 }) <= 1;
    let powers: BTreeSet<String> = player["status"].list().iter().filter(|p| p.is_object()).map(|p| p["id"].text().to_uppercase()).collect();
    relics.contains("RUNIC_PYRAMID") || (first && relics.contains("RINGING_TRIANGLE")) || powers.contains("RETAIN_HAND") || powers.contains("RETAIN_HAND_POWER")
}

/// The next card to give to an optional exhaust or replace (the screen opens with Confirm already lit, 2026-11-26: every one
/// had been confirmed empty, Purity, Ashwater, Gambler's Brew and the Gambling Chip doing nothing), or None to confirm now.
/// Junk first: a Status or Curse is exhausted wherever it is, and replaced unless it is Ethereal (Dazed, Ascender's Bane go
/// by themselves at the end of the turn, where a replaced one would come back). Then a card this turn's plan does not miss:
/// replaced, it is a card drawn for nothing (unplayed it is discarded anyway); exhausted, only a Strike in a boss or elite
/// fight, which the rest of the fight then draws past. Retained cards stay.
pub fn _optional_pick(state: &Value, hs: &Value, prompt: &str) -> Option<Value> {
    let cards: Vec<Value> = hs["cards"].list().iter().filter(|c| !KEEP_AT_ALL_COSTS.contains(&_card_key(c).as_str()) && !_keyword(c, "retain")).cloned().collect();
    let m = _OPTIONAL().captures(prompt);
    let most = m.as_ref().and_then(|m| m.get(1)).map(|g| g.as_str()).filter(|g| !g.is_empty());
    if cards.is_empty() || most.is_some_and(|most| int_of(most) <= hs["selected_cards"].list().len() as i64) {
        return None;
    }
    let replacing = _REPLACE().is_match(prompt);
    let junk: Vec<&Value> = cards.iter().filter(|c| is_junk(c) && !(replacing && _keyword(c, "ethereal"))).collect();
    if !junk.is_empty() {
        return base::min_by_key(junk, |c| (_keyword(c, "ethereal"), _removal_rank(c))).cloned(); // (with few to choose, those that would stay first)
    }
    if !replacing && !matches!(state["_fight"].as_str(), Some("boss" | "elite")) {
        return None;
    }
    if replacing && _hand_kept(state) {
        return None; // (what is not played now is still there next turn: not a card for nothing)
    }
    let pool: Vec<Value> = sorted_by(cards.iter().cloned(), _removal_rank, false).into_iter().filter(|c| !is_junk(c) && (replacing || (is_basic(c) && _card_key(c).starts_with("STRIKE")))).collect();
    if pool.is_empty() {
        return None;
    }
    let asked: Vec<Value> = pool.iter().cloned().chain(cards.iter().filter(|c| !pool.contains(c)).cloned()).collect();
    let costs = handpick::turn_costs(state, &asked, 1e-9).filter(|costs| !costs.is_empty())?;
    pool.into_iter().find(|c| costs.get(&index_of(c)).copied().unwrap_or(handpick::UNPLANNED) <= 1e-6)
}

pub fn hand_select(state: &Value, mem: &mut Memory) -> Vec<Action> {
    let hs = &state["hand_select"];
    let prompt = hs["prompt"].text();
    let kind = if hs["mode"] == "upgrade_select" { "upgrade" } else { "" };
    let confirm = || act_("combat_confirm_selection", json!({}));
    let select = |c: &Value| act_("combat_select_card", json!({"card_index": index_of(c)}));
    let can_confirm = hs["can_confirm"].truthy();
    let names = |cards: &[Value]| cards.iter().map(|c| base::str_of(&c["name"])).collect::<Vec<_>>().join(", ");
    if _OPTIONAL().is_match(&prompt) && (_REPLACE().is_match(&prompt) || _EXHAUST().is_match(&prompt)) && kind.is_empty() {
        let Some(pick) = _optional_pick(state, hs, &prompt) else {
            return vec![confirm()];
        };
        crate::log_debug!(LOG, "hand select ({}): {}", prompt, base::str_of(&pick["name"]));
        return vec![select(&pick), confirm()];
    }
    if _PUT_BACK().is_match(&prompt) && !_WORST_FIRST().is_match(&prompt) {
        let energy = match &state["player"]["energy"] {
            Value::Bool(b) => Some(*b as i64),
            Value::Number(n) => Some(n.as_i64().unwrap_or_else(|| n.as_f64().map_or(0, base::trunc))),
            _ => None,
        };
        let ordered = _put_back_order(hs["cards"].list(), energy);
        let mut out: Vec<Action> = if can_confirm { vec![confirm()] } else { vec![] };
        if !can_confirm && LOG.enabled(Level::Debug) {
            crate::log_debug!(LOG, "hand select ({}): {}", prompt, names(&ordered));
        }
        out.extend(ordered.iter().map(select));
        out.push(confirm());
        return out;
    }
    let mut out: Vec<Action> = Vec::new();
    if can_confirm {
        out.push(confirm());
    } else if LOG.enabled(Level::Debug) {
        crate::log_debug!(LOG, "hand select ({}): {}", prompt, names(hs["cards"].list()));
    }
    let mut ordered = _pick_order(hs["cards"].list(), &prompt, kind, mem.upgrade_gain.as_ref());
    if crate::re!(r"(?i)\bmake free\b").is_match(&prompt) {
        // Touch of Insanity ("It is free to play this combat"): the dearest card, the best of those.
        ordered = sorted_by(ordered, |c| (matches!(base::str_of(&c["type"]).as_str(), "Status" | "Curse"), -_parse(c).cost), false);
    }
    if crate::re!(r"(?i)\badd ethereal\b").is_match(&prompt) {
        // Sculpting Strike ("Add Ethereal to a card in your Hand"): on what the deck is better without, not on one Ethereal already.
        let ethereal = |c: &Value| c["description"].text().to_lowercase().contains("ethereal") || c["keywords"].list().iter().any(|k| base::str_of(&k["name"]) == "Ethereal");
        ordered = sorted_by(ordered, |c| (ethereal(c), _removal_rank(c)), false);
    }
    let discarding = crate::re!(r"(?i)\bdiscard\b").is_match(&prompt);
    // A Sly card discarded from the hand is played for free: the best thing to throw away.
    let sly = |c: &Value| discarding && c["keywords"].list().iter().any(|k| base::str_of(&k["name"]) == "Sly");
    let (sly_ones, others): (Vec<Value>, Vec<Value>) = ordered.into_iter().partition(|c| sly(c));
    ordered = sorted_by(sly_ones, deck_value, true);
    ordered.extend(others);
    // Then the card this turn's plan misses least (unless only to be confirmed): a discard by the turn, an Exhaust unless it costs a kill or our life.
    let costs = if _WORST_FIRST().is_match(&prompt) && kind.is_empty() && !can_confirm {
        handpick::turn_costs(state, &ordered, if discarding { 1e-9 } else { handpick::RUINOUS / 2.0 })
    } else {
        None
    };
    if let Some(costs) = costs.filter(|costs| !costs.is_empty()) {
        let price = |c: &Value| -> f64 {
            let cost = *costs.get(&index_of(c)).unwrap_or_else(|| panic!("KeyError: {}", index_of(c)));
            if sly(c) {
                return f64::NEG_INFINITY;
            }
            if discarding {
                return if KEEP_AT_ALL_COSTS.contains(&_card_key(c).as_str()) { f64::INFINITY } else { base::round(cost / 2.0) as f64 };
            }
            (cost >= handpick::RUINOUS) as i64 as f64 // (a Frantic Escape too, when every other card is the kill)
        };

        ordered = sorted_by(ordered, price, false); // (stable: equal prices keep the order above)
    }
    out.extend(ordered.iter().map(select));
    out.push(confirm());
    out
}

pub fn bundle_select(state: &Value, _mem: &mut Memory) -> Vec<Action> {
    let bs = &state["bundle_select"];
    let mut out: Vec<Action> = Vec::new();
    if bs["preview_showing"].truthy() || bs["can_confirm"].truthy() {
        out.push(act_("confirm_bundle_selection", json!({})));
    }
    let bundles = sorted_by(bs["bundles"].list().iter().cloned(), |b| base::fsum(b["cards"].list().iter().map(deck_value)), true);
    out.extend(bundles.iter().map(|b| act_("select_bundle", json!({"index": index_of(b)}))));
    out.push(act_("confirm_bundle_selection", json!({})));
    out
}

pub fn relic_select(state: &Value, _mem: &mut Memory) -> Vec<Action> {
    let rs = &state["relic_select"];
    let mut out: Vec<Action> = rs["relics"].list().iter().map(|r| act_("select_relic", json!({"index": index_of(r)}))).collect();
    if rs["can_skip"].truthy() {
        out.push(act_("skip_relic_selection", json!({})));
    }
    out
}

pub fn treasure(state: &Value, _mem: &mut Memory) -> Vec<Action> {
    let t = &state["treasure"];
    let relics = t["relics"].list();
    if relics.is_empty() && !t["can_proceed"].truthy() {
        return vec![]; // chest still opening
    }
    let mut out: Vec<Action> = relics.iter().map(|r| act_("claim_treasure_relic", json!({"index": index_of(r)}))).collect();
    out.push(act_("proceed", json!({})));
    out
}

pub fn crystal_sphere(state: &Value, _mem: &mut Memory) -> Vec<Action> {
    let cs = &state["crystal_sphere"];
    if cs["can_proceed"].truthy() {
        return vec![act_("crystal_sphere_proceed", json!({}))];
    }
    let size = |v: &Value, default: i64| if v.truthy() { v.int() } else { default };
    let (w, h) = (size(&cs["grid_width"], 9), size(&cs["grid_height"], 11));
    // from the middle: big tools reveal most there
    let at = |c: &Value, k: &str| c.get(k).map_or(0.0, |x| x.float());
    let cells = sorted_by(cs["clickable_cells"].list().iter().cloned(), |c| (at(c, "x") - base::floordiv(w, 2) as f64).abs() + (at(c, "y") - base::floordiv(h, 2) as f64).abs(), false);
    let mut out: Vec<Action> = cells.iter().take(6).map(|c| act_("crystal_sphere_click_cell", json!({"x": c["x"], "y": c["y"]}))).collect();
    out.push(act_("crystal_sphere_proceed", json!({})));
    out
}

pub const OVER_PATIENCE: i64 = 60; // looks at the end-of-run summary before giving up on it

/// Leave the end-of-run screen, by way of the run summary: that is where the game adds the run's score to the
/// profile, and the epochs that unlock "by accumulating score" only ever come from there.
pub fn game_over(state: &Value, mem: &mut Memory) -> Vec<Action> {
    let options = &state["game_over"]["options"];
    let has = |name: &str| match options {
        Value::Array(list) => list.iter().any(|o| o == name),
        Value::Object(dict) => dict.contains_key(name),
        Value::String(text) => text.contains(name),
        _ => false,
    };
    let go = |option: &str| vec![act_("menu_select", json!({"option": option}))];
    if mem.last_result == "loss" && timeline_complete(Some(&mem.epochs)) && has("force_main_menu") {
        // With every epoch revealed the score has nothing left to buy: a death goes straight back to the menu.
        return go("force_main_menu");
    }
    if has("continue") {
        mem.over_waits = 0;
        return go("continue");
    }
    if has("main_menu") {
        return go("main_menu");
    }
    if has("force_main_menu") {
        // The summary is still counting up: wait for its Main Menu button, but not for ever.
        mem.over_waits += 1;
        return if mem.over_waits > OVER_PATIENCE { go("force_main_menu") } else { vec![] };
    }
    go("main_menu") // a mod without the summary patch
}

pub fn overlay(_state: &Value, _mem: &mut Memory) -> Vec<Action> {
    vec![act_("proceed", json!({})), act_("confirm_selection", json!({})), act_("cancel_selection", json!({}))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games;
    use crate::learn;

    /// A case as it was recorded: `{"$r": n}` stands for `blobs[n]` (with `"$i"`, a dict whose first key is that index).
    fn resolve(v: &Value, blobs: &[Value]) -> Value {
        match v {
            Value::Object(m) if m.contains_key("$r") => {
                let inner = resolve(&blobs[m["$r"].as_u64().unwrap() as usize], blobs);
                match m.get("$i") {
                    Some(index) => {
                        let mut out = serde_json::Map::new();
                        out.insert("index".into(), index.clone());
                        out.extend(inner.as_object().unwrap().clone());
                        Value::Object(out)
                    }
                    None => inner,
                }
            }
            Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), resolve(x, blobs))).collect()),
            Value::Array(a) => Value::Array(a.iter().map(|x| resolve(x, blobs)).collect()),
            other => other.clone(),
        }
    }

    /// `callable(card) -> gain` from the answers the original gave for the cards it was asked about.
    fn table(rows: &Value) -> UpgradeGain {
        let rows: Vec<(Value, Option<f64>)> = rows.list().iter().map(|r| (r[0].clone(), r[1].as_f64())).collect();
        Rc::new(move |card: &Value| rows.iter().find(|(c, _)| c == card).unwrap_or_else(|| panic!("no recorded gain for {card}")).1)
    }

    fn where_of(k: &Value) -> Where {
        (k[0].as_i64(), k[1].as_i64())
    }

    fn nodes_of(v: &Value) -> IndexMap<(i64, i64), Value> {
        match v.get("$nodes") {
            Some(nodes) => nodes.list().iter().map(|n| ((n["col"].as_i64().unwrap(), n["row"].as_i64().unwrap()), n.clone())).collect(),
            None => v.list().iter().map(|kn| ((kn[0][0].as_i64().unwrap(), kn[0][1].as_i64().unwrap()), kn[1].clone())).collect(),
        }
    }

    /// The test-only JSON form of a Memory (the original capture's `mem_json`), read back.
    fn mem_of(j: &Value, gain: UpgradeGain) -> Memory {
        let strings = |v: &Value| -> Vec<String> { v.list().iter().map(base::str_of).collect() };
        Memory {
            deck_size: j["deck_size"].as_i64().unwrap(),
            deck: j["deck"].list().to_vec(),
            max_energy: j["max_energy"].as_i64().unwrap(),
            picks: j["picks"].list().iter().map(|p| (p[0].int(), base::str_of(&p[1]))).collect(),
            card_stats: None,
            card_skips: j["card_skips"].list().iter().map(|kn| (where_of(&kn[0]), kn[1].as_i64().unwrap())).collect(),
            select_key: j["select_key"].s().into(),
            logged_select: j["logged_select"].s().into(),
            logged_shop: j["logged_shop"].s().into(),
            selected: j["selected"].list().iter().map(|i| i.as_i64().unwrap()).collect(),
            runs_finished: j["runs_finished"].as_i64().unwrap(),
            character_selected: j["character_selected"].truthy(),
            boss_next: j["boss_next"].truthy(),
            event_id: j["event_id"].s().into(),
            event_hp0: j["event_hp0"].as_i64().unwrap(),
            map_nodes: nodes_of(&j["map_nodes"]),
            map_at: pair(&j["map_at"]),
            upgrade_gain: if j["upgrade_gain"].is_null() { None } else { Some(gain) },
            over_waits: j["over_waits"].as_i64().unwrap(),
            max_ascension: j["max_ascension"].entries().map(|(k, v)| (k.clone(), v.int())).collect(),
            ascension_waits: j["ascension_waits"].as_i64().unwrap(),
            epochs: j["epochs"].entries().map(|(k, v)| (k.clone(), base::str_of(v))).collect(),
            last_result: j["last_result"].s().into(),
            recent: strings(&j["recent"]),
            timeline_done: j["timeline_done"].truthy(),
            timeline_stuck: strings(&j["timeline_stuck"]).into_iter().collect(),
            parasol: match j["parasol"].list() {
                [act, floor, looks] => Some((act.as_i64(), floor.as_i64(), looks.as_i64().unwrap())),
                _ => None,
            },
            potion_swaps: j["potion_swaps"].list().iter().map(|kn| (where_of(&kn[0]), kn[1].as_i64().unwrap())).collect(),
        }
    }

    fn dict<'a>(items: impl Iterator<Item = (&'a String, Value)>) -> Value {
        Value::Object(items.map(|(k, v)| (k.clone(), v)).collect())
    }

    /// ... and written (the two tables kept by place in a fixed order: Rust keeps them unordered).
    fn mem_json(m: &Memory) -> Value {
        let by_place = |t: &HashMap<Where, i64>| -> Value {
            let mut rows: Vec<(Where, i64)> = t.iter().map(|(k, n)| (*k, *n)).collect();
            rows.sort();
            json!(rows.iter().map(|((act, floor), n)| json!([[act, floor], n])).collect::<Vec<_>>())
        };
        json!({
            "deck_size": m.deck_size, "deck": m.deck, "max_energy": m.max_energy, "picks": m.picks.iter().map(|(floor, name)| json!([floor, name])).collect::<Vec<_>>(),
            "card_skips": by_place(&m.card_skips), "select_key": m.select_key, "logged_select": m.logged_select, "logged_shop": m.logged_shop, "selected": m.selected,
            "runs_finished": m.runs_finished, "character_selected": m.character_selected, "boss_next": m.boss_next, "event_id": m.event_id, "event_hp0": m.event_hp0,
            "map_nodes": m.map_nodes.iter().map(|((col, row), n)| json!([[col, row], n])).collect::<Vec<_>>(), "map_at": m.map_at.map_or(json!([]), |(col, row)| json!([col, row])),
            "upgrade_gain": m.upgrade_gain.is_some(), "over_waits": m.over_waits, "max_ascension": dict(m.max_ascension.iter().map(|(k, v)| (k, json!(v)))),
            "ascension_waits": m.ascension_waits, "epochs": dict(m.epochs.iter().map(|(k, v)| (k, json!(v)))),
            "last_result": m.last_result, "recent": m.recent, "timeline_done": m.timeline_done, "timeline_stuck": m.timeline_stuck,
            "parasol": m.parasol.map_or(json!([]), |(act, floor, looks)| json!([act, floor, looks])), "potion_swaps": by_place(&m.potion_swaps),
        })
    }

    fn actions(out: Vec<Action>) -> Value {
        json!(out.into_iter().map(|(name, params)| json!([name, params])).collect::<Vec<_>>())
    }

    fn strings(v: &Value) -> Vec<String> {
        v.list().iter().map(base::str_of).collect()
    }

    fn epochs_of(v: &Value) -> Option<IndexMap<String, String>> {
        v.dict().map(|d| d.iter().map(|(k, x)| (k.clone(), base::str_of(x))).collect())
    }

    fn rooms_of(v: &Value) -> Vec<(String, Option<i64>)> {
        v.list().iter().map(|r| (r[0].s().to_string(), r[1].as_i64())).collect()
    }

    fn f(v: &Value) -> f64 {
        v.as_f64().unwrap()
    }

    fn i(v: &Value) -> i64 {
        v.as_i64().unwrap()
    }

    /// Every handler and helper as the original tests and the model runs of both games called them (see the capture notes in the
    /// commit that added tests/golden/strategy/): the same actions, the same numbers, the same Memory afterwards.
    fn replay(text: &str) -> usize {
        let g: Value = serde_json::from_str(text).unwrap();
        let blobs = g["blobs"].list();
        let mut seen = 0;
        for case in g["cases"].list() {
            let case = resolve(case, blobs);
            let name = case["fn"].s();
            let _game = games::playing(case["game"].s());
            let _newer = EMERALD_HOLD.set(0.0); // (the recordings are older than the rule)
            let knob: IndexMap<String, f64> = case["knob"].entries().map(|(k, v)| (k.clone(), v.float())).collect();
            let before = learn::knobs();
            learn::set_knobs(&knob);
            set_session(case["session"].as_i64());
            afflictions::_OFFER_reset();
            handpick::_answers_clear();
            let a = &case["args"];
            let state = &a["state"];
            let mut mem = if case.has("mem") { mem_of(&case["mem"], table(&case["gains"]["mem"])) } else { Memory::new() };
            let gain = if a["gain"].is_null() { None } else { Some(table(&case["gains"]["gain"])) };
            let got: Value = match name {
                "_where" => {
                    let (act, floor) = _where(state);
                    json!([act, floor])
                }
                "_vitals" => {
                    let (player, hp, max_hp) = _vitals(state);
                    json!([if player.is_null() { json!({}) } else { player.clone() }, hp, max_hp])
                }
                "_option_names" => json!(_option_names(&a["options"])),
                "menu" => actions(menu(state, &mut mem, a["character"].s())),
                "timeline_complete" => json!(timeline_complete(epochs_of(&a["epochs"]).as_ref())),
                "missing_epochs" => json!(missing_epochs(a["character"].s(), epochs_of(&a["epochs"]).as_ref())),
                "_who" => json!(_who(&base::str_of(&a["name"]))),
                "choose_character" => {
                    let levels: Option<IndexMap<String, i64>> = a["max_ascension"].dict().map(|d| d.iter().map(|(k, v)| (k.clone(), v.int())).collect());
                    let recent = a["recent"].as_array().map(|_| strings(&a["recent"]));
                    json!(choose_character(a["setting"].s(), &strings(&a["unlocked"]), i(&a["runs_finished"]), levels.as_ref(), epochs_of(&a["epochs"]).as_ref(), recent.as_deref()))
                }
                "_room_value" => json!(_room_value(a["kind"].s(), f(&a["hp"]), i(&a["gold"]), a["early"].truthy(), a["act"].as_i64())),
                "_between" => {
                    let points: Vec<(f64, f64)> = a["points"].list().iter().map(|p| (f(&p[0]), f(&p[1]))).collect();
                    json!(_between(&points, f(&a["x"])))
                }
                "boss_odds" => json!(boss_odds(f(&a["hp_ratio"]), i(&a["act"]))),
                "_upgrade_worth" => json!(_upgrade_worth(a["act"].as_i64())),
                "path_value" => json!(path_value(&rooms_of(&a["rooms"]), f(&a["hp"]), i(&a["max_hp"]), i(&a["gold"]), i(&a["act"]), f(&a["harsh"]))),
                "best_onward" => {
                    let mut budget = i(&a["budget"][0]);
                    let got = best_onward(&nodes_of(&a["nodes"]), pair(&a["key"]), &rooms_of(&a["rooms"]), f(&a["hp"]), i(&a["max_hp"]), i(&a["gold"]), i(&a["act"]), &mut budget);
                    assert_eq!(json!([budget]), case["budget_after"], "best_onward's budget {a}");
                    json!(got)
                }
                "map_screen" => actions(map_screen(state, &mut mem)),
                "_belt_full" => json!(_belt_full(&a["player"])),
                "rewards" => actions(rewards(state, &mut mem)),
                "card_reward" => actions(card_reward(state, &mut mem)),
                "rest_site" => actions(rest_site(state, &mut mem)),
                "_shop_card" => _shop_card(&a["item"]),
                "shop" => actions(shop(state, &mut mem, a["key"].s())),
                "_removal_rank" => json!(_removal_rank(&a["card"])),
                "_pick_order" => json!(_pick_order(a["cards"].list(), a["prompt"].s(), a["screen_type"].s(), gain.as_ref())),
                "card_select" => actions(card_select(state, &mut mem)),
                "_put_back_order" => json!(_put_back_order(a["cards"].list(), a["energy"].as_i64())),
                "_keyword" => json!(_keyword(&a["c"], a["word"].s())),
                "_hand_kept" => json!(_hand_kept(state)),
                "_optional_pick" => json!(_optional_pick(state, &a["hs"], a["prompt"].s())),
                "hand_select" => actions(hand_select(state, &mut mem)),
                "bundle_select" => actions(bundle_select(state, &mut mem)),
                "relic_select" => actions(relic_select(state, &mut mem)),
                "treasure" => actions(treasure(state, &mut mem)),
                "crystal_sphere" => actions(crystal_sphere(state, &mut mem)),
                "game_over" => actions(game_over(state, &mut mem)),
                "overlay" => actions(overlay(state, &mut mem)),
                other => panic!("no replay for {other}"),
            };
            assert_eq!(got, case["out"], "{name} {} (game {}, knobs {})", a, case["game"], case["knob"]);
            if case.has("mem") {
                let mut want = case["mem"].clone();
                for (k, v) in case["mem_after"].entries() {
                    want[k.as_str()] = v.clone();
                }
                assert_eq!(mem_json(&mem), mem_json(&mem_of(&want, table(&json!([])))), "{name}'s Memory afterwards {a}");
            }
            learn::set_knobs(&before);
            set_session(None);
            seen += 1;
        }
        seen
    }

    #[test]
    fn golden_calls_0() {
        assert!(replay(include_str!("../../tests/golden/strategy/cases0.json")) > 500);
    }

    #[test]
    fn golden_calls_1() {
        assert!(replay(include_str!("../../tests/golden/strategy/cases1.json")) > 200);
    }

    #[test]
    fn golden_calls_2() {
        assert!(replay(include_str!("../../tests/golden/strategy/cases2.json")) > 200);
    }

    #[test]
    fn golden_calls_3() {
        assert!(replay(include_str!("../../tests/golden/strategy/cases3.json")) > 100);
    }
}
