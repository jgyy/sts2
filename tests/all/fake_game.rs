//! A tiny stand-in for the game that speaks the bridge mod's state/action shapes. It is not Slay the Spire: just enough
//! screens (menu, map, combat, rewards, card reward, rest site, card select, game over) to drive the bot loop for whole runs.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{json, Value};
use sts2bot::client::{Game, GameUnavailable};
use sts2bot::py::{self, JsonExt};

/// A card as the game sends it. (The Python's defaults: target "None", rarity "Common"; its `**extra` and another rarity: `with`.)
pub fn card(index: i64, cid: &str, name: &str, ctype: &str, cost: impl ToString, desc: &str, target: &str) -> Value {
    json!({"index": index, "id": cid, "name": name, "type": ctype, "cost": cost.to_string(), "star_cost": null, "description": desc,
           "target_type": target, "can_play": true, "is_upgraded": false, "rarity": "Common"})
}

/// `{**base, **extra}`: `base` with the keys of `extra` set (new ones at the end).
pub fn with(mut base: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().expect("a dict of keys to set") {
        base[k.as_str()] = v.clone();
    }
    base
}

/// (id, name, type, cost, description, target)
pub const STRIKE: (&str, &str, &str, i64, &str, &str) = ("STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy");
pub const DEFEND: (&str, &str, &str, i64, &str, &str) = ("DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self");
pub const BASH: (&str, &str, &str, i64, &str, &str) = ("BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy");

/// A card of the fake game's deck: (id, name, type, cost, description, target).
pub type Card = (String, String, String, i64, String, String);

fn owned(c: (&str, &str, &str, i64, &str, &str)) -> Card {
    (c.0.to_string(), c.1.to_string(), c.2.to_string(), c.3, c.4.to_string(), c.5.to_string())
}

/// (`off`: the names that are not enabled.)
pub fn _options(names: &[&str], off: &[&str]) -> Value {
    Value::Array(names.iter().map(|n| json!({"name": n, "enabled": !off.contains(n)})).collect())
}

/// The enemy of the fake game's fights.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FakeEnemy {
    pub hp: i64,
    pub block: i64,
    pub vuln: i64,
}

/// The fake game. A test shares it with its bot (`Rc<RefCell<FakeGame>>`: `shared`) and reads afterwards what it was asked.
pub struct FakeGame {
    pub rng: py::Random,
    pub floors: i64,
    pub enemy_hp: i64,
    pub enemy_attack: i64,
    pub actions: Vec<(String, Value)>,
    pub runs_started: i64,
    pub screen: String,
    pub character: Option<String>,
    pub has_save: bool,
    /// (a test sets it: the screen "continue" goes back to)
    pub saved_screen: String,
    /// looks at the state so far (a test's `game.state = lambda: asked.append(1) or state()`)
    pub asked: i64,
    pub floor: i64,
    pub hp: i64,
    pub max_hp: i64,
    pub gold: i64,
    pub deck: Vec<Card>,
    pub round: i64,
    pub enemy: FakeEnemy,
    pub draw: Vec<Card>,
    pub discard: Vec<Card>,
    pub energy: i64,
    pub block: i64,
    pub hand: Vec<Card>,
    pub reward_items: Vec<Value>,
    pub rested: bool,
    pub upgrade_pick: Option<i64>,
}

impl Default for FakeGame {
    fn default() -> Self {
        FakeGame::new(0, 4, 30, 9)
    }
}

impl FakeGame {
    /// `FakeGame(seed=0, floors=4, enemy_hp=30, enemy_attack=9)`.
    pub fn new(seed: i64, floors: i64, enemy_hp: i64, enemy_attack: i64) -> Self {
        FakeGame {
            rng: py::Random::new(seed as i128),
            floors,
            enemy_hp,
            enemy_attack,
            actions: Vec::new(),
            runs_started: 0,
            screen: "main".to_string(),
            character: None,
            has_save: false,
            saved_screen: String::new(),
            asked: 0,
            floor: 0,
            hp: 0,
            max_hp: 0,
            gold: 0,
            deck: Vec::new(),
            round: 0,
            enemy: FakeEnemy::default(),
            draw: Vec::new(),
            discard: Vec::new(),
            energy: 0,
            block: 0,
            hand: Vec::new(),
            reward_items: Vec::new(),
            rested: false,
            upgrade_pick: None,
        }
    }

    /// `FakeGame(seed=seed, floors=floors)`.
    pub fn seeded(seed: i64, floors: i64) -> Self {
        FakeGame::new(seed, floors, 30, 9)
    }

    /// The game as a test and its bot share it.
    pub fn shared(self) -> Rc<RefCell<FakeGame>> {
        Rc::new(RefCell::new(self))
    }

    pub fn _new_run(&mut self) {
        self.runs_started += 1;
        (self.floor, self.hp, self.max_hp, self.gold) = (0, 80, 80, 99);
        self.deck = std::iter::repeat_n(owned(STRIKE), 5).chain(std::iter::repeat_n(owned(DEFEND), 4)).chain([owned(BASH)]).collect();
        (self.screen, self.has_save) = ("map".to_string(), true);
    }

    pub fn _start_combat(&mut self) {
        (self.screen, self.round) = ("combat".to_string(), 0);
        self.enemy = FakeEnemy { hp: self.enemy_hp, block: 0, vuln: 0 };
        self.draw = self.deck.clone();
        self.rng.shuffle(&mut self.draw);
        self.discard = Vec::new();
        self._start_turn();
    }

    pub fn _start_turn(&mut self) {
        self.round += 1;
        (self.energy, self.block, self.hand) = (3, 0, Vec::new());
        for _ in 0..5 {
            if self.draw.is_empty() {
                self.draw = std::mem::take(&mut self.discard);
                self.rng.shuffle(&mut self.draw);
            }
            if let Some(c) = self.draw.pop() {
                self.hand.push(c);
            }
        }
    }

    pub fn _player(&self, combat: bool) -> Value {
        let mut p = json!({"character": "The Ironclad", "hp": self.hp, "max_hp": self.max_hp, "block": self.block, "gold": self.gold,
                           "status": [], "relics": [{"id": "BURNING_BLOOD", "name": "Burning Blood"}], "potions": [], "max_potion_slots": 3});
        if combat {
            p["energy"] = json!(self.energy);
            p["max_energy"] = json!(3);
            p["hand"] = Value::Array(self.hand.iter().enumerate().map(|(i, c)| with(card(i as i64, &c.0, &c.1, &c.2, c.3, &c.4, &c.5), json!({"can_play": c.3 <= self.energy}))).collect());
            p["draw_pile_count"] = json!(self.draw.len());
            p["discard_pile_count"] = json!(self.discard.len());
            p["exhaust_pile_count"] = json!(0);
        }
        p
    }

    /// (`body`: the screen's own keys, as a dict.)
    pub fn _wrap(&self, kind: &str, body: Value) -> Value {
        with(json!({"state_type": kind, "run": {"act": 1, "floor": self.floor, "ascension": 0}, "player": self._player(kind == "monster" || kind == "boss")}), body)
    }

    pub fn state(&mut self) -> Value {
        self.asked += 1;
        self._state()
    }

    fn _state(&self) -> Value {
        let s = self.screen.as_str();
        match s {
            "main" => {
                let opts: Vec<&str> = if self.has_save { vec!["continue", "abandon_run"] } else { vec![] }.into_iter().chain(["singleplayer", "settings", "quit"]).collect();
                json!({"state_type": "menu", "menu_screen": "main", "options": opts})
            }
            "singleplayer" => json!({"state_type": "menu", "menu_screen": s, "options": _options(&["standard", "back"], &[])}),
            "popup" => json!({"state_type": "menu", "menu_screen": s, "options": _options(&["yes", "no"], &[])}),
            "character_select" => {
                let chars = json!([{"id": "IRONCLAD", "name": "The Ironclad", "locked": false}, {"id": "SILENT", "name": "The Silent", "locked": true}]);
                let off: &[&str] = if self.character.is_some() { &["SILENT"] } else { &["SILENT", "confirm"] };
                json!({"state_type": "menu", "menu_screen": s, "characters": chars, "options": _options(&["IRONCLAD", "SILENT", "confirm", "back"], off)})
            }
            "map" => {
                let kinds = if self.floor % 2 != 0 { ["Monster", "RestSite"] } else { ["Monster", "Monster"] };
                let nxt: Vec<Value> = kinds.iter().enumerate().map(|(i, k)| json!({"index": i, "col": i, "row": self.floor + 1, "type": k, "leads_to": []})).collect();
                self._wrap("map", json!({"map": {"next_options": nxt, "nodes": []}}))
            }
            "combat" => {
                let e = &self.enemy;
                let status = if e.vuln != 0 { json!([{"id": "VULNERABLE_POWER", "amount": e.vuln}]) } else { json!([]) };
                let foe = json!({"entity_id": "JAW_WORM_0", "name": "Jaw Worm", "hp": e.hp, "max_hp": self.enemy_hp, "block": e.block,
                                 "status": status, "intents": [{"type": "Attack", "label": self.enemy_attack.to_string()}]});
                self._wrap("monster", json!({"battle": {"round": self.round, "turn": "player", "is_play_phase": true, "enemies": [foe]}}))
            }
            "rewards" => self._wrap("rewards", json!({"rewards": {"items": self.reward_items, "can_proceed": true}})),
            "card_reward" => {
                let offer = json!([with(card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage. Apply 1 Weak. Apply 1 Vulnerable.", "AnyEnemy"), json!({"rarity": "Uncommon"})),
                                   with(card(1, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy"), json!({"rarity": "Basic"}))]);
                self._wrap("card_reward", json!({"card_reward": {"cards": offer, "can_skip": true}}))
            }
            "rest" => {
                let opts = json!([{"index": 0, "id": "rest", "name": "Rest", "is_enabled": true}, {"index": 1, "id": "smith", "name": "Smith", "is_enabled": true}]);
                self._wrap("rest_site", json!({"rest_site": {"options": opts, "can_proceed": self.rested}}))
            }
            "upgrade" => {
                let picked = self.upgrade_pick.is_some();
                let cards: Vec<Value> = self.deck.iter().enumerate().map(|(i, c)| card(i as i64, &c.0, &c.1, &c.2, c.3, &c.4, &c.5)).collect();
                self._wrap("card_select", json!({"card_select": {"screen_type": "upgrade", "prompt": "Choose a card to Upgrade.", "cards": cards,
                                                                  "preview_showing": picked, "can_confirm": picked, "can_cancel": true}}))
            }
            "game_over" => self._wrap("game_over", json!({"game_over": {"options": ["main_menu"]}})),
            _ => panic!("AssertionError: {s}"),
        }
    }

    /// (`p`: the action's keyword arguments, as a dict.)
    pub fn act(&mut self, action: &str, p: &Value) -> Value {
        let p = if p.is_null() { json!({}) } else { p.clone() };
        self.actions.push((action.to_string(), p.clone()));
        match self._act(action, &p) {
            // (a KeyError, IndexError or ValueError of the Python's)
            Err(e) => json!({"status": "error", "error": e}),
            Ok(None) => json!({"status": "error", "error": format!("{} not valid on {}", action, self.screen)}),
            Ok(Some(msg)) => json!({"status": "ok", "message": msg}),
        }
    }

    pub fn _act(&mut self, action: &str, p: &Value) -> Result<Option<String>, String> {
        // `p[key]`, and the place in a list of `len` it names
        let arg = |key: &str| -> Result<&Value, String> { p.get(key).ok_or_else(|| format!("KeyError: {}", py::str_repr(key))) };
        let at = |key: &str, len: usize| -> Result<usize, String> {
            let i = arg(key)?.int();
            let i = if i < 0 { i + len as i64 } else { i };
            if (0..len as i64).contains(&i) {
                Ok(i as usize)
            } else {
                Err("IndexError: list index out of range".to_string())
            }
        };
        let s = self.screen.clone();
        let s = s.as_str();
        if action == "menu_select" {
            let opt = py::str_of(arg("option")?);
            match (s, opt.as_str()) {
                ("main", "singleplayer") => self.screen = "singleplayer".to_string(),
                ("main", "continue") if self.has_save => self.screen = self.saved_screen.clone(),
                ("main", "abandon_run") if self.has_save => self.screen = "popup".to_string(),
                ("popup", "yes") => (self.has_save, self.screen) = (false, "main".to_string()),
                ("singleplayer", "standard") => (self.screen, self.character) = ("character_select".to_string(), None),
                ("character_select", "IRONCLAD") => self.character = Some(opt.clone()),
                ("character_select", "confirm") if self.character.is_some() => self._new_run(),
                ("game_over", "main_menu") => (self.screen, self.has_save) = ("main".to_string(), false),
                _ => return Ok(None),
            }
            return Ok(Some(opt));
        }
        if s == "map" && action == "choose_map_node" {
            let options = self.state()["map"]["next_options"].clone();
            let kind = py::str_of(&options[at("index", options.list().len())?]["type"]);
            self.floor += 1;
            if kind == "RestSite" {
                (self.screen, self.rested) = ("rest".to_string(), false);
            } else {
                self._start_combat();
            }
            return Ok(Some(kind));
        }
        if s == "combat" && action == "play_card" {
            let index = at("card_index", self.hand.len())?;
            let c = self.hand[index].clone();
            if c.3 > self.energy {
                return Err("ValueError: NotEnoughEnergy".to_string());
            }
            if c.5 == "AnyEnemy" && p["target"] != "JAW_WORM_0" {
                return Err("ValueError: Card requires a target".to_string());
            }
            self.hand.remove(index);
            self.discard.push(c.clone());
            self.energy -= c.3;
            if c.2 == "Attack" {
                let dmg: i64 = if c.0 == "UPPERCUT" { 13 } else { c.4.split_whitespace().nth(1).and_then(|n| n.parse().ok()).expect("a damage number") };
                self.enemy.hp -= if self.enemy.vuln != 0 { py::trunc(dmg as f64 * 1.5) } else { dmg };
                if c.0 == "BASH" || c.0 == "UPPERCUT" {
                    self.enemy.vuln += 2;
                }
            } else {
                self.block += 5;
            }
            if self.enemy.hp <= 0 {
                self.reward_items = vec![json!({"index": 0, "type": "gold", "gold_amount": 20}), json!({"index": 1, "type": "card"})];
                self.screen = if self.floor >= self.floors { "game_over" } else { "rewards" }.to_string();
            }
            return Ok(Some(c.1));
        }
        if s == "combat" && action == "end_turn" {
            self.hp -= 0.max(self.enemy_attack - self.block);
            self.enemy.vuln = 0.max(self.enemy.vuln - 1);
            let hand = self.hand.clone(); // (the hand stays as it is until the next turn deals another)
            self.discard.extend(hand);
            if self.hp <= 0 {
                (self.hp, self.screen) = (0, "game_over".to_string());
            } else {
                self._start_turn();
            }
            return Ok(Some("end".to_string()));
        }
        if s == "rewards" && action == "claim_reward" {
            let index = arg("index")?;
            let at = self.reward_items.iter().position(|i| &i["index"] == index).expect("StopIteration");
            let item = self.reward_items[at].clone();
            if item["type"] == "gold" {
                self.gold += item["gold_amount"].int();
                self.reward_items.remove(at);
            } else {
                self.screen = "card_reward".to_string();
            }
            return Ok(Some(py::str_of(&item["type"])));
        }
        if action == "proceed" && (s == "rewards" || s == "rest" && self.rested) {
            self.screen = "map".to_string();
            return Ok(Some("map".to_string()));
        }
        if s == "card_reward" && action == "select_card_reward" {
            let cards = self.state()["card_reward"]["cards"].clone();
            let picked = &cards[at("card_index", cards.list().len())?];
            let t = |k: &str| py::str_of(&picked[k]);
            self.deck.push((t("id"), t("name"), t("type"), picked["cost"].int(), t("description"), t("target_type")));
            self.reward_items.retain(|i| i["type"] != "card");
            self.screen = "rewards".to_string();
            return Ok(Some(t("name")));
        }
        if s == "card_reward" && action == "skip_card_reward" {
            self.screen = "rewards".to_string(); // the card reward stays on the list, like in the game
            return Ok(Some("skip".to_string()));
        }
        if s == "rest" && action == "choose_rest_option" && !self.rested {
            if arg("index")? == 0 {
                (self.hp, self.rested) = (self.max_hp.min(self.hp + 24), true);
            } else {
                (self.screen, self.upgrade_pick) = ("upgrade".to_string(), None);
            }
            return Ok(Some("rest".to_string()));
        }
        if s == "upgrade" && action == "select_card" {
            self.upgrade_pick = Some(arg("index")?.int());
            return Ok(Some("picked".to_string()));
        }
        if s == "upgrade" && action == "confirm_selection" && self.upgrade_pick.is_some() {
            (self.screen, self.rested) = ("rest".to_string(), true);
            return Ok(Some("upgraded".to_string()));
        }
        Ok(None)
    }

    /// (the Client interface the bot uses)
    pub fn ping(&mut self) -> bool {
        true
    }
}

/// `impl Game` for a test's game that has `state(&mut self) -> Value` and `act(&mut self, action: &str, params: &Value) -> Value`
/// of its own and, like the Python's fakes, no `profile` or `card` (asked for the profile, it fails as the missing attribute did;
/// it knows no card; `ping` is true).
macro_rules! game_impl {
    ($ty:ty) => {
        impl sts2bot::client::Game for $ty {
            fn state(&mut self) -> Result<serde_json::Value, sts2bot::client::GameUnavailable> {
                Ok(<$ty>::state(self))
            }
            fn act(&mut self, action: &str, params: &serde_json::Value) -> Result<serde_json::Value, sts2bot::client::GameUnavailable> {
                Ok(<$ty>::act(self, action, params))
            }
            fn profile(&mut self) -> Result<serde_json::Value, sts2bot::client::GameUnavailable> {
                Err(sts2bot::client::GameUnavailable::new(concat!("'", stringify!($ty), "' object has no attribute 'profile'")))
            }
            fn card(&mut self, _card_id: &str) -> Result<Option<serde_json::Value>, sts2bot::client::GameUnavailable> {
                Ok(None)
            }
            fn ping(&mut self) -> bool {
                true
            }
        }
    };
}
pub(crate) use game_impl;

game_impl!(FakeGame);

/// `Bot(None)`: no game at all (a test that only feeds the bot states).
pub struct NoGame;

impl Game for NoGame {
    fn state(&mut self) -> Result<Value, GameUnavailable> {
        panic!("'NoneType' object has no attribute 'state'")
    }
    fn act(&mut self, _action: &str, _params: &Value) -> Result<Value, GameUnavailable> {
        panic!("'NoneType' object has no attribute 'act'")
    }
    fn profile(&mut self) -> Result<Value, GameUnavailable> {
        Err(GameUnavailable::new("'NoneType' object has no attribute 'profile'"))
    }
    fn card(&mut self, _card_id: &str) -> Result<Option<Value>, GameUnavailable> {
        Ok(None)
    }
    fn ping(&mut self) -> bool {
        false
    }
}
