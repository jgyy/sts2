//! # A game's content, as the engine and the run hold it
//!
//! In Python a game's content is a MODULE handed around as `game` (`run.game.CHARACTERS`, `game.make(key)`,
//! `game.DAMAGE[key](f, c)`, `game.relic_text(r)` ...). Here it is one [`Content`] per game, built once per thread
//! ([`content`]`("sts1") -> Rc<Content>`) by the game's own module (`sts1::content()`, `sts2::content()`), which
//! keeps its tables (`CARDS()`, `pool(character)` ...) in thread-locals of its own and fills the struct with:
//!
//! - **plain data**: `KEY`, `ACTS`, `SHORT_REASON`, `KEEP_POTIONS`, `UNKNOWN_ODDS`, `CHARACTERS`, `CARDS`,
//!   `COMMON`, `UNCOMMON`, `X_PLUS`;
//! - **the card handlers** effects.rs asks for, keyed by card key (`HashMap<String, Rc<dyn Fn>>`: they are only
//!   looked up, never iterated): `DAMAGE`, `PRE`, `AFTER`, `POWER`, `PLAYABLE`, `DRAWN`, `RETAINED`. Slay the
//!   Spire 2 reuses the first game's by key: `sts1_cards::AFTER()` (and `PRE()`, `PLAYABLE()` ...) build the first
//!   game's maps anew, so `sts2` takes the `Rc`s it wants out of one (`after["HAVOC"].clone()`) and adds its own;
//! - **functions** as `fn` pointers (the Python modules' functions and their `X = other.x` aliases: sts2 puts
//!   `sts1::relic_pickup`, `sts1::fight_won`, `sts1::rest_options`, `common::event`, `common::potion_card` in its own
//!   struct as they are).
//!
//! A handler is called as `(g.encounter)(run, kind)`: copy the pointer (or clone the `Rc<Content>`) out of the
//! run or the fight first, `let g = f.game.clone();`, since the call borrows the run or the fight whole.
//!
//! ## How the handlers see a fight (Python's object graph, in Rust)
//!
//! - The fight OWNS the run while it lasts (`f.run`: `Run::fight` moves itself in and takes itself back), and
//!   the one random generator is the run's: Python's `f.rng` is `f.run.rng`.
//! - Enemies are indices into `f.foes` (the list only grows): `Option<usize>` for Python's `foe` or None.
//! - A card is a value in exactly one pile (`f.hand`, `f.draw`, `f.discard`, `f.exhausted`) and is told apart by
//!   its `uid`. The card BEING PLAYED is in no pile (as in Python between `hand.pop` and `put_away`): `PRE`,
//!   `AFTER`, `POWER` and `EXTRA` get it as `&mut Card` next to `&mut Fight`. Rules about a card that sits in the
//!   hand (`DRAWN`, `RETAINED`) get its `uid` and reach it with `f.card(uid)` / `f.card_mut(uid)`. `DAMAGE` and
//!   `PLAYABLE` only read: `(&Fight, &Card)`.
//! - `f.hand_pick` and `f.pile_pick` return CLONES of the cards picked (same `uid`): take the real one out of
//!   its pile with `f.take_from(Pile::Hand, uid)` or change it through `f.card_mut(uid)`.
//! - What Python hung on the objects' `__dict__` are fields: `f.nightmares`, `f.foregone`, `run.fights_in_act`,
//!   `run.act_id`, `run.bosses` (act -> the names drawn for its boss).

pub mod common;
pub mod sts1;
pub mod sts1_cards;
pub mod sts1_foes;
pub mod sts2;
pub mod sts2_foes;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::cards::CardInfo;
use crate::sim::cards::{Card, CardDef};
use crate::sim::engine::{Fight, Foe};
use crate::sim::run::{Potion, Run};

/// `DAMAGE[key](f, c) -> base`: damage the text does not print.
pub type DamageRule = Rc<dyn Fn(&Fight, &Card) -> i64>;
/// `PRE[key](fight, card, foe, info) -> info`: adjusts what is read before it is done (None: nothing is done).
pub type PreRule = Rc<dyn Fn(&mut Fight, &mut Card, Option<usize>, Rc<CardInfo>) -> Option<Rc<CardInfo>>>;
/// `AFTER[key](fight, card, foe, x, dealt)`: the rest of what the card does.
pub type AfterRule = Rc<dyn Fn(&mut Fight, &mut Card, Option<usize>, i64, i64)>;
/// `POWER[key](fight, card)`: a Power played.
pub type PowerRule = Rc<dyn Fn(&mut Fight, &mut Card)>;
/// `PLAYABLE[key](fight, card)`: "Can only be played if ...".
pub type PlayableRule = Rc<dyn Fn(&Fight, &Card) -> bool>;
/// `DRAWN[key](fight, card)`, `RETAINED[key](fight, card)`: the card is in the hand; its `uid` is given.
pub type HandRule = Rc<dyn Fn(&mut Fight, u64)>;

/// A character's start (`CHARACTERS[key]`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Character {
    pub title: String,
    pub hp: i64,
    pub deck: Vec<String>,
    pub relics: Vec<String>,
    pub orbs: i64, // (`start.get("orbs", 0)`)
}

/// A card reward's odds by fight kind: `{"monster": [("Rare", 0.03), ...], "elite": [...]}`.
pub type Odds = IndexMap<String, Vec<(String, f64)>>;

/// An act's map as the bot reads it: (col, row) -> {col, row, type, children}.
pub type Nodes = IndexMap<(i64, i64), Value>;

/// One thing on a shop's shelf: the dict the bot is shown (`v`: index, category, price, cost, card_* ...) and
/// what Python kept beside it under `_card`, `_relic`, `_potion`.
#[derive(Clone, Debug, Default)]
pub struct Item {
    pub v: Map<String, Value>,
    pub card: Option<Card>,
    pub relic: Option<String>,
    pub potion: Option<Potion>,
}

impl Item {
    /// `it["price"]`.
    pub fn price(&self) -> i64 {
        self.v.get("price").and_then(|p| p.as_i64()).unwrap_or(0)
    }

    /// `it["category"]`.
    pub fn category(&self) -> &str {
        self.v.get("category").and_then(|c| c.as_str()).unwrap_or("")
    }
}

/// An option of an event the model makes up: (title, description, what it does to (run, act)).
pub type Choice = (String, String, Option<Rc<dyn Fn(&mut Run, i64)>>);

#[allow(non_snake_case)]
pub struct Content {
    pub KEY: &'static str,
    pub ACTS: i64,
    pub SHORT_REASON: bool,
    pub KEEP_POTIONS: HashSet<String>,
    pub UNKNOWN_ODDS: IndexMap<String, f64>,
    pub CHARACTERS: IndexMap<String, Character>,
    pub CARDS: Rc<IndexMap<String, Rc<CardDef>>>,
    pub COMMON: Vec<String>,
    pub UNCOMMON: Vec<String>,
    pub DAMAGE: HashMap<String, DamageRule>,
    pub PRE: HashMap<String, PreRule>,
    pub AFTER: HashMap<String, AfterRule>,
    pub POWER: HashMap<String, PowerRule>,
    pub PLAYABLE: HashMap<String, PlayableRule>,
    pub DRAWN: HashMap<String, HandRule>,
    pub RETAINED: HashMap<String, HandRule>,
    pub X_PLUS: HashSet<String>,
    // the engine's hooks
    /// The game's own resources (Stars, Osty, Doom, Souls, Forge, Mantra): `EXTRA(f, c, foe, i, xs)`.
    pub EXTRA: fn(&mut Fight, &mut Card, Option<usize>, &CardInfo, i64),
    pub START: fn(&mut Fight),
    pub TURN_START: fn(&mut Fight),
    pub TURN_CARDS: fn(&mut Fight),
    pub TURN_END: fn(&mut Fight),
    pub ROUND_END: fn(&mut Fight),
    pub POTION_CARD: fn(&mut Fight, &Potion),
    /// `ON(f, what, card)`: the engine's events ("play", "exhaust", "block", "stars"), for Powers read from their
    /// text (Slay the Spire 2; the first game has none: Python's `getattr(game, "ON", None)`).
    pub ON: Option<fn(&mut Fight, &str, Option<&Card>)>,
    // cards, relics, potions
    pub make: fn(&str, bool) -> Card,
    pub pool: fn(&str) -> Rc<Vec<Rc<CardDef>>>,
    pub colorless: fn() -> Rc<Vec<Rc<CardDef>>>,
    pub relic_text: fn(&str) -> String,
    pub relic_pickup: fn(&mut Run, &str),
    /// `random_relic(run, chest=False)`.
    pub random_relic: fn(&mut Run, bool) -> String,
    pub random_curse: fn(&mut Run) -> Card,
    pub random_potion: fn(&mut Run) -> Potion,
    /// A Wither of the Aeonglass's level (Slay the Spire 2's `game.wither(level)`).
    pub wither: Option<fn(i64) -> Rc<CardDef>>,
    // the run
    pub setup_run: fn(&mut Run),
    pub max_hp_penalty: fn(i64, &str) -> i64,
    pub start_hp: fn(&Run) -> i64,
    pub boss_heal: fn(&Run) -> f64,
    pub run_start: fn(&mut Run),
    pub act_start: fn(&mut Run),
    pub make_map: fn(&mut Run) -> Nodes,
    pub encounter: fn(&mut Run, &str) -> Vec<Foe>,
    /// `spawn(key, f, minion=False, hp=None)`.
    pub spawn: fn(&str, &mut Fight, bool, Option<i64>) -> Foe,
    pub fight_won: fn(&mut Run, &Fight),
    pub boss_won: fn(&mut Run),
    pub fight_gold: fn(&mut Run, &str) -> i64,
    pub card_choices: fn(&mut Run, &str) -> Vec<Card>,
    pub rest_options: fn(&mut Run) -> Vec<Value>,
    pub shop_stock: fn(&mut Run) -> Vec<Item>,
    pub event: fn(&mut Run),
}

thread_local! {
    static CONTENT: RefCell<HashMap<String, Rc<Content>>> = RefCell::new(HashMap::new());
}

/// The content of a game ("sts1", "sts2"): Python's `importlib.import_module(f"sim.content.{game}")`.
pub fn content(game: &str) -> Rc<Content> {
    if let Some(c) = CONTENT.with(|m| m.borrow().get(game).cloned()) {
        return c;
    }
    let made = Rc::new(match game {
        "sts1" => sts1::content(),
        // TODO(sts2 content): "sts2" => sts2::content(),
        other => panic!("No module named 'sim.content.{other}'"),
    });
    CONTENT.with(|m| m.borrow_mut().insert(game.to_string(), made.clone()));
    made
}
