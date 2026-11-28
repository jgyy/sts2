//! The first game's act 4: the Spire Shield and Spear (who is behind us, and turning to face a target), the Heart's Beat of
//! Death and Invincible in the planner's words, and the potions kept for the Heart.

use std::rc::Rc;

use serde_json::{json, Value};
use sts2bot::cards::parse;
use sts2bot::fight::{battle, combat, triggers};
use sts2bot::games::sts1::screens::Screens;
use sts2bot::base::JsonExt;
use sts2bot::run::potions;

use super::helpers::sts1_case;
use crate::fake_game::with;
use crate::helpers::{combat_state, enemy, DEFEND, STRIKE};

/// An intent: (kind, damage as shown, hits).
type Move = (&'static str, i64, i64);

/// The Shield and Spear as CommunicationMod sends them with the mod's sides: the Shield on our left, the Spear on our right.
/// Each intent is (kind, damage as shown, hits); the one behind us shows its 50% more.
/// (the original's defaults: shield ("ATTACK", 51, 1), spear ("BUFF", -1, 1), shield_behind True.)
fn surrounded_with(shield: Move, spear: Move, shield_behind: bool) -> Screens {
    let mut s = Screens::new();
    let monster = |mid: &str, hp: i64, left: bool, r#move: Move| {
        json!({"id": mid, "name": mid, "current_hp": hp, "max_hp": hp, "block": 0, "powers": [], "intent": r#move.0,
               "move_adjusted_damage": r#move.1, "move_hits": r#move.2, "left": left, "behind": left == shield_behind})
    };
    s.raw = Rc::new(json!({"in_game": true, "available_commands": ["play", "end"], "game_state": {
        "act": 4, "floor": 53, "room_type": "MonsterRoomElite", "screen_type": "NONE", "room_phase": "COMBAT", "current_hp": 70, "max_hp": 80,
        "combat_state": {"turn": 3, "player": {"current_hp": 70, "max_hp": 80, "block": 0, "energy": 3, "powers": [{"id": "Surrounded", "amount": -1}]},
                         "hand": [], "draw_pile": [], "discard_pile": [], "exhaust_pile": [],
                         "monsters": [monster("SpireShield", 110, true, shield), monster("SpireSpear", 160, false, spear)]}}}));
    s
}

fn surrounded() -> Screens {
    surrounded_with(("ATTACK", 51, 1), ("BUFF", -1, 1), true)
}

// class ShieldAndSpear

#[test]
fn test_each_side_is_marked_and_who_is_behind_comes_from_the_game() {
    let _case = sts1_case();
    let state = surrounded().combat();
    let (shield, spear) = (&state["battle"]["enemies"][0], &state["battle"]["enemies"][1]);
    assert_eq!(state["battle"]["enemies"].list().len(), 2);
    assert!(shield["status"].list().iter().any(|p| p["id"] == "BACK_ATTACK_LEFT_POWER"));
    assert!(spear["status"].list().iter().any(|p| p["id"] == "BACK_ATTACK_RIGHT_POWER"));
    let read = battle::read_enemies(&with(state.clone(), json!({"_facing": "SPIRESHIELD_0"}))); // (what we last aimed at does not overrule the game)
    let of = |eid: &str| read.iter().find(|e| &*e.entity_id == eid).unwrap();
    assert_eq!((of("SPIRESHIELD_0").flanker, of("SPIRESHIELD_0").behind, of("SPIRESPEAR_1").behind), (true, true, false));
}

#[test]
fn test_we_turn_to_face_the_one_about_to_hit_hardest() {
    let _case = sts1_case();
    // The Shield's Smash is 34, 51 from behind, while the Spear buffs: a Strike aimed at the Shield turns us to face it. The other
    // way round (the Shield fortifies, the Spear's Skewer is 10x3 in front) the Strike goes at the Spear, though the Shield has
    // less HP: turning to the Shield would put the Skewer behind us (15x3).
    for (shield, spear, target) in [(("ATTACK", 51, 1), ("BUFF", -1, 1), "SPIRESHIELD_0"), (("DEFEND", -1, 1), ("ATTACK", 10, 3), "SPIRESPEAR_1")] {
        let mut state = surrounded_with(shield, spear, true).combat();
        state["player"]["hand"] = json!([STRIKE(0)]);
        let (action, params) = combat::choose_card(&state, None).unwrap();
        assert_eq!((action.as_str(), params["target"].as_str()), ("play_card", Some(target)), "{target}");
    }
}

#[test]
fn test_the_act_4_powers_read_as_the_planner_knows_them() {
    let _case = sts1_case();
    let read = Screens::powers(&json!({"powers": [{"id": "Invincible", "amount": 300}, {"id": "BeatOfDeath", "amount": 1}, {"id": "BackAttack", "amount": -1}]}));
    assert_eq!(read.iter().map(|p| &p["id"]).collect::<Vec<_>>(), ["HARDENED_SHELL_POWER", "BEAT_OF_DEATH_POWER", "BACK_ATTACK_POWER"]);
}

/// (the original's defaults: invincible 300, beat 1, attack None.)
fn heart(invincible: i64, beat: i64) -> Value {
    let mut status = vec![json!({"id": "HARDENED_SHELL_POWER", "amount": invincible})];
    if beat != 0 {
        status.push(json!({"id": "BEAT_OF_DEATH_POWER", "amount": beat}));
    }
    enemy!("CORRUPTHEART_0", 750, attack = Value::Null, status = status)
}

// class TheHeart

#[test]
fn test_every_card_played_costs_its_beat_once_block_is_gone() {
    let _case = sts1_case();
    let state = combat_state!([STRIKE(0), DEFEND(1)], [heart(300, 2)], kind = "boss");
    let (ctx, foes) = (combat::_context(&state), battle::read_enemies(&state));
    let (strike, defend) = (parse(&state["player"]["hand"][0]), parse(&state["player"]["hand"][1]));
    assert_eq!(combat::simulate(&[&strike], &foes, &ctx).unwrap().lost, 2);
    assert_eq!(combat::simulate(&[&defend, &strike], &foes, &ctx).unwrap().lost, 0); // (5 Block takes both beats)
    assert_eq!(combat::simulate(&[&strike, &defend], &foes, &ctx).unwrap().lost, 2); // (the first beat comes before any Block)
}

#[test]
fn test_no_card_is_played_into_a_heart_that_can_lose_no_more_this_turn() {
    let _case = sts1_case();
    // Invincible spent (300 lost this turn): a Strike takes nothing off it and its beat takes 1 HP off us.
    assert!(combat::choose_card(&combat_state!([STRIKE(0)], [heart(0, 1)], kind = "boss"), None).is_none());
    assert!(combat::choose_card(&combat_state!([STRIKE(0)], [heart(300, 1)], kind = "boss"), None).is_some());
}

#[test]
fn test_damage_beyond_what_it_can_still_lose_counts_for_nothing() {
    let _case = sts1_case();
    let state = combat_state!([STRIKE(0), STRIKE(1)], [heart(6, 0)], kind = "boss");
    let (ctx, foes) = (combat::_context(&state), battle::read_enemies(&state));
    let one = combat::simulate(&[parse(&STRIKE(0))], &foes, &ctx).unwrap();
    let two = combat::simulate(&[parse(&STRIKE(0)), parse(&STRIKE(1))], &foes, &ctx).unwrap();
    assert!((one.score - two.score).abs() < 5e-8, "{} != {}", one.score, two.score);
}

#[test]
fn test_the_way_through_act_4_keeps_the_belt_for_the_heart() {
    let _case = sts1_case();
    assert_eq!([50, 51, 52, 53, 54].map(|f| potions::saving(&json!(f))), [false, true, true, true, false]);
}

// class SnakePlant

#[test]
fn test_each_hit_that_hurts_it_buys_it_block_for_the_next_card() {
    let _case = sts1_case();
    // Malleable 3 on 15 HP: three Strikes deal 6, then 6 - 3, then 6 - 4 (11, not 18), so the Chomp still comes.
    let plant = enemy!("SNAKEPLANT_0", 15, attack = "7x3", status = json!([{"id": "MALLEABLE_POWER", "amount": 3}]));
    let plain = enemy!("SNAKEPLANT_0", 15, attack = "7x3");
    let strikes = [STRIKE(0), STRIKE(1), STRIKE(2)];
    let line: Vec<_> = strikes.iter().map(parse).collect();
    for (foe, lost, malleable) in [(plain, 0, false), (plant, 21, true)] {
        let state = combat_state!(strikes.clone(), [foe]);
        assert_eq!(combat::simulate(&line, &battle::read_enemies(&state), combat::_context(&state)).unwrap().lost, lost, "malleable={malleable}");
    }
}

#[test]
fn test_what_the_transient_loses_comes_off_its_blow() {
    let _case = sts1_case();
    // Shifting: three Strikes take 18 off a blow of 40 (13 off 30 through its Weak); its 999 HP are no reason to hit it.
    let powers = |more: Value| json!([{"id": "FADING_POWER", "amount": 5}, {"id": "SHIFTING_POWER", "amount": -1}, more]);
    let strikes = [STRIKE(0), STRIKE(1), STRIKE(2)];
    let line: Vec<_> = strikes.iter().map(parse).collect();
    for (foe, lost) in [(enemy!("TRANSIENT_0", 999, attack = "40", status = powers(json!({}))), 22), (enemy!("TRANSIENT_0", 999, attack = "30", status = powers(json!({"id": "WEAK_POWER", "amount": 1}))), 18)] {
        let state = combat_state!(strikes.clone(), [foe]);
        assert_eq!(combat::simulate(&line, &battle::read_enemies(&state), combat::_context(&state)).unwrap().lost, lost);
    }
    // 2026-11-28: 7 of 15 fights lost. A Strike (6 off the blow) before a Defend (5); with the blow covered, neither.
    let foe = |blow: &str| enemy!("TRANSIENT_0", 999, attack = blow, status = powers(json!({})));
    let state = combat_state!([DEFEND(0), STRIKE(1)], [foe("40")], energy = 1);
    assert_eq!(combat::choose_card(&state, None).unwrap().1["card_index"], json!(1));
    let state = combat_state!([DEFEND(0), STRIKE(1)], [foe("4")], energy = 2, block = 4);
    assert!(combat::choose_card(&state, None).is_none());
}

// class BlueCandle

#[test]
fn test_necronomicurse_is_never_played_and_other_curses_cost_their_hp() {
    let _case = sts1_case();
    // 2026-11-25 20:47: under Blue Candle it came back to the hand at every play, 1 HP each, energy unchanged: a loop.
    let curse = Screens::new().card(&json!({"id": "Necronomicurse", "name": "Necronomicurse", "type": "CURSE", "cost": 0, "is_playable": true}), Some(1), true);
    assert!(curse["description"].s().contains("Lose 1 HP"));
    let state = combat_state!([DEFEND(0), curse.clone()], [enemy!("BYRD_0", 25, attack = "2x5")], energy = 3);
    let action = combat::choose_card(&state, None);
    assert_ne!(action.as_ref().and_then(|a| a.1.get("card_index")), Some(&json!(1)));
    assert!(triggers::shunned(&parse(&curse), &state)); // (whatever it would seem to offer)
    let pain = Screens::new().card(&json!({"id": "Pain", "name": "Pain", "type": "CURSE", "cost": 0, "is_playable": true}), Some(0), true);
    assert_eq!(parse(&pain).hp_loss, 1);
}

#[test]
fn test_under_runic_dome_an_enemy_is_taken_to_hit_its_usual_blow() {
    // 2026-11-28: with the intents hidden the planner saw no attack and never blocked (those runs ended on floor 12 on average).
    let _case = sts1_case();
    let state = |relics: Value| {
        let mut s = surrounded_with(("NONE", -1, 1), ("NONE", -1, 1), true);
        let mut raw = (*s.raw).clone();
        raw["game_state"]["relics"] = relics;
        s.raw = Rc::new(raw);
        s.combat()["battle"]["enemies"][1]["intents"].clone()
    };
    assert_eq!(state(json!([{"id": "Runic Dome"}])), json!([{"type": "Attack", "label": "19"}]));
    assert_eq!(state(json!([]))[0]["type"], "Unknown");
}

// class SelfVulnerable

/// The reborn Awakened One's 44 on a 48 HP Ironclad with 5 Block (2026-11-28 14:06): Berserk's Vulnerable makes it 66.
#[test]
fn test_berserk_is_not_played_into_a_blow_it_makes_lethal() {
    let _case = sts1_case();
    let mut s = Screens::new();
    let hand = [("Berserk", "POWER", 0, false), ("Cleave", "ATTACK", 1, false)];
    s.raw = Rc::new(json!({"in_game": true, "available_commands": ["play", "end"], "game_state": {
        "act": 3, "floor": 50, "room_type": "MonsterRoomBoss", "screen_type": "NONE", "room_phase": "COMBAT", "current_hp": 48, "max_hp": 95,
        "combat_state": {"turn": 8, "player": {"current_hp": 48, "max_hp": 95, "block": 5, "energy": 4, "powers": []},
                         "hand": hand.iter().map(|(id, kind, cost, aimed)| json!({"id": id, "name": id, "type": kind, "cost": cost, "has_target": aimed})).collect::<Vec<_>>(),
                         "draw_pile": [], "discard_pile": [], "exhaust_pile": [],
                         "monsters": [{"id": "AwakenedOne", "name": "Awakened One", "current_hp": 300, "max_hp": 300, "block": 0,
                                       "powers": [{"id": "Regenerate", "amount": 10}, {"id": "Strength", "amount": 4}],
                                       "intent": "ATTACK", "move_adjusted_damage": 44, "move_hits": 1}]}}}));
    let state = s.combat();
    let first = |on: bool| {
        let _rule = battle::SELF_VULN.set(on);
        let mut state = state.clone();
        let mut names = Vec::new();
        while let Some((action, params)) = combat::choose_card(&state, None) {
            if action != "play_card" {
                break;
            }
            let i = params["card_index"].as_i64().unwrap() as usize;
            names.push(state["player"]["hand"][i]["name"].text());
            state["player"]["hand"].as_array_mut().unwrap().remove(i);
            for (n, c) in state["player"]["hand"].as_array_mut().unwrap().iter_mut().enumerate() {
                c["index"] = json!(n);
            }
        }
        names
    };
    assert!(!first(true).iter().any(|n| n == "Berserk"), "{:?}", first(true));
    assert!(first(false).iter().any(|n| n == "Berserk"), "{:?}", first(false)); // (as it played before the rule)
}

/// A Defect with three Lightning orbs (Focus 1) and four Frail Defends against a Spheric Guardian's 12x2 behind 10 Block
/// (2026-11-28 13:27:20): the planner played nothing, expecting to lose 0, and lost 24. Barricade keeps Block; it is Burrowed
/// that is Stunned when its Block is broken.
#[test]
fn test_lightning_orbs_do_not_hide_a_blow() {
    let _case = sts1_case();
    let mut s = Screens::new();
    let defend = json!({"id": "Defend_B", "name": "Defend", "type": "SKILL", "cost": 1, "has_target": false});
    let lightning = json!({"id": "Lightning", "name": "Lightning", "passive_amount": 4, "evoke_amount": 9});
    s.raw = Rc::new(json!({"in_game": true, "available_commands": ["play", "end"], "game_state": {
        "act": 2, "floor": 18, "room_type": "MonsterRoom", "screen_type": "NONE", "room_phase": "COMBAT", "current_hp": 66, "max_hp": 75,
        "combat_state": {"turn": 3, "player": {"current_hp": 66, "max_hp": 75, "block": 0, "energy": 6, "orbs": [lightning, lightning, lightning],
                                               "powers": [{"id": "Focus", "amount": 1}, {"id": "Frail", "amount": 5}]},
                         "hand": [defend, defend, defend, defend], "draw_pile": [], "discard_pile": [], "exhaust_pile": [],
                         "monsters": [{"id": "SphericGuardian", "name": "Spheric Guardian", "current_hp": 20, "max_hp": 20, "block": 10,
                                       "powers": [{"id": "Barricade", "amount": -1}, {"id": "Strength", "amount": 1}],
                                       "intent": "ATTACK", "move_adjusted_damage": 12, "move_hits": 2}]}}}));
    let state = s.combat();
    let got = |on: bool| {
        let _rule = battle::BARRICADE_HOLDS.set(on);
        combat::choose_card(&state, None).map(|(action, _)| action)
    };
    assert_eq!((got(true).as_deref(), got(false)), (Some("play_card"), None)); // (off: as it played)
}
