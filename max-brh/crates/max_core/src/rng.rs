//! Deterministic core RNG — an exact port of `max-cpp-port/src/randomizer.cpp`.
//!
//! Algorithm (DESIGN.md §4.1):
//! * seeding — **SplitMix64**: `state += 0x9E3779B97F4A7C15`, then
//!   `z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9`,
//!   `z = (z ^ (z >> 27)) * 0x94D049BB133111EB`, `z ^ (z >> 31)`;
//! * the four least significant 32-bit words of SplitMix64 form the state of
//!   [`Randomizer`] (as does the `Xoshiro128StarStar(uint64_t)` constructor in
//!   `XoshiroCpp.hpp`);
//! * generation — **Xoshiro128\*\***: `rotl(s[1] * 5, 7) * 9` and the standard
//!   state update.
//!
//! `Randomizer_Generate<T>` (§4.2) — two semantic branches:
//! * `T <= u32` (and `i8..=i32`): `(next as i64 * value) >> 32` (arithmetic
//!   shift, `i64` multiplication);
//! * `T == u64` (`sizeof(...)`/`size_t` sites): `(next as u64 * value) >> 32`
//!   (logical shift, unsigned multiplication).
//!
//! The crate does not use `rand` and does not touch time: the only
//! seeding points are [`Randomizer::new`]/[`Randomizer::set_seed`] (in simulation —
//! `Command::Seed`). The state is serializable and enters the CRC digest.

use serde::{Deserialize, Serialize};

/// Default seed (`XoshiroCpp::DefaultSeed = 1234567890`).
pub const DEFAULT_SEED: u32 = 1_234_567_890;

const SPLITMIX_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const SPLITMIX_MUL1: u64 = 0xBF58_476D_1CE4_E5B9;
const SPLITMIX_MUL2: u64 = 0x94D0_49BB_1331_11EB;

/// One SplitMix64 step; the state is `u64`.
#[inline]
fn splitmix64_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(SPLITMIX_GAMMA);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(SPLITMIX_MUL1);
    z = (z ^ (z >> 27)).wrapping_mul(SPLITMIX_MUL2);
    z ^ (z >> 31)
}

/// The M.A.X. generator — Xoshiro128\*\* seeded with SplitMix64.
///
/// State is `[u32; 4]`, exactly like `XoshiroCpp::Xoshiro128StarStar::state_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Randomizer {
    state: [u32; 4],
}

impl Default for Randomizer {
    fn default() -> Self {
        Self::new(DEFAULT_SEED)
    }
}

impl Randomizer {
    /// Creates a generator with seed `seed` (like `Randomizer_SetSeed`).
    #[inline]
    pub fn new(seed: u32) -> Self {
        let mut randomizer = Self { state: [0; 4] };
        randomizer.set_seed(seed);
        randomizer
    }

    /// Reseeds the generator (like `Randomizer_SetSeed`, including reseed).
    pub fn set_seed(&mut self, seed: u32) {
        let mut splitmix_state = seed as u64;
        for slot in &mut self.state {
            // XoshiroCpp: `static_cast<uint32_t>(splitmix())`.
            *slot = splitmix64_next(&mut splitmix_state) as u32;
        }
    }

    /// Current state `[u32; 4]` (for CRC/saves).
    #[inline]
    pub const fn state(&self) -> [u32; 4] {
        self.state
    }

    /// Restores the state (save loading).
    #[inline]
    pub fn from_state(state: [u32; 4]) -> Self {
        Self { state }
    }

    /// Overwrites the state (save loading).
    #[inline]
    pub fn set_state(&mut self, state: [u32; 4]) {
        self.state = state;
    }

    /// Next 32-bit value — `Xoshiro128StarStar::operator()`.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let result = self.state[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.state[1] << 9;
        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= t;
        self.state[3] = self.state[3].rotate_left(11);
        result
    }

    /// Port of `Randomizer_Generate<T>` for all supported types.
    ///
    /// Performs exactly one [`Self::next_u32`] call and masks the result by type
    /// `T` (see [`GenerateValue`]).
    #[inline]
    pub fn generate<T: GenerateValue>(&mut self, value: T) -> T {
        T::generate_from(self.next_u32(), value)
    }

    /// `i64` branch (C++ `T` — `int8_t..int32_t`/`uint8_t..uint32_t`):
    /// `(next as i64 * value) >> 32`.
    #[inline]
    pub fn generate_i64(&mut self, value: i64) -> i64 {
        ((self.next_u32() as i64).wrapping_mul(value)) >> 32
    }

    /// `u64` branch (C++ `size_t`, `sizeof(...)` sites):
    /// `(next as u64 * value) >> 32`.
    #[inline]
    pub fn generate_u64(&mut self, value: u64) -> u64 {
        ((self.next_u32() as u64).wrapping_mul(value)) >> 32
    }

    /// `Randomizer_Generate<uint32_t>(value)`.
    #[inline]
    pub fn generate_u32(&mut self, value: u32) -> u32 {
        self.generate(value)
    }

    /// `Randomizer_Generate<int32_t>(value)`.
    #[inline]
    pub fn generate_i32(&mut self, value: i32) -> i32 {
        self.generate(value)
    }
}

/// Semantics of `Randomizer_Generate<T>` for a concrete type `T`.
///
/// Implemented exactly for the types occurring in the C++ port: `i8..=i32`,
/// `u8..=u32`, `i64`, `u64`, `bool`. `char` is deliberately not implemented: in C++
/// `char` has platform-defined signedness, while Rust `char` is a Unicode scalar; all
/// byte sites are ported via `i8`/`u8`.
pub trait GenerateValue: Copy {
    /// Converts one `next_u32()` value into a result of type `Self`.
    fn generate_from(next: u32, value: Self) -> Self;
}

macro_rules! impl_generate_signed {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl GenerateValue for $ty {
                #[inline]
                fn generate_from(next: u32, value: Self) -> Self {
                    ((next as i64).wrapping_mul(value as i64) >> 32) as Self
                }
            }
        )+
    };
}

macro_rules! impl_generate_unsigned {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl GenerateValue for $ty {
                #[inline]
                fn generate_from(next: u32, value: Self) -> Self {
                    ((next as i64).wrapping_mul(value as i64) >> 32) as Self
                }
            }
        )+
    };
}

impl_generate_signed!(i8, i16, i32, i64);
impl_generate_unsigned!(u8, u16, u32);

impl GenerateValue for u64 {
    #[inline]
    fn generate_from(next: u32, value: Self) -> Self {
        ((next as u64).wrapping_mul(value)) >> 32
    }
}

impl GenerateValue for bool {
    #[inline]
    fn generate_from(next: u32, value: Self) -> Self {
        // C++: `static_cast<bool>((int64_t(next) * static_cast<int>(value)) >> 32)`.
        (((next as i64).wrapping_mul(value as i64)) >> 32) != 0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Parsing of reference vectors generated by the C++ program
    /// `max-brh/tools/rng_vectors/main.cpp` (see the README next to `vectors.txt`).
    const VECTORS: &str = include_str!("../../../tools/rng_vectors/vectors.txt");

    /// Number of seeds in the generator: {0, 1, 2, 0xDEADBEEF, 1234567890} + 3
    /// deterministic "random" seeds.
    const SEED_COUNT: usize = 8;
    /// Length of each sequence.
    const SEQ_LEN: usize = 1000;
    /// Length of the reseed check block.
    const RESEED_LEN: usize = 32;

    struct Tokens {
        values: Vec<&'static str>,
        position: usize,
    }

    impl Tokens {
        fn new(text: &'static str) -> Self {
            // Comments start with '#'; such lines do not enter the stream.
            Self {
                values: text
                    .lines()
                    .filter(|line| !line.trim_start().starts_with('#'))
                    .flat_map(str::split_whitespace)
                    .collect(),
                position: 0,
            }
        }

        fn next(&mut self) -> &'static str {
            let value = self.values[self.position];
            self.position += 1;
            value
        }

        fn has_next(&self) -> bool {
            self.position < self.values.len()
        }

        fn next_u64(&mut self) -> u64 {
            self.next().parse().expect("expected unsigned integer")
        }

        fn next_u32(&mut self) -> u32 {
            self.next_u64() as u32
        }

        fn next_i64(&mut self) -> i64 {
            self.next().parse().expect("expected signed integer")
        }

        fn take_values(&mut self, count: usize) -> Vec<u64> {
            (0..count).map(|_| self.next_u64()).collect()
        }
    }

    /// Full bit-for-bit comparison with the vectors generated from C++.
    #[test]
    fn matches_cpp_reference_vectors() {
        let mut tokens = Tokens::new(VECTORS);
        let mut seeds_seen = 0usize;
        let mut sections = 0usize;
        let mut values_checked = 0usize;

        while tokens.has_next() {
            let kind = tokens.next();
            match kind {
                "state" => {
                    let seed = tokens.next_u32();
                    let expected: Vec<u64> = tokens.take_values(4);
                    let rng = Randomizer::new(seed);
                    assert_eq!(
                        expected,
                        rng.state().iter().map(|v| *v as u64).collect::<Vec<_>>(),
                        "initial state mismatch for seed {seed}"
                    );
                    sections += 1;
                    values_checked += 4;
                }
                "next" => {
                    let seed = tokens.next_u32();
                    let count = tokens.next_u64() as usize;
                    assert_eq!(count, SEQ_LEN);
                    let expected = tokens.take_values(count);
                    let mut rng = Randomizer::new(seed);
                    for (index, want) in expected.iter().enumerate() {
                        let got = rng.next_u32() as u64;
                        assert_eq!(got, *want, "next_u32[{index}] seed={seed}");
                    }
                    seeds_seen += 1;
                    sections += 1;
                    values_checked += count;
                }
                "gen_u16" => {
                    let seed = tokens.next_u32();
                    let range = tokens.next_u32();
                    let count = tokens.next_u64() as usize;
                    assert_eq!(count, SEQ_LEN);
                    let expected = tokens.take_values(count);
                    let mut rng = Randomizer::new(seed);
                    for (index, want) in expected.iter().enumerate() {
                        let got = rng.generate(range as u16) as u64;
                        assert_eq!(got, *want, "generate<u16>({range})[{index}] seed={seed}");
                    }
                    sections += 1;
                    values_checked += count;
                }
                "gen_i32" => {
                    let seed = tokens.next_u32();
                    let range = tokens.next_i64() as i32;
                    let count = tokens.next_u64() as usize;
                    assert_eq!(count, SEQ_LEN);
                    let expected = tokens.take_values(count);
                    let mut rng = Randomizer::new(seed);
                    for (index, want) in expected.iter().enumerate() {
                        let got = rng.generate(range) as i64 as u64;
                        assert_eq!(got, *want, "generate<i32>({range})[{index}] seed={seed}");
                    }
                    sections += 1;
                    values_checked += count;
                }
                "gen_u64" => {
                    let seed = tokens.next_u32();
                    let range = tokens.next_u64();
                    let count = tokens.next_u64() as usize;
                    assert_eq!(count, SEQ_LEN);
                    let expected = tokens.take_values(count);
                    let mut rng = Randomizer::new(seed);
                    for (index, want) in expected.iter().enumerate() {
                        let got = rng.generate(range);
                        assert_eq!(got, *want, "generate<u64>({range})[{index}] seed={seed}");
                    }
                    sections += 1;
                    values_checked += count;
                }
                "reseed" => {
                    let seed = tokens.next_u32();
                    let count = tokens.next_u64() as usize;
                    assert_eq!(count, RESEED_LEN * 2);
                    let first = tokens.take_values(RESEED_LEN);
                    let second = tokens.take_values(RESEED_LEN);
                    assert_eq!(
                        first, second,
                        "reseed is not idempotent in C++ vectors for seed {seed}"
                    );

                    let mut rng = Randomizer::new(seed);
                    // Warm up the state, then reseed — the values must
                    // match the `first` block.
                    for _ in 0..17 {
                        let _ = rng.next_u32();
                    }
                    rng.set_seed(seed);
                    for (index, want) in first.iter().enumerate() {
                        let got = rng.next_u32() as u64;
                        assert_eq!(got, *want, "reseed[{index}] seed={seed}");
                    }
                    sections += 1;
                    values_checked += count;
                }
                other => panic!("unknown vector record: {other}"),
            }
        }

        assert_eq!(seeds_seen, SEED_COUNT, "expected {SEED_COUNT} seed sequences");
        assert_eq!(
            sections,
            SEED_COUNT * 6,
            "expected {SEED_COUNT} x (state + 4 sequences + reseed)"
        );
        // 4 sequences of 1000 + 2 reseed blocks of 32 + 4 state words.
        assert_eq!(
            values_checked,
            SEED_COUNT * (4 * SEQ_LEN + 2 * RESEED_LEN + 4)
        );
    }

    #[test]
    fn splitmix64_reference_values() {
        // Classic SplitMix64 vectors (prng.di.unimi.it/splitmix64.c).
        let mut state = 0u64;
        assert_eq!(splitmix64_next(&mut state), 0xE220_A839_7B1D_CDAF);
        assert_eq!(splitmix64_next(&mut state), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(splitmix64_next(&mut state), 0x06C4_5D18_8009_454F);
    }

    #[test]
    fn seed_state_matches_splitmix_low_words() {
        for seed in [0u32, 1, 2, 0xDEAD_BEEF, 1_234_567_890] {
            let mut state = seed as u64;
            let expected = [
                splitmix64_next(&mut state) as u32,
                splitmix64_next(&mut state) as u32,
                splitmix64_next(&mut state) as u32,
                splitmix64_next(&mut state) as u32,
            ];
            assert_eq!(Randomizer::new(seed).state(), expected);
        }
    }

    #[test]
    fn generate_is_in_range() {
        let mut rng = Randomizer::new(0);
        for range in [1u32, 2, 5, 7, 21, 24, 100, 101, 31_991] {
            for _ in 0..10_000 {
                let value = rng.generate(range);
                assert!(value < range, "generate({range}) = {value}");
            }
        }
        assert_eq!(rng.generate(0u32), 0);
        assert_eq!(rng.generate_i64(0), 0);
        assert_eq!(rng.generate_u64(0), 0);
        assert!(!rng.generate(false));
    }

    #[test]
    fn generate_respects_type_widths() {
        // A u16 result must stay within 16 bits even with range = u16::MAX.
        let mut rng = Randomizer::new(0xDEAD_BEEF);
        for _ in 0..10_000 {
            let v = rng.generate(u16::MAX);
            assert!(v < u16::MAX);
        }
        let mut rng = Randomizer::new(0xDEAD_BEEF);
        for _ in 0..10_000 {
            let v = rng.generate(i8::MAX);
            assert!((0..i8::MAX).contains(&v));
        }
    }

    #[test]
    fn generate_u64_uses_unsigned_branch() {
        // For value >= 2^32 the signed and unsigned branches diverge.
        let value = u64::MAX;

        let mut source = Randomizer::new(42);
        let first_next = source.next_u32();
        let manual = ((first_next as u64).wrapping_mul(value)) >> 32;

        let mut unsigned = Randomizer::new(42);
        assert_eq!(unsigned.generate_u64(value), manual);

        let mut signed = Randomizer::new(42);
        assert_ne!(signed.generate_i64(value as i64) as u64, manual);
    }

    #[test]
    fn state_roundtrip_restores_sequence() {
        let mut rng = Randomizer::new(7);
        for _ in 0..13 {
            let _ = rng.next_u32();
        }
        let state = rng.state();
        let expected: Vec<u32> = (0..10).map(|_| rng.next_u32()).collect();

        let mut restored = Randomizer::from_state(state);
        let got: Vec<u32> = (0..10).map(|_| restored.next_u32()).collect();
        assert_eq!(got, expected);

        let mut restored = Randomizer::default();
        restored.set_state(state);
        assert_eq!(restored.state(), state);
    }

    #[test]
    fn each_seed_gives_distinct_stream() {
        let streams: Vec<u32> = [0u32, 1, 2, 3, 4, 5]
            .iter()
            .map(|seed| Randomizer::new(*seed).next_u32())
            .collect();
        for i in 0..streams.len() {
            for j in (i + 1)..streams.len() {
                assert_ne!(streams[i], streams[j], "seeds {i} and {j} collide");
            }
        }
    }

    #[test]
    fn seed_zero_state_is_low_words_of_splitmix() {
        // XoshiroCpp takes the low 32 bits of each SplitMix64 output.
        assert_eq!(
            Randomizer::new(0).state(),
            [0x7B1D_CDAF, 0xA1B9_65F4, 0x8009_454F, 0x724C_81EC]
        );
    }

    /// An all-zero Xoshiro state is absorbing: the generator emits zeros forever.
    /// This is the mathematical boundary of the algorithm (a seed never produces
    /// it, but a restored save could), so the behavior is pinned.
    #[test]
    fn zero_state_is_absorbing() {
        let mut rng = Randomizer::from_state([0; 4]);
        for _ in 0..64 {
            assert_eq!(rng.next_u32(), 0);
        }
        assert_eq!(rng.state(), [0; 4]);
    }

    /// `Randomizer_Generate<T>` boundaries: value 0 is always 0, value 1 maps the
    /// high bits only, and the result never reaches the range.
    #[test]
    fn generate_boundaries_for_every_type() {
        let seeds = [0u32, 1, 0xDEAD_BEEF, 1_234_567_890];

        for seed in seeds {
            let mut probe = Randomizer::new(seed);
            let next = probe.next_u32();

            // value 0 -> 0 (C++ `(int64)next * 0 >> 32`).
            assert_eq!(Randomizer::new(seed).generate(0u32), 0);
            assert_eq!(Randomizer::new(seed).generate(0i32), 0);
            assert_eq!(Randomizer::new(seed).generate(0u16), 0);
            assert_eq!(Randomizer::new(seed).generate(0i8), 0);
            assert_eq!(Randomizer::new(seed).generate(0u64), 0);
            assert_eq!(Randomizer::new(seed).generate_i64(0), 0);

            // The result of a type is the 32-bit branch narrowed to the type.
            let expected_i64 = ((next as i64) * i64::from(i32::MAX)) >> 32;
            assert_eq!(
                Randomizer::new(seed).generate(i32::MAX),
                expected_i64 as i32
            );
            let expected_u64 = ((next as u64) * u64::from(u32::MAX)) >> 32;
            assert_eq!(Randomizer::new(seed).generate_u64(u64::from(u32::MAX)), expected_u64);

            // `bool`: C++ multiplies by `static_cast<int>(value)` (0/1) and shifts
            // by 32, so `true` always yields 0 — a documented C++ quirk.
            assert!(!Randomizer::new(seed).generate(false));
            assert!(!Randomizer::new(seed).generate(true));
        }
    }

    /// The result of `generate(range)` is always `< range` for the maximum
    /// representable range of each type (masking never produces `range`).
    #[test]
    fn generate_stays_strictly_below_max_range() {
        let mut rng = Randomizer::new(0x0BAD_F00D);
        for _ in 0..100_000 {
            assert!(rng.generate(u8::MAX) < u8::MAX);
            assert!(rng.generate(i8::MAX) < i8::MAX);
            assert!(rng.generate(u16::MAX) < u16::MAX);
            assert!(rng.generate(i16::MAX) < i16::MAX);
            assert!(rng.generate(u32::MAX) < u32::MAX);
            assert!(rng.generate(i32::MAX) < i32::MAX);
        }
    }

    /// Reseeding in the middle of a stream restarts it exactly (C++
    /// `Randomizer_SetSeed` semantics, already used by the save loader).
    #[test]
    fn set_seed_restarts_the_stream() {
        let mut rng = Randomizer::default();
        let first: Vec<u32> = (0..8).map(|_| rng.next_u32()).collect();
        for _ in 0..1000 {
            let _ = rng.next_u32();
        }
        rng.set_seed(DEFAULT_SEED);
        let again: Vec<u32> = (0..8).map(|_| rng.next_u32()).collect();
        assert_eq!(again, first);
    }
}
