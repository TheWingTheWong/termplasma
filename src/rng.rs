//! A tiny deterministic xorshift32 generator.
//!
//! Effects need cheap noise, not cryptographic randomness, and determinism
//! makes headless runs and tests reproducible.

#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    /// Creates a generator; a zero seed is remapped since xorshift would
    /// otherwise get stuck at zero forever.
    pub fn new(seed: u32) -> Self {
        Rng(if seed == 0 { 0x9e37_79b9 } else { seed })
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform integer in `0..n` (`n > 0`), via the multiply-shift trick.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next_u32() as u64 * n as u64) >> 32) as u32
    }

    /// Uniform `f32` in `[0, 1)`.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform `f32` in `[-1, 1)`.
    #[inline]
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_nonzero() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            let v = a.next_u32();
            assert_eq!(v, b.next_u32());
            assert_ne!(v, 0);
        }
        assert_ne!(Rng::new(0).next_u32(), 0);
    }

    #[test]
    fn ranges() {
        let mut r = Rng::new(7);
        let mut seen = [false; 5];
        for _ in 0..10_000 {
            let v = r.below(5);
            seen[v as usize] = true;
            let u = r.unit();
            assert!((0.0..1.0).contains(&u));
            let s = r.signed();
            assert!((-1.0..1.0).contains(&s));
        }
        assert!(seen.iter().all(|&s| s), "every bucket is reachable");
    }
}
