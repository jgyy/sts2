//! An ordinary act 2 fight as a model: the bot's own planner against the play that keeps the most HP.
//!
//! ```text
//! scripts roomfight [FIGHTS A CELL]      HP lost and deaths by the planner, next to the best play
//! scripts roomfight trace [SEED] [FOE] [DECK] [HP]
//!                                        one fight, turn by turn, with the best play beside ours
//! ... NAME=VALUE                         with a constant of combat.rs or closing.rs, or a setting
//!                                        of learn.rs, set to that (caution=1.35, HP_BASE=2.0)
//! ```
//!
//! The best play is the one with the most HP expected at the end (a death counts DEATH below nothing); the planner is measured
//! by the HP it gives away. The enemies are three of act 2 on Ascension 7 (FOES); the decks and the drawing are demonfight's.

use std::collections::HashMap;
use std::rc::Rc;

use serde_json::{json, Value};

use super::demonfight::{self as demon, better, choices, count, hands, plays, take_patches, Deck, DeckArgs, Grid, Patch, Rules};
use super::modelfight::{exit_code, float, index, int, on_all_cores, out, quiet, set_closing_or_combat, turn, under, Datum};
use crate::learn;
use crate::base::{self, floordiv, max_of, Random};

pub const ROUNDS: i64 = 16;
/// what a death counts for, in HP below ending on none: the rest of the run
pub const DEATH: f64 = 60.0;

/// (HP, what it hits for in rounds 1, 2, 3 ..., the hits that is made of; the last three rounds go round again)
pub type Foe = (i64, &'static [i64], &'static [i64]);

/// name -> (HP, what it hits for in rounds 1, 2, 3 ..., the hits that is made of; the last three rounds go round again)
pub const FOES: [(&str, Foe); 3] = [
    ("toad", (117, &[0, 23, 17], &[0, 1, 1])),             // Spiny Toad: Buff, 23, 17
    ("hunter", (121, &[0, 17, 21, 21], &[0, 1, 3, 3])),    // Hunter Killer: Debuff, then 17 and 7x3
    ("louse", (134, &[9, 0, 19, 14, 0, 24, 19, 0, 29, 24, 0, 34, 29, 0, 39, 34], &[1; 16])), // Louse Progenitor: 5 Strength more each time round
];

/// `FOES[foe]`.
pub fn foe_of(foe: &str) -> Foe {
    FOES.iter().find(|(name, _)| *name == foe).unwrap_or_else(|| panic!("KeyError: {}", base::str_repr(foe))).1
}

pub fn rules(foe: &str, r: i64) -> Rules {
    let (_hp, hit, hits) = foe_of(foe);
    let n = hit.len() as i64;
    let i = (if r <= n { r - 1 } else { n - 3 + (r - 1 - n) % 3 }) as usize;
    Rules { inc: hit[i], hits: 1.max(hits[i]), per: floordiv(hit[i], 1.max(hits[i])), heal: 0, burn: 0, strength: 0, limit: 99, draw: 5, kind: 0, drain: 0 }
}

/// HP expected at the end once a turn has dealt `dmg` and cost us `lost`, for every (our HP, the enemy's HP) before it.
pub fn after(V: &Grid, dmg: i64, lost: i64) -> Grid {
    let (hp, b) = (V.rows as i64, V.cols as i64);
    let mut out = Grid::full(V.rows, V.cols, -DEATH);
    let alive: Vec<bool> = (0..b).map(|col| col > dmg).collect();
    let src: Vec<usize> = (0..b).map(|col| (col - dmg).clamp(0, b - 1) as usize).collect();
    if lost < hp {
        for i in 0..(hp - lost) as usize {
            let (from, to) = (V.row(i), out.row_mut(i + lost as usize));
            for col in 0..b as usize {
                to[col] = if alive[col] { from[src[col]] } else { 0.0 };
            }
        }
    }
    for i in 0..hp as usize {
        // it died first: we keep what we have
        let to = out.row_mut(i);
        for col in 0..b as usize {
            if !alive[col] {
                to[col] = i as f64;
            }
        }
    }
    out.row_mut(0).fill(-DEATH);
    out
}

/// V[r][our HP, the enemy's HP]: the HP the best play expects to end on from the start of round r (a death: -DEATH).
pub fn solve(deck: &Deck, foe: &str) -> Vec<Rc<Grid>> {
    let top = foe_of(foe).0;
    let (rows, cols) = (deck.maxhp as usize + 1, top as usize + 1);
    let mut V: Vec<Rc<Grid>> = vec![Rc::new(Grid::full(0, 0, 0.0)); ROUNDS as usize + 2];
    V[ROUNDS as usize + 1] = Rc::new(Grid::full(rows, cols, -DEATH));
    for r in (1..=ROUNDS).rev() {
        let rl = rules(foe, r);
        let mut cache: HashMap<(i64, i64), Grid> = HashMap::new();
        let mut total = Grid::full(rows, cols, 0.0);
        for (hand, p) in hands(deck, rl.draw, 0) {
            let mut best = None;
            for (dmg, blk, _n) in plays(deck, &hand, deck.energy, rl.limit, 0) {
                let key = (dmg, 0.max(rl.inc - blk));
                let then = cache.entry(key).or_insert_with(|| after(&V[r as usize + 1], key.0, key.1));
                best = better(best, then);
            }
            let best = best.expect("TypeError: unsupported operand type(s) for *: 'float' and 'NoneType'");
            for (t, x) in total.data.iter_mut().zip(&best) {
                *t += p * x;
            }
        }
        total.row_mut(0).fill(-DEATH);
        V[r as usize] = Rc::new(total);
    }
    V
}

pub fn value(V: &[Rc<Grid>], r: i64, rl: &Rules, hp: i64, foe_hp: i64, dmg: i64, blk: i64) -> f64 {
    if foe_hp - dmg <= 0 {
        return hp as f64;
    }
    let left = hp - 0.max(rl.inc - blk);
    if left <= 0 || r + 1 > ROUNDS {
        return -DEATH;
    }
    V[r as usize + 1].at(left, foe_hp - dmg)
}

pub fn state(deck: &Deck, foe: &str, r: i64, rl: &Rules, hp: i64, foe_hp: i64, hand: &[char], energy: i64, block: i64, played: i64) -> Value {
    let mut st = demon::state(deck, r, rl, hp, foe_hp, hand, energy, block, played, 0);
    st["state_type"] = json!("monster");
    st["run"]["floor"] = json!(24);
    let enemy = &mut st["battle"]["enemies"][0];
    enemy["entity_id"] = json!("MODEL_FOE_0");
    enemy["max_hp"] = json!(foe_of(foe).0);
    st
}

/// (round, HP given away, damage and Block beyond the best play's, what was coming, HP share)
pub type Given = (i64, f64, i64, i64, i64, f64);

/// One fight by the planner: (HP left or None for a death, rounds, HP given away turn by turn).
pub fn fight(deck: &Deck, foe: &str, hp: i64, V: &[Rc<Grid>], rng: &mut Random, trace: bool) -> (Option<i64>, i64, Vec<Given>) {
    let (mut hp, mut foe_hp, mut given) = (hp, foe_of(foe).0, Vec::new());
    for r in 1..=ROUNDS {
        let rl = rules(foe, r);
        let mut hand = choices(rng, &deck.weights(0), rl.draw);
        let counts: [i64; 6] = std::array::from_fn(|k| count(&hand, demon::KINDS.as_bytes()[k] as char));
        let (mut energy, mut block, mut dealt, mut played) = (deck.energy, 0, 0, 0);
        let mut shown = hand.clone();
        shown.sort();
        let shown: String = shown.into_iter().collect();
        for _ in 0..12 {
            if foe_hp - dealt <= 0 {
                break;
            }
            let Some((i, c, _act)) = turn(&state(deck, foe, r, &rl, hp, foe_hp - dealt, &hand, energy, block, played), &hand) else { break };
            hand.remove(i);
            played += 1;
            if c == 'B' {
                block += deck.db;
            } else {
                dealt += deck.hit(c, 0);
            }
            energy -= match c {
                'A' | 'B' => 1,
                'H' => 2,
                'Z' => 0,
                _ => panic!("KeyError: {}", base::str_repr(&c.to_string())),
            };
        }
        let ours = value(V, r, &rl, hp, foe_hp, dealt, block);
        let best = max_of(plays(deck, &counts, deck.energy, rl.limit, 0).into_iter().map(|(d, b, _n)| (value(V, r, &rl, hp, foe_hp, d, b), d, b))).expect("ValueError: max() iterable argument is empty");
        given.push((r, if best.0 - ours > 0.0 { best.0 - ours } else { 0.0 }, dealt - best.1, block - best.2, rl.inc, hp as f64 / deck.maxhp as f64));
        if trace {
            out!("r{r:2} hp {hp:3} foe {foe_hp:3} coming {:2} hand {shown:5} | ours {dealt:2} dmg {block:2} blk ({ours:6.2})  best {:2} dmg {:2} blk ({:6.2})", rl.inc, best.1, best.2, best.0);
        }
        foe_hp -= dealt;
        if foe_hp <= 0 {
            return (Some(hp), r, given);
        }
        hp -= 0.max(rl.inc - block);
        if hp <= 0 {
            return (None, r, given);
        }
    }
    (None, ROUNDS, given)
}

/// act 2 decks as the bot brings them: about 26 enemy HP a round, an ordinary fight of four or five rounds
pub fn DECKS() -> Vec<Deck> {
    vec![
        Deck::new([6, 3, 2, 6, 3, 0], DeckArgs { da: 9, db: 8, maxhp: 75, ..Default::default() }),
        Deck::new([6, 3, 2, 6, 3, 0], DeckArgs { da: 11, db: 9, maxhp: 75, ..Default::default() }),
        Deck::new([7, 4, 2, 4, 3, 0], DeckArgs { da: 10, db: 8, maxhp: 75, ..Default::default() }),
        Deck::new([5, 2, 2, 8, 3, 0], DeckArgs { da: 10, db: 9, maxhp: 75, ..Default::default() }),
    ]
}
/// the share of Max HP the fight is entered on
pub const STARTS: [f64; 3] = [1.0, 0.6, 0.35];

/// The constants and settings on this thread from now on.
pub fn patch(values: &Patch) -> Result<(), String> {
    for (k, v) in values {
        if learn::DEFAULTS.contains_key(k.as_str()) {
            learn::set_knob(k, *v);
        } else {
            set_closing_or_combat(k, *v)?;
        }
    }
    Ok(())
}

/// For each share of STARTS: (what the best play ends on, the HP entered on, the fights).
pub type Cells = Vec<(f64, (f64, i64, Vec<(Option<i64>, i64, Vec<Given>)>))>;

pub fn job(args: &(usize, &'static str, i64, i64, Patch)) -> Result<(usize, &'static str, Cells), String> {
    let (d, foe, n, seed, values) = args;
    patch(values)?;
    let deck = &DECKS()[*d];
    let V = solve(deck, foe);
    let mut out = Cells::new();
    for share in STARTS {
        let hp = 1.max(base::round(share * deck.maxhp as f64));
        let mut rng = Random::new(*seed as i128);
        out.push((share, (V[1].at(hp, foe_of(foe).0), hp, (0..*n).map(|_| fight(deck, foe, hp, &V, &mut rng, false)).collect())));
    }
    Ok((*d, foe, out))
}

pub fn run(n: i64, values: &Patch) -> Result<Vec<(usize, &'static str, Cells)>, String> {
    let jobs: Vec<(usize, &'static str, i64, i64, Patch)> =
        (0..DECKS().len()).flat_map(|d| FOES.iter().enumerate().map(move |(i, (foe, _))| (d, *foe, n, 500 + 31 * d as i64 + i as i64, values.clone()))).collect();
    on_all_cores(&|| {}, &job, &jobs, 8).into_iter().collect()
}

/// `f"{share:.0%}"`.
fn percent(share: f64) -> String {
    format!("{:.0}%", share * 100.0)
}

pub fn table(out: &[(usize, &'static str, Cells)]) {
    out!("each cell: HP the planner lost a fight / the best play's, and deaths in a hundred (a death counts as all the HP and {DEATH:.0} more)");
    let mut tot: Vec<(f64, f64, i64, i64, f64)> = STARTS.iter().map(|_| (0.0, 0.0, 0, 0, 0.0)).collect();
    for (at, share) in STARTS.iter().enumerate() {
        out!("entered on {} of Max HP", percent(*share));
        for (d, deck) in DECKS().iter().enumerate() {
            let mut cells = Vec::new();
            for (_d, foe, res) in out.iter().filter(|o| o.0 == d) {
                let (best, hp, fights) = &res.iter().find(|(s, _)| s == share).unwrap().1;
                let mut lost = base::Sum::new(0.0);
                for f in fights {
                    match f.0 {
                        Some(left) => lost.add_i(hp - left),
                        None => lost.add_f(*hp as f64 - -DEATH),
                    }
                }
                let lost = lost.get() / fights.len() as f64;
                let dead = fights.iter().filter(|f| f.0.is_none()).count() as i64;
                cells.push(format!("{foe} {lost:5.1}/{:5.1} †{:3.0}", *hp as f64 - best, (100 * dead) as f64 / fights.len() as f64));
                let t = &mut tot[at];
                t.0 += lost * fights.len() as f64;
                t.1 += (*hp as f64 - best) * fights.len() as f64;
                t.2 += dead;
                t.3 += fights.len() as i64;
                t.4 += fights.iter().map(|f| f.1).sum::<i64>() as f64;
            }
            out!("  {:44} {}", deck.repr(), cells.join("   "));
        }
    }
    for (share, (lost, best, dead, n, rounds)) in STARTS.iter().zip(&tot) {
        let n = *n as f64;
        out!("all, entered on {}: planner lost {:.2} a fight, the best play {:.2}; deaths {:.1}%; rounds {:.2}", percent(*share), lost / n, best / n, (100 * dead) as f64 / n, rounds / n);
    }
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let (values, args) = take_patches(args)?;
    if args.first().map(String::as_str) == Some("trace") {
        // `(sys.argv[2:] + [1, "toad", 0, 1.0][len(sys.argv) - 2:])[:4]`
        let given = &args[1..];
        let seed = if !given.is_empty() { int(&given[0])? } else { 1 };
        let foe = if given.len() > 1 { given[1].as_str() } else { "toad" };
        let d = if given.len() > 2 { int(&given[2])? } else { 0 };
        let share = if given.len() > 3 { float(&given[3])? } else { 1.0 };
        let foe = FOES.iter().find(|(name, _)| *name == foe).ok_or_else(|| format!("KeyError: {}", base::str_repr(foe)))?.0;
        under(
            || patch(&values),
            || -> Result<(), String> {
                let decks = DECKS();
                let deck = index(&decks, d)?;
                let hp = 1.max(base::round(share * deck.maxhp as f64));
                let V = solve(deck, foe);
                out!("{} {foe} the best play ends on {} of {hp}", deck.repr(), base::float_repr(base::round_to(V[1].at(hp, foe_of(foe).0), 2)));
                let (left, rounds, _) = fight(deck, foe, hp, &V, &mut Random::new(seed as i128), true);
                out!("{}", Datum::Tuple(vec![left.map(Datum::Int).unwrap_or(Datum::None), Datum::Int(rounds)]).repr());
                Ok(())
            },
        )??;
    } else {
        table(&run(if !args.is_empty() { int(&args[0])? } else { 100 }, &values)?);
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

    const GOLDEN: &str = include_str!("../../tests/golden/scripts/roomfight.txt");

    #[test]
    fn as_recorded() {
        for command in ["trace", "trace 3 louse 2 0.35", "trace 7 hunter 3 0.6 caution=1.35 HP_BASE=2.0", "2", "3 CLOSE_REACH=0 caution=0.75 HP_BITE=1"] {
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
