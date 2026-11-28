//! The Fool's Arrows: a Shot needs an Arrow that is not standing in an enemy, Aim goes with the first Arrow, and Stretch with the first Block card.

use std::collections::BTreeSet;

use serde_json::{json, Value};
use sts2bot::cards::{self, parse};
use sts2bot::fight::combat::{self, Enemy, SimulateArgs};
use sts2bot::fight::fool::turn::{self, FoolTurn};
use sts2bot::fight::battle;
use sts2bot::learn;
use sts2bot::run::decks;

use super::{combat_state, ctx, pick, power, read_foes};
use crate::fake_game::{card, with};
use crate::helpers::{enemy, DEFEND, STRIKE};

/// (the original's default: 15 damage.)
fn FOOLISH(i: i64) -> Value {
    card(i, "FOOLISH_SHOT", "Foolish Shot", "Attack", 1, "Shoot an Arrow to deal 15 damage.", "AnyEnemy")
}

/// (the original's default: 10 damage.)
fn QUICK(i: i64, dmg: i64) -> Value {
    card(i, "QUICK_SHOT", "Quick Shot", "Attack", 0, &format!("Shoot an Arrow to deal {dmg} damage."), "AnyEnemy")
}

fn NAIL(i: i64) -> Value {
    card(i, "NAIL", "Nail", "Attack", 2, "If the enemy is Impaled, Deal 20 damage and apply 2 Weak and Vulnerable.", "AnyEnemy")
}

fn IMPALED(n: i64) -> Value {
    power("IMPALED", n)
}

fn KI_REST(i: i64) -> Value {
    card(i, "KI_REST", "Ki Rest+", "Skill", 2, "Next turn, gain 6[fool_energy_icon.png]. If you have a Wah Melody or Cho Melody, Retain your Hand this turn.", "None")
}

fn LIMBS(block: i64) -> String {
    format!("Gain {block} Block twice. Costs 2 less [fool_energy_icon.png] if you have a Wah Melody or Ki Melody.")
}

fn ENCORE() -> Value {
    card(0, "ENCORE", "Encore", "Skill", 0, "Gain [fool_energy_icon.png]. Draw 2 cards. You can only play 1 more card this turn.", "None")
}

fn WREST() -> Value {
    card(0, "WREST", "Wrest", "Skill", 0, "If the enemy is Impaled, Dislodge and draw 2 cards.", "AnyEnemy")
}

fn ONES() -> Vec<Value> {
    vec![json!({"name": "Strike", "cost": "1", "description": "Deal 6 damage."}); 10]
}

fn score<L: std::borrow::Borrow<cards::CardInfo>>(line: &[L], foes: &[Enemy], kw: &SimulateArgs) -> f64 {
    combat::simulate(line, foes, kw).unwrap().score
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 5e-8, "{a} != {b}"); // (assertAlmostEqual: 7 places)
}

fn tune(melodies: &[&str]) -> BTreeSet<String> {
    melodies.iter().map(|m| m.to_string()).collect()
}

// Reading

#[test]
fn test_what_the_text_says() {
    // a card, and what parse makes of its text
    let heavy = parse(&card(0, "HEAVY_SHOT", "Heavy Shot", "Attack", 2, "Shoot an Arrow to deal 20 damage. Aim affects this card 2 times.", "AnyEnemy"));
    assert_eq!((heavy.damage, heavy.hits, heavy.shoots, heavy.aim_times), (20, 1, true, 2)); // (was 20 x 2)
    assert!(parse(&FOOLISH(0)).shoots);
    let strike = parse(&STRIKE(0));
    assert!(!strike.shoots && strike.keeps_hand.is_empty());
    assert!(!parse(&card(0, "FLORESCENCE", "Florescence", "Skill", 0, "Exhaust ALL of your cards that Shoot and create The Shot. Exhaust.", "None")).shoots);
    let nock = parse(&card(0, "NOCK", "Nock", "Skill", 0, "Gain 5 Aim.", "None"));
    assert_eq!((nock.aim, nock.known), (5, true));
    let smack = parse(&card(0, "BOW_SMACK", "Bow Smack", "Attack", 1, "Deal 8 damage. If the enemy is Impaled, gain [fool_energy_icon.png].", "AnyEnemy"));
    assert_eq!((smack.damage, smack.energy_gain, smack.on_impaled), (8, 0, Some((1, 0))));
    let hop = parse(&card(0, "KI_HOP", "Ki Hop", "Attack", 1, "Deal 7 damage. If the enemy is Impaled, draw 2 cards.", "AnyEnemy"));
    assert_eq!((hop.damage, hop.draw, hop.on_impaled), (7, 0, Some((0, 2))));
    let nail = parse(&NAIL(0));
    assert_eq!((nail.damage, nail.impaled_only), (20, true));
    let stretch = parse(&card(0, "DEEP_STRETCH", "Deep Stretch", "Skill", 1, "Gain 7 Stretch. Draw 1 card.", "None"));
    assert_eq!((stretch.stretch, stretch.draw), (7, 1));
    assert_eq!(parse(&card(0, "CHO_LIMBS", "Cho Limbs", "Skill", 2, &LIMBS(5), "Self")).block, 10); // (Block twice: it was read as 5 and 7)
    assert_eq!(parse(&card(0, "CHO_LIMBS", "Cho Limbs+", "Skill", 2, &LIMBS(7), "Self")).block, 14);
    let rest = parse(&KI_REST(0));
    assert_eq!((rest.keeps_hand, rest.next_energy), (vec!["WAH".to_string(), "CHO".to_string()], 6)); // Retain your Hand, read with the Melodies it needs
    let convergence = parse(&card(0, "CONVERGENCE", "Convergence", "Skill", 1, "Next turn, gain [regent_energy_icon.png] and [star_icon.png]. Retain your Hand this turn.", "None"));
    assert_eq!(convergence.keeps_hand, ["ANY"]);
}

#[test]
fn test_the_enemy_reads_its_impale_and_hits_count_it_down() {
    let mut foe = read_foes(&[enemy!("A", 50, status = [IMPALED(3)])]).remove(0);
    assert_eq!(foe.impaled, 3);
    battle::_hit(&mut foe, 5, 2, false, false);
    assert_eq!(foe.impaled, 1);
    battle::_hit(&mut foe, 5, 1, false, true); // (an orb's damage is no hit)
    assert_eq!(foe.impaled, 1);
    assert_eq!(turn::arrows_owned(&json!({"status": [power("QUIVER", 1)]}), &[]), 2);
    assert_eq!(turn::arrows_owned(&json!({}), &[]), 1);
    // the mod's count: the free ones, and one for each living enemy that holds one
    let stuck = [enemy!("A", 50, status = [IMPALED(3)]), enemy!("B", 0, status = [IMPALED(1)]), enemy!("C", 9)];
    assert_eq!(turn::arrows_owned(&json!({"arrows": 2, "arrows_free": 1}), &stuck), 2);
}

#[test]
fn test_serrated_arrows_adds_to_every_hit_on_an_impaled_enemy() {
    // Logged 2026-11-27/28: a Strike of 6 took 9 under Serrated Arrows 50 and 12 under 100; the Shot that Impales took its own.
    let mut state = combat_state!([], [enemy!("A", 50, status = [IMPALED(3)]), enemy!("B", 50)]);
    state["player"]["status"] = json!([power("SERRATED_ARROWS", 50)]);
    let mut foes = battle::read_enemies(&state);
    assert_eq!(battle::_hit(&mut foes[0], 6, 4, false, false), 9 + 9 + 9 + 6); // (the Arrow is out after three)
    assert_eq!(battle::_hit(&mut foes[1], 6, 1, false, false), 6);
    let stuck = Enemy { hp: 14, ..battle::read_enemies(&state).remove(0) };
    assert!(battle::_would_kill(&stuck, 6, 2, false) && !battle::_would_kill(&Enemy { serrated: 1.0, ..stuck.clone() }, 6, 2, false));
}

// Planning

#[test]
fn test_a_shot_with_our_only_arrow_in_an_enemy_is_not_played() {
    // Seen 40-odd times in the Fool's first runs: the Shot was paid for and did nothing.
    assert_eq!(pick(&combat_state!([FOOLISH(0), STRIKE(1)], [enemy!("A", 60, attack = 8, status = [IMPALED(3)])], energy = 1)), 1);
    assert_eq!(pick(&combat_state!([FOOLISH(0), STRIKE(1)], [enemy!("A", 60, attack = 8)], energy = 1)), 0);
}

#[test]
fn test_a_second_arrow_from_quiver_is_shot_while_the_first_stands_in_the_enemy() {
    // 2026-11-28, Vantom: Quiver played (it is no power on us), one Arrow in the boss, Shots left in the hand for three turns.
    let mut state = combat_state!([FOOLISH(0), STRIKE(1)], [enemy!("A", 60, attack = 8, status = [IMPALED(3)])], energy = 1);
    state["player"]["arrows"] = json!(2);
    state["player"]["arrows_free"] = json!(1);
    assert_eq!(pick(&state), 0);
    state["player"]["arrows_free"] = json!(0);
    assert_eq!(pick(&state), 1);
}

#[test]
fn test_the_hit_that_frees_the_arrow_goes_first() {
    assert_eq!(pick(&combat_state!([FOOLISH(0), STRIKE(1)], [enemy!("A", 80, attack = 8, status = [IMPALED(1)])], energy = 2)), 1);
}

#[test]
fn test_a_second_shot_needs_a_second_arrow() {
    let (foes, kw, one, two) = (read_foes(&[enemy!("A", 300)]), ctx!(fight = "boss", arrows = 1), parse(&FOOLISH(0)), parse(&QUICK(1, 10)));
    let alone = score(&[one.clone()], &foes, &kw);
    approx(score(&[one.clone(), two.clone()], &foes, &kw), alone);
    assert!(score(&[one, two], &foes, &SimulateArgs { arrows: 2, ..kw.clone() }) > alone);
}

#[test]
fn test_three_hits_after_the_shot_bring_the_arrow_back() {
    let (foes, kw, strike, quick) = (read_foes(&[enemy!("A", 500)]), ctx!(energy = 6, fight = "boss", arrows = 1), parse(&STRIKE(1)), parse(&QUICK(2, 10)));
    let two = vec![parse(&FOOLISH(0)), strike.clone(), strike.clone()];
    approx(score(&[two.clone(), vec![quick.clone()]].concat(), &foes, &kw), score(&two, &foes, &kw));
    let three = [two, vec![strike]].concat();
    assert!(score(&[three.clone(), vec![quick]].concat(), &foes, &kw) > score(&three, &foes, &kw) + 5.0);
}

#[test]
fn test_aim_goes_with_the_first_arrow() {
    let foes = read_foes(&[enemy!("A", 300)]);
    let mut spent = FoolTurn::new::<Enemy>(2, 6, 0, &BTreeSet::new(), 0, 0, None, &[]);
    spent.aim = 0; // a Shot earlier in the line took the Aim
    assert_eq!(spent.before(&parse(&QUICK(0, 16)), &foes, None).damage, 10); // its text counts the 6 Aim on us
    let heavy = parse(&card(0, "HEAVY_SHOT", "Heavy Shot", "Attack", 2, "Shoot an Arrow to deal 32 damage. Aim affects this card 2 times.", "AnyEnemy"));
    assert_eq!(spent.before(&heavy, &foes, None).damage, 20);
    let mut nocked = FoolTurn::new::<Enemy>(1, 0, 0, &BTreeSet::new(), 0, 0, None, &[]);
    nocked.after(&parse(&card(0, "NOCK", "Nock", "Skill", 0, "Gain 5 Aim.", "None")), &mut [], None, 1, 0);
    assert_eq!(nocked.before(&parse(&QUICK(0, 10)), &foes, None).damage, 15); // Aim gained earlier in the line
}

#[test]
fn test_nail_goes_at_the_impaled_enemy_or_not_at_all() {
    let state = combat_state!([NAIL(0), DEFEND(1)], [enemy!("A", 30, attack = 6), enemy!("B", 60, attack = 6, status = [IMPALED(2)])], energy = 2);
    assert_eq!(combat::choose_card(&state, None).unwrap().1, json!({"card_index": 0, "target": "B"}));
    assert_eq!(pick(&combat_state!([NAIL(0), DEFEND(1)], [enemy!("A", 30, attack = 6)], energy = 2)), 1);
}

#[test]
fn test_stretch_goes_with_the_first_block_card() {
    // At Stretch 10 every Defend reads 15; two of them make 20, not 30.
    let d15 = parse(&card(0, "DEFEND_F", "Defend", "Skill", 1, "Gain 15 Block.", "Self"));
    assert_eq!(combat::simulate(&[d15.clone(), d15], &read_foes(&[enemy!("A", 100, attack = 25)]), ctx!(energy = 2, stretch = 10)).unwrap().lost, 5);
}

#[test]
fn test_blossom_shot_gives_stretch_for_the_damage_it_deals() {
    let blossom = parse(&card(0, "BLOSSOM_SHOT", "Blossom Shot", "Attack", 1, "Shoot an Arrow to deal 11 damage and gain Stretch equal to the damage dealt.", "AnyEnemy"));
    assert_eq!((blossom.damage, blossom.shoots, blossom.stretch_dealt), (11, true, true));
    let (foes, kw, d5) = (read_foes(&[enemy!("A", 100, attack = 25)]), ctx!(energy = 2, arrows = 1), parse(&card(1, "DEFEND_F", "Defend", "Skill", 1, "Gain 5 Block.", "Self")));
    assert_eq!(combat::simulate(&[d5.clone()], &foes, &kw).unwrap().lost, 20);
    assert_eq!(combat::simulate(&[blossom.clone(), d5.clone()], &foes, &kw).unwrap().lost, 9); // Stretch 11: the Defend reads 16
    assert_eq!(combat::simulate(&[blossom, d5], &foes, SimulateArgs { arrows: 0, ..kw.clone() }).unwrap().lost, 20); // no Arrow: no damage, no Stretch
}

#[test]
fn test_rain_shot_splashes_the_same_damage_on_the_other_enemies() {
    let rain = parse(&card(0, "RAIN_SHOT", "Rain Shot", "Attack", 2, "Shoot an Arrow to deal 12 damage. Damage ALL other enemies equal to the damage dealt.", "AnyEnemy"));
    assert!(rain.splash);
    let (kw, two) = (ctx!(fight = "boss", arrows = 1), read_foes(&[enemy!("A", 300), enemy!("B", 300)]));
    assert!(score(&[rain.clone()], &two, &kw) > score(&[rain.clone()], &read_foes(&[enemy!("A", 300)]), &kw) + 5.0);
    assert!(score(&[rain.clone()], &two, &SimulateArgs { arrows: 0, ..kw.clone() }) < score(&[rain], &two, &kw) - 10.0); // no Arrow, no splash
}

// Melodies

fn BLAST() -> Value {
    card(0, "WAH_BLAST", "Wah Blast", "Attack", 2, "Deal 14 damage to ALL enemies. If you have a Cho Melody or Ki Melody, hits twice.", "AllEnemies")
}

fn RHYTHM() -> Value {
    card(1, "RHYTHM_STRIKE", "Rhythm Strike", "Attack", 0, "Deal 4 damage. Hits an additional time for each Melody you have. (Hits 1 time)", "AnyEnemy")
}

fn CHO() -> Value {
    card(2, "CHO_READY", "Cho Ready", "Skill", 1, "Gain 8 Block.  Gain 3 Aim.", "Self")
}

#[test]
fn test_a_card_gives_the_melodies_its_name_says() {
    assert_eq!(parse(&card(0, "WAH_CHO_KI", "Wah Cho Ki+", "Attack", 1, "Gain 12 Block. Deal 12 damage.", "AnyEnemy")).melody, ["WAH", "CHO", "KI"]);
    assert_eq!(parse(&card(0, "KI_HOP", "Ki Hop", "Attack", 1, "Deal 7 damage.", "AnyEnemy")).melody, ["KI"]);
    assert!(parse(&card(0, "KILLING", "Killing Blow", "Attack", 1, "Deal 7 damage.", "AnyEnemy")).melody.is_empty());
    assert_eq!(turn::played_melodies(&["Cho Ready", "Wah Sweep+", "Strike"]), tune(&["CHO", "WAH"]));
    let blast = parse(&BLAST());
    assert_eq!((blast.hits, blast.melody_twice), (1, vec!["CHO".to_string(), "KI".to_string()]));
    let shield = parse(&card(0, "HARMONIC_SHIELD", "Harmonic Shield", "Skill", 1, "Gain 5 Block for each Melody you have. (Gain 0 Block)", "Self"));
    assert_eq!((shield.block, shield.melody_block), (0, 5));
}

#[test]
fn test_melody_cards_go_before_the_cards_that_count_them() {
    let (foes, kw, blast, cho, rhythm) = (read_foes(&[enemy!("A", 300)]), ctx!(fight = "boss"), parse(&BLAST()), parse(&CHO()), parse(&RHYTHM()));
    assert!(score(&[cho.clone(), blast.clone()], &foes, &kw) > score(&[blast.clone(), cho.clone()], &foes, &kw));
    assert!(score(&[blast.clone()], &foes, &SimulateArgs { tune: tune(&["KI"]), ..kw.clone() }) > score(&[blast], &foes, &kw));
    assert!(score(&[cho.clone(), rhythm.clone()], &foes, &kw) > score(&[rhythm, cho], &foes, &kw));
}

#[test]
fn test_violent_chord_goes_off_on_the_first_wah_of_the_turn() {
    let (foes, kw) = (read_foes(&[enemy!("A", 300), enemy!("B", 300)]), ctx!());
    let sweep = [parse(&card(0, "WAH_SWEEP", "Wah Sweep", "Attack", 1, "Deal 8 damage to ALL enemies.", "AllEnemies"))];
    let plain = score(&sweep, &foes, &kw);
    assert!(score(&sweep, &foes, &SimulateArgs { chord: 9, ..kw.clone() }) > plain + 10.0);
    approx(score(&sweep, &foes, &SimulateArgs { chord: 9, tune: tune(&["WAH"]), ..kw.clone() }), plain);
}

#[test]
fn test_explosive_arrows_go_off_when_a_shot_impales() {
    let (shot, kw) = ([parse(&FOOLISH(0))], ctx!(arrows = 1));
    let (foes, stuck) = (read_foes(&[enemy!("A", 300), enemy!("B", 300)]), read_foes(&[enemy!("A", 300, status = [IMPALED(2)]), enemy!("B", 300)]));
    let boom = SimulateArgs { boom: 8, ..kw.clone() };
    assert!(score(&shot, &foes, &boom) > score(&shot, &foes, &kw) + 12.0);
    approx(score(&shot, &stuck, &boom), score(&shot, &stuck, &kw));
}

#[test]
fn test_the_bot_hands_the_planner_the_cards_played_this_turn() {
    let state = combat_state!([STRIKE(0)], [enemy!("A", 30, attack = 5)], _played_names = ["Ki Hop", "Defend"]);
    assert_eq!(combat::_context(&state).tune, tune(&["KI"]));
}

// Dislodging

#[test]
fn test_uproot_takes_the_arrow_back_for_the_next_shot() {
    let info = parse(&card(0, "UPROOT", "Uproot", "Skill", 1, "If the enemy is Impaled, Dislodge and apply 3 Vulnerable.", "AnyEnemy"));
    assert_eq!((info.impaled_only, info.dislodge, info.vulnerable), (true, true, 3));
    let (foes, kw, shot) = (read_foes(&[enemy!("A", 300, status = [IMPALED(3)])]), ctx!(energy = 2, fight = "boss", arrows = 1), parse(&FOOLISH(1)));
    assert!(score(&[info.clone(), shot.clone()], &foes, &kw) > score(&[shot, info], &foes, &kw) + 15.0);
}

#[test]
fn test_wrest_draws_only_from_an_impaled_enemy() {
    // 69 of 106 Wrests drew nothing, most played before the Shot that would have let them draw.
    let (wrest, shot) = (parse(&WREST()), parse(&FOOLISH(1)));
    assert_eq!((wrest.impaled_only, wrest.dislodge, wrest.draw), (true, true, 2));
    let (foes, kw) = (read_foes(&[enemy!("A", 300)]), ctx!(energy = 1, fight = "boss", arrows = 1));
    assert_eq!(score(&[wrest.clone()], &foes, &kw), score::<cards::CardInfo>(&[], &foes, &kw));
    assert!(score(&[shot.clone(), wrest.clone()], &foes, &kw) > score(&[wrest, shot], &foes, &kw));
    assert_eq!(pick(&combat_state!([WREST(), FOOLISH(1)], [enemy!("A", 300, attack = 5)], energy = 1)), 1); // the Shot first
}

#[test]
fn test_encore_leaves_one_more_card() {
    // Encore went first, then one Wrest, and the turn ended on 3-4 energy (a Soul Fysh and a Lagavulin Matriarch lost from 69 and 82 HP).
    let encore = parse(&ENCORE());
    assert_eq!((encore.play_limit, encore.energy_gain, encore.draw), (1, 1, 2));
    assert_eq!(parse(&STRIKE(0)).play_limit, -1);
    let (foes, kw, strikes) = (read_foes(&[enemy!("A", 300, attack = 10)]), ctx!(fight = "boss"), [parse(&STRIKE(1)), parse(&STRIKE(2)), parse(&STRIKE(3))]);
    assert!(combat::simulate(&[encore.clone(), strikes[0].clone(), strikes[1].clone()], &foes, &kw).is_none());
    assert!(combat::simulate(&[strikes[0].clone(), strikes[1].clone(), encore], &foes, &kw).is_some());
    let mut state = combat_state!([ENCORE(), STRIKE(1), STRIKE(2), STRIKE(3)], [enemy!("A", 300, attack = 5)], energy = 3);
    assert_ne!(pick(&state), 0); // three Strikes first, Encore last if at all
    state["player"]["status"] = json!([power("ENCORE", 1)]);
    assert_eq!(battle::cards_left_this_turn(&state), Some(1));
    state["player"]["status"] = json!([power("ENCORE", 0)]);
    assert_eq!(battle::cards_left_this_turn(&state), Some(0));
}

#[test]
fn test_cho_firm_blocks_twice_while_an_arrow_is_to_hand() {
    let firm = parse(&card(0, "CHO_FIRM", "Cho Firm", "Skill", 1, "Gain 6 Block. If you have an Arrow, gain Block an additional time.", "Self"));
    assert!(firm.arrow_block);
    assert_eq!(combat::simulate(&[firm.clone()], &read_foes(&[enemy!("A", 100, attack = 20)]), ctx!(energy = 1, arrows = 1)).unwrap().lost, 8);
    assert_eq!(combat::simulate(&[firm], &read_foes(&[enemy!("A", 100, attack = 20, status = [IMPALED(2)])]), ctx!(energy = 1, arrows = 1)).unwrap().lost, 14);
}

// NextTurnEnergy
// Energy next turn is worth what the next hand can spend (live: after Ki Rest+ the turn began on 9.4 and ended on 4.6).

#[test]
fn test_the_room_is_the_next_hands_cost_less_the_energy_it_will_have() {
    let mut state = combat_state!([], [enemy!("A", 100, attack = 10)]);
    assert_eq!(battle::next_room(&state), (99.0, 1.0)); // (no piles: every energy counts, as before)
    state["player"]["draw_pile"] = json!(ONES());
    state["player"]["max_energy"] = json!(3);
    assert_eq!(battle::next_room(&state), (2.0, 1.0)); // five cards at 1, three energy of its own
    state["player"]["status"] = json!([power("ENERGY_NEXT_TURN", 3)]);
    assert_eq!(battle::next_room(&state), (-1.0, 1.0));
    state["player"]["status"] = json!([power("SLOTH", 0), power("MIND_ROT", 1)]);
    assert_eq!(battle::next_room(&state), (0.0, 0.0)); // three cards a turn, and a kept hand adds none to them
    state["player"]["status"] = json!([power("MIND_ROT", 1)]);
    state["player"]["relics"] = json!([{"id": "VELVET_CHOKER", "description": "Gain [regent_energy_icon.png] at the start of each turn. You cannot play more than 6 cards per turn."}]);
    assert_eq!(battle::next_room(&state), (1.0, 1.0)); // four drawn, two more may be played: a kept card still counts
    assert_eq!(battle::next_energy_worth(6, (2.0, 1.0), 0), 6.0 * 2.0 + 1.0 * 4.0);
    assert_eq!(battle::next_energy_worth(6, (2.0, 1.0), 3), 6.0 * 5.0 + 1.0 * 1.0);
    assert_eq!(battle::next_energy_worth(2, (99.0, 1.0), 0), 12.0); // (the old flat 6)
    let beast = with(enemy!("CEREMONIAL_BEAST_0", 200), json!({"intents": [{"type": "Debuff", "label": ""}]}));
    assert_eq!(battle::next_room(&combat_state!([], [beast])).0, -99.0); // Ringing next turn: one card
}

#[test]
fn test_no_ki_rest_before_a_ringing_turn() {
    // Ki Rest+ went before each of the Ceremonial Beast's four Ringing turns, 9 energy spent on one card each time.
    let beast = with(enemy!("CEREMONIAL_BEAST_0", 150), json!({"intents": [{"type": "Debuff", "label": ""}]}));
    let epic = card(1, "EPIC_RETRIEVAL", "Epic Retrieval", "Attack", 2, "Deal 15 damage.", "AnyEnemy");
    let state = combat_state!([KI_REST(0), epic, STRIKE(2)], [beast], energy = 3, kind = "boss", draw_pile = ONES(), max_energy = 3);
    assert_eq!(pick(&state), 1);
    let _room = combat::with_next_room(|_| (99.0, 1.0));
    assert_ne!(pick(&state), 1); // (what it did before: Strike, then Ki Rest+)
}

#[test]
fn test_no_ki_rest_for_a_next_turn_that_sloth_holds_to_three_cards() {
    // The Knowledge Demon's round 6 under Sloth and Mind Rot: Cho Ki then Ki Rest+ (its hand Retained, 9 energy next turn) over Bow Smack+, Strike and Cho Ki; the next turn spent 4 of the 9 on three cards.
    let demon = with(enemy!("KNOWLEDGE_DEMON_0", 144, attack = 14), json!({"status": [power("STRENGTH", 2)]}));
    let cho_ki = card(1, "CHO_KI", "Cho Ki", "Skill", 1, "Gain 8 Block. Regain any spent Stretch.", "Self");
    let smack = card(2, "BIG_HIT", "Big Hit", "Attack", 1, "Deal 11 damage.", "AnyEnemy");
    let state = combat_state!([KI_REST(0), cho_ki, smack, STRIKE(3), DEFEND(4)], [demon], energy = 3, hp = 44, max_hp = 77, kind = "boss", round_no = 6,
                              draw_pile = ONES(), max_energy = 3, status = [power("SLOTH", 0), power("MIND_ROT", 1)]);
    assert!([2, 3].contains(&pick(&state))); // the attacks
    let _room = combat::with_next_room(|_| (0.0, 1.0)); // (what next_room said before: kept cards on top of the three)
    assert!([0, 1].contains(&pick(&state))); // (what it did: Cho Ki, then Ki Rest+)
}

#[test]
fn test_rewards_count_about_two_energy_next_turn() {
    let next_turn = |n: usize| with(card(0, "X", "X", "Skill", 1, &format!("Next turn, gain {}.", "[fool_energy_icon.png]".repeat(n)), "None"), json!({"rarity": "Common"}));
    let _crowd = learn::with_knobs(&[("crowd", 0.0)]); // (cards._crowd_share is the `crowd` setting)
    approx(cards::deck_value(&next_turn(2)) - cards::deck_value(&next_turn(1)), 0.8);
    approx(cards::deck_value(&next_turn(6)) - cards::deck_value(&next_turn(2)), 0.8); // (Ki Rest+: 4.8 for its 6 before)
}

// DeckBuilding

#[test]
fn test_shots_beyond_the_arrows_are_worth_less() {
    let (quick, deck) = (QUICK(0, 10), vec![STRIKE(0); 4]);
    let fit = |more: Vec<Value>| decks::deck_fit(&quick, &[deck.clone(), more].concat(), Some(1), 3);
    assert_eq!(fit(vec![]), 0.0); // the first Shot
    let (second, third) = (fit(vec![FOOLISH(0)]), fit(vec![FOOLISH(0), FOOLISH(0)]));
    assert!(third < second);
    assert!(second < 0.0);
    let quiver = card(0, "QUIVER", "Quiver", "Power", 1, "Gain 1 additional Arrow.", "None");
    assert_eq!(fit(vec![FOOLISH(0), quiver]), 0.0); // a second Arrow for it
}

#[test]
fn test_encore_and_impaled_only_cards_are_rated_for_what_they_do() {
    let encore = with(ENCORE(), json!({"rarity": "Rare"}));
    assert!(cards::deck_value(&encore) <= 2.0); // was 7.4, above every other Fool card offered
    assert_eq!(cards::kind(&encore), "");
    let plain = with(card(0, "DRAWER", "Drawer", "Skill", 0, "Draw 2 cards.", "AnyEnemy"), json!({"rarity": "Uncommon"}));
    let _crowd = learn::with_knobs(&[("crowd", 0.0)]);
    approx(cards::deck_value(&plain) - cards::deck_value(&with(WREST(), json!({"rarity": "Uncommon"}))), 0.8); // half of its 2 cards' 1.6
}

#[test]
fn test_a_card_the_crowd_has_no_numbers_for_is_mixed_with_its_rarity() {
    // The Fool skipped 29% of card rewards on the bare text rating where the others, mixed with crowd.rs, skipped 0-11%.
    let nock = with(card(0, "STEADY_AIM", "Steady Aim", "Skill", 0, "Gain 5 Aim.", "None"), json!({"rarity": "Common"})); // (no tier either: Nock has one since crowd.FOOL_RATING)
    let bare = {
        let _crowd = learn::with_knobs(&[("crowd", 0.0)]);
        cards::deck_value(&nock)
    };
    let _crowd = learn::with_knobs(&[("crowd", 0.6)]);
    approx(cards::deck_value(&nock), 0.4 * bare + 0.6 * cards::_RARITY_CROWD["Common"]);
    assert_eq!(cards::deck_value(&with(STRIKE(0), json!({"rarity": "Basic"}))), -2.5); // (no mean for a Basic card: left alone)
}

#[test]
fn test_dislike_you_goes_before_the_shot_it_adds_to() {
    // 2026-11-24 to 28: of 70 turns with Dislike You and a Shot the Shot went first in 55 (the mark was not read).
    let dislike = |i| card(i, "DISLIKE_YOU", "Dislike You", "Attack", 1, "Deal 8 damage. The enemy takes 50% additional damage from Shots this turn.", "AnyEnemy");
    let state = combat_state!([FOOLISH(0), dislike(1)], [enemy!("A", 200, attack = 8)], energy = 2);
    assert_eq!(pick(&state), 1);
    // ... and on an enemy it already marks, 15 comes to 22
    let marked = read_foes(&[enemy!("A", 22, status = [power("DISLIKE_YOU", 50)])]);
    assert_eq!(marked[0].marked, 50);
    assert!(combat::simulate(&[parse(&FOOLISH(0))], &marked, ctx!(energy = 1, arrows = 1)).unwrap().score > combat::simulate(&[parse(&STRIKE(0))], &marked, ctx!(energy = 1, arrows = 1)).unwrap().score);
}

#[test]
fn test_cho_chorus_goes_before_the_wah_and_ki_cards_it_plays_free() {
    // 2026-11-28: "Wah Hammer, Cho Chorus" and "Cho Ki, Cho Chorus": the Chorus plays those itself, so the energy was thrown away.
    let chorus = card(0, "CHO_CHORUS", "Cho Chorus", "Skill", 3, "Gain 12 Block. Play ALL \u{201c}Wah\u{201d} and \u{201c}Ki\u{201d} cards in your Hand.", "Self");
    let hammer = |i| card(i, "WAH_HAMMER", "Wah Hammer", "Attack", 1, "Deal 9 damage. Deals double damage to Impaled enemies.", "AnyEnemy");
    let state = combat_state!([hammer(1), chorus, hammer(2)], [enemy!("A", 90, attack = 12)], energy = 3);
    assert_eq!(pick(&state), 0);
    // Cho Limbs costs nothing once a Wah Melody is in
    let limbs = parse(&card(1, "CHO_LIMBS", "Cho Limbs", "Skill", 2, &LIMBS(5), "Self"));
    let foes = read_foes(&[enemy!("A", 90, attack = 12)]);
    assert!(combat::simulate(&[parse(&hammer(0)), limbs.clone()], &foes, ctx!(energy = 1)).is_some());
    assert!(combat::simulate(&[limbs, parse(&hammer(0))], &foes, ctx!(energy = 1)).is_none());
}
