//! Regular expressions the way the Python code used them: compiled once and kept.
//!
//! `re!(r"(?i)gain\s+(\d+)")` is a `&'static regex::Regex` compiled on first use; `fre!` the same from `fancy_regex`
//! (for look-around and back-references, which `regex` lacks). [`cached`] keeps patterns built at run time.
//! Python's `re.I` is `(?i)` at the front; `re.match` needs `^`, `re.fullmatch` `^(?:...)$`; in a replacement
//! `\1` is `${1}`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[macro_export]
macro_rules! re {
    ($pattern:expr) => {{
        static RE: ::std::sync::OnceLock<::regex::Regex> = ::std::sync::OnceLock::new();
        RE.get_or_init(|| ::regex::Regex::new($pattern).expect("regex"))
    }};
}

#[macro_export]
macro_rules! fre {
    ($pattern:expr) => {{
        static RE: ::std::sync::OnceLock<::fancy_regex::Regex> = ::std::sync::OnceLock::new();
        RE.get_or_init(|| ::fancy_regex::Regex::new($pattern).expect("regex"))
    }};
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Rc<regex::Regex>>> = RefCell::new(HashMap::new());
    static FANCY: RefCell<HashMap<String, Rc<fancy_regex::Regex>>> = RefCell::new(HashMap::new());
}

/// A pattern built at run time, compiled once per thread (`re.search(pattern, ...)` with a pattern passed in).
pub fn cached(pattern: &str) -> Rc<regex::Regex> {
    CACHE.with(|c| {
        if let Some(r) = c.borrow().get(pattern) {
            return r.clone();
        }
        let r = Rc::new(regex::Regex::new(pattern).unwrap_or_else(|e| panic!("regex {pattern:?}: {e}")));
        c.borrow_mut().insert(pattern.to_string(), r.clone());
        r
    })
}

/// [`cached`], case-insensitive (`re.I`).
pub fn cached_i(pattern: &str) -> Rc<regex::Regex> {
    cached(&format!("(?i){pattern}"))
}

/// A run-time pattern that needs look-around or back-references.
pub fn cached_fancy(pattern: &str) -> Rc<fancy_regex::Regex> {
    FANCY.with(|c| {
        if let Some(r) = c.borrow().get(pattern) {
            return r.clone();
        }
        let r = Rc::new(fancy_regex::Regex::new(pattern).unwrap_or_else(|e| panic!("regex {pattern:?}: {e}")));
        c.borrow_mut().insert(pattern.to_string(), r.clone());
        r
    })
}

/// `re.escape(s)`.
pub fn escape(s: &str) -> String {
    regex::escape(s)
}
