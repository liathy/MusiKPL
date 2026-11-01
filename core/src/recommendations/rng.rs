//! A tiny random number generator (SplitMix64) for varying Home, so it
//! doesn't need a crate. Not for anything security-related.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// Seeded from the clock: different on every run.
    pub fn from_time() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos() as u64);
        Self::new(nanos ^ u64::from(std::process::id()).rotate_left(32))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n` > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// A number in `0.0..1.0`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            items.swap(index, self.below(index + 1));
        }
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }

    /// Take up to `count` items, each draw favoring higher weights, without
    /// repeats. Higher-weighted items are likelier but not certain.
    pub fn weighted_sample<T: Clone>(&mut self, items: &[(T, f64)], count: usize) -> Vec<(T, f64)> {
        let mut pool: Vec<(T, f64)> = items.to_vec();
        let mut picked = Vec::new();
        while picked.len() < count && !pool.is_empty() {
            let total: f64 = pool.iter().map(|(_, weight)| weight.max(0.0)).sum();
            let index = if total <= 0.0 {
                0
            } else {
                let mut target = self.unit() * total;
                pool.iter()
                    .position(|(_, weight)| {
                        target -= weight.max(0.0);
                        target <= 0.0
                    })
                    .unwrap_or(pool.len() - 1)
            };
            picked.push(pool.remove(index));
        }
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_stay_in_range_and_samples_dont_repeat() {
        let mut rng = Rng::new(7);
        for _ in 0..1000 {
            assert!(rng.below(5) < 5);
            let unit = rng.unit();
            assert!((0.0..1.0).contains(&unit));
        }
        let items = [("a", 10.0), ("b", 1.0), ("c", 0.5)];
        let sample = rng.weighted_sample(&items, 3);
        let mut names: Vec<_> = sample.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        assert_eq!(names, ["a", "b", "c"]);
        assert_eq!(rng.weighted_sample(&items, 9).len(), 3);
    }

    #[test]
    fn heavier_items_are_picked_more_often() {
        let mut rng = Rng::new(42);
        let items = [("heavy", 9.0), ("light", 1.0)];
        let heavy = (0..1000)
            .filter(|_| rng.weighted_sample(&items, 1)[0].0 == "heavy")
            .count();
        assert!(heavy > 800, "{heavy}");
    }
}
