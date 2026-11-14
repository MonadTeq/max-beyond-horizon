//! Детерминированный RNG ядра — точный порт `max-cpp-port/src/randomizer.cpp`.
//!
//! Алгоритм (DESIGN.md §4.1):
//! * сидирование — **SplitMix64**: `state += 0x9E3779B97F4A7C15`, затем
//!   `z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9`,
//!   `z = (z ^ (z >> 27)) * 0x94D049BB133111EB`, `z ^ (z >> 31)`;
//! * четыре младших 32-битных слова SplitMix64 образуют состояние
//!   [`Randomizer`] (как конструктор `Xoshiro128StarStar(uint64_t)` в
//!   `XoshiroCpp.hpp`);
//! * генерация — **Xoshiro128\*\***: `rotl(s[1] * 5, 7) * 9` и стандартное
//!   обновление состояния.
//!
//! `Randomizer_Generate<T>` (§4.2) — две ветки семантики:
//! * `T <= u32` (и `i8..=i32`): `(next as i64 * value) >> 32` (арифметический
//!   сдвиг, `i64`-умножение);
//! * `T == u64` (сайты `sizeof(...)`/`size_t`): `(next as u64 * value) >> 32`
//!   (логический сдвиг, беззнаковое умножение).
//!
//! Крейт не использует `rand` и не обращается ко времени: единственная точка
//! сидирования — [`Randomizer::new`]/[`Randomizer::set_seed`] (в симуляции —
//! `Command::Seed`). Состояние сериализуемо и входит в CRC-дайджест.

use serde::{Deserialize, Serialize};

/// Seed по умолчанию (`XoshiroCpp::DefaultSeed = 1234567890`).
pub const DEFAULT_SEED: u32 = 1_234_567_890;

const SPLITMIX_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const SPLITMIX_MUL1: u64 = 0xBF58_476D_1CE4_E5B9;
const SPLITMIX_MUL2: u64 = 0x94D0_49BB_1331_11EB;

/// Один шаг SplitMix64; состояние — `u64`.
#[inline]
fn splitmix64_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(SPLITMIX_GAMMA);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(SPLITMIX_MUL1);
    z = (z ^ (z >> 27)).wrapping_mul(SPLITMIX_MUL2);
    z ^ (z >> 31)
}

/// Генератор M.A.X. — Xoshiro128\*\* с сидированием SplitMix64.
///
/// Состояние — `[u32; 4]`, ровно как `XoshiroCpp::Xoshiro128StarStar::state_type`.
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
    /// Создаёт генератор с сидом `seed` (как `Randomizer_SetSeed`).
    #[inline]
    pub fn new(seed: u32) -> Self {
        let mut randomizer = Self { state: [0; 4] };
        randomizer.set_seed(seed);
        randomizer
    }

    /// Пересидирует генератор (как `Randomizer_SetSeed`, включая reseed).
    pub fn set_seed(&mut self, seed: u32) {
        let mut splitmix_state = seed as u64;
        for slot in &mut self.state {
            // XoshiroCpp: `static_cast<uint32_t>(splitmix())`.
            *slot = splitmix64_next(&mut splitmix_state) as u32;
        }
    }

    /// Текущее состояние `[u32; 4]` (для CRC/сейвов).
    #[inline]
    pub const fn state(&self) -> [u32; 4] {
        self.state
    }

    /// Восстанавливает состояние (загрузка сейва).
    #[inline]
    pub fn from_state(state: [u32; 4]) -> Self {
        Self { state }
    }

    /// Перезаписывает состояние (загрузка сейва).
    #[inline]
    pub fn set_state(&mut self, state: [u32; 4]) {
        self.state = state;
    }

    /// Следующее 32-битное значение — `Xoshiro128StarStar::operator()`.
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

    /// Порт `Randomizer_Generate<T>` для всех поддерживаемых типов.
    ///
    /// Делает ровно один вызов [`Self::next_u32`] и маскирует результат по типу
    /// `T` (см. [`GenerateValue`]).
    #[inline]
    pub fn generate<T: GenerateValue>(&mut self, value: T) -> T {
        T::generate_from(self.next_u32(), value)
    }

    /// Ветка `i64` (C++ `T` — `int8_t..int32_t`/`uint8_t..uint32_t`):
    /// `(next as i64 * value) >> 32`.
    #[inline]
    pub fn generate_i64(&mut self, value: i64) -> i64 {
        ((self.next_u32() as i64).wrapping_mul(value)) >> 32
    }

    /// Ветка `u64` (C++ `size_t`, сайты `sizeof(...)`):
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

/// Семантика `Randomizer_Generate<T>` для конкретного типа `T`.
///
/// Реализована ровно для типов, встречающихся в C++-порту: `i8..=i32`,
/// `u8..=u32`, `i64`, `u64`, `bool`. `char` намеренно не реализован: в C++
/// `char` имеет платформенную знаковость, а Rust `char` — Unicode-скаляр; все
/// байтовые сайты портируются через `i8`/`u8`.
pub trait GenerateValue: Copy {
    /// Преобразует одно значение `next_u32()` в результат типа `Self`.
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
// Тесты
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Разбор эталонных векторов, сгенерированных C++-программой
    /// `max-rust/tools/rng_vectors/main.cpp` (см. README рядом с `vectors.txt`).
    const VECTORS: &str = include_str!("../../../tools/rng_vectors/vectors.txt");

    /// Число сидов в генераторе: {0, 1, 2, 0xDEADBEEF, 1234567890} + 3
    /// детерминированных «случайных» сида.
    const SEED_COUNT: usize = 8;
    /// Длина каждой последовательности.
    const SEQ_LEN: usize = 1000;
    /// Длина блока reseed-проверки.
    const RESEED_LEN: usize = 32;

    struct Tokens {
        values: Vec<&'static str>,
        position: usize,
    }

    impl Tokens {
        fn new(text: &'static str) -> Self {
            // Комментарии начинаются с '#'; такие строки не попадают в поток.
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

    /// Полная сверка бит-в-бит с векторами, сгенерированными из C++.
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
                    // Прогреваем состояние, затем reseed — значения должны
                    // совпасть с блоком `first`.
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
        // 4 последовательности по 1000 + 2 блока reseed по 32 + 4 слова state.
        assert_eq!(
            values_checked,
            SEED_COUNT * (4 * SEQ_LEN + 2 * RESEED_LEN + 4)
        );
    }

    #[test]
    fn splitmix64_reference_values() {
        // Классические векторы SplitMix64 (prng.di.unimi.it/splitmix64.c).
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
        // u16-результат не должен выходить за 16 бит даже при range = u16::MAX.
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
        // Для value >= 2^32 знаковая и беззнаковая ветки расходятся.
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
        // XoshiroCpp берёт младшие 32 бита каждого выхода SplitMix64.
        assert_eq!(
            Randomizer::new(0).state(),
            [0x7B1D_CDAF, 0xA1B9_65F4, 0x8009_454F, 0x724C_81EC]
        );
    }
}
