//! One run: the map, the rooms, the rewards, the shops and the rest sites, every choice made by the bot's own code.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::cards as bot_cards;
use crate::learn;
use crate::base::{self, JsonExt, Random};
use crate::run::strategy;
use crate::sim::cards::{Card, ViewArgs};
use crate::sim::content::{Content, Item, Nodes};
use crate::sim::effects::Powers;
use crate::sim::engine::Fight;

#[derive(Clone, Debug, PartialEq)]
pub struct Potion {
    pub key: String,
    pub name: String,
    pub text: String,
    pub rarity: String,
}

impl Potion {
    /// `Potion(key, name, text, rarity="Common")`.
    pub fn new(key: &str, name: &str, text: &str, rarity: &str) -> Potion {
        Potion { key: key.to_string(), name: name.to_string(), text: text.to_string(), rarity: rarity.to_string() }
    }

    pub fn view(&self, slot: i64, keep: &HashSet<String>) -> Value {
        let low = self.text.to_lowercase();
        let aimed = !low.contains("all enemies") && ["deal ", "apply "].iter().any(|w| low.contains(w));
        json!({"slot": slot, "id": self.key, "name": self.name, "description": self.text, "target_type": if aimed { "AnyEnemy" } else { "Self" },
               "can_use_in_combat": !keep.contains(&self.key), "can_discard": true})
    }
}

/// A card's view with its index (`c.view(i)`).
pub fn view_at(c: &Card, i: usize) -> Value {
    c.view(ViewArgs { index: Some(i as i64), ..Default::default() })
}

pub struct Run {
    pub game: Rc<Content>,
    pub character: String,
    pub seed: i64,
    pub rng: Random,
    pub asc: i64,
    pub title: String,
    pub max_hp: i64,
    pub hp: i64,
    pub gold: i64,
    pub energy: i64,
    pub orb_slots: i64,
    pub deck: Vec<Card>,
    pub relics: Vec<String>,
    pub potions: Vec<Option<Potion>>,
    pub counters: IndexMap<String, i64>,
    pub act: i64,
    pub floor: i64,
    pub mem: strategy::Memory,
    pub knobs: IndexMap<String, f64>,
    /// fights, picks, elites, rests, smiths, removals, shops, events, bought, skips (and "potions" once one is drunk).
    pub log: Map<String, Value>,
    pub digest: Vec<String>,
    pub unknown_odds: IndexMap<String, f64>,
    pub recent: Vec<String>,
    pub potion_chance: f64,
    pub fights_in_act: i64,               // (set by the game's act_start)
    pub act_id: String,                   // (Slay the Spire 2's act: OVERGROWTH, UNDERDOCKS, HIVE, GLORY)
    pub bosses: HashMap<i64, Vec<String>>, // act -> the boss drawn for it (the encounter's name)
}

impl Run {
    /// `Run(game, character, seed, knobs=None, ascension=0)`.
    pub fn new(game: &Rc<Content>, character: &str, seed: i64, knobs: Option<&IndexMap<String, f64>>, ascension: i64) -> Run {
        let start = game.CHARACTERS.get(character).unwrap_or_else(|| panic!("KeyError: {character}")).clone();
        let mut run = Run::hollow(game);
        run.character = character.to_string();
        run.seed = seed;
        run.rng = Random::new(seed as i128);
        run.asc = ascension;
        run.title = start.title.clone();
        run.max_hp = start.hp - (game.max_hp_penalty)(ascension, character);
        run.hp = (game.start_hp)(&run);
        run.gold = 99;
        run.energy = 3;
        run.orb_slots = start.orbs;
        run.deck = start.deck.iter().map(|k| (game.make)(k, false)).collect();
        run.potions = vec![None; 3];
        run.act = 1;
        run.mem.card_stats = None;
        let g = game.clone();
        run.mem.upgrade_gain = Some(Rc::new(move |card: &Value| _upgrade_gain(&g, card)));
        run.knobs = knobs.cloned().unwrap_or_default();
        for (k, v) in [("fights", json!([])), ("picks", json!([])), ("elites", json!(0)), ("rests", json!(0)), ("smiths", json!(0)), ("removals", json!(0)),
                       ("shops", json!(0)), ("events", json!(0)), ("bought", json!([])), ("skips", json!(0))] {
            run.log.insert(k.to_string(), v);
        }
        run.unknown_odds = [("Monster", 0.1), ("Shop", 0.03), ("Treasure", 0.02)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        run.potion_chance = 0.4;
        for rid in &start.relics {
            run.gain_relic(rid);
        }
        (game.setup_run)(&mut run);
        run
    }

    /// An empty run: what stands in a `Run`'s place while a fight holds it.
    pub fn hollow(game: &Rc<Content>) -> Run {
        Run {
            game: game.clone(),
            character: String::new(),
            seed: 0,
            rng: Random::new(0),
            asc: 0,
            title: String::new(),
            max_hp: 0,
            hp: 0,
            gold: 0,
            energy: 0,
            orb_slots: 0,
            deck: Vec::new(),
            relics: Vec::new(),
            potions: Vec::new(),
            counters: IndexMap::new(),
            act: 0,
            floor: 0,
            mem: strategy::Memory::new(),
            knobs: IndexMap::new(),
            log: Map::new(),
            digest: Vec::new(),
            unknown_odds: IndexMap::new(),
            recent: Vec::new(),
            potion_chance: 0.0,
            fights_in_act: 0,
            act_id: String::new(),
            bosses: HashMap::new(),
        }
    }

    // ------------------------------------------------------------------ what the bot sees of us

    pub fn potion_slots(&self) -> i64 {
        self.potions.len() as i64
    }

    pub fn has(&self, relic: &str) -> bool {
        self.relics.iter().any(|r| r == relic)
    }

    pub fn r#where(&self) -> Value {
        json!({"act": self.act, "floor": self.floor, "ascension": self.asc})
    }

    pub fn relic_views(&self) -> Vec<Value> {
        self.relics.iter().map(|r| json!({"id": r, "name": base::title(&r.replace('_', " ")), "description": (self.game.relic_text)(r), "counter": self.counters.get(r.as_str()).copied().unwrap_or(-1)})).collect()
    }

    pub fn potion_views(&self) -> Vec<Value> {
        self.potions.iter().enumerate().filter_map(|(i, p)| p.as_ref().map(|p| p.view(i as i64, &self.game.KEEP_POTIONS))).collect()
    }

    pub fn potion_at(&self, slot: i64) -> Option<&Potion> {
        if 0 <= slot && slot < self.potions.len() as i64 {
            self.potions[slot as usize].as_ref()
        } else {
            None
        }
    }

    pub fn drop_potion(&mut self, slot: i64) {
        if 0 <= slot && slot < self.potions.len() as i64 {
            self.potions[slot as usize] = None;
        }
    }

    pub fn add_potion(&mut self, p: Potion) -> bool {
        if self.has("SOZU") {
            return false;
        }
        match self.potions.iter().position(|x| x.is_none()) {
            Some(free) => {
                self.potions[free] = Some(p);
                true
            }
            None => false,
        }
    }

    pub fn player(&self) -> Value {
        json!({"character": self.title, "hp": self.hp, "max_hp": self.max_hp, "gold": self.gold, "block": 0, "status": [],
               "relics": self.relic_views(), "potions": self.potion_views(), "max_potion_slots": self.potion_slots()})
    }

    /// `screen(kind, **body)`: `body` is the one keyword the screen is given, and its value.
    pub fn screen(&self, kind: &str, body: (&str, Value)) -> Value {
        let mut state = json!({"state_type": kind, "run": self.r#where(), "player": self.player()});
        state[body.0] = body.1;
        state
    }

    /// What the bot remembers of its deck as a fight begins (bot.Bot._note_deck).
    pub fn note_deck(&mut self) {
        const KEPT: [&str; 8] = ["id", "name", "type", "cost", "star_cost", "description", "rarity", "is_upgraded"];
        self.mem.deck = self
            .deck
            .iter()
            .filter(|c| c.r#type() != "Status")
            .map(|c| match c.view(ViewArgs::default()) {
                Value::Object(view) => Value::Object(view.into_iter().filter(|(k, _)| KEPT.contains(&k.as_str())).collect()),
                other => other,
            })
            .collect();
        self.mem.deck_size = self.mem.deck.len() as i64;
        self.mem.max_energy = self.energy;
    }

    /// `log[key] += n` (from 0, as `effects.add(run.log, key, n)` has it).
    pub fn log_add(&mut self, key: &str, n: i64) {
        let now = self.log.get(key).and_then(|v| v.as_i64()).unwrap_or(0) + n;
        self.log.insert(key.to_string(), json!(now));
    }

    /// `log[key].append(item)`.
    pub fn log_push(&mut self, key: &str, item: Value) {
        if let Some(Value::Array(list)) = self.log.get_mut(key) {
            list.push(item);
        }
    }

    // ------------------------------------------------------------------ gains

    pub fn gain_relic(&mut self, rid: &str) {
        if self.has(rid) {
            return;
        }
        self.relics.push(rid.to_string());
        (self.game.relic_pickup)(self, rid);
    }

    pub fn add_card(&mut self, mut c: Card) {
        let egg = match c.r#type() {
            "Attack" => "MOLTEN_EGG",
            "Skill" => "TOXIC_EGG",
            "Power" => "FROZEN_EGG",
            _ => "",
        };
        if self.has(egg) {
            c.up = true;
        }
        self.deck.push(c);
    }

    /// `remove_card(c)`: the deck's card with `c`'s uid.
    pub fn remove_card(&mut self, c: &Card) {
        if self.take_card(c).is_some() {
            self.log_add("removals", 1);
        }
    }

    /// `run.deck.remove(c)`.
    pub fn take_card(&mut self, c: &Card) -> Option<Card> {
        let at = self.deck.iter().position(|d| d.uid == c.uid)?;
        Some(self.deck.remove(at))
    }

    pub fn lose_hp(&mut self, n: i64) {
        self.hp = (self.hp - n).max(1);
    }

    // ------------------------------------------------------------------ the bot's choices

    /// `choose_from_deck(screen_type, prompt, pool=None)`: a clone of the card chosen (the deck's own has its `uid`).
    pub fn choose_from_deck(&mut self, screen_type: &str, prompt: &str, pool: Option<Vec<Card>>) -> Option<Card> {
        let pool = pool.unwrap_or_else(|| self.deck.clone());
        if pool.is_empty() {
            return None;
        }
        let state = self.screen("card_select", ("card_select", json!({"screen_type": screen_type, "prompt": prompt,
                                                                      "cards": pool.iter().enumerate().map(|(i, c)| view_at(c, i)).collect::<Vec<_>>(),
                                                                      "preview_showing": false, "can_confirm": false, "can_cancel": false})));
        self.mem.select_key = String::new();
        self.mem.selected = BTreeSet::new();
        for (action, params) in strategy::card_select(&state, &mut self.mem) {
            let index = params["index"].int_or(-1);
            if action == "select_card" && 0 <= index && index < pool.len() as i64 {
                return Some(pool[index as usize].clone());
            }
        }
        None
    }

    pub fn upgrade_one(&mut self) -> Option<Card> {
        let fresh: Vec<Card> = self.deck.iter().filter(|c| !c.up && !matches!(c.r#type(), "Status" | "Curse")).cloned().collect();
        let mut c = self.choose_from_deck("upgrade", "Choose a card to Upgrade.", Some(fresh))?;
        c.up = true;
        if let Some(own) = self.deck.iter_mut().find(|d| d.uid == c.uid) {
            own.up = true;
        }
        self.digest.push(format!("U{}", c.key()));
        Some(c)
    }

    pub fn remove_one(&mut self) -> Option<Card> {
        let pool: Vec<Card> = self.deck.iter().filter(|c| !matches!(c.key(), "ASCENDER_S_BANE" | "CURSE_OF_THE_BELL" | "NECRONOMICURSE")).cloned().collect();
        let c = self.choose_from_deck("remove", "Choose a card to Remove.", Some(pool))?;
        self.remove_card(&c);
        self.digest.push(format!("R{}", c.key()));
        Some(c)
    }

    /// `card_reward(offered, can_skip=True)`.
    pub fn card_reward(&mut self, offered: Vec<Card>, can_skip: bool) -> Option<Card> {
        if offered.is_empty() {
            return None;
        }
        let state = self.screen("card_reward", ("card_reward", json!({"cards": offered.iter().enumerate().map(|(i, c)| view_at(c, i)).collect::<Vec<_>>(), "can_skip": can_skip})));
        for (action, params) in strategy::card_reward(&state, &mut self.mem) {
            if action == "skip_card_reward" {
                self.log_add("skips", 1);
                self.digest.push("S".to_string());
                return None;
            }
            if action == "select_card_reward" {
                let c = offered[params["card_index"].as_i64().expect("a card index") as usize].clone();
                self.add_card(c.clone());
                let c = self.deck.last().expect("the card just added").clone();
                if let Value::Object(view) = c.view(ViewArgs::default()) {
                    self.mem.deck.push(Value::Object(view.into_iter().filter(|(k, _)| k != "index").collect()));
                }
                self.mem.deck_size += 1;
                self.mem.picks.push((self.floor, c.name()));
                self.log_push("picks", json!([self.floor, c.key()]));
                self.digest.push(format!("C{}", c.key()));
                return Some(c);
            }
        }
        None
    }

    // ------------------------------------------------------------------ the run

    pub fn play(&mut self) -> Value {
        let t0 = Instant::now();
        let g = self.game.clone();
        let mut knobs = learn::defaults(Some(g.KEY));
        for (k, v) in &self.knobs {
            knobs.insert(k.clone(), *v);
        }
        learn::set_knobs(&knobs);
        self.mem.reset_run();
        self.mem.card_stats = None;
        (g.run_start)(self);
        let mut won = false;
        for act in 1..=g.ACTS {
            self.act = act;
            (g.act_start)(self);
            if !self.play_act() {
                break;
            }
            if act == g.ACTS {
                won = true;
            } else {
                self.hp += base::trunc((self.max_hp - self.hp) as f64 * (g.boss_heal)(self));
            }
        }
        let mut deck: Vec<String> = self.deck.iter().map(|c| c.name()).collect();
        deck.sort();
        let mut out = json!({
            "game": g.KEY, "character": self.character, "seed": self.seed, "ascension": self.asc, "knobs": self.knobs.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<String, Value>>(),
            "model_lean": learn::knob("model_lean"), // (sim prior learns only from runs that did not lean on its table)
            "won": won, "act": self.act, "floor": self.floor, "hp": self.hp, "max_hp": self.max_hp, "gold": self.gold,
            "deck": deck, "relics": self.relics, "secs": base::round_to(t0.elapsed().as_secs_f64(), 2),
        });
        for (k, v) in &self.log {
            out[k.as_str()] = v.clone();
        }
        out
    }

    pub fn play_act(&mut self) -> bool {
        let g = self.game.clone();
        let nodes = (g.make_map)(self);
        let mut at: Option<(i64, i64)> = None;
        loop {
            let options = self.next_options(&nodes, at);
            if options.is_empty() {
                return true;
            }
            let pick = self.choose_node(&nodes, &options);
            self.floor += 1;
            let here = (pick["col"].int(), pick["row"].int());
            at = Some(here);
            let kind = pick["type"].s().to_string();
            self.digest.push(format!("M{}{}", kind.chars().take(2).collect::<String>(), here.0));
            if !self.room(&kind) {
                return false;
            }
            if kind == "Boss" {
                (g.boss_won)(self);
                return true;
            }
        }
    }

    pub fn next_options(&self, nodes: &Nodes, at: Option<(i64, i64)>) -> Vec<Value> {
        let pair = |c: &Value| (c[0].int(), c[1].int());
        let keys: Vec<(i64, i64)> = match at {
            None => {
                let mut keys: Vec<(i64, i64)> = nodes.keys().filter(|k| k.1 == 1).copied().collect();
                keys.sort();
                keys
            }
            Some(at) => nodes[&at]["children"].list().iter().map(pair).collect(),
        };
        let mut out = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            let n = &nodes[k];
            let kids: Vec<Value> = n["children"].list().iter().filter_map(|c| nodes.get(&pair(c))).map(|c| json!({"type": c["type"], "row": c["row"]})).collect();
            out.push(json!({"index": i, "col": k.0, "row": k.1, "type": n["type"], "leads_to": kids}));
        }
        out
    }

    pub fn choose_node(&mut self, nodes: &Nodes, options: &[Value]) -> Value {
        let state = self.screen("map", ("map", json!({"next_options": options, "nodes": nodes.values().collect::<Vec<_>>()})));
        for (action, params) in strategy::map_screen(&state, &mut self.mem) {
            if action == "choose_map_node" {
                return options[params["index"].as_i64().expect("an index") as usize].clone();
            }
        }
        options[0].clone()
    }

    pub fn room(&mut self, kind: &str) -> bool {
        let mut kind = kind.to_string();
        if kind == "Unknown" {
            kind = self.unknown_room();
        }
        match kind.as_str() {
            "Monster" | "Elite" | "Boss" => return self.fight(&kind.to_lowercase()),
            "RestSite" => self.rest_site(),
            "Shop" => self.shop(),
            "Treasure" => self.treasure(),
            "Event" => self.event(),
            _ => {}
        }
        kind != "Event" || self.hp > 0
    }

    pub fn unknown_room(&mut self) -> String {
        let mut r = self.rng.random();
        let mut pick = "Event";
        for kind in ["Monster", "Shop", "Treasure"] {
            if r < self.unknown_odds[kind] {
                pick = kind;
                break;
            }
            r -= self.unknown_odds[kind];
        }
        let base = &self.game.UNKNOWN_ODDS;
        for (k, odds) in self.unknown_odds.iter_mut() {
            // (the odds of the room met go back to the start, the others grow)
            *odds = if k == pick { base[k.as_str()] } else { *odds + base[k.as_str()] };
        }
        pick.to_string()
    }

    // ------------------------------------------------------------------ fights

    pub fn fight(&mut self, kind: &str) -> bool {
        let g = self.game.clone();
        let foes = (g.encounter)(self, kind);
        self.note_deck();
        self.mem.boss_next = false;
        let hp0 = self.hp;
        let met: BTreeSet<String> = foes.iter().map(|x| x.key.clone()).collect(); // (the encounter as it starts: splits and summons join the list)
        let drunk = self.log.get("potions").and_then(|v| v.as_i64()).unwrap_or(0);
        let me = std::mem::replace(self, Run::hollow(&g));
        let mut f = Fight::new(me, foes, kind);
        let won = f.run_fight();
        std::mem::swap(self, &mut f.run);
        self.digest.push(format!("F{}", f.digest.concat()));
        let mut entry = json!({"act": self.act, "floor": self.floor, "kind": kind, "foes": met, "hp0": hp0,
                               "hp1": self.hp.max(0), "max_hp": self.max_hp, "rounds": f.round, "won": won,
                               "drunk": self.log.get("potions").and_then(|v| v.as_i64()).unwrap_or(0) - drunk, "left": self.potions.iter().filter(|p| p.is_some()).count()});
        if !won {
            // how much of the enemy was left standing: a lost race reads differently from a deck that never got going
            let left: i64 = f.foes.iter().filter(|x| x.alive() && !x.minion).map(|x| x.hp).sum();
            let whole: i64 = f.foes.iter().filter(|x| !x.minion).map(|x| x.max_hp).sum();
            entry["foe_left"] = json!(base::round_to(left as f64 / whole.max(1) as f64, 2));
        }
        self.log_push("fights", entry);
        self.log_add("elites", (kind == "elite") as i64);
        if won {
            (g.fight_won)(self, &f);
            if !f.escaped {
                self.rewards(kind, &f);
            }
        }
        won
    }

    pub fn rewards(&mut self, kind: &str, _f: &Fight) {
        let g = self.game.clone();
        // (each item: the dict the bot is shown, and the original's "_potion" / "_relic" beside it)
        let mut items: Vec<(Map<String, Value>, Option<Potion>, Option<String>)> = Vec::new();
        let item = |v: Value| match v {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let gold = (g.fight_gold)(self, kind);
        if gold != 0 {
            items.push((item(json!({"type": "gold", "gold_amount": gold})), None, None));
        }
        if kind != "boss" && self.rng.random() < self.potion_chance || self.has("WHITE_BEAST_STATUE") && kind != "boss" {
            self.potion_chance = (self.potion_chance - 0.1).max(0.0);
            let p = (g.random_potion)(self);
            items.push((item(json!({"type": "potion", "potion_id": p.key, "potion_name": p.name, "description": p.text})), Some(p), None));
        } else if kind != "boss" {
            self.potion_chance = (self.potion_chance + 0.1).min(1.0);
        }
        if kind == "elite" {
            let rid = (g.random_relic)(self, false);
            items.push((item(json!({"type": "relic"})), None, Some(rid)));
        }
        items.push((item(json!({"type": "card"})), None, None));
        if self.has("PRAYER_WHEEL") && kind == "monster" {
            items.push((item(json!({"type": "card"})), None, None));
        }
        for (n, it) in items.iter_mut().enumerate() {
            it.0.insert("index".to_string(), json!(n));
        }
        let mut taken: HashSet<usize> = HashSet::new();
        for _ in 0..12 {
            let left: Vec<Value> = items.iter().enumerate().filter(|(i, _)| !taken.contains(i)).map(|(_, it)| Value::Object(it.0.clone())).collect();
            let state = self.screen("rewards", ("rewards", json!({"items": left, "can_proceed": true})));
            let mut acted = false;
            for (action, params) in strategy::rewards(&state, &mut self.mem) {
                if action == "proceed" {
                    return;
                }
                if action == "discard_potion" {
                    self.drop_potion(params["slot"].int());
                    acted = true;
                    break;
                }
                if action == "claim_reward" {
                    let at = params["index"].as_i64().expect("an index") as usize;
                    taken.insert(at);
                    match items[at].0["type"].s() {
                        "gold" => self.gold += items[at].0["gold_amount"].int(),
                        "potion" => {
                            let p = items[at].1.clone().expect("a potion");
                            if !self.add_potion(p) {
                                continue;
                            }
                        }
                        "relic" => {
                            let rid = items[at].2.clone().expect("a relic");
                            self.gain_relic(&rid);
                        }
                        "card" => {
                            let offered = (g.card_choices)(self, kind);
                            self.card_reward(offered, true);
                        }
                        _ => {}
                    }
                    acted = true;
                    break;
                }
            }
            if !acted {
                return;
            }
        }
    }

    // ------------------------------------------------------------------ other rooms

    pub fn rest_site(&mut self) {
        let g = self.game.clone();
        let opts = (g.rest_options)(self);
        if opts.is_empty() {
            return;
        }
        let state = self.screen("rest_site", ("rest_site", json!({"options": opts, "can_proceed": false})));
        for (action, params) in strategy::rest_site(&state, &mut self.mem) {
            if action != "choose_rest_option" {
                continue;
            }
            let Some(o) = opts.iter().find(|o| o["index"] == params["index"]) else { continue };
            let what = o["id"].s();
            match what {
                "rest" => {
                    self.hp = self.max_hp.min(self.hp + base::trunc(self.max_hp as f64 * 0.3) + if self.has("REGAL_PILLOW") { 15 } else { 0 });
                    self.log_add("rests", 1);
                    if self.has("DREAM_CATCHER") {
                        let offered = (g.card_choices)(self, "monster");
                        self.card_reward(offered, true);
                    }
                }
                "smith" => {
                    if self.upgrade_one().is_none() {
                        continue;
                    }
                    self.log_add("smiths", 1);
                }
                "lift" => {
                    let n = self.counters.val("GIRYA") + 1;
                    self.counters.put("GIRYA", n);
                }
                "toke" => {
                    self.remove_one();
                }
                "dig" => {
                    let rid = (g.random_relic)(self, false);
                    self.gain_relic(&rid);
                }
                _ => {}
            }
            self.digest.push(format!("r{what}"));
            return;
        }
    }

    pub fn treasure(&mut self) {
        let g = self.game.clone();
        let rid = (g.random_relic)(self, true);
        self.gain_relic(&rid);
        if self.rng.random() < 0.5 {
            self.gold += self.rng.randint(20, 60);
        }
        if self.has("CURSED_KEY") {
            let curse = (g.random_curse)(self);
            self.deck.push(curse);
        }
    }

    pub fn event(&mut self) {
        self.log_add("events", 1);
        (self.game.event)(self);
    }

    pub fn shop(&mut self) {
        self.log_add("shops", 1);
        let mut stock: Vec<Item> = (self.game.shop_stock)(self);
        if self.has("MEMBERSHIP_CARD") {
            for it in stock.iter_mut() {
                let price = base::trunc(it.price() as f64 * 0.5);
                it.v.insert("price".to_string(), json!(price));
            }
        }
        for _ in 0..10 {
            let shown: Vec<Value> = stock
                .iter()
                .map(|it| {
                    let mut v = it.v.clone();
                    v.insert("can_afford".to_string(), json!(it.price() <= self.gold));
                    v.insert("is_stocked".to_string(), json!(true));
                    Value::Object(v)
                })
                .collect();
            let state = self.screen("shop", ("shop", json!({"items": shown, "can_proceed": true})));
            let mut bought = false;
            for (action, params) in strategy::shop(&state, &mut self.mem, "shop") {
                if action == "proceed" {
                    return;
                }
                if action != "shop_purchase" {
                    continue;
                }
                let Some(at) = stock.iter().position(|x| x.v["index"] == params["index"]) else { continue };
                let it = stock[at].clone();
                if it.price() > self.gold {
                    continue;
                }
                if it.category() == "potion" && !self.add_potion(it.potion.clone().expect("a potion")) {
                    continue;
                }
                match it.category() {
                    "card_removal" => {
                        if self.remove_one().is_none() {
                            continue;
                        }
                        let n = self.counters.val("removals") + 1;
                        self.counters.put("removals", n);
                    }
                    "card" => {
                        self.add_card(it.card.clone().expect("a card"));
                        let c = self.deck.last().expect("the card just added").clone();
                        self.mem.deck.push(c.view(ViewArgs::default()));
                        self.mem.deck_size += 1;
                    }
                    "relic" => self.gain_relic(it.relic.as_deref().expect("a relic")),
                    _ => {}
                }
                self.gold -= it.price();
                if self.has("MAW_BANK") {
                    self.relics.retain(|r| r != "MAW_BANK");
                }
                stock.remove(at);
                self.log_push("bought", json!(it.category()));
                self.digest.push(format!("B{}", it.category().chars().take(2).collect::<String>()));
                bought = true;
                break;
            }
            if !bought {
                return;
            }
        }
    }
}

/// `Run._upgrade_gain(card)`: what the card's upgrade adds, or None if the game has no such card.
pub fn _upgrade_gain(game: &Rc<Content>, card: &Value) -> Option<f64> {
    let d = game.CARDS.get(card["id"].text().as_str())?;
    Some(bot_cards::upgrade_gain(&Card::new(d, false).view(ViewArgs::default()), &Card::new(d, true).view(ViewArgs::default())))
}
