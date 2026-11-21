//! Палитра M.A.X. и таблицы перекраски.
//!
//! Источники (`max-original-dasm/formats/spw.md` §6, оракул
//! `max-cpp-port/src/{color.cpp,resource_manager.cpp,world.cpp}`):
//!
//! * `COLOR.PAL` — 33 536 байт: сначала 256 × RGB по **6 бит** на компонент,
//!   затем таблица `RGB555 -> ColorIndex` (32 768 байт), которая рендеру не
//!   нужна. SDL-порт при записи системной палитры умножает компоненты на 4
//!   (`Color_SetSystemPaletteEntry`), поэтому [`Palette`] сразу хранит 8-битный
//!   вариант — именно его использует отрисовка.
//! * Командные (командные цвета команды) слоты палитры — **32–39 и 48–63**
//!   (24 слота, «заглушки» художника); `ResourceManager_BuildColorTables`
//!   подменяет их 8 цветами команды циклически (`j & 7`), остальные индексы
//!   остаются identity — это [`ColorLut`].
//! * Яркость — 7 уровней затемнения плюс identity для полной яркости
//!   (§6.3): `rgb' = rgb * (level * 32) / 224`, индекс 31 принудительно равен 31,
//!   подбор цвета — [`map_color`] (`Color_MapColor`, поиск ближайшего в RGB).
//!   Уровень 5 — затемнение тени (`Gfx_DecodeShadow`).
//! * **Палитровые циклы** — `World::InitColorCycles` (`world.cpp:542`) +
//!   `World::UpdateColorAnimations` (`world.cpp:190`): 11 диапазонов слотов,
//!   которые вращаются с частотой 2/6/8/9/10 fps. Это и есть «мигание» маяков
//!   зданий и индикатора дрели в оригинале
//!   (`max-original-dasm/formats/unit-animation.md` §7.3): кадры спрайта не
//!   листаются, меняются только цвета слотов DAC. Реализация — [`ColorCycles`]
//!   (часы) + [`rotate_palette`] (снимок палитры на кадр), см. [`COLOR_CYCLES`].
//!   `dt = 0` не двигает часы, поэтому палитра остаётся исходной (golden не
//!   дрейфуют).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::buffer::Rgba;

/// Цветов в палитре (`PALETTE_SIZE`).
pub const PALETTE_SIZE: usize = 256;
/// Каналов на цвет (`PALETTE_STRIDE`).
pub const PALETTE_STRIDE: usize = 3;
/// Байт в палитре (`256 * 3`).
pub const PALETTE_BYTES: usize = PALETTE_SIZE * PALETTE_STRIDE;
/// Полный размер `COLOR.PAL`: палитра (768) + таблица RGB555 (32 768).
pub const COLOR_PAL_FILE_SIZE: usize = PALETTE_BYTES + (1 << 15);
/// Множитель 6-битных компонент палитры в 8-битные (SDL-порт: `r * 4`).
pub const SIX_BIT_SCALE: u8 = 4;
/// Первый командный слот палитры (32–39).
pub const COMMAND_SLOT_GREEN: u8 = 32;
/// Второй блок командных слотов (48–63).
pub const COMMAND_SLOT_BLUE: u8 = 48;
/// Сколько слотов подменяется цветами команды: 8 + 16 = 24.
pub const COMMAND_SLOT_COUNT: u8 = 24;
/// Индекс палитры, который яркость принудительно оставляет равным 31 (§6.3).
pub const FIXED_BRIGHTNESS_INDEX: u8 = 31;
/// Уровней яркости в таблице плюс identity-уровень 7.
pub const BRIGHTNESS_LEVELS: usize = 8;
/// Уровень затемнения тени (`BrightnessColorIndexTable[5 * 256]`).
pub const SHADOW_BRIGHTNESS_LEVEL: u8 = 5;
/// Уровень «без затемнения» (identity-блок 15 в C++-порте).
pub const FULL_BRIGHTNESS_LEVEL: u8 = 7;

/// Цвета красной команды (слоты -> реальные индексы).
pub const TEAM_RED_COLORS: [u8; 8] = [56, 57, 58, 59, 60, 61, 62, 63];
/// Цвета зелёной команды.
pub const TEAM_GREEN_COLORS: [u8; 8] = [32, 33, 34, 35, 36, 37, 38, 39];
/// Цвета синей команды.
pub const TEAM_BLUE_COLORS: [u8; 8] = [48, 49, 50, 51, 52, 53, 54, 55];
/// Цвета серой (нейтральной) команды.
pub const TEAM_GRAY_COLORS: [u8; 8] = [255, 161, 172, 169, 216, 213, 212, 207];
/// Цвета брошенной (derelict) техники.
pub const TEAM_DERELICT_COLORS: [u8; 8] = [216, 215, 214, 213, 212, 211, 210, 209];

// ---------------------------------------------------------------------------
// Ошибки
// ---------------------------------------------------------------------------

/// Ошибка загрузки палитры.
#[derive(Debug)]
pub enum PaletteError {
    /// Файл/срез короче 768 байт палитры.
    TooShort {
        /// Фактическая длина.
        len: usize,
        /// Минимально необходимая длина.
        needed: usize,
    },
    /// Файл не читается.
    Io {
        /// Путь, который читался.
        path: PathBuf,
        /// Причина от ОС.
        source: io::Error,
    },
}

impl fmt::Display for PaletteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len, needed } => write!(
                f,
                "палитра слишком короткая: {len} байт (нужно минимум {needed})"
            ),
            Self::Io { path, source } => {
                write!(f, "не удалось прочитать {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for PaletteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Палитра
// ---------------------------------------------------------------------------

/// Палитра `COLOR.PAL`: 256 цветов, 6-битные компоненты из файла и 8-битные
/// для отрисовки (`* 4`, как в SDL-порте).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    raw: [[u8; 3]; PALETTE_SIZE],
    rgb: [[u8; 3]; PALETTE_SIZE],
}

impl Palette {
    /// Разбирает начало `COLOR.PAL` (нужны первые 768 байт).
    ///
    /// Полный файл содержит ещё 32 768 байт таблицы RGB555 — она проверяется
    /// только на общую длину и в палитру не попадает.
    pub fn from_color_pal(bytes: &[u8]) -> Result<Self, PaletteError> {
        if bytes.len() < PALETTE_BYTES {
            return Err(PaletteError::TooShort {
                len: bytes.len(),
                needed: PALETTE_BYTES,
            });
        }

        let mut raw = [[0u8; 3]; PALETTE_SIZE];
        let mut rgb = [[0u8; 3]; PALETTE_SIZE];

        for index in 0..PALETTE_SIZE {
            let offset = index * PALETTE_STRIDE;
            let entry = [bytes[offset], bytes[offset + 1], bytes[offset + 2]];

            raw[index] = entry;
            rgb[index] = [
                entry[0].saturating_mul(SIX_BIT_SCALE),
                entry[1].saturating_mul(SIX_BIT_SCALE),
                entry[2].saturating_mul(SIX_BIT_SCALE),
            ];
        }

        Ok(Self { raw, rgb })
    }

    /// Читает палитру из файла `COLOR.PAL` (нужны первые 768 байт; полный
    /// файл содержит ещё таблицу RGB555).
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PaletteError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| PaletteError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Self::from_color_pal(&bytes)
    }

    /// Пиксель из 6-битной палитры (`0..=63` на канал).
    #[inline]
    pub fn raw(&self, index: u8) -> [u8; 3] {
        self.raw[index as usize]
    }

    /// Пиксель из 8-битной палитры (`raw * 4`).
    #[inline]
    pub fn rgb(&self, index: u8) -> [u8; 3] {
        self.rgb[index as usize]
    }

    /// Непрозрачный цвет палитры.
    #[inline]
    pub fn rgba(&self, index: u8) -> Rgba {
        Rgba::from_rgb(self.rgb(index))
    }

    /// Цвет палитры с заданной альфой.
    #[inline]
    pub fn rgba_with_alpha(&self, index: u8, alpha: u8) -> Rgba {
        let rgb = self.rgb(index);

        Rgba::from_rgba([rgb[0], rgb[1], rgb[2], alpha])
    }

    /// Сырые 6-битные компоненты всей палитры (для [`BrightnessLut`]).
    #[inline]
    pub fn raw_entries(&self) -> &[[u8; 3]; PALETTE_SIZE] {
        &self.raw
    }

    /// 8-битные компоненты всей палитры.
    #[inline]
    pub fn entries(&self) -> &[[u8; 3]; PALETTE_SIZE] {
        &self.rgb
    }

    /// 8-битная палитра как 768 байт (порядок `r, g, b`).
    pub fn to_rgb_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(PALETTE_BYTES);

        for entry in &self.rgb {
            out.extend_from_slice(entry);
        }

        out
    }

    /// Таблица `RGB555 -> ColorIndex` из `COLOR.PAL` (не нужна рендеру, но
    /// нужна для полноты формата и проверки размера файла).
    pub fn rgb555_table(bytes: &[u8]) -> Option<&[u8]> {
        bytes.get(PALETTE_BYTES..PALETTE_BYTES + (1 << 15))
    }

    /// Палитра из **8-битных** компонент (системная палитра игры).
    ///
    /// Оригинал держит DAC 6-битным: `Color_SetSystemPaletteEntry` получает
    /// уже поделённые на 4 значения (`world.cpp:330`), а SDL-порт при записи
    /// умножает их на 4. Поэтому здесь компоненты усекаются до 6 бит
    /// (`rgb & 0xFC`), как и в остальном порте: `raw = rgb / 4`.
    pub fn from_rgb8(bytes: &[u8]) -> Result<Self, PaletteError> {
        if bytes.len() < PALETTE_BYTES {
            return Err(PaletteError::TooShort {
                len: bytes.len(),
                needed: PALETTE_BYTES,
            });
        }

        let mut raw = [[0u8; 3]; PALETTE_SIZE];
        let mut rgb = [[0u8; 3]; PALETTE_SIZE];

        for index in 0..PALETTE_SIZE {
            let offset = index * PALETTE_STRIDE;
            let entry = [bytes[offset], bytes[offset + 1], bytes[offset + 2]];
            let raw_entry = [
                entry[0] / SIX_BIT_SCALE,
                entry[1] / SIX_BIT_SCALE,
                entry[2] / SIX_BIT_SCALE,
            ];

            raw[index] = raw_entry;
            rgb[index] = [
                raw_entry[0].saturating_mul(SIX_BIT_SCALE),
                raw_entry[1].saturating_mul(SIX_BIT_SCALE),
                raw_entry[2].saturating_mul(SIX_BIT_SCALE),
            ];
        }

        Ok(Self { raw, rgb })
    }

    /// Снимок палитры с применёнными палитровыми циклами ([`rotate_palette`]).
    pub fn rotated(&self, cycles: &ColorCycles) -> Self {
        rotate_palette(self, cycles)
    }
}

// ---------------------------------------------------------------------------
// Палитровые циклы (`World::InitColorCycles`)
// ---------------------------------------------------------------------------

/// Направление вращения диапазона палитры (`ColorCycleData::rotate_direction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotateDirection {
    /// `0` — «влево»: слот `start` переезжает в `end`, остальные сдвигаются
    /// на один к началу (`memmove(&start[3], start, …)` в C++).
    Left,
    /// `1` — «вправо»: слот `end` переезжает в `start`.
    Right,
}

/// Один палитровый цикл — дословный порт `ColorCycleData` (`world.hpp:44`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorCycle {
    /// Первый слот диапазона (включительно).
    pub start_index: u8,
    /// Последний слот диапазона (включительно).
    pub end_index: u8,
    /// Направление вращения.
    pub direction: RotateDirection,
    /// Миллисекунд между шагами (`TIMER_FPS_TO_MS(fps) = 1000 / fps`).
    pub step_ms: u32,
}

impl ColorCycle {
    /// Сколько слотов в диапазоне (`end - start + 1`).
    pub const fn len(self) -> usize {
        self.end_index as usize - self.start_index as usize + 1
    }

    /// Одиночный слот (`end == start`): C++ крутит у него только зелёный канал.
    pub const fn is_single_slot(self) -> bool {
        self.start_index == self.end_index
    }
}

/// Циклы оригинала — построчный порт `World::InitColorCycles`
/// (`max-cpp-port/src/world.cpp:542-557`).
///
/// `TIMER_FPS_TO_MS(fps) = 1000 / fps`: 9 fps → 111 мс, 6 fps → 166 мс,
/// 2 fps → 500 мс, 8 fps → 125 мс, 10 fps → 100 мс. Маяки шахты используют
/// диапазон 13–16 @6 fps (жёлтый → оранжевый → красный → оранжевый,
/// полный цикл 4 × 166.7 = 666.7 мс), индикатор дрели — 21–24 @6 fps
/// (`unit-animation.md` §7.3).
pub const COLOR_CYCLES: [ColorCycle; 11] = [
    ColorCycle {
        start_index: 9,
        end_index: 12,
        direction: RotateDirection::Left,
        step_ms: 1000 / 9,
    },
    ColorCycle {
        start_index: 13,
        end_index: 16,
        direction: RotateDirection::Right,
        step_ms: 1000 / 6,
    },
    ColorCycle {
        start_index: 17,
        end_index: 20,
        direction: RotateDirection::Right,
        step_ms: 1000 / 9,
    },
    ColorCycle {
        start_index: 21,
        end_index: 24,
        direction: RotateDirection::Right,
        step_ms: 1000 / 6,
    },
    ColorCycle {
        start_index: 25,
        end_index: 30,
        direction: RotateDirection::Right,
        step_ms: 1000 / 2,
    },
    ColorCycle {
        start_index: 31,
        end_index: 31,
        direction: RotateDirection::Right,
        step_ms: 1000 / 6,
    },
    ColorCycle {
        start_index: 96,
        end_index: 102,
        direction: RotateDirection::Right,
        step_ms: 1000 / 8,
    },
    ColorCycle {
        start_index: 103,
        end_index: 109,
        direction: RotateDirection::Right,
        step_ms: 1000 / 8,
    },
    ColorCycle {
        start_index: 110,
        end_index: 116,
        direction: RotateDirection::Right,
        step_ms: 1000 / 10,
    },
    ColorCycle {
        start_index: 117,
        end_index: 122,
        direction: RotateDirection::Right,
        step_ms: 1000 / 6,
    },
    ColorCycle {
        start_index: 123,
        end_index: 127,
        direction: RotateDirection::Right,
        step_ms: 1000 / 6,
    },
];

/// Первое значение зелёного канала одиночного слота 31 (`m_color_cycle_step`).
pub const SINGLE_SLOT_START: u8 = 63;

/// Значение зелёного канала слота 31 после `steps` шагов.
///
/// C++ (`world.cpp:222-231`): `if (step <= 7) step = 63; else step /= 2;`
/// начиная с `m_color_cycle_step = 63` (конструктор `World`). Ряд:
/// `63 → 31 → 15 → 7 → 63 → …`.
pub fn single_slot_value(steps: u64) -> u8 {
    let mut value = SINGLE_SLOT_START;

    for _ in 0..(steps % 4) {
        value = if value <= 7 { SINGLE_SLOT_START } else { value / 2 };
    }

    value
}

/// Часы палитровых циклов (только отрисовка; в снапшот/CRC не входят).
///
/// C++ хранит по `time_stamp` на цикл и вращает диапазон, когда
/// `current_time - time_stamp >= time_limit` (`world.cpp:195-239`). Здесь
/// накапливается `dt` на цикл и выполняется столько шагов, сколько периодов
/// уложилось в накопленное время, — тот же темп без накопления ошибки
/// округления. `dt = 0` (QA/headless) часы не двигает: [`ColorCycles::step_counts`]
/// остаются нулевыми, палитра — исходной.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorCycles {
    elapsed_ms: [f64; COLOR_CYCLES.len()],
    steps: [u64; COLOR_CYCLES.len()],
}

impl Default for ColorCycles {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorCycles {
    /// Часы в исходном состоянии (все счётчики нулевые).
    pub fn new() -> Self {
        Self {
            elapsed_ms: [0.0; COLOR_CYCLES.len()],
            steps: [0; COLOR_CYCLES.len()],
        }
    }

    /// Сбрасывает часы (`dt = 0`): палитра снова исходная.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Продвигает часы на `dt_seconds`; `true`, если хотя бы один цикл шагнул.
    pub fn advance(&mut self, dt_seconds: f32) -> bool {
        let milliseconds = f64::from(dt_seconds.max(0.0)) * 1000.0;

        if !(milliseconds > 0.0) {
            return false;
        }

        let mut advanced = false;

        for (index, cycle) in COLOR_CYCLES.iter().enumerate() {
            self.elapsed_ms[index] += milliseconds;

            while self.elapsed_ms[index] >= f64::from(cycle.step_ms) {
                self.elapsed_ms[index] -= f64::from(cycle.step_ms);
                self.steps[index] = self.steps[index].wrapping_add(1);
                advanced = true;
            }
        }

        advanced
    }

    /// Сколько шагов сделал каждый цикл (индекс — [`COLOR_CYCLES`]).
    pub fn step_counts(&self) -> &[u64; COLOR_CYCLES.len()] {
        &self.steps
    }

    /// Ни один цикл не двигался (палитра равна исходной).
    pub fn is_static(&self) -> bool {
        self.steps.iter().all(|&steps| steps == 0)
    }
}

/// Снимок палитры с повёрнутыми диапазонами — порт `UpdateColorAnimations`.
///
/// Вращается 6-битная палитра (`WindowManager_ColorPalette`), 8-битные
/// компоненты пересчитываются как `raw * 4` (`Color_SetSystemPaletteEntry`).
pub fn rotate_palette(palette: &Palette, cycles: &ColorCycles) -> Palette {
    let mut raw = palette.raw;

    for (index, cycle) in COLOR_CYCLES.iter().enumerate() {
        let steps = cycles.steps[index];

        if steps == 0 {
            continue;
        }

        let start = usize::from(cycle.start_index);
        let end = usize::from(cycle.end_index);

        if cycle.is_single_slot() {
            raw[start][1] = single_slot_value(steps);
            continue;
        }

        let len = cycle.len();
        let shift = (steps % len as u64) as usize;

        if shift == 0 {
            continue;
        }

        let mut range: Vec<[u8; 3]> = raw[start..=end].to_vec();

        match cycle.direction {
            // Вправо: слот `end` переезжает в `start`.
            RotateDirection::Right => range.rotate_right(shift),
            // Влево: слот `start` переезжает в `end`.
            RotateDirection::Left => range.rotate_left(shift),
        }

        raw[start..=end].copy_from_slice(&range);
    }

    let mut rgb = [[0u8; 3]; PALETTE_SIZE];

    for (index, entry) in raw.iter().enumerate() {
        rgb[index] = [
            entry[0].saturating_mul(SIX_BIT_SCALE),
            entry[1].saturating_mul(SIX_BIT_SCALE),
            entry[2].saturating_mul(SIX_BIT_SCALE),
        ];
    }

    Palette { raw, rgb }
}

// ---------------------------------------------------------------------------
// Командные цвета
// ---------------------------------------------------------------------------

/// Команда (набор командных цветов).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TeamColors {
    /// Красная команда (`PLAYER_TEAM_RED`).
    Red,
    /// Зелёная команда.
    Green,
    /// Синяя команда.
    Blue,
    /// Серая (независимая) команда.
    Gray,
    /// Брошенная техника.
    Derelict,
}

impl TeamColors {
    /// Все наборы в порядке C++.
    pub const ALL: [Self; 5] = [
        Self::Red,
        Self::Green,
        Self::Blue,
        Self::Gray,
        Self::Derelict,
    ];

    /// Восемь реальных индексов палитры, подставляемых в командные слоты.
    #[inline]
    pub const fn colors(self) -> [u8; 8] {
        match self {
            Self::Red => TEAM_RED_COLORS,
            Self::Green => TEAM_GREEN_COLORS,
            Self::Blue => TEAM_BLUE_COLORS,
            Self::Gray => TEAM_GRAY_COLORS,
            Self::Derelict => TEAM_DERELICT_COLORS,
        }
    }

    /// «Кодовое» имя как в C++/эталоне (`red`, `green`, …).
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Gray => "gray",
            Self::Derelict => "derelict",
        }
    }

    /// LUT перекраски этой команды (`ResourceManager_BuildColorTables`).
    pub fn lut(self) -> ColorLut {
        ColorLut::for_team(self)
    }
}

/// LUT «индекс палитры -> индекс палитры»: подмена командных слотов.
///
/// Построение повторяет C++ дословно: `j = 0`, `k = 32`; для `j == 8`
/// `k += 8`; затем `lut[k] = colors[j & 7]`, `k += 1`, `j += 1`, пока
/// `j < 24`. Итог: слоты 32–39 получают цвета 0…7, слоты 48–63 — те же
/// восемь цветов дважды (48–55 и 56–63).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorLut(pub [u8; PALETTE_SIZE]);

impl ColorLut {
    /// Identity-LUT (без перекраски).
    pub fn identity() -> Self {
        let mut lut = [0u8; PALETTE_SIZE];

        for (index, slot) in lut.iter_mut().enumerate() {
            *slot = index as u8;
        }

        Self(lut)
    }

    /// LUT команды.
    pub fn for_team(team: TeamColors) -> Self {
        let colors = team.colors();
        let mut lut = Self::identity();

        let mut j = 0usize;
        let mut k = usize::from(COMMAND_SLOT_GREEN);

        while j < usize::from(COMMAND_SLOT_COUNT) {
            if j == 8 {
                k += 8;
            }

            lut.0[k] = colors[j & 7];
            k += 1;
            j += 1;
        }

        lut
    }

    /// Индекс после перекраски.
    #[inline]
    pub fn map(&self, index: u8) -> u8 {
        self.0[index as usize]
    }

    /// Таблица целиком.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// LUT ничего не меняет.
    pub fn is_identity(&self) -> bool {
        self.0
            .iter()
            .enumerate()
            .all(|(index, &slot)| index as u8 == slot)
    }
}

/// Слоты палитры, которые подменяются командными цветами, в порядке присвоения.
///
/// Возвращает 24 пары `(слот, позиция цвета в наборе команды)`: сначала
/// 32–39 (`0..7`), затем 48–63 (`0..7`, `0..7`).
pub fn command_slots() -> [(u8, u8); COMMAND_SLOT_COUNT as usize] {
    let mut slots = [(0u8, 0u8); COMMAND_SLOT_COUNT as usize];
    let mut j = 0usize;
    let mut k = usize::from(COMMAND_SLOT_GREEN);

    while j < usize::from(COMMAND_SLOT_COUNT) {
        if j == 8 {
            k += 8;
        }

        slots[j] = (k as u8, (j & 7) as u8);
        k += 1;
        j += 1;
    }

    slots
}

// ---------------------------------------------------------------------------
// Яркость
// ---------------------------------------------------------------------------

/// Подбор ближайшего цвета палитры — `Color_MapColor` (`color.cpp`).
///
/// `full_scan = false` исключает диапазоны индексов 9–31 и 96–127: они
/// зарезервированы под служебные/командные цвета, и оригинал их не
/// использует при подборе. Расстояние — сумма квадратов разностей RGB,
/// сравнение строгое (первый минимум выигрывает), точное совпадение
/// прерывает поиск.
pub fn map_color(palette: &[[u8; 3]; PALETTE_SIZE], r: u8, g: u8, b: u8, full_scan: bool) -> u8 {
    let mut color_index = 0u8;
    let mut best_distance = u32::MAX;

    for (index, entry) in palette.iter().enumerate() {
        if !full_scan && ((9..=31).contains(&index) || (96..=127).contains(&index)) {
            continue;
        }

        let dr = i32::from(entry[0]) - i32::from(r);
        let dg = i32::from(entry[1]) - i32::from(g);
        let db = i32::from(entry[2]) - i32::from(b);
        let distance = (dr * dr + dg * dg + db * db) as u32;

        if distance < best_distance {
            best_distance = distance;
            color_index = index as u8;

            if distance == 0 {
                break;
            }
        }
    }

    color_index
}

/// Таблицы яркости (§6.3): 8 уровней, каждый — LUT «индекс -> индекс».
///
/// Уровни 0–6 строятся формулой `rgb' = rgb * (level * 32) / 224` по
/// **6-битной** палитре (как `World::ApplyPaletteAndTints`), индекс 31
/// принудительно остаётся 31. Уровень 7 — identity (полная яркость;
/// в C++-порте это блок 15 таблицы, заполненный identity-значениями).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrightnessLut {
    levels: [[u8; PALETTE_SIZE]; BRIGHTNESS_LEVELS],
}

impl BrightnessLut {
    /// Строит таблицы по палитре (`WindowManager_ColorPalette`).
    pub fn build(palette: &Palette) -> Self {
        let raw = palette.raw_entries();
        let mut levels = [[0u8; PALETTE_SIZE]; BRIGHTNESS_LEVELS];

        for (level, table) in levels.iter_mut().enumerate().take(BRIGHTNESS_LEVELS - 1) {
            let factor = level as u32 * 32;

            for (index, slot) in table.iter_mut().enumerate() {
                if index == usize::from(FIXED_BRIGHTNESS_INDEX) {
                    *slot = FIXED_BRIGHTNESS_INDEX;
                    continue;
                }

                let entry = raw[index];
                let r = (u32::from(entry[0]) * factor / (7 * 32)) as u8;
                let g = (u32::from(entry[1]) * factor / (7 * 32)) as u8;
                let b = (u32::from(entry[2]) * factor / (7 * 32)) as u8;

                *slot = map_color(raw, r, g, b, false);
            }
        }

        // Уровень полной яркости — identity.
        for (index, slot) in levels[BRIGHTNESS_LEVELS - 1].iter_mut().enumerate() {
            *slot = index as u8;
        }

        Self { levels }
    }

    /// LUT уровня `level` (значения выше 7 клампятся к 7).
    #[inline]
    pub fn level(&self, level: u8) -> &[u8; PALETTE_SIZE] {
        &self.levels[usize::from(level.min(7))]
    }

    /// Индекс после применения яркости.
    #[inline]
    pub fn map(&self, level: u8, index: u8) -> u8 {
        self.level(level)[index as usize]
    }

    /// Уровень полной яркости.
    pub fn full(&self) -> &[u8; PALETTE_SIZE] {
        self.level(FULL_BRIGHTNESS_LEVEL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Синтетическая палитра с уникальным 6-битным цветом на каждый индекс:
    /// `(index % 64, (index / 64) * 16, 0)`.
    fn synthetic_palette() -> Palette {
        let mut bytes = vec![0u8; PALETTE_BYTES];

        for index in 0..PALETTE_SIZE {
            bytes[index * 3] = (index % 64) as u8;
            bytes[index * 3 + 1] = ((index / 64) * 16) as u8;
            bytes[index * 3 + 2] = 0;
        }

        Palette::from_color_pal(&bytes).unwrap()
    }

    #[test]
    fn six_bit_components_are_scaled_by_four() {
        let mut bytes = vec![0u8; PALETTE_BYTES];

        bytes[0] = 63;
        bytes[1] = 1;
        bytes[2] = 0;
        bytes[3] = 16;
        bytes[4] = 32;
        bytes[5] = 48;

        let palette = Palette::from_color_pal(&bytes).unwrap();

        assert_eq!(palette.raw(0), [63, 1, 0]);
        assert_eq!(palette.rgb(0), [252, 4, 0]);
        assert_eq!(palette.raw(1), [16, 32, 48]);
        assert_eq!(palette.rgb(1), [64, 128, 192]);
        assert_eq!(palette.rgba(1), Rgba::rgb(64, 128, 192));
        assert_eq!(palette.rgba_with_alpha(1, 7).a, 7);
        assert_eq!(palette.to_rgb_bytes().len(), PALETTE_BYTES);

        assert!(matches!(
            Palette::from_color_pal(&bytes[..PALETTE_BYTES - 1]),
            Err(PaletteError::TooShort { .. })
        ));
    }

    #[test]
    fn team_luts_replace_command_slots_and_keep_the_rest() {
        let red = ColorLut::for_team(TeamColors::Red);
        let green = ColorLut::for_team(TeamColors::Green);
        let blue = ColorLut::for_team(TeamColors::Blue);
        let gray = ColorLut::for_team(TeamColors::Gray);
        let derelict = ColorLut::for_team(TeamColors::Derelict);

        assert!(!red.is_identity());
        assert!(ColorLut::identity().is_identity());

        // Слоты 32–39 и 48–63 подменяются, всё остальное — identity.
        for index in 0..PALETTE_SIZE {
            let in_slots = (32..=39).contains(&index) || (48..=63).contains(&index);

            if !in_slots {
                assert_eq!(red.map(index as u8), index as u8, "индекс {index}");
                assert_eq!(derelict.map(index as u8), index as u8, "индекс {index}");
            }
        }

        assert_eq!(&red.0[32..40], &TEAM_RED_COLORS);
        assert_eq!(&red.0[48..56], &TEAM_RED_COLORS);
        assert_eq!(&red.0[56..64], &TEAM_RED_COLORS);
        assert_eq!(&green.0[32..40], &TEAM_GREEN_COLORS);
        assert_eq!(&blue.0[48..56], &TEAM_BLUE_COLORS);
        assert_eq!(&gray.0[32..40], &TEAM_GRAY_COLORS);
        // Слоты 48–63 = восемь цветов дважды.
        assert_eq!(&derelict.0[48..56], &TEAM_DERELICT_COLORS);
        assert_eq!(&derelict.0[56..64], &TEAM_DERELICT_COLORS);
    }

    #[test]
    fn command_slots_match_the_cpp_loop() {
        let slots = command_slots();

        assert_eq!(slots.len(), 24);
        assert_eq!(slots[0], (32, 0));
        assert_eq!(slots[7], (39, 7));
        assert_eq!(slots[8], (48, 0));
        assert_eq!(slots[15], (55, 7));
        assert_eq!(slots[16], (56, 0));
        assert_eq!(slots[23], (63, 7));

        // Та же нумерация, что в LUT команды.
        let lut = ColorLut::for_team(TeamColors::Green);

        for (slot, color) in slots {
            assert_eq!(lut.map(slot), TEAM_GREEN_COLORS[color as usize]);
        }
    }

    #[test]
    fn map_color_finds_exact_and_skips_reserved_ranges() {
        let palette = synthetic_palette();
        let value = palette.raw(40);

        assert_eq!(
            map_color(&palette.raw, value[0], value[1], value[2], false),
            40
        );
        assert_eq!(
            map_color(&palette.raw, value[0], value[1], value[2], true),
            40
        );

        // Цвета индексов 9–31 и 96–127 недостижимы при full_scan = false.
        for reserved in [9u8, 20, 31, 96, 110, 127] {
            let value = palette.raw(reserved);
            let found = map_color(&palette.raw, value[0], value[1], value[2], false);

            assert!(
                !((9..=31).contains(&found) || (96..=127).contains(&found)),
                "map_color вернул зарезервированный индекс {found}"
            );
            // С full_scan точное совпадение находится.
            assert_eq!(
                map_color(&palette.raw, value[0], value[1], value[2], true),
                reserved
            );
        }
    }

    #[test]
    fn brightness_levels_are_monotonic_and_level_seven_is_identity() {
        let palette = synthetic_palette();
        let brightness = BrightnessLut::build(&palette);

        for index in 0..PALETTE_SIZE {
            assert_eq!(
                brightness.map(FULL_BRIGHTNESS_LEVEL, index as u8),
                index as u8,
                "уровень 7 — identity"
            );
        }

        // Уровень 0 — самый тёмный: все цвета, кроме фиксированного индекса 31,
        // сходятся к чёрному.
        for index in 0..PALETTE_SIZE {
            if index == usize::from(FIXED_BRIGHTNESS_INDEX) {
                continue;
            }

            let mapped = brightness.map(0, index as u8);

            assert_eq!(palette.raw(mapped), [0, 0, 0], "уровень 0, индекс {index}");
        }

        // Уровни 0–6 считаются ровно как в `World::ApplyPaletteAndTints`:
        // rgb' = rgb * (level * 32) / 224, затем `Color_MapColor` без full_scan.
        let raw = palette.raw_entries();

        for level in 0..7u32 {
            let factor = level * 32;

            for index in 0..PALETTE_SIZE {
                if index == usize::from(FIXED_BRIGHTNESS_INDEX) {
                    continue;
                }

                let entry = raw[index];
                let expected = map_color(
                    raw,
                    (u32::from(entry[0]) * factor / (7 * 32)) as u8,
                    (u32::from(entry[1]) * factor / (7 * 32)) as u8,
                    (u32::from(entry[2]) * factor / (7 * 32)) as u8,
                    false,
                );

                assert_eq!(
                    brightness.map(level as u8, index as u8),
                    expected,
                    "уровень {level}, индекс {index}"
                );
            }
        }

        // Уровень 5 (тень) заметно темнее: меняется большинство цветов.
        let changed = (0..PALETTE_SIZE)
            .filter(|&index| brightness.map(SHADOW_BRIGHTNESS_LEVEL, index as u8) != index as u8)
            .count();

        assert!(changed > 100, "уровень 5 изменил всего {changed} цветов");

        // Индекс 31 фиксирован на всех уровнях 0–6.
        for level in 0..=6u8 {
            assert_eq!(brightness.map(level, FIXED_BRIGHTNESS_INDEX), 31);
        }

        // Уровни клампятся.
        assert_eq!(brightness.level(200), brightness.level(7));
    }

    /// Палитра, у которой каждый слот раскрашен по номеру: `(i, 2i, 4i)`.
    fn indexed_palette() -> Palette {
        let mut bytes = vec![0u8; PALETTE_BYTES];

        for index in 0..PALETTE_SIZE {
            bytes[index * 3] = index as u8;
            bytes[index * 3 + 1] = (index * 2 % 64) as u8;
            bytes[index * 3 + 2] = (index * 4 % 64) as u8;
        }

        Palette::from_color_pal(&bytes).unwrap()
    }

    #[test]
    fn color_cycle_table_matches_the_cpp_init_color_cycles() {
        // Порядок и тайминги — построчно `World::InitColorCycles`
        // (`world.cpp:542-557`); `TIMER_FPS_TO_MS(fps) = 1000 / fps`.
        let table: Vec<(u8, u8, RotateDirection, u32)> = COLOR_CYCLES
            .iter()
            .map(|cycle| {
                (
                    cycle.start_index,
                    cycle.end_index,
                    cycle.direction,
                    cycle.step_ms,
                )
            })
            .collect();

        assert_eq!(
            table,
            vec![
                (9, 12, RotateDirection::Left, 111),
                (13, 16, RotateDirection::Right, 166),
                (17, 20, RotateDirection::Right, 111),
                (21, 24, RotateDirection::Right, 166),
                (25, 30, RotateDirection::Right, 500),
                (31, 31, RotateDirection::Right, 166),
                (96, 102, RotateDirection::Right, 125),
                (103, 109, RotateDirection::Right, 125),
                (110, 116, RotateDirection::Right, 100),
                (117, 122, RotateDirection::Right, 166),
                (123, 127, RotateDirection::Right, 166),
            ]
        );

        // Маяки шахты — 6 fps на четырёх слотах: полный цикл 666.7 мс.
        let beacons = COLOR_CYCLES[1];

        assert_eq!((beacons.start_index, beacons.end_index), (13, 16));
        assert_eq!(beacons.len(), 4);
        assert_eq!(beacons.step_ms * beacons.len() as u32, 664);
    }

    #[test]
    fn zero_dt_keeps_the_palette_identical() {
        let palette = indexed_palette();
        let mut cycles = ColorCycles::new();

        assert!(cycles.is_static());
        assert!(!cycles.advance(0.0));
        assert_eq!(rotate_palette(&palette, &cycles), palette);
        assert_eq!(cycles.step_counts(), &[0; COLOR_CYCLES.len()]);
    }

    #[test]
    fn beacon_range_rotates_one_slot_per_sixth_of_a_second() {
        let palette = indexed_palette();
        let mut cycles = ColorCycles::new();

        // Меньше самого короткого периода (10 fps = 100 мс) — ни одного шага.
        assert!(!cycles.advance(0.09));
        assert_eq!(rotate_palette(&palette, &cycles), palette);

        // 170 мс — ровно один шаг 6 fps (166 мс): 13..16 сдвигается вправо.
        assert!(cycles.advance(0.08));
        assert_eq!(cycles.step_counts()[1], 1);
        let once = rotate_palette(&palette, &cycles);

        assert_eq!(once.raw(13), palette.raw(16), "start <- end (вправо)");
        assert_eq!(once.raw(14), palette.raw(13));
        assert_eq!(once.raw(15), palette.raw(14));
        assert_eq!(once.raw(16), palette.raw(15));
        // Соседние с 13..16 слоты не тронуты (9..12 и 17..20 вращаются своими
        // циклами 9 fps, 13..16 от них не зависит).
        assert_eq!(once.raw(8), palette.raw(8));
        assert_eq!(once.raw(32), palette.raw(32));
        // 8-битные компоненты пересчитаны как raw * 4.
        assert_eq!(once.rgb(13), [once.raw(13)[0] * 4, once.raw(13)[1] * 4, once.raw(13)[2] * 4]);

        // Полный цикл (4 шага по 1/6 с) возвращает слоты 13..16 на место
        // (остальные диапазоны живут своим темпом и тоже двигаются).
        for _ in 0..3 {
            cycles.advance(1.0 / 6.0);
        }
        assert_eq!(cycles.step_counts()[1], 4);
        let full = rotate_palette(&palette, &cycles);

        for index in 13..=16 {
            assert_eq!(full.raw(index), palette.raw(index), "слот {index}");
        }

        // 5-й шаг = 4 (круг) + 1: слоты 13..16 как после одного шага.
        cycles.advance(1.0 / 6.0);
        assert_eq!(cycles.step_counts()[1], 5);
        let after_five = rotate_palette(&palette, &cycles);

        for index in 13..=16 {
            assert_eq!(after_five.raw(index), once.raw(index), "слот {index}");
        }
    }

    #[test]
    fn each_cycle_uses_its_own_rate_and_direction() {
        let palette = indexed_palette();
        let mut cycles = ColorCycles::new();

        // 0.5 с: 9..12 @9 fps (4 шага = полный круг), 25..30 @2 fps (1 шаг),
        // 110..116 @10 fps (5 шагов = полный круг).
        cycles.advance(0.5);
        let steps = *cycles.step_counts();

        assert_eq!(steps[0], 4, "9 fps: 111 мс × 4 = 444 мс");
        assert_eq!(steps[4], 1, "2 fps: 500 мс");
        assert_eq!(steps[8], 5, "10 fps: 100 мс × 5");
        assert_eq!(steps[1], 3, "6 fps: 166 мс × 3");
        assert_eq!(steps[5], 3, "одиночный слот 31 — тот же 6 fps");

        let rotated = rotate_palette(&palette, &cycles);

        // 9..12 — единственный диапазон с вращением влево; 4 шага = круг.
        for index in 9..=12 {
            assert_eq!(rotated.raw(index), palette.raw(index));
        }
        // 25..30 @2 fps сдвинулся на один вправо.
        assert_eq!(rotated.raw(25), palette.raw(30));
        assert_eq!(rotated.raw(26), palette.raw(25));

        // Другой шаг: 9..12 влево на один слот.
        let mut left = ColorCycles::new();
        left.advance(1.0 / 9.0);
        let rotated = rotate_palette(&palette, &left);

        assert_eq!(rotated.raw(9), palette.raw(10), "влево: start <- next");
        assert_eq!(rotated.raw(12), palette.raw(9), "влево: end <- start");
    }

    #[test]
    fn single_slot_31_walks_the_green_component() {
        assert_eq!(single_slot_value(0), 63);
        assert_eq!(single_slot_value(1), 31);
        assert_eq!(single_slot_value(2), 15);
        assert_eq!(single_slot_value(3), 7);
        assert_eq!(single_slot_value(4), 63);
        assert_eq!(single_slot_value(9), 31);

        let palette = indexed_palette();
        let mut cycles = ColorCycles::new();

        cycles.advance(1.0 / 6.0);
        let rotated = rotate_palette(&palette, &cycles);

        assert_eq!(rotated.raw(31)[1], 31, "зелёный канал 63 -> 31");
        assert_eq!(rotated.raw(31)[0], palette.raw(31)[0], "красный не меняется");
        assert_eq!(rotated.raw(31)[2], palette.raw(31)[2], "синий не меняется");
    }

    #[test]
    fn from_rgb8_truncates_to_six_bit_and_roundtrips() {
        let mut bytes = vec![0u8; PALETTE_BYTES];

        bytes[0..3].copy_from_slice(&[255, 243, 159]);
        bytes[3..6].copy_from_slice(&[0, 64, 128]);

        let palette = Palette::from_rgb8(&bytes).unwrap();

        assert_eq!(palette.raw(0), [63, 60, 39]);
        assert_eq!(palette.rgb(0), [252, 240, 156]);
        assert_eq!(palette.raw(1), [0, 16, 32]);
        assert!(matches!(
            Palette::from_rgb8(&bytes[..PALETTE_BYTES - 1]),
            Err(PaletteError::TooShort { .. })
        ));
    }
}
