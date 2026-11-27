//! Python's text forms: `repr` and `str` of floats, `json.dumps`, and a few `str` methods.

use serde_json::Value;

/// `repr(x)` / `str(x)` of a float: the shortest digits that read back the same, "1.0" for whole numbers, "1e-05" and "1e+16" beyond the fixed range.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let mut sci = format!("{:e}", x.abs());
    if sci.split_once('e').unwrap().0.len() >= 18 {
        sci = format!("{:.16e}", x.abs()); // 17 digits: the nearest ones (a tie to even), as Python picks them
    }
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let sign = if x < 0.0 { "-" } else { "" };
    let body = if !(-4..16).contains(&exp) {
        let m = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits.clone() };
        format!("{}e{}{:02}", m, if exp < 0 { "-" } else { "+" }, exp.abs())
    } else if exp < 0 {
        format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
    } else {
        let int_len = exp as usize + 1;
        if digits.len() <= int_len {
            format!("{}{}.0", digits, "0".repeat(int_len - digits.len()))
        } else {
            format!("{}.{}", &digits[..int_len], &digits[int_len..])
        }
    };
    format!("{sign}{body}")
}

/// `f"{x:g}"` (also `%g`): six significant digits, trailing zeros dropped.
pub fn float_g(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if !x.is_finite() {
        return float_repr(x);
    }
    let sci = format!("{:.5e}", x);
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let strip = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if !(-4..6).contains(&exp) {
        format!("{}e{}{:02}", strip(mant), if exp < 0 { "-" } else { "+" }, exp.abs())
    } else {
        strip(&format!("{:.*}", (5 - exp).max(0) as usize, x))
    }
}

fn json_str(s: &str, ensure_ascii: bool, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ensure_ascii && (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", u));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// How `json.dumps` is to write: Python's defaults unless changed.
#[derive(Clone, Copy, Debug)]
pub struct Dumps {
    pub indent: Option<usize>,
    pub sort_keys: bool,
    pub ensure_ascii: bool,
    /// (item separator, key separator); None: Python's default for the indent
    pub separators: Option<(&'static str, &'static str)>,
}

impl Default for Dumps {
    fn default() -> Self {
        Dumps { indent: None, sort_keys: false, ensure_ascii: true, separators: None }
    }
}

impl Dumps {
    pub fn dumps(&self, v: &Value) -> String {
        let (item, key) = self.separators.unwrap_or(if self.indent.is_some() { (",", ": ") } else { (", ", ": ") });
        let mut out = String::new();
        self.write(v, 0, item, key, &mut out);
        out
    }

    fn newline(&self, depth: usize, out: &mut String) {
        if let Some(n) = self.indent {
            out.push('\n');
            out.push_str(&" ".repeat(n * depth));
        }
    }

    fn write(&self, v: &Value, depth: usize, item: &str, key: &str, out: &mut String) {
        match v {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => {
                if n.is_f64() {
                    let x = n.as_f64().unwrap();
                    out.push_str(&if x.is_nan() {
                        "NaN".to_string()
                    } else if x.is_infinite() {
                        if x > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
                    } else {
                        float_repr(x)
                    });
                } else {
                    out.push_str(&n.to_string());
                }
            }
            Value::String(s) => json_str(s, self.ensure_ascii, out),
            Value::Array(a) => {
                if a.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push_str(item);
                    }
                    self.newline(depth + 1, out);
                    self.write(x, depth + 1, item, key, out);
                }
                self.newline(depth, out);
                out.push(']');
            }
            Value::Object(m) => {
                if m.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                let mut entries: Vec<(&String, &Value)> = m.iter().collect();
                if self.sort_keys {
                    entries.sort_by(|a, b| a.0.cmp(b.0));
                }
                for (i, (k, x)) in entries.into_iter().enumerate() {
                    if i > 0 {
                        out.push_str(item);
                    }
                    self.newline(depth + 1, out);
                    json_str(k, self.ensure_ascii, out);
                    out.push_str(key);
                    self.write(x, depth + 1, item, key, out);
                }
                self.newline(depth, out);
                out.push('}');
            }
        }
    }
}

/// `json.dumps(v)` with Python's defaults (", " and ": " between items, non-ASCII escaped, floats as `repr`).
pub fn json_dumps(v: &Value) -> String {
    Dumps::default().dumps(v)
}

/// `json.dumps(v, indent=n, sort_keys=sort_keys)`.
pub fn json_dumps_pretty(v: &Value, indent: usize, sort_keys: bool) -> String {
    Dumps { indent: Some(indent), sort_keys, ..Default::default() }.dumps(v)
}

/// `str(v)` of a JSON value as Python prints the object it stands for (a string bare, None, True, 1.0; containers as `repr`).
pub fn str_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => repr_of(other),
    }
}

/// `repr(v)` of a JSON value as Python prints the object it stands for.
pub fn repr_of(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True".into() } else { "False".into() },
        Value::Number(n) => if n.is_f64() { float_repr(n.as_f64().unwrap()) } else { n.to_string() },
        Value::String(s) => str_repr(s),
        Value::Array(a) => format!("[{}]", a.iter().map(repr_of).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => format!("{{{}}}", m.iter().map(|(k, x)| format!("{}: {}", str_repr(k), repr_of(x))).collect::<Vec<_>>().join(", ")),
    }
}

/// `repr(s)` of a string: single quotes unless it holds one (and no double quote).
pub fn str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `s.title()`: the first letter of every run of letters in upper case, the rest lower.
pub fn title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_cased = false;
    for ch in s.chars() {
        if ch.is_alphabetic() {
            if prev_cased {
                out.extend(ch.to_lowercase());
            } else {
                out.extend(ch.to_uppercase());
            }
            prev_cased = true;
        } else {
            out.push(ch);
            prev_cased = false;
        }
    }
    out
}

/// `s.capitalize()`.
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars.flat_map(|c| c.to_lowercase())).collect(),
        None => String::new(),
    }
}

/// `urllib.parse.quote(s)` (safe="/").
pub fn url_quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}
