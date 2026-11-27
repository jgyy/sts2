//! The Kin (act 1 boss: the Kin Priest and two Kin Followers) as a model fight, played by the bot's own planner.
//!
//! ```text
//! scripts kinfight [FIGHTS A DECK] [ARMS]        each arm on the same shuffles (default arms: off on)
//! scripts kinfight trace [SEED] [DECK] [ARM]     one fight, play by play (DECK: its place in DECKS)
//! scripts kinfight flips [DECK] [ARM ARM]        the fights one arm wins and the other loses
//! ```
//!
//! Arms: "on" as the bot plays (battle::MAIN_MINIONS: the Followers first), "off" without it (the Priest first), NAME=VALUE
//! a constant of battle/combat/triggers. The fight is the Ascension 8 logs' (HP, `intent`); Frail and Weak take a quarter off.

use serde_json::{json, Value};

use super::modelfight::{
    aimed_text, attack, counter, deal, exit_code, face, firsts, flips, float, game_state, hand_cards, index, int, mean, name_value, out, piles, played, quiet, setting, shuffled, status, table, target,
    trace_args, turn, under, Face, GameStateArgs, Py,
};
use crate::fight::battle;
use crate::py::{self, trunc, Random};

/// (in the order the game lists them)
pub const HP: [(&str, i64); 3] = [("KIN_FOLLOWER_0", 63), ("KIN_FOLLOWER_1", 62), ("KIN_PRIEST_0", 199)];

/// (damage a hit, hits, Strength it gains, debuff it puts on us, kind) on round `r`.
pub fn intent(eid: &str, r: i64, st: i64) -> (i64, i64, i64, &'static str, &'static str) {
    if eid == "KIN_PRIEST_0" {
        return [(8 + st, 1, 0, "FRAIL", "Attack"), (8 + st, 1, 0, "WEAK", "Attack"), (3 + st, 3, 0, "", "Attack"), (0, 0, 2, "", "Buff")][((r - 1) % 4) as usize];
    }
    [(0, 0, 2, "", "Buff"), (5 + st, 1, 0, "", "Attack"), (2 + st, 2, 0, "", "Attack")][((r - 1 + (eid == "KIN_FOLLOWER_1") as i64) % 3) as usize]
}

/// kind: (id, name, type, cost, text, target) from the number on the card (damage or Block)
pub fn CARDS(c: char, n: i64) -> Face {
    match c {
        'A' => face("STRIKE_R", "Strike", "Attack", 1, &format!("Deal {n} damage."), "AnyEnemy"),
        'H' => face("HEAVY", "Heavy Blow", "Attack", 2, &format!("Deal {n} damage."), "AnyEnemy"),
        'S' => face("SWEEP", "Sweeping Blow", "Attack", 1, &format!("Deal {n} damage to ALL enemies."), "AllEnemies"),
        'B' => face("DEFEND_R", "Defend", "Skill", 1, &format!("Gain {n} Block."), "Self"),
        'F' => face("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
        _ => panic!("KeyError: {}", py::str_repr(&c.to_string())),
    }
}

pub fn dealt(c: char, da: i64, weak: bool) -> i64 {
    let d = match c {
        'A' => da,
        'H' => trunc(2.4 * da as f64),
        'S' => 1.max(trunc(0.75 * da as f64)),
        _ => panic!("KeyError: {}", py::str_repr(&c.to_string())),
    };
    if weak {
        trunc(d as f64 * 0.75)
    } else {
        d
    }
}

/// (Python's defaults: maxhp 70, no trace.)
pub fn fight(spec: &[i64], rng: &mut Random, maxhp: i64, trace: bool) -> Py {
    let (da, db) = (spec[0], spec[1]);
    let (mut draw, mut disc, mut hp, mut order) = (shuffled(rng, "ABHSF", &spec[2..]), Vec::new(), maxhp, String::new());
    let mut E: Vec<(&str, i64, i64)> = HP.iter().map(|(k, v)| (*k, *v, 0)).collect(); // (id, hp, st)
    let (mut frail, mut weak) = (false, false);
    for r in 1..=30 {
        let (mut hand, mut en, mut block) = (deal(&mut draw, &mut disc, rng, 5), 3, 0);
        let b_blk = if frail { trunc(db as f64 * 0.75) } else { db };
        let shown = |c: char, w: Option<bool>| -> i64 {
            if c == 'B' {
                b_blk
            } else if c == 'F' {
                0
            } else {
                dealt(c, da, w.unwrap_or(weak))
            }
        };

        let state = |E: &[(&str, i64, i64)], hand: &[char], draw: &[char], disc: &[char], hp: i64, block: i64, en: i64| -> Value {
            let mut enemies = Vec::new();
            for (n, &(eid, ehp, est)) in E.iter().enumerate() {
                if ehp > 0 {
                    let (d, h, _, debuff, kind) = intent(eid, r, est);
                    let mut intents = vec![if d != 0 { attack(d, h) } else { json!({"type": kind, "label": ""}) }];
                    if !debuff.is_empty() {
                        intents.push(json!({"type": "Debuff", "label": ""}));
                    }
                    let st = status(&[("STRENGTH_POWER", est), ("MINION_POWER", (eid != "KIN_PRIEST_0") as i64)]);
                    enemies.push(json!({"entity_id": eid, "hp": ehp, "max_hp": HP[n].1, "block": 0, "status": st, "intents": intents}));
                }
            }
            let cards = hand_cards(hand, &|c| CARDS(c, shown(c, None)), "F", &[]); // (the numbers as the game shows them: Weak and Frail in)
            game_state(
                "boss",
                json!({"act": 1, "floor": 17, "ascension": 8}),
                r,
                enemies,
                cards,
                &piles(&|c| CARDS(c, if c == 'B' { db } else { shown(c, Some(false)) })),
                draw,
                disc,
                GameStateArgs { hp, maxhp, block, energy: en, status: status(&[("FRAIL_POWER", frail as i64), ("WEAK_POWER", weak as i64)]), extra: vec![] },
            )
        };

        for _ in 0..12 {
            if E[2].1 <= 0 {
                break;
            }
            let Some((i, c, act)) = turn(&state(&E, &hand, &draw, &disc, hp, block, en), &hand) else { break };
            let aims: Vec<usize> = if c == 'S' {
                (0..E.len()).filter(|&x| E[x].1 > 0).collect()
            } else if "AH".contains(c) {
                let t = target(&act, &E.iter().map(|e| (e.0, e.1)).collect::<Vec<_>>());
                vec![E.iter().position(|e| e.0 == t).unwrap()]
            } else {
                vec![]
            };
            for t in aims {
                E[t].1 = 0.max(E[t].1 - dealt(c, da, weak));
                if E[t].1 == 0 {
                    order.push_str(&E[t].0[4..5]);
                }
            }
            block += if c == 'B' { b_blk } else { 0 };
            en -= CARDS(c, 0).3;
            disc.push(hand.remove(i));
            if trace {
                out!("   play {} {} {}", c, aimed_text(&act), Py::Dict(E.iter().map(|e| (Py::str(&e.0[4..]), Py::Int(e.1))).collect()).repr());
            }
        }
        if E[2].1 <= 0 {
            // (the Followers are Minions: they go with him)
            return Py::Tuple(vec![Py::Bool(true), Py::Int(hp), Py::Int(r), Py::Str(order)]);
        }
        let mut inc = 0;
        (frail, weak) = (false, false);
        for e in E.iter_mut() {
            if e.1 > 0 {
                let (d, h, gain, debuff, _) = intent(e.0, r, e.2);
                inc += d * h;
                e.2 += gain;
                (frail, weak) = (frail || debuff == "FRAIL", weak || debuff == "WEAK");
            }
        }
        let lost = 0.max(inc - block);
        hp -= lost;
        if trace {
            out!("r{r} took {lost} hp {hp} | {}", E.iter().map(|e| format!("{} {}", &e.0[4..], e.1)).collect::<Vec<_>>().join(" "));
        }
        disc.append(&mut hand);
        if hp <= 0 {
            return Py::Tuple(vec![Py::Bool(false), Py::Int(0), Py::Int(r), Py::Str(order)]);
        }
    }
    Py::Tuple(vec![Py::Bool(false), Py::Int(0), Py::Int(30), Py::Str(order)])
}

/// The arm's settings on this thread from now on (see `modelfight::under`).
pub fn arm_patch(arm: &str) -> Result<(), String> {
    if arm.contains('=') {
        let (name, value) = name_value(arm)?;
        return setting(name, float(value)?);
    }
    if arm == "off" {
        battle::set_main_minions(Some(Default::default()));
    } // "on": as the bot plays
    Ok(())
}

pub fn run(arm: &str, spec: &[i64], n: i64, seed: i64) -> Result<Py, String> {
    let res = under(|| arm_patch(arm), || played(|spec: &[i64], rng| fight(spec, rng, 70, false), spec, n, seed))?;
    let first = counter(res.iter().map(|o| Py::str(o.at(3).as_str().get(..1).unwrap_or("-"))));
    Ok(Py::Tuple(vec![Py::str(arm), Py::ints(spec), Py::Float(mean(&res, 0)), Py::Float(mean(&res, 1)), Py::Float(mean(&res, 2)), first]))
}

/// damage, Block, Strikes, Defends, two-cost attacks, ALL-enemy attacks, dead cards (an act 1 boss deck: 20 to 30 a round)
pub fn DECKS() -> Vec<Vec<i64>> {
    vec![vec![11, 9, 5, 5, 2, 0, 2], vec![12, 10, 5, 5, 2, 0, 2], vec![11, 10, 5, 6, 3, 0, 1], vec![12, 9, 5, 5, 2, 1, 2], vec![11, 10, 4, 5, 2, 2, 2], vec![13, 10, 5, 5, 2, 1, 2]]
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let decks = DECKS();
    if args.first().map(String::as_str) == Some("trace") {
        let (seed, dk, arm) = trace_args(args, &decks)?;
        let row = under(|| arm_patch(&arm), || fight(&dk, &mut Random::new(seed as i128), 70, true))?;
        out!("{}", row.repr());
    } else if args.first().map(String::as_str) == Some("flips") {
        let i = if args.len() > 1 { int(&args[1])? } else { 1 };
        let (a, b) = if args.len() > 3 { (args[2].clone(), args[3].clone()) } else { ("off".to_string(), "on".to_string()) };
        let deck = index(&decks, i)?;
        for (seed, res) in flips(i, &[a.clone(), b.clone()], None, &|arm, rng| under(|| arm_patch(arm), || fight(deck, rng, 70, false)))? {
            if res.get(&Py::str(&a)).unwrap().at(0) != res.get(&Py::str(&b)).unwrap().at(0) {
                out!("{seed} {}", res.repr());
            }
        }
    } else {
        let (out, arms) = table(args, &run, &decks, &["off", "on"])?;
        for (arm, rows) in &arms {
            out!("{arm:20}: won {:.3}  hp left {:5.1}  rounds {:4.1}  first to die {}", mean(rows, 2), mean(rows, 3), mean(rows, 4), firsts(rows, 5).repr());
        }
        for dk in &decks {
            let dk = Py::ints(dk);
            out!("{} {}", dk.repr(), out.iter().filter(|o| *o.at(1) == dk).map(|o| format!("{}: won {:.2} hp {:4.1}", o.at(0).as_str(), o.at(2).float(), o.at(3).float())).collect::<Vec<_>>().join("  "));
        }
    }
    Ok(())
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(script(&args))
}

#[cfg(test)]
mod tests {
    use super::super::modelfight::{golden, section};
    use super::main;

    const GOLDEN: &str = include_str!("../../tests/golden/scripts/kinfight.txt");

    #[test]
    fn as_python() {
        for command in ["trace 2 3 off", "trace", "trace 5 4 THREAT_FLOOR=30", "3", "2 off on FOCUS_FLOOR=1 HP_BASE=3", "flips 3"] {
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
