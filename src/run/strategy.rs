//! (The port of sts2bot/run/strategy.py. Only its `Memory` class is here so far, for run/events/mod.rs, which reads and writes
//! it; the rest of strategy.py follows below it.)

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::Value;

use crate::learn::CardStats;

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
    pub map_at: Option<(i64, i64)>, // the node we walked to (Python's `()`: None)
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

// (The rest of strategy.py follows: `_where`, `_vitals`, the menu, the map, rewards, shops, card selects, rest sites, ...)
