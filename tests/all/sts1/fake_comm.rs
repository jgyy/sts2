//! A tiny stand-in for the first Slay the Spire as CommunicationMod shows it. Like tests/all/fake_game.rs it is not the game: a
//! main menu, a short map (fight, event, shop, rest, chest, boss), fights against one monster, rewards and game over, in
//! CommunicationMod's message format and taking its commands ("start", "play", "end", "choose", "proceed", "skip", "confirm", ...).

use std::cell::RefCell;

use serde_json::{json, Value};
use sts2bot::py::{self, JsonExt};

use crate::fake_game::with;

thread_local! {
    /// Python's module-level `random` (not seeded): only the cards' "uuid" comes from it.
    static RANDOM: RefCell<py::Random> = RefCell::new(py::Random::unseeded());
}

/// (The Python's defaults: target False, rarity "BASIC", upgrades 0.)
pub fn card(cid: &str, name: &str, ctype: &str, cost: i64, target: bool, rarity: &str, upgrades: i64) -> Value {
    let uuid = format!("{cid}-{}", py::float_repr(RANDOM.with(|r| r.borrow_mut().random())));
    json!({"id": cid, "name": format!("{name}{}", if upgrades != 0 { "+" } else { "" }), "type": ctype, "cost": cost, "has_target": target,
           "rarity": rarity, "upgrades": upgrades, "uuid": uuid, "exhausts": false})
}

/// A card of the fake's deck: (id, name, type, cost, has a target).
pub type Card = (&'static str, &'static str, &'static str, i64, bool);

pub const STRIKE: Card = ("Strike_R", "Strike", "ATTACK", 1, true);
pub const DEFEND: Card = ("Defend_R", "Defend", "SKILL", 1, false);
pub const BASH: Card = ("Bash", "Bash", "ATTACK", 2, true);
pub const DAMAGE: [(&str, i64); 4] = [("Strike_R", 6), ("Bash", 8), ("Pommel Strike", 9), ("Cleave", 8)];
// The act: (x, y, symbol). One road, so every choice is the only one.
pub const ROAD: [(i64, i64, &str); 6] = [(0, 0, "M"), (0, 1, "?"), (0, 2, "$"), (0, 3, "R"), (0, 4, "T"), (0, 5, "E")];
// Commands that only lead from one screen to another (both of the event's options end it).
pub const MOVES: [((&str, &str), &str); 7] = [(("COMBAT_REWARD", "proceed"), "MAP"), (("CHEST", "proceed"), "MAP"), (("CARD_REWARD", "skip"), "COMBAT_REWARD"), (("EVENT", "choose"), "MAP"),
                                              (("SHOP_ROOM", "proceed"), "MAP"), (("SHOP_SCREEN", "leave"), "SHOP_ROOM"), (("REST", "proceed"), "MAP")];

const POMMEL_STRIKE: Card = ("Pommel Strike", "Pommel Strike", "ATTACK", 1, true);
const CLASH: Card = ("Clash", "Clash", "ATTACK", 0, true);

/// The monster of the fake's fights.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Monster {
    pub hp: i64,
    pub vuln: i64,
}

/// What `_do` raises and `command` answers as an error: Python's IndexError, ValueError and KeyError, as `f"{type(e).__name__}: {e}"`.
type Raised = String;

/// `int(s)`.
fn int(s: &str) -> Result<i64, Raised> {
    s.trim().parse::<i64>().map_err(|_| format!("ValueError: invalid literal for int() with base 10: {}", py::str_repr(s)))
}

/// `args[i]`.
fn arg<'a>(args: &[&'a str], i: usize) -> Result<&'a str, Raised> {
    args.get(i).copied().ok_or_else(|| "IndexError: list index out of range".to_string())
}

/// The place of `seq[i]` (a negative index counts from the end, as Python's does).
fn at(len: usize, i: i64) -> Result<usize, Raised> {
    let j = if i < 0 { i + len as i64 } else { i };
    if j < 0 || j >= len as i64 {
        return Err("IndexError: list index out of range".to_string());
    }
    Ok(j as usize)
}

pub struct FakeComm {
    pub rng: py::Random,
    pub monster_hp: i64,
    pub attack: i64,
    pub in_game: bool,
    pub sent: Vec<String>,
    pub runs: i64,
    pub cls: String,
    pub asc: i64,
    pub hp: i64,
    pub max_hp: i64,
    pub gold: i64,
    pub y: i64,
    pub screen: String,
    pub room: String,
    pub deck: Vec<Card>,
    pub potions: Vec<Value>,
    pub victory: bool,
    pub rested: bool,
    pub chest_open: bool,
    pub picked: Vec<Card>,
    pub grid: String,
    /// (None until the first fight or room: `getattr(self, "phase", "COMPLETE")`)
    pub phase: Option<String>,
    pub monster: Monster,
    pub max_monster: i64,
    pub draw: Vec<Card>,
    pub discard: Vec<Card>,
    pub turn: i64,
    pub energy: i64,
    pub block: i64,
    pub hand: Vec<Card>,
    pub rewards: Vec<Value>,
    /// (`getattr(self, "intent_pending", 0)`: a test sets it)
    pub intent_pending: i64,
}

impl Default for FakeComm {
    fn default() -> Self {
        FakeComm::new(0, 20, 7)
    }
}

impl FakeComm {
    /// `FakeComm(seed=0, monster_hp=20, attack=7)`.
    pub fn new(seed: i64, monster_hp: i64, attack: i64) -> Self {
        FakeComm {
            rng: py::Random::new(seed as i128),
            monster_hp,
            attack,
            in_game: false,
            sent: Vec::new(),
            runs: 0,
            cls: String::new(),
            asc: 0,
            hp: 0,
            max_hp: 0,
            gold: 0,
            y: 0,
            screen: String::new(),
            room: String::new(),
            deck: Vec::new(),
            potions: Vec::new(),
            victory: false,
            rested: false,
            chest_open: false,
            picked: Vec::new(),
            grid: String::new(),
            phase: None,
            monster: Monster::default(),
            max_monster: 0,
            draw: Vec::new(),
            discard: Vec::new(),
            turn: 0,
            energy: 0,
            block: 0,
            hand: Vec::new(),
            rewards: Vec::new(),
            intent_pending: 0,
        }
    }

    /// `FakeComm(seed=seed)`.
    pub fn seeded(seed: i64) -> Self {
        FakeComm::new(seed, 20, 7)
    }

    pub fn _start(&mut self, cls: &str, asc: i64) {
        (self.in_game, self.cls, self.asc) = (true, cls.to_string(), asc);
        self.runs += 1;
        self.max_hp = 80;
        self.hp = 80;
        (self.gold, self.y, self.screen, self.room) = (99, -1, "MAP".to_string(), "NeowRoom".to_string());
        self.deck = std::iter::repeat_n(STRIKE, 5).chain(std::iter::repeat_n(DEFEND, 4)).chain([BASH]).collect();
        self.potions = vec![json!({"id": "Fire Potion", "name": "Fire Potion", "can_use": false, "can_discard": true, "requires_target": true}),
                            json!({"id": "Potion Slot", "name": "Potion Slot"}), json!({"id": "Potion Slot", "name": "Potion Slot"})];
        (self.victory, self.rested, self.chest_open, self.picked, self.grid) = (false, false, false, Vec::new(), String::new());
    }

    pub fn _fight(&mut self, room: &str) {
        (self.room, self.screen, self.phase) = (room.to_string(), "NONE".to_string(), Some("COMBAT".to_string()));
        self.monster = Monster { hp: self.monster_hp * (if room == "MonsterRoomBoss" { 3 } else { 1 }), vuln: 0 };
        self.max_monster = self.monster.hp;
        self.draw = self.deck.clone();
        self.rng.shuffle(&mut self.draw);
        (self.discard, self.turn) = (Vec::new(), 0);
        for p in self.potions.iter_mut() {
            p["can_use"] = json!(p["id"] != "Potion Slot");
        }
        self._turn();
    }

    pub fn _turn(&mut self) {
        self.turn += 1;
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

    pub fn _won_fight(&mut self) {
        for p in self.potions.iter_mut() {
            p["can_use"] = json!(false);
        }
        if self.room == "MonsterRoomBoss" {
            (self.screen, self.victory) = ("GAME_OVER".to_string(), true);
            return;
        }
        (self.screen, self.phase) = ("COMBAT_REWARD".to_string(), Some("COMPLETE".to_string()));
        self.rewards = vec![json!({"reward_type": "GOLD", "gold": 15}), json!({"reward_type": "CARD"})];
    }

    pub fn commands(&self) -> Vec<&'static str> {
        if !self.in_game {
            return vec!["start", "state"];
        }
        let s = self.screen.as_str();
        if s == "NONE" {
            return if self.monster.hp > 0 { vec!["play", "end", "potion", "wait", "state"] } else { vec!["state"] };
        }
        match s {
            "MAP" => vec!["choose", "state"],
            "COMBAT_REWARD" => vec!["choose", "proceed", "state"],
            "CARD_REWARD" => vec!["choose", "skip", "state"],
            "EVENT" => vec!["choose", "state"],
            "SHOP_ROOM" => vec!["choose", "proceed", "state"],
            "SHOP_SCREEN" => vec!["choose", "leave", "state"],
            "REST" => {
                if self.rested {
                    vec!["choose", "proceed", "state"]
                } else {
                    vec!["choose", "state"]
                }
            }
            "GRID" => vec!["choose", "confirm", "state"],
            "CHEST" => vec!["choose", "proceed", "state"],
            "GAME_OVER" => vec!["proceed", "state"],
            other => panic!("KeyError: {}", py::str_repr(other)),
        }
    }

    /// (The Python's default: upgrades 0.)
    pub fn _card(&self, c: &Card, upgrades: i64) -> Value {
        card(c.0, c.1, c.2, c.3, c.4, "BASIC", upgrades)
    }

    fn _cards(&self, cards: &[Card]) -> Vec<Value> {
        cards.iter().map(|c| self._card(c, 0)).collect()
    }

    pub fn message(&self) -> Value {
        let mut msg = json!({"available_commands": self.commands(), "ready_for_command": true, "in_game": self.in_game});
        if !self.in_game {
            return msg;
        }
        let road: Vec<Value> = ROAD
            .iter()
            .map(|(x, y, s)| json!({"x": x, "y": y, "symbol": s, "children": if *y < 5 { json!([{"x": 0, "y": y + 1}]) } else { json!([{"x": 3, "y": 7}]) }, "parents": []}))
            .collect();
        let mut g = json!({
            "screen_type": self.screen, "room_type": self.room, "room_phase": self.phase.as_deref().unwrap_or("COMPLETE"), "current_hp": self.hp,
            "max_hp": self.max_hp, "gold": self.gold, "floor": self.y + 1, "act": 1, "class": self.cls, "ascension_level": self.asc,
            "seed": 42 + self.runs, "relics": [{"id": "Burning Blood", "name": "Burning Blood", "counter": -1}], "potions": self.potions,
            "deck": self._cards(&self.deck),
            "map": road,
        });
        let s = self.screen.as_str();
        if s == "NONE" {
            let pending = self.intent_pending > 0; // (the real game's first message of a fight comes before the intents are set)
            let hand: Vec<Value> = self.hand.iter().map(|c| with(self._card(c, 0), json!({"is_playable": c.3 <= self.energy}))).collect();
            let draw_pile = self._cards(&self.draw);
            let discard_pile = self._cards(&self.discard);
            g["combat_state"] = json!({
                "turn": self.turn, "hand": hand,
                "draw_pile": draw_pile, "discard_pile": discard_pile, "exhaust_pile": [],
                "player": {"current_hp": self.hp, "max_hp": self.max_hp, "block": self.block, "energy": self.energy, "powers": [], "orbs": []},
                "monsters": [{"id": "JawWorm", "name": "Jaw Worm", "current_hp": self.monster.hp, "max_hp": self.max_monster, "block": 0,
                              "intent": if pending { "DEBUG" } else { "ATTACK" }, "move_adjusted_damage": if pending { -1 } else { self.attack }, "move_hits": 1,
                              "half_dead": false, "is_gone": false,
                              "powers": if self.monster.vuln != 0 { json!([{"id": "Vulnerable", "name": "Vulnerable", "amount": self.monster.vuln}]) } else { json!([]) }}],
            });
        } else if s == "MAP" {
            let nxt = if self.y >= 5 { json!([]) } else { json!([{"x": 0, "y": self.y + 1, "symbol": ROAD[(self.y + 1) as usize].2}]) };
            g["screen_state"] = json!({"next_nodes": nxt, "boss_available": self.y >= 5, "current_node": {"x": 0, "y": self.y}});
            g["choice_list"] = if self.y >= 5 { json!(["boss"]) } else { json!(["x=0"]) };
        } else if s == "COMBAT_REWARD" {
            g["screen_state"] = json!({"rewards": self.rewards});
        } else if s == "CARD_REWARD" {
            let pommel = with(self._card(&POMMEL_STRIKE, 0), json!({"rarity": "COMMON"}));
            let clash = with(self._card(&CLASH, 0), json!({"rarity": "COMMON"}));
            g["screen_state"] = json!({"cards": [pommel, clash], "skip_available": true});
        } else if s == "EVENT" {
            g["screen_state"] = json!({"event_name": "Golden Idol", "event_id": "Golden Idol", "body_text": "An idol.",
                                       "options": [{"text": "[Take] #gObtain #gGolden #gIdol. #rTrigger #ra #rtrap.", "label": "Take", "disabled": false, "choice_index": 0},
                                                   {"text": "[Leave]", "label": "Leave", "disabled": false, "choice_index": 1}]});
        } else if s == "SHOP_SCREEN" {
            let cleave = with(self._card(&("Cleave", "Cleave", "ATTACK", 1, false), 0), json!({"price": 50, "rarity": "COMMON"}));
            g["screen_state"] = json!({"cards": [cleave],
                                       "relics": [{"id": "Anchor", "name": "Anchor", "price": 150}], "potions": [], "purge_available": true, "purge_cost": 75});
        } else if s == "REST" {
            g["screen_state"] = json!({"rest_options": if self.rested { json!([]) } else { json!(["rest", "smith"]) }, "has_rested": self.rested});
        } else if s == "GRID" {
            let cards = self._cards(&self.deck);
            let selected = self._cards(&self.picked);
            g["screen_state"] = json!({"cards": cards, "selected_cards": selected,
                                       "num_cards": 1, "any_number": false, "confirm_up": !self.picked.is_empty(), "for_upgrade": self.grid == "upgrade",
                                       "for_purge": self.grid == "purge", "for_transform": false});
        } else if s == "CHEST" {
            g["screen_state"] = json!({"chest_type": "SmallChest", "chest_open": self.chest_open});
        } else if s == "GAME_OVER" {
            g["screen_state"] = json!({"victory": self.victory, "score": 100});
        }
        msg["game_state"] = g;
        msg
    }

    /// Carry out one command and answer like CommunicationMod: the new state, or an error.
    pub fn command(&mut self, line: &str) -> Value {
        self.sent.push(line.to_string());
        let words: Vec<&str> = line.split_whitespace().collect();
        let verb = words.first().copied().unwrap_or("");
        if !self.commands().contains(&verb) {
            return json!({"error": format!("Invalid command: {line}"), "ready_for_command": true});
        }
        let ok = match self._do(verb, &words[1..]) {
            Ok(ok) => ok,
            Err(e) => return json!({"error": e, "ready_for_command": true}),
        };
        if !ok {
            return json!({"error": format!("Invalid command: {line}"), "ready_for_command": true});
        }
        self.message()
    }

    pub fn _do(&mut self, verb: &str, args: &[&str]) -> Result<bool, Raised> {
        let s = if self.in_game { self.screen.clone() } else { "MENU".to_string() };
        let s = s.as_str();
        if verb == "state" {
            return Ok(true);
        }
        if verb == "wait" {
            self.intent_pending = (self.intent_pending - 1).max(0);
            return Ok(true);
        }
        if verb == "start" {
            let cls = arg(args, 0)?.to_uppercase();
            let asc = if args.len() > 1 { int(args[1])? } else { 0 };
            self._start(&cls, asc);
            return Ok(true);
        }
        if let Some((_, to)) = MOVES.iter().find(|(from, _)| *from == (s, verb)) {
            self.screen = to.to_string();
            return Ok(true);
        }
        if s == "NONE" && verb == "play" {
            let i = at(self.hand.len(), int(arg(args, 0)?)? - 1)?;
            let c = self.hand[i];
            if c.3 > self.energy || (c.4 && args.len() < 2) {
                return Ok(false);
            }
            let first = self.hand.iter().position(|h| *h == c).unwrap();
            self.hand.remove(first);
            self.discard.push(c);
            self.energy -= c.3;
            if c.2 == "ATTACK" {
                let damage = DAMAGE.iter().find(|(k, _)| *k == c.0).map_or(6, |(_, d)| *d);
                self.monster.hp -= if self.monster.vuln != 0 { py::trunc(damage as f64 * 1.5) } else { damage };
                if c.0 == "Bash" {
                    self.monster.vuln += 2;
                }
            } else {
                self.block += 5;
            }
            if self.monster.hp <= 0 {
                self._won_fight();
            }
            return Ok(true);
        }
        if s == "NONE" && verb == "end" {
            self.hp -= (self.attack - self.block).max(0);
            self.monster.vuln = (self.monster.vuln - 1).max(0);
            self.discard.extend(self.hand.iter().copied());
            if self.hp <= 0 {
                (self.hp, self.screen) = (0, "GAME_OVER".to_string());
            } else {
                self._turn();
            }
            return Ok(true);
        }
        if s == "NONE" && verb == "potion" {
            let slot = int(arg(args, 1)?)?;
            if args[0] != "use" {
                return Ok(false);
            }
            let slot = at(self.potions.len(), slot)?;
            if self.potions[slot]["id"] == "Potion Slot" || args.len() < 3 {
                return Ok(false);
            }
            self.potions[slot] = json!({"id": "Potion Slot", "name": "Potion Slot"});
            self.monster.hp -= 20;
            if self.monster.hp <= 0 {
                self._won_fight();
            }
            return Ok(true);
        }
        if s == "MAP" && verb == "choose" {
            self.y += 1;
            let kind = if self.y > 5 { "B" } else { ROAD[self.y as usize].2 };
            if "MEB".contains(kind) {
                self._fight(match kind {
                    "M" => "MonsterRoom",
                    "E" => "MonsterRoomElite",
                    _ => "MonsterRoomBoss",
                });
            } else {
                self.phase = Some("EVENT".to_string());
                let (room, screen) = match kind {
                    "?" => ("EventRoom", "EVENT"),
                    "$" => ("ShopRoom", "SHOP_ROOM"),
                    "R" => ("RestRoom", "REST"),
                    "T" => ("TreasureRoom", "CHEST"),
                    other => return Err(format!("KeyError: {}", py::str_repr(other))),
                };
                (self.room, self.screen) = (room.to_string(), screen.to_string());
                (self.rested, self.chest_open, self.picked) = (false, false, Vec::new());
            }
            return Ok(true);
        }
        if s == "COMBAT_REWARD" && verb == "choose" {
            let i = at(self.rewards.len(), int(arg(args, 0)?)?)?;
            let item = self.rewards[i].clone();
            if item["reward_type"] == "CARD" {
                self.screen = "CARD_REWARD".to_string();
            } else {
                self.gold += item.get("gold").map_or(0, |g| g.int());
                let first = self.rewards.iter().position(|r| *r == item).unwrap();
                self.rewards.remove(first);
            }
            return Ok(true);
        }
        if s == "CARD_REWARD" && verb == "choose" {
            self.deck.push(if arg(args, 0)? == "0" { POMMEL_STRIKE } else { CLASH });
            self.rewards.retain(|r| r["reward_type"] != "CARD");
            self.screen = "COMBAT_REWARD".to_string();
            return Ok(true);
        }
        if s == "SHOP_ROOM" && verb == "choose" && args == ["shop"] {
            self.screen = "SHOP_SCREEN".to_string();
            return Ok(true);
        }
        if s == "SHOP_SCREEN" && verb == "choose" {
            if args.join(" ") == "purge" && self.gold >= 75 {
                self.gold -= 75;
                (self.screen, self.grid, self.picked) = ("GRID".to_string(), "purge".to_string(), Vec::new());
                return Ok(true);
            }
            return Ok(false);
        }
        if s == "REST" && verb == "choose" && !self.rested {
            if int(arg(args, 0)?)? == 0 {
                (self.hp, self.rested) = (self.max_hp.min(self.hp + 24), true);
            } else {
                (self.screen, self.grid, self.picked) = ("GRID".to_string(), "upgrade".to_string(), Vec::new());
            }
            return Ok(true);
        }
        if s == "GRID" && verb == "choose" {
            let i = at(self.deck.len(), int(arg(args, 0)?)?)?;
            self.picked = vec![self.deck[i]];
            return Ok(true);
        }
        if s == "GRID" && verb == "confirm" && !self.picked.is_empty() {
            if self.grid == "purge" {
                let first = self.deck.iter().position(|c| *c == self.picked[0]).ok_or_else(|| "ValueError: list.remove(x): x not in list".to_string())?;
                self.deck.remove(first);
                self.screen = "SHOP_SCREEN".to_string();
            } else {
                (self.screen, self.rested) = ("REST".to_string(), true);
            }
            return Ok(true);
        }
        if s == "CHEST" && verb == "choose" {
            self.chest_open = true;
            (self.screen, self.rewards) = ("COMBAT_REWARD".to_string(), vec![json!({"reward_type": "RELIC", "relic": {"id": "Anchor", "name": "Anchor", "counter": -1}})]);
            return Ok(true);
        }
        if s == "GAME_OVER" && verb == "proceed" {
            self.in_game = false;
            return Ok(true);
        }
        Ok(false)
    }
}

/// Without the cards' uuids (Python's unseeded `random.random()`).
fn scrub(x: &Value) -> Value {
    match x {
        Value::Object(d) => Value::Object(d.iter().filter(|(k, _)| *k != "uuid").map(|(k, v)| (k.clone(), scrub(v))).collect()),
        Value::Array(items) => Value::Array(items.iter().map(scrub).collect()),
        other => other.clone(),
    }
}

/// The fake against tests/sts1/fake_comm.py (tests/golden/sts1_fake_comm.json): its answers to a list of commands, errors too
/// (as Python writes them, keys in order), and every command the bot sent it over whole runs, won and lost.
#[test]
fn the_fake_and_the_bot_over_it_are_the_pythons() {
    use std::rc::Rc;
    use sts2bot::bot::BotArgs;
    use sts2bot::games::sts1::client::Prefs;

    let _case = super::helpers::sts1_case();
    let golden: Value = serde_json::from_str(include_str!("../../golden/sts1_fake_comm.json")).unwrap();
    let mut game = FakeComm::seeded(9);
    for (n, step) in golden["commands"].list().iter().enumerate() {
        let answer = scrub(&game.command(step[0].s()));
        assert_eq!(py::json_dumps(&answer), py::json_dumps(&step[1]), "command {n}: {}", step[0]);
    }
    for run in golden["runs"].list() {
        let game = Rc::new(RefCell::new(FakeComm::new(run["seed"].int(), run["monster_hp"].int(), run["attack"].int())));
        let who = run["character"].s();
        let client = super::helpers::direct_with(&game, Prefs::Dict(json!({"max_ascension": {who: run["ascension"]}})));
        let mut bot = crate::helpers::make_bot_kw(Rc::new(RefCell::new(client)), BotArgs { character: who.to_string(), ..Default::default() });
        bot.run(Some(run["max_runs"].int())).unwrap();
        let sent: Vec<&str> = run["sent"].list().iter().map(|s| s.s()).collect();
        assert_eq!(game.borrow().sent, sent, "seed {}", run["seed"]);
        assert_eq!(bot.wins, run["wins"].int(), "seed {}", run["seed"]);
    }
}
