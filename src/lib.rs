//! A bot that plays Slay the Spire 2 (or the first Slay the Spire) around the clock, and the model runs (`sim`)
//! that play whole runs of either game with the bot's own code.

#![allow(clippy::too_many_arguments, clippy::type_complexity, non_snake_case, non_upper_case_globals)]

pub mod logging;
pub mod base;
pub mod tunable;

pub mod bot;
pub mod cards;
pub mod cli;
pub mod client;
pub mod fight;
pub mod fightrecord;
pub mod games;
pub mod learn;
pub mod run;
pub mod scripts;
pub mod service;
pub mod sim;
pub mod supervisor;

/// What the bot does next: the action's name and its parameters (`("play_card", {"card_index": 2, "target": "JAW_WORM_0"})`).
pub type Action = (String, serde_json::Value);
