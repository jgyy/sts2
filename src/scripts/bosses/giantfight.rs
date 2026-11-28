//! The Waterfall Giant (the act 1 boss that blows up after it is beaten) as a model fight, played by the bot's own planner.
//!
//! ```text
//! scripts bosses/giantfight [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of battle/combat/triggers
//!                                              or of fight/eruption.rs), K.name=VALUE (a setting of learn::CHOICES), joined by "+"
//! scripts bosses/giantfight trace [SEED] [DECK] [ARM]   one fight, play by play
//! ```
//!
//! The fight is the Ascension 9 logs': 250 HP; Pressurize on turn 1 (Steam Eruption 20), then Stomp 16 with 1 Weak on us, Ram 11,
//! Siphon (heals 15), Pressure Gun 23 (5 more each time), Pressure Up 14, round and round, and every one of them adds 3 to the
//! Steam Eruption. Beaten, it stands out of reach, Stunned for its turn, and at the end of our next turn it deals its Steam
//! Eruption: an attack, so Block stops it and Weak on the Giant takes a quarter off (its Weak wears off a turn in the Stun too).
//!
//! 2026-11-28: 7 of the bot's 9 losses to it in the logs came after the kill, with 4 to 27 HP against a blast of 38 to 56.

use serde_json::{json, Value};

use crate::scripts::modelfight::{attack, deal, exit_code, face, game_state, hand_cards, hit, knob_setting, mean, out, patched, piles, played, quiet, shuffled, status, table, through, trace_args, turn, under, Face, GameStateArgs, Datum};
use crate::base::{self, trunc, Random};
use crate::fight::eruption;

pub const BOSS_HP: i64 = 250;
pub const STEAM: i64 = 20;
pub const OUT_OF_REACH: i64 = 999_999_999;

/// (damage, Weak on us, healed, kind) for its k-th turn after Pressurize (k from 0); `guns`: Pressure Guns fired so far.
pub fn intent(k: i64, guns: i64) -> (i64, i64, i64, &'static str) {
    [(16, 1, 0, "Attack+Debuff+Buff"), (11, 0, 0, "Attack+Buff"), (0, 0, 15, "Heal+Buff"), (23 + 5 * guns, 0, 0, "Attack+Buff"), (14, 0, 0, "Attack+Buff")][(k % 5) as usize]
}

/// kind: (id, name, type, cost, text, target); `d` after our Strength or Weak
pub fn CARDS(c: char, d: i64) -> Face {
    match c {
        'A' => face("STRIKE_R", "Strike", "Attack", 1, &format!("Deal {} damage.", d.max(0)), "AnyEnemy"),
        'H' => face("HEAVY", "Heavy Blow", "Attack", 2, &format!("Deal {} damage.", d.max(0)), "AnyEnemy"),
        'T' => face("TWIN", "Twin Blow", "Attack", 1, &format!("Deal {} damage 2 times.", d.max(0)), "AnyEnemy"),
        'B' => face("DEFEND_R", "Defend", "Skill", 1, &format!("Gain {} Block.", d.max(0)), "Self"),
        'P' => face("INFLAME", "Inflame", "Power", 1, "Gain 2 Strength.", "Self"),
        'F' => face("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
        'W' => face("NEUTRALIZE", "Neutralize", "Attack", 0, &format!("Deal {} damage. Apply 1 Weak.", d.max(0)), "AnyEnemy"),
        _ => panic!("KeyError: {}", base::str_repr(&c.to_string())),
    }
}

/// (the defaults: maxhp 77, no trace.) Returns (won, HP left: ours or its, rounds, the round it was beaten in or 0, lost to the blast).
pub fn fight(spec: &[i64], rng: &mut Random, maxhp: i64, trace: bool) -> Datum {
    let (da, db, energy, size) = (spec[0], spec[1], spec[9], spec[10]);
    let (mut draw, mut disc, mut hp) = (shuffled(rng, "ABHTPFW", &spec[2..9]), Vec::new(), maxhp);
    let (mut boss_hp, mut steam, mut st, mut weak, mut boss_weak) = (BOSS_HP, 0, 0, 0, 0);
    let (mut moved, mut guns, mut beaten) = (0, 0, 0); // `moved`: its turns taken after Pressurize; `beaten`: the round it fell in
    let over = |won: bool, left: i64, r: i64, beaten: i64| Datum::Tuple(vec![Datum::Bool(won), Datum::Int(left), Datum::Int(r), Datum::Int(beaten), Datum::Bool(!won && beaten != 0)]);
    for r in 1..=40 {
        let (mut hand, mut en, mut block) = (deal(&mut draw, &mut disc, rng, size), energy, 0);
        let cut = |d: i64, weak: i64| if weak > 0 { trunc(0.75 * d as f64) } else { d };
        let dealt = |c: char, st: i64, weak: i64| match c {
            'A' => (cut(da + st, weak), 1),
            'H' => (cut(trunc(2.2 * da as f64) + st, weak), 1),
            'W' => (cut(3 + st, weak), 1),
            _ => (cut(trunc(0.6 * da as f64) + st, weak), 2),
        };
        let face = |c: char, st: i64, weak: i64| match c {
            'B' => CARDS(c, db),
            'A' | 'H' | 'T' | 'W' => CARDS(c, dealt(c, st, weak).0),
            _ => CARDS(c, 0),
        };
        macro_rules! state {
            () => {{
                let label: Vec<Value> = if beaten == r {
                    vec![json!({"type": "Stun", "label": ""})]
                } else if beaten != 0 {
                    vec![json!({"type": "DeathBlow", "label": cut(steam, boss_weak).to_string()})]
                } else if r == 1 {
                    vec![json!({"type": "Buff", "label": ""})]
                } else {
                    let (d, _, healed, _) = intent(moved, guns);
                    let mut out = if d != 0 { vec![attack(cut(d, boss_weak), 1)] } else { vec![json!({"type": "Heal", "label": healed.to_string()})] };
                    if intent(moved, guns).1 != 0 {
                        out.push(json!({"type": "Debuff", "label": ""}));
                    }
                    out.push(json!({"type": "Buff", "label": ""}));
                    out
                };
                // (the game takes the Steam Eruption off the Giant once the blast is its intent)
                let powers = status(&[("STEAM_ERUPTION_POWER", if beaten != 0 && beaten != r { 0 } else { steam }), ("WEAK_POWER", boss_weak)]);
                let shown = if beaten != 0 { OUT_OF_REACH } else { boss_hp };
                let enemies = vec![json!({"entity_id": "WATERFALL_GIANT_0", "hp": shown, "max_hp": if beaten != 0 { OUT_OF_REACH } else { BOSS_HP }, "block": 0, "status": powers, "intents": label})];
                let now = |c: char| face(c, st, weak);
                let mut state = game_state(
                    "boss",
                    json!({"act": 1, "floor": 17, "ascension": 9}),
                    r,
                    enemies,
                    hand_cards(&hand, &now, "F", &[]),
                    &piles(&now),
                    &draw,
                    &disc,
                    GameStateArgs { hp, maxhp, block, energy: en, status: status(&[("STRENGTH_POWER", st), ("WEAK_POWER", weak)]), extra: vec![] },
                );
                state["player"]["max_energy"] = json!(energy);
                state
            }};
        }
        if trace {
            out!("r{r} hand {} hp {hp} giant {boss_hp} steam {steam} weak {boss_weak} ours st {st} weak {weak}{}", hand.iter().collect::<String>(), if beaten != 0 { "  (beaten: the blast comes)" } else { "" });
        }
        for _ in 0..20 {
            let Some((i, c, _act)) = turn(&state!(), &hand) else { break };
            en -= CARDS(c, 0).3;
            match c {
                'A' | 'H' | 'T' | 'W' => {
                    let (d, hits) = dealt(c, st, weak);
                    if beaten == 0 {
                        for _ in 0..hits {
                            hit(&mut boss_hp, &mut 0, d.max(0));
                        }
                    }
                    if c == 'W' {
                        boss_weak += 1;
                    }
                }
                'B' => block += db.max(0),
                _ => st += 2,
            }
            if c == 'P' {
                hand.remove(i); // a Power is played once
            } else {
                disc.push(hand.remove(i));
            }
            if trace {
                out!("   play {c}  giant {boss_hp}hp weak {boss_weak} en {en} blk {block}");
            }
            if boss_hp <= 0 && beaten == 0 {
                beaten = r;
            }
        }
        disc.append(&mut hand);
        weak = 0.max(weak - 1);
        // its turn
        if beaten != 0 && beaten != r {
            let (_, lost) = through(block, cut(steam, boss_weak));
            hp -= lost;
            if trace {
                out!("r{r} the blast: {} through {block} Block, hp {hp}", cut(steam, boss_weak));
            }
            return if hp > 0 { over(true, hp, r, beaten) } else { over(false, 0, r, beaten) };
        }
        if beaten == 0 {
            if r == 1 {
                steam = STEAM;
            } else {
                let (d, weakens, healed, _) = intent(moved, guns);
                let (_, lost) = through(block, cut(d, boss_weak));
                hp -= lost;
                weak += weakens;
                boss_hp = BOSS_HP.min(boss_hp + healed);
                guns += (moved % 5 == 3) as i64;
                steam += 3;
                moved += 1;
            }
        }
        boss_weak = 0.max(boss_weak - 1);
        if trace {
            out!("r{r} over: hp {hp} | giant {boss_hp} steam {steam}");
        }
        if hp <= 0 {
            return over(false, boss_hp, r, 0);
        }
    }
    over(false, boss_hp, 40, 0)
}

/// NAME=VALUE: a constant of fight/eruption.rs, else as the other model fights take them.
fn setting(name: &str, value: f64) -> Result<(), String> {
    if eruption::set_tunable(name, value) {
        Ok(())
    } else {
        knob_setting(name, value)
    }
}

pub fn run(arm: &str, spec: &[i64], n: i64, seed: i64) -> Result<Datum, String> {
    let res = under(|| patched(arm, &setting), || played(|spec: &[i64], rng| fight(spec, rng, 77, false), spec, n, seed))?;
    let won: Vec<Datum> = res.iter().filter(|r| r.at(0).truthy()).cloned().collect();
    let share = |i: usize| res.iter().filter(|r| r.at(i).truthy()).count() as f64 / res.len().max(1) as f64;
    Ok(Datum::Tuple(vec![Datum::str(arm), Datum::ints(spec), Datum::Float(mean(&res, 0)), Datum::Float(if won.is_empty() { 0.0 } else { mean(&won, 1) }), Datum::Float(mean(&res, 2)), Datum::Float(share(4))]))
}

/// damage, Block, Strikes, Defends, heavy, twin, Inflame, dead, Neutralize, energy, hand size (tuned until the planner wins 15-85%)
pub fn DECKS() -> Vec<Vec<i64>> {
    vec![
        vec![12, 8, 5, 5, 2, 2, 0, 1, 0, 3, 5],
        vec![12, 8, 5, 5, 2, 2, 0, 1, 1, 3, 5],
        vec![10, 8, 5, 5, 1, 1, 2, 1, 1, 3, 5],
        vec![13, 8, 4, 6, 3, 3, 1, 2, 0, 3, 5],
        vec![11, 8, 5, 4, 3, 3, 1, 0, 2, 3, 5],
        vec![13, 9, 6, 7, 2, 2, 1, 3, 1, 3, 5],
    ]
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let decks = DECKS();
    if args.first().map(String::as_str) == Some("trace") {
        let (seed, dk, arm) = trace_args(args, &decks)?;
        let row = under(|| patched(&arm, &setting), || fight(&dk, &mut Random::new(seed as i128), 77, true))?;
        out!("{}", row.repr());
    } else {
        let (out, arms) = table(args, &run, &decks, &["on"])?;
        for (arm, rows) in &arms {
            out!("{arm:28}: won {:.3}  our HP left when won {:5.1}  rounds {:4.1}  lost to the blast {:.3}", mean(rows, 2), mean(rows, 3), mean(rows, 4), mean(rows, 5));
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
