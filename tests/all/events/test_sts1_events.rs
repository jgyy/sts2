//! The first game's events one by one (src/run/events/sts1.rs): the choice each rule makes, and that every event of the game
//! is covered by a rule or was checked against the generic scorer.

use serde_json::{json, Value};
use sts2bot::games;
use sts2bot::run::events::{self, sts1};
use sts2bot::run::strategy;

// Every event in the game's localization/eng/events.json (desktop-1.0.jar), by the id the bot sees (cards.key_of), 2026-11-26.
const GAME_EVENTS: &str = "ACCURSED_BLACKSMITH ADDICT BACK_TO_BASICS BEGGAR BIG_FISH BONFIRE_ELEMENTALS COLOSSEUM CURSED_TOME DEAD_ADVENTURER DESIGNER
DRUG_DEALER DUPLICATOR FACETRADER FALLING FORGOTTEN_ALTAR FOUNTAIN_OF_CLEANSING GHOSTS GOLDEN_IDOL GOLDEN_SHRINE GOLDEN_WING KNOWING_SKULL LAB
LIARS_GAME LIVING_WALL MASKED_BANDITS MATCH_AND_KEEP MINDBLOOM MUSHROOMS MYSTERIOUS_SPHERE NEST NOTEFORYOURSELF N_LOTH PROCEED_SCREEN PURIFIER
SCRAP_OOZE SECRETPORTAL SENSORYSTONE SHINING_LIGHT SPIRE_HEART THE_CLERIC THE_JOUST THE_LIBRARY THE_MAUSOLEUM THE_MOAI_HEAD THE_WOMAN_IN_BLUE
THINKING_WITH_PUDDLES TOMB_OF_LORD_RED_MASK TRANSMORGRIFIER UPGRADE_SHRINE VAMPIRES WEMEETAGAIN WHEEL_OF_CHANGE WINDING_HALLS WORLD_OF_GOOP";

fn STARTER() -> Vec<Value> {
    let mut deck = vec![json!({"name": "Strike", "id": "Strike_R", "type": "Attack", "cost": "1", "description": "Deal 6 damage."}); 5];
    deck.extend(vec![json!({"name": "Defend", "id": "Defend_R", "type": "Skill", "cost": "1", "description": "Gain 5 Block."}); 4]);
    deck.push(json!({"name": "Bash", "id": "Bash", "type": "Attack", "cost": "2", "description": "Deal 8 damage. Apply 2 Vulnerable."}));
    deck
}

/// The keyword arguments of the Python's `pick`, with its defaults.
struct Pick {
    hp: i64,
    max_hp: i64,
    gold: i64,
    act: i64,
    floor: i64,
    deck: Vec<Value>,
    potions: Vec<Value>,
    relics: Vec<&'static str>,
}

impl Default for Pick {
    fn default() -> Self {
        Pick { hp: 60, max_hp: 80, gold: 100, act: 1, floor: 5, deck: STARTER(), potions: Vec::new(), relics: Vec::new() }
    }
}

/// (`setUp`: the first game is the one played, for as long as the test runs.)
fn pick(event_id: &str, options: &[(&str, &str)], p: Pick) -> String {
    let _game = games::playing("sts1");
    let mut mem = strategy::Memory::new();
    mem.deck = p.deck;
    let opts: Vec<Value> = options.iter().enumerate().map(|(i, (t, d))| json!({"index": i, "title": t, "description": d})).collect();
    let relics: Vec<Value> = p.relics.iter().map(|r| json!({"id": r})).collect();
    let state = json!({"state_type": "event", "run": {"act": p.act, "floor": p.floor}, "event": {"event_id": event_id, "options": opts},
                       "player": {"hp": p.hp, "max_hp": p.max_hp, "gold": p.gold, "potions": p.potions, "relics": relics, "max_potion_slots": 3}});
    let index = events::event(&state, &mut mem).into_iter().find(|a| a.0 == "choose_event_option").expect("an option chosen").1["index"].as_u64().unwrap();
    opts[index as usize]["title"].as_str().unwrap().to_string()
}

// Sts1Events

#[test]
fn test_every_event_of_the_game_has_a_rule_or_was_checked() {
    let _game = games::playing("sts1");
    let missing: Vec<&str> = GAME_EVENTS.split_whitespace().filter(|e| !sts1::COVERED.contains(e)).collect();
    assert_eq!(missing, Vec::<&str>::new());
}

#[test]
fn test_back_to_basics_upgrades_every_basic_card_early() {
    let opts = [("Elegance", "Remove a card from your deck."), ("Simplicity", "Upgrade all Strikes and Defends.")];
    assert_eq!(pick("BACK_TO_BASICS", &opts, Pick::default()), "Simplicity");
    let starter = STARTER();
    let mut few = vec![starter[4].clone(), starter[starter.len() - 1].clone()];
    few.extend(vec![json!({"name": "Shrug It Off", "type": "Skill", "cost": "1", "description": "Gain 8 Block. Draw 1 card."}); 8]);
    assert_eq!(pick("BACK_TO_BASICS", &opts, Pick { deck: few, ..Pick::default() }), "Elegance");
}

#[test]
fn test_masked_bandits_are_paid_off_when_gold_is_short() {
    let opts = [("Fight!", ""), ("Pay", "Lose ALL of your Gold.")];
    assert_eq!(pick("MASKED_BANDITS", &opts, Pick { gold: 15, act: 2, floor: 25, hp: 40, ..Pick::default() }), "Pay");
    assert_eq!(pick("MASKED_BANDITS", &opts, Pick { gold: 300, act: 2, floor: 25, hp: 80, ..Pick::default() }), "Fight!");
}

#[test]
fn test_n_loth_gets_none_of_our_relics() {
    let opts = [("Offer: Pure Water", "Lose this relic. Obtain a special relic."), ("Leave", "")];
    assert_eq!(pick("N_LOTH", &opts, Pick { act: 2, relics: vec!["PureWater"], ..Pick::default() }), "Leave");
}

#[test]
fn test_the_woman_in_blue_fills_the_belt() {
    let opts = [("Buy 1 Potion", "20 Gold."), ("Buy 2 Potions", "30 Gold."), ("Buy 3 Potions", "40 Gold."), ("Leave", "")];
    assert_eq!(pick("THE_WOMAN_IN_BLUE", &opts, Pick::default()), "Buy 3 Potions");
    let full = vec![json!({"name": "Fire Potion"}), json!({"name": "Block Potion"})];
    assert_eq!(pick("THE_WOMAN_IN_BLUE", &opts, Pick { potions: full, ..Pick::default() }), "Buy 1 Potion"); // (one free slot)
}

#[test]
fn test_falling_loses_the_card_we_miss_least() {
    let opts = [("Land", "Lose Shrug It Off"), ("Channel", "Lose Inflame"), ("Strike", "Lose Strike")];
    assert_eq!(pick("FALLING", &opts, Pick::default()), "Strike");
}

#[test]
fn test_the_library_reads_unless_badly_hurt() {
    let opts = [("Read", "Choose 1 of 20 cards to add to your deck."), ("Sleep", "Heal 26 HP.")];
    assert_eq!(pick("THE_LIBRARY", &opts, Pick { hp: 75, ..Pick::default() }), "Read");
    assert_eq!(pick("THE_LIBRARY", &opts, Pick { hp: 25, ..Pick::default() }), "Sleep");
}

#[test]
fn test_scrap_ooze_reaches_while_the_odds_pay() {
    let opts = [("Leave", ""), ("Deeper", "Lose 5 HP. 45%: Find a Relic.")];
    assert_eq!(pick("SCRAP_OOZE", &opts, Pick { hp: 60, ..Pick::default() }), "Deeper");
    assert_eq!(pick("SCRAP_OOZE", &opts, Pick { hp: 8, ..Pick::default() }), "Leave");
}

#[test]
fn test_tomb_of_lord_red_mask_for_little_gold() {
    let opts = [("Leave", ""), ("Offer", "40 Gold: Lose all Gold. Obtain a Relic.")];
    assert_eq!(pick("TOMB_OF_LORD_RED_MASK", &opts, Pick { gold: 40, act: 3, floor: 38, ..Pick::default() }), "Offer");
}

#[test]
fn test_colosseum_victory_is_two_elites_at_once() {
    let opts = [("COWARDICE", "Escape."), ("VICTORY", "A powerful fight with many rewards.")];
    assert_eq!(pick("COLOSSEUM", &opts, Pick { hp: 80, act: 2, floor: 25, ..Pick::default() }), "COWARDICE");
}

#[test]
fn test_neow_s_boss_relic_over_small_gifts() {
    let _game = games::playing("sts1");
    let opts = [("", "Max HP +7"), ("", "Lose your starting Relic Obtain a random boss Relic")];
    assert_eq!(pick("NEOW_EVENT", &opts, Pick { floor: 0, ..Pick::default() }), "");
    let ctx = events::context(&json!({"player": {"hp": 80, "max_hp": 80}, "run": {}}), &strategy::Memory::new(), 0);
    let ruled: Vec<&str> = ["Max HP +7", "Lose your starting Relic Obtain a random boss Relic"]
        .into_iter()
        .filter(|o| events::rule_value("NEOW_EVENT", &json!({"title": "", "description": o}), &ctx).is_some())
        .collect();
    assert!(ruled[0].contains("boss"));
}
