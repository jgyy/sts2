//! The Waterfall Giant (act 1 boss): beaten, it deals its Steam Eruption at the end of our next turn (src/fight/eruption.rs).

use serde_json::{json, Value};
use sts2bot::fight::{battle, eruption};

use crate::fake_game::{card, with};
use crate::fight::{combat_state, play_turn, power, worth};
use crate::helpers::{enemy, DEFEND, STRIKE};

fn BLUR(i: i64) -> Value {
    card(i, "BLUR", "Blur", "Skill", 1, "Gain 5 Block. Block is not removed at the start of your next turn.", "Self")
}

fn pile(names: &[(&str, i64, &str)]) -> Vec<Value> {
    names.iter().map(|(name, cost, text)| json!({"name": name, "cost": cost.to_string(), "description": text})).collect()
}

/// The hand that came after the kill on 2026-11-28 10:01: one Defend, a Neutralize and three Strikes.
fn NEXT_HAND() -> Vec<Value> {
    pile(&[("Defend", 1, "Gain 5 Block."), ("Neutralize", 0, "Deal 3 damage. Apply 1 Weak."), ("Strike", 1, "Deal 6 damage."), ("Strike", 1, "Deal 6 damage."), ("Strike", 1, "Deal 6 damage.")])
}

fn giant(hp: i64, steam: i64, attack: i64, more: &[Value]) -> Value {
    let mut status = vec![power("STEAM_ERUPTION", steam)];
    status.extend(more.iter().cloned());
    with(enemy!("WATERFALL_GIANT_0", hp, attack = attack), json!({"max_hp": 250, "status": status}))
}

/// The names of the cards the planner plays, their Block counted as the game would.
fn plays(mut state: Value) -> Vec<String> {
    let mut blur = |state: &mut Value, c: &Value, _: &Value| -> bool {
        if c["name"] == "Blur" {
            state["player"]["status"] = json!([power("BLUR", 1)]); // (as the game shows it for the rest of the turn)
        }
        false
    };
    let mut names: Vec<String> = play_turn(&mut state, 5, true, Some(&mut blur), None).iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
    names.sort();
    names
}

#[test]
fn test_block_is_carried_into_the_blast_when_poison_does_the_killing() {
    // 2026-11-28 10:01, a Silent on 24 HP: the Giant on 15 with 17 Poison and a Steam Eruption of 47. She spent two energy on
    // attacks that Poison made needless, kept Blur and two Defends in her hand, and died of the blast (35 after Neutralize,
    // through one Defend). Blur and both Defends carry 15 Block into that turn: 35 - 15 - 5 leaves her on 9.
    let hand = [DEFEND(0), STRIKE(1), BLUR(2), STRIKE(3), DEFEND(4)];
    let state = |status: &[Value]| combat_state!(hand.clone(), [giant(15, 47, 14, status)], hp = 24, max_hp = 77, kind = "boss", round_no = 11, max_energy = 3, draw_pile = NEXT_HAND(), discard_pile = Vec::<Value>::new());
    assert_eq!(plays(state(&[power("POISON", 17)])), ["Blur", "Defend", "Defend"]);
    // (before the rule: a Giant about to fall was a fight won, and nothing more was played once it was)
    let _off = eruption::ERUPTION.set(false);
    assert!(!plays(state(&[power("POISON", 17)])).contains(&"Blur".to_string()));
}

#[test]
fn test_a_kill_is_worth_the_chance_of_living_through_the_blast() {
    // Two Strikes beat a Giant on 12. The next hand is known (five cards left to draw) and raises 5 Block, or 5 with the blast a
    // quarter down: on 40 HP the 32 it takes is paid; on 24 the blast of 47 is the end, and the kill is worth far less.
    let state = |hp: i64| combat_state!([STRIKE(0), STRIKE(1), DEFEND(2)], [giant(12, 47, 14, &[])], hp = hp, max_hp = 77, kind = "boss", round_no = 11, max_energy = 3, draw_pile = NEXT_HAND(), discard_pile = Vec::<Value>::new());
    let kill = [STRIKE(0), STRIKE(1)];
    let (safe, doomed) = (worth(&state(40), &kill), worth(&state(24), &kill));
    assert!(safe > doomed + 0.9 * eruption::BLAST_DEATH.get(), "{safe} against {doomed}");
    // ... but still worth more than leaving it standing: every turn waited adds 3 to the blast and one more attack
    assert!(doomed > 0.0, "{doomed}");
}

#[test]
fn test_hp_is_dearer_while_the_giant_stands() {
    // A blast of 44 and rising against hands that raise 10 Block: on 45 HP there is little to spare, and a Defend against its 23
    // is worth more than it was when HP only had to last until the Giant fell.
    let draw = pile(&[("Defend", 1, "Gain 5 Block."), ("Defend", 1, "Gain 5 Block."), ("Strike", 1, "Deal 6 damage."), ("Strike", 1, "Deal 6 damage."), ("Strike", 1, "Deal 6 damage.")]);
    let state = combat_state!([STRIKE(0), DEFEND(1)], [giant(120, 44, 23, &[])], hp = 45, max_hp = 77, kind = "boss", round_no = 10, max_energy = 3, draw_pile = draw, discard_pile = Vec::<Value>::new());
    let with_rule = worth(&state, &[DEFEND(1)]);
    let _off = eruption::ERUPTION.set(false);
    assert!(with_rule > 1.3 * worth(&state, &[DEFEND(1)]), "{with_rule}");
}

#[test]
fn test_the_blast_is_read_until_it_is_the_intent() {
    let read = |e: Value| battle::read_enemies(&combat_state!([STRIKE(0)], [e], kind = "boss"))[0].blast;
    assert_eq!(read(giant(120, 32, 16, &[])), 32);
    // beaten this turn: out of reach and Stunned, the power still on it
    let stunned = with(giant(999_999_999, 47, 0, &[]), json!({"intents": [{"type": "Stun", "label": ""}]}));
    assert_eq!(read(stunned), 47);
    // the turn of the blast: it is an attack like any other by then
    let blowing = with(giant(999_999_999, 47, 0, &[]), json!({"intents": [{"type": "DeathBlow", "label": "47"}]}));
    assert_eq!(read(blowing), 0);
    let _off = eruption::ERUPTION.set(false);
    assert_eq!(read(giant(120, 32, 16, &[])), 0);
}
