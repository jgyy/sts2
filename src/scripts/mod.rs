//! The analysis scripts, as one program: `scripts NAME ARGS...` (what was `python3 scripts/NAME.py ARGS...`).

pub mod audit_coverage;
pub mod audit_noeffect;
pub mod audit_parse_vs_text;
pub mod audit_simcards;
pub mod bosses_aeonfight;
pub mod bosses_bowlfight;
pub mod bosses_queenfight;
pub mod bosses_sadfight;
pub mod bosses_subjectfight;
pub mod crabfight;
pub mod decklog;
pub mod demon_ancients;
pub mod demon_offers;
pub mod demon_scaling;
pub mod demonfight;
pub mod fightlog;
pub mod kinfight;
pub mod knightfight;
pub mod modelfight;
pub mod relearn;
pub mod roomfight;

pub const NAMES: &str = "audit/coverage, audit/noeffect, audit/parse_vs_text, audit/simcards, bosses/aeonfight, bosses/bowlfight, bosses/queenfight, bosses/sadfight, bosses/subjectfight, crabfight, decklog, demon/ancients, demon/offers, demon/scaling, demonfight, fightlog, kinfight, knightfight, relearn, roomfight";

pub fn main(args: Vec<String>) -> i32 {
    let Some(name) = args.first().cloned() else {
        eprintln!("usage: scripts NAME [ARGS...]\n  NAME: {NAMES}");
        return 2;
    };
    let rest: Vec<String> = args[1..].to_vec();
    match name.trim_end_matches(".py").trim_end_matches(".rs") {
        "audit/coverage" | "audit_coverage" => audit_coverage::main(rest),
        "audit/noeffect" | "audit_noeffect" => audit_noeffect::main(rest),
        "audit/parse_vs_text" | "audit_parse_vs_text" => audit_parse_vs_text::main(rest),
        "audit/simcards" | "audit_simcards" => audit_simcards::main(rest),
        "bosses/aeonfight" | "bosses_aeonfight" => bosses_aeonfight::main(rest),
        "bosses/bowlfight" | "bosses_bowlfight" => bosses_bowlfight::main(rest),
        "bosses/queenfight" | "bosses_queenfight" => bosses_queenfight::main(rest),
        "bosses/sadfight" | "bosses_sadfight" => bosses_sadfight::main(rest),
        "bosses/subjectfight" | "bosses_subjectfight" => bosses_subjectfight::main(rest),
        "crabfight" => crabfight::main(rest),
        "decklog" => decklog::main(rest),
        "demon/ancients" | "demon_ancients" => demon_ancients::main(rest),
        "demon/offers" | "demon_offers" => demon_offers::main(rest),
        "demon/scaling" | "demon_scaling" => demon_scaling::main(rest),
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
