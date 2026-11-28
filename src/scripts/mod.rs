//! The analysis scripts, as one program: `scripts NAME ARGS...`.

pub mod audit;
pub mod bosses;
pub mod crabfight;
pub mod decklog;
pub mod demon;
pub mod demonfight;
pub mod fightlog;
pub mod kinfight;
pub mod knightfight;
pub mod modelfight;
pub mod relearn;
pub mod roomfight;

pub const NAMES: &str = "audit/coverage, audit/noeffect, audit/parse_vs_text, audit/simcards, bosses/aeonfight, bosses/bowlfight, bosses/giantfight, bosses/matriarchfight, bosses/queenfight, bosses/sadfight, bosses/subjectfight, crabfight, decklog, demon/ancients, demon/offers, demon/scaling, demonfight, fightlog, kinfight, knightfight, relearn, roomfight";

pub fn main(args: Vec<String>) -> i32 {
    let Some(name) = args.first().cloned() else {
        eprintln!("usage: scripts NAME [ARGS...]\n  NAME: {NAMES}");
        return 2;
    };
    let rest: Vec<String> = args[1..].to_vec();
    match name.trim_end_matches(".rs") {
        "audit/coverage" | "audit_coverage" => audit::coverage::main(rest),
        "audit/noeffect" | "audit_noeffect" => audit::noeffect::main(rest),
        "audit/parse_vs_text" | "audit_parse_vs_text" => audit::parse_vs_text::main(rest),
        "audit/simcards" | "audit_simcards" => audit::simcards::main(rest),
        "bosses/aeonfight" | "bosses_aeonfight" => bosses::aeonfight::main(rest),
        "bosses/bowlfight" | "bosses_bowlfight" => bosses::bowlfight::main(rest),
        "bosses/giantfight" | "bosses_giantfight" => bosses::giantfight::main(rest),
        "bosses/matriarchfight" | "bosses_matriarchfight" => bosses::matriarchfight::main(rest),
        "bosses/queenfight" | "bosses_queenfight" => bosses::queenfight::main(rest),
        "bosses/sadfight" | "bosses_sadfight" => bosses::sadfight::main(rest),
        "bosses/subjectfight" | "bosses_subjectfight" => bosses::subjectfight::main(rest),
        "crabfight" => crabfight::main(rest),
        "decklog" => decklog::main(rest),
        "demon/ancients" | "demon_ancients" => demon::ancients::main(rest),
        "demon/offers" | "demon_offers" => demon::offers::main(rest),
        "demon/scaling" | "demon_scaling" => demon::scaling::main(rest),
        "demonfight" => demonfight::main(rest),
        "fightlog" => fightlog::main(rest),
        "kinfight" => kinfight::main(rest),
        "knightfight" => knightfight::main(rest),
        "relearn" => relearn::main(rest),
        "roomfight" => roomfight::main(rest),
        other => {
            eprintln!("scripts: no script called {other}\n  there are: {NAMES}");
            2
        }
    }
}
