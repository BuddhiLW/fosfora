//! Property-test kit for control ops: a seeded generator and a case runner.
//! Deterministic (a failure prints its seed and case number, and replays),
//! and dependency-free.

/// xorshift64*: small, fast, good enough to explore op inputs.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `0..n` (`0` when `n == 0`).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        usize::try_from(self.next_u64() % n as u64).unwrap_or(0)
    }

    /// Uniform in `lo..=hi`.
    pub fn range_i(&mut self, lo: isize, hi: isize) -> isize {
        let span = usize::try_from(hi - lo).unwrap_or(0) + 1;
        lo + isize::try_from(self.below(span)).unwrap_or(0)
    }

    pub fn coin(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// A short name from a small alphabet, so collisions happen.
    pub fn name(&mut self) -> String {
        const SYL: &[&str] = &["ta", "ne", "ri", "Do", "pe", " ", "Copy", "x"];
        let len = 1 + self.below(4);
        (0..len).map(|_| SYL[self.below(SYL.len())]).collect()
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        items.get(self.below(items.len()))
    }
}

/// Run `body` for `n` cases from `seed`, naming the failing case.
pub fn cases(n: usize, seed: u64, mut body: impl FnMut(&mut Rng, usize)) {
    let mut rng = Rng::new(seed);
    for case in 0..n {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&mut rng, case)));
        if let Err(e) = result {
            eprintln!("property failed: seed {seed}, case {case}");
            std::panic::resume_unwind(e);
        }
    }
}
