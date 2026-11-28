//! The Fool in the model runs: Arrows and Impale, Aim, Stretch, Melodies, Sadness and Joy, as the game does them (the planner reads
//! the same rules in fight/fool). Until 2026-11-28 the model had no Fool, so its card table (`sim prior`) said nothing of his cards
//! and the live bot rated them from one writer's tiers alone, while the other five had the model's word on theirs.
//!
//! - A card that Shoots needs a free Arrow (we own 1, Quiver more) and does nothing without one. The Arrow stands in its target
//!   (IMPALED 3) until three more Attack hits land there or the target dies; Wrest, Uproot and Recall take it out at once.
//! - Aim adds to the next Arrow's damage (Heavy Shot: twice) and Stretch to the next Block a card gives; the hand's texts show both.
//! - A Wah, Cho or Ki card gives that Melody for the rest of the turn; Song Path, Motif, Lullaby and Violent Chord pay when one is new.
//! - Sadness is Unplayable and Retained; Joy (0: gain [E], draw 1; Exhaust) is what Ecstasy and Punch of Joy turn one into.
//! - Not in the Fool's card pool here: Mythic Form (Tarot cards), Florescence (The Shot) and Encore, their rules not being modelled,
//!   and Rapid Shot, which v0.114 no longer has.
//!
//! The cards are as the game v0.114.0 has them (sts2_data.json's Fool rows come from the mod's wiki, not v0.111.1's files: 53 of
//! 84 had changed, among them Growth, Trick Shot, Tune, Wah Ki, Recall and Arrow Stab rewritten and Wah Cho Ki at 2 energy).
//!
//! Counters are kept among our powers under names starting with "_" (not shown to the planner); an enemy's Arrows in `mem`.

use std::collections::BTreeSet;
use std::rc::Rc;

use serde_json::{json, Value};

use crate::base;
use crate::cards::{melodies, CardInfo, MELODIES};
use crate::sim::cards::Card;
use crate::sim::effects::{self as fx, add, Powers};
use crate::sim::engine::{Fight, Pile};

pub const LEFT_OUT: &[&str] = &["MYTHIC_FORM", "FLORESCENCE", "ENCORE", "RAPID_SHOT"];
pub const IMPALE_HITS: i64 = 3;

/// Powers of the Fool's handled here (what `_register` read as their amount is put right in [`power`]).
pub const POWERS: &[&str] = &[
    "SONG_PATH", "MOTIF", "LULLABY", "SERRATED_ARROWS", "HONING", "EXPLOSIVE_ARROWS", "VIOLENT_CHORD", "ARCHERY", "HIDDEN_ROOTS",
    "ALL_IS_WELL", "YAH", "QUIVER", "ENNUI", "ECSTASY", "TUNE", "SIMPLETON",
];

pub fn playing(f: &Fight) -> bool {
    f.run.character == "FOOL"
}

/// The Melodies the cards played this turn have given (the first `n` of them: all with None).
pub fn tune(f: &Fight, n: Option<usize>) -> BTreeSet<String> {
    f.turn_cards.iter().take(n.unwrap_or(usize::MAX)).flat_map(|name| melodies(name)).collect()
}

fn arrows_in(f: &Fight, i: usize) -> i64 {
    f.foes[i].mem.get("arrows", 0)
}

fn impaled(f: &Fight, foe: Option<usize>) -> bool {
    foe.is_some_and(|t| f.foes[t].alive() && arrows_in(f, t) > 0)
}

/// Arrows that can be shot: ours less those standing in a living enemy.
pub fn free(f: &Fight) -> i64 {
    let stuck: i64 = f.alive().into_iter().map(|i| arrows_in(f, i)).sum();
    (f.pw.val("_ARROWS") - stuck).max(0)
}

/// Our Arrows come back out of enemy `i`; Yah!'s energy the first time a turn. Whether there were any.
pub fn dislodge(f: &mut Fight, i: usize) -> bool {
    f.foes[i].powers.shift_remove("IMPALED");
    if arrows_in(f, i) == 0 {
        return false;
    }
    f.foes[i].mem.set("arrows", 0);
    if f.pw.on("YAH") && !f.pw.on("_YAH_DONE") {
        f.pw.put("_YAH_DONE", 1);
        f.energy += f.pw["YAH"];
    }
    true
}

/// Dead enemies give their Arrows back.
fn reap(f: &mut Fight) {
    for i in 0..f.foes.len() {
        if !f.foes[i].alive() && arrows_in(f, i) > 0 {
            dislodge(f, i);
        }
    }
}

/// A Shot's damage a hit before Strength: its own, its Aim (Heavy Shot's 3 times), Honing's double and Quietus' double or triple.
pub fn shot_damage(f: &Fight, c: &Card) -> i64 {
    let i = c.info();
    let base = i.damage + c.bonus + f.pw.val("AIM") * i.aim_times;
    (base + base * f.pw.val("HONING") / 100) * f.pw.val("_QUIETUS").max(1)
}

/// Anguish: "Deals 6 additional damage for each Sadness you have created this combat."
pub fn anguish(f: &Fight, c: &Card) -> i64 {
    (if c.up { 7 } else { 6 }) * (1 + f.pw.val("_SAD_MADE"))
}

/// Wah Symphony: "Deals 4 additional damage for ALL your cards containing "Cho" or "Ki"".
pub fn symphony(f: &Fight, c: &Card) -> i64 {
    let piles = f.hand.iter().chain(&f.draw).chain(&f.discard).chain(&f.exhausted);
    let n = piles.filter(|h| melodies(&h.name()).iter().any(|m| m == "CHO" || m == "KI")).count() as i64;
    6 + (if c.up { 6 } else { 4 }) * n
}

/// What a play is, at this moment of the turn (None: it does nothing at all).
pub fn pre(f: &mut Fight, _c: &mut Card, foe: Option<usize>, i: Rc<CardInfo>) -> Option<Rc<CardInfo>> {
    if i.shoots && free(f) == 0 {
        return None; // (Hobbling Shot's Weak and Vulnerable ride on the Arrow)
    }
    let stuck = impaled(f, foe);
    if (i.impaled_only || i.dislodge) && !stuck {
        return None;
    }
    let now = tune(f, None);
    let (energy, draw) = i.on_impaled.filter(|_| stuck).unwrap_or((0, 0));
    let twice = i.melody_twice.iter().any(|m| now.contains(m));
    if energy == 0 && draw == 0 && !i.melody_hits && !twice && i.key != "STEADY_CADENCE" {
        return Some(i);
    }
    let mut out = (*i).clone();
    out.energy_gain += energy;
    out.draw += draw;
    if i.melody_hits {
        out.hits = 1 + now.len() as i64;
    }
    if twice {
        out.hits *= 2;
    }
    if i.key == "STEADY_CADENCE" {
        out.draw = now.len() as i64;
    }
    Some(Rc::new(out))
}

/// One hit of ours on enemy `t` (`amount` before Block, as worked out for anyone): Wah Hammer's double, Serrated Arrows and Dislike
/// You added. Whether the hit wears an Arrow down (an Attack on an enemy one stands in).
pub fn hit(f: &Fight, c: &Card, i: &CardInfo, t: usize, amount: &mut i64) -> bool {
    let stuck = c.r#type() == "Attack" && f.foes[t].alive() && arrows_in(f, t) > 0;
    if stuck && i.impaled_double {
        *amount *= 2;
    }
    if stuck && f.pw.on("SERRATED_ARROWS") {
        *amount += *amount * f.pw["SERRATED_ARROWS"] / 100;
    }
    if i.shoots && f.foes[t].powers.on("DISLIKE_YOU") {
        *amount += *amount * f.foes[t].powers["DISLIKE_YOU"] / 100;
    }
    stuck
}

/// After a hit that `hit` said wears an Arrow down: the third such hit, or the enemy's death, brings our Arrows back.
pub fn wear(f: &mut Fight, c: &Card, t: usize) {
    let left = f.foes[t].powers.val("IMPALED") - 1;
    if f.foes[t].alive() && left > 0 {
        f.foes[t].powers.put("IMPALED", left);
    } else if dislodge(f, t) && c.key() == "EPIC_RETRIEVAL" {
        f.energy += if c.up { 4 } else { 3 };
    }
}

/// Block from a card: Stretch goes on the first.
pub fn stretched(f: &mut Fight, base: i64) -> i64 {
    let s = f.pw.shift_remove("STRETCH").unwrap_or(0);
    if s != 0 {
        add(&mut f.pw, "_STRETCH_SPENT", s);
    }
    base + s
}

/// A card that Shoots an Arrow.
fn shoots(c: &Card) -> bool {
    c.info().shoots
}

pub fn sadden(f: &mut Fight, n: i64) {
    for _ in 0..n {
        f.add_new("SADNESS", 1, "hand", false);
        add(&mut f.pw, "_SAD_MADE", 1);
        if f.pw.on("ALL_IS_WELL") {
            f.gain_block(f.pw["ALL_IS_WELL"]);
        }
    }
}

/// `n` random Sadness in the hand become Joy.
fn cheer(f: &mut Fight, n: i64) {
    for _ in 0..n {
        let sad: Vec<u64> = f.hand.iter().filter(|h| h.key() == "SADNESS").map(|h| h.uid).collect();
        if sad.is_empty() {
            return;
        }
        let uid = *f.run.rng.choice(&sad);
        let at = f.hand.iter().position(|h| h.uid == uid).expect("the Sadness is in the hand");
        f.hand[at] = (f.game.make)("JOY", false);
    }
}

/// A card of the hand played for nothing (Cho Chorus, Simpleton), at the weakest enemy.
fn play_free(f: &mut Fight, uid: u64) {
    let Some(mut c) = f.take_from(Pile::Hand, uid) else { return };
    f.turn_cards.push(c.name());
    let times = 1 + encore(f, &c);
    for _ in 0..times {
        if f.won || f.lost {
            break;
        }
        let at = base::min_by_key(f.alive(), |i| f.foes[*i].hp);
        fx::resolve(f, &mut c, at, 0);
    }
    f.attacks_turn += (c.r#type() == "Attack") as i64;
    f.skills_turn += (c.r#type() == "Skill") as i64;
    fx::played(f, &c);
    f.put_away(c);
    put_back(f, uid);
}

/// Ki Verse ("The next "Wah" or "Cho" card you play this turn is played an additional time"): 1 if this card is.
pub fn encore(f: &mut Fight, c: &Card) -> i64 {
    if f.pw.on("KI_VERSE") && melodies(&c.name()).iter().any(|m| m == "WAH" || m == "CHO") {
        f.pw["KI_VERSE"] -= 1;
        return 1;
    }
    0
}

/// Coda: "Costs 2 less for each Melody you have"; Cho Limbs: "Costs 2 less if you have a Wah Melody or Ki Melody".
pub fn cost(f: &Fight, c: &Card, cost: i64) -> i64 {
    match c.key() {
        "CODA" => (cost - 2 * tune(f, None).len() as i64).max(0),
        "CHO_LIMBS" if tune(f, None).iter().any(|m| m == "WAH" || m == "KI") => (cost - 2).max(0),
        _ => cost,
    }
}

/// What the Fool's cards do beyond their damage, Block and the rest `effects::resolve` reads (each time a card is played).
pub fn after(f: &mut Fight, c: &mut Card, foe: Option<usize>, xs: i64, dealt: i64) {
    let i = c.info();
    reap(f);
    if i.shoots {
        f.pw.shift_remove("AIM");
        f.pw.shift_remove("_QUIETUS");
        if let Some(t) = foe.filter(|t| f.foes[*t].alive()) {
            f.foes[t].powers.put("IMPALED", IMPALE_HITS);
            f.foes[t].mem.add("arrows", 1);
            if i.splash && dealt > 0 {
                for o in f.alive().into_iter().filter(|o| *o != t) {
                    f.hurt(o, dealt, false);
                }
            }
            if f.pw.on("EXPLOSIVE_ARROWS") {
                f.hurt_all(f.pw["EXPLOSIVE_ARROWS"]);
            }
            reap(f);
        }
        let whistles: Vec<u64> = f.discard.iter().filter(|h| h.key() == "WHISTLING_ARROWS").map(|h| h.uid).collect();
        for uid in whistles {
            let w = f.take_from(Pile::Discard, uid).expect("in the discard pile");
            f.add_card(w, "hand");
        }
    }
    if i.dislodge {
        if let Some(t) = foe {
            dislodge(f, t);
        }
    }
    for (k, n) in [("AIM", i.aim * xs), ("STRETCH", i.stretch * xs + if i.stretch_dealt { dealt } else { 0 }), ("KI_VERSE", i.verse)] {
        if n != 0 {
            add(&mut f.pw, k, n);
        }
    }
    if i.melody_block != 0 {
        // Harmonic Shield: "Gain 4 Block. Gain 4 additional Block for each Melody you have."
        let base = if c.text().contains("additional Block for each Melody") { 1 } else { 0 };
        let b = stretched(f, i.melody_block * (base + tune(f, None).len() as i64));
        f.gain_block(f.block_value(b));
    }
    if !i.keeps_hand.is_empty() && (i.keeps_hand.iter().any(|m| m == "ANY") || i.keeps_hand.iter().any(|m| tune(f, None).contains(m))) {
        f.pw.put("KEEP_HAND", 1);
    }
    if let Some(m) = crate::re!(r"(?i)\badd (\d+|an?) sadness").captures(c.text()).filter(|_| c.r#type() != "Power") {
        let n = m[1].parse().unwrap_or(1);
        sadden(f, n);
    }
    let up = c.up;
    let number = |digits: &str| digits.parse::<i64>().unwrap_or(1);
    match c.key() {
        "PUNCH_OF_JOY" => cheer(f, crate::re!(r"(?i)transform (\d+) random sadness").captures(c.text()).map_or(1, |m| number(&m[1]))),
        "BRIGHTEN_UP" => f.add_new("JOY", if up { 3 } else { 2 }, "hand", false),
        "GROWTH" => {
            // "Transform 1 random Sadness in your Hand into a copy of this card."
            let sad: Vec<usize> = (0..f.hand.len()).filter(|n| f.hand[*n].key() == "SADNESS").collect();
            if !sad.is_empty() {
                let at = *f.run.rng.choice(&sad);
                f.hand[at] = c.copy();
            }
        }
        "QUIETUS" => f.pw.put("_QUIETUS", if up { 3 } else { 2 }),
        "OPTIMISM" => {
            // "Exhaust ALL Sadness in your Hand. Draw 1 card for each card Exhausted."
            let sad: Vec<u64> = f.hand.iter().filter(|h| h.key() == "SADNESS").map(|h| h.uid).collect();
            for uid in &sad {
                let h = f.take_from(Pile::Hand, *uid).expect("in the hand");
                f.exhaust(h);
            }
            f.draw_cards(sad.len() as i64);
        }
        "CHO_KI" => {
            let s = f.pw.shift_remove("_STRETCH_SPENT").unwrap_or(0);
            add(&mut f.pw, "STRETCH", s);
        }
        "DISLIKE_YOU" => {
            if let Some(t) = foe.filter(|t| f.foes[*t].alive()) {
                add(&mut f.foes[t].powers, "DISLIKE_YOU", if up { 75 } else { 50 });
            }
        }
        "CHO_BARBS" => {
            let n = crate::re!(r"(?i)gain (\d+) thorns").captures(c.text()).map_or(0, |m| number(&m[1]));
            add(&mut f.pw, "THORNS", n);
            add(&mut f.pw, "CHO_BARBS", n);
        }
        "WAH_KI" => {
            let pool = f.discard.clone();
            super::sts1_cards::_pick(f, "Choose a card to put on top of your draw pile.", Pile::Discard, Some(pool), 1, Some(|f, c| f.draw.push(c)));
        }
        "LOCKED_IN" => {
            let pool: Vec<Card> = f.draw.iter().filter(|h| shoots(h)).cloned().collect();
            super::sts1_cards::_pick(f, "Choose a card to put into your hand.", Pile::Draw, Some(pool), 1, None);
        }
        "BLOT_OUT_THE_SKY" => {
            let pool = (f.game.pool)(&f.run.character);
            let choices: Vec<_> = pool.iter().filter(|d| super::sts2::make(&d.key, false).info().shoots).cloned().collect();
            super::common::free_card(f, &choices);
        }
        "TIMBER" if foe.is_some_and(|t| !f.foes[t].alive()) => f.hurt_all(crate::re!(r"(?i)deal (\d+) damage to all").captures(c.text()).map_or(0, |m| number(&m[1]))),
        "MOURNING_BLOW" => f.pw.put(&format!("_SAD:{}", c.uid), 1),
        "BOUNCE" => f.pw.put(&format!("_BOUNCE:{}", c.uid), 1),
        "CHO_CHORUS" => {
            let songs: Vec<u64> = f.hand.iter().filter(|h| melodies(&h.name()).iter().any(|m| m == "WAH" || m == "KI")).map(|h| h.uid).collect();
            for uid in songs {
                play_free(f, uid);
            }
        }
        "KI_CHANT" => {
            let n = (if up { 4 } else { 3 }).min(f.draw.len());
            let top: Vec<Card> = f.draw[f.draw.len() - n..].to_vec();
            let kept: Vec<u64> = f.pile_pick("Choose a card to add into your Hand.", &top, 1).iter().map(|k| k.uid).collect();
            for t in top {
                let card = f.take_from(Pile::Draw, t.uid).expect("on top of the draw pile");
                if kept.contains(&t.uid) {
                    f.add_card(card, "hand");
                } else {
                    f.discard.push(card);
                }
            }
        }
        _ => {}
    }
    tuned(f);
}

/// What a Melody new this turn sets off (once a card, whatever plays it twice).
fn tuned(f: &mut Fight) {
    let n = f.turn_cards.len();
    if f.pw.val("_TUNED") == n as i64 || n == 0 {
        return;
    }
    f.pw.put("_TUNED", n as i64);
    let (before, now) = (tune(f, Some(n - 1)), tune(f, None));
    let new: Vec<&String> = now.difference(&before).collect();
    if new.is_empty() {
        return;
    }
    let (cho, wah) = (new.iter().any(|m| *m == "CHO"), new.iter().any(|m| *m == "WAH"));
    if cho && f.pw.on("SONG_PATH") {
        f.gain_block(f.pw["SONG_PATH"]);
    }
    if wah && f.pw.on("VIOLENT_CHORD") {
        f.hurt_all(f.pw["VIOLENT_CHORD"]);
        reap(f);
    }
    if new.iter().any(|m| *m == "KI") && f.pw.on("TUNE") {
        // Tune: "Whenever you gain a Ki Melody, Upgrade a random card in your Hand."
        for _ in 0..f.pw["TUNE"] {
            let plain: Vec<usize> = (0..f.hand.len()).filter(|n| !f.hand[*n].up && !matches!(f.hand[*n].r#type(), "Status" | "Curse")).collect();
            if plain.is_empty() {
                break;
            }
            let at = *f.run.rng.choice(&plain);
            f.hand[at].up = true;
        }
    }
    if now.len() == MELODIES.len() && f.pw.on("LULLABY") {
        f.gain_block(f.pw["LULLABY"]);
    }
    if f.pw.on("MOTIF") {
        f.draw_cards(f.pw["MOTIF"] * new.len() as i64);
    }
}

/// After the card has gone to its pile: Mourning Blow turned into a Sadness.
pub fn put_back(f: &mut Fight, uid: u64) {
    if f.pw.shift_remove(&format!("_SAD:{uid}")).is_some() {
        if let Some(c) = f.take_from(Pile::Discard, uid) {
            f.exhausted.push(c); // (it is that Sadness now)
            sadden(f, 1);
        }
    }
}

/// A Power of the Fool's played (`_register` has added the first number of its text).
pub fn power(f: &mut Fight, c: &Card) {
    let up = c.up;
    match c.key() {
        "YAH" if up => add(&mut f.pw, "YAH", 1),
        "QUIVER" => {
            let n = f.pw.shift_remove("QUIVER").unwrap_or(1); // (no power the game shows)
            add(&mut f.pw, "_ARROWS", n)
        }
        "HONING" => add(&mut f.pw, "HONING", 99), // "Arrows deal double damage" (a copy: 100% more)
        "ENNUI" => {
            add(&mut f.pw, "_ENNUI_SAD", 1);
            add(&mut f.pw, "ENNUI", if up { 2 } else { 1 }) // (its amount is its draw: 2, 3 upgraded)
        }
        _ => 0,
    };
}

pub fn fight_start(f: &mut Fight) {
    f.pw.put("_ARROWS", 1);
}

/// Before the turn's draw: Archery, Hidden Roots, Ecstasy, Ennui, Bounce; Cho Barbs' Thorns gone.
pub fn turn_start(f: &mut Fight) {
    for k in ["_YAH_DONE", "_STRETCH_SPENT", "_TUNED"] {
        f.pw.shift_remove(k);
    }
    if let Some(n) = f.pw.shift_remove("CHO_BARBS") {
        add(&mut f.pw, "THORNS", -n);
    }
    for (power, gives) in [("ARCHERY", "AIM"), ("HIDDEN_ROOTS", "STRETCH"), ("ENNUI", "DRAW_CARDS_NEXT_TURN")] {
        let n = f.pw.val(power);
        if n != 0 {
            add(&mut f.pw, gives, n);
        }
    }
    cheer(f, f.pw.val("ECSTASY"));
    sadden(f, f.pw.val("_ENNUI_SAD"));
    let bounced: Vec<u64> = f.discard.iter().filter(|h| f.pw.on(&format!("_BOUNCE:{}", h.uid))).map(|h| h.uid).collect();
    for uid in bounced {
        f.pw.shift_remove(&format!("_BOUNCE:{uid}"));
        let c = f.take_from(Pile::Discard, uid).expect("in the discard pile");
        f.add_card(c, "hand");
    }
}

/// Before the hand goes: Simpleton plays the cheapest card; Ki Verse, Quietus and Dislike You end.
pub fn turn_end(f: &mut Fight) {
    for _ in 0..f.pw.val("SIMPLETON") {
        let cheapest = base::min_by_key(f.hand.iter().filter(|h| h.cost() >= 0 && fx::can_play(f, h)).map(|h| (h.uid, f.cost_of(h))), |(_, c)| *c);
        match cheapest {
            Some((uid, _)) if !f.won && !f.lost => play_free(f, uid),
            _ => break,
        }
    }
    f.pw.shift_remove("KI_VERSE");
    f.pw.shift_remove("_QUIETUS");
    for foe in f.foes.iter_mut() {
        foe.powers.shift_remove("DISLIKE_YOU");
    }
}

/// The state's Fool part: our Arrows as the mod counts them, Yah! spent, and the hits the Melody cards' texts count.
pub fn view(f: &Fight, player: &mut Value, state: &mut Value) {
    player["arrows"] = json!(f.pw.val("_ARROWS"));
    player["arrows_free"] = json!(free(f));
    state["_dislodged"] = json!(f.pw.on("_YAH_DONE"));
    let hits = 1 + tune(f, None).len();
    for card in player["hand"].as_array_mut().into_iter().flatten() {
        if let Some(text) = card["description"].as_str().filter(|t| t.contains("for each Melody you have") && t.contains("Hits")) {
            card["description"] = json!(format!("{text} (Hits {hits} time{})", if hits == 1 { "" } else { "s" }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::audit::simcards;
    use crate::sim::content;

    fn fight() -> Fight {
        let g = content::content("sts2");
        let mut f = simcards::fight(&g, "sts2", "FOOL");
        f.hand.clear();
        f.foes[0].hp = 200;
        f.foes[0].max_hp = 200;
        f
    }

    fn give(f: &mut Fight, key: &str) -> i64 {
        let c = super::super::sts2::make(key, false);
        f.hand.push(c);
        (f.hand.len() - 1) as i64
    }

    fn eid(f: &Fight) -> String {
        f.foes[0].eid.clone()
    }

    #[test]
    fn a_shot_stands_in_its_target_until_three_hits_and_yah_pays_once() {
        let _g = crate::games::playing("sts2");
        let mut f = fight();
        assert_eq!((f.pw.val("_ARROWS"), f.pw.val("AIM")), (1, 6), "the Song Bow's Aim");
        f.pw.put("YAH", 1);
        let e = eid(&f);
        let at = give(&mut f, "FOOLISH_SHOT");
        f.play(at, Some(&e));
        assert_eq!((f.foes[0].hp, f.foes[0].powers.val("IMPALED"), free(&f), f.pw.val("AIM")), (200 - 21, 3, 0, 0));
        let at = give(&mut f, "FOOLISH_SHOT");
        let energy = f.energy;
        f.play(at, Some(&e));
        assert_eq!((f.foes[0].hp, f.energy), (200 - 21, energy - 1), "no Arrow free: nothing");
        for n in [2, 1] {
            let at = give(&mut f, "STRIKE_FOOL");
            f.play(at, Some(&e));
            assert_eq!(f.foes[0].powers.val("IMPALED"), n);
        }
        let energy = f.energy;
        let at = give(&mut f, "STRIKE_FOOL");
        f.play(at, Some(&e));
        assert_eq!((free(&f), f.energy, f.pw.val("_YAH_DONE")), (1, energy - 1 + 1, 1), "the third hit: the Arrow back, Yah!'s energy");
        let state = f.state();
        assert_eq!((state["player"]["arrows_free"].as_i64(), state["_dislodged"].as_bool()), (Some(1), Some(true)));
    }

    #[test]
    fn melodies_pay_once_and_count_for_the_cards_that_read_them() {
        let _g = crate::games::playing("sts2");
        let mut f = fight();
        f.pw.put("SONG_PATH", 5);
        f.pw.put("MOTIF", 1);
        f.draw.extend((0..5).map(|_| super::super::sts2::make("DEFEND_FOOL", false)));
        let e = eid(&f);
        let at = give(&mut f, "CHO_FIRM");
        let held = f.hand.len();
        f.play(at, None);
        assert_eq!((f.block, f.hand.len()), (5 + 5 + 5, held), "Cho Firm twice with an Arrow free, Song Path, Motif's card");
        let at = give(&mut f, "CHO_BARBS");
        f.play(at, None);
        assert_eq!(f.block, 15 + 8, "Cho again: no new Melody");
        let at = give(&mut f, "RHYTHM_STRIKE");
        let view = &f.state()["player"]["hand"][at as usize]["description"];
        assert!(view.as_str().unwrap().ends_with("(Hits 2 times)"), "{view}");
        f.play(at, Some(&e));
        assert_eq!(f.foes[0].hp, 200 - 8);
    }

    #[test]
    fn stretch_goes_on_the_first_block_and_sadness_is_made_and_kept() {
        let _g = crate::games::playing("sts2");
        let mut f = fight();
        f.pw.put("ALL_IS_WELL", 4);
        let e = eid(&f);
        let at = give(&mut f, "DEEP_STRETCH");
        f.play(at, None);
        assert!(f.state()["player"]["hand"].as_array().unwrap().is_empty() || f.pw.val("STRETCH") == 7);
        let at = give(&mut f, "DEFEND_FOOL");
        f.play(at, None);
        let at = give(&mut f, "DEFEND_FOOL");
        f.play(at, None);
        assert_eq!(f.block, 5 + 7 + 5);
        let at = give(&mut f, "SORROWFUL_STRIKE");
        f.play(at, Some(&e));
        let at = give(&mut f, "VITRIOL");
        f.play(at, Some(&e));
        assert_eq!((f.hand.iter().filter(|h| h.key() == "SADNESS").count(), f.pw.val("_SAD_MADE"), f.block), (3, 3, 17 + 12));
        let at = give(&mut f, "ANGUISH");
        let hp = f.foes[0].hp;
        f.play(at, Some(&e));
        assert_eq!(hp - f.foes[0].hp, 24);
        f.player_turn_end();
        assert_eq!(f.hand.iter().filter(|h| h.key() == "SADNESS").count(), 3, "Retained");
    }
}
