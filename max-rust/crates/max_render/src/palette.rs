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
}
