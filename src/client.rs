//! Thin HTTP client for the STS2MCP bridge mod (localhost REST API), over a plain socket (an HTTP library would add megabytes of TLS and proxy code).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::base::{self, JsonExt};

/// The game (or the bridge mod inside it) is not answering.
#[derive(Clone, Debug, PartialEq)]
pub struct GameUnavailable(pub String);

impl GameUnavailable {
    pub fn new(message: impl Into<String>) -> Self {
        GameUnavailable(message.into())
    }
}

impl std::fmt::Display for GameUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GameUnavailable {}

/// What the bot needs of a game: the bridge mod's client, the first game's client, or a test's fake game.
/// `params` is a JSON object of the action's named arguments (`{"card_index": 2, "target": "JAW_WORM_0"}`; `{}` or null for none).
pub trait Game {
    fn state(&mut self) -> Result<Value, GameUnavailable>;
    fn act(&mut self, action: &str, params: &Value) -> Result<Value, GameUnavailable>;
    /// Profile statistics, including each character's highest unlocked ascension.
    fn profile(&mut self) -> Result<Value, GameUnavailable>;
    /// The game's own entry for a card we have met: its text as it is and as it is upgraded.
    fn card(&mut self, card_id: &str) -> Result<Option<Value>, GameUnavailable>;
    fn ping(&mut self) -> bool;
}

/// `str(e)` of the OSError the original would have raised: "[Errno 111] Connection refused", "timed out".
pub fn os_error_text(e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) {
        return "timed out".to_string();
    }
    match e.raw_os_error() {
        Some(code) => {
            let text = unsafe { std::ffi::CStr::from_ptr(libc::strerror(code)) }.to_string_lossy().into_owned();
            format!("[Errno {code}] {text}")
        }
        None => e.to_string(),
    }
}

fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

fn base_strip(s: &str) -> &str {
    s.trim_matches(is_py_space)
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|b| *b as char).collect()
}

fn bytes_repr(bytes: &[u8]) -> String {
    let quote = if bytes.contains(&b'\'') && !bytes.contains(&b'"') { '"' } else { '\'' };
    let mut out = String::from("b");
    out.push(quote);
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b if b as char == quote => {
                out.push('\\');
                out.push(b as char);
            }
            b if !(0x20..0x7f).contains(&b) => out.push_str(&format!("\\x{:02x}", b)),
            b => out.push(b as char),
        }
    }
    out.push(quote);
    out
}

/// `int(text, base)` for base 10 or 16: blanks around it, a sign, `0x` before hex digits, single underscores between
/// digits. None for anything else (the original's ValueError). Numbers beyond an i64 stop at its ends.
fn base_int(text: &str, base: u32, ascii_blanks: bool) -> Option<i64> {
    let t = if ascii_blanks { text.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')) } else { base_strip(text) };
    let (negative, mut t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if base == 16 {
        if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
            t = rest.strip_prefix('_').unwrap_or(rest);
        }
    }
    let mut value: i64 = 0;
    let mut last_digit = false;
    for c in t.chars() {
        if c == '_' {
            if !last_digit {
                return None;
            }
            last_digit = false;
            continue;
        }
        let d = c.to_digit(base)? as i64;
        value = value.saturating_mul(base as i64).saturating_add(d);
        last_digit = true;
    }
    if !last_digit {
        return None;
    }
    Some(if negative { -value } else { value })
}

/// A position as the original's slices and `index` read it: from the end when negative, kept inside the bytes.
fn clamp(i: i64, len: usize) -> usize {
    if i < 0 {
        (i.saturating_add(len as i64)).max(0) as usize
    } else {
        (i as u64).min(len as u64) as usize
    }
}

fn slice(body: &[u8], a: i64, b: i64) -> &[u8] {
    let (a, b) = (clamp(a, body.len()), clamp(b, body.len()));
    if a >= b {
        &[]
    } else {
        &body[a..b]
    }
}

/// The body of a response sent with "Transfer-Encoding: chunked". (The error: the text of the original's ValueError.)
pub fn _dechunk(body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut pos: i64 = 0;
    loop {
        let start = clamp(pos, body.len());
        let eol = if pos > body.len() as i64 { None } else { body[start..].windows(2).position(|w| w == b"\r\n").map(|i| (start + i) as i64) };
        let eol = eol.ok_or_else(|| "subsection not found".to_string())?;
        let line = slice(body, pos, eol);
        let word = line.split(|b| *b == b';').next().unwrap_or(&[]);
        let size = base_int(&latin1(word), 16, true).ok_or_else(|| format!("invalid literal for int() with base 16: {}", bytes_repr(word)))?;
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(slice(body, eol + 2, (eol + 2).saturating_add(size)));
        pos = (eol + 2).saturating_add(size).saturating_add(2);
    }
}

/// The parts of a URL the request needs: (the host with its port, the path with its query).
fn urlsplit(url: &str) -> (String, String) {
    let rest = match url.find("://") {
        Some(i) => &url[i + 3..],
        None => url,
    };
    let rest = rest.split('#').next().unwrap_or("");
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (netloc, tail) = rest.split_at(end);
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, q),
        None => (tail, ""),
    };
    let mut target = if path.is_empty() { "/".to_string() } else { path.to_string() };
    if !query.is_empty() {
        target.push('?');
        target.push_str(query);
    }
    (netloc.to_string(), target)
}

/// What a whole answer read off the socket says: (status code, body).
pub fn _parse(raw: &[u8]) -> Result<(i64, Vec<u8>), GameUnavailable> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n");
    let (header, sep, body) = match split {
        Some(i) => (&raw[..i], true, &raw[i + 4..]),
        None => (raw, false, &raw[raw.len()..]),
    };
    let header = latin1(header);
    let lines: Vec<&str> = header.split("\r\n").collect();
    let status: Vec<&str> = lines[0].splitn(3, ' ').collect();
    if !sep || status.len() < 2 || !status[0].starts_with("HTTP/") {
        return Err(GameUnavailable::new("not an HTTP answer from the game"));
    }
    let mut fields: Vec<(String, &str)> = Vec::new();
    for line in &lines[1..] {
        let (k, v) = line.split_once(':').unwrap_or((line, ""));
        let (k, v) = (base_strip(k).to_lowercase(), base_strip(v));
        match fields.iter_mut().find(|(name, _)| *name == k) {
            Some(field) => field.1 = v,
            None => fields.push((k, v)),
        }
    }
    let field = |name: &str| fields.iter().find(|(k, _)| k == name).map(|(_, v)| *v).unwrap_or("");
    let bad_int = |text: &str| GameUnavailable(format!("invalid literal for int() with base 10: {}", base::str_repr(text)));
    let length = field("content-length");
    let body = if field("transfer-encoding").to_lowercase().contains("chunked") {
        _dechunk(body).map_err(GameUnavailable)?
    } else if !length.is_empty() && length.chars().all(|c| c.is_ascii_digit() || matches!(c, '\u{b2}' | '\u{b3}' | '\u{b9}')) {
        let n = base_int(length, 10, false).ok_or_else(|| bad_int(length))?;
        slice(body, 0, n).to_vec()
    } else {
        body.to_vec()
    };
    let code = base_int(status[1], 10, false).ok_or_else(|| bad_int(status[1]))?;
    Ok((code, body))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Client {
    pub base: String,
    pub url: String,
    pub timeout: f64,
    pub _address: (String, u16), // (a socket of our own: localhost traffic never goes through a system proxy)
}

impl Default for Client {
    fn default() -> Self {
        Client::new("127.0.0.1", 15526, 15.0)
    }
}

impl Client {
    pub fn new(host: &str, port: u16, timeout: f64) -> Self {
        let base = format!("http://{host}:{port}");
        Client { url: format!("{base}/api/v1/singleplayer"), base, timeout, _address: (host.to_string(), port) }
    }

    /// `Client(port=port)`.
    pub fn on_port(port: u16) -> Self {
        Client::new("127.0.0.1", port, 15.0)
    }

    fn connect(&self) -> std::io::Result<TcpStream> {
        let timeout = Duration::try_from_secs_f64(self.timeout).ok().filter(|t| !t.is_zero());
        let mut last: Option<std::io::Error> = None;
        for address in (self._address.0.as_str(), self._address.1).to_socket_addrs()? {
            let made = match timeout {
                Some(t) => TcpStream::connect_timeout(&address, t),
                None => TcpStream::connect(address),
            };
            match made {
                Ok(sock) => {
                    sock.set_read_timeout(timeout)?;
                    sock.set_write_timeout(timeout)?;
                    return Ok(sock);
                }
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("getaddrinfo returns an empty list")))
    }

    /// Send one request and read the whole answer: (status code, body).
    pub fn _exchange(&self, url: &str, data: Option<&[u8]>) -> Result<(i64, Vec<u8>), GameUnavailable> {
        let (netloc, path) = urlsplit(url);
        let mut head = format!("{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: application/json\r\n", if data.is_none() { "GET" } else { "POST" }, path, netloc);
        if let Some(data) = data {
            head += &format!("Content-Type: application/json\r\nContent-Length: {}\r\n", data.len());
        }
        let mut message: Vec<u8> = Vec::with_capacity(head.len() + 2 + data.map_or(0, |d| d.len()));
        for (i, c) in head.chars().enumerate() {
            if c as u32 > 0xff {
                return Err(GameUnavailable(format!("'latin-1' codec can't encode character {} in position {}: ordinal not in range(256)", base::str_repr(&c.to_string()), i)));
            }
            message.push(c as u32 as u8);
        }
        message.extend_from_slice(b"\r\n");
        message.extend_from_slice(data.unwrap_or(b""));
        let io = |e: std::io::Error| GameUnavailable(os_error_text(&e));
        let mut sock = self.connect().map_err(io)?;
        sock.write_all(&message).map_err(io)?;
        let mut raw: Vec<u8> = Vec::new();
        let mut chunk = vec![0u8; 65536];
        loop {
            match sock.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => raw.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(io(e)),
            }
        }
        drop(sock);
        _parse(&raw)
    }

    pub fn _request(&self, url: &str, body: Option<&Value>) -> Result<Value, GameUnavailable> {
        let data = body.map(|b| base::json_dumps(b).into_bytes());
        let (code, raw) = self._exchange(url, data.as_deref())?; // (also a status or chunk size that is not a number)
        if code >= 400 {
            // the mod reports action failures as 4xx/5xx with a JSON body
            let mut parsed = match serde_json::from_slice::<Value>(&raw) {
                Ok(Value::Object(m)) => m,
                _ => Map::new(),
            };
            parsed.entry("status").or_insert_with(|| json!("error"));
            parsed.entry("error").or_insert_with(|| json!(format!("HTTP {code}")));
            return Ok(Value::Object(parsed));
        }
        match serde_json::from_slice::<Value>(&raw) {
            Ok(parsed @ Value::Object(_)) => Ok(parsed),
            Ok(parsed) => Ok(json!({ "value": parsed })),
            Err(e) => Err(GameUnavailable(format!("bad JSON from game: {e}"))),
        }
    }

    pub fn ping(&self) -> bool {
        self._request(&format!("{}/", self.base), None).is_ok()
    }

    /// Profile statistics, including each character's highest unlocked ascension.
    pub fn profile(&self) -> Result<Value, GameUnavailable> {
        self._request(&format!("{}/api/v1/profile", self.base), None)
    }

    /// The game's own entry for a card we have met: its text as it is and as it is upgraded.
    pub fn card(&self, card_id: &str) -> Result<Option<Value>, GameUnavailable> {
        let found = self._request(&format!("{}/api/v1/wiki?type=card&limit=3&q={}", self.base, base::url_quote(card_id)), None)?;
        for entry in found["results"].list() {
            if base::str_of(&entry["id"]) == card_id {
                return Ok(Some(entry.clone()));
            }
        }
        Ok(None)
    }

    pub fn state(&self) -> Result<Value, GameUnavailable> {
        self._request(&self.url, None)
    }

    /// `params`: the action's named arguments as a JSON object; they follow "action" in the request, in their order.
    pub fn act(&self, action: &str, params: &Value) -> Result<Value, GameUnavailable> {
        let mut body = Map::new();
        body.insert("action".to_string(), json!(action));
        for (k, v) in params.entries() {
            body.insert(k.clone(), v.clone());
        }
        self._request(&self.url, Some(&Value::Object(body)))
    }
}

impl Game for Client {
    fn state(&mut self) -> Result<Value, GameUnavailable> {
        Client::state(self)
    }

    fn act(&mut self, action: &str, params: &Value) -> Result<Value, GameUnavailable> {
        Client::act(self, action, params)
    }

    fn profile(&mut self) -> Result<Value, GameUnavailable> {
        Client::profile(self)
    }

    fn card(&mut self, card_id: &str) -> Result<Option<Value>, GameUnavailable> {
        Client::card(self, card_id)
    }

    fn ping(&mut self) -> bool {
        Client::ping(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{:02x}", x)).collect()
    }

    /// A game that answers every request with the next of `answers` as it is, and keeps what it was sent.
    fn canned(answers: Vec<Vec<u8>>) -> (u16, std::thread::JoinHandle<Vec<Vec<u8>>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for answer in answers {
                let (mut sock, _) = listener.accept().unwrap();
                let mut got: Vec<u8> = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    // the head, then as many bytes as it says follow
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
                seen.push(got);
                sock.write_all(&answer).unwrap();
            }
            seen
        });
        (port, handle)
    }

    #[test]
    fn chunked_answers_are_read_whole() {
        assert_eq!(_dechunk(b"4\r\n{\"a\"\r\n5;x=1\r\n: 12}\r\n0\r\n\r\n").unwrap(), b"{\"a\": 12}");
    }

    #[test]
    fn dechunk_as_recorded() {
        let golden: Value = serde_json::from_str(include_str!("../tests/golden/client.json")).unwrap();
        let cases = golden["dechunk"].list();
        assert!(cases.len() > 30);
        for case in cases {
            let got = _dechunk(&unhex(case["body"].s()));
            match &case["error"] {
                Value::Null => assert_eq!(got.as_ref().map(|b| hex(b)), Ok(case["out"].s().to_string()), "{case}"),
                e => assert_eq!(got, Err(e.s().to_string()), "{case}"),
            }
        }
    }

    /// Every crafted answer, served over a real socket: `_exchange` and `_request` give what the original's gave.
    #[test]
    fn answers_as_recorded() {
        let golden: Value = serde_json::from_str(include_str!("../tests/golden/client.json")).unwrap();
        let cases = golden["answers"].list();
        assert!(cases.len() > 40);
        let (port, handle) = canned(cases.iter().flat_map(|c| [unhex(c["raw"].s()), unhex(c["raw"].s())]).collect());
        let client = Client::new("127.0.0.1", port, 5.0);
        for case in cases {
            let raw = unhex(case["raw"].s());
            assert_eq!(_parse(&raw), client._exchange(&client.url, None), "{case}");
            match (&case["exchange_error"], _parse(&raw)) {
                (Value::Null, Ok((code, body))) => assert_eq!((code, hex(&body)), (case["code"].int(), case["body"].s().to_string()), "{case}"),
                (e, got) => assert_eq!(got, Err(GameUnavailable(e.s().to_string())), "{case}"),
            }
            let got = client._request(&client.url, Some(&json!({"action": "end_turn"})));
            match (&case["request_error"], got) {
                (Value::Null, Ok(v)) => assert_eq!(base::json_dumps(&v), case["request"].s(), "{case}"),
                // (the JSON reader's own words differ from the original's)
                (e, Err(got)) if e.s().starts_with("bad JSON from game: ") => assert!(got.0.starts_with("bad JSON from game: "), "{case}"),
                (e, got) => assert_eq!(got, Err(GameUnavailable(e.s().to_string())), "{case}"),
            }
        }
        let seen = handle.join().unwrap();
        assert_eq!(seen.len(), 2 * cases.len());
    }

    #[test]
    fn requests_are_sent_as_the_record_sent_them() {
        let golden: Value = serde_json::from_str(include_str!("../tests/golden/client.json")).unwrap();
        let ok = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".to_vec();
        let wiki = b"HTTP/1.1 200 OK\r\n\r\n{\"results\": [{\"id\": \"OTHER\"}, {\"id\": \"A B/C+\", \"n\": 1}]}".to_vec();
        let (port, handle) = canned(vec![ok.clone(), ok.clone(), ok.clone(), wiki.clone(), wiki, ok.clone(), ok]);
        let mut client = Client::on_port(port);
        assert!(client.ping());
        assert_eq!(client.state(), Ok(json!({})));
        assert_eq!(client.profile(), Ok(json!({})));
        assert_eq!(client.card("A B/C+"), Ok(Some(json!({"id": "A B/C+", "n": 1}))));
        assert_eq!(client.card("ÉCLAIR"), Ok(None));
        assert_eq!(client.act("play_card", &json!({"card_index": 2, "target": "JAW_WORM_0", "x": 1.5, "name": "é"})), Ok(json!({})));
        assert_eq!(Game::act(&mut client, "end_turn", &Value::Null), Ok(json!({})));
        let seen = handle.join().unwrap();
        let sent = golden["sent"].list();
        assert_eq!(seen.len(), sent.len());
        for (got, want) in seen.iter().zip(sent) {
            let want = String::from_utf8(unhex(want.s())).unwrap().replace("PORT", &port.to_string());
            // (Content-Length is of the body alone: the port's digits do not change it)
            assert_eq!(String::from_utf8_lossy(got), want);
        }
    }

    #[test]
    fn a_game_that_is_not_there_is_unavailable() {
        let port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let client = Client::on_port(port);
        assert_eq!(client.state(), Err(GameUnavailable("[Errno 111] Connection refused".to_string())));
        assert!(!client.ping());
        // a game that accepts and says nothing: the timeout applies to the read
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let slow = Client::new("127.0.0.1", listener.local_addr().unwrap().port(), 0.2);
        assert_eq!(slow.state(), Err(GameUnavailable("timed out".to_string())));
    }
}
