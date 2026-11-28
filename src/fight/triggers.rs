//! Powers that act each time a card is played, and other card rules the planner applies by name.

use std::sync::LazyLock;

use indexmap::IndexMap;
use serde_json::Value;

use super::battle::{power_amount, Enemy};
use crate::cards::CardInfo;
use crate::base::{self, JsonExt};

crate::tunables! {
    /// share of what an Ethereal card does when played that keeping it in the deck is worth, as a lasting effect (x span)
    pub ETHEREAL_KEEP: f64 = 0.3;
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Triggers {
    /// Sleight of Flesh: an enemy takes this for each debuff we put on it
    pub per_debuff: i64,
    /// Serpent Form: damage to a random enemy for every card
    pub per_card: i64,
    /// Haunt: HP a random enemy loses for every Soul
    pub per_soul: i64,
    /// Afterimage: Block for every card
    pub card_block: i64,
    /// Rage: Block for every Attack
    pub attack_block: i64,
    /// Danse Macabre: Block for every card that costs 2 or more
    pub costly_block: i64,
    /// Shroud: Block whenever we apply Doom
    pub doom_block: i64,
    /// enemy id -> HP it loses for every card (Strangle)
    pub strangle: IndexMap<String, i64>,
    /// enemy id -> Doom it gains for every card (Oblivion)
    pub oblivion: IndexMap<String, i64>,
}

/// The card-play triggers in force, or None when there are none (the usual case).
pub fn read(state: &Value) -> Option<Triggers> {
    let player = &state["player"];
    let foes = state["battle"]["enemies"].list();
    let amount = |power: &str| 0.max(power_amount(player, power));
    let on_foes = |power: &str| {
        let mut out: IndexMap<String, i64> = IndexMap::new();
        for e in foes {
            let n = 0.max(power_amount(e, power));
            if n != 0 {
                out.insert(base::str_of(&e["entity_id"]), n);
            }
        }
        out
    };
    let t = Triggers {
        per_debuff: amount("SLEIGHT_OF_FLESH"),
        per_card: amount("SERPENT_FORM"),
        per_soul: amount("HAUNT"),
        card_block: amount("AFTERIMAGE"),
        attack_block: amount("RAGE"),
        costly_block: amount("DANSE_MACABRE"),
        doom_block: amount("SHROUD"),
        strangle: on_foes("STRANGLE"),
        oblivion: on_foes("OBLIVION"),
    };
    if t == *NONE {
        None
    } else {
        Some(t)
    }
}

pub static NONE: LazyLock<Triggers> = LazyLock::new(Triggers::default);

/// An enemy's debuffs: (vulnerable, vuln_turns, weak, weak_turns, doom, poison).
pub type Marks = (bool, i64, bool, i64, i64, i64);

/// Each enemy's debuffs, to see afterwards which of them a card added to.
pub fn marks(foes: &[Enemy]) -> Vec<Marks> {
    foes.iter().map(|e| (e.vulnerable, e.vuln_turns, e.weak, e.weak_turns, e.doom, e.poison)).collect()
}

/// What the triggers do once card `c` has been played: (points, Block). `hit(enemy, damage)` returns the HP removed, `gain` prices it.
pub fn fire(t: &Triggers, c: &CardInfo, foes: &mut [Enemy], before: &[Marks], hit: &mut dyn FnMut(&mut Enemy, i64) -> i64, gain: &mut dyn FnMut(&mut Enemy, i64) -> f64) -> (f64, i64) {
    let (mut score, mut block) = (0.0, 0);
    let alive: Vec<usize> = (0..foes.len()).filter(|&i| foes[i].alive()).collect();

    // HP loss: no Block in the way
    fn lose(e: &mut Enemy, n: i64, gain: &mut dyn FnMut(&mut Enemy, i64) -> f64) -> f64 {
        let lost = e.hp.min(n);
        e.hp -= lost;
        gain(e, lost)
    }

    let at_random =|foes: &mut [Enemy], n: i64, through_block: bool, hit: &mut dyn FnMut(&mut Enemy, i64) -> i64, gain: &mut dyn FnMut(&mut Enemy, i64) -> f64| -> f64 {
        if alive.len() == 1 && foes[alive[0]].alive() {
            let e = &mut foes[alive[0]];
            return if through_block {
                let removed = hit(e, n);
                gain(e, removed)
            } else {
                lose(e, n, gain)
            };
        }
        // no telling who: counted, but nobody is planned dead on it
        if !alive.is_empty() {
            n as f64 * 0.8
        } else {
            0.0
        }
    };

    if c.doom_kills {
        // End of Days: "Kill enemies with at least as much Doom as HP", now, before they act
        for &i in &alive {
            let e = &mut foes[i];
            if e.doom >= e.hp && e.hit_cap == 0 {
                let all = e.hp;
                score += lose(e, all, gain);
            }
        }
    }
    let mut doomed = false;
    for (e, was) in foes.iter_mut().zip(before) {
        let now: Marks = (e.vulnerable, e.vuln_turns, e.weak, e.weak_turns, e.doom, e.poison);
        let debuffs = ((now.0, now.1) != (was.0, was.1)) as i64 + ((now.2, now.3) != (was.2, was.3)) as i64 + (now.4 > was.4) as i64 + (now.5 > was.5) as i64;
        doomed = doomed || now.4 > was.4;
        if t.per_debuff != 0 && debuffs != 0 && e.alive() {
            for _ in 0..debuffs {
                let removed = hit(e, t.per_debuff);
                score += gain(e, removed);
            }
        }
    }
    if t.per_card != 0 {
        score += at_random(foes, t.per_card, true, hit, gain);
    }
    if t.per_soul != 0 && c.key == "SOUL" {
        score += at_random(foes, t.per_soul, false, hit, gain);
    }
    for e in foes.iter_mut() {
        if e.alive() {
            if let Some(&n) = t.strangle.get(&*e.entity_id).filter(|n| **n != 0) {
                score += lose(e, n, gain);
            }
        }
        if e.alive() {
            if let Some(&n) = t.oblivion.get(&*e.entity_id).filter(|n| **n != 0) {
                e.doom += n;
                score += gain(e, n) * 0.8;
            }
        }
    }
    block += t.card_block;
    if c.r#type == "Attack" {
        block += t.attack_block;
    }
    if c.cost >= 2 {
        block += t.costly_block;
    }
    if doomed {
        block += t.doom_block;
    }
    (score, block)
}

/// The hand, with the cards whose numbers depend on the hand read for this one (Flechettes: "Deal 5 damage for each Skill in your Hand").
pub fn counted(mut hand: Vec<CardInfo>) -> Vec<CardInfo> {
    let skills = hand.iter().filter(|c| c.r#type == "Skill").count() as i64;
    for c in hand.iter_mut() {
        if c.key == "FLECHETTES" {
            c.hits = skills;
        }
    }
    hand
}

/// Doom a card applies beyond the number its text begins with (No Escape: "plus an additional 5 Doom for every 10 Doom already on this enemy").
pub fn more_doom(c: &CardInfo, e: &Enemy) -> i64 {
    if c.key == "NO_ESCAPE" {
        5 * base::floordiv(e.doom, 10)
    } else {
        0
    }
}

pub const BURNS_HAND: [&str; 3] = ["FIEND_FIRE", "SECOND_WIND", "STOKE"]; // "Exhaust your Hand" (Second Wind: all of it that is not an Attack)
// The first game's Necronomicurse: playable under Blue Candle (1 HP, Exhausted), and back in the hand at once whenever Exhausted;
// 2026-11-25 20:47 the bot played it over and over, a 0-cost card worth +0.1 to the planner, losing 1 HP each time.
pub const COMES_BACK: [&str; 1] = ["NECRONOMICURSE"];

/// Cards the planner must not play here, whatever they seem to offer. Neurosurge puts 3 Doom on us every turn, which kills us once
/// it reaches our HP: only for a fight that will be over before the Doom adds up, with our HP well above it by then.
pub fn shunned(c: &CardInfo, state: &Value) -> bool {
    let (battle, player) = (&state["battle"], &state["player"]);
    if COMES_BACK.contains(&c.key.as_str()) {
        return true;
    }
    if BURNS_HAND.contains(&c.key.as_str()) {
        // the planner counts what it burns as nothing: not while a Power is in the hand beside it
        return player["hand"].list().iter().any(|h| h["type"].as_str() == Some("Power"));
    }
    if c.key != "NEUROSURGE" {
        return false;
    }
    let foes: Vec<&Value> = battle["enemies"].list().iter().filter(|e| 0 < e["hp"].int() && e["hp"].int() < 100_000).collect();
    let left: i64 = foes.iter().map(|e| e["hp"].int() + e["block"].int()).sum();
    if !matches!(state["state_type"].as_str(), Some("boss") | Some("elite")) {
        return left > 60;
    }
    let rounds = (if battle["round"].truthy() { battle["round"].int() } else { 1 }) - 1;
    let dealt: i64 = foes.iter().map(|e| 0.max(e["max_hp"].int() - e["hp"].int())).sum();
    let pace = if rounds >= 2 {
        let so_far = dealt as f64 / rounds as f64;
        if so_far > 15.0 {
            so_far
        } else {
            15.0
        }
    } else {
        25.0
    };
    let turns = -base::ffloordiv((-left) as f64, pace) + 1.0; // (and one to spare: fights run over)
    let doom = 0.max(power_amount(player, "DOOM")) as f64 + (3 + 0.max(power_amount(player, "NEUROSURGE"))) as f64 * turns;
    let then = player["hp"].int() as f64 - 0.08 * player["max_hp"].int() as f64 * turns; // our HP by then: a boss takes some 8% of Max HP a round
    doom > 0.4 * (if then > 0.0 { then } else { 0.0 })
}

/// What playing an Ethereal card is worth beyond the play: left in the hand it is Exhausted, lost for the fight (not Powers: combat.simulate).
pub fn kept(c: &CardInfo, dmg_value: f64) -> f64 {
    if !c.ethereal || !c.playable || !(c.r#type == "Attack" || c.r#type == "Skill") {
        return 0.0;
    }
    ETHEREAL_KEEP.get() * (c.total_damage() as f64 * dmg_value + 0.5 * c.block as f64)
}

/// cards that make the next card of one type free
pub static FREE_NEXT: LazyLock<IndexMap<&'static str, &'static str>> = LazyLock::new(|| IndexMap::from([("UNRELENTING", "Attack"), ("POUNCE", "Skill")]));
/// ... and cards that make every card after them dearer this turn
pub static SURCHARGE: LazyLock<IndexMap<&'static str, i64>> = LazyLock::new(|| IndexMap::from([("BORROWED_TIME", 1)]));
/// cards that play the next card of one type an extra time
pub static ECHO_NEXT: LazyLock<IndexMap<&'static str, &'static str>> = LazyLock::new(|| IndexMap::from([("BURST", "Skill"), ("ONE_TWO_PUNCH", "Attack")]));
pub const PAYS: [&str; 2] = ["DOUBLE_ENERGY", "BULLET_TIME"]; // cards that pay for others without saying "gain energy" (energy doubled; the hand free)
/// cards with no numbers in their text that the planner works out all the same
pub const READ: [&str; 8] = ["BURST", "ONE_TWO_PUNCH", "DOUBLE_ENERGY", "BULLET_TIME", "CASCADE", "TIMES_UP", "TIME_S_UP", "KI_VERSE"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::parse;
    use crate::fight::battle::testing::*;
    use crate::fight::battle::{read_enemies, _hit, _progress};
    use serde_json::json;

    fn to_json(t: &Triggers) -> Value {
        let table = |x: &IndexMap<String, i64>| Value::Object(x.iter().map(|(k, n)| (k.clone(), json!(n))).collect());
        json!({"per_debuff": t.per_debuff, "per_card": t.per_card, "per_soul": t.per_soul, "card_block": t.card_block, "attack_block": t.attack_block, "costly_block": t.costly_block,
               "doom_block": t.doom_block, "strangle": table(&t.strangle), "oblivion": table(&t.oblivion)})
    }

    fn from_json(v: &Value) -> Triggers {
        let table = |x: &Value| x.entries().map(|(k, n)| (k.clone(), n.int())).collect::<IndexMap<String, i64>>();
        let v = &v["T"];
        Triggers {
            per_debuff: v["per_debuff"].int(),
            per_card: v["per_card"].int(),
            per_soul: v["per_soul"].int(),
            card_block: v["card_block"].int(),
            attack_block: v["attack_block"].int(),
            costly_block: v["costly_block"].int(),
            doom_block: v["doom_block"].int(),
            strangle: table(&v["strangle"]),
            oblivion: table(&v["oblivion"]),
        }
    }

    fn marks_from(v: &Value) -> Vec<Marks> {
        v.list().iter().map(|m| (m[0].truthy(), m[1].int(), m[2].truthy(), m[3].int(), m[4].int(), m[5].int())).collect()
    }

    #[test]
    fn golden_states() {
        let (mut checked, mut some) = (0, 0);
        for (n, row) in states().iter().enumerate() {
            let _env = env(row);
            let (state, r) = (&row["s"], &row["r"]);
            let got = read(state);
            some += got.is_some() as usize;
            assert_eq!(got.as_ref().map(to_json).unwrap_or(Value::Null), r["trig"], "state {n}");
            let es = read_enemies(state);
            let hand: Vec<CardInfo> = state["player"]["hand"].list().iter().map(parse).collect();
            for (c, want) in hand.iter().zip(r["hand"].list()) {
                let doom = if es.is_empty() { 0 } else { more_doom(c, &es[0]) };
                assert_eq!(json!([shunned(c, state), kept(c, 1.0), kept(c, 2.5), doom]), json!([want[0], want[1], want[2], want[9]]), "state {n}: {}", c.name);
                checked += 4;
            }
            assert_eq!(json!(counted(hand).iter().map(|c| c.hits).collect::<Vec<_>>()), r["counted"], "state {n}");
            let got: Vec<Value> = marks(&es).iter().map(|m| json!([m.0, m.1, m.2, m.3, m.4, m.5])).collect();
            assert_eq!(json!(got), r["marks"], "state {n}");
            checked += 3;
        }
        assert!(checked > 5000 && some > 10, "{checked} checks, {some} states with triggers");
    }

    #[test]
    fn golden_calls() {
        let cards: Value = serde_json::from_str(include_str!("../../tests/golden/fight/cards.json")).unwrap();
        for row in calls(&cards, "cards", 1200) {
            let c = card_from_json(&row["c"]);
            assert_eq!(json!([kept(&c, 1.0), kept(&c, 2.2)]), row["kept"], "{}", c.name);
        }
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/fight/triggers.json")).unwrap();
        for (n, rec) in calls(&g, "triggers.fire", 600).iter().enumerate() {
            let _env = env(&rec["env"]);
            let a = &rec["a"];
            let (t, c, mut all, before) = (from_json(&a[0]), card_from_json(&a[1]), foes(&a[2]), marks_from(&a[3]));
            let (chip_rate, dmg_value) = (rec["chip_rate"].float(), rec["dmg_value"].float());
            let got = fire(&t, &c, &mut all, &before, &mut |e, d| _hit(e, d, 1, false, true), &mut |e, hp| _progress(e, hp, chip_rate) * dmg_value * e.worth());
            assert_eq!(json!([got.0, got.1]), rec["r"], "fire case {n}: {a}");
            assert_eq!(all, foes(&rec["m"][0]), "fire case {n}: {a}");
        }
        for rec in calls(&g, "triggers.counted", 80) {
            let _env = env(&rec["env"]);
            assert_eq!(counted(cards_from_json(&rec["a"][0])), cards_from_json(&rec["r"]), "counted {}", rec["a"]);
        }
        for rec in calls(&g, "triggers.kept", 50) {
            let _env = env(&rec["env"]);
            assert_eq!(json!(kept(&card_from_json(&rec["a"][0]), rec["a"][1].float())), rec["r"], "kept {}", rec["a"]);
        }
        for rec in calls(&g, "triggers.more_doom", 200) {
            assert_eq!(json!(more_doom(&card_from_json(&rec["a"][0]), &foe(&rec["a"][1]))), rec["r"], "more_doom {}", rec["a"]);
        }
        for rec in calls(&g, "triggers.shunned", 40) {
            let _env = env(&rec["env"]);
            assert_eq!(json!(shunned(&card_from_json(&rec["a"][0]), &rec["a"][1])), rec["r"], "shunned {}", rec["a"]);
        }
    }

    #[test]
    fn the_tables() {
        assert_eq!(*NONE, Triggers::default());
        assert_eq!((FREE_NEXT.get("POUNCE"), SURCHARGE.get("BORROWED_TIME"), ECHO_NEXT.get("BURST")), (Some(&"Skill"), Some(&1), Some(&"Skill")));
        for key in ECHO_NEXT.keys().chain(PAYS.iter()) {
            assert!(READ.contains(key));
        }
        assert_eq!(READ.len(), ECHO_NEXT.len() + PAYS.len() + 4);
    }
}
