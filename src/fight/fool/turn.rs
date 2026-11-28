//! The Fool's Arrows, Aim, Stretch and Melodies, as a line of cards the planner tries plays out.
//!
//! A Shot puts our Arrow in its target (IMPALED 3) until three more hits land on it or it dies; with no Arrow to hand it does nothing.
//! Aim and Stretch show in every Shot's and Block card's text, and only the first such card gets them. A Wah, Cho or Ki card gives that
//! Melody for the turn; powers "whenever you gain a Melody" pay once a Melody a turn. Yah! pays the first time an Arrow comes out.

use std::borrow::{Borrow, Cow};
use std::collections::BTreeSet;

use serde_json::Value;

use crate::cards::{melodies, CardInfo, MELODIES};
use crate::fight::battle::{Enemy, _stacks};

pub const IMPALE_HITS: i64 = 3;
pub const AIM_LATER: f64 = 0.6; // what a point of Aim kept for a later turn is worth, in damage (the next Shot drawn takes it)
pub const STRETCH_LATER: f64 = 0.5; // ... and a point of Stretch, in Block

/// `fool_powers`' five: (Song Path's Block, Motif's draw, Lullaby's Block, Ki Verse's cards, Yah!'s energy).
pub type Powers = (i64, i64, i64, i64, i64);

/// Arrows we have: one, and one more for each Quiver ("Gain 1 additional Arrow").
pub fn arrows_owned(player: &Value) -> i64 {
    1 + _stacks(player, "QUIVER")
}

/// Enemies holding one of our Arrows.
fn _stuck<E: Borrow<Enemy>>(foes: &[E]) -> i64 {
    foes.iter().map(|e| e.borrow()).filter(|e| e.alive() && e.impaled > 0).count() as i64
}

/// Arrows that can still be shot: those owned less those standing in a living enemy (-1 owned: not counted).
pub fn free_arrows<E: Borrow<Enemy>>(arrows: i64, foes: &[E]) -> i64 {
    if arrows < 0 {
        1
    } else {
        arrows - _stuck(foes)
    }
}

/// The Melodies the cards played so far this turn have given.
pub fn played_melodies<S: AsRef<str>>(names: &[S]) -> BTreeSet<String> {
    names.iter().flat_map(|name| melodies(name.as_ref())).collect()
}

/// (Song Path's Block, Motif's draw, Lullaby's Block, Ki Verse's cards, Yah!'s energy), None with none up (Yah!: 0 once `dislodged`).
pub fn fool_powers(player: &Value, dislodged: bool) -> Option<Powers> {
    let got = (_stacks(player, "SONG_PATH"), _stacks(player, "MOTIF"), _stacks(player, "LULLABY"), _stacks(player, "KI_VERSE"), if dislodged { 0 } else { _stacks(player, "YAH") });
    if got != (0, 0, 0, 0, 0) {
        Some(got)
    } else {
        None
    }
}

/// Whom a card that wants an Impaled target can go at: the Impaled ones, if there are any (their places in `foes`; else every place).
pub fn impaled_pool<E: Borrow<Enemy>>(c: &CardInfo, foes: &[E]) -> Vec<usize> {
    if c.impaled_only || c.on_impaled.is_some() || c.impaled_double || c.dislodge {
        let stuck: Vec<usize> = (0..foes.len()).filter(|&i| foes[i].borrow().alive() && foes[i].borrow().impaled > 0).collect();
        if !stuck.is_empty() {
            return stuck;
        }
    }
    (0..foes.len()).collect()
}

/// Whether a card needs FoolTurn.before at all (most of every other character's do not).
#[inline]
pub fn concerns(c: &CardInfo) -> bool {
    c.shoots || c.impaled_only || c.on_impaled.is_some() || c.impaled_double || c.arrow_block || c.melody_hits || c.melody_block != 0 || !c.melody_twice.is_empty()
}

/// What a line has done so far to our Arrows, Aim, Stretch and Melodies.
#[derive(Clone, Debug, PartialEq)]
pub struct FoolTurn {
    /// owned (-1: not counted)
    pub arrows: i64,
    /// Aim on us now, and as the card texts were written
    pub aim: i64,
    pub aim0: i64,
    pub stretch: i64,
    pub stretch0: i64,
    /// what this line has added and not spent
    pub aim_got: i64,
    pub stretch_got: i64,
    /// Melodies this turn (and how many the texts counted)
    pub tune: BTreeSet<String>,
    pub tune0: i64,
    /// Violent Chord: damage to ALL enemies whenever we gain a Wah Melody
    pub chord: i64,
    /// Explosive Arrows: damage to ALL enemies whenever we Impale one
    pub boom: i64,
    /// the card being played Shoots an Arrow it has
    pub live: bool,
    // Song Path (Block on a new Cho Melody), Motif (draws on each new Melody), Lullaby (Block once all three are in), Ki Verse (cards to play again), Yah! (energy)
    pub song: i64,
    pub motif: i64,
    pub lullaby: i64,
    pub verse: i64,
    pub yah: i64,
    /// enemies holding one of our Arrows
    pub stuck: i64,
    /// (Block, cards drawn, energy) the last card booked set off through those powers
    pub got: (i64, i64, i64),
}

impl Default for FoolTurn {
    /// `FoolTurn()`: Arrows not counted, nothing else.
    fn default() -> Self {
        FoolTurn::new::<Enemy>(-1, 0, 0, &BTreeSet::new(), 0, 0, None, &[])
    }
}

impl FoolTurn {
    /// (the original's defaults: arrows -1, aim 0, stretch 0, no tune, chord 0, boom 0, no powers, no foes.)
    pub fn new<E: Borrow<Enemy>>(arrows: i64, aim: i64, stretch: i64, tune: &BTreeSet<String>, chord: i64, boom: i64, powers: Option<Powers>, foes: &[E]) -> FoolTurn {
        let (song, motif, lullaby, verse, yah) = powers.unwrap_or((0, 0, 0, 0, 0));
        FoolTurn {
            arrows,
            aim,
            aim0: aim,
            stretch,
            stretch0: stretch,
            aim_got: 0,
            stretch_got: 0,
            tune: tune.clone(),
            tune0: tune.len() as i64,
            chord,
            boom,
            live: false,
            song,
            motif,
            lullaby,
            verse,
            yah,
            stuck: if yah != 0 { _stuck(foes) } else { 0 },
            got: (0, 0, 0),
        }
    }

    /// Ki Verse: 1 if this card is played an additional time (it is a Wah, Cho or Ki card and a Ki Verse waits).
    pub fn encore(&mut self, c: &CardInfo) -> i64 {
        if self.verse != 0 && !c.melody.is_empty() {
            self.verse -= 1;
            return 1;
        }
        0
    }

    /// The card as it plays out at this point of the line (the card itself, borrowed, when nothing about it changes).
    pub fn before<'a, E: Borrow<Enemy>>(&mut self, c: &'a CardInfo, foes: &[E], aimed_at: Option<&str>) -> Cow<'a, CardInfo> {
        let mut c = Cow::Borrowed(c);
        self.live = c.shoots && free_arrows(self.arrows, foes) > 0;
        if c.shoots && !self.live {
            // (Hobbling Shot's Weak and Vulnerable ride on the Arrow)
            let c = c.to_mut();
            c.damage = 0;
            c.block = 0;
            c.weak = 0;
            c.vulnerable = 0;
        } else if c.shoots && self.aim != self.aim0 {
            c.to_mut().damage = 0.max(c.damage + (self.aim - self.aim0) * c.aim_times);
        }
        if c.impaled_only || c.on_impaled.is_some() || c.impaled_double {
            let stuck = foes.iter().map(|e| e.borrow()).any(|e| e.alive() && e.impaled > 0 && aimed_at.is_none_or(|id| &*e.entity_id == id));
            if !stuck && c.impaled_only {
                // (its draw and energy too)
                let c = c.to_mut();
                c.damage = 0;
                c.weak = 0;
                c.vulnerable = 0;
                c.draw = 0;
                c.energy_gain = 0;
                c.dislodge = false;
            } else if let (true, Some((energy, draw))) = (stuck, c.on_impaled) {
                let c = c.to_mut();
                c.energy_gain += energy;
                c.draw += draw;
            } else if stuck && c.impaled_double {
                c.to_mut().damage *= 2;
            }
        }
        let tune = self.tune.len() as i64;
        if c.melody_hits && tune != self.tune0 {
            c.to_mut().hits = 1.max(c.hits + tune - self.tune0); // (its text counted the Melodies there were)
        }
        if c.melody_block != 0 {
            c.to_mut().block = c.melody_block * tune;
        }
        if !c.melody_twice.is_empty() && c.melody_twice.iter().any(|m| self.tune.contains(m)) {
            c.to_mut().hits *= 2;
        }
        if c.arrow_block && free_arrows(self.arrows, foes) > 0 {
            c.to_mut().block *= 2;
        }
        if c.block != 0 && (self.stretch != 0 || self.stretch0 != 0) {
            // every Block card's text shows the Stretch, the first one played takes it
            c.to_mut().block = 0.max(c.block - self.stretch0 + self.stretch);
            self.stretch = 0;
            self.stretch_got = 0;
        }
        c
    }

    /// Book what the card did (`dealt`: what its hits did to the target); return the damage to ALL it sets off, leave the rest in `got`.
    /// `target`: its place in `foes` (the original defaults: xs 1, dealt 0, no foes).
    pub fn after(&mut self, c: &CardInfo, foes: &mut [Enemy], target: Option<usize>, xs: i64, dealt: i64) -> i64 {
        let (mut boom, mut energy) = (0, 0);
        // (Yah!: the third hit on the Impaled enemy and its death are counted by now; a Dislodge is about to be.)
        if self.yah != 0 && (_stuck(foes) < self.stuck || (c.dislodge && target.is_some_and(|t| foes[t].alive() && foes[t].impaled > 0))) {
            energy = self.yah;
            self.yah = 0;
        }
        if let Some(t) = target {
            if self.live && foes[t].alive() {
                foes[t].impaled = IMPALE_HITS;
                boom = self.boom;
            }
            if c.dislodge {
                foes[t].impaled = 0; // (Uproot, Recall: the Arrow comes back)
            }
        }
        if self.yah != 0 {
            self.stuck = _stuck(foes);
        }
        if self.live {
            self.aim = 0;
            self.aim_got = 0;
        }
        self.aim += c.aim * xs;
        self.aim_got += c.aim * xs;
        let gained = c.stretch + (if c.stretch_dealt { dealt } else { 0 });
        self.stretch += gained * xs;
        self.stretch_got += gained * xs;
        let new: Vec<&String> = c.melody.iter().filter(|m| !self.tune.contains(*m)).collect();
        let fresh = new.iter().collect::<BTreeSet<_>>().len(); // (as a set: a name twice is one Melody)
        let most = MELODIES.len();
        let full = self.tune.len() < most && most <= self.tune.len() + fresh; // (Lullaby: "When you gain every Melody")
        let (cho, wah) = (new.iter().any(|m| *m == "CHO"), new.iter().any(|m| *m == "WAH"));
        self.got = (self.song * cho as i64 + self.lullaby * full as i64, self.motif * fresh as i64, energy);
        for m in new {
            self.tune.insert(m.clone());
        }
        self.verse += c.verse;
        self.live = false; // (only this card Shoots)
        (if wah { self.chord } else { 0 }) + boom
    }

    /// Whether `after` has anything to do for this card (the planner skips it for the rest, most of every other character's).
    #[inline]
    pub fn books(&self, c: &CardInfo) -> bool {
        self.live || !c.melody.is_empty() || c.aim != 0 || c.stretch != 0 || c.stretch_dealt || c.dislodge || c.verse != 0 || self.yah != 0
    }

    /// What the Aim and Stretch this line adds and leaves unspent are worth next turn.
    pub fn later(&self, dmg_value: f64) -> f64 {
        self.aim.min(self.aim_got) as f64 * AIM_LATER * dmg_value + self.stretch.min(self.stretch_got) as f64 * STRETCH_LATER
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::parse;
    use crate::fight::battle::testing::*;
    use crate::fight::battle::{read_enemies, _hit};
    use crate::base::JsonExt;
    use serde_json::json;

    fn fool(v: &Value) -> FoolTurn {
        let v = &v["F"];
        let n = |k: &str| {
            assert!(v[k].is_int(), "{k} in {v}");
            v[k].int()
        };
        FoolTurn {
            arrows: n("arrows"),
            aim: n("aim"),
            aim0: n("aim0"),
            stretch: n("stretch"),
            stretch0: n("stretch0"),
            aim_got: n("aim_got"),
            stretch_got: n("stretch_got"),
            tune: v["tune"].list().iter().map(|m| m.s().to_string()).collect(),
            tune0: n("tune0"),
            chord: n("chord"),
            boom: n("boom"),
            live: v["live"].as_bool().unwrap(),
            song: n("song"),
            motif: n("motif"),
            lullaby: n("lullaby"),
            verse: n("verse"),
            yah: n("yah"),
            stuck: n("stuck"),
            got: (v["got"][0].int(), v["got"][1].int(), v["got"][2].int()),
        }
    }

    fn five(v: &Value) -> Option<Powers> {
        if v.list().is_empty() {
            None
        } else {
            Some((v[0].int(), v[1].int(), v[2].int(), v[3].int(), v[4].int()))
        }
    }

    #[test]
    fn golden_states() {
        let (mut checked, mut fools) = (0, 0);
        for (n, row) in states().iter().enumerate() {
            let _env = env(row);
            let (state, r) = (&row["s"], &row["r"]);
            let player = &state["player"];
            let powers = |dislodged: bool| fool_powers(player, dislodged).map(|p| json!([p.0, p.1, p.2, p.3, p.4])).unwrap_or(Value::Null);
            let names: Vec<String> = state["_played_names"].list().iter().map(|x| x.text()).collect();
            let got = json!([arrows_owned(player), powers(false), powers(true), played_melodies(&names)]);
            fools += (got != json!([1, null, null, []])) as usize;
            assert_eq!(got, r["fool"], "state {n}");
            let es = read_enemies(state);
            for (c, want) in player["hand"].list().iter().map(parse).zip(r["hand"].list()) {
                assert_eq!(json!([concerns(&c), impaled_pool(&c, &es)]), json!([want[7], want[8]]), "state {n}: {}", c.name);
                checked += 2;
            }
            assert_eq!(json!([free_arrows(-1, &es), free_arrows(1, &es), free_arrows(2, &es)]), r["free_arrows"], "state {n}");
            checked += 7;
        }
        assert!(checked > 5000 && fools > 10, "{checked} checks, {fools} states of the Fool's");
    }

    #[test]
    fn golden_calls() {
        let cards: Value = serde_json::from_str(include_str!("../../../tests/golden/fight/cards.json")).unwrap();
        for row in calls(&cards, "cards", 1200) {
            let c = card_from_json(&row["c"]);
            assert_eq!(json!(concerns(&c)), row["concerns"], "{}", c.name);
        }
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/fight/turn.json")).unwrap();
        for rec in calls(&g, "turn.concerns", 50) {
            assert_eq!(json!(concerns(&card_from_json(&rec["a"][0]))), rec["r"], "concerns {}", rec["a"]);
        }
        for rec in calls(&g, "turn.free_arrows", 150) {
            let all = foes(&rec["a"][1]);
            assert_eq!(json!(free_arrows(rec["a"][0].int(), &all)), rec["r"], "free_arrows {}", rec["a"]);
            assert_eq!(json!(free_arrows(rec["a"][0].int(), &all.iter().collect::<Vec<_>>())), rec["r"]);
        }
        for rec in calls(&g, "turn.impaled_pool", 100) {
            assert_eq!(json!(impaled_pool(&card_from_json(&rec["a"][0]), &foes(&rec["a"][1]))), rec["r"], "impaled_pool {}", rec["a"]);
        }
        for rec in calls(&g, "turn.played_melodies", 40) {
            let names: Vec<&str> = rec["a"][0].list().iter().map(|x| x.s()).collect();
            assert_eq!(json!(played_melodies(&names)), rec["r"], "played_melodies {}", rec["a"]);
        }
        for rec in calls(&g, "FoolTurn.__init__", 150) {
            let a = &rec["a"];
            let tune: BTreeSet<String> = a[3].list().iter().map(|m| m.s().to_string()).collect();
            let got = FoolTurn::new(a[0].int(), a[1].int(), a[2].int(), &tune, a[4].int(), a[5].int(), five(&a[6]), &foes(&a[7]));
            assert_eq!(got, fool(&rec["after"]), "FoolTurn {a}");
        }
        assert_eq!(FoolTurn::default(), FoolTurn::new::<Enemy>(-1, 0, 0, &BTreeSet::new(), 0, 0, None, &[]));
        for rec in calls(&g, "FoolTurn.encore", 80) {
            let mut ft = fool(&rec["f"]);
            assert_eq!(json!(ft.encore(&card_from_json(&rec["a"][0]))), rec["r"], "encore {}", rec["a"]);
            assert_eq!(ft, fool(&rec["after"]), "encore {}", rec["a"]);
        }
        for rec in calls(&g, "FoolTurn.before", 40) {
            let (mut ft, c) = (fool(&rec["f"]), card_from_json(&rec["a"][0]));
            let got = ft.before(&c, &foes(&rec["a"][1]), rec["a"][2].as_str()).into_owned();
            assert_eq!(got, card_from_json(&rec["r"]), "before {} {}", rec["f"], rec["a"]);
            assert_eq!(ft, fool(&rec["after"]), "before {} {}", rec["f"], rec["a"]);
        }
        for rec in calls(&g, "FoolTurn.after", 80) {
            let (mut ft, a) = (fool(&rec["f"]), &rec["a"]);
            let mut all = foes(&a[1]);
            let got = ft.after(&card_from_json(&a[0]), &mut all, a[2].as_u64().map(|t| t as usize), a[3].int(), a[4].int());
            assert_eq!(json!(got), rec["r"], "after {} {a}", rec["f"]);
            assert_eq!(ft, fool(&rec["after"]), "after {} {a}", rec["f"]);
            assert_eq!(all, foes(&rec["m"]), "after {} {a}", rec["f"]);
        }
        for rec in calls(&g, "FoolTurn.books", 80) {
            assert_eq!(json!(fool(&rec["f"]).books(&card_from_json(&rec["a"][0]))), rec["r"], "books {} {}", rec["f"], rec["a"]);
        }
        for rec in calls(&g, "FoolTurn.later", 25) {
            assert_eq!(json!(fool(&rec["f"]).later(rec["a"][0].float())), rec["r"], "later {} {}", rec["f"], rec["a"]);
        }
        // One card of a line, as combat.simulate goes through it: encore, before, the hits, books, after, later.
        let (mut shot, mut changed) = (0, 0);
        for (n, rec) in calls(&g, "FoolTurn.step", 600).iter().enumerate() {
            let (mut ft, c, mut all, want) = (fool(&rec["f"]), card_from_json(&rec["c"]), foes(&rec["foes"]), &rec["r"]);
            let why = format!("step {n}: {} {}", rec["f"], rec["c"]);
            let pool = impaled_pool(&c, &all);
            let encore = ft.encore(&c);
            let played = ft.before(&c, &all, rec["aimed"].as_str());
            changed += matches!(played, Cow::Owned(_)) as usize;
            let played = played.into_owned();
            assert_eq!(played, card_from_json(&want["played"]), "{why}");
            let (live, target) = (ft.live, rec["t"].as_u64().map(|t| t as usize));
            shot += live as usize;
            let mut dealt = 0;
            if let Some(t) = target {
                if played.damage != 0 {
                    dealt = _hit(&mut all[t], played.damage, played.hits, false, false);
                }
            }
            let books = ft.books(&played);
            let after = ft.after(&played, &mut all, target, rec["xs"].int(), dealt);
            let later = ft.later(rec["dv"].float());
            assert_eq!(json!({"pool": pool, "encore": encore, "played": want["played"], "live": live, "books": books, "dealt": dealt, "after": after, "later": later}), *want, "{why}");
            assert_eq!(ft, fool(&rec["after"]), "{why}");
            assert_eq!(all, foes(&rec["m"]), "{why}");
        }
        assert!(shot > 50 && changed > 100, "{shot} Shots, {changed} cards changed");
    }
}
