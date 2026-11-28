use indexmap::IndexMap;
use serde_json::{json, Value};
use sts2bot::cards::{parse, CardInfo};
use sts2bot::fight::combat::{self, Plan, SimulateArgs};
use sts2bot::fight::triggers;
use sts2bot::base::JsonExt;
use sts2bot::run::strategy::{self, Memory};
use sts2bot::Action;

use crate::fake_game::{card, with};
use crate::fight::{aimed, combat_state, ctx, gain, outcome, pick, pick_plan, play_turn, potion, power, read_foes, worth};
use crate::helpers::{enemy, DEFEND, STRIKE};

/// (the original's default: cost 1.)
fn ESCAPE(i: i64, cost: i64) -> Value {
    card(i, "FRANTIC_ESCAPE", "Frantic Escape", "Status", cost, "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.", "None")
}

/// (the original's default: target "Self".)
fn BECKON(i: i64, target: &str) -> Value {
    card(i, "BECKON", "Beckon", "Status", 1, "At the end of your turn, if this is in your Hand,  lose 6 HP.", target)
}

fn play_card(params: Value) -> Option<Action> {
    Some(("play_card".to_string(), params))
}

fn indexes(cards: &[Value]) -> Vec<i64> {
    cards.iter().map(|c| c["index"].int()).collect()
}

fn sorted<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort();
    v
}

/// `assertAlmostEqual(a, b)` (seven places; `places` otherwise).
fn almost(a: f64, b: f64, places: i32) -> bool {
    sts2bot::base::round_to(a - b, places) == 0.0
}

/// `combat.simulate(line, foes, **kw).score`.
fn score(line: &[CardInfo], foes: &[combat::Enemy], kw: &SimulateArgs) -> f64 {
    combat::simulate(line, foes, kw).unwrap().score
}

// BossMechanics: Things the bosses do that the planner got wrong (found in the fight logs of 2026-11-18).

#[test]
fn test_a_death_blow_is_an_attack() {
    let blow = json!({"type": "DeathBlow", "label": "39"});
    assert_eq!((combat::intent_damage(&blow), combat::intent_hits(&blow)), (39, 1));
}

#[test]
fn test_the_giants_last_blow_is_blocked_and_the_dead_giant_left_alone() {
    // Killed, the Waterfall Giant stands on 999,999,999 HP until it blows up.
    let giant = with(enemy!("WATERFALL_GIANT_0", 999999999, status = [power("STEAM_ERUPTION", 39)]), json!({"intents": [{"type": "DeathBlow", "label": "39"}]}));
    let big = card(0, "KICK", "Kingly Kick", "Attack", 0, "Deal 35 damage.", "AnyEnemy");
    let reflect = card(1, "REFLECT", "Reflect", "Skill", 1, "Gain 15 Block.", "Self");
    let mut state = combat_state!([big, reflect, DEFEND(2), DEFEND(3)], [giant], energy = 3, hp = 36, max_hp = 75, kind = "boss");
    assert_eq!(sorted(indexes(&play_turn(&mut state, 3, true, None, None))), [1, 2, 3]); // every Block card, and not one hit on the giant
    assert_eq!(combat::choose_card(&state, None), None); // the free attack is not worth playing either
}

#[test]
fn test_beckon_is_seen_despite_the_double_space() {
    // The mod turns the line break in Beckon's text into a second space.
    let state = combat_state!([BECKON(0, "Self"), DEFEND(1)], [enemy!("FYSH", 200, attack = 4)], energy = 1, kind = "boss");
    assert_eq!(combat::hand_burns(&state["player"]), vec![(0, 6, false)]);
    assert_eq!(combat::choose_card(&state, None), play_card(json!({"card_index": 0}))); // 6 saved beats 4 blocked
}

#[test]
fn test_a_turn_only_the_fortifier_saves_is_played_for_the_fortifier() {
    // The Soul Fysh on 6 HP, us on 4 with 36 coming, a Beckon in hand and a Fortifier on the belt: three Block cards (22, tripled 66) left the Beckon's 6 to kill; the Beckon and two of them are 15, tripled 45.
    let guard = |i: i64, n: i64| card(i, &format!("GUARD_{i}"), &format!("Guard {n}"), "Skill", 1, &format!("Gain {n} Block."), "Self");
    let turn = |belt: Vec<Value>, kind: &str| -> (Vec<String>, Action) {
        let mut state = combat_state!([guard(0, 7), guard(1, 7), BECKON(2, "Self"), guard(3, 8)], [enemy!("SOUL_FYSH", 6, attack = 36)], energy = 3, hp = 4, max_hp = 82, kind = kind, potions = belt);
        let played = play_turn(&mut state, 9, true, None, Some(&|s: &Value| combat::actions(s).into_iter().next()));
        (sorted(played.iter().map(|c| c["name"].s().to_string()).collect()), combat::actions(&state).remove(0))
    };
    // The Beckon and two Block cards (14 or 15: either holds once tripled), then the potion on the Block that is up.
    let fortifier = potion(0, "Fortifier", "Triple your Block.", "Self");
    let (played, then) = turn(vec![fortifier.clone()], "boss");
    assert_eq!((played[0].as_str(), played.len(), then.clone()), ("Beckon", 3, ("use_potion".to_string(), json!({"slot": 0}))));
    assert_eq!(turn(vec![fortifier], "monster"), (played, then)); // (in an ordinary fight too: it is the danger that counts)
    assert_eq!(turn(vec![], "boss").1, ("end_turn".to_string(), json!({}))); // (without it nothing saves the turn, whatever is played)
}

#[test]
fn test_block_stops_cards_that_deal_damage_but_not_ones_that_take_hp() {
    let wither = card(0, "WITHER", "Wither+2", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 9 damage.", "None");
    let foe = read_foes(&[enemy!("A", 300, attack = 10)]);
    let behind = |c: &Value, block: i64| score(&[], &foe, &ctx!(energy = 0, hp = 50, block = block, fight = "boss", burns = combat::hand_burns(&json!({"hand": [c]}))));
    assert_eq!(behind(&wither, 19), behind(&wither, 25)); // 19 Block takes the Wither and the attack
    assert!(behind(&wither, 10) < behind(&wither, 19));
    assert_eq!(behind(&BECKON(0, "None"), 10), behind(&BECKON(0, "None"), 25)); // the 6 HP go whatever the Block
}

#[test]
fn test_a_wither_costs_three_for_every_level() {
    let named = |name: &str| json!({"hand": [{"name": name, "description": "At the end of your turn, if this is in your Hand, take 3 damage."}]});
    assert_eq!(combat::hand_burns(&named("Wither"))[0].1, 3);
    assert_eq!(combat::hand_burns(&named("Wither+2"))[0].1, 9);
    assert_eq!(combat::_next_wither(&json!({"discard_pile": [{"name": "Wither+1"}]}), 1), 6);
    assert_eq!(combat::_next_wither(&json!({}), 8), 9); // two of Aeonglass's upgrades have gone by
}

#[test]
fn test_the_card_that_brings_the_next_wither_is_not_held_back() {
    // Aeonglass's count on 1 and 25 coming: the turn ended on three energy with a Block card in hand, as the Wither it would bring cost more this turn than the Block saved. But the count carries over, and the Wither came all the same.
    let count = |n: i64| json!([power("WITHERING_PRESENCE", n)]);
    let state = combat_state!(
        [DEFEND(0)],
        [enemy!("AEONGLASS", 410, attack = 25, status = count(1))],
        energy = 3,
        hp = 62,
        max_hp = 109,
        block = 4,
        kind = "boss",
        round_no = 4,
        draw_pile_count = 20,
        discard_pile_count = 9
    ); // a deck of thirty
    assert_eq!(combat::choose_card(&state, None), play_card(json!({"card_index": 0})));
    // A card worth less than the Withers it brings nearer still stays in the hand, on any count.
    let poke = card(0, "POKE", "Poke", "Attack", 0, "Deal 3 damage.", "AnyEnemy");
    for n in [1, 4] {
        assert_eq!(combat::choose_card(&combat_state!([poke.clone()], [enemy!("AEONGLASS", 400, status = count(n))], kind = "boss"), None), None);
    }
}

#[test]
fn test_a_wither_made_late_in_the_fight_has_less_time_to_come_round() {
    let deck = json!({"hand": [{}, {}, {}, {}, {}], "draw_pile_count": 15, "discard_pile_count": 10}); // thirty cards: six turns to come round
    let (early, late) = (combat::_wither_bill(&deck, 2, 3), combat::_wither_bill(&deck, 9, 9));
    assert!(almost(early, 5.25, 7)); // (what a card cost on rounds 1-3 before: a 3-damage Poke is still not worth it)
    assert!(late < early / 2.0); // two rounds left of eleven: it will hardly be drawn again
    assert!(combat::_wither_bill(&deck, 14, 12) > 0.0); // a fight that runs long still pays something
}

#[test]
fn test_dead_on_paper_it_plays_the_block_it_has() {
    // 29 coming, 22 HP, one Defend+: nothing saves it for sure, but the Block comes closest.
    let defrag = card(0, "DEFRAGMENT", "Defragment", "Power", 1, "Gain 1 Focus.", "Self");
    let guard = card(1, "DEFEND", "Defend+", "Skill", 1, "Gain 8 Block.", "Self");
    let turn = |hand: Vec<Value>, energy: i64| combat::choose_card(&combat_state!(hand, [enemy!("A", 300, attack = 40)], energy = energy, hp = 22, max_hp = 90, kind = "boss"), None);
    assert_eq!(turn(vec![defrag.clone(), guard.clone()], 1), play_card(json!({"card_index": 1})));
    // With energy for both, the cards are drawn first: one of them may be the Block that is missing.
    assert_eq!(turn(vec![defrag, guard, card(2, "SKIM", "Skim", "Skill", 1, "Draw 3 cards.", "Self")], 2), play_card(json!({"card_index": 2})));
}

#[test]
fn test_an_escape_is_played_before_the_margin_is_gone() {
    // Sandpit 3 and one Escape in hand: ending on 2 needs an Escape in the very next hand.
    let boss = enemy!("INS", 200, attack = 0, status = [power("SANDPIT", 3)]);
    let pile = || {
        let mut pile = vec![json!({"name": "Strike"}); 20];
        pile.extend(vec![json!({"name": "Frantic Escape"}); 2]);
        pile
    };
    let mut state = combat_state!([ESCAPE(0, 1), STRIKE(1), STRIKE(2), STRIKE(3)], [boss.clone()], energy = 3, kind = "boss", draw_pile = pile());
    assert!(indexes(&play_turn(&mut state, 3, false, None, None)).contains(&0));
    // ... even at 2 energy, the whole of what is left.
    let state = combat_state!([ESCAPE(0, 2), STRIKE(1), STRIKE(2)], [boss], energy = 2, kind = "boss", draw_pile = pile());
    assert_eq!(combat::choose_card(&state, None), play_card(json!({"card_index": 0})));
}

#[test]
fn test_escape_odds_follow_the_piles() {
    let strikes = |n: usize| vec![json!({"name": "Strike"}); n];
    let escapes = |n: usize| vec![json!({"name": "Frantic Escape"}); n];
    let none_left = json!({"draw_pile": strikes(10), "discard_pile": escapes(6), "hand": []});
    assert_eq!(combat::_escape_odds(&none_left, 5).0, 1.0); // the next five cards hold none for certain
    assert_eq!(combat::_escape_odds(&json!({"draw_pile": escapes(5), "discard_pile": [], "hand": []}), 5).0, 0.0);
    let mixed = json!({"draw_pile": ([strikes(24), escapes(6)].concat()), "discard_pile": [], "hand": []});
    assert!(almost(combat::_escape_odds(&mixed, 5).0, 0.298, 2));
}

#[test]
fn test_beating_a_form_of_the_test_subject_is_not_winning_the_fight() {
    let (kw, kill) = (ctx!(energy = 2, fight = "boss"), vec![parse(&STRIKE(0))]);
    let foes = |status: Value| read_foes(&[enemy!("TEST_SUBJECT_0", 5, attack = 15, status = status)]);
    assert!(score(&kill, &foes(json!([])), &kw) > combat::WIN_BONUS.get());
    let beaten = score(&kill, &foes(json!([power("ADAPTABLE", 1)])), &kw);
    assert!(beaten < combat::WIN_BONUS.get() / 2.0);
    assert!(beaten > score(&[], &foes(json!([power("ADAPTABLE", 1)])), &kw) + 40.0); // ... but it is worth doing: its attack this turn does not happen
    assert!(almost(score(&[kill.clone(), vec![parse(&STRIKE(1))]].concat(), &foes(json!([power("ADAPTABLE", 1)])), &kw), beaten, 7)); // once it is gone nothing more can be aimed at it this turn
}

#[test]
fn test_set_up_cards_are_played_while_the_enemy_is_away() {
    let r#loop = card(0, "LOOP", "Loop", "Power", 1, "At the start of your turn, trigger the passive of your rightmost Orb.", "Self");
    let zap = card(1, "ZAP", "Zap", "Skill", 1, "Channel 1 Lightning.", "Self");
    let mut state = combat_state!([r#loop, zap.clone(), STRIKE(2), DEFEND(3)], Vec::<Value>::new(), energy = 2, kind = "boss");
    assert_eq!(combat::choose_card(&state, None), None); // a fight that is simply over
    state["_revive_pending"] = json!(true);
    assert_eq!(combat::choose_card(&state, None), play_card(json!({"card_index": 0})));
    state["player"]["hand"] = json!([zap, STRIKE(2), DEFEND(3)]);
    state["player"]["energy"] = json!(1);
    assert_eq!(combat::choose_card(&state, None), play_card(json!({"card_index": 1})));
    state["player"]["hand"] = json!([STRIKE(2), DEFEND(3)]);
    assert_eq!(combat::choose_card(&state, None), None); // no use hitting or blocking thin air
}

#[test]
fn test_an_x_cost_summon_is_set_up_too() {
    let dirge = card(0, "DIRGE", "Dirge", "Skill", "X", "Summon 3 X times. Add X Souls into your Draw Pile. Exhaust.", "Self");
    assert!(parse(&dirge).is_x());
    let mut state = combat_state!([dirge, DEFEND(1)], Vec::<Value>::new(), energy = 2, kind = "boss", _revive_pending = true);
    assert_eq!(combat::choose_card(&state, None), play_card(json!({"card_index": 0})));
    state["player"]["energy"] = json!(0);
    assert_eq!(combat::choose_card(&state, None), None);
}

#[test]
fn test_vital_spark_taints_by_its_own_amount() {
    let state = combat_state!([DEFEND(0)], [enemy!("PRISM", 60, attack = "5x3", status = [power("VITAL_SPARK", 4)])], energy = 1, hp = 18);
    assert_eq!(combat::_context(&state).taint, 4);
    assert_eq!(combat::choose_card(&state, None), None); // a 5-Block Skill that adds 4 to each of three hits lets more through, not less
}

#[test]
fn test_an_escape_is_never_the_card_given_up() {
    let hand = [ESCAPE(0, 1), with(card(1, "X", "Hyperbeam", "Attack", 2, "Deal 26 damage to ALL enemies.", "None"), json!({"rarity": "Rare"})), DEFEND(2)];
    let state = json!({"state_type": "hand_select", "hand_select": {"prompt": "Choose a card to Exhaust.", "cards": hand, "can_confirm": false}});
    assert_eq!(strategy::hand_select(&state, &mut Memory::new())[0], ("combat_select_card".to_string(), json!({"card_index": 2})));
    let order: Vec<i64> = strategy::hand_select(&state, &mut Memory::new()).iter().filter(|a| a.0 == "combat_select_card").map(|a| a.1["card_index"].as_i64().unwrap()).collect();
    assert_eq!(*order.last().unwrap(), 0); // the Escape goes last, after even the best card
}

// CardPlayTriggers: Powers that act on every card played are part of the turn's arithmetic.

/// (the original's default: energy 3.)
fn subject(hp: i64, hand: Vec<Value>, powers: Value, energy: i64) -> Value {
    let foe = json!({"entity_id": "TEST_SUBJECT_0", "hp": hp, "max_hp": 200, "block": 0, "status": [power("ADAPTABLE", 1), power("DOOM", 28)], "intents": [{"type": "Attack", "label": "10x5"}]});
    combat_state!(hand, [foe], energy = energy, hp = 10, max_hp = 60, kind = "boss", status = powers)
}

#[test]
fn test_sleight_of_flesh_finishes_what_the_attack_started() {
    let exterminate = card(0, "EXTERMINATE", "Exterminate+", "Attack", 1, "Deal 7 damage to ALL enemies 4 times.", "AllEnemies");
    let bringer = |i: i64| card(i, "DEATHBRINGER", "Deathbringer+", "Skill", 2, "Apply 26 Doom and 1 Weak to ALL enemies.", "AllEnemies");
    let defend = |i: i64| card(i, "DEFEND_N", "Defend+", "Skill", 1, "Gain 8 Block.", "Self");
    let sleight = || json!([power("SLEIGHT_OF_FLESH", 13)]);
    // 33 HP: the attack takes 28, the two debuffs 13 each. The form dies and its 50 damage never comes.
    assert!([0, 1].contains(&pick(&subject(33, vec![exterminate.clone(), bringer(1), defend(2)], sleight(), 3)))); // (either first: the two together are the kill)
    assert_eq!(combat::choose_card(&subject(5, vec![bringer(0), defend(1)], sleight(), 2), None), play_card(json!({"card_index": 0})));
    // Without the Power there is no kill in the hand, and no Block that saves us either (10 HP in front of 50): the attack that leaves the form on 5 HP is the best try.
    assert!([0, 1].contains(&pick(&subject(33, vec![exterminate, bringer(1), defend(2)], json!([]), 3)))); // (the Doom card goes first now; both are played)
}

#[test]
fn test_afterimage_and_strangle_count_for_every_card() {
    let s = combat_state!([DEFEND(0), DEFEND(1)], [enemy!("RAT_0", 5, attack = 9, status = [power("STRANGLE", 3)])], energy = 2, hp = 30, status = [power("AFTERIMAGE", 1)]);
    let t = triggers::read(&s).unwrap();
    assert_eq!((t.card_block, t.strangle), (1, IndexMap::from([("RAT_0".to_string(), 3)])));
    assert_eq!(outcome(&s, s["player"]["hand"].list()).unwrap().lost, 0); // two Defends strangle it: 6 HP lost on 5
    assert!(triggers::read(&combat_state!([DEFEND(0)], [enemy!("A", 5)])).is_none());
}

// PersonalHive: The Entomancer: every Attack that hits it puts Dazed into our draw pile.

/// (the original's defaults: junk 0, energy 1.)
fn first(hive: i64, hand: Vec<Value>, junk: usize) -> Option<i64> {
    let dazed = card(9, "DAZED", "Dazed", "Status", 0, "Unplayable. Ethereal.", "None");
    let draw_pile = [vec![STRIKE(9); 12 - junk], vec![dazed; junk]].concat();
    let s = combat_state!(hand, [enemy!("ENTOMANCER_0", 140, status = [power("PERSONAL_HIVE", hive)])], energy = 1, hp = 60, kind = "elite", draw_pile = draw_pile, discard_pile = json!([]));
    combat::choose_card(&s, None).map(|act| act.1["card_index"].int())
}

#[test]
fn test_small_attacks_stop_as_the_hive_grows() {
    let big = card(1, "BLUDGEON", "Bludgeon", "Attack", 1, "Deal 32 damage.", "AnyEnemy");
    let doom = card(2, "NO_ESCAPE", "No Escape", "Skill", 1, "Apply 15 Doom.", "AnyEnemy");
    assert_eq!(first(1, vec![card(0, "DRAIN_POWER", "Drain Power", "Attack", 1, "Deal 12 damage.", "AnyEnemy")], 0), Some(0)); // one Dazed for 12 damage: worth it
    assert_eq!(first(1, vec![STRIKE(0)], 0), Some(0)); // ... and for 6: the hand has a card to spare for one Dazed
    assert_eq!(first(3, vec![STRIKE(0)], 0), None); // three of them for 6 is a bad trade
    assert_eq!(first(3, vec![STRIKE(0), big], 0), Some(1)); // a big one is not
    assert_eq!(first(3, vec![STRIKE(0), doom], 0), Some(2)); // and what is not an Attack costs nothing
    assert_eq!(first(3, vec![STRIKE(0)], 8), Some(0)); // (in a deck already full of them a draw is worth little)
    assert_eq!(read_foes(&[enemy!("E", 9, status = [power("PERSONAL_HIVE", 2)])])[0].hive, 2);
}

#[test]
fn test_the_hive_never_stops_every_attack() {
    // (Nothing was played with 4 energy, three attacks in hand and 4x7 coming, at hive 2.)
    let unleash = card(0, "UNLEASH", "Unleash", "Attack", 1, "Deal 10 damage.", "AnyEnemy");
    let storm = card(1, "SOUL_STORM", "Soul Storm", "Attack", 1, "Deal 9 damage.", "AnyEnemy");
    let hive = |hp: i64, n: i64| enemy!("ENTOMANCER_0", hp, attack = "4x7", status = [power("PERSONAL_HIVE", n)]);
    let mut s = combat_state!([unleash.clone(), storm.clone(), STRIKE(2)], [hive(88, 2)], energy = 4, hp = 65, max_hp = 76, kind = "elite", draw_pile = vec![STRIKE(9); 20], discard_pile = json!([]));
    assert_eq!(pick(&s), 0); // the biggest one goes
    s["player"]["hand"] = json!([STRIKE(2)]);
    assert_eq!(pick(&s), 2); // ... a Strike too, when it is the only one
    // After it, each further attack is two whole cards later on: only one that is worth them.
    let (foes, kw) = (combat::read_enemies(&s), ctx!(energy = 4, hp = 65, max_hp = 76, fight = "elite", deck = (21, 0)));
    assert!(score(&[parse(&unleash), parse(&storm)], &foes, &kw) < score(&[parse(&unleash)], &foes, &kw));
    // With the kill in the hand nothing is drawn afterwards: everything goes.
    let foe = hive(24, 3);
    let mut hit = |state: &mut Value, c: &Value, _: &Value| -> bool {
        let dealt = match c["name"].s() {
            "Unleash" => 10,
            "Soul Storm" => 9,
            "Strike" => 6,
            other => panic!("KeyError: {other}"),
        };
        let hp = &mut state["battle"]["enemies"][0]["hp"];
        *hp = json!(hp.int() - dealt);
        hp.int() <= 0
    };
    let mut state = combat_state!([unleash, storm, STRIKE(2)], [foe], energy = 4, hp = 65, max_hp = 76, kind = "elite");
    assert_eq!(play_turn(&mut state, 4, false, Some(&mut hit), None).len(), 3);
}

// WhatStopsDamage

#[test]
fn test_block_stops_constrict_too() {
    let mut state = combat_state!([DEFEND(0), DEFEND(1), STRIKE(2)], [enemy!("SLITHERING_STRANGLER_0", 40)], energy = 2, hp = 40, status = [power("CONSTRICT", 6)]);
    assert_eq!(combat::expected_end_loss(&state), Some(6)); // no attack coming: the squeeze alone
    // One Defend takes 5 of the 6; the Strike is worth more than the last point.
    assert_eq!(sorted(play_turn(&mut state, 3, true, None, None).iter().map(|c| c["name"].s().to_string()).collect()), ["Defend", "Strike"]);
    assert_eq!(combat::expected_end_loss(&state), Some(1));
}

#[test]
fn test_block_stops_the_burn_like_any_other_damage() {
    let mut state = combat_state!([DEFEND(0), DEFEND(1)], [enemy!("KNOWLEDGE_DEMON_0", 80, attack = 6)], energy = 2, hp = 40, kind = "boss", status = [power("DISINTEGRATION", 8)]);
    assert_eq!(combat::expected_end_loss(&state), Some(14)); // 8 and then 6, nothing in the way
    state["player"]["block"] = json!(10);
    assert_eq!(combat::expected_end_loss(&state), Some(4)); // seen live: 8 a turn, 0 HP lost behind 25 Block
}

#[test]
fn test_each_hit_stopped_is_a_wound_saved() {
    let guard = |i: i64| card(i, "DEFEND_B", "Defend+", "Skill", 1, "Gain 10 Block.", "Self");
    let score = |line: &[usize], stabs: bool| -> f64 {
        let foe = json!({"entity_id": "T", "hp": 190, "max_hp": 200, "block": 0, "intents": [{"type": "Attack", "label": "10x3"}], "status": if stabs { json!([power("PAINFUL_STABS", 1)]) } else { json!([]) }});
        let state = combat_state!([guard(0), guard(1)], [foe], energy = 2, hp = 80, kind = "boss");
        outcome(&state, &line.iter().map(|&i| state["player"]["hand"][i].clone()).collect::<Vec<_>>()).unwrap().score
    };
    let wound = score(&[], false) - score(&[], true); // three hits through: three Wounds
    assert!(wound > 0.0);
    assert!(almost(score(&[0, 1], false) - score(&[0, 1], true), wound / 3.0, 7)); // two hits stopped, one Wound
}

#[test]
fn test_buffer_played_now_stops_the_hit_that_would_kill() {
    let buffer = card(0, "BUFFER", "Buffer", "Power", 2, "Prevent the next time you would lose HP.", "Self");
    let guard = card(1, "GLACIER", "Glacier", "Skill", 2, "Gain 11 Block.", "Self");
    let state = combat_state!([buffer, guard], [enemy!("AMALGAM_0", 56, attack = 27)], energy = 2, hp = 6, max_hp = 73, kind = "boss");
    assert_eq!(combat::choose_card(&state, None), play_card(json!({"card_index": 0})));
}

// Necrobinder

fn POKE(i: i64) -> Value {
    card(i, "POKE", "Poke", "Attack", 0, "Osty deals 6 damage.", "AnyEnemy")
}

fn SHARDS(i: i64) -> Value {
    card(i, "BONE_SHARDS", "Bone Shards", "Attack", 1, "If Osty is alive, he deals 9 damage to ALL enemies and you gain 9 Block. Osty dies.", "AllEnemies")
}

fn PULSE(i: i64) -> Value {
    card(i, "NEGATIVE_PULSE", "Negative Pulse", "Skill", 1, "Gain 5 Block. Apply 7 Doom to ALL enemies.", "AllEnemies")
}

/// `Necrobinder.state(hand, osty, foe_hp=40, attack=12, doom=0, energy=3)`.
fn necro(hand: Vec<Value>, osty: i64, foe_hp: i64, attack: i64, doom: i64, energy: i64) -> Value {
    let foe = enemy!("SLUG_0", foe_hp, attack = attack, status = if doom != 0 { json!([power("DOOM", doom)]) } else { json!([]) });
    combat_state!(hand, [foe], energy = energy, hp = 40, pets = [json!({"name": "Osty", "hp": osty, "alive": osty > 0})])
}

#[test]
fn test_a_summon_goes_before_unleash() {
    let unleash = card(0, "UNLEASH", "Unleash", "Attack", 1, "Osty deals 7 damage. Deals additional damage equal to Osty's current HP.", "AnyEnemy");
    let guard = card(1, "BODYGUARD", "Bodyguard", "Skill", 1, "Summon 5.", "Self");
    let s = necro(vec![unleash.clone(), guard.clone()], 1, 40, 0, 0, 3);
    assert_eq!(pick(&s), 1);
    assert_eq!(outcome(&s, &[guard.clone(), unleash.clone()]).unwrap().score - outcome(&s, &[unleash.clone(), guard]).unwrap().score, 5.0); // 12 damage, not 7
    // After Bone Shards he is dead, whatever was Summoned before it.
    assert!(outcome(&s, &[SHARDS(2), unleash.clone()]).unwrap().score < outcome(&s, &[unleash, SHARDS(2)]).unwrap().score);
}

#[test]
fn test_times_up_hits_for_the_doom_on_the_enemy() {
    let times = card(0, "TIMES_UP", "Time's Up", "Attack", 2, "Deal damage equal to the enemy's Doom.", "AnyEnemy");
    let s = necro(vec![times.clone(), PULSE(1)], 1, 200, 0, 30, 3);
    assert_eq!(worth(&s, &[times.clone()]), 30.0);
    // ... and the Doom applied first counts: the pulse goes before it.
    assert_eq!(pick(&s), 1);
    assert!(outcome(&s, &[PULSE(1), times.clone()]).unwrap().score > outcome(&s, &[times, PULSE(1)]).unwrap().score + 6.0);
}

#[test]
fn test_no_escape_grows_with_the_doom_already_there() {
    let escape = card(0, "NO_ESCAPE", "No Escape", "Skill", 1, "Apply 10 Doom, plus an additional 5 Doom for every 10 Doom already on this enemy.", "AnyEnemy");
    let scourge = card(1, "SCOURGE", "Scourge", "Skill", 1, "Apply 13 Doom. Draw 1 card.", "AnyEnemy");
    let s = necro(vec![escape.clone(), scourge.clone()], 1, 300, 0, 67, 3);
    assert_eq!(worth(&s, &[escape.clone()]), 0.8 * (10 + 5 * 6) as f64); // 67 Doom: six tens
    // ... so it goes after the turn's other Doom: 80 Doom is eight tens.
    assert_eq!(worth(&s, &[scourge.clone(), escape.clone()]) - worth(&s, &[escape, scourge]), 0.8 * 10.0);
    assert_eq!(pick(&s), 1);
}

#[test]
fn test_deaths_door_blocks_three_times_after_doom() {
    let door = card(0, "DEATHS_DOOR", "Death's Door", "Skill", 1, "Gain 6 Block. If you applied Doom this turn, gain Block 2 additional times.", "Self");
    let s = necro(vec![door.clone(), PULSE(1)], 0, 200, 30, 0, 3);
    assert_eq!(outcome(&s, &[door.clone(), PULSE(1)]).unwrap().lost, 19); // 6 + 5
    assert_eq!(outcome(&s, &[PULSE(1), door]).unwrap().lost, 7); // 5 + 18
    assert_eq!(pick(&s), 1);
}

#[test]
fn test_sacrifice_takes_osty_with_it() {
    let sacrifice = card(0, "SACRIFICE", "Sacrifice", "Skill", 1, "Retain. If Osty is alive, he dies and you gain Block equal to triple his Max HP. (Gain 15 Block)", "Self");
    assert!(parse(&sacrifice).osty_dies);
    assert_eq!(outcome(&necro(vec![sacrifice.clone()], 5, 40, 24, 0, 3), &[sacrifice.clone()]).unwrap().lost, 9); // 15 Block against 24: his 5 HP are gone with him
    assert_eq!(outcome(&necro(vec![sacrifice.clone()], 0, 40, 24, 0, 3), &[sacrifice]).unwrap().lost, 24);
}

#[test]
fn test_under_necro_mastery_what_osty_takes_the_enemy_takes() {
    let mut s = necro(vec![DEFEND(0)], 10, 40, 10, 0, 1);
    let plain = outcome(&s, &[]).unwrap().score;
    s["player"]["status"] = json!([power("NECRO_MASTERY", 1)]);
    assert_eq!(outcome(&s, &[]).unwrap().score - plain, 10.0);
}

#[test]
fn test_hang_hits_twice_as_hard_each_time() {
    let hang = |i: i64| card(i, "HANG", "Hang", "Attack", 1, "Deal 10 damage. Double the damage ALL Hang cards deal to this enemy.", "AnyEnemy");
    let s = combat_state!([hang(0), hang(1)], [enemy!("SLUG_0", 100, status = [power("HANG", 4)])], energy = 2);
    assert_eq!(combat::read_enemies(&s)[0].hang, 4);
    assert_eq!(worth(&s, &[hang(0)]), 40.0); // 10 x 4
    assert!(worth(&s, &[hang(0), hang(1)]) > combat::WIN_BONUS.get()); // 40 + 80: the enemy is dead
    assert_eq!(gain(&[parse(&hang(0))], &read_foes(&[enemy!("SLUG_0", 100)]), &combat::_context(&s)), 10.0);
}

#[test]
fn test_block_that_keeps_osty_standing_is_worth_something() {
    let s = necro(vec![DEFEND(0)], 10, 40, 10, 0, 1);
    assert_eq!(combat::expected_end_loss(&s), Some(0)); // he takes it all
    assert_eq!(combat::choose_card(&s, None), play_card(json!({"card_index": 0}))); // ... and with 5 Block keeps 5 HP
    assert_eq!(combat::choose_card(&necro(vec![DEFEND(0)], 10, 40, 0, 0, 1), None), None); // nothing coming: nothing to protect him from
}

#[test]
fn test_an_osty_attack_goes_before_flatten_and_rattle() {
    let flatten = card(0, "FLATTEN", "Flatten", "Attack", 2, "Osty deals 12 damage. This card costs 0 [necrobinder_energy_icon.png] if Osty has attacked this turn.", "AnyEnemy");
    let rattle = card(0, "RATTLE", "Rattle", "Attack", 1, "Osty deals 7 damage. Hits an additional time for each other time he has attacked this turn. (Hits 1 times)", "AnyEnemy");
    // 2 energy: Flatten first leaves nothing for the Strike; after the Poke it is free. Rattle after the Poke hits twice.
    assert_eq!(pick(&necro(vec![flatten, POKE(1), STRIKE(2), STRIKE(3)], 3, 90, 0, 0, 2)), 1);
    assert_eq!(pick(&necro(vec![rattle, POKE(1)], 3, 90, 0, 0, 1)), 1);
}

#[test]
fn test_doom_on_ourselves_counts_as_hp_already_gone() {
    let surge = card(0, "NEUROSURGE", "Neurosurge", "Power", 0, "Gain 3 energy. Draw 2 cards. At the start of your turn, apply 3 Doom to yourself.", "Self");
    assert_eq!(pick(&combat_state!([surge.clone(), STRIKE(1)], [enemy!("A", 300, attack = 10)], hp = 60, kind = "boss")), 1); // a long fight ahead: not yet
    assert_eq!(combat::choose_card(&combat_state!([surge.clone()], [enemy!("A", 30, attack = 10)], hp = 60), None), play_card(json!({"card_index": 0})));
    // Round 8, the boss on 90 of 300 at 30 a round: three turns and one to spare is 12 Doom, on 60 HP.
    let mut late = combat_state!([surge.clone(), STRIKE(1)], [with(enemy!("A", 90, attack = 10), json!({"max_hp": 300}))], hp = 60, kind = "boss", round_no = 8);
    assert_eq!(pick(&late), 0);
    // Round 5 of the Insatiable on 67 of 76: six turns to go is 18 Doom, and the boss will have taken half our HP by then.
    assert_eq!(pick(&combat_state!([surge, STRIKE(1)], [with(enemy!("A", 175, attack = 20), json!({"max_hp": 321}))], hp = 67, max_hp = 76, kind = "boss", round_no = 5)), 1);
    late["player"]["hp"] = json!(25); // ... which is too much of 25
    assert_eq!(pick(&late), 1);
    late["player"]["hp"] = json!(60);
    late["player"]["status"] = json!([power("DOOM", 15), power("NEUROSURGE", 3)]); // a second one on top
    assert_eq!(pick(&late), 1);
    // 75 HP with 66 Doom and 3 more on the way is 6 HP as far as the plan goes.
    let mut hurt = combat_state!([STRIKE(0)], [enemy!("A", 300, attack = 10)], hp = 75, kind = "boss");
    assert_eq!(combat::_context(&hurt).hp, 75);
    hurt["player"]["status"] = json!([power("DOOM", 66), power("NEUROSURGE", 3)]);
    assert_eq!(combat::_context(&hurt).hp, 6);
}

#[test]
fn test_ostys_attacks_do_nothing_while_he_is_dead() {
    assert_eq!(combat::choose_card(&necro(vec![POKE(0)], 3, 40, 12, 0, 3), None), play_card(json!({"card_index": 0, "target": "SLUG_0"})));
    assert_eq!(combat::choose_card(&necro(vec![POKE(0)], 0, 40, 12, 0, 3), None), None);
    assert_eq!(pick(&necro(vec![SHARDS(0), POKE(1)], 1, 40, 12, 0, 3)), 1); // Bone Shards kills him: a Poke after it is wasted, so the Poke goes first
    let mut bare = necro(vec![POKE(0)], 0, 40, 12, 0, 3);
    bare["player"].as_object_mut().unwrap().shift_remove("pets"); // without the pet in the state nothing is assumed about him
    assert!(combat::choose_card(&bare, None).is_some());
}

#[test]
fn test_bone_shards_gives_up_the_hp_osty_stood_in_front_with() {
    let s = necro(vec![SHARDS(0)], 10, 40, 12, 0, 3);
    assert_eq!((outcome(&s, &[]).unwrap().lost, outcome(&s, s["player"]["hand"].list()).unwrap().lost), (2, 3)); // Osty soaks 10 of 12; after it, 9 Block instead of his 10 HP
}

#[test]
fn test_end_of_days_kills_the_doomed_at_once() {
    let days = card(0, "END_OF_DAYS", "End of Days", "Skill", 3, "Apply 29 Doom to ALL enemies. Kill enemies with at least as much Doom as HP.", "AllEnemies");
    let guard = card(1, "DEFEND_N", "Defend", "Skill", 1, "Gain 5 Block.", "Self");
    // 40 HP, 15 Doom already: 44 covers it. The enemy dies before its 30 damage, which three Defends would not stop.
    let s = necro(vec![days.clone(), guard.clone(), with(guard.clone(), json!({"index": 2})), with(guard, json!({"index": 3}))], 0, 40, 30, 15, 3);
    assert_eq!(combat::choose_card(&s, None), play_card(json!({"card_index": 0})));
    assert_eq!(combat::expected_end_loss(&necro(vec![days], 0, 40, 30, 15, 3)), Some(30));
}

// StrengthSapped

#[test]
fn test_piercing_wail_is_played_in_front_of_a_many_hit_attack() {
    let wail = card(0, "PIERCING_WAIL", "Piercing Wail", "Skill", 1, "ALL enemies lose 6 Strength this turn. Exhaust.", "AllEnemies");
    let foe = json!({"entity_id": "TEST_SUBJECT_0", "hp": 97, "max_hp": 200, "block": 0, "intents": [{"type": "Attack", "label": "10x6"}], "status": [power("PAINFUL_STABS", 1)]});
    let state = combat_state!([wail], [foe], energy = 1, hp = 33, max_hp = 128, block = 26, kind = "boss");
    assert_eq!(combat::expected_end_loss(&state), Some(34)); // dead as it stands
    assert_eq!(combat::choose_card(&state, None), play_card(json!({"card_index": 0}))); // six hits of 4: all behind the Block
}

#[test]
fn test_an_attack_on_all_that_saps_them_counts_the_sap() {
    let crush = card(0, "CRUSH_UNDER", "Crush Under", "Attack", 1, "Deal 8 damage to ALL enemies. All enemies lose 1 Strength this turn.", "AllEnemies");
    let state = combat_state!([crush.clone()], [enemy!("A", 50, attack = "3x4"), enemy!("B", 50, attack = 10)], energy = 1, hp = 40);
    assert_eq!(outcome(&state, &[crush]).unwrap().lost, 2 * 4 + 9);
}

#[test]
fn test_x_amounts_are_read_per_energy() {
    let malaise = card(0, "MALAISE", "Malaise", "Skill", "X", "Enemy loses X Strength. Apply X Weak. Exhaust.", "AnyEnemy");
    let info = parse(&malaise);
    assert_eq!((info.is_x(), info.enemy_strength_loss, info.weak, info.known), (true, 1, 1, true));
    // Three energy into a 5x6 attack: 2x6, and a quarter off that for Weak.
    assert_eq!(combat::choose_card(&combat_state!([malaise], [enemy!("A", 80, attack = "5x6")], energy = 3, hp = 30), None), play_card(json!({"card_index": 0, "target": "A"})));
}

// DeadOnPaper: Aeonglass on 51 HP about to hit for 40, the bot on 26 behind 11 Block with a Wither in hand.

/// `DeadOnPaper.state(boss_hp, hail=True)`.
fn aeonglass(boss_hp: i64, hail: bool) -> Value {
    let wither = card(2, "WITHER", "Wither+4", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 15 damage.", "Self");
    let hand = [
        card(0, "COOLHEADED", "Coolheaded", "Skill", 1, "Channel 1 Frost. Draw 1 card.", "Self"),
        card(1, "MOMENTUM_STRIKE", "Momentum Strike+", "Attack", 0, "Deal 19 damage.", "AnyEnemy"),
        with(wither, json!({"can_play": false, "unplayable_reason": "HasUnplayableKeyword"})),
        card(3, "DEFEND_B", "Defend", "Skill", 1, "Gain 8 Block.", "Self"),
        card(4, "FOCUSED_STRIKE", "Focused Strike+", "Attack", 1, "Deal 15 damage.", "AnyEnemy"),
    ];
    let orbs: Vec<Value> = [("FROST", 2), ("GLASS", 4), ("LIGHTNING", 3)].iter().map(|(k, p)| json!({"id": format!("{k}_ORB"), "passive_val": p, "evoke_val": 8})).collect();
    let boss = enemy!("AEONGLASS_0", boss_hp, attack = 40, status = [power("WITHERING_PRESENCE", 1)]);
    combat_state!(
        hand,
        [boss],
        energy = 3,
        hp = 26,
        max_hp = 89,
        block = 11,
        kind = "boss",
        round_no = 13,
        orbs = orbs,
        orb_slots = 3,
        status = if hail { json!([power("HAILSTORM", 12)]) } else { json!([]) }
    )
}

#[test]
fn test_hailstorm_and_the_orbs_finish_the_boss_before_it_hits() {
    // 19 + 15 from the cards, 12 from Hailstorm, 4 and 3 from the orbs: 53 on a boss with 51.
    let mut plan = Plan::default();
    assert!(combat::choose_card(&aeonglass(51, true), Some(&mut plan)).is_some());
    assert!(plan.lost.unwrap() < 26);
}

#[test]
fn test_far_past_saving_the_bot_still_swings() {
    // Every card brings the next Wither, so playing nothing was "least dead": the turn ended with two attacks paid for.
    let mut state = aeonglass(200, false);
    state["player"]["hp"] = json!(20); // 24 short with nothing played: no Block in this hand mends that
    state["player"]["hand"] = json!(state["player"]["hand"].list()[1..]); // no card that draws
    assert!([1, 4].contains(&pick(&state)));
}

// LostOnPaper: When every line ends in our death, Block that leaves us short is worth nothing: the kill is.

/// `LostOnPaper.state(hp)`.
fn queen(hp: i64) -> Value {
    let swing = |i: i64| card(i, "SWING", "Swing", "Attack", 1, "Deal 12 damage.", "AnyEnemy");
    let guard = card(2, "GUARD", "Guard", "Skill", 1, "Gain 8 Block.", "Self");
    combat_state!([swing(0), swing(1), guard], [enemy!("QUEEN_0", 30, attack = 35)], energy = 2, hp = hp, max_hp = 80, kind = "boss")
}

#[test]
fn test_the_bot_goes_for_the_kill_it_cannot_quite_count() {
    // 15 HP in front of 35: one attack and the Block lose 27, both attacks lose 35 and leave her on 6. Dead either way by the count; only the second can turn out a kill.
    let mut state = queen(15);
    assert_eq!(pick(&state), 0);
    let rest: Vec<Value> = state["player"]["hand"].list()[1..].iter().enumerate().map(|(n, c)| with(c.clone(), json!({"index": n}))).collect();
    state["player"]["hand"] = json!(rest);
    state["player"]["energy"] = json!(1);
    state["battle"]["enemies"][0]["hp"] = json!(18);
    assert_eq!(pick(&state), 0); // the second attack, not the Block
}

#[test]
fn test_block_that_saves_us_is_still_played() {
    let mut plan = Plan::default();
    combat::choose_card(&queen(30), Some(&mut plan)); // 30 HP: an attack and the Block lose 27
    assert_eq!(plan.lost, Some(27));
}

#[test]
fn test_reach_is_what_has_to_die_for_the_blow_not_to_land() {
    let foes = read_foes(&[enemy!("QUEEN_0", 300), enemy!("AMALGAM_0", 12, attack = 40, block = 3, status = [power("MINION", 1), power("POISON", 5)])]);
    assert_eq!(combat::kill_reach(&foes, true), 10.0); // the minion that attacks: 12 less 5 Poison, and 3 Block
    assert_eq!(combat::kill_reach(&foes, false), 300.0); // something else kills us too: only the fight's end helps
    assert_eq!(combat::kill_reach(&read_foes(&[enemy!("FYSH_0", 9, attack = 20, status = [power("INTANGIBLE", 1)])]), true), f64::INFINITY);
}

// SmallReads

#[test]
fn test_debilitate_doubles_vulnerable_for_the_cards_after_it() {
    let debilitate = card(0, "DEBILITATE", "Debilitate", "Attack", 1, "Deal 10 damage. Vulnerable and Weak are twice as effective against the enemy for the next 2 turns.", "AnyEnemy");
    let slam = card(1, "SLAM", "Slam", "Attack", 1, "Deal 13 damage.", "AnyEnemy");
    // 15 for the Debilitate (Vulnerable), then 26 and not 19 for the Slam: 41 on 40 HP.
    let mut plan = Plan::default();
    let state = combat_state!([debilitate, slam], [enemy!("A", 40, attack = 30, status = [power("VULNERABLE", 2)])], energy = 2, hp = 50);
    assert_eq!(combat::choose_card(&state, Some(&mut plan)), play_card(json!({"card_index": 0, "target": "A"})));
    assert_eq!(plan.lost, Some(0));
}

#[test]
fn test_energy_for_next_turn_is_worth_a_small_attack_now() {
    let invoke = card(0, "INVOKE", "Invoke+", "Skill", 1, "Next turn, Summon 3 and gain [necrobinder_energy_icon.png][necrobinder_energy_icon.png][necrobinder_energy_icon.png].", "Self");
    let drain = card(1, "DRAIN_POWER", "Drain Power", "Attack", 1, "Deal 7 damage.", "AnyEnemy");
    let queen = enemy!("QUEEN_0", 150, status = [power("VULNERABLE", 4)]); // she only buffs this turn
    assert_eq!(combat::choose_card(&combat_state!([invoke, drain], [queen], energy = 1, hp = 35, max_hp = 97, kind = "boss", round_no = 9), None), play_card(json!({"card_index": 0})));
}

#[test]
fn test_a_bolt_at_random_is_not_let_loose_next_to_a_crab_it_could_kill() {
    let rage = || json!([power("CRAB_RAGE", 1)]);
    let cool = card(0, "COOLHEADED", "Coolheaded", "Skill", 1, "Channel 1 Frost. Draw 1 card.", "Self");
    let strike = card(1, "ULTIMATE_STRIKE", "Ultimate Strike", "Attack", 1, "Deal 12 damage.", "AnyEnemy");
    let ball = card(2, "BALL_LIGHTNING", "Ball Lightning+", "Attack", 1, "Deal 9 damage. Channel 1 Lightning.", "AnyEnemy");
    let bolt = json!({"id": "LIGHTNING_ORB", "name": "Lightning", "passive_val": 3, "evoke_val": 8});
    let state = combat_state!(
        [cool, strike, ball],
        [enemy!("CRUSHER_0", 3, status = rage()), enemy!("ROCKET_0", 20, attack = 52, status = rage())],
        energy = 4,
        hp = 3,
        max_hp = 85,
        block = 7,
        kind = "boss",
        orbs = [bolt.clone(), bolt.clone(), bolt],
        orb_slots = 3
    );
    // Coolheaded first pushes a Lightning out at one of the two: on the Crusher it kills, and the Rocket takes 99 Block with its 52 still coming. The two attacks kill the Rocket first.
    let first = combat::choose_card(&state, None).unwrap();
    assert_eq!((first.1["card_index"].int(), first.1["target"].as_str()), (1, Some("ROCKET_0")));
}

#[test]
fn test_hits_on_all_enemies_stop_counting_once_one_crab_dies_of_them() {
    let rage = || json!([power("CRAB_RAGE", 1)]);
    let hand = [
        card(0, "EXTERMINATE", "Exterminate+", "Attack", 1, "Deal 10 damage to ALL enemies 4 times.", "AllEnemies"),
        card(1, "BEAT_INTO_SHAPE", "Beat into Shape", "Attack", 1, "Deal 11 damage.", "AnyEnemy"),
        card(2, "SOLAR_STRIKE", "Solar Strike", "Attack", 1, "Deal 15 damage.", "AnyEnemy"),
        card(3, "CRUSH_UNDER", "Crush Under", "Attack", 1, "Deal 14 damage.", "AnyEnemy"),
    ];
    // 15 on the Rocket and four rounds of 10 looks like both dead; the Rocket dies of the first round and the Crusher keeps 29 HP behind 99 Block. 15 + 14 + 11 on the Crusher kills what is about to hit us.
    let mut plan = Plan::default();
    let first = combat::choose_card(&combat_state!(hand, [enemy!("CRUSHER_0", 39, attack = 24, status = rage()), enemy!("ROCKET_0", 22, status = rage())], energy = 3, hp = 4, max_hp = 91, kind = "boss"), Some(&mut plan)).unwrap();
    assert_eq!(first.1["target"].as_str(), Some("CRUSHER_0"));
    assert_ne!(first.1["card_index"], 0);
    assert_eq!(plan.lost, Some(0));
}

// Asleep: The Lagavulin Matriarch sleeps for three turns unless she loses HP: those turns are for setting up.

/// `Asleep.state(turns, round_no=1)`.
fn asleep(turns: i64, round_no: i64) -> Value {
    let sleeper = json!({"entity_id": "LAGAVULIN_MATRIARCH_0", "hp": 222, "max_hp": 222, "block": 12, "status": [power("PLATING", 12), power("ASLEEP", turns)], "intents": [{"type": "Sleep", "label": ""}]});
    let inflame = card(1, "INFLAME", "Inflame", "Power", 1, "Gain 2 Strength.", "Self");
    combat_state!([STRIKE(0), inflame, DEFEND(2), STRIKE(3)], [sleeper], energy = 3, kind = "boss", round_no = round_no)
}

#[test]
fn test_she_is_left_asleep_while_the_powers_go_down() {
    let mut state = asleep(3, 1);
    assert_eq!(combat::choose_card(&state, None), play_card(json!({"card_index": 1})));
    state["player"]["hand"] = json!([STRIKE(0), DEFEND(1), STRIKE(2)]);
    assert_eq!(combat::choose_card(&state, None), None); // nothing that lasts: the turn ends with the energy unspent
}

#[test]
fn test_the_last_turn_of_her_sleep_is_for_hitting() {
    let mut state = asleep(1, 3);
    state["player"]["hand"] = json!([STRIKE(0), DEFEND(1), STRIKE(2)]);
    assert_eq!(pick(&state), 0);
    assert!(combat::choose_card(&asleep(3, 3), None).is_some()); // ... and so is round 3 if the count does not come down by itself
}

#[test]
fn test_nothing_is_thrown_at_a_sleeper() {
    let mut state = asleep(3, 1);
    state["player"]["potions"] = json!([potion(0, "Fire Potion", "Deal 20 damage to an enemy.", "AnyEnemy"), potion(1, "Flex Potion", "Gain 5 Strength this turn.", "Self")]);
    // (Nor drunk ourselves: Strength for this turn, with nothing to hit until she wakes, is poured out.)
    assert_eq!(combat::potion_actions(&state, Some(0), None), vec![]);
    let awake = with(asleep(1, 3), json!({"player": state["player"]}));
    assert_eq!(sorted(combat::potion_actions(&awake, Some(0), None).iter().map(|a| a.1["slot"].int()).collect()), [0, 1]);
}

// SlipperyPotions

#[test]
fn test_strength_is_not_drunk_while_every_hit_takes_one_hp() {
    let vantom = enemy!("VANTOM_0", 172, attack = 7, status = [power("SLIPPERY", 7)]);
    let mut state = combat_state!([STRIKE(0)], [vantom], kind = "boss", potions = [potion(0, "Flex Potion", "Gain 5 Strength this turn.", "Self"), potion(1, "Swift Potion", "Draw 3 cards.", "Self")]);
    assert_eq!(combat::potion_actions(&state, Some(0), None).iter().map(|a| a.1["slot"].int()).collect::<Vec<_>>(), [1]);
    state["battle"]["enemies"][0]["status"] = json!([]);
    assert_eq!(combat::potion_actions(&state, Some(0), None).iter().map(|a| a.1["slot"].int()).collect::<Vec<_>>(), [0, 1]);
}

// Debris: Crash Landing fills the hand with Debris (1 energy: Exhaust), which stays in the deck for the fight.

fn CRASH(i: i64) -> Value {
    card(i, "CRASH_LANDING", "Crash Landing", "Attack", 1, "Deal 21 damage to ALL enemies. Fill your Hand with Debris.", "AllEnemies")
}

/// `Debris.line(enemies, kind="boss", round_no=2)`.
fn line(enemies: Vec<Value>, kind: &str, round_no: i64) -> Vec<i64> {
    let mut s = combat_state!([STRIKE(0), CRASH(1), DEFEND(2), DEFEND(3), STRIKE(4)], enemies, energy = 3, hp = 70, max_hp = 75, kind = kind, round_no = round_no, draw_pile_count = 9, discard_pile_count = 5);
    indexes(&play_turn(&mut s, 3, false, None, None))
}

#[test]
fn test_it_is_read_as_filling_the_hand() {
    assert_eq!(parse(&CRASH(0)).debris, -1);
    assert_eq!(parse(&card(0, "COLLISION_COURSE", "Collision Course", "Attack", 0, "Deal 10 damage. Add a Debris into your Hand.", "AnyEnemy")).debris, 1);
}

#[test]
fn test_early_in_a_boss_fight_it_stays_in_the_hand() {
    // Six dead cards in a deck of nineteen, for eight more rounds: Regent bosses with Crash Landing in the deck were won 1 time in 6 (22 in 34 without).
    assert!(!line(vec![enemy!("LAGAVULIN_MATRIARCH_0", 200, attack = 14)], "boss", 2).contains(&1));
}

#[test]
fn test_it_finishes_a_fight_and_clears_a_room() {
    assert_eq!(line(vec![enemy!("LAGAVULIN_MATRIARCH_0", 20, attack = 14)], "boss", 2)[0], 1); // the kill: no later to pay for
    assert!(line(vec![enemy!("LAGAVULIN_MATRIARCH_0", 60, attack = 14)], "boss", 9).contains(&1)); // the fight is nearly done
    assert_eq!(line(vec![enemy!("A_0", 30, attack = 8), enemy!("B_0", 28, attack = 7), enemy!("C_0", 25, attack = 6)], "monster", 1)[0], 1);
}

#[test]
fn test_debris_is_cleared_with_energy_left_over() {
    let mut s = combat_state!([STRIKE(0), card(1, "DEBRIS", "Debris", "Status", 1, "Exhaust.", "None")], [enemy!("LAGAVULIN_MATRIARCH_0", 200)], energy = 2, kind = "boss");
    assert_eq!(indexes(&play_turn(&mut s, 3, false, None, None)), [0, 1]);
}

// PowersAndRelics

#[test]
fn test_the_rocks_attack_is_blocked_whole() {
    // Bowlbug (Rock): "If its attacks are fully blocked, it becomes Stunned." Three Defends stop its 15 and take its next turn away; two of them and an attack only save 10 HP.
    let big = card(0, "BLUDGEON", "Bludgeon", "Attack", 1, "Deal 16 damage.", "AnyEnemy");
    let rock = |status: Value| combat_state!([big.clone(), DEFEND(1), DEFEND(2), DEFEND(3)], [enemy!("BOWLBUG_ROCK_0", 60, attack = 15, status = status)], energy = 3, hp = 70, max_hp = 75);
    assert_eq!(sorted(indexes(&play_turn(&mut rock(json!([power("IMBALANCED", 1)])), 3, true, None, None))), [1, 2, 3]);
    assert_eq!(sorted(indexes(&play_turn(&mut rock(json!([])), 3, true, None, None))), [0, 1, 2]);
}

#[test]
fn test_each_gardener_is_hit_once_a_turn() {
    // "The first time it is hit each turn, it gains 6 Block": a second Strike on the same one does nothing.
    let foes = [enemy!("PHANTASMAL_GARDENER_0", 29, status = [power("SKITTISH", 6)]), enemy!("PHANTASMAL_GARDENER_1", 30, status = [power("SKITTISH", 6)])];
    let mut state = combat_state!([STRIKE(0), STRIKE(1)], foes, energy = 2);
    let first = combat::choose_card(&state, None).unwrap().1;
    let target = first["target"].s().to_string();
    let hit: usize = target[target.len() - 1..].parse().unwrap();
    state["battle"]["enemies"][hit]["hp"] = json!(23); // hit once: 6 HP gone, 6 Block up
    state["battle"]["enemies"][hit]["block"] = json!(6);
    state["player"]["hand"] = json!([STRIKE(1)]);
    state["player"]["energy"] = json!(1);
    assert_ne!(aimed(&state), Some(target));
}

fn segments(front: i64, middle: i64, back: i64) -> Value {
    let names = ["DECIMILLIPEDE_SEGMENT_FRONT_0", "DECIMILLIPEDE_SEGMENT_MIDDLE_0", "DECIMILLIPEDE_SEGMENT_BACK_0"];
    let foes: Vec<Value> = names.iter().zip([front, middle, back]).filter(|(_, hp)| *hp != 0).map(|(n, hp)| enemy!(n, hp, attack = 6, status = [power("REATTACH", 25)])).collect();
    combat_state!([STRIKE(0)], foes, energy = 1, kind = "elite")
}

#[test]
fn test_a_segment_is_not_killed_while_the_others_are_far_from_dead() {
    // "If other segments are still alive, revives in 2 turns with 25 HP": the Strike that would kill the middle one (7 HP) goes into the healthiest instead, until all three can die within two turns.
    assert_eq!(aimed(&segments(46, 7, 44)).as_deref(), Some("DECIMILLIPEDE_SEGMENT_FRONT_0"));
}

#[test]
fn test_the_segments_are_finished_once_the_rest_is_within_reach() {
    assert_eq!(aimed(&segments(20, 5, 18)).as_deref(), Some("DECIMILLIPEDE_SEGMENT_MIDDLE_0"));
    assert_eq!(aimed(&segments(0, 5, 0)).as_deref(), Some("DECIMILLIPEDE_SEGMENT_MIDDLE_0")); // the last one standing: the fight
}

#[test]
fn test_a_turn_costs_no_more_than_the_relic_allows() {
    // "You cannot lose more than 20 HP in a single turn": 5 Block in front of 40 saves nothing, the Strike counts.
    let mut state = combat_state!([STRIKE(0), DEFEND(1)], [enemy!("A", 60, attack = 40)], energy = 1, hp = 70);
    assert_eq!(pick(&state), 1);
    state["player"]["relics"] = json!([{"id": "BEATING_REMNANT", "description": "You cannot lose more than 20 HP in a single turn."}]);
    let mut plan = Plan::default();
    assert_eq!(pick_plan(&state, &mut plan), 0);
    assert_eq!(plan.lost, Some(20));
}

// VoidForm

fn FORM(i: i64) -> Value {
    card(i, "VOID_FORM", "Void Form", "Power", 3, "Ethereal. End your turn. The first 2 cards you play each turn are free to play.", "None")
}

#[test]
fn test_the_card_that_ends_the_turn_is_played_last() {
    // Played first, as Powers are, it ended round 1 of the Queen with two potions drunk and 62 damage in hand.
    let mut state = combat_state!([FORM(0), STRIKE(1), DEFEND(2)], [enemy!("QUEEN_0", 400, attack = 10)], energy = 5, kind = "boss");
    assert_ne!(pick(&state), 0);
    state["player"]["hand"] = json!([FORM(0)]);
    state["player"]["energy"] = json!(3);
    assert_eq!(pick(&state), 0);
}

#[test]
fn test_its_free_plays_go_to_the_dearest_cards() {
    // With free plays left the hand shows every cost as 0: the two free ones are the 3 and the 2, not two Strikes.
    let free = |c: Value| with(c, json!({"cost": "0"}));
    let blow = card(2, "KNOCKOUT_BLOW", "Knockout Blow", "Attack", 3, "Deal 30 damage.", "AnyEnemy");
    let blade = card(3, "SOVEREIGN_BLADE", "Sovereign Blade", "Attack", 2, "Deal 18 damage.", "AnyEnemy");
    let mut state = combat_state!(
        [free(STRIKE(0)), free(STRIKE(1)), free(blow.clone()), free(blade)],
        [enemy!("A", 300)],
        energy = 2,
        kind = "boss",
        status = [power("VOID_FORM", 2)],
        _costs = json!({"Strike": 1, "Knockout Blow": 3, "Sovereign Blade": 2})
    );
    assert_eq!(combat::free_plays(&state), 2);
    assert!([2, 3].contains(&pick(&state)));
    // Once they are used up the costs show again, and nothing is free.
    state["_plays_this_turn"] = json!(2);
    state["player"]["hand"] = json!([STRIKE(0), STRIKE(1), blow]);
    assert_eq!(combat::free_plays(&state), 0);
    assert!([0, 1].contains(&pick(&state)));
    assert_eq!(combat::free_plays(&with(state, json!({"_plays_this_turn": 0}))), 0); // (a hand that shows a cost says so, whatever the bot has counted)
}

// StunsAndShells

/// (the original's default: attack 21.)
fn hopper(flutter: i64) -> Value {
    enemy!("THIEVING_HOPPER_0", 40, attack = 21, status = [power("ESCAPE_ARTIST", 3), power("SWIPE", 1), power("FLUTTER", flutter)])
}

#[test]
fn test_the_last_flutter_hit_stuns_it() {
    // "Deal attack damage 5 times to Stun it": two hits from that, the Strikes are this turn's Block and more.
    let turn = |flutter: i64| combat_state!([DEFEND(0), DEFEND(1), DEFEND(2), STRIKE(3), STRIKE(4)], [hopper(flutter)], energy = 3, hp = 50);
    let mut plan = Plan::default();
    assert!([3, 4].contains(&pick_plan(&turn(2), &mut plan)));
    assert_eq!(plan.lost, Some(0));
    combat::choose_card(&turn(5), Some(&mut plan)); // five hits away with two attacks in hand, the 21 has to be blocked as before
    assert!(plan.lost.unwrap() > 0);
}

#[test]
fn test_attacks_do_half_until_then_and_what_is_not_an_attack_does_not() {
    let mut foe = read_foes(&[hopper(2)]).remove(0);
    assert_eq!(combat::_hit(&mut foe, 10, 1, false, false), 5);
    assert_eq!(combat::_hit(&mut foe, 10, 1, false, true), 10); // (an orb, a Power's damage: not an Attack, and it does not count)
    assert_eq!((foe.flutter, foe.incoming), (1, 21));
    assert_eq!(combat::_hit(&mut foe, 10, 2, false, false), 5 + 10); // the hit that Stuns it is the last one halved
    assert_eq!((foe.flutter, foe.incoming, foe.damage_mult), (0, 0, 1.0));
}

#[test]
fn test_the_one_holding_our_card_is_worth_killing() {
    let foe = read_foes(&[hopper(5)]).remove(0);
    assert!(foe.thief);
    assert!(foe.worth() > 1.0);
}

#[test]
fn test_curl_up_is_block_after_the_first_hit() {
    let mut foe = read_foes(&[enemy!("LOUSE_PROGENITOR_0", 60, status = [power("CURL_UP", 14)])]).remove(0);
    assert_eq!(combat::_hit(&mut foe, 6, 2, false, false), 6); // the second Strike of the turn meets the 14 Block
    assert_eq!(foe.block, 8);
}

#[test]
fn test_the_terror_eel_is_stunned_as_its_hp_crosses_the_shriek() {
    // "The first time Terror Eel's HP reaches 70 or below, it becomes Stunned": the Strike is this turn's Block.
    let mut plan = Plan::default();
    let state = combat_state!([DEFEND(0), STRIKE(1)], [enemy!("TERROR_EEL_0", 75, attack = 22, status = [power("SHRIEK", 70)])], energy = 1, hp = 40, kind = "elite");
    assert_eq!(pick_plan(&state, &mut plan), 1);
    assert_eq!(plan.lost, Some(0));
}

#[test]
fn test_a_hardened_shell_lets_through_what_it_has_left_this_turn() {
    let colony = |left: i64| enemy!("SKULKING_COLONY_0", 60, attack = 14, status = [power("HARDENED_SHELL", left)]);
    let mut foe = read_foes(&[colony(8)]).remove(0);
    assert_eq!(combat::_hit(&mut foe, 6, 2, false, false), 8);
    assert_eq!(combat::_hit(&mut foe, 6, 1, false, false), 0);
    // With nothing left to take off it this turn, the energy goes to Block.
    assert_eq!(pick(&combat_state!([STRIKE(0), DEFEND(1)], [colony(0)], energy = 1, hp = 40, kind = "elite")), 1);
    assert_eq!(read_foes(&[enemy!("A", 60)])[0].shell, -1);
}

// PlayedAgain

fn BLADE(i: i64) -> Value {
    card(i, "SOVEREIGN_BLADE", "Sovereign Blade", "Attack", 1, "Retain. Deal 30 damage. Replay 1.", "AnyEnemy")
}

#[test]
fn test_replay_is_read_from_the_card_and_not_from_one_that_hands_it_out() {
    assert_eq!(parse(&BLADE(0)).replay, 1);
    let sage = card(0, "SWORD_SAGE", "Sword Sage", "Power", 2, "Sovereign Blade gains Replay 1.", "None");
    assert_eq!(parse(&with(sage, json!({"keywords": [{"name": "Replay", "description": "Plays this card an additional time."}]}))).replay, 0);
    assert_eq!(parse(&with(STRIKE(0), json!({"keywords": [{"name": "Replay", "description": "Plays this card an additional 2 times."}]}))).replay, 2);
}

#[test]
fn test_a_card_played_twice_is_two_of_the_three_sloth_allows() {
    // Against the Knowledge Demon the Blade took Sloth from 0 to 2, the plan's third card could not be played, and 41 HP went on 42.
    let foes = read_foes(&[enemy!("KNOWLEDGE_DEMON_0", 160, attack = 36)]);
    let (blade, strike, guard) = (parse(&BLADE(0)), parse(&STRIKE(1)), parse(&DEFEND(2)));
    let kw = ctx!(energy = 4, hp = 42, max_hp = 108, fight = "boss", card_limit = 3);
    assert!(combat::simulate(&[blade.clone(), strike.clone(), guard.clone()], &foes, &kw).is_none());
    assert!(combat::simulate(&[blade.clone(), guard.clone()], &foes, &kw).is_some());
    assert!(combat::simulate(&[strike, guard, blade.clone()], &foes, &kw).is_some()); // (as the third it still goes)
    assert!(gain(&[blade], &foes, &kw) > 60.0 * 1.5 - 1.0);
    // The count is the game's own, the number on the power.
    let state = combat_state!([BLADE(0), STRIKE(1), DEFEND(2)], [enemy!("KNOWLEDGE_DEMON_0", 160, attack = 36)], energy = 4, hp = 42, kind = "boss", status = [power("SLOTH", 2)]);
    assert_eq!(combat::cards_left_this_turn(&state), Some(1));
    assert_eq!(combat::cards_left_this_turn(&with(state, json!({"_plays_this_turn": 1}))), Some(1));
}

#[test]
fn test_the_last_card_sloth_allows_is_not_one_that_only_draws() {
    // Sweeping Gaze, Soul and Soul+ were played with five energy in hand, 10 damage dealt and 13 coming.
    let soul = |i: i64| card(i, "SOUL", "Soul", "Skill", 0, "Draw 2 cards. Exhaust.", "Self");
    let mut state = combat_state!([soul(0), soul(1), STRIKE(2)], [enemy!("KNOWLEDGE_DEMON_0", 190, attack = 13)], energy = 5, hp = 32, kind = "boss", status = [power("SLOTH", 2)]);
    assert_eq!(pick(&state), 2); // one play left: the Strike
    state["player"]["status"] = json!([power("SLOTH", 3)]);
    assert_eq!(combat::choose_card(&state, None), None);
    let (foes, kw, one, two) = (combat::read_enemies(&state), ctx!(energy = 5, hp = 32, fight = "boss"), parse(&soul(0)), parse(&soul(1)));
    let free = score(&[one.clone(), two.clone()], &foes, &kw) - score(&[one.clone()], &foes, &kw);
    let limited = SimulateArgs { card_limit: Some(2), ..kw.clone() };
    let last = score(&[one.clone(), two], &foes, &limited) - score(&[one], &foes, &limited);
    assert!(free > last + 1.5); // the second Soul's cards can no longer be played
}

#[test]
fn test_under_echo_form_the_first_card_of_a_turn_is_the_one_worth_two() {
    let heavy = card(1, "BLUDGEON", "Bludgeon", "Attack", 2, "Deal 14 damage.", "AnyEnemy");
    let state = combat_state!([STRIKE(0), heavy], [enemy!("A", 300)], energy = 3, kind = "boss", status = [power("ECHO_FORM", 1)]);
    let mut plan = Plan::default();
    assert_eq!(pick_plan(&state, &mut plan), 1);
    let twice = plan.score.unwrap();
    combat::choose_card(&with(state, json!({"_plays_this_turn": 1})), Some(&mut plan));
    assert!(twice > plan.score.unwrap() + 14.0); // (the second card of the turn is played once)
}

// DoomAndAttacksTogether: Doom is health already gone: it and the attacks go on the same enemy, and not on one it has finished.

fn SCOURGE(i: i64) -> Value {
    card(i, "SCOURGE", "Scourge", "Skill", 1, "Apply 13 Doom. Draw 1 card.", "AnyEnemy")
}

/// (card name, target) of every card of the turn, the enemies taking what each card did (near enough for the next choice).
/// (the original's default: energy 3.)
fn plays(hand: Vec<Value>, foes: Vec<Value>, energy: i64) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut did = |state: &mut Value, c: &Value, params: &Value| -> bool {
        out.push((c["name"].s().to_string(), params["target"].as_str().map(str::to_string)));
        for e in state["battle"]["enemies"].as_array_mut().unwrap() {
            if e["entity_id"] == params["target"] && c["name"] == "Scourge" {
                e["status"] = json!([power("DOOM", 13 + e["status"].list().iter().map(|p| p["amount"].int()).sum::<i64>())]);
            } else if e["entity_id"] == params["target"] {
                e["hp"] = json!(e["hp"].int() - 6);
            }
        }
        false
    };
    play_turn(&mut combat_state!(hand, foes, energy = energy, hp = 49, max_hp = 66), 99, false, Some(&mut did), None);
    out
}

#[test]
fn test_scourge_and_the_strikes_go_on_one_nibbit() {
    // The Doom went on the Nibbit with 43 HP, the attacks on the one with 42.
    let foes = vec![enemy!("NIBBIT_0", 42, attack = 6), enemy!("NIBBIT_1", 43)];
    let targets: std::collections::BTreeSet<Option<String>> = plays(vec![SCOURGE(0), STRIKE(1), STRIKE(2)], foes, 3).into_iter().map(|(_name, t)| t).collect();
    assert_eq!(targets.len(), 1);
}

#[test]
fn test_an_enemy_doom_has_finished_is_left_alone() {
    // 21 HP under 26 Doom dies when its turn ends; two Strikes do not kill it sooner, and the other one is still to do.
    let foes = vec![enemy!("NIBBIT_0", 21, attack = 10, status = [power("DOOM", 26)]), enemy!("NIBBIT_1", 28, attack = 10)];
    let targets: std::collections::BTreeSet<Option<String>> = plays(vec![STRIKE(0), STRIKE(1)], foes, 2).into_iter().map(|(_name, t)| t).collect();
    assert_eq!(targets, std::collections::BTreeSet::from([Some("NIBBIT_1".to_string())]));
    // ... but an attack that kills it now stops its hit.
    let foes = vec![enemy!("NIBBIT_0", 6, attack = 10, status = [power("DOOM", 26)]), enemy!("NIBBIT_1", 28, attack = 10)];
    assert_eq!(plays(vec![STRIKE(0)], foes, 1), [("Strike".to_string(), Some("NIBBIT_0".to_string()))]);
}

#[test]
fn test_more_doom_goes_on_the_one_that_still_needs_it() {
    let foes = vec![enemy!("NIBBIT_0", 21, attack = 10, status = [power("DOOM", 26)]), enemy!("NIBBIT_1", 40, attack = 10)];
    assert_eq!(plays(vec![SCOURGE(0)], foes, 1), [("Scourge".to_string(), Some("NIBBIT_1".to_string()))]);
}
