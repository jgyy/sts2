//! The first game's cards: what the text tables (crate::games::sts1::text) leave out, and what their text cannot say.
//!
//! META: type, cost, rarity and whether the card is aimed ("t"), one card a line. Then the handlers effects.rs asks
//! for (see its doc comment), for the cards whose text says more than the planner's parser reads, or says it under a
//! condition.
//!
//! The tables are functions that build them (`AFTER()`, `PRE()` ...): the game's `Content` keeps one of each, and
//! Slay the Spire 2's takes the rules it shares out of another.

#![allow(non_snake_case)]

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::cards::CardInfo;
use crate::sim::cards::Card;
use crate::sim::content::common;
use crate::sim::content::{AfterRule, DamageRule, HandRule, PlayableRule, PowerRule, PreRule};
use crate::sim::effects::{self as fx, add, Powers};
use crate::sim::engine::{Fight, Pile};

// key  type cost rarity [t]   (type: A S P St Cu; cost: number, X, U = unplayable; rarity: B C U R S = basic, common, uncommon, rare, special)
pub const META: &str = "
STRIKE_R A1Bt DEFEND_R S1B BASH A2Bt
ANGER A0Ct ARMAMENTS S1C BODY_SLAM A1Ct CLASH A0Ct CLEAVE A1C CLOTHESLINE A2Ct FLEX S0C HAVOC S1C HEADBUTT A1Ct HEAVY_BLADE A2Ct
IRON_WAVE A1Ct PERFECTED_STRIKE A2Ct POMMEL_STRIKE A1Ct SHRUG_IT_OFF S1C SWORD_BOOMERANG A1C THUNDERCLAP A1C TRUE_GRIT S1C TWIN_STRIKE A1Ct
WARCRY S0C WILD_STRIKE A1Ct
BATTLE_TRANCE S0U BLOOD_FOR_BLOOD A4Ut BLOODLETTING S0U BURNING_PACT S1U CARNAGE A2Ut COMBUST P1U DARK_EMBRACE P2U DISARM S1Ut DROPKICK A1Ut
DUAL_WIELD S1U ENTRENCH S2U EVOLVE P1U FEEL_NO_PAIN P1U FIRE_BREATHING P1U FLAME_BARRIER S2U GHOSTLY_ARMOR S1U HEMOKINESIS A1Ut
INFERNAL_BLADE S1U INFLAME P1U INTIMIDATE S0U METALLICIZE P1U POWER_THROUGH S1U PUMMEL A1Ut RAGE S0U RAMPAGE A1Ut RECKLESS_CHARGE A0Ut
RUPTURE P1U SEARING_BLOW A2Ut SECOND_WIND S1U SEEING_RED S1U SENTINEL S1U SEVER_SOUL A2Ut SHOCKWAVE S2U SPOT_WEAKNESS S1Ut UPPERCUT A2Ut
WHIRLWIND AXU
BARRICADE P3R BERSERK P0R BLUDGEON A3Rt BRUTALITY P0R CORRUPTION P3R DEMON_FORM P3R DOUBLE_TAP S1R EXHUME S1R FEED A1Rt FIEND_FIRE A2Rt
IMMOLATE A2R IMPERVIOUS S2R JUGGERNAUT P2R LIMIT_BREAK S1R OFFERING S0R REAPER A2R
STRIKE_G A1Bt DEFEND_G S1B NEUTRALIZE A0Bt SURVIVOR S1B
ACROBATICS S1C BACKFLIP S1C BANE A1Ct BLADE_DANCE S1C CLOAK_AND_DAGGER S1C DAGGER_SPRAY A1C DAGGER_THROW A1Ct DEADLY_POISON S1Ct
DEFLECT S0C DODGE_AND_ROLL S1C FLYING_KNEE A1Ct OUTMANEUVER S1C PIERCING_WAIL S1C POISONED_STAB A1Ct PREPARED S0C QUICK_SLASH A1Ct
SLICE A0Ct SNEAKY_STRIKE A2Ct SUCKER_PUNCH A1Ct
ACCURACY P1U ALL_OUT_ATTACK A1U BACKSTAB A0Ut BLUR S1U BOUNCING_FLASK S2U CALCULATED_GAMBLE S0U CALTROPS P1U CATALYST S1Ut CHOKE A2Ut
CONCENTRATE S0U CRIPPLING_CLOUD S2U DASH A2Ut DISTRACTION S1U ENDLESS_AGONY A0Ut ESCAPE_PLAN S0U EVISCERATE A3Ut EXPERTISE S1U
FINISHER A1Ut FLECHETTES A1Ut FOOTWORK P1U HEEL_HOOK A1Ut INFINITE_BLADES P1U LEG_SWEEP S2Ut MASTERFUL_STAB A0Ut NOXIOUS_FUMES P1U
PREDATOR A2Ut REFLEX SUU RIDDLE_WITH_HOLES A2Ut SETUP S1U SKEWER AXUt TACTICIAN SUU TERROR S1Ut WELL_LAID_PLANS P1U
A_THOUSAND_CUTS P2R ADRENALINE S0R AFTER_IMAGE P1R ALCHEMIZE S1R BULLET_TIME S3R BURST S1R CORPSE_EXPLOSION S2Rt DIE_DIE_DIE A1R
DOPPELGANGER SXR ENVENOM P2R GLASS_KNIFE A1Rt GRAND_FINALE A0R MALAISE SXRt NIGHTMARE S3R PHANTASMAL_KILLER S1R STORM_OF_STEEL S1R
TOOLS_OF_THE_TRADE P1R UNLOAD A1Rt WRAITH_FORM P3R
STRIKE_B A1Bt DEFEND_B S1B ZAP S1B DUALCAST S1B
BALL_LIGHTNING A1Ct BARRAGE A1Ct BEAM_CELL A0Ct CHARGE_BATTERY S1C CLAW A0Ct COLD_SNAP A1Ct COMPILE_DRIVER A1Ct COOLHEADED S1C
GO_FOR_THE_EYES A0Ct HOLOGRAM S1C LEAP S1C REBOUND A1Ct RECURSION S1C STACK S1C STEAM_BARRIER S0C STREAMLINE A2Ct SWEEPING_BEAM A1C TURBO S0C
AGGREGATE S1U AUTO_SHIELDS S1U BLIZZARD A1U BOOT_SEQUENCE S0U BULLSEYE A1Ut CAPACITOR P1U CHAOS S1U CHILL S0U CONSUME S2U DARKNESS S1U
DEFRAGMENT P1U DOOM_AND_GLOOM A2U DOUBLE_ENERGY S1U EQUILIBRIUM S2U FORCE_FIELD S4U FTL A0Ut FUSION S2U GENETIC_ALGORITHM S1U GLACIER S2U
HEATSINKS P1U HELLO_WORLD P1U LOOP P1U MELTER A1Ut OVERCLOCK S0U RECYCLE S1U REINFORCED_BODY SXU REPROGRAM S1U RIP_AND_TEAR A1U
SCRAPE A1Ut SELF_REPAIR P1U SKIM S1U STATIC_DISCHARGE P1U STORM P1U SUNDER A3Ut TEMPEST SXU WHITE_NOISE S1U
ALL_FOR_ONE A2Rt AMPLIFY S1R BIASED_COGNITION P1R BUFFER P2R CORE_SURGE A1Rt CREATIVE_AI P3R ECHO_FORM P3R ELECTRODYNAMICS P2R FISSION S0R
HYPERBEAM A2R MACHINE_LEARNING P1R METEOR_STRIKE A5Rt MULTI_CAST SXR RAINBOW S2R REBOOT S0R SEEK S0R THUNDER_STRIKE A3R
STRIKE_P A1Bt DEFEND_P S1B ERUPTION A2Bt VIGILANCE S2B
BOWLING_BASH A1Ct CONSECRATE A0C CRESCENDO S1C CRUSH_JOINTS A1Ct CUT_THROUGH_FATE A1Ct EMPTY_BODY S1C EMPTY_FIST A1Ct EVALUATE S1C
FLURRY_OF_BLOWS A0Ct FLYING_SLEEVES A1Ct FOLLOW_UP A1Ct HALT S0C JUST_LUCKY A0Ct PRESSURE_POINTS S1Ct PROSTRATE S0C PROTECT S2C SASH_WHIP A1Ct
TRANQUILITY S1C THIRD_EYE S1C
BATTLE_HYMN P1U CARVE_REALITY A1Ut COLLECT SXU CONCLUDE A1U DECEIVE_REALITY S1U EMPTY_MIND S1U FASTING P2U FEAR_NO_EVIL A1Ut
FOREIGN_INFLUENCE S0U FORESIGHT P1U INDIGNATION S1U INNER_PEACE S1U LIKE_WATER P1U MEDITATE S1U MENTAL_FORTRESS P1U NIRVANA P1U
PERSEVERANCE S1U PRAY S1U REACH_HEAVEN A2Ut RUSHDOWN P1U SANCTITY S1U SANDS_OF_TIME A4Ut SIGNATURE_MOVE A2Ut SIMMERING_FURY S1U STUDY P2U
SWIVEL S2U TALK_TO_THE_HAND A1Ut TANTRUM A1Ut WALLOP A2Ut WAVE_OF_THE_HAND S1U WEAVE A0Ut WHEEL_KICK A2Ut WINDMILL_STRIKE A2Ut WORSHIP S2U
WREATH_OF_FLAME S1U
ALPHA S1R BLASPHEMY S1R BRILLIANCE A1Rt CONJURE_BLADE SXR DEUS_EX_MACHINA SUR DEVA_FORM P3R DEVOTION P1R ESTABLISHMENT P1R JUDGMENT S1Rt
LESSON_LEARNED A2Rt MASTER_REALITY P1R OMNISCIENCE S4R RAGNAROK A3R SCRAWL S1R SPIRIT_SHIELD S2R VAULT S3R WISH S3R
BANDAGE_UP S0U BLIND S0Ut DARK_SHACKLES S0Ut DEEP_BREATH S0U DISCOVERY S1U DRAMATIC_ENTRANCE A0U ENLIGHTENMENT S0U FINESSE S0U
FLASH_OF_STEEL A0Ut FORETHOUGHT S0U GOOD_INSTINCTS S0U IMPATIENCE S0U JACK_OF_ALL_TRADES S0U MADNESS S1U MIND_BLAST A2Ut PANACEA S0U
PANIC_BUTTON S0U PURITY S0U SWIFT_STRIKE A0Ut TRIP S0Ut
APOTHEOSIS S2R CHRYSALIS S2R HAND_OF_GREED A2Rt MAGNETISM P2R MASTER_OF_STRATEGY S0R MAYHEM P2R METAMORPHOSIS S2R PANACHE P0R
SADISTIC_NATURE P0R SECRET_TECHNIQUE S0R SECRET_WEAPON S0R THE_BOMB S2R THINKING_AHEAD S0R TRANSMUTATION SXR VIOLENCE S0R
SHIV A0St MIRACLE S0S SMITE A1St INSIGHT S0S SAFETY S1S THROUGH_VIOLENCE A0St BETA S2S OMEGA P3S EXPUNGER A1St APPARITION S1S
BITE A1St J_A_X S0S RITUAL_DAGGER A1St
BURN StUS DAZED StUS SLIMED St1S VOID StUS WOUND StUS
ASCENDER_S_BANE CuUS CLUMSY CuUS CURSE_OF_THE_BELL CuUS DECAY CuUS DOUBT CuUS INJURY CuUS NECRONOMICURSE CuUS NORMALITY CuUS PAIN CuUS
PARASITE CuUS PRIDE Cu1S REGRET CuUS SHAME CuUS WRITHE CuUS
";

pub fn TYPES(kind: &str) -> &'static str {
    match kind {
        "A" => "Attack",
        "S" => "Skill",
        "P" => "Power",
        "St" => "Status",
        "Cu" => "Curse",
        _ => panic!("KeyError: {kind}"),
    }
}

pub fn RARITIES(rarity: &str) -> &'static str {
    match rarity {
        "B" => "Basic",
        "C" => "Common",
        "U" => "Uncommon",
        "R" => "Rare",
        "S" => "Event",
        _ => panic!("KeyError: {rarity}"),
    }
}

pub fn NAMES(key: &str) -> Option<&'static str> {
    Some(match key {
        "STRIKE_R" | "STRIKE_G" | "STRIKE_B" | "STRIKE_P" => "Strike",
        "DEFEND_R" | "DEFEND_G" | "DEFEND_B" | "DEFEND_P" => "Defend",
        "J_A_X" => "J.A.X.",
        "ASCENDER_S_BANE" => "Ascender's Bane",
        "A_THOUSAND_CUTS" => "A Thousand Cuts",
        "FTL" => "FTL",
        "DIE_DIE_DIE" => "Die Die Die",
        _ => return None,
    })
}

/// X cards whose upgrade adds one (Malaise: X Strength down and X Weak, both).
pub fn X_PLUS() -> HashSet<String> {
    ["MALAISE", "TEMPEST", "MULTI_CAST", "DOPPELGANGER", "COLLECT"].iter().map(|k| k.to_string()).collect()
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

fn p(rule: impl Fn(&mut Fight, &mut Card) + 'static) -> PowerRule {
    Rc::new(rule)
}

/// `replace(i, ...)`: the card read with these fields changed.
pub fn replace(i: &Rc<CardInfo>, change: impl FnOnce(&mut CardInfo)) -> Option<Rc<CardInfo>> {
    let mut read = (**i).clone();
    change(&mut read);
    Some(Rc::new(read))
}

fn gain(f: &mut Fight, c: &Card, triples: &[(&'static str, i64, i64)]) {
    for (k, base, up) in triples {
        add(&mut f.pw, k, if c.up { *up } else { *base });
    }
}

/// A handler that adds to our powers: for each power its key, the amount, the amount once upgraded.
pub fn _gain(triples: &[(&'static str, i64, i64)]) -> AfterRule {
    let triples = triples.to_vec();
    a(move |f, c, _, _, _| gain(f, c, &triples))
}

/// `_gain` as a Power's rule.
pub fn _gain_power(triples: &[(&'static str, i64, i64)]) -> PowerRule {
    let triples = triples.to_vec();
    p(move |f, c| gain(f, c, &triples))
}

/// `_to_hand(key, n=1, where="hand")`.
pub fn _to_hand(key: &'static str, n: i64, where_: &'static str) -> AfterRule {
    a(move |f, c, _, _, _| f.add_new(key, n, where_, c.up && matches!(key, "SMITE" | "SAFETY" | "INSIGHT" | "THROUGH_VIOLENCE" | "EXPUNGER")))
}

/// Read the card as if the named fields were 0 unless `test(f, c, foe)` holds (`zero` sets them to 0).
pub fn _if(test: impl Fn(&mut Fight, &Card, Option<usize>) -> bool + 'static, zero: fn(&mut CardInfo)) -> PreRule {
    pre(move |f, c, foe, i| if test(f, c, foe) { Some(i) } else { replace(&i, zero) })
}

pub fn _attacking(f: &mut Fight, _c: &Card, foe: Option<usize>) -> bool {
    match foe {
        Some(foe) => {
            let m = f.foes[foe].r#move();
            m.dmg > 0 && m.kind.is_empty()
        }
        None => false,
    }
}

/// Exhaust these cards (by `uid`) from the hand; how many of them.
pub fn _exhaust(f: &mut Fight, cards: Vec<u64>) -> i64 {
    for uid in &cards {
        if let Some(card) = f.take_from(Pile::Hand, *uid) {
            f.exhaust(card);
        }
    }
    cards.len() as i64
}

/// `_exhaust_pick(n, prompt="Choose {n} card(s) to Exhaust.")(f)`: `prompt` as it reads with `n` in it.
pub fn _exhaust_pick(f: &mut Fight, n: i64, prompt: &str) {
    for card in f.hand_pick(prompt, n, None) {
        if let Some(card) = f.take_from(Pile::Hand, card.uid) {
            f.exhaust(card);
        }
    }
}

/// Discard these cards (by `uid`) from the hand (by a card's say); how many of them.
pub fn _discard(f: &mut Fight, cards: Vec<u64>) -> i64 {
    for uid in &cards {
        if let Some(h) = f.take_from(Pile::Hand, *uid) {
            let seen = h.clone();
            f.discard.push(h);
            fx::discarded(f, &seen);
        }
    }
    cards.len() as i64
}

fn hand_uids(f: &Fight, keep: impl Fn(&Card) -> bool) -> Vec<u64> {
    f.hand.iter().filter(|h| keep(h)).map(|h| h.uid).collect()
}

/// Up to n cards the bot picks from a pile (or from `pool`, part of it), each taken out and put in the hand (or by `put`).
/// (`_pick(f, prompt, pile, pool=None, n=1, put=None)`.)
pub fn _pick(f: &mut Fight, prompt: &str, pile: Pile, pool: Option<Vec<Card>>, n: i64, put: Option<fn(&mut Fight, Card)>) {
    let pool = pool.unwrap_or_else(|| f.pile(pile).clone());
    for card in f.pile_pick(prompt, &pool, n) {
        let card = f.take_from(pile, card.uid).expect("the card picked is in its pile");
        match put {
            Some(put) => put(f, card),
            None => f.add_card(card, "hand"),
        }
    }
}

/// A card of the hand on top of the draw pile (Warcry, Thinking Ahead; Setup: free to play this fight).
pub fn _to_top(f: &mut Fight, free: bool) {
    for card in f.hand_pick("Put a card on top of your draw pile.", 1, None) {
        let mut card = f.take_from(Pile::Hand, card.uid).expect("the card picked is in the hand");
        if free {
            card.cost_fight = Some(0);
        }
        f.draw.push(card);
    }
}

pub fn _random_card(kind: &'static str) -> AfterRule {
    a(move |f, _, _, _, _| {
        let pool = (f.game.pool)(&f.run.character);
        let choices: Vec<_> = pool.iter().filter(|d| d.r#type == kind).cloned().collect();
        common::free_card(f, &choices);
    })
}

pub fn _scry(base: i64, up: i64) -> AfterRule {
    a(move |f, c, _, _, _| fx::scry(f, if c.up { up } else { base }))
}

pub fn _secret(kind: &'static str) -> AfterRule {
    a(move |f, _, _, _, _| {
        let pool: Vec<Card> = f.draw.iter().filter(|d| d.r#type() == kind).cloned().collect();
        _pick(f, "Choose a card to put into your hand.", Pile::Draw, Some(pool), 1, None);
    })
}

pub fn _second_wind(f: &mut Fight, c: &Card) {
    for uid in hand_uids(f, |h| h.r#type() != "Attack") {
        if let Some(h) = f.take_from(Pile::Hand, uid) {
            f.exhaust(h);
        }
        let b = f.block_value(if c.up { 7 } else { 5 });
        f.gain_block(b);
    }
}

pub fn _havoc(f: &mut Fight, foe: Option<usize>) {
    f.refill();
    if let Some(mut top) = f.draw.pop() {
        if top.cost() >= -1 {
            let at = match foe {
                Some(i) if f.foes[i].alive() => Some(i),
                _ => f.alive().first().copied(),
            };
            let x = if top.cost() == -1 { f.energy } else { 0 };
            fx::resolve(f, &mut top, at, x);
        }
        f.exhaust(top);
    }
}

pub fn _feed(f: &mut Fight, c: &Card, foe: Option<usize>) {
    if let Some(foe) = foe {
        if !f.foes[foe].alive() && !f.foes[foe].minion {
            f.run.max_hp += if c.up { 4 } else { 3 };
            f.heal(if c.up { 4 } else { 3 });
        }
    }
}

pub fn _recursion(f: &mut Fight) {
    if let Some(kind) = f.orbs.first().map(|o| o.kind.clone()) {
        f.evoke(1, false);
        f.channel(&kind);
    }
}

/// (No table names it: in the Python its row of AFTER sits inside a comment.)
pub fn _scrape(f: &mut Fight, c: &Card) {
    let before = f.hand.len();
    f.draw_cards(if c.up { 5 } else { 4 });
    let drawn: Vec<u64> = f.hand.iter().skip(before).filter(|h| h.cost() != 0).map(|h| h.uid).collect();
    for uid in drawn {
        if let Some(h) = f.take_from(Pile::Hand, uid) {
            f.discard.push(h);
        }
    }
}

pub fn _fission(f: &mut Fight, c: &Card) {
    let n = f.orbs.len() as i64;
    while !f.orbs.is_empty() {
        if c.up {
            f.evoke(1, false);
        } else {
            f.orbs.remove(0);
        }
    }
    f.energy += n;
    f.draw_cards(n);
}

pub fn _all_for_one(f: &mut Fight) {
    let free: Vec<u64> = f.discard.iter().filter(|h| h.cost() == 0).map(|h| h.uid).collect();
    for uid in free {
        if f.hand.len() < 10 {
            if let Some(h) = f.take_from(Pile::Discard, uid) {
                f.hand.push(h);
            }
        }
    }
}

pub fn DAMAGE() -> HashMap<String, DamageRule> {
    let rows: Vec<(&str, DamageRule)> = vec![
        ("BODY_SLAM", d(|f, _c| f.block)),
        ("PERFECTED_STRIKE", d(|f, c| {
            let strikes = f.hand.iter().chain(&f.draw).chain(&f.discard).chain(&f.exhausted).filter(|x| x.d.name.contains("Strike")).count() as i64;
            6 + (if c.up { 3 } else { 2 }) * (strikes + 1)
        })),
        ("HEAVY_BLADE", d(|f, c| 14 + ((if c.up { 5 } else { 3 }) - 1) * f.strength())),
        ("MIND_BLAST", d(|f, _c| f.draw.len() as i64)),
        ("BOWLING_BASH", d(|_f, c| if c.up { 10 } else { 7 })),
        ("FLECHETTES", d(|_f, c| if c.up { 6 } else { 4 })),
        ("FINISHER", d(|_f, c| if c.up { 8 } else { 6 })),
        ("BARRAGE", d(|_f, c| if c.up { 7 } else { 5 })),
        ("BRILLIANCE", d(|f, c| (if c.up { 16 } else { 12 }) + f.pw.val("MANTRA_GAINED"))),
        ("BLIZZARD", d(|f, c| (if c.up { 3 } else { 2 }) * f.pw.val("FROST_CHANNELED"))),
        ("THUNDER_STRIKE", d(|_f, c| if c.up { 9 } else { 7 })),
        ("EXPUNGER", d(|_f, c| if c.up { 15 } else { 9 })),
        ("RAMPAGE", d(|_f, c| 8 + c.bonus)),
        ("CLAW", d(|f, c| (if c.up { 5 } else { 3 }) + f.pw.val("CLAW_BONUS"))),
        ("GLASS_KNIFE", d(|_f, c| (if c.up { 12 } else { 8 }) + c.bonus)),
        ("WINDMILL_STRIKE", d(|_f, c| (if c.up { 10 } else { 7 }) + c.bonus)),
    ];
    rows.into_iter().map(|(k, r)| (k.to_string(), r)).collect()
}

pub fn PRE() -> HashMap<String, PreRule> {
    let rows: Vec<(&str, PreRule)> = vec![
        ("DROPKICK", _if(|f, _c, foe| foe.is_some_and(|i| f.foes[i].powers.on("VULNERABLE")), |i| {
            i.energy_gain = 0;
            i.draw = 0;
        })),
        ("HEEL_HOOK", _if(|f, _c, foe| foe.is_some_and(|i| f.foes[i].powers.on("WEAK")), |i| {
            i.energy_gain = 0;
            i.draw = 0;
        })),
        ("SPOT_WEAKNESS", _if(_attacking, |i| i.strength = 0)),
        ("GO_FOR_THE_EYES", _if(_attacking, |i| i.weak = 0)),
        ("FEAR_NO_EVIL", _if(_attacking, |i| i.stance = String::new())),
        ("FOLLOW_UP", _if(|f, _c, _foe| f.last_type == "Attack", |i| i.energy_gain = 0)),
        ("SANCTITY", _if(|f, _c, _foe| f.last_type == "Skill", |i| i.draw = 0)),
        ("CRUSH_JOINTS", _if(|f, _c, _foe| f.last_type == "Skill", |i| i.vulnerable = 0)),
        ("SASH_WHIP", _if(|f, _c, _foe| f.last_type == "Attack", |i| i.weak = 0)),
        ("FTL", _if(|f, c, _foe| (f.turn_cards.len() as i64 - 1) < if c.up { 4 } else { 3 }, |i| i.draw = 0)),
        ("SNEAKY_STRIKE", _if(|f, _c, _foe| f.pw.on("DISCARDED_TURN"), |i| i.energy_gain = 0)),
        ("EVISCERATE", pre(|_f, _c, _foe, i| replace(&i, |i| i.hits = 3))),
        ("FIEND_FIRE", pre(|f, _c, _foe, i| {
            let hand = hand_uids(f, |_| true);
            let n = _exhaust(f, hand);
            replace(&i, |i| i.hits = n)
        })),
        ("BURNING_PACT", pre(|f, _c, _foe, i| {
            _exhaust_pick(f, 1, "Choose 1 card(s) to Exhaust.");
            Some(i)
        })),
        ("SECOND_WIND", pre(|_f, _c, _foe, i| replace(&i, |i| i.block = 0))),
        ("COMPILE_DRIVER", pre(|f, _c, _foe, i| {
            let kinds: HashSet<&str> = f.orbs.iter().map(|o| o.kind.as_str()).collect();
            let n = kinds.len() as i64;
            replace(&i, |i| i.draw = n)
        })),
        ("FINISHER", pre(|f, _c, _foe, i| replace(&i, |i| i.hits = f.attacks_turn.max(0)))),
        ("FLECHETTES", pre(|f, _c, _foe, i| replace(&i, |i| i.hits = f.hand.iter().filter(|h| h.r#type() == "Skill").count() as i64))),
        ("BARRAGE", pre(|f, _c, _foe, i| replace(&i, |i| i.hits = f.orbs.len() as i64))),
        ("BOWLING_BASH", pre(|f, _c, _foe, i| replace(&i, |i| i.hits = f.alive().len() as i64))),
        ("THUNDER_STRIKE", pre(|f, _c, _foe, i| {
            replace(&i, |i| {
                i.hits = f.pw.val("LIGHTNING_CHANNELED");
                i.random_target = true;
            })
        })),
        ("EXPUNGER", pre(|f, _c, _foe, i| replace(&i, |i| i.hits = f.pw.get("EXPUNGER_X").copied().unwrap_or(1).max(1)))),
        ("MELTER", pre(|f, _c, foe, i| {
            if let Some(foe) = foe {
                f.foes[foe].block = 0;
            }
            Some(i)
        })),
        ("AUTO_SHIELDS", pre(|_f, _c, _foe, i| replace(&i, |i| i.block = 0))),
        ("BLIZZARD", pre(|_f, _c, _foe, i| replace(&i, |i| i.aoe = true))),
    ];
    rows.into_iter().map(|(k, r)| (k.to_string(), r)).collect()
}

pub fn AFTER() -> HashMap<String, AfterRule> {
    let discard_one = || a(|f, _, _, _, _| {
        fx::discard_from_hand(f, 1);
    });
    let rows: Vec<(&str, AfterRule)> = vec![
        ("ANGER", a(|f, c, _, _, _| f.add_card(c.copy(), "discard"))),
        ("ARMAMENTS", a(|f, c, _, _, _| {
            if c.up {
                for h in f.hand.iter_mut() {
                    h.up = true;
                }
            } else {
                let pool: Vec<Card> = f.hand.iter().filter(|h| !h.up && !matches!(h.r#type(), "Status" | "Curse")).cloned().collect();
                for h in f.hand_pick("Choose a card to Upgrade.", 1, Some(&pool)) {
                    if let Some(h) = f.card_mut(h.uid) {
                        h.up = true;
                    }
                }
            }
        })),
        ("HAVOC", a(|f, _c, foe, _, _| _havoc(f, foe))),
        ("HEADBUTT", a(|f, _, _, _, _| _pick(f, "Choose a card to put on top of your draw pile.", Pile::Discard, None, 1, Some(|f, card| f.draw.push(card))))),
        ("TRUE_GRIT", a(|f, c, _, _, _| {
            if !f.hand.is_empty() {
                if c.up {
                    _exhaust_pick(f, 1, "Choose 1 card(s) to Exhaust.");
                } else {
                    let uid = f.run.rng.choice(&f.hand).uid;
                    _exhaust(f, vec![uid]);
                }
            }
        })),
        ("WARCRY", a(|f, _, _, _, _| _to_top(f, false))),
        ("WILD_STRIKE", _to_hand("WOUND", 1, "draw")),
        ("BATTLE_TRANCE", a(|f, _, _, _, _| f.no_draw = true)),
        ("DUAL_WIELD", a(|f, c, _, _, _| {
            let pool: Vec<Card> = f.hand.iter().filter(|h| matches!(h.r#type(), "Attack" | "Power")).cloned().collect();
            for k in f.hand_pick("Choose a card to copy.", 1, Some(&pool)) {
                for _ in 0..(if c.up { 2 } else { 1 }) {
                    let twin = f.card(k.uid).unwrap_or(&k).copy();
                    f.add_card(twin, "hand");
                }
            }
        })),
        ("ENTRENCH", a(|f, _, _, _, _| f.gain_block(f.block))),
        ("FLAME_BARRIER", a(|f, c, _, _, _| {
            add(&mut f.temp, "FLAME_BARRIER", if c.up { 6 } else { 4 });
        })),
        ("INFERNAL_BLADE", _random_card("Attack")),
        ("POWER_THROUGH", _to_hand("WOUND", 2, "hand")),
        ("RAGE", _gain(&[("RAGE", 3, 5)])),
        ("RAMPAGE", a(|_f, c, _, _, _| c.bonus += if c.up { 8 } else { 5 })),
        ("RECKLESS_CHARGE", _to_hand("DAZED", 1, "draw")),
        ("SECOND_WIND", a(|f, c, _, _, _| _second_wind(f, c))),
        ("SEVER_SOUL", a(|f, _, _, _, _| {
            let cards = hand_uids(f, |h| h.r#type() != "Attack");
            _exhaust(f, cards);
        })),
        ("DOUBLE_TAP", a(|f, c, _, _, _| f.double_next += if c.up { 2 } else { 1 })),
        ("EXHUME", a(|f, _, _, _, _| {
            let pool: Vec<Card> = f.exhausted.iter().filter(|e| e.key() != "EXHUME").cloned().collect();
            _pick(f, "Choose a card to return to your hand.", Pile::Exhausted, Some(pool), 1, None);
        })),
        ("FEED", a(|f, c, foe, _, _| _feed(f, c, foe))),
        ("IMMOLATE", _to_hand("BURN", 1, "discard")),
        ("LIMIT_BREAK", a(|f, _, _, _, _| {
            let n = f.pw.val("STRENGTH") * 2;
            f.pw.put("STRENGTH", n);
        })),
        ("REAPER", a(|f, _c, _foe, _x, dealt| f.heal(dealt))),
        ("SURVIVOR", discard_one()),
        ("ACROBATICS", discard_one()),
        ("DAGGER_THROW", discard_one()),
        ("PREPARED", a(|f, c, _, _, _| {
            fx::discard_from_hand(f, if c.up { 2 } else { 1 });
        })),
        ("CONCENTRATE", a(|f, c, _, _, _| {
            fx::discard_from_hand(f, if c.up { 2 } else { 3 });
        })),
        ("BANE", a(|f, c, foe, _, _| {
            if let Some(foe) = foe {
                if f.foes[foe].alive() && f.foes[foe].powers.on("POISON") {
                    let n = f.attack_value(if c.up { 10 } else { 7 }, Some(foe), false, false);
                    f.hurt(foe, n, true);
                }
            }
        })),
        ("ALL_OUT_ATTACK", a(|f, _, _, _, _| {
            if !f.hand.is_empty() {
                let uid = f.run.rng.choice(&f.hand).uid;
                _discard(f, vec![uid]);
            }
        })),
        ("BLUR", _gain(&[("BLUR", 1, 1)])),
        ("CALCULATED_GAMBLE", a(|f, _, _, _, _| {
            let hand = hand_uids(f, |_| true);
            let n = _discard(f, hand);
            f.draw_cards(n);
        })),
        ("CATALYST", a(|f, c, foe, _, _| {
            if let Some(foe) = foe {
                if f.foes[foe].powers.on("POISON") {
                    f.foes[foe].powers["POISON"] *= if c.up { 3 } else { 2 };
                }
            }
        })),
        ("CHOKE", a(|f, c, foe, _, _| {
            if let Some(foe) = foe {
                f.foes[foe].mem.add("choke", if c.up { 5 } else { 3 });
            }
        })),
        ("DISTRACTION", _random_card("Skill")),
        ("EXPERTISE", a(|f, c, _, _, _| f.draw_cards(((if c.up { 7 } else { 6 }) - f.hand.len() as i64).max(0)))),
        ("SETUP", a(|f, _, _, _, _| _to_top(f, true))),
        ("BULLET_TIME", a(|f, _, _, _, _| {
            for h in f.hand.iter_mut() {
                h.cost_turn = Some(0);
            }
            f.no_draw = true;
        })),
        ("BURST", a(|f, c, _, _, _| f.burst += if c.up { 2 } else { 1 })),
        ("CORPSE_EXPLOSION", a(|f, _c, foe, _, _| {
            if let Some(foe) = foe {
                f.foes[foe].mem.set_flag("corpse", true);
            }
        })),
        ("DOPPELGANGER", a(|f, _c, _foe, x, _dealt| {
            add(&mut f.pw, "DRAW_CARDS_NEXT_TURN", x);
            add(&mut f.pw, "ENERGY_NEXT_TURN", x);
        })),
        ("GLASS_KNIFE", a(|_f, c, _, _, _| c.bonus -= 2)),
        ("PHANTASMAL_KILLER", _gain(&[("DOUBLE_DAMAGE_NEXT", 1, 1)])),
        ("STORM_OF_STEEL", a(|f, c, _, _, _| {
            let hand = hand_uids(f, |_| true);
            let n = _discard(f, hand);
            f.add_new("SHIV", n, "hand", c.up);
        })),
        ("UNLOAD", a(|f, _, _, _, _| {
            let cards = hand_uids(f, |h| h.r#type() != "Attack");
            _discard(f, cards);
        })),
        ("ALCHEMIZE", a(|f, _, _, _, _| {
            let p = (f.game.random_potion)(&mut f.run);
            f.run.add_potion(p);
        })),
        ("CLAW", _gain(&[("CLAW_BONUS", 2, 2)])),
        ("RECURSION", a(|f, _, _, _, _| _recursion(f))),
        ("STACK", a(|f, c, _, _, _| {
            let b = f.block_value(f.discard.len() as i64 + if c.up { 3 } else { 0 });
            f.gain_block(b);
        })),
        ("STEAM_BARRIER", a(|_f, c, _, _, _| c.bonus -= 1)),
        ("STREAMLINE", a(|_f, c, _, _, _| c.cost_fight = Some((c.cost() - 1).max(0)))),
        ("TURBO", _to_hand("VOID", 1, "discard")),
        ("AUTO_SHIELDS", a(|f, c, _, _, _| {
            if f.block == 0 {
                let b = f.block_value(if c.up { 15 } else { 11 });
                f.gain_block(b);
            }
        })),
        ("CHAOS", a(|f, c, _, _, _| {
            for _ in 0..(if c.up { 2 } else { 1 }) {
                let kind = *f.run.rng.choice(&["LIGHTNING", "FROST", "DARK", "PLASMA"]);
                f.channel(kind);
            }
        })),
        ("CHILL", a(|f, _, _, _, _| {
            for _ in f.alive() {
                f.channel("FROST");
            }
        })),
        ("CONSUME", a(|f, c, _, _, _| {
            add(&mut f.pw, "FOCUS", if c.up { 3 } else { 2 });
            f.orb_slots = (f.orb_slots - 1).max(0);
            f.orbs.truncate(f.orb_slots as usize);
        })),
        ("DOUBLE_ENERGY", a(|f, _, _, _, _| f.energy *= 2)),
        ("EQUILIBRIUM", a(|f, _, _, _, _| {
            // (kept by the end of turn)
            for h in f.hand.iter_mut() {
                h.retained = -1;
            }
            f.pw.put("KEEP_HAND", 1);
        })),
        ("GENETIC_ALGORITHM", a(|_f, c, _, _, _| c.bonus += if c.up { 3 } else { 2 })),
        ("OVERCLOCK", _to_hand("BURN", 1, "discard")),
        ("RECYCLE", a(|f, _, _, _, _| {
            for k in f.hand_pick("Choose a card to Exhaust.", 1, None) {
                let k = f.take_from(Pile::Hand, k.uid).expect("the card picked is in the hand");
                f.energy += k.cost().max(0);
                f.exhaust(k);
            }
        })),
        ("REPROGRAM", _gain(&[("FOCUS", -1, -2)])), // (its Strength and Dexterity are the parse's: until 2026-11-26 counted twice) "SCRAPE": _scrape,
        ("SUNDER", a(|f, _c, foe, _, _| {
            if foe.is_some_and(|foe| !f.foes[foe].alive()) {
                f.energy += 3;
            }
        })),
        ("WHITE_NOISE", _random_card("Power")),
        ("ALL_FOR_ONE", a(|f, _, _, _, _| _all_for_one(f))),
        ("AMPLIFY", _gain(&[("AMPLIFY", 1, 2)])),
        ("CORE_SURGE", _gain(&[("ARTIFACT", 1, 1)])),
        ("FISSION", a(|f, c, _, _, _| _fission(f, c))),
        ("HYPERBEAM", _gain(&[("FOCUS", -3, -3)])),
        ("MULTI_CAST", a(|f, _c, _foe, x, _dealt| f.evoke(x, false))),
        ("RAINBOW", a(|f, _, _, _, _| {
            f.channel("FROST");
            f.channel("DARK");
        })),
        ("REBOOT", a(|f, c, _, _, _| {
            let hand = std::mem::take(&mut f.hand);
            let discard = std::mem::take(&mut f.discard);
            f.draw.extend(hand);
            f.draw.extend(discard);
            f.run.rng.shuffle(&mut f.draw);
            f.draw_cards(if c.up { 6 } else { 4 });
        })),
        ("SEEK", a(|f, c, _, _, _| _pick(f, "Choose a card to put into your hand.", Pile::Draw, None, if c.up { 2 } else { 1 }, None))),
        ("HOLOGRAM", a(|f, _, _, _, _| _pick(f, "Choose a card to return to your hand.", Pile::Discard, None, 1, None))),
        ("PROSTRATE", a(|f, c, _, _, _| f.gain_mantra(if c.up { 3 } else { 2 }))),
        ("WORSHIP", a(|f, _, _, _, _| f.gain_mantra(5))),
        ("PRAY", a(|f, c, _, _, _| {
            f.gain_mantra(if c.up { 4 } else { 3 });
            f.add_new("INSIGHT", 1, "draw", false);
        })),
        ("INNER_PEACE", a(|f, c, _, _, _| if f.stance == "CALM" { f.draw_cards(if c.up { 4 } else { 3 }) } else { f.enter("CALM") })),
        ("INDIGNATION", a(|f, c, _, _, _| if f.stance == "WRATH" { fx::debuff_all(f, "VULNERABLE", if c.up { 5 } else { 3 }) } else { f.enter("WRATH") })),
        ("HALT", a(|f, c, _, _, _| {
            if f.stance == "WRATH" {
                let b = f.block_value(if c.up { 14 } else { 9 });
                f.gain_block(b);
            }
        })),
        ("THIRD_EYE", _scry(3, 5)),
        ("JUST_LUCKY", _scry(1, 2)),
        ("CUT_THROUGH_FATE", _scry(2, 3)),
        ("MEDITATE", a(|f, c, _, _, _| {
            _pick(f, "Choose a card to return to your hand.", Pile::Discard, None, if c.up { 2 } else { 1 }, None);
            f.enter("CALM");
            f.pw.put("TURN_OVER", 1);
        })),
        ("SPIRIT_SHIELD", a(|f, c, _, _, _| {
            let b = f.block_value((if c.up { 4 } else { 3 }) * f.hand.len() as i64);
            f.gain_block(b);
        })),
        ("CONJURE_BLADE", a(|f, c, _foe, x, _dealt| {
            f.pw.put("EXPUNGER_X", x + if c.up { 1 } else { 0 });
            f.add_new("EXPUNGER", 1, "draw", false);
        })),
        ("DECEIVE_REALITY", _to_hand("SAFETY", 1, "hand")),
        ("EVALUATE", _to_hand("INSIGHT", 1, "draw")),
        ("CARVE_REALITY", _to_hand("SMITE", 1, "hand")),
        ("REACH_HEAVEN", _to_hand("THROUGH_VIOLENCE", 1, "draw")),
        ("ALPHA", _to_hand("BETA", 1, "draw")),
        ("BETA", _to_hand("OMEGA", 1, "draw")),
        ("WALLOP", a(|f, _c, _foe, _x, dealt| f.gain_block(dealt))),
        ("JUDGMENT", a(|f, c, foe, _, _| {
            if let Some(foe) = foe {
                if f.foes[foe].alive() && f.foes[foe].hp <= if c.up { 40 } else { 30 } {
                    f.kill(foe);
                }
            }
        })),
        ("PRESSURE_POINTS", a(|f, c, foe, _, _| {
            if let Some(foe) = foe {
                add(&mut f.foes[foe].powers, "MARK", if c.up { 11 } else { 8 });
            }
            for v in f.alive() {
                if f.foes[v].powers.on("MARK") {
                    let n = f.foes[v].powers["MARK"];
                    f.hurt(v, n, false);
                }
            }
        })),
        ("TALK_TO_THE_HAND", a(|f, c, foe, _, _| {
            if let Some(foe) = foe {
                f.foes[foe].mem.add("block_return", if c.up { 3 } else { 2 });
            }
        })),
        ("WAVE_OF_THE_HAND", _gain(&[("WAVE_OF_THE_HAND", 1, 2)])),
        ("SWIVEL", a(|f, _, _, _, _| f.free_attack = true)),
        ("SIMMERING_FURY", a(|f, c, _, _, _| {
            f.pw.put("SIMMERING", 1);
            add(&mut f.pw, "DRAW_CARDS_NEXT_TURN", if c.up { 3 } else { 2 });
        })),
        ("WREATH_OF_FLAME", _gain(&[("VIGOR", 5, 8)])),
        ("WISH", _gain(&[("STRENGTH", 3, 4)])),
        ("VAULT", a(|f, _, _, _, _| {
            f.pw.put("EXTRA_TURN", 1);
            f.pw.put("TURN_OVER", 1);
        })),
        ("SCRAWL", a(|f, _, _, _, _| f.draw_cards(10 - f.hand.len() as i64))),
        ("BLASPHEMY", a(|f, _, _, _, _| f.pw.put("BLASPHEMY_DUE", 1))),
        ("CONCLUDE", a(|f, _, _, _, _| f.pw.put("TURN_OVER", 1))),
        ("APOTHEOSIS", a(|f, _, _, _, _| {
            for h in f.hand.iter_mut().chain(&mut f.draw).chain(&mut f.discard) {
                h.up = true;
            }
        })),
        ("PANIC_BUTTON", a(|f, _, _, _, _| f.pw.put("NO_BLOCK", 2))),
        ("ENLIGHTENMENT", a(|f, _, _, _, _| {
            for h in f.hand.iter_mut() {
                if h.cost() > 1 {
                    h.cost_turn = Some(1);
                }
            }
        })),
        ("DEEP_BREATH", a(|f, c, _, _, _| {
            let discard = std::mem::take(&mut f.discard);
            f.draw.extend(discard);
            f.run.rng.shuffle(&mut f.draw);
            f.draw_cards(if c.up { 2 } else { 1 });
        })),
        ("SECRET_TECHNIQUE", _secret("Skill")),
        ("SECRET_WEAPON", _secret("Attack")),
        ("MADNESS", a(|f, _, _, _, _| {
            let pool = hand_uids(f, |h| h.cost() > 0);
            if !pool.is_empty() {
                let uid = *f.run.rng.choice(&pool);
                if let Some(h) = f.card_mut(uid) {
                    h.cost_fight = Some(0);
                }
            }
        })),
        ("THINKING_AHEAD", a(|f, _, _, _, _| _to_top(f, false))),
        ("JACK_OF_ALL_TRADES", _random_card("Skill")),
        ("DISCOVERY", _random_card("Attack")),
        ("FOREIGN_INFLUENCE", _random_card("Attack")),
        ("PURITY", a(|f, _, _, _, _| _exhaust_pick(f, 3, "Choose up to 3 card(s) to Exhaust."))),
    ];
    rows.into_iter().map(|(k, r)| (k.to_string(), r)).collect()
}

pub fn POWER() -> HashMap<String, PowerRule> {
    let mut out: HashMap<String, PowerRule> = HashMap::new();
    for (k, base, up) in [
        ("DEMON_FORM", 2, 3), ("METALLICIZE", 3, 4), ("BARRICADE", 1, 1), ("FEEL_NO_PAIN", 3, 4), ("DARK_EMBRACE", 1, 1), ("CORRUPTION", 1, 1), ("JUGGERNAUT", 5, 7),
        ("EVOLVE", 1, 2), ("FIRE_BREATHING", 6, 10), ("RUPTURE", 1, 2), ("BRUTALITY", 1, 1), ("NOXIOUS_FUMES", 2, 3), ("AFTER_IMAGE", 1, 1), ("A_THOUSAND_CUTS", 1, 2),
        ("ACCURACY", 4, 6), ("INFINITE_BLADES", 1, 1), ("ENVENOM", 1, 1), ("TOOLS_OF_THE_TRADE", 1, 1), ("WELL_LAID_PLANS", 1, 2), ("LOOP", 1, 2), ("STORM", 1, 1),
        ("STATIC_DISCHARGE", 1, 2), ("HEATSINKS", 1, 2), ("HELLO_WORLD", 1, 1), ("CREATIVE_AI", 1, 1), ("MACHINE_LEARNING", 1, 1), ("BUFFER", 1, 2), ("ECHO_FORM", 1, 1),
        ("SELF_REPAIR", 7, 10), ("MENTAL_FORTRESS", 4, 6), ("RUSHDOWN", 2, 2), ("LIKE_WATER", 5, 7), ("NIRVANA", 3, 4), ("DEVOTION", 2, 3), ("DEVA_FORM", 1, 1),
        ("BATTLE_HYMN", 1, 1), ("STUDY", 1, 1), ("FORESIGHT", 3, 4), ("ESTABLISHMENT", 1, 1), ("MASTER_REALITY", 1, 1), ("OMEGA", 50, 60), ("PANACHE", 10, 14),
        ("SADISTIC_NATURE", 5, 7), ("MAYHEM", 1, 1), ("MAGNETISM", 1, 1),
    ] {
        out.insert(k.to_string(), _gain_power(&[(k, base, up)]));
    }
    let rows: Vec<(&str, PowerRule)> = vec![
        ("CALTROPS", _gain_power(&[("THORNS", 3, 5)])),
        ("DEFRAGMENT", _gain_power(&[("FOCUS", 1, 2)])),
        ("COMBUST", _gain_power(&[("COMBUST", 5, 7), ("COMBUST_HP", 1, 1)])),
        ("BERSERK", _gain_power(&[("VULNERABLE", 2, 1), ("BERSERK", 1, 1)])),
        ("WRAITH_FORM", _gain_power(&[("INTANGIBLE", 2, 3), ("WRAITH_FORM", 1, 1)])),
        ("BIASED_COGNITION", _gain_power(&[("FOCUS", 4, 5), ("BIASED_COGNITION", 1, 1)])),
        ("CAPACITOR", p(|f, c| f.orb_slots += if c.up { 3 } else { 2 })),
        ("ELECTRODYNAMICS", p(|f, c| {
            f.pw.put("ELECTRODYNAMICS", 1);
            for _ in 0..(if c.up { 3 } else { 2 }) {
                f.channel("LIGHTNING");
            }
        })),
    ];
    out.extend(rows.into_iter().map(|(k, r)| (k.to_string(), r)));
    out
}

pub fn PLAYABLE() -> HashMap<String, PlayableRule> {
    let rows: Vec<(&str, PlayableRule)> = vec![
        ("CLASH", Rc::new(|f: &Fight, c: &Card| f.hand.iter().filter(|h| h.uid != c.uid).all(|h| h.r#type() == "Attack"))),
        ("GRAND_FINALE", Rc::new(|f: &Fight, _c: &Card| f.draw.is_empty())),
        ("SIGNATURE_MOVE", Rc::new(|f: &Fight, c: &Card| !f.hand.iter().any(|h| h.uid != c.uid && h.r#type() == "Attack"))),
        ("SECRET_TECHNIQUE", Rc::new(|f: &Fight, _c: &Card| f.draw.iter().any(|d| d.r#type() == "Skill"))),
        ("SECRET_WEAPON", Rc::new(|f: &Fight, _c: &Card| f.draw.iter().any(|d| d.r#type() == "Attack"))),
    ];
    rows.into_iter().map(|(k, r)| (k.to_string(), r)).collect()
}

pub fn DRAWN() -> HashMap<String, HandRule> {
    let rows: Vec<(&str, HandRule)> = vec![
        ("ENDLESS_AGONY", Rc::new(|f: &mut Fight, uid: u64| {
            if let Some(twin) = f.card(uid).map(|c| c.copy()) {
                f.add_card(twin, "hand");
            }
        })),
        ("DEUS_EX_MACHINA", Rc::new(|f: &mut Fight, uid: u64| {
            if let Some(c) = f.take_from(Pile::Hand, uid) {
                let up = c.up;
                f.exhaust(c);
                f.add_new("MIRACLE", if up { 3 } else { 2 }, "hand", false);
            }
        })),
    ];
    rows.into_iter().map(|(k, r)| (k.to_string(), r)).collect()
}

pub fn RETAINED() -> HashMap<String, HandRule> {
    fn with(f: &mut Fight, uid: u64, change: impl FnOnce(&mut Card)) {
        if let Some(c) = f.card_mut(uid) {
            change(c);
        }
    }
    let rows: Vec<(&str, HandRule)> = vec![
        ("WINDMILL_STRIKE", Rc::new(|f: &mut Fight, uid: u64| with(f, uid, |c| c.bonus += if c.up { 5 } else { 4 }))),
        ("PERSEVERANCE", Rc::new(|f: &mut Fight, uid: u64| with(f, uid, |c| c.bonus += if c.up { 3 } else { 2 }))),
        ("SANDS_OF_TIME", Rc::new(|f: &mut Fight, uid: u64| with(f, uid, |c| c.cost_fight = Some((c.cost() - 1).max(0))))),
    ];
    rows.into_iter().map(|(k, r)| (k.to_string(), r)).collect()
}
