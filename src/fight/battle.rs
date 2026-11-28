//! What a combat state says: the enemies, their intents and powers, and what our own hand will cost us (for combat.rs's planner).

use std::borrow::Borrow;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::cards::{parse, CardInfo};
use crate::base::{self, fmax, fmin, JsonExt};
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
    /// the first game's Transient read as it plays (off: a 999 HP enemy like any other, as before 2026-11-28)
    pub SHIFTING: bool = true;
    /// the Fool's cards read as they play: Dislike You's mark on Shots, Cho Chorus's free Wah and Ki cards, the Melodies
    /// that make Cho Limbs and Coda cheaper (off: as before 2026-11-28)
    pub FOOL_READS: bool = true;
    /// the first game's Champ under half HP wipes every debuff off him and gains Strength for good: a turn that takes him under
    /// by little costs this less what it takes him under by, in points of damage (2026-11-28, model runs: 1,536 against 1,505
    /// of 2,723 fights met at the same HP; 100 was worse; 0: off)
    pub CHAMP_HOLD: f64 = 40.0;
    /// a card that makes us Vulnerable (the first game's Berserk) puts half again on this turn's blows when we are not already
    /// (2026-11-28: an Ironclad on 48 HP played Berserk into the reborn Awakened One's 44 and died to 66; off: as before)
    pub SELF_VULN: bool = true;
    /// the first game's Barricade (Spheric Guardian) is not Burrowed: its Block stays, but breaking it Stuns nothing (2026-11-28: a
    /// Defect with 4 Defends and 3 Lightning orbs took 12x2 for nothing, the orbs read as Stunning it; off: read as Burrowed, as before)
    pub BARRICADE_HOLDS: bool = true;
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
            /// The defaults of the original dataclass (its six fields without one: "", 0, 0, false, false, 0).
            fn default() -> Self {
                Enemy { $($name: $default,)* }
            }
        }

        impl Enemy {
            /// Every field by name, in the order above, as `dataclasses.asdict` gives them.
            pub fn to_json(&self) -> Value {
                let mut out = Map::new();
                $(out.insert(stringify!($name).into(), EnemyField::put(&self.$name));)*
                if !self.barricade {
                    out.remove("barricade"); // (as the records older than it have it)
                }
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
    /// Barricade (BARRICADE_HOLDS): its Block stays too, and breaking it Stuns nothing
    barricade: bool = false;
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
    /// Dislike You: Shots deal this percent more on it this turn
    marked: i64 = 0;
    /// Beat of Death (the first game's Heart): damage we take for every card we play, once it has resolved (Block takes it)
    beat: i64 = 0;
    /// Malleable (the first game's Snake Plant, Writhing Mass): Block after each hit that hurts it, 1 more each time this turn
    malleable: i64 = 0;
    /// ... that Block, still to come: it lands once the card being played has done its hits
    soft: i64 = 0;
    /// Steam Eruption (the Waterfall Giant): killed, it deals this much at the end of our next turn (eruption.rs)
    blast: i64 = 0;
    /// Serrated Arrows: what an Attack's hit on it does, times this, while our Arrow stands in it
    serrated: f64 = 1.0;
    /// Shifting (the first game's Transient): each HP it loses comes off its blow this turn, times this (0: no such power)
    shifting: f64 = 0.0;
    /// Fading: it leaves by itself in a few turns, long before its HP runs out (`_progress`)
    fading: bool = false;
}

impl Enemy {
    /// How much damaging this enemy brings the fight closer to its end.
    #[inline]
    pub fn worth(&self) -> f64 {
        if let Some(value) = _MINION_WORTH.with(|w| w.get()) {
            return if self.minion { value } else { 1.0 };
        }
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

    /// Shifting: HP it has just lost comes off its blow this turn (as Strength: Weak cuts that too).
    #[inline]
    pub fn shift(&mut self, lost: i64) {
        if self.shifting != 0.0 {
            self.incoming = 0.max(self.incoming - base::trunc(lost as f64 * self.shifting * (if self.weak { self.weak_cut } else { 1.0 })) * 1.max(self.incoming_hits));
        }
    }
}

/// `dataclasses.replace` for an Enemy at a third of the cost: a plan copies the enemies thousands of times.
/// (Changes go in with the struct update syntax: `Enemy { chipped: 0, ..clone(e) }`.)
#[inline]
pub fn clone(e: &Enemy) -> Enemy {
    e.clone()
}

/// `str(d.get(key, ""))`.
fn _str_of(d: &Value, key: &str) -> String {
    match d.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(x) => base::str_of(x),
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
            let upper = base::str_of(other).to_uppercase();
            upper.strip_suffix("_POWER").unwrap_or(&upper) == power_id
        }
    }
}

/// The number the game shows on a power, named like VULNERABLE_POWER (`blank` if it shows none; None if there is no such power:
/// the original's `missing`, which callers give with `.unwrap_or(missing)`).
pub fn _shown(creature: &Value, power_id: &str, blank: i64) -> Option<i64> {
    for p in creature["status"].list() {
        let named = match p.get("id") {
            Some(id) => _names(id, power_id),
            None => power_id.is_empty(),
        };
        if named {
            return Some(match &p["amount"] {
                Value::Number(n) => n.as_i64().unwrap_or_else(|| n.as_f64().map(base::trunc).unwrap_or(blank)),
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
    // scripts/bosses/queenfight.rs's MW=X: `worth` is X for a minion and 1.0 for anyone else while this is set
    // (the script's `mock.patch.object(battle.Enemy, "worth", property(lambda e: value if e.minion else 1.0))`)
    static _MINION_WORTH: Cell<Option<f64>> = const { Cell::new(None) };
    static _KNOWN_THREAT: RefCell<Option<IndexMap<String, f64>>> = const { RefCell::new(None) };
    static _MAIN_MINIONS: RefCell<Option<BTreeSet<String>>> = const { RefCell::new(None) };
}

/// Another `Enemy::worth` on this thread from now on: `value` for a minion, 1.0 for any other enemy (None: the rule as written).
pub fn set_minion_worth(value: Option<f64>) -> Option<f64> {
    _MINION_WORTH.with(|w| w.replace(value))
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
    base::sort_by_key(&mut order, |&i| -fmax(enemies[i].threat, floor) / left(&enemies[i]));
    let total = base::fsum(order.iter().map(|&i| fmax(enemies[i].threat, floor)));
    for n in 0..order.len() {
        let rest = base::fsum(order[n..].iter().map(|&o| fmax(enemies[o].threat, floor)));
        enemies[order[n]].focus = rest / total;
    }
}

/// A Reactive enemy's new intent as the shown ones are: its Strength (3 hits' worth), our Vulnerable, its Weak, Wrath.
pub fn _reroll(e: &Value, player: &Value, base: i64) -> i64 {
    if !_has_power(e, "REACTIVE") {
        return 0;
    }
    let hit = ((base + 3 * _stacks(e, "STRENGTH")) as f64) * (if _has_power(player, "VULNERABLE") { 1.5 } else { 1.0 }) * (if _has_power(e, "WEAK") { 0.75 } else { 1.0 });
    base::round(hit * (if player["stance"].text().to_uppercase() == "WRATH" { 2.0 } else { 1.0 }))
}

pub fn read_enemies(state: &Value) -> Vec<Enemy> {
    let mut out: Vec<Enemy> = Vec::new();
    let player = &state["player"];
    let listed = state["battle"]["enemies"].list();
    let ids: Vec<String> = listed.iter().map(|e| base::str_of(&e["entity_id"])).collect();
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
            keeps_block: _has_power(e, "BURROWED") || (_has_power(e, "BARRICADE") && !BARRICADE_HOLDS.get()),
            barricade: _has_power(e, "BARRICADE") && BARRICADE_HOLDS.get(),
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
            marked: if FOOL_READS.get() { _stacks(e, "DISLIKE_YOU") } else { 0 },
            serrated: 1.0 + _stacks(player, "SERRATED_ARROWS") as f64 / 100.0,
            beat: _stacks(e, "BEAT_OF_DEATH"),
            malleable: _stacks(e, "MALLEABLE"),
            // (not once the blast is its intent: it is in `incoming` then)
            // (as its intent shows the blow: with our Vulnerable and Wrath in it)
            shifting: if SHIFTING.get() && _has_power(e, "SHIFTING") { (if _has_power(player, "VULNERABLE") { 1.5 } else { 1.0 }) * (if player["stance"].text().to_uppercase() == "WRATH" { 2.0 } else { 1.0 }) } else { 0.0 },
            fading: SHIFTING.get() && _has_power(e, "FADING"),
            blast: if crate::fight::eruption::ERUPTION.get() && (!spent || intents.iter().map(intent_damage).sum::<i64>() == 0) { _stacks(e, "STEAM_ERUPTION") } else { 0 },
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
    base::trunc(per)
}

/// Whether this attack leaves the enemy dead, without touching it (worked out directly in the plain case: it is asked very often).
pub fn _would_kill(e: &Enemy, dmg_per_hit: i64, hits: i64, double_if_vulnerable: bool) -> bool {
    if e.skittish != 0 || e.hit_cap != 0 || e.slippery != 0 || e.shell >= 0 || e.flutter != 0 || (e.impaled != 0 && e.serrated != 1.0) {
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
    if e.fading {
        return 0.0; // (it is gone long before its HP is: a hit is worth what it takes off its blow, see `shift`)
    }
    // A Slippery stack taken off is a later hit that lands in full.
    let strip = e.stripped;
    e.stripped = 0;
    let hp_removed = hp_removed as f64 + strip as f64 * fmin(SLIP_WORTH.get(), e.hp as f64 / 3.0);
    hp_removed + chip as f64 * (if e.keeps_block || e.barricade || (e.incoming == 0 && chip != 0 && e.block == 0 && chip_rate >= 1.0) { 1.0 } else { chip_rate })
}

/// Apply an attack hit by hit, return HP actually removed. `raw` is damage that is not an Attack (an orb's): Vulnerable does nothing for it.
pub fn _hit(e: &mut Enemy, dmg_per_hit: i64, hits: i64, double_if_vulnerable: bool, raw: bool) -> i64 {
    let mut per = _per_hit(e, dmg_per_hit, raw);
    let mut removed = 0;
    for _ in 0.._hit_count(e, hits, double_if_vulnerable) {
        if e.hp <= 0 {
            break;
        }
        let mut hit = per;
        if e.impaled != 0 && !raw {
            hit = base::trunc(per as f64 * e.serrated); // (the Shot that Impales it lands before the Arrow stands)
            e.impaled -= 1; // (Block or not)
        }
        let absorbed = e.block.min(hit);
        e.block -= absorbed;
        e.chipped += absorbed;
        if absorbed != 0 && e.block == 0 && e.keeps_block {
            e.keeps_block = false;
            e.incoming = 0; // "Stunned if all Block is removed."
        }
        // Skittish: the first hit of a turn lands, the rest meet its Block
        e.block += e.skittish;
        e.skittish = 0;
        let mut through = hit - absorbed;
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
        e.shift(lost);
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

    let Some(draw) = player["draw_pile"].as_array() else { return (0.3, 0.3) };
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
    (free / n, paid.len() as f64 / n, if paid.is_empty() { 0.0 } else { base::fsum(paid.iter().copied()) / paid.len() as f64 })
}

/// Damage the next Wither does if still in the hand when the turn ends: 3 a level ("Wither+2" does 9); Aeonglass adds a level every third round.
pub fn _next_wither(player: &Value, round_no: i64) -> i64 {
    let mut level = 0.max(base::floordiv(round_no - 1, 3));
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
        if RINGERS.contains(&_kind(&base::str_of(&e["entity_id"])).as_str()) && e["intents"].list().iter().any(|i| _str_of(i, "type").to_lowercase().contains("debuff")) {
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
        let n = if c["cost"].truthy() { base::str_of(&c["cost"]) } else { "0".to_string() };
        if !n.is_empty() && n.chars().all(|ch| ch.is_ascii_digit()) {
            fmin(3.0, n.parse::<f64>().unwrap_or(3.0))
        } else {
            3.0
        }
    };

    let per = base::fsum(pile.iter().map(cost)) / pile.len() as f64;
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
        let cost = if c["cost"].truthy() { base::str_of(&c["cost"]) } else { "0".to_string() };
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
        return turns * stack - base::floordiv(turns * (turns - 1), 2);
    }
    base::floordiv(stack * (stack + 1), 2)
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
            return base::max_by_key(if spare.is_empty() { segs } else { spare }, |(_, e)| e.hp + e.block).map(|(i, _)| i);
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
    let soaks = |e: &Enemy| e.block >= dmg_per_hit * hits && !e.keeps_block && !e.barricade;
    let left = |e: &Enemy| ((1.max(e.hp - e.doom) + e.block) as f64) / (if e.vulnerable { 1.5 } else { 1.0 });
    base::min_by_key(alive(), |(_, e)| (_fated(e), -e.worth(), soaks(e), left(e) / fmax(e.threat, floor), -e.incoming)).map(|(i, _)| i)
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


/// What the tests of the fight modules share: the states of tests/golden/fight, and the original's objects read back from JSON.
#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests;
