use serde_json::json;
use sts2bot::fight::combat;

use super::combat_state;
use crate::fake_game::{card, with};
use crate::helpers::enemy;

// ClosingARace

#[test]
fn test_a_boss_that_dies_next_turn_is_raced() {
    // Round 8 of a 379-HP fight with 13 coming: a Strike (0) or a Defend (1) for the last energy. 379 -> 40 in seven rounds is 48 a round: one more turn kills the boss, and the 8 HP a Defend would save are worth little then.
    for (why, foe, foe_hp, hp, kind, first) in [
        ("a boss that dies next turn is hit and not blocked", "KNOWLEDGE_DEMON_0", 40, 30, "boss", 0),
        ("a boss with turns to live is still blocked", "KNOWLEDGE_DEMON_0", 200, 30, "boss", 1),
        ("the last of our HP is never the cheap kind: 13 coming on 10 HP", "KNOWLEDGE_DEMON_0", 40, 10, "boss", 1),
        ("an elite is not raced this way", "BYRDONIS_0", 40, 30, "elite", 1),
    ] {
        let hand = [card(0, "STRIKE_R", "Strike", "Attack", 1, "Deal 9 damage.", "AnyEnemy"), card(1, "DEFEND_R", "Defend", "Skill", 1, "Gain 8 Block.", "Self")];
        let state = combat_state!(hand, [with(enemy!(foe, foe_hp, attack = 13), json!({"max_hp": 379}))], energy = 1, hp = hp, max_hp = 75, kind = kind, round_no = 8);
        assert_eq!(combat::choose_card(&state, None).unwrap().1["card_index"], first, "{why}");
    }
}
