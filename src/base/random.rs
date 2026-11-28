//! the original's `random.Random`, bit for bit: the Mersenne Twister and the methods the bot and the model runs use,
//! so that a seeded run draws exactly what the original version drew.

const N: usize = 624;
const M: usize = 397;

#[derive(Clone)]
pub struct Random {
    mt: [u32; N],
    idx: usize,
    gauss_next: Option<f64>,
}

impl std::fmt::Debug for Random {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Random(idx={})", self.idx)
    }
}

impl Default for Random {
    fn default() -> Self {
        Self::unseeded()
    }
}

impl Random {
    /// `random.Random(seed)` for an integer seed (its absolute value, as the original takes it).
    pub fn new(seed: i128) -> Self {
        let mut r = Random { mt: [0; N], idx: N + 1, gauss_next: None };
        r.seed(seed);
        r
    }

    /// `random.Random()`: seeded from the clock and the process.
    pub fn unseeded() -> Self {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let here = &t as *const _ as usize as u128;
        Self::new(((t ^ (here << 64) ^ ((std::process::id() as u128) << 32)) & (u128::MAX >> 1)) as i128)
    }

    pub fn seed(&mut self, seed: i128) {
        let mut a = seed.unsigned_abs();
        let mut key: Vec<u32> = Vec::new();
        while a > 0 {
            key.push((a & 0xffff_ffff) as u32);
            a >>= 32;
        }
        if key.is_empty() {
            key.push(0);
        }
        self.init_by_array(&key);
        self.gauss_next = None;
    }

    fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for i in 1..N {
            self.mt[i] = 1812433253u32.wrapping_mul(self.mt[i - 1] ^ (self.mt[i - 1] >> 30)).wrapping_add(i as u32);
        }
        self.idx = N;
    }

    fn init_by_array(&mut self, key: &[u32]) {
        self.init_genrand(19650218);
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..N.max(key.len()) {
            self.mt[i] = (self.mt[i] ^ (self.mt[i - 1] ^ (self.mt[i - 1] >> 30)).wrapping_mul(1664525)).wrapping_add(key[j]).wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            self.mt[i] = (self.mt[i] ^ (self.mt[i - 1] ^ (self.mt[i - 1] >> 30)).wrapping_mul(1566083941)).wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
        }
        self.mt[0] = 0x8000_0000;
    }

    fn genrand(&mut self) -> u32 {
        if self.idx >= N {
            for k in 0..N {
                let y = (self.mt[k] & 0x8000_0000) | (self.mt[(k + 1) % N] & 0x7fff_ffff);
                self.mt[k] = self.mt[(k + M) % N] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.idx = 0;
        }
        let mut y = self.mt[self.idx];
        self.idx += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `random()`: a float in [0, 1).
    pub fn random(&mut self) -> f64 {
        let a = (self.genrand() >> 5) as f64;
        let b = (self.genrand() >> 6) as f64;
        (a * 67108864.0 + b) * (1.0 / 9007199254740992.0)
    }

    /// `getrandbits(k)` for 0 < k <= 64.
    pub fn getrandbits(&mut self, k: u32) -> u64 {
        assert!(k > 0 && k <= 64, "getrandbits: 1..=64 bits");
        if k <= 32 {
            return (self.genrand() >> (32 - k)) as u64;
        }
        let low = self.genrand() as u64;
        let high = (self.genrand() >> (64 - k)) as u64;
        low | (high << 32)
    }

    /// `_randbelow(n)`: an integer in [0, n), n > 0.
    pub fn randbelow(&mut self, n: u64) -> u64 {
        assert!(n > 0, "randbelow: empty range");
        let k = 64 - n.leading_zeros();
        let mut r = self.getrandbits(k);
        while r >= n {
            r = self.getrandbits(k);
        }
        r
    }

    /// `randrange(start, stop)`.
    pub fn randrange(&mut self, start: i64, stop: i64) -> i64 {
        assert!(stop > start, "randrange: empty range");
        start + self.randbelow((stop - start) as u64) as i64
    }

    /// `randrange(stop)`.
    pub fn randrange1(&mut self, stop: i64) -> i64 {
        self.randrange(0, stop)
    }

    /// `randint(a, b)`: both ends included.
    pub fn randint(&mut self, a: i64, b: i64) -> i64 {
        self.randrange(a, b + 1)
    }

    /// The index `choice` would pick in a sequence of `len` items.
    pub fn choice_index(&mut self, len: usize) -> usize {
        assert!(len > 0, "choice from an empty sequence");
        self.randbelow(len as u64) as usize
    }

    /// `choice(seq)`.
    pub fn choice<'a, T>(&mut self, seq: &'a [T]) -> &'a T {
        &seq[self.choice_index(seq.len())]
    }

    /// `shuffle(seq)`.
    pub fn shuffle<T>(&mut self, seq: &mut [T]) {
        for i in (1..seq.len()).rev() {
            let j = self.randbelow(i as u64 + 1) as usize;
            seq.swap(i, j);
        }
    }

    /// The indices `sample(population, k)` would pick from a population of `n`, in order.
    pub fn sample_indices(&mut self, n: usize, k: usize) -> Vec<usize> {
        assert!(k <= n, "sample larger than population");
        let mut setsize = 21usize;
        if k > 5 {
            setsize += 4usize.pow(((k * 3) as f64).log(4.0).ceil() as u32);
        }
        let mut out = Vec::with_capacity(k);
        if n <= setsize {
            let mut pool: Vec<usize> = (0..n).collect();
            for i in 0..k {
                let j = self.randbelow((n - i) as u64) as usize;
                out.push(pool[j]);
                pool[j] = pool[n - i - 1];
            }
        } else {
            let mut seen = std::collections::HashSet::new();
            for _ in 0..k {
                let mut j = self.randbelow(n as u64) as usize;
                while seen.contains(&j) {
                    j = self.randbelow(n as u64) as usize;
                }
                seen.insert(j);
                out.push(j);
            }
        }
        out
    }

    /// `sample(population, k)`.
    pub fn sample<T: Clone>(&mut self, population: &[T], k: usize) -> Vec<T> {
        self.sample_indices(population.len(), k).into_iter().map(|i| population[i].clone()).collect()
    }

    /// `uniform(a, b)`.
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.random()
    }

    /// `gauss(mu, sigma)`.
    pub fn gauss(&mut self, mu: f64, sigma: f64) -> f64 {
        let z = match self.gauss_next.take() {
            Some(z) => z,
            None => {
                let x2pi = self.random() * std::f64::consts::TAU;
                let g2rad = (-2.0 * (1.0 - self.random()).ln()).sqrt();
                self.gauss_next = Some(x2pi.sin() * g2rad);
                x2pi.cos() * g2rad
            }
        };
        mu + z * sigma
    }
}
