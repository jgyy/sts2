//! Bowlbug Rock and Bowlbug Nectar (an act 2 hallway fight) as a model, played by the bot's own planner.
//!
//! ```text
//! scripts bosses/bowlfight [FIGHTS A DECK] [ARMS]       each arm on the same shuffles (default arms: off on)
//! scripts bosses/bowlfight trace [SEED] [DECK] [ARM]    one fight, play by play (DECK: its place in DECKS)
//! scripts bosses/bowlfight flips [DECK] [ARM ARM]       the fights where the arms differ most in HP lost
//! ```
//!
//! Arms ("+" joins them): "on" as the bot plays (battle::KNOWN_THREAT as it stands), "off" without BOWLBUG_NECTAR in it,
//! "egg" adds a Bowlbug Egg, "nectar=N" puts the Nectar in KNOWN_THREAT at N, NAME=VALUE a constant of battle/combat/triggers.
//! The fight is the Ascension 9 logs' (`intent`); the planner gets the bot's threat average (`_threat`). The measure is HP lost.

use serde_json::{json, Map, Value};

use super::modelfight::{
    aimed, attack, counter, deal, exit_code, face, firsts, flips, float, game_state, hand_cards, hit, index, int, mean, mean_of, name_value, out, piles, played, quiet, setting, shuffled, status,
    table, target, trace_args, turn, under, Face, GameStateArgs, Py,
};
use crate::fight::battle;
use crate::py::{self, sort_by_key, trunc, JsonExt, Random};

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

pub fn dealt(c: char, da: i64) -> i64 {
    match c {
        'A' => da,
        'H' => trunc(2.4 * da as f64),
        'S' => 1.max(trunc(0.75 * da as f64)),
        _ => panic!("KeyError: {}", py::str_repr(&c.to_string())),
    }
}

#[derive(Clone, Debug)]
pub struct Bug {
    pub id: &'static str,
    pub hp: i64,
    pub mx: i64,
    pub st: i64,
    pub blk: i64,
    pub stunned: bool,
}

/// (damage, Strength gained, Block gained, kind) for this enemy on round `r`.
pub fn intent(e: &Bug, r: i64) -> (i64, i64, i64, &'static str) {
    if e.id.starts_with("BOWLBUG_ROCK") {
        // Imbalanced: Stunned for a turn when our Block stops his attack whole
        return if e.stunned { (0, 0, 0, "Stun") } else { (16, 0, 0, "Attack") };
    }
    if e.id.starts_with("BOWLBUG_NECTAR") {
        return if r != 2 { (3 + e.st, 0, 0, "Attack") } else { (0, 16, 0, "Buff") };
    }
    (8, 0, 8, "Attack") // the Egg: 8 and 8 Block
}

/// (Python's defaults: no egg, maxhp 75, start 60, no trace.)
pub fn fight(spec: &[i64], rng: &mut Random, egg: bool, maxhp: i64, start: i64, trace: bool) -> Py {
    let (da, db) = (spec[0], spec[1]);
    let (mut draw, mut disc, mut hp, mut order) = (shuffled(rng, "ABHSF", &spec[2..]), Vec::new(), start, String::new());
    let mut E = vec![
        Bug { id: "BOWLBUG_ROCK_0", hp: rng.randint(46, 49), mx: 49, st: 0, blk: 0, stunned: false },
        Bug { id: "BOWLBUG_NECTAR_0", hp: rng.randint(36, 39), mx: 39, st: 0, blk: 0, stunned: false },
    ];
    if egg {
        E.push(Bug { id: "BOWLBUG_EGG_0", hp: rng.randint(23, 24), mx: 24, st: 0, blk: 0, stunned: false });
    }
    let mut seen: Vec<(i64, i64)> = E.iter().map(|_| (0, 0)).collect();
    let face = |c: char| {
        CARDS(
            c,
            if c == 'B' {
                db
            } else if c == 'F' {
                0
            } else {
                dealt(c, da)
            },
        )
    };
    for r in 1..=20 {
        let (mut hand, mut en, mut block) = (deal(&mut draw, &mut disc, rng, 5), 3, 0);
        for (e, tally) in E.iter().zip(seen.iter_mut()) {
            if e.hp > 0 {
                tally.0 += intent(e, r).0;
                tally.1 += 1;
            }
        }

        let state = |E: &[Bug], hand: &[char], draw: &[char], disc: &[char], hp: i64, block: i64, en: i64| -> Value {
            let mut enemies = Vec::new();
            for e in E {
                if e.hp > 0 {
                    let (d, _, _, kind) = intent(e, r);
                    let mut intents = vec![if d != 0 { attack(d, 1) } else { json!({"type": kind, "label": ""}) }];
                    if e.id.starts_with("BOWLBUG_EGG") {
                        intents.push(json!({"type": "Defend", "label": ""}));
                    }
                    let st = status(&[("STRENGTH_POWER", e.st), ("IMBALANCED_POWER", e.id.starts_with("BOWLBUG_ROCK") as i64)]);
                    enemies.push(json!({"entity_id": e.id, "hp": e.hp, "max_hp": e.mx, "block": e.blk, "status": st, "intents": intents}));
                }
            }
            let threat: Map<String, Value> = E.iter().zip(&seen).map(|(e, (t, n))| (e.id.to_string(), json!(*t as f64 / 1.max(*n) as f64))).collect();
            game_state(
                "monster",
                json!({"act": 2, "floor": 25, "ascension": 9}),
                r,
                enemies,
                hand_cards(hand, &face, "F", &[]),
                &piles(&face),
                draw,
                disc,
                GameStateArgs { hp, maxhp, block, energy: en, status: vec![], extra: vec![("_threat", Value::Object(threat))] },
            )
        };

        for _ in 0..12 {
            if E.iter().all(|e| e.hp <= 0) {
                break;
            }
            let Some((i, c, act)) = turn(&state(&E, &hand, &draw, &disc, hp, block, en), &hand) else { break };
            let aims: Vec<usize> = if c == 'S' {
                (0..E.len()).filter(|&x| E[x].hp > 0).collect()
            } else if "AH".contains(c) {
                let t = target(&act, &E.iter().map(|e| (e.id, e.hp)).collect::<Vec<_>>());
                vec![E.iter().position(|e| e.id == t).unwrap()]
            } else {
                vec![]
            };
            for t in aims {
                let e = &mut E[t];
                hit(&mut e.hp, &mut e.blk, dealt(c, da));
                if e.hp == 0 && !order.contains(&e.id[8..9]) {
                    order.push_str(&e.id[8..9]);
                }
            }
            block += if c == 'B' { db } else { 0 };
            en -= CARDS(c, 0).3;
            disc.push(hand.remove(i));
            if trace {
                let aim = if act.1.has("target") { aimed(&act).expect("TypeError: 'NoneType' object is not subscriptable") } else { "" };
                out!("   play {} {} {}", c, aim.get(8..).unwrap_or(""), Py::Dict(E.iter().map(|e| (Py::str(&e.id[8..12]), Py::Int(e.hp))).collect()).repr());
            }
        }
        if E.iter().all(|e| e.hp <= 0) {
            return Py::Tuple(vec![Py::Int(start - hp), Py::Int(r), Py::Str(order)]);
        }
        let mut lost = 0;
        for e in E.iter_mut() {
            // (in order: the Rock first)
            if e.hp > 0 {
                e.blk = 0;
                let (d, gain, blk, kind) = intent(e, r);
                let soaked = block.min(d);
                block -= soaked;
                lost += d - soaked;
                if e.id.starts_with("BOWLBUG_ROCK") {
                    e.stunned = kind == "Attack" && d > 0 && soaked >= d;
                }
                e.st += gain;
                e.blk += blk;
            }
        }
        hp -= lost;
        disc.append(&mut hand);
        if trace {
            out!("r{r} took {lost} hp {hp} | {}", E.iter().map(|e| format!("{} {}", &e.id[8..12], e.hp)).collect::<Vec<_>>().join(" "));
        }
        if hp <= 0 {
            return Py::Tuple(vec![Py::Int(start), Py::Int(r), Py::Str(order + "x")]);
        }
    }
    Py::Tuple(vec![Py::Int(start - hp), Py::Int(20), Py::Str(order)])
}

/// The patches of an arm like "off", "on+egg" or "nectar=12+egg" (the Egg itself: `eggs`), on this thread from now on
/// (see `modelfight::under`).
pub fn arm_patch(arm: &str) -> Result<(), String> {
    let mut known = battle::known_threat();
    for part in arm.split('+') {
        if part == "off" {
            known.shift_remove("BOWLBUG_NECTAR");
        } else if let Some(n) = part.strip_prefix("nectar=") {
            known.insert("BOWLBUG_NECTAR".to_string(), float(n.split('=').next().unwrap())?);
        } else if part.contains('=') {
            let (name, value) = name_value(part)?;
            setting(name, float(value)?)?;
        }
    }
    battle::set_known_threat(Some(known));
    Ok(())
}

pub fn eggs(arm: &str) -> bool {
    arm.split('+').any(|part| part == "egg")
}

pub fn run(arm: &str, spec: &[i64], n: i64, seed: i64) -> Result<Py, String> {
    let egg = eggs(arm);
    let res = under(|| arm_patch(arm), || played(|spec: &[i64], rng| fight(spec, rng, egg, 75, 60, false), spec, n, seed))?;
    let ends: Vec<Py> = res.iter().map(|o| Py::Bool(o.at(2).as_str().ends_with('x'))).collect();
    let first = counter(res.iter().map(|o| Py::str(o.at(2).as_str().get(..1).unwrap_or("-"))));
    Ok(Py::Tuple(vec![Py::str(arm), Py::ints(spec), Py::Float(mean(&res, 0)), Py::Float(mean(&res, 1)), Py::Float(mean_of(&ends, n as usize)), first]))
}

/// damage, Block, Strikes, Defends, two-cost attacks, ALL-enemy attacks, dead cards (an act 2 deck: 20 to 30 a round)
pub fn DECKS() -> Vec<Vec<i64>> {
    vec![vec![8, 7, 5, 5, 2, 0, 3], vec![9, 8, 5, 5, 2, 0, 2], vec![9, 8, 5, 6, 3, 0, 2], vec![10, 8, 5, 5, 2, 1, 3], vec![9, 9, 4, 5, 2, 2, 2], vec![11, 9, 5, 5, 2, 1, 2]]
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let decks = DECKS();
    if args.first().map(String::as_str) == Some("trace") {
        let (seed, dk, arm) = trace_args(args, &decks)?;
        let row = under(|| arm_patch(&arm), || fight(&dk, &mut Random::new(seed as i128), eggs(&arm), 75, 60, true))?;
        out!("{}", row.repr());
    } else if args.first().map(String::as_str) == Some("flips") {
        let i = if args.len() > 1 { int(&args[1])? } else { 1 };
        let (a, b) = if args.len() > 3 { (args[2].clone(), args[3].clone()) } else { ("off".to_string(), "on".to_string()) };
        let deck = index(&decks, i)?;
        let mut rows: Vec<(i64, i64, Py)> = flips(i, &[a.clone(), b.clone()], None, &|arm, rng| under(|| arm_patch(arm), || fight(deck, rng, eggs(arm), 75, 60, false)))?
            .into_iter()
            .map(|(seed, res)| (res.get(&Py::str(&a)).unwrap().at(0).int() - res.get(&Py::str(&b)).unwrap().at(0).int(), seed, res))
            .collect();
        sort_by_key(&mut rows, |x| -x.0.abs());
        for (d, seed, res) in rows.iter().take(12) {
            out!("{seed} {d} {}", res.repr());
        }
    } else {
        let (out, arms) = table(args, &run, &decks, &["off", "on"])?;
        for (arm, rows) in &arms {
            out!("{arm:20}: hp lost {:5.2}  rounds {:4.1}  died {:.3}  first to die {}", mean(rows, 2), mean(rows, 3), mean(rows, 4), firsts(rows, 5).repr());
        }
        for dk in &decks {
            let dk = Py::ints(dk);
            out!("{} {}", dk.repr(), out.iter().filter(|o| *o.at(1) == dk).map(|o| format!("{}: lost {:5.2}", o.at(0).as_str(), o.at(2).float())).collect::<Vec<_>>().join("  "));
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

    const GOLDEN: &str = include_str!("../../tests/golden/scripts/bosses_bowlfight.txt");

    #[test]
    fn as_python() {
        for command in ["trace", "trace 4 3 off+egg", "trace 2 5 nectar=30+HP_BASE=1", "4", "2 off on+egg nectar=2+THREAT_FLOOR=1", "flips", "flips 4 on+egg nectar=40"] {
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
