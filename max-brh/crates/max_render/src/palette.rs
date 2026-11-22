//! The M.A.X. palette and recolor tables.
//!
//! Sources (`max-original-dasm/formats/spw.md` §6, the oracle
//! `max-cpp-port/src/{color.cpp,resource_manager.cpp,world.cpp}`):
//!
//! * `COLOR.PAL` — 33 536 bytes: first 256 × RGB at **6 bits** per component,
//!   then the `RGB555 -> ColorIndex` table (32 768 bytes), which the renderer does not
//!   need. The SDL port multiplies the components by 4 when setting the system palette
//!   (`Color_SetSystemPaletteEntry`), so [`Palette`] stores the 8-bit variant
//!   right away — that is exactly what rendering uses.
//! * The team (team color) slots of the palette are **32–39 and 48–63**
//!   (24 slots, the artist's "placeholders"); `ResourceManager_BuildColorTables`
//!   replaces them cyclically with the 8 colors of the team (`j & 7`), the other indices
//!   stay the identity — that is [`ColorLut`].
//! * Brightness — 7 darkening levels plus the identity for full brightness
//!   (§6.3): `rgb' = rgb * (level * 32) / 224`, index 31 is forced to 31,
//!   color matching — [`map_color`] (`Color_MapColor`, nearest search in RGB).
//!   Level 5 — shadow darkening (`Gfx_DecodeShadow`).
//! * **Palette cycles** — `World::InitColorCycles` (`world.cpp:542`) +
//!   `World::UpdateColorAnimations` (`world.cpp:190`): 11 slot ranges
//!   that rotate at 2/6/8/9/10 fps. This is the "blinking" of the beacons
//!   of buildings and of the drill indicator in the original
//!   (`max-original-dasm/formats/unit-animation.md` §7.3): the sprite frames are not
//!   flipped, only the colors of the DAC slots change. The implementation is [`ColorCycles`]
//!   (the clock) + [`rotate_palette`] (the palette snapshot for a frame), see [`COLOR_CYCLES`].
//!   `dt = 0` does not move the clock, so the palette stays the initial one (the goldens do not
//!   drift).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::buffer::Rgba;

/// Colors in the palette (`PALETTE_SIZE`).
pub const PALETTE_SIZE: usize = 256;
/// Channels per color (`PALETTE_STRIDE`).
pub const PALETTE_STRIDE: usize = 3;
/// Bytes in the palette (`256 * 3`).
pub const PALETTE_BYTES: usize = PALETTE_SIZE * PALETTE_STRIDE;
/// Full size of `COLOR.PAL`: palette (768) + RGB555 table (32 768).
pub const COLOR_PAL_FILE_SIZE: usize = PALETTE_BYTES + (1 << 15);
/// Multiplier of the 6-bit palette components to 8 bits (SDL port: `r * 4`).
pub const SIX_BIT_SCALE: u8 = 4;
/// First team slot of the palette (32–39).
pub const COMMAND_SLOT_GREEN: u8 = 32;
/// Second block of team slots (48–63).
pub const COMMAND_SLOT_BLUE: u8 = 48;
/// How many slots are replaced with team colors: 8 + 16 = 24.
pub const COMMAND_SLOT_COUNT: u8 = 24;
/// Palette index that brightness forcibly leaves equal to 31 (§6.3).
pub const FIXED_BRIGHTNESS_INDEX: u8 = 31;
/// Brightness levels in the table plus the identity level 7.
pub const BRIGHTNESS_LEVELS: usize = 8;
/// Shadow darkening level (`BrightnessColorIndexTable[5 * 256]`).
pub const SHADOW_BRIGHTNESS_LEVEL: u8 = 5;
/// "No darkening" level (identity block 15 in the C++ port).
pub const FULL_BRIGHTNESS_LEVEL: u8 = 7;

/// Red team colors (slots -> real indices).
pub const TEAM_RED_COLORS: [u8; 8] = [56, 57, 58, 59, 60, 61, 62, 63];
/// Green team colors.
pub const TEAM_GREEN_COLORS: [u8; 8] = [32, 33, 34, 35, 36, 37, 38, 39];
/// Blue team colors.
pub const TEAM_BLUE_COLORS: [u8; 8] = [48, 49, 50, 51, 52, 53, 54, 55];
/// Gray (neutral) team colors.
pub const TEAM_GRAY_COLORS: [u8; 8] = [255, 161, 172, 169, 216, 213, 212, 207];
/// Colors of derelict equipment.
pub const TEAM_DERELICT_COLORS: [u8; 8] = [216, 215, 214, 213, 212, 211, 210, 209];

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Palette loading error.
#[derive(Debug)]
pub enum PaletteError {
    /// The file/slice is shorter than the 768-byte palette.
    TooShort {
        /// Actual length.
        len: usize,
        /// Minimum required length.
        needed: usize,
    },
    /// The file cannot be read.
    Io {
        /// The path that was being read.
        path: PathBuf,
        /// Reason from the OS.
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
// Palette
// ---------------------------------------------------------------------------

/// `COLOR.PAL` palette: 256 colors, 6-bit components from the file and 8-bit
/// ones for rendering (`* 4`, as in the SDL port).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    raw: [[u8; 3]; PALETTE_SIZE],
    rgb: [[u8; 3]; PALETTE_SIZE],
}

impl Palette {
    /// Parses the beginning of `COLOR.PAL` (the first 768 bytes are needed).
    ///
    /// The full file contains another 32 768 bytes of the RGB555 table — it is checked
    /// only for the overall length and does not enter the palette.
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

    /// Reads the palette from the `COLOR.PAL` file (the first 768 bytes are needed; the full
    /// file contains the RGB555 table as well).
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PaletteError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| PaletteError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Self::from_color_pal(&bytes)
    }

    /// Pixel from the 6-bit palette (`0..=63` per channel).
    #[inline]
    pub fn raw(&self, index: u8) -> [u8; 3] {
        self.raw[index as usize]
    }

    /// Pixel from the 8-bit palette (`raw * 4`).
    #[inline]
    pub fn rgb(&self, index: u8) -> [u8; 3] {
        self.rgb[index as usize]
    }

    /// Opaque color of the palette.
    #[inline]
    pub fn rgba(&self, index: u8) -> Rgba {
        Rgba::from_rgb(self.rgb(index))
    }

    /// Palette color with the given alpha.
    #[inline]
    pub fn rgba_with_alpha(&self, index: u8, alpha: u8) -> Rgba {
        let rgb = self.rgb(index);

        Rgba::from_rgba([rgb[0], rgb[1], rgb[2], alpha])
    }

    /// Raw 6-bit components of the whole palette (for [`BrightnessLut`]).
    #[inline]
    pub fn raw_entries(&self) -> &[[u8; 3]; PALETTE_SIZE] {
        &self.raw
    }

    /// 8-bit components of the whole palette.
    #[inline]
    pub fn entries(&self) -> &[[u8; 3]; PALETTE_SIZE] {
        &self.rgb
    }

    /// The 8-bit palette as 768 bytes (order `r, g, b`).
    pub fn to_rgb_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(PALETTE_BYTES);

        for entry in &self.rgb {
            out.extend_from_slice(entry);
        }

        out
    }

    /// The `RGB555 -> ColorIndex` table from `COLOR.PAL` (not needed by the renderer, but
    /// needed for format completeness and file size checking).
    pub fn rgb555_table(bytes: &[u8]) -> Option<&[u8]> {
        bytes.get(PALETTE_BYTES..PALETTE_BYTES + (1 << 15))
    }

    /// Palette of **8-bit** components (the system palette of the game).
    ///
    /// The original keeps the DAC at 6 bits: `Color_SetSystemPaletteEntry` receives
    /// values already divided by 4 (`world.cpp:330`), and the SDL port multiplies
    /// them by 4 when setting. Therefore here the components are truncated to 6 bits
    /// (`rgb & 0xFC`), as in the rest of the port: `raw = rgb / 4`.
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

    /// Palette snapshot with the palette cycles applied ([`rotate_palette`]).
    pub fn rotated(&self, cycles: &ColorCycles) -> Self {
        rotate_palette(self, cycles)
    }
}

// ---------------------------------------------------------------------------
// Palette cycles (`World::InitColorCycles`)
// ---------------------------------------------------------------------------

/// Rotation direction of a palette range (`ColorCycleData::rotate_direction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotateDirection {
    /// `0` — "left": the slot `start` moves to `end`, the others shift
    /// by one toward the beginning (`memmove(&start[3], start, …)` in C++).
    Left,
    /// `1` — "right": the slot `end` moves to `start`.
    Right,
}

/// One palette cycle — a verbatim port of `ColorCycleData` (`world.hpp:44`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorCycle {
    /// First slot of the range (inclusive).
    pub start_index: u8,
    /// Last slot of the range (inclusive).
    pub end_index: u8,
    /// Rotation direction.
    pub direction: RotateDirection,
    /// Milliseconds between steps (`TIMER_FPS_TO_MS(fps) = 1000 / fps`).
    pub step_ms: u32,
}

impl ColorCycle {
    /// How many slots are in the range (`end - start + 1`).
    pub const fn len(self) -> usize {
        self.end_index as usize - self.start_index as usize + 1
    }

    /// Single slot (`end == start`): C++ rotates only its green channel.
    pub const fn is_single_slot(self) -> bool {
        self.start_index == self.end_index
    }
}

/// The original's cycles — a row-by-row port of `World::InitColorCycles`
/// (`max-cpp-port/src/world.cpp:542-557`).
///
/// `TIMER_FPS_TO_MS(fps) = 1000 / fps`: 9 fps → 111 ms, 6 fps → 166 ms,
/// 2 fps → 500 ms, 8 fps → 125 ms, 10 fps → 100 ms. The mine beacons use
/// the range 13–16 @6 fps (yellow → orange → red → orange,
/// full cycle 4 × 166.7 = 666.7 ms), the drill indicator — 21–24 @6 fps
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

/// First value of the green channel of single slot 31 (`m_color_cycle_step`).
pub const SINGLE_SLOT_START: u8 = 63;

/// Value of the green channel of slot 31 after `steps` steps.
///
/// C++ (`world.cpp:222-231`): `if (step <= 7) step = 63; else step /= 2;`
/// starting from `m_color_cycle_step = 63` (the `World` constructor). The series:
/// `63 → 31 → 15 → 7 → 63 → …`.
pub fn single_slot_value(steps: u64) -> u8 {
    let mut value = SINGLE_SLOT_START;

    for _ in 0..(steps % 4) {
        value = if value <= 7 { SINGLE_SLOT_START } else { value / 2 };
    }

    value
}

/// Palette cycle clock (render-only; not part of the snapshot/CRC).
///
/// C++ keeps a `time_stamp` per cycle and rotates the range when
/// `current_time - time_stamp >= time_limit` (`world.cpp:195-239`). Here
/// `dt` accumulates per cycle and as many steps are performed as periods
/// fit into the accumulated time — the same rate without accumulating rounding
/// error. `dt = 0` (QA/headless) does not move the clock: [`ColorCycles::step_counts`]
/// stay zero, the palette — the initial one.
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
    /// Clock in the initial state (all counters zero).
    pub fn new() -> Self {
        Self {
            elapsed_ms: [0.0; COLOR_CYCLES.len()],
            steps: [0; COLOR_CYCLES.len()],
        }
    }

    /// Resets the clock (`dt = 0`): the palette is the initial one again.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Advances the clock by `dt_seconds`; `true` if at least one cycle stepped.
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

    /// How many steps each cycle made (index — [`COLOR_CYCLES`]).
    pub fn step_counts(&self) -> &[u64; COLOR_CYCLES.len()] {
        &self.steps
    }

    /// No cycle moved (the palette equals the initial one).
    pub fn is_static(&self) -> bool {
        self.steps.iter().all(|&steps| steps == 0)
    }
}

/// Palette snapshot with the ranges rotated — a port of `UpdateColorAnimations`.
///
/// The 6-bit palette rotates (`WindowManager_ColorPalette`), the 8-bit
/// components are recomputed as `raw * 4` (`Color_SetSystemPaletteEntry`).
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
            // Right: the slot `end` moves to `start`.
            RotateDirection::Right => range.rotate_right(shift),
            // Left: the slot `start` moves to `end`.
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
// Team colors
// ---------------------------------------------------------------------------

/// Team (a set of team colors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TeamColors {
    /// Red team (`PLAYER_TEAM_RED`).
    Red,
    /// Green team.
    Green,
    /// Blue team.
    Blue,
    /// Gray (independent) team.
    Gray,
    /// Derelict equipment.
    Derelict,
}

impl TeamColors {
    /// All sets in C++ order.
    pub const ALL: [Self; 5] = [
        Self::Red,
        Self::Green,
        Self::Blue,
        Self::Gray,
        Self::Derelict,
    ];

    /// Eight real palette indices substituted into the team slots.
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

    /// The "code" name as in C++/the reference (`red`, `green`, …).
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

    /// Recolor LUT of this team (`ResourceManager_BuildColorTables`).
    pub fn lut(self) -> ColorLut {
        ColorLut::for_team(self)
    }
}

/// "Palette index -> palette index" LUT: substitution of the team slots.
///
/// The construction repeats C++ verbatim: `j = 0`, `k = 32`; for `j == 8`
/// `k += 8`; then `lut[k] = colors[j & 7]`, `k += 1`, `j += 1`, while
/// `j < 24`. Result: slots 32–39 get colors 0…7, slots 48–63 — the same
/// eight colors twice (48–55 and 56–63).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorLut(pub [u8; PALETTE_SIZE]);

impl ColorLut {
    /// Identity LUT (no recolor).
    pub fn identity() -> Self {
        let mut lut = [0u8; PALETTE_SIZE];

        for (index, slot) in lut.iter_mut().enumerate() {
            *slot = index as u8;
        }

        Self(lut)
    }

    /// LUT of the team.
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

    /// Index after recoloring.
    #[inline]
    pub fn map(&self, index: u8) -> u8 {
        self.0[index as usize]
    }

    /// The whole table.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// The LUT changes nothing.
    pub fn is_identity(&self) -> bool {
        self.0
            .iter()
            .enumerate()
            .all(|(index, &slot)| index as u8 == slot)
    }
}

/// Palette slots that are replaced with team colors, in assignment order.
///
/// Returns 24 pairs `(slot, position of the color in the team set)`: first
/// 32–39 (`0..7`), then 48–63 (`0..7`, `0..7`).
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
// Brightness
// ---------------------------------------------------------------------------

/// Nearest palette color search — `Color_MapColor` (`color.cpp`).
///
/// `full_scan = false` excludes the index ranges 9–31 and 96–127: they
/// are reserved for service/team colors, and the original does not
/// use them when matching. The distance is the sum of squared RGB differences,
/// the comparison is strict (the first minimum wins), an exact match
/// stops the search.
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

/// Brightness tables (§6.3): 8 levels, each an "index -> index" LUT.
///
/// Levels 0–6 are built by the formula `rgb' = rgb * (level * 32) / 224` over
/// the **6-bit** palette (like `World::ApplyPaletteAndTints`), index 31
/// is forcibly kept 31. Level 7 — the identity (full brightness;
/// in the C++ port this is block 15 of the table, filled with identity values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrightnessLut {
    levels: [[u8; PALETTE_SIZE]; BRIGHTNESS_LEVELS],
}

impl BrightnessLut {
    /// Builds the tables from the palette (`WindowManager_ColorPalette`).
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

        // The full brightness level — identity.
        for (index, slot) in levels[BRIGHTNESS_LEVELS - 1].iter_mut().enumerate() {
            *slot = index as u8;
        }

        Self { levels }
    }

    /// LUT of level `level` (values above 7 are clamped to 7).
    #[inline]
    pub fn level(&self, level: u8) -> &[u8; PALETTE_SIZE] {
        &self.levels[usize::from(level.min(7))]
    }

    /// Index after applying brightness.
    #[inline]
    pub fn map(&self, level: u8, index: u8) -> u8 {
        self.level(level)[index as usize]
    }

    /// Full brightness level.
    pub fn full(&self) -> &[u8; PALETTE_SIZE] {
        self.level(FULL_BRIGHTNESS_LEVEL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic palette with a unique 6-bit color per index:
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

        // Slots 32–39 and 48–63 are substituted, everything else is the identity.
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
        // Slots 48–63 = eight colors twice.
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

        // The same numbering as in the team LUT.
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

        // The colors of indices 9–31 and 96–127 are unreachable with full_scan = false.
        for reserved in [9u8, 20, 31, 96, 110, 127] {
            let value = palette.raw(reserved);
            let found = map_color(&palette.raw, value[0], value[1], value[2], false);

            assert!(
                !((9..=31).contains(&found) || (96..=127).contains(&found)),
                "map_color вернул зарезервированный индекс {found}"
            );
            // With full_scan the exact match is found.
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

        // Level 0 is the darkest: all colors except the fixed index 31
        // converge to black.
        for index in 0..PALETTE_SIZE {
            if index == usize::from(FIXED_BRIGHTNESS_INDEX) {
                continue;
            }

            let mapped = brightness.map(0, index as u8);

            assert_eq!(palette.raw(mapped), [0, 0, 0], "уровень 0, индекс {index}");
        }

        // Levels 0–6 are computed exactly as in `World::ApplyPaletteAndTints`:
        // rgb' = rgb * (level * 32) / 224, then `Color_MapColor` without full_scan.
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

        // Level 5 (shadow) is noticeably darker: most colors change.
        let changed = (0..PALETTE_SIZE)
            .filter(|&index| brightness.map(SHADOW_BRIGHTNESS_LEVEL, index as u8) != index as u8)
            .count();

        assert!(changed > 100, "уровень 5 изменил всего {changed} цветов");

        // Index 31 is fixed at all levels 0–6.
        for level in 0..=6u8 {
            assert_eq!(brightness.map(level, FIXED_BRIGHTNESS_INDEX), 31);
        }

        // Levels are clamped.
        assert_eq!(brightness.level(200), brightness.level(7));
    }

    /// Palette where each slot is colored by its number: `(i, 2i, 4i)`.
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
        // Order and timings — row by row `World::InitColorCycles`
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

        // The mine beacons — 6 fps on four slots: full cycle 666.7 ms.
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

        // Less than the shortest period (10 fps = 100 ms) — not a single step.
        assert!(!cycles.advance(0.09));
        assert_eq!(rotate_palette(&palette, &cycles), palette);

        // 170 ms — exactly one 6 fps step (166 ms): 13..16 shifts right.
        assert!(cycles.advance(0.08));
        assert_eq!(cycles.step_counts()[1], 1);
        let once = rotate_palette(&palette, &cycles);

        assert_eq!(once.raw(13), palette.raw(16), "start <- end (вправо)");
        assert_eq!(once.raw(14), palette.raw(13));
        assert_eq!(once.raw(15), palette.raw(14));
        assert_eq!(once.raw(16), palette.raw(15));
        // The slots neighboring 13..16 are untouched (9..12 and 17..20 rotate by their own
        // 9 fps cycles, 13..16 do not depend on them).
        assert_eq!(once.raw(8), palette.raw(8));
        assert_eq!(once.raw(32), palette.raw(32));
        // 8-bit components are recomputed as raw * 4.
        assert_eq!(once.rgb(13), [once.raw(13)[0] * 4, once.raw(13)[1] * 4, once.raw(13)[2] * 4]);

        // A full cycle (4 steps of 1/6 s) returns slots 13..16 to their place
        // (the other ranges live at their own rate and move as well).
        for _ in 0..3 {
            cycles.advance(1.0 / 6.0);
        }
        assert_eq!(cycles.step_counts()[1], 4);
        let full = rotate_palette(&palette, &cycles);

        for index in 13..=16 {
            assert_eq!(full.raw(index), palette.raw(index), "слот {index}");
        }

        // The 5th step = 4 (a full circle) + 1: slots 13..16 as after one step.
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

        // 0.5 s: 9..12 @9 fps (4 steps = full circle), 25..30 @2 fps (1 step),
        // 110..116 @10 fps (5 steps = full circle).
        cycles.advance(0.5);
        let steps = *cycles.step_counts();

        assert_eq!(steps[0], 4, "9 fps: 111 мс × 4 = 444 мс");
        assert_eq!(steps[4], 1, "2 fps: 500 мс");
        assert_eq!(steps[8], 5, "10 fps: 100 мс × 5");
        assert_eq!(steps[1], 3, "6 fps: 166 мс × 3");
        assert_eq!(steps[5], 3, "одиночный слот 31 — тот же 6 fps");

        let rotated = rotate_palette(&palette, &cycles);

        // 9..12 — the only range rotating left; 4 steps = a circle.
        for index in 9..=12 {
            assert_eq!(rotated.raw(index), palette.raw(index));
        }
        // 25..30 @2 fps shifted one to the right.
        assert_eq!(rotated.raw(25), palette.raw(30));
        assert_eq!(rotated.raw(26), palette.raw(25));

        // Another step: 9..12 left by one slot.
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
