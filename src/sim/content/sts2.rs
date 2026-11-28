//! Slay the Spire 2 (v0.111.1), as the simulator plays it: cards, monsters, potions and relic names from sts2_data.json.
//!
//! Powers are read from their own text: "At the start of your turn, X", "At the end of your turn, X", "Whenever you
//! play a card/an Attack/a Skill/a Power, X", "Whenever a card is Exhausted, X", "Whenever you gain Block, X", "Gain
//! [E] at the start of each turn", and what they do at once; X is done as a card with that text would do it.
//!
//! (The Python module's tables are functions here, built once per thread: `DATA()`, `CARDS()`, `COMMON()` ...; see
//! sim/content/mod.rs for how a game's content is held.)

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::cards::CardInfo;
use crate::py::{self, JsonExt};
use crate::sim::cards::{Card, CardDef};
use crate::sim::content::sts1_cards::{_pick, replace};
use crate::sim::content::{common, sts1, sts1_cards, sts2_foes, AfterRule, Character, Choice, Content, DamageRule, HandRule, Item, Nodes, Odds, PlayableRule, PowerRule, PreRule};
use crate::sim::effects::{self as fx, add, Powers};
use crate::sim::engine::{Fight, Foe, Pile, Spec};
use crate::sim::run::{Potion, Run};

pub const KEY: &str = "sts2";
pub const ACTS: i64 = 3;
pub const SHORT_REASON: bool = true; // (the mod says why a card cannot be played: "EnergyCostTooHigh")
pub const KEEP_POTIONS: &[&str] = &["FAIRY_IN_A_BOTTLE"];

pub fn UNKNOWN_ODDS() -> IndexMap<String, f64> {
    sts1::UNKNOWN_ODDS()
}

pub fn CARD_ODDS() -> Odds {
    sts1::CARD_ODDS()
}

pub fn ROOM_WEIGHTS() -> IndexMap<String, f64> {
    [("Monster", 0.42), ("Unknown", 0.30), ("RestSite", 0.12), ("Shop", 0.05), ("Elite", 0.07)].into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub const PLAYED: &[&str] = &["Main", "IRONCLAD", "SILENT", "DEFECT", "NECROBINDER", "REGENT"];

/// The game's data as the file has it (the Python reads sts2_data.json next to the module; here it is built in).
pub const DATA_TEXT: &str = include_str!("sts2_data.json");

/// A7's card odds: half the rare chance, the other half to commons.
pub fn _scarce(odds: &[(String, f64)]) -> Vec<(String, f64)> {
    let rare = odds.iter().rev().find(|(r, _)| r == "Rare").expect("KeyError: 'Rare'").1;
    odds.iter()
        .map(|(r, p)| {
            (r.clone(), if r == "Rare" {
                p / 2.0
            } else if r == "Common" {
                p + rare / 2.0
            } else {
                *p
            })
        })
        .collect()
}

pub fn SCARCE_ODDS() -> Odds {
    CARD_ODDS().iter().map(|(k, v)| (k.clone(), _scarce(v))).collect()
}

pub fn _defs() -> IndexMap<String, Rc<CardDef>> {
    let data = DATA();
    let mut out = IndexMap::new();
    for (key, row) in data["cards"].entries() {
        let (name, char, ctype, cost, cost_up, rarity, target, text, text_up, kws, kws_up, star, star_up) =
            (&row[0], &row[1], &row[2], &row[3], &row[4], &row[5], &row[6], &row[7], &row[8], &row[9], &row[10], &row[11], &row[12]);
        if ctype.s() == "Quest" {
            continue;
        }
        let stars = |v: &Value| match v.as_i64() {
            None | Some(0) => None,
            n => n,
        };
        let words = |v: &Value| v.list().iter().map(|k| k.s().to_string()).collect::<Vec<_>>();
        out.insert(key.clone(), Rc::new(CardDef {
            key: key.clone(),
            name: name.s().to_string(),
            r#type: ctype.s().to_string(),
            cost: cost.int(),
            rarity: rarity.s().to_string(),
            targeted: target.s() == "AnyEnemy",
            text: text.s().to_string(),
            text_up: text_up.s().to_string(),
            cost_up: cost_up.int(),
            color: char.s().to_string(),
            star_cost: stars(star),
            star_cost_up: stars(star_up),
            keywords: words(kws),
            keywords_up: words(kws_up),
        }));
    }
    out
}

type Pool = Rc<Vec<Rc<CardDef>>>;
type Rules = Rc<Vec<(String, String)>>;

thread_local! {
    static DATA_TABLE: Rc<Value> = Rc::new(serde_json::from_str(DATA_TEXT).expect("sts2_data.json"));
    static CARD_TABLE: Rc<IndexMap<String, Rc<CardDef>>> = Rc::new(_defs());
    static POOLS: RefCell<HashMap<String, Pool>> = RefCell::new(HashMap::new());
    static COLORLESS: Pool = Rc::new(CARDS().values().filter(|d| d.color == "COLORLESS" && matches!(d.rarity.as_str(), "Uncommon" | "Rare")).cloned().collect());
    static POWER_RULES: RefCell<HashMap<String, Rules>> = RefCell::new(HashMap::new());
    static EFFECT_CARDS: RefCell<HashMap<String, Rc<CardDef>>> = RefCell::new(HashMap::new());
    static SPECS: RefCell<HashMap<(String, i64), Rc<Spec>>> = RefCell::new(HashMap::new());
    static WITHERS: RefCell<HashMap<i64, Rc<CardDef>>> = RefCell::new(HashMap::new());
    static PLAY_KIND: Rc<HashMap<String, String>> = Rc::new(_play_kinds());
    static RELIC_TIERS: Rc<[Vec<String>; 3]> = Rc::new(_relic_tiers());
    static POTION_TABLE: Rc<IndexMap<String, (String, String, String)>> = Rc::new(_potions());
}

pub fn DATA() -> Rc<Value> {
    DATA_TABLE.with(|d| d.clone())
}

pub fn CARDS() -> Rc<IndexMap<String, Rc<CardDef>>> {
    CARD_TABLE.with(|c| c.clone())
}

pub fn CHARACTERS() -> IndexMap<String, Character> {
    let data = DATA();
    let names = |v: &Value| v.list().iter().map(|k| k.s().to_string()).collect::<Vec<_>>();
    data["characters"]
        .entries()
        .filter(|(k, _)| PLAYED.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), Character { title: v["title"].s().to_string(), hp: v["hp"].int(), deck: names(&v["deck"]), relics: names(&v["relics"]), orbs: v["orbs"].int() }))
        .collect()
}

/// `make(key, up=False)`.
pub fn make(key: &str, up: bool) -> Card {
    let key = if key == "ASCENDER_S_BANE" { "ASCENDERS_BANE" } else { key };
    CARD_TABLE.with(|c| Card::new(c.get(key).unwrap_or_else(|| panic!("KeyError: {key}")), up))
}

pub fn pool(character: &str) -> Pool {
    if let Some(p) = POOLS.with(|p| p.borrow().get(character).cloned()) {
        return p;
    }
    let made: Pool = Rc::new(CARDS().values().filter(|d| d.color == character && matches!(d.rarity.as_str(), "Common" | "Uncommon" | "Rare")).cloned().collect());
    POOLS.with(|p| p.borrow_mut().insert(character.to_string(), made.clone()));
    made
}

pub fn colorless() -> Pool {
    COLORLESS.with(|c| c.clone())
}

// ------------------------------------------------------------------ cards the text does not cover

/// The Sovereign Blade among our cards: the pile it is in and its `uid`.
pub fn _blade(f: &Fight) -> Option<(Pile, u64)> {
    [(Pile::Hand, &f.hand), (Pile::Draw, &f.draw), (Pile::Discard, &f.discard)].into_iter().find_map(|(pile, cards)| cards.iter().find(|c| c.key() == "SOVEREIGN_BLADE").map(|c| (pile, c.uid)))
}

pub fn _forge(f: &mut Fight, n: i64) {
    add(&mut f.pw, "FORGE", n);
    if _blade(f).is_none() {
        f.add_card(make("SOVEREIGN_BLADE", false), "hand");
    }
}

// What a card's text puts in our piles or on the enemy that the engine's generic part left out until 2026-11-26 (Fight Through's two
// Wounds, Boost Away's Dazed, Gunk Up's Slimed, Overclock's Burn, Turbo's Void; Fight Me's Strength for the enemy).
pub fn _STATUS_ADDED() -> &'static regex::Regex {
    crate::re!(r"(?i)\b(?:add|shuffle)\s+(a|an|\d+)\s+(wound|dazed|slimed|burn|void|soot|toxic)s?\b[^.]*?\binto\s+your\s+(discard pile|draw pile|hand)")
}

pub fn _PILES(pile: &str) -> &'static str {
    match pile {
        "discard pile" => "discard",
        "draw pile" => "draw",
        "hand" => "hand",
        _ => panic!("KeyError: {pile}"),
    }
}

pub fn _ENEMY_GAINS() -> &'static regex::Regex {
    crate::re!(r"(?i)\bthe enemy gains (\d+) strength")
}

// ... and what it gives (Adaptive Strike's and Undeath's copies, Bundle of Joy's and Manifest Authority's Colorless cards, Quasar's choice,
// Stoke's new hand, Hyperbeam's lost Focus).
pub fn _SELF_COPY() -> &'static regex::Regex {
    crate::re!(r"(?i)\badd a (0\[energy_icon\.png\] )?copy of this card into your discard pile")
}

pub fn _COLORLESS() -> &'static regex::Regex {
    crate::re!(r"(?i)\badd (\d+) random colorless cards? into your hand")
}

pub fn _QUASAR() -> &'static regex::Regex {
    crate::re!(r"(?i)\bchoose 1 of (\d+) random colorless cards to add into your hand")
}

pub fn _STOKE() -> &'static regex::Regex {
    crate::re!(r"(?i)\bexhaust your hand\. add 1 random card into your hand for each card exhausted")
}

pub fn _FOCUS_TURN() -> &'static regex::Regex {
    crate::re!(r"(?i)\blose (\d+) focus this turn")
}

fn number(text: &str) -> i64 {
    text.parse().expect("a number")
}

/// Stars, Forge, Osty, Doom, Souls, Debris, statuses, copies and cards moved: what Slay the Spire 2's text says that the engine's generic part leaves out.
pub fn EXTRA(f: &mut Fight, c: &mut Card, foe: Option<usize>, i: &CardInfo, xs: i64) {
    let text = c.text().to_string();
    for m in _STATUS_ADDED().captures_iter(&text) {
        let n = if matches!(m[1].to_lowercase().as_str(), "a" | "an") { 1 } else { number(&m[1]) };
        f.add_new(&m[2].to_uppercase(), n, _PILES(&m[3].to_lowercase()), false);
    }
    if let (Some(m), Some(foe)) = (_ENEMY_GAINS().captures(&text), foe) {
        if f.foes[foe].alive() {
            add(&mut f.foes[foe].powers, "STRENGTH", number(&m[1]));
        }
    }
    if i.osty_dies && f.osty > 0 && !f.game.AFTER.contains_key(c.key()) {
        // Bone Shards: "Osty dies" (until 2026-11-26 he lived on; Sacrifice has its own rule)
        f.osty = 0;
    }
    if let Some(m) = _SELF_COPY().captures(&text) {
        let mut twin = c.copy();
        if m.get(1).is_some() {
            twin.cost_fight = Some(0);
        }
        f.add_card(twin, "discard");
    }
    if let Some(m) = _COLORLESS().captures(&text) {
        let choices = colorless();
        for _ in 0..number(&m[1]) {
            let d = f.run.rng.choice(&choices).clone();
            f.add_card(Card::new(&d, false), "hand");
        }
    }
    if let Some(m) = _QUASAR().captures(&text) {
        let offered: Vec<Card> = f.run.rng.sample(&colorless(), number(&m[1]) as usize).iter().map(|d| Card::new(d, false)).collect();
        for picked in f.pile_pick("Choose a card to add into your Hand.", &offered, 1).into_iter().take(1) {
            f.add_card(picked, "hand");
        }
    }
    if _STOKE().is_match(&text) {
        let burnt: Vec<u64> = f.hand.iter().filter(|h| h.uid != c.uid).map(|h| h.uid).collect();
        for uid in &burnt {
            let h = f.take_from(Pile::Hand, *uid).expect("ValueError: list.remove(x): x not in list");
            f.exhaust(h);
        }
        let own = pool(&f.run.character);
        for _ in &burnt {
            let d = f.run.rng.choice(&own).clone();
            f.add_card(Card::new(&d, false), "hand");
        }
    }
    if let Some(m) = _FOCUS_TURN().captures(&text) {
        add(&mut f.temp, "FOCUS", -number(&m[1]));
    }
    if i.star_gain != 0 {
        f.stars += i.star_gain;
        _star_hooks(f, i.star_gain);
    }
    if i.forge != 0 {
        _forge(f, i.forge * xs);
    }
    if i.summon != 0 {
        _summon(f, i.summon * xs);
    }
    if i.doom != 0 {
        let targets: Vec<usize> = if i.aoe {
            f.alive()
        } else {
            match foe {
                Some(v) if f.foes[v].alive() => vec![v],
                _ => vec![],
            }
        };
        for v in targets {
            let mut n = i.doom * xs + if c.key() == "NO_ESCAPE" { 5 * py::floordiv(f.foes[v].powers.val("DOOM"), 10) } else { 0 };
            if c.key() == "BLIGHT_STRIKE" {
                n = f.attack_value(i.damage + c.bonus, Some(v), false, false);
            }
            fx::debuff(f, Some(v), "DOOM", n);
            if f.pw.on("SHROUD") {
                f.gain_block(f.pw["SHROUD"]);
            }
        }
        if i.doom_kills {
            for v in f.alive() {
                if f.foes[v].powers.val("DOOM") >= f.foes[v].hp {
                    f.kill(v);
                }
            }
        }
    }
    f.add_new("SOUL", i.souls * xs, "draw", false);
    if i.debris != 0 {
        let n = if i.debris > 0 { i.debris } else { (10 - f.hand.len() as i64).max(0) };
        f.add_new("DEBRIS", n, "hand", false);
    }
    for _ in 0..xs {
        for kind in &i.channel {
            add(&mut f.pw, &format!("{kind}_CHANNELED"), 1);
        }
    }
}

pub fn _summon(f: &mut Fight, n: i64) {
    if f.osty < 0 {
        return;
    }
    f.osty += n;
    let most = f.pw.val("OSTY_MAX").max(f.osty);
    f.pw.put("OSTY_MAX", most);
}

pub fn _star_hooks(f: &mut Fight, n: i64) {
    if f.pw.on("BLACK_HOLE") {
        f.hurt_all(f.pw["BLACK_HOLE"] * n.abs());
    }
}

fn d(rule: impl Fn(&Fight, &Card) -> i64 + 'static) -> DamageRule {
    Rc::new(rule)
}

fn pre(rule: impl Fn(&mut Fight, &mut Card, Option<usize>, Rc<CardInfo>) -> Option<Rc<CardInfo>> + 'static) -> PreRule {
    Rc::new(rule)
}

fn a(rule: impl Fn(&mut Fight, &mut Card, Option<usize>, i64, i64) + 'static) -> AfterRule {
    Rc::new(rule)
}

pub fn DAMAGE() -> HashMap<String, DamageRule> {
    let rows: Vec<(&str, DamageRule)> = vec![
        ("BODY_SLAM", d(|f, _c| f.block)),
        ("SOVEREIGN_BLADE", d(|f, _c| 10 + f.pw.val("FORGE"))),
        ("UNLEASH", d(|f, c| (if c.up { 9 } else { 6 }) + f.osty.max(0))),
        ("PROTECTOR", d(|f, c| (if c.up { 15 } else { 10 }) + f.pw.val("OSTY_MAX"))),
        ("TIMES_UP", d(|_f, _c| 0)),
        ("DEATH_MARCH", d(|f, c| (if c.up { 10 } else { 8 }) + (if c.up { 5 } else { 4 }) * f.pw.val("DRAWN_TURN"))),
        ("RAMPAGE", d(|_f, c| 10 + c.bonus)),
    ];
    rows.into_iter().map(|(k, r)| (k.to_string(), r)).collect()
}

pub fn PRE() -> HashMap<String, PreRule> {
    let mut out: HashMap<String, PreRule> = HashMap::new();
    out.insert("TIMES_UP".to_string(), pre(|f, _c, foe, i| match foe {
        None => Some(i),
        Some(foe) => {
            let doom = f.foes[foe].powers.val("DOOM");
            replace(&i, |i| {
                i.damage = doom;
                i.hits = 1;
            })
        }
    }));
    let osty: PreRule = pre(|f, _c, _foe, i| if f.osty <= 0 && i.osty_attack { replace(&i, |i| i.damage = 0) } else { Some(i) });
    for k in ["FLATTEN", "POKE", "RATTLE", "SNAP", "SIC_EM", "UNLEASH"] {
        out.insert(k.to_string(), osty.clone());
    }
    // "If Osty is alive, he deals ... and you gain ..."
    out.insert("BONE_SHARDS".to_string(), pre(|f, _c, _foe, i| {
        if f.osty <= 0 {
            replace(&i, |i| {
                i.damage = 0;
                i.block = 0;
            })
        } else {
            Some(i)
        }
    }));
    let first = sts1_cards::PRE();
    for k in ["FIEND_FIRE", "BURNING_PACT", "SECOND_WIND"] {
        out.insert(k.to_string(), first[k].clone());
    }
    out
}

/// Thrash: "Exhaust a random Attack in your Hand and add its damage to this card" (a hit's worth, for the rest of the fight).
pub fn _thrash(f: &mut Fight, c: &mut Card) {
    let attacks: Vec<u64> = f.hand.iter().filter(|h| h.r#type() == "Attack" && h.uid != c.uid).map(|h| h.uid).collect();
    if !attacks.is_empty() {
        let uid = *f.run.rng.choice(&attacks);
        let h = f.take_from(Pile::Hand, uid).expect("the card is in the hand");
        let gain = (h.info().damage + h.bonus).max(0);
        f.exhaust(h);
        c.bonus += gain;
    }
}

/// Nightmare: "Choose a card. Next turn, add 3 copies of that card into your Hand" (turn_start brings them).
pub fn _nightmare(f: &mut Fight) {
    for h in f.hand_pick("Choose a card to copy.", 1, None) {
        let twin = f.card(h.uid).unwrap_or(&h).copy();
        f.nightmares.push(twin);
    }
}

pub fn _summon_forth(f: &mut Fight) {
    match _blade(f) {
        None => f.add_card(make("SOVEREIGN_BLADE", false), "hand"),
        Some((Pile::Hand, _)) => {}
        Some((pile, uid)) => {
            let blade = f.take_from(pile, uid).expect("the Sovereign Blade");
            f.add_card(blade, "hand");
        }
    }
}

pub fn AFTER() -> HashMap<String, AfterRule> {
    let first = sts1_cards::AFTER();
    let mut out: HashMap<String, AfterRule> = HashMap::new();
    for k in ["HAVOC", "TRUE_GRIT", "BATTLE_TRANCE", "DUAL_WIELD", "INFERNAL_BLADE", "RAGE", "DOUBLE_TAP", "LIMIT_BREAK",
              "SURVIVOR", "ACROBATICS", "DAGGER_THROW", "PREPARED", "CALCULATED_GAMBLE", "BULLET_TIME", "BURST",
              "STORM_OF_STEEL", "UNLOAD", "CHAOS", "DOUBLE_ENERGY", "WHITE_NOISE", "REBOOT", "SEEK", "HOLOGRAM",
              "STACK", "SECOND_WIND", "FEED", "ARMAMENTS", "ANGER", "HEADBUTT"] {
        out.insert(k.to_string(), first[k].clone());
    }
    let rows: Vec<(&str, AfterRule)> = vec![
        // What these move between piles (until 2026-11-26 nothing: the first game's Hologram, Headbutt, All for One and Secret Technique rules).
        ("GRAVEBLAST", first["HOLOGRAM"].clone()),
        ("COSMIC_INDIFFERENCE", first["HEADBUTT"].clone()),
        ("ALL_FOR_ONE", first["ALL_FOR_ONE"].clone()),
        ("SECRET_TECHNIQUE", first["SECRET_TECHNIQUE"].clone()),
        ("DREDGE", a(|f, _, _, _, _| _pick(f, "Choose a card to return to your hand.", Pile::Discard, None, 3, None))),
        ("THRASH", a(|f, c, _, _, _| _thrash(f, c))),
        ("NIGHTMARE", a(|f, _, _, _, _| _nightmare(f))),
        ("FOREGONE_CONCLUSION", a(|f, c, _, _, _| f.foregone += if c.up { 3 } else { 2 })),
        ("SACRIFICE", a(|f, _, _, _, _| {
            if f.osty > 0 {
                f.osty = 0;
                f.gain_block(3 * f.pw.val("OSTY_MAX"));
            }
        })),
        ("SPUR", a(|f, _, _, _, _| {
            if f.osty > 0 {
                f.osty += 5;
            }
        })),
        ("EXPOSE", a(|f, _c, foe, _, _| {
            if let Some(foe) = foe {
                f.foes[foe].block = 0;
                f.foes[foe].powers.shift_remove("ARTIFACT");
            }
        })),
        ("UP_MY_SLEEVE", a(|_f, c, _, _, _| c.cost_fight = Some((c.cost() - 1).max(0)))),
        ("SUMMON_FORTH", a(|f, _, _, _, _| _summon_forth(f))),
        ("KNOCKOUT_BLOW", a(|f, _c, foe, _, _| {
            if foe.is_some_and(|foe| !f.foes[foe].alive()) {
                f.stars += 5;
            }
        })),
        ("HANG", a(|f, _c, foe, _, _| {
            if let Some(foe) = foe {
                let n = (f.foes[foe].powers.val("HANG") * 2).max(2);
                f.foes[foe].powers.put("HANG", n);
            }
        })),
        ("RAMPAGE", a(|_f, c, _, _, _| c.bonus += if c.up { 10 } else { 5 })),
        ("ONE_TWO_PUNCH", a(|f, _, _, _, _| f.double_next += 1)),
        ("HOTFIX", a(|f, c, _, _, _| {
            add(&mut f.temp, "FOCUS", if c.up { 3 } else { 2 });
        })),
        ("TERRAFORMING", a(|f, c, _, _, _| {
            add(&mut f.pw, "VIGOR", if c.up { 10 } else { 7 });
        })),
        ("MONOLOGUE", a(|f, _, _, _, _| f.pw.put("MONOLOGUE", 1))),
        ("SHADOWMELD", a(|f, _, _, _, _| f.pw.put("SHADOWMELD", 1))),
        ("FRANTIC_ESCAPE", a(|f, c, _, _, _| {
            for v in f.alive() {
                if f.foes[v].powers.on("SANDPIT") {
                    f.foes[v].powers["SANDPIT"] += 1;
                }
            }
            c.cost_fight = Some(c.cost() + 1);
        })),
        ("CORROSIVE_WAVE", a(|f, c, _, _, _| {
            add(&mut f.pw, "CORROSIVE_WAVE", if c.up { 3 } else { 2 });
        })),
        ("OBLIVION", a(|f, _c, foe, _, _| {
            if let Some(foe) = foe {
                f.foes[foe].mem.add("oblivion", 3);
            }
        })),
        ("SIGNAL_BOOST", a(|f, _, _, _, _| {
            add(&mut f.pw, "AMPLIFY", 1);
        })),
    ];
    for (k, r) in rows {
        out.insert(k.to_string(), r);
    }
    out
}

// "Can only be played if ...": worded as in the first game (until 2026-11-26 none held here, and Grand Finale was 60 damage to ALL
// enemies for nothing whenever drawn: +7.5 floors a pick at A8, and the card table had it at its +3 cap).
pub fn PLAYABLE() -> HashMap<String, PlayableRule> {
    let first = sts1_cards::PLAYABLE();
    ["CLASH", "GRAND_FINALE"].into_iter().map(|k| (k.to_string(), first[k].clone())).collect()
}

pub fn DRAWN() -> HashMap<String, HandRule> {
    HashMap::new()
}

pub fn RETAINED() -> HashMap<String, HandRule> {
    HashMap::new()
}

pub fn X_PLUS() -> HashSet<String> {
    HashSet::new()
}

// ------------------------------------------------------------------ powers, read from their text

/// (pattern, when): the last group of the pattern is what the Power does then.
pub fn _TRIGGERS() -> [(&'static regex::Regex, &'static str); 6] {
    [
        (crate::re!(r"(?i)^at the start of your turn, (.+)$"), "start"),
        (crate::re!(r"(?i)^at the end of your turn, (.+)$"), "end"),
        (crate::re!(r"(?i)^whenever you play an? (attack|skill|power|card)(?: that costs [^,]*)?, (.+)$"), "play"),
        (crate::re!(r"(?i)^whenever a card is exhausted, (.+)$"), "exhaust"),
        (crate::re!(r"(?i)^whenever you gain block, (.+)$"), "block"),
        (crate::re!(r"(?i)^gain ((?:\[energy_icon\.png\])+) at the start of each turn\.?$"), "energy"),
    ]
}

// (what the planner's parser already reads in a Power)
pub fn _DONE_BY_PARSE() -> &'static regex::Regex {
    crate::re!(r"(?i)^(gain|lose) \d+ (strength|dexterity)\.?$")
}

pub fn _SENTENCES() -> &'static fancy_regex::Regex {
    crate::fre!(r"(?<=[.!?])\s+")
}

/// `_SENTENCES.split(text)`.
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0;
    for m in _SENTENCES().find_iter(text) {
        let m = m.expect("a match");
        out.push(&text[from..m.start()]);
        from = m.end();
    }
    out.push(&text[from..]);
    out
}

/// (trigger, what it does) for each sentence of a Power's text; trigger "now" for what happens as it is played.
pub fn power_rules(text: &str) -> Rules {
    if let Some(rules) = POWER_RULES.with(|m| m.borrow().get(text).cloned()) {
        return rules;
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for s in sentences(text.trim()) {
        let s = s.trim();
        if s.is_empty() || _DONE_BY_PARSE().is_match(s) {
            continue;
        }
        let mut found = false;
        for (pat, when) in _TRIGGERS() {
            if let Some(m) = pat.captures(s) {
                // (`m.group(m.lastindex)`: every group of these patterns takes part in a match, so it is the last one)
                out.push((when.to_string(), m.get(m.len() - 1).expect("the last group").as_str().to_string()));
                found = true;
                break;
            }
        }
        if !found {
            out.push(("now".to_string(), s.to_string()));
        }
    }
    let rules: Rules = Rc::new(out);
    POWER_RULES.with(|m| m.borrow_mut().insert(text.to_string(), rules.clone()));
    rules
}

pub fn _effect_card(text: &str) -> Rc<CardDef> {
    if let Some(d) = EFFECT_CARDS.with(|m| m.borrow().get(text).cloned()) {
        return d;
    }
    let mut chars = text.chars();
    let shown = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    let made = Rc::new(CardDef {
        key: "_EFFECT".to_string(),
        name: "Effect".to_string(),
        r#type: "Skill".to_string(),
        cost: 0,
        rarity: "Token".to_string(),
        targeted: false,
        text: shown,
        text_up: text.to_string(),
        cost_up: 0,
        color: "COLORLESS".to_string(),
        ..CardDef::default()
    });
    EFFECT_CARDS.with(|m| m.borrow_mut().insert(text.to_string(), made.clone()));
    made
}

/// Carry out a sentence of a Power's (or a relic's) text as a card that says it would.
pub fn do_text(f: &mut Fight, text: &str) {
    let alive = f.alive();
    let at = if alive.is_empty() { None } else { Some(*f.run.rng.choice(&alive)) };
    let mut card = Card::new(&_effect_card(text), false);
    fx::resolve(f, &mut card, at, 0);
}

pub fn _register(f: &mut Fight, c: &mut Card) {
    let rules = power_rules(c.text());
    let amount = crate::re!(r"(\d+)").captures(c.text()).map_or(1, |m| number(&m[1]));
    add(&mut f.pw, c.key(), amount);
    if let Some(own) = OWN_POWER(c.key()) {
        own(f, c);
        return;
    }
    for (when, what) in rules.iter() {
        if when == "now" {
            do_text(f, what);
        } else if when == "energy" {
            add(&mut f.pw, "TURN_ENERGY", what.matches("energy_icon").count() as i64);
        } else {
            f.triggers.push((when.clone(), what.clone(), c.key().to_string()));
        }
    }
}

/// `OWN_POWER.get(key)`.
pub fn OWN_POWER(key: &str) -> Option<fn(&mut Fight, &Card)> {
    Some(match key {
        "BARRICADE" => |f, _c| f.pw.put("BARRICADE", 1),
        "CORRUPTION" => |f, _c| f.pw.put("CORRUPTION", 1),
        "ECHO_FORM" => |f, _c| f.pw.put("ECHO_FORM", 1),
        "BUFFER" => |_f, _c| {},
        "ACCURACY" => |_f, _c| {},
        "ENVENOM" => |f, _c| f.pw.put("ENVENOM", 1),
        "AFTERIMAGE" => |f, _c| {
            add(&mut f.pw, "AFTER_IMAGE", 1);
        },
        "WELL_LAID_PLANS" => |f, _c| f.pw.put("KEEP_HAND_ALWAYS", 1),
        "VOID_FORM" => |f, _c| {
            f.pw.put("VOID_FORM", 2);
            f.pw.put("TURN_OVER", 1);
        },
        "CAPACITOR" => |f, c| f.orb_slots += if c.up { 3 } else { 2 },
        "BULK_UP" => |f, _c| f.orb_slots = (f.orb_slots - 1).max(0),
        "DEFRAGMENT" => |f, c| {
            add(&mut f.pw, "FOCUS", if c.up { 2 } else { 1 });
        },
        "BIASED_COGNITION" => |f, c| {
            add(&mut f.pw, "FOCUS", if c.up { 6 } else { 5 });
            f.pw.put("BIASED_COGNITION", 1);
        },
        "WRAITH_FORM" => |f, c| {
            add(&mut f.pw, "INTANGIBLE", if c.up { 3 } else { 2 });
            f.pw.put("WRAITH_FORM", 1);
        },
        "NECRO_MASTERY" => |f, c| _summon(f, if c.up { 8 } else { 5 }),
        "SEEKING_EDGE" => |f, c| _forge(f, if c.up { 11 } else { 7 }),
        "FAN_OF_KNIVES" => |f, _c| f.add_new("SHIV", 4, "hand", false),
        "ABRASIVE" => |f, c| {
            add(&mut f.pw, "THORNS", if c.up { 6 } else { 4 });
        },
        "STONE_ARMOR" => |f, c| {
            add(&mut f.pw, "PLATING", if c.up { 6 } else { 4 });
        },
        "NEUTRON_AEGIS" => |f, c| {
            add(&mut f.pw, "PLATING", if c.up { 11 } else { 8 });
        },
        "NEUROSURGE" => |f, _c| {
            f.energy += 3;
            f.draw_cards(2);
            f.pw.put("NEUROSURGE", 3);
        },
        _ => return None,
    })
}

/// `trigger(f, when, card=None)`.
pub fn trigger(f: &mut Fight, when: &str, card: Option<&Card>) {
    let kinds = _PLAY_KIND();
    for (w, what, key) in f.triggers.clone() {
        // (a Power that names the type of card it waits for)
        let want = match (when, card) {
            ("play", Some(_)) => kinds.get(&key).map(|k| k.as_str()).unwrap_or(""),
            _ => "",
        };
        let other = !want.is_empty() && card.is_some_and(|card| want != card.r#type() && want != "card");
        if w == when && !other {
            do_text(f, &what);
        }
    }
}

fn _play_kinds() -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (k, d) in CARDS().iter() {
        if d.r#type == "Power" {
            if let Some(m) = crate::re!(r"(?i)whenever you play an? (attack|skill|power|card)").captures(&d.text) {
                out.insert(k.clone(), if m[1].to_lowercase() != "card" { py::title(&m[1]) } else { "card".to_string() });
            }
        }
    }
    out
}

/// card key -> the type of card a Power waits for ("Attack", "Skill", "Power", or "card").
pub fn _PLAY_KIND() -> Rc<HashMap<String, String>> {
    PLAY_KIND.with(|k| k.clone())
}

pub fn POWER() -> HashMap<String, PowerRule> {
    let rule: PowerRule = Rc::new(_register);
    CARDS().iter().filter(|(_, d)| d.r#type == "Power").map(|(k, _)| (k.clone(), rule.clone())).collect()
}

// ------------------------------------------------------------------ the engine's hooks

pub fn START(f: &mut Fight) {
    f.triggers = Vec::new();
    if f.run.has("CRACKED_CORE") {
        f.channel("LIGHTNING");
    }
    if f.run.has("DIVINE_RIGHT") {
        f.stars += 3;
    }
    if f.foes.iter().any(|foe| foe.powers.on("BACK_ATTACK_LEFT") || foe.powers.on("BACK_ATTACK_RIGHT")) {
        f.pw.put("SURROUNDED", 1);
    }
    for foe in f.foes.iter_mut() {
        foe.mem.set("fight", 1);
        if foe.powers.on("ILLUSION") {
            foe.minion = true;
        }
    }
}

pub fn TURN_START(f: &mut Fight) {
    if f.run.has("BOUND_PHYLACTERY") {
        _summon(f, 1);
    }
    f.energy += f.pw.val("TURN_ENERGY");
    if f.pw.on("VOID_FORM") {
        let n = f.pw["VOID_FORM"];
        f.pw.put("VOID_FREE", n);
    }
    f.pw.shift_remove("DRAWN_TURN");
    let n = std::mem::take(&mut f.foregone);
    if n != 0 {
        // Foregone Conclusion: "Next turn, put 2 cards from your Draw Pile into your Hand"
        _pick(f, "Choose a card to put into your hand.", Pile::Draw, None, n, None);
    }
    for h in std::mem::take(&mut f.nightmares) {
        for _ in 0..3 {
            f.add_card(h.copy(), "hand");
        }
    }
    if f.pw.on("NEUROSURGE") {
        let n = f.pw["NEUROSURGE"];
        add(&mut f.pw, "DOOM", n);
    }
    for foe in f.alive() {
        // Rampart: the Living Shield's Turret Operator gains 25 Block as our turn starts
        if f.foes[foe].powers.on("RAMPART") {
            let n = f.foes[foe].powers["RAMPART"];
            for x in f.alive() {
                if f.foes[x].key == "TURRET_OPERATOR" {
                    f.foes[x].block += n;
                }
            }
        }
    }
    trigger(f, "start", None);
}

pub fn TURN_END(f: &mut Fight) {
    trigger(f, "end", None);
    if f.pw.on("PLATING") {
        // (its Block is in: Plating wears off by 1 a turn)
        f.pw["PLATING"] -= 1;
    }
    if f.pw.on("DISINTEGRATION") {
        f.take(f.pw["DISINTEGRATION"], None, false);
    }
    if f.pw.on("DOOM") && f.pw["DOOM"] >= f.run.hp {
        f.lose_hp(f.run.hp, false);
    }
    f.temp.shift_remove("FOCUS");
    for k in ["MONOLOGUE", "SHADOWMELD", "CORROSIVE_WAVE"] {
        f.pw.shift_remove(k);
    }
    for foe in f.foes.iter_mut() {
        foe.mem.ints.remove("oblivion");
    }
    if f.pw.on("KEEP_HAND_ALWAYS") {
        f.pw.put("KEEP_HAND", 1);
    }
}

pub fn TURN_CARDS(_f: &mut Fight) {}

pub fn ROUND_END(f: &mut Fight) {
    sts2_foes::revive_round(f);
    for i in f.alive() {
        let foe = &mut f.foes[i];
        if foe.powers.on("SANDPIT") {
            foe.powers["SANDPIT"] -= 1;
            if foe.powers["SANDPIT"] <= 0 {
                f.lose_hp(f.run.hp, false);
            }
        }
        let foe = &mut f.foes[i];
        for k in ["TERRITORIAL", "HIGH_VOLTAGE"] {
            if foe.powers.on(k) {
                let n = foe.powers[k];
                add(&mut foe.powers, "STRENGTH", n);
            }
        }
        if foe.powers.on("PLATING") {
            foe.powers["PLATING"] -= 1;
        }
        if foe.powers.on("NEMESIS") {
            // Intangible every other turn
            let now = !foe.mem.truthy("nemesis");
            foe.mem.set_flag("nemesis", now);
            if now {
                add(&mut foe.powers, "INTANGIBLE", 1);
            }
        }
        if foe.powers.on("SLUMBER") {
            sts2_foes::slumber(foe, true);
        }
        if foe.powers.on("ESCAPE_ARTIST") {
            foe.powers["ESCAPE_ARTIST"] -= 1;
            if foe.powers["ESCAPE_ARTIST"] <= 0 {
                foe.gone = true;
            }
        }
        if foe.powers.on("ASLEEP") {
            foe.powers["ASLEEP"] -= 1;
        }
        foe.mem.ints.remove("spark");
    }
    for k in ["RINGING", "SMOGGY", "TENDER", "TANGLED"] {
        if f.pw.on(k) && !f.fresh.contains(k) {
            f.pw[k] -= 1;
        }
    }
    if !f.foes.iter().any(|x| x.alive() && !x.minion) {
        f.won = true;
    }
}

// ------------------------------------------------------------------ relics and potions

/// `sts1.COMMON + sts1.UNCOMMON + sts1.RARE`.
pub fn SHARED() -> Vec<&'static str> {
    sts1::COMMON.iter().chain(sts1::UNCOMMON).chain(sts1::RARE).copied().collect()
}

/// `_RELIC.get(key)`: [name, rarity, text].
fn relic_row(data: &Value, key: &str) -> Option<(String, String, String)> {
    let row = data["relics"].dict()?.get(key)?;
    Some((row[0].s().to_string(), row[1].s().to_string(), row[2].s().to_string()))
}

fn _relic_tiers() -> [Vec<String>; 3] {
    let data = DATA();
    let rarity = |r: &str| relic_row(&data, r).map_or(String::new(), |row| row.1);
    let tier = |wanted: &[&str]| SHARED().into_iter().filter(|r| wanted.contains(&rarity(r).as_str())).map(|r| r.to_string()).collect::<Vec<_>>();
    [tier(&["Common"]), tier(&["Uncommon"]), tier(&["Rare", "Shop"])]
}

pub fn COMMON() -> Vec<String> {
    RELIC_TIERS.with(|t| t[0].clone())
}

pub fn UNCOMMON() -> Vec<String> {
    RELIC_TIERS.with(|t| t[1].clone())
}

pub fn RARE() -> Vec<String> {
    RELIC_TIERS.with(|t| t[2].clone())
}

pub const ANCIENT: &[&str] = &["SOZU", "VELVET_CHOKER", "PHILOSOPHER_S_STONE", "ECTOPLASM", "SNECKO_EYE", "RUNIC_PYRAMID", "BLACK_STAR", "ASTROLABE",
                               "CALLING_BELL", "EMPTY_CAGE", "PANDORA_S_BOX", "ORRERY", "WHITE_BEAST_STATUE"];

/// The relic's key in the game's data (the model keeps the first game's ids, "PHILOSOPHER_S_STONE").
pub fn _data_key(rid: &str) -> &str {
    match rid {
        "PHILOSOPHER_S_STONE" => "PHILOSOPHERS_STONE",
        "PANDORA_S_BOX" => "PANDORAS_BOX",
        "CAPTAIN_S_WHEEL" => "CAPTAINS_WHEEL",
        "CHARON_S_ASHES" => "CHARONS_ASHES",
        _ => rid,
    }
}

pub fn relic_text(rid: &str) -> String {
    relic_row(&DATA(), _data_key(rid)).map_or(String::new(), |row| row.2)
}

/// As the game prints it ("Philosopher's Stone"): what the bot's tables key an offered relic by (events._ANCIENT).
pub fn relic_name(rid: &str) -> String {
    match relic_row(&DATA(), _data_key(rid)) {
        Some((name, _, _)) if !name.is_empty() => name,
        _ => py::title(&rid.replace('_', " ")),
    }
}

pub fn max_hp_penalty(_asc: i64, _character: &str) -> i64 {
    0
}

/// (no damaged start: StS2's A6 prices removal instead)
pub fn start_hp(run: &Run) -> i64 {
    run.max_hp
}

/// (no heal after a boss: the next act's Ancient heals, act_start)
pub fn boss_heal(_run: &Run) -> f64 {
    0.0
}

/// `random_relic(run, chest=False)`.
pub fn random_relic(run: &mut Run, _chest: bool) -> String {
    let r = run.rng.random();
    let tiers = RELIC_TIERS.with(|t| t.clone());
    let tier = if r < 0.5 {
        &tiers[0]
    } else if r < 0.83 {
        &tiers[1]
    } else {
        &tiers[2]
    };
    let mut left: Vec<&str> = tier.iter().filter(|x| !run.has(x)).map(|x| x.as_str()).collect();
    if left.is_empty() {
        left = tiers.iter().flatten().filter(|x| !run.has(x)).map(|x| x.as_str()).collect();
    }
    if left.is_empty() {
        left = vec!["ANCHOR"];
    }
    run.rng.choice(&left).to_string()
}

pub fn random_curse(run: &mut Run) -> Card {
    if CARDS().contains_key("REGRET") {
        let key = *run.rng.choice(&["REGRET", "DOUBT", "SHAME", "INJURY", "CLUMSY", "DECAY", "WRITHE"]);
        make(key, false)
    } else {
        make("WOUND", false)
    }
}

pub const _NO_POTION: &[&str] = &["ENTROPIC_BREW", "ASHWATER", "GAMBLERS_BREW", "DROPLET_OF_PRECOGNITION",
                                  "LIQUID_MEMORIES", "SNECKO_OIL", "TOUCH_OF_INSANITY", "STABLE_SERUM",
                                  "SOLDIERS_STEW", "BOTTLED_POTENTIAL", "COSMIC_CONCOCTION", "OROBIC_ACID",
                                  "GLOWWATER_POTION", "FOUL_POTION", "POTION_SHAPED_ROCK", "AMBERGRIS"];

fn _potions() -> IndexMap<String, (String, String, String)> {
    DATA()["potions"].entries().filter(|(k, _)| !_NO_POTION.contains(&k.as_str())).map(|(k, v)| (k.clone(), (v[0].s().to_string(), v[1].s().to_string(), v[2].s().to_string()))).collect()
}

/// key -> (name, rarity, text).
pub fn _POTIONS() -> Rc<IndexMap<String, (String, String, String)>> {
    POTION_TABLE.with(|p| p.clone())
}

pub fn _OWN_POTION(key: &str) -> Option<&'static str> {
    Some(match key {
        "FOCUS_POTION" | "POTION_OF_CAPACITY" | "ESSENCE_OF_DARKNESS" => "DEFECT",
        "CUNNING_POTION" => "SILENT",
        "BONE_BREW" | "POT_OF_GHOULS" | "POTION_OF_DOOM" => "NECROBINDER",
        "STAR_POTION" | "KINGS_COURAGE" => "REGENT",
        _ => return None,
    })
}

pub fn random_potion(run: &mut Run) -> Potion {
    let r = run.rng.random();
    let want = if r < 0.65 {
        "Common"
    } else if r < 0.9 {
        "Uncommon"
    } else {
        "Rare"
    };
    let potions = _POTIONS();
    let keys: Vec<&String> = potions.iter().filter(|(k, (_n, rar, _t))| rar == want && _OWN_POTION(k).unwrap_or(&run.character) == run.character).map(|(k, _)| k).collect();
    let key = *run.rng.choice(&keys);
    let (name, _rar, text) = &potions[key];
    Potion::new(key, name, text, want)
}

// ------------------------------------------------------------------ the run

pub fn _relic_options(rids: &[String]) -> Vec<Choice> {
    rids.iter()
        .map(|rid| {
            let name = relic_name(rid);
            let rid = rid.clone();
            let text = format!("Obtain {name}. {}", relic_text(&rid));
            (name, text, Some(Rc::new(move |r: &mut Run, _a: i64| r.gain_relic(&rid)) as Rc<dyn Fn(&mut Run, i64)>))
        })
        .collect()
}

/// Neow: two relics or a card removed, the bot's event handler choosing.
pub fn run_start(run: &mut Run) {
    let left: Vec<String> = COMMON().into_iter().chain(UNCOMMON()).filter(|r| !run.has(r)).collect();
    let mut options = _relic_options(&run.rng.sample(&left, 2));
    options.push(("Remove".to_string(), "Remove a card from your Deck.".to_string(), Some(Rc::new(|r: &mut Run, _a: i64| {
        r.remove_one();
    }))));
    sts1::_choose(run, options);
}

pub fn act_start(run: &mut Run) {
    run.fights_in_act = 0;
    run.unknown_odds = UNKNOWN_ODDS();
    if run.act == 1 {
        run.act_id = run.rng.choice(&["OVERGROWTH", "UNDERDOCKS"]).to_string();
        return;
    }
    run.act_id = match run.act {
        2 => "HIVE",
        3 => "GLORY",
        other => panic!("KeyError: {other}"),
    }
    .to_string();
    let heal = (run.max_hp - run.hp) as f64 * if run.asc < 2 { 1.0 } else { 0.8 };
    run.hp = run.max_hp.min(run.hp + py::trunc(heal));
    let left: Vec<String> = ANCIENT.iter().filter(|r| !run.has(r)).map(|r| r.to_string()).collect();
    let options = _relic_options(&run.rng.sample(&left, 3));
    sts1::_choose(run, options);
}

/// The act's row of the data (`next(a for a in DATA["acts"] if a["id"] == run.act_id)`).
pub fn _act(run: &Run) -> Value {
    DATA()["acts"].list().iter().find(|a| a["id"].s() == run.act_id).expect("StopIteration: the run's act").clone()
}

pub fn make_map(run: &mut Run) -> Nodes {
    let act = _act(run);
    let mut weights = ROOM_WEIGHTS();
    if run.asc >= 1 {
        weights["Elite"] *= 1.6;
    }
    let rows = act["map_rooms"].int();
    common::make_map(&mut run.rng, &weights, 6, rows - 6, rows)
}

pub fn encounter(run: &mut Run, kind: &str) -> Vec<Foe> {
    let act = _act(run);
    let pool_name = if kind == "monster" {
        run.fights_in_act += 1;
        if run.fights_in_act <= act["weak_fights"].int() {
            "weak"
        } else {
            "normal"
        }
    } else {
        kind
    };
    let options: Vec<String> = act[pool_name].as_array().unwrap_or_else(|| panic!("KeyError: {pool_name}")).iter().map(|e| e.s().to_string()).collect();
    let name = if kind == "boss" {
        if !run.bosses.contains_key(&run.act) {
            let drawn = run.rng.sample(&options, if run.act == 3 && run.asc >= 10 { 2 } else { 1 });
            run.bosses.insert(run.act, drawn);
        }
        run.bosses[&run.act][0].clone()
    } else {
        let recent = &run.recent[run.recent.len().saturating_sub(2)..];
        let mut fresh: Vec<&String> = options.iter().filter(|e| !recent.contains(e)).collect();
        if fresh.is_empty() {
            fresh = options.iter().collect();
        }
        (*run.rng.choice(&fresh)).clone()
    };
    run.recent.push(name.clone());
    let data = DATA();
    let enc = &data["encounters"][name.as_str()];
    let mut keys: Vec<String> = enc["monsters"].list().iter().map(|k| k.s().to_string()).collect();
    if enc["randomized"].truthy() {
        let alts: Vec<String> = if enc["options"].truthy() { enc["options"].list().iter().map(|k| k.s().to_string()).collect() } else { keys.clone() };
        let tail = |k: &str| k.rsplit('_').next().unwrap_or("").to_string();
        let mut drawn = Vec::with_capacity(keys.len());
        for k in &keys {
            let mut like: Vec<&String> = alts.iter().filter(|a| tail(a) == tail(k)).collect();
            if like.is_empty() {
                like = vec![k];
            }
            drawn.push((*run.rng.choice(&like)).clone());
        }
        keys = drawn;
    }
    // (slot: how many of its kind came before)
    (0..keys.len()).map(|n| _new(&keys[n], n as i64, run, keys[..n].iter().filter(|k| **k == keys[n]).count() as i64, keys.len() == 1, None)).collect()
}

pub fn _spec(key: &str, asc: i64) -> Rc<Spec> {
    if let Some(spec) = SPECS.with(|m| m.borrow().get(&(key.to_string(), asc)).cloned()) {
        return spec;
    }
    let data = DATA();
    let m = data["monsters"].dict().and_then(|m| m.get(key)).unwrap_or_else(|| panic!("KeyError: {key}"));
    let spec = Rc::new(sts2_foes::spec_of(key, m, asc));
    SPECS.with(|m| m.borrow_mut().insert((key.to_string(), asc), spec.clone()));
    spec
}

/// `_new(key, n, run, slot=0, alone=False, hp=None)`.
pub fn _new(key: &str, n: i64, run: &mut Run, slot: i64, alone: bool, hp: Option<i64>) -> Foe {
    let spec = _spec(key, run.asc);
    let hp = match hp {
        Some(hp) => hp,
        None => run.rng.randint(spec.hp.0, spec.hp.1),
    };
    sts2_foes::new_foe(&spec, n, hp, slot, alone)
}

/// `spawn(key, f, minion=False, hp=None)`.
pub fn spawn(key: &str, f: &mut Fight, minion: bool, hp: Option<i64>) -> Foe {
    let slot = f.foes.iter().filter(|x| x.key == key).count() as i64;
    let mut foe = _new(key, f.foes.len() as i64, &mut f.run, slot, false, hp);
    foe.minion = minion || foe.minion;
    foe.mem.set("fight", 1);
    foe
}

pub fn boss_won(run: &mut Run) {
    if run.act >= ACTS {
        return;
    }
    let offered = card_choices(run, "boss");
    run.card_reward(offered, true);
}

pub fn fight_gold(run: &mut Run, kind: &str) -> i64 {
    if run.has("ECTOPLASM") {
        return 0;
    }
    // (the Python builds the dict whole: both are drawn)
    let (monster, elite) = (run.rng.randint(10, 20), run.rng.randint(35, 45));
    let g = match kind {
        "monster" => monster,
        "elite" => elite,
        "boss" => 100,
        other => panic!("KeyError: {other}"),
    };
    py::trunc(g as f64 * if run.asc >= 3 { 0.75 } else { 1.0 })
}

pub fn card_choices(run: &mut Run, kind: &str) -> Vec<Card> {
    let n = 3 + run.has("QUESTION_CARD") as i64 - 2 * run.has("BUSTED_CROWN") as i64;
    let up = match run.act {
        1 => 0.0,
        2 => 0.25,
        _ => 0.5,
    } * if run.asc >= 7 { 0.5 } else { 1.0 };
    let odds = if run.asc >= 7 { SCARCE_ODDS() } else { CARD_ODDS() }; // (A7: rares half as common)
    let kind = if CARD_ODDS().contains_key(kind) || kind == "boss" { kind } else { "monster" };
    let own = pool(&run.character);
    common::offer(run, &own, n.max(1), &odds, kind, up)
}

pub fn shop_stock(run: &mut Run) -> Vec<Item> {
    let mut items = sts1::shop_stock(run);
    for it in items.iter_mut() {
        if it.category() == "card_removal" && run.asc >= 6 {
            let n = run.counters.val("removals");
            it.v.insert("price".to_string(), json!(100 + 50 * n));
            it.v.insert("cost".to_string(), json!(100 + 50 * n));
        }
        if it.category() == "relic" {
            let price = py::trunc(it.price() as f64 * 0.85);
            it.v.insert("price".to_string(), json!(price));
            it.v.insert("cost".to_string(), json!(price));
        }
    }
    items
}

pub fn setup_run(run: &mut Run) {
    if run.asc >= 4 {
        run.potions.truncate(2);
    }
    if run.asc >= 5 {
        run.deck.push(make("ASCENDERS_BANE", false));
    }
}

/// The engine's events, for Powers read from their text: a card played, a card Exhausted, Block gained (`ON(f, what, card=None)`).
pub fn ON(f: &mut Fight, what: &str, card: Option<&Card>) {
    if !f.triggers.is_empty() {
        trigger(f, what, card);
    }
    if what == "play" {
        for i in f.alive() {
            // the Aeonglass's Withering Presence: a Wither into our hand every 6 cards played
            let foe = &mut f.foes[i];
            if foe.powers.on("WITHERING_PRESENCE") {
                foe.powers["WITHERING_PRESENCE"] -= 1;
                if foe.powers["WITHERING_PRESENCE"] <= 0 {
                    foe.powers["WITHERING_PRESENCE"] = 6;
                    let level = foe.mem.get("wither_level", 0);
                    f.add_card(Card::new(&wither(level), false), "hand");
                }
            }
        }
    }
}

/// A Wither of the Aeonglass's level: each time it buffs, every Wither does 3 more.
pub fn wither(level: i64) -> Rc<CardDef> {
    if let Some(d) = WITHERS.with(|m| m.borrow().get(&level).cloned()) {
        return d;
    }
    let d = CARDS()["WITHER"].clone();
    let made = if level != 0 {
        Rc::new(CardDef { name: format!("Wither+{level}"), text: format!("Unplayable. At the end of your turn, if this is in your Hand, take {} damage.", 3 * level + 3), ..(*d).clone() })
    } else {
        d
    };
    WITHERS.with(|m| m.borrow_mut().insert(level, made.clone()));
    made
}

/// The game as the engine and the run hold it (Python's module `sim.content.sts2`).
pub fn content() -> Content {
    Content {
        KEY,
        ACTS,
        SHORT_REASON,
        KEEP_POTIONS: KEEP_POTIONS.iter().map(|k| k.to_string()).collect(),
        UNKNOWN_ODDS: UNKNOWN_ODDS(),
        CHARACTERS: CHARACTERS(),
        CARDS: CARDS(),
        COMMON: COMMON(),
        UNCOMMON: UNCOMMON(),
        DAMAGE: DAMAGE(),
        PRE: PRE(),
        AFTER: AFTER(),
        POWER: POWER(),
        PLAYABLE: PLAYABLE(),
        DRAWN: DRAWN(),
        RETAINED: RETAINED(),
        X_PLUS: X_PLUS(),
        EXTRA,
        START,
        TURN_START,
        TURN_CARDS,
        TURN_END,
        ROUND_END,
        POTION_CARD: common::potion_card,
        ON: Some(ON),
        make,
        pool,
        colorless,
        relic_text,
        relic_pickup: sts1::relic_pickup,
        random_relic,
        random_curse,
        random_potion,
        wither: Some(wither),
        setup_run,
        max_hp_penalty,
        start_hp,
        boss_heal,
        run_start,
        act_start,
        make_map,
        encounter,
        spawn,
        fight_won: sts1::fight_won,
        boss_won,
        fight_gold,
        card_choices,
        rest_options: sts1::rest_options,
        shop_stock,
        event: common::event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::py::Random;
    use crate::sim::monsters::Arg;

    /// What the Python builds from the data (tests/golden/sim_sts2.json, written by a scratch script from sim/content/sts2.py
    /// and sts2_foes.py): every monster's spec at Ascension 0, 8 and 9 and its opening state by slot, every Power's rules,
    /// the card table, the pools, the relic tiers and the potions.
    #[test]
    fn golden() {
        let _playing = crate::games::playing("sts2");
        let want: Value = serde_json::from_str(include_str!("../../../tests/golden/sim_sts2.json")).unwrap();
        let g = crate::sim::content::content("sts2");
        let data = DATA();
        let mut checked = 0;
        let run = Run::new(&g, "IRONCLAD", 1, None, 0);
        let mut f = Fight::new(run, vec![], "monster");
        for case in want["specs"].list() {
            let (key, asc) = (case["key"].s(), case["asc"].int());
            let m = &data["monsters"][key];
            let spec = Rc::new(sts2_foes::spec_of(key, m, asc));
            let moves: Vec<Value> = spec
                .moves
                .iter()
                .map(|(k, mv)| {
                    let fx: Vec<Value> = mv
                        .fx
                        .iter()
                        .map(|fx| {
                            let mut row = vec![json!(fx.word)];
                            row.extend(fx.args.iter().map(|a| match a {
                                Arg::Int(n) => json!(n),
                                Arg::Str(s) => json!(s),
                                Arg::Call(_) => json!("<call>"),
                            }));
                            Value::Array(row)
                        })
                        .collect();
                    json!([k, mv.dmg, mv.hits, mv.block, mv.kind, fx])
                })
                .collect();
            let states: IndexMap<String, Value> = m["states"].list().iter().map(|s| (s["id"].s().to_string(), s.clone())).collect();
            let mut firsts = Vec::new();
            for slot in 0..4 {
                for alone in [false, true] {
                    f.foes = vec![sts2_foes::new_foe(&spec, 0, spec.hp.0, slot, alone)];
                    f.run.rng = Random::new((slot * 2 + alone as i64 + 1) as i128);
                    let first = sts2_foes::_initial(&mut f, 0, m, &states);
                    firsts.push(json!([first, f.run.rng.random()]));
                }
            }
            let got = json!({"key": key, "asc": asc, "name": spec.name, "hp": [spec.hp.0, spec.hp.1], "powers": spec.powers.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
                             "minion": spec.minion, "block": spec.block, "hooks": spec.hooks.keys().collect::<Vec<_>>(), "moves": moves, "initial": firsts});
            assert_eq!(py::json_dumps(&got), py::json_dumps(case), "{key} at Ascension {asc}");
            checked += 1;
        }
        for case in want["power_rules"].list() {
            let rules: Vec<Value> = power_rules(case[0].s()).iter().map(|(when, what)| json!([when, what])).collect();
            assert_eq!(json!(rules), case[1], "{}", case[0]);
            checked += 1;
        }
        let mut kinds: Vec<(String, String)> = _PLAY_KIND().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        kinds.sort();
        assert_eq!(json!(kinds), want["play_kind"]);
        let cards = CARDS();
        let powers: Vec<&String> = cards.iter().filter(|(k, _)| g.POWER.contains_key(*k)).map(|(k, _)| k).collect();
        assert_eq!(json!(powers), want["power_keys"]);
        assert_eq!(powers.len(), g.POWER.len());
        assert_eq!(json!([COMMON(), UNCOMMON(), RARE()]), want["relics"]);
        for row in want["relic_names"].list() {
            assert_eq!(json!([row[0], relic_name(row[0].s()), relic_text(row[0].s())]), *row);
            checked += 1;
        }
        let potions: Vec<Value> = _POTIONS().iter().map(|(k, (name, rarity, text))| json!([k, name, rarity, text])).collect();
        assert_eq!(json!(potions), want["potions"]);
        let pools: serde_json::Map<String, Value> = CHARACTERS().keys().map(|ch| (ch.clone(), json!(pool(ch).iter().map(|d| d.key.clone()).collect::<Vec<_>>()))).collect();
        assert_eq!(py::json_dumps(&Value::Object(pools)), py::json_dumps(&want["pools"]));
        assert_eq!(json!(colorless().iter().map(|d| d.key.clone()).collect::<Vec<_>>()), want["colorless"]);
        let rows: Vec<Value> = cards.values().map(|d| json!([d.key, d.name, d.r#type, d.cost, d.rarity, d.targeted, d.text, d.text_up, d.cost_up, d.color, d.star_cost, d.star_cost_up, d.keywords, d.keywords_up])).collect();
        assert_eq!(rows.len(), want["cards"].list().len());
        for (got, wanted) in rows.iter().zip(want["cards"].list()) {
            assert_eq!(got, wanted);
            checked += 1;
        }
        let characters: serde_json::Map<String, Value> = CHARACTERS().iter().map(|(k, c)| (k.clone(), json!({"title": c.title, "hp": c.hp, "deck": c.deck, "relics": c.relics, "orbs": c.orbs}))).collect();
        assert_eq!(py::json_dumps(&Value::Object(characters)), py::json_dumps(&want["characters"]));
        let scarce: serde_json::Map<String, Value> = SCARCE_ODDS().iter().map(|(k, v)| (k.clone(), json!(v.iter().map(|(r, p)| json!([r, p])).collect::<Vec<_>>()))).collect();
        assert_eq!(py::json_dumps(&Value::Object(scarce)), py::json_dumps(&want["scarce"]));
        for row in want["wither"].list() {
            let d = wither(row[0].int());
            assert_eq!(json!([row[0], d.name, d.text]), *row);
        }
        for row in want["effect"].list() {
            let d = _effect_card(row[0].s());
            assert_eq!(json!([row[0], d.text, d.text_up]), *row);
        }
        let sorted = |keys: Vec<&String>| {
            let mut keys = keys;
            keys.sort();
            json!(keys)
        };
        assert_eq!(sorted(g.AFTER.keys().collect()), want["after"]);
        assert_eq!(sorted(g.PRE.keys().collect()), want["pre"]);
        assert_eq!(sorted(g.DAMAGE.keys().collect()), want["damage"]);
        for key in want["own_power"].list() {
            assert!(OWN_POWER(key.s()).is_some(), "{key}");
        }
        assert!(cards.keys().filter(|k| OWN_POWER(k).is_some()).count() <= want["own_power"].list().len());
        assert!(OWN_POWER("INFLAME").is_none());
        assert!(checked > 1200, "{checked} golden cases");
    }
}
