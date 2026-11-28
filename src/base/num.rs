//! the original's arithmetic where Rust's differs: `sum` of floats (compensated since the original 3.12), `round` (half to even),
//! `//` and `%` (towards minus infinity), `max`/`min` (the first of equals).

use std::cmp::Ordering;

/// A running `sum(...)` the way the first version+ adds: floats with Neumaier compensation, ints added plainly.
/// `Sum::new(start).f(x).i(n).get()`; for a sum of floats alone use [`fsum`].
#[derive(Clone, Copy, Debug)]
pub struct Sum {
    total: f64,
    c: f64,
}

impl Sum {
    pub fn new(start: f64) -> Self {
        Sum { total: start, c: 0.0 }
    }

    /// Add a float item.
    pub fn add_f(&mut self, x: f64) {
        let t = self.total + x;
        if self.total.abs() >= x.abs() {
            self.c += (self.total - t) + x;
        } else {
            self.c += (x - t) + self.total;
        }
        self.total = t;
    }

    /// Add an int item met in a float sum (the first version adds it without compensation).
    pub fn add_i(&mut self, n: i64) {
        self.total += n as f64;
    }

    pub fn get(&self) -> f64 {
        if self.c != 0.0 && self.c.is_finite() {
            self.total + self.c
        } else {
            self.total
        }
    }
}

/// `sum(floats)`: every item a float (the start an int 0 or a float 0.0).
pub fn fsum<I: IntoIterator<Item = f64>>(items: I) -> f64 {
    fsum_from(0.0, items)
}

/// `sum(floats, start)` with a float start.
pub fn fsum_from<I: IntoIterator<Item = f64>>(start: f64, items: I) -> f64 {
    let mut s = Sum::new(start);
    for x in items {
        s.add_f(x);
    }
    s.get()
}

/// `max(a, b)` of two floats as the original has it: the first unless the second is greater.
#[inline]
pub fn fmax(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// `min(a, b)` of two floats as the original has it: the first unless the second is less.
#[inline]
pub fn fmin(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// `round(x)`: to the nearest integer, halves to the even one.
pub fn round(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// `round(x, ndigits)`: correctly rounded on the decimal value, as the original does it.
pub fn round_to(x: f64, ndigits: i32) -> f64 {
    if !x.is_finite() {
        return x;
    }
    if ndigits >= 0 {
        format!("{:.*}", ndigits as usize, x).parse().unwrap_or(x)
    } else {
        let p = 10f64.powi(-ndigits);
        (x / p).round_ties_even() * p
    }
}

/// `a // b` for ints.
pub fn floordiv(a: i64, b: i64) -> i64 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

/// `a % b` for ints (the sign of `b`).
pub fn modulo(a: i64, b: i64) -> i64 {
    let r = a % b;
    if r != 0 && ((r < 0) != (b < 0)) {
        r + b
    } else {
        r
    }
}

/// `a % b` for floats (the sign of `b`).
pub fn fmodulo(a: f64, b: f64) -> f64 {
    let r = a % b;
    if r != 0.0 && ((r < 0.0) != (b < 0.0)) {
        r + b
    } else if r == 0.0 {
        0.0f64.copysign(b)
    } else {
        r
    }
}

/// `a // b` for floats.
pub fn ffloordiv(a: f64, b: f64) -> f64 {
    let m = fmodulo(a, b);
    let div = (a - m) / b;
    if div == 0.0 {
        return 0.0f64.copysign(a / b);
    }
    let fl = div.floor();
    if div - fl > 0.5 {
        fl + 1.0
    } else {
        fl
    }
}

/// `int(x)` of a float: towards zero.
pub fn trunc(x: f64) -> i64 {
    x.trunc() as i64
}

fn cmp<K: PartialOrd>(a: &K, b: &K) -> Ordering {
    a.partial_cmp(b).unwrap_or(Ordering::Equal)
}

/// `max(items, key=key)`: the FIRST of the largest (Rust's `max_by_key` gives the last). None when empty.
pub fn max_by_key<T, K: PartialOrd, I: IntoIterator<Item = T>, F: FnMut(&T) -> K>(items: I, mut key: F) -> Option<T> {
    let mut best: Option<(T, K)> = None;
    for item in items {
        let k = key(&item);
        match &best {
            Some((_, bk)) if cmp(&k, bk) != Ordering::Greater => {}
            _ => best = Some((item, k)),
        }
    }
    best.map(|(t, _)| t)
}

/// `min(items, key=key)`: the first of the smallest. None when empty.
pub fn min_by_key<T, K: PartialOrd, I: IntoIterator<Item = T>, F: FnMut(&T) -> K>(items: I, mut key: F) -> Option<T> {
    let mut best: Option<(T, K)> = None;
    for item in items {
        let k = key(&item);
        match &best {
            Some((_, bk)) if cmp(&k, bk) != Ordering::Less => {}
            _ => best = Some((item, k)),
        }
    }
    best.map(|(t, _)| t)
}

/// `max(items)` of anything ordered the way the original orders it (floats, tuples of them): the first of the largest.
pub fn max_of<T: PartialOrd, I: IntoIterator<Item = T>>(items: I) -> Option<T> {
    let mut best: Option<T> = None;
    for item in items {
        match &best {
            Some(b) if cmp(&item, b) != Ordering::Greater => {}
            _ => best = Some(item),
        }
    }
    best
}

/// `sorted(items, key=key)` / `list.sort(key=key)`: stable, on keys that may hold floats.
pub fn sort_by_key<T, K: PartialOrd, F: FnMut(&T) -> K>(items: &mut [T], mut key: F) {
    items.sort_by(|a, b| cmp(&key(a), &key(b)));
}

/// `sorted(items, key=key, reverse=True)`: stable, equal items keep their order (as the original's reverse sort does).
pub fn sort_by_key_rev<T, K: PartialOrd, F: FnMut(&T) -> K>(items: &mut [T], mut key: F) {
    items.sort_by(|a, b| cmp(&key(b), &key(a)));
}

/// `math.comb(n, k)`.
pub fn comb(n: i64, k: i64) -> i64 {
    if k < 0 || k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut out: i128 = 1;
    for i in 0..k {
        out = out * (n - i) as i128 / (i + 1) as i128;
    }
    out as i64
}
