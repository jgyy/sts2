use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{json, Value};
use sts2bot::cards::{deck_value, parse, CardInfo};
use sts2bot::fight::combat::{self, Enemy, Plan, SimulateArgs};
use sts2bot::fight::battle;
use sts2bot::learn;
use sts2bot::run::{decks, potions};

use super::{aimed, combat_state, ctx, gain, outcome, pick, pick_plan, play_turn, potion, power, read_foes, worth};
use crate::fake_game::{card, with};
use crate::helpers::{enemy, DEFEND, STRIKE};

const ICON: &str = "[defect_energy_icon.png]";
const STAR: &str = "[star_icon.png]";

fn BARRICADE() -> Value {
    card(0, "BARRICADE", "Barricade", "Power", 3, "Block is not removed at the start of your turn.", "Self")
}

fn DISMANTLE() -> Value {
    card(0, "DISMANTLE", "Dismantle", "Attack", 1, "Deal 8 damage. If the enemy is Vulnerable, hits twice.", "AnyEnemy")
}

fn HEMO() -> Value {
    card(0, "HEMO", "Hemokinesis", "Attack", 1, "Lose 2 HP. Deal 15 damage.", "AnyEnemy")
}

fn BIG() -> Value {
    card(0, "BIG", "Big", "Attack", 1, "Deal 14 damage.", "AnyEnemy")
}

fn ZAP() -> Value {
    card(0, "ZAP", "Zap", "Skill", 1, "Channel 1 Lightning.", "Self")
}

fn DEADLY() -> Value {
    card(0, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy")
}

fn STRENGTH(slot: i64) -> Value {
    potion(slot, "Strength Potion", "Gain 2 Strength.", "Self")
}

fn FIRE(slot: i64) -> Value {
    potion(slot, "Fire Potion", "Deal 20 damage.", "AnyEnemy")
}

/// A 200-HP enemy as the planner sees it (`dummy(incoming, incoming_hits=hits)`; the original's defaults: 0 and 0).
fn dummy(incoming: i64, incoming_hits: i64) -> Vec<Enemy> {
    vec![Enemy { entity_id: "A".into(), hp: 200, incoming, incoming_hits, ..Default::default() }]
}

fn score<L: std::borrow::Borrow<CardInfo>>(line: &[L], foes: &[Enemy], kw: &SimulateArgs) -> f64 {
    combat::simulate(line, foes, kw).unwrap().score
}

fn orb(kind: &str, passive: i64, burst: i64) -> (String, i64, i64) {
    (kind.to_string(), passive, burst)
}

fn slots(actions: &[sts2bot::Action]) -> Vec<i64> {
    actions.iter().map(|a| a.1["slot"].as_i64().unwrap()).collect()
}

fn sorted(mut v: Vec<i64>) -> Vec<i64> {
    v.sort();
    v
}

// CardParsing

#[test]
fn test_what_the_text_says() {
    // a card, and what parse makes of its text
    let i = parse(&card(0, "X", "X", "Attack", 2, "Deal 5 damage 3 times. Apply 2 Vulnerable.", "AnyEnemy"));
    assert_eq!((i.damage, i.hits, i.vulnerable, i.cost, i.needs_target), (5, 3, 2, 2, true));
    let i = parse(&DISMANTLE());
    assert_eq!((i.damage, i.hits, i.double_if_vulnerable), (8, 1, true));
    let i = parse(&card(0, "DEBILITATE", "Debilitate", "Attack", 1, "Deal 10 damage. Vulnerable and Weak are twice as effective against the enemy for the next 2 turns.", "AnyEnemy"));
    assert_eq!((i.damage, i.hits), (10, 1));
    let i = parse(&card(0, "TWIN_STRIKE", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy"));
    assert_eq!((i.damage, i.hits), (5, 2)); // ("twice" counts hits only when it is about the damage)
    let i = parse(&card(0, "FISTICUFFS", "Fisticuffs", "Attack", 1, "Deal 7 damage. Gain Block equal to damage dealt.", "AnyEnemy"));
    assert_eq!((i.damage, i.block), (7, 7));
    let i = parse(&json!({"index": 1, "type": "Skill", "cost": "X", "description": "Gain 7 Block. Draw 2 cards."}));
    assert_eq!((i.is_x(), i.block, i.draw), (true, 7, 2));
    let i = parse(&card(0, "DRUM_OF_BATTLE", "Drum of Battle", "Skill", 1, "Draw 2 cards. When this card is Exhausted, gain [ironclad_energy_icon.png][ironclad_energy_icon.png].", "Self"));
    assert_eq!((i.draw, i.energy_gain), (2, 0));
    assert_eq!(parse(&card(0, "X", "X", "Skill", 2, "Apply 3 Poison to a random enemy 3 times.", "None")).poison, 9); // (the Silent's, Regent's, Necrobinder's and Defect's)
    assert_eq!(parse(&card(0, "X", "X", "Skill", 1, "Add 3 Shivs into your Hand. Exhaust.", "None")).shivs, 3);
    assert_eq!(parse(&card(0, "X", "X", "Skill", 0, &format!("Gain 9 {STAR}. Exhaust."), "None")).star_gain, 9);
    assert_eq!(parse(&card(0, "X", "X", "Skill", 1, &format!("Gain {STAR}{STAR}."), "None")).star_gain, 2);
    let i = parse(&card(0, "X", "X", "Skill", 1, &format!("Gain 7 Block. Next turn, gain {ICON}."), "None"));
    assert_eq!((i.block, i.next_energy, i.energy_gain), (7, 1, 0));
    let i = parse(&card(0, "X", "X", "Skill", 1, "Gain 4 Block. Next turn, gain 4 Block.", "None"));
    assert_eq!((i.block, i.next_block), (4, 4));
    let i = parse(&card(0, "X", "X", "Attack", 1, "Deal 6 damage. Channel 1 Frost.", "AnyEnemy"));
    assert_eq!((i.damage, i.channel), (6, vec!["FROST".to_string()]));
    assert_eq!(parse(&card(0, "X", "X", "Attack", 1, "Deal 5 damage for each Skill in your Hand. (Hits 0 times)", "AnyEnemy")).damage, 0);
    assert_eq!(parse(&card(0, "X", "X", "Skill", 2, "Summon 4. Gain 7 Block.", "None")).summon, 4);
    assert_eq!(parse(&card(0, "X", "X", "Skill", 1, "ALL enemies lose 6 Strength this turn. Exhaust.", "None")).enemy_strength_loss, 6);
}

#[test]
fn test_intent_damage() {
    assert_eq!(combat::intent_damage(&json!({"type": "Attack", "label": "5x3"})), 15);
    assert_eq!(combat::intent_damage(&json!({"type": "AttackDebuff", "label": "11"})), 11);
    assert_eq!(combat::intent_damage(&json!({"type": "Buff", "label": "3"})), 0);
}

#[test]
fn test_deck_value_ordering() {
    let strike = with(card(0, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "None"), json!({"rarity": "Basic"}));
    let good = with(card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage. Apply 1 Weak. Apply 1 Vulnerable.", "None"), json!({"rarity": "Uncommon"}));
    assert!(deck_value(&good) > deck_value(&strike));
    assert!(deck_value(&strike) > deck_value(&card(0, "REGRET", "Regret", "Curse", 0, "Unplayable.", "None")));
}

/// What the planner does first: a hand index, a target, the params, the whole action, or nothing.
enum First {
    Index(i64),
    Target(&'static str),
    Params(Value),
    Action(Value),
    Nothing,
}

/// (the rule, a turn, what the planner does first)
fn FIRST_MOVES() -> Vec<(&'static str, Value, First)> {
    use First::*;
    let play = |params: Value| Action(json!(["play_card", params]));
    vec![
        ("takes lethal instead of blocking", combat_state!([DEFEND(0), STRIKE(1), STRIKE(2)], [enemy!("A_0", 12, attack = 20)], hp = 30), Params(json!({"card_index": 1, "target": "A_0"}))),
        ("blocks when the attack would kill", combat_state!([STRIKE(0), DEFEND(1), DEFEND(2), DEFEND(3)], [enemy!("A_0", 60, attack = 14)], hp = 12, max_hp = 80), Index(1)),
        ("attacks an enemy that is not attacking", combat_state!([DEFEND(0), STRIKE(1)], [enemy!("A_0", 60)], energy = 1), Index(1)),
        ("kills the enemy it can kill", combat_state!([STRIKE(0)], [enemy!("BIG_0", 50, attack = 5), enemy!("SMALL_0", 6, attack = 10)], energy = 1), Target("SMALL_0")),
        ("Vulnerable makes lethal reachable", combat_state!([STRIKE(0), DEFEND(1)], [enemy!("A_0", 9, attack = 30, status = [power("VULNERABLE", 2)])], energy = 1, hp = 10), Index(0)),
        (
            "a hit doubled on the Vulnerable: 8 * 1.5 * 2 = 24 kills it",
            combat_state!([DISMANTLE()], [enemy!("PLAIN_0", 20, attack = 5), enemy!("VULN_0", 24, attack = 5, status = [power("VULNERABLE", 1)])], energy = 1),
            Target("VULN_0"),
        ),
        ("blocks at full health when it cannot kill", combat_state!([STRIKE(0), DEFEND(1)], [enemy!("A_0", 40, attack = 10)], energy = 1), Index(1)),
        ("no energy spent on Status cards", combat_state!([card(0, "SLIMED", "Slimed", "Status", 1, "Exhaust.", "None"), STRIKE(1)], [enemy!("A_0", 40)], energy = 1), Index(1)),
        ("the Strike eats the Slippery", combat_state!([HEMO(), STRIKE(1)], [enemy!("V_0", 100, status = [power("SLIPPERY", 1)])], energy = 2), Index(1)),
        ("never HP paid for 1 damage", combat_state!([HEMO(), STRIKE(1)], [enemy!("V_0", 100, status = [power("SLIPPERY", 8)])], energy = 1), Index(1)),
        (
            "Thorns cost HP per hit",
            combat_state!(
                [card(0, "TWIN", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy"), card(1, "BIG", "Big", "Attack", 1, "Deal 10 damage.", "AnyEnemy")],
                [enemy!("A_0", 60, status = [power("THORNS", 5)])],
                energy = 1
            ),
            Index(1),
        ),
        (
            "an Illusion that just revives is ignored",
            combat_state!([STRIKE(0)], [enemy!("EYE_0", 6, status = [power("ILLUSION", 1), power("MINION", 1)]), enemy!("FOGMOG_0", 50, attack = 8)], energy = 1),
            Target("FOGMOG_0"),
        ),
        (
            "the leader's death wins with minions left",
            combat_state!([STRIKE(0), DEFEND(1)], [enemy!("MINION_0", 20, attack = 30, status = [power("MINION", 1)]), enemy!("LEADER_0", 6)], energy = 1, hp = 10),
            play(json!({"card_index": 0, "target": "LEADER_0"})),
        ),
        (
            "races to the stun threshold: 6 damage stuns it, 5 Block would not save us",
            combat_state!([STRIKE(0), DEFEND(1)], [enemy!("BEAST_0", 155, attack = 24, status = [power("PLOW", 150)])], energy = 1, hp = 19),
            Index(0),
        ),
        ("Block cards are useless while Block is locked", combat_state!([STRIKE(0), DEFEND(1)], [enemy!("A_0", 40, attack = 10)], energy = 1, status = [power("NO_BLOCK", 1)]), Index(0)),
        ("a Slow enemy gets the Skills first", combat_state!([BIG(), STRIKE(1), DEFEND(2)], [enemy!("EFFIGY_0", 120, attack = 10, status = [power("SLOW", 0)])], energy = 3), Index(2)),
        ("... and the big hit last", combat_state!([BIG(), STRIKE(1)], [enemy!("EFFIGY_0", 120, status = [power("SLOW", 0)])], energy = 2), Index(1)),
        (
            "the Slow order that kills: 6x3 then 10 (+10%) is 29, the other way 28",
            combat_state!(
                [card(0, "PHOTON_CUT", "Photon Cut", "Attack", 1, "Deal 10 damage.", "AnyEnemy"), card(1, "CELESTIAL_MIGHT", "Celestial Might", "Attack", 2, "Deal 6 damage 3 times.", "AnyEnemy"), DEFEND(2)],
                [enemy!("E", 29, attack = 23, status = [power("SLOW", 0)])],
                energy = 3,
                hp = 40
            ),
            play(json!({"card_index": 1, "target": "E"})),
        ),
        (
            "Weak goes on the attacker",
            combat_state!([card(0, "W", "Weaken", "Skill", 1, "Apply 2 Weak.", "AnyEnemy")], [enemy!("IDLE_0", 30), enemy!("HITTER_0", 60, attack = 20)], energy = 1, hp = 30),
            play(json!({"card_index": 0, "target": "HITTER_0"})),
        ),
        (
            "Debilitate doubles Vulnerable",
            combat_state!([STRIKE(0), DEFEND(1)], [enemy!("A", 12, attack = 5, status = [power("VULNERABLE", 1), power("DEBILITATE", 2)])], energy = 1),
            play(json!({"card_index": 0, "target": "A"})),
        ),
        (
            "Toxic is played: 5 saved beats 4 blocked",
            combat_state!([card(0, "TOXIC", "Toxic", "Status", 1, "At the end of your turn, if this is in your Hand, take 5 damage. Exhaust.", "Self"), DEFEND(1)], [enemy!("A", 90, attack = 4)], energy = 1),
            play(json!({"card_index": 0})),
        ),
        (
            "a Burn's 2 and the 5 coming would kill",
            combat_state!(
                [with(card(0, "BURN", "Burn", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 2 damage.", "Self"), json!({"can_play": false})), STRIKE(1), DEFEND(2)],
                [enemy!("A", 90, attack = 5)],
                energy = 1,
                hp = 6
            ),
            play(json!({"card_index": 2})),
        ),
        (
            "Lightning orbs kill a lone enemy before it acts",
            combat_state!([DEFEND(0), DEFEND(1)], [enemy!("A", 5, attack = 28)], energy = 2, hp = 20, orbs = vec![json!({"id": "LIGHTNING_ORB", "passive_val": 3, "evoke_val": 8}); 2], orb_slots = 3),
            Nothing,
        ),
        ("Enrage keeps the Skill back", combat_state!([ZAP(), STRIKE(1)], [enemy!("A", 100, attack = 10, status = [power("ENRAGE", 2)])], energy = 1, kind = "boss"), Index(1)),
        ("... for when it is needed", combat_state!([ZAP()], [enemy!("A", 100, attack = 10, status = [power("ENRAGE", 2)])], energy = 1, kind = "boss"), Nothing),
        (
            "Poison does not kill through Intangible",
            combat_state!([DEFEND(0)], [enemy!("FYSH", 6, attack = 9, status = [power("POISON", 8), power("INTANGIBLE", 1)])], energy = 1, hp = 29),
            play(json!({"card_index": 0})),
        ),
        (
            "Poison that covers its HP stops the attack",
            combat_state!([DEADLY(), DEFEND(1)], [enemy!("A", 9, attack = 20, status = [power("POISON", 4)])], energy = 1, hp = 30),
            play(json!({"card_index": 0, "target": "A"})),
        ),
    ]
}

// CombatPlanning

#[test]
fn test_first_moves() {
    for (why, state, want) in FIRST_MOVES() {
        let act = combat::choose_card(&state, None);
        match want {
            First::Index(i) => assert_eq!(act.unwrap().1["card_index"], i, "{why}"),
            First::Target(t) => assert_eq!(act.unwrap().1["target"], t, "{why}"),
            First::Params(p) => assert_eq!(act.unwrap().1, p, "{why}"),
            First::Action(a) => {
                let act = act.unwrap();
                assert_eq!(json!([act.0, act.1]), a, "{why}")
            }
            First::Nothing => assert_eq!(act, None, "{why}"),
        }
    }
}

#[test]
fn test_a_slippery_stack_taken_off_is_worth_a_hit() {
    // Vantom on 8 Slippery: each stack is a later hit that lands in full, so the hits go before the Defends (fights still on Slippery at round 5 were lost 4 times in 8) ...
    let twin = card(0, "TWIN_STRIKE", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy");
    let vantom = |attack: i64| enemy!("VANTOM_0", 173, attack = attack, status = [power("SLIPPERY", 8)]);
    let mut s = combat_state!([twin.clone(), DEFEND(1), STRIKE(2), DEFEND(3), STRIKE(4)], [vantom(7)], energy = 3, kind = "boss");
    assert_eq!(sorted(play_turn(&mut s, 3, false, None, None).iter().map(|c| c["index"].as_i64().unwrap()).collect()), [0, 2, 4]);
    // ... but not in front of the big hit when already hurt: there the Block comes first.
    assert!([1, 3].contains(&pick(&combat_state!([twin, DEFEND(1), DEFEND(3)], [vantom(26)], energy = 2, hp = 30, kind = "boss"))));
}

#[test]
fn test_never_plays_a_card_that_can_kill_us() {
    let gambit = card(0, "THE_GAMBIT", "The Gambit", "Skill", 0, "Gain 50 Block. If you take unblocked attack damage this combat, die.", "Self");
    assert!(parse(&gambit).deadly);
    assert!(deck_value(&gambit) < 0.0);
    assert!(combat::choose_card(&combat_state!([gambit], [enemy!("A_0", 60, attack = 20)], energy = 3), None).is_none());
}

#[test]
fn test_card_limit_spends_plays_on_the_best_cards() {
    let hand = [card(0, "BT", "Battle Trance", "Skill", 0, "Draw 3 cards.", "Self"), card(1, "BIG", "Big", "Attack", 1, "Deal 14 damage.", "AnyEnemy")];
    let mut s = combat_state!(hand, [enemy!("A_0", 60)], energy = 3, status = [power("RINGING", 1)]);
    assert_eq!(pick(&s), 1);
    s["_plays_this_turn"] = json!(1);
    assert!(combat::choose_card(&s, None).is_none());
}

#[test]
fn test_blocks_a_hit_that_would_take_most_of_what_is_left() {
    let aoe = card(0, "BREAK", "Breakthrough", "Attack", 1, "Lose 1 HP. Deal 9 damage to ALL enemies.", "AllEnemies");
    let bash = card(1, "BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy");
    let foes = [enemy!("F_0", 39, attack = 9, status = [power("MINION", 1)]), enemy!("F_1", 38, attack = 8, status = [power("MINION", 1)]), enemy!("PRIEST_0", 98, attack = 10)];
    let mut s = combat_state!([aoe, bash, DEFEND(2), DEFEND(3), STRIKE(4)], foes, energy = 3, hp = 37, kind = "boss");
    assert_eq!(play_turn(&mut s, 3, true, None, None).iter().filter(|c| c["name"] == "Defend").count(), 2);
}

#[test]
fn test_known_cards_are_nudged() {
    let shrug = card(0, "SHRUG_IT_OFF", "Shrug It Off", "Skill", 1, "Gain 8 Block. Draw 1 card.", "Self");
    let plain = card(0, "SOMETHING_NEW", "Something New", "Skill", 1, "Gain 8 Block. Draw 1 card.", "Self");
    let _crowd = learn::with_knobs(&[("crowd", 0.0)]); // our own table (other players' runs rate Barricade high)
    assert!(deck_value(&shrug) > deck_value(&plain));
    assert!(deck_value(&BARRICADE()) < deck_value(&plain));
    assert!(deck_value(&with(shrug, json!({"name": "Shrug It Off+", "id": null}))) > deck_value(&plain));
}

#[test]
fn test_an_enemy_that_grows_every_turn_is_raced() {
    let sculptor = |powers: Vec<Value>| enemy!("DEVOTED_SCULPTOR_0", 150, attack = 12, status = powers);
    let first = |powers: Vec<Value>, hp: i64| pick(&combat_state!([STRIKE(0), STRIKE(1), DEFEND(2), DEFEND(3)], [sculptor(powers)], energy = 2, hp = hp, max_hp = 90));
    assert!([2, 3].contains(&first(vec![], 70))); // an enemy like any other: 12 coming, two Defends
    assert!([0, 1].contains(&first(vec![power("RITUAL", 9)], 70))); // 9 Strength a turn: every turn it lives is dearer
    assert!([2, 3].contains(&first(vec![power("RITUAL", 9)], 14))); // ... but not into the grave
    assert_eq!(read_foes(&[sculptor(vec![power("STRENGTH", 6)])])[0].grows, 6.0);
    assert_eq!(combat::read_enemies(&combat_state!([], [sculptor(vec![power("STRENGTH", 6)])], round_no = 4))[0].grows, 2.0); // 6 Strength in three turns
}

#[test]
fn test_a_big_attack_is_not_spent_on_a_minion_that_is_not_attacking() {
    let squeeze = card(0, "SQUEEZE", "Squeeze", "Attack", 1, "Osty deals 35 damage.", "AnyEnemy");
    let egg = with(enemy!("TOUGH_EGG_0", 17), json!({"status": [power("MINION", 1)], "intents": [{"type": "Summon", "label": ""}]}));
    let copter = enemy!("OVICOPTER_0", 105, attack = 16);
    assert_eq!(aimed(&combat_state!([squeeze], [egg.clone(), copter.clone()], hp = 69, pets = [json!({"name": "Osty", "hp": 3, "alive": true})])).as_deref(), Some("OVICOPTER_0"));
    // A small attack (an 18-damage Strike) that just kills the egg is still a kill worth taking, and so is any kill of a minion about to hit us.
    let foes = read_foes(&[egg.clone(), copter.clone()]);
    assert_eq!(&*foes[combat::_pick_target(&foes, 18, 1, false).unwrap()].entity_id, "TOUGH_EGG_0");
    let foes = read_foes(&[with(egg, json!({"intents": [{"type": "Attack", "label": "6"}]})), copter]);
    assert_eq!(&*foes[combat::_pick_target(&foes, 35, 1, false).unwrap()].entity_id, "TOUGH_EGG_0");
}

#[test]
fn test_the_enemy_that_does_most_for_its_health_is_hit_first() {
    let beetle = with(enemy!("SLUMBERING_BEETLE_0", 83), json!({"intents": [{"type": "Attack", "label": "16"}, {"type": "Buff", "label": ""}]}));
    let state = combat_state!([STRIKE(0)], [enemy!("BOWLBUG_ROCK_0", 46, attack = 15), beetle], hp = 80);
    assert_eq!(aimed(&state).as_deref(), Some("BOWLBUG_ROCK_0")); // nothing known of them: the one that dies soonest
    // Four rounds in: the bug has hit every other turn, the beetle slept for three and now grows every turn.
    let mut seen = with(state, json!({"_threat": {"BOWLBUG_ROCK_0": 7.5, "SLUMBERING_BEETLE_0": 4.0}}));
    assert_eq!(aimed(&seen).as_deref(), Some("SLUMBERING_BEETLE_0"));
    assert_eq!(combat::read_enemies(&seen)[1].threat, 24.0);
    // ... but a bug nearly dead is finished first, and two that hit alike are taken lowest first.
    seen["battle"]["enemies"][0]["hp"] = json!(12);
    assert_eq!(aimed(&seen).as_deref(), Some("BOWLBUG_ROCK_0"));
    let twins = combat_state!([STRIKE(0)], [enemy!("A", 50, attack = 10), enemy!("B", 40, attack = 10)], hp = 80);
    assert_eq!(aimed(&with(twins, json!({"_threat": {"A": 10.0, "B": 10.0}}))).as_deref(), Some("B"));
    // Boss and elite fights keep their own order (the crabs, minions and their leaders).
    let mut boss = with(seen, json!({"state_type": "boss"}));
    boss["battle"]["enemies"][0]["hp"] = json!(46);
    assert_eq!(aimed(&boss).as_deref(), Some("BOWLBUG_ROCK_0"));
}

#[test]
fn test_energy_written_as_a_number_and_an_icon_is_read() {
    let boost = card(0, "SUPERCRITICAL", "Supercritical", "Skill", 0, &format!("Gain 4{ICON}. Exhaust."), "Self");
    assert_eq!(parse(&boost).energy_gain, 4);
    assert_eq!(parse(&card(0, "TURBO", "TURBO", "Skill", 0, &format!("Gain {ICON}{ICON}. Add a Void into your Discard Pile."), "Self")).energy_gain, 2);
    // With it, the 3-cost attack that the energy in hand cannot pay for is played.
    let big = with(card(1, "METEOR", "Meteor", "Attack", 3, "Deal 30 damage.", "AnyEnemy"), json!({"can_play": false, "unplayable_reason": "EnergyCostTooHigh"}));
    assert_eq!(pick(&combat_state!([boost, big], [enemy!("JAW_WORM_0", 80)], energy = 1)), 0);
}

#[test]
fn test_borrowed_time_makes_the_cards_after_it_dearer() {
    let icon = "[necrobinder_energy_icon.png]";
    let time = parse(&card(0, "BORROWED_TIME", "Borrowed Time", "Skill", 1, &format!("Gain 4{icon}. Cards cost an additional {icon} this turn."), "Self"));
    let (strikes, kw): (Vec<CardInfo>, _) = ((1..5).map(|i| parse(&STRIKE(i))).collect(), ctx!(hp = 60, max_hp = 60));
    // 3 - 1 + 4 = 6 energy at 2 a Strike: three of them, not four.
    assert!(combat::simulate(&[vec![time.clone()], strikes[..3].to_vec()].concat(), &dummy(0, 0), &kw).is_some());
    assert!(combat::simulate(&[vec![time], strikes.clone()].concat(), &dummy(0, 0), &kw).is_none());
    assert!(combat::simulate(&strikes[..3], &dummy(0, 0), &kw).is_some());
}

#[test]
fn test_cards_that_pay_for_or_repeat_the_next_one() {
    let kw = ctx!(hp = 60, max_hp = 60);
    let value = |line: &[&CardInfo]| gain(&line.iter().map(|c| (*c).clone()).collect::<Vec<_>>(), &dummy(0, 0), &kw);
    let skill = |cid: &str, name: &str, text: &str, cost: i64| parse(&card(0, cid, name, "Skill", cost, text, "Self"));
    let (big, strike, guard) = (parse(&card(1, "METEOR", "Meteor", "Attack", 3, "Deal 30 damage.", "AnyEnemy")), parse(&STRIKE(2)), parse(&DEFEND(3)));
    let punch = skill("ONE_TWO_PUNCH", "One-Two Punch", "This turn, your next Attack is played an extra time.", 1);
    assert_eq!(value(&[&punch, &strike, &strike]), 18.0); // One-Two Punch: the next Attack twice, and only the next one
    assert_eq!(value(&[&punch, &guard, &strike]), 12.0); // (a Skill in between does not use it up)
    let burst = skill("BURST", "Burst", "This turn, your next Skill is played an extra time.", 1);
    assert_eq!(combat::simulate(&[burst, guard], &dummy(20, 1), &kw).unwrap().lost, 10); // Burst: the next Skill's Block twice
    let double = skill("DOUBLE_ENERGY", "Double Energy", "Double your Energy. Exhaust.", 1);
    assert!(combat::simulate(&[big.clone(), strike.clone()], &dummy(0, 0), &kw).is_none());
    assert_eq!(value(&[&double, &big, &strike]), 36.0); // Double Energy: 3 - 1 = 2, doubled, pays for the 3-cost card with one to spare
    let bullet = skill("BULLET_TIME", "Bullet Time", "You cannot draw additional cards this turn. ALL cards in your Hand are free to play this turn.", 3);
    assert_eq!(value(&[&bullet, &big, &strike, &strike]), 42.0); // Bullet Time: what is left of the hand costs nothing
}

#[test]
fn test_the_attack_after_unrelenting_costs_nothing() {
    let unrelenting = card(0, "UNRELENTING", "Unrelenting", "Attack", 2, "Deal 14 damage. The next Attack you play costs 0 [ironclad_energy_icon.png].", "AnyEnemy");
    let bludgeon = card(1, "BLUDGEON", "Bludgeon", "Attack", 3, "Deal 32 damage.", "AnyEnemy");
    let state = combat_state!([unrelenting.clone(), bludgeon.clone()], [enemy!("A", 200, attack = 0)], energy = 2);
    assert!(outcome(&state, &[unrelenting.clone(), bludgeon.clone()]).is_some());
    assert!(outcome(&state, &[bludgeon, unrelenting]).is_none()); // Bludgeon first is 3 energy
}

#[test]
fn test_a_power_other_players_win_with_is_worth_playing() {
    assert!(battle::_power_worth(&parse(&BARRICADE()), true) > 12.0); // (our own table says 4)
    assert_eq!(battle::_power_worth(&parse(&card(0, "NEW_POWER", "New Power", "Power", 2, "Something nobody has rated.", "Self")), true), 9.0);
    // A boss that is only buffing itself: the turn goes to Barricade, not to three Strikes.
    let quiet = combat_state!([BARRICADE(), STRIKE(1), STRIKE(2), STRIKE(3)], [enemy!("QUEEN_0", 400)], hp = 80, kind = "boss");
    assert_eq!(pick(&quiet), 0);
    // An ordinary fight is over before it pays: there the three Strikes go first, as they always did.
    assert_eq!(battle::_power_worth(&parse(&BARRICADE()), false), 4.0);
    assert_ne!(pick(&with(quiet, json!({"state_type": "monster"}))), 0);
}

#[test]
fn test_block_left_over_counts_once_it_stays() {
    let guard = card(0, "IMPERVIOUS", "Impervious", "Skill", 2, "Gain 30 Block. Exhaust.", "Self");
    let mut state = combat_state!([guard.clone()], [enemy!("A", 200, attack = 10)], energy = 2, hp = 40);
    let plain = outcome(&state, &[guard.clone()]).unwrap().score;
    state["player"]["status"] = json!([power("BARRICADE", 1)]);
    assert!(outcome(&state, &[guard]).unwrap().score > plain + 15.0); // 20 Block for next turn
}

#[test]
fn test_an_ethereal_power_is_played_while_it_can_be() {
    let echo = card(0, "ECHO_FORM", "Echo Form", "Power", 3, "Ethereal. The first card you play each turn is played an extra time.", "Self");
    assert!(parse(&echo).ethereal);
    let boss = |hp: i64| combat_state!([echo.clone(), STRIKE(1), STRIKE(2), DEFEND(3)], [enemy!("AEONGLASS_0", 500, attack = 22)], hp = hp, max_hp = 80, kind = "boss");
    assert_eq!(pick(&boss(75)), 0); // 22 damage taken for the rest of the fight doubled
    assert_ne!(pick(&boss(12)), 0);
}

#[test]
fn test_a_heal_is_worth_the_hp_it_gives_back() {
    let heal = parse(&card(0, "NOT_YET", "Not Yet", "Skill", 2, "Heal 10 HP. Exhaust.", "Self"));
    assert_eq!(heal.heal, 10);
    let (foe, hurt, full) = (dummy(0, 0), ctx!(hp = 20, max_hp = 60), ctx!(hp = 60, max_hp = 60));
    let none: [CardInfo; 0] = [];
    assert!(score(&[heal.clone()], &foe, &hurt) > score(&none, &foe, &hurt) + 20.0);
    assert!(score(&[heal.clone()], &foe, &full) < score(&none, &foe, &full) + 1.0);
    let foe = dummy(25, 1); // ... and it is there for the blow that comes this turn
    assert!(score(&[heal.clone()], &foe, &hurt) < 0.0); // 30 HP against 25: hurt badly
    assert!(score(&[heal], &foe, &hurt) > score(&none, &foe, &hurt) + 100_000.0); // ... not dead
}

#[test]
fn test_plating_counts_as_block() {
    let mut s = combat_state!([STRIKE(0), DEFEND(1)], [enemy!("A_0", 40, attack = 5)], energy = 1);
    assert_eq!(combat::expected_loss(&s), 5);
    s["player"]["status"] = json!([power("PLATING", 5)]);
    assert_eq!(combat::expected_loss(&s), 0);
    assert_eq!(pick(&s), 0); // nothing to block: attack
}

#[test]
fn test_respects_energy_and_unplayable_cards() {
    let s = combat_state!([card(0, "BIG", "Big", "Attack", 3, "Deal 30 damage.", "AnyEnemy"), with(STRIKE(1), json!({"can_play": false}))], [enemy!("A_0", 60)], energy = 2);
    assert!(combat::choose_card(&s, None).is_none());
    assert_eq!(combat::actions(&s), [("end_turn".to_string(), json!({}))]);
}

#[test]
fn test_potions_only_when_it_matters() {
    let fire = json!({"slot": 1, "id": "FIRE_POTION", "can_use_in_combat": true, "target_type": "AnyEnemy"});
    assert_eq!(combat::potion_actions(&combat_state!([STRIKE(0)], [enemy!("A_0", 40, attack = 5)], potions = [fire.clone()]), None, None), []);
    assert_eq!(
        combat::potion_actions(&combat_state!([STRIKE(0)], [enemy!("A_0", 40, attack = 5)], potions = [fire], kind = "boss"), None, None),
        [("use_potion".to_string(), json!({"slot": 1, "target": "A_0"}))]
    );
}

fn FOUL() -> Value {
    potion(0, "Foul Potion", "Deal 12 damage to ALL players and enemies. Can be thrown at the Merchant for 100 Gold instead.", "Self")
}

fn BLOOD() -> Value {
    potion(0, "Blood Potion", "Heal for 20% of your Max HP.", "Self")
}

fn FIRES() -> Vec<Value> {
    (0..3).map(FIRE).collect()
}

fn VULN() -> Value {
    potion(0, "Vulnerable Potion", "Apply 3 Vulnerable.", "AnyEnemy")
}

fn WEAK() -> Value {
    potion(0, "Weak Potion", "Apply 3 Weak.", "AnyEnemy")
}

/// (when, the belt, `boss` keywords, the slots drunk)
fn BELTS() -> Vec<(&'static str, Vec<Value>, Value, Vec<i64>)> {
    let aeonglass = |more: Value| with(json!({"foe": "AEONGLASS_0", "foe_hp": 500, "attack": 22, "floor": 48}), more);
    vec![
        ("Entropic Brew is drunk at once", vec![potion(0, "Entropic Brew", "Fill all your empty potion slots with random potions.", "Self"), STRENGTH(1)], json!({"floor": 5, "kind": "monster"}), vec![0]),
        ("Fruit Juice in the first fight, with nothing wrong and room on the belt", vec![potion(0, "Fruit Juice", "Gain 5 Max HP.", "Self"), STRENGTH(1)], json!({"floor": 5, "kind": "monster"}), vec![0]),
        ("... where the Strength Potion waits", vec![STRENGTH(1)], json!({"floor": 5, "kind": "monster"}), vec![]),
        ("a potion that hits us too waits for Block", vec![FOUL(), FIRE(1)], json!({}), vec![1]),
        ("... the Block that is up is for the 10 coming", vec![FOUL(), FIRE(1)], json!({"block": 12}), vec![1]),
        ("... and there is enough of it", vec![FOUL(), FIRE(1)], json!({"block": 22}), vec![0, 1]),
        ("healing waits until it is needed", vec![BLOOD()], json!({}), vec![]),
        ("... and then it goes", vec![BLOOD()], json!({"hp": 30}), vec![0]),
        ("a full belt has one to spare", FIRES(), json!({"floor": 20, "kind": "monster"}), vec![0]),
        ("... but not two rooms from the boss", FIRES(), json!({"floor": 31, "kind": "monster"}), vec![]),
        ("... nor in a fight that is over anyway", FIRES(), json!({"floor": 33, "foe_hp": 20}), vec![]),
        ("an elite gets the spare one", FIRES(), json!({"floor": 22, "kind": "elite"}), vec![0]),
        ("... nothing of a belt with room", FIRES()[..2].to_vec(), json!({"floor": 22, "kind": "elite"}), vec![]),
        ("... nor with the boss two rooms on", FIRES(), json!({"floor": 31, "kind": "elite"}), vec![]),
        ("... unless the fight has gone wrong", FIRES()[..2].to_vec(), json!({"floor": 24, "kind": "elite", "hp": 35}), vec![0, 1]),
        ("the boss gets all of them", FIRES()[..2].to_vec(), json!({"floor": 33}), vec![0, 1]),
        (
            "the Test Subject's first form: our Strength lasts, its Vulnerable would not",
            vec![VULN(), STRENGTH(1)],
            json!({"foe": "TEST_SUBJECT_0", "foe_hp": 100, "floor": 48, "powers": [power("ADAPTABLE", 1), power("ENRAGE", 2)]}),
            vec![1],
        ),
        ("... its second form", vec![VULN(), STRENGTH(1)], json!({"foe": "TEST_SUBJECT_0", "foe_hp": 100, "floor": 48, "powers": [power("ADAPTABLE", 1), power("PAINFUL_STABS", 1)]}), vec![0, 1]),
        ("... Intangible: 20 damage would take 1 HP", vec![FIRE(0)], json!({"foe": "TEST_SUBJECT_0", "floor": 3, "powers": [power("INTANGIBLE", 1)]}), vec![]),
        ("a debuff does not go into Artifact", vec![WEAK(), STRENGTH(1)], aeonglass(json!({"powers": [power("ARTIFACT", 3)]})), vec![1]),
        ("... without it, it does", vec![WEAK(), STRENGTH(1)], aeonglass(json!({})), vec![0, 1]),
        ("... and about to die nothing is being saved", vec![WEAK(), STRENGTH(1)], aeonglass(json!({"powers": [power("ARTIFACT", 3)], "hp": 20})), vec![0, 1]),
    ]
}

// Potions

/// `boss(belt, hp=75, block=0, foe_hp=300, floor=33, kind="boss", foe="A", attack=10, powers=())`, the keywords as a dict.
fn boss(belt: Vec<Value>, kw: &Value) -> Vec<i64> {
    let or = |k: &str, default: Value| kw.get(k).cloned().unwrap_or(default);
    let foe = enemy!(or("foe", json!("A")).as_str().unwrap(), or("foe_hp", json!(300)).as_i64().unwrap(), attack = or("attack", json!(10)), status = or("powers", json!([])));
    let state = combat_state!([STRIKE(0)], [foe], hp = or("hp", json!(75)), max_hp = 75, block = or("block", json!(0)), kind = or("kind", json!("boss")), potions = belt, floor = or("floor", json!(33)));
    slots(&combat::potion_actions(&state, None, None))
}

#[test]
fn test_which_potions_are_drunk() {
    for (why, belt, kw, want) in BELTS() {
        assert_eq!(boss(belt, &kw), want, "{why}");
    }
}

#[test]
fn test_a_belt_that_refills_every_fight_is_emptied_every_fight() {
    let mut state = combat_state!([STRIKE(0)], [enemy!("A", 60, attack = 10)], hp = 75, max_hp = 75, potions = [STRENGTH(0), FIRE(1)], floor = 40, max_potion_slots = 3);
    assert_eq!(combat::potion_actions(&state, None, None), []); // an ordinary fight, nothing wrong
    state["player"]["relics"] = json!([{"id": "DELICATE_FROND", "description": "At the start of each combat, fill all empty potion slots with random Potions."}]);
    assert_eq!(sorted(slots(&combat::potion_actions(&state, None, None))), [0, 1]);
}

#[test]
fn test_a_big_belt_keeps_three_for_the_boss_and_drinks_the_rest_on_the_way() {
    // Six slots and five held was never "full", and a Regent lost 37 HP in two fights holding all five. The two a boss needs least go on the way; near the boss none.
    let belt = [
        STRENGTH(0),
        potion(1, "Swift Potion", "Draw 3 cards.", "Self"),
        FIRE(2),
        potion(3, "Attack Potion", "Add 1 of 3 random Attack cards into your Hand.", "Self"),
        potion(4, "Focus Potion", "Gain 2 Focus.", "Self"),
    ];
    let mut state = combat_state!([STRIKE(0)], [enemy!("A", 60, attack = 10)], hp = 60, max_hp = 75, potions = belt, floor = 21, max_potion_slots = 6);
    assert_eq!(sorted(slots(&combat::potion_actions(&state, None, None))), [1, 3]);
    state["run"]["floor"] = json!(30); // three floors before the boss: all five are for it
    assert_eq!(combat::potion_actions(&state, None, None), []);
}

#[test]
fn test_the_potion_drunk_on_the_way_is_the_one_a_boss_needs_least() {
    // A full belt cannot take the next potion, so one goes in an ordinary fight: not the Strength Potion.
    let strength = STRENGTH(0);
    let mut state = combat_state!([STRIKE(0)], [enemy!("A", 60, attack = 10)], hp = 75, max_hp = 75, potions = [strength.clone(), potion(1, "Swift Potion", "Draw 3 cards.", "Self")], floor = 22, max_potion_slots = 2);
    assert_eq!(slots(&combat::potion_actions(&state, None, None)), [1]);
    // ... and none at all once the boss is near (nine floors in act 2): a slot emptied there often stays empty.
    state["run"]["floor"] = json!(24);
    assert_eq!(combat::potion_actions(&state, None, None), []);
    state["run"]["floor"] = json!(23);
    assert_eq!(combat::potion_actions(&state, None, None).len(), 1);
    assert!(potions::boss_worth(&strength) > potions::boss_worth(&potion(0, "Fire Potion", "Deal 20 damage.", "Self")));
    assert_eq!(potions::boss_worth(&json!({"potion_name": "Odd Vial", "potion_description": "Deal 31 damage."})), 31.0); // (unlisted: from its text)
    assert!(potions::boss_worth(&json!({"name": "Odd Tonic", "description": "Gain 3 Strength."})) > 20.0);
}

#[test]
fn test_no_potion_is_poured_out_at_a_sleeping_boss() {
    let energy = potion(0, "Energy Potion", "Gain [defect_energy_icon.png][defect_energy_icon.png].", "Self");
    let sleeper = enemy!("LAGAVULIN_MATRIARCH_0", 222, status = [power("ASLEEP", 3)]);
    let mut state = combat_state!((0..5).map(STRIKE).collect::<Vec<_>>(), [sleeper], kind = "boss", potions = [energy, STRENGTH(1)]);
    assert_eq!(combat::potion_actions(&state, None, None), []);
    state["battle"]["enemies"][0]["status"][0]["amount"] = json!(1); // the turn before she wakes: now they count
    let mut plan = Plan::default();
    combat::choose_card(&state, Some(&mut plan));
    assert_eq!(sorted(slots(&combat::potion_actions(&state, plan.lost, plan.line.as_deref()))), [0, 1]);
}

#[test]
fn test_an_energy_potion_waits_for_a_hand_that_can_spend_it() {
    let energy = potion(0, "Energy Potion", "Gain [ironclad_energy_icon.png][ironclad_energy_icon.png].", "Self");
    let first = |hand: Vec<Value>, foe: Value| combat::actions(&combat_state!(hand, [foe], kind = "boss", potions = [energy.clone()])).remove(0);
    assert_eq!(first(vec![STRIKE(0), STRIKE(1), DEFEND(2), DEFEND(3), DEFEND(4)], enemy!("A", 300)).0, "play_card"); // (a boss that only buffs: no use for the Defends)
    let big = card(3, "BLUDGEON", "Bludgeon", "Attack", 3, "Deal 32 damage.", "AnyEnemy");
    let drunk = ("use_potion".to_string(), json!({"slot": 0}));
    assert_eq!(first(vec![STRIKE(0), STRIKE(1), DEFEND(2), big], enemy!("A", 300)), drunk); // ... an attack three energy cannot pay for: drunk
    assert_eq!(first((0..5).map(DEFEND).collect(), enemy!("A", 300, attack = 30)), drunk); // ... and for Block when the plan still loses HP
}

#[test]
fn test_no_potion_when_the_hand_already_ends_the_danger() {
    let belt = [STRENGTH(0), potion(1, "Foul Potion", "Deal 12 damage to ALL players and enemies.", "Self")];
    let drunk = |state: Value| combat::actions(&state).into_iter().filter(|a| a.0 == "use_potion").collect::<Vec<_>>();
    // two Strikes kill it: nothing is coming
    assert_eq!(drunk(combat_state!([STRIKE(0), STRIKE(1)], [enemy!("A", 12, attack = 20)], max_hp = 90, kind = "monster", potions = belt.clone(), hp = 31, floor = 30)), []);
    // out of energy behind enough Block: nothing is coming either
    assert_eq!(drunk(combat_state!([STRIKE(0)], [enemy!("A", 60, attack = 12)], max_hp = 90, kind = "monster", potions = belt.clone(), energy = 0, hp = 20, block = 17, floor = 24)), []);
    // 20 of 31 HP is coming and the hand cannot stop it
    assert!(!drunk(combat_state!([STRIKE(0), STRIKE(1)], [enemy!("A", 60, attack = 20)], max_hp = 90, kind = "monster", potions = belt, hp = 31, floor = 30)).is_empty());
}

#[test]
fn test_a_potion_waits_for_the_moment_it_does_something() {
    // (the original's defaults: line None, block 0, attack 30, discard 0.)
    fn used(drink: &Value, hand: Vec<Value>, line: Option<Vec<CardInfo>>, block: i64, attack: i64, discard: i64) -> bool {
        let state = combat_state!(hand, [enemy!("A", 300, attack = attack)], hp = 75, max_hp = 75, block = block, kind = "boss", potions = [drink.clone()], floor = 33, discard_pile_count = discard);
        !combat::potion_actions(&state, None, line.as_deref()).is_empty()
    }
    // "Triple your Block": 11 of 14 tripled nothing. It goes once the turn's Block is up and is still short.
    let fortifier = potion(0, "Fortifier", "Triple your Block.", "Self");
    assert!(!used(&fortifier, vec![DEFEND(0)], Some(vec![parse(&DEFEND(0))]), 0, 30, 0));
    assert!(!used(&fortifier, vec![], Some(vec![]), 0, 30, 0));
    assert!(used(&fortifier, vec![], Some(vec![]), 10, 30, 0));
    assert!(!used(&fortifier, vec![], Some(vec![]), 10, 8, 0));
    // "Put a card from your Discard Pile into your Hand": not on round 1, when there is none.
    let memories = potion(0, "Liquid Memories", "Put a card from your Discard Pile into your Hand. It's free to play this turn.", "Self");
    assert!(!used(&memories, vec![STRIKE(0)], None, 0, 30, 0));
    assert!(used(&memories, vec![STRIKE(0)], None, 0, 30, 6));
    // Strength for one turn is for a turn of attacks (it was kept for the turns we Block).
    let flex = potion(0, "Flex Potion", "Gain 5 Strength. At the end of your turn, lose 5 Strength.", "Self");
    assert!(!used(&flex, vec![STRIKE(0), DEFEND(1), DEFEND(2)], Some(vec![parse(&DEFEND(1)), parse(&DEFEND(2))]), 0, 30, 0));
    assert!(used(&flex, vec![STRIKE(0), STRIKE(1), STRIKE(2)], Some((0..3).map(|i| parse(&STRIKE(i))).collect()), 0, 30, 0));
    // "Exhaust your Hand. Draw 10 cards": for a hand of dead cards, not for the opening one.
    let glow = potion(0, "Glowwater Potion", "Exhaust your Hand. Draw 10 cards.", "Self");
    let wound = |i: i64| card(i, "WOUND", "Wound", "Status", 0, "Unplayable.", "None");
    assert!(!used(&glow, vec![STRIKE(0), DEFEND(1), STRIKE(2)], None, 0, 30, 0));
    assert!(used(&glow, vec![STRIKE(0), wound(1), wound(2)], None, 0, 30, 0));
    // Snecko Oil is drunk like any other (the costs it rolls are in the state).
    assert!(used(&potion(0, "Snecko Oil", "Draw 7 cards. Randomize the cost of cards in your Hand this turn.", "Self"), vec![STRIKE(0)], None, 0, 30, 0));
    // "Your next card is played an extra time": kept for a card worth playing twice, and that card is the one played next.
    let twice = potion(0, "Duplicator", "This turn, your next card is played an extra time.", "Self");
    let heavy = card(1, "BLUDGEON", "Bludgeon", "Attack", 2, "Deal 14 damage.", "AnyEnemy");
    assert!(!used(&twice, vec![STRIKE(0)], Some(vec![parse(&STRIKE(0))]), 0, 30, 0));
    assert!(used(&twice, vec![STRIKE(0), heavy.clone()], Some(vec![parse(&STRIKE(0)), parse(&heavy)]), 0, 30, 0));
    assert_eq!(pick(&combat_state!([STRIKE(0), heavy], [enemy!("A", 300)], energy = 3, kind = "boss", status = [power("DUPLICATION", 1)])), 1);
}

// PowersThePlannerKnows

#[test]
fn test_vigor_only_helps_the_first_attack() {
    // With 8 Vigor each Strike reads "Deal 14 damage", but only the first one gets it: 14 + 6 + 6 does not kill 30 HP.
    let strike = |i: i64| card(i, "STRIKE_R", "Strike", "Attack", 1, "Deal 14 damage.", "AnyEnemy");
    let state = combat_state!([strike(0), strike(1), strike(2), DEFEND(3)], [enemy!("A", 30, attack = 30)], hp = 20, status = [power("VIGOR", 8)]);
    assert!(outcome(&state, &state["player"]["hand"].as_array().unwrap()[..3]).unwrap().score < 0.0); // the enemy lives and we do not
}

#[test]
fn test_tender_weakens_every_later_card() {
    let mut state = combat_state!([DEFEND(0), DEFEND(1), DEFEND(2)], [enemy!("A", 90, attack = 14)], hp = 10);
    let hand = state["player"]["hand"].as_array().unwrap().clone();
    let plain = outcome(&state, &hand).unwrap();
    state["player"]["status"] = json!([power("TENDER", 0)]);
    assert!(outcome(&state, &hand).unwrap().score < plain.score); // 5 + 4 + 3 Block, not 15
}

#[test]
fn test_stars_made_in_a_line_pay_for_a_later_card() {
    let star = with(card(0, "FALLING_STAR", "Falling Star", "Attack", 0, "Deal 8 damage. Apply 1 Weak. Apply 1 Vulnerable.", "AnyEnemy"), json!({"star_cost": 2, "can_play": false, "unplayable_reason": "StarCostTooHigh"}));
    let venerate = card(1, "VENERATE", "Venerate", "Skill", 1, &format!("Gain {STAR}{STAR}."), "Self");
    let mut state = combat_state!([star, venerate, STRIKE(2), STRIKE(3)], [enemy!("A", 28)], energy = 2, stars = 1);
    let venerated = Some(("play_card".to_string(), json!({"card_index": 1})));
    assert_eq!(combat::choose_card(&state, None), venerated);
    state["player"]["hand"][0]["unplayable_reason"] = json!("BlockedByHook, StarCostTooHigh"); // ... but a card that is unplayable for another reason stays out
    assert_ne!(combat::choose_card(&state, None), venerated);
}

#[test]
fn test_withering_presence_makes_small_cards_not_worth_playing() {
    let poke = card(0, "POKE", "Poke", "Attack", 0, "Deal 3 damage.", "AnyEnemy");
    assert!(combat::choose_card(&combat_state!([poke.clone()], [enemy!("A", 400, status = [power("WITHERING_PRESENCE", 4)])], kind = "boss"), None).is_none());
    assert!(combat::choose_card(&combat_state!([poke, STRIKE(1)], [enemy!("A", 400)], kind = "boss"), None).is_some());
}

#[test]
fn test_crabs_are_brought_down_together() {
    let crabs = |crusher: i64, rocket: i64| [enemy!("CRUSHER", crusher, attack = 6, status = [power("CRAB_RAGE", 1)]), enemy!("ROCKET", rocket, attack = 6, status = [power("CRAB_RAGE", 1)])];
    // Killing the Crusher now would enrage a Rocket with 120 HP left: hit the Rocket. Both within reach: take them together.
    assert_eq!(aimed(&combat_state!([STRIKE(0)], crabs(6, 120), energy = 1, kind = "boss")).as_deref(), Some("ROCKET"));
    assert!(combat::choose_card(&combat_state!([STRIKE(0), STRIKE(1)], crabs(6, 6), energy = 2, kind = "boss"), None).is_some());
}

#[test]
fn test_drawing_is_worth_what_is_left_to_draw() {
    let mut state = combat_state!([card(0, "SKIM", "Skim", "Skill", 1, "Draw 3 cards.", "Self"), STRIKE(1)], [enemy!("A", 200)], energy = 1);
    state["player"]["draw_pile"] = json!(vec![json!({"name": "Wound", "cost": "0", "description": "Unplayable."}); 6]);
    assert_eq!(pick(&state), 1); // nothing worth drawing: Strike
    state["player"]["draw_pile"] = json!(vec![json!({"name": "Claw", "cost": "0", "description": "Deal 9 damage."}); 6]);
    assert_eq!(pick(&state), 0); // three free attacks beat one Strike
}

#[test]
fn test_orichalcum_and_regen_are_counted() {
    let mut state = combat_state!([], [enemy!("A", 100, attack = 18)], hp = 60, max_hp = 80);
    assert_eq!(combat::expected_end_loss(&state), Some(18));
    state["player"]["relics"] = json!([{"id": "ORICHALCUM", "description": "If you end your turn without Block, gain 6 Block."}]);
    assert_eq!(combat::expected_end_loss(&state), Some(12));
    state["player"]["status"] = json!([power("REGEN", 5)]);
    assert_eq!(combat::expected_end_loss(&state), Some(7));
    state["player"]["block"] = json!(3); // some Block already: the relic stays out of it
    assert_eq!(combat::expected_end_loss(&state), Some(10));
}

#[test]
fn test_deck_is_read_at_the_start_of_a_fight_only() {
    let mut bot = crate::helpers::make_bot(crate::fake_game::FakeGame::default().shared());
    let mut state = combat_state!([STRIKE(0), DEFEND(1)], [enemy!("A", 30, attack = 5)], draw_pile_count = 2, draw_pile = json!([{"name": "Bash", "cost": "2"}, {"name": "Strike", "cost": "1"}]));
    bot._combat(&state);
    assert_eq!(bot.mem.deck_size, 4);
    // Later in the fight the piles hold Wounds and other things that are not in the deck.
    state["battle"]["round"] = json!(3);
    (state["player"]["discard_pile_count"], state["player"]["draw_pile_count"], state["player"]["draw_pile"]) = (json!(9), json!(1), json!([{"name": "Wound"}]));
    bot._combat(&state);
    assert_eq!(bot.mem.deck_size, 4);
}

#[test]
fn test_star_cards_are_worth_less_without_stars_to_pay_for_them() {
    let comet = with(card(0, "COMET", "Comet", "Attack", 0, "Deal 33 damage.", "AnyEnemy"), json!({"star_cost": 5}));
    let pulse = with(card(0, "ASTRAL_PULSE", "Astral Pulse", "Attack", 0, "Deal 6 damage to ALL enemies twice.", "AllEnemies"), json!({"star_cost": 3}));
    let glow = card(0, "GLOW", "Glow", "Skill", 1, &format!("Gain {STAR}. Draw 1 card."), "Self");
    let deck: Vec<Value> = (0..4).map(STRIKE).chain((0..4).map(DEFEND)).chain([pulse.clone(), pulse]).collect();
    // (strategy.deck_fit is decks.deck_fit)
    assert!(decks::deck_fit(&comet, &deck, None, 3) < -2.0);
    assert!(decks::deck_fit(&glow, &deck, None, 3) > 0.5);
    assert_eq!(decks::deck_fit(&glow, &[STRIKE(0)], None, 3), 0.0);
}

#[test]
fn test_the_stars_a_fight_starts_with_do_not_pay_for_every_cycle() {
    // The starter deck (Falling Star 2, Venerate +2) with one Astral Pulse: a third star card would ask for 8 Stars a cycle of a deck that makes 2 (and has 3 once, when the fight starts).
    let star = |cid: &str, name: &str, n: i64| with(card(0, cid, name, "Attack", 0, "Deal 8 damage.", "AnyEnemy"), json!({"star_cost": n}));
    let venerate = card(0, "VENERATE", "Venerate", "Skill", 1, &format!("Gain {STAR}{STAR}."), "Self");
    let mut deck: Vec<Value> = (0..4).map(STRIKE).chain((0..4).map(DEFEND)).chain([star("FALLING_STAR", "Falling Star", 2), venerate, star("ASTRAL_PULSE", "Astral Pulse", 3)]).collect();
    assert!(decks::deck_fit(&star("GAMMA_BLAST", "Gamma Blast", 3), &deck, None, 3) < -2.0);
    let genesis = card(0, "GENESIS", "Genesis", "Power", 2, &format!("At the start of your turn, gain {STAR}{STAR}."), "Self");
    deck.push(genesis);
    assert!(decks::deck_fit(&star("GAMMA_BLAST", "Gamma Blast", 3), &deck, None, 3) > -0.1); // Genesis pays for it
}

// OtherCharacters
// What the Silent, Regent, Necrobinder and Defect do that the Ironclad does not.

#[test]
fn test_poison_is_worth_more_on_a_stack_and_in_a_long_fight() {
    assert_eq!(combat::_poison_total(5, 7), 15); // 5 + 4 + 3 + 2 + 1
    assert_eq!(combat::_poison_total(15, 7) - combat::_poison_total(10, 7), 35); // 5 more for seven turns
    let stack = |n: i64, fight: &str| {
        let status = if n != 0 { vec![power("POISON", n)] } else { vec![] };
        gain(&[parse(&DEADLY())], &read_foes(&[enemy!("A", 300, status = status)]), &ctx!(energy = 1, fight = fight))
    };
    assert!(stack(10, "boss") > 2.0 * stack(0, "boss"));
    assert!(stack(0, "boss") > stack(0, "monster"));
}

#[test]
fn test_a_plasma_orb_is_evoked_to_pay_for_more_cards() {
    // Seen against the Insatiable: 1 energy, a Plasma orb in front, and Ball Lightning played alone. Dualcast first is 4 energy: both attacks.
    let hand = vec![
        card(0, "BALL_LIGHTNING", "Ball Lightning+", "Attack", 1, "Deal 10 damage. Channel 1 Lightning.", "AnyEnemy"),
        card(1, "DUALCAST", "Dualcast", "Skill", 1, "Evoke your rightmost Orb twice.", "Self"),
        card(2, "EXTERMINATE", "Exterminate", "Attack", 1, "Deal 4 damage to ALL enemies 4 times.", "AllEnemies"),
        card(3, "STRIKE_D", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy"),
    ];
    let mut state = combat_state!(hand.clone(), [enemy!("THE_INSATIABLE_0", 161, attack = "10x2")], energy = 1, hp = 35, max_hp = 75, kind = "boss", orbs = [json!({"id": "PLASMA_ORB", "passive_val": 1, "evoke_val": 2})], orb_slots = 3);
    assert_eq!(pick(&state), 1);
    state["player"]["hand"] = json!(hand[..2]); // with nothing to spend it on, the orb and its energy every turn are kept
    assert_eq!(pick(&state), 0);
}

#[test]
fn test_frost_orbs_count_as_block() {
    let mut state = combat_state!([DEFEND(0), STRIKE(1)], [enemy!("A", 100, attack = 7)], energy = 1, hp = 10);
    assert_eq!(pick(&state), 0); // must block: 7 would hurt
    state["player"]["orbs"] = json!(vec![json!({"id": "FROST_ORB", "name": "Frost", "passive_val": 2, "evoke_val": 5}); 4]);
    state["player"]["orb_slots"] = json!(4);
    assert_eq!(pick(&state), 1); // the orbs block it; attack
}

#[test]
fn test_evoke_takes_the_orb_channeled_first() {
    // Orbs come oldest first, and "your rightmost Orb" is that oldest one.
    let dualcast = parse(&card(0, "DUALCAST", "Dualcast", "Skill", 0, "Evoke your rightmost Orb twice.", "Self"));
    assert_eq!(dualcast.evoke, 2);
    assert_eq!(parse(&card(0, "QUADCAST", "Quadcast", "Skill", 1, "Evoke your rightmost Orb 4 times.", "None")).evoke, 4);
    let (frost, glass) = (orb("FROST", 2, 5), orb("GLASS", 4, 8));
    let (foes, kw) = (read_foes(&[enemy!("A", 100, attack = 20)]), ctx!(energy = 0, hp = 19, max_hp = 100, block = 6, fight = "boss", orb_slots = 3));
    let frost_first = score(&[dualcast.clone()], &foes, &SimulateArgs { orbs: vec![frost.clone(), glass.clone(), glass.clone()], ..kw.clone() });
    assert!(frost_first > score(&[dualcast], &foes, &SimulateArgs { orbs: vec![glass.clone(), glass, frost], ..kw.clone() }) + 20.0); // 10 Block that keeps us alive against 16 damage
}

#[test]
fn test_shatter_sets_off_every_orb() {
    let shatter = parse(&card(0, "SHATTER", "Shatter", "Attack", 1, "Deal 7 damage to ALL enemies. Evoke all of your Orbs twice. Exhaust.", "AllEnemies"));
    assert_eq!(shatter.evoke_all, 2);
    let (foes, kw) = (read_foes(&[enemy!("A", 200, attack = 30)]), ctx!(energy = 1, hp = 40, orb_slots = 3));
    let loaded = score(&[shatter.clone()], &foes, &SimulateArgs { orbs: vec![orb("LIGHTNING", 3, 8), orb("FROST", 2, 5)], ..kw.clone() });
    assert!(loaded > score(&[shatter], &foes, &kw) + 30.0); // 16 damage and 10 Block on top
}

#[test]
fn test_a_kill_by_orb_is_a_kill() {
    let dualcast = card(0, "DUALCAST", "Dualcast", "Skill", 1, "Evoke your rightmost Orb twice.", "Self");
    let orbs = json!([{"id": "LIGHTNING_ORB", "name": "Lightning", "passive_val": 3, "evoke_val": 8}]);
    let state = combat_state!([dualcast.clone(), DEFEND(1)], [enemy!("A", 14, attack = 30)], energy = 1, hp = 20, orbs = orbs.clone(), orb_slots = 3);
    assert_eq!(combat::choose_card(&state, None), Some(("play_card".to_string(), json!({"card_index": 0})))); // 16 kills it: no need to block
    // Vulnerable does nothing for orbs: 17 HP is out of reach, so it blocks (and lives, on 26 HP).
    let state = combat_state!([dualcast, DEFEND(1)], [enemy!("A", 17, attack = 30, status = [power("VULNERABLE", 2)])], energy = 1, hp = 26, orbs = orbs, orb_slots = 3);
    assert_eq!(combat::choose_card(&state, None), Some(("play_card".to_string(), json!({"card_index": 1}))));
}

#[test]
fn test_focus_strengthens_the_orbs_a_card_channels() {
    let (zap, foes, kw) = ([parse(&ZAP())], read_foes(&[enemy!("A", 100)]), ctx!(energy = 1, hp = 50, orb_slots = 3));
    assert!(score(&zap, &foes, &SimulateArgs { focus_power: 3, ..kw.clone() }) > score(&zap, &foes, &kw));
}

#[test]
fn test_osty_takes_the_hit() {
    let mut state = combat_state!([DEFEND(0), STRIKE(1)], [enemy!("A", 100, attack = 8)], energy = 1, hp = 80, pets = [json!({"id": "OSTY", "alive": true, "hp": 9, "max_hp": 9})]);
    assert_eq!(combat::expected_end_loss(&state), Some(0));
    assert_eq!(pick(&state), 1);
    state["player"]["hp"] = json!(10); // ... but what he soaks now he cannot soak later: badly hurt, the Defend keeps him standing
    assert_eq!(pick(&state), 0);
}

#[test]
fn test_regret_costs_a_point_for_every_card_still_held() {
    let regret = with(card(0, "REGRET", "Regret", "Curse", 0, "Unplayable. At the end of your turn, if this is in your Hand, lose 1 HP for each card in your Hand.", "None"), json!({"can_play": false}));
    let mut state = combat_state!([regret, DEFEND(1), DEFEND(2), STRIKE(3)], [enemy!("A", 100)], energy = 0, hp = 30);
    assert_eq!(combat::expected_end_loss(&state), Some(4)); // (it was read as "lose 1 HP")
    state["player"]["energy"] = json!(1);
    assert!(combat::choose_card(&state, None).is_some()); // a card played is a point saved
}

#[test]
fn test_osty_does_not_stop_what_burns_in_the_hand() {
    let toxic = |i: i64| card(i, "TOXIC", "Toxic", "Status", 1, "At the end of your turn, if this is in your Hand, take 5 damage. Exhaust.", "None");
    let mut state = combat_state!([toxic(0), toxic(1)], [enemy!("A", 100, attack = 6)], energy = 0, hp = 21, max_hp = 75, pets = [json!({"id": "OSTY", "alive": true, "hp": 20, "max_hp": 20})]);
    assert_eq!(combat::expected_end_loss(&state), Some(10)); // he takes the attack; the Toxics are ours
    state["player"]["block"] = json!(7);
    assert_eq!(combat::expected_end_loss(&state), Some(3)); // our own Block does stop them
    state["player"]["pets"][0]["hp"] = json!(2);
    assert_eq!(combat::expected_end_loss(&state), Some(7)); // ... and then there is none left for the attack he cannot take whole
}

#[test]
fn test_strength_given_up_for_good_is_counted() {
    // Two Friendships went down on sight, and Strikes hit for 2 with 7 energy and nothing to spend it on.
    let friendship = |i: i64| card(i, "FRIENDSHIP", "Friendship", "Power", 1, "Lose 2 Strength. Gain [necrobinder_energy_icon.png] at the start of each turn.", "None");
    let info = parse(&friendship(0));
    assert_eq!((info.strength, info.strength_lasts), (-2, true));
    let flex = parse(&card(0, "FLEX", "Flex", "Skill", 0, "Gain 2 Strength. At the end of your turn, lose 2 Strength.", "None"));
    assert_eq!(flex.strength, 2); // (given back at the end of the turn is not a price paid)
    let (foes, kw) = (read_foes(&[enemy!("A", 300)]), ctx!(hp = 70, fight = "boss"));
    let none: [CardInfo; 0] = [];
    let (nothing, first) = (score(&none, &foes, &kw), score(&[info.clone()], &foes, &kw));
    assert!(first > nothing); // an energy every turn of a boss fight is worth 2 Strength
    assert!(score(&[info.clone()], &foes, &SimulateArgs { friends: 1, ..kw.clone() }) < nothing); // ... a second one is not
    assert!(score(&[info.clone(), parse(&friendship(1))], &foes, &kw) < first);
    let strike = parse(&STRIKE(2)); // in the same line the Strength is gone for the attacks after it
    assert!(score(&[info, strike.clone()], &foes, &kw) - first < score(&[strike], &foes, &kw) - nothing);
}

#[test]
fn test_strength_taken_off_an_enemy_for_good_is_for_the_one_that_hits_often() {
    let info = parse(&card(0, "SHARED_FATE", "Shared Fate", "Skill", 0, "Lose 2 Strength. Enemy loses 2 Strength. Exhaust.", "AnyEnemy"));
    assert_eq!((info.strength, info.enemy_strength_loss, info.enemy_strength_lasts), (-2, 2, true));
    assert!(!parse(&card(0, "PIERCING_WAIL", "Piercing Wail", "Skill", 1, "ALL enemies lose 6 Strength this turn. Exhaust.", "None")).enemy_strength_lasts);
    let kw = ctx!(hp = 70, block = 30, fight = "boss"); // (behind Block: only what lasts is being weighed)
    let against = |attack: &str| gain(&[info.clone()], &read_foes(&[enemy!("A", 300, attack = attack)]), &kw);
    assert!(against("8x3") > 0.0); // 6 off every such turn for our 2 Strength
    assert!(against("20") < 0.0); // 2 off a single hit is not worth them
    assert!(against("8x3") > against("20"));
}

#[test]
fn test_stars_pay_for_star_cards() {
    let venerate = card(0, "VENERATE", "Venerate", "Skill", 1, &format!("Gain {STAR}{STAR}."), "None");
    let falling = with(card(1, "FALLING_STAR", "Falling Star", "Attack", 0, "Deal 8 damage. Apply 1 Weak. Apply 1 Vulnerable.", "AnyEnemy"), json!({"star_cost": "2"}));
    let (foes, kw) = (read_foes(&[enemy!("A", 100)]), ctx!(energy = 1, hp = 80));
    assert!(combat::simulate(&[parse(&falling)], &foes, &kw).is_none()); // no Stars yet
    assert!(combat::simulate(&[parse(&venerate), parse(&falling)], &foes, &kw).is_some());
}

#[test]
fn test_more_stars_are_worth_little_once_plenty_are_held() {
    let venerate = card(0, "VENERATE", "Venerate", "Skill", 1, &format!("Gain {STAR}{STAR}."), "Self");
    let held = |stars: i64| worth(&combat_state!([venerate.clone()], [enemy!("A", 60, attack = 0)], energy = 1, stars = stars), &[venerate.clone()]);
    assert!(held(0) > 4.0);
    assert!(held(12) < 1.0);
}

#[test]
fn test_their_key_cards_are_rated() {
    let fumes = with(card(0, "NOXIOUS_FUMES", "Noxious Fumes", "Power", 1, "At the start of your turn, apply 2 Poison to ALL enemies.", "None"), json!({"rarity": "Uncommon"}));
    assert!(deck_value(&fumes) > 5.0);
    assert!(deck_value(&fumes) > deck_value(&card(1, "SLICE", "Slice", "Attack", 0, "Deal 6 damage.", "AnyEnemy")));
}

// RageAndRespawn

/// (the original's defaults: the Crusher on 6, the Rocket on 120.)
fn crabs(crusher_hp: i64, rocket_hp: i64) -> Vec<Enemy> {
    let crab = |eid: &str, hp: i64| Enemy { entity_id: eid.into(), hp, incoming: 6, rage: true, ..Default::default() };
    vec![crab("CRUSHER_0", crusher_hp), crab("ROCKET_0", rocket_hp)]
}

#[test]
fn test_a_surviving_crab_gets_block_once_per_ally_that_died() {
    let (mut foes, mut known) = (crabs(0, 120), HashSet::new());
    combat::_rage_blocks(&mut foes, &mut known);
    combat::_rage_blocks(&mut foes, &mut known); // the same death is not counted twice
    assert_eq!(foes[1].block, combat::RAGE_BLOCK.get());
    assert_eq!(known, HashSet::from([Arc::<str>::from("CRUSHER_0")]));
}

#[test]
fn test_hitting_the_second_crab_after_the_first_dies_is_worth_less() {
    let (line, kw) = ([parse(&STRIKE(0)), parse(&STRIKE(1))], ctx!(energy = 2, hp = 80, fight = "boss")); // kill the Crusher, then hit the Rocket
    let with_rage = score(&line, &crabs(6, 120), &kw);
    let _none = battle::RAGE_BLOCK.set(0);
    assert!(with_rage < score(&line, &crabs(6, 120), &kw));
}

/// (the original's defaults: 5 HP, Stock 2.)
fn axebot(hp: i64, respawns: i64) -> Enemy {
    Enemy { entity_id: "AXEBOT_0".into(), hp, incoming: 15, respawns, max_hp: 85, ..Default::default() }
}

#[test]
fn test_a_stock_enemy_comes_back_at_full_hp_while_it_has_stock() {
    let mut foes = [axebot(0, 1)];
    combat::_respawns(&mut foes); // first death: back at full HP, one stack used
    assert_eq!((foes[0].alive(), foes[0].hp, foes[0].respawns), (true, 85, 0));
    foes[0].hp = 0;
    combat::_respawns(&mut foes); // second death, no stock left: really dead
    assert!(!foes[0].alive());
}

#[test]
fn test_killing_a_stock_enemy_is_not_a_win() {
    let (line, kw) = ([parse(&STRIKE(0))], ctx!(energy = 1, hp = 80, fight = "boss")); // 6 damage on a 5-HP Axebot
    let with_stock = score(&line, &[axebot(5, 2)], &kw);
    assert!(with_stock < score(&line, &[axebot(5, 0)], &kw) - combat::WIN_BONUS.get() / 2.0);
}

#[test]
fn test_killing_a_stock_enemy_takes_its_attack_away_for_the_turn() {
    // Axebot on 32 about to hit for 17: the Hammer and the Crush Under kill it, and the new one only Defends and Buffs this turn. That beats 10 damage and 8 Block in front of the 17.
    let hammer = card(0, "HEIRLOOM_HAMMER", "Heirloom Hammer+", "Attack", 2, "Deal 22 damage.", "AnyEnemy");
    let crush = card(1, "CRUSH_UNDER", "Crush Under+", "Attack", 1, "Deal 10 damage.", "AnyEnemy");
    let wall = card(2, "GLITTERSTREAM", "Glitterstream", "Skill", 2, "Gain 8 Block.", "Self");
    let mut plan = Plan::default();
    let foe = with(enemy!("AXEBOT_0", 32, attack = 17, status = [power("STOCK", 1)]), json!({"max_hp": 87}));
    assert!([0, 1].contains(&pick_plan(&combat_state!([hammer, crush, wall], [foe], energy = 3, hp = 82, max_hp = 139), &mut plan)));
    assert_eq!(plan.lost, Some(0));
}

#[test]
fn test_stock_is_read_from_the_state() {
    let foes = read_foes(&[json!({"entity_id": "AXEBOT_0", "hp": 5, "max_hp": 85, "block": 0, "status": [power("STOCK", 2)], "intents": []})]);
    assert_eq!(foes.len(), 1);
    assert_eq!((foes[0].respawns, foes[0].max_hp), (2, 85));
}
