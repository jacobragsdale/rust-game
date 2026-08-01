//! The simulation's only source of randomness: seeded, explicit, and written
//! out here rather than pulled from a crate.
//!
//! A golden trace recorded today has to reproduce in three years. That is a
//! promise about a specific sequence of bits, not about an API, so the
//! generator is spelled out in full below and pinned by
//! [`tests::the_sequence_is_fixed_forever`]. Swapping in a library — however
//! good — would make every recorded trace hostage to that library's next
//! release, and `rand`'s default generator has changed algorithm before.
//!
//! The rule that matters more than the plumbing: **nothing in the game may
//! call any other source of randomness.** Time, thread-local generators and
//! hash iteration order are all invisible in the source and fatal to a tape.
//! [`tests::nothing_reaches_for_an_unseeded_generator`] enforces the first two.
//!
//! The algorithm is SplitMix64 (Steele, Lea and Flood, 2014; the same one
//! Java's `SplittableRandom` uses). It is a counter run through a fixed
//! finalizing mix, which is why it is six lines long, has a full 2^64 period
//! from *any* seed including zero, and needs no "the state must not be zero"
//! special case.

/// Where every sim starts unless something says otherwise. The constant is
/// `"SuperGam"` in ASCII — arbitrary, and arbitrary on purpose: it just has to
/// never change, because every golden trace is recorded at it.
pub const DEFAULT_SEED: u64 = 0x5375_7065_7247_616D;

/// The golden-ratio increment SplitMix64 walks its counter by.
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// A seeded random number generator.
///
/// Deterministic given its seed, and `Send + Sync` because it is two integers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    /// The seed it was created with, kept so a run can report what it used.
    seed: u64,
    /// The counter, which is the whole state.
    state: u64,
}

impl Default for Rng {
    fn default() -> Self {
        Rng::new(DEFAULT_SEED)
    }
}

impl Rng {
    pub const fn new(seed: u64) -> Rng {
        Rng { seed, state: seed }
    }

    /// The seed this generator was created with.
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// The next 64 bits. Everything else here is built on this one function.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// The high 32 bits, which are the best-mixed ones.
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// A float in `[0, 1)`.
    ///
    /// Built from 24 bits, which is exactly `f32`'s mantissa: taking more
    /// would round, and rounding up from 0.999... would produce a 1.0 that
    /// every caller assumes cannot happen.
    pub fn next_f32(&mut self) -> f32 {
        const SCALE: f32 = 1.0 / (1u32 << 24) as f32;
        (self.next_u32() >> 8) as f32 * SCALE
    }

    /// A float in `[lo, hi)`. Reversed bounds give `[hi, lo)`; an empty range
    /// gives `lo`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    /// True with probability `p`. `p <= 0` never fires and `p >= 1` always
    /// does, since [`Rng::next_f32`] is in `[0, 1)`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    /// One element, or `None` from an empty slice.
    ///
    /// The index is a multiply-shift rather than a modulo: `%` biases toward
    /// low indices, which for a loot table means the first entry quietly drops
    /// more often than the last.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        let index = (u64::from(self.next_u32()) * items.len() as u64) >> 32;
        items.get(index as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one test that must never be "fixed" by updating its expected
    /// values. These are the published SplitMix64 vectors for seed 1; if this
    /// fails, the generator changed and every golden trace recorded against it
    /// is now meaningless.
    #[test]
    fn the_sequence_is_fixed_forever() {
        let mut rng = Rng::new(1);
        let actual: Vec<u64> = (0..8).map(|_| rng.next_u64()).collect();
        assert_eq!(
            actual,
            vec![
                0x910A_2DEC_8902_5CC1,
                0xBEEB_8DA1_658E_EC67,
                0xF893_A2EE_FB32_555E,
                0x71C1_8690_EE42_C90B,
                0x71BB_54D8_D101_B5B9,
                0xC34D_0BFF_9015_0280,
                0xE099_EC6C_D736_3CA5,
                0x85E7_BB0F_1227_8575,
            ]
        );
    }

    /// Seed zero is an ordinary seed. Xorshift generators need a special case
    /// here; a counter-based one does not, and this is the check that the
    /// choice was actually made rather than assumed.
    #[test]
    fn a_zero_seed_is_not_degenerate() {
        let mut rng = Rng::new(0);
        let values: Vec<u64> = (0..4).map(|_| rng.next_u64()).collect();
        assert!(values.iter().all(|&v| v != 0));
        assert_eq!(values[0], 0xE220_A839_7B1D_CDAF, "the published vector");
    }

    #[test]
    fn the_same_seed_replays_and_a_different_one_does_not() {
        let sample = |seed| {
            let mut rng = Rng::new(seed);
            (0..64).map(|_| rng.next_u64()).collect::<Vec<u64>>()
        };
        assert_eq!(sample(DEFAULT_SEED), sample(DEFAULT_SEED));
        assert_ne!(sample(DEFAULT_SEED), sample(DEFAULT_SEED + 1));
        assert_ne!(sample(1), sample(2));
    }

    #[test]
    fn floats_stay_inside_the_unit_interval() {
        let mut rng = Rng::new(DEFAULT_SEED);
        for _ in 0..100_000 {
            let value = rng.next_f32();
            assert!((0.0..1.0).contains(&value), "{value} is out of [0, 1)");
        }
    }

    /// Uniformity, not just "in bounds": a generator whose low bits are stuck
    /// still passes a bounds check.
    #[test]
    fn range_is_uniform_over_a_large_sample() {
        const BUCKETS: usize = 16;
        const SAMPLES: usize = 320_000;
        let (lo, hi) = (-2.0f32, 6.0f32);

        let mut rng = Rng::new(DEFAULT_SEED);
        let mut counts = [0usize; BUCKETS];
        let mut sum = 0.0f64;
        for _ in 0..SAMPLES {
            let value = rng.range(lo, hi);
            assert!(value >= lo && value < hi, "{value} escaped [{lo}, {hi})");
            sum += f64::from(value);
            let bucket = ((value - lo) / (hi - lo) * BUCKETS as f32) as usize;
            counts[bucket.min(BUCKETS - 1)] += 1;
        }

        let expected = SAMPLES as f64 / BUCKETS as f64;
        for (index, &count) in counts.iter().enumerate() {
            let error = (count as f64 - expected).abs() / expected;
            assert!(
                error < 0.05,
                "bucket {index} was {count}, expected ~{expected}"
            );
        }
        let mean = sum / SAMPLES as f64;
        assert!((mean - 2.0).abs() < 0.05, "mean was {mean}, expected ~2.0");
    }

    #[test]
    fn chance_fires_at_about_its_probability() {
        let mut rng = Rng::new(DEFAULT_SEED);
        let hits = (0..100_000).filter(|_| rng.chance(0.25)).count();
        assert!((24_000..26_000).contains(&hits), "{hits} hits in 100k");

        assert!(!rng.chance(0.0), "impossible things do not happen");
        assert!(rng.chance(1.0), "certain things do");
    }

    #[test]
    fn pick_covers_every_element_and_handles_an_empty_slice() {
        let items = ["a", "b", "c", "d", "e"];
        let mut rng = Rng::new(DEFAULT_SEED);
        let mut counts = [0usize; 5];
        for _ in 0..50_000 {
            let choice = rng.pick(&items).expect("non-empty");
            let index = items.iter().position(|i| i == choice).unwrap();
            counts[index] += 1;
        }
        for (index, &count) in counts.iter().enumerate() {
            assert!(
                (9_000..11_000).contains(&count),
                "`{}` came up {count} times in 50k",
                items[index]
            );
        }

        let empty: [u8; 0] = [];
        assert_eq!(rng.pick(&empty), None);
    }

    /// The rule the whole module exists to enforce. Any unseeded generator
    /// anywhere in `src/` makes tapes and golden traces meaningless, and it is
    /// invisible in a diff unless something looks for it.
    #[test]
    fn nothing_reaches_for_an_unseeded_generator() {
        // Assembled at runtime so this test does not match its own source.
        let banned = [
            concat!("thread", "_rng"),
            concat!("rand::", "random"),
            concat!("SystemTime::", "now"),
        ];

        let mut offenders = Vec::new();
        let mut stack = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("src/ is readable") {
                let path = entry.expect("readable entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("source is readable");
                for needle in banned {
                    if text.contains(needle) {
                        offenders.push(format!("{}: {needle}", path.display()));
                    }
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "unseeded randomness in the simulation makes every golden trace \
             meaningless — use `Sim::rng`:\n  {}",
            offenders.join("\n  ")
        );
    }
}
