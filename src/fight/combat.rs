//! Combat decisions: pick the best set of cards for this turn, play one, re-plan.
//!
//! Each call scores every affordable subset of the hand with a small simulation (damage, kills, Block against the intents)
//! and returns the first card of the winning line; re-planning after every card picks up draws and other surprises.

use std::borrow::{Borrow, Cow};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::{json, Value};

// (also the names other modules and the tests reach through here)
pub use crate::fight::battle::{
    clone, constrict, cards_left_this_turn, deck_count, end_of_turn_block, end_of_turn_loss, expected_loss, free_plays, hand_burns, intent_damage, intent_hits, junk_cost, kill_reach, loss_cap,
    next_energy_worth, osty_hp, power_amount, reacted, read_enemies, Enemy, _bare_block, _draw_worth, _escape_odds, _fated, _has_power, _hit, _hit_count, _is_attack, _next_wither, _pick_target,
    _poison_total, _power_worth, _progress, _rage_blocks, _resists, _respawns, _sandpit, _set_up, _wither_bill, _would_kill, BEYOND_REACH, ENERGY_DRAIN, LETHAL_PENALTY, ORB_VALUES, POWER_VALUE,
    RAGE_BLOCK, RAGE_COST, REATTACH_REACH, SLIP_WORTH, THREAT_FLOOR, WIN_BONUS,
};
use crate::cards::{order_key, parse, CardInfo};
use crate::fight::battle::{self, _pick_target_among};
use crate::fight::fool::sadness as sad_hand;
use crate::fight::fool::turn::{arrows_owned, concerns, fool_powers, impaled_pool, played_melodies, FoolTurn, Powers};
use crate::fight::{artifact, closing, sandpit as pit, triggers};
use crate::games::sts1::stance::{carry_cost, read as read_stance, stuck_odds, Stance, CALM_EXIT, CALM_LATER, MANTRA_DIVINITY, MANTRA_LATER, TAKEN};
use crate::learn::knob;
use crate::logging::{Level, Logger};
use crate::py::{self, JsonExt};
pub use crate::run::potions::{describe, fortifier, potion_actions};
pub use crate::Action;

static LOG: Logger = Logger::new("sts2bot.combat");

mod own {
    crate::tunables! {
        pub MAX_SEARCH_CARDS: i64 = 11;
        // what a point of our HP costs a line, in damage: at full health, more as it runs out, more for a turn that takes it all
        pub HP_BASE: f64 = 1.5;
        pub HP_SLOPE: f64 = 2.5;
        pub HP_BITE: f64 = 3.0;
        /// Personal Hive: what a Dazed in our draw pile costs once the hand has no card to spare, in points
        pub HIVE_COST: f64 = 9.0;
        /// ... and the damage a round the Entomancer is taken to lose, for how long the fight still is
        pub HIVE_PACE: f64 = 22.0;
        /// turns of a debuff beyond this are too far off to count
        pub DEBUFF_CAP: i64 = 4;
        /// the Test Subject: what beating one of its forms is worth, in points (that much of the fight is done)
        pub FORM_WORTH: f64 = 40.0;
        /// Enrage: points a Skill costs for each Strength it gives, per hit coming and per 35 HP the enemy has left
        pub ENRAGE_PRICE: f64 = 2.0;
    }
}
pub use own::{DEBUFF_CAP, ENRAGE_PRICE, FORM_WORTH, HIVE_COST, HIVE_PACE, HP_BASE, HP_BITE, HP_SLOPE, MAX_SEARCH_CARDS, TUNABLES};

/// Give the constant called `name` another value on this thread from now on: one of this module's, or one of battle.rs's
/// (in Python `combat.RAGE_COST` was the planner's own binding of it; here there is one). False if neither has it.
pub fn set_tunable(name: &str, value: f64) -> bool {
    own::set_tunable(name, value) || battle::set_tunable(name, value)
}

/// `max(a, b)` of two floats as Python has it: the first unless the second is greater.
#[inline]
fn fmax(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// `min(a, b)` of two floats as Python has it: the first unless the second is less.
#[inline]
fn fmin(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

thread_local! {
    // scripts/bosses/aeonfight.rs's WB=X: the planner's Wither bill times X while this is set
    // (the script's `mock.patch.object(combat, "_wither_bill", lambda *a: bill(*a) * value)`)
    static WITHER_BILL_TIMES: Cell<Option<f64>> = const { Cell::new(None) };
    static NEXT_ROOM: RefCell<Option<Rc<dyn Fn(&Value) -> (f64, f64)>>> = const { RefCell::new(None) };
    static CHOOSE_CARD: RefCell<Option<Rc<dyn Fn(&Value, Option<&mut Plan>) -> Option<Action>>>> = const { RefCell::new(None) };
}

/// The planner's Wither bill times `times` on this thread from now on (None: as battle.rs has it).
pub fn set_wither_bill_times(times: Option<f64>) -> Option<f64> {
    WITHER_BILL_TIMES.with(|t| t.replace(times))
}

/// battle.rs's `next_room`, or what a test has put in its place (`with_next_room`): `_context` asks here.
pub fn next_room(state: &Value) -> (f64, f64) {
    match NEXT_ROOM.with(|h| h.borrow().clone()) {
        Some(hook) => hook(state),
        None => battle::next_room(state),
    }
}

/// Puts the earlier `next_room` back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct NextRoomGuard(Option<Rc<dyn Fn(&Value) -> (f64, f64)>>);

impl Drop for NextRoomGuard {
    fn drop(&mut self) {
        NEXT_ROOM.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(combat, "next_room", f)`: on this thread, until the guard is dropped.
pub fn with_next_room(f: impl Fn(&Value) -> (f64, f64) + 'static) -> NextRoomGuard {
    NextRoomGuard(NEXT_ROOM.with(|h| h.borrow_mut().replace(Rc::new(f))))
}

/// Puts the earlier `choose_card` back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct ChooseCardGuard(Option<Rc<dyn Fn(&Value, Option<&mut Plan>) -> Option<Action>>>);

impl Drop for ChooseCardGuard {
    fn drop(&mut self) {
        CHOOSE_CARD.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(combat, "choose_card", ...)`: `f` answers every `combat::choose_card` on this thread (those of `actions` too)
/// until the guard is dropped. A spy (`wraps=combat.choose_card`) counts and calls `combat::_choose_card`, the planner itself.
pub fn with_choose_card_hook(f: impl Fn(&Value, Option<&mut Plan>) -> Option<Action> + 'static) -> ChooseCardGuard {
    ChooseCardGuard(CHOOSE_CARD.with(|h| h.borrow_mut().replace(Rc::new(f))))
}

/// stands in for a rule that cannot apply in this fight
#[inline]
pub fn _idle() {}

/// `sum(_progress(e, _hit(e, amount, 1, raw=True), chip_rate) * dmg_value * e.worth for e in foes if e.alive [and e is not skip])`.
fn _hit_all(foes: &mut [Enemy], amount: i64, skip: Option<usize>, chip_rate: f64, dmg_value: f64) -> f64 {
    let mut total = py::Sum::new(0.0);
    for (i, e) in foes.iter_mut().enumerate() {
        if e.alive() && skip != Some(i) {
            let removed = _hit(e, amount, 1, false, true);
            total.add_f(_progress(e, removed, chip_rate) * dmg_value * e.worth());
        }
    }
    total.get()
}

/// What an orb's damage is worth, taken off the enemies it lands on: Glass hits everyone, Dark the weakest, Lightning one at random.
pub fn _orb_damage(kind: &str, amount: i64, foes: &mut [Enemy], chip_rate: f64, dmg_value: f64) -> f64 {
    let alive: Vec<usize> = (0..foes.len()).filter(|&i| foes[i].alive()).collect();
    if amount <= 0 || alive.is_empty() {
        return 0.0;
    }
    let dark;
    let targets: &[usize] = if kind == "GLASS" {
        &alive
    } else if kind == "DARK" {
        dark = [py::min_by_key(alive.iter().copied(), |&i| foes[i].hp).unwrap()];
        &dark
    } else if alive.len() == 1 {
        &alive
    } else {
        // A bolt at random may kill the wrong crab, and the other gains 99 Block (_rage_blocks).
        let mut risk = py::Sum::new(0.0);
        for &i in &alive {
            if foes[i].rage && foes[i].hp + foes[i].block <= amount {
                for &o in &alive {
                    if o != i && foes[o].rage {
                        risk.add_f(4.0 * foes[o].hp as f64 + 10.0);
                    }
                }
            }
        }
        return amount as f64 * 0.8 * dmg_value - risk.get() / alive.len() as f64;
    };
    let mut total = py::Sum::new(0.0);
    for &i in targets {
        let e = &mut foes[i];
        let removed = _hit(e, amount, 1, false, true);
        total.add_f(_progress(e, removed, chip_rate) * dmg_value * e.worth());
    }
    total.get()
}

/// (The Python's default: 1 turn.)
pub fn _expose(e: &mut Enemy, turns: i64) {
    if !_resists(e) {
        e.vulnerable = true;
        let fresh = 0.max(turns.min(DEBUFF_CAP.get() - e.vuln_turns));
        e.vuln_turns += fresh;
        e.vuln_added += fresh;
    }
}

/// Everything a card puts on one enemy. (The Python's default: x 1.)
pub fn _debuff(e: &mut Enemy, c: &CardInfo, x: i64) {
    if c.vulnerable != 0 {
        _expose(e, c.vulnerable * x);
    }
    if c.weak != 0 {
        _weaken(e, c.weak * x);
    }
    if c.enemy_strength_loss != 0 && !_resists(e) {
        // the enemy loses Strength for this turn: every hit is that much weaker
        e.incoming = 0.max(e.incoming - c.enemy_strength_loss * x * e.incoming_hits);
    }
    if c.enemy_strength_gain != 0 && e.incoming != 0 {
        // (Fight Me: the enemy's hits this turn are that much harder too)
        e.incoming += c.enemy_strength_gain * x * 1.max(e.incoming_hits);
    }
    if c.key == "DEBILITATE" && e.vuln_mult < 2.0 && !_resists(e) {
        // "Vulnerable and Weak are twice as effective": for the attacks that follow it this turn too.
        e.incoming = if e.weak { py::trunc(e.incoming as f64 * 0.5 / e.weak_cut) } else { e.incoming };
        e.vuln_mult = 2.0;
        e.weak_cut = 0.5;
    }
}

/// A newly Weak enemy hits for 25% less this turn. (The Python's default: 1 turn.)
pub fn _weaken(e: &mut Enemy, turns: i64) {
    if _resists(e) {
        return;
    }
    let fresh = 0.max(turns.min(DEBUFF_CAP.get() - e.weak_turns));
    e.weak_turns += fresh;
    e.weak_added += fresh;
    if !e.weak {
        e.weak = true;
        e.incoming = py::trunc(e.incoming as f64 * e.weak_cut);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub score: f64,
    /// (the enemy's entity_id, shared with the Enemy it was read from)
    pub first_target: Option<Arc<str>>,
    /// HP the line expects to lose when the turn ends
    pub lost: i64,
}

/// `simulate`'s `focus`: aim every targeted card at this enemy while it lives (`At`, Python's plain id), or only the last
/// aimed card, so that the turn ends facing it (`Last`, Python's `("last", id)`).
#[derive(Clone, Debug, PartialEq)]
pub enum Focus {
    At(String),
    Last(String),
}

/// `_wrath_outlook`'s: (the chance next turn's hand has no way out of Wrath, each enemy's (blow, share of turns it attacked) so far,
/// the same chance with Simmering Fury's cards drawn).
pub type Wrath = (f64, IndexMap<String, (f64, f64)>, f64);

/// The keywords of `simulate`, `Default` giving the Python's defaults (its five without one, energy to block: 0).
#[derive(Clone, Debug, PartialEq)]
pub struct SimulateArgs {
    pub energy: i64,
    pub stars: i64,
    pub hp: i64,
    pub max_hp: i64,
    pub block: i64,
    pub fight: String,
    pub round_no: i64,
    pub card_limit: Option<i64>,
    pub can_block: bool,
    pub sandpit: Option<i64>,
    /// HP we lose at the end of the turn whatever we play
    pub end_loss: i64,
    /// (hand index, HP, stopped by Block) of cards that hurt while held
    pub burns: Vec<(i64, i64, bool)>,
    pub hand_size: i64,
    pub focus: Option<Focus>,
    /// (kind, passive, evoke), oldest first
    pub orbs: Vec<(String, i64, i64)>,
    pub orb_slots: i64,
    pub shiv_damage: i64,
    /// each card played costs 1 Strength and 1 Dexterity
    pub tender: bool,
    /// vigor, giant: the next Attack only
    pub vigor: i64,
    pub giant: bool,
    pub buffer: i64,
    pub loss_cap: i64,
    /// Vital Spark (each Skill makes every hit on us harder)
    pub taint: i64,
    pub draw_worth: (f64, f64, f64),
    pub bare_block: i64,
    pub regen: i64,
    pub focus_power: i64,
    pub escape_odds: (f64, f64),
    /// wither_*: Withering Presence
    pub wither_in: i64,
    pub wither_hp: i64,
    pub wither_bill: f64,
    pub trig: Option<triggers::Triggers>,
    /// Osty's HP, already in `block` (-1: none)
    pub pet: i64,
    pub hail: i64,
    pub deck: (i64, i64),
    pub mastery: i64,
    /// Strength not in the card texts yet
    pub might0: i64,
    /// Barricade
    pub holds: bool,
    pub strip: (f64, f64),
    pub friends: i64,
    pub drained: i64,
    /// Void Form's free plays
    pub free: i64,
    /// the next card is played twice
    pub doubled: bool,
    /// a Fortifier's multiplier on our Block
    pub fortify: i64,
    pub pace: f64,
    pub room: (f64, f64),
    /// arrows to fool: the Fool's (fool/turn.rs)
    pub arrows: i64,
    pub aiming: i64,
    pub stretch: i64,
    pub tune: BTreeSet<String>,
    pub chord: i64,
    pub boom: i64,
    pub fool: Option<Powers>,
    pub stance0: String,
    /// for a Watcher (`_wrath_outlook`)
    pub wrath: Option<Wrath>,
    /// the Fool's Sadness and what is to come of it (fight/fool/sadness.rs `outlook`)
    pub sadness: Option<sad_hand::View>,
    /// the Watcher's Mantra
    pub mantra0: i64,
}

impl Default for SimulateArgs {
    fn default() -> Self {
        SimulateArgs {
            energy: 0,
            stars: 0,
            hp: 0,
            max_hp: 0,
            block: 0,
            fight: "monster".to_string(),
            round_no: 1,
            card_limit: None,
            can_block: true,
            sandpit: None,
            end_loss: 0,
            burns: Vec::new(),
            hand_size: 0,
            focus: None,
            orbs: Vec::new(),
            orb_slots: 0,
            shiv_damage: 4,
            tender: false,
            vigor: 0,
            giant: false,
            buffer: 0,
            loss_cap: 0,
            taint: 0,
            draw_worth: (2.0, 0.0, 0.0),
            bare_block: 0,
            regen: 0,
            focus_power: 0,
            escape_odds: (0.3, 0.3),
            wither_in: 0,
            wither_hp: 5,
            wither_bill: 7.0,
            trig: None,
            pet: -1,
            hail: 0,
            deck: (20, 0),
            mastery: 0,
            might0: 0,
            holds: false,
            strip: (0.0, 0.0),
            friends: 0,
            drained: 0,
            free: 0,
            doubled: false,
            fortify: 1,
            pace: 0.0,
            room: (99.0, 1.0),
            arrows: -1,
            aiming: 0,
            stretch: 0,
            tune: BTreeSet::new(),
            chord: 0,
            boom: 0,
            fool: None,
            stance0: String::new(),
            wrath: None,
            sadness: None,
            mantra0: 0,
        }
    }
}

/// Score playing `line` in order (None if it cannot be afforded), in the turn _context reads. Among the keywords: end_loss: HP we lose
/// at the end of the turn whatever we play; burns: (hand index, HP, stopped by Block) of cards that hurt while held; focus: aim every
/// targeted card at this enemy while it lives; orbs: (kind, passive, evoke), oldest first; tender: each card played costs 1 Strength and
/// 1 Dexterity; vigor, giant: the next Attack only; taint: Vital Spark (each Skill makes every hit on us harder); pet: Osty's HP, already
/// in `block` (-1: none); might0: Strength not in the card texts yet; holds: Barricade; free: Void Form's free plays; doubled: the next card
/// is played twice; fortify: a Fortifier's multiplier on our Block; wither_*: Withering Presence; arrows to fool: the Fool's (fool/turn.rs);
/// wrath: (the chance next turn's hand has no way out of Wrath, each enemy's (blow, share of turns it attacked) so far) for a Watcher (`_wrath_outlook`);
/// sadness: the Fool's Sadness and what is to come of it (fight/fool/sadness.rs `outlook`); mantra0: the Watcher's Mantra.
///
/// `line` is a slice of cards or of references to them; `args` a `SimulateArgs` or a reference to one.
pub fn simulate<L: Borrow<CardInfo>, A: Borrow<SimulateArgs>>(line: &[L], enemies: &[Enemy], args: A) -> Option<Outcome> {
    let args = args.borrow();
    simulate_aimed(line, enemies, args, args.focus.as_ref())
}

#[inline]
fn info<L: Borrow<CardInfo>>(c: &L) -> &CardInfo {
    c.borrow()
}

/// `simulate(line, enemies, **args, focus=focus)`: the keywords as they are, and this `focus` whatever theirs is (the planner
/// tries a line at every aim without copying the keywords).
pub fn simulate_aimed<L: Borrow<CardInfo>>(line: &[L], enemies: &[Enemy], args: &SimulateArgs, focus: Option<&Focus>) -> Option<Outcome> {
    let (mut energy, mut stars, mut hp, max_hp, mut block) = (args.energy, args.stars, args.hp, args.max_hp, args.block);
    let (fight, round_no, can_block, hand_size) = (args.fight.as_str(), args.round_no, args.can_block, args.hand_size);
    let (mut card_limit, mut buffer, mut friends, mut drained) = (args.card_limit, args.buffer, args.friends, args.drained);
    let (deck, taint, pet_start) = (args.deck, args.taint, args.pet);
    let (caution, debuff_worth) = (knob("caution"), knob("debuff_worth"));
    if card_limit.is_some_and(|limit| line.len() as i64 > limit) {
        return None;
    }
    if line.len() > 1 && line[..line.len() - 1].iter().any(|c| info(c).ends_turn) {
        return None; // "End your turn": nothing is played after it
    }
    let mut foes: Vec<Enemy> = enemies.iter().map(|e| Enemy { chipped: 0, stripped: 0, vuln_added: 0, weak_added: 0, ..clone(e) }).collect();
    let mut score = 0.0;
    // ("last", id): the last aimed card goes at that enemy, so the turn ends facing it
    let (turn_to, focus): (Option<&str>, Option<&str>) = match focus {
        Some(Focus::Last(id)) => (Some(id.as_str()), None),
        Some(Focus::At(id)) => (None, Some(id.as_str())),
        None => (None, None),
    };
    let last_aimed: i64 = line.iter().enumerate().filter(|(_, c)| info(*c).needs_target).map(|(n, _)| n as i64).max().unwrap_or(-1);
    let mut first_target: Option<Arc<str>> = None;
    // Damage is worth more where the fight is a race: bosses and elites, enemies that gain Strength every turn, fights that drag on.
    let mut dmg_value = (match fight {
        "boss" => 1.5,
        "elite" => 1.3,
        _ => 1.0,
    }) + fmin(0.4, 0.08 * 0.max(round_no - 5) as f64);
    dmg_value *= 1.0 + fmin(1.5, 0.15 * py::max_of(enemies.iter().filter(|e| e.worth() >= 1.0).map(|e| e.grows)).unwrap_or(0.0));
    dmg_value += 0.5 * 0.max(round_no - 12) as f64;
    let chip_rate = fmin(1.0, 0.25 + 0.05 * 0.max(round_no - 10) as f64); // worth of temporary Block removed
    let kept_block: Vec<&Arc<str>> = enemies.iter().filter(|e| e.keeps_block).map(|e| &e.entity_id).collect();
    // As the line goes on:
    let (mut self_loss, mut paid) = (0i64, 0i64); // HP it costs us, and of that what the cards themselves cost (Offering, Hemokinesis)
    let (mut escapes, mut osty_hits, mut attacks, mut plays) = (0i64, 0i64, 0i64, 0i64); // Frantic Escapes, Osty attacks, Attacks and all cards played (one played again counts again)
    let mut free_next: Option<&'static str> = None; // the kind of card that costs nothing next, after one that says so
    let mut surcharge = 0i64; // energy every further card costs on top of its own, after one that says so
    let mut echo: Option<&'static str> = if args.doubled { Some("Any") } else { None }; // the kind of card that is played an extra time next (Burst, One-Two Punch)
    let (mut dooming, mut free_all) = (false, false); // a card applied Doom; Bullet Time: everything left in the hand is free this turn
    let (mut healed, mut nimble, mut might, mut summoned, mut pet, pet0) = (0i64, 0i64, args.might0, 0i64, pet_start, pet_start); // HP given back; Dexterity and Strength gained; HP given Osty (and his HP as the texts were written)
    let mut faced: Option<Arc<str>> = None; // the last enemy a card was aimed at
    let mut held: Vec<(&str, i64, i64)> = args.orbs.iter().map(|(kind, passive, burst)| (kind.as_str(), *passive, *burst)).collect(); // the Defect's orbs
    let (mut drawn, mut debris, mut dazed, mut tainted, mut junked) = (0i64, 0i64, 0i64, 0i64, 0i64); // cards drawn; Debris and Dazed (Personal Hive) put in our deck; Vital Spark's extra damage a hit; other statuses
    let (mut lasting, mut enduring, mut future) = (0.0, 0.0, 0.0); // worth of effects that pay off over the fight (x `span`), of what stays all fight, of all that pays off after this turn
    let (mut later, mut nexte, mut kept) = (0.0, 0i64, false); // worth of things that arrive next turn; energy next turn, and whether the hand is Retained
    let mut mantra = args.mantra0; // the Watcher's Mantra as the line goes on
    let (mut sad, mut sad_rate, mut sad_draw) = (0i64, 0i64, 0i64); // Sadness the line puts in the hand, and more (or fewer) each turn from its Powers, with their draw
    // What a lasting effect is still worth: boss fights go on for a dozen turns, ordinary ones are over in four.
    let mut span = match fight {
        "boss" => 3.0,
        "elite" => 2.0,
        _ => 1.0,
    };
    let late = match fight {
        "boss" => 7,
        "elite" => 5,
        _ => 3,
    };
    span *= if round_no <= late { 1.0 } else { 0.4 };
    // the Fool's Arrows, Aim, Stretch and Melodies; the Watcher's stance
    let mut ft = FoolTurn::new(args.arrows, args.aiming, args.stretch, &args.tune, args.chord, args.boom, args.fool, &foes);
    let mut st = if args.stance0.is_empty() { Stance { start: "", now: "" } } else { Stance::new(args.stance0.as_str()) };
    let mut known_dead: HashSet<Arc<str>> = foes.iter().filter(|e| !e.alive()).map(|e| e.entity_id.clone()).collect();
    let (beating, softens) = (foes.iter().any(|e| e.beat != 0), foes.iter().any(|e| e.malleable != 0)); // (the first game's Heart; its Snake Plant)
    // (Only Stock and Adaptable enemies come back and only crabs hand on Block: for the rest both are skipped.)
    let (respawn, rage_blocks) = (foes.iter().any(|e| e.respawns > 0 || e.adapts), foes.iter().any(|e| e.rage));
    let trig = args.trig.as_ref();
    let mut alive: Vec<usize> = Vec::with_capacity(foes.len());
    let mut alive_now: Vec<usize> = Vec::with_capacity(foes.len());

    macro_rules! evoke {
        ($orb:expr, $plasma:expr) => {{
            let (kind, _passive, burst): (&str, i64, i64) = $orb;
            if kind == "FROST" && can_block {
                block += burst;
            } else if kind == "LIGHTNING" || kind == "DARK" || kind == "GLASS" {
                score += _orb_damage(kind, burst, &mut foes, chip_rate, dmg_value);
            } else if kind == "PLASMA" && $plasma {
                energy += 2;
            }
        }};
    }

    for (n, card) in line.iter().enumerate() {
        let card: &CardInfo = info(card);
        let mut c: Cow<CardInfo> = Cow::Borrowed(card);
        let n = n as i64;
        if respawn {
            _respawns(&mut foes);
        }
        if rage_blocks {
            _rage_blocks(&mut foes, &mut known_dead); // a crab that died with the last card hands its ally Block
        }
        let before = if trig.is_some() { triggers::marks(&foes) } else { Vec::new() };
        if c.star_cost > stars {
            return None;
        }
        let mut x = 1;
        if c.star_cost < 0 {
            // costs X Stars: all of them
            if stars <= 0 {
                return None;
            }
            x = stars;
            stars = 0;
        } else {
            stars -= c.star_cost;
        }
        if c.is_x() {
            if energy <= 0 {
                return None;
            }
            x = energy;
            energy = 0;
            score += 8.0 * x as f64 * dmg_value * ((c.key == "CASCADE") as i64 as f64); // "Play the top X cards of your Draw Pile": a card is about 8
        } else {
            // Flatten costs 0 once Osty has attacked this turn, after a Poke in the same line too.
            let free_kind = free_next.is_some_and(|kind| c.r#type == kind);
            let cost = if (c.key == "FLATTEN" && osty_hits != 0) || free_kind || free_all || n < args.free { 0 } else { c.cost + surcharge };
            free_next = if free_kind { None } else { free_next };
            if cost > energy {
                return None;
            }
            energy -= cost;
        }
        // Played again (its own Replay, or the card before said so): everything it does, it does again, and each time is one of the cards a turn allows.
        let echoed = echo.is_some_and(|kind| kind == "Any" || c.r#type == kind);
        let k = 1 + c.replay + echoed as i64 + ft.encore(&c); // (Ki Verse: the next Wah, Cho or Ki card again)
        echo = if echoed { None } else { echo };
        if card_limit.is_some_and(|limit| plays >= limit) {
            return None;
        }
        plays += k;
        if c.play_limit >= 0 {
            // Encore: "You can only play 1 more card this turn"
            card_limit = Some(match card_limit {
                None => plays + c.play_limit,
                Some(limit) => limit.min(plays + c.play_limit),
            });
        }
        if k > 1 {
            let c = c.to_mut();
            c.hits *= if c.damage != 0 { k } else { 1 };
            c.energy_gain *= k;
            c.block *= k;
            c.draw *= k;
            c.summon *= k;
            c.doom *= k;
            c.poison *= k;
            c.shivs *= k;
            c.souls *= k;
        }
        echo = triggers::ECHO_NEXT.get(c.key.as_str()).copied().or(echo);
        if concerns(&c) || (c.block != 0 && (ft.stretch != 0 || ft.stretch0 != 0)) {
            let aim = match turn_to {
                Some(id) if !id.is_empty() && n == last_aimed => Some(id),
                _ => focus,
            };
            let changed = match ft.before(&c, &foes, aim) {
                Cow::Owned(new) => Some(new),
                Cow::Borrowed(_) => None,
            };
            if let Some(new) = changed {
                c = Cow::Owned(new);
            }
        }
        energy *= if c.key == "DOUBLE_ENERGY" { 2 } else { 1 };
        free_all = free_all || c.key == "BULLET_TIME";
        energy += c.energy_gain;
        free_next = triggers::FREE_NEXT.get(c.key.as_str()).copied().or(free_next); // Unrelenting: "The next Attack you play costs 0"
        surcharge += triggers::SURCHARGE.get(c.key.as_str()).copied().unwrap_or(0);
        healed += c.heal;
        self_loss += c.hp_loss;
        paid += c.hp_loss;
        if c.hp_loss != 0 && paid >= hp + healed {
            // a card's HP is paid as it is played: no Regen, Block or kill at the end of the turn gives it back
            score -= LETHAL_PENALTY.get();
        }
        if c.osty_attack && pet == 0 {
            // Osty is dead: his attacks do nothing
            let c = c.to_mut();
            c.damage = 0;
            c.block = if c.osty_dies { 0 } else { c.block };
        } else if c.osty_attack {
            if c.key == "RATTLE" {
                c.to_mut().hits += osty_hits;
            }
            osty_hits += 1;
            // Unleash adds Osty's HP to its damage and Protector his Max HP: what was Summoned earlier in the line counts too.
            let grown = if c.key == "UNLEASH" {
                pet - pet0
            } else if c.key == "PROTECTOR" {
                summoned
            } else {
                0
            };
            if grown != 0 {
                c.to_mut().damage = 0.max(c.damage + grown);
            }
        }
        if c.osty_dies && pet > 0 {
            // ... and the HP he stood in front of us with goes with him
            block -= block.min(pet);
            pet = 0;
        }
        pet += if pet >= 0 { c.summon * x } else { 0 };
        if can_block && c.block != 0 {
            block += 0.max(c.block + nimble) * x;
        }
        if can_block && c.summon != 0 {
            // Osty's HP soaks hits like Block that does not go away at the end of the turn
            block += c.summon * x;
            summoned += c.summon * x;
        }
        if c.block_lock {
            // trading away future Block is only worth it to survive this turn
            score -= 30.0;
        }
        if c.deadly {
            score -= LETHAL_PENALTY.get(); // never gamble the run on one card
        }
        let xs = if c.is_x() { x } else { 1 };
        alive.clear();
        alive.extend((0..foes.len()).filter(|&i| foes[i].alive()));
        // the enemy this card is aimed at, if any
        let aim = match turn_to {
            Some(id) if !id.is_empty() && n == last_aimed => Some(id),
            _ => focus,
        };
        let mut focused: Option<usize> = match aim {
            Some(aim) => alive.iter().copied().find(|&i| &*foes[i].entity_id == aim),
            None => None,
        };
        let mut t = focused;
        if (c.key == "TIMES_UP" || c.key == "TIME_S_UP") && !alive.is_empty() {
            // "Deal damage equal to the enemy's Doom": no number in its text
            focused = focused.or_else(|| py::max_by_key(alive.iter().copied(), |&i| foes[i].doom));
            t = focused;
            let c = c.to_mut();
            c.damage = foes[t.unwrap()].doom;
            c.hits = 1;
        }
        if (c.key == "DEATHS_DOOR" || c.key == "DEATH_S_DOOR") && dooming && can_block {
            // "If you applied Doom this turn, gain Block 2 additional times"
            block += 2 * 0.max(c.block + nimble);
        }
        dooming = dooming || c.doom != 0;
        if c.key == "FIEND_FIRE" {
            // "Exhaust your Hand. Deal 7 damage for each card Exhausted": a hit for every card still in it
            c.to_mut().hits = 0.max(hand_size - n - 1 + drawn);
        }
        let mut dealt = 0; // what the card's hits do to its target: a Shot's Stretch (fool/turn.rs), Rain Shot's splash below
        if c.damage != 0 {
            let (mut per, hits) = (c.damage + (if c.osty_attack { 0 } else { might }), c.hits * x); // (Osty's attacks take no Strength)
            let attack = c.r#type == "Attack";
            if attack {
                if attacks != 0 {
                    // Vigor and Gigantification went with the first Attack
                    per = if args.giant { py::round(per as f64 / 3.0) } else { per };
                    per -= args.vigor;
                }
                attacks += 1;
                per = st.deal(per);
            }
            per = 0.max(per);
            dealt = per * hits;
            let one;
            let struck: &[usize] = if c.aoe {
                t = None;
                &alive
            } else if c.random_target && alive.len() > 1 {
                t = None;
                score += (per * hits) as f64 * 0.8 * dmg_value;
                &[]
            } else {
                t = focused.or_else(|| {
                    if c.impaled_only || c.on_impaled.is_some() || c.impaled_double || c.dislodge {
                        _pick_target_among(&foes, &impaled_pool(&c, &foes), per, hits, c.double_if_vulnerable)
                    } else {
                        _pick_target(&foes, per, hits, c.double_if_vulnerable)
                    }
                });
                if let (true, Some(i)) = (c.key == "HANG", t) {
                    // "Double the damage ALL Hang cards deal to this enemy": 10, 20, 40, 80 on the same enemy, though the text goes on saying 10.
                    let hang = 1.max(foes[i].hang);
                    per *= hang;
                    foes[i].hang = 2 * hang;
                }
                match t {
                    Some(i) => {
                        one = [i];
                        &one
                    }
                    None => &[],
                }
            };
            // Several hits on ALL enemies land one round at a time: a crab that dies of the first hands the other its 99 Block for the rest.
            let waves = if c.aoe && hits > 1 && struck.iter().any(|&i| foes[i].rage) { hits } else { 1 };
            for _ in 0..waves {
                for &i in struck {
                    let e = &mut foes[i];
                    if !e.alive() {
                        continue;
                    }
                    let n_hits = py::floordiv(hits, waves);
                    self_loss += e.thorns * _hit_count(e, n_hits, c.double_if_vulnerable);
                    dazed += e.hive * _hit_count(e, n_hits, c.double_if_vulnerable) * attack as i64;
                    let done = _fated(e); // damage that does not kill it now changes nothing
                    let removed = _hit(e, per, n_hits, c.double_if_vulnerable, false);
                    let gain = _progress(e, removed, chip_rate) * dmg_value * e.worth() * e.focus;
                    score += gain * (if done && e.alive() { 0.2 } else { 1.0 });
                    if e.reacts != 0 && e.alive() && attack {
                        // (a new intent in place of the one shown: battle.REACTIVE_HIT)
                        e.incoming = reacted(e.incoming, e.reacts, e.reacts_worst);
                        e.incoming_hits = e.incoming_hits.max(1);
                    }
                }
                if waves > 1 {
                    _rage_blocks(&mut foes, &mut known_dead);
                }
            }
            for &i in struck {
                if foes[i].alive() {
                    _debuff(&mut foes[i], &c, xs);
                }
            }
        } else if c.needs_target {
            if focused.is_some() {
            } else if c.enemy_strength_loss != 0 {
                t = py::max_by_key(alive.iter().copied(), |&i| foes[i].incoming.min(c.enemy_strength_loss * foes[i].incoming_hits));
            } else if c.weak != 0 && c.vulnerable == 0 {
                t = py::max_by_key(alive.iter().copied(), |&i| (!foes[i].weak, foes[i].incoming)); // the hardest hitter not Weak yet
            } else if c.doom != 0 && c.vulnerable == 0 {
                t = _pick_target(&foes, 0, 0, false); // Doom goes where the attacks go: together they end one enemy sooner
            } else if c.poison_needs {
                t = py::max_by_key(alive.iter().copied(), |&i| (foes[i].poison, foes[i].worth())); // (Bubble Bubble) only the poisoned one takes it
            } else {
                // (Uproot: the Impaled one)
                let refs: Vec<&Enemy> = alive.iter().map(|&i| &foes[i]).collect();
                let pool = impaled_pool(&c, &refs);
                let fresh: Vec<usize> = pool.iter().copied().filter(|&k| !refs[k].vulnerable).collect();
                let among = if fresh.is_empty() { &pool } else { &fresh };
                // Vulnerable: on whom we will hit the longest (the next to die, by a kill order)
                t = py::max_by_key(among.iter().map(|&k| alive[k]), |&i| (foes[i].focus, foes[i].hp + foes[i].block));
            }
            if let Some(i) = t {
                _debuff(&mut foes[i], &c, xs);
            }
        } else {
            t = None;
            if c.aoe {
                for &i in &alive {
                    _debuff(&mut foes[i], &c, xs);
                }
            }
        }
        if ft.books(&c) {
            // Violent Chord, Explosive Arrows: a Wah Melody gained, an enemy Impaled hits ALL enemies
            let chord = ft.after(&c, &mut foes, t, xs, dealt);
            if chord != 0 || ft.got != (0, 0, 0) {
                // (Song Path, Lullaby; Motif; Yah!)
                block += ft.got.0 * can_block as i64;
                drawn += ft.got.1;
                energy += ft.got.2;
                score += if chord != 0 { _hit_all(&mut foes, chord, None, chip_rate, dmg_value) } else { 0.0 };
            }
        }
        if let (true, Some(i), true) = (c.splash, t, dealt != 0) {
            // Rain Shot: "Damage ALL other enemies equal to the damage dealt"
            score += _hit_all(&mut foes, dealt, Some(i), chip_rate, dmg_value);
        }
        if let Some(i) = t {
            faced = Some(foes[i].entity_id.clone());
            if n == 0 {
                first_target = Some(foes[i].entity_id.clone());
            }
        }
        alive_now.clear();
        alive_now.extend((0..foes.len()).filter(|&i| foes[i].alive()));
        let aim: Option<&str> = match &faced {
            Some(id) if !id.is_empty() => Some(id),
            _ => focus,
        };
        let aimed: Option<usize> = match aim {
            Some(aim) => alive_now.iter().copied().find(|&i| &*foes[i].entity_id == aim),
            None => None,
        };
        if c.poison != 0 || c.doom != 0 || c.enemy_hp_loss != 0 || c.shivs != 0 {
            let one;
            let victims: &[usize] = if c.aoe {
                &alive_now
            } else if let (Some(i), true) = (aimed, c.needs_target || c.damage != 0) {
                one = [i];
                &one
            } else {
                // untargeted or random: the enemy that matters most
                match py::min_by_key(alive_now.iter().copied(), |&i| (-foes[i].worth(), foes[i].hp)) {
                    Some(i) => {
                        one = [i];
                        &one
                    }
                    None => &[],
                }
            };
            for (nth, &i) in victims.iter().enumerate() {
                let e = &mut foes[i];
                if c.poison != 0 && (e.poison > 0 || !c.poison_needs) && !_resists(e) {
                    e.poison += c.poison * x;
                }
                if c.doom != 0 && !_resists(e) {
                    let doom = c.doom * x + triggers::more_doom(&c, e);
                    // (Doom beyond its HP: nothing, unless a form follows)
                    score += (if e.adapts { doom } else { doom.min(0.max(e.hp - e.doom)) }) as f64 * 0.8 * dmg_value * e.worth();
                    e.doom += doom;
                }
                if c.enemy_hp_loss != 0 {
                    let lost_hp = e.hp.min(c.enemy_hp_loss);
                    e.hp -= lost_hp;
                    score += lost_hp as f64 * dmg_value * e.worth();
                }
                if c.shivs != 0 && nth == 0 {
                    // Shivs are free 4-damage cards: as good as that damage now
                    let removed = _hit(e, args.shiv_damage + might, c.shivs * x, false, false);
                    score += _progress(e, removed, chip_rate) * dmg_value * e.worth() * 0.9;
                }
            }
        }
        if c.needs_target && c.damage == 0 && aimed.is_none() && !alive_now.is_empty() && (c.poison != 0 || c.doom != 0) && n == 0 && first_target.is_none() {
            let i = py::min_by_key(alive_now.iter().copied(), |&i| (-foes[i].worth(), -foes[i].hp)).unwrap();
            first_target = Some(foes[i].entity_id.clone());
        }
        if trig.is_some() || c.doom_kills {
            let (won, shield) = triggers::fire(
                trig.unwrap_or(&triggers::NONE),
                &c,
                &mut foes,
                &before,
                &mut |e: &mut Enemy, d: i64| _hit(e, d, 1, false, true),
                &mut |e: &mut Enemy, hp: i64| _progress(e, hp, chip_rate) * dmg_value * e.worth(),
            );
            score += won;
            block += if can_block { shield } else { 0 };
        }
        // Stars are worth what they will be spent on: less the more of them there are.
        score += c.star_gain as f64
            * (if stars < 4 {
                2.5
            } else if stars < 8 {
                1.0
            } else {
                0.2
            })
            + c.souls as f64 * 2.5;
        stars += c.star_gain;
        enduring += c.forge as f64 * 0.5;
        later = later + c.next_draw as f64 * 2.0 * dmg_value + c.next_block as f64 * 0.8;
        nexte += c.next_energy * xs; // (energy: priced after the line)
        kept = kept || (c.keeps_hand.len() == 1 && c.keeps_hand[0] == "ANY") || c.keeps_hand.iter().any(|m| ft.tune.contains(m)); // (Ki Rest with a Wah or Cho Melody)
        energy += st.enter(&c.stance); // (after the card's own damage: Eruption hits, then she is in Wrath)
        if c.mantra != 0 {
            // (the Watcher's: at 10 she enters Divinity at once, and the 10 are used up)
            mantra += c.mantra * xs;
            if mantra >= MANTRA_DIVINITY {
                mantra -= MANTRA_DIVINITY;
                energy += st.enter("DIVINITY");
            }
        }
        if c.dexterity != 0 {
            nimble += c.dexterity;
            if c.dexterity_lasts {
                enduring += c.dexterity as f64 * 3.5;
            }
        }
        if rage_blocks && (!c.channel.is_empty() || c.evoke != 0 || c.evoke_all != 0) {
            _rage_blocks(&mut foes, &mut known_dead); // a crab this card killed has already handed on its Block
        }
        for _ in 0..xs {
            for kind in &card.channel {
                let kind = kind.as_str();
                if args.orb_slots != 0 && held.len() as i64 >= args.orb_slots {
                    evoke!(held.remove(0), false); // no room: the oldest orb is evoked to make space
                }
                let (mut passive, mut burst) = ORB_VALUES.get(kind).copied().unwrap_or((3, 6));
                if kind != "PLASMA" {
                    passive = 0.max(passive + args.focus_power);
                    burst = 0.max(burst + args.focus_power);
                }
                held.push((kind, passive, burst));
                lasting += match kind {
                    "FROST" => 3.0,
                    "LIGHTNING" => 3.0,
                    "DARK" => 4.0,
                    "PLASMA" => 6.0,
                    "GLASS" => 4.0,
                    _ => 3.0,
                };
            }
        }
        // "Evoke your rightmost Orb": the one channeled first, which the game lists first.
        for _ in 0..(if !held.is_empty() { c.evoke * xs } else { 0 }) {
            evoke!(held[0], true);
        }
        if c.evoke != 0 && !held.is_empty() {
            lasting -= 6.0 * ((held[0].0 == "PLASMA") as i64 as f64); // its energy every turn goes with it
            held.remove(0); // evoking uses the orb up
        }
        if c.evoke_all != 0 {
            // Shatter: every orb goes off, and all of them are used up
            for at in 0..held.len() {
                for _ in 0..c.evoke_all {
                    evoke!(held[at], true);
                }
            }
            lasting -= 6.0 * held.iter().filter(|o| o.0 == "PLASMA").count() as f64;
            held.clear();
        }
        escapes += (c.key == "FRANTIC_ESCAPE") as i64;
        if c.sadness != 0 || c.sad_turn.is_some() {
            sad += c.sadness;
            if let Some((rate, draw)) = c.sad_turn {
                sad_rate += rate;
                sad_draw += draw;
            }
        }
        junked += c.junk;
        if c.debris != 0 {
            // (Crash Landing fills what room the hand has left: fewest when it is played first)
            debris += if c.debris > 0 { c.debris } else { 0.max(10 - (hand_size - (n + 1) + drawn + debris)) };
        }
        buffer += (c.key == "BUFFER") as i64; // played now, it stops this turn's hit
        for e in foes.iter_mut() {
            if e.slow >= 0 {
                e.slow += 10; // for the cards after this one (the card itself gets no bonus)
            }
        }
        if args.tender {
            might -= 1;
            nimble -= 1;
        }
        if c.r#type == "Skill" {
            tainted += taint;
            // Enrage: each Skill, Strength for good
            score -= py::fsum(foes.iter().filter(|e| e.alive()).map(|e| e.enrage as f64 * ENRAGE_PRICE.get() * 1.max(e.incoming_hits) as f64 * fmin(4.0, 1.0 + e.hp as f64 / 35.0)));
        }
        drawn += c.draw * xs;
        if c.strength != 0 {
            might += c.strength;
            if c.strength_lasts {
                // pays off on every attack for the rest of the fight (or costs on every one: Friendship, Shared Fate)
                future += c.strength as f64 * 4.0 * span;
            }
        }
        if c.enemy_strength_lasts {
            // ... and Strength taken off an enemy for good comes off every hit it has left (this turn's share: in `incoming`)
            let all = c.aoe || t.is_none();
            let on = (if all { &alive[..] } else { &[] }).iter().copied().chain(if all { None } else { t });
            future += py::fsum(on.filter(|&i| foes[i].alive()).map(|i| (c.enemy_strength_loss * 1.max(foes[i].incoming_hits)) as f64 * 2.0 * foes[i].worth())) * span;
        }
        if c.enemy_strength_gain != 0 {
            // ... and Strength given to one goes on every hit it has left (Fight Me; until 2026-11-26 not seen at all)
            let all = c.aoe || t.is_none();
            let on = (if all { &alive[..] } else { &[] }).iter().copied().chain(if all { None } else { t });
            future -= py::fsum(on.filter(|&i| foes[i].alive()).map(|i| (c.enemy_strength_gain * 1.max(foes[i].incoming_hits)) as f64 * 2.0 * foes[i].worth())) * span;
        }
        if c.r#type == "Power" {
            // (as Enrage: Curiosity)
            score -= py::fsum(foes.iter().filter(|e| e.alive()).map(|e| e.curious as f64 * ENRAGE_PRICE.get() * 1.max(e.incoming_hits) as f64 * fmin(4.0, 1.0 + e.hp as f64 / 35.0)));
            // (An Ethereal Power not played now is never played: more in a long fight. A second Friendship is energy the hand has no cards for.)
            let again = if c.key == "FRIENDSHIP" {
                let again = 0.5f64.powf(friends as f64);
                friends += 1;
                again
            } else {
                1.0
            };
            // (Ennui's Sadness and draw: as fight/fool/sadness.rs works them out over the turns left.)
            let told = (c.strength_lasts && c.strength > 0) || (args.sadness.is_some() && c.sad_turn.is_some_and(|turn| turn.0 > 0));
            future += if told { 0.0 } else { again * _power_worth(&c, fight != "monster") * span * (if c.ethereal && fight != "monster" { 1.6 } else { 1.0 }) };
            future -= c.energy_drain as f64 * ENERGY_DRAIN.get() * (1 + drained) as f64 * span;
            drained += c.energy_drain;
        } else if c.r#type == "Status" || c.r#type == "Curse" {
            // only to use up energy nothing else can, unless playing it gets it out of the deck (Debris, Slimed)
            score += 0.1 + (if c.key == "DEBRIS" || c.key == "SLIMED" { junk_cost(fight, round_no, (deck.0 - 1, 0.max(deck.1 - 1)), 1) * dmg_value } else { 0.0 });
        } else if !c.known && !triggers::READ.contains(&c.key.as_str()) {
            score += 2.5 + c.cost.max(0) as f64;
        }
        future += triggers::kept(&c, dmg_value) * span; // (an Ethereal card not played now is Exhausted)
        if beating {
            // Beat of Death: each play hits us once it has resolved, Block first
            let beat = foes.iter().filter(|e| e.alive()).map(|e| e.beat).sum::<i64>() * k;
            if beat != 0 {
                let soak = 0.max(block).min(beat);
                block -= soak;
                self_loss = self_loss + beat - soak;
            }
        }
        if softens {
            // Malleable: the Block its hits earned it comes once the card is done
            for e in foes.iter_mut() {
                e.block += e.soft;
                e.soft = 0;
            }
        }
    }

    if healed != 0 {
        // HP given back is HP not lost, at the price HP has right now; and it is there for this turn's blow
        healed = healed.min(0.max(max_hp - hp));
        score += healed as f64 * (1.5 + 2.5 * (1.0 - hp as f64 / 1.max(max_hp) as f64)) * caution;
        hp += healed;
    }
    // Cards drawn: the free ones as they come, the rest as far as the energy left over stretches (and only as many as may still be played).
    drawn = match card_limit {
        None => drawn,
        Some(limit) => drawn.min(0.max(limit - plays)),
    };
    let (free, paid_share, per_energy) = args.draw_worth;
    score += drawn as f64 * fmax(1.0, free) + fmin(energy.max(0) as f64, drawn as f64 * paid_share) * per_energy;
    score += 15.0 * foes.iter().filter(|e| e.alive() && kept_block.contains(&&e.entity_id) && !e.keeps_block).count() as f64; // breaking Block it was keeping Stuns it
    // The Defect's orbs act when the turn ends, before the enemy does (Frost adds Block, Lightning hits something): one that dies of them does not attack.
    if respawn {
        _respawns(&mut foes);
    }
    if rage_blocks {
        _rage_blocks(&mut foes, &mut known_dead);
    }
    for &(kind, passive, _burst) in &held {
        if kind == "FROST" && can_block {
            block += passive;
        } else if kind == "LIGHTNING" || kind == "GLASS" {
            score += 0.9 * _orb_damage(kind, passive, &mut foes, chip_rate, dmg_value);
        }
    }
    if args.hail != 0 && held.iter().any(|o| o.0 == "FROST") {
        score += _hit_all(&mut foes, args.hail, None, chip_rate, dmg_value);
    }
    later += 0.max(mantra - args.mantra0) as f64 * MANTRA_LATER; // (Mantra short of Divinity: on the way to it)
    if let Some(sadness) = args.sadness {
        if sad != 0 || sad_rate != 0 || sad_draw != 0 {
            later += sad_hand::worth(sadness, sad, sad_rate, sad_draw);
        }
    }
    let fury = line.iter().any(|c| info(c).wrath_next); // Simmering Fury: next turn begins in Wrath, whatever this one ends in
    let calm_out = if fury && st.now == "CALM" { CALM_EXIT - CALM_LATER } else { 0 }; // (... and leaves Calm for its 2 energy then)
    // (Aim, Stretch)
    later += ft.later(dmg_value) + next_energy_worth(nexte + st.later_energy() + calm_out, args.room, if kept { 0.max(hand_size + drawn - line.len() as i64) } else { 0 }) * dmg_value;
    future += (lasting + enduring) * span + later;
    score += future;
    if debris != 0 {
        score -= junk_cost(fight, round_no, deck, debris) * dmg_value;
    }
    if junked != 0 {
        // Wounds, Dazed, Burns and Voids the line puts in our piles (until 2026-11-26 free): dead draws for the rest of the fight
        score -= junk_cost(fight, round_no, deck, junked) * dmg_value;
    }
    if dazed != 0 {
        // A Dazed is a draw lost (it is Ethereal): a card of ours, as far as the deck has live ones. The first two a turn cost little (a hand
        // of five has cards to spare), the rest a card each, and only as far as the fight will last long enough to draw them.
        let live = 10.max(deck.0);
        let left: i64 = foes.iter().filter(|e| e.alive() && e.hive != 0).map(|e| e.hp + e.block).sum();
        let in_time = if left != 0 { fmin(1.0, fmax(0.25, (left as f64 / HIVE_PACE.get()) / (live as f64 / 5.0))) } else { 0.0 };
        score -= HIVE_COST.get() * (0.35 * dazed.min(2) as f64 + 0.max(dazed - 2) as f64) * in_time * fmax(0.3, 1.0 - deck.1 as f64 / live as f64);
    }
    // Poison bites before the enemy acts: poisoned through is as good as dead. Otherwise what is added ticks for as long as the stack lasts.
    let poison_turns = if round_no <= late {
        match fight {
            "boss" => 7,
            "elite" => 4,
            _ => 3,
        }
    } else {
        3
    };
    for (e, orig) in foes.iter_mut().zip(enemies) {
        if !e.alive() {
            continue;
        }
        if (if e.hit_cap != 0 { e.poison.min(e.hit_cap) } else { e.poison }) >= e.hp {
            // (not through Intangible)
            e.hp = 0;
            continue;
        }
        if e.poison > orig.poison {
            let gain = _poison_total(e.poison, poison_turns) - _poison_total(orig.poison, poison_turns);
            score += e.hp.min(gain) as f64 * e.worth() * (if e.hp > 60 { dmg_value } else { 1.0 });
        }
        score += e.hp.min(e.poison) as f64 * 0.3 * e.worth(); // the tick that comes this turn
    }
    // Vulnerable is half again our damage and Weak a quarter off theirs, for every turn they last: in long fights more than any single attack.
    let vuln_turn = (match fight {
        "boss" => 7.0,
        "elite" => 5.0,
        _ => 3.0,
    }) * debuff_worth;
    let weak_turn = (match fight {
        "boss" => 5.0,
        "elite" => 3.5,
        _ => 2.0,
    }) * debuff_worth;
    for e in &foes {
        if e.alive() {
            // (The first turn of Weak is in `incoming` already, and Vulnerable used this turn in the damage dealt.)
            score += (0.max(e.vuln_added - 1) as f64 + 0.5 * e.vuln_added.min(1) as f64) * vuln_turn * e.worth();
            score += (0.max(e.weak_added - 1) as f64 + 0.3 * e.weak_added.min(1) as f64) * weak_turn * (if e.incoming_hits != 0 || !e.minion { 1.0 } else { 0.5 });
        }
    }
    score += py::fsum(foes.iter().zip(enemies).filter(|(e, orig)| e.alive() && e.artifact < orig.artifact).map(|(e, orig)| artifact::gain(orig.artifact, e.artifact, args.strip) * e.worth()));
    score += 20.0 * py::fsum(foes.iter().zip(enemies).filter(|(e, orig)| e.alive() && e.doom >= e.hp && orig.doom < orig.hp).map(|(e, _)| e.worth())); // Doom kills at the end of its turn
    if respawn {
        _respawns(&mut foes); // a Stock enemy killed by the last card comes back too
    }
    let kills = foes.iter().zip(enemies).filter(|(e, orig)| !e.alive() && orig.alive()).count() as i64;
    score += 25.0 * py::fsum(foes.iter().zip(enemies).filter(|(e, orig)| !e.alive() && orig.alive()).map(|(e, _)| e.worth()));
    score += FORM_WORTH.get() * foes.iter().filter(|e| e.adapted).count() as f64; // a form beaten: that much of the fight is done
    let mut survivors: Vec<usize> = (0..foes.len()).filter(|&i| foes[i].alive()).collect();
    if survivors.is_empty() || (survivors.iter().all(|&i| foes[i].minion) && !enemies.iter().all(|e| e.minion)) {
        // every leader dead: minions leave with them
        score += WIN_BONUS.get();
        survivors.clear();
    }
    if kills != 0 && !survivors.is_empty() {
        // Crab Rage: the one left standing gets 99 Block (its attacks this turn are wasted, see _rage_blocks) and 6 Strength for good.
        for &i in &survivors {
            let e = &mut foes[i];
            if e.rage {
                score -= kills as f64 * RAGE_COST.get() * e.hp as f64;
                e.incoming += 6 * e.incoming_hits * kills;
            }
        }
    }
    if let (Some(sandpit), false) = (args.sandpit, survivors.is_empty()) {
        // The Insatiable: Sandpit counts down every turn and at 0 we are eaten; each Frantic Escape played pushes it back by one (sandpit.rs).
        if sandpit + escapes <= 1 {
            score -= LETHAL_PENALTY.get();
        } else {
            let dealt = enemies.iter().filter(|e| !e.minion).map(|e| e.max_hp - e.hp).max().unwrap_or(0);
            score -= pit::price(sandpit + escapes, survivors.iter().map(|&i| foes[i].hp).max().unwrap(), dealt, round_no, args.escape_odds);
        }
    }
    if let Some(faced) = &faced {
        // turning to face an enemy takes away its 50% bonus and gives it to the other one
        for &i in &survivors {
            let e = &mut foes[i];
            if e.flanker && e.behind && e.entity_id == *faced {
                e.incoming = py::round(e.incoming as f64 / 1.5);
            } else if e.flanker && !e.behind && e.entity_id != *faced {
                e.incoming = py::round(e.incoming as f64 * 1.5);
            }
        }
    }
    let survivors: Vec<&Enemy> = survivors.iter().map(|&i| &foes[i]).collect();
    let incoming = py::round(survivors.iter().map(|e| e.incoming + tainted * e.incoming_hits).sum::<i64>() as f64 * st.taken());
    if block <= 0 && can_block {
        block = args.bare_block;
    }
    let played = |index: i64| line.iter().any(|c| info(c).index == index);
    let left = 0.max(hand_size - line.len() as i64);
    // Cards that burn in the hand go off when our turn ends, before the enemy moves: the ones that deal damage eat into our Block first.
    let mut scorch: i64 = args.burns.iter().filter(|(index, _, blocked)| *blocked && !played(*index)).map(|(_, amount, _)| *amount).sum();
    let mut burnt: i64 = args.burns.iter().filter(|(index, _, blocked)| !*blocked && !played(*index)).map(|(_, amount, _)| if *amount < 0 { left } else { *amount }).sum();
    if args.wither_in > 0 && line.len() as i64 >= args.wither_in {
        // a Wither lands in the hand and burns when the turn ends
        scorch += args.wither_hp * (1 + py::floordiv(line.len() as i64 - args.wither_in, 6));
    }
    let guard = pet.max(0).min(block.max(0)); // (Osty stops attacks and nothing else)
    let own = 0.max(block - guard) * args.fortify; // Block that is ours
    burnt += 0.max(scorch - own);
    let mut through = 0.max(incoming - 0.max(own - scorch) - guard);
    if through != 0 && buffer >= survivors.iter().filter(|e| e.incoming != 0).map(|e| e.incoming_hits).sum::<i64>() {
        through = 0; // Buffer swallows each hit that would have hurt
    }
    if through != 0 {
        // only the hits that get through leave a scar
        for e in &survivors {
            if e.incoming != 0 && e.scar != 0.0 {
                let per = 1.max(py::floordiv(e.incoming, 1.max(e.incoming_hits)));
                score -= e.scar * e.incoming_hits.min(-py::floordiv(-through, per)) as f64;
            }
        }
    }
    if args.wither_in > 0 {
        // ... and a Wither stays in the deck, burning whenever it is drawn: every card played is a sixth of that bill
        score -= line.len() as f64 * args.wither_bill;
    }
    let mut lost = through + self_loss + args.end_loss + burnt;
    lost = 0.max(lost - args.regen.min(0.max(max_hp - hp + self_loss))); // Regen mends what was missing first
    lost = if args.loss_cap != 0 { lost.min(args.loss_cap) } else { lost };
    let ratio = hp as f64 / 1.max(max_hp) as f64;
    // Imbalanced (Bowlbug Rock): an attack of his that our Block stops whole leaves him Stunned for his next turn (Osty's HP left out).
    let mut cover = 0.max(block - pet.max(0) - scorch);
    for e in &survivors {
        let hit = e.incoming + tainted * e.incoming_hits;
        if e.imbalanced && hit != 0 && cover >= hit {
            score += 0.85 * hit as f64 * (1.5 + 2.5 * (1.0 - ratio)) * caution;
        }
        cover -= cover.min(hit);
    }
    // Osty keeps the HP he is not hit for: unlike Block it is still there next turn (and what he soaks now he cannot soak later).
    if pet > 0 {
        let kept = pet.min(0.max(0.max(own - scorch) + guard - incoming));
        // (less than ours: the fight may end first)
        score += kept as f64
            * (1.5 + 2.5 * (1.0 - ratio))
            * caution
            * (match fight {
                "boss" => 0.85,
                "elite" => 0.7,
                _ => 0.5,
            });
        if args.mastery != 0 {
            // ... and under Necro Mastery what he is hit for, every enemy is hit for as well
            score += ((pet - kept) * args.mastery) as f64 * dmg_value * py::fsum(survivors.iter().map(|e| e.worth()));
        }
    }
    if args.holds {
        // Block that outlasts the turn is next turn's Block (as Osty's HP is, and a little surer)
        score += 0.max(block - pet.max(0) - scorch - incoming) as f64
            * (1.5 + 2.5 * (1.0 - ratio))
            * caution
            * (match fight {
                "boss" => 0.8,
                "elite" => 0.7,
                _ => 0.5,
            });
    }
    // Every leader left is Doomed to die at the end of its turn: the fight is won if we live through that turn, so more damage is wasted energy.
    let mut leaders = survivors.iter().filter(|e| !e.minion).peekable();
    let doomed = leaders.peek().is_some() && leaders.all(|e| e.doom >= e.hp) && !survivors.iter().any(|e| e.adapts || e.spent);
    if doomed {
        score += WIN_BONUS.get() / 2.0;
    }
    // A point of our HP is worth more than a point of enemy HP even at full health, and more still when this one turn would take a big bite of what is left.
    let bite = fmin(1.0, lost as f64 / 1.max(hp) as f64);
    let worth = HP_BASE.get() + HP_SLOPE.get() * (1.0 - ratio) + HP_BITE.get() * bite;
    let mut price = lost as f64 * worth * (if doomed { 4.0 } else { 1.0 }) * caution;
    price *= closing::discount(args.pace, lost, hp, &survivors); // (a boss about to die: the HP we keep past its death buys nothing)
    score -= price;
    if let (Some((odds, blows, odds_fury)), true) = (&args.wrath, (st.now == "WRATH" || fury) && !survivors.is_empty() && lost < hp) {
        // Ending the turn in Wrath: next turn's blows land double unless the next hand holds a way out (2026-11-25: a Hyper Beam of 104).
        let odds = if fury { *odds_fury } else { *odds }; // (Simmering Fury's cards drawn are more chances of a way out)
        // (a first turn: what it shows now)
        let seen: Vec<(f64, f64)> = survivors.iter().map(|e| blows.get(&*e.entity_id).copied().unwrap_or_else(|| (e.incoming as f64 / TAKEN.get(st.start).copied().unwrap_or(1.0), 1.0))).collect();
        score -= carry_cost(odds, py::fsum(seen.iter().map(|(b, _)| *b)), hp - lost, py::max_of(seen.iter().filter(|(b, _)| *b > 0.0).map(|(_, share)| *share)).unwrap_or(0.0));
    }
    if (line.len() as i64) < args.wither_in {
        // stopping short of the next Wither only puts it off (the count carries over) to the next turn's first card
        score -= args.wither_hp as f64 * (1.5 + 2.5 * (1.0 - ratio)) * caution;
    }
    if lost >= hp {
        // Dead on paper is dead: the HP is not priced; what counts is cards drawn while there is energy to play them, and how near the line comes to a kill.
        let short = lost - hp;
        score += price - fmax(0.0, future); // (nor is there a later)
        score -= LETHAL_PENALTY.get() - (if energy > 0 { 150.0 } else { 30.0 }) * drawn.min(3) as f64;
        score += 400.0 * fmax(0.0, 1.0 - short as f64 / 12.0) + 3000.0 * fmax(0.0, 1.0 - kill_reach(&survivors, lost - through < hp) / 30.0);
    }
    Some(Outcome { score, first_target, lost })
}

/// HP the planner expects to lose if the turn ended now (logged, to catch what it does not model).
pub fn expected_end_loss(state: &Value) -> Option<i64> {
    let enemies = read_enemies(state);
    if enemies.is_empty() {
        return None;
    }
    Some(simulate::<CardInfo, _>(&[], &enemies, _context(state)).unwrap().lost) // (an empty line can always be afforded)
}

/// `int(x or default)`.
fn _int_or(x: &Value, default: i64) -> i64 {
    if x.truthy() {
        x.int()
    } else {
        default
    }
}

/// The keywords of `simulate` for the turn this state shows.
pub fn _context(state: &Value) -> SimulateArgs {
    let player = &state["player"];
    let battle = &state["battle"];
    let draws = _draw_worth(player);
    let foes = battle["enemies"].list();
    let round_no = _int_or(&battle["round"], 1);
    let fight = if state["state_type"].truthy() { state["state_type"].text() } else { "monster".to_string() };
    let mut burns = hand_burns(player);
    // (damage that Block stops)
    burns.extend([end_of_turn_loss(player), constrict(player)].into_iter().enumerate().filter(|(_, hp)| *hp != 0).map(|(n, hp)| (-1 - n as i64, hp, true)));
    let wither_hp = _next_wither(player, round_no);
    SimulateArgs {
        energy: player["energy"].int(),
        stars: player["stars"].int(),
        hp: 1.max(_int_or(&player["hp"], 1) - 0.max(power_amount(player, "DOOM")) - 0.max(power_amount(player, "NEUROSURGE"))), // (Doom on us kills once it reaches our HP)
        max_hp: _int_or(&player["max_hp"], 1),
        block: player["block"].int() + end_of_turn_block(player) + osty_hp(player),
        fortify: _int_or(&state["_fortify"], 1),
        round_no,
        card_limit: cards_left_this_turn(state),
        can_block: !_has_power(player, "NO_BLOCK"),
        sandpit: _sandpit(state),
        escape_odds: _escape_odds(player, 5),
        burns,
        hand_size: player["hand"].list().len() as i64,
        wither_in: foes.iter().map(|e| power_amount(e, "WITHERING_PRESENCE")).max().unwrap_or(0),
        wither_hp,
        wither_bill: match WITHER_BILL_TIMES.with(|t| t.get()) {
            Some(times) => _wither_bill(player, round_no, wither_hp) * times,
            None => _wither_bill(player, round_no, wither_hp),
        },
        draw_worth: draws,
        orbs: player["orbs"].list().iter().map(|o| (o["id"].or(&o["name"]).text().to_uppercase().replace("_ORB", ""), o["passive_val"].int(), o["evoke_val"].int())).collect(),
        orb_slots: player["orb_slots"].int(),
        bare_block: _bare_block(player),
        regen: 0.max(power_amount(player, "REGEN")),
        focus_power: if _has_power(player, "FOCUS") { power_amount(player, "FOCUS") } else { 0 },
        shiv_damage: 4 + 0.max(power_amount(player, "ACCURACY")),
        tender: _has_power(player, "TENDER"),
        hail: 0.max(power_amount(player, "HAILSTORM")),
        deck: deck_count(player),
        strip: if foes.iter().any(|e| _has_power(e, "ARTIFACT")) { artifact::outlook(player, &fight, round_no) } else { (0.0, 0.0) },
        vigor: 0.max(power_amount(player, "VIGOR")),
        giant: _has_power(player, "GIGANTIFICATION"),
        buffer: 0.max(power_amount(player, "BUFFER")),
        loss_cap: loss_cap(player),
        trig: triggers::read(state),
        pet: if player["pets"].truthy() { osty_hp(player) } else { -1 },
        mastery: 0.max(power_amount(player, "NECRO_MASTERY")),
        might0: state["_might"].int(),
        holds: _has_power(player, "BARRICADE"),
        friends: 0.max(power_amount(player, "FRIENDSHIP")),
        drained: 0.max(power_amount(player, "ENERGY_DOWN")),
        free: free_plays(state),
        doubled: _has_power(player, "DUPLICATION") || 0.max(power_amount(player, "ECHO_FORM")) > state["_plays_this_turn"].int(), // (Echo Form: the first card(s) a turn)
        taint: std::iter::once(player).chain(foes).map(|e| power_amount(e, "VITAL_SPARK")).max().unwrap_or(0),
        pace: closing::pace(state),
        room: next_room(state),
        arrows: arrows_owned(player),
        aiming: 0.max(power_amount(player, "AIM")),
        stretch: 0.max(power_amount(player, "STRETCH")),
        tune: played_melodies(&state["_played_names"].list().iter().map(|name| name.text()).collect::<Vec<_>>()),
        chord: 0.max(power_amount(player, "VIOLENT_CHORD")),
        boom: 0.max(power_amount(player, "EXPLOSIVE_ARROWS")),
        fool: fool_powers(player, state["_dislodged"].truthy()),
        stance0: player["stance"].text(),
        wrath: _wrath_outlook(state),
        sadness: sad_hand::outlook(state, draws),
        mantra0: 0.max(power_amount(player, "MANTRA")),
        fight,
        // (the rest: as `simulate` has them when they are not given)
        end_loss: 0,
        focus: None,
    }
}

/// For a Watcher who is in Wrath or holds a card that enters it: the chance that next turn's hand holds no way out, and each
/// enemy's blow on the turns it attacked, as it would be outside Wrath, with how often it attacked (`_blow`, kept by the bot). Else None.
pub fn _wrath_outlook(state: &Value) -> Option<Wrath> {
    let player = &state["player"];
    if player["stance"].is_null() {
        return None; // (a game without stances)
    }
    let hand: Vec<CardInfo> = player["hand"].list().iter().map(parse).collect();
    if read_stance(&player["stance"]) != "WRATH" && !hand.iter().any(|c| c.stance == "WRATH" || c.wrath_next) {
        return None;
    }
    let fury = hand.iter().filter(|c| c.wrath_next).map(|c| c.next_draw).max().unwrap_or(0);
    let blows = state["_blow"].entries().map(|(id, seen)| (id.clone(), (seen[0].float(), seen[1].float()))).collect();
    Some((stuck_odds(player, 0), blows, if fury != 0 { stuck_odds(player, fury) } else { 1.0 }))
}

/// What `choose_card` leaves for its caller: `lost`, the HP the best line still loses; its `score`; the `line` itself.
/// (Python's `plan` dict: a field is None while the key is not there.)
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Plan {
    pub lost: Option<i64>,
    pub score: Option<f64>,
    pub line: Option<Vec<CardInfo>>,
}

/// A line's cards as the memo knows them: each card's sort and whether it is aimed, and the aim (its place in `aims`; 0 for a line with nothing aimed).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum MemoKey {
    Packed(u128, usize),
    Long(Vec<(usize, bool)>, usize),
}

/// `itertools.permutations(range(n))` after the first: the next arrangement in its order, false after the last.
fn next_permutation(order: &mut [usize]) -> bool {
    let n = order.len();
    if n < 2 {
        return false;
    }
    let mut i = n - 1;
    while i > 0 && order[i - 1] >= order[i] {
        i -= 1;
    }
    if i == 0 {
        return false;
    }
    let mut j = n - 1;
    while order[j] <= order[i - 1] {
        j -= 1;
    }
    order.swap(i - 1, j);
    order[i..].reverse();
    true
}

/// `itertools.combinations(range(n), len(picked))` after the first: the next choice in its order, false after the last.
fn next_combination(picked: &mut [usize], n: usize) -> bool {
    let size = picked.len();
    let mut i = size;
    while i > 0 {
        i -= 1;
        if picked[i] != i + n - size {
            picked[i] += 1;
            for j in i + 1..size {
                picked[j] = picked[j - 1] + 1;
            }
            return true;
        }
    }
    false
}

/// Best card to play right now, or None to end the turn (`plan.lost`: HP the best line still loses).
/// (Answered by a test's stand-in while one is in place: `with_choose_card_hook`.)
pub fn choose_card(state: &Value, plan: Option<&mut Plan>) -> Option<Action> {
    match CHOOSE_CARD.with(|h| h.borrow().clone()) {
        Some(hook) => hook(state, plan),
        None => _choose_card(state, plan),
    }
}

/// `choose_card` itself, whatever stands in for it.
pub fn _choose_card(state: &Value, plan: Option<&mut Plan>) -> Option<Action> {
    let player = &state["player"];
    let enemies = read_enemies(state);
    let ctx = _context(state);
    let mut hand = triggers::counted(player["hand"].list().iter().map(parse).collect());
    let free = ctx.free;
    if free != 0 {
        // Void Form: with free plays left every card shows as costing 0; what each costs after them is what the deck said when the fight began.
        for c in hand.iter_mut() {
            if let Some(cost) = state["_costs"].get(c.name.as_str()) {
                c.cost = cost.int();
            }
        }
    }
    let hand = hand;
    let mut playable: Vec<&CardInfo> = hand.iter().filter(|c| c.playable && (c.is_x() || free != 0 || c.cost <= ctx.energy) && c.star_cost <= ctx.stars).collect();
    playable.retain(|c| !triggers::shunned(c, state));
    let owned = |cards: &[&CardInfo]| cards.iter().map(|c| (*c).clone()).collect::<Vec<CardInfo>>();
    if enemies.is_empty() && state["_revive_pending"].truthy() {
        return _set_up(&owned(&playable), ctx.energy);
    }
    if enemies.len() == 1 && enemies[0].asleep >= 2 && ctx.round_no <= 2 {
        // Asleep ("Awakens upon losing HP or after 3 turns"): she wakes on her own time whatever we do, so the turns before the last are for what lasts.
        return _set_up(&owned(&playable), ctx.energy);
    }
    if playable.is_empty() || enemies.is_empty() {
        if let (Some(plan), false) = (plan, enemies.is_empty()) {
            if let Some(base) = simulate::<CardInfo, _>(&[], &enemies, &ctx) {
                // (ending the turn is the plan: handpick prices a card by it)
                plan.lost = Some(base.lost);
                plan.score = Some(base.score);
                plan.line = Some(Vec::new());
            }
        }
        return None;
    }
    // Cards we cannot pay for yet can still come later in a line that first makes the energy or Stars for them (evoking a Plasma orb too).
    let orbs = &ctx.orbs;
    let pays = |c: &CardInfo| {
        c.energy_gain != 0
            || c.star_gain != 0
            || triggers::PAYS.contains(&c.key.as_str())
            || (c.evoke != 0 && !orbs.is_empty() && orbs[0].0 == "PLASMA")
            || (c.evoke_all != 0 && orbs.iter().any(|o| o.0 == "PLASMA"))
    };
    let makers = playable.iter().any(|c| pays(c) || triggers::FREE_NEXT.contains_key(c.key.as_str()));
    if makers {
        for c in hand.iter() {
            if c.short && !playable.contains(&c) {
                playable.push(c);
            }
        }
    }
    let most = MAX_SEARCH_CARDS.get();
    if playable.len() as i64 > most {
        py::sort_by_key(&mut playable, |c| -(c.total_damage() + c.block + 3 * c.draw));
        playable.truncate(most.max(0) as usize);
    }
    if ctx.card_limit.is_some_and(|limit| limit <= 0) {
        return None;
    }
    if ctx.card_limit.is_some_and(|limit| limit <= 3) {
        py::sort_by_key(&mut playable, |c| (-(c.total_damage() + c.block), c.index)); // few plays allowed: spend them on what does the most now
    } else if enemies.iter().any(|e| e.slow >= 0) {
        py::sort_by_key(&mut playable, |c| (if c.damage != 0 { 1 } else { 0 }, c.total_damage(), c.index)); // Slow: every card played makes later attacks hit harder
    } else if enemies.iter().any(|e| e.slippery != 0) {
        py::sort_by_key(&mut playable, |c| (if c.damage == 0 { order_key(c).0 } else { 3 }, c.damage, c.index)); // Slippery: burn it off with the weakest hits
    } else {
        // (Flatten and Rattle after Osty's other attacks: one is free then, the other hits again. No Escape after the other Doom.)
        let after = |c: &CardInfo| -> (i64, f64, i64) {
            let key = order_key(c);
            if c.key == "FLATTEN" || c.key == "RATTLE" || c.key == "NO_ESCAPE" {
                (key.0, 1e3, c.index)
            } else {
                (key.0, key.1 as f64, key.2)
            }
        };
        py::sort_by_key(&mut playable, |c| {
            if pays(c) {
                let key = order_key(c);
                (1, key.1 as f64, key.2)
            } else {
                after(c)
            }
        });
    }
    // The free plays go to the dearest cards; a card that ends the turn (Void Form itself) goes last.
    let mut dear: Vec<&CardInfo> = playable.iter().copied().filter(|c| c.cost > 0).collect();
    py::sort_by_key(&mut dear, |c| -c.cost);
    dear.truncate(free.max(0) as usize);
    if ctx.doubled && !playable.is_empty() && !playable.iter().any(|c| c.r#type == "Power") {
        dear.push(py::max_by_key(playable.iter().copied(), |c| c.total_damage() + c.block + 3 * c.draw).unwrap()); // the card worth playing twice leads
    }
    py::sort_by_key(&mut playable, |c| (c.ends_turn, !dear.contains(c))); // (stable: the rest keep their order)
    // Bound cards: one per turn (once one is played the game marks the rest unplayable, so any still playable means it is unused).
    let bound_left: Option<i64> = if playable.iter().any(|c| c.bound) { Some(1) } else { None };
    let skill_left: Option<i64> = if _has_power(&state["player"], "SMOGGY") { Some(1) } else { None }; // Smoggy ("You can only play 1 Skill per turn") the same way
    // Copies of a card that differ only in their place in the hand, and a line with nothing aimed at every aim, play out alike: simulated once.
    let mut sorts: Vec<CardInfo> = Vec::new();
    let sort_of: Vec<(usize, bool)> = playable
        .iter()
        .map(|c| {
            let sort = if ctx.burns.iter().any(|b| b.0 == c.index) { (*c).clone() } else { CardInfo { index: -1, ..(*c).clone() } };
            let id = match sorts.iter().position(|known| *known == sort) {
                Some(id) => id,
                None => {
                    sorts.push(sort);
                    sorts.len() - 1
                }
            };
            (id, c.damage != 0 || c.needs_target || c.osty_attack || c.key == "TIMES_UP" || c.key == "TIME_S_UP")
        })
        .collect();
    let mut memo: HashMap<MemoKey, Option<Outcome>> = HashMap::new();
    let baseline = simulate::<CardInfo, _>(&[], &enemies, &ctx);
    let (mut best_score, mut best_lost) = match &baseline {
        Some(baseline) => (baseline.score, Some(baseline.lost)),
        None => (f64::NEG_INFINITY, None),
    };
    let mut best_line: Vec<&CardInfo> = Vec::new();
    let mut best_target: Option<Arc<str>> = None;
    // Each line is also tried aimed wholly at one enemy: when surrounded (whom we face decides who hits us from behind), and in any small group.
    let mut aims: Vec<Option<Focus>> = vec![None];
    aims.extend(enemies.iter().filter(|e| e.flanker).map(|e| Some(Focus::Last(e.entity_id.to_string()))));
    aims.extend(enemies.iter().filter(|e| e.flanker || (1 < enemies.len() && enemies.len() <= 3 && e.incoming > 0)).map(|e| Some(Focus::At(e.entity_id.to_string()))));
    // Order matters too (the Slow bonus, what an X card leaves): short lines are tried in every order, the usual one first so that it wins ties.
    let mut budget: i64 = 2500;
    let cards = playable.len();
    let packed = cards <= 16 && sorts.len() < 127;
    let mut order: Vec<&CardInfo> = Vec::with_capacity(cards);
    for size in 1..=cards {
        let mut found = false;
        let mut picked: Vec<usize> = (0..size).collect();
        // keeps order_key order
        loop {
            let line = &picked;
            let skip = bound_left.is_some_and(|left| line.iter().filter(|&&i| playable[i].bound).count() as i64 > left) // "Only 1 Bound card can be played each turn."
                || skill_left.is_some_and(|left| line.iter().filter(|&&i| playable[i].r#type == "Skill").count() as i64 > left);
            if !skip {
                let every_order = 1 < size && size <= 4;
                let mut places: Vec<usize> = (0..size).collect(); // starts with `line` itself
                let mut nth = 0;
                loop {
                    if nth != 0 && budget <= 0 {
                        break;
                    }
                    let mut usable = true;
                    let any_aimed = places.iter().any(|&p| sort_of[line[p]].1);
                    let sig: Option<u128> = if packed { Some(places.iter().fold(0u128, |sig, &p| (sig << 8) | (((sort_of[line[p]].0 as u128 + 1) << 1) | sort_of[line[p]].1 as u128))) } else { None };
                    order.clear();
                    order.extend(places.iter().map(|&p| playable[line[p]]));
                    for (at, aim) in aims.iter().enumerate() {
                        if nth != 0 {
                            budget -= 1;
                        }
                        let at = if any_aimed { at } else { 0 };
                        let key = match sig {
                            Some(sig) => MemoKey::Packed(sig, at),
                            None => MemoKey::Long(places.iter().map(|&p| sort_of[line[p]]).collect(), at),
                        };
                        let out = memo.entry(key).or_insert_with(|| simulate_aimed(&order, &enemies, &ctx, aim.as_ref()));
                        let Some(out) = out else {
                            usable = false;
                            break;
                        };
                        found = true;
                        if out.score > best_score + 1e-9 {
                            best_score = out.score;
                            best_line.clear();
                            best_line.extend(order.iter().copied());
                            best_target = out.first_target.clone();
                            best_lost = Some(out.lost);
                        }
                    }
                    if !usable && nth == 0 && !line.iter().any(|&i| playable[i].is_x() || pays(playable[i]) || playable[i].key == "FLATTEN" || triggers::FREE_NEXT.contains_key(playable[i].key.as_str())) {
                        break; // cannot be paid for in any order
                    }
                    nth += 1;
                    if !(every_order && next_permutation(&mut places)) {
                        break;
                    }
                }
            }
            if !next_combination(&mut picked, cards) {
                break;
            }
        }
        if !found {
            break;
        }
    }
    if let Some(plan) = plan {
        plan.lost = best_lost;
        plan.score = Some(best_score);
        plan.line = Some(best_line.iter().map(|c| (*c).clone()).collect());
    }
    if best_line.is_empty() {
        return None;
    }
    let card = best_line[0];
    let mut params = json!({"card_index": card.index});
    if card.needs_target {
        params["target"] = match best_target.filter(|id| !id.is_empty()) {
            Some(id) => json!(&*id),
            None => json!(&*py::min_by_key(enemies.iter(), |e| e.hp + e.block).unwrap().entity_id),
        };
    }
    Some(("play_card".to_string(), params))
}

/// Ordered candidate actions for a combat state (first workable one is taken).
pub fn actions(state: &Value) -> Vec<Action> {
    if LOG.enabled(Level::Debug) {
        crate::log_debug!(LOG, "combat {}", describe(state));
    }
    if LOG.enabled(Level::Debug) {
        if let Some(sandpit) = _sandpit(state) {
            let odds = _escape_odds(&state["player"], 5);
            crate::log_debug!(LOG, "sandpit {}, no Escape next hand {:.2}, later {:.2}", sandpit, odds.0, odds.1);
        }
    }
    let mut plan = Plan::default();
    let mut card = choose_card(state, Some(&mut plan));
    if plan.lost.unwrap_or(0) >= _int_or(&state["player"]["hp"], 1) && fortifier(&state["player"]) {
        // Dead on paper with a Fortifier on the belt: it is drunk after the last card (potions._ill_timed), so the cards are chosen for the Block it will triple.
        let mut braced = Plan::default();
        let mut fortified = state.clone();
        fortified["_fortify"] = json!(3);
        let with_it = choose_card(&fortified, Some(&mut braced));
        if braced.lost.or(plan.lost) < plan.lost {
            // (the potions still go by the loss without it: the danger it is drunk for)
            card = with_it;
            plan.line = Some(braced.line.unwrap_or_default());
        }
    }
    // (nothing playable: no plan)
    let planned_loss = if plan.lost.is_some() { plan.lost } else { expected_end_loss(state) };
    let mut out = potion_actions(state, planned_loss, Some(plan.line.as_deref().unwrap_or(&[])));
    if let Some(card) = card {
        out.push(card);
    }
    out.push(("end_turn".to_string(), json!({})));
    out
}

/// What the golden tests of the planner and of run/potions.rs share: the cases of tests/golden/combat, read back.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::fight::battle::testing::card_from_json;
    use crate::games;

    /// The tables the cases refer to: whole cards, relics and potions of the states (`atoms`), parsed cards, and the settings in force.
    pub struct Golden {
        pub atoms: Vec<Value>,
        pub cards: Vec<CardInfo>,
        pub knobs: Vec<Vec<(String, f64)>>,
    }

    /// Every list under `key` in these files, one after the other.
    pub fn rows(texts: &[&str], key: &str) -> Vec<Value> {
        let mut out = Vec::new();
        for text in texts {
            let v: Value = serde_json::from_str(text).unwrap();
            out.extend(v[key].list().iter().cloned());
        }
        out
    }

    pub fn golden() -> Golden {
        let meta = [include_str!("../../tests/golden/combat/meta.json")];
        let atoms = rows(&[include_str!("../../tests/golden/combat/atoms0.json"), include_str!("../../tests/golden/combat/atoms1.json")], "atoms");
        let knobs = rows(&meta, "knobs").iter().map(|k| k.entries().map(|(name, x)| (name.clone(), x.float())).collect()).collect();
        Golden { atoms, cards: rows(&meta, "cards").iter().map(card_from_json).collect(), knobs }
    }

    /// The game and settings a case was recorded under, in force until dropped.
    pub struct Env {
        _game: games::Playing,
        _knobs: crate::learn::KnobGuard,
    }

    impl Golden {
        pub fn env(&self, v: &Value) -> Env {
            let knobs: Vec<(&str, f64)> = self.knobs[v["k"].int() as usize].iter().map(|(name, x)| (name.as_str(), *x)).collect();
            Env { _game: games::playing(v["g"].s()), _knobs: crate::learn::with_knobs(&knobs) }
        }

        fn item(&self, x: &Value) -> Value {
            match x {
                Value::Number(id) => self.atoms[id.as_u64().unwrap() as usize].clone(),
                Value::Array(pair) => {
                    let mut card = serde_json::Map::new();
                    card.insert("index".to_string(), pair[1].clone());
                    card.extend(self.atoms[pair[0].int() as usize].as_object().unwrap().clone());
                    Value::Object(card)
                }
                other => other["="].clone(),
            }
        }

        /// The player as recorded: the cards, relics and potions of its piles put back.
        pub fn player(&self, packed: &Value) -> Value {
            let mut player = packed.clone();
            for pile in ["hand", "draw_pile", "discard_pile", "exhaust_pile", "relics", "potions"] {
                if packed[pile].is_array() {
                    player[pile] = packed[pile].list().iter().map(|x| self.item(x)).collect();
                }
            }
            player
        }

        pub fn state(&self, packed: &Value) -> Value {
            let mut state = packed.clone();
            if packed["player"].is_object() {
                state["player"] = self.player(&packed["player"]);
            }
            if packed["_costs"].has("@") {
                state["_costs"] = self.atoms[packed["_costs"]["@"].int() as usize].clone();
            }
            state
        }

        /// Parsed cards by their place in the table (None for null).
        pub fn line(&self, ids: &Value) -> Option<Vec<CardInfo>> {
            ids.as_array().map(|ids| ids.iter().map(|id| self.cards[id.int() as usize].clone()).collect())
        }
    }

    pub fn wrath(v: &Value) -> Option<Wrath> {
        if v.is_null() {
            return None;
        }
        Some((v[0].float(), v[1].entries().map(|(id, seen)| (id.clone(), (seen[0].float(), seen[1].float()))).collect(), v[2].float()))
    }

    /// `simulate`'s keywords from the ones a case gives (the rest: the defaults).
    pub fn args(v: &Value) -> SimulateArgs {
        let mut a = SimulateArgs::default();
        let two = |x: &Value| (x[0].float(), x[1].float());
        let whole = |x: &Value| {
            assert!(x.is_int() || x.is_boolean(), "not an int: {x}");
            x.int()
        };
        for (k, x) in v.entries() {
            match k.as_str() {
                "energy" => a.energy = whole(x),
                "stars" => a.stars = whole(x),
                "hp" => a.hp = whole(x),
                "max_hp" => a.max_hp = whole(x),
                "block" => a.block = whole(x),
                "fight" => a.fight = x.s().to_string(),
                "round_no" => a.round_no = whole(x),
                "card_limit" => a.card_limit = if x.is_null() { None } else { Some(whole(x)) },
                "can_block" => a.can_block = x.truthy(),
                "sandpit" => a.sandpit = if x.is_null() { None } else { Some(whole(x)) },
                "end_loss" => a.end_loss = whole(x),
                "burns" => a.burns = x.list().iter().map(|b| (whole(&b[0]), whole(&b[1]), b[2].truthy())).collect(),
                "hand_size" => a.hand_size = whole(x),
                "focus" => {
                    a.focus = if x.is_null() {
                        None
                    } else if x.has("last") {
                        Some(Focus::Last(x["last"].s().to_string()))
                    } else {
                        Some(Focus::At(x["at"].s().to_string()))
                    }
                }
                "orbs" => a.orbs = x.list().iter().map(|o| (o[0].s().to_string(), whole(&o[1]), whole(&o[2]))).collect(),
                "orb_slots" => a.orb_slots = whole(x),
                "shiv_damage" => a.shiv_damage = whole(x),
                "tender" => a.tender = x.truthy(),
                "vigor" => a.vigor = whole(x),
                "giant" => a.giant = x.truthy(),
                "buffer" => a.buffer = whole(x),
                "loss_cap" => a.loss_cap = whole(x),
                "taint" => a.taint = whole(x),
                "draw_worth" => a.draw_worth = (x[0].float(), x[1].float(), x[2].float()),
                "bare_block" => a.bare_block = whole(x),
                "regen" => a.regen = whole(x),
                "focus_power" => a.focus_power = whole(x),
                "escape_odds" => a.escape_odds = two(x),
                "wither_in" => a.wither_in = whole(x),
                "wither_hp" => a.wither_hp = whole(x),
                "wither_bill" => a.wither_bill = x.float(),
                "trig" => {
                    let table = |t: &Value| t.entries().map(|(id, n)| (id.clone(), n.int())).collect::<IndexMap<String, i64>>();
                    a.trig = if x.is_null() {
                        None
                    } else {
                        Some(triggers::Triggers {
                            per_debuff: x["per_debuff"].int(),
                            per_card: x["per_card"].int(),
                            per_soul: x["per_soul"].int(),
                            card_block: x["card_block"].int(),
                            attack_block: x["attack_block"].int(),
                            costly_block: x["costly_block"].int(),
                            doom_block: x["doom_block"].int(),
                            strangle: table(&x["strangle"]),
                            oblivion: table(&x["oblivion"]),
                        })
                    }
                }
                "pet" => a.pet = whole(x),
                "hail" => a.hail = whole(x),
                "deck" => a.deck = (whole(&x[0]), whole(&x[1])),
                "mastery" => a.mastery = whole(x),
                "might0" => a.might0 = whole(x),
                "holds" => a.holds = x.truthy(),
                "strip" => a.strip = two(x),
                "friends" => a.friends = whole(x),
                "drained" => a.drained = whole(x),
                "free" => a.free = whole(x),
                "doubled" => a.doubled = x.truthy(),
                "fortify" => a.fortify = whole(x),
                "pace" => a.pace = x.float(),
                "room" => a.room = two(x),
                "arrows" => a.arrows = whole(x),
                "aiming" => a.aiming = whole(x),
                "stretch" => a.stretch = whole(x),
                "tune" => a.tune = x.list().iter().map(|m| m.s().to_string()).collect(),
                "chord" => a.chord = whole(x),
                "boom" => a.boom = whole(x),
                "fool" => a.fool = if x.is_null() { None } else { Some((whole(&x[0]), whole(&x[1]), whole(&x[2]), whole(&x[3]), whole(&x[4]))) },
                "stance0" => a.stance0 = x.s().to_string(),
                "wrath" => a.wrath = wrath(x),
                "sadness" => a.sadness = if x.is_null() { None } else { Some((whole(&x[0]), whole(&x[1]), whole(&x[2]), x[3].float(), x[4].float())) },
                "mantra0" => a.mantra0 = whole(x),
                other => panic!("no keyword {other}"),
            }
        }
        a
    }

    pub fn action(a: &Option<Action>) -> Value {
        match a {
            Some((name, params)) => json!([name, params]),
            None => Value::Null,
        }
    }

    pub fn actions_json(all: &[Action]) -> Value {
        all.iter().map(|(name, params)| json!([name, params])).collect()
    }

    pub const CALLS: &str = include_str!("../../tests/golden/combat/calls.json");
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn golden_choose_card() {
        let g = golden();
        let cases = rows(
            &[
                include_str!("../../tests/golden/combat/choose0.json"),
                include_str!("../../tests/golden/combat/choose1.json"),
                include_str!("../../tests/golden/combat/choose2.json"),
            ],
            "choose",
        );
        assert!(cases.len() >= 3000, "{} cases", cases.len());
        let (mut played, mut bad) = (0, Vec::new());
        for (n, rec) in cases.iter().enumerate() {
            let _env = g.env(&rec["env"]);
            let state = g.state(&rec["s"]);
            let mut plan = Plan::default();
            let got = _choose_card(&state, Some(&mut plan));
            played += got.is_some() as usize;
            let want = &rec["plan"];
            let line: Option<Vec<i64>> = want["line"].as_array().map(|l| l.iter().map(|i| i.int()).collect());
            let same = action(&got) == rec["r"]
                && plan.lost == want["lost"].as_i64()
                && plan.score == want["score"].as_f64()
                && plan.line.as_ref().map(|l| l.iter().map(|c| c.index).collect::<Vec<_>>()) == line
                && (!rec.has("ctx") || _context(&state) == args(&rec["ctx"]))
                && (!rec.has("describe") || json!(describe(&state)) == rec["describe"])
                && json!(expected_end_loss(&state)) == rec["end"]
                && json!(fortifier(&state["player"])) == rec["fort"]
                && _wrath_outlook(&state) == wrath(&rec["wrath"]);
            if !same {
                if bad.len() < 5 {
                    eprintln!(
                        "case {n} ({}): got {} lost {:?} score {:?} line {:?}\n  want {} plan {}\n  ctx {}\n  describe {}",
                        rec["env"],
                        action(&got),
                        plan.lost,
                        plan.score,
                        plan.line.as_ref().map(|l| l.iter().map(|c| c.index).collect::<Vec<_>>()),
                        rec["r"],
                        want,
                        !rec.has("ctx") || _context(&state) == args(&rec["ctx"]),
                        describe(&state),
                    );
                }
                bad.push(n);
            }
        }
        assert!(bad.is_empty(), "{} of {} cases differ: {:?}", bad.len(), cases.len(), &bad[..bad.len().min(40)]);
        assert!(played > 1500, "{played} cards played");
    }

    #[test]
    fn golden_simulate() {
        let g = golden();
        let cases = rows(&[include_str!("../../tests/golden/combat/sim0.json"), include_str!("../../tests/golden/combat/sim1.json")], "sim");
        assert!(cases.len() >= 3000, "{} cases", cases.len());
        let (mut none, mut bad) = (0, Vec::new());
        for (n, rec) in cases.iter().enumerate() {
            let _env = g.env(&rec["env"]);
            let line = g.line(&rec["line"]).unwrap();
            let foes: Vec<Enemy> = rec["foes"].list().iter().map(Enemy::from_json).collect();
            let before = foes.clone();
            let got = simulate(&line, &foes, args(&rec["kw"]));
            assert_eq!(foes, before);
            none += got.is_none() as usize;
            let shown = match &got {
                Some(out) => json!([out.score, out.first_target.as_deref(), out.lost]),
                None => Value::Null,
            };
            if shown != rec["r"] {
                if bad.len() < 5 {
                    eprintln!("case {n}: got {shown} want {}\n  {}", rec["r"], rec);
                }
                bad.push(n);
            }
        }
        assert!(bad.is_empty(), "{} of {} cases differ: {:?}", bad.len(), cases.len(), &bad[..bad.len().min(40)]);
        assert!(none > 100, "{none} lines that cannot be played");
    }

    #[test]
    fn golden_actions() {
        let g = golden();
        let cases = rows(&[CALLS], "actions");
        assert!(cases.len() >= 300, "{} cases", cases.len());
        let mut potions = 0;
        for (n, rec) in cases.iter().enumerate() {
            let _env = g.env(&rec["env"]);
            let got = actions(&g.state(&rec["s"]));
            potions += got.iter().filter(|a| a.0 == "use_potion").count();
            assert_eq!(actions_json(&got), rec["r"], "case {n}");
        }
        assert!(potions > 50, "{potions} potions drunk");
    }

    #[test]
    fn golden_small() {
        let g = golden();
        let cases = rows(&[CALLS], "small");
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (n, rec) in cases.iter().enumerate() {
            let _env = g.env(&rec["env"]);
            let (a, m) = (&rec["a"], &rec["m"]);
            *seen.entry(rec["f"].s().to_string()).or_default() += 1;
            match rec["f"].s() {
                "_orb_damage" => {
                    let mut foes: Vec<Enemy> = a[2].list().iter().map(Enemy::from_json).collect();
                    let got = _orb_damage(a[0].s(), a[1].int(), &mut foes, a[3].float(), a[4].float());
                    assert_eq!(json!(got), rec["r"], "case {n}: {rec}");
                    assert_eq!(foes, m[2].list().iter().map(Enemy::from_json).collect::<Vec<_>>(), "case {n}");
                }
                "_debuff" => {
                    let mut e = Enemy::from_json(&a[0]);
                    _debuff(&mut e, &g.cards[a[1].int() as usize], a[2].int());
                    assert_eq!(e, Enemy::from_json(&m[0]), "case {n}: {rec}");
                }
                "_expose" => {
                    let mut e = Enemy::from_json(&a[0]);
                    _expose(&mut e, a[1].int());
                    assert_eq!(e, Enemy::from_json(&m[0]), "case {n}: {rec}");
                }
                "_weaken" => {
                    let mut e = Enemy::from_json(&a[0]);
                    _weaken(&mut e, a[1].int());
                    assert_eq!(e, Enemy::from_json(&m[0]), "case {n}: {rec}");
                }
                other => panic!("no function {other}"),
            }
        }
        for f in ["_orb_damage", "_debuff", "_expose", "_weaken"] {
            assert!(seen.get(f).copied().unwrap_or(0) >= 100, "{f}: {:?}", seen.get(f));
        }
    }

    #[test]
    fn the_orders_of_itertools() {
        let mut picked = vec![0, 1];
        let mut all = vec![picked.clone()];
        while next_combination(&mut picked, 4) {
            all.push(picked.clone());
        }
        assert_eq!(all, [[0, 1], [0, 2], [0, 3], [1, 2], [1, 3], [2, 3]]);
        let mut order = vec![0, 1, 2];
        let mut all = vec![order.clone()];
        while next_permutation(&mut order) {
            all.push(order.clone());
        }
        assert_eq!(all, [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]]);
        let mut one = vec![0];
        assert!(!next_permutation(&mut one) && !next_combination(&mut one, 1));
    }

    #[test]
    fn the_hooks_and_the_constants() {
        let state = json!({"player": {"hp": 50, "max_hp": 80, "energy": 3, "hand": []}, "battle": {"round": 1, "enemies": []}});
        assert_eq!(next_room(&state), (99.0, 1.0));
        {
            let _room = with_next_room(|_| (0.0, 1.0));
            assert_eq!(_context(&state).room, (0.0, 1.0));
        }
        assert_eq!(_context(&state).room, (99.0, 1.0));
        let calls = Rc::new(std::cell::Cell::new(0));
        {
            let spy = calls.clone();
            let _hook = with_choose_card_hook(move |s, plan| {
                spy.set(spy.get() + 1);
                _choose_card(s, plan)
            });
            assert_eq!(actions(&state), vec![("end_turn".to_string(), json!({}))]);
        }
        assert_eq!((calls.get(), choose_card(&state, None)), (1, None));
        assert_eq!((MAX_SEARCH_CARDS.get(), HP_BASE.get(), HP_SLOPE.get(), HP_BITE.get(), HIVE_COST.get(), HIVE_PACE.get()), (11, 1.5, 2.5, 3.0, 9.0, 22.0));
        assert_eq!((DEBUFF_CAP.get(), FORM_WORTH.get(), ENRAGE_PRICE.get()), (4, 40.0, 2.0));
        assert!(TUNABLES.contains(&"HP_BASE") && !set_tunable("NO_SUCH", 1.0));
        let defaults = SimulateArgs::default();
        assert_eq!((defaults.fight.as_str(), defaults.round_no, defaults.pet, defaults.arrows, defaults.fortify, defaults.shiv_damage, defaults.wither_hp), ("monster", 1, -1, -1, 1, 4, 5));
        assert_eq!((defaults.draw_worth, defaults.escape_odds, defaults.room, defaults.deck, defaults.wither_bill), ((2.0, 0.0, 0.0), (0.3, 0.3), (99.0, 1.0), (20, 0), 7.0));
    }
}
