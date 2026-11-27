//! Which game the bot plays: Slay the Spire 2 (the default) or the first Slay the Spire (games/sts1: CommunicationMod and a relay).
//! A choice made once at start-up (`select`; `--game both` runs a process for each); one game's tables ask `uses_sts2_tables()`.
//! The choice is per thread, so that model runs of both games (and tests) can go side by side.

use std::cell::Cell;

pub mod both;
pub mod sts1;

pub const GAMES: [&str; 2] = ["sts2", "sts1"];
pub const DEFAULT_PORT: [(&str, u16); 2] = [("sts2", 15526), ("sts1", 15527)];

thread_local! {
    static ACTIVE: Cell<&'static str> = const { Cell::new("sts2") };
}

/// The game's title: "Slay the Spire 2", "Slay the Spire".
pub fn title(game: &str) -> &'static str {
    if game == "sts1" {
        "Slay the Spire"
    } else {
        "Slay the Spire 2"
    }
}

pub fn default_port(game: &str) -> u16 {
    if game == "sts1" {
        15527
    } else {
        15526
    }
}

/// Make `game` the one being played (on this thread, from now on).
pub fn select(game: &str) -> Result<&'static str, String> {
    let lower = game.to_lowercase();
    match GAMES.iter().find(|g| **g == lower) {
        Some(g) => {
            ACTIVE.with(|a| a.set(g));
            Ok(g)
        }
        None => Err(format!("unknown game {} (expected one of {})", crate::py::str_repr(game), GAMES.join(", "))),
    }
}

pub fn active() -> &'static str {
    ACTIVE.with(|a| a.get())
}

/// True while playing Slay the Spire 2, whose cards the hand-made ratings, crowd win rates and relic ratings were made for.
pub fn uses_sts2_tables() -> bool {
    active() == "sts2"
}

/// Puts the earlier game back when dropped (tests: `let _g = games::playing("sts1");`).
pub struct Playing(&'static str);

impl Drop for Playing {
    fn drop(&mut self) {
        ACTIVE.with(|a| a.set(self.0));
    }
}

pub fn playing(game: &str) -> Playing {
    let before = active();
    select(game).expect("a known game");
    Playing(before)
}
