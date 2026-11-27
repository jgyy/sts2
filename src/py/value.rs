//! The game's state is JSON read as `serde_json::Value`; these are the ways the Python code read its dicts.
//! `v["key"]` on a `Value` already gives `Value::Null` for a missing key or a non-object, like `(d or {}).get("key")`.

use serde_json::{Map, Value};

static EMPTY: Vec<Value> = Vec::new();

pub trait JsonExt {
    /// Python truthiness: None, False, 0, 0.0, "", [] and {} are false.
    fn truthy(&self) -> bool;
    /// `int(x or 0)`: a number (a float towards zero), a bool, or a string of digits; anything else 0.
    fn int(&self) -> i64;
    /// `x if x is not None else default`, as an int.
    fn int_or(&self, default: i64) -> i64;
    /// `float(x or 0)`.
    fn float(&self) -> f64;
    fn float_or(&self, default: f64) -> f64;
    /// The string, or "" for anything that is not one (`x or ""` where x is a str or None).
    fn s(&self) -> &str;
    /// `str(x or "")`: a string as it is, a number or bool as Python prints it, "" for None and other falsy values.
    fn text(&self) -> String;
    /// `x or []`: the items of a list, none for anything else.
    fn list(&self) -> &[Value];
    /// The dict, if it is one.
    fn dict(&self) -> Option<&Map<String, Value>>;
    /// `key in d` for a dict (false for anything else).
    fn has(&self, key: &str) -> bool;
    /// `d.items()` (nothing for a non-dict).
    fn entries(&self) -> Box<dyn Iterator<Item = (&String, &Value)> + '_>;
    /// `isinstance(x, int)` and not a bool.
    fn is_int(&self) -> bool;
    /// `x is None` (also a missing key read through `v["key"]`).
    fn is_none(&self) -> bool;
    /// `self or other`.
    fn or<'a>(&'a self, other: &'a Value) -> &'a Value;
}

impl JsonExt for Value {
    fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Object(m) => !m.is_empty(),
        }
    }

    fn int(&self) -> i64 {
        self.int_or(0)
    }

    fn int_or(&self, default: i64) -> i64 {
        match self {
            Value::Null => default,
            Value::Bool(b) => *b as i64,
            Value::Number(n) => n.as_i64().unwrap_or_else(|| n.as_f64().map(|x| x.trunc() as i64).unwrap_or(default)),
            Value::String(s) => s.trim().parse().unwrap_or(default),
            _ => default,
        }
    }

    fn float(&self) -> f64 {
        self.float_or(0.0)
    }

    fn float_or(&self, default: f64) -> f64 {
        match self {
            Value::Null => default,
            Value::Bool(b) => *b as i64 as f64,
            Value::Number(n) => n.as_f64().unwrap_or(default),
            Value::String(s) => s.trim().parse().unwrap_or(default),
            _ => default,
        }
    }

    fn s(&self) -> &str {
        self.as_str().unwrap_or("")
    }

    fn text(&self) -> String {
        if self.truthy() {
            super::text::str_of(self)
        } else {
            String::new()
        }
    }

    fn list(&self) -> &[Value] {
        self.as_array().unwrap_or(&EMPTY)
    }

    fn dict(&self) -> Option<&Map<String, Value>> {
        self.as_object()
    }

    fn has(&self, key: &str) -> bool {
        self.as_object().map(|m| m.contains_key(key)).unwrap_or(false)
    }

    fn entries(&self) -> Box<dyn Iterator<Item = (&String, &Value)> + '_> {
        match self.as_object() {
            Some(m) => Box::new(m.iter()),
            None => Box::new(std::iter::empty()),
        }
    }

    fn is_int(&self) -> bool {
        matches!(self, Value::Number(n) if n.is_i64() || n.is_u64())
    }

    fn is_none(&self) -> bool {
        self.is_null()
    }

    fn or<'a>(&'a self, other: &'a Value) -> &'a Value {
        if self.truthy() {
            self
        } else {
            other
        }
    }
}
