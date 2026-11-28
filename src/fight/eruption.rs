//! The Waterfall Giant's Steam Eruption: "When killed, deals N damage at the end of your next turn", and N grows by 3 on every
//! turn it takes. Beating it is not the end of the fight: one more hand has to stop the blast (an attack: Block stops it, Weak
//! on the Giant takes a quarter off, and its Weak wears off a turn while it stands Stunned).
//!
//! Until 2026-11-28 the planner read a beaten Giant as a fight won: 7 of its 9 losses to it in the logs came after the kill,
//! with 4 to 27 HP against a blast of 38 to 56. Two things follow from the power. The kill is worth the chance of living
//! through the blast (`View::after`: over hands drawn from the piles as they are, the Block each can raise). And all through
//! the fight a part of our HP is spoken for (`View::reserve`: the blast as it will be when the Giant falls, less the Block a
//! usual hand raises), so a turn's losses are priced against what is left over it.

use serde_json::Value;

use super::battle::power_amount;
use crate::base::{self, JsonExt};
use crate::cards::parse;

crate::tunables! {
    /// the rule as a whole (off: a beaten Giant is a fight won, as before 2026-11-28)
    pub ERUPTION: bool = true;
    /// what dying of the blast costs a line that beats the Giant, in points, for each point of chance. Under WIN_BONUS (the
    /// fight won): putting the kill off for a better hand lost fights in the model (100,000: 70.0% won against 78.9%), since
    /// every turn waited adds 3 to the blast and one more attack. It is the lines that do beat it that this tells apart.
    pub BLAST_DEATH: f64 = 8_000.0;
    /// next hands looked at for the Block they raise
    pub HANDS: i64 = 64;
    /// the share of the reserve taken off our HP when a turn's losses are priced
    pub RESERVE: f64 = 1.0;
    /// damage a round the Giant is taken to lose until the fight says (the logs' 250 HP in 8 to 12 rounds)
    pub PACE: f64 = 25.0;
}

/// What Weak takes off the blast.
pub fn cut(blast: i64) -> i64 {
    base::trunc(0.75 * blast as f64)
}

/// What the next hand can do about the blast, from the piles as they are.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// for each hand looked at: (the most Block it raises, the most while also playing a card that applies Weak, or -1)
    pub hands: Vec<(i64, i64)>,
    /// the mean of the first
    pub usual: f64,
    /// Blur played earlier this turn: the Block we end on is still there for the blast
    pub blurred: bool,
}

/// (cost, Block, applies Weak) of a card in a pile; None for one that does neither, or cannot be played.
fn useful(card: &Value) -> Option<(i64, i64, bool)> {
    if crate::re!(r"(?i)\bunplayable\b").is_match(&card["description"].text()) {
        return None;
    }
    let info = parse(card);
    if info.is_x() || (info.block <= 0 && info.weak <= 0) {
        return None;
    }
    Some((info.cost.max(0), info.block.max(0), info.weak > 0))
}

/// (the most Block, the most with a Weak card played or -1) from these cards on `energy`.
fn best(cards: &[(i64, i64, bool)], energy: i64) -> (i64, i64) {
    let (mut plain, mut weakened) = (0, -1);
    for picked in 0u32..(1 << cards.len().min(12)) {
        let (mut cost, mut block, mut weak) = (0, 0, false);
        for (n, c) in cards.iter().enumerate().take(12) {
            if picked & (1 << n) != 0 {
                cost += c.0;
                block += c.1;
                weak |= c.2;
            }
        }
        if cost <= energy {
            plain = plain.max(block);
            if weak {
                weakened = weakened.max(block);
            }
        }
    }
    (plain, weakened)
}

/// For a fight with a Steam Eruption still to come: the next hand's Block, hand by hand. None in any other fight.
pub fn outlook(state: &Value) -> Option<View> {
    if !ERUPTION.get() || !state["battle"]["enemies"].list().iter().any(|e| power_amount(e, "STEAM_ERUPTION") > 0) {
        return None;
    }
    let player = &state["player"];
    let energy = if player["max_energy"].truthy() { player["max_energy"].int() } else { 3 };
    let blurred = power_amount(player, "BLUR") > 0;
    let draw: Vec<Option<(i64, i64, bool)>> = player["draw_pile"].list().iter().map(useful).collect();
    // (once the draw pile runs out the discards come back, and with them the hand we hold now)
    let rest: Vec<Option<(i64, i64, bool)>> = player["discard_pile"].list().iter().chain(player["hand"].list()).map(useful).collect();
    if draw.len() + rest.len() < 5 {
        return Some(View { hands: vec![(10, -1)], usual: 10.0, blurred }); // (piles the state does not list: two Defends)
    }
    // A fixed sequence, so that the same state is always read the same way.
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15 ^ ((draw.len() as u64) << 32) ^ rest.len() as u64 ^ ((state["battle"]["round"].int() as u64) << 16);
    let mut below = |n: usize| -> usize {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) % n.max(1) as u64) as usize
    };
    let mut hands = Vec::new();
    for _ in 0..HANDS.get().max(1) {
        let mut hand: Vec<(i64, i64, bool)> = Vec::new();
        let mut take = |pile: &[Option<(i64, i64, bool)>], count: usize, below: &mut dyn FnMut(usize) -> usize| {
            let mut left: Vec<usize> = (0..pile.len()).collect();
            for _ in 0..count.min(pile.len()) {
                let at = below(left.len());
                hand.extend(pile[left.swap_remove(at)]);
            }
        };
        take(&draw, 5, &mut below);
        if draw.len() < 5 {
            take(&rest, 5 - draw.len(), &mut below);
        }
        hands.push(best(&hand, energy));
    }
    let usual = hands.iter().map(|h| h.0 as f64).sum::<f64>() / hands.len() as f64;
    Some(View { hands, usual, blurred })
}

impl View {
    /// (the chance the blast kills us, the HP it takes on the whole) once the Giant has fallen: `blast` as its Steam Eruption
    /// stands, `weak` when it will still be Weak then, `carried` the Block we bring into that turn, `hp` what we have left.
    pub fn after(&self, blast: i64, weak: bool, carried: i64, hp: i64) -> (f64, f64) {
        let (mut dead, mut taken) = (0.0, 0.0);
        for &(plain, weakened) in &self.hands {
            let mut lost = 0.max((if weak { cut(blast) } else { blast }) - carried - plain);
            if !weak && weakened >= 0 {
                lost = lost.min(0.max(cut(blast) - carried - weakened));
            }
            if lost >= hp {
                dead += 1.0;
            }
            taken += lost.min(hp) as f64;
        }
        let n = self.hands.len().max(1) as f64;
        (dead / n, taken / n)
    }

    /// HP that is spoken for while the Giant lives: the blast as it will stand when it falls (3 more for every round its HP
    /// still takes at `pace`, PACE before the fight says), less the Block a usual hand raises.
    pub fn reserve(&self, blast: i64, giant_hp: i64, pace: f64) -> i64 {
        let pace = if pace > 0.0 { (pace + PACE.get()) / 2.0 } else { PACE.get() }; // (a good start is not the fight's pace)
        let rounds = (giant_hp.max(0) as f64 / pace.max(8.0)).ceil();
        base::trunc(RESERVE.get() * (blast as f64 + 3.0 * rounds - self.usual).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pile(cards: &[(&str, i64, &str)]) -> Vec<Value> {
        cards.iter().map(|(name, cost, text)| json!({"name": name, "cost": cost.to_string(), "description": text})).collect()
    }

    fn state(steam: i64, draw: Vec<Value>, discard: Vec<Value>) -> Value {
        json!({
            "state_type": "boss",
            "battle": {"round": 6, "enemies": [{"entity_id": "WATERFALL_GIANT_0", "hp": 80, "max_hp": 250, "status": if steam != 0 { json!([{"id": "STEAM_ERUPTION_POWER", "amount": steam}]) } else { json!([]) }, "intents": []}]},
            "player": {"hp": 30, "max_hp": 77, "max_energy": 3, "hand": [], "draw_pile": draw, "discard_pile": discard},
        })
    }

    const DEFEND: (&str, i64, &str) = ("Defend", 1, "Gain 5 Block.");
    const STRIKE: (&str, i64, &str) = ("Strike", 1, "Deal 6 damage.");
    const NEUTRALIZE: (&str, i64, &str) = ("Neutralize", 0, "Deal 3 damage. Apply 1 Weak.");

    #[test]
    fn only_a_steam_eruption_fight_has_an_outlook() {
        assert_eq!(outlook(&state(0, pile(&[DEFEND; 5]), vec![])), None);
        let _off = ERUPTION.set(false);
        assert_eq!(outlook(&state(32, pile(&[DEFEND; 5]), vec![])), None);
    }

    #[test]
    fn the_next_hand_is_the_draw_pile_when_it_holds_five() {
        // five cards left to draw: that hand is known, three Defends on three energy
        let view = outlook(&state(32, pile(&[DEFEND, DEFEND, DEFEND, STRIKE, STRIKE]), pile(&[DEFEND; 8]))).unwrap();
        assert!(view.hands.iter().all(|h| *h == (15, -1)), "{:?}", view.hands);
        assert_eq!(view.usual, 15.0);
        // 32 through 15 Block is 17: dead on 17 HP, alive on 18
        assert_eq!(view.after(32, false, 0, 17), (1.0, 17.0));
        assert_eq!(view.after(32, false, 0, 18), (0.0, 17.0));
        // Block brought along (Blur) and Weak on the Giant both count
        assert_eq!(view.after(32, false, 5, 13), (0.0, 12.0));
        assert_eq!(view.after(32, true, 0, 10), (0.0, 9.0));
    }

    #[test]
    fn a_weak_card_in_the_hand_takes_a_quarter_off() {
        let view = outlook(&state(40, pile(&[DEFEND, DEFEND, NEUTRALIZE, STRIKE, STRIKE]), vec![])).unwrap();
        assert_eq!(view.hands[0], (10, 10));
        // 40 -> 30 with Neutralize, 10 Block: 20 lost
        assert_eq!(view.after(40, false, 0, 25), (0.0, 20.0));
        assert_eq!(view.after(40, false, 0, 20), (1.0, 20.0));
    }

    #[test]
    fn the_rest_of_the_hand_comes_from_the_discards() {
        // two cards to draw, the other three from a discard pile of Strikes: two Defends at most
        let view = outlook(&state(30, pile(&[DEFEND, DEFEND]), pile(&[STRIKE; 6]))).unwrap();
        assert!(view.hands.iter().all(|h| h.0 == 10));
        // drawn from a mixed pile the hands differ, and the odds lie between
        let mixed = outlook(&state(30, pile(&[DEFEND, DEFEND, DEFEND, DEFEND, STRIKE, STRIKE, STRIKE, STRIKE, STRIKE, STRIKE]), vec![])).unwrap();
        assert_eq!(mixed.hands.len(), 64);
        let (dead, _) = mixed.after(30, false, 0, 20);
        assert!(dead > 0.1 && dead < 0.9, "{dead}");
        assert_eq!(outlook(&state(30, pile(&[DEFEND, DEFEND, DEFEND, DEFEND, STRIKE, STRIKE, STRIKE, STRIKE, STRIKE, STRIKE]), vec![])).unwrap(), mixed);
    }

    #[test]
    fn the_reserve_grows_with_the_rounds_still_to_come() {
        let view = View { hands: vec![(15, -1)], usual: 15.0, blurred: false };
        // 100 HP at 25 a round is four rounds: 32 + 12 - 15
        assert_eq!(view.reserve(32, 100, 0.0), 29);
        assert_eq!(view.reserve(32, 0, 0.0), 17);
        assert_eq!(view.reserve(10, 0, 0.0), 0);
        assert!(view.reserve(32, 100, 50.0) < 29);
        let _half = RESERVE.set(0.5);
        assert_eq!(view.reserve(32, 100, 0.0), 14);
    }
}
