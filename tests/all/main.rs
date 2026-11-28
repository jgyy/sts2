//! The tests, against fake games: the bot loop, the StS1 relay, the planner, the model runs.

#![allow(dead_code, unused_imports, non_snake_case, clippy::too_many_arguments)]

mod fake_game;
mod helpers;

mod bosses;
mod events;
mod fight;
mod run;
mod sts1;

mod test_bot;
mod test_both;
mod test_fightrecord;
mod test_learn;
mod test_base;
mod test_service;
mod test_sim;
mod test_size;
mod test_trace;
