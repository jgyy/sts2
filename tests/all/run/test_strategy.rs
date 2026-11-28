use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{json, Value};
use sts2bot::cards::{deck_value, upgrade_gain};
use sts2bot::fight::{combat, handpick};
use sts2bot::learn;
use sts2bot::logging;
use sts2bot::base::Random;
use sts2bot::run::events::{self, EventOptionValueArgs};
use sts2bot::run::strategy::{self, Memory};
use sts2bot::run::{crowd, decks};
use sts2bot::Action;

use crate::fake_game::{card, with};
use crate::helpers::{approx, combat_state, enemy, DEFEND, STRIKE};

/// A card as the deck lists it. (the original's `**kw`: `with`.)
fn c(cid: &str, name: &str, ctype: &str, cost: i64, text: &str) -> Value {
    json!({"id": cid, "name": name, "type": ctype, "cost": cost, "description": text})
}

fn IRON_STRIKE() -> Value {
    c("STRIKE_IRONCLAD", "Strike", "Attack", 1, "Deal 6 damage.")
}
fn IRON_DEFEND() -> Value {
    c("DEFEND_IRONCLAD", "Defend", "Skill", 1, "Gain 5 Block.")
}
const STAR: &str = "[star_icon.png]";

fn node(col: i64, row: i64, kind: &str, children: &[(i64, i64)]) -> Value {
    json!({"col": col, "row": row, "type": kind, "children": children.iter().map(|(c, r)| json!([c, r])).collect::<Vec<_>>()})
}

fn opt(index: i64, col: i64, row: i64, kind: &str) -> Value {
    json!({"index": index, "col": col, "row": row, "type": kind})
}

/// (the original's default: stocked True; its `**kw`: `with`.)
fn item(index: i64, category: &str, price: i64) -> Value {
    json!({"index": index, "category": category, "price": price, "is_stocked": true, "can_afford": true})
}

/// What the bot remembers standing on the rest site at (col, row), the boss next.
fn boss_after(col: i64, row: i64, boss_col: i64) -> Memory {
    let mut mem = Memory { boss_next: true, ..Memory::new() };
    mem.map_nodes = IndexMap::from([((col, row), node(col, row, "RestSite", &[(boss_col, row + 1)])), ((boss_col, row + 1), node(boss_col, row + 1, "Boss", &[]))]);
    mem.map_at = Some((col, row));
    mem
}

/// The cards a hand selection clicks on, in order.
fn picks(state: &Value) -> Vec<i64> {
    strategy::hand_select(state, &mut Memory::new()).iter().filter(|a| a.0 == "combat_select_card").map(|a| a.1["card_index"].as_i64().unwrap()).collect()
}

/// The character select screen ({id: locked}), with the confirm button lit. (the original's defaults: ascension 0, max_ascension 3.)
fn character_select(locked: &[(&str, bool)]) -> Value {
    let mut options: Vec<Value> = locked.iter().filter(|(_, v)| !v).map(|(k, _)| json!({"name": k, "enabled": true})).collect();
    options.push(json!({"name": "confirm", "enabled": true}));
    json!({"state_type": "menu", "menu_screen": "character_select", "characters": locked.iter().map(|(k, v)| json!({"id": k, "locked": v})).collect::<Vec<_>>(),
           "options": options, "ascension": 0, "max_ascension": 3})
}

fn act(name: &str, params: Value) -> Action {
    (name.to_string(), params)
}

/// `strategy.path_value(rooms, hp, max_hp, gold, act)`.
fn path_value(rooms: &[(&str, i64)], hp: f64, max_hp: i64, gold: i64, act: i64) -> f64 {
    let rooms: Vec<(&str, Option<i64>)> = rooms.iter().map(|(kind, row)| (*kind, Some(*row))).collect();
    strategy::path_value(&rooms, hp, max_hp, gold, act, 1.0)
}

/// `strategy.deck_fit(card, deck, act)`: energy 3.
fn deck_fit(card: &Value, deck: &[Value], act: i64) -> f64 {
    strategy::deck_fit(card, deck, Some(act), 3)
}

fn times(card: Value, n: usize) -> Vec<Value> {
    vec![card; n]
}

/// The first action's params.
fn first(acts: &[Action]) -> &Value {
    &acts[0].1
}

// LearnedRatings

#[test]
fn test_cards_that_went_far_rise_and_the_rest_fall() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("card_stats.json");
    let mut stats = learn::CardStats::new(Some(path.clone()));
    for floor in [10, 17, 17, 20, 25, 33] {
        // what is usual for this character
        stats.record("The Silent", floor, false, &[]);
    }
    assert_eq!(stats.shift("The Regent", "Footwork", None), 0.0); // nothing known: no opinion
    let fresh = stats.shift("The Silent", "Footwork", None);
    for _ in 0..40 {
        // (a card's own record counts slowly: learn.PRIOR_PICKS)
        stats.record("The Silent", 45, false, &[(3, "Footwork+".to_string()), (5, "Slice".to_string())]);
        stats.record("The Silent", 8, false, &[(3, "Slice".to_string())]);
    }
    assert!(stats.shift("The Silent", "Footwork", None) > fresh + 0.5);
    assert!(stats.shift("The Silent", "Slice", None) < stats.shift("The Silent", "Footwork", None));
    let mut again = learn::CardStats::new(Some(path)); // kept between sessions
    assert_eq!(again.shift("The Silent", "Footwork", None), stats.shift("The Silent", "Footwork", None));
    assert!(again.finals["The Silent"].len() as i64 <= learn::HISTORY);
}

#[test]
fn test_an_elite_met_hurt_costs_more_than_its_average() {
    let (healthy, sore, hurt, low) = {
        let _k = learn::with_knobs(&[("elite_worth", 2.5)]);
        // At 70% HP one of these fights was lost in 8; at 60%, a third of them.
        let v: Vec<f64> = [80.0, 56.0, 48.0, 38.0].iter().map(|hp| path_value(&[("Elite", 5), ("Monster", 6)], *hp, 80, 0, 2)).collect();
        (v[0], v[1], v[2], v[3])
    };
    assert!(sore < healthy - 2.0);
    assert!(hurt < healthy - 5.0);
    assert!(low < hurt - 5.0);
}

#[test]
fn test_a_fight_met_badly_hurt_is_priced_as_the_risk_it_is() {
    let fight_first = [("Monster", 10), ("RestSite", 11), ("RestSite", 12), ("Boss", 13)];
    let rest_first = [("RestSite", 10), ("Monster", 11), ("RestSite", 12), ("Boss", 13)];
    // On 23 of 80 HP in act 3 the fight that comes before any rest is the one that ends runs.
    assert!(path_value(&fight_first, 23.0, 80, 100, 3) < path_value(&rest_first, 23.0, 80, 100, 3) - 8.0);
    // Healthy, the order makes no difference; and act 1 fights are not priced this way at all.
    assert!((path_value(&fight_first, 70.0, 80, 100, 3) - path_value(&rest_first, 70.0, 80, 100, 3)).abs() <= 0.5);
    assert!(path_value(&[("Monster", 10)], 23.0, 80, 100, 1) > path_value(&[("Monster", 10)], 23.0, 80, 100, 3) + 7.0);
}

#[test]
fn test_an_act_3_elite_is_a_fifth_of_a_run_and_its_relic_has_one_boss_left() {
    // Sixteen met on Ascension 5-6: three ended the run there, two more left it to die in the next room.
    let on = [("RestSite", 10), ("Monster", 11), ("RestSite", 12), ("Boss", 13)];
    let (with_elite, without) = ([vec![("Elite", 9)], on.to_vec()].concat(), [vec![("Unknown", 9)], on.to_vec()].concat());
    let _k = learn::with_knobs(&[("elite_worth", 6.0), ("upgrade_worth", 2.5)]);
    assert!(path_value(&with_elite, 98.0, 100, 0, 3) < path_value(&without, 98.0, 100, 0, 3));
    // ... while in act 2 the same choice still goes to the elite (its relic has an act and a half to pay).
    assert!(path_value(&with_elite, 98.0, 100, 0, 2) > path_value(&without, 98.0, 100, 0, 2));
}

#[test]
fn test_hp_given_up_in_an_act_3_event_costs_more() {
    let mushroom = json!({"title": "Fragrant Mushroom", "description": "Lose 15 HP. Upgrade 2 random cards."});
    let value = |act: Option<i64>| events::_event_option_value(&mushroom, 60, 80, EventOptionValueArgs { act, ..Default::default() });
    let (early, late) = (value(Some(2)), value(Some(3)));
    approx(early - late, 12.0 * 15.0 / 60.0 * (events::ACT3_HP - 1.0));
    assert_eq!(value(None), early);
}

#[test]
fn test_star_makers_are_worth_less_once_nothing_spends_their_stars() {
    let venerate = |i: i64| card(i, "VENERATE", "Venerate", "Skill", 1, &format!("Gain {STAR}{STAR}."), "Self");
    let deck = [venerate(0), venerate(1), STRIKE(2), STRIKE(3)]; // makes 4 Stars a cycle, spends none
    let cache = card(9, "HIDDEN_CACHE", "Hidden Cache", "Skill", 1, &format!("Gain {STAR}. Next turn, gain {STAR}{STAR}{STAR}."), "Self");
    assert!(strategy::deck_fit(&cache, &deck, None, 3) < -0.4);
    assert_eq!(strategy::deck_fit(&cache, &[STRIKE(0)], None, 3), 0.0); // an empty star bank still wants makers
}

#[test]
fn test_a_deck_short_of_stars_wants_makers_not_one_more_card_that_spends_them() {
    let star = |i: i64, name: &str, need: i64, text: &str| with(card(i, &name.to_uppercase().replace(' ', "_"), name, "Attack", 0, text, "AnyEnemy"), json!({"star_cost": need}));
    let venerate = card(0, "VENERATE", "Venerate", "Skill", 1, &format!("Gain {STAR}{STAR}."), "Self");
    let blow = card(1, "KNOCKOUT_BLOW", "Knockout Blow", "Attack", 3, &format!("Deal 30 damage. If this kills an enemy, gain {}.", STAR.repeat(5)), "AnyEnemy");
    let deck = [venerate, blow, star(2, "Falling Star", 2, "Deal 8 damage."), star(3, "Astral Pulse", 3, "Deal 6 damage to ALL enemies twice."), STRIKE(4)];
    // Needs 5 Stars a cycle and makes 3 (the Blow's five come with a kill: one), plus the one the fight starts with.
    assert!(strategy::deck_fit(&star(9, "Seven Stars", 7, "Deal 7 damage to ALL enemies 7 times."), &deck, None, 3) < -3.5);
    let glow = card(8, "GLOW", "Glow", "Skill", 1, &format!("Gain {STAR}. Draw 1 card. Next turn, draw 1 card."), "Self");
    assert!(strategy::deck_fit(&glow, &deck, None, 3) > 0.4);
}

#[test]
fn test_a_strike_is_removed_before_a_defend() {
    let cards_ = [DEFEND(0), STRIKE(1), card(2, "POMMEL_STRIKE", "Pommel Strike", "Attack", 1, "Deal 9 damage. Draw 1 card.", "AnyEnemy")];
    let order: Vec<i64> = strategy::_pick_order(&cards_, "Choose a card to Remove.", "", None).iter().map(|x| x["index"].as_i64().unwrap()).collect();
    assert_eq!(order, [1, 0, 2]);
}

#[test]
fn test_an_attack_is_worth_more_to_a_deck_that_cannot_kill() {
    let attack = card(0, "DEFILE", "Defile", "Attack", 1, "Ethereal. Deal 13 damage.", "AnyEnemy");
    let block = card(1, "DEFY", "Defy", "Skill", 1, "Gain 6 Block. Apply 1 Weak.", "AnyEnemy");
    let starter: Vec<Value> = (0..4).map(STRIKE).chain((4..10).map(DEFEND)).collect();
    let first = deck_fit(&attack, &starter, 1);
    assert!(first > 1.0); // the starter deck does 2.4 for an energy
    assert_eq!(deck_fit(&block, &starter, 1), 0.0);
    assert_eq!(strategy::deck_fit(&attack, &starter, None, 3), 0.0); // (no act: no opinion)
    // Each attack taken makes the next one matter less, and a deck with enough of them gets no nudge.
    let second = deck_fit(&attack, &[starter.clone(), vec![with(attack.clone(), json!({"name": "Something Else", "id": "ELSE"}))]].concat(), 1);
    assert!(second < first);
    let armed: Vec<Value> = starter.iter().cloned().chain((10..18).map(|i| card(i, &format!("BIG_{i}"), &format!("Big {i}"), "Attack", 1, "Deal 14 damage.", "AnyEnemy"))).collect();
    assert_eq!(deck_fit(&attack, &armed, 2), 0.0);
    // A deck of Block and Powers late in a run is as short of damage as the starter was.
    let soft: Vec<Value> = starter.iter().cloned().chain((0..10).map(|i| with(block.clone(), json!({"id": format!("B{i}"), "name": format!("B{i}")})))).collect();
    assert!(deck_fit(&attack, &soft, 2) > 1.0);
}

#[test]
fn test_the_first_attack_of_a_run_comes_before_what_pays_off_later() {
    let defend = c("DEFEND_REGENT", "Defend", "Skill", 1, "Gain 5 Block.");
    let attack = c("KINGLY_PUNCH", "Kingly Punch", "Attack", 1, "Deal 8 damage.");
    let star = with(c("FALLING_STAR", "Falling Star", "Attack", 0, "Deal 8 damage. Apply 1 Weak. Apply 1 Vulnerable."), json!({"star_cost": 2}));
    let starter = [times(c("STRIKE_REGENT", "Strike", "Attack", 1, "Deal 6 damage."), 4), times(defend.clone(), 4), vec![star]].concat();
    let other = with(attack.clone(), json!({"id": "OTHER", "name": "Other"}));
    let first = deck_fit(&attack, &starter, 1);
    assert!(first >= 3.0); // (the Regent's own Falling Star is not one it has taken)
    let second = deck_fit(&attack, &[starter.clone(), vec![other.clone()]].concat(), 1);
    assert!(second <= 2.0);
    assert!(deck_fit(&attack, &starter, 2) <= 2.0); // act 1 only
    // How hard a deck short of damage leans to attacks is a setting under test.
    let soft: Vec<Value> = starter.iter().cloned().chain((0..8).map(|i| with(defend.clone(), json!({"id": format!("B{i}"), "name": format!("B{i}")})))).chain([other]).collect();
    {
        let _k = learn::with_knobs(&[("punch", 1.8)]);
        assert!(deck_fit(&attack, &soft, 2) > 3.0);
    }
    {
        let _k = learn::with_knobs(&[("punch", 0.9)]);
        assert!(deck_fit(&attack, &soft, 2) <= 2.0);
    }
}

#[test]
fn test_the_first_two_sources_of_weak_are_wanted_under_the_setting() {
    let skill = |name: &str, text: &str| c(&name.to_uppercase(), name, "Skill", 1, text);
    let (weak, other, deck) = (skill("Sap", "Apply 2 Weak."), skill("Guard", "Gain 8 Block."), times(IRON_STRIKE(), 9));
    let (plain, plain_other) = (deck_fit(&weak, &deck, 2), deck_fit(&other, &deck, 2));
    let _k = learn::with_knobs(&[("weak_want", 1.0)]);
    approx(deck_fit(&weak, &deck, 2), plain + 1.0);
    approx(deck_fit(&other, &deck, 2), plain_other);
    let held = [deck, vec![skill("Hex", "Apply 1 Weak."), skill("Curse Word", "Apply 1 Weak.")]].concat();
    approx(deck_fit(&weak, &held, 2), deck_fit(&other, &held, 2)); // two held: no more than any card
}

#[test]
fn test_the_first_block_cards_beyond_the_defends_are_wanted_under_the_setting() {
    let guard = |i: i64| json!({"name": format!("Guard {i}"), "cost": 1, "description": "Gain 8 Block."}); // (as the draw pile lists a card)
    let guards = |n: i64| -> Vec<Value> { (0..n).map(guard).collect() };
    let offer = c("SHRUG_IT_OFF", "Shrug It Off", "Skill", 1, "Gain 8 Block. Draw 1 card.");
    let other = c("SAP", "Sap", "Skill", 1, "Draw 2 cards.");
    // The Defends are not Block cards the deck took, upgraded or not.
    let base = [times(IRON_STRIKE(), 5), times(IRON_DEFEND(), 4), vec![json!({"name": "Defend+", "cost": 1, "description": "Gain 8 Block."})]].concat();
    assert!(!base.iter().any(strategy::_blocks));
    let plain = deck_fit(&offer, &base, 1);
    let held: Vec<f64> = {
        let _k = learn::with_knobs(&[("block_want", 1.5)]);
        let held = [0, 1, 2, 3].iter().map(|n| deck_fit(&offer, &[base.clone(), guards(*n)].concat(), 1)).collect();
        approx(deck_fit(&other, &base, 1), deck_fit(&other, &[base.clone(), guards(1)].concat(), 1)); // (no other card moves)
        // A deck of 24 has room for one more.
        let big = [base.clone(), times(IRON_STRIKE(), 12), guards(2)].concat();
        approx(deck_fit(&offer, &big, 2) - deck_fit(&other, &big, 2), 1.5);
        // One that costs more than a turn's energy is not wanted for it.
        assert!(deck_fit(&c("BULWARK", "Bulwark", "Skill", 4, "Gain 30 Block."), &base, 1) < 0.0);
        held
    };
    approx(held[0], plain + 1.5);
    approx(held[1], held[0]); // the second as much as the first
    approx(held[2], plain + 0.75); // half for the third
    approx(held[3], plain); // and then a Block card is worth what it is rated
}

#[test]
fn test_the_lean_to_attacks_lets_go_once_the_deck_has_its_share_under_the_setting() {
    let hit = |i: i64| json!({"name": format!("Hit {i}"), "cost": 1, "description": "Deal 9 damage."}); // (as the draw pile lists a card: no id, no type)
    let with_hits = |base: &[Value], n: i64| -> Vec<Value> { base.iter().cloned().chain((0..n).map(hit)).collect() };
    let offer = c("NEW_HIT", "New Hit", "Attack", 1, "Deal 10 damage.");
    // Upgraded Strikes are not cards the deck took: they leave the room as it was.
    let base = [times(IRON_STRIKE(), 3), times(json!({"name": "Strike+", "cost": 1, "description": "Deal 9 damage."}), 2), times(IRON_DEFEND(), 9)].concat();
    assert_eq!(strategy::_hitters(&with_hits(&base, 2)), 2);
    let (always, capped): (Vec<f64>, Vec<f64>) = {
        let _k = learn::with_knobs(&[("punch", 1.8)]);
        let always = [3, 6].iter().map(|n| deck_fit(&offer, &with_hits(&base, *n), 2)).collect();
        let _k2 = learn::with_knobs(&[("attack_room", 3.0)]);
        (always, [3, 4, 5, 6].iter().map(|n| deck_fit(&offer, &with_hits(&base, *n), 2)).collect())
    };
    assert!(always[1] > 1.5); // (without the setting a deck of six such attacks still leans to a seventh)
    approx(capped[0], always[0]); // three held: the lean in full
    for (more, fewer) in capped.iter().zip(&capped[1..]) {
        assert!(more > fewer);
    }
    approx(capped[3], 0.0); // six held: an attack is worth what it is rated, no more
}

#[test]
fn test_a_card_is_worth_more_beside_the_cards_other_players_win_with_it() {
    let power = |name: &str| c(&name.to_uppercase().replace(' ', "_"), name, "Power", 1, "Whenever a card is Exhausted, draw 1 card.");
    let deck = [times(IRON_STRIKE(), 9), times(c("TRUE_GRIT", "True Grit", "Skill", 1, "Gain 7 Block. Exhaust a random card in your Hand."), 2)].concat();
    let beside_feel = [deck.clone(), vec![power("Feel No Pain")]].concat();
    let alone = deck_fit(&power("Dark Embrace"), &deck, 2);
    let beside = deck_fit(&power("Dark Embrace"), &beside_feel, 2);
    assert!(beside > alone + 1.0);
    // A card of no such group gains nothing from it.
    assert_eq!(deck_fit(&power("Barricade"), &beside_feel, 2), deck_fit(&power("Barricade"), &deck, 2));
}

#[test]
fn test_a_card_dearer_than_a_turns_energy_is_not_the_damage_a_deck_is_short_of() {
    let strike = c("STRIKE_NECROBINDER", "Strike", "Attack", 1, "Deal 6 damage.");
    let bury = c("BURY", "Bury", "Attack", 4, "Deal 52 damage.");
    let kick = c("KINGLY_KICK", "Kingly Kick", "Attack", 4, "Deal 27 damage. Whenever you draw this card, reduce its cost by 1.");
    let wisp = c("WISP", "Wisp", "Skill", 0, "Gain [necrobinder_energy_icon.png]. Exhaust.");
    let reave = with(strike.clone(), json!({"id": "REAVE", "name": "Reave", "description": "Deal 10 damage."}));
    let deck = [times(strike, 4), times(c("DEFEND_NECROBINDER", "Defend", "Skill", 1, "Gain 5 Block."), 4), vec![reave]].concat();
    assert!(deck_fit(&bury, &deck, 2) <= -3.0); // 13 damage an energy on paper, and never played
    assert!(strategy::deck_fit(&bury, &deck, Some(2), 4) > 0.0); // with a fourth energy it is what the deck wants
    assert!(deck_fit(&bury, &[deck.clone(), vec![wisp.clone(), wisp]].concat(), 2) > deck_fit(&bury, &deck, 2));
    assert!(deck_fit(&kick, &deck, 2) >= 0.0); // one that gets cheaper is not held against
}

#[test]
fn test_a_card_is_judged_against_the_characters_other_picks() {
    let mut stats = learn::CardStats::new(None);
    // Every pick "beat the usual" (the bot got better meanwhile); Slice only by a little.
    stats.cards.insert("The Silent".into(), IndexMap::from([("FOOTWORK".to_string(), [120.0, 1200.0]), ("SLICE".to_string(), [120.0, 240.0])]));
    stats.finals.insert("The Silent".into(), vec![20; 10]);
    assert!(stats.shift("The Silent", "Footwork", None) > 0.5);
    assert!(stats.shift("The Silent", "Slice", None) < -0.5);
}

// EndOfRun

#[test]
fn test_goes_through_the_summary_so_the_score_is_banked() {
    let mut mem = Memory::new();
    let over = |options: &[&str]| json!({"state_type": "game_over", "game_over": {"options": options}});
    assert_eq!(strategy::game_over(&over(&["continue", "force_main_menu"]), &mut mem), [act("menu_select", json!({"option": "continue"}))]);
    assert_eq!(strategy::game_over(&over(&["force_main_menu"]), &mut mem), []); // the score is still counting up
    assert_eq!(strategy::game_over(&over(&["main_menu", "force_main_menu"]), &mut mem), [act("menu_select", json!({"option": "main_menu"}))]);
    for _ in 0..strategy::OVER_PATIENCE {
        strategy::game_over(&over(&["force_main_menu"]), &mut mem);
    }
    assert_eq!(strategy::game_over(&over(&["force_main_menu"]), &mut mem), [act("menu_select", json!({"option": "force_main_menu"}))]);
    // An older mod only knows the one option.
    assert_eq!(strategy::game_over(&json!({"state_type": "game_over"}), &mut Memory::new()), [act("menu_select", json!({"option": "main_menu"}))]);
}

// Upgrades

#[test]
fn test_the_upgrade_that_adds_most_is_taken() {
    let strike = (card(0, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy"), json!({"type": "Attack", "cost": 1, "description": "Deal 9 damage."}));
    let big = (card(1, "DEVASTATE", "Devastate", "Attack", 1, "Deal 35 damage.", "AnyEnemy"), json!({"type": "Attack", "cost": 1, "description": "Deal 47 damage."}));
    let cheap = (card(2, "X", "Body Slam", "Attack", 1, "Deal damage equal to your Block.", "AnyEnemy"), json!({"type": "Attack", "cost": 0, "description": "Deal damage equal to your Block."}));
    assert_eq!(upgrade_gain(&strike.0, &strike.1), 3.0);
    assert_eq!(upgrade_gain(&big.0, &big.1), 12.0);
    assert_eq!(upgrade_gain(&cheap.0, &cheap.1), 6.0); // a point of energy saved on every play
    let table = |x: &Value| -> Option<f64> {
        match x["id"].as_str() {
            Some("STRIKE_R") => Some(3.0),
            Some("DEVASTATE") => Some(12.0),
            Some("SHRUG_IT_OFF") => Some(3.0),
            _ => None,
        }
    };
    let shrug = card(3, "SHRUG_IT_OFF", "Shrug It Off", "Skill", 1, "Gain 8 Block. Draw 1 card.", "Self");
    let mut mem = Memory { upgrade_gain: Some(Rc::new(table)), ..Memory::new() };
    let state = json!({"state_type": "card_select", "run": {"act": 1, "floor": 6},
                       "card_select": {"screen_type": "upgrade", "prompt": "Choose a card to Upgrade.", "cards": [strike.0, shrug, big.0]}});
    let acts = strategy::card_select(&state, &mut mem);
    let first = acts.iter().find(|a| a.0 == "select_card").unwrap();
    assert_eq!(first.1["index"], 1); // Devastate, though Shrug It Off is the better-rated card
}

// Tuning

#[test]
fn test_settings_that_do_well_get_used_more() {
    // (Whatever is on trial just now: the sampler is what is tested, on a setting with two values.)
    {
        let d = tempfile::tempdir().unwrap();
        let _k = learn::with_knobs(&[]);
        let _c = learn::with_choices(&[("caution", &[0.75, 1.0])]);
        let path = d.path().join("tuning.json");
        let mut knobs = learn::Knobs::new(Some(path.clone()), Some(Random::new(1)));
        for _ in 0..300 {
            knobs.draw();
            knobs.record(Some(if learn::knob("caution") == 0.75 { 8.0 } else { -4.0 })); // bold runs get further
        }
        let used: Vec<f64> = (0..40).map(|n| learn::Knobs::new(Some(path.clone()), Some(Random::new(n))).draw()["caution"]).collect();
        assert!(used.iter().filter(|x| **x == 0.75).count() > 30);
    }
    assert_eq!(learn::knob("caution"), 1.0); // the defaults are back
}

// Strategy

/// `self.base(kind, hp=80, gold=100, act=1, max_hp=80, **body)`: the keywords and the body as one dict.
fn base(kind: &str, kw: Value) -> Value {
    let or = |k: &str, default: i64| kw.get(k).cloned().unwrap_or(json!(default));
    let mut state = json!({"state_type": kind, "run": {"act": or("act", 1), "floor": 5}, "player": {"hp": or("hp", 80), "max_hp": or("max_hp", 80), "gold": or("gold", 100), "potions": []}});
    for (k, v) in kw.as_object().unwrap() {
        if !["hp", "gold", "act", "max_hp"].contains(&k.as_str()) {
            state[k.as_str()] = v.clone();
        }
    }
    state
}

/// `self.rest(hp, ids=("REST", "SMITH"), **kw)`.
fn rest(hp: i64, ids: &[&str], kw: Value) -> Value {
    let options: Vec<Value> = ids.iter().enumerate().map(|(i, x)| json!({"index": i, "id": x, "is_enabled": true})).collect();
    base("rest_site", with(with(json!({"hp": hp}), json!({"rest_site": {"options": options}})), kw))
}

/// `self.shop(gold, *items, **kw)`.
fn shop(gold: i64, items: &[Value]) -> Value {
    base("shop", json!({"gold": gold, "shop": {"items": items, "can_proceed": true}}))
}

fn first_index(acts: &[Action]) -> i64 {
    acts[0].1["index"].as_i64().unwrap()
}

#[test]
fn test_map_avoids_elite_when_hurt_and_takes_it_when_healthy() {
    let m = json!({"next_options": [opt(0, 0, 1, "Elite"), opt(1, 1, 1, "Monster")], "nodes": [node(0, 1, "Elite", &[]), node(1, 1, "Monster", &[])]});
    let (hurt, healthy) = (base("map", json!({"hp": 30, "act": 2, "map": m})), base("map", json!({"hp": 80, "act": 2, "map": m}))); // (early act 1 elites are avoided regardless)
    assert_eq!(*first(&strategy::map_screen(&hurt, &mut Memory::new())), json!({"index": 1}));
    assert_eq!(*first(&strategy::map_screen(&healthy, &mut Memory::new())), json!({"index": 0}));
}

#[test]
fn test_map_looks_ahead() {
    let m = json!({"next_options": [opt(0, 0, 1, "Monster"), opt(1, 1, 1, "Monster")],
                   "nodes": [node(0, 1, "Monster", &[(0, 2)]), node(1, 1, "Monster", &[(1, 2)]), node(0, 2, "Monster", &[]), node(1, 2, "Treasure", &[])]});
    assert_eq!(*first(&strategy::map_screen(&base("map", json!({"map": m})), &mut Memory::new())), json!({"index": 1}));
}

#[test]
fn test_badly_hurt_an_elite_is_never_the_way_on_while_there_is_another() {
    // 14 of 76 HP in act 2: both roads are dead on paper (-67.1 and -72.2), and the elite's relic and the rest site
    // behind it tipped it. The Unknown room is a fight one time in five; the elite always is.
    let nodes = [node(2, 10, "Elite", &[(2, 11)]), node(3, 10, "Unknown", &[(3, 11)]), node(2, 11, "RestSite", &[(2, 12)]), node(3, 11, "Monster", &[(2, 12)]),
                 node(2, 12, "Monster", &[(2, 13)]), node(2, 13, "Boss", &[])];
    let opts = [opt(0, 2, 10, "Elite"), opt(1, 3, 10, "Unknown")];
    let mut state = base("map", json!({"hp": 14, "max_hp": 76, "act": 2, "map": {"next_options": opts, "nodes": nodes}}));
    let _k = learn::with_knobs(&[("elite_worth", 6.0)]);
    assert_eq!(*first(&strategy::map_screen(&state, &mut Memory::new())), json!({"index": 1}));
    {
        let _floor = strategy::ELITE_FLOOR.set(0.0);
        assert_eq!(*first(&strategy::map_screen(&state, &mut Memory::new())), json!({"index": 0})); // (what the values alone said)
    }
    state["map"]["next_options"] = json!(opts[..1]); // no other way: the elite it is
    assert_eq!(*first(&strategy::map_screen(&state, &mut Memory::new())), json!({"index": 0}));
}

#[test]
fn test_route_keeps_enough_health_for_the_boss() {
    // An elite right before the boss costs more than its relic is worth, even from full health. With a rest site in between
    // an act 1 elite (20 HP) is paid for: from 80% HP up a boss is beaten as often as from full. An act 2 elite (27 HP)
    // needs two, and nothing pays for one met badly hurt.
    fn pick(hp: i64, mid: &str, act: i64) -> i64 {
        let mids: Vec<&str> = mid.split('+').collect();
        let mut nodes = vec![node(0, 12, "Elite", &[(0, 13)]), node(1, 12, "Monster", &[(0, 13)])];
        nodes.extend(mids.iter().enumerate().map(|(n, kind)| node(0, 13 + n as i64, kind, &[(0, 14 + n as i64)])));
        nodes.push(node(0, 13 + mids.len() as i64, "Boss", &[]));
        let route = json!({"next_options": [opt(0, 0, 12, "Elite"), opt(1, 1, 12, "Monster")], "nodes": nodes});
        first_index(&strategy::map_screen(&base("map", json!({"hp": hp, "act": act, "map": route})), &mut Memory::new()))
    }

    assert_eq!(pick(80, "Unknown", 1), 1);
    {
        let _k = learn::with_knobs(&[("elite_worth", 6.0)]); // (the worth in force; at 2.5 the monster and an upgrade win in act 1)
        assert_eq!(pick(80, "RestSite", 1), 0);
    }
    assert_eq!(pick(60, "RestSite", 1), 1);
    assert_eq!(pick(80, "RestSite", 2), 1);
    {
        let _k = learn::with_knobs(&[("elite_worth", 4.0)]); // (since the act 2 curve, 18.2 against 18.3 at 2.5)
        assert_eq!(pick(80, "RestSite+RestSite", 2), 0);
    }
    assert_eq!(pick(40, "RestSite+RestSite", 1), 1);
    assert_eq!(strategy::boss_odds(0.9, 1), strategy::boss_odds(1.0, 1));
    assert!(strategy::boss_odds(0.8, 1) > 2.0 * strategy::boss_odds(0.6, 1) - 0.1);
    assert!(strategy::boss_odds(0.6, 1) > strategy::boss_odds(0.5, 1));
    // The last boss has only been beaten from 95% HP up: there, 85% is a third of the way to nothing.
    assert_eq!(strategy::boss_odds(0.96, 3), 1.0);
    assert!(strategy::boss_odds(0.85, 3) < 0.6);
    assert!(strategy::boss_odds(0.7, 3) < 0.25);
    let fight_or_rest = ([("Monster", 12), ("RestSite", 13), ("Boss", 14)], [("RestSite", 12), ("RestSite", 13), ("Boss", 14)]);
    assert!(path_value(&fight_or_rest.1, 50.0, 80, 0, 3) > path_value(&fight_or_rest.0, 50.0, 80, 0, 3) + 3.0);
}

#[test]
fn test_rest_heals_when_low_or_before_the_boss_and_smiths_when_healthy() {
    let s = rest(60, &["REST", "SMITH"], json!({})); // 75%
    assert_eq!(first_index(&strategy::rest_site(&s, &mut Memory::new())), 1);
    assert_eq!(first_index(&strategy::rest_site(&s, &mut Memory { boss_next: true, ..Memory::new() })), 0);
    assert_eq!(*first(&strategy::rest_site(&rest(30, &["rest", "smith"], json!({})), &mut Memory::new())), json!({"index": 0}));
    assert_eq!(*first(&strategy::rest_site(&rest(78, &["rest", "smith"], json!({})), &mut Memory::new())), json!({"index": 1}));
}

#[test]
fn test_avoids_elites_early_in_act_one_and_notes_the_boss() {
    let mut nodes = vec![node(0, 2, "Elite", &[(0, 3)]), node(1, 2, "Monster", &[(0, 3)]), node(0, 3, "Boss", &[])];
    let mut s = base("map", json!({"hp": 80, "map": {"next_options": [opt(0, 0, 2, "Elite"), opt(1, 1, 2, "Monster")], "nodes": nodes}}));
    let mut mem = Memory::new();
    assert_eq!(first_index(&strategy::map_screen(&s, &mut mem)), 1);
    assert!(mem.boss_next);
    // Later on an elite is fine, as long as there are rest sites enough before the boss.
    nodes.truncate(2);
    nodes.extend([node(0, 3, "RestSite", &[(0, 4)]), node(0, 4, "RestSite", &[(0, 5)]), node(0, 5, "Boss", &[])]);
    s["map"]["nodes"] = json!(nodes);
    s["run"]["act"] = json!(2);
    {
        let _k = learn::with_knobs(&[("elite_worth", 4.0)]); // where the learner values a relic that much
        assert_eq!(first_index(&strategy::map_screen(&s, &mut mem)), 0);
    }
    // At a low worth it is not: a bad fight there means meeting the boss short of full HP.
    let _k = learn::with_knobs(&[("elite_worth", 1.5)]);
    assert_eq!(first_index(&strategy::map_screen(&s, &mut mem)), 1);
}

#[test]
fn test_a_badly_hurt_bot_rests_even_when_the_path_says_smith() {
    // Act 3 at 6 of 109 HP, the boss next: the path values came out small, so the flat Smith bonus beat healing.
    let (s, mut mem) = (rest(6, &["REST", "SMITH"], json!({"max_hp": 109, "act": 3})), boss_after(1, 13, 3));
    let _k = learn::with_knobs(&[("upgrade_worth", 2.5)]); // the setting the learner usually picks
    assert_eq!(first_index(&strategy::rest_site(&s, &mut mem)), 0);
}

#[test]
fn test_an_upgrade_counts_the_same_in_act_one_whatever_the_setting() {
    // Act 1 lost 20 runs in 86 with the upgrade at 2.5 and 16 in 143 at 4.0; from act 2 on the lower worth got past the
    // boss oftener. So the setting only speaks from act 2.
    fn value(act: i64, worth: f64) -> f64 {
        let _k = learn::with_knobs(&[("upgrade_worth", worth)]); // (too healthy to rest: the upgrade is taken)
        path_value(&[("RestSite", 9), ("Monster", 10), ("Boss", 16)], 76.0, 80, 100, act)
    }

    assert_eq!(value(1, 2.5), value(1, 4.0));
    approx(value(2, 4.0) - value(2, 2.5), 1.5);
}

#[test]
fn test_heals_before_a_boss_when_below_ninety_percent() {
    // Act 2 at 61 of 75 HP, the boss next: a flat upgrade bonus beat the heal (the 19% of HP a rest gives is worth
    // little to the path model), and the bot entered the boss at 81%. Before a boss, rest.
    let (mut s, mut mem) = (rest(61, &["REST", "SMITH"], json!({"max_hp": 75, "act": 2})), boss_after(0, 14, 3));
    let _k = learn::with_knobs(&[("upgrade_worth", 2.5)]);
    assert_eq!(first_index(&strategy::rest_site(&s, &mut mem)), 0);
    // From 85% HP up the act 1 boss is beaten as often as from full: upgrade then.
    (s["player"]["hp"], s["run"]["act"]) = (json!(66), json!(1));
    assert_eq!(first_index(&strategy::rest_site(&s, &mut mem)), 1);
    // ... but not before the act 2 boss (85-97% HP won 0.88 as often as 97%+) or the last.
    for act in [2, 3] {
        s["run"]["act"] = json!(act);
        assert_eq!(first_index(&strategy::rest_site(&s, &mut mem)), 0, "{act}");
    }
    (s["player"]["hp"], s["run"]["act"]) = (json!(72), json!(2)); // 96%: upgrade
    assert_eq!(first_index(&strategy::rest_site(&s, &mut mem)), 1);
}

#[test]
fn test_event_avoids_fatal_and_cursed_options() {
    let ev = json!({"options": [{"index": 0, "title": "Grab", "description": "Obtain a relic. Lose 50 HP."}, {"index": 1, "title": "Pray", "description": "Gain 30 gold."},
                                {"index": 2, "title": "Take", "description": "Obtain a relic. Gain a Curse.", "is_locked": true}]});
    let acts = strategy::event(&base("event", json!({"hp": 20, "event": ev})), &mut Memory::new());
    assert_eq!(*first(&acts), json!({"index": 1}));
    assert!(!acts.contains(&act("choose_event_option", json!({"index": 2}))));
}

#[test]
fn test_the_card_made_free_for_the_fight_is_the_dearest() {
    let cards_ = [with(card(0, "ASTRAL_PULSE", "Astral Pulse+", "Attack", 0, "Deal 18 damage to ALL enemies.", "None"), json!({"rarity": "Rare"})),
                  card(1, "BULWARK", "Bulwark+", "Skill", 2, "Gain 15 Block.", "None"), with(card(2, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "None"), json!({"rarity": "Basic"}))];
    let acts = strategy::hand_select(&base("hand_select", json!({"hand_select": {"prompt": "Choose a card to make free.", "cards": cards_}})), &mut Memory::new());
    assert_eq!(acts[0], act("combat_select_card", json!({"card_index": 1})));
}

#[test]
fn test_removal_picks_curse_then_basics() {
    let cards_ = [with(card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage.", "None"), json!({"rarity": "Uncommon"})),
                  with(card(1, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "None"), json!({"rarity": "Basic"})), card(2, "REGRET", "Regret", "Curse", 0, "Unplayable.", "None")];
    let acts = strategy::card_select(&base("card_select", json!({"card_select": {"screen_type": "select", "prompt": "Choose a card to Remove.", "cards": cards_}})), &mut Memory::new());
    assert_eq!(acts[..3].iter().map(|a| a.1["index"].clone()).collect::<Vec<_>>(), [json!(2), json!(1), json!(0)]);
}

#[test]
fn test_choose_a_card_is_offered_again_after_an_ignored_click() {
    let cards = [card(0, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 7 damage.", "None"),
                 card(1, "SLOTH", "Sloth", "Status", 0, "You cannot play more than 3 cards each turn.", "None")];
    let s = base("card_select", json!({"card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": cards}}));
    let mut mem = Memory::new();
    let first = strategy::card_select(&s, &mut mem);
    assert_eq!(first.iter().map(|a| a.0.as_str()).collect::<BTreeSet<_>>(), BTreeSet::from(["select_card"])); // never confirm_selection here
    mem.selected.extend([0, 1]); // both were clicked on earlier, identical screens
    assert_eq!(strategy::card_select(&s, &mut mem), first);
}

#[test]
fn test_grid_selection_starts_over_when_clicks_did_not_register() {
    let s = base("card_select", json!({"card_select": {"screen_type": "select", "prompt": "Choose 2 cards.", "cards": [STRIKE(0), DEFEND(1)]}}));
    let mut mem = Memory::new();
    strategy::card_select(&s, &mut mem);
    mem.selected.extend([0, 1]);
    assert!(strategy::card_select(&s, &mut mem).contains(&act("select_card", json!({"index": 0}))));
}

#[test]
fn test_stops_ticking_cards_once_enough_are_chosen() {
    let s = base("card_select", json!({"card_select": {"screen_type": "enchant", "prompt": "Choose 3 cards to Enchant.", "cards": (0..5).map(STRIKE).collect::<Vec<_>>()}}));
    let mut mem = Memory::new();
    for _ in 0..3 {
        let (action, params) = strategy::card_select(&s, &mut mem).remove(0);
        assert_eq!(action, "select_card");
        mem.selected.insert(params["index"].as_i64().unwrap());
    }
    assert_eq!(strategy::card_select(&s, &mut mem)[0], act("confirm_selection", json!({})));
}

#[test]
fn test_shop_buys_removal_and_relic_but_not_weak_cards() {
    let strike = with(item(0, "card", 50), json!({"card_id": "STRIKE_R", "card_type": "Attack", "card_cost": "1", "card_rarity": "Basic", "card_description": "Deal 6 damage."}));
    let mut shelf = vec![strike, item(1, "relic", 150), item(2, "card_removal", 75), with(item(3, "relic", 300), json!({"can_afford": false}))];
    let mut mem = Memory::new();
    let (acts, seen) = logging::capture(|| {
        let acts = strategy::shop(&shop(250, &shelf), &mut mem, "shop");
        strategy::shop(&shop(250, &shelf), &mut mem, "shop"); // (looked at again: the shelf is written down once)
        acts
    });
    assert_eq!(acts, [act("shop_purchase", json!({"index": 2})), act("shop_purchase", json!({"index": 1})), act("proceed", json!({}))]);
    // The log has the whole shelf: what each thing cost, what we made of it, and what we could not pay for.
    let lines: Vec<String> = seen.iter().filter(|r| r.logger == "sts2bot.strategy").map(|r| r.output()).filter(|m| m.contains("shop (gold 250, potions 0 of 3)")).collect();
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert!(line.contains("card_removal 75 [9.0]"), "{line}");
    assert!(line.contains("relic 300! [6.5]"), "{line}");
    assert!(regex::Regex::new(r"card \S* ?50 \[-?\d").unwrap().is_match(line), "{line}");
    // Ascension 6: the second removal costs 150, and the relic at the same price comes first.
    shelf[2]["price"] = json!(150);
    assert_eq!(strategy::shop(&shop(250, &shelf), &mut Memory::new(), "shop")[0], act("shop_purchase", json!({"index": 1})));
}

#[test]
fn test_early_in_a_run_good_cards_come_before_the_removal_under_the_setting() {
    let good = with(item(0, "card", 24), json!({"card_id": "SHRUG_IT_OFF", "card_name": "Shrug It Off", "card_type": "Skill", "card_cost": "1", "card_rarity": "Common",
                                                "card_description": "Gain 8 Block. Draw 1 card."}));
    let mut state = shop(113, &[good, item(1, "card_removal", 100)]);
    let buys = |state: &Value| strategy::shop(state, &mut Memory::new(), "shop").remove(0);
    {
        let _v = strategy::with_deck_value_hook(|_| 7.7); // (a card the ratings think well of, as on the shelf seen)
        assert_eq!(buys(&state), act("shop_purchase", json!({"index": 1}))); // as it was: the removal first
        let _k = learn::with_knobs(&[("cards_first", 1.0)]);
        assert_eq!(buys(&state), act("shop_purchase", json!({"index": 0})));
        state["run"]["act"] = json!(3); // by act 3 a deck is thinned before it is fed
        assert_eq!(buys(&state), act("shop_purchase", json!({"index": 1})));
    }
    let _k = learn::with_knobs(&[("cards_first", 1.0)]);
    state["run"]["act"] = json!(1);
    let _v = strategy::with_deck_value_hook(|_| 5.0); // a middling card still waits for the removal
    assert_eq!(buys(&state), act("shop_purchase", json!({"index": 1})));
}

#[test]
fn test_the_fake_merchants_snecko_eye_is_left_on_the_shelf() {
    let fake = |i: i64, rid: &str, price: i64| with(item(i, "relic", price), json!({"relic_id": rid, "relic_name": rid}));
    let shelf = json!({"items": [fake(0, "FAKE_STRIKE_DUMMY", 43), fake(1, "FAKE_SNECKO_EYE", 57), fake(2, "FAKE_ANCHOR", 57)], "can_proceed": true});
    let mut state = base("fake_merchant", json!({"gold": 319, "fake_merchant": {"shop": shelf}}));
    // "Start each combat Confused" with nothing for it is never bought; the others go by what they do, not by price.
    assert_eq!(strategy::shop(&state, &mut Memory::new(), "fake_merchant"), [act("shop_purchase", json!({"index": 2})), act("shop_purchase", json!({"index": 0})), act("proceed", json!({}))]);
    // The gold is the same gold a real Merchant takes: short of plenty, only the ones that do something are bought.
    state["player"]["gold"] = json!(180);
    assert_eq!(strategy::shop(&state, &mut Memory::new(), "fake_merchant"), [act("shop_purchase", json!({"index": 2})), act("proceed", json!({}))]);
}

#[test]
fn test_with_lords_parasol_the_shelf_empties_itself_and_nothing_is_bought() {
    // The relic buys everything in turn; a purchase of ours in the middle crashed the game three times.
    let mut state = shop(250, &[item(0, "relic", 150), with(item(1, "relic", 150), json!({"is_stocked": false}))]);
    state["player"]["relics"] = json!([{"id": "LORDS_PARASOL", "description": "When you encounter the Merchant, immediately obtain EVERYTHING he sells."}]);
    let mut mem = Memory::new();
    assert_eq!(strategy::shop(&state, &mut mem, "shop"), []); // it is still at it: wait
    state["shop"]["items"][0]["is_stocked"] = json!(false);
    assert_eq!(strategy::shop(&state, &mut mem, "shop"), [act("proceed", json!({}))]);
    // Something it cannot take (a potion with the belt full) does not keep us there for ever.
    state["shop"]["items"][0]["is_stocked"] = json!(true);
    let looks: Vec<Vec<Action>> = (0..9).map(|_| strategy::shop(&state, &mut mem, "shop")).collect();
    assert_eq!(looks[8], [act("proceed", json!({}))]);
    assert!(looks.iter().all(|a| a.is_empty() || *a == [act("proceed", json!({}))]));
}

#[test]
fn test_a_shop_before_the_boss_fills_the_potion_belt() {
    let mut state = shop(100, &[item(0, "potion", 60), item(1, "relic", 250)]);
    let buys = |state: &Value| strategy::shop(state, &mut Memory::new(), "shop").remove(0);
    state["player"]["max_potion_slots"] = json!(3);
    // Mid-act the last gold is kept; two rooms from the boss it is not, and the potion comes first.
    assert_eq!(buys(&state), act("shop_purchase", json!({"index": 1})));
    state["run"]["floor"] = json!(31);
    assert_eq!(buys(&state), act("shop_purchase", json!({"index": 0})));
    // ... before a card removal too, until the belt holds two.
    let removal = item(2, "card_removal", 100);
    state["shop"]["items"].as_array_mut().unwrap().push(removal);
    assert_eq!(buys(&state), act("shop_purchase", json!({"index": 0})));
    state["player"]["potions"] = json!([{}, {}]);
    assert_eq!(buys(&state), act("shop_purchase", json!({"index": 2})));
    state["shop"]["items"].as_array_mut().unwrap().pop();
    state["player"]["potions"] = json!([{}, {}, {}]);
    assert_eq!(buys(&state), act("shop_purchase", json!({"index": 1})));
}

#[test]
fn test_a_full_belt_gives_up_its_least_potion_for_a_better_one() {
    let belt = [json!({"slot": 0, "name": "Strength Potion", "description": "Gain 2 Strength."}), json!({"slot": 1, "name": "Swift Potion", "description": "Draw 3 cards."})];
    let offer = |name: &str, text: &str| json!({"index": 1, "type": "potion", "potion_name": name, "potion_description": text});
    let mut state = base("rewards", json!({"rewards": {"items": [{"index": 0, "type": "gold"}, offer("Fire Potion", "Deal 20 damage.")]}}));
    state["player"]["potions"] = json!(belt);
    state["player"]["max_potion_slots"] = json!(2);
    let mut mem = Memory::new();
    assert_eq!(strategy::rewards(&state, &mut mem), [act("discard_potion", json!({"slot": 1})), act("claim_reward", json!({"index": 0})), act("proceed", json!({}))]);
    mem.potion_swaps.insert((Some(1), Some(5)), 2); // (the game said yes twice and the belt is still full: no third time)
    assert_eq!(strategy::rewards(&state, &mut mem), [act("claim_reward", json!({"index": 0})), act("proceed", json!({}))]);
    // One no better than what is held is left lying, as before.
    state["rewards"]["items"][1] = offer("Gambler's Brew", "Discard any number of cards, then draw that many.");
    assert_eq!(strategy::rewards(&state, &mut Memory::new()), [act("claim_reward", json!({"index": 0})), act("proceed", json!({}))]);
    // ... and with Sozu nothing is poured away for a potion that cannot be taken.
    state["rewards"]["items"][1] = offer("Fire Potion", "Deal 20 damage.");
    state["player"]["potions"] = json!(belt[1..]);
    state["player"]["relics"] = json!([{"id": "SOZU", "description": "You can no longer obtain Potions."}]);
    assert_eq!(strategy::rewards(&state, &mut Memory::new()), [act("claim_reward", json!({"index": 0})), act("proceed", json!({}))]);
}

#[test]
fn test_no_potion_is_bought_with_a_relic_that_forbids_them() {
    let mut state = shop(100, &[item(0, "potion", 60)]);
    state["run"]["floor"] = json!(31);
    state["player"]["relics"] = json!([{"id": "SOZU", "description": "Gain energy at the start of each turn. You can no longer obtain Potions."}]);
    assert_eq!(strategy::shop(&state, &mut Memory::new(), "shop"), [act("proceed", json!({}))]);
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn table<V: Clone>(rows: &[(&str, V)]) -> IndexMap<String, V> {
    rows.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// `strategy.choose_character(setting, unlocked, runs_finished, max_ascension, epochs, recent=recent)`.
fn choose(setting: &str, who: &[&str], run: i64, levels: Option<&[(&str, i64)]>, epochs: Option<&IndexMap<String, String>>, recent: Option<&[&str]>) -> Option<String> {
    let levels = levels.map(table);
    let recent = recent.map(strings);
    strategy::choose_character(setting, &strings(who), run, levels.as_ref(), epochs, recent.as_deref())
}

fn some(name: &str) -> Option<String> {
    Some(name.to_string())
}

#[test]
fn test_auto_plays_the_character_with_the_lowest_ascension() {
    let pick = |asc: &[(&str, i64)]| choose("auto", &["IRONCLAD", "SILENT", "DEFECT"], 0, Some(asc), None, None);
    assert_eq!(pick(&[("IRONCLAD", 2), ("SILENT", 1), ("DEFECT", 2)]), some("SILENT"));
    assert!([some("SILENT"), some("DEFECT")].contains(&pick(&[("IRONCLAD", 1)]))); // never played counts as 0
}

#[test]
fn test_embarks_at_the_highest_unlocked_ascension() {
    let mut s = character_select(&[("IRONCLAD", false), ("SILENT", false), ("DEFECT", true)]);
    let mut mem = Memory { max_ascension: table(&[("IRONCLAD", 4), ("SILENT", 3)]), ..Memory::new() };
    assert_eq!(strategy::menu(&s, &mut mem, "auto")[0], act("menu_select", json!({"option": "SILENT"})));
    mem.character_selected = true;
    assert_eq!(strategy::menu(&s, &mut mem, "auto")[0], act("set_ascension", json!({"level": 3})));
    s["ascension"] = json!(3);
    assert_eq!(strategy::menu(&s, &mut mem, "auto")[0], act("menu_select", json!({"option": "confirm"})));
    // The levels are not on screen yet (just after picking the character): hold off, do not embark at ascension 0.
    (s["ascension"], s["max_ascension"]) = (json!(0), json!(0));
    mem.ascension_waits = 0;
    assert_eq!(strategy::menu(&s, &mut mem, "auto")[0], act("menu_select", json!({"option": "SILENT"})));
    // A mod without the patch never shows them: give up waiting and embark.
    s.as_object_mut().unwrap().shift_remove("ascension");
    s.as_object_mut().unwrap().shift_remove("max_ascension");
    let first: Vec<Action> = (0..25).map(|_| strategy::menu(&s, &mut mem, "auto").remove(0)).collect();
    assert_eq!(first[24], act("menu_select", json!({"option": "confirm"})));
}

#[test]
fn test_auto_rotates_among_the_lowest_by_who_played_longest_ago() {
    let ids = ["IRONCLAD", "THE_SILENT", "DEFECT"];
    let levels = [("IRONCLAD", 0), ("THE_SILENT", 0), ("DEFECT", 3)];
    let recent = ["The Silent", "The Ironclad", "The Defect", "The Silent"];
    assert_eq!(choose("auto", &ids, 7, Some(&levels), None, Some(&recent)), some("IRONCLAD")); // (the Silent played last)
    assert_eq!(choose("auto", &ids, 7, Some(&levels), None, Some(&[&recent[..], &["The Ironclad"]].concat())), some("THE_SILENT"));
    assert_eq!(choose("auto", &[&ids[..], &["WATCHER"]].concat(), 7, Some(&levels), None, Some(&recent)), some("WATCHER")); // never played: first
    assert_eq!(choose("auto", &ids, 7, Some(&[("IRONCLAD", 2), ("THE_SILENT", 0), ("DEFECT", 3)]), None, Some(&recent)), some("THE_SILENT")); // lowest still first
}

#[test]
fn test_auto_and_random_draw_among_the_lowest_ascension_only() {
    let who = ["IRONCLAD", "SILENT", "REGENT", "NECROBINDER"];
    let levels = [("IRONCLAD", 1), ("SILENT", 2), ("REGENT", 1), ("NECROBINDER", 1)];
    let set = |names: &[&str]| -> BTreeSet<Option<String>> { names.iter().map(|n| some(n)).collect() };
    for setting in ["auto", "random"] {
        let seen: BTreeSet<Option<String>> = (0..60).map(|run| choose(setting, &who, run, Some(&levels), None, None)).collect();
        assert_eq!(seen, set(&["IRONCLAD", "REGENT", "NECROBINDER"])); // never the Silent, everyone else in turn
        // The menu asks on every poll: the answer must not change during one run.
        assert_eq!((0..20).map(|_| choose(setting, &who, 7, Some(&levels), None, None)).collect::<BTreeSet<_>>().len(), 1);
        // The game's "Random" tile is not a character, whatever level it shows.
        let with_tile = [&who[..], &["RANDOM_CHARACTER"]].concat();
        assert!(!(0..60).map(|run| choose(setting, &with_tile, run, Some(&levels), None, None)).any(|c| c == some("RANDOM_CHARACTER")));
        let regent_low = [("IRONCLAD", 1), ("SILENT", 2), ("REGENT", 0), ("NECROBINDER", 1)];
        assert_eq!((0..20).map(|run| choose(setting, &who, run, Some(&regent_low), None, None)).collect::<BTreeSet<_>>(), set(&["REGENT"]));
    }
}

#[test]
fn test_a_run_is_played_at_the_top_level_even_with_epochs_left() {
    let epochs = table(&[("IRONCLAD4_EPOCH", "Revealed".to_string()), ("IRONCLAD7_EPOCH", "NotObtained".to_string())]);
    let mut mem = Memory { max_ascension: table(&[("IRONCLAD", 3)]), epochs, character_selected: true, ..Memory::new() };
    assert_eq!(strategy::menu(&character_select(&[("IRONCLAD", false)]), &mut mem, "IRONCLAD")[0], act("set_ascension", json!({"level": 3})));
}

#[test]
fn test_auto_plays_the_characters_that_still_owe_an_epoch() {
    let who = ["IRONCLAD", "SILENT", "REGENT", "NECROBINDER", "DEFECT"];
    let mut epochs: IndexMap<String, String> = who.iter().map(|x| (format!("{x}7_EPOCH"), "Revealed".to_string())).collect();
    for k in ["DEFECT7_EPOCH", "NECROBINDER7_EPOCH", "RELIC1_EPOCH"] {
        epochs.insert(k.to_string(), "NotObtained".to_string());
    }
    let set = |names: &[&str]| -> BTreeSet<Option<String>> { names.iter().map(|n| some(n)).collect() };
    // The Ironclad is a level behind, but has nothing of its own left to earn.
    let mut levels = [("IRONCLAD", 1), ("SILENT", 2), ("REGENT", 2), ("NECROBINDER", 2), ("DEFECT", 2)];
    assert_eq!((0..60).map(|run| choose("auto", &who, run, Some(&levels), Some(&epochs), None)).collect::<BTreeSet<_>>(), set(&["NECROBINDER", "DEFECT"]));
    // Among those that owe one, the lowest ascension still goes first.
    levels[4].1 = 1;
    assert_eq!((0..20).map(|run| choose("auto", &who, run, Some(&levels), Some(&epochs), None)).collect::<BTreeSet<_>>(), set(&["DEFECT"]));
    // All earned: back to evening out the levels.
    levels[4].1 = 2;
    let earned: IndexMap<String, String> = epochs.keys().map(|k| (k.clone(), "Revealed".to_string())).collect();
    assert_eq!((0..20).map(|run| choose("auto", &who, run, Some(&levels), Some(&earned), None)).collect::<BTreeSet<_>>(), set(&["IRONCLAD"]));
}

#[test]
fn test_one_character_can_be_put_first() {
    let mut epochs = table(&[("IRONCLAD7_EPOCH", "NotObtained".to_string()), ("REGENT4_EPOCH", "NotObtained".to_string())]);
    let (who, levels) = (["IRONCLAD", "REGENT"], [("IRONCLAD", 1)]);
    assert_eq!(choose("auto", &who, 0, Some(&levels), Some(&epochs), None), some("REGENT"));
    assert_eq!(choose("auto:ironclad", &who, 0, Some(&levels), Some(&epochs), None), some("IRONCLAD"));
    epochs.insert("IRONCLAD7_EPOCH".into(), "Revealed".into()); // done: back to the usual order
    assert_eq!(choose("auto:ironclad", &who, 0, Some(&levels), Some(&epochs), None), some("REGENT"));
}

#[test]
fn test_character_choice() {
    let unlocked = ["IRONCLAD", "SILENT", "REGENT"];
    assert_eq!(choose("silent", &unlocked, 0, None, None, None), some("SILENT"));
    assert_eq!(choose("DEFECT", &unlocked, 0, None, None, None), some("IRONCLAD"));
    assert_eq!((0..4).map(|n| choose("cycle", &unlocked, n, None, None, None)).collect::<Vec<_>>(), [some("IRONCLAD"), some("SILENT"), some("REGENT"), some("IRONCLAD")]);
}

// Afflictions

/// as the game sends it: no enemies on this screen (the original's defaults: turns None, status ().)
fn offer(hp: i64, turns: Value, status: Value) -> i64 {
    let waste = card(0, "WASTE_AWAY", "Waste Away", "Status", 0, "Gain 1 less [energy_icon.png] per turn.", "None");
    let burn = card(1, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 8 damage.", "None");
    let state = json!({"state_type": "card_select", "run": {"act": 2, "floor": 33}, "player": {"hp": hp, "max_hp": 80, "status": status},
                       "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": [waste, burn]}, "_turns": turns});
    first_index(&strategy::card_select(&state, &mut Memory::new()))
}

#[test]
fn test_damage_is_taken_over_lost_energy() {
    assert_eq!(offer(60, json!(null), json!([])), 1);
    assert_eq!(offer(30, json!(null), json!([])), 1); // an energy a turn costs as much, and for longer
    assert_eq!(offer(16, json!(null), json!([])), 0); // two burns from dead: keep the HP
}

#[test]
fn test_an_energy_that_was_not_being_spent_is_the_one_given_up() {
    // A Necrobinder on 5 to 7 energy under Sloth, three cards a turn and 1 to 5 energy over every turn, took 8 damage a turn on
    // 21 HP rather than lose an energy it had no card for.
    let spent: Vec<Value> = [vec![json!([4, 0])], vec![json!([3, 0]); 8]].concat();
    let then = |more: Value| json!([&spent[..5], &more.as_array().unwrap()[..]].concat());
    assert_eq!(offer(60, json!(spent), json!([])), 1);
    assert_eq!(offer(60, then(json!([[3, 4], [3, 1], [3, 5], [3, 4]])), json!([])), 0);
    assert_eq!(offer(60, then(json!([[3, 0], [3, 1], [3, 0], [3, 2]])), json!([])), 0); // (two turns in four is enough)
    assert_eq!(offer(60, then(json!([[3, 0], [3, 0], [3, 0], [3, 2]])), json!([])), 1);
    // ... and a second burn on top of the first is 15 a turn: the energy goes instead.
    assert_eq!(offer(60, json!(spent), json!([{"id": "DISINTEGRATION_POWER", "amount": 7}])), 0);
}

fn sloth(hp: i64, turns: Value) -> i64 {
    let burn = card(0, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 7 damage.", "None");
    let sloth = card(1, "SLOTH", "Sloth", "Status", 0, "You cannot play more than 3 cards each turn.", "None");
    let state = json!({"state_type": "card_select", "run": {"act": 2, "floor": 33}, "player": {"hp": hp, "max_hp": 90}, "_turns": turns,
                       "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": [burn, sloth]}});
    first_index(&strategy::card_select(&state, &mut Memory::new()))
}

#[test]
fn test_three_cards_a_turn_is_dear_for_a_deck_that_plays_six() {
    // The same Necrobinder had played 4, 6, 6 and 8 cards a turn; under Sloth it dealt 19 a round for 40.
    assert_eq!(sloth(48, json!([[2, 2], [4, 0], [6, 0], [6, 0], [8, 0]])), 0);
    assert_eq!(sloth(80, json!([[4, 0], [3, 0], [4, 0], [3, 0], [2, 1]])), 1); // three energy, four cards: Sloth costs little
    assert_eq!(sloth(80, json!([[4, 0], [3, 0], [9, 0], [3, 0], [2, 1]])), 1); // (one long turn is not a deck)
    assert_eq!(sloth(30, json!([[2, 2], [4, 0], [6, 0], [6, 0], [8, 0]])), 1); // no HP for six turns of 7
    assert_eq!(sloth(80, json!([])), 1); // (a bot started in the middle of the fight knows nothing: as before)
}

/// (see afflictions.SLOTH_MARGIN; turn one is left out)
#[test]
fn test_the_burn_is_the_cheaper_price_only_past_two_cards_a_turn_lost() {
    assert_eq!(sloth(80, json!([[2, 2], [6, 0], [4, 0], [5, 0], [3, 0]])), 1); // 3, 1, 2, 0: 1.5 a turn
    assert_eq!(sloth(80, json!([[2, 2], [6, 0], [7, 0], [3, 0], [4, 0]])), 1); // 3, 3, 0, 1: 1.75 a turn
    assert_eq!(sloth(80, json!([[2, 2], [6, 0], [5, 0], [5, 0], [5, 0]])), 0); // 3, 2, 2, 2: 2.25 a turn
}

// HandSelections

#[test]
fn test_a_sly_card_is_the_one_discarded() {
    let flick = with(card(1, "FLICK_FLACK", "Flick-Flack", "Attack", 1, "Deal 7 damage to ALL enemies.", "AllEnemies"),
                     json!({"keywords": [{"name": "Sly", "description": "If this card is discarded from your Hand before the end of your turn, play it for free."}]}));
    let mut state = json!({"state_type": "hand_select", "hand_select": {"prompt": "Choose a card to Discard.", "can_confirm": false, "cards": [STRIKE(0), flick, DEFEND(2)]},
                           "player": {"energy": 1}});
    assert_eq!(picks(&state)[0], 1);
    state["hand_select"]["prompt"] = json!("Choose a card to Exhaust.");
    assert_ne!(picks(&state)[0], 1);
}

/// the selection screen leaves these two out
fn bare(mut hand: Vec<Value>) -> Vec<Value> {
    for x in &mut hand {
        x.as_object_mut().unwrap().shift_remove("target_type");
        x.as_object_mut().unwrap().shift_remove("can_play");
    }
    hand
}

/// A Silent on 9 HP, the Insatiable on 59 with 36 Poison and Serpent Form up: two Strikes and any third card kill it.
fn insatiable(prompt: &str) -> Value {
    let hand = bare(vec![card(0, "FRANTIC_ESCAPE", "Frantic Escape", "Status", 3, "Move 1 space away from the Sandpit.", "Self"), STRIKE(1), STRIKE(2),
                         card(3, "SNAKEBITE", "Snakebite", "Skill", 2, "Retain. Apply 7 Poison.", "AnyEnemy"), DEFEND(4)]);
    let boss = json!({"entity_id": "THE_INSATIABLE_0", "hp": 59, "max_hp": 321, "block": 0, "intents": [{"type": "Attack", "label": "12x2"}],
                      "status": [{"id": "POISON_POWER", "amount": 36}, {"id": "SANDPIT_POWER", "amount": 3}]});
    json!({"state_type": "hand_select", "_fight": "boss", "hand_select": {"prompt": prompt, "can_confirm": false, "cards": hand},
           "battle": {"round": 9, "turn": "player", "is_play_phase": true, "enemies": [boss]},
           "player": {"hp": 9, "max_hp": 79, "block": 0, "energy": 3, "hand": [], "status": [{"id": "SERPENT_FORM_POWER", "amount": 4}]},
           "run": {"act": 2, "floor": 33}})
}

#[test]
fn test_the_discard_is_a_card_the_turn_does_not_need() {
    // By rating alone a Strike went: without it the 24 damage that leaves the boss to its Poison is not there.
    let discards = picks(&insatiable("Choose a card to Discard."));
    assert!(![1, 2].contains(&discards[0]));
    assert_eq!(*discards.last().unwrap(), 0); // and never the way out of the Sandpit
}

#[test]
fn test_a_card_is_not_exhausted_when_that_loses_the_kill() {
    assert!(![1, 2].contains(&picks(&insatiable("Choose a card to Exhaust."))[0]));
}

/// The Insatiable on 59 HP, the bot on 7 before 12x2: three attacks and the Bone Shards make 60 once the Mittens' Strength is in.
#[test]
fn test_an_escape_is_exhausted_when_every_other_card_is_the_kill() {
    let hand = bare(vec![card(0, "FRANTIC_ESCAPE", "Frantic Escape", "Status", 2, "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.", "Self"),
                         card(1, "DEBILITATE", "Debilitate", "Attack", 1, "Deal 19 damage. Vulnerable and Weak are twice as effective against the enemy for the next 2 turns.", "AnyEnemy"),
                         card(2, "VEILPIERCER", "Veilpiercer", "Attack", 1, "Deal 19 damage.", "AnyEnemy"),
                         card(3, "BONE_SHARDS", "Bone Shards", "Attack", 1, "If Osty is alive, he deals 9 damage to ALL enemies and you gain 9 Block. Osty dies.", "AllEnemies"),
                         card(4, "STRIKE_NECROBINDER", "Strike", "Attack", 1, "Deal 10 damage.", "AnyEnemy")]);
    let boss = json!({"entity_id": "THE_INSATIABLE_0", "hp": 59, "max_hp": 321, "block": 0, "intents": [{"type": "Attack", "label": "12x2"}], "status": [{"id": "SANDPIT_POWER", "amount": 2}]});
    let mittens = json!({"id": "TOASTY_MITTENS", "description": "At the start of your turn, Exhaust 1 card from your Hand and gain 1 Strength."});
    let mut state = json!({"state_type": "hand_select", "_fight": "boss", "hand_select": {"prompt": "Choose a card to Exhaust.", "can_confirm": false, "cards": hand},
                           "battle": {"round": 10, "turn": "player", "is_play_phase": true, "enemies": [boss]}, "run": {"act": 2, "floor": 33},
                           "player": {"hp": 7, "max_hp": 66, "block": 0, "energy": 5, "hand": [], "pets": [{"name": "Osty", "hp": 2, "alive": true}], "relics": [mittens]}});
    assert_eq!(picks(&state)[0], 0); // 20 + 20 + 9 + 11 = 60: the Escape is the card nothing is lost with
    // With the boss out of reach the Escape is kept as before, and the lowest-rated card goes.
    state["battle"]["enemies"][0]["hp"] = json!(200);
    assert_ne!(picks(&state)[0], 0);
}

/// An "any number" or "up to" selection, Confirm lit from the start: the actions the bot sends, in order.
/// (the original's defaults: fight "monster", energy 1, selected 0, relics ().)
fn optional(prompt: &str, hand: Vec<Value>, fight: &str, energy: i64, selected: i64, relics: &[&str]) -> Vec<Action> {
    let mut state = combat_state!(hand.clone(), [enemy!("A_0", 60, attack = 12)], energy = energy, hp = 60, max_hp = 80, kind = "hand_select");
    state["_fight"] = json!(fight);
    state["player"]["relics"] = json!(relics.iter().map(|r| json!({"id": r, "name": r})).collect::<Vec<_>>());
    state["hand_select"] = json!({"prompt": prompt, "can_confirm": true, "cards": hand});
    if selected != 0 {
        state["hand_select"]["selected_cards"] = json!((0..selected).map(|i| json!({"index": i})).collect::<Vec<_>>());
    }
    handpick::_answers_clear();
    strategy::hand_select(&state, &mut Memory::new())
}

/// `self.optional(prompt, hand)` with the defaults.
fn optional_(prompt: &str, hand: Vec<Value>) -> Vec<Action> {
    optional(prompt, hand, "monster", 1, 0, &[])
}

fn BANE(i: i64) -> Value {
    with(card(i, "ASCENDERS_BANE", "Ascender's Bane", "Curse", 0, "Unplayable. Ethereal.", "None"), json!({"can_play": false}))
}
fn WOUND(i: i64) -> Value {
    with(card(i, "WOUND", "Wound", "Status", 0, "Unplayable.", "None"), json!({"can_play": false}))
}
fn BURN(i: i64) -> Value {
    with(card(i, "BURN", "Burn", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 2 damage.", "None"), json!({"can_play": false}))
}
fn BASH(i: i64) -> Value {
    card(i, "BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy")
}

fn select(i: i64) -> Action {
    act("combat_select_card", json!({"card_index": i}))
}

fn confirm() -> Action {
    act("combat_confirm_selection", json!({}))
}

#[test]
fn test_an_optional_exhaust_takes_the_junk_before_it_confirms() {
    // 2026-11-26: Purity, Ashwater, Gambler's Brew and the Gambling Chip were all confirmed with nothing chosen.
    let ash = "Choose any number of cards to Exhaust.";
    let acts = optional_(ash, vec![STRIKE(0), BANE(1), WOUND(2), DEFEND(3)]);
    assert_eq!(acts[0], select(2)); // the Wound first: the Bane goes by itself at the end of the turn
    assert_eq!(optional_(ash, vec![STRIKE(0), BANE(1), DEFEND(2)])[0], select(1));
    // No junk left in a hallway fight: confirmed, the Strike kept.
    assert_eq!(optional_(ash, vec![STRIKE(0), DEFEND(1)])[0], confirm());
    // Purity's "up to 3" once three are chosen.
    assert_eq!(optional("Choose up to 3 cards to Exhaust.", vec![WOUND(0), STRIKE(1)], "monster", 1, 3, &[])[0], confirm());
}

#[test]
fn test_in_a_boss_fight_a_strike_the_turn_does_not_play_is_exhausted() {
    let purity = "Choose up to 3 cards to Exhaust.";
    let acts = optional(purity, vec![STRIKE(0), DEFEND(1), STRIKE(2), BASH(3)], "boss", 1, 0, &[]);
    assert_eq!(acts[0].0, "combat_select_card");
    assert!([0, 2].contains(&acts[0].1["card_index"].as_i64().unwrap())); // one energy plays one card: the other Strike is spare
    // A lone Strike that the turn's one energy goes to is kept.
    assert_eq!(optional(purity, vec![STRIKE(0), BASH(1)], "boss", 1, 0, &[])[0], confirm());
}

#[test]
fn test_a_replace_trades_what_the_turn_will_not_play_for_new_cards() {
    let brew = "Choose any number of cards to replace.";
    let hand = vec![BANE(0), STRIKE(1), BURN(2), BASH(3), DEFEND(4)];
    assert_eq!(optional_(brew, hand)[0], select(2)); // the Burn
    let rest = vec![BANE(0), STRIKE(1), BASH(2), DEFEND(3)];
    let acts = optional_(brew, rest.clone());
    assert_eq!(acts[0], select(1)); // one energy goes to the Defend: the Strike is a free draw instead
    assert!(!acts.contains(&select(0))); // (never the Bane: replaced, it would come back)
    let acts = optional_(brew, vec![BANE(0), BASH(1), DEFEND(2)]);
    assert_eq!(acts[0], select(1)); // ... and then Bash, which needs 2
    assert_eq!(optional_(brew, vec![BANE(0), DEFEND(1)])[0], confirm()); // the Defend is the turn
    // Under Runic Pyramid an unplayed card is next turn's: only junk goes.
    assert_eq!(optional(brew, rest, "monster", 1, 0, &["RUNIC_PYRAMID"])[0], confirm());
}

#[test]
fn test_out_of_a_fight_the_rating_decides_as_before() {
    let mut state = insatiable("Choose a card to Discard.");
    state["battle"]["enemies"] = json!([]);
    assert!(handpick::turn_costs(&state, state["hand_select"]["cards"].as_array().unwrap(), 1e-9).is_none());
}

#[test]
fn test_the_card_put_back_on_the_deck_is_one_it_would_not_play() {
    let comet = with(card(0, "COMET", "Comet", "Attack", 3, "Deal 33 damage.", "AnyEnemy"), json!({"rarity": "Rare"}));
    let pulse = card(1, "ASTRAL_PULSE", "Astral Pulse", "Attack", 0, "Deal 8 damage to ALL enemies twice.", "AllEnemies");
    let wound = card(3, "WOUND", "Wound", "Status", 0, "Unplayable.", "None");
    let mut state = json!({"state_type": "hand_select", "player": {"energy": 1},
                           "hand_select": {"prompt": "Choose a card to put on top of your Draw Pile.", "can_confirm": false, "cards": [comet, pulse, DEFEND(2), wound]}});
    assert_eq!(picks(&state)[0], 0); // 3 energy it does not have: Comet waits a turn, at no cost
    // With energy for everything, the dead card goes back before a card it would play.
    state["player"]["energy"] = json!(4);
    assert_eq!(picks(&state)[0], 3);
    // Energy for one card of two: the weaker one is the one left over.
    state["player"]["energy"] = json!(1);
    state["hand_select"]["cards"] = json!([STRIKE(0), card(1, "POMMEL_STRIKE", "Pommel Strike", "Attack", 1, "Deal 9 damage. Draw 1 card.", "AnyEnemy")]);
    assert_eq!(picks(&state)[0], 0);
}

/// Ten cards in hand, six of them copies, in a fight the plan can be made for, and nothing planned yet.
fn big_hand() -> Value {
    let hand = vec![STRIKE(0), STRIKE(1), STRIKE(2), DEFEND(3), DEFEND(4), DEFEND(5), DEFEND(6), DEFEND(7),
                    card(8, "BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy"), card(9, "SHRUG", "Shrug It Off", "Skill", 1, "Gain 8 Block. Draw 1 card.", "Self")];
    let mut state = combat_state!(hand.clone(), [enemy!("A_0", 60, attack = 12), enemy!("B_1", 40, attack = 9)], energy = 3, hp = 60, max_hp = 80, kind = "hand_select");
    state["hand_select"] = json!({"prompt": "Choose a card to Discard.", "can_confirm": false, "cards": hand});
    handpick::_answers_clear();
    state
}

fn cards_of(state: &Value) -> &[Value] {
    state["hand_select"]["cards"].as_array().unwrap()
}

/// `mock.patch.object(combat, "choose_card", wraps=combat.choose_card)`: the guard and the call count.
fn spy() -> (combat::ChooseCardGuard, Rc<Cell<usize>>) {
    let count = Rc::new(Cell::new(0));
    let seen = count.clone();
    let guard = combat::with_choose_card_hook(move |state, plan| {
        seen.set(seen.get() + 1);
        combat::_choose_card(state, plan)
    });
    (guard, count)
}

#[test]
fn test_a_confirmation_does_not_wait_for_the_plan() {
    // The plan per card took about two seconds with ten in hand, and the confirmation is first to go out anyway.
    let mut state = big_hand();
    state["hand_select"]["can_confirm"] = json!(true);
    let acts = {
        let _planned = combat::with_choose_card_hook(|_, _| panic!("planned"));
        strategy::hand_select(&state, &mut Memory::new())
    };
    assert_eq!(acts[0], confirm());
    assert!(acts.iter().any(|a| a.0 == "combat_select_card")); // (the fall-back stays, by rating)
}

#[test]
fn test_each_kind_of_card_is_planned_once() {
    let state = big_hand();
    let (costs, calls) = {
        let (_guard, count) = spy();
        (handpick::turn_costs(&state, cards_of(&state), -1e9).unwrap(), count.get()) // (never stopping early)
    };
    assert_eq!(calls, 5); // the whole hand, then Strike, Defend, Bash, Shrug It Off: not ten
    assert!([0, 1, 2].iter().all(|i| costs[i] == costs[&0])); // (copies cost the same)
    assert!([3, 4, 5, 6, 7].iter().all(|i| costs[i] == costs[&3]));
}

#[test]
fn test_planning_stops_at_a_card_the_turn_does_not_miss() {
    let state = big_hand();
    let (costs, calls) = {
        let (_guard, count) = spy();
        (handpick::turn_costs(&state, cards_of(&state), 1e-9).unwrap(), count.get())
    };
    assert_eq!(calls, 2); // the whole hand and the hand without the first card: nothing lost
    assert_eq!(costs[&0], 0.0);
    assert_eq!(costs[&9], handpick::UNPLANNED); // (the rest keep their order, after it)
}

#[test]
fn test_a_card_that_weighs_on_the_hand_is_always_planned_for() {
    let mut state = big_hand();
    state["hand_select"]["cards"].as_array_mut().unwrap().push(card(10, "BURN", "Burn", "Status", 0, "Unplayable. At the end of your turn, take 2 damage.", "None"));
    let costs = handpick::turn_costs(&state, cards_of(&state), 1e-9).unwrap();
    assert_ne!(costs[&10], handpick::UNPLANNED); // (a Status card may be a burn: it is never skipped over)
}

#[test]
fn test_the_same_selection_is_not_planned_again() {
    let mut state = big_hand();
    let first = handpick::turn_costs(&state, cards_of(&state), 1e-9);
    let (again, after_click) = {
        let _planned = combat::with_choose_card_hook(|_, _| panic!("planned again"));
        let again = handpick::turn_costs(&state, cards_of(&state), 1e-9);
        state["hand_select"]["selected"] = json!([{"index": 3}]); // (a click made on the screen changes nothing the plan reads)
        (again, handpick::turn_costs(&state, cards_of(&state), 1e-9))
    };
    assert_eq!(first, again);
    assert_eq!(first, after_click);
    state["battle"]["enemies"][0]["hp"] = json!(5); // (another fight is another question)
    let (_guard, count) = spy();
    handpick::turn_costs(&state, cards_of(&state), 1e-9);
    assert!(count.get() > 0);
}

// RewardBar

#[test]
fn test_acts_two_and_three_grow_the_bar_half_as_fast() {
    // Growth from a 12-card deck to a 26-card one: 3 in act 1, 1.5 in acts 2 and 3.
    approx(strategy::reward_bar(26, Some(1)) - strategy::reward_bar(12, Some(1)), 3.0);
    approx(strategy::reward_bar(26, Some(2)) - strategy::reward_bar(12, Some(2)), 1.5);
    approx(strategy::reward_bar(26, Some(3)) - strategy::reward_bar(12, Some(3)), 1.5);
    approx(strategy::reward_bar(22, Some(1)) - strategy::reward_bar(22, Some(2)), 1.0);
}

#[test]
fn test_small_decks_have_the_same_bar_in_every_act() {
    assert_eq!(strategy::reward_bar(10, Some(1)), strategy::reward_bar(10, Some(3)));
    assert_eq!(strategy::reward_bar(10, None), strategy::reward_bar(10, Some(1)));
}

// Crowd: other players' runs as a second opinion on a card (crowd.rs).

fn SOW() -> Value {
    card(0, "SOW", "Sow", "Attack", 1, "Retain. Deal 8 damage to ALL enemies.", "AllEnemies")
}
fn DEFY() -> Value {
    card(1, "DEFY", "Defy", "Skill", 1, "Gain 6 Block. Apply 1 Weak.", "AnyEnemy")
}

#[test]
fn test_every_card_in_the_tables_gets_a_rating() {
    assert_eq!(crowd::RATING.len(), 5 * 80 + 33);
    assert!(crowd::RATING.values().all(|v| (1.5..=8.0).contains(v)));
    assert!(crowd::rating("AFTERIMAGE").unwrap() > crowd::rating("DAGGER_SPRAY").unwrap() + 3.0);
    // A card known by its name only is spelt differently from its id.
    assert_eq!(crowd::rating("TIME_S_UP"), crowd::rating("TIMES_UP"));
    assert!(crowd::rating("STRIKE_NECROBINDER").is_none());
}

#[test]
fn test_the_crowd_moves_a_rating_only_as_far_as_the_setting_says() {
    let ours = {
        let _k = learn::with_knobs(&[("crowd", 0.0)]);
        let ours = deck_value(&SOW());
        assert!(ours > deck_value(&DEFY())); // our own table: the attack first
        assert_eq!(deck_value(&with(SOW(), json!({"is_upgraded": true}))), ours + 0.7);
        ours
    };
    let _k = learn::with_knobs(&[("crowd", 0.6)]);
    assert!(deck_value(&SOW()) < deck_value(&DEFY())); // a quarter of the runs with Sow were won
    assert!(deck_value(&SOW()) < ours);
    // A card our table does not list is read from its text, and that is mixed the same way.
    let demesne = with(card(2, "DEMESNE", "Demesne", "Power", 3, "Ethereal. At the start of your turn, gain [necrobinder_energy_icon.png] and draw 1 additional card.", "None"), json!({"rarity": "Rare"}));
    assert!(deck_value(&demesne) > 5.0);
    // ... but not for a card that is only good in hands that can do what the bot cannot.
    let stoke = with(card(3, "STOKE", "Stoke", "Skill", 1, "Exhaust your Hand. Add 1 random card into your Hand for each card Exhausted.", "None"), json!({"rarity": "Rare"}));
    assert_eq!(deck_value(&stoke), 2.0);
}

#[test]
fn test_a_relic_can_limit_the_cards_played_in_a_turn() {
    let mut state = combat_state!((0..3).map(STRIKE).collect::<Vec<_>>(), [enemy!("JAW_WORM_0", 40)]);
    assert!(combat::cards_left_this_turn(&state).is_none());
    state["player"]["relics"] = json!([{"id": "VELVET_CHOKER", "description": "Gain [ironclad_energy_icon.png] at the start of each turn. You cannot play more than 6 cards per turn."}]);
    assert_eq!(combat::cards_left_this_turn(&with(state.clone(), json!({"_plays_this_turn": 4}))), Some(2));
    assert!(combat::choose_card(&with(state.clone(), json!({"_plays_this_turn": 6})), None).is_none());
    assert!(combat::choose_card(&with(state, json!({"_plays_this_turn": 5})), None).is_some());
}

#[test]
fn test_snecko_eye_is_bought_like_any_relic() {
    let state = json!({"state_type": "shop", "run": {"act": 1, "floor": 5}, "player": {"hp": 80, "max_hp": 80, "gold": 250, "potions": []},
                       "shop": {"items": [with(item(0, "relic", 150), json!({"relic_id": "SNECKO_EYE"}))], "can_proceed": true}});
    assert_eq!(strategy::shop(&state, &mut Memory::new(), "shop")[0], act("shop_purchase", json!({"index": 0})));
}

#[test]
fn test_the_burning_elite_is_kept_in_reach_while_healthy() {
    // Five hallway fights lead to the key, a rest and two chests the other way: on paper the first road dies (the values said
    // so from full HP in live runs too).
    let mut nodes: Vec<Value> = (1..6).map(|row| node(0, row, "Monster", &[(0, row + 1)])).collect();
    nodes.extend([node(0, 6, "EmeraldElite", &[]), node(1, 1, "RestSite", &[(1, 2)]), node(1, 2, "Treasure", &[(1, 3)]), node(1, 3, "Treasure", &[])]);
    let opts = [opt(0, 0, 1, "Monster"), opt(1, 1, 1, "RestSite")];
    let way = |hp: i64| first(&strategy::map_screen(&base("map", json!({"hp": hp, "max_hp": 80, "act": 2, "map": {"next_options": opts, "nodes": nodes}})), &mut Memory::new())).clone();
    assert_eq!((way(80), way(36)), (json!({"index": 0}), json!({"index": 1}))); // (under EMERALD_HOLD the values decide)
    // The burning elite next and a rest site the other way: from EMERALD_GO it is the elite (a run on 68% rested, met no other,
    // and won the act 3 boss without the key).
    let mut near: Vec<Value> = (2..6).map(|row| node(0, row, "Monster", &[(0, row + 1)])).collect();
    near.extend([node(0, 1, "EmeraldElite", &[(0, 2)]), node(1, 1, "RestSite", &[(1, 2)]), node(1, 2, "Treasure", &[(1, 3)]), node(1, 3, "Treasure", &[])]);
    let offers = [opt(0, 0, 1, "EmeraldElite"), opt(1, 1, 1, "RestSite")];
    let next = |hp: i64| first(&strategy::map_screen(&base("map", json!({"hp": hp, "max_hp": 80, "act": 1, "map": {"next_options": offers, "nodes": near}})), &mut Memory::new())).clone();
    assert_eq!((next(54), next(44)), (json!({"index": 0}), json!({"index": 1})));
    assert_eq!({ let _was = strategy::EMERALD_GO.set(0.8); next(54) }, json!({"index": 1})); // (where the values had it)
    let _off = strategy::EMERALD_HOLD.set(0.0);
    assert_eq!(way(80), json!({"index": 1}));
}

#[test]
fn test_for_the_key_a_rest_comes_before_the_burning_elite_and_recall_before_a_needless_rest() {
    let rest = |hp: i64, child: &str| {
        let mut mem = Memory::new();
        mem.map_nodes = [((0, 1), node(0, 1, "RestSite", &[(0, 2)])), ((0, 2), node(0, 2, child, &[]))].into_iter().collect();
        mem.map_at = Some((0, 1));
        let state = base("rest_site", json!({"hp": hp, "max_hp": 80, "act": 2, "rest_site": {"options": [{"index": 0, "id": "rest"}, {"index": 1, "id": "smith"}, {"index": 2, "id": "recall"}]}}));
        first(&strategy::rest_site(&state, &mut mem)).clone()
    };
    assert_eq!(rest(68, "EmeraldElite"), json!({"index": 0})); // 85%: heal for the elite
    assert_eq!(rest(68, "Monster"), json!({"index": 2})); // else the Ruby Key
}
