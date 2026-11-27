//! Aeonglass (the act 3 boss with the Withers) as a model fight, played by the bot's own planner.
//!
//!     scripts bosses/aeonfight [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
//!                                                 battle/combat/triggers), WB=X (the planner's Wither bill
//!                                                 times X), K.name=VALUE (a setting of learn::CHOICES),
//!                                                 joined by "+" for several
//!     scripts bosses/aeonfight trace [SEED] [DECK] [ARM]   one fight, play by play
//!     scripts bosses/aeonfight flips DECK ARM [ARM]        the fights one arm wins and the other loses
//!
//! The fight is the Ascension 0 logs' (`intent`): Artifact 3; Withering Presence puts a Wither in our hand every 6 cards
//! played; a Wither left in the hand takes 3 damage a level (Block first), a level for each time the boss has buffed.

use serde_json::{json, Value};

use super::modelfight::{
    attack, deal, differ, exit_code, face, flips, game_state, hand_cards, hit, index, int, knob_setting, mean, out, patched, piles, played, quiet, shuffled, status, table, through, trace_args, turn,
    under, Face, GameStateArgs, Py,
};
use crate::fight::combat;
use crate::py::{self, floordiv, trunc, Random};

pub const BOSS_HP: i64 = 512;
pub const BOSS_BLOCK: i64 = 33;
pub const ARTIFACT: i64 = 3;
pub const EVERY: i64 = 6;

/// (hits, damage a hit before Weak, Block gained, kind) for the k-th round of the cycle (k from 0).
pub fn intent(k: i64, st: i64) -> (i64, i64, i64, &'static str) {
    [(1, 22 + st, BOSS_BLOCK, "Attack+Defend"), (2, 11 + st, 0, "Attack"), (0, 0, 0, "StatusCard+Buff")][(k % 3) as usize]
}

/// kind: (id, name, type, cost, text, target)
pub fn CARDS(c: char, d: i64) -> Face {
    match c {
        'A' => face("STRIKE_R", "Strike", "Attack", 1, &format!("Deal {d} damage."), "AnyEnemy"),
        'H' => face("HEAVY", "Heavy Blow", "Attack", 2, &format!("Deal {} damage.", trunc(2.2 * d as f64)), "AnyEnemy"),
        'V' => face("BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy"),
        'W' => face("SUCKER_PUNCH", "Sucker Punch", "Attack", 1, &format!("Deal {} damage. Apply 2 Weak.", floordiv(d, 2)), "AnyEnemy"),
        'B' => face("DEFEND_R", "Defend", "Skill", 1, &format!("Gain {d} Block."), "Self"),
        'F' => face("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
        _ => panic!("KeyError: {}", py::str_repr(&c.to_string())),
    }
}

pub fn wither(level: i64) -> Face {
    face(
        "WITHER",
        &if level == 0 { "Wither".to_string() } else { format!("Wither+{level}") },
        "Status",
        0,
        &format!("Unplayable. At the end of your turn, if this is in your Hand, take {} damage.", 3 * (level + 1)),
        "None",
    )
}

fn count(cards: &[char], kind: char) -> i64 {
    cards.iter().filter(|c| **c == kind).count() as i64
}

/// (Python's defaults: maxhp 75, no trace.)
pub fn fight(spec: &[i64], rng: &mut Random, maxhp: i64, trace: bool) -> Py {
    let (da, db, energy, size) = (spec[0], spec[1], spec[8], spec[9]);
    let (mut draw, mut disc, mut hp) = (shuffled(rng, "ABHVWF", &spec[2..]), Vec::new(), maxhp);
    let (mut boss_hp, mut boss_block, mut boss_st, mut boss_vuln, mut boss_weak, mut boss_art) = (BOSS_HP, 0, 0, 0, 0, ARTIFACT);
    let (mut level, mut buffs) = (0, 0); // Wither level (the same for all of them); Strength gained so far goes by `buffs`
    let mut left = EVERY; // cards left before the next Wither (Python's `count`)
    for r in 1..=30 {
        let k = (r - 1) % 3;
        let (mut hand, mut en, mut block) = (deal(&mut draw, &mut disc, rng, size), energy, 0);
        let face = |c: char| if c == 'X' { wither(level) } else { CARDS(c, if c == 'B' { db } else { da }) };
        if trace {
            let (hits, d, blk, _) = intent(k, boss_st);
            out!(
                "r{r} hand {} hp {hp} boss {boss_hp}/{boss_block}blk st {boss_st} intent {} withers {} level {level} count {left}",
                hand.iter().collect::<String>(),
                Py::ints(&[hits, d, blk]).repr(),
                count(&draw, 'X') + count(&disc, 'X') + count(&hand, 'X')
            );
        }

        let label = |boss_st: i64, boss_weak: i64| -> Vec<Value> {
            let (hits, d, blk, _kind) = intent(k, boss_st);
            if hits == 0 {
                return vec![json!({"type": "StatusCard", "label": "1"}), json!({"type": "Buff", "label": ""})];
            }
            let mut out = vec![attack(trunc(d as f64 * if boss_weak != 0 { 0.75 } else { 1.0 }), hits)];
            if blk != 0 {
                out.push(json!({"type": "Defend", "label": ""}));
            }
            out
        };

        macro_rules! state {
            () => {{
                let mut st = vec![json!({"id": "WITHERING_PRESENCE_POWER", "amount": left})];
                st.extend(status(&[("ARTIFACT_POWER", boss_art), ("STRENGTH_POWER", boss_st), ("VULNERABLE_POWER", boss_vuln), ("WEAK_POWER", boss_weak)]));
                let enemies = vec![json!({"entity_id": "AEONGLASS_0", "hp": boss_hp, "max_hp": BOSS_HP, "block": boss_block, "status": st, "intents": label(boss_st, boss_weak)})];
                game_state(
                    "boss",
                    json!({"act": 3, "floor": 48, "ascension": 0}),
                    r,
                    enemies,
                    hand_cards(&hand, &face, "FX", &[]),
                    &piles(&face),
                    &draw,
                    &disc,
                    GameStateArgs { hp, maxhp, block, energy: en, status: vec![], extra: vec![] },
                )
            }};
        }

        for _ in 0..20 {
            let Some((i, c, _act)) = turn(&state!(), &hand) else { break };
            en -= CARDS(c, da).3;
            if "AHVW".contains(c) {
                let d = match c {
                    'A' => da,
                    'H' => trunc(2.2 * da as f64),
                    'V' => 8,
                    _ => floordiv(da, 2),
                };
                hit(&mut boss_hp, &mut boss_block, if boss_vuln != 0 { trunc(d as f64 * 1.5) } else { d });
                if "VW".contains(c) {
                    // Artifact swallows the debuff (and is used up)
                    if boss_art != 0 {
                        boss_art -= 1;
                    } else if c == 'V' {
                        boss_vuln += 2;
                    } else {
                        boss_weak += 2;
                    }
                }
            } else if c == 'B' {
                block += db;
            }
            disc.push(hand.remove(i));
            left -= 1;
            if left == 0 {
                // Withering Presence: a Wither lands in the hand
                hand.push('X');
                left = EVERY;
            }
            if trace {
                out!("   play {c}  boss {boss_hp}hp/{boss_block}blk en {en} blk {block} count {left}");
            }
            if boss_hp <= 0 {
                return Py::Tuple(vec![Py::Bool(true), Py::Int(hp), Py::Int(r), Py::Int(buffs)]);
            }
        }
        // End of our turn: each Wither still in the hand burns (Block takes it first); then the boss moves.
        let burn = 3 * (level + 1) * count(&hand, 'X');
        let lost;
        (block, lost) = through(block, burn);
        hp -= lost;
        disc.append(&mut hand);
        let before = hp;
        boss_block = 0; // (its Block from last turn is gone when it acts)
        let (hits, d, blk, kind) = intent(k, boss_st);
        let d = trunc(d as f64 * if boss_weak != 0 { 0.75 } else { 1.0 });
        for _ in 0..hits {
            let lost;
            (block, lost) = through(block, d);
            hp -= lost;
        }
        if blk != 0 {
            boss_block = blk;
        }
        if kind == "StatusCard+Buff" {
            boss_st += 3 + buffs;
            buffs += 1;
            level += 1;
            disc.push('X');
        }
        (boss_vuln, boss_weak) = (0.max(boss_vuln - 1), 0.max(boss_weak - 1));
        if trace {
            out!("r{r} burn {burn} took {} hp {hp} | boss {boss_hp} st {boss_st}", before - hp);
        }
        if hp <= 0 {
            return Py::Tuple(vec![Py::Bool(false), Py::Int(boss_hp), Py::Int(r), Py::Int(buffs)]);
        }
    }
    Py::Tuple(vec![Py::Bool(false), Py::Int(boss_hp), Py::Int(30), Py::Int(buffs)])
}

/// NAME=VALUE and K.name=VALUE as modelfight::knob_setting takes them, or the Wither bill times X (WB=X).
pub fn setting(name: &str, value: f64) -> Result<(), String> {
    if name != "WB" {
        return knob_setting(name, value);
    }
    combat::set_wither_bill_times(Some(value));
    Ok(())
}

pub fn run(arm: &str, spec: &[i64], n: i64, seed: i64) -> Result<Py, String> {
    let res = under(|| patched(arm, &setting), || played(|spec: &[i64], rng| fight(spec, rng, 75, false), spec, n, seed))?;
    Ok(Py::Tuple(vec![Py::str(arm), Py::ints(spec), Py::Float(mean(&res, 0)), Py::Float(mean(&res, 1)), Py::Float(mean(&res, 2))]))
}

/// damage, Block, Strikes, Defends, heavy, Bash, Weak, dead, energy, hand size (tuned until the planner wins 5-55%)
pub fn DECKS() -> Vec<Vec<i64>> {
    vec![
        vec![24, 14, 6, 6, 2, 1, 1, 1, 4, 6],
        vec![26, 14, 6, 6, 2, 1, 1, 1, 4, 5],
        vec![28, 14, 6, 6, 2, 1, 1, 1, 4, 5],
        vec![26, 14, 6, 6, 2, 1, 1, 1, 4, 6],
        vec![34, 14, 6, 6, 2, 1, 1, 1, 3, 5],
        vec![34, 14, 6, 6, 2, 1, 1, 1, 3, 6],
    ]
}

/// `(sys.argv[3:] + ["on", "on"])[:2]`: the two arms of `flips`.
pub fn flip_arms(args: &[String]) -> Vec<String> {
    args.iter().skip(2).cloned().chain(["on".to_string(), "on".to_string()]).take(2).collect()
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let decks = DECKS();
    if args.first().map(String::as_str) == Some("trace") {
        let (seed, dk, arm) = trace_args(args, &decks)?;
        let row = under(|| patched(&arm, &setting), || fight(&dk, &mut Random::new(seed as i128), 75, true))?;
        out!("{}", row.repr());
    } else if args.first().map(String::as_str) == Some("flips") {
        let i = int(args.get(1).ok_or("IndexError: list index out of range")?)?;
        let deck = index(&decks, i)?;
        for (seed, res) in flips(i, &flip_arms(args), Some(3), &|arm, rng| under(|| patched(arm, &setting), || fight(deck, rng, 75, false)))? {
            if differ(&res) {
                out!("{seed} {}", res.repr());
            }
        }
    } else {
        let (out, arms) = table(args, &run, &decks, &["on"])?;
        for (arm, rows) in &arms {
            out!("{arm:28}: won {:.3}  boss HP left {:6.1}  rounds {:4.1}", mean(rows, 2), mean(rows, 3), mean(rows, 4));
        }
        for dk in &decks {
            let dk = Py::ints(dk);
            out!("{} {}", dk.repr(), out.iter().filter(|o| *o.at(1) == dk).map(|o| format!("{}: won {:.2}", o.at(0).as_str(), o.at(2).float())).collect::<Vec<_>>().join("  "));
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

    const GOLDEN: &str = include_str!("../../tests/golden/scripts/bosses_aeonfight.txt");

    #[test]
    fn as_python() {
        for command in ["trace", "trace 3 4 WB=3+K.caution=1.35", "trace 7 0 WITHER_FIGHT=5+HP_SLOPE=1", "2 on WB=0+NEXT_ENERGY=1 K.debuff_worth=3", "flips 1 on WB=4"] {
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
