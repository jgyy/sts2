//! The Queen and her Torch Head Amalgam (an act 3 boss) as a model fight, played by the bot's own planner.
//!
//! ```text
//! scripts bosses/queenfight [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
//!                                              battle/combat/triggers), K.name=VALUE (a setting of
//!                                              learn::CHOICES), joined by "+" for several
//! scripts bosses/queenfight trace [SEED] [DECK] [ARM]   one fight, play by play
//! scripts bosses/queenfight flips DECK ARM [ARM]        the fights one arm wins and the other loses
//! ```
//!
//! The fight is the game data's and the logs' (Ascension 0-7 numbers): round 1 Puppet Strings (Chains of Binding 3: the
//! first 3 cards drawn each turn are Bound, one Bound card a turn), round 2 You Are Mine (99 Weak, Frail and Vulnerable on
//! us), then Burn Bright for Me (the Amalgam +1 Strength, the Queen 20 Block) while the Amalgam lives; from the round after
//! it dies, Off With Your Head 3x5, Execution 15, Enrage +2 Strength. The Amalgam (a minion: it leaves when she dies) hits
//! 26, 18, then 8x3, 14, 14 over and over. Our Strikes read 25% less from round 3 (Weak) and Defends 25% less (Frail).

use serde_json::{json, Value};

use super::bosses_aeonfight::flip_arms;
use super::modelfight::{
    aimed, attack, card_of, differ, exit_code, face, flips, game_state, index, int, knob_setting, mean, out, patched, piles, played, quiet, shuffled, status, table, through, trace_args, turn, under,
    Face, GameStateArgs, Py, UNPLAYABLE,
};
use crate::fight::battle;
use crate::py::{self, floordiv, trunc, Random};

pub const QUEEN_HP: i64 = 400;
pub const AMALGAM_HP: i64 = 199;
pub const QUEEN_BLOCK: i64 = 20;

/// `{"can_play": False, "unplayable_reason": "BlockedByHook"}`
pub fn BOUND_OUT() -> Vec<(&'static str, Value)> {
    vec![("can_play", json!(false)), ("unplayable_reason", json!("BlockedByHook"))]
}

/// kind: (id, name, type, cost, text, target); d, b, v are what a Strike, a Defend and Bash read now (Weak, Frail in)
pub fn CARDS(c: char, d: i64, b: i64, v: i64) -> Face {
    match c {
        'A' => face("STRIKE_R", "Strike", "Attack", 1, &format!("Deal {d} damage."), "AnyEnemy"),
        'H' => face("HEAVY", "Heavy Blow", "Attack", 2, &format!("Deal {} damage.", trunc(2.2 * d as f64)), "AnyEnemy"),
        'V' => face("BASH", "Bash", "Attack", 2, &format!("Deal {v} damage. Apply 2 Vulnerable."), "AnyEnemy"),
        'W' => face("SUCKER_PUNCH", "Sucker Punch", "Attack", 1, &format!("Deal {} damage. Apply 2 Weak.", floordiv(d, 2)), "AnyEnemy"),
        'B' => face("DEFEND_R", "Defend", "Skill", 1, &format!("Gain {b} Block."), "Self"),
        'D' => face("BACKFLIP", "Backflip", "Skill", 1, &format!("Gain {} Block. Draw 2 cards.", floordiv(b * 2, 3)), "Self"),
        'F' => face("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
        _ => panic!("KeyError: {}", py::str_repr(&c.to_string())),
    }
}
pub const KINDS: &str = "ABHVWDF";

/// Damage a card of kind c does when Strikes read d and Bash v.
pub fn dealt(c: char, d: i64, v: i64) -> i64 {
    match c {
        'A' => d,
        'H' => trunc(2.2 * d as f64),
        'V' => v,
        'W' => floordiv(d, 2),
        _ => panic!("KeyError: {}", py::str_repr(&c.to_string())),
    }
}

/// (damage a hit, hits) of the Amalgam on round r, Strength in.
pub fn amalgam_move(r: i64, st: i64) -> (i64, i64) {
    if r == 1 {
        return (26 + st, 1);
    }
    if r == 2 {
        return (18 + st, 1);
    }
    [(8 + st, 3), (14 + st, 1), (14 + st, 1)][((r - 3) % 3) as usize]
}

/// (damage a hit, hits, kind) of the Queen's k-th attack-cycle round (from 0).
pub fn queen_move(k: i64, st: i64) -> (i64, i64, &'static str) {
    [(3 + st, 5, "Attack"), (15 + st, 1, "Attack"), (0, 0, "Buff")][(k % 3) as usize]
}

#[derive(Clone, Debug)]
pub struct Foe {
    pub hp: i64,
    pub block: i64,
    pub st: i64,
    pub vuln: i64,
    pub weak: i64,
}

fn off(on: bool, share: f64) -> f64 {
    if on {
        share
    } else {
        1.0
    }
}

/// (Python's defaults: maxhp 80, no trace.)
pub fn fight(spec: &[i64], rng: &mut Random, maxhp: i64, trace: bool) -> Py {
    let (da, db, energy, size) = (spec[0], spec[1], spec[spec.len() - 2], spec[spec.len() - 1]);
    let (mut draw, mut disc, mut hp) = (shuffled(rng, KINDS, &spec[2..spec.len() - 2]), Vec::new(), maxhp);
    let mut q = Foe { hp: QUEEN_HP, block: 0, st: 0, vuln: 0, weak: 0 };
    let mut a = Foe { hp: AMALGAM_HP, block: 0, st: 0, vuln: 0, weak: 0 };
    let (mut chains, mut debuffed) = (false, false);
    let mut k: Option<i64> = None; // the Queen's attack cycle: None until the Amalgam is gone
    for r in 1..=40 {
        let weak_us = debuffed;
        let (d_now, b_now, v_now) = if debuffed { (trunc(da as f64 * 0.75), trunc(db as f64 * 0.75), 6) } else { (da, db, 8) };
        let face = |c: char| CARDS(c, d_now, b_now, v_now);
        let (mut hand, mut bound) = (Vec::new(), Vec::new());
        for n in 0..size {
            if draw.is_empty() {
                draw = std::mem::take(&mut disc);
                rng.shuffle(&mut draw);
            }
            if let Some(c) = draw.pop() {
                hand.push(c);
                bound.push(chains && n < 3);
            }
        }
        let (mut en, mut block, mut bound_used) = (energy, 0, false);
        if trace {
            out!(
                "r{r} hand {} hp {hp} | amalgam {} st {} | queen {}/{}blk st {} k {}",
                hand.iter().zip(&bound).map(|(c, b)| format!("{c}{}", if *b { "*" } else { "" })).collect::<String>(),
                a.hp,
                a.st,
                q.hp,
                q.block,
                q.st,
                k.map(|k| k.to_string()).unwrap_or_else(|| "None".to_string())
            );
        }

        let intents = |a: &Foe, q: &Foe| -> Vec<Vec<Value>> {
            let mut out = Vec::new();
            if a.hp > 0 {
                let (d, h) = amalgam_move(r, a.st);
                out.push(vec![attack(trunc(d as f64 * off(a.weak != 0, 0.75) * off(debuffed, 1.5)), h)]);
            }
            let qi = if r == 1 {
                vec![json!({"type": "CardDebuff", "label": ""})]
            } else if r == 2 {
                vec![json!({"type": "Debuff", "label": ""})]
            } else if let Some(k) = k {
                let (d, h, _kind) = queen_move(k, q.st);
                if h != 0 {
                    vec![attack(trunc(d as f64 * off(q.weak != 0, 0.75) * 1.5), h)]
                } else {
                    vec![json!({"type": "Buff", "label": ""})]
                }
            } else {
                vec![json!({"type": "Buff", "label": ""}), json!({"type": "Defend", "label": ""})]
            };
            out.push(qi);
            out
        };

        let state = |a: &Foe, q: &Foe, hand: &[char], bound: &[bool], bound_used: bool, draw: &[char], disc: &[char], hp: i64, block: i64, en: i64| -> Value {
            let labels = intents(a, q);
            let mut enemies = Vec::new();
            if a.hp > 0 {
                enemies.push(json!({"entity_id": "TORCH_HEAD_AMALGAM_0", "hp": a.hp, "max_hp": AMALGAM_HP, "block": a.block,
                                    "status": status(&[("MINION_POWER", 1), ("STRENGTH_POWER", a.st), ("VULNERABLE_POWER", a.vuln), ("WEAK_POWER", a.weak)]),
                                    "intents": labels[0]}));
            }
            enemies.push(json!({"entity_id": "QUEEN_0", "hp": q.hp, "max_hp": QUEEN_HP, "block": q.block,
                                "status": status(&[("STRENGTH_POWER", q.st), ("VULNERABLE_POWER", q.vuln), ("WEAK_POWER", q.weak)]), "intents": labels[labels.len() - 1]}));
            let mut cards = Vec::new();
            for (i, (c, b)) in hand.iter().zip(bound).enumerate() {
                let mut extra = if *c == 'F' {
                    UNPLAYABLE()
                } else if *b && bound_used {
                    BOUND_OUT()
                } else {
                    vec![]
                };
                if *b {
                    extra.push(("keywords", json!([{"name": "Bound", "description": "Only 1 Bound card can be played each turn."}])));
                }
                cards.push(card_of(i as i64, &face(*c), &extra));
            }
            let mine = status(&[("CHAINS_OF_BINDING_POWER", 3 * chains as i64), ("FRAIL_POWER", 99 * debuffed as i64), ("WEAK_POWER", 99 * weak_us as i64), ("VULNERABLE_POWER", 99 * debuffed as i64)]);
            game_state(
                "boss",
                json!({"act": 3, "floor": 48, "ascension": 6}),
                r,
                enemies,
                cards,
                &piles(&face),
                draw,
                disc,
                GameStateArgs { hp, maxhp, block, energy: en, status: mine, extra: vec![] },
            )
        };

        for _ in 0..15 {
            let Some((i, c, act)) = turn(&state(&a, &q, &hand, &bound, bound_used, &draw, &disc, hp, block, en), &hand) else { break };
            en -= face(c).3;
            if bound[i] {
                bound_used = true;
            }
            let at_amalgam = aimed(&act) == Some("TORCH_HEAD_AMALGAM_0") && a.hp > 0;
            if "AHVW".contains(c) {
                let foe = if at_amalgam { &mut a } else { &mut q };
                let d = dealt(c, d_now, v_now);
                let d = if foe.vuln != 0 { trunc(d as f64 * 1.5) } else { d };
                let soak = foe.block.min(d);
                foe.block -= soak;
                foe.hp = 0.max(foe.hp - (d - soak));
                if c == 'V' {
                    foe.vuln += 2;
                }
                if c == 'W' {
                    foe.weak += 2;
                }
            } else if "BD".contains(c) {
                block += if c == 'B' { b_now } else { floordiv(b_now * 2, 3) };
            }
            disc.push(hand.remove(i));
            bound.remove(i);
            if c == 'D' {
                for _ in 0..2 {
                    if draw.is_empty() {
                        draw = std::mem::take(&mut disc);
                        rng.shuffle(&mut draw);
                    }
                    if let Some(card) = draw.pop() {
                        hand.push(card);
                        bound.push(false);
                    }
                }
            }
            if trace {
                let at = if "AHVW".contains(c) { format!(" at {}", if at_amalgam { "A" } else { "Q" }) } else { String::new() };
                out!("   play {c}{at}  amalgam {} queen {}/{} en {en} blk {block}", a.hp, q.hp, q.block);
            }
            if q.hp <= 0 {
                return Py::Tuple(vec![Py::Bool(true), Py::Int(hp), Py::Int(r), Py::Bool(a.hp <= 0)]);
            }
        }
        disc.append(&mut hand);
        let before = hp;
        // The enemies' turn: the Amalgam first, then the Queen (her Block from last turn goes when she acts).
        if a.hp > 0 {
            let (d, h) = amalgam_move(r, a.st);
            let d = trunc(d as f64 * off(a.weak != 0, 0.75) * off(debuffed, 1.5));
            for _ in 0..h {
                let lost;
                (block, lost) = through(block, d);
                hp -= lost;
            }
        }
        q.block = 0;
        if r == 1 {
            chains = true;
        } else if r == 2 {
            debuffed = true;
        } else if k.is_none() && a.hp > 0 {
            a.st += 1;
            q.block = QUEEN_BLOCK;
        } else {
            let now = k.unwrap_or(0);
            let (d, h, kind) = queen_move(now, q.st);
            let d = trunc(d as f64 * off(q.weak != 0, 0.75) * off(debuffed, 1.5));
            for _ in 0..h {
                let lost;
                (block, lost) = through(block, d);
                hp -= lost;
            }
            if kind == "Buff" {
                q.st += 2;
            }
            k = Some(now + 1);
        }
        for e in [&mut a, &mut q] {
            (e.vuln, e.weak) = (0.max(e.vuln - 1), 0.max(e.weak - 1));
        }
        if trace {
            out!("r{r} took {} hp {hp}", before - hp);
        }
        if hp <= 0 {
            return Py::Tuple(vec![Py::Bool(false), Py::Int(0), Py::Int(r), Py::Bool(a.hp <= 0)]);
        }
    }
    Py::Tuple(vec![Py::Bool(false), Py::Int(0), Py::Int(40), Py::Bool(a.hp <= 0)])
}

/// NAME=VALUE and K.name=VALUE as modelfight::knob_setting takes them, or the Amalgam's worth as a minion (MW=X; the bot
/// has it at 1.0 while it is the main threat, which it is in this fight).
pub fn setting(name: &str, value: f64) -> Result<(), String> {
    if name != "MW" {
        return knob_setting(name, value);
    }
    battle::set_minion_worth(Some(value));
    Ok(())
}

pub fn run(arm: &str, spec: &[i64], n: i64, seed: i64) -> Result<Py, String> {
    let res = under(|| patched(arm, &setting), || played(|spec: &[i64], rng| fight(spec, rng, 80, false), spec, n, seed))?;
    Ok(Py::Tuple(vec![Py::str(arm), Py::ints(spec), Py::Float(mean(&res, 0)), Py::Float(mean(&res, 1)), Py::Float(mean(&res, 2)), Py::Float(mean(&res, 3))]))
}

/// damage, Block, Strikes, Defends, heavy, Bash, Weak, Backflip, dead, energy, hand size (tuned until the planner wins 10-75%)
pub fn DECKS() -> Vec<Vec<i64>> {
    vec![
        vec![26, 14, 6, 6, 2, 1, 1, 1, 2, 4, 6],
        vec![26, 18, 6, 6, 2, 1, 1, 1, 2, 4, 6],
        vec![26, 18, 6, 6, 2, 1, 1, 1, 2, 5, 5],
        vec![32, 14, 6, 6, 2, 1, 1, 1, 2, 4, 5],
        vec![32, 18, 6, 6, 2, 1, 1, 1, 2, 4, 5],
        vec![36, 14, 6, 6, 2, 1, 1, 1, 2, 4, 5],
    ]
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let decks = DECKS();
    if args.first().map(String::as_str) == Some("trace") {
        let (seed, dk, arm) = trace_args(args, &decks)?;
        let row = under(|| patched(&arm, &setting), || fight(&dk, &mut Random::new(seed as i128), 80, true))?;
        out!("{}", row.repr());
    } else if args.first().map(String::as_str) == Some("flips") {
        let i = int(args.get(1).ok_or("IndexError: list index out of range")?)?;
        let deck = index(&decks, i)?;
        for (seed, res) in flips(i, &flip_arms(args), Some(4), &|arm, rng| under(|| patched(arm, &setting), || fight(deck, rng, 80, false)))? {
            if differ(&res) {
                out!("{seed} {}", res.repr());
            }
        }
    } else {
        let (out, arms) = table(args, &run, &decks, &["on"])?;
        for (arm, rows) in &arms {
            out!("{arm:28}: won {:.3}  hp left {:5.1}  rounds {:4.1}  amalgam dead {:.2}", mean(rows, 2), mean(rows, 3), mean(rows, 4), mean(rows, 5));
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

    const GOLDEN: &str = include_str!("../../tests/golden/scripts/bosses_queenfight.txt");

    #[test]
    fn as_python() {
        for command in ["trace", "trace 5 3 MW=0.4", "trace 2 2 K.caution=0.75+SLIP_WORTH=1", "2 on MW=0.2+HP_BITE=9", "flips 4 on MW=0.3"] {
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
