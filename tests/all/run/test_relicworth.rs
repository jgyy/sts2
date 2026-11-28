//! Shop relics bought by what they do, not by price alone (relicworth.rs).

use std::collections::BTreeSet;

use serde_json::{json, Value};
use sts2bot::run::events::{self, EventOptionValueArgs};
use sts2bot::run::strategy::{self, Memory};
use sts2bot::run::{relics, relicworth};

fn relic(i: i64, rid: &str, name: &str, price: i64) -> Value {
    json!({"index": i, "category": "relic", "relic_id": rid, "relic_name": name, "relic_description": "", "price": price, "is_stocked": true})
}

/// (the original's defaults: act 2, deck (), hp 60, max_hp 75.)
fn bought_(items: &[Value], gold: i64, act: i64, deck: &[Value], hp: i64, max_hp: i64) -> Vec<i64> {
    let player = json!({"gold": gold, "hp": hp, "max_hp": max_hp, "max_potion_slots": 2, "potions": [], "relics": []});
    let floor = if act == 1 {
        8
    } else if act == 2 {
        22
    } else {
        38
    };
    let state = json!({"state_type": "shop", "run": {"act": act, "floor": floor}, "player": player, "shop": {"items": items}});
    let mut mem = Memory::new();
    mem.deck = deck.to_vec();
    strategy::shop(&state, &mut mem, "shop").iter().filter(|a| a.0 == "shop_purchase").map(|a| a.1["index"].as_i64().unwrap()).collect()
}

fn bought(items: &[Value], gold: i64) -> Vec<i64> {
    bought_(items, gold, 2, &[], 60, 75)
}

fn X_CARD() -> Value {
    json!({"name": "Whirlwind", "type": "Attack", "cost": "X", "description": "Deal 5 damage to ALL enemies X times."})
}
fn STRIKE() -> Value {
    json!({"name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."})
}

/// `relicworth.worth(relic_id, deck=(), act=None, hp=0, max_hp=0)`.
fn worth(relic_id: &str) -> f64 {
    relicworth::worth(relic_id, &[], None, 0, 0)
}

fn worth_in(relic_id: &str, act: i64) -> f64 {
    relicworth::worth(relic_id, &[], Some(act), 0, 0)
}

// ShopRelics

#[test]
fn test_the_best_relic_before_the_cheapest() {
    // (A Necrobinder in act 2 with 272 gold: the old rule bought the Cauldron.)
    let shelf = [relic(0, "PANTOGRAPH", "Pantograph", 233), relic(1, "MERCURY_HOURGLASS", "Mercury Hourglass", 210), relic(2, "CAULDRON", "Cauldron", 203)];
    assert_eq!(bought(&shelf, 272)[0], 0);
}

#[test]
fn test_a_relic_the_bot_cannot_use_only_out_of_plenty() {
    let shelf = [relic(0, "MINIATURE_TENT", "Miniature Tent", 250)];
    assert_eq!(bought(&shelf, 300), Vec::<i64>::new());
    assert_eq!(bought(&shelf, 460), [0]);
}

#[test]
fn test_chemical_x_needs_x_cost_cards() {
    let shelf = [relic(0, "CHEMICAL_X", "Chemical X", 195), relic(1, "PEN_NIB", "Pen Nib", 234)];
    assert_eq!(bought_(&shelf, 237, 2, &vec![STRIKE(); 5], 60, 75), [1]); // (the old rule bought Chemical X)
    assert!(relicworth::worth("CHEMICAL_X", &[STRIKE()], None, 0, 0) < relicworth::worth("CHEMICAL_X", &[X_CARD()], None, 0, 0));
    assert!(relicworth::worth("CHEMICAL_X", &[X_CARD()], None, 0, 0) >= 0.0); // one X card: an ordinary relic, two: a good one
}

#[test]
fn test_lees_waffle_is_worth_the_hp_it_gives_back() {
    assert!(relicworth::worth("LEES_WAFFLE", &[], None, 25, 75) > relicworth::worth("LEES_WAFFLE", &[], None, 70, 75));
}

#[test]
fn test_what_pays_over_the_run_is_worth_less_late() {
    assert!(worth_in("MEMBERSHIP_CARD", 1) > worth_in("MEMBERSHIP_CARD", 3));
    assert_eq!(worth_in("ANCHOR", 1), worth_in("ANCHOR", 3));
    // A Pumpkin Candle goes out after five combats, so an act 3 boss may find it spent.
    assert!(worth_in("PUMPKIN_CANDLE", 1) > worth_in("PUMPKIN_CANDLE", 3));
}

#[test]
fn test_an_unknown_relic_is_an_ordinary_one() {
    assert_eq!(worth("SOMETHING_NEW"), 0.0);
    assert_eq!(bought(&[relic(0, "SOMETHING_NEW", "Something New", 200)], 210), [0]); // (as before: 8.0 - price / 200)
}

#[test]
fn test_an_energy_every_turn_is_bought_over_an_ordinary_relic() {
    // (The Demon model puts one energy a turn above Pantograph's 25 HP; the fight-cost table had it at 0.1-1.0.)
    let shelf = [relic(0, "MERCURY_HOURGLASS", "Mercury Hourglass", 150), relic(1, "PAELS_FLESH", "Pael's Flesh", 190)];
    assert_eq!(bought(&shelf, 200)[0], 1); // (the first purchase: the bot re-plans after each one)
    assert!(worth_in("PAELS_FLESH", 2) > worth_in("PANTOGRAPH", 2));
}

#[test]
fn test_every_rated_relic_is_one_the_game_has() {
    // (A misspelt id would be rated as ordinary without a word.)
    let Some(home) = std::env::var_os("HOME") else { return };
    let wiki = std::path::Path::new(&home).join(".local/state/sts2bot/wiki.json");
    if !wiki.exists() {
        return; // skipTest("no wiki dump")
    }
    let dump: Value = serde_json::from_str(&std::fs::read_to_string(&wiki).unwrap()).unwrap();
    let known = &dump["relics"];
    let rated: BTreeSet<&str> = relicworth::WORTH.keys().chain(relicworth::LONG_RUN.iter()).chain(relicworth::_DECK.keys()).copied().chain(["LEES_WAFFLE"]).collect();
    let missing: Vec<&str> = rated.into_iter().filter(|r| known.get(r).is_none() && !known.as_array().is_some_and(|a| a.iter().any(|k| k == r))).collect();
    assert_eq!(missing, Vec::<&str>::new());
}

// RestSiteRelics

fn OSTY() -> Value {
    json!({"name": "Poke", "type": "Attack", "cost": 1, "description": "Osty deals 6 damage."})
}

/// (the original's defaults: ids ("HEAL", "SMITH", "LIFT"), deck None.)
fn chosen(hp: i64, ids: &[&'static str], deck: &[Value]) -> &'static str {
    let options: Vec<Value> = ids.iter().enumerate().map(|(i, oid)| json!({"index": i, "id": oid, "name": sts2bot::base::title(oid), "is_enabled": true})).collect();
    let state = json!({"state_type": "rest_site", "run": {"act": 2, "floor": 25}, "player": {"hp": hp, "max_hp": 80}, "rest_site": {"options": options}});
    let mut mem = Memory::new();
    mem.deck = deck.to_vec();
    let acts = strategy::rest_site(&state, &mut mem);
    let first = acts.iter().find(|a| a.0 == "choose_rest_option").unwrap();
    ids[first.1["index"].as_i64().unwrap() as usize]
}

const LIFT: [&str; 3] = ["HEAL", "SMITH", "LIFT"];

#[test]
fn test_girya_and_shovel_are_used() {
    // (They ranked under Smith: 31 runs held Girya and 26 Shovel, and none lifted or dug.)
    assert_eq!(chosen(78, &LIFT, &vec![STRIKE(); 6]), "LIFT");
    assert_eq!(chosen(78, &["HEAL", "SMITH", "DIG"], &[]), "DIG");
    assert_eq!(chosen(30, &LIFT, &vec![STRIKE(); 6]), "HEAL"); // healing first when it is needed
}

#[test]
fn test_no_lift_for_a_deck_that_does_not_hit() {
    assert_eq!(chosen(78, &LIFT, &[vec![OSTY(); 6], vec![STRIKE()]].concat()), "SMITH");
}

// SculptingStrike

#[test]
fn test_ethereal_goes_on_what_the_deck_is_better_without() {
    // (The best card in hand used to be marked, and Exhausted when it was not played that turn.)
    let hand = json!([{"index": 0, "name": "Grave Warden", "type": "Skill", "cost": 1, "description": "Gain 8 Block. Summon 3."},
                      {"index": 1, "name": "Ascender's Bane", "type": "Curse", "cost": -1, "description": "Unplayable. Ethereal."},
                      {"index": 2, "name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."},
                      {"index": 3, "name": "Defend+", "type": "Skill", "cost": 1, "description": "Gain 8 Block."}]);
    let state = json!({"state_type": "hand_select", "player": {"energy": 3, "hand": hand}, "hand_select": {"prompt": "Choose a card to add Ethereal.", "cards": hand}});
    let acts = strategy::hand_select(&state, &mut Memory::new());
    let first = acts.iter().find(|a| a.0 == "combat_select_card").unwrap();
    assert_eq!(hand[first.1["card_index"].as_i64().unwrap() as usize]["name"], "Strike");
}

// RelicsTraded

fn HELD() -> Vec<Value> {
    ["GORGET", "JOSS_PAPER", "BAG_OF_PREPARATION", "BEATING_REMNANT", "PAPER_KRANE"].iter().map(|r| json!({"id": r})).collect()
}

fn traded(title: &str, text: &str) -> f64 {
    let held = HELD();
    events::_event_option_value(&json!({"title": title, "description": text}), 60, 75, EventOptionValueArgs { act: Some(2), potions: Some(&[]), held: Some(&held), ..Default::default() })
}

#[test]
fn test_the_relic_trader_takes_the_best_trade() {
    // (All three scored 0.5, and the top one, Gorget for Akabeko, was taken.)
    let top = traded("Take the Top One", "Trade Gorget for Akabeko.");
    let middle = traded("Take the Middle One", "Trade Joss Paper for Gremlin Horn.");
    let bottom = traded("Take the Bottom One", "Trade Bag of Preparation for Lucky Fysh.");
    assert!(middle > top);
    assert!(top > bottom);
}

#[test]
fn test_ranwid_keeps_a_good_relic() {
    let gold = traded("Give 100 Gold", "Obtain a random Relic.");
    assert!(traded("Give Paper Krane", "Obtain 2 random Relics.") > gold); // two relics for an ordinary one
    assert!(traded("Give Beating Remnant", "Obtain 2 random Relics.") < gold);
}

#[test]
fn test_a_relic_we_do_not_hold_is_not_charged() {
    let plain = events::_event_option_value(&json!({"title": "Give Anchor", "description": "Obtain a random Relic."}), 60, 75, EventOptionValueArgs { act: Some(2), potions: Some(&[]), ..Default::default() });
    assert_eq!(traded("Give Anchor", "Obtain a random Relic."), plain);
}

// EnergyOrCardsEveryTurn: an Ancient's energy-a-turn relic against its cards-a-turn one, by the energy the deck already has
// (relicworth.energy_lean; scripts/demon/ancients.rs: on 3 energy an energy a turn is worth more than a card, on 4 the card more).

fn FLESH() -> Value {
    json!({"title": "Pael's Flesh", "description": "Gain an additional [necrobinder_energy_icon.png] at the start of your 3rd turn, and every turn after that."})
}
fn BLOOD() -> Value {
    json!({"title": "Pael's Blood", "description": "At the start of your turn, draw 1 additional card."})
}

/// `setUp`: (no runs on record: the table's own numbers)
fn set_up() {
    relics::load(None, events::_ANCIENT.keys());
}

/// (the original's default: held ().)
fn value(opt: &Value, energy: i64, held: &[&str]) -> f64 {
    let held: Vec<Value> = held.iter().map(|r| json!({"id": r})).collect();
    events::_event_option_value(opt, 60, 75, EventOptionValueArgs { act: Some(2), potions: Some(&[]), held: Some(&held), energy: Some(energy), ..Default::default() })
}

#[test]
fn test_paels_flesh_beats_paels_blood_on_three_energy_only() {
    set_up();
    // (Pael's Blood was taken all three times the two were offered together.)
    assert!(value(&FLESH(), 3, &[]) > value(&BLOOD(), 3, &[]));
    assert!(value(&BLOOD(), 4, &[]) > value(&FLESH(), 4, &[]));
    // ... and an energy relic held whose energy comes after the first turn counts as the fourth
    assert!(value(&BLOOD(), 3, &["PAELS_FLESH"]) > value(&FLESH(), 3, &["PAELS_FLESH"]));
}

#[test]
fn test_sozu_over_runic_pyramid_on_three_energy_only() {
    set_up();
    let sozu = json!({"title": "Sozu", "description": "Gain [defect_energy_icon.png] at the start of each turn. You can no longer obtain Potions."});
    let pyramid = json!({"title": "Runic Pyramid", "description": "At the end of your turn, you no longer discard your Hand."});
    assert!(value(&sozu, 3, &[]) > value(&pyramid, 3, &[]));
    assert!(value(&sozu, 4, &[]) < value(&pyramid, 4, &[]));
}

#[test]
fn test_an_energy_power_in_the_deck_counts_for_half() {
    set_up();
    let friendship = json!({"name": "Friendship", "type": "Power", "cost": "1", "description": "Lose 2 Strength. Gain [necrobinder_energy_icon.png] at the start of each turn."});
    assert_eq!(relicworth::spare_energy(3, &[], &[friendship.clone()]), 0.5);
    assert_eq!(relicworth::energy_lean("PAELS_BLOOD", Some(3), &[], &[friendship]), relicworth::TURN_LEAN["PAELS_BLOOD"].1);
}

#[test]
fn test_at_an_ancient_on_three_energy_and_on_four() {
    set_up();
    let ev = |titles: &[&str]| json!({"options": titles.iter().enumerate().map(|(i, t)| json!({"index": i, "title": t, "description": "Gain [necrobinder_energy_icon.png] at the start of each turn.", "relic_name": t})).collect::<Vec<_>>()});

    let first = |titles: &[&str], energy: i64| -> i64 {
        let mut mem = Memory::new();
        mem.max_energy = energy;
        let state = json!({"state_type": "event", "run": {"act": 2, "floor": 18}, "player": {"hp": 60, "max_hp": 80}, "event": ev(titles)});
        strategy::event(&state, &mut mem)[0].1["index"].as_i64().unwrap()
    };

    assert_eq!(first(&["Ectoplasm", "Snecko Eye", "Philosopher's Stone"], 3), 1);
    assert_eq!(first(&["Sozu", "Velvet Choker"], 3), 1);
    // Pael's Blood (a card a turn) over Pael's Flesh (an energy from the third turn) only for a deck already on 4 energy.
    assert_eq!(first(&["Pael's Flesh", "Pael's Blood"], 4), 1);
    assert_eq!(first(&["Pael's Flesh", "Pael's Blood"], 3), 0);
}

#[test]
fn test_unknown_energy_or_another_relic_leaves_the_table_alone() {
    set_up();
    assert_eq!(relicworth::energy_lean("PAELS_FLESH", None, &[], &[]), 0.0);
    assert_eq!(relicworth::energy_lean("FUR_COAT", Some(3), &[], &[]), 0.0);
    assert_eq!(events::_event_option_value(&FLESH(), 60, 75, EventOptionValueArgs::default()), events::_ANCIENT["PAELS_FLESH"]);
}
