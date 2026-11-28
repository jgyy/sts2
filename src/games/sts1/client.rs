//! The bot's client for the first game, over the relay: `state()` in the bot's words (screens.rs), and `act()` sends the
//! CommunicationMod command for the bot's action ("play_card" -> "play 2 0" ...). The menu before a run is played here.
//!
//! In the original `Sts1Client` is a subclass of `Client`; here it holds one (`client`) and uses its `_request`. The tests' seam is
//! the same as the original's: there a subclass overrides `ping`, `_message` and `send` (tests/sts1/helpers.rs `Direct`); here a
//! [`Wire`] given to [`Sts1Client::with_wire`] answers those three in the relay's place.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use super::screens::{Screens, CHARACTERS};
use super::text;
use crate::client::{Client, Game, GameUnavailable};
use crate::logging::Logger;
use crate::base::{self, JsonExt};

static LOG: Logger = Logger::new("sts2bot.sts1");

pub const DEFAULT_PORT: u16 = 15527;
// how often, and for how many frames each time, to wait for a fight's intents
pub const INTENT_WAITS: i64 = 5;
pub const INTENT_WAIT_FRAMES: i64 = 10;
// what Scry discards; clicks before giving the screen back to the bot
pub const SCRY_AWAY: [&str; 2] = ["STATUS", "CURSE"];
pub const SCRY_STEPS: i64 = 12;

/// `answer.get("status") == "ok" and isinstance(answer.get("message"), dict)`.
fn answered(answer: &Value) -> bool {
    answer["status"].as_str() == Some("ok") && answer["message"].is_object()
}

/// `int(x)` of an argument of an action (None is the original's TypeError: the bot never sends one).
fn int(v: &Value) -> i64 {
    match v {
        Value::Bool(b) => *b as i64,
        Value::Number(n) => n.as_i64().unwrap_or_else(|| n.as_f64().map(base::trunc).unwrap_or(0)),
        Value::String(s) => s.trim().parse().unwrap_or_else(|_| panic!("invalid literal for int() with base 10: {}", base::str_repr(s))),
        other => panic!("int() argument must be a string, a bytes-like object or a real number, not {}", base::repr_of(other)),
    }
}

/// A fight's message whose monsters have no intent yet (CommunicationMod says "DEBUG" until the game sets them).
/// (`msg`: null for the original's None.)
pub fn _intents_pending(msg: &Value) -> bool {
    if !msg.is_object() || !msg["available_commands"].list().iter().any(|c| c.as_str() == Some("wait")) {
        return false;
    }
    let monsters = msg["game_state"]["combat_state"]["monsters"].list();
    monsters.iter().filter(|m| !m["is_gone"].truthy() && !m["half_dead"].truthy() && m["current_hp"].float() > 0.0).any(|m| base::str_of(&m["intent"]) == "DEBUG")
}

/// The relay's place, for tests: what the original's tests override in a subclass (`ping`, `_message`, `send`).
pub trait Wire {
    /// (the original's `Direct.ping`: the fake game is always there.)
    fn ping(&mut self) -> bool {
        true
    }
    /// The game's last message (null for none).
    fn _message(&mut self) -> Result<Value, GameUnavailable>;
    /// One CommunicationMod command, answered as the relay answers: `{"status": "ok", "message": ...}` or `{"status": "error", "error": ...}`.
    fn send(&mut self, command: &str) -> Result<Value, GameUnavailable>;
}

/// The constructor's `prefs`: host.read_prefs's dict, or a function each profile() calls again (to see a new ascension or character).
#[derive(Default)]
pub enum Prefs {
    #[default]
    None,
    Dict(Value),
    Read(Box<dyn FnMut() -> Value>),
}

pub struct Sts1Client {
    /// (The base class in the original: the address and `_request`.)
    pub client: Client,
    /// Answers in the relay's place when set (tests).
    pub wire: Option<Box<dyn Wire>>,
    pub screens: Screens,
    /// (seed, floor) of the merchants whose shelf was opened
    pub _shops_opened: Vec<(Value, Value)>,
    pub _read_prefs: Option<Box<dyn FnMut() -> Value>>,
    /// characters the game would not start
    pub _refused: BTreeSet<String>,
}

impl Default for Sts1Client {
    fn default() -> Self {
        Sts1Client::new("127.0.0.1", DEFAULT_PORT, 30.0, Prefs::None)
    }
}

impl Sts1Client {
    /// `prefs`: host.read_prefs's dict, or a function each profile() calls again (to see a new ascension or character).
    /// (the original's defaults: "127.0.0.1", DEFAULT_PORT, 30.0, None.)
    pub fn new(host: &str, port: u16, timeout: f64, prefs: Prefs) -> Self {
        let mut this = Sts1Client { client: Client::new(host, port, timeout), wire: None, screens: Screens::new(), _shops_opened: Vec::new(), _read_prefs: None, _refused: BTreeSet::new() };
        let first = match prefs {
            Prefs::Read(mut read) => {
                let first = read();
                this._read_prefs = Some(read);
                first
            }
            Prefs::Dict(prefs) if prefs.truthy() => prefs,
            _ => json!({}),
        };
        this._take_prefs(&first);
        this
    }

    /// `Sts1Client(port=port)`.
    pub fn on_port(port: u16) -> Self {
        Sts1Client::new("127.0.0.1", port, 30.0, Prefs::None)
    }

    /// The client with the relay taken out (the tests' `Direct(fake, prefs=...)`): `wire` answers `ping`, `_message` and `send`.
    pub fn with_wire(wire: Box<dyn Wire>, prefs: Prefs) -> Self {
        let mut this = Sts1Client::new("127.0.0.1", DEFAULT_PORT, 30.0, prefs);
        this.wire = Some(wire);
        this
    }

    pub fn _take_prefs(&mut self, prefs: &Value) {
        self.screens.max_ascension = prefs["max_ascension"].entries().map(|(k, v)| (k.clone(), int(v))).collect();
        self.screens.wins = self.screens.wins.max(prefs["wins"].int());
        self.screens.locked = prefs["locked"].list().iter().map(base::str_of).chain(self._refused.iter().cloned()).collect();
    }

    pub fn ping(&mut self) -> bool {
        if let Some(wire) = self.wire.as_mut() {
            return wire.ping();
        }
        match self.client._request(&format!("{}/", self.client.base), None) {
            Ok(answer) => answer["connected"].truthy(),
            Err(_) => false,
        }
    }

    /// (Null for the original's None.)
    pub fn _message(&mut self) -> Result<Value, GameUnavailable> {
        if let Some(wire) = self.wire.as_mut() {
            return wire._message();
        }
        let mut answer = self.client._request(&format!("{}/state", self.client.base), None)?;
        let msg = answer["message"].take();
        Ok(if msg.is_object() { msg } else { Value::Null })
    }

    /// One CommunicationMod command, answered with the game's next state (or its error).
    pub fn send(&mut self, command: &str) -> Result<Value, GameUnavailable> {
        if let Some(wire) = self.wire.as_mut() {
            return wire.send(command);
        }
        self.client._request(&format!("{}/command", self.client.base), Some(&json!({"command": command})))
    }

    pub fn state(&mut self) -> Result<Value, GameUnavailable> {
        let mut msg = self._message()?;
        for _ in 0..INTENT_WAITS {
            // A fight's first message comes before the monsters' intents are set: wait a few frames for them.
            if !_intents_pending(&msg) {
                break;
            }
            let mut answer = self.send(&format!("wait {INTENT_WAIT_FRAMES}"))?;
            if !answered(&answer) {
                break;
            }
            msg = answer["message"].take();
        }
        let msg = self._scry(msg)?;
        let mut st = self.screens.translate(&msg);
        if st["_shop_room"].truthy() {
            // The merchant's room comes before his shelf: open it once a visit, the bot only shops on the shelf.
            let game = self.screens.game();
            let place = (game["seed"].clone(), game["floor"].clone());
            if !self._shops_opened.contains(&place) && self.screens.commands().iter().any(|c| c == "choose") {
                self._shops_opened.push(place);
                let answer = self.send("choose shop")?;
                if answered(&answer) {
                    st = self.screens.translate(&answer["message"]);
                }
            }
        }
        Ok(st)
    }

    /// The Watcher's Scry grid, done here: Status and Curse cards go, the rest stay, then confirm.
    /// (`msg`: null for the original's None.)
    pub fn _scry(&mut self, mut msg: Value) -> Result<Value, GameUnavailable> {
        for _ in 0..SCRY_STEPS {
            let g = &msg["game_state"];
            if base::str_of(&g["screen_type"]) != "GRID" || base::str_of(&g["current_action"]) != "ScryAction" {
                return Ok(msg);
            }
            let ss = &g["screen_state"];
            let picked: Vec<&Value> = ss["selected_cards"].list().iter().map(|c| &c["uuid"]).collect();
            let wrong = ss["cards"].list().iter().position(|c| SCRY_AWAY.contains(&base::str_of(&c["type"]).as_str()) != picked.contains(&&c["uuid"]));
            let command = match wrong {
                Some(i) => format!("choose {i}"),
                None => "confirm".to_string(),
            };
            let mut answer = self.send(&command)?;
            if !answered(&answer) {
                return Ok(msg);
            }
            msg = answer["message"].take();
        }
        Ok(msg)
    }

    /// What the save files say, and the wins seen since: enough for the bot to pick a character.
    pub fn profile(&mut self) -> Result<Value, GameUnavailable> {
        if !self.screens.raw["in_game"].truthy() {
            if let Some(read) = self._read_prefs.as_mut() {
                let prefs = read();
                self._take_prefs(&prefs);
            }
        }
        let characters: Vec<Value> = CHARACTERS
            .iter()
            .filter(|c| !self.screens.locked.contains(**c))
            .map(|c| json!({"id": c, "max_ascension": self.screens.max_ascension.get(*c).copied().unwrap_or(0)}))
            .collect();
        Ok(json!({"total_wins": self.screens.wins, "characters": characters}))
    }

    pub fn card(&mut self, card_id: &str) -> Result<Option<Value>, GameUnavailable> {
        let Some(key) = text::card_key(card_id, "") else {
            return Ok(None);
        };
        let cost = text::UPGRADE_COST().get(key.as_str());
        let side = |upgraded: bool| {
            let mut out = Map::new();
            out.insert("id".to_string(), json!(key));
            out.insert("description".to_string(), json!(text::card_text(&key, "", upgraded)));
            if let Some(cost) = cost {
                out.insert("cost".to_string(), json!((if upgraded { cost.1 } else { cost.0 }).to_string()));
            }
            Value::Object(out)
        };
        Ok(Some(json!({"id": key, "base": side(false), "upgraded": side(true)})))
    }

    /// `params`: the action's named arguments as a JSON object (`{}` or null for none).
    pub fn act(&mut self, action: &str, params: &Value) -> Result<Value, GameUnavailable> {
        if let Some(local) = self._local(action, params) {
            return Ok(local);
        }
        let Some(command) = self.command(action, params) else {
            let shown = if params.is_object() { base::repr_of(params) } else { "{}".to_string() };
            return Ok(json!({"status": "error", "error": format!("{action} {shown} is not possible here (commands: {})", self.screens.commands().join(", "))}));
        };
        let answer = self.send(&command)?;
        if answer["status"].as_str() != Some("ok") {
            if command.starts_with("start ") {
                let who = command.split_whitespace().nth(1).unwrap_or("").to_string();
                crate::log_warning!(LOG, "the game would not start {} ({})", who, base::str_of(&answer["error"]));
                self._refused.insert(who.clone());
                self.screens.locked.insert(who);
            }
            let error = if answer["error"].truthy() { base::str_of(&answer["error"]) } else { "the game refused the command".to_string() };
            return Ok(json!({"status": "error", "error": error}));
        }
        if answer["message"].is_object() {
            self.screens.translate(&answer["message"]);
        }
        Ok(json!({"status": "ok", "message": command}))
    }

    /// Menu clicks: the menu before a run is the bot's own (screens::Screens::menu).
    pub fn _local(&mut self, action: &str, params: &Value) -> Option<Value> {
        if self.screens.raw["in_game"].truthy() || !matches!(action, "menu_select" | "set_ascension") {
            return None;
        }
        let commands = self.screens.commands();
        let sc = &mut self.screens;
        let ok = |what: &str| Some(json!({"status": "ok", "message": what}));
        if action == "set_ascension" {
            let level = params["level"].int().min(20).max(0);
            sc.ascension = Some(level);
            return ok(&format!("ascension {level}"));
        }
        let option = params["option"].text();
        if option == "singleplayer" && commands.iter().any(|c| c == "start") {
            sc.menu_screen = "character_select".to_string();
            sc.character = None;
            return ok(&option);
        }
        if option == "back" {
            sc.menu_screen = "main".to_string();
            sc.character = None;
            return ok(&option);
        }
        let upper = option.to_uppercase();
        if sc.menu_screen == "character_select" && CHARACTERS.contains(&upper.as_str()) && !sc.locked.contains(&upper) {
            sc.ascension = Some(sc.max_ascension.get(&upper).copied().unwrap_or(0));
            sc.character = Some(upper);
            return ok(&option);
        }
        if sc.menu_screen == "character_select" && matches!(option.as_str(), "confirm" | "embark") && sc.character.as_deref().is_some_and(|c| !c.is_empty()) {
            return None; // the game starts the run: see command()
        }
        Some(json!({"status": "error", "error": format!("no menu option {} here", base::str_repr(&option))}))
    }

    /// The CommunicationMod command for one of the bot's actions (None: there is none on this screen).
    pub fn command(&self, action: &str, p: &Value) -> Option<String> {
        let sc = &self.screens;
        let cmds: BTreeSet<String> = sc.commands().into_iter().collect();
        let screen = sc.game()["screen_type"].text();
        let choose = |i: &Value| if cmds.contains("choose") { Some(format!("choose {}", int(i))) } else { None };
        let first = |names: &[&str]| names.iter().find(|n| cmds.contains(**n)).map(|n| n.to_string());
        // `p.get(key, default)`: the default only when the key is missing
        let get = |key: &str, default: Value| if p.has(key) { p[key].clone() } else { default };

        if action == "menu_select" {
            let option = p["option"].text();
            if matches!(option.as_str(), "confirm" | "embark") && sc.character.as_deref().is_some_and(|c| !c.is_empty()) && cmds.contains("start") {
                return Some(format!("start {} {}", sc.character.as_deref().unwrap_or(""), sc.ascension.unwrap_or(0)));
            }
            if matches!(option.as_str(), "main_menu" | "force_main_menu" | "continue") {
                return first(&["proceed", "confirm"]);
            }
            return None;
        }
        if action == "play_card" {
            if !cmds.contains("play") {
                return None;
            }
            let target = &p["target"];
            if !p.has("card_index") {
                panic!("KeyError: 'card_index'");
            }
            if !target.is_null() {
                if let Some(index) = sc.targets.get(&base::str_of(target)) {
                    return Some(format!("play {} {}", int(&p["card_index"]) + 1, index));
                }
            }
            return Some(format!("play {}", int(&p["card_index"]) + 1));
        }
        if action == "end_turn" {
            return if cmds.contains("end") { Some("end".to_string()) } else { None };
        }
        if matches!(action, "use_potion" | "discard_potion") {
            if !cmds.contains("potion") {
                return None;
            }
            let verb = if action == "use_potion" { "use" } else { "discard" };
            let target = &p["target"];
            let tail = match sc.targets.get(&base::str_of(target)) {
                Some(index) if verb == "use" && !target.is_null() => format!(" {index}"),
                _ => String::new(),
            };
            return Some(format!("potion {verb} {}{tail}", int(&get("slot", json!(0)))));
        }
        if matches!(action, "choose_map_node" | "claim_reward" | "select_card_reward" | "choose_rest_option" | "choose_event_option" | "select_card" | "combat_select_card" | "select_relic") {
            return choose(&get("index", get("card_index", json!(0))));
        }
        if matches!(action, "claim_treasure_relic" | "advance_dialogue") {
            return choose(&json!(0));
        }
        if action == "shop_purchase" {
            let name = sc.shop_names.get(&int(&get("index", json!(-1))));
            return match name {
                Some(name) if !name.is_empty() && cmds.contains("choose") => Some(format!("choose {name}")),
                _ => None,
            };
        }
        if matches!(action, "skip_card_reward" | "skip_relic_selection") {
            return first(&["skip", "cancel", "return"]);
        }
        if matches!(action, "confirm_selection" | "combat_confirm_selection" | "confirm_bundle_selection") {
            return first(&["confirm", "proceed"]);
        }
        if action == "cancel_selection" {
            return first(&["cancel", "return", "skip"]);
        }
        if action == "proceed" {
            if screen == "SHOP_SCREEN" {
                return first(&["leave", "return", "cancel"]);
            }
            return first(&["proceed", "confirm", "leave", "return", "skip"]);
        }
        None
    }
}

impl Game for Sts1Client {
    fn state(&mut self) -> Result<Value, GameUnavailable> {
        Sts1Client::state(self)
    }

    fn act(&mut self, action: &str, params: &Value) -> Result<Value, GameUnavailable> {
        Sts1Client::act(self, action, params)
    }

    fn profile(&mut self) -> Result<Value, GameUnavailable> {
        Sts1Client::profile(self)
    }

    fn card(&mut self, card_id: &str) -> Result<Option<Value>, GameUnavailable> {
        Sts1Client::card(self, card_id)
    }

    fn ping(&mut self) -> bool {
        Sts1Client::ping(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games;
    use sha1::{Digest, Sha1};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::rc::Rc;

    type Queue = Rc<RefCell<VecDeque<Value>>>;

    /// What the original's client was answered, in order: the wire of the replay.
    struct Replay(Queue);

    fn answer(v: &Value) -> Result<Value, GameUnavailable> {
        match v["unavailable"].as_str() {
            Some(why) => Err(GameUnavailable::new(why)),
            None => Ok(v.clone()),
        }
    }

    impl Wire for Replay {
        fn _message(&mut self) -> Result<Value, GameUnavailable> {
            let next = self.0.borrow_mut().pop_front().expect("a message the original did not ask for");
            assert_eq!(next[0], "_message");
            answer(&next[1])
        }

        fn send(&mut self, command: &str) -> Result<Value, GameUnavailable> {
            let next = self.0.borrow_mut().pop_front().unwrap_or_else(|| panic!("{command:?} sent, which the original did not send"));
            assert_eq!((next[0].s(), next[1].s()), ("send", command));
            answer(&next[2])
        }
    }

    fn snapshot(client: &Sts1Client) -> String {
        let sc = &client.screens;
        let shop_names: Map<String, Value> = sc.shop_names.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        let map = |m: &indexmap::IndexMap<String, i64>| -> Map<String, Value> { m.iter().map(|(k, v)| (k.clone(), json!(v))).collect() };
        base::json_dumps(&json!({
            "menu_screen": sc.menu_screen, "character": sc.character, "ascension": sc.ascension, "max_ascension": map(&sc.max_ascension), "locked": sc.locked, "wins": sc.wins,
            "targets": map(&sc.targets), "shop_names": shop_names, "max_energy": sc.max_energy, "refused": client._refused, "shops_opened": client._shops_opened.len(),
        }))
    }

    fn sha1_hex(text: &str) -> String {
        Sha1::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Every call the original tests (tests/sts1, and some more of the menu, refusals and errors) made of their clients, with
    /// what the game answered: the same commands go out, the same answers and states come back, the same things are kept.
    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/sts1_client.json")).unwrap();
        assert_eq!(json!([INTENT_WAITS, INTENT_WAIT_FRAMES]), g["intent_waits"]);
        assert_eq!(json!([SCRY_AWAY, SCRY_STEPS]), g["scry"]);
        assert_eq!(json!(DEFAULT_PORT), g["default_port"]);
        let _game = games::playing("sts1");
        let grid = g["grid"].list();
        let (mut calls, mut sent, mut commands) = (0, 0, 0);
        for (n, script) in g["scripts"].list().iter().enumerate() {
            let queue: Queue = Rc::new(RefCell::new(script["first"].list().iter().cloned().collect()));
            let prefs = if script["reads"].truthy() {
                let queue = queue.clone();
                Prefs::Read(Box::new(move || {
                    let next = queue.borrow_mut().pop_front().expect("the save files read once more than the original read them");
                    assert_eq!(next[0], "prefs");
                    next[1].clone()
                }))
            } else if script["prefs"].is_null() {
                Prefs::None
            } else {
                Prefs::Dict(script["prefs"].clone())
            };
            let mut client = Sts1Client::with_wire(Box::new(Replay(queue.clone())), prefs);
            assert!(queue.borrow().is_empty());
            let mut kept = base::json_dumps(&script["after"]);
            assert_eq!(snapshot(&client), kept, "client {n} as made");
            for (i, row) in script["calls"].list().iter().enumerate() {
                let why = format!("client {n} call {i}: {} {} {}", row["call"], row["args"], row["kw"]);
                *queue.borrow_mut() = row["wire"].list().iter().cloned().collect();
                sent += row["wire"].list().iter().filter(|w| w[0] == "send").count();
                let got = match row["call"].s() {
                    "state" => client.state(),
                    "act" => client.act(row["args"][0].s(), &row["kw"]),
                    "profile" => client.profile(),
                    "card" => client.card(row["args"][0].s()).map(|c| c.unwrap_or(Value::Null)),
                    "command" => Ok(json!(client.command(row["args"][0].s(), &row["args"][1]))),
                    other => panic!("{other}"),
                };
                assert!(queue.borrow().is_empty(), "{why}: the original went on to {:?}", queue.borrow().front());
                match (&row["unavailable"], got) {
                    (Value::String(want), got) => assert_eq!(got, Err(GameUnavailable::new(want.as_str())), "{why}"),
                    (_, got) => {
                        let got = base::json_dumps(&got.unwrap_or_else(|e| panic!("{why}: {e}")));
                        match row["result"]["sha1"].as_str() {
                            Some(want) => assert_eq!((sha1_hex(&got).as_str(), got.len() as i64), (want, row["result"]["len"].int()), "{why}: {got}"),
                            None => assert_eq!(got, base::json_dumps(&row["result"]["value"]), "{why}"),
                        }
                    }
                }
                if !row["after"].is_null() {
                    kept = base::json_dumps(&row["after"]);
                }
                assert_eq!(snapshot(&client), kept, "{why}: what the client keeps");
                if let Some(list) = row["commands"].as_u64() {
                    // every action with a handful of arguments, on the screen the call left
                    let target = client.screens.targets.keys().next().cloned().unwrap_or_else(|| "JAW_WORM_0".to_string());
                    for (case, want) in grid.iter().zip(g["command_lists"][list as usize].list()) {
                        if want["raises"].truthy() {
                            continue;
                        }
                        let p: Map<String, Value> = case[1].entries().map(|(k, v)| (k.clone(), if v == "$T" { json!(target) } else { v.clone() })).collect();
                        assert_eq!(&json!(client.command(case[0].s(), &Value::Object(p))), want, "{why}, then command {case}");
                        commands += 1;
                    }
                }
                calls += 1;
            }
        }
        assert!(calls > 600 && sent > 150 && commands > 200000, "{calls} calls, {sent} commands sent, {commands} commands asked");
    }

    #[test]
    fn intents_pending_reads_any_message() {
        let fight = |intent: &str, hp: i64, gone: bool| json!({"available_commands": ["play", "wait"], "game_state": {"combat_state": {"monsters": [{"intent": intent, "current_hp": hp, "is_gone": gone}]}}});
        assert!(_intents_pending(&fight("DEBUG", 10, false)));
        assert!(!_intents_pending(&fight("ATTACK", 10, false)));
        assert!(!_intents_pending(&fight("DEBUG", 0, false)));
        assert!(!_intents_pending(&fight("DEBUG", 10, true)));
        assert!(!_intents_pending(&Value::Null));
        assert!(!_intents_pending(&json!("wait")));
        assert!(!_intents_pending(&json!({"available_commands": ["play"], "game_state": fight("DEBUG", 10, false)["game_state"]})));
    }

    /// Over HTTP, as the relay is asked: `/` for ping, `/state` for the message, `/command` with the command.
    #[test]
    fn the_relay_is_asked_over_http() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let bodies = [r#"{"connected": false}"#, r#"{"connected": true, "seq": 3}"#, r#"{"message": {"in_game": false}}"#, r#"{"message": null}"#, r#"{"status": "ok", "message": {"x": 1}}"#];
        let server = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for body in bodies {
                let (mut sock, _) = listener.accept().unwrap();
                let mut got: Vec<u8> = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    if let Some(i) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&got[..i]).to_string();
                        let n: usize = head.lines().find_map(|l| l.strip_prefix("Content-Length: ")).map(|v| v.parse().unwrap()).unwrap_or(0);
                        if got.len() >= i + 4 + n {
                            break;
                        }
                    }
                    let n = sock.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    got.extend_from_slice(&buf[..n]);
                }
                let text = String::from_utf8_lossy(&got).to_string();
                seen.push((text.lines().next().unwrap_or("").to_string(), text.split("\r\n\r\n").nth(1).unwrap_or("").to_string()));
                sock.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).as_bytes()).unwrap();
            }
            seen
        });
        let mut client = Sts1Client::on_port(port);
        assert_eq!((client.client.base.as_str(), client.client.timeout), (format!("http://127.0.0.1:{port}").as_str(), 30.0));
        assert!(!client.ping()); // up, but the game has not said anything yet
        assert!(Game::ping(&mut client));
        assert_eq!(client._message(), Ok(json!({"in_game": false})));
        assert_eq!(client._message(), Ok(Value::Null));
        assert_eq!(client.send("start IRONCLAD 0"), Ok(json!({"status": "ok", "message": {"x": 1}})));
        let seen = server.join().unwrap();
        let lines: Vec<&str> = seen.iter().map(|(line, _)| line.as_str()).collect();
        assert_eq!(lines, ["GET / HTTP/1.1", "GET / HTTP/1.1", "GET /state HTTP/1.1", "GET /state HTTP/1.1", "POST /command HTTP/1.1"]);
        assert_eq!(seen[4].1, r#"{"command": "start IRONCLAD 0"}"#);
        // no relay at all
        let free = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let mut gone = Sts1Client::on_port(free);
        assert!(!gone.ping());
        assert!(gone.state().is_err());
        assert_eq!(gone.profile(), Ok(json!({"total_wins": 0, "characters": [{"id": "IRONCLAD", "max_ascension": 0}, {"id": "THE_SILENT", "max_ascension": 0}, {"id": "DEFECT", "max_ascension": 0}, {"id": "WATCHER", "max_ascension": 0}]})));
    }
}
