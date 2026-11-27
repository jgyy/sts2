//! Crusher and Rocket as a model fight, played by the bot's own planner (combat::choose_card).
//!
//! ```text
//! scripts crabfight [FIGHTS] [ARM,ARM,...]   ARM: a RAGE_COST, NAME=VALUE (a constant of battle.rs or
//!                                            combat.rs, else a learn.rs knob: caution=1.5), or
//!                                            threat=ROCKET/20/CRUSHER/12 (battle::KNOWN_THREAT); '+' joins
//! scripts crabfight FIGHTS ARMS debuff       the decks with a Bash and a Clothesline (Vulnerable, Weak)
//! scripts crabfight trace [SEED]
//! ```
//!
//! The rules are the logs' (C_CYCLE, R_CYCLE): a crab hits half again from behind us, we face the one we hit last, and a
//! crab whose ally dies gains 6 Strength and 99 Block. Trace a fight and read it before believing a table.

use serde_json::{json, Value};

use super::modelfight::{
    aimed_text, attack, counter, deal, exit_code, face, float, game_state, hand_cards, hit, in_battle, in_combat, int, mean, name_value, no_setup, on_all_cores, out, quiet, set_battle, set_combat,
    shuffled, status, target, turn, under, Face, GameStateArgs, Py,
};
use crate::fight::battle;
use crate::learn;
use crate::py::{self, floordiv, trunc, Random};

/// hit, hits, 15 Block, Weak+Frail
pub const C_CYCLE: [(i64, i64, bool, bool); 5] = [(12, 1, false, false), (4, 1, false, false), (6, 2, false, true), (0, 0, false, false), (12, 1, true, false)];
pub const R_CYCLE: [(i64, i64); 5] = [(3, 1), (18, 1), (0, 0), (31, 1), (0, 0)];
pub const KIND_C: [&str; 5] = ["Attack", "Attack", "Attack", "Buff", "Attack"];
pub const KIND_R: [&str; 5] = ["Attack", "Attack", "Buff", "Attack", "Sleep"];

/// kind: (id, name, type, cost, text, target) from the number on the card
pub fn FACES(c: char, n: i64) -> Face {
    match c {
        'A' => face("STRIKE_R", "Strike", "Attack", 1, &format!("Deal {n} damage."), "AnyEnemy"),
        'H' => face("HEAVY", "Heavy Blow", "Attack", 2, &format!("Deal {n} damage."), "AnyEnemy"),
        'V' => face("BASH_R", "Bash", "Attack", 1, &format!("Deal {n} damage. Apply 2 Vulnerable."), "AnyEnemy"),
        'W' => face("CLOTHESLINE_R", "Clothesline", "Attack", 1, &format!("Deal {n} damage. Apply 2 Weak."), "AnyEnemy"),
        'B' => face("DEFEND_R", "Defend", "Skill", 1, &format!("Gain {n} Block."), "Self"),
        'F' => face("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
        _ => panic!("KeyError: {}", py::str_repr(&c.to_string())),
    }
}

/// The damage (or Block) a card does, `w` and `fw` what Weak and Frail leave of it. (Python's defaults: 1 and 1.)
pub fn number(c: char, da: i64, db: i64, w: f64, fw: f64) -> i64 {
    match c {
        'A' => trunc(da as f64 * w),
        'H' => trunc(2.2 * da as f64 * w),
        'V' | 'W' => trunc(floordiv(da, 2) as f64 * w),
        'B' => trunc(db as f64 * fw),
        _ => 0,
    }
}

#[derive(Clone, Debug)]
pub struct Crab {
    pub hp: i64,
    pub mx: i64,
    pub block: i64,
    pub st: i64,
    pub vul: i64,
    pub wk: i64,
}

/// The crabs, in the order the game lists them (the fight's `E`).
pub const IDS: [&str; 2] = ["CRUSHER_0", "ROCKET_0"];

/// What Weak (or Frail) leaves of a number while it is on.
fn left(on: i64) -> f64 {
    if on != 0 {
        0.75
    } else {
        1.0
    }
}

/// (Python's defaults: chp 206, rhp 196, no trace.)
pub fn fight(deck_spec: &[i64], energy: i64, maxhp: i64, rng: &mut Random, chp: i64, rhp: i64, trace: bool) -> Py {
    let (da, db) = (deck_spec[0], deck_spec[1]);
    let (mut draw, mut disc, mut hp) = (shuffled(rng, "ABFHVW", &deck_spec[2..]), Vec::new(), maxhp);
    let mut E = [Crab { hp: chp, mx: chp, block: 0, st: 0, vul: 0, wk: 0 }, Crab { hp: rhp, mx: rhp, block: 0, st: 0, vul: 0, wk: 0 }];
    let mut raged = [false, false];
    let mut facing: Option<usize> = None;
    let mut first_dead = Py::None;
    let (mut weak, mut frail) = (0, 0);
    for r in 1..=30 {
        let (mut hand, mut en, mut block) = (deal(&mut draw, &mut disc, rng, 5), energy, 0);
        let k = ((r - 1) % 5) as usize;

        // (damage a hit, hits) of each crab attacking this round.
        let labels = |E: &[Crab; 2], facing: Option<usize>| -> [Option<(i64, i64)>; 2] {
            let mut out = [None, None];
            let both = E[0].hp > 0 && E[1].hp > 0;
            let face = match facing {
                Some(f) if E[f].hp > 0 => f,
                _ => 1,
            };
            for (eid, (x, hits)) in [(0, (C_CYCLE[k].0, C_CYCLE[k].1)), (1, R_CYCLE[k])] {
                if x != 0 && E[eid].hp > 0 {
                    let mut x = x + E[eid].st;
                    x = if both && face != eid { trunc(x as f64 * 1.5) } else { x };
                    out[eid] = Some((if E[eid].wk != 0 { trunc(x as f64 * 0.75) } else { x }, hits));
                }
            }
            out
        };

        let state = |E: &[Crab; 2], raged: &[bool; 2], facing: Option<usize>, hand: &[char], draw: &[char], disc: &[char], hp: i64, block: i64, en: i64, weak: i64, frail: i64| -> Value {
            let (lab, mut enemies) = (labels(E, facing), Vec::new());
            for (eid, kinds, side) in [(0, &KIND_C, "LEFT"), (1, &KIND_R, "RIGHT")] {
                let e = &E[eid];
                if e.hp > 0 {
                    let st = status(&[
                        (&format!("BACK_ATTACK_{side}_POWER"), 1),
                        ("CRAB_RAGE_POWER", !raged[eid] as i64),
                        ("STRENGTH_POWER", e.st),
                        ("VULNERABLE_POWER", e.vul),
                        ("WEAK_POWER", e.wk),
                    ]);
                    let intents = vec![match lab[eid] {
                        Some((x, hits)) => attack(x, hits),
                        None => json!({"type": kinds[k], "label": ""}),
                    }];
                    enemies.push(json!({"entity_id": IDS[eid], "hp": e.hp, "max_hp": e.mx, "block": e.block, "status": st, "intents": intents}));
                }
            }
            let cards = hand_cards(hand, &|c| FACES(c, number(c, da, db, left(weak), left(frail))), "F", &[]);
            let pile = |cs: &[char]| -> Vec<Value> {
                cs.iter().map(|&c| json!({"name": FACES(c, 0).1, "cost": if c == 'H' { "2" } else { "1" }, "description": FACES(c, number(c, da, db, 1.0, 1.0)).4})).collect()
            };
            game_state(
                "boss",
                json!({"act": 2, "floor": 33, "ascension": 7}),
                r,
                enemies,
                cards,
                &pile,
                draw,
                disc,
                GameStateArgs {
                    hp,
                    maxhp,
                    block,
                    energy: en,
                    status: status(&[("SURROUNDED_POWER", 1), ("WEAK_POWER", weak), ("FRAIL_POWER", frail)]),
                    extra: vec![("_facing", facing.map(|f| json!(IDS[f])).unwrap_or(Value::Null))],
                },
            )
        };

        for _ in 0..12 {
            if E.iter().all(|e| e.hp <= 0) {
                break;
            }
            let Some((i, c, act)) = turn(&state(&E, &raged, facing, &hand, &draw, &disc, hp, block, en, weak, frail), &hand) else { break };
            if "AHVW".contains(c) {
                let t = target(&act, &[(IDS[0], E[0].hp), (IDS[1], E[1].hp)]);
                let t = IDS.iter().position(|id| *id == t).unwrap();
                let d = number(c, da, db, left(weak), 1.0);
                let e = &mut E[t];
                hit(&mut e.hp, &mut e.block, if e.vul != 0 { trunc(d as f64 * 1.5) } else { d });
                facing = Some(t);
                let o = 1 - t;
                if E[t].hp == 0 {
                    if !first_dead.truthy() {
                        first_dead = Py::Tuple(vec![Py::str(IDS[t]), Py::Int(r), Py::Int(E[o].hp)]);
                    }
                    if E[o].hp > 0 {
                        // Crab Rage
                        E[o].st += 6;
                        E[o].block += 99;
                        raged[o] = true;
                    }
                } else if c == 'V' {
                    E[t].vul += 2;
                } else if c == 'W' {
                    E[t].wk += 2;
                }
                en -= if c == 'H' { 2 } else { 1 };
            } else if c == 'B' {
                block += number(c, da, db, 1.0, left(frail));
                en -= 1;
            }
            disc.push(hand.remove(i));
            if trace {
                out!("   play {} {} {}", c, aimed_text(&act), Py::Dict(IDS.iter().zip(&E).map(|(id, e)| (Py::str(id), Py::Int(e.hp))).collect()).repr());
            }
        }
        if E.iter().all(|e| e.hp <= 0) {
            return Py::Tuple(vec![Py::Bool(true), Py::Int(hp), Py::Int(r), first_dead]);
        }
        let lost = 0.max(labels(&E, facing).iter().flatten().map(|(x, h)| x * h).sum::<i64>() - block);
        hp -= lost;
        if trace {
            out!("r{r} lost {lost} hp {hp} | C {} R {} facing {}", E[0].hp, E[1].hp, facing.map(|f| IDS[f]).unwrap_or("None"));
        }
        disc.append(&mut hand);
        if hp <= 0 {
            return Py::Tuple(vec![Py::Bool(false), Py::Int(0), Py::Int(r), first_dead]);
        }
        for e in E.iter_mut() {
            // the enemies' turn: Block goes, then what they did
            (e.block, e.vul, e.wk) = (0, 0.max(e.vul - 1), 0.max(e.wk - 1));
        }
        let (_, _, cdef, cdeb) = C_CYCLE[k];
        if E[0].hp > 0 {
            E[0].block = if cdef { 15 } else { 0 };
            E[0].st += if KIND_C[k] == "Buff" { 2 } else { 0 };
        }
        if E[1].hp > 0 && KIND_R[k] == "Buff" {
            E[1].st += 2;
        }
        (weak, frail) = if E[0].hp > 0 && cdeb { (2, 2) } else { (0.max(weak - 1), 0.max(frail - 1)) };
    }
    Py::Tuple(vec![Py::Bool(false), Py::Int(0), Py::Int(30), first_dead])
}

/// A number (combat's RAGE_COST), threat=ROCKET/20/CRUSHER/12 (battle::KNOWN_THREAT), NAME=VALUE (a constant of battle.rs
/// or combat.rs, else a knob of learn.rs), or several joined by '+': set on this thread from now on (see `modelfight::under`).
pub fn arm_patch(arm: &str) -> Result<(), String> {
    if arm.contains('+') {
        for part in arm.split('+') {
            arm_patch(part)?;
        }
        return Ok(());
    }
    if let Some(rest) = arm.strip_prefix("threat=") {
        let parts: Vec<&str> = rest.split('/').collect();
        let mut table = battle::known_threat();
        for i in (0..parts.len()).step_by(2) {
            table.insert(parts[i].to_string(), float(parts.get(i + 1).ok_or("IndexError: list index out of range")?)?);
        }
        battle::set_known_threat(Some(table));
        return Ok(());
    }
    if arm.contains('=') {
        let (name, value) = name_value(arm)?;
        let value = float(value)?;
        return if in_battle(name) {
            set_battle(name, value)
        } else if in_combat(name) {
            set_combat(name, value)
        } else if name.chars().any(|ch| ch.is_ascii_lowercase()) {
            learn::set_knob(name, value);
            Ok(())
        } else {
            Err(format!("{name} is not a constant of battle.rs or combat.rs that can be set by name"))
        };
    }
    set_combat("RAGE_COST", float(arm)?)
}

/// (arm, deck, energy, fights, seed) -> a row of the table.
pub fn run(args: &(String, Vec<i64>, i64, i64, i64)) -> Result<Py, String> {
    let (arm, dk, energy, n, seed) = args;
    let res: Vec<Py> = under(
        || arm_patch(arm),
        || (0..*n).map(|j| fight(dk, *energy, 80, &mut Random::new((seed * 10007 + j) as i128), 206, 196, false)).collect(), // a seed per fight: every arm plays the same shuffles
    )?;
    let first = counter(res.iter().map(|o| if o.at(3).truthy() { o.at(3).at(0).clone() } else { Py::None }));
    Ok(Py::Tuple(vec![Py::str(arm), Py::ints(dk), Py::Int(*energy), Py::Float(mean(&res, 0)), Py::Float(mean(&res, 1)), Py::Float(mean(&res, 2)), first]))
}

/// damage, Block, Strikes, Defends, dead, heavy
pub fn DECKS() -> Vec<Vec<i64>> {
    vec![vec![13, 9, 8, 7, 2, 2], vec![16, 9, 8, 7, 2, 2], vec![13, 12, 8, 7, 2, 2], vec![16, 12, 7, 8, 2, 2], vec![19, 9, 8, 7, 2, 2], vec![22, 12, 8, 7, 2, 2]]
}

/// two Strikes made a Bash (half a Strike, 2 Vulnerable) and a Clothesline (2 Weak)
pub fn DEBUFF_DECKS() -> Vec<Vec<i64>> {
    DECKS().into_iter().map(|d| vec![d[0], d[1], d[2] - 2, d[3], d[4], d[5], 1, 1]).collect()
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    if args.first().map(String::as_str) == Some("trace") {
        let seed = if args.len() > 1 { int(&args[1])? } else { 1 };
        out!("{}", fight(&DECKS()[3], 4, 80, &mut Random::new(seed as i128), 206, 196, true).repr());
    } else {
        let n = if !args.is_empty() { int(&args[0])? } else { 100 };
        let arms: Vec<String> = if args.len() > 1 { args[1].split(',').map(str::to_string).collect() } else { vec![py::float_repr(battle::RAGE_COST.get())] };
        let decks = if args.get(2).map(String::as_str) == Some("debuff") { DEBUFF_DECKS() } else { DECKS() };
        let jobs: Vec<(String, Vec<i64>, i64, i64, i64)> = arms.iter().flat_map(|a| decks.iter().enumerate().map(move |(i, dk)| (a.clone(), dk.clone(), 4, n, 500 + i as i64))).collect();
        let out: Vec<Py> = on_all_cores(&no_setup, &run, &jobs, 8).into_iter().collect::<Result<_, _>>()?;
        for a in &arms {
            let rows: Vec<Py> = out.iter().filter(|o| o.at(0).as_str() == a).cloned().collect();
            let f = |k: Py| -> i64 { rows.iter().map(|o| o.at(6).get(&k).map(Py::int).unwrap_or(0)).sum() };
            out!(
                "{a}: won {:.3}  hp left {:5.1}  rounds {:4.1}  first to die: Crusher {}, Rocket {}, neither {}",
                mean(&rows, 3),
                mean(&rows, 4),
                mean(&rows, 5),
                f(Py::str("CRUSHER_0")),
                f(Py::str("ROCKET_0")),
                f(Py::None)
            );
        }
        for dk in &decks {
            let dk = Py::ints(dk);
            out!("{} {}", dk.repr(), out.iter().filter(|o| *o.at(1) == dk).map(|o| format!("{}: {:.2}", o.at(0).as_str(), o.at(3).float())).collect::<Vec<_>>().join(" "));
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

    const GOLDEN: &str = include_str!("../../tests/golden/scripts/crabfight.txt");

    #[test]
    fn as_python() {
        for command in ["trace 3", "3 0.3,RAGE_COST=5+caution=1.5,0.9,threat=ROCKET/20/CRUSHER/12+HP_BASE=2", "2 0.3,THREAT_FLOOR=9 debuff"] {
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
