//! Constants that can be given another value for a while: by a test, or by a script trying `NAME=VALUE`.
//!
//! ```ignore
//! tunables! { pub RAGE_BLOCK: i64 = 99; pub HP_BASE: f64 = 1.5; }
//! let b = RAGE_BLOCK.get();
//! let _back = RAGE_BLOCK.set(0);          // until `_back` is dropped, on this thread
//! set_tunable("HP_BASE", 2.0);            // by name, for good (generated per `tunables!` block)
//! ```
//! Reading costs one thread-local check while nothing is overridden. Overrides are per thread: tests running side by
//! side and model runs on worker threads do not see each other's.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

pub trait TunableValue: Copy + 'static {
    fn to_f64(self) -> f64;
    fn from_f64(x: f64) -> Self;
}

impl TunableValue for f64 {
    fn to_f64(self) -> f64 {
        self
    }
    fn from_f64(x: f64) -> Self {
        x
    }
}

impl TunableValue for i64 {
    fn to_f64(self) -> f64 {
        self as f64
    }
    fn from_f64(x: f64) -> Self {
        x as i64
    }
}

impl TunableValue for bool {
    fn to_f64(self) -> f64 {
        self as i64 as f64
    }
    fn from_f64(x: f64) -> Self {
        x != 0.0
    }
}

thread_local! {
    static ACTIVE: Cell<usize> = const { Cell::new(0) };
    static OVERRIDES: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

pub struct Tunable<T: TunableValue> {
    default: T,
}

/// Puts the earlier value back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct Restore {
    key: usize,
    before: Option<f64>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        put(self.key, self.before);
    }
}

fn put(key: usize, value: Option<f64>) {
    OVERRIDES.with(|o| {
        let mut o = o.borrow_mut();
        match value {
            Some(v) => o.insert(key, v),
            None => o.remove(&key),
        };
        ACTIVE.with(|a| a.set(o.len()));
    });
}

impl<T: TunableValue> Tunable<T> {
    pub const fn new(default: T) -> Self {
        Tunable { default }
    }

    fn key(&'static self) -> usize {
        self as *const Self as usize
    }

    #[inline]
    pub fn get(&'static self) -> T {
        if ACTIVE.with(|a| a.get()) == 0 {
            return self.default;
        }
        OVERRIDES.with(|o| o.borrow().get(&self.key()).map(|v| T::from_f64(*v))).unwrap_or(self.default)
    }

    /// Another value on this thread until the guard is dropped.
    pub fn set(&'static self, value: T) -> Restore {
        let key = self.key();
        let before = OVERRIDES.with(|o| o.borrow().get(&key).copied());
        put(key, Some(value.to_f64()));
        Restore { key, before }
    }

    /// Another value on this thread from now on.
    pub fn set_forever(&'static self, value: T) {
        put(self.key(), Some(value.to_f64()));
    }

    pub fn default(&'static self) -> T {
        self.default
    }
}

/// Declares tunable constants and, for the block, `set_tunable(name, value) -> bool` and `TUNABLES: &[&str]`.
#[macro_export]
macro_rules! tunables {
    ($($(#[$meta:meta])* $vis:vis $name:ident : $ty:ty = $value:expr;)*) => {
        $($(#[$meta])* $vis static $name: $crate::tunable::Tunable<$ty> = $crate::tunable::Tunable::new($value);)*
        /// The names of this module's tunable constants.
        #[allow(dead_code)]
        pub const TUNABLES: &[&str] = &[$(stringify!($name)),*];
        /// Give the constant called `name` another value on this thread from now on; false if this module has none by that name.
        #[allow(dead_code)]
        pub fn set_tunable(name: &str, value: f64) -> bool {
            match name {
                $(stringify!($name) => { $name.set_forever(<$ty as $crate::tunable::TunableValue>::from_f64(value)); true })*
                _ => false,
            }
        }
    };
}
