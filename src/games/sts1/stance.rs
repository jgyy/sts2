//! The Watcher's stances along a line of cards the planner tries. Wrath: Attacks deal and take double. Calm: leaving it
//! gives 2 energy. Divinity: Attacks deal triple, entering it gives 3 energy. Cards and intents show the stance the turn began
//! in, so only a change inside the line is worked out. A turn ending in Wrath is charged for the next when the next hand may
//! hold no way out and the doubled blows would kill her (`stuck_odds`, `carry_cost`); one ending in Calm counts some of the
//! energy that leaving it gives.

use std::sync::LazyLock;

use indexmap::IndexMap;
use serde_json::Value;

use crate::cards::parse;
use crate::games::sts1::text::Raw;
use crate::py::{self, JsonExt};

/// an Attack's damage in each stance, against no stance
pub static DEALT: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([("WRATH", 2.0), ("DIVINITY", 3.0)]));
/// damage taken from attacks
pub static TAKEN: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([("WRATH", 2.0)]));
/// energy for leaving Calm, for entering Divinity
pub const CALM_EXIT: i64 = 2;
pub const DIVINITY_ENTRY: i64 = 3;
pub const CALM_LATER: i64 = 1; // energy next turn counted for a turn ended in Calm (it comes only when she leaves it)
pub const MANTRA_DIVINITY: i64 = 10; // Mantra that puts her in Divinity, and is used up doing it
pub const MANTRA_LATER: f64 = 2.0; // points a Mantra short of that is worth (Divinity: 3 energy and Attacks tripled for a turn, about 45, some turns away)
pub const NEXT_HAND: i64 = 5; // cards a new hand draws
// A turn ended in Wrath whose doubled blows next turn would kill her if no way out is drawn: points for that chance (the
// planner's near-kill lines are worth 400). 2026-11-25 17:54: the Watcher ended a Boost turn in Wrath on 31 HP with no
// Calm card in the deck's next five; the Bronze Automaton's Hyper Beam came to 104.
pub const WRATH_DEATH: f64 = 400.0;
pub const STUCK_BLOCK: i64 = 10; // Block a hand with no way out of Wrath still puts up

/// `DEALT.get(stance, 1.0)` (the table's rows, without the lookup: the planner asks for every Attack of every line).
#[inline]
fn _dealt(stance: &str) -> f64 {
    match stance {
        "WRATH" => 2.0,
        "DIVINITY" => 3.0,
        _ => 1.0,
    }
}

/// `TAKEN.get(stance, 1.0)`.
#[inline]
fn _taken(stance: &str) -> f64 {
    match stance {
        "WRATH" => 2.0,
        _ => 1.0,
    }
}

/// The stance's name as one of the constants the planner compares with (a name this module does not know is kept as it is).
fn _name(stance: &str) -> &'static str {
    match stance {
        "" => "",
        "WRATH" => "WRATH",
        "CALM" => "CALM",
        "DIVINITY" => "DIVINITY",
        "EXIT" => "EXIT",
        other => Box::leak(other.to_string().into_boxed_str()),
    }
}

/// WRATH, CALM, DIVINITY, or "" (no stance: "Neutral", or a game without stances).
pub fn read<R: Raw + ?Sized>(stance: &R) -> &'static str {
    match stance.raw().to_uppercase().as_str() {
        "WRATH" => "WRATH",
        "DIVINITY" => "DIVINITY",
        "CALM" => "CALM",
        _ => "",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stance {
    pub start: &'static str,
    pub now: &'static str,
}

impl Stance {
    pub fn new<R: Raw + ?Sized>(start: &R) -> Stance {
        let s = read(start);
        Stance { start: s, now: s }
    }

    /// An Attack's damage a hit, from what its text shows (in the stance the turn began in) to the stance now.
    pub fn deal(&self, per: i64) -> i64 {
        py::round(per as f64 * _dealt(self.now) / _dealt(self.start))
    }

    /// Change to `to` (a card's CardInfo.stance: WRATH, CALM, DIVINITY or EXIT); the energy that gives.
    pub fn enter(&mut self, to: &str) -> i64 {
        let new = if to == "EXIT" { "" } else { to };
        if to.is_empty() || new == self.now {
            return 0;
        }
        let gained = (if self.now == "CALM" { CALM_EXIT } else { 0 }) + (if new == "DIVINITY" { DIVINITY_ENTRY } else { 0 });
        self.now = _name(new);
        gained
    }

    /// What the enemies' attacks (as their intents show them, in the stance the turn began in) come to now.
    pub fn taken(&self) -> f64 {
        _taken(self.now) / _taken(self.start)
    }

    pub fn later_energy(&self) -> i64 {
        if self.now == "CALM" {
            CALM_LATER
        } else {
            0
        }
    }
}

/// Played, it takes her out of Wrath: into Calm or Divinity, or out of any stance (Inner Peace's "otherwise Enter Calm" too;
/// not Blasphemy: she dies next turn).
pub fn leaves_wrath(card: &Value) -> bool {
    let info = parse(card);
    let words = card["description"].text();
    (matches!(info.stance.as_str(), "CALM" | "DIVINITY" | "EXIT") || crate::re!(r"(?i)\benter\s+(?:calm|divinity)\b|\bexit\s+your\s+stance\b").is_match(&words)) && !info.deadly
}

/// The chance that next turn's hand (and `extra` cards more: Simmering Fury's) holds no card that leaves Wrath (none when
/// one is Retained in the hand now). The draw comes off the draw pile; what it lacks comes from the discard pile, this
/// turn's hand in it, shuffled.
pub fn stuck_odds(player: &Value, extra: i64) -> f64 {
    let hand = player["hand"].list();
    if hand.iter().any(|c| leaves_wrath(c) && crate::re!(r"(?i)\bretain\b").is_match(&c["description"].text())) {
        return 0.0;
    }
    let draw = player["draw_pile"].list();
    let mut need = NEXT_HAND + extra;
    let (size, outs);
    if (draw.len() as i64) < need {
        if draw.iter().any(leaves_wrath) {
            return 0.0;
        }
        need -= draw.len() as i64;
        let discard = player["discard_pile"].list();
        size = (discard.len() + hand.len()) as i64;
        outs = discard.iter().chain(hand.iter()).filter(|c| leaves_wrath(c)).count() as i64;
    } else {
        size = draw.len() as i64;
        outs = draw.iter().filter(|c| leaves_wrath(c)).count() as i64;
    }
    if size == 0 {
        return 1.0;
    }
    let need = need.min(size);
    py::comb(size - outs, need) as f64 / py::comb(size, need) as f64
}

/// What ending the turn in Wrath costs: the chance of no way out next turn and of an attack then (`attacks`: how often the
/// enemies have attacked), if their usual blows (`blow`, as they are outside Wrath) doubled would get through a hand's Block
/// (`STUCK_BLOCK`) and kill her. Not the HP a doubled blow takes short of that: in the model runs charging it too lost 1.1
/// floors a run (act 1 bosses 57 deaths in 533 runs against 37: fights went on longer without Wrath).
pub fn carry_cost(odds: f64, blow: f64, hp_left: i64, attacks: f64) -> f64 {
    if odds > 0.0 && blow > 0.0 && 0 < hp_left && hp_left as f64 <= 2.0 * blow - STUCK_BLOCK as f64 {
        odds * attacks * WRATH_DEATH
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games;

    fn stance_of(v: &Value) -> Stance {
        Stance::new(v)
    }

    #[test]
    fn tables() {
        for s in ["", "WRATH", "CALM", "DIVINITY", "EXIT", "Neutral"] {
            assert_eq!(_dealt(s), DEALT.get(s).copied().unwrap_or(1.0));
            assert_eq!(_taken(s), TAKEN.get(s).copied().unwrap_or(1.0));
        }
    }

    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/stance.json")).unwrap();
        let mut checked = 0;
        for row in g["read"].list() {
            assert_eq!(read(&row[0]), row[1].s(), "read {}", row[0]);
            checked += 1;
        }
        for row in g["carry_cost"].list() {
            let got = carry_cost(row[0].as_f64().unwrap(), row[1].as_f64().unwrap(), row[2].as_i64().unwrap(), row[3].as_f64().unwrap());
            assert_eq!(got, row[4].as_f64().unwrap(), "carry_cost {row}");
            checked += 1;
        }
        // A start, then steps: ["enter", to, energy, now] | ["deal", per, damage] | ["taken", x] | ["later", n]
        for case in g["stances"].list() {
            let mut st = stance_of(&case["start"]);
            assert_eq!(st.start, case["read"].s());
            for step in case["steps"].list() {
                match step[0].s() {
                    "enter" => {
                        assert_eq!(st.enter(step[1].s()), step[2].as_i64().unwrap(), "{case}");
                        assert_eq!(st.now, step[3].s(), "{case}");
                    }
                    "deal" => assert_eq!(st.deal(step[1].as_i64().unwrap()), step[2].as_i64().unwrap(), "{case} {step}"),
                    "taken" => assert_eq!(st.taken(), step[1].as_f64().unwrap(), "{case}"),
                    "later" => assert_eq!(st.later_energy(), step[1].as_i64().unwrap(), "{case}"),
                    other => panic!("{other}"),
                }
                checked += 1;
            }
        }
        let pool = g["pool"].list();
        let pile = |ids: &Value| match ids.as_array() {
            Some(ids) => Value::Array(ids.iter().map(|n| pool[n.as_u64().unwrap() as usize].clone()).collect()),
            None => Value::Null,
        };
        for case in g["leaves"].list() {
            let _game = games::playing(case["g"].s());
            let card = &pool[case["c"].as_u64().unwrap() as usize];
            assert_eq!(leaves_wrath(card), case["out"].as_bool().unwrap(), "leaves_wrath {card}");
            checked += 1;
        }
        for case in g["stuck"].list() {
            let _game = games::playing(case["g"].s());
            let mut player = serde_json::Map::new();
            for (key, from) in [("hand", "hand"), ("draw_pile", "draw"), ("discard_pile", "discard")] {
                if !case[from].is_null() {
                    player.insert(key.into(), pile(&case[from]));
                }
            }
            let player = Value::Object(player);
            for (extra, want) in case["odds"].list().iter().enumerate() {
                assert_eq!(stuck_odds(&player, extra as i64), want.as_f64().unwrap(), "stuck_odds {extra} {player}");
                checked += 1;
            }
        }
        assert!(checked > 2000, "{checked} golden cases");
    }
}
