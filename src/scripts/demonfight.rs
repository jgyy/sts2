//! The Knowledge Demon as a model fight: the bot's own planner against the best play there is.
//!
//! ```text
//! scripts demonfight [FIGHTS A DECK]      won by the planner, next to what perfect play wins
//! scripts demonfight regret [FIGHTS]      the turns where the planner gave most away, by kind
//! scripts demonfight trace [SEED] [DECK]  one fight, turn by turn, with the best play beside ours
//! scripts demonfight energy [FIGHTS]      the same decks on 3 and on 4 energy a turn (what an energy relic is worth here)
//! scripts demonfight cards [FIGHTS]       one more card of each sort, an energy-a-turn Power among them (decks::ENERGY_POWER)
//! ... NAME=VALUE                          with a constant of combat.rs or closing.rs set to that (CLOSE_REACH=0)
//! ```
//!
//! The Demon is the Ascension 7 logs' (`rules`; its three afflictions are settings of the fight). Every hand is drawn afresh
//! from the whole deck, so a position is the round, our HP and the Demon's: `solve` works out perfect play exactly, and each
//! of the planner's turns is priced by the chance of winning it gave away.
//!
//! (The tables numpy held are [`Grid`]s, filled by the same operations in the same order. What the scripts in demon/ put
//! in place of this module's `rules` is [`set_rules`], per thread; their kinds of deck are the `hit_fn`, `pd`, `pc` and
//! `pm` of [`Deck`].)

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{json, Value};

use super::modelfight::{exit_code, face, float, game_state, hand_cards, index, int, on_all_cores, out, piles, quiet, set_closing_or_combat, status, turn, under, Face, GameStateArgs, Py};
use crate::py::{self, floordiv, fsum, max_of, sort_by_key, trunc, Random};

pub const BOSS: i64 = 379;
pub const ROUNDS: i64 = 24;
/// Strike, two-cost attack, free attack, Defend, dead card, Power (1 energy: 3 Strength for the rest of the fight)
pub const KINDS: &str = "AHZBFP";
/// Strength a Power gives: that much more on every attack card
pub const GAIN: i64 = 3;

/// The keyword arguments of `Deck::new`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeckArgs {
    pub da: i64,
    pub db: i64,
    pub dz: i64,
    pub maxhp: i64,
    pub energy: i64,
    pub pe: i64,
    pub ps: i64,
}

impl Default for DeckArgs {
    fn default() -> Self {
        DeckArgs { da: 9, db: 8, dz: 7, maxhp: 75, energy: 3, pe: 0, ps: GAIN }
    }
}

#[derive(Clone, Debug)]
pub struct Deck {
    /// cards of each kind, in KINDS' order
    pub counts: [i64; 6],
    pub da: i64,
    pub db: i64,
    pub dz: i64,
    pub dh: i64,
    pub maxhp: i64,
    pub energy: i64,
    pub powers: i64,
    /// a Power in play: `ps` Strength on every attack card (Inflame: 3), `pe` energy a turn (Demesne: 1)
    pub pe: i64,
    pub ps: i64,
    /// demon/scaling.rs's Deck2: damage a turn per Power in play, the Power's cost, and attacks times (1 + pm) per Power in play
    pub pd: i64,
    pub pc: i64,
    pub pm: f64,
    /// a subclass's `hit` (demon/ancients.rs, demon/scaling.rs), in place of [`Deck::_hit`]
    pub hit_fn: Option<fn(&Deck, char, i64) -> i64>,
}

impl Deck {
    pub fn new(counts: [i64; 6], args: DeckArgs) -> Deck {
        Deck {
            counts,
            da: args.da,
            db: args.db,
            dz: args.dz,
            dh: trunc(2.2 * args.da as f64),
            maxhp: args.maxhp,
            energy: args.energy,
            powers: counts[5],
            pe: args.pe,
            ps: args.ps,
            pd: 0,
            pc: 1,
            pm: 0.0,
            hit_fn: None,
        }
    }

    /// Energy a turn starts with, `s` Powers in play. (Python's default `drain`: 0.)
    pub fn turn_energy(&self, s: i64, drain: i64) -> i64 {
        self.energy - drain + self.pe * s
    }

    /// The chance of each kind in a draw, with `s` Powers already in play (they have left the deck).
    pub fn weights(&self, s: i64) -> Vec<f64> {
        let left: Vec<i64> = KINDS.chars().zip(self.counts).map(|(k, n)| n - if k == 'P' { s } else { 0 }).collect();
        let total: i64 = left.iter().sum();
        left.iter().map(|n| *n as f64 / total as f64).collect()
    }

    pub fn hit(&self, kind: char, s: i64) -> i64 {
        match self.hit_fn {
            Some(hit) => hit(self, kind, s),
            None => self._hit(kind, s),
        }
    }

    /// `Deck.hit` itself, whatever a subclass has in its place.
    pub fn _hit(&self, kind: char, s: i64) -> i64 {
        let base = match kind {
            'A' => self.da,
            'H' => self.dh,
            'Z' => self.dz,
            _ => panic!("KeyError: {}", py::str_repr(&kind.to_string())),
        };
        0.max(base + self.ps * s)
    }

    /// `repr(deck)`.
    pub fn repr(&self) -> String {
        let power = if self.pe != 0 { format!(" P=+{}E{:+}S", self.pe, self.ps) } else { String::new() };
        format!(
            "{} hit {} block {} hp {} energy {}{power}",
            KINDS.chars().zip(self.counts).map(|(k, n)| format!("{k}{n}")).collect::<String>(),
            self.da,
            self.db,
            self.maxhp,
            self.energy
        )
    }
}

/// For each of the three offers, whether the burn was taken.
pub type Burns = (bool, bool, bool);

/// A round's rules (Python's dict).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rules {
    pub kind: i64,
    pub inc: i64,
    pub hits: i64,
    pub per: i64,
    pub strength: i64,
    pub heal: i64,
    pub burn: i64,
    pub draw: i64,
    pub limit: i64,
    pub drain: i64,
}

thread_local! {
    static RULES: Cell<Option<fn(i64, Burns) -> Rules>> = const { Cell::new(None) };
}

/// `demonfight.rules = rules`: other rules on this thread from now on (None: this module's own, `_rules`).
pub fn set_rules(rules: Option<fn(i64, Burns) -> Rules>) {
    RULES.with(|r| r.set(rules));
}

/// Round `r` under the afflictions taken (`burns`: for each of the three offers, whether the burn was taken).
/// (Or what a script of demon/ has put in its place on this thread: `set_rules`.)
pub fn rules(r: i64, burns: Burns) -> Rules {
    match RULES.with(|h| h.get()) {
        Some(other) => other(r, burns),
        None => _rules(r, burns),
    }
}

/// `rules` itself, whatever stands in for it.
pub fn _rules(r: i64, burns: Burns) -> Rules {
    let (s, kind) = (2 * floordiv(r - 1, 4), (r - 1) % 4);
    let (inc, hits) = ([0, 17 + s, 3 * (8 + s), 11 + s][kind as usize], [0, 1, 3, 1][kind as usize]);
    let burn = (if burns.0 && r >= 2 { 6 } else { 0 }) + (if burns.1 && r >= 6 { 7 } else { 0 }) + (if burns.2 && r >= 10 { 8 } else { 0 });
    Rules {
        kind,
        inc,
        hits,
        per: floordiv(inc, 1.max(hits)),
        strength: s,
        heal: if kind == 3 { 30 } else { 0 },
        burn,
        draw: 5 - if !burns.0 && r >= 2 { 1 } else { 0 },
        limit: if !burns.1 && r >= 6 { 3 } else { 99 },
        drain: if !burns.2 && r >= 10 { 1 } else { 0 },
    }
}

fn factorial(n: i64) -> i64 {
    (1..=n).product()
}

/// Every hand of `n` cards as counts by kind, with its chance (`s` Powers in play). (Python's default `s`: 0.)
pub fn hands(deck: &Deck, n: i64, s: i64) -> Vec<([i64; 6], f64)> {
    fn each(at: usize, left: i64, combo: &mut [i64; 6], n: i64, weights: &[f64], out: &mut Vec<([i64; 6], f64)>) {
        // (`itertools.product(range(n + 1), repeat=6)` in its order, the combos that do not add up to n left out)
        if at == 5 {
            combo[5] = left;
            let mut p = factorial(n) as f64;
            for (c, q) in combo.iter().zip(weights) {
                p *= q.powf(*c as f64) / factorial(*c) as f64;
            }
            if p > 0.0 {
                out.push((*combo, p));
            }
            return;
        }
        for c in 0..=left {
            combo[at] = c;
            each(at + 1, left - c, combo, n, weights, out);
        }
    }
    let (mut out, weights) = (Vec::new(), deck.weights(s));
    if n >= 0 {
        each(0, n, &mut [0; 6], n, &weights, &mut out);
    }
    out
}

/// What a hand can do: the (damage, Block, Powers played) no other play with as many Powers beats on both counts.
/// (A Power played counts for the attacks of later turns, not of this one: the order within a turn is left out.)
/// (demon/scaling.rs's `plays` is this one too: a Power costs the deck's `pc` and every Power in play deals its `pd`,
/// 1 and 0 in any other deck. The plays come in no particular order, as out of Python's set: nothing depends on it.)
pub fn plays(deck: &Deck, hand: &[i64; 6], energy: i64, limit: i64, s: i64) -> Vec<(i64, i64, i64)> {
    let [a, h, z, b, _f, pw] = *hand;
    let mut seen: HashSet<(i64, i64, i64)> = HashSet::new();
    let mut order = Vec::new();
    for n in 0..=pw.min(deck.powers - s) {
        // (a Power beyond the deck's is dead)
        for i in 0..=a {
            for j in 0..=h {
                for k in 0..=b {
                    if n * deck.pc + i + 2 * j + k <= energy {
                        for zz in 0..=z {
                            if n + i + j + k + zz <= limit {
                                let play = (i * deck.hit('A', s + n) + j * deck.hit('H', s + n) + zz * deck.hit('Z', s + n) + deck.pd * s, k * deck.db, n);
                                if seen.insert(play) {
                                    order.push(play);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    order.iter().filter(|p| !order.iter().any(|q| q != *p && q.2 == p.2 && q.0 >= p.0 && q.1 >= p.1)).copied().collect()
}

/// A table by (our HP, the enemy's HP): numpy's two-dimensional array of floats.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

impl Grid {
    /// `np.full((rows, cols), value)`.
    pub fn full(rows: usize, cols: usize, value: f64) -> Grid {
        Grid { rows, cols, data: vec![value; rows * cols] }
    }

    /// `float(V[row, col])`.
    pub fn at(&self, row: i64, col: i64) -> f64 {
        let (row, col) = (if row < 0 { row + self.rows as i64 } else { row }, if col < 0 { col + self.cols as i64 } else { col });
        assert!(row >= 0 && (row as usize) < self.rows && col >= 0 && (col as usize) < self.cols, "IndexError: index out of bounds");
        self.data[row as usize * self.cols + col as usize]
    }

    pub fn row(&self, row: usize) -> &[f64] {
        &self.data[row * self.cols..(row + 1) * self.cols]
    }

    pub fn row_mut(&mut self, row: usize) -> &mut [f64] {
        &mut self.data[row * self.cols..(row + 1) * self.cols]
    }
}

/// The best of the plays' tables so far (`best = cache[key] if best is None else np.maximum(best, cache[key])`).
pub fn better(best: Option<Vec<f64>>, other: &Grid) -> Option<Vec<f64>> {
    Some(match best {
        None => other.data.clone(),
        Some(mut best) => {
            for (x, y) in best.iter_mut().zip(&other.data) {
                if *y > *x {
                    *x = *y;
                }
            }
            best
        }
    })
}

/// Chance of winning once a turn has dealt `dmg` and cost us `lost`, for every (our HP, the Demon's HP) before it.
pub fn after(V: &Grid, dmg: i64, lost: i64, heal: i64) -> Grid {
    let (hp, b) = (V.rows as i64, V.cols as i64);
    let mut out = Grid::full(V.rows, V.cols, 0.0);
    let src: Vec<usize> = (0..b).map(|col| BOSS.min(col - dmg + heal).clamp(0, b - 1) as usize).collect();
    let alive: Vec<bool> = (0..b).map(|col| col > dmg).collect();
    if lost < hp {
        for i in 0..(hp - lost) as usize {
            // (`rows`: V without its last `lost` rows)
            let (from, to) = (V.row(i), out.row_mut(i + lost as usize));
            for col in 0..b as usize {
                to[col] = if alive[col] { from[src[col]] } else { 1.0 };
            }
        }
    }
    for i in 0..lost.min(hp).max(0) as usize {
        // dead unless the Demon died first
        let to = out.row_mut(i);
        for col in 0..b as usize {
            to[col] = if alive[col] { 0.0 } else { 1.0 };
        }
    }
    out.row_mut(0).fill(0.0);
    out
}

/// `V[r][Powers in play]`: a [`Grid`] by (our HP, the Demon's HP).
pub type Solved = Vec<Vec<Rc<Grid>>>;

/// V[r][Powers in play][our HP, the Demon's HP]: the chance that perfect play wins from the start of round r.
pub fn solve(deck: &Deck, burns: Burns) -> Solved {
    let mut V: Solved = vec![Vec::new(); ROUNDS as usize + 2];
    V[ROUNDS as usize + 1] = (0..=deck.powers).map(|_| Rc::new(Grid::full(deck.maxhp as usize + 1, BOSS as usize + 1, 0.0))).collect();
    for r in (1..=ROUNDS).rev() {
        let rl = rules(r, burns);
        let mut cache: HashMap<(i64, i64, i64), Rc<Grid>> = HashMap::new();
        let mut row = Vec::new();
        for s in 0..=deck.powers {
            let mut total = Grid::full(deck.maxhp as usize + 1, BOSS as usize + 1, 0.0);
            for (hand, p) in hands(deck, rl.draw, s) {
                let mut best = None;
                for (dmg, blk, n) in plays(deck, &hand, deck.turn_energy(s, rl.drain), rl.limit, s) {
                    let lost = 0.max(rl.inc + rl.burn - blk);
                    let key = (dmg, lost, s + n);
                    let then = cache.entry(key).or_insert_with(|| Rc::new(after(&V[r as usize + 1][(s + n) as usize], dmg, lost, rl.heal)));
                    best = better(best, then);
                }
                let best = best.expect("TypeError: unsupported operand type(s) for *: 'float' and 'NoneType'");
                for (t, x) in total.data.iter_mut().zip(&best) {
                    *t += p * x;
                }
            }
            total.row_mut(0).fill(0.0);
            row.push(Rc::new(total));
        }
        V[r as usize] = row;
    }
    V
}

/// The chance of winning after this turn's play (`s`: Powers in play once it is over). (Python's default `s`: 0.)
pub fn value(V: &Solved, r: i64, rl: &Rules, hp: i64, boss: i64, dmg: i64, blk: i64, s: i64) -> f64 {
    if boss - dmg <= 0 {
        return 1.0;
    }
    let left = hp - 0.max(rl.inc + rl.burn - blk);
    if left <= 0 || r + 1 > ROUNDS {
        return 0.0;
    }
    V[r as usize + 1][s as usize].at(left, BOSS.min(boss - dmg + rl.heal))
}

/// (Python's default `s`: 0.)
pub fn state(deck: &Deck, r: i64, rl: &Rules, hp: i64, boss: i64, hand: &[char], energy: i64, block: i64, played: i64, s: i64) -> Value {
    let text = |c: char| -> Face {
        match c {
            'A' => face("STRIKE_R", "Strike", "Attack", 1, &format!("Deal {} damage.", deck.hit('A', s)), "AnyEnemy"),
            'H' => face("HEAVY", "Heavy Blow", "Attack", 2, &format!("Deal {} damage.", deck.hit('H', s)), "AnyEnemy"),
            'Z' => face("FLICK", "Flick", "Attack", 0, &format!("Deal {} damage.", deck.hit('Z', s)), "AnyEnemy"),
            'B' => face("DEFEND_R", "Defend", "Skill", 1, &format!("Gain {} Block.", deck.db), "Self"),
            'P' if deck.pe == 0 => face("INFLAME", "Inflame", "Power", 1, &format!("Gain {} Strength.", deck.ps), "Self"),
            'P' => {
                let lose = if deck.ps < 0 { format!("Lose {} Strength. ", -deck.ps) } else { String::new() };
                face("FRIENDSHIP", "Friendship", "Power", 1, &(lose + "Gain [necrobinder_energy_icon.png] at the start of each turn."), "Self")
            }
            'F' => face("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
            _ => panic!("TypeError: no card of kind {}", py::str_repr(&c.to_string())),
        }
    };
    let mut intents = if rl.inc == 0 {
        vec![json!({"type": "Debuff", "label": ""})]
    } else {
        vec![json!({"type": "Attack", "label": if rl.hits > 1 { format!("{}x{}", rl.per, rl.hits) } else { rl.inc.to_string() }})]
    };
    if rl.heal != 0 {
        intents.push(json!({"type": "Heal", "label": ""}));
        intents.push(json!({"type": "Buff", "label": ""}));
    }
    let mut pst = status(&[("STRENGTH_POWER", deck.ps * s), ("FRIENDSHIP_POWER", if deck.pe != 0 { s } else { 0 }), ("DISINTEGRATION_POWER", rl.burn)]);
    if rl.limit < 99 {
        pst.push(json!({"id": "SLOTH_POWER", "amount": played}));
    }
    let pile: Vec<char> = KINDS.chars().zip(deck.counts).filter(|(k, _)| *k != 'F').flat_map(|(k, n)| (0..n - if k == 'P' { s } else { 0 }).map(move |_| k)).collect();
    let enemies = vec![json!({"entity_id": "KNOWLEDGE_DEMON_0", "hp": boss, "max_hp": BOSS, "block": 0, "status": status(&[("STRENGTH_POWER", rl.strength)]), "intents": intents})];
    let st = game_state(
        "boss",
        json!({"act": 2, "floor": 33, "ascension": 7}),
        r,
        enemies,
        hand_cards(hand, &text, "F", &[]),
        &piles(&text),
        &pile,
        &[],
        GameStateArgs { hp, maxhp: deck.maxhp, block, energy, status: pst, extra: vec![("_plays_this_turn", json!(played))] },
    );
    st
}

/// `rng.choices(KINDS, weights=weights, k=k)`.
pub fn choices(rng: &mut Random, weights: &[f64], k: i64) -> Vec<char> {
    let kinds: Vec<char> = KINDS.chars().collect();
    let mut cum_weights = Vec::new();
    let mut running = 0.0;
    for (n, w) in weights.iter().enumerate() {
        running = if n == 0 { *w } else { running + w };
        cum_weights.push(running);
    }
    let total = cum_weights[cum_weights.len() - 1] + 0.0;
    assert!(total > 0.0, "ValueError: Total of weights must be greater than zero");
    let hi = kinds.len() - 1;
    (0..k)
        .map(|_| {
            let x = rng.random() * total;
            let (mut lo, mut hi) = (0, hi);
            while lo < hi {
                let mid = (lo + hi) / 2;
                if x < cum_weights[mid] {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            kinds[lo]
        })
        .collect()
}

/// (round, kind, HP share, the Demon's HP, chance given away, damage and Block beyond the best play's, what was coming,
/// Powers played beyond the best play's, Powers in the hand)
pub type Turn = (i64, i64, f64, i64, f64, i64, i64, i64, i64, i64);

pub fn count(hand: &[char], kind: char) -> i64 {
    hand.iter().filter(|c| **c == kind).count() as i64
}

/// One fight by the planner: (won, rounds, [(round, kind, HP share, the Demon's HP, chance given away, damage and Block beyond the best play's)]).
pub fn fight(deck: &Deck, burns: Burns, V: &Solved, rng: &mut Random, trace: bool) -> (bool, i64, Vec<Turn>) {
    let (mut hp, mut boss, mut s) = (deck.maxhp, BOSS, 0);
    let mut turns = Vec::new();
    for r in 1..=ROUNDS {
        let rl = rules(r, burns);
        let mut hand = choices(rng, &deck.weights(s), rl.draw);
        while count(&hand, 'P') > deck.powers - s {
            let at = hand.iter().position(|c| *c == 'P').unwrap();
            hand[at] = 'F';
        }
        let counts: [i64; 6] = std::array::from_fn(|k| count(&hand, KINDS.as_bytes()[k] as char));
        let (mut energy, mut block, mut dealt, mut played, s0) = (deck.turn_energy(s, rl.drain), 0, 0, 0, s);
        let mut shown = hand.clone();
        shown.sort();
        let shown: String = shown.into_iter().collect();
        for _ in 0..12 {
            if boss - dealt <= 0 {
                break;
            }
            let Some((i, c, _act)) = turn(&state(deck, r, &rl, hp, boss - dealt, &hand, energy, block, played, s), &hand) else { break };
            hand.remove(i);
            played += 1;
            if c == 'B' {
                block += deck.db;
            } else if c == 'P' {
                s += 1;
            } else {
                dealt += deck.hit(c, s);
            }
            energy -= match c {
                'A' | 'B' | 'P' => 1,
                'H' => 2,
                'Z' => 0,
                _ => panic!("KeyError: {}", py::str_repr(&c.to_string())),
            };
        }
        let ours = value(V, r, &rl, hp, boss, dealt, block, s);
        // (The planner may play its Power before its attacks and so do a little better than any play listed: no chance given away then.)
        let best = max_of(plays(deck, &counts, deck.turn_energy(s0, rl.drain), rl.limit, s0).into_iter().map(|(d, b, n)| (value(V, r, &rl, hp, boss, d, b, s0 + n), d, b, n)))
            .expect("ValueError: max() iterable argument is empty");
        let given = if best.0 - ours > 0.0 { best.0 - ours } else { 0.0 };
        turns.push((r, rl.kind, hp as f64 / deck.maxhp as f64, boss, given, dealt - best.1, block - best.2, rl.inc + rl.burn, (s - s0) - best.3, counts[5]));
        if trace {
            out!(
                "r{r:2} hp {hp:3} demon {boss:3} coming {:2} hand {shown:5} | ours {dealt:2} dmg {block:2} blk {} P ({ours:.3})  best {:2} dmg {:2} blk {} P ({:.3})",
                rl.inc + rl.burn,
                s - s0,
                best.1,
                best.2,
                best.3,
                best.0
            );
        }
        boss -= dealt;
        if boss <= 0 {
            return (true, r, turns);
        }
        hp -= 0.max(rl.inc + rl.burn - block);
        if hp <= 0 {
            return (false, r, turns);
        }
        boss = BOSS.min(boss + rl.heal);
    }
    (false, ROUNDS, turns)
}

/// tuned until perfect play wins between a fifth and four fifths: a deck of plain Strikes and Defends never does
pub fn DECKS() -> Vec<Deck> {
    vec![
        Deck::new([6, 4, 2, 6, 2, 0], DeckArgs { da: 15, db: 11, energy: 4, ..Default::default() }),
        Deck::new([6, 3, 3, 6, 2, 0], DeckArgs { da: 19, db: 12, ..Default::default() }),
        Deck::new([6, 4, 2, 6, 2, 2], DeckArgs { da: 13, db: 10, energy: 4, ..Default::default() }),
        Deck::new([6, 3, 3, 6, 2, 2], DeckArgs { da: 17, db: 12, ..Default::default() }),
        Deck::new([5, 4, 3, 6, 2, 3], DeckArgs { da: 15, db: 11, maxhp: 90, ..Default::default() }),
        Deck::new([4, 3, 5, 7, 3, 3], DeckArgs { da: 14, db: 11, energy: 4, maxhp: 66, ..Default::default() }),
    ]
}
/// the burn taken at each offer, or not
pub const OFFERS: [Burns; 2] = [(false, false, false), (false, false, true)];

/// name -> value set on combat.rs (or closing.rs) in every worker (to try a planner constant): Python's `PATCH`.
pub type Patch = IndexMap<String, f64>;

/// Move each NAME=VALUE of the command line into a [`Patch`]: (the patch, the command line without them).
pub fn take_patches(args: &[String]) -> Result<(Patch, Vec<String>), String> {
    let mut into = Patch::new();
    for arg in args.iter().filter(|a| a.contains('=')) {
        let parts: Vec<&str> = arg.split('=').collect();
        into.insert(parts[0].to_string(), float(parts[1])?);
    }
    Ok((into, args.iter().filter(|a| !a.contains('=')).cloned().collect()))
}

/// The constants on this thread from now on.
pub fn set_constants(values: &Patch) -> Result<(), String> {
    for (k, v) in values {
        set_closing_or_combat(k, *v)?;
    }
    Ok(())
}

/// (deck, burns, what perfect play wins, the fights)
pub type Done = (usize, Burns, f64, Vec<(bool, i64, Vec<Turn>)>);

pub fn job(args: &(usize, Burns, i64, i64, Patch)) -> Result<Done, String> {
    let (d, burns, n, seed, patch) = args;
    set_constants(patch)?;
    let deck = &DECKS()[*d];
    let V = solve(deck, *burns);
    let mut rng = Random::new(*seed as i128);
    Ok((*d, *burns, V[1][0].at(deck.maxhp, BOSS), (0..*n).map(|_| fight(deck, *burns, &V, &mut rng, false)).collect()))
}

/// (Python's default `offers`: OFFERS; `patch`: the command line's.)
pub fn run(n: i64, offers: &[Burns], patch: &Patch) -> Result<Vec<Done>, String> {
    let jobs: Vec<(usize, Burns, i64, i64, Patch)> =
        (0..DECKS().len()).flat_map(|d| offers.iter().enumerate().map(move |(i, burns)| (d, *burns, n, 900 + 17 * d as i64 + i as i64, patch.clone()))).collect();
    on_all_cores(&|| {}, &job, &jobs, 8).into_iter().collect()
}

pub fn table(out: &[Done], n: i64) {
    let name = |burns: &Burns| -> String { [burns.0, burns.1, burns.2].iter().zip("MSW".chars()).map(|(x, w)| if *x { 'b' } else { w }).collect() };
    out!("afflictions: M Mind Rot, S Sloth, W Waste Away, b the burn instead; each cell: planner won / perfect play wins");
    for (d, deck) in DECKS().iter().enumerate() {
        out!(
            "{:48} {}",
            deck.repr(),
            out.iter().filter(|o| o.0 == d).map(|(_, b, best, fights)| format!("{} {:.2}/{:.2}", name(b), fights.iter().filter(|f| f.0).count() as f64 / n as f64, best)).collect::<Vec<_>>().join("  ")
        );
    }
    let fought: usize = out.iter().map(|o| o.3.len()).sum();
    let won = out.iter().flat_map(|o| &o.3).filter(|f| f.0).count() as f64 / fought as f64;
    let given = fsum(out.iter().flat_map(|o| &o.3).flat_map(|f| &f.2).map(|t| t.4)) / fought as f64;
    out!("all: planner {won:.3}, perfect play {:.3}, chance given away a fight {given:.4}", fsum(out.iter().map(|o| o.2)) / out.len() as f64);
}

pub fn regret(out: &[Done]) {
    let rows: Vec<&Turn> = out.iter().flat_map(|o| &o.3).flat_map(|f| &f.2).collect();
    let fights = out.iter().map(|o| o.3.len()).sum::<usize>() as f64;
    out!("chance of winning given away a fight: {:.3} over {:.1} turns", fsum(rows.iter().map(|t| t.4)) / fights, rows.len() as f64 / fights);
    let kind = ["Debuff (nothing coming)", "17", "8x3", "11 and the heal"];
    let band = |t: &Turn| -> &'static str {
        if t.2 >= 0.7 {
            "HP 70%+"
        } else if t.2 >= 0.4 {
            "HP 40-70%"
        } else {
            "HP under 40%"
        }
    };
    let lean = |t: &Turn| -> &'static str {
        if t.8 < 0 {
            "held a Power the best play plays"
        } else if t.8 > 0 {
            "played a Power the best play holds"
        } else if t.6 > 0 {
            "blocked more"
        } else if t.6 < 0 {
            "blocked less"
        } else if t.5 < 0 {
            "same Block, less damage"
        } else {
            "as the best"
        }
    };
    let keys: [(&str, Box<dyn Fn(&Turn) -> String>); 4] = [
        ("by what the Demon was doing", Box::new(|t| kind[t.1 as usize].to_string())),
        ("by our HP", Box::new(|t| band(t).to_string())),
        ("by what the planner did beside the best play", Box::new(|t| lean(t).to_string())),
        ("by both", Box::new(|t| format!("{:13} {:24} {}", band(t), kind[t.1 as usize], lean(t)))),
    ];
    for (title, key) in &keys {
        out!("\n{title}");
        let mut groups: IndexMap<String, Vec<f64>> = IndexMap::new();
        for t in &rows {
            groups.entry(key(t)).or_default().push(t.4);
        }
        let mut sorted: Vec<(&String, &Vec<f64>)> = groups.iter().collect();
        sort_by_key(&mut sorted, |kv| -fsum(kv.1.iter().copied()));
        for (k, v) in sorted {
            let total = fsum(v.iter().copied());
            if total / fights >= 0.002 {
                out!("  {k:60} turns a fight {:5.2}  given away a fight {:.3}  a turn {:.3}", v.len() as f64 / fights, total / fights, total / v.len() as f64);
            }
        }
    }
}

/// `f"{x:.3f}"`, of a number that may be `float("nan")`.
fn f3(x: f64) -> String {
    if x.is_nan() {
        "nan".to_string()
    } else {
        format!("{x:.3}")
    }
}

/// The same deck on 3 and on 4 energy a turn, and what the energy is worth at the Demon's door (relicworth.rs).
pub fn energy_table(n: i64) {
    let offers = OFFERS[0];
    out!("perfect play and the planner, the same deck on 3 and 4 energy a turn (relicworth.py's energy relics):"); // (the text as the Python printed it)
    for (counts, kw) in [([6, 4, 2, 6, 2, 0], DeckArgs { da: 15, db: 11, ..Default::default() }), ([6, 3, 3, 6, 2, 2], DeckArgs { da: 17, db: 12, ..Default::default() })] {
        for energy in [3, 4] {
            let deck = Deck::new(counts, DeckArgs { energy, ..kw });
            let V = solve(&deck, offers);
            let mut rng = Random::new(7);
            let won = (0..n).filter(|_| fight(&deck, offers, &V, &mut rng, false).0).count() as f64 / n as f64;
            out!("  {:52} perfect {:.3}  planner {}", deck.repr(), V[1][0].at(deck.maxhp, BOSS), f3(won));
        }
    }
    let deck = Deck::new([6, 4, 2, 6, 2, 0], DeckArgs { da: 15, db: 11, energy: 4, ..Default::default() });
    let V = solve(&deck, offers);
    let door = Py::Dict([50, 60, 75].iter().map(|h| (Py::Int(*h), Py::Float(py::round_to(V[1][0].at(*h, BOSS), 3)))).collect());
    out!("  4 energy, perfect play by HP at the Demon's door: {}", door.repr());
}

/// One more card of each sort at the Demon, by perfect play (and the planner on `n` fights), in decks with no Power yet.
pub fn card_worth(n: i64) {
    let offers = OFFERS[0];
    let bases = [
        ([6, 4, 2, 6, 2, 0], DeckArgs { da: 15, db: 11, ..Default::default() }),
        ([6, 3, 3, 6, 2, 0], DeckArgs { da: 19, db: 12, ..Default::default() }),
        ([7, 3, 2, 7, 3, 0], DeckArgs { da: 17, db: 12, maxhp: 66, ..Default::default() }),
    ];
    // (the name, the card added, and the Power's `pe` and `ps` where they are not Inflame's)
    let extra: [(&str, [i64; 6], Option<(i64, i64)>); 8] = [
        ("nothing", [0, 0, 0, 0, 0, 0], None),
        ("Strike-like attack", [1, 0, 0, 0, 0, 0], None),
        ("two-cost attack", [0, 1, 0, 0, 0, 0], None),
        ("free attack", [0, 0, 1, 0, 0, 0], None),
        ("Defend-like", [0, 0, 0, 1, 0, 0], None),
        ("Inflame (+3 Strength)", [0, 0, 0, 0, 0, 1], None),
        ("energy Power (Demesne, Pyre)", [0, 0, 0, 0, 0, 1], Some((1, 0))),
        ("Friendship (+1 energy, -2 Strength)", [0, 0, 0, 0, 0, 1], Some((1, -2))),
    ];
    out!("perfect play's chance of beating the Demon from full HP, the deck as it is and with one card more (planner beside it)");
    for (counts, kw) in bases {
        out!("  {}", Deck::new(counts, kw).repr());
        for (name, add, pw) in extra {
            let kw = match pw {
                Some((pe, ps)) => DeckArgs { pe, ps, ..kw },
                None => kw,
            };
            let deck = Deck::new(std::array::from_fn(|k| counts[k] + add[k]), kw);
            let V = solve(&deck, offers);
            let mut rng = Random::new(11);
            let won = if n != 0 { (0..n).filter(|_| fight(&deck, offers, &V, &mut rng, false).0).count() as f64 / n as f64 } else { f64::NAN };
            out!("    + {name:38} perfect {:.3}  planner {}", V[1][0].at(deck.maxhp, BOSS), f3(won));
        }
    }
}

fn script(args: &[String]) -> Result<(), String> {
    quiet();
    let (patch, args) = take_patches(args)?;
    let count = |default: i64| -> Result<i64, String> {
        if args.len() > 1 {
            int(&args[1])
        } else {
            Ok(default)
        }
    };
    match args.first().map(String::as_str) {
        Some("trace") => {
            let (seed, at) = (count(1)?, if args.len() > 2 { int(&args[2])? } else { 0 });
            under(
                || set_constants(&patch),
                || -> Result<(), String> {
                    let decks = DECKS();
                    let (deck, burns) = (index(&decks, at)?, OFFERS[0]);
                    let V = solve(deck, burns);
                    out!("{} perfect play wins {}", deck.repr(), py::float_repr(py::round_to(V[1][0].at(deck.maxhp, BOSS), 3)));
                    let (won, rounds, _) = fight(deck, burns, &V, &mut Random::new(seed as i128), true);
                    out!("{}", Py::Tuple(vec![Py::Bool(won), Py::Int(rounds)]).repr());
                    Ok(())
                },
            )??;
        }
        Some("regret") => regret(&run(count(100)?, &OFFERS, &patch)?),
        Some("cards") => card_worth(count(0)?),
        Some("energy") => energy_table(count(400)?),
        _ => {
            let n = if !args.is_empty() { int(&args[0])? } else { 100 };
            table(&run(n, &OFFERS, &patch)?, n);
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

    const GOLDEN: &str = include_str!("../../tests/golden/scripts/demonfight.txt");

    #[test]
    fn as_python() {
        for command in ["trace", "trace 2 3", "trace 5 5 CLOSE_REACH=0 HP_BASE=2.5", "2", "regret 3 HP_SLOPE=6", "energy 3", "cards 1"] {
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
