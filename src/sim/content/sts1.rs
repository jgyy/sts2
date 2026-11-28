//! The first Slay the Spire, as the simulator plays it: cards from the bot's own text tables, monsters from
//! sts1_foes.rs, and the run's rewards, shops, relics and potions at Ascension 0 values.

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::cards::CardInfo;
use crate::games::sts1::text;
use crate::games::sts1::text::potions as potion_tables;
use crate::base;
use crate::run::strategy;
use crate::sim::cards::{Card, CardDef};
use crate::sim::content::{common, sts1_cards, sts1_foes, Character, Choice, Content, Item, Nodes, Odds};
use crate::sim::effects::{add, Powers};
use crate::sim::engine::{Fight, Foe};
use crate::sim::run::{Potion, Run};

pub const KEY: &str = "sts1";
pub const ACTS: i64 = 3;
pub const SHORT_REASON: bool = false; // (CommunicationMod marks a card it cannot pay for unplayable and says no more)

pub fn UNKNOWN_ODDS() -> IndexMap<String, f64> {
    [("Monster", 0.1), ("Shop", 0.03), ("Treasure", 0.02)].into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub fn ROOM_WEIGHTS() -> IndexMap<String, f64> {
    [("Monster", 0.53), ("Unknown", 0.22), ("RestSite", 0.12), ("Shop", 0.05), ("Elite", 0.08)].into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub fn CARD_ODDS() -> Odds {
    let row = |odds: &[(&str, f64)]| odds.iter().map(|(r, p)| (r.to_string(), *p)).collect::<Vec<_>>();
    IndexMap::from([
        ("monster".to_string(), row(&[("Rare", 0.03), ("Uncommon", 0.37), ("Common", 0.60)])),
        ("elite".to_string(), row(&[("Rare", 0.10), ("Uncommon", 0.40), ("Common", 0.50)])),
    ])
}

pub fn CHARACTERS() -> IndexMap<String, Character> {
    let deck = |parts: &[(&str, usize)]| parts.iter().flat_map(|(k, n)| std::iter::repeat_n(k.to_string(), *n)).collect::<Vec<_>>();
    let who = |title: &str, hp: i64, deck: Vec<String>, relic: &str, orbs: i64| Character { title: title.to_string(), hp, deck, relics: vec![relic.to_string()], orbs };
    IndexMap::from([
        ("IRONCLAD".to_string(), who("The Ironclad", 80, deck(&[("STRIKE_R", 5), ("DEFEND_R", 4), ("BASH", 1)]), "BURNING_BLOOD", 0)),
        ("THE_SILENT".to_string(), who("The Silent", 70, deck(&[("STRIKE_G", 5), ("DEFEND_G", 5), ("NEUTRALIZE", 1), ("SURVIVOR", 1)]), "RING_OF_THE_SNAKE", 0)),
        ("DEFECT".to_string(), who("The Defect", 75, deck(&[("STRIKE_B", 4), ("DEFEND_B", 4), ("ZAP", 1), ("DUALCAST", 1)]), "CRACKED_CORE", 3)),
        ("WATCHER".to_string(), who("The Watcher", 72, deck(&[("STRIKE_P", 4), ("DEFEND_P", 4), ("ERUPTION", 1), ("VIGILANCE", 1)]), "PURE_WATER", 0)),
    ])
}

/// card key -> the character whose card it is.
pub fn COLORS() -> HashMap<&'static str, &'static str> {
    let mut out = HashMap::new();
    for (who, cards) in [("IRONCLAD", text::ironclad::CARDS()), ("THE_SILENT", text::silent::CARDS()), ("DEFECT", text::defect::CARDS()), ("WATCHER", text::watcher::CARDS())] {
        for k in cards.keys() {
            out.insert(*k, who);
        }
    }
    out
}

pub fn _defs() -> IndexMap<String, Rc<CardDef>> {
    let colors = COLORS();
    let mut out = IndexMap::new();
    for m in crate::re!(r"([A-Z_]+) (St|Cu|[ASP])(\d|X|U)([BCURS])(t?)").captures_iter(sts1_cards::META) {
        let (key, kind, cost, rarity, aimed) = (&m[1], &m[2], &m[3], &m[4], &m[5]);
        let cost_n: i64 = match cost {
            "X" => -1,
            "U" => -2,
            n => n.parse().expect("a cost"),
        };
        let (base, up) = text::UPGRADE_COST().get(key).copied().unwrap_or((cost_n, cost_n));
        let name = match sts1_cards::NAMES(key) {
            Some(name) => name.to_string(),
            None => base::title(&key.replace('_', " ")),
        };
        let ctype = sts1_cards::TYPES(kind);
        let color = match colors.get(key) {
            Some(who) => who,
            None => match ctype {
                "Status" => "STATUS",
                "Curse" => "CURSE",
                _ => "COLORLESS",
            },
        };
        let rar = match ctype {
            "Status" => "Status",
            "Curse" => "Curse",
            _ => sts1_cards::RARITIES(rarity),
        };
        out.insert(key.to_string(), Rc::new(CardDef {
            key: key.to_string(),
            name: name.clone(),
            r#type: ctype.to_string(),
            cost: base,
            rarity: rar.to_string(),
            targeted: !aimed.is_empty(),
            text: text::card_text(key, name.as_str(), false),
            text_up: text::card_text(key, name.as_str(), true),
            cost_up: up,
            color: color.to_string(),
            ..CardDef::default()
        }));
    }
    out
}

type Pool = Rc<Vec<Rc<CardDef>>>;

thread_local! {
    static CARD_TABLE: Rc<IndexMap<String, Rc<CardDef>>> = Rc::new(_defs());
    static POOLS: RefCell<HashMap<String, Pool>> = RefCell::new(HashMap::new());
    static COLORLESS: Pool = Rc::new(CARDS().values().filter(|d| d.color == "COLORLESS" && matches!(d.rarity.as_str(), "Uncommon" | "Rare") && !_NO_SHOP.contains(&d.key.as_str())).cloned().collect());
}

pub fn CARDS() -> Rc<IndexMap<String, Rc<CardDef>>> {
    CARD_TABLE.with(|c| c.clone())
}

/// `make(key, up=False)`.
pub fn make(key: &str, up: bool) -> Card {
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

pub const _NO_SHOP: &[&str] = &["DISCOVERY", "JACK_OF_ALL_TRADES", "CHRYSALIS", "METAMORPHOSIS", "TRANSMUTATION", "MAGNETISM", "MAYHEM", "THE_BOMB", "VIOLENCE",
                                "SECRET_TECHNIQUE", "SECRET_WEAPON", "MASTER_OF_STRATEGY", "PURITY", "FORETHOUGHT", "IMPATIENCE"];

// ------------------------------------------------------------------ hooks the engine asks for

pub fn EXTRA(f: &mut Fight, _c: &mut Card, _foe: Option<usize>, i: &CardInfo, xs: i64) {
    for _ in 0..xs {
        for kind in &i.channel {
            add(&mut f.pw, &format!("{kind}_CHANNELED"), 1);
        }
    }
}

pub fn START(f: &mut Fight) {
    if f.run.has("CRACKED_CORE") {
        f.channel("LIGHTNING");
    }
    let miracles = f.run.has("PURE_WATER") as i64 + 3 * f.run.has("HOLY_WATER") as i64;
    f.add_new("MIRACLE", miracles, "hand", false);
    if f.run.has("SNECKO_EYE") {
        f.pw.put("CONFUSED", 1);
    }
    if f.run.has("SLAVER_S_COLLAR") && f.kind != "monster" {
        f.pw.entry("ENERGY_NEXT_TURN".to_string()).or_insert(0);
    }
    for foe in f.foes.iter_mut() {
        if foe.key == "DARKLING" {
            foe.powers.put("REATTACH", foe.max_hp / 2);
        }
    }
}

pub fn TURN_START(f: &mut Fight) {
    if f.run.has("SLAVER_S_COLLAR") && f.kind != "monster" {
        f.energy += 1;
    }
    if f.run.has("INSERTER") && f.round % 2 == 0 {
        f.orb_slots += 1;
    }
}

pub fn TURN_CARDS(f: &mut Fight) {
    let own = pool(&f.run.character);
    for _ in 0..f.pw.val("CREATIVE_AI") {
        let choices: Vec<&Rc<CardDef>> = own.iter().filter(|d| d.r#type == "Power").collect();
        let c = Card::new(*f.run.rng.choice(&choices), false);
        f.add_card(c, "hand");
    }
    for _ in 0..f.pw.val("HELLO_WORLD") {
        let choices: Vec<&Rc<CardDef>> = own.iter().filter(|d| d.rarity == "Common").collect();
        let c = Card::new(*f.run.rng.choice(&choices), false);
        f.add_card(c, "hand");
    }
    for _ in 0..f.pw.val("MAYHEM") {
        let at = f.alive().first().copied();
        sts1_cards::_havoc(f, at);
    }
}

pub fn TURN_END(_f: &mut Fight) {}

// ------------------------------------------------------------------ relics

pub const COMMON: &[&str] = &["ANCHOR", "BAG_OF_MARBLES", "BAG_OF_PREPARATION", "BLOOD_VIAL", "BRONZE_SCALES", "HAPPY_FLOWER", "LANTERN", "MEAL_TICKET", "NUNCHAKU",
                              "ODDLY_SMOOTH_STONE", "ORICHALCUM", "PEN_NIB", "POTION_BELT", "PRESERVED_INSECT", "REGAL_PILLOW", "STRAWBERRY", "THE_BOOT",
                              "TOY_ORNITHOPTER", "VAJRA", "WAR_PAINT", "WHETSTONE", "DREAM_CATCHER", "CERAMIC_FISH", "SMILING_MASK", "MAW_BANK"];
pub const UNCOMMON: &[&str] = &["KUNAI", "SHURIKEN", "ORNAMENTAL_FAN", "LETTER_OPENER", "MEAT_ON_THE_BONE", "MERCURY_HOURGLASS", "HORN_CLEAT", "PEAR",
                                "GREMLIN_HORN", "PAPER_PHROG", "MOLTEN_EGG", "TOXIC_EGG", "FROZEN_EGG", "INK_BOTTLE", "QUESTION_CARD", "STRIKE_DUMMY",
                                "ETERNAL_FEATHER", "SINGING_BOWL"];
pub const RARE: &[&str] = &["DEAD_BRANCH", "GINGER", "GIRYA", "INCENSE_BURNER", "LIZARD_TAIL", "MANGO", "OLD_COIN", "PEACE_PIPE", "PRAYER_WHEEL", "SHOVEL",
                            "STONE_CALENDAR", "THREAD_AND_NEEDLE", "TORII", "TUNGSTEN_ROD", "TURNIP", "CALIPERS", "CAPTAIN_S_WHEEL", "FOSSILIZED_HELIX",
                            "CHARON_S_ASHES", "BIRD_FACED_URN", "MAGIC_FLOWER"];
pub const BOSS: &[&str] = &["COFFEE_DRIPPER", "CURSED_KEY", "ECTOPLASM", "FUSION_HAMMER", "PHILOSOPHER_S_STONE", "RUNIC_DOME", "SLAVER_S_COLLAR", "SOZU",
                            "VELVET_CHOKER", "BUSTED_CROWN", "SNECKO_EYE", "RUNIC_PYRAMID", "SACRED_BARK", "BLACK_STAR", "WHITE_BEAST_STATUE", "TINY_HOUSE",
                            "ASTROLABE", "PANDORA_S_BOX", "CALLING_BELL", "EMPTY_CAGE", "ORRERY"];

pub fn OWN_BOSS(character: &str) -> &'static [&'static str] {
    match character {
        "IRONCLAD" => &["BLACK_BLOOD", "MARK_OF_PAIN"],
        "THE_SILENT" => &["RING_OF_THE_SERPENT"],
        "DEFECT" => &["FROZEN_CORE", "NUCLEAR_BATTERY", "INSERTER"],
        "WATCHER" => &["HOLY_WATER", "VIOLET_LOTUS"],
        _ => panic!("KeyError: {character}"),
    }
}

pub const ENERGY: &[&str] = &["COFFEE_DRIPPER", "CURSED_KEY", "ECTOPLASM", "FUSION_HAMMER", "PHILOSOPHER_S_STONE", "RUNIC_DOME", "SOZU", "VELVET_CHOKER",
                              "BUSTED_CROWN", "MARK_OF_PAIN", "NUCLEAR_BATTERY"];

pub fn REPLACES(rid: &str) -> Option<&'static str> {
    Some(match rid {
        "BLACK_BLOOD" => "BURNING_BLOOD",
        "RING_OF_THE_SERPENT" => "RING_OF_THE_SNAKE",
        "FROZEN_CORE" => "CRACKED_CORE",
        "HOLY_WATER" => "PURE_WATER",
        _ => return None,
    })
}

pub fn TEXT(rid: &str) -> Option<&'static str> {
    Some(match rid {
        "VELVET_CHOKER" => "Gain 1 Energy at the start of each turn. You cannot play more than 6 cards per turn.",
        "COFFEE_DRIPPER" => "Gain 1 Energy at the start of each turn. You can no longer Rest at Rest Sites.",
        "FUSION_HAMMER" => "Gain 1 Energy at the start of each turn. You can no longer Smith at Rest Sites.",
        "ECTOPLASM" => "Gain 1 Energy at the start of each turn. You can no longer gain Gold.",
        _ => return potion_tables::RELICS().get(rid).copied(),
    })
}

pub fn relic_text(rid: &str) -> String {
    TEXT(rid).unwrap_or("").to_string()
}

pub fn relic_pickup(run: &mut Run, rid: &str) {
    let g = run.game.clone();
    if ENERGY.contains(&rid) {
        run.energy += 1;
    }
    if let Some(old) = REPLACES(rid) {
        if let Some(at) = run.relics.iter().position(|r| r == old) {
            run.relics.remove(at);
        }
    }
    let bump = match rid {
        "STRAWBERRY" => 7,
        "PEAR" => 10,
        "MANGO" => 14,
        "TINY_HOUSE" => 5,
        _ => 0,
    };
    run.max_hp += bump;
    run.hp += bump;
    match rid {
        "POTION_BELT" => run.potions.extend([None, None]),
        "OLD_COIN" => run.gold += 300,
        "WAR_PAINT" | "WHETSTONE" => {
            let want = if rid == "WAR_PAINT" { "Skill" } else { "Attack" };
            common::upgrade_random(run, |c| c.r#type() == want, 2);
        }
        "EMPTY_CAGE" => {
            run.remove_one();
            run.remove_one();
        }
        "PANDORA_S_BOX" => {
            let basics: Vec<Card> = run.deck.iter().filter(|c| c.d.rarity == "Basic" && matches!(c.name().trim_end_matches('+'), "Strike" | "Defend")).cloned().collect();
            let own = (g.pool)(&run.character);
            for c in basics {
                run.take_card(&c);
                let d = run.rng.choice(&own).clone();
                run.deck.push(Card::new(&d, false));
            }
        }
        "ASTROLABE" => {
            let own = (g.pool)(&run.character);
            for _ in 0..3 {
                if let Some(c) = run.choose_from_deck("transform", "Choose a card to Transform.", None) {
                    run.take_card(&c);
                    let d = run.rng.choice(&own).clone();
                    run.deck.push(Card::new(&d, true));
                }
            }
        }
        "CALLING_BELL" => {
            let curse = if g.CARDS.contains_key("CURSE_OF_THE_BELL") { (g.make)("CURSE_OF_THE_BELL", false) } else { (g.random_curse)(run) };
            run.deck.push(curse);
            for _ in 0..3 {
                let rid = (g.random_relic)(run, false);
                run.gain_relic(&rid);
            }
        }
        "TINY_HOUSE" => {
            run.gold += 50;
            run.upgrade_one();
            let p = (g.random_potion)(run);
            run.add_potion(p);
            let offered = (g.card_choices)(run, "monster");
            run.card_reward(offered, true);
        }
        "ORRERY" => {
            for _ in 0..5 {
                let offered = (g.card_choices)(run, "monster");
                run.card_reward(offered, true);
            }
        }
        _ => {}
    }
}

/// `random_relic(run, chest=False)`.
pub fn random_relic(run: &mut Run, chest: bool) -> String {
    let r = run.rng.random();
    let tier: &[&str] = if chest {
        let size = run.rng.random();
        if size < 0.5 {
            if r < 0.75 {
                COMMON
            } else {
                UNCOMMON
            }
        } else if size < 0.83 {
            if r < 0.35 {
                COMMON
            } else if r < 0.85 {
                UNCOMMON
            } else {
                RARE
            }
        } else if r < 0.75 {
            UNCOMMON
        } else {
            RARE
        }
    } else if r < 0.5 {
        COMMON
    } else if r < 0.83 {
        UNCOMMON
    } else {
        RARE
    };
    let mut left: Vec<&str> = tier.iter().filter(|x| !run.has(x)).copied().collect();
    if left.is_empty() {
        left = COMMON.iter().chain(UNCOMMON).chain(RARE).filter(|x| !run.has(x)).copied().collect();
    }
    if left.is_empty() {
        left = vec!["CIRCLET"];
    }
    run.rng.choice(&left).to_string()
}

pub fn random_curse(run: &mut Run) -> Card {
    make(run.rng.choice(&["REGRET", "DOUBT", "SHAME", "INJURY", "CLUMSY", "DECAY", "WRITHE"]), false)
}

pub fn random_potion(run: &mut Run) -> Potion {
    const NOT: [&str; 14] = ["FAIRY_IN_A_BOTTLE", "SMOKE_BOMB", "ENTROPIC_BREW", "ELIXIR", "GAMBLER_S_BREW", "LIQUID_MEMORIES", "SNECKO_OIL", "STANCE_POTION", "AMBROSIA", "BOTTLED_MIRACLE",
                             "ESSENCE_OF_DARKNESS", "POTION_OF_CAPACITY", "FOCUS_POTION", "CUNNING_POTION"];
    let mut names: Vec<&str> = potion_tables::POTIONS().keys().filter(|k| !NOT.contains(k)).copied().collect();
    let own: &[&str] = match run.character.as_str() {
        "IRONCLAD" => &[],
        "THE_SILENT" => &["CUNNING_POTION"],
        "DEFECT" => &["FOCUS_POTION", "ESSENCE_OF_DARKNESS", "POTION_OF_CAPACITY"],
        "WATCHER" => &["AMBROSIA", "BOTTLED_MIRACLE", "STANCE_POTION"],
        other => panic!("KeyError: {other}"),
    };
    names.extend(own);
    names.push("FAIRY_IN_A_BOTTLE");
    let key = *run.rng.choice(&names);
    Potion::new(key, &base::title(&key.replace("_S_", "'s ").replace('_', " ")), &text::potion_text(key, key), "Common")
}

// ------------------------------------------------------------------ the run

fn choice(title: &str, text: &str, effect: impl Fn(&mut Run, i64) + 'static) -> Choice {
    (title.to_string(), text.to_string(), Some(Rc::new(effect)))
}

/// Neow: three blessings, one of each kind, chosen by the bot's event handler.
pub fn run_start(run: &mut Run) {
    let first = match run.rng.choice_index(3) {
        0 => choice("Remove", "Remove a card from your Deck.", |r, _a| {
            r.remove_one();
        }),
        1 => choice("Upgrade", "Upgrade a card.", |r, _a| {
            r.upgrade_one();
        }),
        _ => choice("Learn", "Choose a card to add to your Deck.", |r, _a| {
            let offered = card_choices(r, "monster");
            r.card_reward(offered, false);
        }),
    };
    let second = match run.rng.choice_index(3) {
        0 => choice("Relic", "Obtain a random common Relic.", |r, _a| {
            let left: Vec<&str> = COMMON.iter().filter(|x| !r.has(x)).copied().collect();
            let rid = *r.rng.choice(&left);
            r.gain_relic(rid);
        }),
        1 => choice("Gold", "Gain 100 Gold.", |r, _a| r.gold += 100),
        _ => choice("Grow", &format!("Gain {} Max HP.", run.max_hp / 10), |r, _a| {
            r.max_hp += r.max_hp / 10;
            r.hp += r.max_hp / 11;
        }),
    };
    let third = match run.rng.choice_index(2) {
        0 => choice("Rare", "Lose all Gold. Choose a Rare card to add to your Deck.", |r, _a| {
            r.gold = 0;
            let offered = card_choices(r, "boss");
            r.card_reward(offered, false);
        }),
        _ => choice("Brew", "Obtain 3 random potions.", |r, _a| {
            for _ in 0..3 {
                let p = random_potion(r);
                r.add_potion(p);
            }
        }),
    };
    _choose(run, vec![first, second, third]);
}

pub fn _choose(run: &mut Run, options: Vec<Choice>) {
    let state = common::event_state(run, "NEOW", "Neow", &options, false);
    let mut pick = 0;
    for (action, params) in strategy::event(&state, &mut run.mem) {
        if action == "choose_event_option" {
            pick = params["index"].as_i64().expect("an index") as usize;
            break;
        }
    }
    run.digest.push(format!("N{}", options[pick].0.chars().take(2).collect::<String>()));
    if let Some(effect) = &options[pick].2 {
        effect(run, 0);
    }
}

pub fn act_start(run: &mut Run) {
    run.fights_in_act = 0;
}

pub fn make_map(run: &mut Run) -> Nodes {
    let mut weights = ROOM_WEIGHTS();
    if run.asc >= 1 {
        weights["Elite"] *= 1.6;
    }
    common::make_map(&mut run.rng, &weights, 6, 9, common::ROWS)
}

pub fn encounter(run: &mut Run, kind: &str) -> Vec<Foe> {
    let act = run.act;
    let pool_name = if kind == "monster" {
        run.fights_in_act += 1;
        if run.fights_in_act <= sts1_foes::WEAK_FIGHTS(act) {
            "weak"
        } else {
            "strong"
        }
    } else {
        kind
    };
    let table = sts1_foes::ENCOUNTERS();
    let options = &table[&act][pool_name];
    let recent = &run.recent[run.recent.len().saturating_sub(2)..];
    let mut fresh: Vec<&sts1_foes::Encounter> = options.iter().filter(|e| !recent.contains(&_name(e))).collect();
    if fresh.is_empty() {
        fresh = options.iter().collect();
    }
    let choice: sts1_foes::Encounter = if kind == "boss" {
        if !run.bosses.contains_key(&act) {
            let drawn = _name(run.rng.choice(options));
            run.bosses.insert(act, vec![drawn]);
        }
        let name = &run.bosses[&act][0];
        options.iter().find(|e| _name(e) == *name).expect("the act's boss").clone()
    } else {
        (*run.rng.choice(&fresh)).clone()
    };
    run.recent.push(_name(&choice));
    let keys: Vec<String> = match &choice {
        sts1_foes::Encounter::Keys(keys) => keys.iter().map(|k| k.to_string()).collect(),
        sts1_foes::Encounter::Drawn(_, rule) => rule(&mut run.rng),
    };
    let mut foes: Vec<Foe> = keys.iter().enumerate().map(|(n, key)| _new(key, n as i64, run, kind, None)).collect();
    if sts1_foes::MINIONS(&keys[0]).is_some() {
        for foe in foes.iter_mut().skip(1) {
            foe.minion = true;
        }
    }
    foes
}

/// An encounter's name for the "not the last two again" rule (its id for a drawn one).
pub fn _name(e: &sts1_foes::Encounter) -> String {
    match e {
        sts1_foes::Encounter::Keys(keys) => keys.join("+"),
        sts1_foes::Encounter::Drawn(name, _) => name.clone(),
    }
}

pub fn _hp_mult(run: &Run, kind: &str) -> f64 {
    let a = run.asc;
    1.0 + 0.08 * ((kind == "monster" && a >= 7) as i64 + (kind == "elite" && a >= 8) as i64 + (kind == "boss" && a >= 9) as i64) as f64
}

/// `_new(key, n, run, kind, hp=None)`.
pub fn _new(key: &str, n: i64, run: &mut Run, kind: &str, hp: Option<i64>) -> Foe {
    let specs = sts1_foes::S();
    let spec = specs.get(key).unwrap_or_else(|| panic!("KeyError: {key}"));
    match hp {
        Some(hp) => Foe::new(spec, n, hp, 1.0),
        None => {
            let hp = run.rng.randint(spec.hp.0, spec.hp.1);
            Foe::new(spec, n, hp, _hp_mult(run, kind))
        }
    }
}

/// `spawn(key, f, minion=False, hp=None)`.
pub fn spawn(key: &str, f: &mut Fight, minion: bool, hp: Option<i64>) -> Foe {
    let key = if key == "GREMLIN" { *f.run.rng.choice(&sts1_foes::GREMLINS) } else { key };
    let kind = f.kind.clone();
    let mut foe = _new(key, f.foes.len() as i64, &mut f.run, &kind, hp);
    foe.minion = minion || foe.minion;
    foe.mem.set("fight", 1);
    if let Some(hp) = hp {
        foe.max_hp = foe.max_hp.max(hp);
    }
    foe
}

pub fn fight_won(run: &mut Run, f: &Fight) {
    let mut heal = 6 * run.has("BURNING_BLOOD") as i64 + 12 * run.has("BLACK_BLOOD") as i64 + f.pw.val("SELF_REPAIR");
    if run.has("MEAT_ON_THE_BONE") && run.hp <= run.max_hp / 2 {
        heal += 12;
    }
    run.hp = run.max_hp.min(run.hp + heal);
    if run.has("MAW_BANK") {
        run.gold += 12;
    }
}

pub fn boss_won(run: &mut Run) {
    if run.act >= ACTS {
        return;
    }
    let left: Vec<&str> = BOSS.iter().chain(OWN_BOSS(&run.character)).filter(|b| !run.has(b)).copied().collect();
    let mut offered = run.rng.sample(&left, 3);
    base::sort_by_key(&mut offered, |r| text::boss_relic_rank(&json!({"id": r, "name": r})));
    let first = offered[0];
    run.gain_relic(first);
    run.digest.push(format!("b{first}"));
    let offered = card_choices(run, "boss");
    run.card_reward(offered, true);
}

pub fn fight_gold(run: &mut Run, kind: &str) -> i64 {
    if run.has("ECTOPLASM") {
        return 0;
    }
    // (the original builds the dict whole: all three are drawn)
    let (monster, elite, boss) = (run.rng.randint(10, 20), run.rng.randint(25, 35), run.rng.randint(95, 105));
    match kind {
        "monster" => monster,
        "elite" => elite,
        "boss" => boss,
        other => panic!("KeyError: {other}"),
    }
}

pub fn card_choices(run: &mut Run, kind: &str) -> Vec<Card> {
    let n = 3 + run.has("QUESTION_CARD") as i64 - 2 * run.has("BUSTED_CROWN") as i64;
    let up = match run.act {
        1 => 0.0,
        2 => 0.25,
        _ => 0.5,
    } * if run.asc >= 12 { 0.5 } else { 1.0 };
    let odds = CARD_ODDS();
    let kind = if odds.contains_key(kind) || kind == "boss" { kind } else { "monster" };
    let own = pool(&run.character);
    common::offer(run, &own, n.max(1), &odds, kind, up)
}

pub fn rest_options(run: &mut Run) -> Vec<Value> {
    let mut opts: Vec<&str> = Vec::new();
    if !run.has("COFFEE_DRIPPER") {
        opts.push("rest");
    }
    if !run.has("FUSION_HAMMER") {
        opts.push("smith");
    }
    if run.has("GIRYA") && run.counters.val("GIRYA") < 3 {
        opts.push("lift");
    }
    if run.has("PEACE_PIPE") {
        opts.push("toke");
    }
    if run.has("SHOVEL") {
        opts.push("dig");
    }
    if run.has("ETERNAL_FEATHER") {
        run.hp = run.max_hp.min(run.hp + 3 * (run.deck.len() as i64 / 5));
    }
    opts.iter().enumerate().map(|(i, o)| json!({"index": i, "id": o, "name": base::title(o), "is_enabled": true})).collect()
}

pub fn shop_stock(run: &mut Run) -> Vec<Item> {
    common::shop_stock(run, &[("Common", 50), ("Uncommon", 75), ("Rare", 150)], 1.2)
}

pub fn start_hp(run: &Run) -> i64 {
    if run.asc < 6 {
        run.max_hp
    } else {
        base::trunc(run.max_hp as f64 * 0.9) // (A6: start each run damaged)
    }
}

/// The share of missing HP healed on the way to the next act (A5: 75%).
pub fn boss_heal(run: &Run) -> f64 {
    if run.asc < 5 {
        1.0
    } else {
        0.75
    }
}

pub fn max_hp_penalty(asc: i64, character: &str) -> i64 {
    if asc >= 14 {
        if character == "IRONCLAD" {
            5
        } else {
            4
        }
    } else {
        0
    }
}

pub fn setup_run(run: &mut Run) {
    if run.asc >= 10 {
        run.deck.push(make("ASCENDER_S_BANE", false));
    }
    if run.asc >= 11 {
        run.potions.truncate(2);
    }
}

/// The game as the engine and the run hold it (the original's module `sim.content.sts1`).
pub fn content() -> Content {
    Content {
        KEY,
        ACTS,
        SHORT_REASON,
        KEEP_POTIONS: potion_tables::KEEP.iter().map(|k| k.to_string()).collect(),
        UNKNOWN_ODDS: UNKNOWN_ODDS(),
        CHARACTERS: CHARACTERS(),
        CARDS: CARDS(),
        COMMON: COMMON.iter().map(|r| r.to_string()).collect(),
        UNCOMMON: UNCOMMON.iter().map(|r| r.to_string()).collect(),
        DAMAGE: sts1_cards::DAMAGE(),
        PRE: sts1_cards::PRE(),
        AFTER: sts1_cards::AFTER(),
        POWER: sts1_cards::POWER(),
        PLAYABLE: sts1_cards::PLAYABLE(),
        DRAWN: sts1_cards::DRAWN(),
        RETAINED: sts1_cards::RETAINED(),
        X_PLUS: sts1_cards::X_PLUS(),
        EXTRA,
        START,
        TURN_START,
        TURN_CARDS,
        TURN_END,
        ROUND_END: sts1_foes::revive_round,
        POTION_CARD: common::potion_card,
        ON: None,
        make,
        pool,
        colorless,
        relic_text,
        relic_pickup,
        random_relic,
        random_curse,
        random_potion,
        wither: None,
        setup_run,
        max_hp_penalty,
        start_hp,
        boss_heal,
        run_start,
        act_start,
        make_map,
        encounter,
        spawn,
        fight_won,
        boss_won,
        fight_gold,
        card_choices,
        rest_options,
        shop_stock,
        event: common::event,
    }
}
