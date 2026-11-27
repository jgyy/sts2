//! Main loop: read state, decide, act, repeat. One Bot plays run after run.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};

use crate::cards;
use crate::client::{Game, GameUnavailable};
use crate::fight::bossfight::BossWatch;
use crate::fight::combat;
use crate::fightrecord::FightWatch;
use crate::games::both::{self, KeyboardInterrupt};
use crate::games::sts1::stance;
use crate::learn;
use crate::logging::{Level, Logger};
use crate::py::{self, JsonExt};
use crate::run::strategy::{self, Memory, Where};
use crate::run::{events, relics};
use crate::Action;

static LOG: Logger = Logger::new("sts2bot");

pub const COMBAT_STATES: [&str; 3] = ["monster", "elite", "boss"];
pub const RECENT_RUNS: usize = 40; // runs whose characters the rotation looks back over (strategy.choose_character)
/// Screens strategy.rs has a handler for, under the screen's own name (the map's is map_screen).
pub const SCREENS: [&str; 14] = ["hand_select", "rewards", "card_reward", "map", "event", "rest_site", "shop", "treasure", "card_select", "bundle_select", "relic_select", "crystal_sphere", "game_over", "overlay"];
pub const MAX_STRIKES: i64 = 2; // failed attempts before an action is shelved for the current state
pub const FRESH: f64 = 0.05; // a state the wait for the last action fetched this recently is the next step's (one request less a play)
pub const PACE_STEPS: i64 = 1000; // steps between pace lines in the log
pub const ROOM_PAUSE: f64 = 0.1; // after leaving a room or buying; between looks while a room loads (StS2)
pub const ROOM_POLL: f64 = 0.06;
pub const FIGHT_POLL: f64 = 0.08; // seconds between looks in a fight when there is nothing to send (2026-11-25: the 0.2 s poll held up every turn's start and end)
pub const MAX_PLAYS_PER_TURN: i64 = 40;
pub const _VOLATILE: [&str; 2] = ["is_hovered", "is_highlighted"];
/// Cards a relic or an enemy shuffles in when a fight starts (the piles do not say what type a card is).
pub const NOT_OURS: [&str; 11] = ["Dazed", "Wound", "Burn", "Slimed", "Soot", "Debris", "Infection", "Toxic", "Void", "Wither", "Frantic Escape"];

/// Nothing the bot tries changes the game any more.
#[derive(Clone, Debug, PartialEq)]
pub struct Stuck(pub String);

impl std::fmt::Display for Stuck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Stuck {}

/// What ends a `step` or a `run` early: the Python's exceptions out of the loop (the supervisor tells them apart).
#[derive(Clone, Debug, PartialEq)]
pub enum BotError {
    /// Nothing the bot tries changes the game any more.
    Stuck(Stuck),
    /// The game (or the bridge mod inside it) is not answering.
    GameUnavailable(GameUnavailable),
    /// Ctrl+C, or SIGTERM from systemd (Python's KeyboardInterrupt: `games::both::stopped()`).
    Interrupted(KeyboardInterrupt),
}

impl std::fmt::Display for BotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BotError::Stuck(e) => e.fmt(f),
            BotError::GameUnavailable(e) => e.fmt(f),
            BotError::Interrupted(_) => f.write_str("KeyboardInterrupt"),
        }
    }
}

impl std::error::Error for BotError {}

impl From<Stuck> for BotError {
    fn from(e: Stuck) -> Self {
        BotError::Stuck(e)
    }
}

impl From<GameUnavailable> for BotError {
    fn from(e: GameUnavailable) -> Self {
        BotError::GameUnavailable(e)
    }
}

impl From<KeyboardInterrupt> for BotError {
    fn from(e: KeyboardInterrupt) -> Self {
        BotError::Interrupted(e)
    }
}

/// `{k: _scrub(v) for k, v in obj.items() if k not in _VOLATILE}`, all the way down.
pub fn _scrub(obj: &Value) -> Value {
    match obj {
        Value::Object(m) => Value::Object(m.iter().filter(|(k, _)| !_VOLATILE.contains(&k.as_str())).map(|(k, v)| (k.clone(), _scrub(v))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(_scrub).collect()),
        other => other.clone(),
    }
}

/// `json.dumps(x, sort_keys=True)`.
fn dumps_sorted(v: &Value) -> String {
    py::Dumps { sort_keys: true, ..Default::default() }.dumps(v)
}

pub fn fingerprint(state: &Value) -> String {
    let raw = dumps_sorted(&_scrub(state));
    Sha1::digest(raw.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Python's `a == b` for two values out of the game's JSON: numbers (and bools) by what they count, the rest as they are.
fn _eq(a: &Value, b: &Value) -> bool {
    let num = |v: &Value| match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(*b as i64 as f64),
        _ => None,
    };
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x == y,
        (None, None) => a == b,
        _ => false,
    }
}

/// `state.get("state_type") in COMBAT_STATES`.
fn in_combat(kind: &Value) -> bool {
    kind.as_str().is_some_and(|k| COMBAT_STATES.contains(&k))
}

/// `time.strftime("%Y-%m-%dT%H:%M:%S%z")`: the local time now, with its offset from UTC.
pub fn _now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    let off = tm.tm_gmtoff;
    let (sign, off) = if off < 0 { ('-', -off) } else { ('+', off) };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec, sign, off / 3600, off % 3600 / 60)
}

/// The game the bot plays through: the bridge mod's client, the first game's, or a test's fake game. Shared: the supervisor
/// pings and configures the same game, and a test reads what its fake game was asked (`Rc::new(RefCell::new(FakeGame::new()))`).
pub type SharedGame = Rc<RefCell<dyn Game>>;

/// (act, floor, round): a turn of a fight. (Python's `()` before the first one: None.)
pub type TurnKey = (Where, Value);

/// (act, floor, action, its parameters as JSON): a request, where it was made.
pub type Sent = (Value, Value, String, String);

/// `Bot.progress()`: (runs finished, act, floor) — without the act and floor before the first room is seen. Prints as the Python's tuple.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress(pub i64, pub Option<Where>);

impl std::fmt::Display for Progress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let none = |x: Option<i64>| x.map_or("None".to_string(), |n| n.to_string());
        match self.1 {
            Some((act, floor)) => write!(f, "({}, {}, {})", self.0, none(act), none(floor)),
            None => write!(f, "({},)", self.0),
        }
    }
}

/// The keyword arguments of `Bot(client, ...)`, with the Python's defaults.
pub struct BotArgs {
    pub character: String,
    pub log_dir: Option<PathBuf>,
    pub poll: f64,
    pub settle: f64,
    pub stuck_timeout: f64,
    /// try different settings from run to run and learn which work (learn.Knobs)
    pub tune: bool,
    /// (None: `time.monotonic`)
    pub clock: Option<Box<dyn Fn() -> f64>>,
    /// (None: `time.sleep`, cut short when the process is told to stop)
    pub sleep: Option<Box<dyn Fn(f64)>>,
}

impl Default for BotArgs {
    fn default() -> Self {
        BotArgs { character: "auto".to_string(), log_dir: None, poll: 0.4, settle: 3.0, stuck_timeout: 300.0, tune: false, clock: None, sleep: None }
    }
}

pub struct Bot {
    pub client: SharedGame,
    pub character: String,
    pub log_dir: Option<PathBuf>,
    pub poll: f64,
    // Pauses after leaving a room or buying, and between looks while a room loads: faster crashed StS2 (StS1's are ~0: cli.rs).
    // 0.4 and 0.15 until 2026-11-26 (user: "reduce the delay to select the next floor"): the map shows 0.25 s after a proceed and
    // a floor took 2.3 s, a quarter of it between the rewards and the next room; the mod has guarded its scene walks since.
    pub room_pause: f64,
    pub room_poll: f64,
    pub fight_poll: f64,
    pub _seen: (f64, Option<Value>), // (when, state) the last wait for an action to show ended on
    pub settle: f64,
    pub travel_settle: f64,
    pub quiet_settle: f64,
    pub choose_settle: f64,
    pub careful_until: f64, // after two crashes in one place (a progress()): act slowly there until then
    pub careful_at: Option<Progress>,
    pub _crashed: Vec<Sent>, // (act, floor, action, params) asked last before a crash: the game reloads that room, so avoid it there (a set)
    pub _last_sent: Option<Sent>, // the last such request since the game was (re)started
    pub time_scale: f64, // the game clock's speed-up (0: none), re-sent every few seconds: boss fights reset it
    pub _scale_at: f64,
    pub stuck_timeout: f64,
    pub clock: Box<dyn Fn() -> f64>,
    pub sleep: Box<dyn Fn(f64)>,
    pub _pace: IndexMap<&'static str, f64>, // wall seconds by kind of work since the last pace line (`run`): where a run's time goes

    pub mem: Memory,
    pub card_stats: Rc<RefCell<learn::CardStats>>,
    pub _upgrades: Rc<RefCell<HashMap<String, Option<f64>>>>, // card id -> what its upgrade adds (None: the game could not say)
    pub knobs: learn::Knobs,
    pub _knobs_whole_run: bool, // the settings in force were there from the run's first floor
    pub tune: bool,
    pub max_runs: Option<i64>,
    pub abandon_next: bool, // set by the supervisor to drop a run that keeps hanging
    pub runs_finished: i64,
    pub wins: i64,
    pub _strikes: HashMap<(String, String), i64>,
    pub _dead_buys: HashMap<(String, String, String), i64>, // purchases accepted that changed nothing, by (act, floor, item): never forgiven
    pub _last_fp: String,
    pub _last_change: f64,
    pub _last_retry: f64,
    pub _over_logged: bool,
    pub _prev_kind: Value, // (the state before this one: a run that ends straight out of a fight was lost)
    pub _boss_watch: BossWatch,
    pub _fight_watch: FightWatch, // (every fight, kept past the logs)
    // This turn: its key, plays, cards played (a potion is not one of the "3 cards each turn") and their names (the Fool's Melodies).
    pub _turn_key: Option<TurnKey>,
    pub _turn_plays: i64,
    pub _turn_cards: i64,
    pub _turn_names: Vec<String>,
    pub _stuck: BTreeSet<String>, // enemies holding a Fool's Arrow, and whether one came out this turn (Yah!'s energy is spent then)
    pub _dislodged: bool,
    pub _fight_state: Option<Value>, // the combat state as the planner last saw it (for a potion's offer of cards)
    pub _turn_energy: i64, // energy when we last looked at this turn
    pub _turns: Vec<(i64, i64)>, // (cards played, energy left) for each turn ended in this fight
    pub _last_where: Option<Where>,
    pub _seen_powers: HashSet<String>,
    pub _keys_seen: Option<Value>, // (the first game's act 4 keys, as last logged)
    pub _wins_seen: Option<i64>, // the game's win counter when we last looked
    pub _wins_asked: bool, // asked once for a run already under way
    pub _timeline_idle: i64, // Timeline clicks in a row that changed nothing
    pub _facing: Value, // enemy we last aimed at, in this fight
    pub _facing_fight: Option<Where>,
    pub _threat: IndexMap<String, (i64, i64, f64, i64)>, // enemy id -> [damage it has aimed at us in this fight, rounds seen, its blows, the turns it attacked]
    pub _threat_fight: Option<Where>,
    pub _revive_fight: Option<Where>, // the fight in which the enemy last seen revives when beaten
    pub _end_check: Option<(TurnKey, i64, Vec<String>)>, // the hand we last thought of ending the turn on
    pub _played_from: Value, // ... and the one we last played from (null: none)
    pub _fight_kind: String, // monster, elite or boss: the fight we were last in
    pub _played_at: f64,
}

impl Bot {
    /// `Bot(client, character="auto", log_dir=None, poll=0.4, settle=3.0, stuck_timeout=300.0, tune=False, clock=time.monotonic, sleep=time.sleep)`.
    pub fn new(client: SharedGame, args: BotArgs) -> Bot {
        let BotArgs { character, log_dir, poll, settle, stuck_timeout, tune, clock, sleep } = args;
        let clock: Box<dyn Fn() -> f64> = clock.unwrap_or_else(|| Box::new(both::monotonic));
        let sleep: Box<dyn Fn(f64)> = sleep.unwrap_or_else(|| {
            Box::new(|seconds| {
                let _ = both::sleep(seconds); // (left early when we are told to stop: the loop then sees `both::stopped()`)
            })
        });

        let runs = log_dir.as_ref().map(|d| d.join("runs.jsonl"));
        let mut mem = Memory::new();
        let card_stats = Rc::new(RefCell::new(learn::CardStats::new(log_dir.as_ref().map(|d| d.join("card_stats.json")))));
        mem.card_stats = Some(card_stats.clone());
        if let Some(runs) = &runs {
            learn::seed_from_history(&mut card_stats.borrow_mut(), Some(runs));
            let recent: Vec<String> = learn::history(Some(runs)).map(|r| r["character"].text()).collect(); // (the rotation goes on across restarts)
            mem.recent = recent[recent.len().saturating_sub(RECENT_RUNS)..].to_vec();
        }
        relics::load(runs.as_deref(), events::_ANCIENT.keys()); // what the Ancients' relics did in our runs
        let _upgrades: Rc<RefCell<HashMap<String, Option<f64>>>> = Rc::new(RefCell::new(HashMap::new()));
        {
            let (client, upgrades) = (client.clone(), _upgrades.clone());
            mem.upgrade_gain = Some(Rc::new(move |card: &Value| _upgrade_gain(&client, &upgrades, card)));
        }
        let mut knobs = learn::Knobs::new(log_dir.as_ref().map(|d| d.join("tuning.json")), None);
        if tune {
            knobs.draw();
        }
        let (_last_change, _last_retry) = (clock(), clock());
        let _fight_watch = FightWatch::new(log_dir.as_ref().map(|d| d.join("fights.jsonl")));
        Bot {
            client,
            character,
            log_dir,
            poll,
            room_pause: ROOM_PAUSE,
            room_poll: ROOM_POLL,
            fight_poll: FIGHT_POLL,
            _seen: (0.0, None),
            settle,
            travel_settle: 8.0,
            quiet_settle: 0.3,
            choose_settle: 0.5,
            careful_until: 0.0,
            careful_at: None,
            _crashed: Vec::new(),
            _last_sent: None,
            time_scale: 0.0,
            _scale_at: 0.0,
            stuck_timeout,
            clock,
            sleep,
            _pace: IndexMap::new(),
            mem,
            card_stats,
            _upgrades,
            knobs,
            _knobs_whole_run: false,
            tune,
            max_runs: None,
            abandon_next: false,
            runs_finished: 0,
            wins: 0,
            _strikes: HashMap::new(),
            _dead_buys: HashMap::new(),
            _last_fp: String::new(),
            _last_change,
            _last_retry,
            _over_logged: false,
            _prev_kind: Value::Null,
            _boss_watch: BossWatch::new(),
            _fight_watch,
            _turn_key: None,
            _turn_plays: 0,
            _turn_cards: 0,
            _turn_names: Vec::new(),
            _stuck: BTreeSet::new(),
            _dislodged: false,
            _fight_state: None,
            _turn_energy: 0,
            _turns: Vec::new(),
            _last_where: None,
            _seen_powers: HashSet::new(),
            _keys_seen: None,
            _wins_seen: None,
            _wins_asked: false,
            _timeline_idle: 0,
            _facing: Value::Null,
            _facing_fight: None,
            _threat: IndexMap::new(),
            _threat_fight: None,
            _revive_fight: None,
            _end_check: None,
            _played_from: Value::Null,
            _fight_kind: "monster".to_string(),
            _played_at: 0.0,
        }
    }

    /// `self.clock()`.
    pub fn clock(&self) -> f64 {
        (self.clock)()
    }

    /// `self.sleep(seconds)`.
    pub fn sleep(&self, seconds: f64) {
        (self.sleep)(seconds)
    }

    /// Changes whenever the bot gets somewhere new (used to spot repeat hangs).
    pub fn progress(&self) -> Progress {
        Progress(self.runs_finished, self._last_where)
    }

    pub fn _careful(&self) -> bool {
        self.clock() < self.careful_until && (self.careful_at.is_none() || self.careful_at == Some(self.progress()))
    }

    /// The game just died: never ask its last request again in that room. True if that is something new to avoid.
    pub fn note_crash(&mut self) -> bool {
        let Some(sent) = self._last_sent.take() else {
            return false;
        };
        if self.crashed_on(&sent.0, &sent.1, &sent.2, &sent.3) {
            return false;
        }
        crate::log_warning!(LOG, "the game died after {} {} at act {} floor {}: that will not be tried there again", sent.2, sent.3, py::str_of(&sent.0), py::str_of(&sent.1));
        self._crashed.push(sent);
        true
    }

    /// `(act, floor, action, params) in self._crashed`.
    fn crashed_on(&self, act: &Value, floor: &Value, action: &str, params: &str) -> bool {
        self._crashed.iter().any(|c| _eq(&c.0, act) && _eq(&c.1, floor) && c.2 == action && c.3 == params)
    }

    pub fn candidates(&mut self, state: &Value) -> Vec<Action> {
        let kind = &state["state_type"];
        if kind == "menu" {
            if state["menu_screen"] == "character_select" && !self.mem.character_selected {
                self._refresh_ascensions();
            }
            if self.abandon_next && (!state["menu_screen"].truthy() || state["menu_screen"] == "main") {
                let names: Vec<&Value> = state["options"].list().iter().map(|o| if o.is_string() { o } else { &o["name"] }).collect();
                if names.iter().any(|n| *n == "abandon_run") {
                    return vec![("menu_select".to_string(), json!({"option": "abandon_run"}))];
                }
                // No run to abandon only once the menu offers a new one (just after a launch it lists neither for a moment).
                if !names.iter().any(|n| *n == "singleplayer") {
                    return vec![];
                }
                self.abandon_next = false;
            }
            return strategy::menu(state, &mut self.mem, &self.character);
        }
        if in_combat(kind) {
            return self._combat(state);
        }
        if kind == "fake_merchant" {
            return strategy::shop(state, &mut self.mem, "fake_merchant");
        }
        let mut own: Option<Value> = None; // (the state with what the bot adds to it)
        if kind == "hand_select" {
            let mut s = state.clone();
            s["_fight"] = json!(self._fight_kind); // the screen does not say what kind of fight it is in
            own = Some(s);
        }
        let in_fight = kind == "card_select" || (kind == "card_reward" && state["card_reward"]["in_combat"].truthy());
        if in_fight && self._turn_key.as_ref().map(|k| k.0) == Some(strategy::_where(state)) {
            let mut s = own.take().unwrap_or_else(|| state.clone());
            let mut turns: Vec<Value> = self._turns.iter().map(|t| json!([t.0, t.1])).collect();
            turns.push(json!([self._turn_cards, self._turn_energy]));
            s["_turns"] = Value::Array(turns); // ... nor how the fight has gone
            s["_fight_state"] = self._fight_state.clone().unwrap_or(Value::Null); // ... nor the enemies, the hand or the energy (a potion's offer is for this turn)
            own = Some(s);
        }
        let state = own.as_ref().unwrap_or(state);
        let mem = &mut self.mem;
        match kind.as_str().unwrap_or("") {
            "hand_select" => strategy::hand_select(state, mem),
            "rewards" => strategy::rewards(state, mem),
            "card_reward" => strategy::card_reward(state, mem),
            "map" => strategy::map_screen(state, mem),
            "event" => strategy::event(state, mem),
            "rest_site" => strategy::rest_site(state, mem),
            "shop" => strategy::shop(state, mem, "shop"),
            "treasure" => strategy::treasure(state, mem),
            "card_select" => strategy::card_select(state, mem),
            "bundle_select" => strategy::bundle_select(state, mem),
            "relic_select" => strategy::relic_select(state, mem),
            "crystal_sphere" => strategy::crystal_sphere(state, mem),
            "game_over" => strategy::game_over(state, mem),
            "overlay" => strategy::overlay(state, mem),
            _ => vec![],
        }
    }

    /// What upgrading this card adds, from the game's own text for both versions.
    pub fn _upgrade_gain(&self, card: &Value) -> Option<f64> {
        _upgrade_gain(&self.client, &self._upgrades, card)
    }

    /// The game's own records (None if they cannot be asked for: an old mod or a hiccup must not stop the bot).
    pub fn _profile(&self) -> Option<Value> {
        let profile = self.client.borrow_mut().profile();
        match profile {
            Ok(profile) => profile.is_object().then_some(profile),
            Err(e) => {
                crate::log_debug!(LOG, "could not read the profile: {}", e);
                None
            }
        }
    }

    pub fn _epochs(profile: &Value) -> IndexMap<String, String> {
        profile["epochs"].list().iter().filter(|e| e["id"].truthy()).map(|e| (py::str_of(&e["id"]), py::str_of(&e["state"]))).collect()
    }

    /// Ask the game how far each character has climbed (used to pick who plays next).
    pub fn _refresh_ascensions(&mut self) {
        let Some(profile) = self._profile() else {
            return;
        };
        if profile["total_wins"].is_number() || profile["total_wins"].is_boolean() {
            self._wins_seen = Some(profile["total_wins"].int());
        }
        // (What Python raised on a profile of another shape, an AttributeError, TypeError or ValueError, is the `Err` here.)
        let read = || -> Result<(IndexMap<String, i64>, IndexMap<String, String>), String> {
            let listed = |v: &Value, what: &str| if v.truthy() && !v.is_array() { Err(format!("{what} is not a list")) } else { Ok(()) };
            listed(&profile["characters"], "characters")?;
            let mut climbed = IndexMap::new();
            for c in profile["characters"].list() {
                if !c.is_object() {
                    return Err(format!("{} object has no attribute 'get'", py::repr_of(c)));
                }
                if !c["id"].truthy() {
                    continue;
                }
                let top = &c["max_ascension"];
                let n = match top {
                    Value::String(s) if !s.is_empty() => s.trim().parse::<i64>().map_err(|_| format!("invalid literal for int() with base 10: {}", py::str_repr(s)))?,
                    Value::Array(a) if !a.is_empty() => return Err("int() argument must be a string, a bytes-like object or a real number, not 'list'".to_string()),
                    Value::Object(m) if !m.is_empty() => return Err("int() argument must be a string, a bytes-like object or a real number, not 'dict'".to_string()),
                    other => other.int(),
                };
                climbed.insert(py::str_of(&c["id"]), n);
            }
            listed(&profile["epochs"], "epochs")?;
            if let Some(e) = profile["epochs"].list().iter().find(|e| !e.is_object()) {
                return Err(format!("{} object has no attribute 'get'", py::repr_of(e)));
            }
            Ok((climbed, Bot::_epochs(&profile)))
        };
        match read() {
            Ok((climbed, epochs)) => {
                self.mem.max_ascension = climbed;
                self.mem.epochs = epochs;
            }
            Err(e) => crate::log_debug!(LOG, "could not read the profile: {}", e),
        }
    }

    pub fn _combat(&mut self, state: &Value) -> Vec<Action> {
        let (battle, player) = (&state["battle"], &state["player"]);
        if !battle.truthy() || !(player.truthy() && player.has("hand")) {
            return vec![]; // combat is starting or just ended
        }
        if battle["turn"] != "player" || !battle["is_play_phase"].truthy() {
            return vec![];
        }
        self._fight_kind = py::str_of(&state["state_type"]);
        self._note_deck(state);
        let at = strategy::_where(state);
        let key: TurnKey = (at, battle["round"].clone());
        let stuck: BTreeSet<String> = battle["enemies"].list().iter().filter(|e| e["hp"].int() > 0 && combat::power_amount(e, "IMPALED") > 0).map(|e| py::str_of(&e["entity_id"])).collect();
        self._dislodged = Some(&key) == self._turn_key.as_ref() && (self._dislodged || self._stuck.difference(&stuck).next().is_some());
        self._stuck = stuck;
        if Some(&key) != self._turn_key.as_ref() {
            // (What each turn of the fight came to: the Knowledge Demon's offers are judged by it, see afflictions.rs.)
            if Some(at) == self._turn_key.as_ref().map(|k| k.0) {
                self._turns.push((self._turn_cards, self._turn_energy));
            } else {
                self._turns = Vec::new();
            }
            (self._turn_key, self._turn_plays, self._turn_cards, self._turn_names) = (Some(key.clone()), 0, 0, Vec::new());
            // Each enemy's damage over the fight: one intent says little, the average says who is doing the damage.
            if Some(at) != self._threat_fight {
                (self._threat_fight, self._threat) = (Some(at), IndexMap::new());
            }
            let calm = stance::TAKEN.get(stance::read(&player["stance"])).copied().unwrap_or(1.0); // (the first game's intents show Wrath's doubling)
            for e in battle["enemies"].list() {
                let seen = self._threat.entry(py::str_of(&e["entity_id"])).or_insert((0, 0, 0.0, 0));
                let hit: i64 = e["intents"].list().iter().map(combat::intent_damage).sum();
                seen.0 += hit;
                seen.1 += 1;
                if hit != 0 {
                    // its blows, as they would be outside Wrath (what ending a turn in Wrath risks)
                    (seen.2, seen.3) = (seen.2 + hit as f64 / calm, seen.3 + 1);
                }
            }
        }
        self._turn_energy = player["energy"].int();
        if Some(at) != self._facing_fight {
            (self._facing_fight, self._facing) = (Some(at), Value::Null); // a new fight
        }
        if self._turn_plays >= MAX_PLAYS_PER_TURN {
            return vec![("end_turn".to_string(), json!({}))];
        }
        // A card just played lingers in the hand: sending it again (and again) left the game hanging at the end of turn.
        if !self._played_from.is_null() && Bot::_hand_sig(state) == self._played_from && self.clock() - self._played_at < 2.0 {
            return vec![];
        }
        // An enemy that revives stronger (Adaptable) leaves the field for the rest of the turn it is beaten in: it is coming back.
        let standing: Vec<&Value> = battle["enemies"].list().iter().filter(|e| e["hp"].int() > 0).collect();
        if !standing.is_empty() {
            self._revive_fight = if standing.iter().any(|e| combat::power_amount(e, "ADAPTABLE") != 0) { Some(at) } else { None };
        }
        let revive = standing.is_empty() && self._revive_fight == Some(at);
        let threat: Map<String, Value> = self._threat.iter().map(|(k, (total, rounds, _b, _n))| (k.clone(), json!(*total as f64 / *rounds as f64))).collect();
        // (its blow, how often it attacks)
        let blow: Map<String, Value> =
            self._threat.iter().filter(|(_, t)| t.3 != 0).map(|(k, (_t, rounds, blows, attacks))| (k.clone(), json!([*blows / *attacks as f64, *attacks as f64 / *rounds as f64]))).collect();
        let mut costs = Map::new(); // (as the fight began: Void Form hides them)
        for c in &self.mem.deck {
            costs.insert(py::str_of(&c["name"]), json!(cards::_cost(&c["cost"])));
        }
        let mut fight_state = state.clone();
        fight_state["_plays_this_turn"] = json!(self._turn_cards);
        fight_state["_played_names"] = json!(self._turn_names);
        fight_state["_dislodged"] = json!(self._dislodged);
        fight_state["_facing"] = self._facing.clone();
        fight_state["_revive_pending"] = json!(revive);
        fight_state["_threat"] = Value::Object(threat);
        fight_state["_blow"] = Value::Object(blow);
        fight_state["_costs"] = Value::Object(costs);
        let wanted = combat::actions(&fight_state);
        self._fight_state = Some(fight_state);
        // Drawn cards (or Reboot's new hand) show up a moment late: look once more before ending a turn with energy left.
        let ending = wanted.first().is_some_and(|w| w.0 == "end_turn");
        if ending && self._turn_plays != 0 && player["energy"].int() > 0 {
            let check = (key, self._turn_plays, player["hand"].list().iter().map(|c| py::str_of(&c["name"])).collect::<Vec<String>>());
            if Some(&check) != self._end_check.as_ref() {
                self._end_check = Some(check);
                return vec![];
            }
        }
        if ending && LOG.enabled(Level::Debug) {
            if let Some(expect) = combat::expected_end_loss(state) {
                crate::log_debug!(LOG, "ending r{} on {} HP, expecting to lose {}", py::str_of(&battle["round"]), py::str_of(&player["hp"]), expect);
            }
        }
        wanted
    }

    /// The hand and what we have to spend: unchanged until a played card has left it.
    pub fn _hand_sig(state: &Value) -> Value {
        let player = &state["player"];
        let hand: Vec<Value> = player["hand"].list().iter().map(|c| json!([c["index"], c["name"]])).collect();
        json!([state["battle"]["round"], hand, player["energy"], player["stars"]])
    }

    /// Remember the deck as a fight begins: later the piles also hold what the fight added (Wounds, Shivs, Souls).
    pub fn _note_deck(&mut self, state: &Value) {
        let (player, battle) = (&state["player"], &state["battle"]);
        let fresh = battle["round"].int() == 1 && player["discard_pile_count"].int() == 0 && player["exhaust_pile_count"].int() == 0;
        if fresh && player["max_energy"].int() > 0 {
            self.mem.max_energy = player["max_energy"].int();
        }
        if !fresh {
            if self.mem.deck_size <= 0 {
                self.mem.deck_size = player["hand"].list().len() as i64 + player["draw_pile_count"].int();
            }
            return;
        }
        let keep = ["id", "name", "type", "cost", "star_cost", "description", "rarity", "is_upgraded"];
        let pile = &player["draw_pile"];
        let whole = pile.is_array() && pile.list().len() as i64 == player["draw_pile_count"].int();
        let cards: Vec<Value> = player["hand"]
            .list()
            .iter()
            .chain(if whole { pile.list() } else { &[] })
            .map(|c| Value::Object(keep.iter().filter(|k| !c[**k].is_null()).map(|k| (k.to_string(), c[*k].clone())).collect()))
            .collect();
        if whole {
            self.mem.deck = cards.into_iter().filter(|c| py::str_of(&c["type"]) != "Status" && !NOT_OURS.contains(&py::str_of(&c["name"]).as_str())).collect();
            self.mem.deck_size = self.mem.deck.len() as i64;
        } else {
            // a mod that does not show the piles: the count has to do
            self.mem.deck_size = cards.len() as i64 + player["draw_pile_count"].int();
        }
    }

    pub fn _after_success(&mut self, state: &Value, action: &str, params: &Value) {
        let kind = &state["state_type"];
        if action == "select_card" {
            if let Some(index) = params["index"].as_i64() {
                self.mem.selected.insert(index);
            }
        } else if action == "select_card_reward" && !state["card_reward"]["in_combat"].truthy() {
            // (a card for this fight only is no pick)
            for c in state["card_reward"]["cards"].list() {
                if _eq(&c["index"], &params["card_index"]) {
                    self.mem.deck.push(Value::Object(c.entries().filter(|(k, _)| !["index", "keywords"].contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()));
                    self.mem.deck_size += 1;
                    self.mem.picks.push((state["run"]["floor"].int(), py::str_of(&c["name"])));
                }
            }
        } else if action == "skip_card_reward" || action == "discard_potion" {
            let (table, at) = (if action == "skip_card_reward" { &mut self.mem.card_skips } else { &mut self.mem.potion_swaps }, strategy::_where(state));
            *table.entry(at).or_insert(0) += 1;
        } else if action == "play_card" || action == "use_potion" {
            self._turn_plays += 1;
            if action == "play_card" {
                self._turn_cards += 1; // (apart from potions: under Sloth a potion and two cards are not three cards)
                (self._played_from, self._played_at) = (Bot::_hand_sig(state), self.clock());
                let hand = state["player"]["hand"].list();
                self._turn_names.extend(hand.iter().filter(|c| _eq(&c["index"], &params["card_index"])).map(|c| py::str_of(&c["name"])));
            }
            if params["target"].truthy() {
                self._facing = params["target"].clone(); // we turn towards whatever we aim at
            }
        } else if action == "menu_select" && kind == "menu" {
            let option = py::str_of(&params["option"]);
            if state["menu_screen"] == "character_select" && !["confirm", "embark"].contains(&option.as_str()) {
                self.mem.character_selected = true;
            } else if option == "abandon_run" {
                crate::log_warning!(LOG, "abandoning the current run");
                self.careful_until = 0.0;
                self._crashed.clear();
            } else if ["yes", "confirm"].contains(&option.as_str()) && self.abandon_next && state["menu_screen"] == "popup" {
                self.abandon_next = false;
                self.mem.reset_run();
            }
        }
    }

    /// Follow the Timeline: the mod reveals one thing per click and says when nothing is left.
    pub fn _after_timeline_click(&mut self, params: &Value, result: &Value) {
        let message = result["message"].text();
        if message.starts_with("Revealing epoch") || message.starts_with("Confirmed") {
            crate::log_info!(LOG, "timeline: {}", message);
        }
        if py::str_of(&params["option"]) == "advance" {
            let progressed = ["Revealing", "Confirmed", "Closed", "Clicked", "Opening"].iter().any(|p| message.starts_with(p));
            // "Done" must hold for a few clicks (between animations the Timeline looks finished), or nothing happen for long.
            let done = result["done"].truthy();
            self._timeline_idle = if progressed { 0 } else { self._timeline_idle + if done { 3 } else { 1 } };
            if self._timeline_idle >= if done { 9 } else { 45 } {
                self._timeline_idle = 0;
                self.mem.timeline_done = true;
                self.mem.timeline_stuck = result["pending_epoch_ids"].list().iter().map(py::str_of).collect(); // cannot be revealed yet: do not come back for it
            }
        }
        self.sleep(0.6); // let the reveal animation run before the next click
    }

    /// Log the text of each power, odd card, keyword and potion the first time it is met (to notice what the planner lacks).
    pub fn _note_powers(&mut self, state: &Value) {
        let seen_powers = &mut self._seen_powers;
        let mut once = |seen: String, message: &dyn Fn() -> String| {
            if !seen_powers.contains(&seen) {
                seen_powers.insert(seen);
                crate::log_info!(LOG, "{}", message());
            }
        };
        let s = py::str_of;
        let player = &state["player"];
        for c in std::iter::once(player).chain(state["battle"]["enemies"].list()) {
            for p in c["status"].list() {
                let on = c["name"].or(&c["character"]);
                once(s(&p["id"]), &|| format!("power {} (on {}): {}", s(&p["id"]), if on.truthy() { s(on) } else { "player".to_string() }, s(&p["description"])));
            }
        }
        for c in player["hand"].list() {
            if ["Status", "Curse"].contains(&s(&c["type"]).as_str()) {
                once(format!("card:{}", s(&c["name"])), &|| format!("card {} ({}, cost {}): {}", s(&c["name"]), s(&c["type"]), s(&c["cost"]), s(&c["description"])));
            }
            for k in c["keywords"].list() {
                once(format!("kw:{}", s(&k["name"])), &|| format!("keyword {} (on {}): {}", s(&k["name"]), s(&c["name"]), s(&k["description"])));
            }
        }
        for p in player["potions"].list() {
            // (the mod's wiki has no potions)
            once(format!("potion:{}", s(&p["name"])), &|| format!("potion {} ({}): {}", s(&p["name"]), s(&p["target_type"]), s(&p["description"])));
        }
    }

    pub fn _note_state(&mut self, state: &Value) {
        let kind = &state["state_type"];
        let prev = std::mem::replace(&mut self._prev_kind, kind.clone());
        let ended = self._boss_watch.update(state, kind.as_str());
        if let Some(ended) = ended.as_ref().filter(|e| !e.is_empty()) {
            crate::log_info!(LOG, "{}", ended);
        }
        if let Some(brought) = self._boss_watch.brought.take() {
            if !brought.is_empty() {
                crate::log_info!(LOG, "{}", brought);
            }
        }
        self._fight_watch.update(state, kind.as_str());
        if in_combat(kind) {
            self._note_powers(state);
        }
        if state["run"].truthy() {
            if self._wins_seen.is_none() && kind != "game_over" && !self._wins_asked {
                // Started mid-run, the bot has not read the win counter in the menu (else the run's end falls back on HP).
                self._wins_asked = true;
                self._wins_seen = self._profile_wins();
            }
            let at = strategy::_where(state);
            if Some(at) != self._last_where {
                self._last_where = Some(at);
                let (p, run) = (&state["player"], &state["run"]);
                let s = py::str_of;
                crate::log_info!(LOG, "act {} floor {}  {}  hp {}/{}  gold {}", s(&run["act"]), s(&run["floor"]), s(kind), s(&p["hp"]), s(&p["max_hp"]), s(&p["gold"]));
            }
            let held = &state["run"]["keys"];
            if !held.is_null() && Some(held) != self._keys_seen.as_ref() {
                if self._keys_seen.is_some() || held.truthy() {
                    let names = held.list().iter().map(py::str_of).collect::<Vec<_>>().join(", ");
                    crate::log_info!(LOG, "keys: {}", if names.is_empty() { "none" } else { &names });
                }
                self._keys_seen = Some(held.clone());
            }
        }
        if kind == "game_over" {
            if !self._over_logged {
                self._over_logged = true;
                // A death: straight out of a fight, or a boss fight the boss watch saw lost (a won last boss leaves its fight first).
                self._record_run(state, in_combat(&prev) || ended.is_some_and(|e| e.contains(" lost;")));
            }
        } else if self._over_logged {
            self._over_logged = false;
            self._keys_seen = None;
            self.mem.reset_run();
            self._crashed.clear();
            if self.tune {
                let drawn = self.knobs.draw();
                crate::log_info!(LOG, "settings for the next run: {{{}}}", drawn.iter().map(|(k, v)| format!("{}: {}", py::str_repr(k), py::float_repr(*v))).collect::<Vec<_>>().join(", "));
                self._knobs_whole_run = true;
            }
            self.mem.runs_finished = self.runs_finished;
        }
    }

    /// Total wins according to the game's own records (None if it cannot be asked).
    pub fn _profile_wins(&self) -> Option<i64> {
        let profile = self._profile()?;
        let wins = &profile["total_wins"];
        (wins.is_number() || wins.is_boolean()).then(|| wins.int())
    }

    /// Log the Timeline epochs the run just finished has earned, and remember where the Timeline stands.
    pub fn _note_epochs_earned(&mut self) {
        let Some(profile) = self._profile() else {
            return;
        };
        let epochs = Bot::_epochs(&profile);
        let earned = |table: &IndexMap<String, String>| -> BTreeSet<String> { table.iter().filter(|(_, v)| strategy::_EARNED.contains(&v.as_str())).map(|(k, _)| k.clone()).collect() };
        let had = earned(&self.mem.epochs);
        let new: Vec<String> = earned(&epochs).into_iter().filter(|k| !had.contains(k)).collect(); // (sorted: a BTreeSet's order)
        if !new.is_empty() && !self.mem.epochs.is_empty() {
            // (a bot started mid-run has not seen the Timeline yet: everything would look new)
            crate::log_info!(LOG, "epochs earned this run: {}", new.join(", "));
        }
        self.mem.epochs = epochs;
    }

    /// (The Python's default: `died` false.)
    pub fn _record_run(&mut self, state: &Value, died: bool) {
        let (p, run) = (&state["player"], &state["run"]);
        // The end screen looks the same for a win and a loss, and its HP is unreliable; the game's win counter goes up on a win.
        let (before, mut now) = (self._wins_seen, self._profile_wins());
        if before.is_some() && now == before && run["act"].int() >= 3 && !died {
            // The game records a win a moment after the end screen comes up (not awaited after a death: no win is coming).
            for _ in 0..8 {
                self.sleep(0.5);
                now = self._profile_wins();
                if now.is_none() || now > before {
                    break;
                }
            }
        }
        self._note_epochs_earned();
        if now.is_some() {
            self._wins_seen = now;
        }
        let won = match (before, now) {
            (Some(before), Some(now)) => now > before,
            _ => p["hp"].int() > 0,
        };
        self.runs_finished += 1;
        self.wins += won as i64;
        self.mem.last_result = if won { "win" } else { "loss" }.to_string();
        self.mem.recent.push(p["character"].text());
        if self.mem.recent.len() > RECENT_RUNS {
            self.mem.recent.drain(..self.mem.recent.len() - RECENT_RUNS);
        }
        let mut deck: Vec<String> = self.mem.deck.iter().map(|c| py::str_of(&c["name"])).collect();
        deck.sort();
        let mut entry = json!({
            "time": _now(), "character": p["character"], "result": if won { "win" } else { "loss" },
            "act": run["act"], "floor": run["floor"], "ascension": run["ascension"], "hp": p["hp"], "max_hp": p["max_hp"],
            "gold": p["gold"], "relics": p["relics"].list().iter().map(|r| r["id"].clone()).collect::<Vec<Value>>(), "deck_size": self.mem.deck_size,
            "deck": deck, "picks": self.mem.picks.iter().map(|(floor, name)| json!([floor, name])).collect::<Vec<Value>>(),
        });
        if !run["keys"].is_null() {
            entry["keys"] = run["keys"].clone(); // (the first game's act 4 keys, once act 4 is open)
        }
        let who = entry["character"].text();
        let usual = self.card_stats.borrow().usual(&who, 0);
        let floor = &entry["floor"];
        if let (true, Some(usual), true) = (self._knobs_whole_run, usual, floor.is_int() || floor.is_boolean()) {
            self.knobs.record(Some((floor.int() + if won { learn::WIN_BONUS } else { 0 }) as f64 - usual));
        }
        entry["knobs"] = if self._knobs_whole_run { Value::Object(learn::knobs().into_iter().map(|(k, v)| (k, json!(v))).collect()) } else { Value::Null };
        self.card_stats.borrow_mut().record(&who, entry["floor"].int(), won, &self.mem.picks);
        relics::note(&entry, true);
        crate::log_info!(LOG, "run {} over: {} on act {} floor {} ({} wins so far)", self.runs_finished, py::str_of(&entry["result"]), py::str_of(&entry["act"]), py::str_of(&entry["floor"]), self.wins);
        if let Some(dir) = &self.log_dir {
            let written = std::fs::create_dir_all(dir).and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(dir.join("runs.jsonl"))).and_then(|mut f| f.write_all(format!("{}\n", py::json_dumps(&entry)).as_bytes()));
            if let Err(e) = written {
                panic!("could not write {}: {}", dir.join("runs.jsonl").display(), e); // (Python's OSError out of the loop)
            }
        }
    }

    /// Actions that work without the state changing (ticking a grid card, picking the highlighted character): no settle wait.
    pub fn _is_quiet(state: &Value, action: &str, params: &Value) -> bool {
        if action == "select_card" {
            return state["card_select"]["screen_type"] != "choose";
        }
        if action == "menu_select" && state["menu_screen"] == "character_select" {
            return !["confirm", "embark"].contains(&py::str_of(&params["option"]).as_str());
        }
        if action == "menu_select" && state["menu_screen"] == "timeline" {
            return true; // the Timeline animates between clicks; the state says nothing about it
        }
        false
    }

    /// Picking one card on a "choose 1 of 3" screen: ignored while the screen slides in, so re-sent after a short wait.
    pub fn _is_choose(state: &Value, action: &str) -> bool {
        action == "select_card" && state["card_select"]["screen_type"] == "choose"
    }

    /// Wait for the action to show; true if the state changed. With `leave`, until that screen is gone (the map answers during travel).
    /// (The Python's defaults: `leave` None, `quiet` false, `settle` None, `slow` false.)
    pub fn _wait_for_change(&mut self, fp: &str, leave: Option<&str>, quiet: bool, settle: Option<f64>, slow: bool) -> Result<bool, BotError> {
        let deadline = self.clock()
            + match settle {
                Some(settle) => settle,
                None if leave.is_some() => self.travel_settle,
                None if quiet => self.quiet_settle,
                None => self.settle,
            };
        let mut changed = false;
        // Ask less often while a room loads (a state request walks the scene being rebuilt; rapid polling crashed the game).
        let mut pause = (if leave.is_some() || slow { self.room_poll } else { 0.03 }).min(self.poll);
        if self._careful() {
            pause = pause.max(0.5);
        }
        while self.clock() < deadline {
            if both::stopped() {
                return Err(KeyboardInterrupt.into());
            }
            let t = Instant::now();
            self.sleep(pause);
            self._timed("wait", t);
            let t = Instant::now();
            let state = self.client.borrow_mut().state();
            self._timed("look", t);
            let state = state?;
            changed = changed || fingerprint(&state) != fp;
            let gone = leave.is_some_and(|leave| state["state_type"] != leave);
            self._seen = (self.clock(), Some(state)); // (the next step starts from it: asking again a moment later only costs the game a state)
            if leave.is_some() {
                if gone {
                    return Ok(true);
                }
            } else if changed {
                return Ok(true);
            }
        }
        Ok(changed)
    }

    /// One read-decide-act cycle. Returns true if an action was sent.
    pub fn step(&mut self) -> Result<bool, BotError> {
        let now = self.clock();
        if self.time_scale != 0.0 && now - self._scale_at > 3.0 {
            self._scale_at = now;
            self.client.borrow_mut().act("set_time_scale", &json!({"scale": self.time_scale}))?;
        }
        let (seen_at, seen) = std::mem::replace(&mut self._seen, (0.0, None));
        let t = Instant::now();
        let state = match seen {
            Some(seen) if now - seen_at <= FRESH => Ok(seen),
            _ => self.client.borrow_mut().state(),
        };
        self._timed("look", t);
        let state = state?;
        let fp = fingerprint(&state);
        if fp != self._last_fp {
            (self._last_fp, self._last_change, self._last_retry) = (fp.clone(), now, now);
            self._strikes.clear();
        } else if now - self._last_change > self.stuck_timeout {
            return Err(Stuck(format!("no change for {:.0}s in state {}", self.stuck_timeout, py::repr_of(&state["state_type"]))).into());
        } else if now - self._last_retry > 8.0 {
            // clicks sent mid-animation are ignored; try again soon
            self._last_retry = now;
            self._strikes.clear(); // give shelved actions another go
        }

        self._note_state(&state);
        if self._done() {
            return Ok(false);
        }
        let t = Instant::now();
        let mut todo = self.candidates(&state);
        self._timed("plan", t);
        let run = &state["run"];
        let (act, floor) = (&run["act"], &run["floor"]);
        if !self._crashed.is_empty() {
            let suspect = |a: &Action| self.crashed_on(act, floor, &a.0, &dumps_sorted(&a.1));
            if todo.iter().any(|a| !suspect(a)) {
                todo = todo.iter().filter(|a| !suspect(a)).cloned().collect(); // (when it is all there is to do, it is done again)
            }
        }
        for (action, params) in todo {
            let key = (action.clone(), dumps_sorted(&params));
            if self._strikes.get(&key).copied().unwrap_or(0) >= MAX_STRIKES {
                continue;
            }
            let buy = (action == "shop_purchase").then(|| (py::json_dumps(act), py::json_dumps(floor), key.1.clone()));
            if buy.as_ref().is_some_and(|buy| self._dead_buys.get(buy).copied().unwrap_or(0) >= MAX_STRIKES) {
                continue;
            }
            if action != "menu_select" && !in_combat(&state["state_type"]) {
                self._last_sent = Some((act.clone(), floor.clone(), key.0.clone(), key.1.clone()));
            }
            let t = Instant::now();
            let result = self.client.borrow_mut().act(&action, &params);
            self._timed("act", t);
            let result = result?;
            if result["status"] == "error" || result.has("error") {
                *self._strikes.entry(key).or_insert(0) += 1;
                crate::log_debug!(LOG, "{} {} rejected: {}", action, py::repr_of(&params), py::str_of(&result["error"]));
                self.sleep(self.poll.min(0.15));
                return Ok(true);
            }
            crate::log_debug!(LOG, "{} {} -> {}", action, py::repr_of(&params), py::str_of(&result["message"]));
            self._after_success(&state, &action, &params);
            if action == "menu_select" && state["menu_screen"] == "timeline" {
                self._after_timeline_click(&params, &result);
            }
            let (quiet, choose) = (Bot::_is_quiet(&state, &action, &params), Bot::_is_choose(&state, &action));
            // Leaving a room, buying and embarking rebuild the scene (rapid polling or clicks then crashed the game): pause, then poll less.
            let fragile = action == "proceed" || action == "shop_purchase";
            let embarking = action == "menu_select" && state["menu_screen"] == "character_select" && ["confirm", "embark"].contains(&py::str_of(&params["option"]).as_str());
            if (fragile || embarking) && self.room_pause != 0.0 {
                self.sleep(self.room_pause);
            }
            if self._careful() {
                self.sleep(1.0);
            }
            let leave = if action == "choose_map_node" {
                Some("map")
            } else if embarking {
                Some("menu")
            } else {
                None
            };
            let changed = self._wait_for_change(&fp, leave, quiet, choose.then_some(self.choose_settle), fragile)?;
            if !quiet && !choose && !changed {
                *self._strikes.entry(key).or_insert(0) += 1; // accepted, but nothing happened
                if let Some(buy) = buy {
                    *self._dead_buys.entry(buy).or_insert(0) += 1;
                }
            }
            return Ok(true);
        }
        // Nothing to send: in a fight that is the enemy's turn or a look again for late draws, and the next look comes sooner
        // (after every card the bot already asks every 0.03 s; only a room being rebuilt needs the slow pace, see _wait_for_change).
        let fight = in_combat(&state["state_type"]) && state["battle"].truthy();
        let t = Instant::now();
        self.sleep(if fight { self.poll.min(self.fight_poll) } else { self.poll });
        self._timed("idle", t);
        Ok(false)
    }

    /// `with self._timed(kind):` — the wall time since `since` goes to that kind of work.
    pub fn _timed(&mut self, kind: &'static str, since: Instant) {
        *self._pace.entry(kind).or_insert(0.0) += since.elapsed().as_secs_f64();
    }

    /// Where the time went (2026-11-26: x16 on the game's clock left a floor at 2.3 s, as x6 had: the bot's own loop sets the pace).
    pub fn _pace_line(&mut self, steps: i64, since: Instant) {
        let wall = since.elapsed().as_secs_f64().max(1e-9);
        let mut kinds: Vec<(&'static str, f64)> = self._pace.iter().map(|(k, v)| (*k, *v)).collect();
        py::sort_by_key(&mut kinds, |kv| -kv.1);
        let parts = kinds.iter().map(|(k, v)| format!("{} {:.0}%", k, 100.0 * v / wall)).collect::<Vec<_>>().join(", ");
        crate::log_info!(LOG, "pace: {} steps in {:.0} s ({:.0} ms a step): {}", steps, wall, 1000.0 * wall / steps.max(1) as f64, parts);
        self._pace.clear();
    }

    /// Ask the game afresh at the next step (after the game was restarted or reconnected to under the bot).
    pub fn forget(&mut self) {
        self._seen = (0.0, None);
    }

    /// True once the requested number of runs is over and its end screen is dismissed.
    pub fn _done(&self) -> bool {
        self.max_runs.is_some_and(|max_runs| self.runs_finished >= max_runs) && !self._over_logged
    }

    /// Play until `max_runs` runs have finished (forever if None). Told to stop (Ctrl+C, SIGTERM: `games::both::stopped()`),
    /// it leaves between two steps with `BotError::Interrupted`, where Python's KeyboardInterrupt came out of the loop.
    pub fn run(&mut self, max_runs: Option<i64>) -> Result<(), BotError> {
        self.max_runs = max_runs;
        self._last_change = self.clock();
        self.forget();
        let (mut steps, mut since) = (0, Instant::now());
        while !self._done() {
            if both::stopped() {
                return Err(KeyboardInterrupt.into());
            }
            self.step()?;
            steps += 1;
            if steps >= PACE_STEPS {
                self._pace_line(steps, since);
                (steps, since) = (0, Instant::now());
            }
        }
        Ok(())
    }
}

/// What upgrading this card adds, from the game's own text for both versions (`Bot._upgrade_gain`; also `Memory.upgrade_gain`,
/// which outlives any one borrow of the bot: hence the shared game and table).
pub fn _upgrade_gain(client: &SharedGame, upgrades: &Rc<RefCell<HashMap<String, Option<f64>>>>, card: &Value) -> Option<f64> {
    let cid = card["id"].text();
    if cid.is_empty() {
        return None;
    }
    if let Some(gain) = upgrades.borrow().get(&cid) {
        return *gain;
    }
    let entry = match client.borrow_mut().card(&cid) {
        Ok(entry) => entry,
        Err(e) => {
            // a hiccup must not hold up the choice
            crate::log_debug!(LOG, "could not look up {}: {}", cid, e);
            return None;
        }
    };
    let gain = entry.filter(|e| e.truthy() && e["base"].truthy() && e["upgraded"].truthy()).map(|e| cards::upgrade_gain(&e["base"], &e["upgraded"]));
    upgrades.borrow_mut().insert(cid, gain);
    gain
}
