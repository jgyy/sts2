//! The first game's end of the bridge. CommunicationMod starts it and talks to it over stdin/stdout (a JSON state per line
//! when the game is ready, commands a line at a time); the bot talks to it over localhost:
//!
//! ```text
//! GET  /         {"status": "ok", "connected": true once the game has sent a state}
//! GET  /state    {"seq": n, "message": the last state the game sent}
//! POST /command  {"command": "play 1 0"} -> {"status": "ok"|"error", "message": the next state, ...}
//! ```
//!
//! Standard library only (and the crate's JSON). Stdout is the game's command channel: logs go to stderr.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::client::os_error_text;
use crate::base::{self, JsonExt};

pub const DEFAULT_PORT: u16 = 15527;
pub const ANSWER_TIMEOUT: f64 = 20.0; // seconds to wait for the game to answer a command (an enemy turn can take a while)

fn localtime(format: &str) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if format == "GMT" {
        unsafe { libc::gmtime_r(&now, &mut tm) };
        const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        return format!("{}, {:02} {} {} {:02}:{:02}:{:02} GMT", DAYS[tm.tm_wday as usize % 7], tm.tm_mday, MONTHS[tm.tm_mon as usize % 12], tm.tm_year + 1900, tm.tm_hour, tm.tm_min, tm.tm_sec);
    }
    unsafe { libc::localtime_r(&now, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

pub fn _log(message: &str) {
    eprintln!("{} relay: {}", localtime("%H:%M:%S"), message);
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the relay knows of the game (the original's attributes of the same names, under the condition's lock).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub seq: i64,                         // messages received from the game
    pub latest: Option<Value>,            // the last game state (errors are kept apart)
    pub last_error: Option<(i64, String)>, // (seq, text) of the last error message
    pub waiting: bool,                    // a command was sent and the game has not answered yet
}

/// Holds the game's latest message and passes commands to it, one at a time.
pub struct Relay {
    pub out: Mutex<Box<dyn Write + Send>>,
    pub answer_timeout: f64,
    pub state: Mutex<State>,
    pub cond: Condvar,
    pub send_lock: Mutex<()>,
}

/// Seconds as a wait: none for a time that is not ahead, a long one for one without end.
fn wait_time(seconds: f64) -> Duration {
    if seconds > 0.0 {
        Duration::try_from_secs_f64(seconds.min(1e9)).unwrap_or(Duration::ZERO)
    } else {
        Duration::ZERO
    }
}

impl Relay {
    /// `out`: where commands go (None: stdout). `answer_timeout`: ANSWER_TIMEOUT unless a test wants less.
    pub fn new(out: Option<Box<dyn Write + Send>>, answer_timeout: f64) -> Self {
        Relay { out: Mutex::new(out.unwrap_or_else(|| Box::new(std::io::stdout()))), answer_timeout, state: Mutex::new(State::default()), cond: Condvar::new(), send_lock: Mutex::new(()) }
    }

    /// Messages received from the game.
    pub fn seq(&self) -> i64 {
        locked(&self.state).seq
    }

    /// `out.write(text); out.flush()`.
    pub fn write(&self, text: &str) {
        let mut out = locked(&self.out);
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }

    /// `cond.wait_for(ready, timeout)`: the state once `ready` holds, or as it is when the time is up (and whether it held).
    fn wait_for<'a>(&self, state: MutexGuard<'a, State>, ready: impl Fn(&State) -> bool, timeout: f64) -> (MutexGuard<'a, State>, bool) {
        let (state, _) = self.cond.wait_timeout_while(state, wait_time(timeout), |s| !ready(s)).unwrap_or_else(|e| e.into_inner());
        let held = ready(&state);
        (state, held)
    }

    /// One line from the game.
    pub fn feed(&self, line: &str) {
        let line = base::strip(line);
        if line.is_empty() {
            return;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(msg) => msg,
            Err(_) => {
                _log(&format!("not JSON from the game: {}", line.chars().take(200).collect::<String>()));
                return;
            }
        };
        if !msg.is_object() {
            return;
        }
        let mut state = locked(&self.state);
        state.seq += 1;
        if !msg["error"].is_null() {
            state.last_error = Some((state.seq, base::str_of(&msg["error"])));
        } else {
            state.latest = Some(msg);
        }
        state.waiting = false;
        self.cond.notify_all();
    }

    pub fn snapshot(&self) -> Value {
        let state = locked(&self.state);
        json!({"seq": state.seq, "message": state.latest})
    }

    /// Pass one command to the game and wait for its answer (`timeout`: None for the relay's own).
    pub fn send(&self, command: &str, timeout: Option<f64>) -> Value {
        let command = command.split(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" "); // one line, whatever was sent
        if command.is_empty() {
            return json!({"status": "error", "error": "empty command"});
        }
        let timeout = timeout.unwrap_or(self.answer_timeout);
        let _one = locked(&self.send_lock);
        let state = locked(&self.state);
        // The game takes one command at a time: one sent before it answered the last is lost.
        let (mut state, _) = self.wait_for(state, |s| !s.waiting, timeout);
        let before = state.seq;
        state.waiting = true;
        self.write(&format!("{command}\n"));
        let (state, answered) = self.wait_for(state, |s| s.seq > before, timeout);
        if !answered {
            return json!({"status": "ok", "message": null, "pending": true, "seq": state.seq});
        }
        match &state.last_error {
            Some((at, text)) if *at > before => json!({"status": "error", "error": text, "seq": state.seq}),
            _ => json!({"status": "ok", "message": state.latest, "seq": state.seq}),
        }
    }
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        501 => "Not Implemented",
        _ => "",
    }
}

/// `float(x)` of a request's "timeout": None for what the original's would not take (its handler then answers nothing).
fn as_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(*b as i64 as f64),
        Value::String(s) => s.trim().replace('_', "").parse().ok(),
        _ => None,
    }
}

/// One request of the bot's: the answer's status code and body, or None for a request left without an answer.
pub fn handle(relay: &Relay, method: &str, path: &str, content_length: Option<&str>, body: &[u8]) -> Option<(u16, Value)> {
    match method {
        "GET" => Some((200, if path.starts_with("/state") { relay.snapshot() } else { json!({"status": "ok", "game": "sts1", "connected": relay.seq() > 0}) })),
        "POST" => {
            let bad = Some((400, json!({"status": "error", "error": "bad JSON"})));
            if content_length.is_some_and(|n| !n.trim().is_empty() && n.trim().parse::<i64>().is_err()) {
                return bad;
            }
            let Ok(body) = serde_json::from_slice::<Value>(if body.is_empty() { b"{}" } else { body }) else { return bad };
            if !path.starts_with("/command") || !body.is_object() {
                return Some((404, json!({"status": "error", "error": "unknown request"})));
            }
            let timeout = match &body["timeout"] {
                Value::Null => None,
                given => Some(as_float(given)?),
            };
            let result = relay.send(&body["command"].text(), timeout);
            Some((if result["status"] == "ok" { 200 } else { 400 }, result))
        }
        _ => Some((501, json!({"status": "error", "error": format!("Unsupported method ({})", base::str_repr(method))}))),
    }
}

fn answer(sock: &mut TcpStream, code: u16, body: &Value) {
    let raw = base::json_dumps(body);
    let head = format!("HTTP/1.0 {} {}\r\nServer: sts2bot relay\r\nDate: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", code, reason(code), localtime("GMT"), raw.len());
    let _ = sock.write_all(format!("{head}{raw}").as_bytes());
    let _ = sock.flush();
}

/// One connection: one request, one answer.
fn connection(relay: &Relay, sock: TcpStream) {
    let Ok(mut out) = sock.try_clone() else { return };
    let mut reader = BufReader::new(sock);
    let mut line: Vec<u8> = Vec::new();
    if reader.by_ref().take(65537).read_until(b'\n', &mut line).is_err() || line.is_empty() {
        return;
    }
    let first: String = line.iter().map(|b| *b as char).collect();
    let words: Vec<&str> = first.trim_end_matches(['\r', '\n']).split_whitespace().collect();
    if !(2..=3).contains(&words.len()) || (words.len() == 3 && !words[2].starts_with("HTTP/")) {
        answer(&mut out, 400, &json!({"status": "error", "error": format!("Bad request syntax ({})", base::str_repr(first.trim_end_matches(['\r', '\n'])))}));
        return;
    }
    let (method, path) = (words[0], words[1]);
    let mut content_length: Option<String> = None;
    loop {
        line.clear();
        if reader.by_ref().take(65537).read_until(b'\n', &mut line).is_err() {
            return;
        }
        let text: String = line.iter().map(|b| *b as char).collect();
        let text = text.trim_end_matches(['\r', '\n']);
        if text.is_empty() {
            break;
        }
        if let Some((name, value)) = text.split_once(':') {
            if content_length.is_none() && name.trim().eq_ignore_ascii_case("content-length") {
                content_length = Some(value.trim().to_string());
            }
        }
    }
    let mut body: Vec<u8> = Vec::new();
    if method == "POST" {
        match content_length.as_deref().unwrap_or("").parse::<i64>() {
            Ok(n) if n >= 0 => {
                let _ = reader.by_ref().take(n as u64).read_to_end(&mut body);
            }
            Ok(_) => {
                let _ = reader.read_to_end(&mut body); // (a negative length: all there is, as the original's read(-1))
            }
            Err(_) => {}
        }
    }
    if let Some((code, result)) = handle(relay, method, path, content_length.as_deref(), &body) {
        answer(&mut out, code, &result);
    }
    let _ = out.shutdown(std::net::Shutdown::Write);
}

/// The relay's HTTP side (the original's ThreadingHTTPServer): a thread for every connection.
pub struct Server {
    listener: Mutex<Option<TcpListener>>,
    pub server_address: SocketAddr,
    pub relay: Arc<Relay>,
    stop: AtomicBool,
    serving: Mutex<bool>,
    idle: Condvar,
}

impl Server {
    /// Answer requests until `shutdown`.
    pub fn serve_forever(&self) {
        let Some(listener) = locked(&self.listener).as_ref().and_then(|l| l.try_clone().ok()) else { return };
        self.stop.store(false, Ordering::SeqCst);
        *locked(&self.serving) = true;
        while !self.stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((sock, _)) => {
                    if self.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let relay = self.relay.clone();
                    std::thread::spawn(move || connection(&relay, sock));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        self.stop.store(false, Ordering::SeqCst);
        *locked(&self.serving) = false;
        self.idle.notify_all();
    }

    /// Have `serve_forever` return, and wait until it has. (Requests being answered go on.)
    pub fn shutdown(&self) {
        let mut serving = locked(&self.serving);
        if !*serving {
            return;
        }
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.server_address, Duration::from_secs(1)); // (wakes the wait for a connection)
        let deadline = Instant::now() + Duration::from_secs(5);
        while *serving && Instant::now() < deadline {
            serving = self.idle.wait_timeout(serving, Duration::from_millis(100)).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Stop listening.
    pub fn server_close(&self) {
        *locked(&self.listener) = None;
    }
}

/// Listen on localhost. A relay of an earlier game session may still be closing: wait for its port.
pub fn serve(port: u16, relay: Arc<Relay>, tries: usize) -> std::io::Result<Arc<Server>> {
    for attempt in 0..tries {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                let server_address = listener.local_addr()?;
                return Ok(Arc::new(Server { listener: Mutex::new(Some(listener)), server_address, relay, stop: AtomicBool::new(false), serving: Mutex::new(false), idle: Condvar::new() }));
            }
            Err(e) => {
                if attempt == tries - 1 {
                    return Err(e);
                }
                _log(&format!("port {} busy ({}); retrying", port, os_error_text(&e)));
                std::thread::sleep(Duration::from_secs_f64(1.0));
            }
        }
    }
    Err(std::io::Error::other("unreachable"))
}

/// The game sends its state when it has something to say; at start-up that can be never. Ask until it does.
pub fn _ask_until_heard(relay: &Relay, every: f64) {
    loop {
        std::thread::sleep(wait_time(every));
        if relay.seq() != 0 {
            return;
        }
        relay.send("state", Some(every));
    }
}

/// The relay's life: listen on `port`, say "ready" on `out`, pass on every line of `input` (the game), and return when
/// the game has closed it. (`main` with its two ends given: stdin and stdout there.)
pub fn run(port: u16, mut input: Box<dyn BufRead + Send>, out: Box<dyn Write + Send>) -> i32 {
    let relay = Arc::new(Relay::new(Some(out), ANSWER_TIMEOUT));
    let server = match serve(port, relay.clone(), 30) {
        Ok(server) => server,
        Err(e) => {
            _log(&format!("cannot listen on 127.0.0.1:{} ({})", port, os_error_text(&e)));
            return 1;
        }
    };
    let serving = server.clone();
    std::thread::spawn(move || serving.serve_forever());
    _log(&format!("listening on 127.0.0.1:{port}"));
    relay.write("ready\n"); // CommunicationMod waits for this before it sends anything
    let asking = relay.clone();
    std::thread::spawn(move || _ask_until_heard(&asking, 5.0));
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        match input.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => relay.feed(&String::from_utf8_lossy(&line)),
        }
    }
    _log("the game closed the connection; exiting");
    server.shutdown();
    0
}

const USAGE: &str = "usage: sts2bot relay [-h] [--port PORT]";

/// `sts2bot relay [--port PORT]`: what CommunicationMod starts. Does not return while the game is there, and ends the
/// process when it has gone (request threads may still be waiting on the game); returns only for a command line it
/// could not read (2), --help (0), or a port it could not listen on (1).
pub fn main(args: Vec<String>) -> i32 {
    let error = |message: String| {
        eprintln!("{USAGE}\nsts2bot relay: error: {message}");
        2
    };
    let mut port = DEFAULT_PORT as i64;
    let mut extras: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let word = args[i].as_str();
        i += 1;
        let (name, explicit) = match word.split_once('=') {
            Some((n, e)) => (n, Some(e.to_string())),
            None => (word, None),
        };
        let long = |full: &str| name.len() > 2 && name.starts_with("--") && full.starts_with(name);
        if word == "-h" || (long("--help") && !long("--port")) {
            println!("{USAGE}\n\nRelay between CommunicationMod (stdin/stdout) and the bot (localhost HTTP).\n\noptions:\n  -h, --help   show this help message and exit\n  --port PORT");
            return 0;
        } else if long("--port") && !long("--help") {
            let value = match explicit {
                Some(v) => v,
                None => match args.get(i) {
                    Some(v) if !v.starts_with('-') || v[1..].parse::<f64>().is_ok() => {
                        i += 1;
                        v.clone()
                    }
                    _ => return error("argument --port: expected one argument".to_string()),
                },
            };
            match value.trim().replace('_', "").parse::<i64>() {
                Ok(n) => port = n,
                Err(_) => return error(format!("argument --port: invalid int value: {}", base::str_repr(&value))),
            }
        } else {
            extras.push(word.to_string());
        }
    }
    if !extras.is_empty() {
        return error(format!("unrecognized arguments: {}", extras.join(" ")));
    }
    let Ok(port) = u16::try_from(port) else {
        _log(&format!("cannot listen on 127.0.0.1:{port} (bind(): port must be 0-65535.)"));
        return 1;
    };
    let code = run(port, Box::new(BufReader::new(std::io::stdin())), Box::new(std::io::stdout()));
    if code != 0 {
        return code;
    }
    std::process::exit(0) // (request threads may still be waiting on the game)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Client;
    use std::sync::OnceLock;

    /// Where a test's relay writes: kept, and answered by a pretend game a moment later.
    #[derive(Clone, Default)]
    struct Pipe {
        sent: Arc<Mutex<Vec<String>>>,
        relay: Arc<OnceLock<Arc<Relay>>>,
        game: Option<fn(&str) -> Value>,
    }

    impl Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            for line in String::from_utf8_lossy(buf).lines() {
                let line = line.trim().to_string();
                if line.is_empty() || line == "ready" {
                    continue;
                }
                self.sent.lock().unwrap().push(line.clone());
                if let (Some(game), Some(relay)) = (self.game, self.relay.get().cloned()) {
                    std::thread::spawn(move || relay.feed(&base::json_dumps(&game(&line))));
                }
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A game that starts a run when told to, and knows no other command.
    fn game(command: &str) -> Value {
        if command.starts_with("start ") {
            json!({"in_game": true, "available_commands": ["play", "end"], "game_state": {"class": command.split(' ').nth(1)}})
        } else if command == "state" {
            json!({"in_game": false, "available_commands": ["start"]})
        } else {
            json!({"error": format!("Invalid command: {command}"), "ready_for_command": true})
        }
    }

    fn relay_to(game: Option<fn(&str) -> Value>, timeout: f64) -> (Arc<Relay>, Pipe) {
        let pipe = Pipe { game, ..Default::default() };
        let relay = Arc::new(Relay::new(Some(Box::new(pipe.clone())), timeout));
        let _ = pipe.relay.set(relay.clone());
        (relay, pipe)
    }

    #[test]
    fn a_command_is_answered_with_the_games_next_message() {
        let (relay, pipe) = relay_to(Some(game), 2.0);
        relay.feed(&base::json_dumps(&json!({"in_game": false, "available_commands": ["start"]})));
        assert_eq!(relay.snapshot()["message"]["in_game"], false);
        let answer = relay.send("start  IRONCLAD\n0 ", None);
        assert_eq!(base::json_dumps(&answer), r#"{"status": "ok", "message": {"in_game": true, "available_commands": ["play", "end"], "game_state": {"class": "IRONCLAD"}}, "seq": 2}"#);
        let bad = relay.send("end", None); // (not on the map)
        assert_eq!(base::json_dumps(&bad), r#"{"status": "error", "error": "Invalid command: end", "seq": 3}"#);
        assert_eq!(relay.snapshot()["message"]["in_game"], true); // an error does not replace the state
        assert_eq!(relay.send(" \n ", None), json!({"status": "error", "error": "empty command"}));
        assert_eq!(*pipe.sent.lock().unwrap(), ["start IRONCLAD 0", "end"]);
    }

    #[test]
    fn a_game_that_does_not_answer_leaves_the_command_pending() {
        let (relay, _pipe) = relay_to(None, 0.05);
        assert_eq!(base::json_dumps(&relay.send("state", None)), r#"{"status": "ok", "message": null, "pending": true, "seq": 0}"#);
        relay.feed("not json"); // (ignored)
        relay.feed("[1, 2]");
        relay.feed("  \n");
        assert!(locked(&relay.state).waiting);
        relay.feed(&base::json_dumps(&json!({"in_game": false, "available_commands": ["start"]})));
        assert_eq!(relay.snapshot()["seq"], 1);
        assert!(!locked(&relay.state).waiting);
        relay.feed(r#"{"error": 5}"#);
        assert_eq!(locked(&relay.state).last_error, Some((2, "5".to_string())));
        assert_eq!(base::json_dumps(&relay.snapshot()), r#"{"seq": 2, "message": {"in_game": false, "available_commands": ["start"]}}"#);
    }

    #[test]
    fn requests_are_answered_as_the_the_record_relay_answered_them() {
        let (relay, _pipe) = relay_to(Some(game), 2.0);
        let ask = |method: &str, path: &str, body: &str| handle(&relay, method, path, Some(&body.len().to_string()), body.as_bytes()).map(|(code, v)| (code, base::json_dumps(&v)));
        let said = |code: u16, text: &str| Some((code, text.to_string()));
        assert_eq!(ask("GET", "/", ""), said(200, r#"{"status": "ok", "game": "sts1", "connected": false}"#));
        assert_eq!(ask("GET", "/state", ""), said(200, r#"{"seq": 0, "message": null}"#));
        assert_eq!(ask("POST", "/command", "{nope"), said(400, r#"{"status": "error", "error": "bad JSON"}"#));
        assert_eq!(handle(&relay, "POST", "/command", Some("x"), b"{}").map(|a| a.0), Some(400));
        assert_eq!(ask("POST", "/other", "{}"), said(404, r#"{"status": "error", "error": "unknown request"}"#));
        assert_eq!(ask("POST", "/command", "[1]"), said(404, r#"{"status": "error", "error": "unknown request"}"#));
        assert_eq!(ask("POST", "/command", ""), said(400, r#"{"status": "error", "error": "empty command"}"#));
        assert_eq!(ask("POST", "/command", r#"{"command": null}"#), said(400, r#"{"status": "error", "error": "empty command"}"#));
        assert_eq!(ask("POST", "/command?x=1", r#"{"command": "state", "timeout": "1.5"}"#), said(200, r#"{"status": "ok", "message": {"in_game": false, "available_commands": ["start"]}, "seq": 1}"#));
        assert_eq!(ask("GET", "/anything", ""), said(200, r#"{"status": "ok", "game": "sts1", "connected": true}"#));
        assert_eq!(ask("POST", "/command", r#"{"command": "end", "timeout": 2}"#), said(400, r#"{"status": "error", "error": "Invalid command: end", "seq": 2}"#));
        assert_eq!(ask("POST", "/command", r#"{"command": "state", "timeout": [1]}"#), None);
        assert_eq!(ask("PUT", "/command", "{}").map(|a| a.0), Some(501));
    }

    /// The requests of the golden file, sent over a socket one after another: the original relay's status lines and bodies.
    #[test]
    fn requests_over_http_as_recorded() {
        let golden: Value = serde_json::from_str(include_str!("../../../tests/golden/relay.json")).unwrap();
        let (relay, _pipe) = relay_to(Some(game), 2.0);
        let server = serve(0, relay, 30).unwrap();
        let serving = server.clone();
        std::thread::spawn(move || serving.serve_forever());
        assert!(golden.list().len() >= 25);
        for case in golden.list() {
            let mut head = format!("{} {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n", case["method"].s(), case["path"].s());
            if let Some(body) = case["body"].as_str() {
                head += &format!("Content-Length: {}\r\n", body.len());
            }
            let mut sock = TcpStream::connect(server.server_address).unwrap();
            sock.write_all(format!("{}\r\n{}", head, case["body"].s()).as_bytes()).unwrap();
            let mut raw = String::new();
            sock.read_to_string(&mut raw).unwrap();
            let (header, body) = raw.split_once("\r\n\r\n").unwrap();
            let lines: Vec<&str> = header.split("\r\n").collect();
            assert_eq!(lines[0], case["status"].s(), "{case}");
            assert_eq!(body, case["answer"].s(), "{case}");
            let names: Vec<Value> = lines[1..].iter().map(|l| json!(l.split(':').next().unwrap())).collect();
            assert_eq!(Value::Array(names), case["headers"], "{case}");
            assert!(header.contains(&format!("\r\nContent-Length: {}", body.len())), "{case}");
        }
        server.shutdown();
    }

    #[test]
    fn the_bot_reaches_the_relay_over_http() {
        let (relay, pipe) = relay_to(Some(game), 5.0);
        let server = serve(0, relay.clone(), 30).unwrap();
        let serving = server.clone();
        let thread = std::thread::spawn(move || serving.serve_forever());
        let client = Client::on_port(server.server_address.port());
        let base = client.base.clone();
        assert_eq!(client._request(&format!("{base}/"), None), Ok(json!({"status": "ok", "game": "sts1", "connected": false}))); // up, but the game has not said anything yet
        relay.feed(r#"{"in_game": false}"#);
        assert_eq!(client._request(&format!("{base}/"), None).unwrap()["connected"], true);
        assert_eq!(client._request(&format!("{base}/state"), None), Ok(json!({"seq": 1, "message": {"in_game": false}})));
        let started = client._request(&format!("{base}/command"), Some(&json!({"command": "start WATCHER 0"}))).unwrap();
        assert_eq!((started["status"].s(), started["message"]["game_state"]["class"].s(), started["seq"].int()), ("ok", "WATCHER", 2));
        // an error comes back as a 400 with the relay's own body (the client adds nothing to it)
        assert_eq!(client._request(&format!("{base}/command"), Some(&json!({"command": "end"}))), Ok(json!({"status": "error", "error": "Invalid command: end", "seq": 3})));
        assert_eq!(client._request(&format!("{base}/nowhere"), Some(&json!({}))), Ok(json!({"status": "error", "error": "unknown request"})));
        assert_eq!(*pipe.sent.lock().unwrap(), ["start WATCHER 0", "end"]);
        // the answer as it goes over the wire
        let mut sock = TcpStream::connect(server.server_address).unwrap();
        sock.write_all(b"GET /state HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut raw = String::new();
        sock.read_to_string(&mut raw).unwrap();
        assert!(raw.starts_with("HTTP/1.0 200 OK\r\n") && raw.contains("\r\nContent-Type: application/json\r\n"), "{raw}");
        assert!(raw.ends_with("\r\n\r\n{\"seq\": 3, \"message\": {\"in_game\": true, \"available_commands\": [\"play\", \"end\"], \"game_state\": {\"class\": \"WATCHER\"}}}"), "{raw}");
        server.shutdown();
        thread.join().unwrap();
        server.server_close();
        assert!(!client.ping());
    }

    #[test]
    fn the_relay_says_ready_and_leaves_with_the_game() {
        #[derive(Clone, Default)]
        struct Out(Arc<Mutex<Vec<u8>>>);
        impl Write for Out {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let out = Out::default();
        // the game says one thing and is gone
        let code = run(0, Box::new(std::io::Cursor::new(b"{\"in_game\": false}\n".to_vec())), Box::new(out.clone()));
        assert_eq!(code, 0);
        assert_eq!(String::from_utf8_lossy(&out.0.lock().unwrap()), "ready\n"); // what CommunicationMod waits for
    }

    #[test]
    fn command_lines_that_cannot_be_read_are_refused() {
        let words = |w: &[&str]| w.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        assert_eq!(main(words(&["--port", "x"])), 2);
        assert_eq!(main(words(&["--port"])), 2);
        assert_eq!(main(words(&["extra"])), 2);
        assert_eq!(main(words(&["--help"])), 0);
        assert_eq!(main(words(&["--port=70000"])), 1);
    }
}
