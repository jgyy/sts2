//! Events: what each option on offer is worth, and which one to take.

use std::sync::LazyLock;

use indexmap::{IndexMap, IndexSet};
use serde_json::{json, Value};

pub mod sts1;
pub mod sts2;
pub mod value;

use self::sts1 as rules_sts1;
use self::sts2 as rules_sts2;
pub use self::value::Ctx;
use crate::cards::{deck_value, is_basic, is_known as _known_card};
use crate::games::uses_sts2_tables;
use crate::logging::{Level, Logger};
use crate::base::{self, JsonExt};
use crate::run::potions::boss_worth;
use crate::run::relics;
use crate::run::relicworth::{energy_lean, id_of as relic_id, worth as relic_worth};
use crate::run::strategy::Memory;
pub use crate::Action;

static LOG: Logger = Logger::new("sts2bot.strategy");
pub const _RELIC_WORD: &str = r"\brelics?\b";
#[rustfmt::skip]
pub const _GOOD: [(&str, f64); 10] = [
    (_RELIC_WORD, 6.0), (r"\bremove\b", 4.0), (r"\bupgrade\b", 4.0), (r"\btransform\b", 2.0), (r"\bpotions?\b", 1.5), (r"\bcards?\b", 1.0), (r"\benchant\b", 3.0),
    (r"draw \d+ additional cards?", 5.0), (r"energy_icon", 5.0), (r"\bgreater rewards\b", 6.0), // (more draw: every turn of the run; Punch Off: relic, card, potion)
];
// (The Lantern Key's fight costs about 27 HP and leaves an unplayable card; Foul Potions hit us as hard as the enemy.)
#[rustfmt::skip]
pub const _BAD: [(&str, f64); 8] = [
    (r"\bfoul potions?\b", 1.0), (r"\bfight to obtain the key\b", 6.0), (r"\bcurse\b", 6.0), (r"\bfight\b", 2.0), (r"draw \d+ fewer cards?", 4.0), (r"\bdowngrade\b", 2.0), (r"\bsacrifice\b", 5.0),
    (r"\bcan no longer heal\b", 14.0), // (the first game's Mind Bloom "I am Awake": rated 7-12 and always taken in act 3; 2026-11-25 a Watcher met the boss on 10/79 HP)
];
pub const EVENT_FIGHT_HP: [(&str, f64); 1] = [(r"\bfight them\b", 32.0)]; // HP an option that starts a fight costs, priced as "Lose N HP" is (Punch Off's constructs)
/// (doubles every draw pile and dilutes the good cards)
pub fn _DUPLICATE_DECK() -> &'static regex::Regex {
    crate::re!(r"duplicate (?:your|the) (?:whole )?deck")
}
/// What something bad received costs (a relic is 6): little for a card drawn once a fight, much for one that hurts or stays (two relics too).
#[rustfmt::skip]
pub static _CURSES: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([
    ("clumsy", 1.5), ("guilty", 1.5), ("poor sleep", 2.0), ("folly", 2.5), ("spore mind", 2.5), ("injury", 3.0), ("debt", 4.0), ("decay", 4.5), ("doubt", 4.5), ("shame", 4.5), ("greed", 5.0), ("curse of the bell", 5.0), ("regret", 5.0), ("writhe", 5.0),
    ("normality", 8.0), ("enthralled", 8.0), ("bad luck", 10.0), ("royal poison", 9.0), ("sword of stone", -0.5),
]));
/// (the first game writes "Become Cursed - Regret." and, in Mind Bloom, "Cursed - 2 Normality.")
pub fn _CURSE_GAIN() -> &'static regex::Regex {
    crate::re!(&format!(
        "{}{}{}",
        r"\b(?:receive|obtain|add|gain|become cursed|cursed -)(?:\b|\s)[^.]*?\b(",
        _CURSES.keys().copied().collect::<Vec<_>>().join("|"),
        r"|wound|burn|dazed|hex|parasite|pain|spite|void|mind rot|sloth|waste away|disintegration|curse)\b"
    ))
}
/// The run ended in all but name ("Lose ALL of your Gold" is not: it is the gold held, priced in the per-event rules).
pub fn _RUINOUS() -> &'static fancy_regex::Regex {
    crate::fre!(r"(?i)\blose (?:all|everything)\b(?![^.]*\bgold\b)|\bset your max hp\b|\bmax hp (to|becomes) \d\b")
}
#[rustfmt::skip]
pub const _JUNK_NAMES: [&str; 19] = ["CLUMSY", "GUILTY", "NORMALITY", "BADLUCK", "REGRET", "POORSLEEP", "INJURY", "WOUND", "DAZED", "SOOT", "SLIMED", "INFECTION", "ENTHRALLED", "DOUBT", "SHAME", "DECAY",
    "PARASITE", "PAIN", "WRITHE"];
/// What the Ancients' relics (the event opening each act) are worth on the scale of the scores below; relics.rs moves each by its runs.
#[rustfmt::skip]
pub static _ANCIENT: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([
    // more energy or cards every turn
    ("BLOOD_SOAKED_ROSE", 6.5), ("SPIKED_GAUNTLETS", 8.5), ("PHILOSOPHERS_STONE", 10.0), ("SOZU", 8.5), ("BLESSED_ANTLER", 8.0), ("PAELS_FLESH", 12.5), ("PAELS_BLOOD", 13.5), ("FIDDLE", 11.5), ("WHISPERING_EARRING", 12.0), ("PAELS_TEARS", 9.0),
    ("VERY_HOT_COCOA", 11.5), ("PUMPKIN_CANDLE", 8.5), ("SEAL_OF_GOLD", 10.0), ("BOOMING_CONCH", 5.5), ("SNECKO_EYE", 13.0), ("VELVET_CHOKER", 11.5), ("PRISMATIC_GEM", 9.0), ("ECTOPLASM", 7.0), ("RUNIC_PYRAMID", 9.0), ("IRON_CLUB", 9.5),
    // something every turn or every fight
    ("SAI", 13.0), ("CROSSBOW", 12.0), ("TOASTY_MITTENS", 11.0), ("MUSIC_BOX", 10.0), ("DELICATE_FROND", 12.0), ("CHOICES_PARADOX", 9.0), ("DIAMOND_DIADEM", 9.0), ("PAELS_LEGION", 8.0), ("RADIANT_PEARL", 9.5), ("JEWELED_MASK", 7.0), ("BRILLIANT_SCARF", 6.5),
    ("FUR_COAT", 10.0), ("BLACK_STAR", 8.5), ("ALCHEMICAL_COFFER", 9.5),
    // health, gold, relics
    ("LOOMING_FRUIT", 8.5), ("NUTRITIOUS_OYSTER", 8.5), ("STONE_HUMIDIFIER", 9.5), ("LORDS_PARASOL", 12.0), ("SIGNET_RING", 9.5), ("LARGE_CAPSULE", 8.5), ("SMALL_CAPSULE", 8.0), ("TOY_BOX", 9.0), ("LAVA_ROCK", 8.0), ("NEOWS_BONES", 7.0), ("GOLDEN_PEARL", 6.5),
    ("CURSED_PEARL", 6.0),
    // the deck
    ("YUMMY_COOKIE", 7.5), ("PAELS_TOOTH", 10.0), ("JEWELRY_BOX", 10.0), ("WAR_HAMMER", 10.5), ("FISHING_ROD", 8.5), ("PANDORAS_BOX", 10.5), ("SAND_CASTLE", 8.5), ("ELECTRIC_SHRYMP", 8.0), ("GLASS_EYE", 6.0), ("CALLING_BELL", 6.5), ("BIIIG_HUG", 9.0),
    ("ARCHAIC_TOOTH", 7.0), ("NUTRITIOUS_SOUP", 8.5), ("PAELS_CLAW", 7.0), ("BEAUTIFUL_BRACELET", 8.0), ("TRI_BOOMERANG", 7.5), ("PAELS_GROWTH", 7.5), ("PHIAL_HOLSTER", 7.5), ("PRECISE_SCISSORS", 6.5), ("PRESERVED_FOG", 7.0), ("PRECARIOUS_SHEARS", 5.0),
    ("LOST_COFFER", 6.5), ("ARCANE_SCROLL", 6.5), ("CLAWS", 7.0), ("SILVER_CRUCIBLE", 7.0), ("GLITTER", 7.0), ("POMANDER", 6.5), ("KALEIDOSCOPE", 6.5), ("SCROLL_BOXES", 7.5), ("DEMON_GLASS", 6.5), ("LEAD_PAPERWEIGHT", 8.5), ("DUSTY_TOME", 6.5), ("STORYBOOK", 6.5),
    ("NEOWS_TALISMAN", 6.0), ("NEW_LEAF", 5.5), ("DOWSING_ROD", 5.5), ("TANXS_WHISTLE", 5.5), ("NEOWS_TORMENT", 5.5), ("PAELS_HORN", 5.5), ("GOLDEN_COMPASS", 5.0), ("ASTROLABE", 9.0), ("EMPTY_CAGE", 8.5), ("THROWING_AXE", 7.5), ("DRIFTWOOD", 6.5),
    // things the bot cannot make use of
    ("WINGED_BOOTS", 4.0), ("PAELS_WING", 4.0), ("MEAT_CLEAVER", 3.5), ("PAELS_EYE", 3.0), ("DISTINGUISHED_CAPE", 3.0),
]));
pub const RELIC: f64 = 6.0; // what a relic counts for in an event (_GOOD)
pub const ACT3_HP: f64 = 1.6; // how much more a point of HP given up in an event costs in act 3 (the last boss is won from full HP)
pub const FIGHT_DEATH_LOW: f64 = 0.22; // chance an ordinary fight ends the run, entered under half HP / from half up
pub const FIGHT_DEATH_HIGH: f64 = 0.005;

/// Chance of getting through `fights` more ordinary fights from `hp`: a reward that comes after them needs that.
pub fn _lives_through(fights: i64, hp: i64, max_hp: i64) -> f64 {
    let death = if (hp as f64) < 0.5 * max_hp as f64 { FIGHT_DEATH_LOW } else { FIGHT_DEATH_HIGH };
    (1.0 - death).powf(fights as f64)
}

/// The first game's Secret Portal ("IMMEDIATELY travel to the boss"): the rest of act 3 (its rest sites, rewards, the keys still
/// missing) for the boss now. Taken on a 0.5-0.5 tie at 39-42 HP three times (2026-11-25), each a lost boss; the last boss is
/// won from (nearly) full HP only (strategy.boss_odds).
pub fn TO_BOSS() -> &'static regex::Regex {
    crate::re!(r"(?i)\btravel to the boss\b")
}
pub const TO_BOSS_HP: f64 = 0.9;
pub const TO_BOSS_PRICE: f64 = 4.0;

/// `_GOOD`'s patterns compiled: (the pattern, the same after "lose", "pay" or "give" in its sentence).
static GOOD_RE: LazyLock<Vec<(regex::Regex, regex::Regex)>> = LazyLock::new(|| {
    let compile = |p: &str| regex::Regex::new(p).expect("regex");
    _GOOD.iter().map(|(pat, _)| (compile(pat), compile(&format!("(?:lose|pay|give)[^.]*{pat}")))).collect()
});
static BAD_RE: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| _BAD.iter().map(|(pat, _)| regex::Regex::new(pat).expect("regex")).collect());
static FIGHT_RE: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| EVENT_FIGHT_HP.iter().map(|(pat, _)| regex::Regex::new(pat).expect("regex")).collect());

/// `int(digits)` of a number a pattern found (one too long for an i64, never seen, reads as a very large one).
fn int(digits: &str) -> i64 {
    digits.parse().unwrap_or(i64::MAX >> 16)
}

/// `str.strip()`: the original's blanks (those of Unicode and the four separators 0x1c-0x1f).
fn strip(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

/// `min(a, b)` of two floats as the original has it (the first of equals).
fn fmin(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// `max(a, b)` of two floats as the original has it (the first of equals).
fn fmax(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// The keyword arguments of [`_event_option_value`], with the original's defaults (`potions`, `held`, `deck`: None when not given).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EventOptionValueArgs<'a> {
    pub bled: i64,
    pub act: Option<i64>,
    pub potions: Option<&'a [Value]>,
    pub held: Option<&'a [Value]>,
    pub deck: Option<&'a [Value]>,
    pub energy: Option<i64>,
    pub keys_wanted: bool,
}

pub fn _event_option_value(opt: &Value, hp: i64, max_hp: i64, args: EventOptionValueArgs) -> f64 {
    let EventOptionValueArgs { bled, act, potions, held, deck, energy, keys_wanted } = args;
    let late = act.is_some_and(|a| a >= 3);
    let text = format!("{}. {}", opt["title"].text(), opt["description"].text()).to_lowercase();
    let key = crate::re!(r"[^A-Z0-9]+").replace_all(&crate::re!("['\u{2019}]").replace_all(&opt["title"].text(), "").to_uppercase(), "_").trim_matches('_').to_string();
    if let Some(known) = _ANCIENT.get(key.as_str()) {
        // (moved by how the runs that held it went, and for energy or a card every turn by the deck's energy)
        return known + relics::shift(&key) + energy_lean(&key, energy, held.unwrap_or(&[]), deck.unwrap_or(&[]));
    }
    let mut score = 0.5;
    if opt["relic_name"].truthy() {
        score += 6.0;
    }
    // Relics that come "after N combats" are had only if the run lives through the fights.
    let mut lives = 1.0;
    if let Some(m) = crate::re!(r"\bobtain \d+ random relics after (\d+) combats\b").captures(&text) {
        lives = _lives_through(int(&m[1]), hp, max_hp);
    }
    for ((pat, w), (re, lost)) in _GOOD.iter().zip(GOOD_RE.iter()) {
        if re.is_match(&text) && !lost.is_match(&text) {
            score += w * (if *pat == _RELIC_WORD { lives } else { 1.0 });
        }
    }
    // Gold is worth what it buys, a heal what is missing, Max HP its amount.
    if let Some(m) = crate::re!(r"\bgain (\d+) max hp").captures(&text) {
        score += fmin(6.0, 0.35 * int(&m[1]) as f64);
    } else if crate::re!(r"\bmax hp\b").is_match(&text) && !crate::re!(r"(?:lose|pay|give)[^.]*\bmax hp\b").is_match(&text) {
        score += 3.0;
    }
    if let Some(m) = crate::re!(r"\bgain (\d+) gold").captures(&text) {
        score += fmin(6.0, int(&m[1]) as f64 / 50.0);
    } else if crate::re!(r"\bgold\b").is_match(&text) && !crate::re!(r"(?:lose|pay|give)[^.]*\bgold\b").is_match(&text) {
        score += 2.0;
    }
    if let Some(m) = crate::re!(r"\bheal(?: (\d+)(%?))?").captures(&text) {
        let amount = match m.get(1).map(|g| g.as_str()).filter(|g| !g.is_empty()) {
            None => max_hp,
            Some(n) => {
                if m.get(2).is_some_and(|g| !g.as_str().is_empty()) {
                    base::floordiv(int(n) * max_hp, 100)
                } else {
                    int(n)
                }
            }
        };
        score += 12.0 * amount.min((max_hp - hp).max(0)) as f64 / max_hp.max(1) as f64;
    }
    // (A set in the original, summed in its order: the prices are halves, whose sum is the same in any order.)
    let cursed: IndexSet<&str> = _CURSE_GAIN().captures_iter(&text).map(|m| m.get(1).map_or("", |g| g.as_str())).collect();
    let copies = |name: &str| -> i64 {
        // ("Cursed - 2 Normality")
        let counted = base::re::cached(&format!(r"\b(\d+)\s+{}\b", base::re::escape(name)));
        counted.captures_iter(&text).map(|m| int(&m[1])).fold(1, i64::max)
    };
    if !cursed.is_empty() {
        score -= base::fsum(cursed.iter().map(|name| _CURSES.get(name).copied().unwrap_or(6.0) * copies(name) as f64));
    }
    // "Obtain the Sword of Stone", "Receive the Spoils Map": a relic by name (each judged on its own: "Receive Bad Luck" is not one).
    if !opt["relic_name"].truthy() && !text.contains("relic") {
        for gain in crate::fre!(r"\b(?:obtain|receive) (?:the )?(?!a\b|an\b|\d)[a-z][a-z' ]*").find_iter(&text) {
            let gain = gain.expect("regex");
            if !_CURSE_GAIN().is_match(gain.as_str()) {
                score += 5.0;
                break;
            }
        }
    }
    if _DUPLICATE_DECK().is_match(&text) {
        score -= 12.0;
    }
    if let Some(m) = crate::re!(r"\b(?:pay|give|lose) (\d+) gold").captures(&text) {
        score -= int(&m[1]) as f64 / (if late { 300.0 } else { 150.0 }); // (gold has little left to buy in act 3)
    }
    // A potion handed over is one fewer at the boss, where the second one on the belt goes with twice the wins.
    for p in potions.unwrap_or(&[]) {
        let name = strip(&p["name"].text()).to_lowercase();
        if !name.is_empty() && base::re::cached(&format!(r"\b(?:lose|give|insert)\b[^.]*\b{}", base::re::escape(&name))).is_match(&text) {
            score -= (1.0 + boss_worth(p) / 12.0) * (if late { 1.5 } else { 1.0 });
            break;
        }
    }
    // A relic of ours handed over costs a relic and what it does (relicworth.rs); one traded in brings what it does.
    let ours: Vec<String> = held.unwrap_or(&[]).iter().map(|r| base::str_of(&r["id"])).collect();
    let deck_known = deck.unwrap_or(&[]);
    // (the original's `$`: the end of the text, or just before a newline that ends it.)
    for gone in crate::fre!(r"\b(?:give|trade|lose) ([a-z][a-z' -]*?)(?= for |[.:]|\n?\z)").captures_iter(&text) {
        let gone = gone.expect("regex");
        let given = relic_id(gone.get(1).map_or("", |g| g.as_str()));
        if ours.contains(&given) {
            score -= RELIC + relic_worth(&given, deck_known, act, hp, max_hp);
            if let Some(m) = crate::fre!(r"\btrade [a-z' -]+? for ([a-z][a-z' -]*?)(?=[.:]|\n?\z)").captures(&text).expect("regex") {
                score += RELIC + relic_worth(&relic_id(m.get(1).map_or("", |g| g.as_str())), deck_known, act, hp, max_hp);
            }
            break;
        }
    }
    if let Some(m) = crate::re!(r"\bobtain (\d+) random relics\b").captures(&text) {
        score += RELIC * (int(&m[1]) - 1) as f64 * lives; // (the word "relics" above counts for one)
    }
    if potions.is_some_and(|p| p.len() < 2) && crate::re!(r"\bprocure\b[^.]*\bpotions?\b").is_match(&text) && !text.contains("foul") {
        score += 1.0; // (a belt short of two: the potion is worth more than the flat 1.5 above)
    }
    for ((pat, w), re) in _BAD.iter().zip(BAD_RE.iter()) {
        if re.is_match(&text) && !(!cursed.is_empty() && *pat == r"\bcurse\b") {
            // (a curse named or "a random Curse" is charged once, above)
            score -= w;
        }
    }
    if _RUINOUS().is_match(&text).expect("regex") {
        score -= 1000.0;
    }
    if let Some(m) = crate::re!(r"\.\s*(.+?) is removed from your deck").captures(&text) {
        // A named card goes: good riddance to a Strike, Defend or curse, a loss otherwise.
        let name = base::title(strip(&m[1]));
        let gone = json!({"name": name});
        let junk = is_basic(&gone) || _JUNK_NAMES.contains(&crate::re!(r"\W").replace_all(&name, "").to_uppercase().as_str());
        score += if junk { 2.0 } else { -0.8 * fmax(0.0, deck_value(&gone) - 2.0) };
    }
    if let Some(m) = crate::re!(r"\badd (?:an? |1 )?([a-z' !-]+?) to your deck").captures(&text) {
        let name = base::title(strip(&m[1]));
        let gained = json!({"name": name});
        if _JUNK_NAMES.contains(&crate::re!(r"\W").replace_all(&name, "").to_uppercase().as_str()) {
            score -= if !cursed.is_empty() { 0.0 } else { 4.0 }; // (a curse has been charged for by name)
        } else {
            score += if _known_card(&gained) { 0.5 * (deck_value(&gained) - 3.0) } else { 0.0 };
        }
    }
    if let Some(m) = crate::re!(r"lose\s+(\d+)(%?)\s+max\s+hp").captures(&text) {
        // Max HP never comes back: a small bite is a fair price for an upgrade or a relic, a big one is not.
        let share = if !m[2].is_empty() { int(&m[1]) as f64 / 100.0 } else { int(&m[1]) as f64 / max_hp.max(1) as f64 };
        score -= 1.5 + 60.0 * share + (if share >= 0.25 { 1000.0 } else { 0.0 });
    }
    if let Some(m) = crate::re!(r"(?:lose|take)\s+(\d+)\s+(?:hp|damage)").captures(&text) {
        let loss = int(&m[1]);
        score -= if loss >= hp { 1000.0 } else { 12.0 * loss as f64 / hp.max(1) as f64 * (if late { ACT3_HP } else { 1.0 }) };
        if bled >= 5 {
            score -= 2.0 + 0.5 * bled as f64; // each further bite of one event costs more, so a chain of small losses stops
        }
    }
    if TO_BOSS().is_match(&text) && (keys_wanted || (hp as f64) < TO_BOSS_HP * max_hp as f64) {
        score -= TO_BOSS_PRICE;
    }
    for ((_, cost), re) in EVENT_FIGHT_HP.iter().zip(FIGHT_RE.iter()) {
        if re.is_match(&text) {
            score -= if *cost >= hp as f64 { 1000.0 } else { 12.0 * cost / hp.max(1) as f64 * (if late { ACT3_HP } else { 1.0 }) };
            break;
        }
    }
    score
}

/// Events whose way out costs HP however late it is taken (the first game's Knowing Skull: 6 HP to leave, and every purchase
/// keeps us in the room): that HP is spent already, so only the purchases are priced. Priced like the rest, leaving always looked
/// a little worse than one more purchase (2026-11-25: 47 -> 1 and 66 -> 5 HP in one visit).
pub static PAID_EXIT: LazyLock<IndexMap<&'static str, &'static str>> = LazyLock::new(|| IndexMap::from([("KNOWING_SKULL", "How do I leave?")]));

/// `int(x or default)`.
fn int_or(v: &Value, default: i64) -> i64 {
    if v.truthy() {
        v.int()
    } else {
        default
    }
}

/// `x if isinstance(x, int) else None` (a bool is an int in the original).
fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(*b as i64),
        other if other.is_int() => Some(other.int()),
        _ => None,
    }
}

/// The run as the per-event rules see it. (`bled`: 0 in the original when left out.)
pub fn context(state: &Value, mem: &Memory, bled: i64) -> Ctx {
    let (player, run) = (&state["player"], &state["run"]);
    let (act, floor) = (&run["act"], &run["floor"]);
    Ctx {
        hp: int_or(&player["hp"], 1),
        max_hp: int_or(&player["max_hp"], 1),
        gold: int_or(&player["gold"], 0),
        act: as_int(act).unwrap_or(1),
        floor: as_int(floor).unwrap_or(1),
        deck: mem.deck.clone(),
        relics: player["relics"].list().to_vec(),
        potions: player["potions"].list().to_vec(),
        slots: int_or(&player["max_potion_slots"], 3),
        character: player["character"].text(),
        game: (if uses_sts2_tables() { "sts2" } else { "sts1" }).to_string(),
        bled,
        keys: run["keys"].as_array().cloned(),
    }
}

/// The event's own rule for this option (sts2.rs, sts1.rs), or None for the generic scorer.
///
/// (the original catches whatever a rule raises here, logs "event rule %s failed on %s: %s" at debug and gives None: a rule that
/// cannot read an option must not stop the run. The Rust rules raise nothing: see the top of sts1.rs.)
pub fn rule_value(event_id: &str, opt: &Value, ctx: &Ctx) -> Option<f64> {
    if ctx.game == "sts2" {
        rules_sts2::value(event_id, opt, ctx)
    } else {
        rules_sts1::value(event_id, opt, ctx)
    }
}

pub fn event(state: &Value, mem: &mut Memory) -> Vec<Action> {
    let ev = &state["event"];
    if ev["in_dialogue"].truthy() {
        return vec![("advance_dialogue".to_string(), json!({}))];
    }
    let opts: Vec<&Value> = ev["options"].list().iter().filter(|o| !o["is_locked"].truthy()).collect();
    if opts.is_empty() {
        return vec![("advance_dialogue".to_string(), json!({})), ("proceed".to_string(), json!({}))];
    }
    let player = &state["player"];
    let (hp, max_hp) = (int_or(&player["hp"], 1), int_or(&player["max_hp"], 1));
    // One event is one floor: the same event on a later floor starts again from its own HP.
    let eid = format!("{}@{}", ev["event_id"].text(), base::str_of(&state["run"]["floor"]));
    if eid != mem.event_id {
        (mem.event_id, mem.event_hp0) = (eid, hp);
    }
    let bled = (mem.event_hp0 - hp).max(0);
    let act = &state["run"]["act"];
    let event_id = ev["event_id"].text();
    let way_out = PAID_EXIT.get(event_id.as_str()).copied();
    let held_keys = &state["run"]["keys"]; // (the first game once act 4 is open: the keys held so far)
    let keys_held = match held_keys {
        Value::Null => None,
        Value::Array(a) => Some(a.len()),
        Value::Object(m) => Some(m.len()),
        Value::String(s) => Some(s.chars().count()),
        _ => None,
    };
    let args = EventOptionValueArgs {
        bled,
        act: as_int(act),
        potions: Some(player["potions"].list()),
        held: Some(player["relics"].list()),
        deck: Some(&mem.deck),
        energy: Some(mem.max_energy),
        keys_wanted: keys_held.is_some_and(|n| n < 3),
    };
    let priced = |o: &Value| _event_option_value(o, hp, max_hp, args);
    let ctx = context(state, mem, bled);

    let worth = |o: &Value| -> f64 {
        if way_out.is_some_and(|w| o["title"].as_str() == Some(w)) {
            return 0.5; // (0.5: an option's worth before what it gives and costs)
        }
        match rule_value(&event_id, o, &ctx) {
            Some(own) => own,
            None => priced(o),
        }
    };

    let mut rest: Vec<(f64, &Value)> = opts.iter().filter(|o| !o["is_proceed"].truthy()).map(|o| (worth(o), *o)).collect();
    base::sort_by_key_rev(&mut rest, |r| r.0);
    if LOG.enabled(Level::Debug) && !rest.is_empty() {
        let shown: Vec<String> = rest.iter().map(|(_, o)| format!("[{:.1}] {}: {}", worth(o), base::str_of(&o["title"]), base::str_of(&o["description"]))).collect();
        crate::log_debug!(LOG, "event {}: {}", base::str_of(&ev["event_id"]), shown.join(" | "));
    }
    let ordered = opts.iter().filter(|o| o["is_proceed"].truthy()).copied().chain(rest.iter().map(|r| r.1));
    ordered.map(|o| ("choose_event_option".to_string(), json!({"index": o["index"].int_or(0)}))).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::games;
    use crate::learn;

    /// The golden files name what repeats (cards, decks, belts, options) by its place in a pool: `{"$": n}`.
    pub(crate) fn resolve(v: &Value, pool: &[Value]) -> Value {
        match v {
            Value::Object(m) if m.len() == 1 && m.contains_key("$") => resolve(&pool[m["$"].as_u64().unwrap() as usize], pool),
            Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), resolve(x, pool))).collect()),
            Value::Array(a) => Value::Array(a.iter().map(|x| resolve(x, pool)).collect()),
            other => other.clone(),
        }
    }

    pub(crate) fn ctx_of(v: &Value) -> Ctx {
        Ctx {
            hp: v["hp"].as_i64().unwrap(),
            max_hp: v["max_hp"].as_i64().unwrap(),
            gold: v["gold"].as_i64().unwrap(),
            act: v["act"].as_i64().unwrap(),
            floor: v["floor"].as_i64().unwrap(),
            deck: v["deck"].list().to_vec(),
            relics: v["relics"].list().to_vec(),
            potions: v["potions"].list().to_vec(),
            slots: v["slots"].as_i64().unwrap(),
            character: v["character"].s().into(),
            game: v["game"].s().into(),
            bled: v["bled"].as_i64().unwrap(),
            keys: v["keys"].as_array().cloned(),
        }
    }

    /// The game, the settings and the relics' record a case was computed under; the earlier ones come back when it is dropped.
    pub(crate) struct World {
        _game: games::Playing,
        knobs: IndexMap<String, f64>,
        table: IndexMap<String, [f64; 2]>,
    }

    impl Drop for World {
        fn drop(&mut self) {
            learn::set_knobs(&self.knobs);
            relics::_table.with_borrow_mut(|t| *t = std::mem::take(&mut self.table));
        }
    }

    pub(crate) fn world(game: &Value, knobs: &Value, table: &Value) -> World {
        let before = World { _game: games::playing(game.s()), knobs: learn::knobs(), table: relics::_table.with_borrow(|t| t.clone()) };
        if let Some(knobs) = knobs.as_object() {
            learn::set_knobs(&knobs.iter().map(|(k, x)| (k.clone(), x.as_f64().unwrap())).collect());
        }
        relics::_table.with_borrow_mut(|t| {
            t.clear();
            for (k, row) in table.entries() {
                t.insert(k.clone(), [row[0].as_f64().unwrap(), row[1].as_f64().unwrap()]);
            }
        });
        before
    }

    fn args_of(v: &Value) -> EventOptionValueArgs<'_> {
        EventOptionValueArgs {
            bled: v["bled"].int(),
            act: v["act"].as_i64(),
            potions: v["potions"].as_array().map(|a| a.as_slice()),
            held: v["held"].as_array().map(|a| a.as_slice()),
            deck: v["deck"].as_array().map(|a| a.as_slice()),
            energy: v["energy"].as_i64(),
            keys_wanted: v["keys_wanted"].truthy(),
        }
    }

    /// `event`, `context`, `rule_value` and `_lives_through` as the original tests and the model runs of both games called them.
    #[test]
    fn golden_calls() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/events/calls.json")).unwrap();
        let pool = g["pool"].list();
        let (mut events_seen, mut numbers) = (0, 0);
        for case in g["event"].list() {
            let case = resolve(case, pool);
            let _world = world(&case["game"], &case["knobs"], &case["table"]);
            let mut mem = Memory {
                event_id: case["mem"]["event_id"].s().into(),
                event_hp0: case["mem"]["event_hp0"].as_i64().unwrap(),
                deck: case["mem"]["deck"].list().to_vec(),
                max_energy: case["mem"]["max_energy"].as_i64().unwrap(),
                ..Memory::default()
            };
            let got = event(&case["state"], &mut mem);
            let got: Vec<Value> = got.into_iter().map(|(action, params)| json!([action, params])).collect();
            assert_eq!(json!(got), case["out"], "event {}", case["state"]);
            assert_eq!(json!({"event_id": mem.event_id, "event_hp0": mem.event_hp0}), case["after"], "event {}", case["state"]);
            events_seen += 1;
        }
        for case in g["context"].list() {
            let case = resolve(case, pool);
            let _game = games::playing(case["game"].s());
            let mem = Memory { deck: case["deck"].list().to_vec(), ..Memory::default() };
            assert_eq!(context(&case["state"], &mem, case["bled"].as_i64().unwrap()), ctx_of(&case["out"]), "context {}", case["state"]);
        }
        for case in g["rule"].list() {
            let case = resolve(case, pool);
            let _world = world(&case["game"], &case["knobs"], &Value::Null);
            let got = rule_value(case["event_id"].s(), &case["opt"], &ctx_of(&case["ctx"]));
            assert_eq!(got, case["out"].as_f64(), "rule_value {} {}", case["event_id"], case["opt"]);
            numbers += got.is_some() as usize;
        }
        for row in g["lives"].list() {
            assert_eq!(_lives_through(row[0].as_i64().unwrap(), row[1].as_i64().unwrap(), row[2].as_i64().unwrap()), row[3].as_f64().unwrap(), "_lives_through {row}");
        }
        assert!(events_seen > 300 && g["context"].list().len() > 300 && g["rule"].list().len() > 600 && numbers > 80 && g["lives"].list().len() > 80, "{events_seen} events, {numbers} numbers");
    }

    /// `_event_option_value` as the original tests and the model runs of both games called it.
    #[test]
    fn golden_options() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/events/options.json")).unwrap();
        let pool = g["pool"].list();
        for case in g["option"].list() {
            let case = resolve(case, pool);
            let _world = world(&case["game"], &case["knobs"], &case["table"]);
            let got = _event_option_value(&case["opt"], case["hp"].as_i64().unwrap(), case["max_hp"].as_i64().unwrap(), args_of(&case));
            assert_eq!(got, case["out"].as_f64().unwrap(), "{case}");
        }
        assert!(g["option"].list().len() > 600);
    }

    /// `_event_option_value` over every option text on record (hand-written for each branch, the tests', the model runs', both
    /// games' rules') in a grid of HP, acts, belts, relics held and decks, under both games.
    #[test]
    fn golden_grid() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/events/grid.json")).unwrap();
        let pool = g["pool"].list();
        let args = resolve(&g["args"], pool);
        let mut checked = 0;
        for block in g["blocks"].list() {
            let _world = world(&block["game"], &block["knobs"], &block["table"]);
            for (opt, want) in g["texts"].list().iter().zip(block["out"].list()) {
                for (a, want) in args.list().iter().zip(want.list()) {
                    let got = _event_option_value(opt, a["hp"].as_i64().unwrap(), a["max_hp"].as_i64().unwrap(), args_of(a));
                    assert_eq!(got, want.as_f64().unwrap(), "{} {opt} with {a}", block["game"]);
                    checked += 1;
                }
            }
        }
        assert!(checked > 60000, "{checked} golden cases");
    }

    #[test]
    fn tables_as_recorded() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/events/tables.json")).unwrap();
        let dict = |rows: &IndexMap<&'static str, f64>| base::json_dumps(&Value::Object(rows.iter().map(|(k, x)| (k.to_string(), json!(x))).collect()));
        assert_eq!(dict(&_ANCIENT), base::json_dumps(&g["ancient"]));
        assert_eq!(dict(&_CURSES), base::json_dumps(&g["curses"]));
        assert_eq!(json!(_GOOD), g["good"]);
        assert_eq!(json!(_BAD), g["bad"]);
        assert_eq!(json!(EVENT_FIGHT_HP), g["fight_hp"]);
        let mut junk = _JUNK_NAMES.to_vec();
        junk.sort();
        assert_eq!(json!(junk), g["junk"]);
        assert_eq!(Value::Object(PAID_EXIT.iter().map(|(k, x)| (k.to_string(), json!(x))).collect()), g["paid_exit"]);
        assert_eq!(_CURSE_GAIN().as_str(), g["curse_gain"].s());
        assert_eq!(json!([RELIC, ACT3_HP, FIGHT_DEATH_LOW, FIGHT_DEATH_HIGH, TO_BOSS_HP, TO_BOSS_PRICE]), g["consts"]);
    }
}
