//! One fight: the bot's planner picks every card and potion, the engine carries out what they and the enemies do.
//!
//! The order of a round is the first game's: our turn (Block gone unless kept, energy, start-of-turn powers, draw,
//! cards, end-of-turn powers and orbs, discard), then each enemy in turn (its Block gone, Poison, its move), then the
//! debuffs tick down and the enemies choose their next moves. Damage is worked out the game's way: base plus
//! Strength, times Weak, stance and Vulnerable, rounded down, Block first.
//!
//! How this differs from the original (see also sim/content/mod.rs): the fight owns its run while it lasts
//! (`Fight::new(run, foes, kind)`, `f.run`), the random generator is the run's (`f.run.rng`), enemies are named by
//! their index in `f.foes`, cards by their `uid`, orbs are [`Orb`]s.

use std::collections::HashSet;
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::fight::combat;
use crate::base::{self, JsonExt, Random};
use crate::run::strategy;
use crate::sim::cards::{Card, ViewArgs};
use crate::sim::content::{fool, Content};
use crate::sim::effects::{self, add, Powers};
use crate::sim::monsters::{self, FightLike};
use crate::sim::run::Run;

pub type Foe = monsters::Foe<Fight>;
pub type Spec = monsters::Spec<Fight>;
pub type Move = monsters::Move<Fight>;
pub type Fx = monsters::Fx<Fight>;

pub const HAND_MAX: i64 = 10;
pub const MAX_ROUNDS: i64 = 45; // a fight still going then is lost (a stalemate the planner cannot break)
pub const MAX_ACTIONS: i64 = 80; // actions in one turn before the engine ends it for the bot

/// `ORB_BASE[kind]`: (passive, evoke).
#[allow(non_snake_case)]
pub fn ORB_BASE(kind: &str) -> Option<(i64, i64)> {
    Some(match kind {
        "LIGHTNING" => (3, 8),
        "FROST" => (2, 5),
        "DARK" => (6, 6),
        "PLASMA" => (1, 2),
        _ => return None,
    })
}

/// Counters the engine keeps among our powers that the game would not show as powers.
pub const INTERNAL: &[&str] = &[
    "DRAWN_TURN", "OSTY_MAX", "FORGE", "TURN_ENERGY", "VOID_FREE", "KEEP_HAND_ALWAYS", "SURCHARGE", "MANTRA_GAINED", "CLAW_BONUS",
    "EXPUNGER_X", "DISCARDED_TURN", "BLASPHEMY_DUE", "SIMMERING", "DEVA_ENERGY", "KEEP_HAND", "KEEP_BLOCK", "TURN_OVER", "EXTRA_TURN",
    "COMBUST_HP", "DOUBLE_DAMAGE_NEXT", "AMPLIFY", "CONFUSED", "BLUR", "WELL_LAID_PLANS",
    "LIGHTNING_CHANNELED", "FROST_CHANNELED", "DARK_CHANNELED", "PLASMA_CHANNELED", "GLASS_CHANNELED",
];

/// An orb: the original's `[kind, Dark's stored damage]` (the list object's identity is `uid`).
#[derive(Clone, Debug, PartialEq)]
pub struct Orb {
    pub kind: String,
    pub dark: i64,
    pub uid: u64,
}

/// One of our piles of cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pile {
    Hand,
    Draw,
    Discard,
    Exhausted,
}

pub struct Fight {
    pub run: Run,
    pub game: Rc<Content>,
    pub kind: String,
    pub foes: Vec<Foe>,
    pub draw: Vec<Card>, // (the end of the list is the top)
    pub hand: Vec<Card>,
    pub discard: Vec<Card>,
    pub exhausted: Vec<Card>,
    pub block: i64,
    pub energy: i64,
    pub pw: IndexMap<String, i64>,   // our powers
    pub temp: IndexMap<String, i64>, // Strength and Dexterity that go at the end of this turn
    pub orbs: Vec<Orb>,              // oldest first
    pub orb_slots: i64,
    pub stance: String,
    pub mantra: i64,
    pub stars: i64,
    pub osty: i64,
    pub round: i64,
    pub attacks_turn: i64,
    pub skills_turn: i64,
    pub turn_cards: Vec<String>,
    pub last_type: String,
    pub double_next: i64, // Double Tap: Attacks to play twice
    pub burst: i64,       // Burst: Skills to play twice
    pub no_draw: bool,
    pub free_attack: bool,
    pub won: bool,
    pub lost: bool,
    pub escaped: bool,
    pub threat: IndexMap<String, (i64, i64, f64, i64)>,
    pub facing: Option<String>,
    pub plays: i64, // cards the planner has played (for the trace)
    pub digest: Vec<String>,
    pub triggers: Vec<(String, String, String)>, // Slay the Spire 2's Powers, read from their text: (when, what, card key)
    pub fresh: HashSet<String>, // debuffs an enemy gave us this round: the round's end passes them by once
    pub bound: HashSet<u64>,    // Chains of Binding: the cards Bound this turn (one of them can be played)
    pub nightmares: Vec<Card>,  // (Slay the Spire 2's Nightmare: the original's `f.__dict__["nightmares"]`)
    pub foregone: i64,          // (Foregone Conclusion: `f.__dict__["foregone"]`)
    orb_uid: u64,
}

impl FightLike for Fight {
    fn with_foe<R>(&mut self, i: usize, body: impl FnOnce(&mut Foe, &mut Random) -> R) -> R {
        body(&mut self.foes[i], &mut self.run.rng)
    }
}

impl Fight {
    pub fn new(mut run: Run, foes: Vec<Foe>, kind: &str) -> Fight {
        let mut deck: Vec<Card> = run.deck.iter().map(|c| c.copy()).collect();
        run.rng.shuffle(&mut deck);
        let (innate, rest): (Vec<Card>, Vec<Card>) = deck.into_iter().partition(|c| c.has("Innate"));
        let mut draw = rest;
        draw.extend(innate);
        let orb_slots = run.orb_slots;
        let osty = if run.character == "NECROBINDER" { 0 } else { -1 };
        Fight {
            game: run.game.clone(),
            run,
            kind: kind.to_string(),
            foes,
            draw,
            hand: Vec::new(),
            discard: Vec::new(),
            exhausted: Vec::new(),
            block: 0,
            energy: 0,
            pw: IndexMap::new(),
            temp: IndexMap::new(),
            orbs: Vec::new(),
            orb_slots,
            stance: String::new(),
            mantra: 0,
            stars: 0,
            osty,
            round: 0,
            attacks_turn: 0,
            skills_turn: 0,
            turn_cards: Vec::new(),
            last_type: String::new(),
            double_next: 0,
            burst: 0,
            no_draw: false,
            free_attack: false,
            won: false,
            lost: false,
            escaped: false,
            threat: IndexMap::new(),
            facing: None,
            plays: 0,
            digest: Vec::new(),
            triggers: Vec::new(),
            fresh: HashSet::new(),
            bound: HashSet::new(),
            nightmares: Vec::new(),
            foregone: 0,
            orb_uid: 0,
        }
    }

    // ------------------------------------------------------------------ cards by identity

    pub fn pile(&self, pile: Pile) -> &Vec<Card> {
        match pile {
            Pile::Hand => &self.hand,
            Pile::Draw => &self.draw,
            Pile::Discard => &self.discard,
            Pile::Exhausted => &self.exhausted,
        }
    }

    pub fn pile_mut(&mut self, pile: Pile) -> &mut Vec<Card> {
        match pile {
            Pile::Hand => &mut self.hand,
            Pile::Draw => &mut self.draw,
            Pile::Discard => &mut self.discard,
            Pile::Exhausted => &mut self.exhausted,
        }
    }

    /// `c in pile`.
    pub fn in_pile(&self, pile: Pile, uid: u64) -> bool {
        self.pile(pile).iter().any(|c| c.uid == uid)
    }

    /// `pile.remove(c)`: the card itself, taken out (None if it is not there).
    pub fn take_from(&mut self, pile: Pile, uid: u64) -> Option<Card> {
        let cards = self.pile_mut(pile);
        let at = cards.iter().position(|c| c.uid == uid)?;
        Some(cards.remove(at))
    }

    /// The card with this `uid`, in whichever pile it lies.
    pub fn card(&self, uid: u64) -> Option<&Card> {
        self.hand.iter().chain(&self.draw).chain(&self.discard).chain(&self.exhausted).find(|c| c.uid == uid)
    }

    pub fn card_mut(&mut self, uid: u64) -> Option<&mut Card> {
        self.hand.iter_mut().chain(&mut self.draw).chain(&mut self.discard).chain(&mut self.exhausted).find(|c| c.uid == uid)
    }

    // ------------------------------------------------------------------ numbers

    pub fn strength(&self) -> i64 {
        self.pw.val("STRENGTH") + self.temp.val("STRENGTH")
    }

    pub fn dexterity(&self) -> i64 {
        self.pw.val("DEXTERITY") + self.temp.val("DEXTERITY")
    }

    /// Focus, this turn's included (Slay the Spire 2's Hotfix and Hyperbeam: until 2026-11-26 only the lasting kind counted).
    pub fn focus(&self) -> i64 {
        self.pw.val("FOCUS") + self.temp.val("FOCUS")
    }

    pub fn stance_mult(&self) -> f64 {
        match self.stance.as_str() {
            "WRATH" => 2.0,
            "DIVINITY" => 3.0,
            _ => 1.0,
        }
    }

    /// What one hit of ours does before Block: Strength, Vigor, Weak, stance, and the target's Vulnerable (`plain`: none of them).
    pub fn attack_value(&self, base: i64, foe: Option<usize>, osty: bool, plain: bool) -> i64 {
        if plain {
            return base.max(0);
        }
        let mut d = (base + if osty { self.pw.val("CALCIFY") } else { self.strength() + self.pw.val("VIGOR") }) as f64;
        if self.pw.on("WEAK") && !osty {
            d *= 0.75;
        }
        if self.pw.on("SHRINK") && !osty {
            d *= 0.7;
        }
        d *= if osty { 1.0 } else { self.stance_mult() };
        if let Some(foe) = foe.map(|i| &self.foes[i]) {
            if foe.powers.on("VULNERABLE") {
                d *= (if self.run.has("PAPER_PHROG") { 1.75 } else { 1.5 }) + (if self.pw.on("CRUELTY") { 0.25 } else { 0.0 });
            }
            if foe.powers.on("WEAK") && self.pw.on("TRACKING") {
                d *= 1.5;
            }
            if foe.powers.on("FLUTTER") || foe.powers.on("SOAR") {
                d *= 0.5;
            }
            if foe.powers.on("SLOW") {
                // 10% more for each card played before this one this turn
                d *= 1.0 + 0.1 * (self.turn_cards.len() as i64 - 1).max(0) as f64;
            }
        }
        (d.floor() as i64).max(0)
    }

    pub fn block_value(&self, base: i64) -> i64 {
        let mut b = (base + self.dexterity()) as f64;
        if self.pw.on("FRAIL") {
            b *= 0.75;
        }
        (b.floor() as i64).max(0)
    }

    /// What one of its hits does to us before Block (what its intent shows).
    pub fn foe_hit(&self, foe: usize, base: i64) -> i64 {
        let foe = &self.foes[foe];
        let mut d = (base + foe.powers.val("STRENGTH") + foe.mem.get("spark", 0)) as f64;
        if self.pw.on("SURROUNDED") && (foe.powers.on("BACK_ATTACK_LEFT") || foe.powers.on("BACK_ATTACK_RIGHT")) && self.facing.as_deref() != Some(foe.eid.as_str()) {
            d *= 1.5;
        }
        if foe.powers.on("WEAK") {
            d *= 0.75;
        }
        if self.pw.on("VULNERABLE") {
            d *= if self.run.has("ODD_MUSHROOM") { 1.25 } else { 1.5 };
        }
        if self.stance == "WRATH" {
            d *= 2.0;
        }
        (d.floor() as i64).max(0)
    }

    // ------------------------------------------------------------------ what the bot sees

    pub fn live(&self, c: &Card) -> (Option<i64>, Option<i64>) {
        let i = c.info();
        let base = effects::base_damage(self, c);
        let dmg = match base {
            Some(base) if c.r#type() == "Attack" => Some(self.attack_value(base, None, i.osty_attack, false)),
            _ => None,
        };
        let blk = if i.block != 0 && i.melody_block == 0 { Some(self.block_value(i.block + c.bonus + self.pw.val("STRETCH"))) } else { None };
        (dmg, blk)
    }

    pub fn playable(&self, c: &Card) -> bool {
        let cost = self.cost_of(c);
        if cost == -2
            || !effects::can_play(self, c)
            || self.pw.on("ENTANGLED") && c.r#type() == "Attack"
            || self.pw.on("SMOGGY") && c.r#type() == "Skill" && self.skills_turn != 0
            || self.limit_left() == Some(0)
            || c.star_cost().is_some_and(|s| s > self.stars)
            || self.bound.contains(&c.uid) && self.pw.on("BOUND_PLAYED")
        {
            return false;
        }
        cost < 0 || cost <= self.energy
    }

    pub fn limit_left(&self) -> Option<i64> {
        let caps = [
            if self.run.has("VELVET_CHOKER") { 6 } else { 0 },
            if self.hand.iter().any(|h| h.key() == "NORMALITY") { 3 } else { 0 },
            if self.pw.on("RINGING") { 1 } else { 0 },
            if self.pw.on("SLOTH") { 3 } else { 0 },
        ];
        let least = caps.iter().filter(|n| **n != 0).min()?;
        Some((least - self.turn_cards.len() as i64).max(0))
    }

    pub fn cost_of(&self, c: &Card) -> i64 {
        let cost = c.cost();
        if cost < 0 {
            return cost;
        }
        if c.r#type() == "Skill" && self.pw.on("CORRUPTION") || c.r#type() == "Attack" && self.free_attack || self.pw.on("VOID_FREE") {
            return 0;
        }
        let cost = if fool::playing(self) { fool::cost(self, c, cost) } else { cost };
        cost + self.pw.val("SURCHARGE") + if c.r#type() == "Attack" && self.pw.on("TANGLED") { 1 } else { 0 }
    }

    pub fn state(&self) -> Value {
        let mut hand = Vec::with_capacity(self.hand.len());
        for (n, c) in self.hand.iter().enumerate() {
            let (dmg, blk) = self.live(c);
            let mut view = c.view(ViewArgs { index: Some(n as i64), damage: dmg, block: blk, playable: self.playable(c) });
            let cost = self.cost_of(c);
            view["cost"] = json!(if cost == -1 { "X".to_string() } else { cost.max(0).to_string() });
            if !view["can_play"].truthy() && self.game.SHORT_REASON && cost > self.energy && effects::can_play(self, c) {
                view["unplayable_reason"] = json!("EnergyCostTooHigh");
            }
            hand.push(view);
        }
        let mut shown: IndexMap<&str, i64> = self.pw.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        for (k, v) in self.temp_view() {
            shown.insert(k, v);
        }
        let mut status: Vec<Value> = shown.iter().filter(|(k, v)| **v != 0 && !INTERNAL.contains(k) && !k.starts_with('_')).map(|(k, v)| json!({"id": format!("{k}_POWER"), "amount": v, "description": ""})).collect();
        if self.mantra != 0 {
            status.push(json!({"id": "MANTRA_POWER", "amount": self.mantra, "description": ""})); // (the game shows Mantra as a power)
        }
        let mut player = json!({
            "character": self.run.title, "hp": self.run.hp, "max_hp": self.run.max_hp, "block": self.block, "energy": self.energy,
            "max_energy": self.run.energy, "stars": self.stars, "gold": self.run.gold, "status": status, "hand": hand,
            "draw_pile": self.draw.iter().map(|c| c.view(ViewArgs::default())).collect::<Vec<_>>(),
            "discard_pile": self.discard.iter().map(|c| c.view(ViewArgs::default())).collect::<Vec<_>>(),
            "draw_pile_count": self.draw.len(), "discard_pile_count": self.discard.len(), "exhaust_pile_count": self.exhausted.len(),
            "potions": self.run.potion_views(), "max_potion_slots": self.run.potion_slots(), "relics": self.run.relic_views(),
            "orbs": self.orbs.iter().map(|o| json!({"id": o.kind, "passive_val": self.orb_passive(o), "evoke_val": self.orb_evoke(o)})).collect::<Vec<_>>(),
            "orb_slots": self.orb_slots, "stance": if self.stance.is_empty() { "Neutral" } else { self.stance.as_str() },
        });
        if self.osty >= 0 {
            player["pets"] = json!([{"name": "Osty", "hp": self.osty, "alive": self.osty > 0}]);
        }
        let enemies: Vec<Value> = (0..self.foes.len()).filter(|i| !self.foes[*i].gone).map(|i| self.foe_view(i)).collect();
        let threat: Map<String, Value> = self.threat.iter().map(|(k, (total, rounds, _b, _n))| (k.clone(), json!(*total as f64 / *rounds as f64))).collect();
        let blow: Map<String, Value> = self
            .threat
            .iter()
            .filter(|(_, (_t, _r, _b, attacks))| *attacks != 0)
            .map(|(k, (_t, rounds, blows, attacks))| (k.clone(), json!([blows / *attacks as f64, *attacks as f64 / *rounds as f64])))
            .collect();
        let mut costs = Map::new();
        for c in &self.run.deck {
            costs.insert(c.name(), json!(c.cost().max(0)));
        }
        let mut state = json!({
            "state_type": self.kind, "run": self.run.r#where(), "player": player,
            "battle": {"round": self.round, "turn": "player", "is_play_phase": true, "enemies": enemies},
            "_plays_this_turn": self.turn_cards.len(), "_played_names": self.turn_cards, "_dislodged": false, "_facing": self.facing,
            "_revive_pending": false, "_threat": if self.kind == "monster" { Value::Object(threat) } else { json!({}) }, "_blow": blow, "_costs": costs,
        });
        if fool::playing(self) {
            let mut player = state["player"].take();
            fool::view(self, &mut player, &mut state);
            state["player"] = player;
        }
        state
    }

    pub fn temp_view(&self) -> Vec<(&str, i64)> {
        self.temp.iter().filter(|(k, _)| matches!(k.as_str(), "STRENGTH" | "DEXTERITY" | "FOCUS")).map(|(k, v)| (k.as_str(), self.pw.val(k) + v)).collect()
    }

    pub fn foe_view(&self, i: usize) -> Value {
        let f = &self.foes[i];
        let m = match f.spec.moves.get(&f.move_name) {
            Some(m) => m.clone(),
            None => f.spec.moves.first().expect("a monster with no moves").1.clone(),
        };
        let mut intents: Vec<Value> = Vec::new();
        if self.run.has("RUNIC_DOME") || !m.kind.is_empty() {
            intents.push(json!({"type": if self.run.has("RUNIC_DOME") { "Unknown" } else { m.kind.as_str() }, "label": ""}));
        } else {
            if m.dmg != 0 {
                let (d, hits) = (self.foe_hit(i, effects::move_damage(self, i, &m)), effects::move_hits(self, i, &m));
                intents.push(json!({"type": "Attack", "label": if hits > 1 { format!("{d}x{hits}") } else { d.to_string() }}));
            }
            let has = |words: &[&str]| m.fx.iter().any(|fx| words.contains(&fx.word.as_str()));
            for (kind, shown) in [("Debuff", has(effects::DEBUFF_FX)), ("Buff", has(effects::BUFF_FX)), ("Defend", m.block != 0 && m.dmg == 0)] {
                if shown {
                    intents.push(json!({"type": kind, "label": ""}));
                }
            }
            if intents.is_empty() {
                intents.push(json!({"type": "Unknown", "label": ""}));
            }
        }
        let status: Vec<Value> = f.powers.iter().filter(|(_, v)| **v != 0).map(|(k, v)| json!({"id": format!("{k}_POWER"), "amount": v, "description": ""})).collect();
        json!({"entity_id": f.eid, "name": f.name, "hp": if f.half_dead { 0 } else { f.hp }, "max_hp": f.max_hp, "block": f.block,
               "status": status, "intents": intents})
    }

    pub fn orb_passive(&self, o: &Orb) -> i64 {
        let base = ORB_BASE(&o.kind).unwrap_or((0, 0)).0;
        if o.kind == "PLASMA" {
            base
        } else {
            (base + self.focus()).max(0)
        }
    }

    pub fn orb_evoke(&self, o: &Orb) -> i64 {
        let base = ORB_BASE(&o.kind).unwrap_or((0, 0)).1;
        if o.kind == "DARK" {
            o.dark
        } else if o.kind == "PLASMA" {
            base
        } else {
            (base + self.focus()).max(0)
        }
    }

    // ------------------------------------------------------------------ hurting

    /// The enemies alive, by index.
    pub fn alive(&self) -> Vec<usize> {
        (0..self.foes.len()).filter(|i| self.foes[*i].alive()).collect()
    }

    /// Damage, not an attack, on every enemy alive.
    pub fn hurt_all(&mut self, amount: i64) {
        for i in self.alive() {
            self.hurt(i, amount, false);
        }
    }

    /// Damage on an enemy; the HP it lost.
    pub fn hurt(&mut self, i: usize, mut amount: i64, attack: bool) -> i64 {
        if !self.foes[i].alive() || amount <= 0 {
            return 0;
        }
        let boot = attack && self.run.has("THE_BOOT");
        let foe = &mut self.foes[i];
        if foe.powers.on("INTANGIBLE") {
            amount = amount.min(1);
        }
        if attack && foe.powers.on("SLIPPERY") {
            foe.powers["SLIPPERY"] -= 1;
            amount = amount.min(1);
        }
        if foe.powers.on("HARD_TO_KILL") {
            amount = amount.min(foe.block + foe.powers["HARD_TO_KILL"]);
        }
        if foe.powers.on("HARDENED_SHELL") {
            amount = amount.min(foe.block + (foe.powers["HARDENED_SHELL"] - foe.mem.get("shell", 0)).max(0));
        }
        if boot && 0 < amount && amount < 5 {
            amount = 5;
        }
        let took = foe.block.min(amount);
        foe.block -= took;
        let rest = foe.hp.min(amount - took);
        if attack && foe.powers.on("CURL_UP") && amount > 0 {
            foe.block += foe.powers.shift_remove("CURL_UP").unwrap_or(0);
        }
        if foe.powers.on("SKITTISH") && !foe.mem.truthy("skittish") && amount > 0 {
            foe.mem.set_flag("skittish", true);
            foe.block += foe.powers["SKITTISH"];
        }
        if took != 0 && foe.block == 0 && foe.powers.on("BURROWED") {
            foe.powers.shift_remove("BURROWED");
            foe.stun();
        }
        if rest > 0 {
            foe.hp -= rest;
            foe.mem.add("shell", rest);
            effects::foe_hurt(self, i, rest, attack);
        }
        if self.foes[i].hp <= 0 {
            self.kill(i);
        }
        rest
    }

    pub fn kill(&mut self, i: usize) {
        self.foes[i].hp = 0;
        if effects::foe_hook(self, i, "death", 0, false) {
            return; // it is not gone (a Darkling waiting to rise, the Awakened One reborn)
        }
        self.foes[i].gone = true;
        for ally in self.foes.iter_mut() {
            if ally.alive() && ally.powers.on("CRAB_RAGE") {
                add(&mut ally.powers, "STRENGTH", 6);
                ally.block += 99;
            }
        }
        if self.foes[i].mem.truthy("corpse") {
            let n = self.foes[i].max_hp;
            self.hurt_all(n);
        }
        if self.run.has("GREMLIN_HORN") {
            self.energy += 1;
            self.draw_cards(1);
        }
        if !self.foes.iter().any(|f| f.alive() && !f.minion) {
            for f in self.foes.iter_mut() {
                f.gone = true;
            }
            self.won = true;
        }
    }

    /// Damage on us; the HP we lost.
    pub fn take(&mut self, mut amount: i64, foe: Option<usize>, attack: bool) -> i64 {
        if amount <= 0 {
            return 0;
        }
        if self.pw.on("INTANGIBLE") {
            amount = amount.min(1);
        }
        let took = self.block.min(amount);
        self.block -= took;
        let mut rest = amount - took;
        if rest != 0 && self.osty > 0 && attack {
            let soak = self.osty.min(rest);
            self.osty -= soak;
            rest -= soak;
        }
        if let (true, Some(foe)) = (attack, foe) {
            let thorns = self.pw.val("THORNS") + self.temp.val("FLAME_BARRIER");
            if thorns != 0 {
                self.hurt(foe, thorns, false);
            }
        }
        if rest <= 0 {
            return 0;
        }
        if attack && self.run.has("TORII") && rest <= 5 {
            rest = 1;
        }
        if self.run.has("TUNGSTEN_ROD") {
            rest -= 1;
        }
        if rest <= 0 {
            return 0;
        }
        if self.pw.on("BUFFER") {
            self.pw["BUFFER"] -= 1;
            return 0;
        }
        self.lose_hp(rest, false)
    }

    pub fn lose_hp(&mut self, n: i64, from_card: bool) -> i64 {
        if n <= 0 {
            return 0;
        }
        let n = n.min(self.run.hp);
        self.run.hp -= n;
        if from_card && self.pw.on("RUPTURE") {
            let r = self.pw["RUPTURE"];
            add(&mut self.pw, "STRENGTH", r);
        }
        if self.run.hp <= 0 {
            effects::on_death(self);
        }
        n
    }

    pub fn heal(&mut self, n: i64) {
        self.run.hp = self.run.max_hp.min(self.run.hp + n.max(0));
    }

    pub fn gain_block(&mut self, mut n: i64) {
        if n <= 0 || self.pw.on("NO_BLOCK") {
            return;
        }
        n *= if self.pw.on("SHADOWMELD") { 2 } else { 1 };
        self.block += n;
        effects::block_gained(self, n);
    }

    // ------------------------------------------------------------------ piles

    pub fn draw_cards(&mut self, n: i64) {
        if self.no_draw {
            return;
        }
        for _ in 0..n {
            if self.hand.len() as i64 >= HAND_MAX {
                return;
            }
            self.refill();
            let Some(c) = self.draw.pop() else { return };
            let uid = c.uid;
            self.hand.push(c);
            add(&mut self.pw, "DRAWN_TURN", 1);
            effects::drawn(self, uid);
        }
    }

    /// An empty draw pile takes the discard pile, shuffled.
    pub fn refill(&mut self) {
        if self.draw.is_empty() && !self.discard.is_empty() {
            self.draw = std::mem::take(&mut self.discard);
            self.run.rng.shuffle(&mut self.draw);
        }
    }

    /// `add_card(c, where="hand")`: `where_` is "hand", "draw" (somewhere in it), "top" (of the draw pile) or "discard".
    pub fn add_card(&mut self, c: Card, where_: &str) {
        if where_ == "hand" && (self.hand.len() as i64) < HAND_MAX {
            self.hand.push(c);
        } else if where_ == "draw" {
            let at = self.run.rng.randrange1(self.draw.len() as i64 + 1) as usize;
            self.draw.insert(at, c);
        } else if where_ == "top" {
            self.draw.push(c);
        } else {
            self.discard.push(c);
        }
    }

    /// n new copies of a card (Shivs, statuses ...) into a pile (`add_new(key, n=1, where="hand", up=False)`).
    pub fn add_new(&mut self, key: &str, n: i64, where_: &str, up: bool) {
        let make = self.game.make;
        for _ in 0..n {
            self.add_card(make(key, up), where_);
        }
    }

    pub fn exhaust(&mut self, c: Card) {
        let seen = c.clone();
        self.exhausted.push(c);
        effects::exhausted(self, &seen);
    }

    // ------------------------------------------------------------------ orbs and stances

    pub fn channel(&mut self, kind: &str) {
        if self.orb_slots <= 0 {
            return;
        }
        if self.orbs.len() as i64 >= self.orb_slots {
            self.evoke(1, false);
        }
        self.orb_uid += 1;
        self.orbs.push(Orb { kind: kind.to_string(), dark: if kind == "DARK" { 6 + self.focus() } else { 0 }, uid: self.orb_uid });
    }

    /// `evoke(times=1, keep=False)`.
    pub fn evoke(&mut self, times: i64, keep: bool) {
        let Some(o) = self.orbs.first().cloned() else { return };
        for _ in 0..times {
            let amount = self.orb_evoke(&o);
            self.orb_effect(&o, amount, true);
        }
        if !keep && !self.orbs.is_empty() {
            self.orbs.remove(0);
        }
    }

    pub fn orb_effect(&mut self, o: &Orb, amount: i64, evoke: bool) {
        match o.kind.as_str() {
            "FROST" => self.gain_block(amount),
            "PLASMA" => self.energy += amount,
            "LIGHTNING" => {
                let targets = if self.pw.on("ELECTRODYNAMICS") {
                    self.alive()
                } else {
                    let alive = self.alive();
                    if alive.is_empty() {
                        vec![]
                    } else {
                        vec![*self.run.rng.choice(&alive)]
                    }
                };
                for f in targets {
                    self.hurt(f, amount, false);
                }
            }
            "DARK" if evoke => {
                let alive = self.alive();
                if let Some(low) = base::min_by_key(alive, |i| self.foes[*i].hp) {
                    self.hurt(low, amount, false);
                }
            }
            _ => {}
        }
    }

    pub fn enter(&mut self, stance: &str) {
        let new = if stance == "EXIT" { "" } else { stance };
        if new == self.stance {
            return;
        }
        if self.stance == "CALM" {
            self.energy += 2 + self.run.has("VIOLET_LOTUS") as i64;
        }
        if new == "DIVINITY" {
            self.energy += 3;
        }
        self.stance = new.to_string();
        effects::stance_changed(self, new);
    }

    pub fn gain_mantra(&mut self, n: i64) {
        self.mantra += n;
        add(&mut self.pw, "MANTRA_GAINED", n);
        if self.mantra >= 10 {
            self.mantra -= 10;
            self.enter("DIVINITY");
        }
    }

    // ------------------------------------------------------------------ playing

    /// The enemy with this id, if it is alive.
    pub fn foe_named(&self, target: Option<&str>) -> Option<usize> {
        let target = target?;
        (0..self.foes.len()).find(|i| self.foes[*i].eid == target && self.foes[*i].alive())
    }

    pub fn play(&mut self, index: i64, target: Option<&str>) -> bool {
        if !(0 <= index && index < self.hand.len() as i64) {
            return false;
        }
        let index = index as usize;
        let seen = self.hand[index].clone();
        if !self.playable(&seen) {
            return false;
        }
        let cost = self.cost_of(&seen);
        let mut x = 0;
        if cost == -1 {
            x = self.energy + if self.run.has("CHEMICAL_X") { 2 } else { 0 };
            self.energy = 0;
        } else {
            self.energy -= cost;
        }
        if let Some(stars) = seen.star_cost().filter(|s| *s != 0) {
            self.stars -= stars;
            self.game_hook("stars", Some(&seen));
        }
        if self.pw.on("VOID_FREE") && cost >= 0 {
            self.pw["VOID_FREE"] -= 1;
        }
        if seen.r#type() == "Attack" && self.free_attack {
            self.free_attack = false;
        }
        let mut foe = self.foe_named(target);
        if seen.info().needs_target && foe.is_none() {
            foe = base::min_by_key(self.alive(), |i| self.foes[*i].hp);
        }
        if let Some(foe) = foe {
            self.facing = Some(self.foes[foe].eid.clone());
        }
        let mut c = match self.hand.get(index) {
            Some(h) if h.uid == seen.uid => self.hand.remove(index),
            _ => self.take_from(Pile::Hand, seen.uid).unwrap_or(seen),
        };
        self.turn_cards.push(c.name());
        if self.bound.contains(&c.uid) {
            self.pw.insert("BOUND_PLAYED".to_string(), 1);
        }
        let ctype = c.r#type().to_string();
        let mut times = 1;
        if ctype == "Attack" && self.double_next != 0 {
            self.double_next -= 1;
            times = 2;
        } else if ctype == "Skill" && self.burst != 0 {
            self.burst -= 1;
            times = 2;
        }
        if self.pw.on("DUPLICATION") {
            self.pw["DUPLICATION"] -= 1;
            times += 1;
        }
        if self.pw.val("ECHO_FORM") > self.turn_cards.len() as i64 - 1 {
            times += 1;
        }
        if ctype == "Power" && self.pw.on("AMPLIFY") {
            self.pw["AMPLIFY"] -= 1;
            times += 1;
        }
        times += c.info().replay + fool::encore(self, &c);
        for _ in 0..times {
            if self.won || self.lost {
                break;
            }
            let at = match foe {
                Some(i) if !self.foes[i].alive() => self.alive().first().copied(),
                other => other,
            };
            effects::resolve(self, &mut c, at, x);
        }
        self.attacks_turn += (ctype == "Attack") as i64;
        self.skills_turn += (ctype == "Skill") as i64;
        self.last_type = ctype;
        effects::played(self, &c);
        let uid = c.uid;
        self.put_away(c);
        fool::put_back(self, uid);
        true
    }

    pub fn put_away(&mut self, c: Card) {
        if c.r#type() == "Power" {
            return;
        }
        if effects::BACK_TO_DRAW.contains(&c.key()) {
            self.add_card(c, "draw");
        } else if c.has("Exhaust") || (c.r#type() == "Skill" && self.pw.on("CORRUPTION")) {
            self.exhaust(c);
        } else {
            self.discard.push(c);
        }
    }

    pub fn use_potion(&mut self, slot: i64, target: Option<&str>) -> bool {
        let Some(pot) = self.run.potion_at(slot).cloned() else { return false };
        if !effects::potion_usable(self, &pot) {
            return false;
        }
        let foe = self.foe_named(target).or_else(|| self.alive().first().copied());
        self.run.drop_potion(slot);
        self.run.log_add("potions", 1);
        effects::drink(self, &pot, foe);
        if self.run.has("TOY_ORNITHOPTER") {
            self.heal(5);
        }
        true
    }

    // ------------------------------------------------------------------ the fight

    pub fn start(&mut self) {
        effects::fight_start(self);
        let mut i = 0;
        while i < self.foes.len() {
            monsters::choose(self, i);
            i += 1;
        }
    }

    /// `game_hook(what, card=None)`.
    pub fn game_hook(&mut self, what: &str, card: Option<&Card>) {
        if let Some(hook) = self.game.ON {
            hook(self, what, card);
        }
    }

    pub fn player_turn_start(&mut self) {
        self.round += 1;
        for f in self.foes.iter_mut() {
            f.mem.ints.remove("shell");
            f.mem.ints.remove("skittish");
        }
        self.turn_cards = Vec::new();
        self.attacks_turn = 0;
        self.skills_turn = 0;
        self.pw.shift_remove("DISCARDED_TURN");
        self.no_draw = false;
        if self.round > 1 && !self.pw.on("BARRICADE") && self.pw.shift_remove("KEEP_BLOCK").unwrap_or(0) == 0 {
            self.block = if self.run.has("CALIPERS") { (self.block - 15).max(0) } else { 0 };
        }
        if self.stance == "DIVINITY" {
            self.enter("");
        }
        self.energy = (self.run.energy - self.pw.val("ENERGY_DOWN")).max(0) + self.pw.shift_remove("ENERGY_NEXT_TURN").unwrap_or(0);
        effects::turn_start(self);
        let mut draw = 5 + self.pw.shift_remove("DRAW_CARDS_NEXT_TURN").unwrap_or(0);
        draw += effects::extra_draw(self);
        draw -= self.pw.shift_remove("DRAW_REDUCTION").unwrap_or(0);
        let held = self.hand.len();
        self.draw_cards(draw.max(0));
        self.pw.shift_remove("BOUND_PLAYED");
        let chains = self.pw.val("CHAINS_OF_BINDING").max(0) as usize;
        self.bound = self.hand.iter().skip(held).take(chains).map(|c| c.uid).collect();
        effects::after_draw(self);
    }

    pub fn player_turn_end(&mut self) {
        effects::turn_end(self);
        let keep_all = self.pw.shift_remove("KEEP_HAND").unwrap_or(0) != 0 || self.run.has("RUNIC_PYRAMID");
        let mut planned: Vec<&Card> = self.hand.iter().filter(|c| !matches!(c.r#type(), "Status" | "Curse") && !c.has("Retain")).collect();
        base::sort_by_key(&mut planned, |c| -c.cost());
        let kept: HashSet<u64> = planned.iter().take(self.pw.val("WELL_LAID_PLANS").max(0) as usize).map(|c| c.uid).collect();
        let hexed = self.foes.iter().any(|x| x.alive() && x.powers.on("HEX")); // (the Spectral Knight: all our cards are Ethereal)
        let hand: Vec<u64> = self.hand.iter().map(|c| c.uid).collect();
        for uid in hand {
            let Some(c) = self.hand.iter_mut().find(|c| c.uid == uid) else { continue };
            let ethereal = hexed || c.has("Ethereal");
            if c.has("Retain") || kept.contains(&uid) || keep_all && !matches!(c.r#type(), "Status" | "Curse") && !ethereal {
                c.retained += 1;
                effects::retained(self, uid);
                continue;
            }
            let c = self.take_from(Pile::Hand, uid).expect("the card is in the hand");
            if ethereal {
                self.exhaust(c);
            } else {
                self.discard.push(c);
            }
        }
        for k in ["STRENGTH", "DEXTERITY", "FOCUS", "FLAME_BARRIER"] {
            self.temp.shift_remove(k);
        }
        for k in ["RAGE", "ENTANGLED"] {
            self.pw.shift_remove(k);
        }
        self.free_attack = false;
        for c in self.hand.iter_mut().chain(&mut self.draw).chain(&mut self.discard) {
            c.cost_turn = None;
        }
    }

    pub fn enemies_turn(&mut self) {
        for i in 0..self.foes.len() {
            if self.won || self.lost {
                return;
            }
            if !self.foes[i].alive() {
                continue;
            }
            let f = &mut self.foes[i];
            if !(f.powers.on("BARRICADE") || f.powers.on("BURROWED")) {
                f.block = 0;
            }
            if f.powers.on("POISON") {
                let dose = f.powers["POISON"];
                f.powers["POISON"] = dose - 1;
                let n = if f.powers.on("INTANGIBLE") { 1 } else { dose };
                self.hurt(i, n, false);
                if !self.foes[i].alive() {
                    continue;
                }
            }
            let f = &mut self.foes[i];
            let m = f.r#move();
            f.history.push(f.move_name.clone());
            if m.kind == "Escape" || m.fx.iter().any(|fx| fx.word == "escape") {
                f.gone = true;
                if !self.foes.iter().any(|x| x.alive() && !x.minion) {
                    self.escaped = true;
                    self.won = true;
                }
                continue;
            }
            effects::foe_acts(self, i, &m);
            let f = &self.foes[i];
            if f.powers.on("DOOM") && f.alive() && f.powers["DOOM"] >= f.hp {
                self.kill(i);
            }
        }
        self.end_of_round();
    }

    pub fn end_of_round(&mut self) {
        for f in self.foes.iter_mut() {
            // Strength lost "this turn" (Piercing Wail, Shifting) is back once the enemies have acted
            let back = f.mem.pop("str_back", 0);
            *f.powers.entry("STRENGTH".to_string()).or_insert(0) += back;
        }
        for k in ["WEAK", "VULNERABLE", "FRAIL", "INTANGIBLE"] {
            if self.pw.on(k) && !self.fresh.contains(k) {
                self.pw[k] -= 1;
            }
        }
        if self.pw.on("REGEN") {
            let n = self.pw["REGEN"];
            self.heal(n);
            self.pw["REGEN"] -= 1;
        }
        let mut i = 0;
        while i < self.foes.len() {
            if self.foes[i].alive() {
                let f = &mut self.foes[i];
                for k in ["WEAK", "VULNERABLE", "INTANGIBLE"] {
                    if f.powers.on(k) {
                        f.powers[k] -= 1;
                    }
                }
                effects::foe_round_end(self, i);
            }
            i += 1;
        }
        (self.game.ROUND_END)(self);
        self.fresh.clear();
        let mut i = 0;
        while i < self.foes.len() {
            if self.foes[i].alive() {
                monsters::choose(self, i);
            }
            i += 1;
        }
    }

    /// Each foe's damage a turn so far and its blows, as the bot keeps them (`_threat`, `_blow`; a blow as it is outside Wrath).
    pub fn note_threat(&mut self) {
        for i in 0..self.foes.len() {
            if self.foes[i].alive() {
                let m = self.foes[i].r#move();
                let hit = if m.dmg != 0 && m.kind.is_empty() { self.foe_hit(i, effects::move_damage(self, i, &m)) * effects::move_hits(self, i, &m) } else { 0 };
                let wrath = self.stance == "WRATH";
                let seen = self.threat.entry(self.foes[i].eid.clone()).or_insert((0, 0, 0.0, 0));
                seen.0 += hit;
                seen.1 += 1;
                if hit != 0 {
                    seen.2 += hit as f64 / if wrath { 2.0 } else { 1.0 };
                    seen.3 += 1;
                }
            }
        }
    }

    /// Ask the planner for one action at a time until it ends the turn.
    pub fn bot_turn(&mut self) {
        for _ in 0..MAX_ACTIONS {
            if self.won || self.lost || self.pw.shift_remove("TURN_OVER").unwrap_or(0) != 0 {
                return;
            }
            let mut acted = false;
            for (action, params) in combat::actions(&self.state()) {
                if action == "end_turn" {
                    self.digest.push("E".to_string());
                    return;
                }
                if action == "play_card" && self.play(params["card_index"].int_or(-1), params["target"].as_str()) {
                    self.digest.push(format!("P{}{}", base::str_of(&params["card_index"]), params["target"].s()));
                    self.plays += 1;
                    acted = true;
                    break;
                }
                if action == "use_potion" && self.use_potion(params["slot"].int_or(-1), params["target"].as_str()) {
                    self.digest.push(format!("Q{}", base::str_of(&params["slot"])));
                    acted = true;
                    break;
                }
            }
            if !acted {
                return;
            }
        }
    }

    /// Play the fight out. True if it was won (or escaped from).
    pub fn run_fight(&mut self) -> bool {
        self.start();
        'fight: while !(self.won || self.lost) {
            if self.round >= MAX_ROUNDS {
                self.lost = true;
                break;
            }
            let steps: [fn(&mut Fight); 4] = [Fight::player_turn_start, Fight::note_threat, Fight::bot_turn, Fight::player_turn_end];
            for step in steps {
                step(self);
                if self.won || self.lost {
                    continue 'fight;
                }
            }
            if self.pw.shift_remove("EXTRA_TURN").unwrap_or(0) == 0 {
                self.enemies_turn();
            }
        }
        self.won && !self.lost
    }

    /// Cards from the hand for a card's "choose a card" (the bot's hand_select handler orders them): clones of
    /// the cards picked, out of `cards` (default: the whole hand).
    pub fn hand_pick(&mut self, prompt: &str, n: i64, cards: Option<&[Card]>) -> Vec<Card> {
        let pool: Vec<Card> = match cards {
            Some(cards) => cards.to_vec(),
            None => self.hand.clone(),
        };
        if pool.is_empty() || n <= 0 {
            return vec![];
        }
        let mut state = self.state();
        state["state_type"] = json!("hand_select");
        state["_fight"] = json!(self.kind);
        state["hand_select"] = json!({"prompt": prompt, "cards": pool.iter().enumerate().map(|(i, c)| c.view(ViewArgs { index: Some(i as i64), ..Default::default() })).collect::<Vec<_>>(),
                                      "can_confirm": false, "mode": "simple_select"});
        let mut order: Vec<i64> = Vec::new();
        for (a, p) in strategy::hand_select(&state, &mut self.run.mem) {
            if a == "combat_select_card" {
                let i = p["card_index"].as_i64().expect("a card index");
                if 0 <= i && i < pool.len() as i64 && !order.contains(&i) {
                    order.push(i);
                }
            }
        }
        order.into_iter().take(n as usize).map(|i| pool[i as usize].clone()).collect()
    }

    /// Cards from a pile for a "choose" screen (Headbutt, Exhume, Seek): the bot's card_select handler orders them.
    /// Clones of the cards picked (`pile_pick(prompt, pool, n=1)`).
    pub fn pile_pick(&mut self, prompt: &str, pool: &[Card], n: i64) -> Vec<Card> {
        if pool.is_empty() {
            return vec![];
        }
        let state = json!({"state_type": "card_select", "run": self.run.r#where(), "player": {"hp": self.run.hp, "max_hp": self.run.max_hp},
                           "card_select": {"screen_type": "choose", "prompt": prompt,
                                           "cards": pool.iter().enumerate().map(|(i, c)| c.view(ViewArgs { index: Some(i as i64), ..Default::default() })).collect::<Vec<_>>()}});
        let mem = &mut self.run.mem;
        mem.select_key = String::new();
        let order: Vec<i64> = strategy::card_select(&state, mem).into_iter().filter(|(a, _)| a == "select_card").map(|(_, p)| p["index"].as_i64().expect("an index")).collect();
        order.into_iter().take(n.max(0) as usize).filter(|i| 0 <= *i && *i < pool.len() as i64).map(|i| pool[i as usize].clone()).collect()
    }
}
