//! What the model runs (sim/) do with each card, set against its text: every card with a drawback or a pile clause played in a
//! model fight, and what changed (our HP, powers and piles, the enemy's powers, Osty, orb slots); then the cards whose Strength or
//! Dexterity differ from what the planner's parse says. A clause that changes nothing is one the engine leaves out, and the card
//! table (`sim prior`) then learns the model's bug (2026-11-26: Grand Finale playable with a full draw pile, Bone Shards' Osty
//! never dying, Fight Me's Strength for the enemy, Fight Through's Wounds; the first game's Reprogram counted twice).
//!
//! ```text
//!     scripts audit_simcards [sts2|sts1] [CARD ...]
//! ```
//!
//! Clauses that need a pile to act on (a card from the discard pile, next turn) are played with a few cards in each pile.

use std::collections::BTreeSet;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::cards::parse;
use crate::games;
use crate::learn;
use crate::base;
use crate::scripts::modelfight::{exit_code, out, quiet};
use crate::sim::cards::{Card, CardDef, ViewArgs};
use crate::sim::content::{self, sts1, sts2, Content};
use crate::sim::engine::Fight;
use crate::sim::run::Run;

pub fn CLAUSE() -> &'static regex::Regex {
    crate::re!(concat!(
        r"(?i)enem(?:y|ies) gains? \d+ strength|osty dies|he dies|can only be played|gain \d+ (?:vulnerable|weak|frail)|lose \d+ (?:strength|dexterity|focus)|",
        r"orb slot|(?:shuffle|add|put|place)\b[^.]*\binto your|copy of this card|lose \d+ hp|end your turn|this turn"
    ))
}

/// `FOE`: the games the script knows ("sts2": a Nibbit, "sts1": a Jaw Worm).
pub const FOE: [&str; 2] = ["sts2", "sts1"];

pub fn fight(g: &Rc<Content>, game: &str, character: &str) -> Fight {
    let mut run = Run::new(g, character, 1, Some(&IndexMap::new()), 0);
    run.act_id = "OVERGROWTH".to_string();
    run.note_deck();
    let foe = if game == "sts2" { sts2::_new("NIBBIT", 0, &mut run, 0, false, None) } else { sts1::_new("JAW_WORM", 0, &mut run, "monster", None) };
    let mut f = Fight::new(run, vec![foe], "monster");
    f.start();
    f.player_turn_start();
    f.energy = 6;
    if f.osty >= 0 {
        f.osty = 5;
    }
    let starter: Vec<String> = g.CHARACTERS[character].deck.iter().take(3).cloned().collect();
    for k in starter {
        f.discard.push((g.make)(&k, false));
    }
    f
}

pub fn owner(g: &Content, d: &CardDef) -> String {
    let color = d.color.to_uppercase();
    g.CHARACTERS.keys().find(|c| color == **c || color == c.replace("THE_", "")).or_else(|| g.CHARACTERS.keys().next()).expect("a character").clone()
}

/// A dict of powers as the script's snapshot keeps it.
type Dict = IndexMap<String, i64>;

/// `a.get(k)` as the original prints it.
fn shown(d: &Dict, k: &str) -> String {
    d.get(k).map_or("None".to_string(), |v| v.to_string())
}

struct Snap {
    hp: i64,
    dicts: [Dict; 3], // powers, this turn, enemy
    osty: i64,
    orb_slots: i64,
    cards: IndexMap<String, i64>,
    piles: [usize; 3], // hand, discard, draw
}

fn snap(f: &Fight) -> Snap {
    let mut cards: IndexMap<String, i64> = IndexMap::new();
    for c in f.hand.iter().chain(&f.draw).chain(&f.discard) {
        *cards.entry(c.key().to_string()).or_insert(0) += 1;
    }
    Snap { hp: f.run.hp, dicts: [f.pw.clone(), f.temp.clone(), f.foes[0].powers.clone()], osty: f.osty, orb_slots: f.orb_slots, cards, piles: [f.hand.len(), f.discard.len(), f.draw.len()] }
}

pub fn played(g: &Rc<Content>, game: &str, key: &str) -> String {
    let d = g.CARDS.get(key).unwrap_or_else(|| panic!("KeyError: {key}")).clone();
    let mut f = fight(g, game, &owner(g, &d));
    let before = snap(&f);
    let c = Card::new(&d, false);
    f.hand.push(c.clone());
    if !f.playable(&c) {
        return "not playable".to_string();
    }
    let target = f.foes[0].eid.clone();
    f.play(f.hand.len() as i64 - 1, Some(&target));
    let after = snap(&f);
    let mut out: Vec<String> = Vec::new();
    if before.hp != after.hp {
        out.push(format!("hp {}->{}", before.hp, after.hp));
    }
    for (name, (a, b)) in ["powers", "this turn", "enemy"].into_iter().zip(before.dicts.iter().zip(&after.dicts)) {
        let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
        for k in keys {
            if a.get(k) != b.get(k) && k != "DRAWN_TURN" {
                out.push(format!("{name}.{k} {}->{}", shown(a, k), shown(b, k)));
            }
        }
    }
    if before.osty != after.osty {
        out.push(format!("osty {}->{}", before.osty, after.osty));
    }
    if before.orb_slots != after.orb_slots {
        out.push(format!("orb slots {}->{}", before.orb_slots, after.orb_slots));
    }
    // (`after - before` of two Counters keeps what there is more of, in the order of `after`)
    let mut made: IndexMap<String, i64> = after.cards.iter().map(|(k, n)| (k.clone(), n - before.cards.get(k).copied().unwrap_or(0))).filter(|(_, n)| *n > 0).collect();
    *made.entry(key.to_string()).or_insert(0) -= 1;
    made.retain(|_, n| *n > 0);
    if !made.is_empty() {
        out.push(format!("cards made {{{}}}", made.iter().map(|(k, n)| format!("{}: {n}", base::str_repr(k))).collect::<Vec<_>>().join(", ")));
    }
    out.push(format!("hand {}->{} discard {}->{} draw {}->{}", before.piles[0], after.piles[0], before.piles[1], after.piles[1], before.piles[2], after.piles[2]));
    out.join("; ")
}

/// Cards whose Strength or Dexterity in the model is not what the parse reads: (key, parse's Strength and Dexterity, the model's, text).
pub fn doubled(g: &Rc<Content>, game: &str) -> Vec<(String, i64, i64, i64, i64, String)> {
    let mut rows = Vec::new();
    let mut keys: Vec<&String> = g.CARDS.keys().collect();
    keys.sort();
    for key in keys {
        let d = &g.CARDS[key];
        if matches!(d.r#type.as_str(), "Status" | "Curse" | "Power") || !crate::re!(r"(?i)\b(strength|dexterity)\b").is_match(&d.text) {
            continue;
        }
        let mut f = fight(g, game, &owner(g, d));
        let c = Card::new(d, false);
        let i = parse(&c.view(ViewArgs { index: Some(0), ..Default::default() }));
        let (s0, d0) = (f.strength(), f.dexterity());
        f.hand.push(c.clone());
        if f.playable(&c) {
            let target = f.foes[0].eid.clone();
            f.play(f.hand.len() as i64 - 1, Some(&target));
            let (ds, dd) = (f.strength() - s0, f.dexterity() - d0);
            if (i.strength != 0 && ds != i.strength) || (i.dexterity != 0 && dd != i.dexterity) {
                rows.push((key.clone(), i.strength, i.dexterity, ds, dd, d.text.clone()));
            }
        }
    }
    rows
}

/// `text[:n]`.
fn head(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

fn script(argv: &[String]) -> Result<(), String> {
    let game = match argv.first() {
        Some(first) if FOE.contains(&first.as_str()) => first.as_str(),
        _ => "sts2",
    };
    let keys: Vec<String> = argv.iter().skip(1).filter(|a| !FOE.contains(&a.as_str())).map(|a| a.to_uppercase()).collect();
    quiet();
    learn::update_knobs(&learn::DEFAULTS.iter().map(|(k, v)| (k.to_string(), *v)).collect());
    games::select(game)?;
    let g = content::content(game);
    let mut pools: BTreeSet<String> = ["RED", "GREEN", "BLUE", "PURPLE", "COLORLESS"].iter().map(|c| c.to_string()).collect();
    for c in g.CHARACTERS.keys() {
        pools.insert(c.clone());
        pools.insert(c.replace("THE_", ""));
    }
    let all = keys.is_empty();
    let keys = if all {
        let mut keys: Vec<String> = g.CARDS.iter().filter(|(_, d)| CLAUSE().is_match(&d.text) && (pools.contains(&d.color.to_uppercase()) || game == "sts1")).map(|(k, _)| k.clone()).collect();
        keys.sort();
        keys
    } else {
        keys
    };
    for key in &keys {
        let d = g.CARDS.get(key).ok_or_else(|| format!("KeyError: {}", base::str_repr(key)))?;
        out!("{key:22} {}\n    {}", played(&g, game, key), head(&d.text, 150));
    }
    if all {
        out!("\nStrength or Dexterity unlike the parse's:");
        for (key, ps, pd, ds, dd, text) in doubled(&g, game) {
            out!("  {key:22} parse {ps:+}/{pd:+}, model {ds:+}/{dd:+} | {}", head(&text, 100));
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

    const GOLDEN: &str = include_str!("../../tests/golden/scripts/audit_simcards.txt");

    #[test]
    fn as_recorded() {
        for command in ["sts2", "sts1", "sts2 bone_shards FIGHT_ME hyperbeam", "sts1 REPROGRAM"] {
            let _playing = crate::games::playing("sts2");
            golden(main, &command.split(' ').collect::<Vec<_>>(), section(GOLDEN, command));
        }
    }
}
