//! The Test Subject (the act 3 boss in three forms) as a model fight, played by the bot's own planner.
//!
//! ```text
//! scripts bosses/subjectfight [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
//!                                                battle/combat/triggers), K.name=VALUE (a setting of
//!                                                learn::CHOICES), joined by "+" for several
//! scripts bosses/subjectfight trace [SEED] [DECK] [ARM]   one fight, play by play
//! scripts bosses/subjectfight flips DECK ARM [ARM]        the fights one arm wins and the other loses
//! ```
//!
//! The fight is the Ascension 9 logs' (`intent`): form 1 (111 HP) Enrage, form 2 (212) Painful Stabs (a Wound for each hit
//! that gets through), form 3 (313) Nemesis (Intangible every other round) and Burns. A form killed does not attack; the
//! next starts the round after, with its Strength and debuffs at nothing.

use serde_json::{json, Value};

use super::bosses_aeonfight::flip_arms;
use super::modelfight::{
    attack, deal, differ, exit_code, face, flips, game_state, hand_cards, index, int, knob_setting, mean, mean_of, out, patched, piles, played, quiet, shuffled, status, table, through, trace_args,
    turn, under, Face, GameStateArgs, Py,
};
use crate::py::{self, floordiv, trunc, Random};

pub const FORM_HP: [i64; 3] = [111, 212, 313];
// Ascension 9
pub const ENRAGE: i64 = 3;
pub const HIT: i64 = 11;
pub const BURNS: i64 = 5;
pub const BUFF: i64 = 3;

/// (damage a hit before Weak/Vulnerable, hits, Burns, Strength gained, kind) on the k-th round of a form (k from 0).
pub fn intent(form: i64, k: i64, st: i64) -> (i64, i64, i64, i64, &'static str) {
    if form == 1 {
        return if k % 2 == 0 { (22 + st, 1, 0, 0, "Attack") } else { (16 + st, 1, 0, 0, "Attack+Debuff") };
    }
    if form == 2 {
        return (HIT, 3 + k, 0, 0, "Attack");
    }
    [(HIT + st, 3, 0, 0, "Attack"), (45 + st, 1, 0, 0, "Attack"), (0, 0, BURNS, BUFF, "StatusCard+Buff")][(k % 3) as usize]
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
        'U' => face("WOUND", "Wound", "Status", 0, "Unplayable.", "None"),
        'R' => face("BURN", "Burn", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 2 damage.", "Self"),
        _ => panic!("KeyError: {}", py::str_repr(&c.to_string())),
    }
}

fn count(cards: &[char], kind: char) -> i64 {
    cards.iter().filter(|c| **c == kind).count() as i64
}

fn off(on: bool, share: f64) -> f64 {
    if on {
        share
    } else {
        1.0
    }
}

/// `lost_in`: {form: HP lost to it}.
fn by_form(lost_in: &[i64; 3]) -> Py {
    Py::Dict((1..=3).map(|f| (Py::Int(f), Py::Int(lost_in[f as usize - 1]))).collect())
}

/// (Python's defaults: maxhp 75, no trace.)
pub fn fight(spec: &[i64], rng: &mut Random, maxhp: i64, trace: bool) -> Py {
    let (da, db, energy, size) = (spec[0], spec[1], spec[8], spec[9]);
    let (mut draw, mut disc, mut hp) = (shuffled(rng, "ABHVWF", &spec[2..]), Vec::new(), maxhp);
    let (mut form, mut k) = (1, 0); // form, and rounds it has had
    let (mut e_hp, mut e_st, mut e_vuln, mut e_weak) = (FORM_HP[0], 0, 0, 0);
    let mut our_vuln = 0; // rounds of Vulnerable on us
    let mut lost_in = [0, 0, 0];
    let face = |c: char| CARDS(c, if c == 'B' { db } else { da });
    for r in 1..40 {
        let (mut hand, mut en, mut block) = (deal(&mut draw, &mut disc, rng, size), energy, 0);
        let intangible = form == 3 && k % 2 == 0;
        if trace {
            let (d, h, ..) = intent(form, k, e_st);
            out!("r{r} F{form} hand {} hp {hp} boss {e_hp} intent {}{}", hand.iter().collect::<String>(), Py::ints(&[d, h]).repr(), if intangible { " INTANGIBLE" } else { "" });
        }

        let label = |e_st: i64, e_weak: i64| -> Vec<Value> {
            let (d, h, burns, _, kind) = intent(form, k, e_st);
            if d == 0 {
                return vec![json!({"type": "StatusCard", "label": burns.to_string()}), json!({"type": "Buff", "label": ""})];
            }
            let mut out = vec![attack(trunc(d as f64 * off(e_weak != 0, 0.75) * off(our_vuln != 0, 1.5)), h)];
            if kind.contains("Debuff") {
                out.push(json!({"type": "Debuff", "label": ""}));
            }
            out
        };

        let state = |e_hp: i64, e_st: i64, e_vuln: i64, e_weak: i64, hand: &[char], draw: &[char], disc: &[char], hp: i64, block: i64, en: i64| -> Value {
            let st = status(&[
                ("ADAPTABLE_POWER", (form < 3) as i64),
                (["ENRAGE_POWER", "PAINFUL_STABS_POWER", "NEMESIS_POWER"][form as usize - 1], if form == 1 { ENRAGE } else { 1 }),
                ("INTANGIBLE_POWER", intangible as i64),
                ("STRENGTH_POWER", e_st),
                ("VULNERABLE_POWER", e_vuln),
                ("WEAK_POWER", e_weak),
            ]);
            let enemies = vec![json!({"entity_id": "TEST_SUBJECT_0", "hp": e_hp, "max_hp": FORM_HP[form as usize - 1], "block": 0, "status": st, "intents": label(e_st, e_weak)})];
            game_state(
                "boss",
                json!({"act": 3, "floor": 48, "ascension": 9}),
                r,
                enemies,
                hand_cards(hand, &face, "FUR", &[]),
                &piles(&face),
                draw,
                disc,
                GameStateArgs { hp, maxhp, block, energy: en, status: status(&[("VULNERABLE_POWER", our_vuln)]), extra: vec![] },
            )
        };

        for _ in 0..15 {
            let Some((i, c, _act)) = turn(&state(e_hp, e_st, e_vuln, e_weak, &hand, &draw, &disc, hp, block, en), &hand) else { break };
            en -= CARDS(c, da).3;
            if "AHVW".contains(c) {
                let d = match c {
                    'A' => da,
                    'H' => trunc(2.2 * da as f64),
                    'V' => 8,
                    _ => floordiv(da, 2),
                };
                e_hp = 0.max(
                    e_hp - if intangible {
                        1
                    } else if e_vuln != 0 {
                        trunc(d as f64 * 1.5)
                    } else {
                        d
                    },
                );
                if c == 'V' {
                    e_vuln += 2;
                } else if c == 'W' {
                    e_weak += 2;
                }
            } else if c == 'B' {
                block += db;
                e_st += if form == 1 { ENRAGE } else { 0 };
            }
            disc.push(hand.remove(i));
            if trace {
                out!("   play {c}  F{form} {e_hp}hp st {e_st} en {en} blk {block}");
            }
            if e_hp <= 0 {
                break;
            }
        }
        let killed = e_hp <= 0;
        if killed && form == 3 {
            return Py::Tuple(vec![Py::Bool(true), Py::Int(hp), Py::Int(r), Py::Int(3), by_form(&lost_in)]);
        }
        let burning = count(&hand, 'R');
        disc.append(&mut hand);
        let before = hp;
        if killed {
            (form, k) = (form + 1, 0);
            (e_hp, e_st, e_vuln, e_weak) = (FORM_HP[form as usize - 1], 0, 0, 0);
            our_vuln = 0.max(our_vuln - 1);
            if trace {
                out!("r{r} form {} killed: no attack | hp {hp}", form - 1);
            }
            continue;
        }
        // End of our turn: each Burn in the hand burns (Block takes it first), then the boss acts.
        let burnt;
        (block, burnt) = through(block, 2 * burning);
        hp -= burnt;
        let (d, h, burns, gain, kind) = intent(form, k, e_st);
        let d = trunc(d as f64 * off(e_weak != 0, 0.75) * off(our_vuln != 0, 1.5));
        for _ in 0..h {
            let lost;
            (block, lost) = through(block, d);
            hp -= lost;
            if form == 2 && lost > 0 {
                disc.push('U');
            }
        }
        for _ in 0..burns {
            disc.push('R');
        }
        e_st += gain;
        lost_in[form as usize - 1] += before - hp;
        if trace {
            out!(
                "r{r} F{form} took {} hp {hp} | boss {e_hp} st {e_st} | deck {} wounds {} burns {}",
                before - hp,
                draw.len() + disc.len(),
                count(&draw, 'U') + count(&disc, 'U'),
                count(&draw, 'R') + count(&disc, 'R')
            );
        }
        if hp <= 0 {
            return Py::Tuple(vec![Py::Bool(false), Py::Int(0), Py::Int(r), Py::Int(form), by_form(&lost_in)]);
        }
        our_vuln = if kind.contains("Debuff") { 1 } else { 0.max(our_vuln - 1) };
        (e_vuln, e_weak) = (0.max(e_vuln - 1), 0.max(e_weak - 1));
        k += 1;
    }
    Py::Tuple(vec![Py::Bool(false), Py::Int(0), Py::Int(40), Py::Int(form), by_form(&lost_in)])
}

pub fn run(arm: &str, spec: &[i64], n: i64, seed: i64) -> Result<Py, String> {
    let res = under(|| patched(arm, &knob_setting), || played(|spec: &[i64], rng| fight(spec, rng, 75, false), spec, n, seed))?;
    let died = Py::Dict((1..=3).map(|f| (Py::Int(f), Py::Int(res.iter().filter(|o| !o.at(0).truthy() && o.at(3).int() == f).count() as i64))).collect());
    let lost = Py::Dict((1..=3).map(|f| (Py::Int(f), Py::Float(res.iter().map(|o| o.at(4).get(&Py::Int(f)).unwrap().int()).sum::<i64>() as f64 / n as f64))).collect());
    Ok(Py::Tuple(vec![Py::str(arm), Py::ints(spec), Py::Float(mean(&res, 0)), Py::Float(mean(&res, 1)), Py::Float(mean(&res, 2)), died, lost]))
}

/// damage, Block, Strikes, Defends, heavy, Bash, Weak, dead, energy, hand size (tuned until the planner wins 0-60%)
pub fn DECKS() -> Vec<Vec<i64>> {
    vec![
        vec![24, 15, 6, 5, 2, 1, 1, 2, 4, 6],
        vec![20, 14, 6, 6, 2, 1, 2, 2, 5, 6],
        vec![28, 18, 6, 6, 2, 1, 2, 2, 4, 6],
        vec![30, 18, 6, 6, 2, 1, 1, 2, 4, 6],
        vec![24, 16, 6, 6, 2, 1, 2, 2, 5, 6],
        vec![28, 16, 6, 6, 2, 1, 2, 2, 5, 5],
    ]
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let decks = DECKS();
    if args.first().map(String::as_str) == Some("trace") {
        let (seed, dk, arm) = trace_args(args, &decks)?;
        let row = under(|| patched(&arm, &knob_setting), || fight(&dk, &mut Random::new(seed as i128), 75, true))?;
        out!("{}", row.repr());
    } else if args.first().map(String::as_str) == Some("flips") {
        let i = int(args.get(1).ok_or("IndexError: list index out of range")?)?;
        let deck = index(&decks, i)?;
        for (seed, res) in flips(i, &flip_arms(args), Some(4), &|arm, rng| under(|| patched(arm, &knob_setting), || fight(deck, rng, 75, false)))? {
            if differ(&res) {
                out!("{seed} {}", res.repr());
            }
        }
    } else {
        let (out, arms) = table(args, &run, &decks, &["on"])?;
        for (arm, rows) in &arms {
            let died = Py::Dict((1..=3).map(|f| (Py::Int(f), Py::Int(rows.iter().map(|o| o.at(5).get(&Py::Int(f)).unwrap().int()).sum()))).collect());
            let lost = (1..=3).map(|f| format!("{:.0}", mean_of(rows.iter().map(|o| o.at(6).get(&Py::Int(f)).unwrap()), rows.len()))).collect::<Vec<_>>().join(" ");
            out!("{arm:28}: won {:.3}  hp left {:5.1}  rounds {:4.1}  died in form {}  hp lost by form {lost}", mean(rows, 2), mean(rows, 3), mean(rows, 4), died.repr());
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

    const GOLDEN: &str = include_str!("../../tests/golden/scripts/bosses_subjectfight.txt");

    #[test]
    fn as_python() {
        for command in ["trace", "trace 4 2 FORM_WORTH=0+STAB_SCAR=30", "trace 9 5 K.caution=1.35+ENRAGE_PRICE=0", "3 on ENRAGE_PRICE=9+K.debuff_worth=1 STAB_SCAR=0", "flips 2 on FORM_WORTH=0"] {
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
