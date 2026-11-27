//! What a combat state says: the enemies, their intents and powers, and what our own hand will cost us (for combat.rs's planner).

use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::cards::{parse, CardInfo};
use crate::py::{self, JsonExt};
use crate::run::crowd::rating as crowd_rating;
use crate::Action;

crate::tunables! {
    pub LETHAL_PENALTY: f64 = 1_000_000.0;
    pub WIN_BONUS: f64 = 10_000.0;
    /// HP no enemy really has: the game's way of saying it cannot be hurt any more
    pub BEYOND_REACH: i64 = 100_000;
    /// Decimillipede: HP the other segments may have left for a kill to be the start of the end (they have two turns)
    pub REATTACH_REACH: i64 = 40;
    /// Crab Rage: Block a crab gains for each ally that dies (gone again when its next turn starts)
    pub RAGE_BLOCK: i64 = 99;
    /// ... and what the 6 Strength it keeps costs, in points for each HP it still has
    pub RAGE_COST: f64 = 0.3;
    /// what taking one Slippery stack off is worth, in damage: the hit it lets through later
    pub SLIP_WORTH: f64 = 7.0;
    /// energy next turn, in damage: while the next hand has cards to spend it on, and beyond that (next_room)
    pub NEXT_ENERGY: f64 = 6.0;
    pub SPARE_ENERGY: f64 = 1.0;
    // The Writhing Mass draws a new intent whenever an Attack hits it: a little over the mean, or its worst when that could be the end of us.
    pub REACTIVE_HIT: i64 = 20;
    pub REACTIVE_WORST: i64 = 32;
    pub REACTIVE_SAFE_HP: i64 = 40;
    /// Painful Stabs (the Test Subject's second form): what each Wound from a hit that gets through costs, in points
    pub STAB_SCAR: f64 = 8.0;
    /// a Power that takes an energy away every turn (Fasting), in POWER_VALUE's units, per energy and per copy already in play
    pub ENERGY_DRAIN: f64 = 20.0;
    /// damage a turn every enemy is taken to do at least, in that order (as combat.THREAT_FLOOR)
    pub FOCUS_FLOOR: f64 = 6.0;
    /// the round an Aeonglass fight is over by when it is won
    pub WITHER_FIGHT: i64 = 11;
    /// damage a turn every enemy is taken to do at least, when choosing whom to hit first
    pub THREAT_FLOOR: f64 = 6.0;
}

/// whose Debuff leaves us Ringing ("You can only play 1 card this turn") next turn
pub const RINGERS: [&str; 1] = ["CEREMONIAL_BEAST"];

/// What the Writhing Mass's turn comes to once an Attack has hit it: a new intent drawn in place of the one shown, `mean` on the
/// whole. Under REACTIVE_SAFE_HP (`worst` set) its worst, but only on a quiet turn: when the one shown is an attack anyway, a new
/// draw is no worse on the whole, and pricing every hit at the worst stopped the attacks for good (2026-11-26: 11 rounds on 21 HP,
/// the Mass 160 -> 120, then dead; 5 of 19 fights lost).
pub fn reacted(incoming: i64, mean: i64, worst: i64) -> i64 {
    if worst != 0 && incoming < mean {
        incoming.max(worst)
    } else {
        mean
    }
}

/// a new orb: (each turn, when evoked), before Focus
pub static ORB_VALUES: LazyLock<IndexMap<&'static str, (i64, i64)>> =
    LazyLock::new(|| IndexMap::from([("LIGHTNING", (3, 8)), ("FROST", (2, 5)), ("DARK", (6, 6)), ("PLASMA", (0, 0)), ("GLASS", (4, 8))]));

/// What playing each Power is worth in an ordinary fight, in points of damage (more for elites and bosses); unlisted ones: a guess from the cost.
#[rustfmt::skip]
pub static POWER_VALUE: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| {
    IndexMap::from([
        ("DEMON_FORM", 24.0), ("ROLLING_BOULDER", 18.0), ("PYRE", 16.0), ("CRIMSON_MANTLE", 12.0), ("MAYHEM", 9.0), ("STONE_ARMOR", 8.0), ("AGGRESSION", 8.0), ("UNMOVABLE", 7.0), ("FASTEN", 6.0),
        ("JUGGERNAUT", 6.0), ("HELLRAISER", 6.0), ("STAMPEDE", 5.0), ("PANACHE", 4.0), ("JUGGLING", 4.0), ("VICIOUS", 4.0), ("DARK_EMBRACE", 4.0), ("STRATAGEM", 4.0), ("BARRICADE", 4.0),
        ("FEEL_NO_PAIN", 3.0), ("RUPTURE", 3.0), ("SERPENT_FORM", 16.0), ("NOXIOUS_FUMES", 14.0), ("AFTERIMAGE", 10.0), ("CREATIVE_AI", 12.0), ("ABRASIVE", 9.0), ("HAILSTORM", 9.0),
        ("INFINITE_BLADES", 8.0), ("ENVENOM", 8.0), ("FAN_OF_KNIVES", 8.0), ("SEEKING_EDGE", 8.0), ("CAPACITOR", 7.0), ("LETHALITY", 7.0), ("ACCURACY", 6.0), ("TOOLS_OF_THE_TRADE", 6.0),
        ("SPEEDSTER", 6.0), ("PARRY", 6.0), ("CHILD_OF_THE_STARS", 6.0), ("CALL_OF_THE_VOID", 6.0), ("TYRANNY", 6.0), ("SPECTRUM_SHIFT", 6.0), ("WELL_LAID_PLANS", 5.0), ("TRACKING", 5.0),
        ("SHROUD", 5.0), ("HAUNT", 5.0), ("ORBIT", 5.0), ("STORM", 5.0), ("THUNDER", 5.0), ("ACCELERANT", 4.0), ("PILLAR_OF_CREATION", 4.0), ("PALE_BLUE_DOT", 4.0), ("FERAL", 4.0),
        ("ITERATION", 3.0), ("TRASH_TO_TREASURE", 3.0), ("ROYALTIES", 2.0), ("ECHO_FORM", 22.0), ("FRIENDSHIP", 12.0), ("MACHINE_LEARNING", 10.0), ("DEFRAGMENT", 10.0), ("GENESIS", 9.0),
        ("DEMESNE", 20.0), ("FURNACE", 10.0), ("COUNTDOWN", 8.0), ("REAPER_FORM", 20.0), ("VOID_FORM", 16.0), ("SWORD_SAGE", 10.0), ("ARSENAL", 8.0), ("CRUELTY", 8.0), ("MONARCHS_GAZE", 8.0),
        ("AUTOMATION", 7.0), ("DEVOUR_LIFE", 6.0), ("SPIRIT_OF_ASH", 6.0), ("COOLANT", 8.0), ("SPINNER", 8.0), ("BUFFER", 8.0), ("SENTRY_MODE", 8.0), ("NECRO_MASTERY", 8.0), ("LOOP", 7.0),
        ("BLACK_HOLE", 7.0), ("SLEIGHT_OF_FLESH", 6.0), ("NEUROSURGE", 6.0), ("DANSE_MACABRE", 4.0), ("SUBROUTINE", 3.0), ("PAGESTORM", 2.0),
    ])
});

/// A field of `Enemy` as JSON and back (`Enemy::to_json`, `Enemy::from_json`).
pub trait EnemyField: Sized {
    fn put(&self) -> Value;
    fn take(v: &Value) -> Self;
}

impl EnemyField for i64 {
    fn put(&self) -> Value {
        json!(self)
    }
    fn take(v: &Value) -> Self {
        v.int()
    }
}

impl EnemyField for f64 {
    fn put(&self) -> Value {
        json!(self)
    }
    fn take(v: &Value) -> Self {
        v.float()
    }
}

impl EnemyField for bool {
    fn put(&self) -> Value {
        json!(self)
    }
    fn take(v: &Value) -> Self {
        v.truthy()
    }
}

impl EnemyField for Arc<str> {
    fn put(&self) -> Value {
        json!(&**self)
    }
    fn take(v: &Value) -> Self {
        Arc::from(v.s())
    }
}

macro_rules! enemy {
    ($($(#[$meta:meta])* $name:ident : $ty:ty = $default:expr;)*) => {
        /// An enemy as a plan sees it. A plain value: a plan copies the enemies thousands of times (`clone`); the id is shared, not copied.
        #[derive(Clone, Debug, PartialEq)]
        pub struct Enemy {
            $($(#[$meta])* pub $name: $ty,)*
        }

        impl Default for Enemy {
            /// The defaults of the Python dataclass (its six fields without one: "", 0, 0, false, false, 0).
            fn default() -> Self {
                Enemy { $($name: $default,)* }
            }
        }

        impl Enemy {
            /// Every field by name, in the order above, as `dataclasses.asdict` gives them.
            pub fn to_json(&self) -> Value {
                let mut out = Map::new();
                $(out.insert(stringify!($name).into(), EnemyField::put(&self.$name));)*
                Value::Object(out)
            }

            /// An enemy from the fields `v` names, the rest as `Enemy::default()` has them.
            pub fn from_json(v: &Value) -> Enemy {
                let mut e = Enemy::default();
                $(if let Some(x) = v.get(stringify!($name)) {
                    e.$name = EnemyField::take(x);
                })*
                e
            }
        }
    };
}

enemy! {
    entity_id: Arc<str> = Arc::from("");
    hp: i64 = 0;
    block: i64 = 0;
    vulnerable: bool = false;
    weak: bool = false;
    /// damage this enemy intends to deal this turn
    incoming: i64 = 0;
    /// the next N hits only remove 1 HP each
    slippery: i64 = 0;
    /// e.g. Flutter: 50% less damage from attacks
    damage_mult: f64 = 1.0;
    /// damage we take back per hit
    thorns: i64 = 0;
    /// Illusion: comes back next turn at full HP
    revives: bool = false;
    /// Plow, Shriek (the Terror Eel): Stunned once HP drops to this, and its move this turn is gone
    stun_at: i64 = 0;
    /// Slow: % extra attack damage taken so far this turn, +10 per card we play (-1: none)
    slow: i64 = -1;
    /// leaves the fight when its leader dies
    minion: bool = false;
    /// how many separate hits make up `incoming`
    incoming_hits: i64 = 0;
    /// Burrowed: its Block stays until we break it, which Stuns it
    keeps_block: bool = false;
    /// Block our attacks have removed so far in this plan
    chipped: i64 = 0;
    /// ... and Slippery stacks our hits have taken off
    stripped: i64 = 0;
    /// the next N debuffs we apply do nothing
    artifact: i64 = 0;
    /// Hard to Kill: no single hit removes more HP than this
    hit_cap: i64 = 0;
    /// Strength it gains whenever we play a Skill
    enrage: i64 = 0;
    /// ... a Power (Curiosity: the first game's Awakened One)
    curious: i64 = 0;
    /// turns of Vulnerable / Weak it has left
    vuln_turns: i64 = 0;
    weak_turns: i64 = 0;
    /// ... and how many of those this plan added
    vuln_added: i64 = 0;
    weak_added: i64 = 0;
    /// Poison on it: loses that much HP at the start of its turn, before it acts
    poison: i64 = 0;
    /// Doom on it: dies at the end of its turn once Doom covers its HP
    doom: i64 = 0;
    /// Back Attack: hits 50% harder from behind us
    flanker: bool = false;
    /// ... and is behind us right now (its intent already shows the bonus)
    behind: bool = false;
    /// what Vulnerable multiplies our attacks by (Debilitate doubles its effect)
    vuln_mult: f64 = 1.5;
    /// what Weak multiplies its attacks by
    weak_cut: f64 = 0.75;
    /// Tracking: our attacks hit a Weak enemy this much harder
    weak_mult: f64 = 1.0;
    /// Crab Rage: gains 6 Strength and 99 Block when an ally dies
    rage: bool = false;
    /// extra cost of each of its hits that gets through (Paper Cuts: Max HP; Painful Stabs: a Wound)
    scar: f64 = 0.0;
    /// a minion that is doing the real damage and is quicker to kill than its leader
    main_threat: bool = false;
    /// Stock: when killed it is summoned again in its place, at full HP, this many more times
    respawns: i64 = 0;
    /// its full HP (what a Stock summon comes back with)
    max_hp: i64 = 0;
    /// already beaten and out of reach: only its last blow is still to come
    spent: bool = false;
    /// Adaptable (the Test Subject): killed, it is gone for the turn and back, stronger, the next
    adapts: bool = false;
    /// ... and this plan has done that
    adapted: bool = false;
    /// Asleep: turns until it wakes by itself (losing HP wakes it at once)
    asleep: i64 = 0;
    /// Imbalanced (Bowlbug Rock): Stunned next turn if its attack is blocked whole
    imbalanced: bool = false;
    /// Reactive (the first game's Writhing Mass): an Attack that hits it draws a new intent; what to expect of that
    reacts: i64 = 0;
    /// ... and its worst, when that could be the end of us (`reacted`)
    reacts_worst: i64 = 0;
    /// Skittish: Block it gains the first time it is hit this turn (0 once that has happened)
    skittish: i64 = 0;
    /// Hang: what the Hang cards' damage on it is multiplied by (2, 4, 8 ...; 0: none played yet)
    hang: i64 = 0;
    /// damage a turn it has been doing in this fight (ordinary fights only; 0: not known)
    threat: f64 = 0.0;
    /// Strength it gains a turn (Ritual, Territorial, or what its Strength so far comes to)
    grows: f64 = 0.0;
    /// Reattach (a Decimillipede segment): killed while another segment lives, it is back two turns later on this much HP
    reattach: i64 = 0;
    /// Personal Hive (the Entomancer): Dazed put into our draw pile for every Attack that hits it
    hive: i64 = 0;
    /// Flutter (the Thieving Hopper): Attack hits still to land before it is Stunned and takes full damage again
    flutter: i64 = 0;
    /// Swipe: it holds a card of ours, which comes back if it dies and is gone for good if it gets away
    thief: bool = false;
    /// Hardened Shell (the Skulking Colony): HP it can still lose this turn (-1: no limit)
    shell: i64 = -1;
    /// what a point of damage on it is worth next to one on the enemy to kill first (KNOWN_THREAT fights only)
    focus: f64 = 1.0;
    /// Impaled: our Arrow stands in it until this many more hits land (or it dies); 0: none
    impaled: i64 = 0;
    /// Beat of Death (the first game's Heart): damage we take for every card we play, once it has resolved (Block takes it)
    beat: i64 = 0;
    /// Malleable (the first game's Snake Plant, Writhing Mass): Block after each hit that hurts it, 1 more each time this turn
    malleable: i64 = 0;
    /// ... that Block, still to come: it lands once the card being played has done its hits
    soft: i64 = 0;
}

impl Enemy {
    /// How much damaging this enemy brings the fight closer to its end.
    #[inline]
    pub fn worth(&self) -> f64 {
        if self.revives || self.spent {
            return 0.0;
        }
        if self.minion {
            return if self.main_threat { 1.0 } else { 0.4 };
        }
        if self.thief {
            1.3
        } else {
            1.0
        }
    }

    #[inline]
    pub fn alive(&self) -> bool {
        self.hp > 0
    }
}

/// `dataclasses.replace` for an Enemy at a third of the cost: a plan copies the enemies thousands of times.
/// (Changes go in with the struct update syntax: `Enemy { chipped: 0, ..clone(e) }`.)
#[inline]
pub fn clone(e: &Enemy) -> Enemy {
    e.clone()
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

/// `str(d.get(key, ""))`.
fn _str_of(d: &Value, key: &str) -> String {
    match d.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(x) => py::str_of(x),
        None => String::new(),
    }
}

/// `int(x or default)`.
fn _int_or(v: &Value, default: i64) -> i64 {
    if v.truthy() {
        v.int()
    } else {
        default
    }
}

/// `re.sub(r"_\d+$", "", entity_id)`: the enemy's kind, without its place in the row.
fn _kind(entity_id: &str) -> String {
    crate::re!(r"_\d+(\n?)\z").replace(entity_id, "${1}").into_owned()
}

/// Intents that hit us (the Waterfall Giant's last move, a "DeathBlow", too).
pub fn _is_attack(intent: &Value) -> bool {
    let kind = _str_of(intent, "type").to_lowercase();
    kind.contains("attack") || kind.contains("deathblow")
}

pub fn intent_damage(intent: &Value) -> i64 {
    if !_is_attack(intent) {
        return 0;
    }
    for field in ["label", "description"] {
        let text = intent[field].text();
        if let Some(m) = crate::re!(r"(\d+)\s*[xX×*]\s*(\d+)").captures(&text) {
            return m[1].parse::<i64>().unwrap_or(0) * m[2].parse::<i64>().unwrap_or(0);
        }
        if let Some(m) = crate::re!(r"(\d+)").captures(&text) {
            return m[1].parse::<i64>().unwrap_or(0);
        }
    }
    0
}

pub fn intent_hits(intent: &Value) -> i64 {
    if !_is_attack(intent) {
        return 0;
    }
    for field in ["label", "description"] {
        if let Some(m) = crate::re!(r"\d+\s*[xX×*]\s*(\d+)").captures(&intent[field].text()) {
            return m[1].parse::<i64>().unwrap_or(0);
        }
    }
    if intent_damage(intent) != 0 {
        1
    } else {
        0
    }
}

/// Whether a power's id, as the game names it (VULNERABLE_POWER, or Vulnerable in any case), is `power_id`.
fn _names(id: &Value, power_id: &str) -> bool {
    match id {
        Value::String(s) if s.is_ascii() => {
            let (s, p) = (s.as_bytes(), power_id.as_bytes());
            let whole = s.len() >= 6 && s[s.len() - 6..].eq_ignore_ascii_case(b"_POWER");
            if whole {
                s[..s.len() - 6].eq_ignore_ascii_case(p)
            } else {
                s.eq_ignore_ascii_case(p)
            }
        }
        other => {
            let upper = py::str_of(other).to_uppercase();
            upper.strip_suffix("_POWER").unwrap_or(&upper) == power_id
        }
    }
}

/// The number the game shows on a power, named like VULNERABLE_POWER (`blank` if it shows none; None if there is no such power:
/// the Python's `missing`, which callers give with `.unwrap_or(missing)`).
pub fn _shown(creature: &Value, power_id: &str, blank: i64) -> Option<i64> {
    for p in creature["status"].list() {
        let named = match p.get("id") {
            Some(id) => _names(id, power_id),
            None => power_id.is_empty(),
        };
        if named {
            return Some(match &p["amount"] {
                Value::Number(n) => n.as_i64().unwrap_or_else(|| n.as_f64().map(py::trunc).unwrap_or(blank)),
                Value::Bool(b) => *b as i64,
                _ => blank,
            });
        }
    }
    None
}

/// Stacks of a power on a creature (1 for a power that shows no number).
pub fn power_amount(creature: &Value, power_id: &str) -> i64 {
    match _shown(creature, power_id, 1) {
        None => 0,
        Some(0) => 1,
        Some(n) => n,
    }
}

pub fn _has_power(creature: &Value, power_id: &str) -> bool {
    power_amount(creature, power_id) != 0
}

pub fn _stacks(creature: &Value, power_id: &str) -> i64 {
    power_amount(creature, power_id).max(0)
}

/// Block the player gains by itself when the turn ends (Plating).
pub fn end_of_turn_block(player: &Value) -> i64 {
    _stacks(player, "PLATING")
}

// (\s*: the mod turns line breaks into spaces, sometimes two.)
pub fn _WITHER() -> &'static regex::Regex {
    crate::re!(r"^Wither(?:\+(\d+))?\n?\z")
}

pub fn _HAND_BURN() -> &'static regex::Regex {
    crate::re!(r"(?i)at the end of your turn,\s*if this is in your hand,\s*(?:lose|take)\s+(\d+)\s+(?:hp|damage)")
}

/// Damage we take when our turn ends, before the enemy moves (Disintegration). It eats into Block first.
pub fn end_of_turn_loss(player: &Value) -> i64 {
    _stacks(player, "DISINTEGRATION")
}

/// The level a Wither's name shows ("Wither+2": 2; "Wither": 0), None for any other card.
fn _wither_level(name: &str) -> Option<i64> {
    _WITHER().captures(name).map(|w| w.get(1).map(|g| g.as_str().parse::<i64>().unwrap_or(0)).unwrap_or(0))
}

/// Cards that hurt if still in the hand when the turn ends: (hand index, HP, whether Block stops it ("take N damage", not "lose N HP")).
/// An amount of -1 stands for Regret: 1 HP for every card left in the hand.
pub fn hand_burns(player: &Value) -> Vec<(i64, i64, bool)> {
    let mut out = Vec::new();
    for c in player["hand"].list() {
        let text = c["description"].text();
        let m = _HAND_BURN().captures(&text);
        if crate::re!(r"(?i)if this is in your hand,\s*lose 1 hp for each card in your hand").is_match(&text) {
            // (Regret first: its text begins like the others)
            out.push((c["index"].int_or(-1), -1, false));
        } else if let Some(m) = m {
            let mut amount = m[1].parse::<i64>().unwrap_or(0);
            if let Some(level) = _wither_level(&c["name"].text()) {
                // whatever the text says, a Wither has cost 3 for each level
                amount = amount.max(3 * (level + 1));
            }
            out.push((c["index"].int_or(-1), amount, m[0].to_lowercase().contains("damage")));
        }
    }
    out
}

/// HP of the Necrobinder's pet, who takes attacks before she does.
pub fn osty_hp(player: &Value) -> i64 {
    player["pets"].list().iter().filter(|p| p.get("alive").map(|a| a.truthy()).unwrap_or(true)).map(|p| p["hp"].int()).sum()
}

/// Damage a turn some enemies do over a whole fight, where their first intents say much less: it orders whom to hit first (`Enemy.threat`).
/// (The table as written; the one in force, which tests and scripts may replace for a while: `known_threat()`.)
pub static KNOWN_THREAT: LazyLock<IndexMap<&'static str, f64>> =
    LazyLock::new(|| IndexMap::from([("MAGI_KNIGHT", 22.0), ("FLAIL_KNIGHT", 16.0), ("SPECTRAL_KNIGHT", 12.0), ("ROCKET", 20.0), ("CRUSHER", 13.0), ("BOWLBUG_NECTAR", 19.0)]));
/// Minions to kill before their leader although his death takes them along (most of the damage, in hits too small for read_enemies' rule).
/// (The table as written; the one in force: `main_minions()`.)
pub const MAIN_MINIONS: [&str; 1] = ["KIN_FOLLOWER"];

thread_local! {
    static _KNOWN_THREAT: RefCell<Option<IndexMap<String, f64>>> = const { RefCell::new(None) };
    static _MAIN_MINIONS: RefCell<Option<BTreeSet<String>>> = const { RefCell::new(None) };
}

/// KNOWN_THREAT as it is in force on this thread.
pub fn known_threat() -> IndexMap<String, f64> {
    _KNOWN_THREAT.with(|t| match &*t.borrow() {
        Some(table) => table.clone(),
        None => KNOWN_THREAT.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
    })
}

/// `KNOWN_THREAT.get(kind)`, of the table in force.
pub fn known_threat_of(kind: &str) -> Option<f64> {
    _KNOWN_THREAT.with(|t| match &*t.borrow() {
        Some(table) => table.get(kind).copied(),
        None => KNOWN_THREAT.get(kind).copied(),
    })
}

/// Another KNOWN_THREAT on this thread from now on (None: the table as written).
pub fn set_known_threat(table: Option<IndexMap<String, f64>>) -> Option<IndexMap<String, f64>> {
    _KNOWN_THREAT.with(|t| t.replace(table))
}

/// Puts the earlier KNOWN_THREAT back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct KnownThreatGuard(Option<Option<IndexMap<String, f64>>>);

impl Drop for KnownThreatGuard {
    fn drop(&mut self) {
        set_known_threat(self.0.take().unwrap_or(None));
    }
}

/// Another KNOWN_THREAT on this thread until the guard is dropped (`mock.patch.object(battle, "KNOWN_THREAT", table)`).
pub fn with_known_threat(table: IndexMap<String, f64>) -> KnownThreatGuard {
    KnownThreatGuard(Some(set_known_threat(Some(table))))
}

/// MAIN_MINIONS as it is in force on this thread.
pub fn main_minions() -> BTreeSet<String> {
    _MAIN_MINIONS.with(|t| match &*t.borrow() {
        Some(table) => table.clone(),
        None => MAIN_MINIONS.iter().map(|k| k.to_string()).collect(),
    })
}

/// `kind in MAIN_MINIONS`, of the table in force.
pub fn is_main_minion(kind: &str) -> bool {
    _MAIN_MINIONS.with(|t| match &*t.borrow() {
        Some(table) => table.contains(kind),
        None => MAIN_MINIONS.contains(&kind),
    })
}

/// Another MAIN_MINIONS on this thread from now on (None: the table as written).
pub fn set_main_minions(table: Option<BTreeSet<String>>) -> Option<BTreeSet<String>> {
    _MAIN_MINIONS.with(|t| t.replace(table))
}

/// Puts the earlier MAIN_MINIONS back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct MainMinionsGuard(Option<Option<BTreeSet<String>>>);

impl Drop for MainMinionsGuard {
    fn drop(&mut self) {
        set_main_minions(self.0.take().unwrap_or(None));
    }
}

/// Another MAIN_MINIONS on this thread until the guard is dropped (`mock.patch.object(battle, "MAIN_MINIONS", set())`).
pub fn with_main_minions(table: &[&str]) -> MainMinionsGuard {
    MainMinionsGuard(Some(set_main_minions(Some(table.iter().map(|k| k.to_string()).collect()))))
}

/// In a fight with a KNOWN_THREAT enemy, what damage on each is worth: by kill order (threat over health left), a point on the
/// first brings all the deaths after it sooner and is worth the whole threat, one on the last only its own.
pub fn _set_focus(enemies: &mut [Enemy]) {
    let mut order: Vec<usize> = (0..enemies.len()).filter(|&i| enemies[i].alive() && !enemies[i].minion && !enemies[i].spent).collect();
    if order.len() < 2 || !order.iter().any(|&i| enemies[i].threat != 0.0 && known_threat_of(&_kind(&enemies[i].entity_id)).is_some()) {
        return;
    }
    let floor = FOCUS_FLOOR.get();
    let left = |e: &Enemy| ((1.max(e.hp - e.doom) + e.block) as f64) / (if e.vulnerable { 1.5 } else { 1.0 });
    py::sort_by_key(&mut order, |&i| -fmax(enemies[i].threat, floor) / left(&enemies[i]));
    let total = py::fsum(order.iter().map(|&i| fmax(enemies[i].threat, floor)));
    for n in 0..order.len() {
        let rest = py::fsum(order[n..].iter().map(|&o| fmax(enemies[o].threat, floor)));
        enemies[order[n]].focus = rest / total;
    }
}

/// A Reactive enemy's new intent as the shown ones are: its Strength (3 hits' worth), our Vulnerable, its Weak, Wrath.
pub fn _reroll(e: &Value, player: &Value, base: i64) -> i64 {
    if !_has_power(e, "REACTIVE") {
        return 0;
    }
    let hit = ((base + 3 * _stacks(e, "STRENGTH")) as f64) * (if _has_power(player, "VULNERABLE") { 1.5 } else { 1.0 }) * (if _has_power(e, "WEAK") { 0.75 } else { 1.0 });
    py::round(hit * (if player["stance"].text().to_uppercase() == "WRATH" { 2.0 } else { 1.0 }))
}

pub fn read_enemies(state: &Value) -> Vec<Enemy> {
    let mut out: Vec<Enemy> = Vec::new();
    let player = &state["player"];
    let listed = state["battle"]["enemies"].list();
    let ids: Vec<String> = listed.iter().map(|e| py::str_of(&e["entity_id"])).collect();
    let mut raw: HashMap<&str, &Value> = HashMap::new();
    for (id, e) in ids.iter().zip(listed) {
        raw.insert(id.as_str(), e);
    }
    let seen: &Value = if state["state_type"].as_str() == Some("monster") { &state["_threat"] } else { &Value::Null };
    let rounds = 1.max(_int_or(&state["battle"]["round"], 1) - 1);
    for (id, e) in ids.iter().zip(listed) {
        let hp = e["hp"].int();
        if hp <= 0 {
            continue;
        }
        // Damage a turn so far; one that attacks and Buffs itself in the same move is growing: what it does now and half again.
        let intents = e["intents"].list();
        let kinds: Vec<String> = intents.iter().map(|i| _str_of(i, "type").to_lowercase()).collect();
        let grows = kinds.iter().any(|k| k.contains("buff") && !k.contains("debuff")) && intents.iter().any(_is_attack);
        let mut threat = match seen.get(id.as_str()) {
            Some(x) => x.float(),
            None => 0.0,
        };
        if grows && seen.truthy() {
            threat = 1.5 * fmax(threat, intents.iter().map(intent_damage).sum::<i64>() as f64);
        }
        threat = fmax(threat, known_threat_of(&_kind(id)).unwrap_or(0.0));
        let spent = hp >= BEYOND_REACH.get(); // (the Waterfall Giant, once killed, stands on 999,999,999 HP for the turn it takes to blow up)
        let or = |a: i64, b: &dyn Fn() -> i64| if a != 0 { a } else { b() };
        out.push(Enemy {
            entity_id: Arc::from(id.as_str()),
            hp,
            spent,
            block: e["block"].int(),
            vulnerable: _has_power(e, "VULNERABLE"),
            weak: _has_power(e, "WEAK"),
            vuln_turns: _stacks(e, "VULNERABLE"),
            weak_turns: _stacks(e, "WEAK"),
            slippery: _stacks(e, "SLIPPERY"),
            damage_mult: if spent {
                0.0
            } else if _has_power(e, "FLUTTER") || _has_power(e, "SOAR") {
                0.5
            } else {
                1.0
            },
            hit_cap: if _has_power(e, "INTANGIBLE") { 1 } else { _stacks(e, "HARD_TO_KILL") }, // (Intangible: every hit takes 1 HP)
            enrage: _stacks(e, "ENRAGE"),
            curious: _stacks(e, "CURIOSITY"),
            thorns: _stacks(e, "THORNS"),
            revives: _has_power(e, "ILLUSION"),
            stun_at: or(_stacks(e, "PLOW"), &|| _stacks(e, "SHRIEK")),
            shell: _shown(e, "HARDENED_SHELL", 0).unwrap_or(-1),
            slow: _shown(e, "SLOW", 0).unwrap_or(-1),
            minion: _has_power(e, "MINION"),
            keeps_block: _has_power(e, "BURROWED") || _has_power(e, "BARRICADE"),
            artifact: _stacks(e, "ARTIFACT"),
            poison: _stacks(e, "POISON"),
            doom: _stacks(e, "DOOM"),
            incoming: intents.iter().map(intent_damage).sum(),
            incoming_hits: intents.iter().map(intent_hits).sum(),
            vuln_mult: if _has_power(e, "DEBILITATE") { 2.0 } else { 1.5 },
            weak_cut: if _has_power(e, "DEBILITATE") { 0.5 } else { 0.75 },
            weak_mult: if _has_power(player, "TRACKING") { 1.5 } else { 1.0 },
            scar: 8.0 * (_has_power(e, "PAPER_CUTS") as i64 as f64) + STAB_SCAR.get() * (_has_power(e, "PAINFUL_STABS") as i64 as f64),
            rage: _has_power(e, "CRAB_RAGE"),
            respawns: _stacks(e, "STOCK"),
            adapts: _has_power(e, "ADAPTABLE"),
            asleep: _stacks(e, "ASLEEP"),
            imbalanced: _has_power(e, "IMBALANCED"),
            reacts: _reroll(e, player, REACTIVE_HIT.get()),
            reacts_worst: if player["hp"].int() < REACTIVE_SAFE_HP.get() { _reroll(e, player, REACTIVE_WORST.get()) } else { 0 },
            hang: _stacks(e, "HANG"),
            threat,
            hive: _stacks(e, "PERSONAL_HIVE"),
            flutter: _stacks(e, "FLUTTER"),
            thief: _has_power(e, "SWIPE"),
            reattach: _stacks(e, "REATTACH"),
            grows: fmax((_stacks(e, "RITUAL") + _stacks(e, "TERRITORIAL")) as f64, _stacks(e, "STRENGTH") as f64 / rounds as f64),
            skittish: if e["block"].int() != 0 { 0 } else { or(_stacks(e, "SKITTISH"), &|| _stacks(e, "CURL_UP")) },
            max_hp: e["max_hp"].int(),
            impaled: _stacks(e, "IMPALED"),
            beat: _stacks(e, "BEAT_OF_DEATH"),
            malleable: _stacks(e, "MALLEABLE"),
            ..Enemy::default()
        });
    }
    // Surrounded: we face whoever we targeted last, and the one behind us hits 50% harder. The fight starts with us facing right.
    if _has_power(player, "SURROUNDED") {
        let facing = &state["_facing"];
        for e in out.iter_mut() {
            let was = raw[&*e.entity_id];
            let left = _has_power(was, "BACK_ATTACK_LEFT");
            e.flanker = left || _has_power(was, "BACK_ATTACK_RIGHT");
            if e.flanker {
                // (the first game's mod says who is behind: CommunicationMod's play turns us as a click would, see speedmod.rs)
                e.behind = if was.has("behind") {
                    was["behind"].truthy()
                } else {
                    match facing.as_str() {
                        Some(f) if raw.contains_key(f) => &*e.entity_id != f,
                        _ => left,
                    }
                };
            }
        }
    }
    // Minions leave when their leader dies, so he is normally the target; not when a minion with far less HP deals all the damage.
    _set_focus(&mut out);
    if out.iter().any(|e| !e.minion) {
        let leader_hp: i64 = out.iter().filter(|e| !e.minion).map(|e| e.hp + e.block).sum();
        let leader_hits: i64 = out.iter().filter(|e| !e.minion).map(|e| e.incoming).sum();
        for e in out.iter_mut() {
            let hard = e.incoming as f64 / (if e.weak { e.weak_cut } else { 1.0 }) >= 15.0; // (its hit as it would be without our Weak)
            if e.minion && hard && e.incoming > 2 * leader_hits && (e.hp + e.block) as f64 <= 0.6 * leader_hp as f64 {
                e.main_threat = true;
            } else if e.minion && is_main_minion(&_kind(&e.entity_id)) {
                e.main_threat = true; // (worth what the leader is: the health order then takes it first while he is far from dead)
            }
        }
    }
    out
}

#[inline]
pub fn _hit_count(e: &Enemy, hits: i64, double_if_vulnerable: bool) -> i64 {
    hits * (if double_if_vulnerable && e.vulnerable { 2 } else { 1 })
}

/// What one hit takes off this enemy's Block and HP (see `_hit`).
#[inline]
pub fn _per_hit(e: &Enemy, dmg_per_hit: i64, raw: bool) -> i64 {
    let mut per = dmg_per_hit as f64 * (if !(raw && e.damage_mult != 0.0) { e.damage_mult } else { 1.0 }); // (Flutter and Soar: "less damage from Attacks")
    if !raw {
        per *= (if e.vulnerable { e.vuln_mult } else { 1.0 }) * (if e.weak { e.weak_mult } else { 1.0 });
        if e.slow > 0 {
            per *= 1.0 + e.slow as f64 / 100.0;
        }
    }
    py::trunc(per)
}

/// Whether this attack leaves the enemy dead, without touching it (worked out directly in the plain case: it is asked very often).
pub fn _would_kill(e: &Enemy, dmg_per_hit: i64, hits: i64, double_if_vulnerable: bool) -> bool {
    if e.skittish != 0 || e.hit_cap != 0 || e.slippery != 0 || e.shell >= 0 || e.flutter != 0 {
        let mut trial = clone(e);
        _hit(&mut trial, dmg_per_hit, hits, double_if_vulnerable, false);
        return !trial.alive();
    }
    e.hp <= 0 || _per_hit(e, dmg_per_hit, false) * _hit_count(e, hits, double_if_vulnerable) >= e.hp + e.block
}

/// How far an attack moved the fight on: HP removed, plus Block worn down (Block an enemy keeps between turns is as good as HP).
pub fn _progress(e: &mut Enemy, hp_removed: i64, chip_rate: f64) -> f64 {
    let chip = e.chipped;
    e.chipped = 0;
    // A Slippery stack taken off is a later hit that lands in full.
    let strip = e.stripped;
    e.stripped = 0;
    let hp_removed = hp_removed as f64 + strip as f64 * fmin(SLIP_WORTH.get(), e.hp as f64 / 3.0);
    hp_removed + chip as f64 * (if e.keeps_block || (e.incoming == 0 && chip != 0 && e.block == 0 && chip_rate >= 1.0) { 1.0 } else { chip_rate })
}

/// Apply an attack hit by hit, return HP actually removed. `raw` is damage that is not an Attack (an orb's): Vulnerable does nothing for it.
pub fn _hit(e: &mut Enemy, dmg_per_hit: i64, hits: i64, double_if_vulnerable: bool, raw: bool) -> i64 {
    let mut per = _per_hit(e, dmg_per_hit, raw);
    let mut removed = 0;
    for _ in 0.._hit_count(e, hits, double_if_vulnerable) {
        if e.hp <= 0 {
            break;
        }
        if e.impaled != 0 && !raw {
            e.impaled -= 1; // (Block or not)
        }
        let absorbed = e.block.min(per);
        e.block -= absorbed;
        e.chipped += absorbed;
        if absorbed != 0 && e.block == 0 && e.keeps_block {
            e.keeps_block = false;
            e.incoming = 0; // "Stunned if all Block is removed."
        }
        // Skittish: the first hit of a turn lands, the rest meet its Block
        e.block += e.skittish;
        e.skittish = 0;
        let mut through = per - absorbed;
        if through <= 0 {
            continue;
        }
        if e.hit_cap != 0 {
            through = through.min(e.hit_cap);
        }
        if e.slippery > 0 {
            e.slippery -= 1;
            e.stripped += 1;
            through = 1;
        }
        let mut lost = e.hp.min(through);
        if e.shell >= 0 {
            // "Cannot lose more than 20 HP each turn", counted down as it goes
            lost = lost.min(e.shell);
            e.shell -= lost;
        }
        e.hp -= lost;
        removed += lost;
        if e.malleable != 0 && lost != 0 && e.hp > 0 && !raw {
            // (an Attack's hit that hurts without killing; orbs and Poison do not count)
            e.soft += e.malleable;
            e.malleable += 1;
        }
        if e.stun_at != 0 && e.hp <= e.stun_at {
            e.stun_at = 0;
            e.incoming = 0; // Stunned: its attack this turn never happens
        }
        if e.flutter != 0 && !raw {
            // Flutter ("Deal attack damage 5 times to Stun it"): the fifth hit takes away this turn's move, and the 50% off our Attacks.
            e.flutter -= 1;
            if e.flutter == 0 {
                e.incoming = 0;
                e.incoming_hits = 0;
                e.damage_mult = 1.0;
                per *= 2;
            }
        }
    }
    removed
}

/// HP still in the way of the kill that would save a turn lost on paper (infinite: none would): every leader, or every attacker
/// when the attacks are all that kills us. Poison bites before they move.
pub fn kill_reach<E: Borrow<Enemy>>(survivors: &[E], blow_is_all: bool) -> f64 {
    let left = |pick: &dyn Fn(&Enemy) -> bool| -> f64 {
        let mut group = survivors.iter().map(|e| e.borrow()).filter(|e| pick(e)).peekable();
        if group.peek().is_none() {
            return f64::INFINITY;
        }
        let mut total = 0;
        for e in group {
            if e.spent || e.hit_cap != 0 || e.slippery != 0 || e.respawns != 0 {
                return f64::INFINITY;
            }
            total += 0.max(e.hp - e.poison) + e.block;
        }
        total as f64
    };
    let leaders = left(&|e| !e.minion);
    let attackers = if blow_is_all { left(&|e| e.incoming > 0) } else { f64::INFINITY };
    fmin(leaders, attackers)
}

/// Damage we take at the end of our turn while the Strangler lives.
pub fn constrict(player: &Value) -> i64 {
    _stacks(player, "CONSTRICT")
}

/// HP we lose this turn if we do nothing more.
pub fn expected_loss(state: &Value) -> i64 {
    let player = &state["player"];
    let incoming: i64 = read_enemies(state).iter().map(|e| e.incoming).sum();
    0.max(incoming - player["block"].int() - end_of_turn_block(player) - osty_hp(player))
}

/// Turns until the Sandpit swallows us (None when there is none).
pub fn _sandpit(state: &Value) -> Option<i64> {
    std::iter::once(&state["player"]).chain(state["battle"]["enemies"].list()).find_map(|c| _shown(c, "SANDPIT", 1))
}

/// Chance that the next hand holds no Frantic Escape (from the piles as they are), and a hand a few turns on (from the whole deck).
pub fn _escape_odds(player: &Value, hand_draw: i64) -> (f64, f64) {
    let is_escape = |c: &&Value| c["name"].text().starts_with("Frantic Escape");

    /// No Escape among d cards drawn from n that hold k of them.
    fn none(n: i64, k: i64, d: i64) -> f64 {
        if n <= 0 || d <= 0 {
            return 1.0;
        }
        let d = d.min(n);
        let mut p = 1.0;
        for i in 0..d {
            p *= fmax(0.0, (n - k - i) as f64 / (n - i) as f64);
        }
        p
    }

    let Some(draw) = player["draw_pile"].as_array() else {
        return (0.3, 0.3);
    };
    let later: Vec<&Value> = player["discard_pile"].list().iter().chain(player["hand"].list()).collect();
    let (n, k) = (draw.len() as i64, draw.iter().filter(is_escape).count() as i64);
    let (m, j) = (later.len() as i64, later.iter().copied().filter(is_escape).count() as i64);
    let next_hand = if n >= hand_draw {
        none(n, k, hand_draw)
    } else {
        // the pile runs out: the rest of the hand comes from the reshuffled discards
        none(n, k, n) * none(m, j, hand_draw - n)
    };
    let usual = if n + m >= hand_draw { none(n + m, k + j, hand_draw) } else { 0.3 };
    (next_hand, fmax(usual, 0.05))
}

/// (cards we are cycling through this fight: hand, draw and discard piles; how many of them are dead weight).
pub fn deck_count(player: &Value) -> (i64, i64) {
    let piles: Vec<&Value> = ["hand", "draw_pile", "discard_pile"].iter().flat_map(|pile| player[*pile].list()).collect();
    if !player["draw_pile"].is_array() {
        return (piles.len() as i64 + player["draw_pile_count"].int() + player["discard_pile_count"].int(), 0);
    }
    let dead = |c: &&&Value| {
        let text = c["description"].text();
        crate::re!(r"(?i)\bunplayable\b").is_match(&text) || text.trim() == "Exhaust."
    };
    (piles.len() as i64, piles.iter().filter(dead).count() as i64)
}

/// What `added` more dead cards in the deck cost over the rest of the fight, in damage: each takes a card's place in a hand, every shuffle.
pub fn junk_cost(fight: &str, round_no: i64, deck: (i64, i64), added: i64) -> f64 {
    let (cards, junk) = deck;
    let turns = 1.max((match fight {
        "boss" => 10,
        "elite" => 6,
        _ => 4,
    }) - round_no);
    let size = 10.max(cards + added);
    added as f64 * (turns as f64 * 5.0 / size as f64) * 3.0 * (1.0 + 2.0 * (junk + added) as f64 / size as f64)
}

/// Block a relic gives when the turn ends without any (Orichalcum).
pub fn _bare_block(player: &Value) -> i64 {
    player["relics"]
        .list()
        .iter()
        .find_map(|r| crate::re!(r"(?i)end your turn without block, gain (\d+) block").captures(&r["description"].text()).map(|m| m[1].parse::<i64>().unwrap_or(0)))
        .unwrap_or(0)
}

/// What drawing a card is worth right now, from what is left to draw: (points if free, share that cost energy, points per energy).
pub fn _draw_worth(player: &Value) -> (f64, f64, f64) {
    let mut pile: Vec<&Value> = player["draw_pile"].list().iter().collect();
    if pile.len() < 3 {
        pile.extend(player["discard_pile"].list()); // it is about to be shuffled back in
    }
    if pile.len() < 3 {
        return (2.0, 0.0, 0.0);
    }
    let mut free = 0.0;
    let mut paid: Vec<f64> = Vec::new();
    for c in &pile {
        let text = c["description"].text();
        if crate::re!(r"(?i)\bunplayable\b").is_match(&text) {
            continue;
        }
        let info = parse(c);
        let mut worth = 1.2 * info.total_damage() as f64 * (if info.aoe { 1.4 } else { 1.0 }) + info.block as f64 + 2.0 * ((info.vulnerable > 0) as i64 as f64) + 1.5 * info.draw as f64;
        if !info.known {
            worth = 3.0;
        }
        if info.cost == 0 && info.star_cost <= 0 {
            free += worth;
        } else {
            paid.push(worth / 1.max(if info.is_x() { 2 } else { info.cost }) as f64);
        }
    }
    let n = pile.len() as f64;
    (free / n, paid.len() as f64 / n, if paid.is_empty() { 0.0 } else { py::fsum(paid.iter().copied()) / paid.len() as f64 })
}

/// Damage the next Wither does if still in the hand when the turn ends: 3 a level ("Wither+2" does 9); Aeonglass adds a level every third round.
pub fn _next_wither(player: &Value, round_no: i64) -> i64 {
    let mut level = 0.max(py::floordiv(round_no - 1, 3));
    for pile in ["hand", "draw_pile", "discard_pile", "exhaust_pile"] {
        for c in player[pile].list() {
            if let Some(n) = _wither_level(&c["name"].text()) {
                level = level.max(n);
            }
        }
    }
    3 * (level + 1)
}

/// What each card played costs in Withers still to come, in points: a sixth of what one does as it comes round again with the deck.
pub fn _wither_bill(player: &Value, round_no: i64, wither_hp: i64) -> f64 {
    let count = |p: &str| {
        let shown = &player[format!("{p}_count").as_str()];
        if shown.truthy() {
            shown.int()
        } else {
            player[p].list().len() as i64
        }
    };
    let deck = player["hand"].list().len() as i64 + count("draw_pile") + count("discard_pile");
    let again = fmin(1.5, 1.max(WITHER_FIGHT.get() - round_no) as f64 / fmax(3.0, deck as f64 / 5.0)); // times it comes round again
    again * (2.5 * (wither_hp + 3) as f64 + 6.0) / 6.0
}

/// How many more cards we may play this turn, when something limits it.
pub fn cards_left_this_turn(state: &Value) -> Option<i64> {
    let player = &state["player"];
    let played = state["_plays_this_turn"].int();
    let mut limits: Vec<i64> = Vec::new();
    if _has_power(player, "RINGING") {
        // "You can only play 1 card this turn."
        limits.push(1);
    }
    if _has_power(player, "SLOTH") {
        // "You cannot play more than 3 cards each turn." (by the number on the power: a card played twice counts twice)
        limits.push(played + 3 - 0.max(_shown(player, "SLOTH", 0).unwrap_or(0)));
    }
    for c in player["hand"].list().iter().chain(player["relics"].list()) {
        // (a relic too: Velvet Choker, 6 a turn)
        if let Some(m) = crate::re!(r"(?i)cannot play more than (\d+) cards").captures(&c["description"].text()) {
            limits.push(m[1].parse::<i64>().unwrap_or(0));
        }
    }
    let mut left: Vec<i64> = limits.iter().map(|n| 0.max(n - played)).collect();
    // The first game's Time Eater counts every card we play (Time Warp, kept from turn to turn): the 12th ends our turn.
    left.extend(state["battle"]["enemies"].list().iter().filter_map(|e| _shown(e, "TIME_WARP", 0)).map(|warp| 12 - 0.max(warp)));
    // "You can only play 1 more card this turn": the game counts it down, 1 then 0
    if let Some(encore) = _shown(player, "ENCORE", 1) {
        left.push(0.max(encore));
    }
    left.into_iter().min()
}

/// (Energy the next hand can spend beyond what it will have already, what each card kept in the hand adds to that): five cards
/// of the draw pile at their mean cost (X and over 3 as 3, Unplayable as 0) against the energy coming. None before a Ringing turn.
pub fn next_room(state: &Value) -> (f64, f64) {
    let player = &state["player"];
    for e in state["battle"]["enemies"].list() {
        if RINGERS.contains(&_kind(&py::str_of(&e["entity_id"])).as_str()) && e["intents"].list().iter().any(|i| _str_of(i, "type").to_lowercase().contains("debuff")) {
            return (-99.0, 0.0);
        }
    }
    let mut pile: Vec<&Value> = player["draw_pile"].list().iter().collect();
    if pile.len() < 5 {
        pile.extend(player["discard_pile"].list()); // (shuffled back in before the hand is drawn)
    }
    if pile.is_empty() {
        return (99.0, 1.0); // (no piles in the state: every energy counts)
    }

    let cost = |c: &&Value| -> f64 {
        if crate::re!(r"(?i)\bunplayable\b").is_match(&c["description"].text()) {
            return 0.0;
        }
        let n = if c["cost"].truthy() { py::str_of(&c["cost"]) } else { "0".to_string() };
        if !n.is_empty() && n.chars().all(|ch| ch.is_ascii_digit()) {
            fmin(3.0, n.parse::<f64>().unwrap_or(3.0))
        } else {
            3.0
        }
    };

    let per = py::fsum(pile.iter().map(cost)) / pile.len() as f64;
    let draws = 5 + 0.max(power_amount(player, "DRAW_CARDS_NEXT_TURN")) - 0.max(power_amount(player, "MIND_ROT")); // ("Draw 1 fewer card each turn")
    let have = _int_or(&player["max_energy"], 3) + 0.max(power_amount(player, "ENERGY_NEXT_TURN"));
    let mut caps: Vec<i64> = if _has_power(player, "SLOTH") { vec![3] } else { vec![] }; // (the Knowledge Demon's: "You cannot play more than 3 cards each turn")
    caps.extend(player["relics"].list().iter().filter_map(|r| crate::re!(r"(?i)cannot play more than (\d+) cards").captures(&r["description"].text()).map(|m| m[1].parse::<i64>().unwrap_or(0))));
    if let Some(&cap) = caps.iter().min() {
        if draws >= cap {
            // The plays a turn allows are used up by what it draws: a Retained hand adds no play, it only stands in for a card drawn.
            return (cap as f64 * per - have as f64, 0.0);
        }
    }
    (draws as f64 * per - have as f64, per)
}

/// What `gained` energy next turn is worth, in damage, with `kept` cards retained in the hand (next_room's `room`).
pub fn next_energy_worth(gained: i64, room: (f64, f64), kept: i64) -> f64 {
    let usable = fmin(gained as f64, fmax(0.0, room.0 + kept as f64 * room.1));
    NEXT_ENERGY.get() * usable + SPARE_ENERGY.get() * (gained as f64 - usable)
}

/// Void Form ("The first 2 cards you play each turn are free to play"): how many of the next cards cost nothing (a cost on show: none).
pub fn free_plays(state: &Value) -> i64 {
    let player = &state["player"];
    let costly = |c: &Value| {
        let cost = if c["cost"].truthy() { py::str_of(&c["cost"]) } else { "0".to_string() };
        !matches!(cost.trim(), "0" | "X")
    };
    if player["hand"].list().iter().any(costly) {
        return 0;
    }
    0.max(_stacks(player, "VOID_FORM") - state["_plays_this_turn"].int())
}

/// Artifact swallows a debuff (and a charge goes with it).
#[inline]
pub fn _resists(e: &mut Enemy) -> bool {
    if e.artifact > 0 {
        e.artifact -= 1;
        return true;
    }
    false
}

/// HP a Poison stack takes over the next `turns` turns (it loses 1 a turn).
pub fn _poison_total(stack: i64, turns: i64) -> i64 {
    let stack = stack.max(0);
    if stack >= turns {
        return turns * stack - py::floordiv(turns * (turns - 1), 2);
    }
    py::floordiv(stack * (stack + 1), 2)
}

/// What playing a Power is worth in an ordinary fight: from the table, else a guess from its cost; in a long fight, not less than crowd.rs says.
pub fn _power_worth(c: &CardInfo, long_fight: bool) -> f64 {
    let worth = match POWER_VALUE.get(c.key.as_str()) {
        Some(w) => *w,
        None => 5.0 + 2.0 * c.cost.max(0) as f64,
    };
    let crowd = if long_fight { crowd_rating(&c.key) } else { None };
    match crowd {
        None => worth,
        Some(crowd) => fmax(worth, 2.5 * c.cost.max(1) as f64 + 2.0 * (crowd - 4.0)),
    }
}

/// A card worth playing with nobody to aim at (the enemy is gone for the turn and comes back): whatever lasts, and draws while energy is left.
pub fn _set_up(playable: &[CardInfo], energy: i64) -> Option<Action> {
    let mut best: Option<&CardInfo> = None;
    let mut best_worth = 0.0;
    for c in playable {
        if c.needs_target || c.r#type == "Status" || c.r#type == "Curse" || c.deadly || c.block_lock || (c.is_x() && energy <= 0) {
            continue;
        }
        let (x, cost) = if c.is_x() { (energy, energy) } else { (1, c.cost) };
        let b = |flag: bool| flag as i64 as f64;
        let mut worth = if c.r#type == "Power" { _power_worth(c, true) } else { 0.0 };
        worth += (4.0 * c.channel.len() as f64 + c.summon as f64 + 1.5 * c.souls as f64) * x as f64 + 0.5 * c.forge as f64 + 2.5 * c.star_gain as f64;
        worth += 4.0 * c.strength as f64 * b(c.strength_lasts) + 3.5 * c.dexterity as f64 * b(c.dexterity_lasts) - ENERGY_DRAIN.get() * c.energy_drain as f64;
        worth += 5.0 * c.next_energy as f64 + 1.5 * c.next_draw as f64 + 0.8 * c.next_block as f64;
        if energy - cost > 0 || c.energy_gain != 0 {
            worth += 1.0 * c.draw as f64 + 2.0 * c.energy_gain as f64;
        }
        worth -= 3.0 * c.hp_loss as f64;
        worth /= cost as f64 + 0.5;
        if worth > best_worth {
            best = Some(c);
            best_worth = worth;
        }
    }
    best.map(|c| ("play_card".to_string(), json!({"card_index": c.index})))
}

/// Stock: an enemy that dies while it has Stock is summoned again at full HP (not a kill). Adaptable: gone for the turn, back stronger.
pub fn _respawns(foes: &mut [Enemy]) {
    for e in foes.iter_mut() {
        if !e.alive() && e.respawns > 0 && e.max_hp > 0 {
            e.hp = e.max_hp;
            e.block = 0;
            e.respawns -= 1;
            // the new one spends this turn on Defend and Buff: the attack of the one we killed is gone
            e.incoming = 0;
            e.incoming_hits = 0;
        } else if !e.alive() && e.adapts {
            // the Test Subject's first two forms: out of reach for the rest of the turn, and its attack with it
            e.hp = BEYOND_REACH.get();
            e.block = 0;
            e.spent = true;
            e.damage_mult = 0.0;
            e.incoming = 0;
            e.incoming_hits = 0;
            e.poison = 0;
            e.doom = 0;
            e.adapts = false;
            e.adapted = true;
        }
    }
}

/// Crab Rage, as the line plays out: each ally that dies gives the crabs still standing 99 Block for the rest of this turn.
pub fn _rage_blocks(foes: &mut [Enemy], known_dead: &mut HashSet<Arc<str>>) {
    let mut newly = 0;
    for e in foes.iter() {
        if !e.alive() && !known_dead.contains(&e.entity_id) {
            newly += 1;
        }
    }
    if newly == 0 {
        return;
    }
    for e in foes.iter() {
        if !e.alive() {
            known_dead.insert(e.entity_id.clone());
        }
    }
    for e in foes.iter_mut() {
        if e.alive() && e.rage {
            e.block += RAGE_BLOCK.get() * newly;
        }
    }
}

/// Beating Remnant: "You cannot lose more than 20 HP in a single turn." (0: no such relic)
pub fn loss_cap(player: &Value) -> i64 {
    player["relics"]
        .list()
        .iter()
        .find_map(|r| crate::re!(r"(?i)cannot lose more than (\d+) HP in a single turn").captures(&r["description"].text()).map(|m| m[1].parse::<i64>().unwrap_or(0)))
        .unwrap_or(0)
}

/// Whom an attack goes at: its place in `enemies` (None when nobody is alive).
pub fn _pick_target<E: Borrow<Enemy>>(enemies: &[E], dmg_per_hit: i64, hits: i64, double_if_vulnerable: bool) -> Option<usize> {
    let alive = || enemies.iter().enumerate().map(|(i, e)| (i, e.borrow())).filter(|(_, e)| e.alive());
    alive().next()?;
    // Reattach: a segment killed while another lives comes back, so until the others are within reach none is finished: they die together.
    let mut stay: Vec<&str> = Vec::new();
    if alive().filter(|(_, e)| e.reattach != 0).count() > 1 {
        let segs: Vec<(usize, &Enemy)> = alive().filter(|(_, e)| e.reattach != 0).collect();
        for (i, e) in &segs {
            if segs.iter().filter(|(o, _)| o != i).map(|(_, o)| o.hp + o.block).sum::<i64>() > REATTACH_REACH.get() && !stay.contains(&&*e.entity_id) {
                stay.push(&e.entity_id);
            }
        }
        if !stay.is_empty() && segs.len() == alive().count() {
            let spare: Vec<(usize, &Enemy)> = segs.iter().copied().filter(|(_, e)| !(stay.contains(&&*e.entity_id) && _would_kill(e, dmg_per_hit, hits, double_if_vulnerable))).collect();
            return py::max_by_key(if spare.is_empty() { segs } else { spare }, |(_, e)| e.hp + e.block).map(|(i, _)| i);
        }
    }
    // The biggest threat first, wasting the least damage; not one that revives unless it is about to hit us, nor an idle minion hit with twice its size.
    let size = dmg_per_hit * hits;
    let mut leader: Option<bool> = None; // any(o.worth >= 1.0 for o in alive), once it is asked
    let mut best: Option<(usize, (bool, i64, i64))> = None;
    for (i, e) in alive() {
        if (!stay.is_empty() && stay.contains(&&*e.entity_id)) || !_would_kill(e, dmg_per_hit, hits, double_if_vulnerable) {
            continue;
        }
        if !(e.worth() > 0.0 || e.incoming > 0) {
            continue;
        }
        if e.minion && !e.main_threat && e.incoming == 0 && size > 2 * (e.hp + e.block) && *leader.get_or_insert_with(|| alive().any(|(_, o)| o.worth() >= 1.0)) {
            continue;
        }
        let key = (e.worth() > 0.0, e.incoming, e.hp + e.block);
        if best.as_ref().is_none_or(|(_, held)| key > *held) {
            best = Some((i, key));
        }
    }
    if let Some((i, _)) = best {
        return Some(i);
    }
    // Else whoever dies soonest for the damage it does (Doom as health gone), leaders first, not one Doomed already or whose Block swallows it all.
    let floor = THREAT_FLOOR.get();
    let soaks = |e: &Enemy| e.block >= dmg_per_hit * hits && !e.keeps_block;
    let left = |e: &Enemy| ((1.max(e.hp - e.doom) + e.block) as f64) / (if e.vulnerable { 1.5 } else { 1.0 });
    py::min_by_key(alive(), |(_, e)| (_fated(e), -e.worth(), soaks(e), left(e) / fmax(e.threat, floor), -e.incoming)).map(|(i, _)| i)
}

/// `_pick_target` among some of the enemies (`pool`: their places in `foes`, as `fool::turn::impaled_pool` gives them): the target's place in `foes`.
pub fn _pick_target_among(foes: &[Enemy], pool: &[usize], dmg_per_hit: i64, hits: i64, double_if_vulnerable: bool) -> Option<usize> {
    if pool.len() == foes.len() {
        return _pick_target(foes, dmg_per_hit, hits, double_if_vulnerable); // (every enemy, in order: `impaled_pool` with nobody Impaled)
    }
    let some: Vec<&Enemy> = pool.iter().map(|&i| &foes[i]).collect();
    _pick_target(&some, dmg_per_hit, hits, double_if_vulnerable).map(|k| pool[k])
}

/// Doom covers its HP: it dies when its turn ends, whatever else we do to it (a boss with another form to come does not).
#[inline]
pub fn _fated(e: &Enemy) -> bool {
    e.doom >= e.hp && e.hp > 0 && !e.adapts
}

/// What the tests of the fight modules share: the states of tests/golden/fight, and Python's objects read back from JSON.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::games;

    /// A CardInfo from the fields `v` names (`{"C": {...}}` or the fields themselves), the rest as `CardInfo::default()` has them.
    pub fn card_from_json(v: &Value) -> CardInfo {
        let v = if v.has("C") { &v["C"] } else { v };
        let mut c = CardInfo::default();
        for (k, x) in v.entries() {
            match k.as_str() {
                "index" => c.index = x.int(),
                "name" => c.name = x.s().to_string(),
                "type" => c.r#type = x.s().to_string(),
                "cost" => c.cost = x.int(),
                "star_cost" => c.star_cost = x.int(),
                "damage" => c.damage = x.int(),
                "hits" => c.hits = x.int(),
                "double_if_vulnerable" => c.double_if_vulnerable = x.truthy(),
                "aoe" => c.aoe = x.truthy(),
                "random_target" => c.random_target = x.truthy(),
                "block" => c.block = x.int(),
                "vulnerable" => c.vulnerable = x.int(),
                "weak" => c.weak = x.int(),
                "draw" => c.draw = x.int(),
                "energy_gain" => c.energy_gain = x.int(),
                "hp_loss" => c.hp_loss = x.int(),
                "needs_target" => c.needs_target = x.truthy(),
                "playable" => c.playable = x.truthy(),
                "deadly" => c.deadly = x.truthy(),
                "block_lock" => c.block_lock = x.truthy(),
                "known" => c.known = x.truthy(),
                "strength" => c.strength = x.int(),
                "strength_lasts" => c.strength_lasts = x.truthy(),
                "enemy_strength_loss" => c.enemy_strength_loss = x.int(),
                "enemy_strength_lasts" => c.enemy_strength_lasts = x.truthy(),
                "key" => c.key = x.s().to_string(),
                "bound" => c.bound = x.truthy(),
                "poison" => c.poison = x.int(),
                "poison_needs" => c.poison_needs = x.truthy(),
                "doom" => c.doom = x.int(),
                "shivs" => c.shivs = x.int(),
                "star_gain" => c.star_gain = x.int(),
                "forge" => c.forge = x.int(),
                "summon" => c.summon = x.int(),
                "dexterity" => c.dexterity = x.int(),
                "dexterity_lasts" => c.dexterity_lasts = x.truthy(),
                "channel" => c.channel = x.list().iter().map(|s| s.s().to_string()).collect(),
                "evoke" => c.evoke = x.int(),
                "next_energy" => c.next_energy = x.int(),
                "next_draw" => c.next_draw = x.int(),
                "next_block" => c.next_block = x.int(),
                "keeps_hand" => c.keeps_hand = x.list().iter().map(|s| s.s().to_string()).collect(),
                "enemy_hp_loss" => c.enemy_hp_loss = x.int(),
                "souls" => c.souls = x.int(),
                "evoke_all" => c.evoke_all = x.int(),
                "short" => c.short = x.truthy(),
                "osty_attack" => c.osty_attack = x.truthy(),
                "osty_dies" => c.osty_dies = x.truthy(),
                "doom_kills" => c.doom_kills = x.truthy(),
                "debris" => c.debris = x.int(),
                "junk" => c.junk = x.int(),
                "enemy_strength_gain" => c.enemy_strength_gain = x.int(),
                "heal" => c.heal = x.int(),
                "ethereal" => c.ethereal = x.truthy(),
                "ends_turn" => c.ends_turn = x.truthy(),
                "replay" => c.replay = x.int(),
                "play_limit" => c.play_limit = x.int(),
                "shoots" => c.shoots = x.truthy(),
                "aim_times" => c.aim_times = x.int(),
                "aim" => c.aim = x.int(),
                "stretch" => c.stretch = x.int(),
                "impaled_only" => c.impaled_only = x.truthy(),
                "on_impaled" => c.on_impaled = if x.list().len() == 2 { Some((x[0].int(), x[1].int())) } else { None },
                "splash" => c.splash = x.truthy(),
                "stretch_dealt" => c.stretch_dealt = x.truthy(),
                "dislodge" => c.dislodge = x.truthy(),
                "impaled_double" => c.impaled_double = x.truthy(),
                "arrow_block" => c.arrow_block = x.truthy(),
                "melody" => c.melody = x.list().iter().map(|s| s.s().to_string()).collect(),
                "melody_hits" => c.melody_hits = x.truthy(),
                "melody_block" => c.melody_block = x.int(),
                "melody_twice" => c.melody_twice = x.list().iter().map(|s| s.s().to_string()).collect(),
                "verse" => c.verse = x.int(),
                "stance" => c.stance = x.s().to_string(),
                "wrath_next" => c.wrath_next = x.truthy(),
                "energy_drain" => c.energy_drain = x.int(),
                "mantra" => c.mantra = x.int(),
                "sadness" => c.sadness = x.int(),
                "sad_turn" => c.sad_turn = if x.list().len() == 2 { Some((x[0].int(), x[1].int())) } else { None },
                other => panic!("no CardInfo field {other}"),
            }
        }
        c
    }

    pub fn cards_from_json(v: &Value) -> Vec<CardInfo> {
        v.list().iter().map(card_from_json).collect()
    }

    /// An Enemy from `{"E": {...}}`.
    pub fn foe(v: &Value) -> Enemy {
        assert!(v.has("E"), "not an enemy: {v}");
        Enemy::from_json(&v["E"])
    }

    pub fn foes(v: &Value) -> Vec<Enemy> {
        v.list().iter().map(foe).collect()
    }

    /// A float as the golden files hold it (an infinite one as "inf").
    pub fn fj(x: f64) -> Value {
        if x == f64::INFINITY {
            json!("inf")
        } else if x == f64::NEG_INFINITY {
            json!("-inf")
        } else {
            json!(x)
        }
    }

    /// The game, constants, tables and settings a case was recorded under, in force until dropped.
    pub struct Env {
        _game: games::Playing,
        _constants: Vec<crate::tunable::Restore>,
        _threat: Option<KnownThreatGuard>,
        _minions: Option<MainMinionsGuard>,
        _knobs: crate::learn::KnobGuard,
    }

    pub fn env(v: &Value) -> Env {
        let mut constants = Vec::new();
        for (name, x) in v["tun"].entries() {
            constants.push(match name.as_str() {
                "RAGE_BLOCK" => RAGE_BLOCK.set(x.int()),
                "ETHEREAL_KEEP" => crate::fight::triggers::ETHEREAL_KEEP.set(x.float()),
                other => panic!("no constant {other} to set"),
            });
        }
        Env {
            _game: games::playing(v["g"].as_str().unwrap_or("sts2")),
            _constants: constants,
            _threat: v.get("kt").map(|t| with_known_threat(t.entries().map(|(k, x)| (k.clone(), x.float())).collect())),
            _minions: v.get("mm").map(|t| with_main_minions(&t.list().iter().map(|x| x.s()).collect::<Vec<_>>())),
            _knobs: crate::learn::with_knobs(&[("debuff_worth", v["dw"].float_or(2.0))]),
        }
    }

    /// The states of tests/golden/fight/states*.json: {"g": game, "dw": the debuff_worth setting, "s": state, "r": what Python made of it}.
    pub fn states() -> Vec<Value> {
        let mut out = Vec::new();
        for text in [include_str!("../../tests/golden/fight/states.json"), include_str!("../../tests/golden/fight/states2.json")] {
            let v: Value = serde_json::from_str(text).unwrap();
            out.extend(v["states"].list().iter().cloned());
        }
        assert!(out.len() > 100, "{} states", out.len());
        out
    }

    /// The recorded calls of one function: {"a": arguments, "r": result, "m": arguments as it left them, "env": ...}.
    pub fn calls<'a>(golden: &'a Value, name: &str, at_least: usize) -> &'a [Value] {
        let got = golden[name].list();
        assert!(got.len() >= at_least, "{name}: {} cases", got.len());
        got
    }

    pub const TARGETS: [(i64, i64, bool); 7] = [(0, 0, false), (6, 1, false), (12, 1, false), (5, 3, false), (30, 1, false), (9, 2, true), (60, 1, false)];
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    const HITS: [(i64, i64, bool, bool); 6] = [(6, 1, false, false), (4, 3, false, false), (9, 2, true, false), (30, 1, false, false), (7, 1, false, true), (100, 2, false, false)];

    #[test]
    fn golden_states() {
        let mut checked = 0;
        for (n, row) in states().iter().enumerate() {
            let _env = env(row);
            let (state, r) = (&row["s"], &row["r"]);
            let (player, listed) = (&state["player"], state["battle"]["enemies"].list());
            let round_no = if state["battle"]["round"].truthy() { state["battle"]["round"].int() } else { 1 };
            let why = |what: &str| format!("state {n}: {what}");
            let es = read_enemies(state);
            assert_eq!(es.len(), r["read_enemies"].list().len(), "{}", why("read_enemies"));
            for (e, want) in es.iter().zip(r["read_enemies"].list()) {
                assert_eq!(*e, Enemy::from_json(&want[0]), "{}", why("read_enemies"));
                assert_eq!(json!([e.worth(), e.alive()]), json!([want[1], want[2]]), "{}", why("worth"));
            }
            let room = next_room(state);
            assert_eq!(json!([expected_loss(state), _sandpit(state), cards_left_this_turn(state), [room.0, room.1], free_plays(state)]), r["state"], "{}", why("state"));
            let (deck, draw) = (deck_count(player), _draw_worth(player));
            let burns: Vec<Value> = hand_burns(player).iter().map(|b| json!([b.0, b.1, b.2])).collect();
            let got = json!([end_of_turn_block(player), end_of_turn_loss(player), burns, osty_hp(player), constrict(player), [deck.0, deck.1], _bare_block(player), [draw.0, draw.1, draw.2], loss_cap(player)]);
            assert_eq!(got, r["player"], "{}", why("player"));
            let odds = |d: i64| {
                let o = _escape_odds(player, d);
                json!([o.0, o.1])
            };
            assert_eq!(json!([odds(5), odds(3), odds(7)]), r["escape"], "{}", why("escape"));
            let got = json!([_next_wither(player, 1), _next_wither(player, round_no), _next_wither(player, 8), _wither_bill(player, round_no, 3), _wither_bill(player, 2, 9)]);
            assert_eq!(got, r["wither"], "{}", why("wither"));
            for (c, rows) in std::iter::once(player).chain(listed).zip(r["powers"].list()) {
                for want in rows.list() {
                    let pid = want[0].s();
                    let got = json!([pid, _shown(c, pid, 0), _shown(c, pid, 1).unwrap_or(-1), power_amount(c, pid), _has_power(c, pid), _stacks(c, pid)]);
                    assert_eq!(got, *want, "{}", why("powers"));
                    checked += 5;
                }
            }
            let got: Vec<Vec<Value>> = listed.iter().map(|e| e["intents"].list().iter().map(|i| json!([_is_attack(i), intent_damage(i), intent_hits(i)])).collect()).collect();
            assert_eq!(json!(got), r["intents"], "{}", why("intents"));
            let got: Vec<Value> = listed.iter().map(|e| json!([_reroll(e, player, 20), _reroll(e, player, 32)])).collect();
            assert_eq!(json!(got), r["reroll"], "{}", why("reroll"));
            let hand: Vec<CardInfo> = player["hand"].list().iter().map(parse).collect();
            for (c, want) in hand.iter().zip(r["hand"].list()) {
                assert_eq!(json!([_power_worth(c, true), _power_worth(c, false)]), json!([want[5], want[6]]), "{}", why(&c.key));
            }
            let playable: Vec<CardInfo> = hand.iter().filter(|c| c.playable).cloned().collect();
            let got: Vec<Value> = [0, player["energy"].int(), 3].iter().map(|n| _set_up(&playable, *n).map(|a| json!([a.0, a.1])).unwrap_or(Value::Null)).collect();
            assert_eq!(json!(got), r["set_up"], "{}", why("set_up"));
            let got: Vec<Option<usize>> = TARGETS.iter().map(|(d, h, dv)| _pick_target(&es, *d, *h, *dv)).collect();
            assert_eq!(json!(got), r["target"], "{}", why("target"));
            let refs: Vec<&Enemy> = es.iter().collect();
            let all: Vec<usize> = (0..es.len()).collect();
            for ((d, h, dv), want) in TARGETS.iter().zip(&got) {
                assert_eq!(_pick_target(&refs, *d, *h, *dv), *want, "{}", why("target by reference"));
                assert_eq!(_pick_target_among(&es, &all, *d, *h, *dv), *want, "{}", why("target among all"));
            }
            let got: Vec<Vec<bool>> = TARGETS.iter().map(|(d, h, dv)| es.iter().map(|e| _would_kill(e, *d, *h, *dv)).collect()).collect();
            assert_eq!(json!(got), r["kill"], "{}", why("kill"));
            assert_eq!(json!(es.iter().map(_fated).collect::<Vec<_>>()), r["fated"], "{}", why("fated"));
            assert_eq!(json!([fj(kill_reach(&es, true)), fj(kill_reach(&es, false))]), r["reach"], "{}", why("reach"));
            for ((d, h, dv, raw), rows) in HITS.iter().zip(r["hits"].list()) {
                for (e, want) in es.iter().zip(rows.list()) {
                    let mut e2 = clone(e);
                    let per = _per_hit(&e2, *d, *raw);
                    let removed = _hit(&mut e2, *d, *h, *dv, *raw);
                    assert_eq!(e2, Enemy::from_json(&want[2]), "{}", why("hit"));
                    let first = _progress(&mut e2, removed, 0.25);
                    let second = _progress(&mut Enemy { chipped: 3, stripped: 1, ..clone(&e2) }, removed, 1.0);
                    assert_eq!(json!([per, removed, want[2], first, second]), *want, "{}", why("hit"));
                    checked += 5;
                }
            }
            let mut gone: Vec<Enemy> = es.iter().map(|e| Enemy { hp: 0, ..clone(e) }).collect();
            _respawns(&mut gone);
            assert_eq!(gone, r["respawn"].list().iter().map(Enemy::from_json).collect::<Vec<_>>(), "{}", why("respawn"));
            let mut crabs: Vec<Enemy> = es.iter().map(clone).collect();
            let mut dead: HashSet<Arc<str>> = HashSet::new();
            if !crabs.is_empty() {
                crabs[0].hp = 0;
            }
            _rage_blocks(&mut crabs, &mut dead);
            let mut dead: Vec<&str> = dead.iter().map(|d| &**d).collect();
            dead.sort();
            assert_eq!(json!([crabs.iter().map(|e| e.block).collect::<Vec<_>>(), dead]), r["rage"], "{}", why("rage"));
            checked += 30 + es.len() * 12 + hand.len() * 2;
        }
        assert!(checked > 20_000, "{checked} checks");
    }

    #[test]
    fn golden_calls() {
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/fight/battle.json")).unwrap();
        let mut checked = 0;
        let mut each = |name: &str, at_least: usize, check: &dyn Fn(&Value, &Value) -> (Value, Value)| {
            for (n, rec) in calls(&g, name, at_least).iter().enumerate() {
                let _env = env(&rec["env"]);
                let (got, want) = check(&rec["a"], rec);
                assert_eq!(got, want, "{name} case {n}: {}", py::json_dumps(&rec["a"]));
                checked += 1;
            }
        };
        each("battle.reacted", 30, &|a, rec| (json!(reacted(a[0].int(), a[1].int(), a[2].int())), rec["r"].clone()));
        each("battle._hit_count", 20, &|a, rec| (json!(_hit_count(&foe(&a[0]), a[1].int(), a[2].truthy())), rec["r"].clone()));
        each("battle._per_hit", 300, &|a, rec| (json!(_per_hit(&foe(&a[0]), a[1].int(), a[2].truthy())), rec["r"].clone()));
        each("battle._would_kill", 300, &|a, rec| (json!(_would_kill(&foe(&a[0]), a[1].int(), a[2].int(), a[3].truthy())), rec["r"].clone()));
        each("battle._fated", 300, &|a, rec| (json!(_fated(&foe(&a[0]))), rec["r"].clone()));
        each("battle._hit", 500, &|a, rec| {
            let mut e = foe(&a[0]);
            let removed = _hit(&mut e, a[1].int(), a[2].int(), a[3].truthy(), a[4].truthy());
            (json!([removed, e.to_json()]), json!([rec["r"], foe(&rec["m"][0]).to_json()]))
        });
        each("battle._progress", 400, &|a, rec| {
            let mut e = foe(&a[0]);
            let gained = _progress(&mut e, a[1].int(), a[2].float());
            (json!([gained, e.to_json()]), json!([rec["r"], foe(&rec["m"][0]).to_json()]))
        });
        each("battle._resists", 300, &|a, rec| {
            let mut e = foe(&a[0]);
            let held = _resists(&mut e);
            (json!([held, e.to_json()]), json!([rec["r"], foe(&rec["m"][0]).to_json()]))
        });
        each("battle._pick_target", 400, &|a, rec| (json!(_pick_target(&foes(&a[0]), a[1].int(), a[2].int(), a[3].truthy())), rec["r"].clone()));
        each("battle.kill_reach", 250, &|a, rec| (fj(kill_reach(&foes(&a[0]), a[1].truthy())), rec["r"].clone()));
        each("battle._respawns", 200, &|a, rec| {
            let mut all = foes(&a[0]);
            _respawns(&mut all);
            (json!(all.iter().map(Enemy::to_json).collect::<Vec<_>>()), json!(foes(&rec["m"][0]).iter().map(Enemy::to_json).collect::<Vec<_>>()))
        });
        each("battle._rage_blocks", 200, &|a, rec| {
            let mut all = foes(&a[0]);
            let mut dead: HashSet<Arc<str>> = a[1].list().iter().map(|d| Arc::from(d.s())).collect();
            _rage_blocks(&mut all, &mut dead);
            let mut dead: Vec<&str> = dead.iter().map(|d| &**d).collect();
            dead.sort();
            (json!([all.iter().map(Enemy::to_json).collect::<Vec<_>>(), dead]), json!([foes(&rec["m"][0]).iter().map(Enemy::to_json).collect::<Vec<_>>(), rec["m"][1]]))
        });
        each("battle._set_focus", 200, &|a, rec| {
            let mut all = foes(&a[0]);
            _set_focus(&mut all);
            (json!(all.iter().map(Enemy::to_json).collect::<Vec<_>>()), json!(foes(&rec["m"][0]).iter().map(Enemy::to_json).collect::<Vec<_>>()))
        });
        each("battle.junk_cost", 100, &|a, rec| (json!(junk_cost(a[0].s(), a[1].int(), (a[2][0].int(), a[2][1].int()), a[3].int())), rec["r"].clone()));
        each("battle.next_energy_worth", 70, &|a, rec| (json!(next_energy_worth(a[0].int(), (a[1][0].float(), a[1][1].float()), a[2].int())), rec["r"].clone()));
        each("battle._poison_total", 30, &|a, rec| (json!(_poison_total(a[0].int(), a[1].int())), rec["r"].clone()));
        each("battle._power_worth", 30, &|a, rec| (json!(_power_worth(&card_from_json(&a[0]), a[1].truthy())), rec["r"].clone()));
        each("battle._set_up", 200, &|a, rec| (_set_up(&cards_from_json(&a[0]), a[1].int()).map(|x| json!([x.0, x.1])).unwrap_or(Value::Null), rec["r"].clone()));
        assert!(checked > 4000, "{checked} cases");
    }

    #[test]
    fn golden_cards() {
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/fight/cards.json")).unwrap();
        let rows = calls(&g, "cards", 1200);
        for (game, field) in [("sts2", "pw"), ("sts1", "pw1")] {
            let _game = crate::games::playing(game);
            for row in rows {
                let c = card_from_json(&row["c"]);
                assert_eq!(json!([_power_worth(&c, true), _power_worth(&c, false)]), row[field], "{game} {}", c.name);
            }
        }
    }

    #[test]
    fn the_tables_can_be_put_aside() {
        let state = json!({"state_type": "monster", "player": {"hp": 50}, "battle": {"round": 1, "enemies": [
            {"entity_id": "KIN_PRIEST_0", "hp": 190, "max_hp": 190, "intents": [{"type": "Attack", "label": "5"}]},
            {"entity_id": "KIN_FOLLOWER_0", "hp": 58, "max_hp": 58, "status": [{"id": "MINION_POWER", "amount": null}], "intents": [{"type": "Attack", "label": "5"}]},
            {"entity_id": "ROCKET_1", "hp": 190, "max_hp": 190, "intents": [{"type": "Buff"}]}]}});
        let read = || {
            let es = read_enemies(&state);
            (es[1].main_threat, es[2].threat, es[0].focus)
        };
        // (python3: battle.read_enemies(state) -> main_threat True, threat 20.0, focus 6/26)
        assert_eq!(read(), (true, 20.0, 6.0 / 26.0));
        {
            let _minions = with_main_minions(&[]);
            let _threat = with_known_threat(IndexMap::from([("KIN_PRIEST".to_string(), 30.0)]));
            assert_eq!(read(), (false, 0.0, 1.0));
            assert_eq!((known_threat_of("ROCKET"), known_threat_of("KIN_PRIEST"), is_main_minion("KIN_FOLLOWER")), (None, Some(30.0), false));
        }
        assert_eq!(read(), (true, 20.0, 6.0 / 26.0));
        assert_eq!((known_threat().len(), main_minions().len()), (6, 1));
        let mut crabs = vec![Enemy { entity_id: "A".into(), hp: 0, ..Enemy::default() }, Enemy { entity_id: "B".into(), hp: 9, rage: true, ..Enemy::default() }];
        {
            let _none = RAGE_BLOCK.set(0);
            _rage_blocks(&mut crabs.clone(), &mut HashSet::new());
            let mut calm = crabs.clone();
            _rage_blocks(&mut calm, &mut HashSet::new());
            assert_eq!(calm[1].block, 0);
        }
        _rage_blocks(&mut crabs, &mut HashSet::new());
        assert_eq!(crabs[1].block, 99);
    }
}
