//! What cards, powers, potions and enemy moves do, for the engine (engine::Fight).
//!
//! A card does what its text says as the planner reads it (crate::cards::parse): damage, Block, debuffs, draw,
//! energy, Strength, Poison, orbs, stances. Cards whose text says more than that, or says it under a condition,
//! have a handler in the game's content: `PRE[key](fight, card, foe, info) -> info` adjusts what is read
//! before it is done, `AFTER[key](fight, card, foe, x, dealt)` adds the rest, `DAMAGE[key](fight, card) -> base`
//! works out damage the text does not print. Powers are kept in `fight.pw` under their ids and act through the
//! hooks below.

use std::rc::Rc;

use indexmap::IndexMap;

use crate::cards::CardInfo;
use crate::base;
use crate::sim::cards::Card;
use crate::sim::engine::{Fight, Fx, Move, Orb, Pile};
use crate::sim::monsters;
use crate::sim::run::Potion;

pub const DEBUFF_FX: &[&str] = &["weak", "vuln", "frail", "status", "str_down", "dex_down", "focus_down", "draw_down", "entangle", "hex", "constrict", "confuse", "steal_gold"];
pub const BUFF_FX: &[&str] = &["str", "str_all", "ritual", "block_ally", "block_all", "heal", "heal_all", "summon", "metallicize", "plated", "artifact", "thorns", "intangible", "enrage"];
pub const BACK_TO_DRAW: &[&str] = &["TANTRUM"];

/// d[key] += n (from 0); the new value.
pub fn add(d: &mut IndexMap<String, i64>, key: &str, n: i64) -> i64 {
    match d.get_mut(key) {
        Some(v) => {
            *v += n;
            *v
        }
        None => {
            d.insert(key.to_string(), n);
            n
        }
    }
}

/// How the original reads a dict of powers or counters.
pub trait Powers {
    /// `d.get(key, 0)`.
    fn val(&self, key: &str) -> i64;
    /// `bool(d.get(key))`.
    fn on(&self, key: &str) -> bool;
    /// `d[key] = value`.
    fn put(&mut self, key: &str, value: i64);
}

impl Powers for IndexMap<String, i64> {
    fn val(&self, key: &str) -> i64 {
        self.get(key).copied().unwrap_or(0)
    }

    fn on(&self, key: &str) -> bool {
        self.get(key).is_some_and(|v| *v != 0)
    }

    fn put(&mut self, key: &str, value: i64) {
        match self.get_mut(key) {
            Some(v) => *v = value,
            None => {
                self.insert(key.to_string(), value);
            }
        }
    }
}

/// The monster's own rule for an event ("start", "hit", "death", "end" ...), if it has one; the truth of what it returns.
pub fn foe_hook(f: &mut Fight, foe: usize, event: &str, amount: i64, attack: bool) -> bool {
    match f.foes[foe].spec.hook(event) {
        Some(rule) => rule(f, foe, amount, attack),
        None => false,
    }
}

/// A Power whose effect Slay the Spire 2's text triggers carry out (so the first game's handler for its name stays out).
pub fn by_text(f: &Fight, key: &str) -> bool {
    f.triggers.iter().any(|(_w, _t, k)| k == key)
}

/// A card's damage a hit before Strength, or None if it deals none.
pub fn base_damage(f: &Fight, c: &Card) -> Option<i64> {
    if let Some(rule) = f.game.DAMAGE.get(c.key()) {
        return Some(rule(f, c));
    }
    let damage = c.info().damage;
    if damage != 0 {
        Some(damage + c.bonus + if c.key() == "SHIV" { f.pw.val("ACCURACY") } else { 0 })
    } else {
        None
    }
}

pub fn can_play(f: &Fight, c: &Card) -> bool {
    match f.game.PLAYABLE.get(c.key()) {
        Some(rule) => rule(f, c),
        None => true,
    }
}

pub fn move_damage(f: &Fight, foe: usize, m: &Move) -> i64 {
    let foe = &f.foes[foe];
    foe.mem.dmg.get(&foe.move_name).copied().unwrap_or(m.dmg)
}

pub fn move_hits(f: &Fight, foe: usize, m: &Move) -> i64 {
    let foe = &f.foes[foe];
    foe.mem.hits.get(&foe.move_name).copied().unwrap_or(m.hits)
}

// ---------------------------------------------------------------- playing a card

/// Everything one play of `c` does (`x`: energy spent on an X card). The card is in no pile while this runs.
pub fn resolve(f: &mut Fight, c: &mut Card, foe: Option<usize>, x: i64) {
    let g = f.game.clone();
    let mut i: Rc<CardInfo> = c.info();
    if let Some(pre) = g.PRE.get(c.key()) {
        match pre(f, c, foe, i) {
            Some(read) => i = read,
            None => return,
        }
    }
    let is_x = c.printed_cost() == -1;
    let xs = if is_x { (x + if c.up && g.X_PLUS.contains(c.key()) { 1 } else { 0 }).max(0) } else { 1 }; // (1 for a card that is not X)
    let mut dealt = 0;
    let base = base_damage(f, c);
    if let Some(base) = base {
        if i.damage != 0 || g.DAMAGE.contains_key(c.key()) {
            let hits = i.hits * if is_x && crate::re!(r"\bX times\b").is_match(c.text()) { xs } else { 1 };
            dealt = attack(f, c, foe, base, hits, &i);
            if f.won || f.lost {
                after(f, c, foe, xs, dealt);
                return;
            }
        }
    }
    let victims: Vec<usize> = if i.aoe {
        f.alive()
    } else {
        match foe {
            Some(v) if f.foes[v].alive() => vec![v],
            _ => vec![],
        }
    };
    for &v in &victims {
        if i.vulnerable != 0 {
            debuff(f, Some(v), "VULNERABLE", i.vulnerable * xs);
        }
        if i.weak != 0 {
            debuff(f, Some(v), "WEAK", i.weak * xs);
        }
        let foe = &mut f.foes[v];
        if i.enemy_strength_loss != 0 && !foe.powers.on("ARTIFACT") {
            add(&mut foe.powers, "STRENGTH", -i.enemy_strength_loss * xs);
            if !i.enemy_strength_lasts {
                foe.mem.add("str_back", i.enemy_strength_loss * xs);
            }
        } else if i.enemy_strength_loss != 0 {
            foe.powers["ARTIFACT"] -= 1;
        }
    }
    if i.poison != 0 && !i.random_target {
        let targets: Vec<usize> = if victims.is_empty() { foe.into_iter().collect() } else { victims.clone() };
        for v in targets {
            if f.foes[v].alive() && (!i.poison_needs || f.foes[v].powers.on("POISON")) {
                debuff(f, Some(v), "POISON", i.poison * xs);
            }
        }
    } else if i.poison != 0 {
        for _ in 0..xs.max(1) {
            let alive = f.alive();
            if !alive.is_empty() {
                let v = *f.run.rng.choice(&alive);
                debuff(f, Some(v), "POISON", i.poison);
            }
        }
    }
    if i.block != 0 && i.melody_block == 0 {
        for _ in 0..xs {
            let b = f.block_value(i.block + c.bonus);
            f.gain_block(b);
        }
    }
    if i.hp_loss != 0 {
        f.lose_hp(i.hp_loss, true);
        if f.lost {
            return;
        }
    }
    if i.heal != 0 {
        f.heal(i.heal);
    }
    let power = c.r#type() == "Power";
    if i.strength != 0 {
        add(if i.strength_lasts || power { &mut f.pw } else { &mut f.temp }, "STRENGTH", i.strength * xs);
    }
    if i.dexterity != 0 {
        add(if i.dexterity_lasts || power { &mut f.pw } else { &mut f.temp }, "DEXTERITY", i.dexterity);
    }
    if i.energy_drain != 0 {
        add(&mut f.pw, "ENERGY_DOWN", i.energy_drain);
    }
    if i.energy_gain != 0 {
        f.energy += i.energy_gain * xs;
    }
    if i.shivs != 0 {
        let up = c.key() == "CUNNING" || f.pw.val("MASTER_REALITY") > 0;
        f.add_new("SHIV", i.shivs * xs, "hand", up);
    }
    for _ in 0..xs {
        for kind in &i.channel {
            f.channel(kind);
        }
    }
    if i.evoke != 0 {
        f.evoke(i.evoke * xs, false);
    }
    if i.evoke_all != 0 {
        while !f.orbs.is_empty() {
            f.evoke(i.evoke_all, false);
        }
    }
    if !i.stance.is_empty() {
        f.enter(&i.stance);
    }
    if i.next_energy != 0 {
        add(&mut f.pw, "ENERGY_NEXT_TURN", i.next_energy * xs);
    }
    if i.next_draw != 0 {
        add(&mut f.pw, "DRAW_CARDS_NEXT_TURN", i.next_draw * xs);
    }
    if i.next_block != 0 {
        add(&mut f.pw, "NEXT_BLOCK", i.next_block);
    }
    if i.enemy_hp_loss != 0 {
        for &v in &victims {
            f.hurt(v, i.enemy_hp_loss, false);
        }
    }
    if i.draw != 0 {
        f.draw_cards(i.draw * xs);
    }
    (g.EXTRA)(f, c, foe, &i, xs); // the game's own resources (Stars, Osty, Doom, Souls, Forge, Mantra)
    after(f, c, foe, xs, dealt);
}

pub fn after(f: &mut Fight, c: &mut Card, foe: Option<usize>, xs: i64, dealt: i64) {
    let g = f.game.clone();
    if let Some(rule) = g.AFTER.get(c.key()) {
        rule(f, c, foe, xs, dealt);
    }
    if c.r#type() == "Power" {
        if let Some(rule) = g.POWER.get(c.key()) {
            rule(f, c);
        }
    }
}

/// The hits of an attack; the HP they took off.
pub fn attack(f: &mut Fight, c: &Card, foe: Option<usize>, base: i64, hits: i64, i: &CardInfo) -> i64 {
    let mut dealt = 0;
    let (osty, atk) = (i.osty_attack, c.r#type() == "Attack");
    if osty && f.osty == 0 {
        return 0;
    }
    let mut double = if f.pw.on("DOUBLE_DAMAGE") && atk { 2 } else { 1 };
    if atk && f.run.counters.val("PEN_NIB") >= 9 && f.run.has("PEN_NIB") {
        double *= 2;
    }
    for _ in 0..hits.max(0) {
        let alive = f.alive();
        let targets: Vec<usize> = if i.aoe {
            alive
        } else if i.random_target {
            if alive.is_empty() {
                vec![]
            } else {
                vec![*f.run.rng.choice(&alive)]
            }
        } else {
            match foe {
                Some(t) if f.foes[t].alive() => vec![t],
                _ => vec![],
            }
        };
        for t in targets {
            let before = f.foes[t].hp;
            let amount = f.attack_value(base, Some(t), osty, !atk) * double;
            dealt += f.hurt(t, amount, atk);
            if atk && f.foes[t].powers.on("THORNS") && !osty {
                let n = f.foes[t].powers["THORNS"];
                f.take(n, Some(t), false);
            }
            if atk && f.foes[t].powers.on("PERSONAL_HIVE") {
                let n = f.foes[t].powers["PERSONAL_HIVE"];
                f.add_new("DAZED", n, "draw", false);
            }
            if atk && f.pw.on("REAPER_FORM") && f.foes[t].alive() && before > f.foes[t].hp {
                let n = before - f.foes[t].hp;
                debuff(f, Some(t), "DOOM", n);
            }
            if atk && f.pw.on("ENVENOM") && f.foes[t].hp < before && f.foes[t].alive() {
                let n = f.pw["ENVENOM"];
                debuff(f, Some(t), "POISON", n);
            }
            if atk && f.foes[t].mem.truthy("block_return") {
                let n = f.foes[t].mem.get("block_return", 0);
                f.gain_block(n);
            }
        }
        if f.won || f.lost {
            break;
        }
    }
    if atk && f.pw.on("VIGOR") {
        f.pw.shift_remove("VIGOR");
    }
    dealt
}

pub fn debuff(f: &mut Fight, foe: Option<usize>, power: &str, n: i64) {
    let Some(i) = foe else { return };
    if n <= 0 {
        return;
    }
    let foe = &mut f.foes[i];
    if foe.powers.on("ARTIFACT") {
        foe.powers["ARTIFACT"] -= 1;
        return;
    }
    add(&mut foe.powers, power, n);
    if f.pw.on("SADISTIC_NATURE") {
        let n = f.pw["SADISTIC_NATURE"];
        f.hurt(i, n, false);
    }
}

pub fn debuff_all(f: &mut Fight, power: &str, n: i64) {
    for foe in f.alive() {
        debuff(f, Some(foe), power, n);
    }
}

pub fn debuff_me(f: &mut Fight, power: &str, n: i64) {
    if f.pw.on("ARTIFACT") {
        f.pw["ARTIFACT"] -= 1;
        return;
    }
    if power == "WEAK" && f.run.has("GINGER") || power == "FRAIL" && f.run.has("TURNIP") {
        return;
    }
    add(&mut f.pw, power, n);
}

/// What happens after any card is played: powers and relics that count cards, and the enemies that react.
pub fn played(f: &mut Fight, c: &Card) {
    let ctype = c.r#type();
    if f.pw.on("AFTER_IMAGE") {
        f.gain_block(f.pw["AFTER_IMAGE"]);
    }
    if f.pw.on("A_THOUSAND_CUTS") {
        f.hurt_all(f.pw["A_THOUSAND_CUTS"]);
    }
    for foe in f.alive() {
        if f.foes[foe].mem.truthy("choke") {
            let n = f.foes[foe].mem.get("choke", 0);
            f.hurt(foe, n, false);
        }
    }
    if f.pw.on("PANACHE") && f.turn_cards.len() % 5 == 0 {
        f.hurt_all(f.pw["PANACHE"]);
    }
    if ctype == "Attack" {
        if f.pw.on("RAGE") {
            f.gain_block(f.pw["RAGE"]);
        }
        if f.attacks_turn % 3 == 0 {
            // (Shuriken, Kunai, Ornamental Fan: every third Attack in one turn)
            if f.run.has("SHURIKEN") {
                add(&mut f.pw, "STRENGTH", 1);
            }
            if f.run.has("KUNAI") {
                add(&mut f.pw, "DEXTERITY", 1);
            }
            if f.run.has("ORNAMENTAL_FAN") {
                f.gain_block(4);
            }
        }
        if f.run.has("PEN_NIB") {
            let n = (f.run.counters.val("PEN_NIB") + 1) % 10;
            f.run.counters.put("PEN_NIB", n);
        }
    }
    if ctype == "Skill" {
        if f.run.has("LETTER_OPENER") && f.skills_turn % 3 == 0 {
            f.hurt_all(5);
        }
        for foe in f.alive() {
            let foe = &mut f.foes[foe];
            if foe.powers.on("ENRAGE") {
                let n = foe.powers["ENRAGE"];
                add(&mut foe.powers, "STRENGTH", n);
            }
        }
    }
    if f.pw.on("MONOLOGUE") {
        add(&mut f.temp, "STRENGTH", 1);
    }
    if f.pw.on("TENDER") {
        add(&mut f.temp, "STRENGTH", -1);
        add(&mut f.temp, "DEXTERITY", -1);
    }
    for foe in f.alive() {
        if f.foes[foe].mem.truthy("oblivion") {
            let n = f.foes[foe].mem.get("oblivion", 0);
            debuff(f, Some(foe), "DOOM", n);
        }
        let foe = &mut f.foes[foe];
        if ctype == "Skill" && foe.powers.on("VITAL_SPARK") {
            let n = foe.powers["VITAL_SPARK"];
            foe.mem.add("spark", n);
        }
    }
    f.game_hook("play", Some(c));
    if ctype != "Attack" && f.pw.on("HEX_DAZED") {
        f.add_new("DAZED", f.pw["HEX_DAZED"], "draw", false);
    }
    if ctype == "Power" {
        for foe in f.alive() {
            let foe = &mut f.foes[foe];
            if foe.powers.on("CURIOSITY") {
                let n = foe.powers["CURIOSITY"];
                add(&mut foe.powers, "STRENGTH", n);
            }
        }
        if f.pw.on("STORM") && !by_text(f, "STORM") {
            for _ in 0..f.pw["STORM"] {
                f.channel("LIGHTNING");
            }
        }
        if f.pw.on("HEATSINKS") && !by_text(f, "HEATSINKS") {
            f.draw_cards(f.pw["HEATSINKS"]);
        }
    }
    if f.run.has("NUNCHAKU") && ctype == "Attack" && add(&mut f.run.counters, "NUNCHAKU", 1) % 10 == 0 {
        f.energy += 1;
    }
    if f.run.has("INK_BOTTLE") && add(&mut f.run.counters, "INK_BOTTLE", 1) % 10 == 0 {
        f.draw_cards(1);
    }
    for foe in f.alive() {
        let x = &mut f.foes[foe];
        if x.powers.contains_key("TIME_WARP") && x.alive() && add(&mut x.powers, "TIME_WARP", 1) >= 12 {
            x.powers["TIME_WARP"] = 0;
            for other in f.alive() {
                add(&mut f.foes[other].powers, "STRENGTH", 2);
            }
            f.pw.put("TURN_OVER", 1);
        }
        if f.foes[foe].powers.on("SHARP_HIDE") && ctype == "Attack" {
            let n = f.foes[foe].powers["SHARP_HIDE"];
            f.take(n, Some(foe), false);
        }
    }
    let pain = f.hand.iter().filter(|h| h.key() == "PAIN").count() as i64;
    if pain != 0 {
        f.lose_hp(pain, false);
    }
}

/// A card drawn: it is in the hand (`uid`).
pub fn drawn(f: &mut Fight, uid: u64) {
    let Some(c) = f.card(uid).cloned() else { return };
    if f.pw.on("CORROSIVE_WAVE") {
        debuff_all(f, "POISON", f.pw["CORROSIVE_WAVE"]);
    }
    if c.key() == "VOID" {
        f.energy = (f.energy - 1).max(0);
    }
    if c.r#type() == "Status" && f.pw.on("EVOLVE") {
        f.draw_cards(f.pw["EVOLVE"]);
    }
    if matches!(c.r#type(), "Status" | "Curse") && f.pw.on("FIRE_BREATHING") {
        f.hurt_all(f.pw["FIRE_BREATHING"]);
    }
    if f.pw.on("CONFUSED") && f.card(uid).map_or(c.cost(), |c| c.cost()) >= 0 {
        let cost = f.run.rng.randint(0, 3);
        if let Some(card) = f.card_mut(uid) {
            card.cost_fight = Some(cost);
        }
    }
    let g = f.game.clone();
    if let Some(rule) = g.DRAWN.get(c.key()) {
        rule(f, uid);
    }
}

pub fn exhausted(f: &mut Fight, c: &Card) {
    f.game_hook("exhaust", Some(c));
    if f.pw.on("FEEL_NO_PAIN") && f.game.KEY == "sts1" {
        f.gain_block(f.pw["FEEL_NO_PAIN"]);
    }
    if f.pw.on("DARK_EMBRACE") && f.game.KEY == "sts1" {
        f.draw_cards(f.pw["DARK_EMBRACE"]);
    }
    if c.key() == "SENTINEL" {
        f.energy += if c.up { 3 } else { 2 };
    }
    if f.run.has("CHARON_S_ASHES") {
        f.hurt_all(3);
    }
    if f.run.has("DEAD_BRANCH") {
        let pool = (f.game.pool)(&f.run.character);
        if !pool.is_empty() {
            let d = f.run.rng.choice(&pool).clone();
            f.add_card(Card::new(&d, false), "hand");
        }
    }
}

/// A card discarded from the hand by a card's own say (not at the end of the turn). `c`: the card as it was
/// put on the discard pile (a clone: the pile's own is found by its `uid`).
pub fn discarded(f: &mut Fight, c: &Card) {
    add(&mut f.pw, "DISCARDED_TURN", 1);
    if c.has("Sly") && f.in_pile(Pile::Discard, c.uid) {
        // (Slay the Spire 2: a Sly card discarded from the hand is played for free)
        let mut card = f.take_from(Pile::Discard, c.uid).expect("the card is in the discard pile");
        let alive = f.alive();
        let at = if alive.is_empty() { None } else { Some(*f.run.rng.choice(&alive)) };
        let x = if card.printed_cost() == -1 { f.energy } else { 0 };
        resolve(f, &mut card, at, x);
        played(f, &card);
        f.put_away(card);
    }
    if c.key() == "REFLEX" {
        f.draw_cards(if c.up { 3 } else { 2 });
    } else if c.key() == "TACTICIAN" {
        f.energy += if c.up { 2 } else { 1 };
    }
    for card in f.hand.iter_mut().chain(&mut f.draw).chain(&mut f.discard) {
        if card.key() == "EVISCERATE" && card.cost() > 0 {
            card.cost_turn = Some((card.cost() - 1).max(0));
        }
    }
}

/// `discard_from_hand(f, n, prompt="Choose {n} card(s) to Discard.")`.
pub fn discard_from_hand(f: &mut Fight, n: i64) -> i64 {
    let picked = f.hand_pick(&format!("Choose {n} card(s) to Discard."), n, None);
    for c in &picked {
        if let Some(card) = f.take_from(Pile::Hand, c.uid) {
            let seen = card.clone();
            f.discard.push(card);
            discarded(f, &seen);
        }
    }
    picked.len() as i64
}

pub fn block_gained(f: &mut Fight, _n: i64) {
    f.game_hook("block", None);
    if f.pw.on("JUGGERNAUT") && !f.alive().is_empty() && f.game.KEY == "sts1" {
        let alive = f.alive();
        let at = *f.run.rng.choice(&alive);
        f.hurt(at, f.pw["JUGGERNAUT"], false);
    }
    if f.pw.on("WAVE_OF_THE_HAND") {
        debuff_all(f, "WEAK", f.pw["WAVE_OF_THE_HAND"]);
    }
}

pub fn stance_changed(f: &mut Fight, new: &str) {
    if f.pw.on("MENTAL_FORTRESS") {
        f.gain_block(f.pw["MENTAL_FORTRESS"]);
    }
    if new == "WRATH" && f.pw.on("RUSHDOWN") {
        f.draw_cards(f.pw["RUSHDOWN"]);
    }
}

/// A card kept in the hand as the turn ends (`uid`).
pub fn retained(f: &mut Fight, uid: u64) {
    let g = f.game.clone();
    let Some(key) = f.card(uid).map(|c| c.key().to_string()) else { return };
    if let Some(rule) = g.RETAINED.get(&key) {
        rule(f, uid);
    }
    let n = f.pw.val("ESTABLISHMENT");
    if n != 0 {
        if let Some(c) = f.card_mut(uid) {
            if c.cost() > 0 {
                c.cost_fight = Some((c.cost() - n).max(0));
            }
        }
    }
}

/// Look at the top n cards: the dead ones (statuses, curses, Strikes and Defends when the deck is fine) go.
pub fn scry(f: &mut Fight, n: i64) {
    if n <= 0 {
        return;
    }
    f.refill();
    let from = f.draw.len().saturating_sub(n as usize);
    let top: Vec<(u64, bool)> = f.draw[from..].iter().map(|c| (c.uid, matches!(c.r#type(), "Status" | "Curse"))).collect();
    for (uid, dead) in top {
        if dead {
            if let Some(c) = f.take_from(Pile::Draw, uid) {
                f.discard.push(c);
            }
        }
    }
    if f.pw.on("NIRVANA") {
        f.gain_block(f.pw["NIRVANA"]);
    }
    let weaves: Vec<u64> = f.discard.iter().filter(|c| c.key() == "WEAVE").map(|c| c.uid).collect();
    for uid in weaves {
        if let Some(c) = f.take_from(Pile::Discard, uid) {
            f.add_card(c, "hand");
        }
    }
}

pub fn on_death(f: &mut Fight) {
    let run = &mut f.run;
    let fairy = run.potions.iter().position(|p| p.as_ref().is_some_and(|p| p.key == "FAIRY_IN_A_BOTTLE"));
    if let Some(fairy) = fairy {
        run.drop_potion(fairy as i64);
        run.hp = base::trunc(run.max_hp as f64 * 0.3).max(1);
        return;
    }
    if run.has("LIZARD_TAIL") && !run.counters.on("LIZARD_TAIL_USED") {
        run.counters.put("LIZARD_TAIL_USED", 1);
        run.hp = (run.max_hp / 2).max(1);
        return;
    }
    run.hp = 0;
    f.lost = true;
}

pub fn foe_hurt(f: &mut Fight, i: usize, amount: i64, attack: bool) {
    let foe = &mut f.foes[i];
    if attack && foe.powers.on("ANGRY") {
        let n = foe.powers["ANGRY"];
        add(&mut foe.powers, "STRENGTH", n);
    }
    if attack && foe.powers.on("MALLEABLE") {
        foe.block += foe.powers["MALLEABLE"];
        foe.powers["MALLEABLE"] += 1;
    }
    if attack && foe.powers.on("FLUTTER") {
        foe.powers["FLUTTER"] -= 1;
        if foe.powers["FLUTTER"] <= 0 {
            foe.powers.shift_remove("FLUTTER");
            foe_hook(f, i, "grounded", 0, false);
        }
    }
    let foe = &mut f.foes[i];
    if foe.powers.on("SHIFTING") {
        add(&mut foe.powers, "STRENGTH", -amount);
        foe.mem.add("str_back", amount);
    }
    if foe.powers.on("ASLEEP") {
        foe.powers.shift_remove("ASLEEP");
        foe_hook(f, i, "wake", 0, false);
    }
    foe_hook(f, i, "hit", amount, attack);
}

// ---------------------------------------------------------------- turns

pub fn fight_start(f: &mut Fight) {
    (f.game.START)(f);
    for rid in f.run.relics.clone() {
        RELIC_START(f, &rid);
    }
    let mut i = 0;
    while i < f.foes.len() {
        foe_hook(f, i, "start", 0, false);
        i += 1;
    }
}

/// `RELIC_START[rid](f)`, for the relics that have a rule.
#[allow(non_snake_case)]
pub fn RELIC_START(f: &mut Fight, rid: &str) {
    match rid {
        "ANCHOR" => f.gain_block(10),
        "BLOOD_VIAL" => f.heal(2),
        "MUTAGENIC_STRENGTH" => {
            add(&mut f.temp, "STRENGTH", 3);
        }
        "BAG_OF_MARBLES" => debuff_all(f, "VULNERABLE", 1),
        "RED_MASK" => debuff_all(f, "WEAK", 1),
        "TWISTED_FUNNEL" => debuff_all(f, "POISON", 4),
        "PHILOSOPHER_S_STONE" => {
            for x in f.alive() {
                add(&mut f.foes[x].powers, "STRENGTH", 1);
            }
        }
        "MARK_OF_PAIN" => f.add_new("WOUND", 2, "draw", false),
        "PRESERVED_INSECT" => {
            if f.kind == "elite" {
                for x in f.alive() {
                    f.foes[x].hp = base::trunc(f.foes[x].hp as f64 * 0.75);
                }
            }
        }
        "NUCLEAR_BATTERY" => f.channel("PLASMA"),
        "SYMBIOTIC_VIRUS" => f.channel("DARK"),
        "GIRYA" => {
            let n = f.run.counters.val("GIRYA");
            add(&mut f.pw, "STRENGTH", n);
        }
        _ => {
            let (k, n) = match rid {
                "VAJRA" => ("STRENGTH", 1),
                "ODDLY_SMOOTH_STONE" => ("DEXTERITY", 1),
                "BRONZE_SCALES" => ("THORNS", 3),
                "AKABEKO" => ("VIGOR", 8),
                "THREAD_AND_NEEDLE" => ("PLATING", 4),
                "CLOCKWORK_SOUVENIR" => ("ARTIFACT", 1),
                "FOSSILIZED_HELIX" => ("BUFFER", 1),
                "DATA_DISK" => ("FOCUS", 1),
                _ => return,
            };
            add(&mut f.pw, k, n);
        }
    }
}

pub fn extra_draw(f: &Fight) -> i64 {
    let (run, mut n) = (&f.run, 0);
    if f.round == 1 {
        n += 2 * run.has("RING_OF_THE_SNAKE") as i64 + 2 * run.has("BAG_OF_PREPARATION") as i64;
    }
    n += 2 * run.has("SNECKO_EYE") as i64 + run.has("RING_OF_THE_SERPENT") as i64 + f.pw.val("MACHINE_LEARNING");
    n
}

pub fn turn_start(f: &mut Fight) {
    if f.pw.on("NEXT_BLOCK") {
        let n = f.pw.shift_remove("NEXT_BLOCK").unwrap_or(0);
        f.gain_block(n);
    }
    if f.round == 1 {
        f.energy += f.run.has("LANTERN") as i64;
        if f.run.has("PANTOGRAPH") && f.kind == "boss" {
            f.heal(25);
        }
    }
    if f.round == 2 && f.run.has("HORN_CLEAT") {
        f.gain_block(14);
    }
    if f.round == 3 && f.run.has("CAPTAIN_S_WHEEL") {
        f.gain_block(18);
    }
    if f.run.has("HAPPY_FLOWER") && add(&mut f.run.counters, "HAPPY_FLOWER", 1) % 3 == 0 {
        f.energy += 1;
    }
    if f.run.has("MERCURY_HOURGLASS") {
        f.hurt_all(3);
    }
    if f.run.has("INCENSE_BURNER") && add(&mut f.run.counters, "INCENSE_BURNER", 1) % 6 == 0 {
        add(&mut f.pw, "INTANGIBLE", 1);
    }
    f.energy += f.orbs.iter().filter(|o| o.kind == "PLASMA").count() as i64;
    if f.pw.on("DEMON_FORM") && !by_text(f, "DEMON_FORM") {
        let n = f.pw["DEMON_FORM"];
        add(&mut f.pw, "STRENGTH", n);
    }
    if f.pw.on("NOXIOUS_FUMES") && !by_text(f, "NOXIOUS_FUMES") {
        debuff_all(f, "POISON", f.pw["NOXIOUS_FUMES"]);
    }
    if f.pw.on("BRUTALITY") {
        f.lose_hp(f.pw["BRUTALITY"], false);
        f.draw_cards(f.pw["BRUTALITY"]);
    }
    if f.pw.on("BERSERK") {
        f.energy += f.pw["BERSERK"];
    }
    if f.pw.on("INFINITE_BLADES") && !by_text(f, "INFINITE_BLADES") {
        f.add_new("SHIV", f.pw["INFINITE_BLADES"], "hand", false);
    }
    if f.pw.on("WRAITH_FORM") {
        let n = f.pw["WRAITH_FORM"];
        add(&mut f.pw, "DEXTERITY", -n);
    }
    if f.pw.on("BIASED_COGNITION") {
        let n = f.pw["BIASED_COGNITION"];
        add(&mut f.pw, "FOCUS", -n);
    }
    if f.pw.on("LOOP") && !f.orbs.is_empty() {
        for _ in 0..f.pw["LOOP"] {
            let Some(o) = f.orbs.first().cloned() else { break };
            passive(f, &o);
        }
    }
    if f.pw.on("DEVOTION") {
        f.gain_mantra(f.pw["DEVOTION"]);
    }
    if f.pw.on("DEVA_FORM") {
        let n = f.pw["DEVA_FORM"];
        f.energy += add(&mut f.pw, "DEVA_ENERGY", n);
    }
    if f.pw.on("BATTLE_HYMN") {
        f.add_new("SMITE", f.pw["BATTLE_HYMN"], "hand", false);
    }
    if f.pw.shift_remove("SIMMERING").unwrap_or(0) != 0 {
        f.enter("WRATH");
    }
    if f.pw.on("BLASPHEMY_DUE") {
        f.lose_hp(f.run.hp, false);
    }
    (f.game.TURN_START)(f);
}

pub fn after_draw(f: &mut Fight) {
    if f.pw.on("TOOLS_OF_THE_TRADE") && !by_text(f, "TOOLS_OF_THE_TRADE") {
        f.draw_cards(f.pw["TOOLS_OF_THE_TRADE"]);
        discard_from_hand(f, f.pw["TOOLS_OF_THE_TRADE"]);
    }
    if f.pw.on("FORESIGHT") {
        scry(f, f.pw["FORESIGHT"]);
    }
    if f.pw.on("CREATIVE_AI") || f.pw.on("HELLO_WORLD") || f.pw.on("MAYHEM") {
        (f.game.TURN_CARDS)(f);
    }
}

/// An orb's passive (`o`: the orb as it was; a Dark orb still among ours grows).
pub fn passive(f: &mut Fight, o: &Orb) {
    if o.kind == "DARK" {
        let live = f.orbs.iter().find(|x| x.uid == o.uid).cloned().unwrap_or_else(|| o.clone());
        let n = f.orb_passive(&live);
        if let Some(x) = f.orbs.iter_mut().find(|x| x.uid == o.uid) {
            x.dark += n;
        }
    } else if o.kind != "PLASMA" {
        let n = f.orb_passive(o);
        f.orb_effect(o, n, false);
    }
}

pub fn turn_end(f: &mut Fight) {
    if f.pw.on("METALLICIZE") {
        f.gain_block(f.pw["METALLICIZE"]);
    }
    if f.pw.on("PLATING") {
        f.gain_block(f.pw["PLATING"]);
    }
    if f.pw.on("LIKE_WATER") && f.stance == "CALM" {
        f.gain_block(f.pw["LIKE_WATER"]);
    }
    if f.pw.on("COMBUST") {
        f.lose_hp(f.pw.get("COMBUST_HP").copied().unwrap_or(1), false);
        f.hurt_all(f.pw["COMBUST"]);
    }
    if f.pw.on("OMEGA") {
        f.hurt_all(f.pw["OMEGA"]);
    }
    if f.pw.on("STUDY") {
        f.add_new("INSIGHT", f.pw["STUDY"], "draw", false);
    }
    for o in f.orbs.clone() {
        if f.won {
            break;
        }
        passive(f, &o);
    }
    if f.run.has("FROZEN_CORE") && (f.orbs.len() as i64) < f.orb_slots {
        f.channel("FROST");
    }
    if f.run.has("ORICHALCUM") && f.block == 0 {
        f.gain_block(6);
    }
    if f.run.has("STONE_CALENDAR") && f.round == 7 {
        f.hurt_all(52);
    }
    if f.pw.on("CONSTRICT") {
        f.take(f.pw["CONSTRICT"], None, false);
    }
    for c in f.hand.clone() {
        match c.key() {
            "BURN" | "DECAY" => {
                f.take(if c.up && c.key() == "BURN" { 4 } else { 2 }, None, false);
            }
            "REGRET" => {
                f.lose_hp(f.hand.len() as i64, false);
            }
            "WITHER" => {
                // 3 damage a level (Slay the Spire 2's Aeonglass): "Wither+2" does 9
                let level: i64 = c.d.name.split_once('+').map(|(_, n)| n).filter(|n| !n.is_empty()).map_or(0, |n| n.parse().expect("a Wither's level"));
                f.take(3 + 3 * level, None, false);
            }
            "DOUBT" => debuff_me(f, "WEAK", 1),
            "SHAME" => debuff_me(f, "FRAIL", 1),
            _ => {}
        }
    }
    if f.pw.on("WAVE_OF_THE_HAND") {
        f.pw.shift_remove("WAVE_OF_THE_HAND");
    }
    if f.pw.on("DOUBLE_DAMAGE_NEXT") {
        let n = f.pw.shift_remove("DOUBLE_DAMAGE_NEXT").unwrap_or(0);
        f.pw.put("DOUBLE_DAMAGE", n);
    } else if f.pw.on("DOUBLE_DAMAGE") {
        f.pw.shift_remove("DOUBLE_DAMAGE");
    }
    if f.pw.on("BLUR") {
        f.pw.put("KEEP_BLOCK", 1);
        f.pw["BLUR"] -= 1;
    }
    (f.game.TURN_END)(f);
}

pub fn foe_round_end(f: &mut Fight, i: usize) {
    let foe = &mut f.foes[i];
    if foe.powers.on("RITUAL") {
        if foe.mem.truthy("ritual_ready") {
            let n = foe.powers["RITUAL"];
            add(&mut foe.powers, "STRENGTH", n);
        }
        foe.mem.set_flag("ritual_ready", true);
    }
    foe.block += foe.powers.val("METALLICIZE") + foe.powers.val("PLATING");
    if foe.powers.on("REGEN") {
        foe.hp = foe.max_hp.min(foe.hp + foe.powers["REGEN"]);
    }
    if foe.powers.on("MALLEABLE") {
        foe.powers["MALLEABLE"] = foe.mem.setdefault("malleable", foe.powers["MALLEABLE"]);
    }
    foe_hook(f, i, "end", 0, false);
}

// ---------------------------------------------------------------- enemies

pub fn foe_acts(f: &mut Fight, i: usize, m: &Move) {
    if matches!(m.kind.as_str(), "Sleep" | "Stun" | "Unknown") && m.dmg == 0 {
        for fx in &m.fx {
            do_(f, i, fx);
        }
        return;
    }
    if m.dmg != 0 {
        let (base, hits) = (move_damage(f, i, m), move_hits(f, i, m));
        let hp0 = f.run.hp;
        for _ in 0..hits {
            if f.lost || !f.foes[i].alive() {
                break;
            }
            let lost = f.take(f.foe_hit(i, base), Some(i), true);
            if lost != 0 && f.pw.on("STATIC_DISCHARGE") {
                for _ in 0..f.pw["STATIC_DISCHARGE"] {
                    f.channel("LIGHTNING");
                }
            }
            if lost != 0 && (f.foes[i].mem.truthy("painful_stabs") || f.foes[i].powers.on("PAINFUL_STABS")) {
                f.add_new("WOUND", 1, "discard", false);
            }
            let foe = &mut f.foes[i];
            if lost != 0 && foe.powers.on("SUCK") {
                let n = foe.powers["SUCK"];
                add(&mut foe.powers, "STRENGTH", n);
            }
            if lost != 0 && foe.powers.on("PAPER_CUTS") {
                f.run.max_hp -= foe.powers["PAPER_CUTS"];
                f.run.hp = f.run.hp.min(f.run.max_hp);
            }
        }
        if f.foes[i].powers.on("IMBALANCED") && f.run.hp == hp0 {
            f.foes[i].mem.set_flag("off_balance", true);
        }
    }
    if m.block != 0 {
        f.foes[i].block += m.block;
    }
    for fx in &m.fx {
        if f.lost {
            return;
        }
        do_(f, i, fx);
    }
    foe_hook(f, i, "acted", 0, false);
}

/// One effect of an enemy's move (the original's `do`).
pub fn do_(f: &mut Fight, i: usize, fx: &Fx) {
    let word = fx.word.as_str();
    let n = fx.int(0);
    let alive = f.alive();
    match word {
        "str" => {
            add(&mut f.foes[i].powers, "STRENGTH", n);
        }
        "str_all" => {
            for x in alive {
                add(&mut f.foes[x].powers, "STRENGTH", n);
            }
        }
        "ritual" | "metallicize" | "plated" | "artifact" | "thorns" | "intangible" | "enrage" | "regen" | "curiosity" => {
            let power = if word == "plated" { "PLATING".to_string() } else { word.to_uppercase() };
            add(&mut f.foes[i].powers, &power, n);
        }
        "block_ally" => {
            let others: Vec<usize> = alive.into_iter().filter(|x| *x != i).collect();
            let pool = if others.is_empty() { vec![i] } else { others };
            let at = *f.run.rng.choice(&pool);
            f.foes[at].block += n;
        }
        "block_all" => {
            for x in alive {
                f.foes[x].block += n;
            }
        }
        "weak" | "vuln" | "frail" => {
            let power = match word {
                "weak" => "WEAK",
                "vuln" => "VULNERABLE",
                _ => "FRAIL",
            };
            if !f.pw.on(power) {
                f.fresh.insert(power.to_string());
            }
            debuff_me(f, power, n);
        }
        "status" => {
            let key = fx.str(0).expect("a status card's key").to_string();
            let (count, pile) = (fx.int(1), if fx.args.len() > 2 { fx.str(2).expect("a pile").to_string() } else { "discard".to_string() });
            let up = f.foes[i].mem.truthy("burn_up") && key == "BURN";
            f.add_new(&key, count, &pile, up);
        }
        "heal" => {
            let foe = &mut f.foes[i];
            foe.hp = foe.max_hp.min(foe.hp + n);
        }
        "heal_all" => {
            for x in alive {
                let x = &mut f.foes[x];
                x.hp = x.max_hp.min(x.hp + n);
            }
        }
        "summon" => {
            let key = fx.str(0).expect("who is summoned").to_string();
            for _ in 0..(if fx.args.len() > 1 { fx.int(1) } else { 1 }) {
                if f.foes.iter().filter(|x| x.alive()).count() < 5 {
                    let new = (f.game.spawn)(&key, f, true, None);
                    f.foes.push(new);
                    monsters::choose(f, f.foes.len() - 1);
                }
            }
        }
        "str_down" | "dex_down" | "focus_down" => {
            let power = match word {
                "str_down" => "STRENGTH",
                "dex_down" => "DEXTERITY",
                _ => "FOCUS",
            };
            add(&mut f.pw, power, -n);
        }
        "draw_down" => f.pw.put("DRAW_REDUCTION", n),
        "entangle" => f.pw.put("ENTANGLED", 1),
        "hex" => f.pw.put("HEX_DAZED", 1),
        "confuse" => f.pw.put("CONFUSED", 1),
        "constrict" => {
            add(&mut f.pw, "CONSTRICT", n);
        }
        "steal_gold" => {
            let took = f.run.gold.min(n);
            f.run.gold -= took;
            f.foes[i].mem.add("stolen", took);
        }
        "remove_debuffs" => {
            let foe = &mut f.foes[i];
            for k in ["WEAK", "VULNERABLE", "POISON"] {
                foe.powers.shift_remove(k);
            }
            if foe.powers.val("STRENGTH") < 0 {
                foe.powers["STRENGTH"] = 0;
            }
        }
        "die" => f.kill(i),
        "damage_all" => {
            // an explosion
            f.take(n, Some(i), false);
        }
        "call" => fx.fun(0).expect("a function to call")(f, i),
        "power" => {
            add(&mut f.foes[i].powers, fx.str(0).expect("a power"), fx.int(1));
        }
        "player_power" => {
            let power = fx.str(0).expect("a power");
            if !f.pw.on(power) {
                f.fresh.insert(power.to_string());
            }
            add(&mut f.pw, power, fx.int(1));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- potions

pub fn potion_usable(_f: &Fight, pot: &Potion) -> bool {
    !matches!(pot.key.as_str(), "FAIRY_IN_A_BOTTLE" | "SMOKE_BOMB")
}

pub fn drink(f: &mut Fight, pot: &Potion, foe: Option<usize>) {
    let t = pot.text.as_str();
    let low = t.to_lowercase();
    let sacred = if f.run.has("SACRED_BARK") { 2 } else { 1 };
    let num = |pat: &str| -> i64 { base::re::cached_i(pat).captures(t).map_or(0, |m| m[1].parse::<i64>().expect("a number") * sacred) };
    let n = num(r"deal (\d+) damage to all enemies");
    if n != 0 {
        f.hurt_all(n);
    } else {
        let n = num(r"deal (\d+) damage");
        if let (true, Some(foe)) = (n != 0, foe) {
            f.hurt(foe, n, false);
        }
    }
    let n = num(r"gain (\d+) block");
    if n != 0 {
        f.gain_block(n);
    }
    for (word, power) in [
        ("strength", "STRENGTH"), ("dexterity", "DEXTERITY"), ("focus", "FOCUS"), ("artifact", "ARTIFACT"), ("plated armor", "PLATING"),
        ("metallicize", "METALLICIZE"), ("thorns", "THORNS"), ("regen", "REGEN"), ("intangible", "INTANGIBLE"), ("ritual", "RITUAL_ME"),
    ] {
        let n = num(&format!(r"gain (\d+) {word}"));
        if n != 0 {
            let passing = matches!(power, "STRENGTH" | "DEXTERITY") && low.contains("at the end of your turn");
            add(if passing { &mut f.temp } else { &mut f.pw }, power, n);
        }
    }
    for (word, power) in [("weak", "WEAK"), ("vulnerable", "VULNERABLE"), ("poison", "POISON")] {
        let n = num(&format!(r"apply (\d+) {word}"));
        if n != 0 {
            let targets: Vec<usize> = if low.contains("all enemies") { f.alive() } else { foe.into_iter().collect() };
            for x in targets {
                debuff(f, Some(x), power, n);
            }
        }
    }
    let n = num(r"draw (\d+) cards");
    if n != 0 {
        f.draw_cards(n);
    }
    let mut n = num(r"gain (\d+) energy");
    if n == 0 {
        n = t.matches("energy_icon").count() as i64 * sacred;
    }
    if n != 0 {
        f.energy += n;
    }
    let n = num(r"heal (\d+)% of your max hp");
    if n != 0 {
        f.heal(base::floordiv(f.run.max_hp * n, 100));
    }
    let n = num(r"gain (\d+) max hp");
    if n != 0 {
        f.run.max_hp += n;
        f.heal(n);
    }
    let n = num(r"add (\d+) upgraded shivs");
    if n != 0 {
        f.add_new("SHIV", n, "hand", true);
    }
    if low.contains("miracles") {
        f.add_new("MIRACLE", 2 * sacred, "hand", false);
    }
    if low.contains("enter divinity") {
        f.enter("DIVINITY");
    } else if low.contains("enter calm or wrath") {
        f.enter("WRATH");
    }
    if low.contains("next card is played twice") {
        add(&mut f.pw, "DUPLICATION", sacred);
    }
    let n = num(r"gain (\d+) orb slots");
    if n != 0 {
        f.orb_slots += n;
    }
    if low.contains("channel 1 dark for each orb slot") {
        for _ in 0..f.orb_slots {
            f.channel("DARK");
        }
    }
    if low.contains("play the top 3 cards") {
        for _ in 0..3 * sacred {
            f.refill();
            if !f.draw.is_empty() && !f.won {
                let mut c = f.draw.pop().expect("a card");
                let at = f.alive().first().copied();
                resolve(f, &mut c, at, f.energy);
                f.put_away(c);
            }
        }
    }
    if low.contains("upgrade all cards in your hand") {
        for c in f.hand.iter_mut() {
            c.up = true;
        }
    }
    if low.contains("add a random") || low.contains("choose 1 of 3") {
        (f.game.POTION_CARD)(f, pot);
    }
}
