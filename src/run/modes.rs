//! Blitz and Dread beside the standard game (user, 2026-11-28: with every standard character at A10 and Blitz not yet at
//! A5, play Blitz only; finish every Blitz and standard ascension before Dread; the characters lowest in each mode first).
//!
//! Blitz: three short acts of fixed rooms (no map), five ascensions shared by all characters. Dread: the standard game
//! at Ascension 10 with harder monsters. Only standard runs teach the card record, the settings and the relics.

use std::collections::HashMap;

use indexmap::IndexMap;
use serde_json::Value;

use crate::base::{self, JsonExt};
use crate::run::strategy::_who;

pub const MODES: [&str; 2] = ["blitz", "standard"]; // they take turns (this order breaks ties between modes never played)
pub const LAST_MODE: &str = "dread"; // played once neither of them has anything left (user, 2026-11-28)
pub const BLITZ_TOP: i64 = 5; // the highest Blitz ascension the game has
pub const STANDARD_TOP: i64 = 10; // Slay the Spire 2's highest standard ascension
const KEPT: usize = 40; // runs the rotations look back over (strategy's own RECENT_RUNS)

/// The mode of a run record, of a state's `run`, or of a character select screen ("standard" when it does not say).
pub fn of(v: &Value) -> &str {
    v["mode"].as_str().unwrap_or("standard")
}

pub fn standard(v: &Value) -> bool {
    of(v) == "standard"
}

/// What the rotation knows: what was played lately and won, from runs.jsonl and the game's profile.
#[derive(Clone, Debug, Default)]
pub struct Record {
    pub played: Vec<String>,                  // the modes of the last runs, oldest first
    pub recent: HashMap<String, Vec<String>>, // blitz, dread -> the characters of their last runs
    pub won: HashMap<(String, String), i64>,  // (mode, character) -> the highest ascension won
    pub max_blitz: i64,                       // the Blitz ascension unlocked (the profile's)
    pub dread_wins: HashMap<String, i64>,     // character -> Dread wins the game has on record
}

fn keep(list: &mut Vec<String>, item: String) {
    list.push(item);
    if list.len() > KEPT {
        list.drain(..list.len() - KEPT);
    }
}

impl Record {
    /// A finished run (a line of runs.jsonl).
    pub fn note(&mut self, run: &Value) {
        let (mode, who) = (of(run).to_string(), _who(&run["character"].text()));
        keep(&mut self.played, mode.clone());
        if mode != "standard" {
            keep(self.recent.entry(mode.clone()).or_default(), who.clone());
        }
        if run["result"] == "win" {
            let top = self.won.entry((mode, who)).or_insert(0);
            *top = (*top).max(run["ascension"].int());
        }
    }

    /// What the game's profile says of the modes (nothing, from a game without them).
    pub fn profile(&mut self, profile: &Value) {
        if profile["max_blitz_ascension"].is_number() {
            self.max_blitz = profile["max_blitz_ascension"].int();
        }
        for c in profile["characters"].list().iter().filter(|c| c["dread_wins"].is_number()) {
            self.dread_wins.insert(_who(&c["id"].text()), c["dread_wins"].int());
        }
    }

    fn won_at(&self, mode: &str, who: &str) -> Option<i64> {
        self.won.get(&(mode.to_string(), who.to_string())).copied()
    }

    /// How far a character is in a mode, for the rotation (lowest first). `unlocked`: its standard ascension.
    pub fn level(&self, mode: &str, character: &str, unlocked: i64) -> i64 {
        let who = _who(character);
        match mode {
            "blitz" => self.won_at(mode, &who).map_or(0, |a| a + 1),
            "dread" => (self.won_at(mode, &who).is_some() || self.dread_wins.get(&who).is_some_and(|n| *n > 0)) as i64,
            _ => unlocked + (unlocked > 0 && self.won_at("standard", &who).is_some_and(|a| a >= unlocked)) as i64,
        }
    }

    /// Something is still to be won or unlocked in this mode (`max_ascension`: the standard one of each character).
    fn open(&self, mode: &str, max_ascension: &IndexMap<String, i64>) -> bool {
        if max_ascension.is_empty() || (mode == "blitz" && self.max_blitz < BLITZ_TOP) {
            return true;
        }
        max_ascension.iter().any(|(c, top)| match mode {
            "blitz" => self.level(mode, c, *top) <= BLITZ_TOP,
            "dread" => self.level(mode, c, *top) == 0,
            _ => self.level(mode, c, *top) <= *top,
        })
    }

    /// The mode for the next run: Blitz only while every character is at standard A10 and Blitz is not yet at A5; else of
    /// Blitz and standard with something left to win the one played longest ago; Dread when neither has anything.
    /// (`offered`: the menu's options, in lower case.)
    pub fn next(&self, offered: &[String], max_ascension: &IndexMap<String, i64>) -> &'static str {
        let open = |m: &&str| offered.iter().any(|o| o == m) && self.open(m, max_ascension);
        let climbed = !max_ascension.is_empty() && max_ascension.values().all(|top| *top >= STANDARD_TOP);
        if climbed && self.max_blitz < BLITZ_TOP && offered.iter().any(|o| o == "blitz") {
            return "blitz";
        }
        MODES
            .into_iter()
            .filter(open)
            .min_by_key(|m| self.played.iter().rposition(|p| p == m).map_or(0, |i| i + 1))
            .or_else(|| Some(LAST_MODE).filter(open))
            .unwrap_or("standard")
    }
}

// ------------------------------------------------------------- the Blitz shop
pub const CURSE_COST: f64 = 4.0; // what the curse of a cursed entry takes off it

/// What a Blitz shop entry is worth: three free picks of six, the cursed one adding a curse to the deck.
pub fn blitz_worth(item: &Value, deck: &[Value], hp: i64, max_hp: i64) -> f64 {
    let cursed_deck = deck.iter().any(|c| base::str_of(&c["type"]) == "Curse");
    let missing = 1.0 - hp as f64 / max_hp.max(1) as f64;
    let worth = match item["blitz_type"].as_str().unwrap_or("") {
        "TwoRelics" => 14.0,
        "Relic" | "UpgradedRareCardReward" => 9.0,
        "CardRewardOfEachRarity" => 8.5,
        "RareCardReward" => 7.5,
        "TwoMoreChoices" => 7.0,
        "UpgradedRareCard" | "Potions" => 6.5,
        "CardRewards" | "Glam" => 6.0,
        "CardRemoval" => {
            if cursed_deck {
                8.0
            } else {
                6.0
            }
        }
        "CardUpgrade" | "RandomUpgrades" | "UpgradedCardReward" => 5.5,
        "CardEnchant" | "EnchantedCardReward" => 5.0,
        "CardTransform" => 4.5,
        "ColorlessCardReward" => 4.0,
        "FullHealAndMaxHp" => 3.0 + 14.0 * missing,
        _ => 3.5, // PrismaticCardReward (another character's cards), and anything new
    };
    worth - if item["cursed"].truthy() { CURSE_COST } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn all(levels: i64) -> IndexMap<String, i64> {
        ["IRONCLAD", "SILENT"].iter().map(|c| (c.to_string(), levels)).collect()
    }

    fn offered(modes: &[&str]) -> Vec<String> {
        modes.iter().map(|m| m.to_string()).collect()
    }

    #[test]
    fn with_every_character_at_a10_blitz_alone_is_played_until_it_is_at_a5() {
        let menu = offered(&["standard", "blitz", "dread"]);
        let mut record = Record::default();
        for max_blitz in 0..BLITZ_TOP {
            record.max_blitz = max_blitz;
            assert_eq!(record.next(&menu, &all(10)), "blitz");
            record.note(&json!({"character": "The Silent", "mode": "blitz", "result": "loss", "ascension": max_blitz}));
        }
        record.max_blitz = BLITZ_TOP;
        assert_eq!(record.next(&menu, &all(10)), "standard"); // (back to taking turns)
    }

    #[test]
    fn blitz_and_standard_take_turns_before_dread_is_played() {
        let menu = offered(&["standard", "daily", "blitz", "dread", "custom"]);
        let mut record = Record::default();
        let mut turns = Vec::new();
        for _ in 0..4 {
            let mode = record.next(&menu, &all(9));
            turns.push(mode);
            record.note(&json!({"character": "The Silent", "mode": mode, "result": "loss", "ascension": 0}));
        }
        assert_eq!(turns, ["blitz", "standard", "blitz", "standard"]);
        // A game without Blitz or Dread plays the standard one.
        assert_eq!(record.next(&offered(&["standard", "daily"]), &all(10)), "standard");
    }

    #[test]
    fn the_games_stand_in_tiles_are_no_characters() {
        use crate::run::strategy::is_character;
        assert!(["IRONCLAD", "FOOL"].iter().all(|c| is_character(c)) && !is_character("RANDOM_CHARACTER") && !is_character("ROTATING_CHARACTER"));
    }

    #[test]
    fn a_mode_with_nothing_left_is_left_out() {
        let mut record = Record { max_blitz: BLITZ_TOP, ..Record::default() };
        for (character, mode, ascension) in [("The Ironclad", "blitz", 5), ("The Silent", "blitz", 5), ("The Ironclad", "standard", 10), ("The Silent", "standard", 10)] {
            record.note(&json!({"character": character, "mode": mode, "result": "win", "ascension": ascension}));
        }
        let menu = offered(&["standard", "blitz", "dread"]);
        assert_eq!(record.next(&menu, &all(10)), "dread"); // (Blitz and the standard game are both finished)
        record.profile(&json!({"max_blitz_ascension": 5, "characters": [{"id": "IRONCLAD", "dread_wins": 1}, {"id": "SILENT", "dread_wins": 2}]}));
        assert_eq!(record.next(&menu, &all(10)), "standard"); // (all won: back to the standard game)
    }

    #[test]
    fn a_characters_level_is_what_it_has_won_in_that_mode() {
        let mut record = Record::default();
        record.note(&json!({"character": "The Silent", "mode": "blitz", "result": "win", "ascension": 1}));
        record.note(&json!({"character": "The Silent", "result": "win", "ascension": 10}));
        record.note(&json!({"character": "The Ironclad", "result": "win", "ascension": 9}));
        assert_eq!((record.level("blitz", "SILENT", 10), record.level("blitz", "IRONCLAD", 10)), (2, 0));
        assert_eq!((record.level("standard", "SILENT", 10), record.level("standard", "IRONCLAD", 10)), (11, 10));
        assert_eq!(record.level("dread", "SILENT", 10), 0);
    }

    #[test]
    fn a_curse_is_paid_for_only_where_it_is_worth_it() {
        let entry = |kind: &str, cursed: bool| json!({"blitz_type": kind, "cursed": cursed});
        let worth = |kind: &str, cursed: bool, hp: i64| blitz_worth(&entry(kind, cursed), &[], hp, 80);
        assert!(worth("TwoRelics", true, 80) > worth("Relic", false, 80));
        assert!(worth("FullHealAndMaxHp", true, 80) < 0.0); // (nothing to heal)
        assert!(worth("FullHealAndMaxHp", true, 20) > worth("CardUpgrade", false, 20));
        assert!(blitz_worth(&entry("CardRemoval", false), &[json!({"type": "Curse"})], 80, 80) > worth("CardRemoval", false, 80));
    }
}
