//! The Fool's Sadness in a boss fight, as a model played by the bot's own planner.
//!
//! ```text
//! scripts bosses/sadfight [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
//!                                            battle/combat/triggers), S.NAME=VALUE (a constant of
//!                                            fight/fool/sadness.rs), K.name=VALUE (a setting), joined by "+"
//! scripts bosses/sadfight trace [SEED] [DECK] [ARM]   one fight, play by play
//! scripts bosses/sadfight flips DECK ARM [ARM]        the fights one arm wins and the other loses
//! ```
//!
//! A Fool deck with Sorrowful Strike (13 damage, 2 Sadness), Mourning Blow (0 cost, 10 damage, then a Sadness itself) and
//! Ennui (a Sadness and 2 more cards every turn) against an act 1 boss on a fixed cycle (12, 6x2, 16, +2 Strength, 10) that
//! goes Intangible every fifth round, as Soul Fysh does. Sadness is Unplayable and Retained, and a hand holds 10: from
//! 2026-11-25 19:03, the Fool lost to Soul Fysh on 25 HP with ten Sadness in the hand from round 10.

use serde_json::{json, Value};

use crate::scripts::bosses::aeonfight::flip_arms;
use crate::scripts::modelfight::{
    attack, card_of, differ, exit_code, face, flips, game_state, index, int, knob_setting, mean, out, patched, piles, played, quiet, set_sadness, shuffled, status, table, through, trace_args, turn,
    under, Face, GameStateArgs, Datum, UNPLAYABLE,
};
use crate::base::{self, Random};

pub const HAND_CAP: usize = 10;

/// kind: (id, name, type, cost, text, target)
pub fn CARDS(c: char) -> Face {
    match c {
        'A' => face("STRIKE_FOOL", "Strike", "Attack", 1, "Deal {d} damage.", "AnyEnemy"),
        'H' => face("HEAVY", "Heavy Blow", "Attack", 2, "Deal {h} damage.", "AnyEnemy"),
        'B' => face("DEFEND_FOOL", "Defend", "Skill", 1, "Gain {b} Block.", "Self"),
        'S' => face("SORROWFUL_STRIKE", "Sorrowful Strike", "Attack", 1, "Deal {s} damage. Add 2 Sadness into your Hand.", "AnyEnemy"),
        'M' => face("MOURNING_BLOW", "Mourning Blow", "Attack", 0, "Deal {m} damage. Transform this into Sadness and return it to your Hand.", "AnyEnemy"),
        'E' => face("ENNUI", "Ennui", "Power", 1, "At the start of your turn, add 1 Sadness to your Hand and draw 2 additional cards.", "Self"),
        'X' => face("SADNESS", "Sadness", "Status", 0, "Unplayable. Retain.", "None"),
        'F' => face("ASCENDERS_BANE", "Ascender's Bane", "Curse", -1, "Unplayable. Ethereal.", "None"),
        _ => panic!("KeyError: {}", base::str_repr(&c.to_string())),
    }
}

pub fn DAMAGE(c: char) -> Option<i64> {
    match c {
        'A' => Some(6),
        'H' => Some(14),
        'S' => Some(13),
        'M' => Some(10),
        _ => None,
    }
}
pub const KINDS: &str = "AHBSMEF";

/// (damage a hit, hits, kind) on round r (from 1), Strength in.
pub fn intent(r: i64, st: i64) -> (i64, i64, &'static str) {
    [(12 + st, 1, "Attack"), (6 + st, 2, "Attack"), (16 + st, 1, "Attack"), (0, 0, "Buff"), (10 + st, 1, "Attack")][((r - 1) % 5) as usize]
}

fn count(cards: &[char], kind: char) -> i64 {
    cards.iter().filter(|c| **c == kind).count() as i64
}

/// (the original's defaults: maxhp 77, no trace.)
pub fn fight(spec: &[i64], rng: &mut Random, maxhp: i64, trace: bool) -> Datum {
    let (boss_hp, db, energy) = (spec[0], spec[1], spec[spec.len() - 1]);
    let (mut draw, mut disc, mut hp) = (shuffled(rng, KINDS, &spec[2..spec.len() - 1]), Vec::new(), maxhp);
    let (mut e_hp, mut e_st) = (boss_hp, 0);
    let (mut hand, mut ennui, mut peak): (Vec<char>, i64, i64) = (Vec::new(), 0, 0);
    // (`str(x).format(d=.., h=.., s=.., m=.., b=db)` of every part of the card)
    let face = |c: char| -> Face {
        let f = CARDS(c);
        let text = f.4.replace("{d}", &DAMAGE('A').unwrap().to_string()).replace("{h}", &DAMAGE('H').unwrap().to_string()).replace("{s}", &DAMAGE('S').unwrap().to_string());
        let text = text.replace("{m}", &DAMAGE('M').unwrap().to_string()).replace("{b}", &db.to_string());
        (f.0, f.1, f.2, f.3, text, f.5)
    };
    for r in 1..=30 {
        hand.retain(|c| *c == 'X'); // (only Sadness is Retained; the Bane is Ethereal and the rest discarded)
        if ennui != 0 && hand.len() < HAND_CAP {
            hand.push('X');
        }
        for _ in 0..5 + 2 * ennui {
            if hand.len() >= HAND_CAP {
                break;
            }
            if draw.is_empty() {
                draw = std::mem::take(&mut disc);
                rng.shuffle(&mut draw);
            }
            let Some(c) = draw.pop() else { break };
            hand.push(c);
        }
        peak = peak.max(count(&hand, 'X'));
        let (mut en, mut block) = (energy, 0);
        let intangible = r % 5 == 0;
        if trace {
            out!("r{r} hand {} hp {hp} boss {e_hp} st {e_st} ennui {ennui}{}", hand.iter().collect::<String>(), if intangible { " INTANGIBLE" } else { "" });
        }

        let state = |e_hp: i64, e_st: i64, ennui: i64, hand: &[char], draw: &[char], disc: &[char], hp: i64, block: i64, en: i64| -> Value {
            let (d, h, _kind) = intent(r, e_st);
            let labels = if h != 0 { vec![attack(d, h)] } else { vec![json!({"type": "Buff", "label": ""})] };
            let foes = vec![json!({"entity_id": "SOUL_FYSH_0", "hp": e_hp, "max_hp": boss_hp, "block": 0,
                                   "status": status(&[("STRENGTH_POWER", e_st), ("INTANGIBLE_POWER", intangible as i64)]), "intents": labels})];
            let cards = hand.iter().enumerate().map(|(i, c)| card_of(i as i64, &face(*c), &if "XF".contains(*c) { UNPLAYABLE() } else { vec![] })).collect();
            game_state(
                "boss",
                json!({"act": 1, "floor": 17, "ascension": 5}),
                r,
                foes,
                cards,
                &piles(&face),
                draw,
                disc,
                GameStateArgs { hp, maxhp, block, energy: en, status: status(&[("ENNUI_POWER", 2 * ennui)]), extra: vec![] },
            )
        };

        for _ in 0..15 {
            let Some((i, c, _act)) = turn(&state(e_hp, e_st, ennui, &hand, &draw, &disc, hp, block, en), &hand) else { break };
            en -= CARDS(c).3;
            hand.remove(i);
            if let Some(d) = DAMAGE(c) {
                e_hp = 0.max(e_hp - if intangible { 1 } else { d });
            }
            if c == 'B' {
                block += db;
            }
            if c == 'S' {
                for _ in 0..2.min(HAND_CAP as i64 - hand.len() as i64) {
                    hand.push('X');
                }
            }
            if c == 'M' {
                if hand.len() < HAND_CAP {
                    hand.push('X');
                }
            } else if c == 'E' {
                ennui = 1;
            } else {
                disc.push(c);
            }
            if trace {
                out!("   play {c}  boss {e_hp} en {en} blk {block} sadness {}", count(&hand, 'X'));
            }
            if e_hp <= 0 {
                return Datum::Tuple(vec![Datum::Bool(true), Datum::Int(hp), Datum::Int(r), Datum::Int(peak)]);
            }
        }
        disc.extend(hand.iter().filter(|c| !"XF".contains(**c)));
        let before = hp;
        let (d, h, kind) = intent(r, e_st);
        for _ in 0..h {
            let lost;
            (block, lost) = through(block, d);
            hp -= lost;
        }
        if kind == "Buff" {
            e_st += 2;
        }
        if trace {
            out!("r{r} took {} hp {hp}", before - hp);
        }
        if hp <= 0 {
            return Datum::Tuple(vec![Datum::Bool(false), Datum::Int(0), Datum::Int(r), Datum::Int(peak)]);
        }
    }
    Datum::Tuple(vec![Datum::Bool(false), Datum::Int(0), Datum::Int(30), Datum::Int(peak)])
}

/// S.NAME=VALUE: a constant of fight/fool/sadness.rs; anything else as modelfight::knob_setting takes it.
pub fn setting(name: &str, value: f64) -> Result<(), String> {
    match name.strip_prefix("S.") {
        Some(constant) => set_sadness(constant, value),
        None => knob_setting(name, value),
    }
}

pub fn run(arm: &str, spec: &[i64], n: i64, seed: i64) -> Result<Datum, String> {
    let res = under(|| patched(arm, &setting), || played(|spec: &[i64], rng| fight(spec, rng, 77, false), spec, n, seed))?;
    Ok(Datum::Tuple(vec![Datum::str(arm), Datum::ints(spec), Datum::Float(mean(&res, 0)), Datum::Float(mean(&res, 1)), Datum::Float(mean(&res, 2)), Datum::Float(mean(&res, 3))]))
}

/// boss HP, Block, Strikes, Heavy, Defends, Sorrowful, Mourning, Ennui, Bane, energy (tuned until the planner wins 15-75%)
pub fn DECKS() -> Vec<Vec<i64>> {
    vec![
        vec![150, 6, 4, 1, 4, 2, 1, 1, 1, 3],
        vec![165, 6, 4, 1, 4, 2, 1, 1, 1, 3],
        vec![180, 6, 4, 1, 4, 2, 1, 1, 1, 3],
        vec![180, 6, 4, 1, 4, 2, 1, 0, 1, 3],
        vec![180, 7, 3, 2, 4, 3, 1, 1, 1, 3],
        vec![200, 7, 3, 2, 4, 3, 1, 1, 1, 3],
    ]
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let decks = DECKS();
    if args.first().map(String::as_str) == Some("trace") {
        let (seed, dk, arm) = trace_args(args, &decks)?;
        let row = under(|| patched(&arm, &setting), || fight(&dk, &mut Random::new(seed as i128), 77, true))?;
        out!("{}", row.repr());
    } else if args.first().map(String::as_str) == Some("flips") {
        let i = int(args.get(1).ok_or("IndexError: list index out of range")?)?;
        let deck = index(&decks, i)?;
        for (seed, res) in flips(i, &flip_arms(args), Some(4), &|arm, rng| under(|| patched(arm, &setting), || fight(deck, rng, 77, false)))? {
            if differ(&res) {
                out!("{seed} {}", res.repr());
            }
        }
    } else {
        let (out, arms) = table(args, &run, &decks, &["on"])?;
        for (arm, rows) in &arms {
            out!("{arm:28}: won {:.3}  hp left {:5.1}  rounds {:4.1}  peak Sadness {:4.1}", mean(rows, 2), mean(rows, 3), mean(rows, 4), mean(rows, 5));
        }
        for dk in &decks {
            let dk = Datum::ints(dk);
            out!("{} {}", dk.repr(), out.iter().filter(|o| *o.at(1) == dk).map(|o| format!("{}: won {:.2} sad {:3.1}", o.at(0).as_str(), o.at(2).float(), o.at(5).float())).collect::<Vec<_>>().join("  "));
        }
    }
    Ok(())
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(script(&args))
}

#[cfg(test)]
mod tests {
    use crate::scripts::modelfight::{golden, section};
    use super::main;

    const GOLDEN: &str = include_str!("../../../tests/golden/scripts/bosses_sadfight.txt");

    #[test]
    fn as_recorded() {
        for command in ["trace", "trace 3 4 S.LONG=1+S.LATER=0.2", "trace 6 0 K.caution=1.35+S.SPARE=0.9", "3 on S.LONG=1 S.PLAYED=2+HP_BASE=2.5", "flips 2 on S.LONG=1"] {
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
