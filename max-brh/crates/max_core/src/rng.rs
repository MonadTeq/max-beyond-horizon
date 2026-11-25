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
#[path = "rng_tests.rs"]
mod tests;
