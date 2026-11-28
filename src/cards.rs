//! Card understanding from the text the game shows on the card. There is no card database on purpose: the bot reads the
//! rendered description, so it keeps working when the game adds or rebalances cards.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::LazyLock;

use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::games;
use crate::base::{self, fmax, fmin, JsonExt};
use crate::run::crowd::rating as crowd_rating;

pub const _WORD_NUM: [(&str, i64); 2] = [("twice", 2), ("thrice", 3)];

/// `re.search(pattern, text, re.I)` with the pattern compiled once: the match's groups, if any.
macro_rules! search {
    ($pattern:literal, $text:expr) => {
        $crate::re!(concat!("(?i)", $pattern)).captures($text)
    };
}

/// `bool(re.search(pattern, text, re.I))`.
macro_rules! found {
    ($pattern:literal, $text:expr) => {
        $crate::re!(concat!("(?i)", $pattern)).is_match($text)
    };
}

/// `_int(pattern, text)` with the pattern compiled once.
macro_rules! int {
    ($pattern:literal, $text:expr) => {
        match search!($pattern, $text) {
            Some(m) => to_int(&m[1]),
            None => 0,
        }
    };
}

/// `int(digits)` of what `\d+` matched.
fn to_int(digits: &str) -> i64 {
    digits.parse().unwrap_or(0)
}

crate::defaults! {
    /// What a card does, as read from its text. A plain value: `CardInfo { cost: 0, ..c.clone() }` for `dataclasses.replace(c, cost=0)`.
    /// `CardInfo::default()` is all zeros, empty and false but for `aim_times` (1) and `play_limit` (-1), the two defaults that are neither.
    #[derive(Clone, Debug, PartialEq)]
    pub struct CardInfo {
        pub index: i64,
        pub name: String,
        /// Attack, Skill, Power, Status, Curse
        pub r#type: String,
        /// energy; X-cost cards report -1
        pub cost: i64,
        pub star_cost: i64,
        /// per hit
        pub damage: i64,
        pub hits: i64,
        /// "If the enemy is Vulnerable, hits twice."
        pub double_if_vulnerable: bool,
        pub aoe: bool,
        pub random_target: bool,
        pub block: i64,
        pub vulnerable: i64,
        pub weak: i64,
        pub draw: i64,
        pub energy_gain: i64,
        pub hp_loss: i64,
        pub needs_target: bool,
        pub playable: bool,
        /// the text says it can kill us outright ("... die.")
        pub deadly: bool,
        /// stops us gaining Block afterwards
        pub block_lock: bool,
        /// False when no effect could be parsed from the text
        pub known: bool,
        /// Strength we gain when it is played
        pub strength: i64,
        /// ... and keep after this turn
        pub strength_lasts: bool,
        /// "Enemy loses N Strength this turn."
        pub enemy_strength_loss: i64,
        /// ... or for good, without the "this turn" (Shared Fate, Resonance)
        pub enemy_strength_lasts: bool,
        /// card id without the upgrade mark, e.g. DEMON_FORM
        pub key: String,
        /// afflicted with Bound: only one such card can be played per turn
        pub bound: bool,
        // --- other characters' mechanics
        /// Poison applied (per application)
        pub poison: i64,
        /// ... only to an enemy that already has Poison (Bubble Bubble: nothing otherwise)
        pub poison_needs: bool,
        /// Doom applied
        pub doom: i64,
        /// Shivs (0-cost 4 damage cards) added to the hand
        pub shivs: i64,
        /// Stars gained (the Regent's second resource)
        pub star_gain: i64,
        /// damage added to the Sovereign Blade for the fight
        pub forge: i64,
        /// HP given to Osty, who takes hits for the Necrobinder
        pub summon: i64,
        /// Dexterity gained
        pub dexterity: i64,
        pub dexterity_lasts: bool,
        /// orbs channeled, e.g. ["LIGHTNING", "FROST"] (the original: a tuple of names; empty for none)
        pub channel: Vec<String>,
        /// times the rightmost orb is evoked
        pub evoke: i64,
        /// energy gained next turn
        pub next_energy: i64,
        /// extra cards drawn next turn
        pub next_draw: i64,
        /// Block gained next turn
        pub next_block: i64,
        /// "Retain your Hand this turn": ["ANY"], or the Melodies it needs (Ki Rest: "If you have a Wah Melody or Cho Melody") (the original: a tuple of names; empty for none)
        pub keeps_hand: Vec<String>,
        /// "Enemy loses N HP." (ignores Block)
        pub enemy_hp_loss: i64,
        /// Souls added to the draw pile (each: draw 2 cards)
        pub souls: i64,
        /// times every orb is evoked (Shatter)
        pub evoke_all: i64,
        /// cannot be played right now only for want of energy or Stars
        pub short: bool,
        /// "Osty deals N damage": nothing happens while Osty is dead
        pub osty_attack: bool,
        /// Bone Shards: Osty dies, and with him the HP that stood in front of us
        pub osty_dies: bool,
        /// End of Days: enemies with as much Doom as HP die at once, not at the end of their turn
        pub doom_kills: bool,
        /// Debris (1 energy: Exhaust) it adds to our hand; -1: as many as the hand has room for (Crash Landing)
        pub debris: i64,
        /// other statuses it puts in our piles (Wild Strike's Wound, Fight Through's two, Overclock's Burn, Turbo's Void)
        pub junk: i64,
        /// Strength it gives the enemy for good (Fight Me: "The enemy gains 1 Strength")
        pub enemy_strength_gain: i64,
        /// HP it gives back when played (Not Yet)
        pub heal: i64,
        /// gone for the fight if it is still in the hand when the turn ends
        pub ethereal: bool,
        /// "End your turn." (Void Form), "Exhaust your Hand." (Fiend Fire, Stoke): nothing in the hand can be played after it
        pub ends_turn: bool,
        /// times it is played again by itself ("Replay 1": Sword Sage's Sovereign Blade, Hidden Gem, Transfigure)
        pub replay: i64,
        /// "You can only play 1 more card this turn." (Encore): cards allowed after it (-1: no limit)
        pub play_limit: i64 = -1,
        // --- the Fool's Arrows: a card that Shoots needs an Arrow not standing in an enemy, or it does nothing
        pub shoots: bool,
        /// "Aim affects this card 2 times" (Heavy Shot): the Aim in its text counts this often
        pub aim_times: i64 = 1,
        /// Aim gained ("Your next Arrow deals more damage"), per energy on an X card (Brace)
        pub aim: i64,
        /// Stretch gained ("The next time you gain Block from a card, gain additional Block")
        pub stretch: i64,
        /// Nail: "If the enemy is Impaled, Deal 20 damage ...": nothing otherwise
        pub impaled_only: bool,
        /// (energy, cards drawn) it adds when its target is Impaled (Bow Smack, Ki Hop) (the original: that pair, or the empty tuple: None here)
        pub on_impaled: Option<(i64, i64)>,
        /// Rain Shot: "Damage ALL other enemies equal to the damage dealt": the same again on each other enemy
        pub splash: bool,
        /// Blossom Shot: "gain Stretch equal to the damage dealt" (what its hits did, as Stretch)
        pub stretch_dealt: bool,
        /// takes our Arrow back out of its (Impaled) target: Uproot, Recall
        pub dislodge: bool,
        /// Wah Hammer: "Deals double damage to Impaled enemies"
        pub impaled_double: bool,
        /// Cho Firm: "If you have an Arrow, gain Block an additional time"
        pub arrow_block: bool,
        /// the Melodies playing it gives for the turn: the Wah, Cho and Ki in its name (the original: a tuple of names, in MELODIES' order)
        pub melody: Vec<String>,
        /// "Hits an additional time for each Melody you have" (its text counts those there are)
        pub melody_hits: bool,
        /// "Gain 5 Block for each Melody you have" (Harmonic Shield): Block a Melody
        pub melody_block: i64,
        /// Wah Blast: "If you have a Cho Melody or Ki Melody, hits twice" (the original: a tuple of names; empty for none)
        pub melody_twice: Vec<String>,
        /// Ki Verse: "The next (2) "Wah" or "Cho" card(s) you play this turn ... played an additional time" (before v0.114: "Wah", "Cho", or "Ki")
        pub verse: i64,
        /// the first game's Watcher: the stance the card puts her in (WRATH, CALM, DIVINITY, EXIT; games/sts1/stance.rs)
        pub stance: String,
        /// Simmering Fury: "At the start of your next turn, enter Wrath" (the next turn begins in it, as one ended in Wrath)
        pub wrath_next: bool,
        /// a Power's energy taken away at the start of every turn from now on (the first game's Fasting)
        pub energy_drain: i64,
        /// the first game's Watcher: Mantra gained (at 10 she enters Divinity)
        pub mantra: i64,
        /// the Fool's Sadness it puts in the hand when played (Sorrowful Strike 2, Mourning Blow itself), less those it turns into Joy
        pub sadness: i64,
        /// a Power's (Sadness, cards drawn) added at the start of every turn: Ennui (1, 2); Ecstasy's Joy (-1, 0) (fight/fool/sadness.rs) (the original: that pair, or the empty tuple: None here)
        pub sad_turn: Option<(i64, i64)>,
    }
}

impl CardInfo {
    pub fn is_x(&self) -> bool {
        self.cost < 0
    }

    pub fn total_damage(&self) -> i64 {
        self.damage * self.hits
    }

    /// Every field by name, in the order above, as `dataclasses.asdict` gives them (a pair as a list, no pair as an empty one).
    pub fn to_json(&self) -> Value {
        let pair = |p: &Option<(i64, i64)>| match p {
            Some((a, b)) => json!([a, b]),
            None => json!([]),
        };
        let mut out = serde_json::Map::new();
        out.insert("index".into(), json!(self.index));
        out.insert("name".into(), json!(self.name));
        out.insert("type".into(), json!(self.r#type));
        out.insert("cost".into(), json!(self.cost));
        out.insert("star_cost".into(), json!(self.star_cost));
        out.insert("damage".into(), json!(self.damage));
        out.insert("hits".into(), json!(self.hits));
        out.insert("double_if_vulnerable".into(), json!(self.double_if_vulnerable));
        out.insert("aoe".into(), json!(self.aoe));
        out.insert("random_target".into(), json!(self.random_target));
        out.insert("block".into(), json!(self.block));
        out.insert("vulnerable".into(), json!(self.vulnerable));
        out.insert("weak".into(), json!(self.weak));
        out.insert("draw".into(), json!(self.draw));
        out.insert("energy_gain".into(), json!(self.energy_gain));
        out.insert("hp_loss".into(), json!(self.hp_loss));
        out.insert("needs_target".into(), json!(self.needs_target));
        out.insert("playable".into(), json!(self.playable));
        out.insert("deadly".into(), json!(self.deadly));
        out.insert("block_lock".into(), json!(self.block_lock));
        out.insert("known".into(), json!(self.known));
        out.insert("strength".into(), json!(self.strength));
        out.insert("strength_lasts".into(), json!(self.strength_lasts));
        out.insert("enemy_strength_loss".into(), json!(self.enemy_strength_loss));
        out.insert("enemy_strength_lasts".into(), json!(self.enemy_strength_lasts));
        out.insert("key".into(), json!(self.key));
        out.insert("bound".into(), json!(self.bound));
        out.insert("poison".into(), json!(self.poison));
        out.insert("poison_needs".into(), json!(self.poison_needs));
        out.insert("doom".into(), json!(self.doom));
        out.insert("shivs".into(), json!(self.shivs));
        out.insert("star_gain".into(), json!(self.star_gain));
        out.insert("forge".into(), json!(self.forge));
        out.insert("summon".into(), json!(self.summon));
        out.insert("dexterity".into(), json!(self.dexterity));
        out.insert("dexterity_lasts".into(), json!(self.dexterity_lasts));
        out.insert("channel".into(), json!(self.channel));
        out.insert("evoke".into(), json!(self.evoke));
        out.insert("next_energy".into(), json!(self.next_energy));
        out.insert("next_draw".into(), json!(self.next_draw));
        out.insert("next_block".into(), json!(self.next_block));
        out.insert("keeps_hand".into(), json!(self.keeps_hand));
        out.insert("enemy_hp_loss".into(), json!(self.enemy_hp_loss));
        out.insert("souls".into(), json!(self.souls));
        out.insert("evoke_all".into(), json!(self.evoke_all));
        out.insert("short".into(), json!(self.short));
        out.insert("osty_attack".into(), json!(self.osty_attack));
        out.insert("osty_dies".into(), json!(self.osty_dies));
        out.insert("doom_kills".into(), json!(self.doom_kills));
        out.insert("debris".into(), json!(self.debris));
        out.insert("junk".into(), json!(self.junk));
        out.insert("enemy_strength_gain".into(), json!(self.enemy_strength_gain));
        out.insert("heal".into(), json!(self.heal));
        out.insert("ethereal".into(), json!(self.ethereal));
        out.insert("ends_turn".into(), json!(self.ends_turn));
        out.insert("replay".into(), json!(self.replay));
        out.insert("play_limit".into(), json!(self.play_limit));
        out.insert("shoots".into(), json!(self.shoots));
        out.insert("aim_times".into(), json!(self.aim_times));
        out.insert("aim".into(), json!(self.aim));
        out.insert("stretch".into(), json!(self.stretch));
        out.insert("impaled_only".into(), json!(self.impaled_only));
        out.insert("on_impaled".into(), pair(&self.on_impaled));
        out.insert("splash".into(), json!(self.splash));
        out.insert("stretch_dealt".into(), json!(self.stretch_dealt));
        out.insert("dislodge".into(), json!(self.dislodge));
        out.insert("impaled_double".into(), json!(self.impaled_double));
        out.insert("arrow_block".into(), json!(self.arrow_block));
        out.insert("melody".into(), json!(self.melody));
        out.insert("melody_hits".into(), json!(self.melody_hits));
        out.insert("melody_block".into(), json!(self.melody_block));
        out.insert("melody_twice".into(), json!(self.melody_twice));
        out.insert("verse".into(), json!(self.verse));
        out.insert("stance".into(), json!(self.stance));
        out.insert("wrath_next".into(), json!(self.wrath_next));
        out.insert("energy_drain".into(), json!(self.energy_drain));
        out.insert("mantra".into(), json!(self.mantra));
        out.insert("sadness".into(), json!(self.sadness));
        out.insert("sad_turn".into(), pair(&self.sad_turn));
        Value::Object(out)
    }
}

pub const MELODIES: [&str; 3] = ["WAH", "CHO", "KI"];

/// The Fool's Melodies a card gives when played ("Wah Melody: activates if you played a "Wah" card this turn"): the words of its name.
pub fn melodies(name: &str) -> Vec<String> {
    let upper = name.to_uppercase();
    let words: Vec<&str> = crate::re!(r"[A-Z]+").find_iter(&upper).map(|m| m.as_str()).collect();
    MELODIES.iter().filter(|m| words.contains(m)).map(|m| m.to_string()).collect()
}

/// A card's name or id as the tables spell it: DEMON_FORM for "Demon Form+".
pub fn key_of(name: &str) -> String {
    crate::re!(r"[^A-Z0-9]+").replace_all(name.to_uppercase().trim_end_matches('+'), "_").trim_matches('_').to_string()
}

// A status a card puts in one of our piles: a dead draw for the rest of the fight (Debris, which can be played, is counted apart).
pub fn _JUNK_ADDED() -> &'static regex::Regex {
    crate::re!(r"(?i)\b(?:add|shuffle)\s+(a|an|\d+)\s+(?:wound|dazed|slimed|burn|void)s?\b[^.]*?\binto\s+your\s+(?:discard\s+pile|draw\s+pile|hand)")
}

/// `int(group 1)` of the first match of `pattern` (case ignored) in `text`, 0 for none.
pub fn _int(pattern: &str, text: &str) -> i64 {
    match base::re::cached_i(pattern).captures(text) {
        Some(m) => to_int(&m[1]),
        None => 0,
    }
}

/// Energy gained, as the game writes it: "Gain" and one icon per point, or a number and one icon ("Gain 4[icon]").
pub fn _icons(text: &str) -> i64 {
    if let Some(m) = search!(r"gain\s+(\d+)\s*\[[a-z_]*energy_icon\.png\]", text) {
        return to_int(&m[1]);
    }
    match search!(r"gain\s+((?:\[[a-z_]*energy_icon\.png\]\s*)+)", text) {
        Some(m) => _ENERGY_ICON().find_iter(&m[1]).count() as i64,
        None => 0,
    }
}

/// The energy a card costs as the game sends it (a number, a string of digits, "X": -1; anything else 0).
pub fn _cost(raw: &Value) -> i64 {
    match raw {
        Value::Null => 0,
        Value::Number(n) => match n.as_i64() {
            Some(n) => n.max(0),
            None => 0, // (a float reads "1.0": not an int)
        },
        Value::String(s) => {
            let s = s.trim();
            if s.to_uppercase() == "X" {
                return -1;
            }
            s.parse::<i64>().map(|n| n.max(0)).unwrap_or(0)
        }
        _ => 0,
    }
}

/// "Retain your Hand this turn": ["ANY"]; behind a Melody ("If you have a Wah Melody or Cho Melody, Retain ..."): those Melodies.
pub fn _keeps_hand(text: &str) -> Vec<String> {
    let Some(m) = search!(r"(?:if\s+you\s+have\s+([^,.]*?melody[^,.]*?),\s*)?retain\s+your\s+hand", text) else { return Vec::new() };
    match m.get(1).map(|g| g.as_str()).filter(|g| !g.is_empty()) {
        Some(needs) => crate::re!(r"(?i)(\w+)\s+melody").captures_iter(needs).map(|w| w[1].to_uppercase()).collect(),
        None => vec!["ANY".to_string()],
    }
}

/// Extra times the card plays itself: "Replay 2." in its own text, or the keyword alone (not "Sovereign Blade gains Replay 1.").
pub fn _replay(card: &Value, text: &str) -> i64 {
    if let Some(m) = search!(r"(?:^|[.\n]\s*)replay\s+(\d+)", text) {
        return to_int(&m[1]);
    }
    if found!(r"replay", text) {
        return 0;
    }
    for k in card["keywords"].list() {
        if k.is_object() && base::str_of(&k["name"]).to_lowercase() == "replay" {
            let times = int!(r"additional\s+(\d+)\s+times", &k["description"].text());
            return if times != 0 { times } else { 1 };
        }
    }
    0
}

pub fn is_basic(card: &Value) -> bool {
    crate::re!(r"(?i)^(strike|defend)(_[a-z]+)?\n?$").is_match(&card["id"].or(&card["name"]).text())
}

pub fn is_junk(card: &Value) -> bool {
    matches!(base::str_of(&card["type"]).as_str(), "Status" | "Curse")
}

/// What sort of card this is to a deck whose turns run out of energy before they run out of cards (learn.rs keeps a
/// record of each): "energy" gives some, now or next turn; "free" is an attack that costs none; "draw" costs some and
/// brings cards and little else; "" for every other card (free Skills too).
pub fn kind(card: &Value) -> &'static str {
    let ctype = base::str_of(&card["type"]);
    if !matches!(ctype.as_str(), "Attack" | "Skill") {
        return "";
    }
    let info = parse(card);
    if (info.energy_gain != 0 || info.next_energy != 0) && info.play_limit < 0 {
        // (Encore's energy pays for one card)
        return "energy";
    }
    if info.cost == 0 && info.star_cost == 0 {
        return if ctype == "Attack" && info.total_damage() != 0 { "free" } else { "" };
    }
    let put = match search!(r"put (?:up to )?(a|\d+) cards? from your discard pile into your hand", &card["description"].text()) {
        None => 0,
        Some(m) if m[1].to_lowercase() == "a" => 1,
        Some(m) => to_int(&m[1]),
    };
    let draws = info.draw + 2 * info.souls + put;
    let does = info.total_damage() + info.doom + 3 * info.poison + 4 * info.shivs >= 8 || info.block >= 5;
    if draws >= 2 && !does {
        "draw"
    } else {
        ""
    }
}

// Sentences about what happens later or only sometimes ("Whenever you gain Block, deal 6 damage ..."): not what the card does when played.
pub fn _LATER() -> &'static regex::Regex {
    crate::re!(r"(?i)^(whenever|every time|at the (start|end) of|next turn|if you|if your|if this|if fatal|the (first|next) time|this turn, your next)")
}

pub fn _ENERGY_ICON() -> &'static regex::Regex {
    crate::re!(r"(?i)\[[a-z_]*energy_icon\.png\]")
}

/// `re.split(r"(?<=[.!?])\s+", text)`: the text cut at the white space that follows a full stop, "!" or "?".
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut prev) = (0, ' ');
    let mut chars = text.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        if ch.is_whitespace() && matches!(prev, '.' | '!' | '?') {
            let mut end = i + ch.len_utf8();
            while let Some(&(j, next)) = chars.peek() {
                if !next.is_whitespace() {
                    break;
                }
                end = j + next.len_utf8();
                chars.next();
            }
            out.push(&text[start..i]);
            start = end;
        }
        prev = ch;
    }
    out.push(&text[start..]);
    out
}

/// The part of a card's text that happens as it is played.
pub fn immediate_text(text: &str) -> String {
    let mut keep: Vec<&str> = Vec::new();
    for sentence in sentences(text) {
        let s = sentence.trim();
        if s.is_empty() || _LATER().is_match(s) {
            continue;
        }
        // "Gain 12 Block. Whenever you are attacked this turn, deal 4 damage back."
        keep.push(if keep.is_empty() {
            s
        } else {
            match crate::re!(r"(?i),?\s+(?:whenever|if you|if your)\b").find(s) {
                Some(m) => &s[..m.start()],
                None => s,
            }
        });
    }
    keep.join(" ")
}

thread_local! {
    static _PARSED: RefCell<HashMap<String, CardInfo>> = RefCell::new(HashMap::new());
    static _PARSED_KEY: RefCell<String> = const { RefCell::new(String::new()) };
}

/// One part of the key `parse` remembers a card by: its type and value, so that no two different cards read alike.
fn key_part(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push('n'),
        Value::Bool(b) => out.push(if *b { 't' } else { 'f' }),
        Value::Number(n) => {
            let _ = write!(out, "#{n}");
        }
        Value::String(s) => {
            let _ = write!(out, "s{}:", s.len());
            out.push_str(s);
        }
        Value::Array(a) => {
            let _ = write!(out, "[{}:", a.len());
            for x in a {
                key_part(x, out);
            }
        }
        Value::Object(m) => {
            let _ = write!(out, "{{{}:", m.len());
            for (k, x) in m {
                let _ = write!(out, "{}:", k.len());
                out.push_str(k);
                key_part(x, out);
            }
        }
    }
    out.push('\u{1f}');
}

/// What a card does, read from its text (memoised: the planner reads the same hand thousands of times a turn).
pub fn parse(card: &Value) -> CardInfo {
    _PARSED_KEY.with(|buffer| {
        let mut key = buffer.borrow_mut();
        key.clear();
        match card.get("index") {
            Some(index) => key_part(index, &mut key),
            None => key.push_str("#0\u{1f}"),
        }
        for field in ["id", "name", "type", "cost", "star_cost", "description", "target_type"] {
            key_part(&card[field], &mut key);
        }
        match card.get("can_play") {
            Some(can) => key_part(can, &mut key),
            None => key.push_str("t\u{1f}"),
        }
        key_part(&card["unplayable_reason"], &mut key);
        match &card["keywords"] {
            Value::Array(kws) => {
                let _ = write!(key, "({}:", kws.len());
                for k in kws {
                    if k.is_object() {
                        key.push('k');
                        key_part(&k["name"], &mut key);
                        key_part(&k["description"], &mut key);
                    } else {
                        key_part(k, &mut key);
                    }
                }
            }
            kws => key_part(kws, &mut key),
        }
        _PARSED.with(|parsed| {
            let mut parsed = parsed.borrow_mut();
            if let Some(info) = parsed.get(key.as_str()) {
                return info.clone();
            }
            if parsed.len() > 100_000 {
                parsed.clear();
            }
            let info = _parse(card);
            parsed.insert(key.clone(), info.clone());
            info
        })
    })
}

pub fn _parse(card: &Value) -> CardInfo {
    let (full_text, ctype, target) = (card["description"].text(), card["type"].text(), card["target_type"].text());
    let full_text = full_text.as_str();
    // A Power does nothing the moment it is played; its text is all "later".
    let mut text = if ctype == "Power" { String::new() } else { immediate_text(full_text) };
    // "If the enemy is Impaled, ..." (the Fool's Arrow stands in it): Nail does nothing without one; Bow Smack's energy and Ki Hop's draw come only with one.
    let (mut impaled_only, mut on_impaled) = (false, (0, 0));
    let cut = match search!(r"if\s+the\s+enemy\s+is\s+impaled,?\s*((?:\[[^\]]*\]|[^.])*)\.?", &text) {
        // (an icon's ".png" is not the end)
        Some(m) => {
            if found!(r"deal\s+\d+\s+damage|\bapply\b|\bdislodge\b", &m[1]) {
                impaled_only = true; // (Uproot: "Dislodge and apply 3 Vulnerable"; Recall: "Dislodge and put a card ...")
                None
            } else {
                on_impaled = (_icons(&m[1]), int!(r"draw\s+(\d+)\s+card", &m[1]));
                Some(m.get(0).unwrap().range())
            }
        }
        None => None,
    };
    if let Some(range) = cut {
        text.replace_range(range, "");
    }
    // "Aim affects this card 2 times" (Heavy Shot) is one hit with the Aim counted twice, not 2 hits; so is the first game's Heavy Blade's Strength.
    let mut aim_times = 1;
    let cut = search!(r"(aim|strength)\s+affects\s+this\s+card\s+(?:(\d+)\s+times?|twice)\.?", &text).map(|m| {
        aim_times = if m[1].to_lowercase() == "aim" { m.get(2).map(|n| to_int(n.as_str())).unwrap_or(2) } else { 1 };
        m.get(0).unwrap().range()
    });
    if let Some(range) = cut {
        text.replace_range(range, "");
    }
    let text = text.as_str();
    // "Next turn, gain 4 Block." is read separately below, not as this turn's.
    // (and "At the start of your next turn, ..." as next turn's too: Simmering Fury's 2 cards)
    let next_text = if ctype == "Power" {
        String::new()
    } else {
        sentences(full_text).into_iter().filter(|x| found!(r"^\s*(?:next turn|at the start of your next turn)\b", x)).collect::<Vec<_>>().join(" ")
    };
    let next_text = next_text.as_str();
    let mut damage = int!(r"deals?\s+(\d+)\s+damage", text);
    if damage == 0 {
        damage = int!(r"\(deals\s+(\d+)\s+damage\)", text); // (or "Deal damage equal to your Block. (Deals 14 damage)")
    }
    let (mut hits, mut double_if_vulnerable) = (1, false);
    if damage != 0 {
        let mut plain = text.to_string(); // (a conditional second hit must not count as an unconditional one)
        if let Some(m) = crate::re!(r"(?i)if\s+the\s+enemy\s+is\s+vulnerable,?\s+hits\s+twice\.?").find(text) {
            double_if_vulnerable = true;
            plain.replace_range(m.range(), "");
        }
        hits = int!(r"(\d+)\s+times", &plain);
        if hits == 0 {
            hits = 1;
        }
        // Only where it counts the hits (Debilitate: "Vulnerable and Weak are twice as effective" is one hit).
        if found!(r"(?:damage[^.]*|\bhits?\s+)\btwice\b", &plain) {
            hits = _WORD_NUM[0].1;
        }
        if found!(r"(?:damage[^.]*|\bhits?\s+)\bthrice\b", &plain) {
            hits = _WORD_NUM[1].1;
        }
    }
    if let Some(m) = search!(r"\(hits\s+(\d+)\s+times?\)", full_text) {
        if damage != 0 {
            // "Deal 5 damage for each Skill in your Hand. (Hits 2 times)"
            hits = to_int(&m[1]);
            if hits == 0 {
                damage = 0;
            }
        }
    }
    let mut block = int!(r"gain\s+(\d+)\s+block", text);
    if block != 0 && found!(r"gain\s+\d+\s+block\s+twice", text) {
        block *= _WORD_NUM[0].1; // "Gain 5 Block twice" (Cho Limbs): two gains of 5, read as one
    }
    let melody_block = if found!(r"block\s+for\s+each\s+melody", text) { block } else { 0 };
    if let Some(m) = search!(r"\(gain\s+(\d+)\s+block\)", text) {
        block = to_int(&m[1]); // "Gain 5 Block for each Melody you have. (Gain 0 Block)": what it gives now
    }
    if block == 0 && damage != 0 && found!(r"gain\s+block\s+equal\s+to\s+(?:the\s+)?damage\s+dealt", text) {
        block = damage * hits; // Fisticuffs
    }
    let mut vulnerable = int!(r"apply\s+(\d+)\s+vulnerable", text);
    if vulnerable == 0 {
        vulnerable = int!(r"(\d+)\s+(?:weak\s+and\s+)?vulnerable", text);
    }
    let mut weak = int!(r"apply\s+(\d+)\s+weak", text);
    if weak == 0 {
        weak = int!(r"(\d+)\s+(?:vulnerable\s+and\s+)?weak", text);
    }
    let mut draw = int!(r"draw\s+(\d+)\s+card", text);
    if draw == 0 {
        draw = found!(r"draw\s+a\s+card", text) as i64;
    }
    let mut energy_gain = _icons(text); // the game writes energy as icons, one per point
    if energy_gain == 0 {
        energy_gain = int!(r"gain\s+(\d+)\s+energy", text);
    }
    if found!(r"when this card is exhausted,\s*gain", text) {
        energy_gain = 0; // Drum of Battle: playing it only draws; the energy comes if something Exhausts it
    }
    let hp_loss = int!(r"lose\s+(\d+)\s+hp", text);
    let stat_text = if ctype == "Power" && !_LATER().is_match(full_text.trim()) { full_text } else { text };

    /// (Strength or Dexterity gained, whether it is kept after this turn).
    fn stat(m: Option<regex::Captures>) -> (i64, bool) {
        match m {
            Some(m) => (to_int(&m[1]), m.get(2).is_none()),
            None => (0, false),
        }
    }

    let (mut strength, mut strength_lasts) = stat(search!(r"gain\s+(\d+)\s+strength(\s+this\s+turn)?", stat_text));
    let (dexterity, dexterity_lasts) = stat(search!(r"gain\s+(\d+)\s+dexterity(\s+this\s+turn)?", stat_text));
    // "Lose 2 Strength." as the price of the card (Friendship, Shared Fate): for the rest of the fight.
    if strength == 0 {
        if let Ok(Some(m)) = crate::fre!(r"(?i)(?:^|\.\s+)lose\s+(\d+)\s+strength\b(?!\s+this\s+turn)").captures(stat_text) {
            (strength, strength_lasts) = (-to_int(&m[1]), true);
        }
    }
    let mut enemy_strength_loss = int!(r"enem(?:y|ies)\s+loses?\s+(\d+)\s+strength", text);
    let enemy_strength_lasts = enemy_strength_loss != 0 && !found!(r"enem(?:y|ies)\s+loses?\s+\d+\s+strength\s+this\s+turn", text);
    // "X" amounts on an X-cost card (Malaise: "Enemy loses X Strength. Apply X Weak."): 1 per energy spent.
    if found!(r"enem(?:y|ies)\s+loses?\s+X\s+strength", text) {
        enemy_strength_loss = 1;
    }
    if weak == 0 {
        weak = found!(r"apply\s+X\s+weak", text) as i64;
    }
    if vulnerable == 0 {
        vulnerable = found!(r"apply\s+X\s+vulnerable", text) as i64;
    }
    let times = int!(r"poison\s+to\s+a\s+random\s+enemy\s+(\d+)\s+times", text);
    let poison = int!(r"apply\s+(\d+)\s+poison", text) * if times != 0 { times } else { 1 };
    let poison_needs = poison != 0 && found!(r"if\s+the\s+enemy\s+has\s+poison", text);
    let mut doom = int!(r"apply\s+(\d+)\s+doom", text);
    if doom == 0 && found!(r"apply\s+doom\s+equal\s+to\s+damage\s+dealt", text) {
        doom = int!(r"deals?\s+(\d+)\s+damage", text); // Blight Strike: as much Doom as it hits for
    }
    let shivs = int!(r"add\s+(\d+)\s+(?:\w+\s+)?shivs?\s+into\s+your\s+hand", text); // ("2 Inky Shivs" too: Blade of Ink)
    let heal = int!(r"\bheal\s+(\d+)\s+hp", text);
    let star_gain = match search!(r"gain\s+(\d+)?\s*((?:\[star_icon\.png\]\s*)+)", text) {
        Some(m) => match m.get(1) {
            Some(n) => to_int(n.as_str()),
            None => m[2].matches("star_icon").count() as i64,
        },
        None => 0,
    };
    let (forge, summon) = (int!(r"forge\s+(\d+)", text), int!(r"summon\s+(\d+)", text));
    let mut channel: Vec<String> = Vec::new();
    for m in crate::re!(r"(?i)channel\s+(\d+)\s+(lightning|frost|dark|plasma|glass)").captures_iter(text) {
        for _ in 0..to_int(&m[1]) {
            channel.push(m[2].to_uppercase());
        }
    }
    if channel.is_empty() {
        if let Some(m) = search!(r"channel\s+X\s+(lightning|frost|dark|plasma|glass)", text) {
            channel = vec![m[1].to_uppercase()]; // Tempest: one per energy spent
        }
    }
    // "Evoke your rightmost Orb twice." / "... 4 times."
    let evoke = if found!(r"\bevoke\s+your", text) {
        if found!(r"evoke[^.]*twice", text) {
            2
        } else {
            match int!(r"evoke[^.]*?(\d+)\s+times", text) {
                0 => 1,
                n => n,
            }
        }
    } else {
        0
    };
    let evoke_all = match search!(r"\bevoke\s+all\s+(?:of\s+)?your\s+orbs(\s+twice)?", text) {
        Some(m) => {
            if m.get(1).is_some() {
                2
            } else {
                1
            }
        }
        None => 0,
    };
    let (next_energy, next_draw, next_block) = (_icons(next_text), int!(r"draw\s+(\d+)\s+cards?", next_text), int!(r"gain\s+(\d+)\s+block", next_text));
    let enemy_hp_loss = int!(r"enemy\s+loses\s+(\d+)\s+hp", text);
    let mut souls = int!(r"add\s+(\d+)\s+souls?", text);
    if souls == 0 {
        souls = found!(r"add\s+a\s+soul", text) as i64;
    }
    let aoe = target == "AllEnemies" || found!(r"all\s+enemies", text);
    let random_target = found!(r"random\s+enemy", text);
    let (aim, stretch) = (int!(r"gain\s+(\d+)\s+aim", text), int!(r"gain\s+(\d+)\s+stretch", text));
    let mantra = int!(r"gain\s+(\d+)\s+mantra", text);
    let stance = match search!(r"\benter\s+(wrath|calm|divinity)\b", text) {
        Some(m) => m[1].to_uppercase(),
        None => {
            if found!(r"\bexit\s+your\s+stance\b", text) {
                "EXIT".to_string()
            } else {
                String::new()
            }
        }
    };
    let known = [damage, block, vulnerable, weak, draw, energy_gain, strength, enemy_strength_loss, poison, doom, shivs, aim, stretch, melody_block, impaled_only as i64]
        .iter()
        .chain([star_gain, forge, summon, dexterity, channel.len() as i64, evoke, evoke_all, next_energy, next_draw, next_block, enemy_hp_loss, souls, heal, !stance.is_empty() as i64, mantra].iter())
        .any(|x| *x != 0);
    let can_play = card.get("can_play").map(|c| c.truthy()).unwrap_or(true);
    let keywords = card["keywords"].list();
    let name = card["name"].or(&card["id"]);
    CardInfo {
        index: card["index"].int(),
        name: if name.truthy() { base::str_of(name) } else { "?".to_string() },
        r#type: ctype.clone(),
        cost: _cost(&card["cost"]),
        star_cost: if card["star_cost"].is_null() || card["star_cost"].as_str() == Some("") { 0 } else { _cost(&card["star_cost"]) },
        key: _card_key(card),
        damage,
        hits,
        double_if_vulnerable,
        aoe,
        random_target,
        block,
        vulnerable,
        weak,
        draw,
        energy_gain,
        hp_loss,
        known,
        strength,
        strength_lasts,
        enemy_strength_loss,
        enemy_strength_lasts,
        poison,
        poison_needs,
        doom,
        shivs,
        star_gain,
        forge,
        summon,
        dexterity,
        dexterity_lasts,
        channel,
        evoke,
        evoke_all,
        next_energy,
        next_draw,
        next_block,
        enemy_hp_loss,
        souls,
        heal,
        needs_target: target == "AnyEnemy",
        playable: can_play,
        deadly: found!(r"\bdie\b", full_text),
        block_lock: found!(r"(cannot|can't|can not)\s+gain\s+(any\s+)?block", full_text),
        osty_attack: found!(r"\bOsty\b[^.]*\bdeals\b|if\s+Osty\s+is\s+alive", full_text),
        // (Hex: "Hexed")
        ethereal: crate::re!(r"\bEthereal\b").is_match(full_text) || keywords.iter().any(|k| k.is_object() && matches!(base::str_of(&k["name"]).to_lowercase().as_str(), "ethereal" | "hexed")),
        ends_turn: found!(r"(?:^|\.\s*)(?:end\s+your\s+turn|exhaust\s+your\s+hand)\.", full_text),
        replay: _replay(card, full_text),
        play_limit: if found!(r"only\s+play\s+\d+\s+more\s+card", full_text) { int!(r"you\s+can\s+only\s+play\s+(\d+)\s+more\s+cards?\s+this\s+turn", full_text) } else { -1 },
        shoots: found!(r"\bshoot\s+(?:an?|\d+|x)\s+arrows?\b", text),
        aim_times,
        aim,
        stretch,
        impaled_only,
        on_impaled: if on_impaled != (0, 0) { Some(on_impaled) } else { None },
        splash: found!(r"damage\s+all\s+other\s+enemies\s+equal\s+to\s+the\s+damage\s+dealt", text),
        stretch_dealt: found!(r"gain\s+stretch\s+equal\s+to\s+the\s+damage\s+dealt", text),
        dislodge: found!(r"\bdislodge\b", text),
        impaled_double: found!(r"double\s+damage\s+to\s+impaled", text),
        arrow_block: found!(r"if\s+you\s+have\s+an\s+arrow,\s*gain\s+block\s+an\s+additional\s+time", full_text), // ("If you ..." is not in `text`)
        melody: melodies(&card["name"].text()),
        melody_block,
        keeps_hand: _keeps_hand(full_text),
        melody_hits: found!(r"additional\s+time\s+for\s+each\s+melody", text),
        melody_twice: match crate::re!(r"(?i)if\s+you\s+have\s+an?\s+[^,.]*melody[^,.]*,\s*hits\s+twice").find(full_text) {
            Some(m) => crate::re!(r"(?i)(\w+)\s+melody").captures_iter(m.as_str()).map(|w| w[1].to_uppercase()).collect(),
            None => Vec::new(),
        },
        energy_drain: if ctype == "Power" { int!(r"gain\s+(\d+)\s+less\s+energy\s+at\s+the\s+start\s+of\s+(?:your|each)\s+turn", full_text) } else { 0 },
        stance,
        wrath_next: found!(r"\bnext\s+turn,\s*enter\s+wrath\b", next_text),
        verse: match search!(r"the\s+next\s+(?:(\d+)\s+)?\W*wah\W+(?:or\s+\W*)?cho\W*(?:,?\s*or\s+\W*ki\W*)?\s*cards?\s+you\s+play\s+this\s+turn\s+(?:is|are)\s+played\s+an\s+additional\s+time", full_text) {
            Some(m) => m.get(1).map(|n| to_int(n.as_str())).unwrap_or(1),
            None => 0,
        },
        // (Sacrifice: "he dies")
        osty_dies: found!(r"\b(?:Osty|he)\s+dies", full_text),
        doom_kills: found!(r"kill\s+enemies\s+with\s+at\s+least\s+as\s+much\s+Doom", full_text),
        debris: if found!(r"fill\s+your\s+hand\s+with\s+debris", text) {
            -1
        } else {
            match int!(r"add\s+(\d+)\s+debris", text) {
                0 => found!(r"add\s+a\s+debris", text) as i64,
                n => n,
            }
        },
        junk: if !matches!(ctype.as_str(), "Power" | "Status" | "Curse") { _JUNK_ADDED().captures_iter(full_text).map(|m| _count(&m[1])).sum() } else { 0 },
        enemy_strength_gain: int!(r"\bthe\s+enem(?:y|ies)\s+gains?\s+(\d+)\s+strength", full_text),
        bound: keywords.iter().any(|k| k.is_object() && base::str_of(&k["name"]).to_lowercase() == "bound"),
        short: !can_play
            && card["unplayable_reason"].truthy()
            && base::str_of(&card["unplayable_reason"]).split(',').all(|r| matches!(r.trim(), "EnergyCostTooHigh" | "StarCostTooHigh")),
        mantra,
        sadness: _sadness(text),
        sad_turn: if ctype == "Power" { _sad_turn(full_text) } else { None },
    }
}

pub fn _count(word: &str) -> i64 {
    if matches!(word.to_lowercase().as_str(), "a" | "an") {
        1
    } else {
        to_int(word)
    }
}

/// Sadness a played card puts in the hand ("Add 2 Sadness into your Hand", "Transform this into Sadness and return it to your
/// Hand"), less what it turns into Joy ("Transform 1 random Sadness in your Hand into Joy").
pub fn _sadness(text: &str) -> i64 {
    if !text.to_lowercase().contains("sadness") {
        return 0;
    }
    let mut added: i64 = crate::re!(r"(?i)add\s+(\d+|an?)\s+sadness\s+(?:in)?to\s+your\s+hand").captures_iter(text).map(|m| _count(&m[1])).sum();
    added += found!(r"transform\s+this\s+into\s+sadness", text) as i64;
    added - crate::re!(r"(?i)transform\s+(\d+)\s+random\s+sadness\s+in\s+your\s+hand\s+into\s+joy").captures_iter(text).map(|m| to_int(&m[1])).sum::<i64>()
}

/// A Power's (Sadness, extra cards drawn) at the start of every turn: Ennui (1, 2), Ecstasy (-1, 0); None for the rest.
pub fn _sad_turn(text: &str) -> Option<(i64, i64)> {
    if let Some(m) = search!(r"at\s+the\s+start\s+of\s+your\s+turn,\s*add\s+(\d+|an?)\s+sadness\s+to\s+your\s+hand(?:\s+and\s+draw\s+(\d+)\s+additional\s+cards?)?", text) {
        return Some((_count(&m[1]), m.get(2).map(|n| to_int(n.as_str())).unwrap_or(0)));
    }
    if let Some(m) = search!(r"at\s+the\s+start\s+of\s+your\s+turn,\s*transform\s+(\d+)\s+random\s+sadness\s+in\s+your\s+hand\s+into\s+joy", text) {
        return Some((-to_int(&m[1]), 0));
    }
    None
}

// A nudge, on the deck_value scale (+1 is about one rarity step), for cards _TIER does not rate whose text understates or overstates them.
#[rustfmt::skip]
pub static _KNOWN: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([
    ("IMMOLATE", 3.0), ("DISARM", 2.0), ("REAPER", 2.0), ("CARNAGE", 1.5), ("SPOT_WEAKNESS", 1.5), ("METALLICIZE", 1.5), ("CLEAVE", 1.0), ("POWER_THROUGH", 1.0), ("GHOSTLY_ARMOR", 1.0), ("SEEING_RED", 1.0), ("INTIMIDATE", 1.0), ("COMBUST", 0.5), ("DOUBLE_TAP", 0.5),
    ("ENTRENCH", -1.5), ("CLASH", -1.0), ("SEARING_BLOW", -1.0), ("RECKLESS_CHARGE", -1.0), ("FIRE_BREATHING", -1.0), ("EVOLVE", -1.0), ("BERSERK", -0.5), ("WARCRY", -0.5), ("WILD_STRIKE", -0.5), ("DUAL_WIELD", -0.5), ("LIMIT_BREAK", -0.5),]));

pub fn _known_nudge(card: &Value) -> f64 {
    if !games::uses_sts2_tables() {
        return 0.0;
    }
    [key_of(&card["id"].text()), key_of(&card["name"].text())].iter().find_map(|k| _KNOWN.get(k.as_str()).copied()).unwrap_or(0.0)
}

// What each card we have met is worth in the deck on the deck_value scale (3 = fine early, 5 = good, 7+ = always), judged by how the
// bot plays it (one turn at a time: build-around cards rate low). Cards not listed fall back to the text estimate in deck_value.
#[rustfmt::skip]
pub static _TIER: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([
    // commons
    ("SHRUG_IT_OFF", 5.5), ("POMMEL_STRIKE", 5.0), ("BREAKTHROUGH", 5.0), ("THUNDERCLAP", 4.5), ("TWIN_STRIKE", 4.0), ("ARMAMENTS", 3.0), ("BLOOD_WALL", 4.0), ("HEADBUTT", 3.5), ("IRON_WAVE", 3.5), ("CINDER", 4.0), ("TRUE_GRIT", 3.5), ("TAUNT", 3.5), ("ANGER", 3.0),
    ("SETUP_STRIKE", 3.0), ("SWORD_BOOMERANG", 3.0), ("TREMBLE", 2.5), ("PERFECTED_STRIKE", 2.5), ("BODY_SLAM", 1.5), ("HAVOC", 1.0),
    // uncommons
    ("SHOCKWAVE", 7.5), ("UPPERCUT", 6.0), ("FLAME_BARRIER", 6.0), ("INFLAME", 6.5), ("HEMOKINESIS", 6.0), ("BATTLE_TRANCE", 5.5), ("WHIRLWIND", 5.0), ("ULTIMATE_STRIKE", 5.0), ("ULTIMATE_DEFEND", 5.0), ("THRUMMING_HATCHET", 5.0), ("STONE_ARMOR", 5.0),
    ("DRAMATIC_ENTRANCE", 5.0), ("THE_BOMB", 5.0), ("BLUDGEON", 5.5), ("FIGHT_ME", 4.5), ("OMNISLICE", 4.5), ("DISMANTLE", 4.0), ("RAMPAGE", 4.0), ("FLASH_OF_STEEL", 4.0), ("FINESSE", 4.0), ("SALVO", 4.0), ("SEEKER_STRIKE", 4.0), ("SPITE", 3.5),
    ("UNRELENTING", 3.5), ("EVIL_EYE", 3.5), ("BURNING_PACT", 3.5), ("EQUILIBRIUM", 3.5), ("DARK_SHACKLES", 3.5), ("STOMP", 2.5), ("HOWL_FROM_BEYOND", 3.5), ("FISTICUFFS", 3.5), ("VOLLEY", 3.5), ("BULLY", 3.0), ("COLOSSUS", 3.0), ("FASTEN", 3.0),
    ("BLOODLETTING", 3.0), ("PRODUCTION", 3.0), ("PILLAGE", 3.0), ("VICIOUS", 2.5), ("PANACHE", 2.5), ("RAGE", 2.5), ("INFERNAL_BLADE", 2.5), ("FORGOTTEN_RITUAL", 2.5), ("DISCOVERY", 2.5), ("THINKING_AHEAD", 2.5), ("PANIC_BUTTON", 2.5), ("ASHEN_STRIKE", 2.5),
    ("MIND_BLAST", 2.5), ("SECOND_WIND", 2.5), ("FEEL_NO_PAIN", 2.0), ("JUGGLING", 2.0), ("STRATAGEM", 2.0), ("STAMPEDE", 2.0), ("EXPECT_A_FIGHT", 2.0), ("JACK_OF_ALL_TRADES", 2.0), ("IMPATIENCE", 2.0), ("RUPTURE", 1.5), ("RESTLESSNESS", 1.5), ("PURITY", 1.5),
    ("PROLONG", 1.0),
    // rares and event cards
    ("IMPERVIOUS", 8.0), ("OFFERING", 7.5), ("DEMON_FORM", 8.0), ("PYRE", 7.0), ("EXTERMINATE", 7.0), ("FEED", 6.5), ("CRIMSON_MANTLE", 6.0), ("ROLLING_BOULDER", 6.0), ("GIANT_ROCK", 6.0), ("CONFLAGRATION", 5.5), ("HAND_OF_GREED", 5.5),
    ("MASTER_OF_STRATEGY", 5.0), ("MANGLE", 4.5), ("AGGRESSION", 4.5), ("MAYHEM", 4.5), ("NOT_YET", 4.5), ("BOLAS", 4.5), ("UNMOVABLE", 4.0), ("BRAND", 4.0), ("FIEND_FIRE", 4.0), ("JUGGERNAUT", 3.5), ("HELLRAISER", 3.5), ("ONE_TWO_PUNCH", 3.5), ("THRASH", 3.5),
    ("BARRICADE", 3.0), ("DARK_EMBRACE", 3.0), ("CASCADE", 3.0), ("SECRET_WEAPON", 3.0), ("SECRET_TECHNIQUE", 3.0), ("HIDDEN_GEM", 2.5), ("GOLD_AXE", 2.5), ("METAMORPHOSIS", 2.5), ("PRIMAL_FORCE", 2.0), ("STOKE", 2.0), ("THE_GAMBIT", -6.0),
    // --- The Silent
    ("ADRENALINE", 7.0), ("NOXIOUS_FUMES", 7.5), ("FOOTWORK", 6.0), ("AFTERIMAGE", 6.0), ("LEG_SWEEP", 6.0), ("BACKFLIP", 5.5), ("DASH", 5.5), ("SERPENT_FORM", 5.5), ("ASSASSINATE", 5.5), ("DEADLY_POISON", 5.5), ("BOUNCING_FLASK", 5.5), ("ACROBATICS", 5.0),
    ("DAGGER_SPRAY", 5.0), ("BLADE_DANCE", 5.5), ("PIERCING_WAIL", 5.0), ("MALAISE", 5.0), ("ECHOING_SLASH", 5.0), ("NEUTRALIZE", 4.0), ("SURVIVOR", 3.5), ("POISONED_STAB", 4.5), ("HAZE", 4.5), ("DAGGER_THROW", 4.5), ("SUCKER_PUNCH", 4.5),
    ("CLOAK_AND_DAGGER", 4.5), ("DODGE_AND_ROLL", 4.5), ("PREDATOR", 4.5), ("BACKSTAB", 4.5), ("BLUR", 4.5), ("SKEWER", 4.5), ("FLICK_FLACK", 4.5), ("WELL_LAID_PLANS", 4.5), ("FAN_OF_KNIVES", 4.5), ("ACCURACY", 5.5), ("INFINITE_BLADES", 5.5),
    ("TOOLS_OF_THE_TRADE", 4.0), ("ENVENOM", 4.0), ("ABRASIVE", 4.0), ("SNAKEBITE", 4.0), ("OUTBREAK", 4.0), ("DEFLECT", 4.0), ("THE_HUNT", 4.0), ("POUNCE", 4.0), ("UP_MY_SLEEVE", 4.0), ("EXPOSE", 4.0), ("SPEEDSTER", 3.5), ("BUBBLE_BUBBLE", 3.5),
    ("ESCAPE_PLAN", 3.5), ("EXPERTISE", 3.5), ("BURST", 3.5), ("PRECISE_CUT", 3.5), ("STRANGLE", 3.5), ("RICOCHET", 3.5), ("LEADING_STRIKE", 3.5), ("ACCELERANT", 3.0), ("TRACKING", 3.0), ("SLICE", 3.0), ("FINISHER", 3.0), ("BULLET_TIME", 3.0),
    ("STORM_OF_STEEL", 3.0), ("MEMENTO_MORI", 3.0), ("PINPOINT", 3.0), ("UNTOUCHABLE", 3.0), ("SHADOWMELD", 3.0), ("CORROSIVE_WAVE", 3.0), ("ANTICIPATE", 3.0), ("PREPARED", 2.5), ("CALCULATED_GAMBLE", 2.5), ("FLECHETTES", 2.5), ("NIGHTMARE", 2.5), ("MURDER", 2.5),
    ("MIRAGE", 2.5), ("SIDESTEP", 2.5), ("TACTICIAN", 2.0), ("SHADOW_STEP", 2.0), ("GRAND_FINALE", 1.0),
    // --- The Regent
    ("COMET", 6.0), ("GAMMA_BLAST", 5.5), ("DYING_STAR", 5.5), ("ASTRAL_PULSE", 5.0), ("CRUSH_UNDER", 5.0), ("GATHER_LIGHT", 5.0), ("PROPHESIZE", 5.0), ("PARTICLE_WALL", 5.0), ("FALLING_STAR", 4.5), ("VENERATE", 3.5), ("SOLAR_STRIKE", 4.5),
    ("CELESTIAL_MIGHT", 4.5), ("PHOTON_CUT", 4.5), ("HEGEMONY", 4.5), ("HEIRLOOM_HAMMER", 4.5), ("GLOW", 4.5), ("CLOAK_OF_STARS", 4.5), ("SPOILS_OF_BATTLE", 4.5), ("GLITTERSTREAM", 4.5), ("GLIMMER", 4.5), ("SEEKING_EDGE", 4.5), ("WROUGHT_IN_WAR", 4.0),
    ("KNOCKOUT_BLOW", 4.0), ("KINGLY_PUNCH", 4.0), ("SHINING_STRIKE", 4.0), ("BOMBARDMENT", 4.0), ("HIDDEN_CACHE", 4.0), ("KNOW_THY_PLACE", 4.0), ("REFINE_BLADE", 4.0), ("CHILD_OF_THE_STARS", 4.0), ("PARRY", 4.0), ("KINGLY_KICK", 3.5), ("STARDUST", 3.5),
    ("MAKE_IT_SO", 3.5), ("BEAT_INTO_SHAPE", 3.5), ("COSMIC_INDIFFERENCE", 3.5), ("TERRAFORMING", 3.5), ("SUMMON_FORTH", 3.5), ("DECISIONS_DECISIONS", 3.5), ("TYRANNY", 3.5), ("ORBIT", 3.5), ("MONOLOGUE", 3.0), ("ROYAL_GAMBLE", 3.0), ("ROYALTIES", 3.0),
    ("CRASH_LANDING", 4.5), ("SEVEN_STARS", 5.5), ("PILLAR_OF_CREATION", 3.0), ("SPECTRUM_SHIFT", 3.0), ("PALE_BLUE_DOT", 3.0),
    // --- The Necrobinder
    ("SOW", 5.0), ("DEBILITATE", 5.0), ("GRAVE_WARDEN", 6.0), ("PUTREFY", 5.0), ("DRAIN_POWER", 4.5), ("DEFILE", 5.0), ("BLIGHT_STRIKE", 4.5), ("REAVE", 4.5), ("BURY", 3.5), ("DEFY", 4.5), ("NEGATIVE_PULSE", 4.0), ("PARSE", 3.5), ("END_OF_DAYS", 5.5),
    ("BODYGUARD", 3.5), ("UNLEASH", 3.5), ("FEAR", 4.0), ("REAP", 3.0), ("FLATTEN", 4.5), ("PULL_AGGRO", 3.5), ("UNDEATH", 4.0), ("ENFEEBLING_TOUCH", 4.0), ("CAPTURE_SPIRIT", 5.0), ("DEATHS_DOOR", 4.0), ("NO_ESCAPE", 4.0), ("REANIMATE", 4.0), ("SNAP", 3.0),
    ("POKE", 4.0), ("RATTLE", 3.5), ("HANG", 3.5), ("INVOKE", 3.5), ("WISP", 3.5), ("DIRGE", 5.0), ("LETHALITY", 3.5), ("SHROUD", 3.5), ("HAUNT", 3.5), ("CALL_OF_THE_VOID", 3.5), ("ERADICATE", 5.5), ("MISERY", 5.0), ("NECRO_MASTERY", 5.5), ("DEATHBRINGER", 5.5),
    ("SEVERANCE", 4.5), ("SLEIGHT_OF_FLESH", 5.0), ("HIGH_FIVE", 4.5), ("SIC_EM", 3.5), ("GRAVEBLAST", 3.0), ("DEATH_MARCH", 3.0), ("PULL_FROM_BELOW", 2.5), ("BORROWED_TIME", 2.5), ("SEANCE", 2.5),
    // --- The Defect
    ("COOLHEADED", 5.5), ("COLD_SNAP", 5.0), ("BALL_LIGHTNING", 5.0), ("SWEEPING_BEAM", 5.0), ("BOOT_SEQUENCE", 5.0), ("SHADOW_SHIELD", 5.0), ("ZAP", 4.0), ("DUALCAST", 3.5), ("SUNDER", 4.5), ("CHARGE_BATTERY", 4.5), ("LIGHTNING_ROD", 4.5), ("FUSION", 4.5),
    ("GLASSWORK", 4.5), ("HAILSTORM", 4.5), ("BULK_UP", 4.5), ("CAPACITOR", 5.5), ("CREATIVE_AI", 4.5), ("ICE_LANCE", 4.5), ("FIGHT_THROUGH", 4.0), ("GO_FOR_THE_EYES", 4.0), ("FOCUSED_STRIKE", 4.0), ("BEAM_CELL", 4.0), ("MOMENTUM_STRIKE", 4.0),
    ("COMPILE_DRIVER", 4.0), ("FTL", 4.0), ("BOOST_AWAY", 4.0), ("DARKNESS", 4.0), ("MODDED", 4.0), ("GUNK_UP", 3.5), ("UPROAR", 3.5), ("COLLISION_COURSE", 3.5), ("TESLA_COIL", 3.5), ("METEOR_STRIKE", 3.5), ("HOLOGRAM", 3.5), ("OVERCLOCK", 3.5), ("COMPACT", 3.5),
    ("WHITE_NOISE", 3.5), ("DOUBLE_ENERGY", 3.5), ("STORM", 3.5), ("THUNDER", 3.5), ("SCRAPE", 3.0), ("ALL_FOR_ONE", 3.0), ("HOTFIX", 3.0), ("VOLTAIC", 3.0), ("FERAL", 3.0), ("TRASH_TO_TREASURE", 3.0), ("ITERATION", 2.5),
    // --- more, all characters
    ("MACHINE_LEARNING", 6.5), ("DEFRAGMENT", 7.5), ("GLACIER", 6.0), ("FRIENDSHIP", 4.5), ("GENESIS", 5.5), ("ECHO_FORM", 6.5), ("HYPERBEAM", 5.5), ("DEVASTATE", 5.0), ("SKIM", 5.0), ("COOLANT", 5.0), ("BUFFER", 5.0), ("SPINNER", 5.0), ("QUADCAST", 5.0),
    ("GUIDING_STAR", 4.5), ("ADAPTIVE_STRIKE", 4.5), ("BONE_SHARDS", 4.5), ("NULL", 4.5), ("LOOP", 4.5), ("SENTRY_MODE", 4.5), ("CHILL", 4.0), ("TEMPEST", 4.0), ("NEUROSURGE", 1.0), ("BLACK_HOLE", 4.0), ("LEAP", 3.5), ("DELAY", 3.5), ("CHAOS", 3.5),
    ("SOUL_STORM", 3.5), ("REFRACT", 3.5), ("FETCH", 3.5), ("REBOOT", 3.0), ("ALIGNMENT", 3.0), ("CRESCENT_SPEAR", 3.0), ("CONQUEROR", 3.0), ("SYNTHESIS", 3.0), ("RIGHT_HAND_HAND", 3.0), ("SPUR", 3.0), ("CLEANSE", 3.0), ("TIMES_UP", 3.0), ("SHARED_FATE", 3.0),
    ("DANSE_MACABRE", 3.0), ("CLAW", 2.5), ("MELANCHOLY", 2.5), ("SUBROUTINE", 2.0), ("PAGESTORM", 2.0), ("SYNCHRONIZE", 2.0), ("CHARGE", 1.5), ("GUARDS", 1.0),]));
pub static _AFFLICTION: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([("SLOTH", -6.5), ("MIND_ROT", -7.5), ("WASTE_AWAY", -8.5), ("DISINTEGRATION", -9.5)]));

pub fn _card_key(card: &Value) -> String {
    key_of(&card["id"].or(&card["name"]).text())
}

/// True if the card is one we have rated by hand.
pub fn is_known(card: &Value) -> bool {
    games::uses_sts2_tables() && _TIER.contains_key(_card_key(card).as_str())
}

pub static _RARITY: LazyLock<IndexMap<&'static str, f64>> =
    LazyLock::new(|| IndexMap::from([("Basic", 0.0), ("Common", 1.0), ("Uncommon", 2.2), ("Rare", 3.5), ("Ancient", 4.5), ("Event", 2.0)]));
// What the crowd's numbers make a card of each rarity worth on average: mixed in for a card with none of its own (every Fool card).
pub static _RARITY_CROWD: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([("Common", 3.69), ("Uncommon", 4.85), ("Rare", 6.26)]));
pub const IMPALED_SHARE: f64 = 0.5; // of what a card that needs an Impaled enemy does, counted at rewards (see deck_value)
pub const CONDITIONAL_SHARE: f64 = 0.3; // of what a card that "can only be played if" something holds does, counted at rewards
// Cards the crowd is not asked about: what makes them good in other hands is what this bot cannot do (The Gambit can end the run).
pub static _OWN_VIEW: LazyLock<HashSet<&'static str>> = LazyLock::new(|| HashSet::from(["THE_GAMBIT", "STOKE", "GUARDS", "BANSHEES_CRY", "BANSHEE_S_CRY", "KNIFE_TRAP"]));

pub fn _crowd_share() -> f64 {
    use crate::learn::knob; // (learn.rs reads this module for its card keys)
    knob("crowd")
}

/// How much we want this card in the deck (higher is better): card rewards, shop buys, upgrade and removal picks.
pub fn deck_value(card: &Value) -> f64 {
    let ctype = card["type"].text();
    if ctype == "Curse" {
        return -10.0;
    }
    if ctype == "Status" {
        // Some bosses make us keep one of these for the fight: the one that hurts least (Disintegration lost boss fights on its own).
        return _AFFLICTION.get(_card_key(card).as_str()).copied().unwrap_or(-8.0);
    }
    let key = _card_key(card);
    let key = key.as_str();
    let upgraded = if card["is_upgraded"].truthy() { 0.7 } else { 0.0 };
    // Our own rating and the crowd's, mixed (a setting under test, see learn.CHOICES).
    let mut crowd = if _OWN_VIEW.contains(key) { None } else { crowd_rating(key) };
    let mut share = if crowd.is_some() { _crowd_share() } else { 0.0 };
    let tier = if games::uses_sts2_tables() { _TIER.get(key).copied() } else { None };
    if let Some(tier) = tier {
        return (1.0 - share) * tier + share * crowd.unwrap_or(0.0) + upgraded;
    }
    let info = parse(card);
    let rarity = card["rarity"].or(&card["card_rarity"]).text();
    if crowd.is_none() && !_OWN_VIEW.contains(key) && (info.known || ctype == "Power") {
        crowd = _RARITY_CROWD.get(rarity.as_str()).copied();
        if crowd.is_some_and(|c| c != 0.0) {
            share = _crowd_share(); // (the Fool's cards: on the bare text rating they skipped 29% of rewards)
        }
    }
    if info.deadly {
        return -6.0; // e.g. "If you take unblocked attack damage this combat, die."
    }
    let cost = if !info.is_x() { info.cost.max(1) } else { 2 };
    let base = _RARITY.get(rarity.as_str()).copied().unwrap_or(1.0);
    let mut score = base;
    let dmg = info.total_damage() as f64 * if info.aoe { 1.6 } else { 1.0 };
    score += fmin(4.0, fmax(0.0, dmg / cost as f64 - 6.0) * 0.45); // (capped: uncapped, Crash Landing and Seven Stars beat every card rated by hand)
    score += fmax(0.0, info.block as f64 / cost as f64 - 5.0) * 0.45;
    score += info.strength.min(4) as f64 * if info.strength_lasts { 1.0 } else { 0.4 };
    score += info.draw as f64 * 0.8 + info.energy_gain as f64 * 1.2;
    score += info.poison as f64 * if info.poison_needs { 0.15 } else { 0.3 } + info.doom as f64 * 0.12 + info.shivs as f64 * 0.7 + info.star_gain as f64 * 0.6 + info.forge as f64 * 0.12;
    score += info.summon as f64 * 0.15 + info.channel.len() as f64 * 1.3 + info.evoke as f64 * 0.8 + info.souls as f64 * 0.6;
    score += info.dexterity as f64 * if info.dexterity_lasts { 1.5 } else { 0.4 };
    // (Energy next turn: a hand spends about 2 more, whatever comes.)
    score += info.next_energy.min(2) as f64 * 0.8 + (info.next_energy - 2).max(0) as f64 * 0.2 + info.next_draw as f64 * 0.5 + info.next_block as f64 * 0.08 + info.enemy_hp_loss as f64 * 0.15;
    score += info.vulnerable.min(3) as f64 * 0.6 + info.weak.min(3) as f64 * 0.4;
    if info.impaled_only {
        // only with our Arrow in the enemy: about half the turns
        score = base + (score - base) * IMPALED_SHARE;
    }
    if found!(r"can\s+only\s+be\s+played\s+if", &card["description"].text()) {
        score = base + (score - base) * CONDITIONAL_SHARE; // (Clash, Grand Finale, Signature Move: dead most turns)
    }
    if info.cost == 0 && info.known {
        score += 0.5;
    }
    if ctype == "Power" {
        // Powers we cannot read are a gamble; they also cost a turn of tempo.
        score += if info.known { 1.5 } else { -0.5 };
    } else if !info.known {
        score -= 1.0; // the planner cannot use what it cannot read
    }
    if info.cost >= 3 {
        score -= 0.8;
    }
    score -= 0.9 * (if info.star_cost < 0 { 2 } else { info.star_cost }) as f64; // Stars are a second price
    if info.block_lock {
        score -= 3.0;
    }
    score -= info.hp_loss as f64 * 0.1;
    if is_basic(card) {
        score -= 2.5;
    }
    score += _known_nudge(card);
    let value = (1.0 - share) * score + share * crowd.unwrap_or(0.0) + upgraded;
    // Encore: "You can only play 1 more card this turn", at best a cantrip played last.
    if info.play_limit >= 0 {
        fmin(value, 2.0 + upgraded)
    } else {
        value
    }
}

/// What upgrading a card adds, in the planner's points per play, read from the two versions of its text (not from its rating).
pub fn upgrade_gain(base: &Value, upgraded: &Value) -> f64 {
    let aimed = |card: &Value| {
        let mut card = if card.is_object() { card.clone() } else { json!({}) };
        card["target_type"] = json!("AnyEnemy");
        parse(&card)
    };
    let (a, b) = (aimed(base), aimed(upgraded));

    fn worth(i: &CardInfo) -> f64 {
        let channels = if i.r#type != "Power" { i.channel.len() as i64 } else { 0 };
        i.total_damage() as f64 * if i.aoe { 1.6 } else { 1.0 }
            + i.block as f64
            + i.draw as f64 * 3.0
            + i.energy_gain as f64 * 5.0
            + i.vulnerable.min(4) as f64 * 2.5
            + i.weak.min(4) as f64 * 2.0
            + i.strength as f64 * if i.strength_lasts { 4.0 } else { 1.5 }
            + i.dexterity as f64 * if i.dexterity_lasts { 4.0 } else { 1.5 }
            + i.poison as f64 * if i.poison_needs { 1.0 } else { 2.0 }
            + i.doom as f64 * 0.6
            + i.shivs as f64 * 4.0
            + i.star_gain as f64 * 2.5
            + i.forge as f64 * 0.6
            + i.summon as f64
            + channels as f64 * 4.0
            + i.evoke as f64 * 3.0
            + i.enemy_strength_loss as f64 * 1.5
            + i.next_energy as f64 * 4.0
            + i.next_draw as f64 * 1.5
            + i.next_block as f64 * 0.7
            + i.enemy_hp_loss as f64
            + i.souls as f64 * 3.0
    }

    let cost = |i: &CardInfo| if i.is_x() { 3 } else { i.cost };
    let mut gain = worth(&b) - worth(&a) + 6.0 * (cost(&a) - cost(&b)) as f64 + 2.5 * (a.star_cost.max(0) - b.star_cost.max(0)) as f64;
    if gain <= 0.0 {
        // Nothing we can read changed (a Power's numbers, an added keyword): still worth something.
        gain = if base::str_of(&base["description"]) != base::str_of(&upgraded["description"]) { 3.0 } else { 0.5 };
    }
    gain
}

pub fn order_key(c: &CardInfo) -> (i64, i64, i64) {
    let rank = if c.ends_turn {
        9 // whatever else the turn can do comes before the card that ends it
    } else if c.r#type == "Power" || c.key == "FRANTIC_ESCAPE" || c.verse != 0 {
        0 // set-up first; an escape must not be left for energy that is gone; Ki Verse before the cards it plays twice
    } else if c.draw != 0 || c.energy_gain != 0 || c.star_gain != 0 {
        1 // what later cards are paid with comes first
    } else if c.vulnerable != 0 || (c.summon != 0 && !c.is_x()) || (c.doom != 0 && c.damage == 0) {
        2 // (a Summon before Osty's attacks: Unleash hits for his HP; Doom before what counts it)
    } else if c.damage != 0 && !c.is_x() {
        3
    } else if c.is_x() {
        4
    } else if c.block != 0 {
        5
    } else {
        6
    };
    (rank, -c.total_damage(), c.index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn;

    /// The fields of `info` that are not as `CardInfo::default()` has them (what tests/golden/cards.json keeps of each card).
    fn diff(info: &CardInfo) -> Value {
        let base = CardInfo::default().to_json();
        Value::Object(info.to_json().as_object().unwrap().iter().filter(|(k, v)| base[k.as_str()] != **v).map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../tests/golden/cards.json")).unwrap();
        let cases = g["cases"].as_array().unwrap();
        let mut checked = 0;
        for (n, case) in cases.iter().enumerate() {
            let _game = games::playing(case["g"].as_str().unwrap());
            let card = &case["card"];
            let why = format!("case {n} {}", base::json_dumps(card));
            let info = parse(card);
            assert_eq!(info, _parse(card), "{why}");
            assert_eq!(info, parse(card), "{why}");
            assert_eq!(diff(&info), case["info"], "{why}");
            assert_eq!(kind(card), case["kind"].as_str().unwrap(), "{why}");
            assert_eq!(json!([is_basic(card), is_junk(card), is_known(card)]), case["flags"], "{why}");
            let order = order_key(&info);
            assert_eq!(json!([order.0, order.1, order.2]), case["order"], "{why}");
            assert_eq!(json!(melodies(&card["name"].text())), case["mel"], "{why}");
            assert_eq!(key_of(&card["name"].text()), case["key"].as_str().unwrap(), "{why}");
            assert_eq!(_card_key(card), case["ckey"].as_str().unwrap(), "{why}");
            assert_eq!(_known_nudge(card), case["nudge"].as_f64().unwrap(), "{why}");
            assert_eq!(json!([info.is_x(), info.total_damage()]), case["tot"], "{why}");
            let text = card["description"].text();
            assert_eq!(immediate_text(&text), case["imm"].as_str().unwrap_or(&text), "{why}");
            if let Some(values) = case["dv"].as_array() {
                for (crowd, want) in [0.0, 0.3, 0.6].iter().zip(values) {
                    let _knob = learn::with_knobs(&[("crowd", *crowd)]);
                    assert_eq!(deck_value(card), want.as_f64().unwrap(), "deck_value at crowd {crowd}: {why}");
                    checked += 1;
                }
            }
            checked += 10;
        }
        for row in g["gains"].as_array().unwrap() {
            let _game = games::playing(row[0].as_str().unwrap());
            let (a, b) = (&cases[row[1].as_u64().unwrap() as usize]["card"], &cases[row[2].as_u64().unwrap() as usize]["card"]);
            assert_eq!(json!([upgrade_gain(a, b), upgrade_gain(b, a), upgrade_gain(a, a)]), json!([row[3], row[4], row[5]]), "upgrade_gain {a} {b}");
            checked += 3;
        }
        for row in g["cost"].as_array().unwrap() {
            assert_eq!(_cost(&row[0]), row[1].as_i64().unwrap(), "_cost {}", row[0]);
            checked += 1;
        }
        for row in g["ints"].as_array().unwrap() {
            assert_eq!(_int(row[0].as_str().unwrap(), row[1].as_str().unwrap()), row[2].as_i64().unwrap());
            checked += 1;
        }
        assert!(cases.len() > 2400 && checked > 30_000, "{} cases, {checked} checks", cases.len());
    }

    #[test]
    fn a_card_is_remembered_by_all_that_parse_reads() {
        let card = json!({"id": "BASH", "name": "Bash", "type": "Attack", "cost": 2, "description": "Deal 8 damage. Apply 2 Vulnerable.", "target_type": "AnyEnemy"});
        let first = parse(&card);
        assert_eq!((first.damage, first.vulnerable, first.playable, first.index), (8, 2, true, 0));
        let mut other = card.clone();
        other["description"] = json!("Deal 10 damage. Apply 3 Vulnerable.");
        other["can_play"] = json!(false);
        other["index"] = json!(4);
        let second = parse(&other);
        assert_eq!((second.damage, second.vulnerable, second.playable, second.index), (10, 3, false, 4));
        assert_eq!(parse(&card), first);
        assert_eq!(CardInfo { cost: 0, ..first.clone() }.cost, 0);
    }
}
