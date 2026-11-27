//! The Fool's cards have no run statistics, only a writer's tiers (crowd.FOOL_RATING): they stand in for the crowd's part
//! of a rating, so that Aim and Melody cards the parser cannot read are not rated below the Shots a Fool holds one Arrow for.

use std::collections::BTreeSet;

use indexmap::IndexMap;
use serde_json::{json, Value};
use sts2bot::cards;
use sts2bot::learn;
use sts2bot::run::{crowd, decks};

use crate::fake_game::{card, with};

type Row = (&'static str, &'static str, &'static str, i64, &'static str, &'static str);
const QUICK_SHOT: Row = ("QUICK_SHOT", "Quick Shot", "Attack", 0, "Shoot an Arrow to deal 10 damage.", "Common");

/// `card(index, key, name, ctype, cost, text, target, rarity)`.
fn card_(index: i64, key: &str, name: &str, ctype: &str, cost: i64, text: &str, target: &str, rarity: &str) -> Value {
    with(card(index, key, name, ctype, cost, text, target), json!({"rarity": rarity}))
}

fn rated((key, name, ctype, cost, text, rarity): Row) -> f64 {
    cards::deck_value(&card_(0, key, name, ctype, cost, text, "Self", rarity))
}

// FoolTiers

#[test]
fn test_every_card_of_the_list_is_rated_and_the_tiers_are_what_was_set() {
    assert_eq!(crowd::FOOL_RATING.len(), 79);
    let mut tiers: Vec<f64> = Vec::new();
    for v in crowd::FOOL_RATING.values() {
        if !tiers.contains(v) {
            tiers.push(*v);
        }
    }
    tiers.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(tiers, [3.3, 4.5, 5.8, 7.2]);
    assert_eq!((crowd::rating("WAH_CHO_KI"), crowd::rating("WAH_HAMMER"), crowd::rating("HEAVY_SHOT"), crowd::rating("QUICK_SHOT")), (Some(7.2), Some(5.8), Some(4.5), Some(3.3)));
    assert_eq!(crowd::rating("YAH"), Some(5.8)); // "Yah!" is spelt YAH as an id
}

#[test]
fn test_the_tables_of_the_other_characters_are_left_alone() {
    assert_eq!(crowd::RATING.len(), 5 * 80 + 33);
    let fool: BTreeSet<&String> = crowd::FOOL_RATING.keys().collect();
    assert!(crowd::RATING.keys().all(|k| !fool.contains(k)));
    assert!(crowd::rating("NO_SUCH_CARD").is_none());
}

#[test]
fn test_an_aim_card_the_parser_cannot_read_beats_a_shot_that_it_can() {
    let (ready, quick) = {
        let _k = learn::with_knobs(&[("crowd", 0.6)]);
        (rated(("CHO_READY", "Cho Ready", "Skill", 1, "Gain 8 Block. Gain 3 Aim.", "Uncommon")), rated(QUICK_SHOT))
    };
    assert!(ready > quick); // 3.2 against 3.5 before the tiers, a Fool's top pick in 129 of 360 offers
}

#[test]
fn test_a_setting_of_zero_leaves_the_text_rating_alone() {
    let _k = learn::with_knobs(&[("crowd", 0.0)]);
    let with_tiers = rated(QUICK_SHOT);
    let _none = crowd::with_fool_rating(IndexMap::new());
    assert_eq!(with_tiers, rated(QUICK_SHOT));
}

fn SORROW() -> Value {
    card_(0, "SORROWFUL_STRIKE", "Sorrowful Strike", "Attack", 1, "Deal 13 damage. Add 2 Sadness into your Hand.", "AnyEnemy", "Common")
}
fn ENNUI() -> Value {
    card_(0, "ENNUI", "Ennui", "Power", 1, "At the start of your turn, add 1 Sadness to your Hand and draw 2 additional cards.", "Self", "Rare")
}
fn PUNCH() -> Value {
    card_(0, "PUNCH_OF_JOY", "Punch of Joy", "Attack", 2, "Deal 13 damage. Transform 1 random Sadness in your Hand into Joy.", "AnyEnemy", "Uncommon")
}
fn PLAIN(more: &[Value]) -> Vec<Value> {
    let mut deck = vec![card_(0, "STRIKE_FOOL", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy", "Basic"); 10];
    deck.extend(more.iter().cloned());
    deck
}

// SadnessInTheDeck: Sadness stays in the hand all fight: a deck takes its makers only as far as the hands have room for them, or Joy to turn them into.

#[test]
fn test_makers_lose_rating_as_the_sadness_piles_up() {
    let fit = |c: &Value, deck: &[Value]| decks::deck_fit(c, deck, Some(2), 3);
    let (first, second) = (fit(&SORROW(), &PLAIN(&[])), fit(&SORROW(), &PLAIN(&[SORROW()])));
    assert!(second < first - 1.5); // (the copy's 0.6, and a Sadness over the room)
    assert!(fit(&ENNUI(), &PLAIN(&[SORROW()])) < fit(&ENNUI(), &PLAIN(&[])));
    assert!(fit(&ENNUI(), &PLAIN(&[SORROW(), PUNCH(), PUNCH()])) > fit(&ENNUI(), &PLAIN(&[SORROW()]))); // (Joy takes them away)
    assert!(fit(&PUNCH(), &PLAIN(&[SORROW(), SORROW()])) > fit(&PUNCH(), &PLAIN(&[])));
}
