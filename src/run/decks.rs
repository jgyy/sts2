//! What a card is worth in the deck that would take it (what the ratings in cards.rs, judging a card alone, cannot see:
//! strategy::card_reward and strategy::shop add it on), and what a card reward has to be worth to be taken.

use std::collections::HashSet;

use serde_json::{json, Value};

use super::crowd;
use crate::cards::{_card_key, deck_value, is_basic, parse as _parse};
use crate::learn::knob;
use crate::base::{self, JsonExt};

crate::tunables! {
    pub SKIP_THRESHOLD: f64 = 3.0; // card rewards scoring below this are skipped
    pub FEED_KEEP: f64 = 0.3; // of a payoff's worth kept with nothing yet to feed it (one may come later)
}
pub const BIG_DECK: i64 = 24;
pub const SHOT_SPARE: f64 = 0.5; // each Shot beyond the Arrows keeps this share of the one before it (a Shot needs our Arrow back: about one a turn)
pub const STARS_AT_START: f64 = 1.0; // Stars a cycle of the deck from the Regent's starting relic (3 a fight, once; a boss fight cycles two or three times)

/// `max(a, b)` of two floats as the original picks: the first unless the second is greater.
fn fmax(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// `min(a, b)` of two floats as the original picks: the first unless the second is smaller.
fn fmin(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// `str(card.get("description") or "")`.
fn _text(card: &Value) -> String {
    card["description"].text()
}

/// `str(card.get("type")) == "Power"`.
fn _is_power(card: &Value) -> bool {
    card["type"].as_str() == Some("Power")
}

/// (Stars the card needs, Stars it gives) each time it is played.
pub fn _stars(card: &Value) -> (i64, i64) {
    let info = _parse(card);
    let need = if info.star_cost < 0 { 2 } else { info.star_cost };
    let text = _text(card);
    let icons = |m: &str| m.matches("star_icon").count() as i64;
    let mut later: i64 = crate::re!(r"(?i)next turn, gain ((?:\[star_icon\.png\]\s*)+)").captures_iter(&text).map(|m| icons(&m[1])).sum();
    // Genesis ("At the start of your turn, gain 2 Stars"): about three turns to a cycle of the deck.
    later += 3 * crate::re!(r"(?i)start of (?:your|each) turn, gain ((?:\[star_icon\.png\]\s*)+)").captures_iter(&text).map(|m| icons(&m[1])).sum::<i64>();
    let mut give = info.star_gain;
    if give != 0 && crate::re!(r"(?i)if this kills[^.]*gain").is_match(&text) {
        give = 1; // Knockout Blow's 5 Stars come with a kill: counted in full they paid for a Seven Stars on paper
    }
    (need, give + later)
}

/// Damage an energy a deck's cards should average before damage stops being what it is short of (act -> damage; a starter deck does 3).
pub const PUNCH_FLOOR: [(i64, f64); 3] = [(1, 5.5), (2, 5.5), (3, 6.0)];

/// Damage a card does for an energy, counting what stands in for damage (Doom, Poison, Shivs, orbs).
pub fn _punch(card: &Value) -> f64 {
    let i = _parse(card);
    let x = if i.is_x() { 2 } else { 1 };
    let dealt = i.total_damage() as f64 * (if i.aoe { 1.4 } else { 1.0 }) * x as f64
        + 0.8 * i.doom as f64
        + (if i.poison_needs { 1.5 } else { 3.0 }) * i.poison as f64
        + 4.0 * i.shivs as f64
        + i.enemy_hp_loss as f64
        + 6.0 * i.channel.len() as f64;
    dealt / (if i.is_x() { 2 } else { i.cost }).max(1) as f64
}

pub const ATTACK_TAPER: f64 = 3.0; // cards over which the lean to attacks lets go once the deck has its share of them

/// `is_basic(c) or is_basic({"name": str(c.get("name") or "").rstrip("+")})`: a Strike or a Defend, upgraded or not.
fn _plain(c: &Value) -> bool {
    is_basic(c) || is_basic(&json!({"name": c["name"].text().trim_end_matches('+')}))
}

/// Cards the deck holds for their damage: 8 or more an energy, the Strikes (upgraded or not) and the Regent's own Falling Star left out.
pub fn _hitters(deck: &[Value]) -> i64 {
    if let Some(f) = HITTERS.with(|h| h.borrow().clone()) {
        return f(deck);
    }
    deck.iter().filter(|c| _punch(c) >= 8.0 && _card_key(c) != "FALLING_STAR" && !_plain(c)).count() as i64
}

thread_local! {
    static HITTERS: std::cell::RefCell<Option<std::rc::Rc<dyn Fn(&[Value]) -> i64>>> = const { std::cell::RefCell::new(None) };
}

/// Puts the earlier `_hitters` back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct HittersGuard(Option<std::rc::Rc<dyn Fn(&[Value]) -> i64>>);

impl Drop for HittersGuard {
    fn drop(&mut self) {
        HITTERS.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(decks, "_hitters", f)`: on this thread, until the guard is dropped.
pub fn with_hitters_hook(f: impl Fn(&[Value]) -> i64 + 'static) -> HittersGuard {
    HittersGuard(HITTERS.with(|h| h.borrow_mut().replace(std::rc::Rc::new(f))))
}

pub const BLOCK_ROOM: i64 = 2; // Block cards a deck of up to 23 takes at the full lean (learn::CHOICES["block_want"]); half for the next, none after
pub const ENERGY_POWER: f64 = 4.0; // a Power that gives energy every turn, a turn's first energy (on the Knowledge Demon model worth more than any attack)
pub const STRENGTH_TOLL: f64 = 0.65; // of that lost to "Lose 2 Strength" in a deck whose every hit takes Strength (Friendship: +6-8 points there)
pub const _ICONS: &str = r"((?:\[[a-z_]*energy_icon\.png\]\s*)+)";

/// The patterns of `_TURN_ENERGY`, in its order; the rate of each is worked out in `turn_energy`.
pub fn _TURN_ENERGY() -> [&'static regex::Regex; 4] {
    [
        crate::re!(r"(?i)at the start of (?:your|each) turn, gain ((?:\[[a-z_]*energy_icon\.png\]\s*)+)"), // Demesne
        crate::re!(r"(?i)gain ((?:\[[a-z_]*energy_icon\.png\]\s*)+)at the start of (?:your|each) turn"),   // Pyre, Friendship
        crate::re!(r"(?i)every (\d+)\s*\[[a-z_]*energy_icon\.png\] you spend, gain ((?:\[[a-z_]*energy_icon\.png\]\s*)+)"), // Orbit, on 3 a turn
        crate::re!(r"(?i)every (\d+) cards you draw, gain ((?:\[[a-z_]*energy_icon\.png\]\s*)+)"),         // Automation
    ]
}

/// The other way: a Power that takes energy away every turn (the first game's Fasting); each copy held makes the next dearer.
pub fn _ENERGY_LOSS() -> &'static regex::Regex {
    crate::re!(r"(?i)(?:gain\s+(\d+)\s+less\s+energy|lose\s+(\d+)\s+energy)\s+at\s+the\s+start\s+of\s+(?:your|each)\s+turn")
}

/// `int(text)` of a run of digits.
fn _digits(text: &str) -> i64 {
    text.parse().unwrap_or(0)
}

/// Energy a turn that a Power takes away once it is in play (0 for anything else).
pub fn energy_loss(card: &Value) -> i64 {
    if !_is_power(card) {
        return 0;
    }
    match _ENERGY_LOSS().captures(&_text(card)) {
        Some(m) => _digits(m.get(1).or(m.get(2)).map(|g| g.as_str()).unwrap_or("0")),
        None => 0,
    }
}

/// Energy a turn that a Power gives once it is in play (0 for anything else).
pub fn turn_energy(card: &Value) -> f64 {
    if !_is_power(card) {
        return 0.0;
    }
    let text = _text(card);
    for (n, pattern) in _TURN_ENERGY().iter().enumerate() {
        if let Some(m) = pattern.captures(&text) {
            let rate = match n {
                0 | 1 => 1.0,
                2 => 3.0 / _digits(&m[1]) as f64,
                _ => 5.5 / _digits(&m[1]) as f64,
            };
            let icons = m.get(m.len() - 1).map(|g| g.as_str()).unwrap_or("");
            return rate * icons.matches("energy_icon").count() as f64;
        }
    }
    0.0
}

pub const SCALE_POWER: f64 = 12.0; // a Power that adds damage every turn (Countdown, Lethality, Reaper Form), for as much again as a deck deals a turn
pub const SCALE_CAP: f64 = 8.0;
pub const TURN_DAMAGE: f64 = 30.0; // what a deck deals a round at the act 2 boss (fightlog.rs bosspace: under 30 a round 25% won, 35+ 70%)
pub const PLAYS_A_TURN: f64 = 4.0; // cards played a turn, for what a Power adds to every play of a kind

/// Damage (Doom counted) a Power adds every turn against one enemy once it is in play (0 for anything else).
pub fn turn_damage(card: &Value, deck: &[Value]) -> f64 {
    if !_is_power(card) {
        return 0.0;
    }
    let text = _text(card);
    if let Some(m) = crate::re!(r"(?i)at the start of your turn, apply (\d+) doom to (?:a random enemy|all enemies)").captures(&text) {
        // Countdown (Neurosurge's is on us)
        return _digits(&m[1]) as f64;
    }
    let infos: Vec<_> = deck.iter().map(_parse).collect();
    let share = PLAYS_A_TURN / deck.len().max(1) as f64; // of each card in the deck, played a turn
    let hits: Vec<i64> = infos.iter().map(|i| i.total_damage()).filter(|d| *d > 0).collect();
    if let Some(m) = crate::re!(r"(?i)first attack each turn deals (\d+)% additional damage").captures(&text) {
        // Lethality
        return _digits(&m[1]) as f64 / 100.0 * (if hits.is_empty() { 6.0 } else { hits.iter().sum::<i64>() as f64 / hits.len() as f64 });
    }
    if crate::re!(r"(?i)whenever attacks deal damage, apply that much doom").is_match(&text) {
        // Reaper Form
        return share * hits.iter().sum::<i64>() as f64;
    }
    if let Some(m) = crate::re!(r"(?i)osty's attacks deal (\d+) additional damage").captures(&text) {
        // Calcify
        return _digits(&m[1]) as f64 * share * infos.iter().filter(|i| i.osty_attack).count() as f64;
    }
    if let Some(m) = crate::re!(r"(?i)whenever you play a soul, a random enemy loses (\d+) hp").captures(&text) {
        // Haunt
        return _digits(&m[1]) as f64 * share * infos.iter().map(|i| i.souls).sum::<i64>() as f64;
    }
    0.0
}

// The Fool's Sadness stays in the hand all fight (fight/fool/sadness.rs): what a deck's makers put there a cycle, Ennui a turn.
pub const SAD_ROOM: i64 = 3; // Sadness a cycle of the deck that a hand of 5 still has room for
pub const SAD_ENNUI: i64 = 4; // Ennui's share of that: one a turn, all fight
pub const SAD_JOY: i64 = 2; // Sadness each Joy maker (Punch of Joy, Ecstasy) takes off the count
pub const SAD_PRICE: f64 = 1.0; // rating points a maker loses for each Sadness over the room, and at most
pub const SAD_MOST: f64 = 4.0;

/// Sadness a card puts in the hand over a cycle of the deck (Ennui: SAD_ENNUI).
pub fn sadness_made(card: &Value) -> i64 {
    let info = _parse(card);
    info.sadness.max(0) + if matches!(info.sad_turn, Some((n, _)) if n > 0) { SAD_ENNUI } else { 0 }
}

pub fn makes_joy(card: &Value) -> bool {
    let info = _parse(card);
    info.sadness < 0 || matches!(info.sad_turn, Some((n, _)) if n < 0)
}

pub const TIMES_UP_KEYS: [&str; 2] = ["TIMES_UP", "TIME_S_UP"]; // "Deal damage equal to the enemy's Doom" (the key is spelt both ways)

/// Whether a card puts Doom on an enemy (Neurosurge's 3 on the Necrobinder herself does not count).
pub fn makes_doom(card: &Value) -> bool {
    let text = _text(card);
    if crate::re!(r"(?i)apply \d+ doom to yourself").is_match(&text) {
        return false;
    }
    // (The parser leaves out Doom in a delayed clause, as Countdown's and Oblivion's, and Reaper Form's "that much".)
    _parse(card).doom > 0 || crate::re!(r"(?i)apply that much doom|apply \d+ doom to (?:the enemy|a random enemy|all enemies)").is_match(&text)
}

// Cards that pay off only on what other cards make, and do nothing else: what feeds each kind, and what pays off on it.
/// The kinds of `_FEEDS` and `_PAYS`, in the tables' order.
pub const KINDS: [&str; 4] = ["poison", "shiv", "soul", "exhaust"];

/// `_FEEDS[what].search(text)`.
pub fn _FEEDS(what: &str, text: &str) -> bool {
    match what {
        "poison" => crate::re!(r"(?i)apply (?:\d+|x) poison").is_match(text),
        "shiv" => crate::re!(r"(?i)add (?:\d+|x|an?) (?:inky )?shivs?\b").is_match(text),
        "soul" => crate::re!(r"(?i)add (?:\d+|x|an?) souls?\b|into soul\b").is_match(text),
        "exhaust" => crate::fre!(r"(?i)\bexhaust\b(?! pile)").is_match(text).unwrap_or(false),
        _ => panic!("KeyError: {}", base::str_repr(what)),
    }
}

/// `_PAYS[what].search(text)`.
pub fn _PAYS(what: &str, text: &str) -> bool {
    match what {
        "poison" => crate::re!(r"(?i)\bpoison\b").is_match(text),
        "shiv" => crate::re!(r"(?i)\bshivs?\b").is_match(text),
        "soul" => crate::re!(r"(?i)\bsouls?\b").is_match(text),
        "exhaust" => crate::re!(r"(?i)whenever a card is exhausted|if you (?:have )?exhausted a card").is_match(text),
        _ => panic!("KeyError: {}", base::str_repr(what)),
    }
}

pub const FEED_FULL: f64 = 2.0; // feeding cards a payoff needs to be worth its rating (a Power that feeds it every turn counts twice)
// (FEED_KEEP, which a test turns off, is with the tunables at the top.)
pub const FEED_LEAN: f64 = 1.0; // rating points for a feeding card, for each payoff held, while the deck has less than FEED_FULL of them
pub const FEED_LEAN_MOST: f64 = 2.0;

/// How much a card feeds payoffs of `what` (0: not at all; a Power that does it every turn counts 2).
pub fn feeds(card: &Value, what: &str) -> f64 {
    let text = _text(card);
    if !_FEEDS(what, &text) || (what == "exhaust" && _PAYS("exhaust", &text)) {
        return 0.0;
    }
    if what == "poison" && _parse(card).poison_needs {
        return 0.0; // Bubble Bubble's Poison needs Poison already there
    }
    if _is_power(card) {
        2.0
    } else {
        1.0
    }
}

/// What a card needs other cards to make before it does anything (None: it stands on its own).
pub fn pays_off(card: &Value) -> Option<&'static str> {
    let info = _parse(card);
    if info.total_damage() > 0 || info.block > 0 {
        return None;
    }
    let text = _text(card);
    KINDS.into_iter().find(|&what| _PAYS(what, &text) && (feeds(card, what) == 0.0 || (what == "poison" && info.poison_needs)))
}

/// A card taken for its Block: 5 or more when played, the Defends (upgraded or not) left out.
pub fn _blocks(card: &Value) -> bool {
    _parse(card).block >= 5 && !_plain(card)
}

/// How much better or worse a card is in *this* deck than on its own: the Regent's Stars (a 5-Star Comet is dead weight in
/// a deck that makes two a cycle), copies, a deck that cannot kill, energy every turn, payoffs with nothing to feed them.
/// (`act`: None in the original when left out; `energy`: 3.)
pub fn deck_fit(card: &Value, deck: &[Value], act: Option<i64>, energy: i64) -> f64 {
    if deck.is_empty() {
        return 0.0;
    }
    let n = deck.len() as i64;
    if TIMES_UP_KEYS.contains(&_card_key(card).as_str()) && !deck.iter().any(makes_doom) {
        return -deck_value(card); // Time's Up deals the Doom the enemy already has: nothing to hit in a deck that makes none
    }
    let mut fit = 0.0;
    let info = _parse(card);
    if info.shoots {
        let beyond = deck.iter().filter(|c| _parse(c).shoots).count() as i64 - deck.iter().filter(|c| _card_key(c) == "QUIVER").count() as i64;
        fit -= if beyond > 0 { fmax(0.0, deck_value(card)) * (1.0 - SHOT_SPARE.powf(beyond as f64)) } else { 0.0 };
    }
    // A card that costs more than a turn's energy is only ever played with help.
    let dear = info.cost > energy.max(3) && !crate::re!(r"(?i)reduce its cost|costs?\b[^.]*\bless").is_match(&_text(card));
    if dear {
        fit -= if (deck.iter().filter(|c| _parse(c).energy_gain != 0 || _card_key(c) == "FRIENDSHIP").count() as i64) < 2 { 3.0 } else { 1.0 };
    }
    let floor = if n >= 8 { act.and_then(|a| PUNCH_FLOOR.iter().find(|(k, _)| *k == a).map(|(_, v)| *v)) } else { None };
    if let Some(floor) = floor.filter(|f| *f != 0.0) {
        if _punch(card) >= 8.0 && !dear {
            // Worth more for as long as the deck is short of damage (how hard: learn::CHOICES["punch"]) ...
            let lean = knob("punch");
            let short = fmin(fmin(4.0, 2.0 * lean / 0.9), fmax(0.0, lean * (floor - base::fsum(deck.iter().map(_punch)) / n as f64)));
            // ... up to a point: the decks that lost the act 2 boss held the most attacks (learn::CHOICES["attack_room"]) ...
            fit += short * fmin(1.0, fmax(0.0, 1.0 - (_hitters(deck) as f64 - knob("attack_room")) / ATTACK_TAPER));
            // ... and the first one of a run comes before whatever pays off later.
            if act == Some(1) && !deck.iter().any(|c| _punch(c) >= 8.0 && _card_key(c) != "FALLING_STAR") {
                fit += 1.5;
            }
        }
    }
    let rate = turn_energy(card);
    if rate != 0.0 {
        // Each energy a turn already there (such Powers, or a relic's that the last fight began with) halves the next.
        let held = base::fsum(deck.iter().map(turn_energy)) + (energy - 3).max(0) as f64;
        let mut gain = ENERGY_POWER * fmin(rate, 1.0) * 0.5f64.powf(held);
        if info.strength < 0 && info.strength_lasts {
            // Friendship's 2 Strength comes off every hit that takes Strength: not Osty's, not Doom, not Poison.
            let hitters: Vec<&Value> = deck.iter().filter(|c| _parse(c).total_damage() > 0).collect();
            gain *= 1.0 - STRENGTH_TOLL * (hitters.iter().filter(|c| !_parse(c).osty_attack).count() as f64 / hitters.len().max(1) as f64);
        }
        fit += gain;
    }
    let lost = energy_loss(card);
    if lost != 0 {
        fit -= ENERGY_POWER * lost as f64 * (1 + 2 * deck.iter().map(energy_loss).sum::<i64>()) as f64;
    }
    let added = turn_damage(card, deck);
    if added != 0.0 {
        fit += fmin(SCALE_CAP, SCALE_POWER * added / TURN_DAMAGE);
    }
    if info.weak != 0 && (deck.iter().filter(|c| _parse(c).weak != 0).count() as i64) < 2 {
        fit += knob("weak_want"); // the first two sources of Weak (a setting under test, see learn::CHOICES)
    }
    if knob("block_want") != 0.0 && _blocks(card) && !dear {
        // The first Block cards beyond the Defends (learn::CHOICES["block_want"]): two in a deck of up to 23, one more for every eight after.
        let room = BLOCK_ROOM.max(n / 8);
        fit += knob("block_want") * fmin(1.0, fmax(0.0, (room + 1 - deck.iter().filter(|c| _blocks(c)).count() as i64) as f64 / 2.0));
    }
    let (need, give) = _stars(card);
    if need != 0 || give != 0 {
        let pairs: Vec<(i64, i64)> = deck.iter().map(_stars).collect();
        let demand: i64 = pairs.iter().map(|(n, _)| *n).sum();
        let supply = STARS_AT_START + pairs.iter().map(|(_, g)| *g).sum::<i64>() as f64;
        if need != 0 {
            let short = (demand + need) as f64 - supply;
            if short > 0.0 {
                fit -= fmin(4.0, 0.8 * short);
            }
        }
        if give != 0 && demand as f64 > supply {
            fit += fmin(2.5, 0.5 * (demand as f64 - supply));
        } else if give != 0 && supply - STARS_AT_START >= (demand + 3) as f64 {
            fit -= fmin(2.0, 0.5 * give as f64); // Stars nothing in the deck spends
        }
    }
    let made = sadness_made(card);
    if made != 0 {
        // Sadness beyond what the hands have room for is a card not drawn every turn after (2026-11-25: the Kin won on ten Sadness).
        let over = deck.iter().map(sadness_made).sum::<i64>() + made - SAD_ROOM - SAD_JOY * deck.iter().filter(|c| makes_joy(c)).count() as i64;
        fit -= fmin(SAD_MOST, SAD_PRICE * made.min(over.max(0)) as f64);
    } else if makes_joy(card) {
        fit += fmin(2.0, 0.5 * deck.iter().map(sadness_made).sum::<i64>() as f64); // (a deck with nothing to turn into Joy: none)
    }
    let key = _card_key(card);
    let copies = deck.iter().filter(|c| _card_key(c) == key).count() as i64;
    fit -= 0.6 * copies as f64;
    if copies == 0 && !dear {
        fit += crowd::partner(&key, &deck.iter().map(_card_key).collect::<HashSet<String>>()); // cards other players win with together
    }
    if key == "FRIENDSHIP" && copies != 0 {
        fit -= 2.5; // the planner leaves a second one in the hand (2 more Strength for energy it has no cards for)
    }
    for what in KINDS {
        let made = feeds(card, what);
        if made != 0.0 {
            // A feeding card for payoffs the deck already holds and cannot yet feed.
            let payoffs = deck.iter().filter(|c| pays_off(c) == Some(what)).count() as i64;
            let fed = base::fsum(deck.iter().map(|c| feeds(c, what)));
            if payoffs != 0 && fed < FEED_FULL {
                fit += fmin(FEED_LEAN_MOST, FEED_LEAN * payoffs as f64) * (1.0 - fed / FEED_FULL);
            }
        }
    }
    if let Some(need) = pays_off(card) {
        let fed = base::fsum(deck.iter().map(|c| feeds(c, need)));
        let share = FEED_KEEP.get() + (1.0 - FEED_KEEP.get()) * fmin(1.0, fed / FEED_FULL);
        fit -= (1.0 - share) * fmax(0.0, deck_value(card) + fit);
    }
    fit
}

/// What a card reward has to be worth to be taken. (`act`: None for anything that is not an int.)
pub fn reward_bar(deck_size: i64, act: Option<i64>) -> f64 {
    // Pickier as the deck grows (a mediocre card makes the good ones show up less often), half as much in acts 2 and 3 (damage is needed there).
    let mut growth = if deck_size < 16 {
        0.0
    } else if deck_size < 20 {
        1.0
    } else if deck_size < BIG_DECK {
        2.0
    } else {
        3.0
    };
    if matches!(act, Some(a) if a >= 2) {
        growth *= 0.5;
    }
    SKIP_THRESHOLD.get() + knob("pickiness") + growth
}

#[cfg(test)]
mod tests {
    //! Replays tests/golden/decks.json: calls recorded from the original tests and model runs of both games, and every card
    //! of tests/golden/cards.json through each function (recorded from decks.rs).
    use super::*;
    use crate::{games, learn};

    /// A recorded argument with its cards put back: {"$": n} is the file's pool, {"c": n} a card of cards.json, {"L": n} a list of them.
    fn resolve(v: &Value, g: &Value, cards: &[Value]) -> Value {
        match v {
            Value::Object(m) if m.len() == 1 && m.contains_key("$") => g["pool"][m["$"].as_u64().unwrap() as usize].clone(),
            Value::Object(m) if m.len() == 1 && m.contains_key("c") => cards[m["c"].as_u64().unwrap() as usize]["card"].clone(),
            Value::Object(m) if m.len() == 1 && m.contains_key("L") => resolve(&g["lists"][m["L"].as_u64().unwrap() as usize], g, cards),
            Value::Array(a) => Value::Array(a.iter().map(|x| resolve(x, g, cards)).collect()),
            other => other.clone(),
        }
    }

    /// Equal as the original's `==` would have it for results (an int and a float of one value are the same number), floats bit for bit.
    fn same(got: &Value, want: &Value) -> bool {
        match (got, want) {
            (Value::Number(a), Value::Number(b)) if a.is_f64() && b.is_f64() => a.as_f64().unwrap().to_bits() == b.as_f64().unwrap().to_bits(),
            (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
            (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y)),
            _ => got == want,
        }
    }

    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/decks.json")).unwrap();
        let all: Value = serde_json::from_str(include_str!("../../tests/golden/cards.json")).unwrap();
        let cards = all["cases"].as_array().unwrap();
        let mut checked = 0;
        for (name, rows) in g["cases"].as_object().unwrap() {
            for (n, row) in rows.as_array().unwrap().iter().enumerate() {
                let a = resolve(&row[0], &g, cards);
                let _game = games::playing(row[2].as_str().unwrap());
                let before = learn::knobs();
                if let Some(k) = row[1].as_u64() {
                    learn::set_knobs(&g["knobs"][k as usize].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap())).collect());
                }
                let got = match name.as_str() {
                    "_stars" => json!([_stars(&a[0]).0, _stars(&a[0]).1]),
                    "_punch" => json!(_punch(&a[0])),
                    "energy_loss" => json!(energy_loss(&a[0])),
                    "turn_energy" => json!(turn_energy(&a[0])),
                    "sadness_made" => json!(sadness_made(&a[0])),
                    "makes_joy" => json!(makes_joy(&a[0])),
                    "makes_doom" => json!(makes_doom(&a[0])),
                    "pays_off" => json!(pays_off(&a[0])),
                    "_blocks" => json!(_blocks(&a[0])),
                    "_hitters" => json!(_hitters(a[0].list())),
                    "feeds" => json!(feeds(&a[0], a[1].as_str().unwrap())),
                    "turn_damage" => json!(turn_damage(&a[0], a[1].list())),
                    "deck_fit" => json!(deck_fit(&a[0], a[1].list(), a[2].as_i64(), a[3].as_i64().unwrap())),
                    "reward_bar" => json!(reward_bar(a[0].as_i64().unwrap(), a[1].as_i64())),
                    other => panic!("no replay for {other}"),
                };
                learn::set_knobs(&before);
                assert!(same(&got, &row[3]), "{name} case {n}: {} gives {got}, the original {}", base::json_dumps(&a), row[3]);
                checked += 1;
            }
        }
        let per_card = g["per_card"].as_array().unwrap();
        assert_eq!(per_card.len(), cards.len());
        for (case, want) in cards.iter().zip(per_card) {
            let _game = games::playing(case["g"].as_str().unwrap());
            let c = &case["card"];
            let got = json!([
                [_stars(c).0, _stars(c).1],
                _punch(c),
                energy_loss(c),
                turn_energy(c),
                sadness_made(c),
                makes_joy(c),
                makes_doom(c),
                pays_off(c),
                _blocks(c),
                KINDS.iter().map(|w| feeds(c, w)).collect::<Vec<_>>()
            ]);
            assert!(same(&got, want), "{}: {got}, the original {want}", base::json_dumps(c));
            checked += 13;
        }
        assert_eq!(
            json!({"SKIP_THRESHOLD": SKIP_THRESHOLD.get(), "BIG_DECK": BIG_DECK, "PUNCH_FLOOR": PUNCH_FLOOR.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(), "ENERGY_POWER": ENERGY_POWER,
                   "SCALE_POWER": SCALE_POWER, "SCALE_CAP": SCALE_CAP, "TIMES_UP_KEYS": TIMES_UP_KEYS, "KINDS": KINDS}),
            g["consts"]
        );
        assert!(checked > 35_000, "{checked} checks");
    }

    #[test]
    fn the_bar_follows_its_constant() {
        let usual = reward_bar(10, Some(1));
        let _high = SKIP_THRESHOLD.set(99.0);
        assert_eq!(reward_bar(10, Some(1)), usual - 3.0 + 99.0);
    }
}
