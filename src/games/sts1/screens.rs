//! The first game's state, as CommunicationMod sends it, in the words of the STS2_MCP mod the bot was written against
//! (`state_type`, the player, the run, the screen's block), and what the bot's actions need to be sent back as commands. The
//! menu is the bot's own (CommunicationMod only has "start CLASS ASCENSION").

use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use super::{keys, text};
use crate::cards::key_of;
use crate::base::{self, JsonExt};

pub const CHARACTERS: [&str; 4] = ["IRONCLAD", "THE_SILENT", "DEFECT", "WATCHER"];
pub const CHARACTER_NAMES: &[(&str, &str)] = &[("IRONCLAD", "The Ironclad"), ("THE_SILENT", "The Silent"), ("DEFECT", "The Defect"), ("WATCHER", "The Watcher")];

pub const ROOM_KIND: &[(&str, &str)] = &[("MonsterRoom", "monster"), ("MonsterRoomElite", "elite"), ("MonsterRoomBoss", "boss")];
pub const MAP_SYMBOL: &[(&str, &str)] = &[("M", "Monster"), ("E", "Elite"), ("R", "RestSite"), ("$", "Shop"), ("?", "Unknown"), ("T", "Treasure"), ("B", "Boss")];
pub const CARD_TYPE: &[(&str, &str)] = &[("ATTACK", "Attack"), ("SKILL", "Skill"), ("POWER", "Power"), ("STATUS", "Status"), ("CURSE", "Curse")];
pub const RARITY: &[(&str, &str)] = &[("BASIC", "Basic"), ("COMMON", "Common"), ("UNCOMMON", "Uncommon"), ("RARE", "Rare"), ("SPECIAL", "Event"), ("CURSE", "Curse")];
/// The first game's power ids where the planner knows the same thing by another name.
#[rustfmt::skip]
pub const POWER_ID: &[(&str, &str)] = &[
    ("WEAKENED", "WEAK"), ("PLATED_ARMOR", "PLATING"), ("INTANGIBLEPLAYER", "INTANGIBLE"), ("REGENERATION", "REGEN"), ("REGENERATE", "REGEN"), ("CONSTRICTED", "CONSTRICT"),
    ("DRAW_CARD", "DRAW_CARDS_NEXT_TURN"), ("ENERGIZED", "ENERGY_NEXT_TURN"), ("ENERGIZEDBLUE", "ENERGY_NEXT_TURN"), ("LOCKON", "LOCK_ON"), ("SURROUNDED", "SURROUNDED"), ("MINION", "MINION"),
    ("ANGER", "ENRAGE"),  // the Gremlin Nob's Enrage: Strength for each Skill played
    ("GENERIC_STRENGTH_UP", "RITUAL"),  // the Orb Walker's 3 Strength at the end of each of its turns
    ("SHARP_HIDE", "THORNS"),  // the Guardian's 3 damage back for each Attack played (Thorns counts it a hit, close enough)
    ("FLIGHT", "FLUTTER"),  // the Byrd's: half damage from Attacks until it has been hit enough times
    ("NOBLOCKPOWER", "NO_BLOCK"),  // Panic Button's: no Block from cards for a while
    ("HEX", "HEX_DAZED"),  // the Chosen's (a Dazed for each non-Attack played), not StS2's Hex (all cards Ethereal)
    ("ENERGYDOWNPOWER", "ENERGY_DOWN"),  // Fasting's price: an energy less at the start of every turn, one a stack
    ("DUPLICATIONPOWER", "DUPLICATION"),  // the Duplication Potion's "your next card is played twice this turn"
    ("COMPULSIVE", "REACTIVE"),  // the Writhing Mass's Reactive: every Attack that hits it gives it a new intent
    ("UNAWAKENED", "STOCK"),  // the Awakened One's first form: killed, it rises again at full HP
    ("LIFE_LINK", "REATTACH"),  // the Darklings' (revive in 2 turns at 50% HP while others live): StS2's Reattach, whose number is the HP it comes back on
    ("INVINCIBLE", "HARDENED_SHELL"),  // the Heart's "can only lose 300 HP per turn", counted down as it is hit: the Skulking Colony's rule
    ("BEATOFDEATH", "BEAT_OF_DEATH"),  // the Heart's: every card we play deals us 1 damage once it has resolved (Block takes it)
    ("BACKATTACK", "BACK_ATTACK"),  // the Spire Shield's and Spear's mark, never taken off when we turn: who is behind comes from the mod (combat)
];
pub const STANCE_CHOICE: &[(&str, &str)] = &[("WRATH", "Enter Wrath."), ("CALM", "Enter Calm.")]; // the Stance Potion's choice cards (ChooseWrath, ChooseCalm), by name
pub const AMOUNT: &[(&str, i64)] = &[("UNAWAKENED", 1)]; // powers that show no number where the planner needs one
#[rustfmt::skip]
pub const INTENT_TYPE: &[(&str, &str)] = &[("BUFF", "Buff"), ("DEBUFF", "Debuff"), ("STRONG_DEBUFF", "Debuff"), ("DEFEND", "Defend"), ("ESCAPE", "Escape"),
               ("SLEEP", "Sleep"), ("STUN", "Stun"), ("MAGIC", "Buff"), ("UNKNOWN", "Unknown"), ("NONE", "Unknown"), ("DEBUG", "Unknown")];
/// What a hand selection is for, by the action that asked for it (CommunicationMod does not send the game's prompt).
#[rustfmt::skip]
pub const HAND_PROMPT: &[(&str, &str)] = &[
    ("ExhaustAction", "Choose {n} card(s) to Exhaust."), ("PurityAction", "Choose up to {n} card(s) to Exhaust."), ("RecycleAction", "Choose a card to Exhaust."),
    ("DiscardAction", "Choose {n} card(s) to Discard."), ("GamblingChipAction", "Choose any number of cards to Discard."), ("PutOnDeckAction", "Put a card on top of your draw pile."),
    ("WarcryAction", "Put a card on top of your draw pile."), ("SetupAction", "Put a card on top of your draw pile."), ("ThinkingAheadAction", "Put a card on top of your draw pile."),
    ("ForethoughtAction", "Put a card on top of your draw pile."), ("ArmamentsAction", "Choose a card to Upgrade."), ("DualWieldAction", "Choose a card to copy."),
    ("RetainCardsAction", "Choose up to {n} card(s) to Retain."), ("NightmareAction", "Choose a card to copy."),
];

pub fn _COLOR() -> &'static regex::Regex {
    crate::re!(r"#[rgbyp]|~|@|\bNL\b")
}

/// `TABLE.get(key)` of one of the tables above.
pub fn lookup<T: Copy>(table: &[(&str, T)], key: &str) -> Option<T> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Game text without its colour codes and line-break markers. (`s`: `str(s or "")` of it is read.)
pub fn clean(s: &Value) -> String {
    _COLOR().replace_all(&s.text(), "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `d.get(key, default)`: the default only for a key that is missing.
fn get<'a>(d: &'a Value, key: &str, default: &'a Value) -> &'a Value {
    if d.has(key) {
        &d[key]
    } else {
        default
    }
}

/// `a == b` as the original compares what JSON holds (1 == 1.0 == True).
fn base_eq(a: &Value, b: &Value) -> bool {
    let number = |v: &Value| match v {
        Value::Bool(x) => Some(*x as i64 as f64),
        Value::Number(n) => n.as_f64(),
        _ => None,
    };
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x == y,
        (None, None) => a == b,
        _ => false,
    }
}

/// `v == n` for a number n.
fn is_number(v: &Value, n: i64) -> bool {
    base_eq(v, &json!(n))
}

/// `int(v or default)`.
fn int_or(v: &Value, default: i64) -> i64 {
    if v.truthy() {
        v.int()
    } else {
        default
    }
}

/// A map square's place as a dict key: `(x, y)`.
fn place(x: &Value, y: &Value) -> String {
    let one = |v: &Value| match v {
        Value::Bool(b) => base::float_repr(*b as i64 as f64),
        Value::Number(n) => base::float_repr(n.as_f64().unwrap_or(0.0)),
        other => base::repr_of(other),
    };
    format!("{}, {}", one(x), one(y))
}

/// Translates one message at a time, and keeps what the way back needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Screens {
    /// the last message from the game
    pub raw: Rc<Value>,
    /// the bot's own menu, between runs
    pub menu_screen: String,
    pub character: Option<String>,
    pub ascension: Option<i64>,
    /// character -> highest ascension unlocked (from the save files)
    pub max_ascension: IndexMap<String, i64>,
    /// characters the game would not start
    pub locked: BTreeSet<String>,
    /// runs won as seen by this session (plus what the save files said at the start)
    pub wins: i64,
    pub _won_seen: Option<(Value, Value)>,
    /// enemy id -> monster index in the game's list
    pub targets: IndexMap<String, i64>,
    /// shop item index -> choice name
    pub shop_names: IndexMap<i64, String>,
    pub max_energy: i64,
    pub _turn_seen: Option<(Value, Value)>,
}

impl Default for Screens {
    fn default() -> Self {
        Screens::new()
    }
}

impl Screens {
    pub fn new() -> Self {
        Screens {
            raw: Rc::new(json!({})),
            menu_screen: "main".to_string(),
            character: None,
            ascension: None,
            max_ascension: IndexMap::new(),
            locked: BTreeSet::new(),
            wins: 0,
            _won_seen: None,
            targets: IndexMap::new(),
            shop_names: IndexMap::new(),
            max_energy: 3,
            _turn_seen: None,
        }
    }

    /// (A property in the original.)
    pub fn commands(&self) -> Vec<String> {
        self.raw["available_commands"].list().iter().map(base::str_of).collect()
    }

    /// `name in self.commands`.
    fn can(&self, name: &str) -> bool {
        self.raw["available_commands"].list().iter().any(|c| base::str_of(c) == name)
    }

    /// (A property in the original: `raw.get("game_state") or {}`, null here when there is none.)
    pub fn game(&self) -> &Value {
        &self.raw["game_state"]
    }

    /// The game's state, held apart from `self` (so that what is kept can change while it is read).
    fn held(&self) -> Rc<Value> {
        self.raw.clone()
    }

    pub fn _run(&self) -> Value {
        let g = self.game();
        let mut out = json!({"act": g["act"], "floor": g["floor"], "ascension": g["ascension_level"]});
        if g["final_act_available"].truthy() {
            // the act 4 keys held so far (logged as they come, kept in runs.jsonl)
            let mut held: Vec<&str> = keys::KEYS.iter().copied().filter(|k| g["keys"][*k].truthy()).collect();
            held.sort();
            out["keys"] = json!(held);
        }
        out
    }

    /// `live`: a card in the hand, whose numbers the game has worked out for this moment of the fight.
    /// (`index`: None in the original when left out; `live`: false.)
    pub fn card(&self, c: &Value, index: Option<i64>, live: bool) -> Value {
        let upgraded = c["upgrades"].int() > 0;
        let cost = &c["cost"];
        let mut desc = text::card_text(&c["id"], &c["name"], upgraded);
        let ctype = lookup(CARD_TYPE, &base::str_of(&c["type"])).map(str::to_string).unwrap_or_else(|| base::title(&c["type"].text()));
        if live && !desc.is_empty() {
            desc = text::live_numbers(&desc, &ctype, c);
        }
        if ctype == "Curse" && c["is_playable"].truthy() && !is_number(cost, -2) {
            // (Blue Candle: "Whenever you play a curse, lose 1 HP and Exhaust it.")
            desc = crate::re!(r"^Unplayable\.\s*").replace(&desc, "").to_string() + " Lose 1 HP. Exhaust.";
        }
        let target = if c["has_target"].truthy() {
            "AnyEnemy"
        } else if crate::re!(r"(?i)\ball enemies\b").is_match(&desc) {
            "AllEnemies"
        } else {
            "Self"
        };
        let cost_text = if is_number(cost, -1) {
            "X".to_string()
        } else if cost.is_number() || cost.is_boolean() {
            cost.int().max(0).to_string()
        } else if cost.truthy() {
            base::str_of(cost)
        } else {
            "0".to_string()
        };
        let mut out = json!({"id": key_of(&c["id"].text()), "name": c["name"].text(), "type": ctype,
               "cost": cost_text, "star_cost": null, "description": desc,
               "target_type": target, "can_play": get(c, "is_playable", &Value::Bool(true)).truthy(), "is_upgraded": upgraded,
               "rarity": lookup(RARITY, &base::str_of(&c["rarity"])).map(str::to_string).unwrap_or_else(|| base::title(&c["rarity"].text()))});
        if is_number(cost, -2) {
            out["unplayable_reason"] = json!("Unplayable");
        }
        if let Some(index) = index {
            out["index"] = json!(index);
        }
        out
    }

    pub fn powers(creature: &Value) -> Vec<Value> {
        let mut out = vec![];
        for p in creature["powers"].list() {
            let pid = crate::re!(r"_POWER$").replace(&key_of(&p["id"].or(&p["name"]).text()), "").to_string(); // ("Generic Strength Up Power")
            let amount = if pid == "LIFE_LINK" {
                json!(base::floordiv(creature["max_hp"].int(), 2))
            } else {
                lookup(AMOUNT, &pid).map(|n| json!(n)).unwrap_or_else(|| p["amount"].clone())
            };
            out.push(json!({"id": lookup(POWER_ID, &pid).unwrap_or(&pid).to_string() + "_POWER", "name": p["name"], "amount": amount, "description": clean(&p["description"])}));
        }
        out
    }

    pub fn potions(&self) -> Vec<Value> {
        let mut out = vec![];
        for (slot, p) in self.game()["potions"].list().iter().enumerate() {
            if base::str_of(&p["id"]) == "Potion Slot" {
                continue;
            }
            let key = text::potion_key(&p["id"], &p["name"]);
            out.push(json!({"slot": slot, "id": key, "name": p["name"], "description": text::potion_text(&p["id"], &p["name"]), "target_type": if p["requires_target"].truthy() { "AnyEnemy" } else { "Self" },
                        "can_use_in_combat": p["can_use"].truthy() && !text::potions::KEEP.contains(&key.as_str()), "can_discard": get(p, "can_discard", &Value::Bool(true)).truthy()}));
        }
        out
    }

    pub fn player(&mut self, combat: bool) -> Value {
        let raw = self.held();
        let g = &raw["game_state"];
        let cls = g["class"].text();
        let mut p = json!({
            "character": lookup(CHARACTER_NAMES, &cls).map(str::to_string).unwrap_or_else(|| base::title(&cls.replace('_', " "))),
            "hp": g["current_hp"], "max_hp": g["max_hp"], "gold": g["gold"], "block": 0, "status": [],
            "relics": g["relics"].list().iter().map(|r| json!({"id": key_of(&r["id"].text()), "name": r["name"], "counter": r["counter"],
                        "description": text::relic_text(&r["id"], &r["name"])})).collect::<Vec<_>>(),
            "potions": self.potions(), "max_potion_slots": if g["potions"].list().is_empty() { 3 } else { g["potions"].list().len() },
        });
        let cs = &g["combat_state"];
        if !combat || !cs.truthy() {
            return p;
        }
        let me = &cs["player"];
        let hand: Vec<Value> = cs["hand"].list().iter().enumerate().map(|(i, c)| self.card(c, Some(i as i64), true)).collect();
        let draw: Vec<Value> = cs["draw_pile"].list().iter().map(|c| self.card(c, None, false)).collect();
        let orbs: Vec<&Value> = me["orbs"].list().iter().filter(|o| !matches!(base::str_of(&o["id"]).as_str(), "Empty" | "None")).collect();
        let turn = (g["floor"].clone(), cs["turn"].clone());
        if !matches!(&self._turn_seen, Some((floor, n)) if base_eq(floor, &turn.0) && base_eq(n, &turn.1)) {
            // the energy a turn starts with is the most it has
            self._turn_seen = Some(turn);
            if cs["turn"].int() == 1 {
                self.max_energy = int_or(&me["energy"], 3);
            }
        }
        let zero = json!(0);
        let updates = [
            ("hp", get(me, "current_hp", &p["hp"]).clone()),
            ("max_hp", get(me, "max_hp", &p["max_hp"]).clone()),
            ("block", get(me, "block", &zero).clone()),
            ("energy", get(me, "energy", &zero).clone()),
            ("max_energy", json!(self.max_energy)),
            ("status", json!(Self::powers(me))),
            ("hand", json!(hand)),
            ("draw_pile", json!(draw)),
            ("draw_pile_count", json!(cs["draw_pile"].list().len())),
            ("discard_pile", json!(cs["discard_pile"].list().iter().map(|c| self.card(c, None, false)).collect::<Vec<_>>())),
            ("discard_pile_count", json!(cs["discard_pile"].list().len())),
            ("exhaust_pile_count", json!(cs["exhaust_pile"].list().len())),
            ("orbs", json!(orbs.iter().map(|o| json!({"id": key_of(&o["id"].or(&o["name"]).text()), "passive_val": o["passive_amount"], "evoke_val": o["evoke_amount"]})).collect::<Vec<_>>())),
            ("orb_slots", json!(me["orbs"].list().len())),
            ("stance", me["stance"].clone()),
        ];
        for (k, v) in updates {
            p[k] = v;
        }
        p
    }

    /// (`body`: the keyword arguments of the original's `_wrap`, in their order.)
    pub fn _wrap(&mut self, kind: &str, body: Vec<(&str, Value)>) -> Value {
        let mut out = Map::new();
        out.insert("state_type".to_string(), json!(kind));
        out.insert("run".to_string(), self._run());
        out.insert("player".to_string(), self.player(ROOM_KIND.iter().any(|(_, v)| *v == kind)));
        for (k, v) in body {
            out.insert(k.to_string(), v);
        }
        Value::Object(out)
    }

    /// (`msg`: null for the original's None.)
    pub fn translate(&mut self, msg: &Value) -> Value {
        self.raw = Rc::new(if msg.truthy() { msg.clone() } else { json!({}) });
        if !msg.truthy() {
            return json!({"state_type": "loading"});
        }
        if !msg["in_game"].truthy() {
            return self.menu();
        }
        self.menu_screen = "main".to_string();
        self.character = None;
        let screen = self.game()["screen_type"].text();
        let screen = if screen.is_empty() { "NONE".to_string() } else { screen };
        // (the original looks the handler up by name: `getattr(self, "_" + screen.lower(), None)`, which also finds `_run`.)
        match screen.to_lowercase().as_str() {
            "map" => self._map(),
            "combat_reward" => self._combat_reward(),
            "card_reward" => self._card_reward(),
            "event" => self._event(),
            "rest" => self._rest(),
            "shop_room" => self._shop_room(),
            "shop_screen" => self._shop_screen(),
            "chest" => self._chest(),
            "boss_reward" => self._boss_reward(),
            "grid" => self._grid(),
            "hand_select" => self._hand_select(),
            "game_over" => self._game_over(),
            "complete" => self._complete(),
            "run" => self._run(),
            _ => self._none(),
        }
    }

    pub fn menu(&self) -> Value {
        if self.menu_screen == "character_select" {
            let chosen = self.character.as_deref().filter(|c| !c.is_empty());
            let chars: Vec<Value> = CHARACTERS.iter().map(|c| json!({"id": c, "name": lookup(CHARACTER_NAMES, c), "locked": self.locked.contains(*c)})).collect();
            let top = chosen.map(|c| self.max_ascension.get(c).copied().unwrap_or(0));
            let mut options: Vec<Value> = chars.iter().map(|c| json!({"name": c["id"], "enabled": !c["locked"].truthy()})).collect();
            options.push(json!({"name": "confirm", "enabled": self.character.is_some()}));
            options.push(json!({"name": "back", "enabled": true}));
            return json!({"state_type": "menu", "menu_screen": "character_select", "characters": chars,
                    "options": options,
                    "selected": self.character, "ascension": if chosen.is_some() { self.ascension } else { None }, "max_ascension": top});
        }
        json!({"state_type": "menu", "menu_screen": "main", "options": if self.can("start") { vec!["singleplayer"] } else { vec![] }})
    }

    pub fn combat(&mut self) -> Value {
        let raw = self.held();
        let g = &raw["game_state"];
        let cs = &g["combat_state"];
        let mut enemies = vec![];
        self.targets = IndexMap::new();
        for (i, m) in cs["monsters"].list().iter().enumerate() {
            if m["is_gone"].truthy() {
                continue;
            }
            let eid = format!("{}_{}", key_of(&m["id"].or(&m["name"]).text()), i);
            self.targets.insert(eid.clone(), i as i64);
            let mut status = Self::powers(m);
            let behind = if m.has("left") {
                // Surrounded (the Spire Shield and Spear): its side, as STS2's crabs show theirs, and whether it is behind us (speedmod.rs)
                status.push(json!({"id": if m["left"].truthy() { "BACK_ATTACK_LEFT_POWER" } else { "BACK_ATTACK_RIGHT_POWER" }, "name": "Back Attack", "amount": 1, "description": ""}));
                Some(m["behind"].truthy())
            } else {
                None
            };
            let mut entry = json!({"entity_id": eid, "name": m["name"], "hp": if m["half_dead"].truthy() { json!(0) } else { m["current_hp"].clone() }, "max_hp": m["max_hp"],
                     "block": get(m, "block", &json!(0)), "status": status, "intents": Self::intents(m)});
            if let Some(behind) = behind {
                entry["behind"] = json!(behind);
            }
            enemies.push(entry);
        }
        let playing = self.can("play") || self.can("end");
        let battle = json!({"round": cs["turn"], "turn": if playing { "player" } else { "enemy" }, "is_play_phase": playing, "enemies": enemies});
        self._wrap(lookup(ROOM_KIND, &base::str_of(&g["room_type"])).unwrap_or("monster"), vec![("battle", battle)])
    }

    pub fn intents(m: &Value) -> Vec<Value> {
        let intent = m["intent"].text();
        let intent = if intent.is_empty() { "UNKNOWN".to_string() } else { intent };
        let mut out = vec![];
        let kind = |part: &str| lookup(INTENT_TYPE, part).map(str::to_string).unwrap_or_else(|| base::title(part));
        if intent.starts_with("ATTACK") {
            let (dmg, hits) = (&m["move_adjusted_damage"], int_or(&m["move_hits"], 1));
            if (dmg.is_number() || dmg.is_boolean()) && dmg.float() >= 0.0 {
                out.push(json!({"type": "Attack", "label": if hits > 1 { format!("{}x{}", dmg.int(), hits) } else { dmg.int().to_string() }}));
            } else {
                out.push(json!({"type": "Attack", "label": ""})); // (Runic Dome: the numbers are hidden)
            }
            let rest = intent.strip_prefix("ATTACK").unwrap_or(&intent).trim_matches('_');
            if !rest.is_empty() {
                out.push(json!({"type": kind(rest), "label": ""}));
            }
        } else {
            let parts: Vec<&str> = if intent.starts_with("DEFEND_") { intent.split('_').collect() } else { vec![intent.as_str()] };
            for part in parts {
                out.push(json!({"type": kind(part), "label": ""}));
            }
        }
        out
    }

    pub fn _none(&mut self) -> Value {
        if self.game()["room_phase"].as_str() == Some("COMBAT") && self.game()["combat_state"].truthy() {
            return self.combat();
        }
        self._wrap("overlay", vec![])
    }

    pub fn _map(&mut self) -> Value {
        let raw = self.held();
        let g = &raw["game_state"];
        let ss = &g["screen_state"];
        let mut nodes: IndexMap<String, Value> = IndexMap::new();
        let mut top = 0;
        let emerald = keys::wants_emerald(g);
        let symbol = |n: &Value| lookup(MAP_SYMBOL, &base::str_of(&n["symbol"])).unwrap_or("Unknown");
        for n in g["map"].list() {
            let (x, y) = (&n["x"], &n["y"]);
            top = top.max(y.int());
            let mut kind = symbol(n);
            if kind == "Elite" && emerald && n["emerald_key"].truthy() {
                kind = "EmeraldElite"; // (the burning elite, with the Emerald Key: strategy::path_value prices it)
            }
            nodes.insert(place(x, y), json!({"col": x, "row": y.int() + 1, "type": kind, "children": n["children"].list().iter().map(|c| json!([c["x"], c["y"].int() + 1])).collect::<Vec<_>>()}));
        }
        let boss = json!([3, top + 2]);
        let is_boss = |c: &Value| base_eq(&c[0], &boss[0]) && base_eq(&c[1], &boss[1]);
        let places: HashSet<String> = nodes.keys().cloned().collect();
        for node in nodes.values_mut() {
            // the boss is not on the map: every road ends there
            let mut children: Vec<Value> = node["children"].list().iter().map(|c| if places.contains(&place(&c[0], &json!(c[1].int() - 1))) { c.clone() } else { boss.clone() }).collect();
            if is_number(&node["row"], top + 1) && !children.iter().any(is_boss) {
                children.push(boss.clone());
            }
            node["children"] = json!(children);
        }
        let mut listed: Vec<Value> = nodes.values().cloned().collect();
        listed.push(json!({"col": boss[0], "row": boss[1], "type": "Boss", "children": []}));
        let mut options = vec![];
        if ss["boss_available"].truthy() {
            options.push(json!({"index": 0, "col": boss[0], "row": boss[1], "type": "Boss", "leads_to": []}));
        } else {
            for (i, n) in ss["next_nodes"].list().iter().enumerate() {
                let node = nodes.get(&place(&n["x"], &n["y"])).cloned().unwrap_or_else(|| json!({"type": symbol(n), "children": []}));
                let unknown = json!({"type": "Unknown"});
                let kids: Vec<&Value> = node["children"].list().iter().map(|c| listed.iter().find(|k| base_eq(&k["col"], &c[0]) && base_eq(&k["row"], &c[1])).unwrap_or(&unknown)).collect();
                options.push(json!({"index": i, "col": n["x"], "row": n["y"].int() + 1, "type": node["type"],
                                "leads_to": kids.iter().map(|k| json!({"type": k["type"], "row": k["row"]})).collect::<Vec<_>>()}));
            }
        }
        self._wrap("map", vec![("map", json!({"next_options": options, "nodes": listed}))])
    }

    pub fn _combat_reward(&mut self) -> Value {
        let raw = self.held();
        let g = &raw["game_state"];
        let mut items = vec![];
        let rewards = g["screen_state"]["rewards"].list();
        // A chest's Sapphire Key and its relic: taking one takes the other away, so only the one wanted is shown.
        let sapphire = |r: &Value| r["reward_type"].text().to_uppercase() == "SAPPHIRE_KEY";
        let key_wanted = keys::wants_sapphire(g) && rewards.iter().any(sapphire);
        let linked: HashSet<String> = rewards.iter().filter(|r| sapphire(r)).map(|r| key_of(&r["link"]["id"].text())).collect();
        for (i, r) in rewards.iter().enumerate() {
            let kind = r["reward_type"].or(&r["type"]).text().to_uppercase(); // (CommunicationMod's name for it is "reward_type")
            if kind == "SAPPHIRE_KEY" && !key_wanted {
                continue; // (taking it gives up the relic beside it)
            }
            if kind == "RELIC" && key_wanted && linked.contains(&key_of(&r["relic"]["id"].text())) {
                continue;
            }
            let mut item = json!({"index": i, "type": if kind == "STOLEN_GOLD" { "gold".to_string() } else { kind.to_lowercase() }});
            if kind == "GOLD" || kind == "STOLEN_GOLD" {
                item["gold_amount"] = r["gold"].clone();
            } else if kind == "POTION" {
                let p = &r["potion"];
                item["potion_id"] = json!(text::potion_key(&p["id"], &p["name"]));
                item["potion_name"] = p["name"].clone();
                item["potion_description"] = json!(text::potion_text(&p["id"], &p["name"]));
                item["description"] = json!(text::potion_text(&p["id"], &p["name"]));
            } else if kind == "RELIC" {
                let rel = &r["relic"];
                item["relic_id"] = json!(key_of(&rel["id"].text()));
                item["relic_name"] = rel["name"].clone();
            }
            items.push(item);
        }
        let can_proceed = self.can("proceed");
        self._wrap("rewards", vec![("rewards", json!({"items": items, "can_proceed": can_proceed}))])
    }

    pub fn _card_reward(&mut self) -> Value {
        let raw = self.held();
        let g = &raw["game_state"];
        let ss = &g["screen_state"];
        let mut cards: Vec<Value> = ss["cards"].list().iter().enumerate().map(|(i, c)| self.card(c, Some(i as i64), false)).collect();
        let fight = g["room_phase"].as_str() == Some("COMBAT");
        if fight {
            // the Stance Potion's two choices: as the planner can weigh them (a free Skill that changes the stance)
            for c in cards.iter_mut() {
                if let Some(words) = lookup(STANCE_CHOICE, &key_of(&c["name"].text())) {
                    c["type"] = json!("Skill");
                    c["cost"] = json!("0");
                    c["description"] = json!(words);
                    c["target_type"] = json!("Self");
                }
            }
        }
        let can_skip = self.can("skip") || ss["skip_available"].truthy();
        let mut state = self._wrap("card_reward", vec![("card_reward", json!({"cards": cards, "can_skip": can_skip}))]);
        if fight {
            // a card offered in a fight (Discovery, Attack Potion): for this turn
            state["card_reward"]["in_combat"] = json!(true);
        }
        state
    }

    pub fn _event(&mut self) -> Value {
        let raw = self.held();
        let ss = &raw["game_state"]["screen_state"];
        let mut opts = vec![];
        let options = ss["options"].list();
        for o in options {
            let words = clean(&o["text"]);
            let (title, desc) = match crate::re!(r"^\[([^\]]*)\]\s*(.*)").captures(&words) {
                Some(m) => (m[1].to_string(), m[2].to_string()),
                None => {
                    let label = clean(&o["label"]);
                    (if label.is_empty() { words.clone() } else { label }, words.clone())
                }
            };
            let locked = o["disabled"].truthy() || o["choice_index"].is_null();
            opts.push(json!({"index": get(o, "choice_index", &json!(-1)), "title": title, "description": desc, "is_locked": locked, "is_proceed": options.len() == 1}));
        }
        self._wrap(
            "event",
            vec![("event", json!({"event_id": key_of(&ss["event_id"].or(&ss["event_name"]).text()), "event_name": ss["event_name"],
                                          "body": clean(&ss["body_text"]), "options": opts, "in_dialogue": false}))],
        )
    }

    pub fn _rest(&mut self) -> Value {
        let raw = self.held();
        let g = &raw["game_state"];
        let ss = &g["screen_state"];
        let mut opts: Vec<Value> =
            ss["rest_options"].list().iter().enumerate().map(|(i, o)| json!({"index": i, "id": base::str_of(o).to_lowercase(), "name": base::title(&base::str_of(o)), "is_enabled": true})).collect();
        // Recall (the Ruby Key): alone when it has to be taken here, beside the rest while the key is missing (strategy::rest_site
        // takes it over an upgrade), and never by accident once the key is held or act 4 is shut.
        let recall: Vec<Value> = opts.iter().filter(|o| o["id"] == "recall").cloned().collect();
        if !recall.is_empty() && keys::wants_ruby(g) {
            opts = recall;
        } else if !keys::ruby_open(g) {
            opts.retain(|o| o["id"] != "recall");
        }
        let can_proceed = self.can("proceed");
        self._wrap("rest_site", vec![("rest_site", json!({"options": opts, "can_proceed": can_proceed}))])
    }

    pub fn _shop_room(&mut self) -> Value {
        // The merchant's room, his shelf not yet open: the client opens it once a visit (see client.rs).
        let can_proceed = self.can("proceed");
        self._wrap("shop", vec![("shop", json!({"items": [], "can_proceed": can_proceed})), ("_shop_room", json!(true))])
    }

    pub fn _shop_screen(&mut self) -> Value {
        let raw = self.held();
        let g = &raw["game_state"];
        let ss = &g["screen_state"];
        let gold = g["gold"].int();
        let mut items: Vec<Value> = vec![];
        self.shop_names = IndexMap::new();

        let mut add = |names: &mut IndexMap<i64, String>, cat: &str, name: String, price: &Value, fields: Vec<(String, Value)>| {
            let i = items.len() as i64;
            names.insert(i, name.to_lowercase());
            let mut item = json!({"index": i, "category": cat, "price": price, "cost": price, "is_stocked": true, "can_afford": price.int() <= gold});
            for (k, v) in fields {
                item[k.as_str()] = v;
            }
            items.push(item);
        };

        for c in ss["cards"].list() {
            let card = self.card(c, None, false);
            let fields = card.entries().map(|(k, v)| (format!("card_{k}"), v.clone())).collect();
            add(&mut self.shop_names, "card", base::str_of(&c["name"]), &c["price"], fields);
        }
        for r in ss["relics"].list() {
            let fields = vec![("relic_id".to_string(), json!(key_of(&r["id"].text()))), ("relic_name".to_string(), r["name"].clone()), ("description".to_string(), json!(text::relic_text(&r["id"], &r["name"])))];
            add(&mut self.shop_names, "relic", base::str_of(&r["name"]), &r["price"], fields);
        }
        for p in ss["potions"].list() {
            let fields = vec![
                ("potion_id".to_string(), json!(text::potion_key(&p["id"], &p["name"]))),
                ("potion_name".to_string(), p["name"].clone()),
                ("description".to_string(), json!(text::potion_text(&p["id"], &p["name"]))),
            ];
            add(&mut self.shop_names, "potion", base::str_of(&p["name"]), &p["price"], fields);
        }
        if ss["purge_available"].truthy() {
            add(&mut self.shop_names, "card_removal", "purge".to_string(), &ss["purge_cost"], vec![]);
        }
        self._wrap("shop", vec![("shop", json!({"items": items, "can_proceed": true}))])
    }

    pub fn _chest(&mut self) -> Value {
        let raw = self.held();
        let ss = &raw["game_state"]["screen_state"];
        let relics = if ss["chest_open"].truthy() { json!([]) } else { json!([{"index": 0, "name": "chest", "id": key_of(&ss["chest_type"].text())}]) };
        self._wrap("treasure", vec![("treasure", json!({"relics": relics, "can_proceed": true}))])
    }

    pub fn _boss_reward(&mut self) -> Value {
        let raw = self.held();
        let ss = &raw["game_state"]["screen_state"];
        let mut relics: Vec<Value> = ss["relics"].list().iter().enumerate().map(|(i, r)| json!({"index": i, "id": key_of(&r["id"].text()), "name": r["name"]})).collect();
        base::sort_by_key(&mut relics, text::boss_relic_rank);
        let can_skip = self.can("skip");
        self._wrap("relic_select", vec![("relic_select", json!({"relics": relics, "can_skip": can_skip}))])
    }

    pub fn _grid(&mut self) -> Value {
        let raw = self.held();
        let g = &raw["game_state"];
        let ss = &g["screen_state"];
        let n = int_or(&ss["num_cards"], 1);
        let mut kind = [("for_upgrade", "upgrade"), ("for_transform", "transform"), ("for_purge", "remove")].iter().find(|(flag, _)| ss[*flag].truthy()).map(|(_, k)| *k).unwrap_or("");
        let verb = base::title(kind);
        if kind.is_empty() && g["room_phase"].as_str() == Some("COMBAT") && n == 1 && !ss["any_number"].truthy() {
            kind = "choose"; // (Seek, Headbutt, Exhume: one click picks and closes)
        }
        let plural = if n != 1 { "s" } else { "" };
        let prompt = if !verb.is_empty() { format!("Choose {n} card{plural} to {verb}.") } else { format!("Choose {n} card{plural}.") };
        let cards: Vec<Value> = ss["cards"].list().iter().enumerate().map(|(i, c)| self.card(c, Some(i as i64), false)).collect();
        let confirm = ss["confirm_up"].truthy() || (self.can("confirm") && (ss["selected_cards"].list().len() as i64 >= n || ss["any_number"].truthy()));
        let card_select = json!({"screen_type": kind, "prompt": prompt, "cards": cards, "preview_showing": ss["confirm_up"].truthy(),
                                                      "can_confirm": confirm, "can_cancel": self.can("cancel") || self.can("return"),
                                                      "can_skip": self.can("skip")});
        self._wrap("card_select", vec![("card_select", card_select)])
    }

    pub fn _hand_select(&mut self) -> Value {
        let raw = self.held();
        let g = &raw["game_state"];
        let ss = &g["screen_state"];
        let n = int_or(&ss["max_cards"], 1);
        let action = g["current_action"].text();
        let prompt = lookup(HAND_PROMPT, &action).unwrap_or("Choose {n} card(s).").replace("{n}", &n.to_string());
        let cards: Vec<Value> = ss["hand"].list().iter().enumerate().map(|(i, c)| self.card(c, Some(i as i64), true)).collect();
        let picked = ss["selected"].list().len() as i64;
        let ready = self.can("confirm") && (picked >= n || cards.is_empty() || (ss["can_pick_zero"].truthy() && picked > 0));
        let hand_select = json!({"prompt": prompt, "cards": cards, "can_confirm": ready,
                                                       "mode": if action == "ArmamentsAction" { "upgrade_select" } else { "simple_select" }});
        let mut state = self._wrap("hand_select", vec![("hand_select", hand_select)]);
        if g["combat_state"].truthy() {
            // (the planner judges which card to give up from the fight it is in)
            let fight = self.combat();
            state["battle"] = fight["battle"].clone();
            let mut player = fight["player"].clone();
            player["hand"] = json!(cards);
            state["player"] = player;
        }
        state
    }

    pub fn _game_over(&mut self) -> Value {
        let raw = self.held();
        let g = &raw["game_state"];
        let ss = &g["screen_state"];
        if ss["victory"].truthy() {
            let seen = (g["seed"].clone(), g["floor"].clone());
            if !matches!(&self._won_seen, Some((seed, floor)) if base_eq(seed, &seen.0) && base_eq(floor, &seen.1)) {
                self._won_seen = Some(seen);
                self.wins += 1;
            }
        }
        self._wrap("game_over", vec![("game_over", json!({"options": ["main_menu"], "victory": ss["victory"].truthy(), "score": ss["score"]}))])
    }

    pub fn _complete(&mut self) -> Value {
        // A room finished with nothing left to choose ("proceed" goes on), not the end of the run (after the act 3 boss too).
        self._wrap("overlay", vec![])
    }
}

#[cfg(test)]
mod tests {
    //! Replays tests/golden/screens.json: every call the original tests made on each Screens object, in order, on a fresh
    //! one (the client's menu clicks set as they were before each call), two made-up runs through the screens the tests
    //! never show, and every first-game card as the game names it (recorded from screens.rs).
    use super::*;

    fn golden() -> Value {
        serde_json::from_str(include_str!("../../../tests/golden/screens.json")).unwrap()
    }

    /// The same values of the same types in the same order.
    fn text_of(v: &Value) -> String {
        serde_json::to_string(v).unwrap()
    }

    fn arg<'a>(e: &'a Value, n: usize, name: &str) -> &'a Value {
        if e["args"][0].list().len() > n {
            &e["args"][0][n]
        } else {
            &e["args"][1][name]
        }
    }

    #[test]
    fn sequences() {
        let g = golden();
        let (mut calls, mut kinds) = (0, BTreeSet::new());
        for (s, seq) in g["sequences"].as_array().unwrap().iter().enumerate() {
            let mut sc = Screens::new();
            for (n, e) in seq.as_array().unwrap().iter().enumerate() {
                let pre = &e["pre"];
                sc.menu_screen = pre["menu_screen"].as_str().unwrap().to_string();
                sc.character = pre["character"].as_str().map(str::to_string);
                sc.ascension = pre["ascension"].as_i64();
                sc.max_ascension = pre["max_ascension"].entries().map(|(k, v)| (k.clone(), v.as_i64().unwrap())).collect();
                sc.locked = pre["locked"].list().iter().map(|c| c.as_str().unwrap().to_string()).collect();
                sc.wins = pre["wins"].as_i64().unwrap();
                sc.max_energy = pre["max_energy"].as_i64().unwrap();
                if !e["raw"].is_null() {
                    sc.raw = Rc::new(e["raw"].clone());
                }
                let method = e["m"].as_str().unwrap();
                let got = match method {
                    "translate" => sc.translate(arg(e, 0, "msg")),
                    "card" => sc.card(arg(e, 0, "c"), arg(e, 1, "index").as_i64(), arg(e, 2, "live").truthy()),
                    "menu" => sc.menu(),
                    "_run" => sc._run(),
                    "player" => sc.player(arg(e, 0, "combat").truthy()),
                    "combat" => sc.combat(),
                    "potions" => json!(sc.potions()),
                    "_none" => sc._none(),
                    "_map" => sc._map(),
                    "_combat_reward" => sc._combat_reward(),
                    "_card_reward" => sc._card_reward(),
                    "_event" => sc._event(),
                    "_rest" => sc._rest(),
                    "_shop_room" => sc._shop_room(),
                    "_shop_screen" => sc._shop_screen(),
                    "_chest" => sc._chest(),
                    "_boss_reward" => sc._boss_reward(),
                    "_grid" => sc._grid(),
                    "_hand_select" => sc._hand_select(),
                    "_game_over" => sc._game_over(),
                    "_complete" => sc._complete(),
                    other => panic!("no replay for {other}"),
                };
                assert_eq!(text_of(&got), text_of(&e["out"]), "sequence {s} call {n}: {method} {}", e["args"]);
                let post = json!({"menu_screen": sc.menu_screen, "character": sc.character, "wins": sc.wins, "targets": sc.targets.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<String, Value>>(),
                                  "shop_names": sc.shop_names.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(), "max_energy": sc.max_energy, "commands": sc.commands()});
                assert_eq!(text_of(&post), text_of(&e["post"]), "sequence {s} after call {n}: {method}");
                calls += 1;
                kinds.insert(format!("{method} {}", got["state_type"].s()));
            }
        }
        assert!(calls > 280 && kinds.len() > 25, "{calls} calls, {kinds:?}");
        for kind in ["monster", "elite", "boss", "map", "rewards", "card_reward", "event", "rest_site", "shop", "treasure", "relic_select", "card_select", "hand_select", "game_over", "overlay", "menu", "loading"] {
            assert!(kinds.contains(&format!("translate {kind}")), "no {kind} among {kinds:?}");
        }
    }

    #[test]
    fn cards_powers_intents_and_text() {
        let g = golden();
        let sc = Screens::new();
        let rows = g["cards"].as_array().unwrap();
        for row in rows {
            assert_eq!(text_of(&sc.card(&row[0], row[1].as_i64(), row[2].as_bool().unwrap())), text_of(&row[3]), "card {} {} {}", row[0], row[1], row[2]);
        }
        let cases = &g["cases"];
        for row in cases["clean"].list() {
            assert_eq!(clean(&row[0]), row[1].as_str().unwrap(), "clean {}", row[0]);
        }
        for row in cases["powers"].list() {
            assert_eq!(text_of(&json!(Screens::powers(&row[0]))), text_of(&row[1]), "powers {}", row[0]);
        }
        for row in cases["intents"].list() {
            assert_eq!(text_of(&json!(Screens::intents(&row[0]))), text_of(&row[1]), "intents {}", row[0]);
        }
        assert!(rows.len() > 700 && cases["powers"].list().len() > 50 && cases["intents"].list().len() > 140 && cases["clean"].list().len() > 10);
    }

    #[test]
    fn the_tables_are_the_recorded() {
        let g = golden();
        let t = &g["tables"];
        let pairs = |table: &[(&str, &str)]| json!(table.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>());
        assert_eq!(json!(CHARACTERS), t["CHARACTERS"]);
        assert_eq!(pairs(CHARACTER_NAMES), t["CHARACTER_NAMES"]);
        assert_eq!(pairs(ROOM_KIND), t["ROOM_KIND"]);
        assert_eq!(pairs(MAP_SYMBOL), t["MAP_SYMBOL"]);
        assert_eq!(pairs(CARD_TYPE), t["CARD_TYPE"]);
        assert_eq!(pairs(RARITY), t["RARITY"]);
        assert_eq!(pairs(POWER_ID), t["POWER_ID"]);
        assert_eq!(pairs(STANCE_CHOICE), t["STANCE_CHOICE"]);
        assert_eq!(pairs(INTENT_TYPE), t["INTENT_TYPE"]);
        assert_eq!(pairs(HAND_PROMPT), t["HAND_PROMPT"]);
        assert_eq!(json!(AMOUNT.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>()), t["AMOUNT"]);
    }
}
