//! Cards a potion offers mid-fight ("Choose 1 of 3 ... It's free to play this turn") are chosen for the turn they come in, by the planner, not by what each would be worth in the deck.

use serde_json::{json, Value};
use sts2bot::fight::afflictions;

use super::combat_state;
use crate::helpers::{enemy_kw, STRIKE};

// As the choose screen lists them: no target, no "can_play".
fn GUARD() -> Value {
    json!({"index": 0, "id": "ULTIMATE_DEFEND", "name": "Ultimate Defend", "type": "Skill", "cost": "1", "rarity": "Uncommon", "description": "Gain 11 Block."})
}

fn PUNCH() -> Value {
    json!({"index": 1, "id": "SUCKER_PUNCH", "name": "Sucker Punch", "type": "Attack", "cost": "1", "rarity": "Common", "description": "Deal 8 damage. Apply 1 Weak."})
}

fn BLADE() -> Value {
    json!({"index": 2, "id": "BLADE_RUSH", "name": "Blade Rush", "type": "Attack", "cost": "2", "rarity": "Rare", "description": "Deal 14 damage."})
}

/// (The Python's defaults: hp 60, foe_hp 300.)
fn fight(attack: Option<i64>, hp: i64, foe_hp: i64) -> Value {
    combat_state!([STRIKE(0)], [enemy_kw("A", foe_hp, json!({"attack": attack}))], energy = 1, hp = hp, kind = "boss")
}

// Offers (setUp: `afflictions._OFFER[:] = [None, 0]`; each test has a thread, and so an `_OFFER`, of its own)

#[test]
fn test_the_card_taken_is_the_one_for_this_turn() {
    afflictions::_OFFER_reset();
    for (why, offer, state, taken) in [
        ("no Block card on a turn nothing attacks (the Knowledge Demon's first turn is a Debuff)", [GUARD(), PUNCH()], fight(None, 60, 300), "Sucker Punch"),
        ("the Block card when the hit would kill: 20 on 12 HP is 15 and dead behind Sucker Punch's Weak, 9 behind Ultimate Defend", [PUNCH(), GUARD()], fight(Some(20), 12, 300), "Ultimate Defend"),
        ("the offer is free this turn: Blade Rush and the Strike are 20, the kill on 15 HP (Sucker Punch and Strike 14)", [PUNCH(), BLADE()], fight(Some(10), 60, 15), "Blade Rush"),
    ] {
        afflictions::_OFFER_reset(); // (each one a screen of its own)
        assert_eq!(afflictions::offered(&offer, &state)[0]["name"], taken, "{why}");
    }
}

#[test]
fn test_afflictions_and_screens_outside_a_fight_are_left_alone() {
    afflictions::_OFFER_reset();
    let sloth = json!({"index": 0, "name": "Sloth", "type": "Status", "description": "You cannot play more than 3 cards each turn."});
    let burn = json!({"index": 1, "name": "Disintegration", "type": "Status", "description": "At the end of your turn, take 7 damage."});
    assert_eq!(afflictions::offered(&[sloth.clone(), burn.clone()], &fight(Some(10), 60, 300)), vec![sloth, burn]);
    assert_eq!(afflictions::offered(&[GUARD(), PUNCH()], &Value::Null), vec![GUARD(), PUNCH()]);
}

// TODO(bot): test_the_bot_hands_the_screen_the_fight_it_came_in needs make_bot (src/bot.rs) and bot.candidates; its spy on
// `afflictions.offered` is `afflictions::with_offered_hook(|ordered, fight| { ...; afflictions::_offered(ordered, fight) })`.
