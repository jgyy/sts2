//! The Lagavulin Matriarch (the act 1 boss that starts Asleep) as a model fight, played by the bot's own planner.
//!
//! ```text
//! scripts bosses/matriarchfight [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
//!                                                  battle/combat/triggers),
//!                                                  K.name=VALUE (a setting of learn::CHOICES), joined by "+"
//! scripts bosses/matriarchfight trace [SEED] [DECK] [ARM]   one fight, play by play
//! ```
//!
//! The fight is the Ascension 8 logs': 233 HP, Asleep 3 with Plating 12 (Block 12, 12, 11 on the sleeping turns; both gone
//! when she wakes); awake she plays Slash 19, Disembowel 9x2, Slash 12 with 14 Block, Soul Siphon (2 Strength to her, 2
//! Strength and 2 Dexterity off us) in that order. Losing HP wakes her at once and she does nothing that turn.
//!
//! 2026-11-28: the planner waits out her first two sleeping turns (set-up cards only). Waking her early whenever the hand
//! beat what the hands to come were likely to deal won 63% of 12,000 fights against 68% for waiting; always on turn 1, 48%.

use serde_json::{json, Value};

use crate::scripts::modelfight::{attack, deal, exit_code, face, game_state, hand_cards, hit, knob_setting, mean, out, patched, piles, played, quiet, shuffled, status, table, through, trace_args, turn, under, Face, GameStateArgs, Datum};
use crate::base::{self, trunc, Random};

pub const BOSS_HP: i64 = 233;
pub const SLEEP: i64 = 3;

/// (hits, damage a hit, Block gained, kind) for her k-th turn awake (k from 0).
pub fn intent(k: i64, st: i64) -> (i64, i64, i64, &'static str) {
    [(1, 19 + st, 0, "Attack"), (2, 9 + st, 0, "Attack"), (1, 12 + st, 14, "Attack+Defend"), (0, 0, 0, "Debuff+Buff")][(k % 4) as usize]
}

/// kind: (id, name, type, cost, text, target); `d` after our Strength or Dexterity
pub fn CARDS(c: char, d: i64) -> Face {
    match c {
        'A' => face("STRIKE_R", "Strike", "Attack", 1, &format!("Deal {} damage.", d.max(0)), "AnyEnemy"),
        'H' => face("HEAVY", "Heavy Blow", "Attack", 2, &format!("Deal {} damage.", d.max(0)), "AnyEnemy"),
        'T' => face("TWIN", "Twin Blow", "Attack", 1, &format!("Deal {} damage 2 times.", d.max(0)), "AnyEnemy"),
        'B' => face("DEFEND_R", "Defend", "Skill", 1, &format!("Gain {} Block.", d.max(0)), "Self"),
        'P' => face("INFLAME", "Inflame", "Power", 1, "Gain 2 Strength.", "Self"),
        'F' => face("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
        _ => panic!("KeyError: {}", base::str_repr(&c.to_string())),
    }
}

/// (the original-style defaults: maxhp 77, no trace.) Returns (won, HP left: ours or hers, rounds, the round she woke in).
pub fn fight(spec: &[i64], rng: &mut Random, maxhp: i64, trace: bool) -> Datum {
    let (da, db, energy, size) = (spec[0], spec[1], spec[8], spec[9]);
    let (mut draw, mut disc, mut hp) = (shuffled(rng, "ABHTPF", &spec[2..8]), Vec::new(), maxhp);
    let (mut boss_hp, mut boss_st, mut st, mut dex) = (BOSS_HP, 0, 0, 0);
    let (mut asleep, mut plating, mut boss_block, mut awake, mut woke) = (SLEEP, 12, 12, 0, 0); // `awake`: her turns taken awake
    for r in 1..=30 {
        let (mut hand, mut en, mut block) = (deal(&mut draw, &mut disc, rng, size), energy, 0);
        let mut stunned = false;
        let dealt = |c: char, st: i64| match c {
            'A' => (da + st, 1),
            'H' => (trunc(2.2 * da as f64) + st, 1),
            _ => (trunc(0.6 * da as f64) + st, 2),
        };
        let face = |c: char, st: i64, dex: i64| match c {
            'B' => CARDS(c, db + dex),
            'A' | 'H' | 'T' => CARDS(c, dealt(c, st).0),
            _ => CARDS(c, 0),
        };
        macro_rules! state {
            () => {{
                let label: Vec<Value> = if asleep > 0 {
                    vec![json!({"type": "Sleep", "label": ""})]
                } else if stunned {
                    vec![json!({"type": "Stun", "label": ""})]
                } else {
                    let (hits, d, blk, _) = intent(awake, boss_st);
                    if hits == 0 {
                        vec![json!({"type": "Debuff", "label": ""}), json!({"type": "Buff", "label": ""})]
                    } else {
                        let mut out = vec![attack(d, hits)];
                        if blk != 0 {
                            out.push(json!({"type": "Defend", "label": ""}));
                        }
                        out
                    }
                };
                let powers = status(&[("PLATING_POWER", plating), ("ASLEEP_POWER", asleep), ("STRENGTH_POWER", boss_st)]);
                let enemies = vec![json!({"entity_id": "LAGAVULIN_MATRIARCH_0", "hp": boss_hp, "max_hp": BOSS_HP, "block": boss_block, "status": powers, "intents": label})];
                let now = |c: char| face(c, st, dex);
                let mut state = game_state(
                    "boss",
                    json!({"act": 1, "floor": 17, "ascension": 8}),
                    r,
                    enemies,
                    hand_cards(&hand, &now, "F", &[]),
                    &piles(&now),
                    &draw,
                    &disc,
                    GameStateArgs { hp, maxhp, block, energy: en, status: status(&[("STRENGTH_POWER", st), ("DEXTERITY_POWER", dex)]), extra: vec![] },
                );
                state["player"]["max_energy"] = json!(energy);
                state
            }};
        }
        if trace {
            out!("r{r} hand {} hp {hp} boss {boss_hp}/{boss_block}blk asleep {asleep} st {boss_st} ours {st}/{dex}", hand.iter().collect::<String>());
        }
        for _ in 0..20 {
            let Some((i, c, _act)) = turn(&state!(), &hand) else { break };
            en -= CARDS(c, 0).3;
            match c {
                'A' | 'H' | 'T' => {
                    let (d, hits) = dealt(c, st);
                    let before = boss_hp;
                    for _ in 0..hits {
                        hit(&mut boss_hp, &mut boss_block, d.max(0));
                    }
                    if boss_hp < before && asleep > 0 {
                        (asleep, plating, stunned, woke) = (0, 0, true, r);
                    }
                }
                'B' => block += (db + dex).max(0),
                _ => st += 2,
            }
            if c == 'P' {
                hand.remove(i); // a Power is played once
            } else {
                disc.push(hand.remove(i));
            }
            if trace {
                out!("   play {c}  boss {boss_hp}hp/{boss_block}blk en {en} blk {block}");
            }
            if boss_hp <= 0 {
                return Datum::Tuple(vec![Datum::Bool(true), Datum::Int(hp), Datum::Int(r), Datum::Int(woke)]);
            }
        }
        disc.append(&mut hand);
        // her turn
        boss_block = 0;
        if asleep > 0 {
            asleep -= 1;
            if asleep == 0 {
                (plating, woke) = (0, r + 1);
            } else {
                if r >= 2 {
                    plating -= 1;
                }
                boss_block = plating;
            }
        } else if !stunned {
            let (hits, d, blk, kind) = intent(awake, boss_st);
            for _ in 0..hits {
                let lost;
                (block, lost) = through(block, d);
                hp -= lost;
            }
            boss_block = blk;
            if kind == "Debuff+Buff" {
                boss_st += 2;
                st -= 2;
                dex -= 2;
            }
            awake += 1;
        }
        if trace {
            out!("r{r} over: hp {hp} | boss {boss_hp}/{boss_block}blk st {boss_st}");
        }
        if hp <= 0 {
            return Datum::Tuple(vec![Datum::Bool(false), Datum::Int(boss_hp), Datum::Int(r), Datum::Int(woke)]);
        }
    }
    Datum::Tuple(vec![Datum::Bool(false), Datum::Int(boss_hp), Datum::Int(30), Datum::Int(woke)])
}

pub fn run(arm: &str, spec: &[i64], n: i64, seed: i64) -> Result<Datum, String> {
    let res = under(|| patched(arm, &knob_setting), || played(|spec: &[i64], rng| fight(spec, rng, 77, false), spec, n, seed))?;
    let lost: Vec<Datum> = res.iter().filter(|r| !r.at(0).truthy()).cloned().collect();
    Ok(Datum::Tuple(vec![Datum::str(arm), Datum::ints(spec), Datum::Float(mean(&res, 0)), Datum::Float(if lost.is_empty() { 0.0 } else { mean(&lost, 1) }), Datum::Float(mean(&res, 2)), Datum::Float(mean(&res, 3))]))
}

/// damage, Block, Strikes, Defends, heavy, twin, Inflame, dead, energy, hand size (tuned until the planner wins 15-85%)
pub fn DECKS() -> Vec<Vec<i64>> {
    vec![
        vec![11, 8, 5, 5, 2, 2, 0, 1, 3, 5],
        vec![12, 8, 5, 5, 2, 2, 0, 1, 3, 5],
        vec![10, 8, 5, 5, 1, 1, 2, 1, 3, 5],
        vec![12, 9, 4, 6, 3, 3, 1, 2, 3, 5],
        vec![10, 8, 5, 4, 3, 3, 1, 0, 3, 5],
        vec![13, 9, 6, 7, 2, 2, 1, 3, 3, 5],
    ]
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let decks = DECKS();
    if args.first().map(String::as_str) == Some("trace") {
        let (seed, dk, arm) = trace_args(args, &decks)?;
        let row = under(|| patched(&arm, &knob_setting), || fight(&dk, &mut Random::new(seed as i128), 77, true))?;
        out!("{}", row.repr());
    } else {
        let (out, arms) = table(args, &run, &decks, &["on"])?;
        for (arm, rows) in &arms {
            out!("{arm:28}: won {:.3}  her HP left when lost {:6.1}  rounds {:4.1}  woke in round {:.2}", mean(rows, 2), mean(rows, 3), mean(rows, 4), mean(rows, 5));
        }
        for dk in &decks {
            let dk = Datum::ints(dk);
            out!("{} {}", dk.repr(), out.iter().filter(|o| *o.at(1) == dk).map(|o| format!("{}: won {:.2}", o.at(0).as_str(), o.at(2).float())).collect::<Vec<_>>().join("  "));
        }
    }
    Ok(())
}

pub fn main(args: Vec<String>) -> i32 {
    exit_code(script(&args))
}
