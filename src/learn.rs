//! Card ratings learned from the bot's own runs (a card taken scores how much further its run got than the character's runs
//! usually get from that floor), and the settings the bot tunes as it plays.

use std::cell::RefCell;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use indexmap::IndexMap;
use serde_json::{Map, Value};

pub use crate::cards::key_of as card_key;
use crate::games;
use crate::logging::Logger;
use crate::base::{self, fmax, fmin, JsonExt, Random};

static LOG: Logger = Logger::new("sts2bot.learn");
pub const WIN_BONUS: i64 = 15; // a win counts as this many floors beyond the last one
pub const HISTORY: i64 = 300; // runs per character the "usual" result is taken from
pub const PRIOR_PICKS: f64 = 400.0; // picks before a card's own record counts for half (a run is a noisy measurement)
pub const FLOORS_PER_POINT: f64 = 1.0; // floors gained per pick, beyond the character's average pick, worth one point of rating
pub const MAX_SHIFT: f64 = 2.5; // a rating never moves further than this
pub const CURIOSITY: f64 = 0.4; // head start for a card never tried, so that it gets tried (shrinks with picks)
pub const KIND_PRIOR: f64 = 300.0; // picks of a kind (cards.kind) before the kind's record counts for half of where its cards start
pub const MAX_KIND: f64 = 1.5; // ... and a kind never moves its cards further than this

fn shown(path: &Path) -> String {
    path.display().to_string()
}

/// The JSON object kept in `path` ({} when there is none yet, or it cannot be read).
pub fn _load(path: Option<&Path>, what: &str) -> Map<String, Value> {
    let Some(path) = path else {
        return Map::new();
    };
    let text = match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => {
                crate::log_warning!(LOG, "could not read {} ({}); starting the {} afresh", shown(path), e, what);
                return Map::new();
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Map::new(),
        Err(e) => {
            crate::log_warning!(LOG, "could not read {} ({}); starting the {} afresh", shown(path), e, what);
            return Map::new();
        }
    };
    _loads(&text, &shown(path), what)
}

fn _loads(text: &str, path: &str, what: &str) -> Map<String, Value> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(data)) => data,
        Ok(_) => {
            crate::log_warning!(LOG, "could not read {} ({}); starting the {} afresh", path, "not a JSON object", what);
            Map::new()
        }
        Err(e) => {
            crate::log_warning!(LOG, "could not read {} ({}); starting the {} afresh", path, e, what);
            Map::new()
        }
    }
}

pub fn _save(path: Option<&Path>, data: &Value, what: &str) {
    let Some(path) = path else {
        return;
    };
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, base::json_dumps_pretty(data, 1, true))?;
        std::fs::rename(&tmp, path)
    };
    if let Err(e) = write() {
        crate::log_warning!(LOG, "could not save the {}: {}", what, e);
    }
}

fn number(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn row_value(row: &[f64; 2]) -> Value {
    Value::Array(vec![number(row[0]), number(row[1])])
}

/// `list(x)` of a [runs or picks, floors] row: two numbers, or what is wrong with it.
fn row_of(x: &Value) -> Result<[f64; 2], String> {
    match x.as_array() {
        Some(a) if a.len() == 2 && a.iter().all(|n| n.is_number()) => Ok([a[0].float(), a[1].float()]),
        _ => Err(format!("not a pair of numbers: {}", base::json_dumps(x))),
    }
}

/// `(data.get(key) or {}).items()`.
fn items<'a>(data: &'a Map<String, Value>, key: &str) -> Result<Vec<(&'a String, &'a Value)>, String> {
    match data.get(key) {
        Some(Value::Object(m)) => Ok(m.iter().collect()),
        Some(v) if v.truthy() => Err(format!("{} is not a JSON object", base::str_repr(key))),
        _ => Ok(Vec::new()),
    }
}

/// character or knob -> card or setting -> [picks or runs, floors gained in total]
pub type Rows = IndexMap<String, IndexMap<String, [f64; 2]>>;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct CardStats {
    pub path: Option<PathBuf>,
    /// character -> results of its latest runs
    pub finals: IndexMap<String, Vec<i64>>,
    /// character -> card -> [picks, floors gained in total]
    pub cards: Rows,
    /// card -> its kind (cards.kind), as the card last looked when offered
    pub kinds: IndexMap<String, String>,
    /// kind -> card -> [picks, floors gained beyond its character's par]
    pub _pool: Option<Rows>,
}

impl CardStats {
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut stats = CardStats { path, ..Default::default() };
        let data = _load(stats.path.as_deref(), "card statistics");
        if let Err(e) = stats.fill(&data) {
            let path = stats.path.as_deref().map(shown).unwrap_or_else(|| "None".to_string());
            crate::log_warning!(LOG, "could not use {} ({}); starting the card statistics afresh", path, e);
        }
        stats
    }

    /// What the file holds, table by table (the tables before one that cannot be used are kept, as each was read in turn).
    fn fill(&mut self, data: &Map<String, Value>) -> Result<(), String> {
        let mut finals = IndexMap::new();
        for (k, v) in items(data, "finals")? {
            let list = v.as_array().ok_or_else(|| format!("not a list: {}", base::json_dumps(v)))?;
            let mut out = Vec::with_capacity(list.len());
            for f in list {
                if !(f.is_int() || f.is_boolean()) {
                    return Err(format!("not a floor: {}", base::json_dumps(f)));
                }
                out.push(f.int());
            }
            finals.insert(k.clone(), out);
        }
        self.finals = finals;
        let mut cards = Rows::new();
        for (k, v) in items(data, "cards")? {
            let table = v.as_object().ok_or_else(|| format!("not a JSON object: {}", base::json_dumps(v)))?;
            let mut out = IndexMap::new();
            for (c, x) in table {
                out.insert(c.clone(), row_of(x)?);
            }
            cards.insert(k.clone(), out);
        }
        self.cards = cards;
        self.kinds = items(data, "kinds")?.into_iter().map(|(k, v)| (k.clone(), base::str_of(v))).collect();
        Ok(())
    }

    /// What a run of this character that got to `floor` usually ends on.
    pub fn usual(&self, character: &str, floor: i64) -> Option<f64> {
        let reached: Vec<i64> = self.finals.get(character).map(|fs| fs.iter().copied().filter(|f| *f >= floor).collect()).unwrap_or_default();
        if reached.len() >= 5 {
            Some(reached.iter().sum::<i64>() as f64 / reached.len() as f64)
        } else {
            None
        }
    }

    /// Floors a pick of this character gains on average: what a card's record is measured against.
    pub fn _par(&self, character: &str) -> f64 {
        let Some(table) = self.cards.get(character) else {
            return 0.0;
        };
        let picks = base::fsum(table.values().map(|x| x[0]));
        if picks >= 50.0 {
            base::fsum(table.values().map(|x| x[1])) / picks
        } else {
            0.0
        }
    }

    /// Where a card of this kind starts: what the kind's cards gained a pick, each against its character's par (`but` left out; "": none).
    pub fn kind_shift(&mut self, kind: &str, but: &str) -> f64 {
        if kind.is_empty() {
            return 0.0;
        }
        if self._pool.is_none() {
            let mut pool = Rows::new();
            for (who, table) in &self.cards {
                let par = self._par(who);
                for (key, [picks, gained]) in table {
                    if let Some(its) = self.kinds.get(key).filter(|k| !k.is_empty()) {
                        let row = pool.entry(its.clone()).or_default().entry(key.clone()).or_insert([0.0, 0.0]);
                        row[0] += picks;
                        row[1] += gained - picks * par;
                    }
                }
            }
            self._pool = Some(pool);
        }
        let rows: Vec<&[f64; 2]> = match self._pool.as_ref().and_then(|p| p.get(kind)) {
            Some(table) => table.iter().filter(|(key, _)| key.as_str() != but).map(|(_, x)| x).collect(),
            None => Vec::new(),
        };
        let learned = base::fsum(rows.iter().map(|x| x[1])) / (base::fsum(rows.iter().map(|x| x[0])) + KIND_PRIOR) / FLOORS_PER_POINT;
        fmax(-MAX_KIND, fmin(MAX_KIND, learned))
    }

    /// How far the record moves this card's rating (0 for an unknown character); `kind`: cards.kind as offered (None: as last seen).
    pub fn shift(&mut self, character: &str, name: &str, kind: Option<&str>) -> f64 {
        let key = card_key(name);
        if let Some(kind) = kind {
            if self.kinds.get(&key).map(String::as_str) != Some(kind) {
                self.kinds.insert(key.clone(), kind.to_string());
                self._pool = None;
            }
        }
        let table = self.cards.get(character);
        if table.is_none() && self.finals.get(character).map(|f| f.is_empty()).unwrap_or(true) {
            return 0.0;
        }
        let [picks, gained] = table.and_then(|t| t.get(&key)).copied().unwrap_or([0.0, 0.0]);
        // Against the character's par, not zero (the bot gets better over time); with no record a card stands where its kind does.
        let its = self.kinds.get(&key).cloned().unwrap_or_default();
        let mut start = self.kind_shift(&its, &key) * FLOORS_PER_POINT;
        if its == "free" {
            start *= knob("free_lean"); // a setting under test (see CHOICES)
        }
        let learned = (gained - picks * self._par(character) + PRIOR_PICKS * start) / (picks + PRIOR_PICKS) / FLOORS_PER_POINT;
        fmax(-MAX_SHIFT, fmin(MAX_SHIFT, learned)) + CURIOSITY / (1.0 + picks).powf(0.5)
    }

    /// Add a finished run: its result, and the cards it took on the way (`picks`: the floor each was taken on and its name).
    pub fn record(&mut self, character: &str, floor: i64, won: bool, picks: &[(i64, String)]) {
        if character.is_empty() {
            return;
        }
        let r#final = floor + if won { WIN_BONUS } else { 0 };
        self.cards.entry(character.to_string()).or_default();
        for (at, name) in picks {
            // Against the runs that left that floor alive, as this one did to be offered the card.
            let Some(usual) = self.usual(character, at + 1) else {
                continue; // too few runs yet to say what is usual
            };
            let entry = self.cards[character].entry(card_key(name)).or_insert([0.0, 0.0]);
            entry[0] += 1.0;
            entry[1] += r#final as f64 - usual;
        }
        let finals = self.finals.entry(character.to_string()).or_default();
        finals.push(r#final);
        if finals.len() > HISTORY as usize {
            finals.drain(..finals.len() - HISTORY as usize);
        }
        self._pool = None;
        self.save();
    }

    pub fn save(&self) {
        let mut data = Map::new();
        data.insert("finals".into(), Value::Object(self.finals.iter().map(|(k, v)| (k.clone(), Value::from(v.clone()))).collect()));
        data.insert("cards".into(), rows_value(&self.cards));
        data.insert("kinds".into(), Value::Object(self.kinds.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect()));
        _save(self.path.as_deref(), &Value::Object(data), "card statistics");
    }
}

fn rows_value(rows: &Rows) -> Value {
    Value::Object(rows.iter().map(|(k, v)| (k.clone(), Value::Object(v.iter().map(|(c, x)| (c.clone(), row_value(x))).collect()))).collect())
}

// What the model runs (sim/, `sim prior`) say of each card for each character: floors a pick gained over the runs that
// left the same floor alive, shrunk towards nothing. Used times the `model_lean` setting (0: not at all; see CHOICES).
// Since 2026-11-26 corrected twice by what the runs that played it still say of each card (`sim prior --onto`): on fresh seeds
// 2.6 floors further in Slay the Spire 2 at A7 and 3.6 in the first game at A0 (docs/strategy/simulation.md, round 4). Round 5
// found it had learned the model's own bugs (Grand Finale, Bone Shards, Fight Me, Fight Through, Reprogram: two dozen cards the model
// played unlike their text, most at the +3 cap): rebuilt on the corrected model, those cards from 0. With round 5's other changes,
// on fresh seeds: 1.8 floors further in StS2 at A8 (wins 5.7% -> 9.5%), 2.4 in the first game at A0 (17% -> 24.8%).
/// The table as it was when the program was built (read when the file itself is not there at run time).
pub const MODEL_CARDS: &str = include_str!("run/model_cards.json");

/// Where the table is kept: next to the source, as at build time (the tools that rewrite it write here).
pub fn model_cards_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("run").join("model_cards.json")
}

/// The model card table: the file at [`model_cards_path`] if it is there, else the copy built in.
pub fn load_model_cards() -> Map<String, Value> {
    let path = model_cards_path();
    if path.exists() {
        _load(Some(&path), "model card table")
    } else {
        _loads(MODEL_CARDS, &shown(&path), "model card table")
    }
}

thread_local! {
    static _model: RefCell<Option<Arc<Value>>> = const { RefCell::new(None) };
}

/// The model card table in force on this thread (None: not read yet; `model_shift` reads it when first asked).
pub fn model() -> Option<Arc<Value>> {
    _model.with(|m| m.borrow().clone())
}

/// Put another model card table in force on this thread (None: the shipped one, read again when next needed).
pub fn set_model(table: Option<Value>) {
    set_model_shared(table.map(Arc::new));
}

/// [`set_model`] with a table that several threads share (the model runs set one per run).
pub fn set_model_shared(table: Option<Arc<Value>>) {
    _model.with(|m| *m.borrow_mut() = table);
}

/// Puts the earlier model card table back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct ModelGuard(Option<Arc<Value>>);

impl Drop for ModelGuard {
    fn drop(&mut self) {
        set_model_shared(self.0.take());
    }
}

/// Another model card table on this thread until the guard is dropped (`mock.patch.object(learn, "_model", table)`).
pub fn with_model(table: Option<Value>) -> ModelGuard {
    let before = model();
    set_model(table);
    ModelGuard(before)
}

/// A character as the model table keys it: "The Silent", "THE_SILENT" and "SILENT" alike.
pub fn who_key(character: &str) -> String {
    let upper: String = character.to_uppercase().chars().map(|ch| if ch.is_alphabetic() { ch } else { '_' }).collect();
    let key = upper.trim_matches('_');
    key.strip_prefix("THE_").unwrap_or(key).to_string()
}

/// The model runs' rating shift for this card in this character's deck, times `model_lean`.
pub fn model_shift(character: &str, name: &str) -> f64 {
    let lean = knob("model_lean");
    if lean == 0.0 {
        return 0.0;
    }
    let table = match model() {
        Some(table) => table,
        None => {
            let table = Arc::new(Value::Object(load_model_cards()));
            set_model_shared(Some(table.clone()));
            table
        }
    };
    match table[games::active()][who_key(character).as_str()].get(card_key(name).as_str()) {
        Some(x) => lean * x.float(),
        None => lean * 0.0,
    }
}

/// Each run in runs.jsonl as a dict, read one line at a time (the file grows by every run ever played).
pub struct History {
    lines: Option<std::io::BufReader<std::fs::File>>,
}

impl Iterator for History {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        let reader = self.lines.as_mut()?;
        loop {
            let mut line = Vec::new();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => {
                    self.lines = None;
                    return None;
                }
                Ok(_) => {}
            }
            if let Ok(r @ Value::Object(_)) = serde_json::from_str::<Value>(&String::from_utf8_lossy(&line)) {
                return Some(r);
            }
        }
    }
}

/// Each run in runs.jsonl as a dict, read one line at a time (the file grows by every run ever played).
pub fn history(runs_file: Option<&Path>) -> History {
    // (no file given, or one that cannot be opened: no runs)
    History { lines: runs_file.and_then(|p| std::fs::File::open(p).ok()).map(std::io::BufReader::new) }
}

/// Fill in the "usual result" from the run history when the statistics are new.
pub fn seed_from_history(stats: &mut CardStats, runs_file: Option<&Path>) {
    if !stats.finals.is_empty() {
        return;
    }
    for r in history(runs_file).filter(crate::run::modes::standard) {
        if r["character"].truthy() && (r["floor"].is_int() || r["floor"].is_boolean()) {
            let r#final = r["floor"].int() + if r["result"].as_str() == Some("win") { WIN_BONUS } else { 0 };
            stats.finals.entry(base::str_of(&r["character"])).or_default().push(r#final);
        }
    }
    for finals in stats.finals.values_mut() {
        if finals.len() > HISTORY as usize {
            finals.drain(..finals.len() - HISTORY as usize);
        }
    }
}

// ----------------------------------------------------------------- knobs
// Judgement calls that are plain numbers: each run plays one setting of each, drawn by Thompson sampling from those whose runs went furthest.
pub static DEFAULTS: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| {
    IndexMap::from([
        ("caution", 1.0),
        ("elite_worth", 2.5),
        ("upgrade_worth", 1.3),
        ("pickiness", 0.0),
        ("crowd", 0.6),
        ("punch", 0.9),
        ("weak_want", 0.0),
        ("attack_room", 99.0),
        ("block_want", 0.0),
        ("cards_first", 0.0),
        ("debuff_worth", 1.0),
        ("free_lean", 1.0),
        ("model_lean", 0.0),
        ("belt_spare", 1.0),
    ])
});
/// The table itself; what is in force on this thread is [`choices`] (a test may change it: [`with_choices`]).
pub static CHOICES: LazyLock<IndexMap<&'static str, Vec<f64>>> = LazyLock::new(|| {
    IndexMap::from([
        ("caution", vec![0.75, 1.0, 1.35]), // weight of HP lost in the combat planner (model runs: 1.35 best in Slay the Spire 2 at A5, no better at A7)
        ("elite_worth", vec![2.5, 6.0]), // what an elite's rewards count for when planning a route
        ("upgrade_worth", vec![2.5, 4.0]), // what an upgrade counts for against resting in acts 2 and 3 (act 1: strategy.UPGRADE_EARLY)
        ("pickiness", vec![-1.0, 0.0]), // added to the rating a card reward needs to be taken
        ("crowd", vec![0.0, 0.3, 0.6]), // share of a card's rating from other players' runs (crowd.rs) over our table (cards._TIER); model runs: 0.6 best in StS1, worst in StS2
        ("punch", vec![0.9, 2.7]), // how hard a deck short of damage leans to attacks: points per damage an energy under decks.PUNCH_FLOOR (model runs at A7: 0.9 a floor ahead)
        ("weak_want", vec![0.0, 1.0]), // rating points for a card that applies Weak while the deck holds fewer than two (decks.deck_fit)
        ("free_lean", vec![0.0, 1.0]), // how much of its kind's start a free attack gets (learn.CardStats.shift)
        ("attack_room", vec![3.0]), // damage cards a deck holds before that lean lets go (gone three later; 99: never): attack-heavy decks lost the act 2 boss
        ("block_want", vec![0.0, 1.5]), // rating points for a card with 5 Block or more while the deck holds few beyond its Defends (decks.deck_fit; model runs: 1.5 ahead in both games)
        ("cards_first", vec![0.0, 1.0]), // 1.0: an early card removal has to beat the cards on a Merchant's shelf (strategy.shop)
        ("debuff_worth", vec![1.0, 2.0, 3.0]), // a lasting turn of Vulnerable or Weak in the combat planner, as a multiple of its old worth (model runs: 1.0 best in the first game)
        ("model_lean", vec![0.0, 1.0, 2.0]), // how much of what the model runs say of each card goes into its rating (model_shift; in the model 1.0 went 3-4 floors further, 2.0 a floor more in StS2)
    ])
});
// What the model runs found best in each game (docs/strategy/simulation.md), over DEFAULTS: played with --no-tuning and by the
// model runs' own default (DEFAULTS stay what the tests are written against). 2026-11-25, Slay the Spire 2 at A7 and the first game
// at A0: model_lean 1 went 3-4 floors further in both on seeds its table had not seen; crowd 0.6 was the worst of three in StS2 four
// batches running, the best in StS1; attack_room 3 about a floor ahead in StS2; cards_first 1 1.4-1.9 floors ahead in StS1, elite_worth 6 a little.
// 2026-11-26: model_lean 2 another 0.9 +- 0.4 floors in StS2 on fresh seeds (3 fell back 0.6); in StS1 2 made no difference (+0.2 +- 0.55).
// Round 5 (StS2 at A8): belt_spare 0, no potion for a hallway fight just because the belt is full, 1.2 +- 0.6 floors further in StS2
// and 1.5 +- 0.65 short in StS1, so StS2's alone (not in CHOICES: the two games want opposite values).
/// The table itself; what is in force on this thread is [`game_defaults`] (a test may change it: [`with_game_defaults`]).
pub static GAME_DEFAULTS: LazyLock<IndexMap<&'static str, IndexMap<&'static str, f64>>> = LazyLock::new(|| {
    IndexMap::from([
        ("sts2", IndexMap::from([("crowd", 0.3), ("attack_room", 3.0), ("model_lean", 2.0), ("belt_spare", 0.0)])),
        ("sts1", IndexMap::from([("elite_worth", 6.0), ("cards_first", 1.0), ("model_lean", 1.0)])),
    ])
});

pub const MIN_RUNS: i64 = 20; // runs a setting gets before the record alone decides how often it is drawn
pub const SPREAD: f64 = 12.0; // how far apart two runs of the same character usually end, in floors

thread_local! {
    // the settings in force right now
    static KNOB: RefCell<IndexMap<String, f64>> = RefCell::new(DEFAULTS.iter().map(|(k, v)| (k.to_string(), *v)).collect());
    static CHOICES_NOW: RefCell<Option<IndexMap<String, Vec<f64>>>> = const { RefCell::new(None) };
    static GAME_DEFAULTS_NOW: RefCell<Option<IndexMap<String, IndexMap<String, f64>>>> = const { RefCell::new(None) };
}

/// CHOICES as in force on this thread (a copy).
pub fn choices() -> IndexMap<String, Vec<f64>> {
    CHOICES_NOW.with(|c| c.borrow().clone()).unwrap_or_else(|| CHOICES.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
}

/// GAME_DEFAULTS as in force on this thread (a copy).
pub fn game_defaults() -> IndexMap<String, IndexMap<String, f64>> {
    GAME_DEFAULTS_NOW
        .with(|c| c.borrow().clone())
        .unwrap_or_else(|| GAME_DEFAULTS.iter().map(|(k, v)| (k.to_string(), v.iter().map(|(n, x)| (n.to_string(), *x)).collect())).collect())
}

/// Puts the earlier CHOICES back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct ChoicesGuard(Option<IndexMap<String, Vec<f64>>>);

impl Drop for ChoicesGuard {
    fn drop(&mut self) {
        let before = self.0.take();
        CHOICES_NOW.with(|c| *c.borrow_mut() = before);
    }
}

/// These rows of CHOICES changed or added on this thread until the guard is dropped (`mock.patch.dict(learn.CHOICES, {...})`).
pub fn with_choices(rows: &[(&str, &[f64])]) -> ChoicesGuard {
    let before = CHOICES_NOW.with(|c| c.borrow().clone());
    let mut now = choices();
    for (name, values) in rows {
        now.insert(name.to_string(), values.to_vec());
    }
    CHOICES_NOW.with(|c| *c.borrow_mut() = Some(now));
    ChoicesGuard(before)
}

/// Puts the earlier GAME_DEFAULTS back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct GameDefaultsGuard(Option<IndexMap<String, IndexMap<String, f64>>>);

impl Drop for GameDefaultsGuard {
    fn drop(&mut self) {
        let before = self.0.take();
        GAME_DEFAULTS_NOW.with(|c| *c.borrow_mut() = before);
    }
}

/// These games' rows of GAME_DEFAULTS replaced on this thread until the guard is dropped (`mock.patch.dict(learn.GAME_DEFAULTS, {...})`).
pub fn with_game_defaults(rows: &[(&str, &[(&str, f64)])]) -> GameDefaultsGuard {
    let before = GAME_DEFAULTS_NOW.with(|c| c.borrow().clone());
    let mut now = game_defaults();
    for (game, table) in rows {
        now.insert(game.to_string(), table.iter().map(|(n, x)| (n.to_string(), *x)).collect());
    }
    GAME_DEFAULTS_NOW.with(|c| *c.borrow_mut() = Some(now));
    GameDefaultsGuard(before)
}

/// The settings to play without tuning: DEFAULTS, with this game's (the one being played if None) from GAME_DEFAULTS.
pub fn defaults(game: Option<&str>) -> IndexMap<String, f64> {
    let game = match game {
        Some(g) if !g.is_empty() => g,
        _ => games::active(),
    };
    let mut out: IndexMap<String, f64> = DEFAULTS.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let over = |out: &mut IndexMap<String, f64>, name: &str, value: f64| {
        out.insert(name.to_string(), value);
    };
    match GAME_DEFAULTS_NOW.with(|c| c.borrow().as_ref().map(|t| t.get(game).cloned())) {
        Some(table) => table.unwrap_or_default().iter().for_each(|(n, x)| over(&mut out, n, *x)),
        None => GAME_DEFAULTS.get(game).into_iter().flatten().for_each(|(n, x)| over(&mut out, n, *x)),
    }
    out
}

/// The setting called `name` as in force on this thread (KNOB, else DEFAULTS; a name that is in neither is a mistake in the code).
pub fn knob(name: &str) -> f64 {
    match KNOB.with(|k| k.borrow().get(name).copied()) {
        Some(value) => value,
        None => *DEFAULTS.get(name).unwrap_or_else(|| panic!("KeyError: {}", base::str_repr(name))),
    }
}

/// `KNOB[name] = value`.
pub fn set_knob(name: &str, value: f64) {
    KNOB.with(|k| {
        k.borrow_mut().insert(name.to_string(), value);
    });
}

/// `dict(KNOB)`: the settings in force on this thread (a copy).
pub fn knobs() -> IndexMap<String, f64> {
    KNOB.with(|k| k.borrow().clone())
}

/// `KNOB.clear(); KNOB.update(table)`: these settings and no others.
pub fn set_knobs(table: &IndexMap<String, f64>) {
    KNOB.with(|k| *k.borrow_mut() = table.clone());
}

/// `KNOB.update(table)`.
pub fn update_knobs(table: &IndexMap<String, f64>) {
    KNOB.with(|k| {
        let mut k = k.borrow_mut();
        for (name, value) in table {
            k.insert(name.clone(), *value);
        }
    });
}

/// Puts the earlier settings back (all of them, as `mock.patch.dict` does) when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct KnobGuard(IndexMap<String, f64>);

impl Drop for KnobGuard {
    fn drop(&mut self) {
        set_knobs(&self.0);
    }
}

/// These settings on this thread until the guard is dropped (`mock.patch.dict(learn.KNOB, {...})`; whatever else is set meanwhile goes too).
pub fn with_knobs(values: &[(&str, f64)]) -> KnobGuard {
    let before = knobs();
    for (name, value) in values {
        set_knob(name, *value);
    }
    KnobGuard(before)
}

#[derive(Clone, Debug)]
pub struct Knobs {
    pub path: Option<PathBuf>,
    pub rng: Random,
    /// knob -> setting -> [runs, floors gained in total]
    pub table: Rows,
}

impl Knobs {
    pub fn new(path: Option<PathBuf>, rng: Option<Random>) -> Self {
        let mut table = Rows::new();
        for (k, v) in _load(path.as_deref(), "tuning") {
            let rows = v.as_object().unwrap_or_else(|| panic!("the tuning record's {} is not a JSON object", base::str_repr(&k)));
            table.insert(k, rows.iter().map(|(c, x)| (c.clone(), row_of(x).unwrap_or_else(|e| panic!("the tuning record: {e}")))).collect());
        }
        Knobs { path, rng: rng.unwrap_or_default(), table }
    }

    fn row(&self, name: &str, value: f64) -> [f64; 2] {
        self.table.get(name).and_then(|t| t.get(&base::float_repr(value))).copied().unwrap_or([0.0, 0.0])
    }

    /// Choose this run's settings and put them in force.
    pub fn draw(&mut self) -> IndexMap<String, f64> {
        for (name, choices) in choices() {
            let mut best = DEFAULTS[name.as_str()];
            let mut best_guess: Option<f64> = None;
            let seen: Vec<[f64; 2]> = choices.iter().map(|v| self.row(&name, *v)).collect();
            // An untried setting starts from what the tried ones average, not from zero (every run beats "the usual" by a few floors).
            let par = base::fsum(seen.iter().map(|x| x[1])) / fmax(1.0, base::fsum(seen.iter().map(|x| x[0])));
            for value in &choices {
                let [runs, gained] = self.row(&name, *value);
                // What this setting's true average might be, given what we have seen of it.
                let guess = self.rng.gauss((gained + par) / (runs + 1.0), SPREAD / (runs + 1.0).powf(0.5));
                if best_guess.is_none_or(|b| guess > b) {
                    best = *value;
                    best_guess = Some(guess);
                }
            }
            // Half the time play a setting with few runs, so that a few bad ones cannot bury it.
            let few: Vec<f64> = choices.iter().zip(&seen).filter(|(_, x)| x[0] < MIN_RUNS as f64).map(|(v, _)| *v).collect();
            if !few.is_empty() && self.rng.random() < 0.5 {
                best = *self.rng.choice(&few);
            }
            set_knob(&name, best);
        }
        knobs()
    }

    /// A run with the current settings ended `gained` floors beyond what is usual for its character.
    pub fn record(&mut self, gained: Option<f64>) {
        let Some(gained) = gained else {
            return;
        };
        let now = knobs();
        for name in choices().keys() {
            let setting = base::float_repr(*now.get(name).unwrap_or_else(|| panic!("KeyError: {}", base::str_repr(name))));
            let entry = self.table.entry(name.clone()).or_default().entry(setting).or_insert([0.0, 0.0]);
            entry[0] += 1.0;
            entry[1] += gained;
        }
        _save(self.path.as_deref(), &rows_value(&self.table), "tuning");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn golden() -> Value {
        serde_json::from_str(include_str!("../tests/golden/learn.json")).unwrap()
    }

    fn pairs(table: &IndexMap<String, f64>) -> Value {
        Value::Array(table.iter().map(|(k, v)| json!([k, v])).collect())
    }

    /// CardStats as the golden file holds one: {"finals", "cards", "kinds"} (numbers compared as numbers: a 2 read from a file is 2.0 here).
    fn same_stats(stats: &CardStats, want: &Value, why: &str) {
        let finals: Vec<(&str, Vec<i64>)> = want["finals"].entries().map(|(k, v)| (k.as_str(), v.list().iter().map(|x| x.as_i64().unwrap()).collect())).collect();
        assert_eq!(stats.finals.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>(), finals, "{why}");
        let cards: Vec<(&str, Vec<(&str, [f64; 2])>)> =
            want["cards"].entries().map(|(k, v)| (k.as_str(), v.entries().map(|(c, x)| (c.as_str(), [x[0].as_f64().unwrap(), x[1].as_f64().unwrap()])).collect())).collect();
        assert_eq!(stats.cards.iter().map(|(k, v)| (k.as_str(), v.iter().map(|(c, x)| (c.as_str(), *x)).collect::<Vec<_>>())).collect::<Vec<_>>(), cards, "{why}");
        let kinds: Vec<(&str, &str)> = want["kinds"].entries().map(|(k, v)| (k.as_str(), v.as_str().unwrap())).collect();
        assert_eq!(stats.kinds.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>(), kinds, "{why}");
    }

    #[test]
    fn golden_tables() {
        let g = golden();
        for row in g["who_key"].list() {
            assert_eq!(who_key(row[0].as_str().unwrap()), row[1].as_str().unwrap());
        }
        assert_eq!(Value::Array(DEFAULTS.iter().map(|(k, v)| json!([k, v])).collect()), g["DEFAULTS"]);
        assert_eq!(Value::Array(CHOICES.iter().map(|(k, v)| json!([k, v])).collect()), g["CHOICES"]);
        assert_eq!(Value::Array(choices().iter().map(|(k, v)| json!([k, v])).collect()), g["CHOICES"]);
        assert_eq!(Value::Array(game_defaults().iter().map(|(k, v)| json!([k, pairs(v)])).collect()), g["GAME_DEFAULTS"]);
        for row in g["defaults"].list() {
            assert_eq!(pairs(&defaults(row[0].as_str())), row[1], "defaults {}", row[0]);
        }
        assert_eq!(defaults(None), defaults(Some("sts2")));
        {
            let _game = games::playing("sts1");
            assert_eq!(defaults(None), defaults(Some("sts1")));
            assert_eq!(defaults(Some("")), defaults(Some("sts1")));
        }
        assert_eq!(pairs(&knobs()), g["DEFAULTS"]);
        assert_eq!(json!([WIN_BONUS, HISTORY, PRIOR_PICKS, FLOORS_PER_POINT, MAX_SHIFT, CURIOSITY, KIND_PRIOR, MAX_KIND, MIN_RUNS, SPREAD]), g["consts"]);
    }

    #[test]
    fn golden_card_stats() {
        let g = golden();
        let mut checked = 0;
        for (n, seq) in g["stats"].list().iter().enumerate() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("sub").join("card_stats.json");
            let mut s = CardStats::new(if seq["path"].truthy() { Some(path.clone()) } else { None });
            if seq["start"].truthy() {
                s.fill(seq["start"].as_object().unwrap()).unwrap();
            }
            for (i, op) in seq["ops"].list().iter().enumerate() {
                let why = format!("sequence {n} step {i}: {op}");
                match op[0].as_str().unwrap() {
                    "record" => {
                        let picks: Vec<(i64, String)> = op[4].list().iter().map(|p| (p[0].as_i64().unwrap(), p[1].as_str().unwrap().to_string())).collect();
                        s.record(op[1].as_str().unwrap(), op[2].as_i64().unwrap(), op[3].as_bool().unwrap(), &picks);
                    }
                    "shift" => {
                        let _knob = with_knobs(&[("free_lean", op[4].as_f64().unwrap())]);
                        assert_eq!(s.shift(op[1].as_str().unwrap(), op[2].as_str().unwrap(), op[3].as_str()), op[5].as_f64().unwrap(), "{why}");
                    }
                    "kind_shift" => assert_eq!(s.kind_shift(op[1].as_str().unwrap(), op[2].as_str().unwrap()), op[3].as_f64().unwrap(), "{why}"),
                    "usual" => assert_eq!(s.usual(op[1].as_str().unwrap(), op[2].as_i64().unwrap()), op[3].as_f64(), "{why}"),
                    "par" => assert_eq!(s._par(op[1].as_str().unwrap()), op[2].as_f64().unwrap(), "{why}"),
                    other => panic!("{other}"),
                }
                checked += 1;
            }
            same_stats(&s, &seq["end"], &format!("sequence {n}"));
            match seq["file"].as_str() {
                Some(text) => {
                    assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "sequence {n}: the file");
                    let mut again = CardStats::new(Some(path.clone()));
                    for row in seq["again"].list() {
                        assert_eq!(again.shift(row[0].as_str().unwrap(), row[1].as_str().unwrap(), None), row[2].as_f64().unwrap(), "sequence {n} read again: {row}");
                        checked += 1;
                    }
                }
                None => assert!(!path.exists()),
            }
        }
        for row in g["bad"].list() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("card_stats.json");
            std::fs::write(&path, row[0].as_str().unwrap()).unwrap();
            let (s, _logged) = crate::logging::capture(|| CardStats::new(Some(path.clone())));
            same_stats(&s, &row[1], row[0].as_str().unwrap());
            checked += 1;
        }
        assert!(checked > 5000, "{checked} checks");
    }

    #[test]
    fn golden_model_shift() {
        let g = golden();
        let own = json!({"sts2": {"SILENT": {"FOOTWORK": 1.5, "DEFLECT": -2.0, "WHOLE": 2}}, "sts1": {"SILENT": {"FOOTWORK": -1.0}}});
        let rows = g["model_shift"].list();
        for row in rows {
            let _game = games::playing(row[1].as_str().unwrap());
            let _table = with_model(if row[0] == "own" { Some(own.clone()) } else { None });
            let _knob = with_knobs(&[("model_lean", row[2].as_f64().unwrap())]);
            assert_eq!(model_shift(row[3].as_str().unwrap(), row[4].as_str().unwrap()), row[5].as_f64().unwrap(), "{row}");
        }
        assert!(rows.len() > 1500);
        assert!(model().is_none());
        // the copy built in is the file it was built from
        assert_eq!(Value::Object(_loads(MODEL_CARDS, "", "model card table")), Value::Object(load_model_cards()));
        assert!(model_cards_path().ends_with("src/run/model_cards.json"));
    }

    #[test]
    fn golden_knobs() {
        let g = golden();
        let mut checked = 0;
        for run in g["knobs"].list() {
            let why = format!("seed {}", run["seed"]);
            let _knob = with_knobs(&[]);
            let patch: Vec<(&str, Vec<f64>)> = run["patch"].list().iter().map(|p| (p[0].as_str().unwrap(), p[1].list().iter().map(|x| x.as_f64().unwrap()).collect())).collect();
            let _choices = with_choices(&patch.iter().map(|(k, v)| (*k, v.as_slice())).collect::<Vec<_>>());
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("tuning.json");
            if run["start"].truthy() {
                std::fs::write(&path, base::json_dumps(&run["start"])).unwrap();
            }
            let mut k = Knobs::new(if run["path"].truthy() { Some(path.clone()) } else { None }, Some(Random::new(run["seed"].as_i64().unwrap() as i128)));
            for (i, op) in run["ops"].list().iter().enumerate() {
                assert_eq!(pairs(&k.draw()), op[0], "{why} draw {i}");
                k.record(op[1].as_f64());
                checked += 1;
            }
            assert_eq!(rows_value(&k.table), run["table"], "{why}");
            assert_eq!(std::fs::read_to_string(&path).ok().as_deref(), run["file"].as_str(), "{why}: the file");
            assert_eq!(k.rng.random(), run["after"].as_f64().unwrap(), "{why}: as many draws");
        }
        assert_eq!(choices().len(), CHOICES.len());
        assert!(checked >= 600, "{checked} checks");
    }

    #[test]
    fn golden_history() {
        let g = golden();
        let h = &g["history"];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        std::fs::write(&path, h["text"].as_str().unwrap()).unwrap();
        assert_eq!(Value::Array(history(Some(&path)).collect()), h["rows"]);
        let mut s = CardStats::new(None);
        seed_from_history(&mut s, Some(&path));
        same_stats(&s, &json!({"finals": h["finals"], "cards": {}, "kinds": {}}), "seeded");
        let mut kept = CardStats::new(None);
        kept.finals.insert("X".into(), vec![1]);
        seed_from_history(&mut kept, Some(&path));
        same_stats(&kept, &json!({"finals": h["kept"], "cards": {}, "kinds": {}}), "kept");
        assert_eq!(history(None).count(), 0);
        assert_eq!(history(Some(&dir.path().join("no.jsonl"))).count(), 0);
    }

    #[test]
    fn the_settings_are_put_back() {
        assert_eq!(knob("caution"), 1.0);
        {
            let _outer = with_knobs(&[("caution", 1.35)]);
            assert_eq!(knob("caution"), 1.35);
            {
                let _inner = with_knobs(&[("caution", 0.75), ("model_table", 1.0)]);
                assert_eq!((knob("caution"), knobs().get("model_table").copied()), (0.75, Some(1.0)));
                set_knob("punch", 2.7);
            }
            assert_eq!((knob("caution"), knob("punch"), knobs().contains_key("model_table")), (1.35, 0.9, false));
            set_knobs(&defaults(Some("sts1")));
            assert_eq!(knob("elite_worth"), 6.0);
        }
        assert_eq!(knobs(), DEFAULTS.iter().map(|(k, v)| (k.to_string(), *v)).collect::<IndexMap<_, _>>());
        let _games = with_game_defaults(&[("sts2", &[("crowd", 0.3)]), ("sts1", &[("crowd", 0.6)])]);
        assert_eq!((defaults(Some("sts2"))["crowd"], defaults(Some("sts1"))["crowd"], defaults(Some("sts1"))["elite_worth"]), (0.3, 0.6, 2.5));
        assert_eq!(defaults(Some("sts2"))["caution"], DEFAULTS["caution"]);
    }
}
