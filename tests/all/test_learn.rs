//! The card record: what a pick is measured against, and the kinds a card starts from.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{json, Value};
use sts2bot::cards::kind;
use sts2bot::games;
use sts2bot::learn;
use sts2bot::run::strategy;

use crate::fake_game::card as _card;

const E: &str = "[necrobinder_energy_icon.png]";

fn card(name: &str, ctype: &str, cost: i64, text: &str) -> Value {
    _card(0, &name.to_uppercase().replace(' ', "_"), name, ctype, cost, text, "None")
}

/// A record of Necrobinder runs that end on floor 25, with these cards' [picks, floors gained] and kinds.
fn necro(cards: &[(&str, [f64; 2])], kinds: &[(&str, &str)]) -> learn::CardStats {
    let mut stats = learn::CardStats::new(None);
    stats.finals.insert("The Necrobinder".into(), vec![25; 20]);
    stats.cards.insert("The Necrobinder".into(), cards.iter().map(|(k, v)| (k.to_string(), *v)).collect());
    for (k, v) in kinds {
        stats.kinds.insert(k.to_string(), v.to_string());
    }
    stats
}

// Kinds

#[test]
fn test_a_card_is_free_gives_energy_or_only_brings_cards() {
    assert_eq!(kind(&card("Poke", "Attack", 0, "Osty deals 6 damage.")), "free");
    assert_eq!(kind(&card("Wisp", "Skill", 0, &format!("Gain {E}. Exhaust."))), "energy");
    assert_eq!(kind(&card("Invoke", "Skill", 1, &format!("Next turn, Summon 2 and gain {E}{E}."))), "energy");
    assert_eq!(kind(&card("Parse", "Skill", 1, "Ethereal. Draw 3 cards.")), "draw");
    assert_eq!(kind(&card("Capture Spirit", "Skill", 1, "Enemy loses 3 HP. Add 3 Souls into your Draw Pile.")), "draw");
    assert_eq!(kind(&card("Dredge", "Skill", 1, "Put 3 cards from your Discard Pile into your Hand. Exhaust.")), "draw");
}

#[test]
fn test_most_cards_are_no_kind() {
    assert_eq!(kind(&card("Strike", "Attack", 1, "Deal 6 damage.")), "");
    assert_eq!(kind(&card("Scourge", "Skill", 1, "Apply 13 Doom. Draw 1 card.")), ""); // it does its own work as well
    assert_eq!(kind(&card("Grave Warden", "Skill", 1, "Gain 8 Block. Add a Soul into your Draw Pile.")), "");
    assert_eq!(kind(&card("Shared Fate", "Skill", 0, "Lose 2 Strength. Enemy loses 2 Strength. Exhaust.")), ""); // a free Skill
    assert_eq!(kind(&card("Friendship", "Power", 1, &format!("Lose 2 Strength. Gain {E} at the start of each turn."))), "");
    assert_eq!(kind(&card("Graveblast", "Attack", 1, "Deal 4 damage. Put a card from your Discard Pile into your Hand. Exhaust.")), "");
}

// Record

#[test]
fn test_a_boss_reward_is_not_credited_with_beating_the_boss() {
    let mut stats = learn::CardStats::new(None);
    for floor in [vec![17; 6], vec![30; 6]].concat() {
        // half the runs end at the act 1 boss
        stats.record("The Regent", floor, false, &[]);
    }
    // Cards from the boss's reward and the next floor, run ended on 30: no further than the runs that left floor 17 alive get.
    stats.record("The Regent", 30, false, &[(17, "Big Bang".to_string()), (18, "Glow".to_string())]);
    assert_eq!(stats.cards["The Regent"]["BIG_BANG"], [1.0, 0.0]);
    assert_eq!(stats.cards["The Regent"]["GLOW"], [1.0, 0.0]);
}

#[test]
fn test_a_card_starts_where_its_kind_stands() {
    // Free attacks have run 4 floors a pick over the other picks, cards that only draw 4 under.
    let mut stats = necro(&[("POKE", [200.0, 800.0]), ("PARSE", [200.0, -800.0]), ("DEFILE", [400.0, 0.0])], &[("POKE", "free"), ("PARSE", "draw"), ("DEFILE", "")]);
    let plain = stats.shift("The Necrobinder", "Reave", Some(""));
    assert!(stats.shift("The Necrobinder", "Fetch", Some("free")) > plain + 0.5);
    assert!(stats.shift("The Necrobinder", "Dredge", Some("draw")) < plain - 0.5);
    // ... for every character (the kinds are pooled), and never further than MAX_KIND.
    stats.finals.insert("The Defect".into(), vec![25; 20]);
    assert!(stats.shift("The Defect", "Claw", Some("free")) > stats.shift("The Defect", "Zap", Some("")) + 0.5);
    assert!(stats.kind_shift("free", "") <= learn::MAX_KIND);
}

#[test]
fn test_the_free_lean_setting_takes_the_start_off_free_attacks_only() {
    // Par is zero here, so a free attack's kind is worth its whole record: 800 floors over 200 picks.
    let mut stats = necro(&[("POKE", [200.0, 800.0]), ("DEFILE", [200.0, -800.0])], &[("POKE", "free"), ("DEFILE", "draw")]);

    let mut shift = |lean: f64, name: &str, its_kind: &str| -> f64 {
        let _k = learn::with_knobs(&[("free_lean", lean)]);
        stats.shift("The Necrobinder", name, Some(its_kind))
    };

    let (on, off) = (shift(1.0, "Fetch", "free"), shift(0.0, "Fetch", "free"));
    assert!(on > off + 1.0); // about 1.5 floors of kind start, less at 0.0
    assert_eq!(shift(1.0, "Dredge", "draw"), shift(0.0, "Dredge", "draw")); // a draw card starts from its kind at either setting
    assert!(learn::CHOICES.contains_key("free_lean"));
    assert_eq!(learn::DEFAULTS["free_lean"], 1.0); // the arm that is today's behaviour
}

#[test]
fn test_a_cards_own_record_outweighs_its_kind_in_the_end() {
    let mut stats = necro(&[("POKE", [300.0, 900.0]), ("DEFILE", [1500.0, 2700.0]), ("RIGHT_HAND_HAND", [1200.0, -3600.0])], &[("POKE", "free"), ("RIGHT_HAND_HAND", "free"), ("DEFILE", "")]);
    // Its kind is 3 floors over par without it; its own 1,200 picks were 3 under.
    assert!(stats.kind_shift("free", "RIGHT_HAND_HAND") > 1.0);
    assert!(stats.shift("The Necrobinder", "Right Hand Hand", Some("free")) < -0.5);
}

#[test]
fn test_the_kinds_are_kept_with_the_record() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("card_stats.json");
    let mut stats = learn::CardStats::new(Some(path.clone()));
    for _ in 0..6 {
        stats.record("The Necrobinder", 20, false, &[]);
    }
    stats.shift("The Necrobinder", "Poke+", Some("free"));
    stats.record("The Necrobinder", 30, false, &[(3, "Poke+".to_string())]);
    let mut again = learn::CardStats::new(Some(path));
    assert_eq!(again.kinds, IndexMap::from([("POKE".to_string(), "free".to_string())]));
    assert_eq!(again.shift("The Necrobinder", "Poke", None), stats.shift("The Necrobinder", "Poke", None));
}

// AtTheMerchant

#[test]
fn test_a_card_on_the_shelf_is_rated_with_its_record_too() {
    let poke = json!({"index": 0, "category": "card", "price": 40, "is_stocked": true, "can_afford": true, "card_id": "POKE", "card_name": "Poke",
                      "card_type": "Attack", "card_cost": "0", "card_rarity": "Common", "card_description": "Osty deals 6 damage."});
    let player = json!({"character": "The Necrobinder", "hp": 60, "max_hp": 66, "gold": 60, "relics": [], "potions": []});
    let state = json!({"state_type": "shop", "run": {"act": 2, "floor": 20}, "player": player, "shop": {"items": [poke], "can_proceed": true}});
    assert_eq!(strategy::shop(&state, &mut strategy::Memory::new(), "shop")[0].0, "proceed"); // rated 3.6 by the tables: under the bar
    let stats = necro(&[("POKE", [800.0, 2400.0]), ("DEFILE", [800.0, -2400.0])], &[]); // ... and 3 floors a pick over par in our runs
    let mut mem = strategy::Memory { card_stats: Some(Rc::new(RefCell::new(stats))), ..strategy::Memory::new() };
    assert_eq!(strategy::shop(&state, &mut mem, "shop")[0], ("shop_purchase".to_string(), json!({"index": 0})));
}

// ModelTable: learn.model_shift: what the model runs say of a card (`sim prior`), weighed by the `model_lean` setting.

#[test]
fn test_the_table_counts_only_as_far_as_the_setting_says() {
    let table = json!({"sts2": {"SILENT": {"FOOTWORK": 1.5, "DEFLECT": -2.0}}, "sts1": {"SILENT": {"FOOTWORK": -1.0}}});
    {
        let _m = learn::with_model(Some(table));
        {
            let _k = learn::with_knobs(&[("model_lean", 0.0)]);
            assert_eq!(learn::model_shift("The Silent", "Footwork"), 0.0); // off: today's behaviour
        }
        {
            let _k = learn::with_knobs(&[("model_lean", 1.0)]);
            assert_eq!(learn::model_shift("The Silent", "Footwork+"), 1.5); // (upgraded or not, by its id)
            assert_eq!(learn::model_shift("THE_SILENT", "Deflect"), -2.0); // (the menu's spelling of her)
            assert_eq!(learn::model_shift("The Silent", "Neutralize"), 0.0); // a card the table leaves out
            assert_eq!(learn::model_shift("The Defect", "Footwork"), 0.0); // ... and a character
            let _g = games::playing("sts1");
            assert_eq!(learn::model_shift("The Silent", "Footwork"), -1.0); // each game its own table
        }
        let _k = learn::with_knobs(&[("model_lean", 2.0)]);
        assert_eq!(learn::model_shift("The Silent", "Deflect"), -4.0); // (twice as far: Slay the Spire 2's default)
    }
    assert_eq!(learn::DEFAULTS["model_lean"], 0.0);
}

#[test]
fn test_the_shipped_table_reads() {
    let table = learn::load_model_cards();
    assert!(!table.is_empty(), "run `sim prior` to make src/run/model_cards.json");
    for (game, chars) in &table {
        assert!(["sts1", "sts2"].contains(&game.as_str()));
        for (who, cards) in chars.as_object().unwrap() {
            assert_eq!(*who, learn::who_key(who));
            assert!(cards.as_object().unwrap().values().all(|v| v.as_f64().unwrap().abs() <= 3.0), "{who}");
        }
    }
}

// GameDefaults

#[test]
fn test_each_game_plays_its_own_best_settings_without_tuning() {
    let _d = learn::with_game_defaults(&[("sts2", &[("crowd", 0.3)]), ("sts1", &[("crowd", 0.6)])]);
    assert_eq!(learn::defaults(Some("sts2"))["crowd"], 0.3);
    assert_eq!(learn::defaults(Some("sts1"))["crowd"], 0.6);
    {
        let _g = games::playing("sts1");
        assert_eq!(learn::defaults(None), learn::defaults(Some("sts1")));
    }
    assert_eq!(learn::defaults(Some("sts2"))["caution"], learn::DEFAULTS["caution"]); // (the rest as DEFAULTS)
}

#[test]
fn test_every_game_default_is_a_setting() {
    for (game, table) in learn::GAME_DEFAULTS.iter() {
        assert!(table.keys().all(|k| learn::DEFAULTS.contains_key(k)), "{game}");
    }
}
