//! The act 3 knights (Flail, Spectral and Magi Knight) as a model fight, played by the bot's own planner.
//!
//!     scripts knightfight [FIGHTS A DECK] [ARMS]          arms: "off" (without battle::KNOWN_THREAT), "on" (as the
//!                                                         bot plays), NAME=VALUE ("on" with that constant); default off on
//!     scripts knightfight trace [SEED] [DECK] [off]       one fight, play by play (DECK: its place in DECKS)
//!     scripts knightfight flips [DECK]                    the fights one arm wins and the other loses
//!
//! The fight is the Ascension 8 logs' (HP, `intent`): the Spectral Knight's Hex makes every card Ethereal while it lives.

use indexmap::IndexMap;
use serde_json::{json, Value};

use super::modelfight::{
    aimed_text, attack, counter, deal, exit_code, face, firsts, flips, float, game_state, hand_cards, hit, index, int, mean, name_value, out, piles, played, quiet, setting, shuffled, status, table,
    target, turn, under, Face, GameStateArgs, Py,
};
use crate::fight::battle;
use crate::py::{self, trunc, Random};

/// (in the order the game lists them)
pub const HP: [(&str, i64); 3] = [("FLAIL_KNIGHT_0", 106), ("SPECTRAL_KNIGHT_0", 95), ("MAGI_KNIGHT_0", 87)];

/// (damage a hit, hits, Block it gains, Strength it gains, kind) on round `r`.
pub fn intent(eid: &str, r: i64, st: i64) -> (i64, i64, i64, i64, &'static str) {
    if eid == "FLAIL_KNIGHT_0" {
        return [(15 + st, 1, 0, 0, "Attack"), (9 + st, 2, 0, 0, "Attack"), (15 + st, 1, 0, 0, "Attack"), (0, 0, 0, 3, "Buff")][((r - 1) % 4) as usize];
    }
    if eid == "SPECTRAL_KNIGHT_0" {
        return if r == 1 { (0, 0, 0, 0, "Debuff") } else { [(15, 1, 0, 0, "Attack"), (3, 3, 0, 0, "Attack"), (15, 1, 0, 0, "Attack")][((r - 2) % 3) as usize] };
    }
    if r == 1 {
        (6, 1, 6, 0, "Attack")
    } else if r == 2 {
        (0, 0, 0, 0, "Debuff")
    } else {
        [(10, 1, 0, 0, "Attack"), (0, 0, 6, 0, "Defend"), (35, 1, 0, 0, "Attack")][((r - 3) % 3) as usize]
    }
}

/// kind: (id, name, type, cost, text, target)
pub fn CARDS(c: char, d: i64) -> Face {
    match c {
        'A' => face("STRIKE_R", "Strike", "Attack", 1, &format!("Deal {d} damage."), "AnyEnemy"),
        'H' => face("HEAVY", "Heavy Blow", "Attack", 2, &format!("Deal {} damage.", trunc(2.2 * d as f64)), "AnyEnemy"),
        'V' => face("BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy"),
        'B' => face("DEFEND_R", "Defend", "Skill", 1, &format!("Gain {d} Block."), "Self"),
        'F' => face("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
        _ => panic!("KeyError: {}", py::str_repr(&c.to_string())),
    }
}

#[derive(Clone, Debug)]
pub struct Knight {
    pub id: &'static str,
    pub hp: i64,
    pub block: i64,
    pub st: i64,
    pub vuln: i64,
}

/// (Python's defaults: maxhp 75, no trace.)
pub fn fight(spec: &[i64], rng: &mut Random, maxhp: i64, trace: bool) -> Py {
    let (da, db) = (spec[0], spec[1]);
    let (mut draw, mut disc, mut hp, mut order) = (shuffled(rng, "ABHVF", &spec[2..]), Vec::new(), maxhp, String::new());
    let mut E: Vec<Knight> = HP.iter().map(|(k, v)| Knight { id: k, hp: *v, block: 0, st: 0, vuln: 0 }).collect();
    let face = |c: char| CARDS(c, if c == 'B' { db } else { da });
    let mut hexed = false;
    for r in 1..25 {
        let (mut hand, mut en, mut block) = (deal(&mut draw, &mut disc, rng, 5), 3, 0);

        let state = |E: &[Knight], hand: &[char], draw: &[char], disc: &[char], hp: i64, block: i64, en: i64| -> Value {
            let mut enemies = Vec::new();
            for (n, e) in E.iter().enumerate() {
                if e.hp > 0 {
                    let (d, h, _, _, kind) = intent(e.id, r, e.st);
                    let mut intents = vec![if d != 0 { attack(d, h) } else { json!({"type": kind, "label": ""}) }];
                    if e.id == "MAGI_KNIGHT_0" && r == 1 {
                        intents.push(json!({"type": "Defend", "label": ""}));
                    }
                    let st = status(&[("STRENGTH_POWER", e.st), ("VULNERABLE_POWER", e.vuln)]);
                    enemies.push(json!({"entity_id": e.id, "hp": e.hp, "max_hp": HP[n].1, "block": e.block, "status": st, "intents": intents}));
                }
            }
            let hex_mark = if hexed { vec![("keywords", json!([{"name": "Hexed", "description": "Add Ethereal to this card."}]))] } else { vec![] };
            game_state(
                "elite",
                json!({"act": 3, "floor": 44, "ascension": 8}),
                r,
                enemies,
                hand_cards(hand, &face, "F", &hex_mark),
                &piles(&face),
                draw,
                disc,
                GameStateArgs { hp, maxhp, block, energy: en, status: if hexed { vec![json!({"id": "HEX_POWER", "amount": 2})] } else { vec![] }, extra: vec![] },
            )
        };

        for _ in 0..12 {
            if E.iter().all(|e| e.hp <= 0) {
                break;
            }
            let Some((i, c, act)) = turn(&state(&E, &hand, &draw, &disc, hp, block, en), &hand) else { break };
            if "AHV".contains(c) {
                let t = target(&act, &E.iter().map(|e| (e.id, e.hp)).collect::<Vec<_>>());
                let e = E.iter_mut().find(|e| e.id == t).unwrap();
                let d = match c {
                    'A' => da,
                    'H' => trunc(2.2 * da as f64),
                    _ => 8,
                };
                hit(&mut e.hp, &mut e.block, if e.vuln != 0 { trunc(d as f64 * 1.5) } else { d });
                if c == 'V' && e.hp > 0 {
                    e.vuln += 2;
                }
                if e.hp == 0 {
                    order.push_str(&t[..1]);
                }
            }
            block += if c == 'B' { db } else { 0 };
            en -= CARDS(c, da).3;
            disc.push(hand.remove(i));
            if trace {
                out!("   play {} {} {}", c, aimed_text(&act), Py::Dict(E.iter().map(|e| (Py::str(&e.id[..1]), Py::Int(e.hp))).collect()).repr());
            }
        }
        if E.iter().all(|e| e.hp <= 0) {
            return Py::Tuple(vec![Py::Bool(true), Py::Int(hp), Py::Int(r), Py::Str(order)]);
        }
        let lost = 0.max(E.iter().filter(|e| e.hp > 0).map(|e| intent(e.id, r, e.st)).map(|(d, h, ..)| d * h).sum::<i64>() - block);
        hp -= lost;
        if trace {
            out!("r{r} took {lost} hp {hp} | {}", E.iter().map(|e| format!("{} {}", &e.id[..1], e.hp)).collect::<Vec<_>>().join(" "));
        }
        if !hexed {
            disc.append(&mut hand); // Ethereal: what is left in the hand is Exhausted
        }
        if hp <= 0 {
            return Py::Tuple(vec![Py::Bool(false), Py::Int(0), Py::Int(r), Py::Str(order)]);
        }
        for e in E.iter_mut() {
            e.block = 0;
            if e.hp > 0 {
                let (_, _, gain, st, _) = intent(e.id, r, e.st);
                e.block += gain;
                e.st += st;
                e.vuln = 0.max(e.vuln - 1);
            }
        }
        hexed = E[1].hp > 0;
    }
    Py::Tuple(vec![Py::Bool(false), Py::Int(0), Py::Int(24), Py::Str(order)])
}

/// The arm's settings on this thread from now on (see `modelfight::under`).
pub fn arm_patch(arm: &str) -> Result<(), String> {
    if arm.contains('=') {
        let (name, value) = name_value(arm)?;
        setting(name, float(value)?)?;
    }
    if arm == "off" {
        battle::set_known_threat(Some(IndexMap::new()));
    }
    Ok(())
}

pub fn run(arm: &str, spec: &[i64], n: i64, seed: i64) -> Result<Py, String> {
    let res = under(|| arm_patch(arm), || played(|spec: &[i64], rng| fight(spec, rng, 75, false), spec, n, seed))?;
    let first = counter(res.iter().map(|o| Py::str(o.at(3).as_str().get(..1).unwrap_or("-"))));
    Ok(Py::Tuple(vec![Py::str(arm), Py::ints(spec), Py::Float(mean(&res, 0)), Py::Float(mean(&res, 1)), Py::Float(mean(&res, 2)), first]))
}

/// damage, Block, Strikes, Defends, heavy, Bash, dead (an act 3 deck: 35 to 55 a round against these three)
pub fn DECKS() -> Vec<Vec<i64>> {
    vec![vec![16, 12, 6, 5, 2, 1, 1], vec![18, 12, 6, 5, 2, 1, 1], vec![16, 15, 5, 6, 2, 1, 1], vec![20, 13, 6, 5, 2, 1, 2], vec![18, 15, 5, 5, 3, 1, 1], vec![22, 12, 6, 5, 2, 1, 1]]
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let decks = DECKS();
    if args.first().map(String::as_str) == Some("trace") {
        let seed = if args.len() > 1 { int(&args[1])? } else { 1 };
        let dk = if args.len() > 2 && !args[2].is_empty() && args[2].chars().all(|ch| ch.is_ascii_digit()) { index(&decks, int(&args[2])?)? } else { &decks[1] };
        let arm = if args.iter().skip(2).any(|a| a == "off") { "off" } else { "on" };
        let row = under(|| arm_patch(arm), || fight(dk, &mut Random::new(seed as i128), 75, true))?;
        out!("{}", row.repr());
    } else if args.first().map(String::as_str) == Some("flips") {
        let i = if args.len() > 1 { int(&args[1])? } else { 5 };
        let deck = index(&decks, i)?;
        for (seed, res) in flips(i, &["off".to_string(), "on".to_string()], None, &|arm, rng| under(|| arm_patch(arm), || fight(deck, rng, 75, false)))? {
            if res.get(&Py::str("off")).unwrap().at(0) != res.get(&Py::str("on")).unwrap().at(0) {
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

    const GOLDEN: &str = include_str!("../../tests/golden/scripts/knightfight.txt");

    #[test]
    fn as_python() {
        for command in ["trace 2 3", "trace 4 0 off", "trace", "3", "2 off on ETHEREAL_KEEP=0.9 DEBUFF_CAP=1", "flips"] {
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
