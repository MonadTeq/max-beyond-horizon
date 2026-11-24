//! Filling the 1:1 game window panels (item A of `max-brh-docs/visual-1to1-status.md`).
//!
//! Over `FRAMEPIC` the original (`game_manager.cpp`) draws the pictures of the
//! buttons/panels from `MAX.RES` and text with the GNW fonts. Here this is collected in
//! [`Panels`]:
//!
//! * top: `FILES_OF/ON` (17,4) 67×18, `PREF_OFF/ON` (86,4) 67×18,
//!   `ENDTRN_U` (390,2) 72×21, `TURNS` (468,2) 61×21, `TIMER` (534,2) 61×21;
//! * the `CORNER_FLIC` portrait (10,29) 128×128 — the unit FLC animation
//!   (`max_assets::flic`, 15 FPS, see [`crate::portrait`]); without FLC/selection —
//!   a black background (or a static sprite ×2 as a fallback);
//! * statistics `STAT_WINDOW` (10,173) 153×48 — four lines in
//!   `GNW_TEXT_FONT_2`, the composition depends on the unit ([`rows_for`], a port of
//!   `GameManager_UpdateInfoDisplay`): mobile — `Hits/Ammo/Speed/Shots`,
//!   buildings — `Hits/Cargo` and the `Usage`/`Power`/`Total` lines of the complex
//!   economy (no `Speed` for a mine);
//! * the toggle grid `SURV_OFF/ON`…`NAMES_UP/DN` (9 buttons 55×16/18);
//! * zoom `ZOOMPNL1` + `ZOOMPTR` at (20,275) 130×19;
//! * bottom: `XYPOS` (242,456) 90×21 and `UNITNAME` (338,456) 222×21;
//! * minimap `WINDOW_MINIMAP` (15,356) 112×112 from the `minimap` layer of `.WRL`.
//!
//! Coordinates — from `window_manager.cpp::windows[]` (see [`max_render::layout`]),
//! labels — from `language.json` (`Files`, `Preferences`, `End Turn`, `Survey`,
//! …), colors — palette indices exactly as in `game_manager.cpp`/`button.cpp`
//! (`FontColor(0xA2, 0xA7, 0xE6)` for labels, `(0xA0, 0xA2, 0xE6)` for
//! End Turn).
//!
//! ## Mouse coordinates
//!
//! The map occupies [`layout::MAIN_MAP`] = (180,18) 448×448. [`screen_to_map`]
//! converts a window point into local map coordinates, and [`panel_button_at`]
//! checks the panels (including End Turn, which deliberately extends onto the map).

use max_assets::image::SimpleImage;
use max_core::enums::{UnitOrder, UnitOrderState, UnitType, VictoryType};
use max_core::game::PLAYER_TEAM_COUNT;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::{UnitInfo, UnitListKind};
use max_core::unit_values::{
    ATTRIB_AMMO, ATTRIB_ATTACK, ATTRIB_HITS, ATTRIB_ROUNDS, ATTRIB_SPEED, ATTRIB_STORAGE,
};
use max_core::universe::StateRef;
use max_core::victory::{
    cargo_life_consumption_rate, cargo_power_consumption_rate, cargo_raw_consumption_rate,
    cargo_type_of, CargoType, NetProduction,
};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::layout;
use max_render::text::{
    draw_text_gnw, draw_text_gnw_outlined, draw_text_gnw_shadowed, GnwFont, GnwFontId,
};
use max_render::viewport::Viewport;

use crate::assets::Assets;

/// Regular panel text color (`0xA2`).
pub const TEXT: u8 = 0xA2;
/// `COLOR_GREEN` (`0x02`) — the unit name in the portrait
/// (`GameManager_MenuUnitSelect`: `win_print(..., COLOR_GREEN)`).
pub const GREEN: u8 = 0x02;
/// `COLOR_RED` (`0x01`) — value ≤ 1/4 of the base (`GameManager_DrawInfoDisplayRow`).
pub const RED: u8 = 0x01;
/// `COLOR_YELLOW` (`0x04`) — value ≤ 1/2 of the base.
pub const YELLOW: u8 = 0x04;
/// Button label outline (`0xA7`).
pub const CAPTION_OUTLINE: u8 = 0xA7;
/// Shadow of button labels and text (`0xE6`).
pub const SHADOW: u8 = 0xE6;
/// End Turn light text (`0xA0`).
pub const END_TURN_TEXT: u8 = 0xA0;
/// End Turn button pressed (not used in a static frame).
pub const END_TURN_OUTLINE: u8 = 0xA2;
/// Shadow of the pressed End Turn label (`FontColor(color, color, 0x3F)`,
/// `game_manager.cpp:7882`).
pub const TEAM_TEXT_SHADOW: u8 = 0x3F;
/// Palette index of the gray team in the baked caption (`game_manager.cpp:7876`:
/// `if (i == PLAYER_TEAM_GRAY) color = 0xFF;`).
pub const GRAY_TEAM_TEXT: u8 = 0xFF;

/// Game window palette.
///
/// In a match the system palette is the `FRAMEPIC` palette (C++:
/// `WindowManager_LoadBigImage(FRAMEPIC, …, palette_from_image=true)`), not
/// the sprite `COLOR.PAL`. Indices 0x01/0x02/0x04 in it are bright red,
/// green and yellow, while 0xA2/0xA7/0xE6 are the text and shadow colors; `COLOR.PAL`
/// gives different colors for these indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiPalette {
    colors: [[u8; 3]; 256],
}

impl UiPalette {
    /// Palette from `FRAMEPIC` (8-bit channels, without `*4`).
    pub fn from_big_image(image: &max_assets::image::BigImage) -> Self {
        let mut colors = [[0u8; 3]; 256];

        for (index, color) in colors.iter_mut().enumerate() {
            *color = image.color(index as u8);
        }

        Self { colors }
    }

    /// Palette from ready colors (for tests and synthetic frames).
    pub const fn from_colors(colors: [[u8; 3]; 256]) -> Self {
        Self { colors }
    }

    /// Window system palette after loading the world: indices 64…159 are taken from
    /// `.WRL` (`World::Load` → `WindowManager_ColorPalette[i] = palette[i]/4`
    /// in C++; here — already 8-bit `TerrainAtlas` colors).
    pub fn with_wrl_palette(mut self, wrl: &max_render::terrain::PaletteRgba) -> Self {
        for (index, color) in wrl.iter().enumerate().skip(64).take(96) {
            self.colors[index] = [color[0], color[1], color[2]];
        }

        self
    }

    /// Palette from the loaded chrome; `None` if `FRAMEPIC` is not loaded.
    pub fn from_assets(assets: &Assets) -> Option<Self> {
        assets
            .chrome
            .as_ref()
            .map(|chrome| Self::from_big_image(&chrome.frame))
    }

    /// Color by palette index.
    pub fn rgba(&self, index: u8) -> Rgba {
        Rgba::from_rgb(self.colors[usize::from(index)])
    }

    /// The whole palette in the render format (`[[u8; 4]; 256]`) — for simple images
    /// (resource markers, cursors) indexed by the window system palette,
    /// not by the `.WRL`/`COLOR.PAL` palette.
    pub fn palette_rgba(&self) -> max_render::terrain::PaletteRgba {
        self.colors.map(|color| [color[0], color[1], color[2], 255])
    }

    /// Nearest palette color — the literal `Color_MapColor(palette, r, g, b, true)`
    /// (`color.cpp:79`): a scan of all 256 colors by the sum of squared differences,
    /// the first minimum wins.
    ///
    /// Needed for planet minimaps: `Menu_LoadPlanetMinimap` converts the `.WRL`
    /// colors (divided by 4) into the window palette with this call.
    pub fn map_color(&self, rgb: [u8; 3]) -> Rgba {
        let mut best = 0usize;
        let mut best_distance = i32::MAX;

        for (index, color) in self.colors.iter().enumerate() {
            let red = i32::from(color[0]) - i32::from(rgb[0]);
            let green = i32::from(color[1]) - i32::from(rgb[1]);
            let blue = i32::from(color[2]) - i32::from(rgb[2]);
            let distance = red * red + green * green + blue * blue;

            if distance < best_distance {
                best_distance = distance;
                best = index;

                if distance == 0 {
                    break;
                }
            }
        }

        Rgba::from_rgb(self.colors[best])
    }

    /// Message box background for one frame pixel — the style colour tables
    /// (`MessageManager_MessageBox_BgColorArray`, `message_manager.cpp:50-54`):
    ///
    /// * `INFO` — `ResourceManager_DarkeningColorIndexTable` (`world.cpp:499-509`):
    ///   `c' = max(7, c) * 31 / 63`;
    /// * `NOTICE` — `Color_GenerateIntensityTable2` (`color.cpp:167-200`) with
    ///   `(63, 63, 63)` and `(63, 0, 0)` on snow;
    /// * `WARNING` — `Color_GenerateIntensityTable3` (`color.cpp:202-220`) with
    ///   `(63, 0, 0)` and factor `31` (`63` on snow).
    ///
    /// The C++ tables are indexed by palette index; a frame pixel already holds the
    /// palette RGB, so applying the formula directly and snapping to the nearest
    /// palette colour (`Color_MapColor`) is equivalent.
    pub fn message_box_color(&self, style: MessageBoxStyle, snow: bool, color: Rgba) -> Rgba {
        let r = i32::from(color.r);
        let g = i32::from(color.g);
        let b = i32::from(color.b);

        let (red, green, blue) = match style {
            MessageBoxStyle::Info => (
                (r.max(7) * 31) / 63,
                (g.max(7) * 31) / 63,
                (b.max(7) * 31) / 63,
            ),
            MessageBoxStyle::Notice => {
                let (r_level, g_level, b_level) = if snow { (63, 0, 0) } else { (63, 63, 63) };
                let max_level = r_level.max(g_level).max(b_level).max(1);
                let max_color = (r.max(g).max(b) + max_level) / 2;

                (
                    (max_color * r_level) / max_level,
                    (max_color * g_level) / max_level,
                    (max_color * b_level) / max_level,
                )
            }
            MessageBoxStyle::Warning => {
                let factor = if snow { 63 } else { 31 };
                let tint = |channel: i32, level: i32| (channel.max(level / 4) * level) / factor;

                (tint(r, 63), tint(g, 0), tint(b, 0))
            }
        };

        let clamp = |value: i32| value.clamp(0, 255) as u8;

        self.map_color([clamp(red), clamp(green), clamp(blue)])
    }

    /// Palette color by index as `[r, g, b]`.
    pub fn color_rgb(&self, index: u8) -> [u8; 3] {
        self.colors[usize::from(index)]
    }

    /// Palette index by the exact frame color (`None` if the color is absent —
    /// e.g. the pixel did not come from this palette).
    ///
    /// Needed by `Color_RecolorPixels`: the backing is recolored by palette
    /// **indices**, while the frame stores RGB already.
    pub fn index_of(&self, color: Rgba) -> Option<u8> {
        self.colors
            .iter()
            .position(|candidate| {
                candidate[0] == color.r && candidate[1] == color.g && candidate[2] == color.b
            })
            .map(|index| index as u8)
    }

    /// `Color_GenerateIntensityTable1(red, green, blue, factor1, factor2, table)`
    /// (`color.cpp:132`): the palette "dimming" table — each palette color
    /// is blended with the level and replaced by the nearest palette color
    /// (Color_MapColor with a full scan, the first minimum wins).
    ///
    /// The mine allocation dialog (`AllocMenu`) builds such a table
    /// `(0, 0, 25, 1, 1)` and recolors the backing under the bar number with it
    /// (`Color_RecolorPixels`).
    pub fn intensity_table(
        &self,
        red_level: i32,
        green_level: i32,
        blue_level: i32,
        factor1: i32,
        factor2: i32,
    ) -> [u8; 256] {
        let factor_sum = (factor1 + factor2).max(1);
        let mut table = [0u8; 256];

        for (index, slot) in table.iter_mut().enumerate() {
            let color = self.colors[index];
            let target = [
                (i32::from(color[0]) * factor2 + red_level * factor1) / factor_sum,
                (i32::from(color[1]) * factor2 + green_level * factor1) / factor_sum,
                (i32::from(color[2]) * factor2 + blue_level * factor1) / factor_sum,
            ];

            // `Color_GetColorDistance` to its own color; at zero
            // distance the C++ search loop does not run at all.
            let mut best = index;
            let mut best_distance = palette_distance(self.colors[index], target);

            if best_distance > 0 {
                for (candidate, value) in self.colors.iter().enumerate() {
                    let distance = palette_distance(*value, target);

                    if distance < best_distance {
                        best_distance = distance;
                        best = candidate;
                    }
                }
            }

            *slot = best as u8;
        }

        table
    }
}

/// `Color_GetColorDistance` (`color.cpp:120`) — the sum of squared differences.
fn palette_distance(color: [u8; 3], target: [i32; 3]) -> i32 {
    let red = i32::from(color[0]) - target[0];
    let green = i32::from(color[1]) - target[1];
    let blue = i32::from(color[2]) - target[2];

    red * red + green * green + blue * blue
}

/// A panel toggle or a button handled by the chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelButton {
    /// Files button (17,4) 67×18.
    Files,
    /// Preferences button (86,4) 67×18.
    Preferences,
    /// End Turn button (390,2) 72×21 (extends onto the map).
    EndTurn,
    /// Play (146,123) 19×18.
    Play,
    /// Pause (146,143) 19×19.
    Pause,
    /// Center (4,227) 20×23.
    Center,
    /// Lock (32,227) 20×22.
    Lock,
    /// Prev (60,228) 38×22.
    Prev,
    /// Done (99,228) 26×22.
    Done,
    /// Next (125,228) 38×22.
    Next,
    /// Help (21,250) 25×24.
    Help,
    /// Reports (51,252) 48×19.
    Reports,
    /// Chat (102,252) 48×19.
    Chat,
    /// Minimap `2x` button (136,387) 27×26.
    Minimap2x,
    /// Minimap `TNT` button (136,413) 27×27.
    MinimapTnt,
    /// Zoom `+` button (2,275) 18×14.
    ZoomPlus,
    /// Zoom slider (20,275) 130×14.
    ZoomSlider,
    /// Zoom `−` button (150,275) 20×14.
    ZoomMinus,
    /// Survey toggle.
    Survey,
    /// Status toggle.
    Status,
    /// Colors toggle.
    Colors,
    /// Hits toggle.
    Hits,
    /// Ammo toggle.
    Ammo,
    /// Range toggle.
    Range,
    /// Scan toggle.
    Scan,
    /// Grid toggle.
    Grid,
    /// Names toggle.
    Names,
}

impl PanelButton {
    /// All buttons in hit-test order.
    pub const ALL: [Self; 27] = [
        Self::Files,
        Self::Preferences,
        Self::EndTurn,
        Self::Play,
        Self::Pause,
        Self::Center,
        Self::Lock,
        Self::Prev,
        Self::Done,
        Self::Next,
        Self::Help,
        Self::Reports,
        Self::Chat,
        Self::Minimap2x,
        Self::MinimapTnt,
        Self::ZoomPlus,
        Self::ZoomSlider,
        Self::ZoomMinus,
        Self::Survey,
        Self::Status,
        Self::Colors,
        Self::Hits,
        Self::Ammo,
        Self::Range,
        Self::Scan,
        Self::Grid,
        Self::Names,
    ];

    /// The nine grid toggles (left column → right).
    pub const TOGGLES: [Self; 9] = [
        Self::Survey,
        Self::Status,
        Self::Colors,
        Self::Hits,
        Self::Ammo,
        Self::Range,
        Self::Scan,
        Self::Grid,
        Self::Names,
    ];

    /// Button rectangle in the window (inclusive `windows[]` coordinates → sizes).
    pub const fn rect(self) -> PixelRect {
        match self {
            Self::Files => layout::FILES_BUTTON,
            Self::Preferences => layout::PREFS_BUTTON,
            Self::EndTurn => layout::END_TURN_BUTTON,
            Self::Play => layout::PLAY_BUTTON,
            Self::Pause => layout::PAUSE_BUTTON,
            Self::Center => layout::CENTER_BUTTON,
            Self::Lock => layout::LOCK_BUTTON,
            Self::Prev => layout::PREV_BUTTON,
            Self::Done => layout::DONE_BUTTON,
            Self::Next => layout::NEXT_BUTTON,
            Self::Help => layout::HELP_BUTTON,
            Self::Reports => layout::REPORTS_BUTTON,
            Self::Chat => layout::CHAT_BUTTON,
            Self::Minimap2x => layout::MINIMAP_2X,
            Self::MinimapTnt => layout::MINIMAP_TNT,
            Self::ZoomPlus => layout::ZOOM_PLUS,
            Self::ZoomSlider => layout::ZOOM_SLIDER_BUTTON,
            Self::ZoomMinus => layout::ZOOM_MINUS,
            Self::Survey => layout::SURVEY_BUTTON,
            Self::Status => layout::STATUS_BUTTON,
            Self::Colors => layout::COLORS_BUTTON,
            Self::Hits => layout::HITS_BUTTON,
            Self::Ammo => layout::AMMO_BUTTON,
            Self::Range => layout::RANGE_BUTTON,
            Self::Scan => layout::SCAN_BUTTON,
            Self::Grid => layout::GRID_BUTTON,
            Self::Names => layout::NAME_BUTTON,
        }
    }

    /// Button label (`language.json`, en-US), if the original has one.
    pub const fn caption(self) -> Option<&'static str> {
        match self {
            Self::Files => Some("Files"),
            Self::Preferences => Some("Preferences"),
            Self::EndTurn => Some("End Turn"),
            Self::Prev => Some("Prev"),
            Self::Done => Some("Done"),
            Self::Next => Some("Next"),
            Self::Reports => Some("Reports"),
            Self::Chat => Some("Chat"),
            Self::Survey => Some("Survey"),
            Self::Status => Some("Status"),
            Self::Colors => Some("Colors"),
            Self::Hits => Some("Hits"),
            Self::Ammo => Some("Ammo"),
            Self::Range => Some("Range"),
            Self::Scan => Some("Scan"),
            Self::Grid => Some("Grid"),
            Self::Names => Some("Names"),
            Self::Play
            | Self::Pause
            | Self::Center
            | Self::Lock
            | Self::Help
            | Self::Minimap2x
            | Self::MinimapTnt
            | Self::ZoomPlus
            | Self::ZoomSlider
            | Self::ZoomMinus => None,
        }
    }

    /// Label font: End Turn — FONT_5, the rest — FONT_2
    /// (`GameManager_MenuInitButtons`).
    pub const fn caption_font(self) -> GnwFontId {
        match self {
            Self::EndTurn => GnwFontId::Font5,
            _ => GnwFontId::Font2,
        }
    }

    /// Label offset `(x, y)` — `GameManager_MenuItemLabelOffsets`
    /// (game_manager.cpp:275-278), 1:1 by the GameManager_MenuItems index.
    ///
    /// C++ `Button::SetCaption(caption, x, y, …)` (`button.cpp:412`) builds
    /// the rectangle `{0, 0, width - x, height - y}` and centers the text **in
    /// it**: horizontal — `Text_TextLine` (`text.cpp:772`) by `width - x`,
    /// vertical — `uly_caption = ((height - y) - Text_GetHeight()) / 2 + y`.
    /// So both offsets shrink the centering area rather than shifting it.
    pub const fn caption_offset(self) -> (i32, i32) {
        match self {
            Self::Files | Self::Preferences => (0, 2),
            Self::Prev => (-10, 2),
            Self::Next => (10, 2),
            Self::Done | Self::Reports | Self::Chat => (0, 3),
            Self::Colors | Self::Range | Self::Names => (2, 4),
            Self::Survey | Self::Status | Self::Hits | Self::Ammo | Self::Scan | Self::Grid => {
                (2, 2)
            }
            _ => (0, 0),
        }
    }

    /// Vertical label offset (`GameManager_MenuItemLabelOffsets[i].y`).
    pub const fn caption_offset_y(self) -> i32 {
        self.caption_offset().1
    }

    /// Label position in the frame — `Button::SetCaption` (`button.cpp:412-449`).
    ///
    /// `SetCaption(caption, x, y, …)` builds `Rect bounds = {0, 0, width - x,
    /// height - y}`, then `Text_TextLine` (`text.cpp:772-783`) centers
    /// the line in `bounds`: `spacing = (swidth - (Text_GetWidth + 1)) / 2`
    /// (a shift only if `spacing > 0`), `uly_caption = ((height - y) -
    /// Text_GetHeight()) / 2 + y`.
    pub fn caption_position(self) -> (i32, i32) {
        let rect = self.rect();
        let font = self.caption_font().font();
        let (offset_x, offset_y) = self.caption_offset();
        let bounds_width = rect.width - offset_x;
        let bounds_height = rect.height - offset_y;
        let text_width = self
            .caption()
            .map(|caption| font.text_width(caption) + 1)
            .unwrap_or(0);
        let spacing = (bounds_width - text_width) / 2;

        (
            rect.ulx + spacing.max(0),
            rect.uly + (bounds_height - font.height()) / 2 + offset_y,
        )
    }

    /// HUD toggle (`PanelButton::is_toggle`).
    pub const fn is_toggle(self) -> bool {
        matches!(
            self,
            Self::Survey
                | Self::Status
                | Self::Colors
                | Self::Hits
                | Self::Ammo
                | Self::Range
                | Self::Scan
                | Self::Grid
                | Self::Names
        )
    }
}

/// Local panel state (`HudState::panels`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelState {
    /// Survey pressed.
    pub survey: bool,
    /// Status pressed.
    pub status: bool,
    /// Colors pressed.
    pub colors: bool,
    /// Hits pressed.
    pub hits: bool,
    /// Ammo pressed.
    pub ammo: bool,
    /// Range pressed.
    pub range: bool,
    /// Scan pressed.
    pub scan: bool,
    /// Grid pressed.
    pub grid: bool,
    /// Names pressed.
    pub names: bool,
    /// Minimap in `2x` mode.
    pub minimap_2x: bool,
    /// Minimap in `TNT` mode.
    pub minimap_tnt: bool,
    /// Cell under the cursor (for the coordinates/description at the bottom), if the mouse is on the map.
    pub cursor_grid: Option<(i16, i16)>,
    /// Remaining turn time in seconds (the timer in the top row).
    pub timer_seconds: u32,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            survey: false,
            status: false,
            colors: false,
            hits: false,
            ammo: false,
            range: false,
            scan: false,
            grid: false,
            names: false,
            minimap_2x: false,
            minimap_tnt: false,
            cursor_grid: None,
            timer_seconds: 0,
        }
    }
}

impl PanelState {
    /// Whether the toggle is on.
    pub fn get(&self, button: PanelButton) -> bool {
        match button {
            PanelButton::Survey => self.survey,
            PanelButton::Status => self.status,
            PanelButton::Colors => self.colors,
            PanelButton::Hits => self.hits,
            PanelButton::Ammo => self.ammo,
            PanelButton::Range => self.range,
            PanelButton::Scan => self.scan,
            PanelButton::Grid => self.grid,
            PanelButton::Names => self.names,
            _ => false,
        }
    }

    /// Toggles the state; returns the new value.
    pub fn toggle(&mut self, button: PanelButton) -> bool {
        let slot = match button {
            PanelButton::Survey => &mut self.survey,
            PanelButton::Status => &mut self.status,
            PanelButton::Colors => &mut self.colors,
            PanelButton::Hits => &mut self.hits,
            PanelButton::Ammo => &mut self.ammo,
            PanelButton::Range => &mut self.range,
            PanelButton::Scan => &mut self.scan,
            PanelButton::Grid => &mut self.grid,
            PanelButton::Names => &mut self.names,
            _ => return false,
        };

        *slot = !*slot;

        *slot
    }
}

/// `STATUS` overlay icons — `DrawMap_RenderStatusDisplay` (`drawmap.cpp:444`).
///
/// C++ loads them from `MAX.RES` on demand via `ResourceManager_LoadResource`
/// (`IL_DSBLD`, `IL_SPEED`, `IL_SHOTS`); the port keeps them next to the panels and
/// draws them with the STATUS button enabled (see [`crate::overlays`]).
#[derive(Debug, Clone, Default)]
pub struct StatusIcons {
    /// `IL_DSBLD` — the unit is in `ORDER_DISABLE`.
    pub disabled: Option<SimpleImage>,
    /// `IL_SPEED` — has speed (`unit->speed != 0`).
    pub speed: Option<SimpleImage>,
    /// `IL_SHOTS` — has shots (`unit->shots > 0`).
    pub shots: Option<SimpleImage>,
}

impl StatusIcons {
    /// Whether there is at least one icon (otherwise the STATUS overlay has nothing to draw).
    pub fn is_empty(&self) -> bool {
        self.disabled.is_none() && self.speed.is_none() && self.shots.is_none()
    }
}

/// Pips and icons of the statistics rows — `GameManager_DrawInfoDisplayRowIcons`
/// (`game_manager.cpp:7290-7367`) with the `SI_*`/`EI_*` resources.
///
/// Every row repeats `base` small icon images (`SI_*`, the "full" pip) and switches to
/// the paired `EI_*` image (`icon + 1` in C++) for the consumed part of the row. The
/// images are 4-19 px wide; five pips form a column block, further pips wrap into the
/// next block to the right (`i / 5`, `i % 5`).
#[derive(Debug, Clone, Default)]
pub struct StatIcons {
    images: std::collections::BTreeMap<&'static str, SimpleImage>,
}

impl StatIcons {
    /// All row icons of the UI-level statistics panel (`resourcetable.hpp:658-687`:
    /// `SI_HITSB`…`EI_WORK`; `SI_x + 1 = EI_x`).
    const NAMES: [&'static str; 22] = [
        "SI_HITSB", "EI_HITSB", "SI_HITSY", "EI_HITSY", "SI_HITSR", "EI_HITSR", "SI_AMMO",
        "EI_AMMO", "SI_RAW", "EI_RAW", "SI_FUEL", "EI_FUEL", "SI_GOLD", "EI_GOLD", "SI_SPEED",
        "EI_SPEED", "SI_SHOTS", "EI_SHOTS", "SI_POWER", "EI_POWER", "SI_WORK", "EI_WORK",
    ];

    /// Loads the icons from `MAX.RES`; missing ones are simply skipped.
    pub fn load(max_res: &max_assets::ResArchive) -> Self {
        let mut images = std::collections::BTreeMap::new();

        for name in Self::NAMES {
            let Ok(bytes) = max_res.read(name) else {
                continue;
            };

            if let Ok(image) = SimpleImage::decode(bytes) {
                images.insert(name, image);
            }
        }

        Self { images }
    }

    /// The `(full, empty)` pair of a row icon.
    fn pair(
        &self,
        normal: &'static str,
        empty: &'static str,
    ) -> Option<(&SimpleImage, &SimpleImage)> {
        Some((self.images.get(normal)?, self.images.get(empty)?))
    }

    /// Whether at least one pair is available.
    pub fn is_empty(&self) -> bool {
        self.images.len() < 2
    }
}

/// The "released/pressed" picture pair.
#[derive(Debug, Clone)]
pub struct ButtonPair {
    up: SimpleImage,
    down: SimpleImage,
}

/// Timer text color — `GameManager_DrawTurnTimer`
/// (`game_manager.cpp:2197-2213`): `turn_time <= 20` → `COLOR_RED` (`0x01`),
/// otherwise `0xA2`. Zero is red too: with the timer off C++ draws «00:00»
/// in red.
pub fn timer_color_index(seconds: u32) -> u8 {
    if seconds <= 20 {
        RED
    } else {
        TEXT
    }
}

/// Whether the End Turn button is pressed on this turn.
///
/// `GameManager_UpdateGui` (`game_manager.cpp:2437-2481`) calls
/// `GameManager_MenuClickEndTurnButton(rest_state)` by the team type:
/// `TEAM_TYPE_PLAYER` → `true` (the button is released), `TEAM_TYPE_COMPUTER` → `0`
/// (the button is pressed, `SetRestState(true)`), `TEAM_TYPE_REMOTE` — does not call it at all
/// (the state remains from the previous team; in the port a remote team's turn is
/// shown released, as after one's own turn).
pub fn end_turn_pressed(active_team_kind: TeamKind) -> bool {
    active_team_kind == TeamKind::Computer
}

/// Palette index of the pressed End Turn label — `FontColor(color, color,
/// 0x3F)` from `GameManager_MenuInitButtons` (`game_manager.cpp:7860-7883`):
/// `color = team + 1`, for the gray team `0xFF`.
pub fn end_turn_caption_color(team: TeamId) -> u8 {
    if team.0 == 3 {
        GRAY_TEAM_TEXT
    } else {
        team.0 + 1
    }
}

/// Turn timer text — `sprintf(text, "%2.2i:%2.2i", t / 60, t % 60)`
/// (`GameManager_DrawTurnTimer`, `game_manager.cpp:2215`).
///
/// `%2.2i` — width 2 and **precision** 2, i.e. at least two digits with a leading zero
/// (`1 → "01"`, `0 → "00"`); `{:02}` in Rust gives the same string for any
/// value. It shows the **remaining turn time** in minutes:seconds, not turns.
pub fn timer_text(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Loaded panel pictures.
#[derive(Debug, Clone)]
pub struct Panels {
    files: ButtonPair,
    preferences: ButtonPair,
    end_turn: SimpleImage,
    /// Pressed End Turn button for the team `RED/GREEN/BLUE/GRAY` —
    /// `R_ENDT_D`/`G_ENDT_D`/`B_ENDT_D`/`W_ENDT_D`
    /// (`GameManager_MenuClickEndTurnButton`: `CopyDown(R_ENDT_D + team)`).
    end_turn_down: [SimpleImage; 4],
    turns: SimpleImage,
    timer: SimpleImage,
    coordinates: SimpleImage,
    unit_description: SimpleImage,
    zoom_background: SimpleImage,
    zoom_pointer: SimpleImage,
    toggles: [ButtonPair; 9],
    minimap_2x: ButtonPair,
    minimap_tnt: ButtonPair,
    /// Other buttons: a ready picture and its position in the window.
    aux: Vec<(SimpleImage, PixelRect)>,
    /// `STATUS` overlay icons (`IL_DSBLD`/`IL_SPEED`/`IL_SHOTS`).
    pub status_icons: StatusIcons,
    /// Statistics row pips (`SI_*`/`EI_*`, `GameManager_DrawInfoDisplayRowIcons`).
    pub stat_icons: StatIcons,
}

impl Panels {
    /// Loads the panel pictures from `MAX.RES`.
    ///
    /// The pictures of the top buttons/counters, the bottom row, zoom and
    /// the nine toggles are mandatory. If something is missing, `None` is returned — the chrome
    /// keeps drawing only `FRAMEPIC`.
    pub fn load(max_res: &max_assets::ResArchive) -> Option<Self> {
        let simple = |name: &str| -> Option<SimpleImage> {
            let bytes = max_res.read(name).ok()?;
            SimpleImage::decode(bytes).ok()
        };
        let pair = |up: &str, down: &str| -> Option<ButtonPair> {
            Some(ButtonPair {
                up: simple(up)?,
                down: simple(down)?,
            })
        };

        let toggles = [
            pair("SURV_OFF", "SURV_ON")?,
            pair("STAT_OFF", "STAT_ON")?,
            pair("COLOR_OF", "COLOR_ON")?,
            pair("HITS_OF", "HITS_ON")?,
            pair("AMMO_OF", "AMMO_ON")?,
            pair("RANG_OFF", "RANG_ON")?,
            pair("VISN_OFF", "VISN_ON")?,
            pair("GRID_OFF", "GRID_ON")?,
            pair("NAMES_UP", "NAMES_DN")?,
        ];

        let mut aux = Vec::new();
        for (name, rect) in [
            ("PLAY_OF", layout::PLAY_BUTTON),
            ("PAUSE_OF", layout::PAUSE_BUTTON),
            ("FIND_OFF", layout::CENTER_BUTTON),
            ("LOCK_OF", layout::LOCK_BUTTON),
            ("PREV_OF", layout::PREV_BUTTON),
            ("UDONE_OF", layout::DONE_BUTTON),
            ("NEXT_OF", layout::NEXT_BUTTON),
            ("HELP_OF", layout::HELP_BUTTON),
            ("REPT_OFF", layout::REPORTS_BUTTON),
            ("CHAT_OFF", layout::CHAT_BUTTON),
        ] {
            if let Some(image) = simple(name) {
                aux.push((image, rect));
            }
        }

        Some(Self {
            files: pair("FILES_OF", "FILES_ON")?,
            preferences: pair("PREF_OFF", "PREF_ON")?,
            end_turn: simple("ENDTRN_U")?,
            end_turn_down: [
                simple("R_ENDT_D")?,
                simple("G_ENDT_D")?,
                simple("B_ENDT_D")?,
                simple("W_ENDT_D")?,
            ],
            turns: simple("TURNS")?,
            timer: simple("TIMER")?,
            coordinates: simple("XYPOS")?,
            unit_description: simple("UNITNAME")?,
            zoom_background: simple("ZOOMPNL1")?,
            zoom_pointer: simple("ZOOMPTR")?,
            toggles,
            minimap_2x: pair("MIN2X_OF", "MIN2X_ON")?,
            minimap_tnt: pair("MINFL_OF", "MINFL_ON")?,
            aux,
            // Optional (C++ loads them only with STATUS enabled).
            status_icons: StatusIcons {
                disabled: simple("IL_DSBLD"),
                speed: simple("IL_SPEED"),
                shots: simple("IL_SHOTS"),
            },
            stat_icons: StatIcons::load(max_res),
        })
    }

    /// Draws the panels over the frame (after `FRAMEPIC` and the map).
    ///
    /// Returns the number of filled pixels.
    pub fn draw<D: max_core::universe::Dispatcher>(
        &self,
        frame: &mut RgbaBuffer,
        app: &crate::App<D>,
    ) -> u32 {
        let Some(assets) = app.assets() else {
            return 0;
        };
        let state = app.state_ref();
        let hud = app.hud();
        let viewport = app.viewport();
        let panel = &hud.panels;
        let Some(palette) = UiPalette::from_assets(assets) else {
            return 0;
        };
        let palette = &palette;
        let mut painted = 0;

        // Top buttons.
        self.draw_pair(
            frame,
            palette,
            &self.files,
            layout::FILES_BUTTON,
            false,
            &mut painted,
        );
        self.draw_caption(frame, palette, PanelButton::Files);
        self.draw_pair(
            frame,
            palette,
            &self.preferences,
            layout::PREFS_BUTTON,
            false,
            &mut painted,
        );
        self.draw_caption(frame, palette, PanelButton::Preferences);

        // End Turn (extends onto the map — drawn over it).
        //
        // `GameManager_MenuClickEndTurnButton` (`game_manager.cpp:3977`):
        // `CopyDown(R_ENDT_D + ActiveTurnTeam)` + `SetRestState(!rest_state)`,
        // while `GameManager_UpdateGui` calls it with `rest_state = true` for
        // `TEAM_TYPE_PLAYER` (the button is released) and `rest_state = 0` for
        // `TEAM_TYPE_COMPUTER` (the button is pressed until the end of the opponents' turns).
        // The pressed button label — `_(aeb0)` "End Turn", baked into the resource
        // in the team color: `FontColor(team + 1, team + 1, 0x3F)`
        // (`game_manager.cpp:7860-7883`; for GRAY — `0xFF`).
        let active_team = state.active_turn_team();
        let active_kind = state
            .teams()
            .get(usize::from(active_team.0))
            .map(|team| team.team_type)
            .unwrap_or(TeamKind::None);
        let pressed = end_turn_pressed(active_kind);

        if pressed {
            let slot = usize::from(active_team.0).min(self.end_turn_down.len() - 1);
            let image = &self.end_turn_down[slot];

            painted += self.blit(
                frame,
                palette,
                image,
                layout::END_TURN_BUTTON.ulx,
                layout::END_TURN_BUTTON.uly,
                true,
            );
            let color = end_turn_caption_color(active_team);
            self.draw_caption_colored(
                frame,
                palette,
                PanelButton::EndTurn,
                color,
                color,
                TEAM_TEXT_SHADOW,
            );
        } else {
            painted += self.blit(
                frame,
                palette,
                &self.end_turn,
                layout::END_TURN_BUTTON.ulx,
                layout::END_TURN_BUTTON.uly,
                true,
            );
            self.draw_caption(frame, palette, PanelButton::EndTurn);
        }

        // Turn number and timer.
        painted += self.blit(
            frame,
            palette,
            &self.turns,
            layout::TURN_COUNTER.ulx,
            layout::TURN_COUNTER.uly,
            true,
        );
        draw_centered(
            frame,
            layout::TURN_COUNTER,
            GnwFontId::Font5.font(),
            &format!("{}", state.turn_counter().max(0)),
            palette.rgba(TEXT),
        );
        painted += self.blit(
            frame,
            palette,
            &self.timer,
            layout::TURN_TIMER.ulx,
            layout::TURN_TIMER.uly,
            true,
        );
        // `GameManager_DrawTurnTimer` (`game_manager.cpp:2193`):
        // `color = (turn_time <= 20) ? COLOR_RED : 0xA2;`
        // `sprintf(text, "%2.2i:%2.2i", turn_time / 60, turn_time % 60);`
        // `%2.2i` — precision 2 digits (leading zeros: `1 → «01»`, `0 → «00»`),
        // so `{:02}` in Rust gives exactly the same string. Red is enabled also
        // at `turn_time == 0` (the timer is off), as in C++.
        let timer_seconds = app.turn_timer_seconds();
        let timer_color = palette.rgba(timer_color_index(timer_seconds));
        draw_centered(
            frame,
            layout::TURN_TIMER,
            GnwFontId::Font5.font(),
            &timer_text(timer_seconds),
            timer_color,
        );

        // Other buttons (Play/Pause/Center/Prev/Done/Next/Help/Reports/Chat).
        for (image, rect) in &self.aux {
            painted += self.blit(frame, palette, image, rect.ulx, rect.uly, true);
        }
        for button in [
            PanelButton::Prev,
            PanelButton::Done,
            PanelButton::Next,
            PanelButton::Reports,
            PanelButton::Chat,
        ] {
            self.draw_caption(frame, palette, button);
        }

        // Toggle grid.
        for (index, button) in PanelButton::TOGGLES.into_iter().enumerate() {
            let pair = &self.toggles[index];
            let active = panel.get(button);
            self.draw_pair(frame, palette, pair, button.rect(), active, &mut painted);
            self.draw_caption(frame, palette, button);
        }

        // Minimap buttons. In the original their released state is already drawn in
        // `FRAMEPIC` (like the whole chrome), and the `MIN2X_OF`/`MINFL_OF` resource lies
        // a few pixels lower — drawing it over would produce a duplicate.
        // Therefore `*_OF` is drawn only without chrome, `*_ON` — always.
        self.draw_minimap_button(
            frame,
            palette,
            &self.minimap_2x,
            layout::MINIMAP_2X,
            panel.minimap_2x,
            assets.chrome.is_none(),
            &mut painted,
        );
        self.draw_minimap_button(
            frame,
            palette,
            &self.minimap_tnt,
            layout::MINIMAP_TNT,
            panel.minimap_tnt,
            assets.chrome.is_none(),
            &mut painted,
        );

        // Zoom: background and pointer (the offset — `GameManager_UpdateZoomSlider`).
        painted += self.blit(
            frame,
            palette,
            &self.zoom_background,
            layout::ZOOM_SLIDER.ulx,
            layout::ZOOM_SLIDER.uly,
            false,
        );
        let offset = zoom_slider_offset(viewport.zoom());
        painted += self.blit_clipped(
            frame,
            palette,
            &self.zoom_pointer,
            layout::ZOOM_SLIDER.ulx + offset,
            layout::ZOOM_SLIDER.uly + 1,
            true,
            layout::ZOOM_SLIDER,
        );

        // Minimap from the `minimap` layer of `.WRL`.
        painted += draw_minimap(frame, app, assets, palette);

        // Portrait and statistics.
        painted += self.draw_portrait(frame, app, assets, palette);
        painted += draw_stats(frame, state, app.selected_unit(), palette, &self.stat_icons);

        // Bottom row: coordinates and the unit description.
        painted += self.blit(
            frame,
            palette,
            &self.coordinates,
            layout::COORDINATES.ulx,
            layout::COORDINATES.uly,
            true,
        );
        let grid = panel
            .cursor_grid
            .or_else(|| app.selected_unit().map(|unit| (unit.grid.x, unit.grid.y)))
            .unwrap_or_else(|| {
                let map = map_viewport(viewport);
                let (width, height) = map.screen();
                map.screen_to_grid(width as i32 / 2, height as i32 / 2)
            });
        let coordinate_rect = PixelRect::new(
            layout::COORDINATES.ulx + 21,
            layout::COORDINATES.uly,
            layout::COORDINATES.width - 21,
            layout::COORDINATES.height,
        );
        draw_centered(
            frame,
            coordinate_rect,
            GnwFontId::Font5.font(),
            &format!("{:03}-{:03}", grid.0.max(0), grid.1.max(0)),
            palette.rgba(TEXT),
        );

        painted += self.blit(
            frame,
            palette,
            &self.unit_description,
            layout::UNIT_DESCRIPTION.ulx,
            layout::UNIT_DESCRIPTION.uly,
            true,
        );
        let described = app.selected_unit().map(unit_name).or_else(|| {
            panel.cursor_grid.and_then(|(x, y)| {
                state
                    .units_at(max_core::geo::Point::new(x, y))
                    .into_iter()
                    .find_map(|id| state.unit(*id))
                    .map(unit_name)
            })
        });
        if let Some(name) = described {
            draw_centered(
                frame,
                layout::UNIT_DESCRIPTION,
                GnwFontId::Font5.font(),
                &name,
                palette.rgba(TEXT),
            );
        }

        painted
    }

    fn draw_portrait<D: max_core::universe::Dispatcher>(
        &self,
        frame: &mut RgbaBuffer,
        app: &crate::App<D>,
        assets: &Assets,
        palette: &UiPalette,
    ) -> u32 {
        let mut painted = 0u32;
        let area = layout::CORNER_FLIC;

        // Black background of the portrait window (like `FillOrRestoreWindow(CORNER_FLIC, BLACK)`).
        frame.fill_rect(area, Rgba::BLACK);
        painted += (area.width * area.height) as u32;

        let Some(unit) = app.selected_unit() else {
            return painted;
        };

        // The main path — FLC animation (`GameManager_MenuCreateFlic` +
        // `AdvanceFlic`): the frame 1:1 into the 128×128 window, indices — by the window
        // palette (`FRAMEPIC`), like C++ (`load_flic_palette = 0`).
        let animation = app
            .portrait()
            .filter(|portrait| portrait.unit() == unit.id && !portrait.frames().is_empty());

        if let Some(animation) = animation {
            let frame_pixels = animation.frame();
            let width = frame_pixels.width.min(area.width as usize);
            let height = frame_pixels.height.min(area.height as usize);

            for y in 0..height {
                for x in 0..width {
                    let index = frame_pixels.pixels[y * frame_pixels.width + x];

                    frame.set(
                        area.ulx + x as i32,
                        area.uly + y as i32,
                        palette.rgba(index),
                    );
                }
            }

            painted += (width * height) as u32;
        } else {
            // Fallback: no FLC (22 types) or the file did not load — sprite ×2.
            let mut portrait =
                RgbaBuffer::filled(area.width as u32, area.height as u32, Rgba::BLACK);
            let anchor = (area.width / 2, area.height / 2);

            let image_base = crate::render::image_base_for(
                unit,
                app.state_ref()
                    .teams()
                    .get(unit.team.0 as usize)
                    .map(|team| team.team_clan),
            );

            // Presentation fields (shadow/brightness) — the `GameState` table
            // (phase 5.3/B), not `UnitInfo`.
            let presentation = app
                .state_ref()
                .presentation(unit.id)
                .cloned()
                .unwrap_or_default();

            if crate::render::draw_unit_sprites_with_base(
                assets,
                unit,
                &presentation,
                image_base,
                &mut portrait,
                anchor,
                64,
            )
            .is_none()
            {
                return painted;
            }

            let center_x = area.width / 2;
            let center_y = area.height / 2;

            for y in 0..area.height {
                for x in 0..area.width {
                    let source_x = center_x + (x - center_x) / 2;
                    let source_y = center_y + (y - center_y) / 2;

                    if let Some(color) = portrait.get(source_x, source_y) {
                        frame.set(area.ulx + x, area.uly + y, color);
                    }
                }
            }
        }

        // Name and status in the upper part of the portrait (`GameManager_MenuUnitSelect`:
        // `win_print` with the name in `COLOR_GREEN`, then `GameManager_UpdateInfoDisplay`
        // — the status `0xA2` one line lower). The original draws them over the FLC frame and
        // during animation repaints only the lines below `text_height` — here
        // the panel is drawn entirely, so the text is simply applied last.
        let font = GnwFontId::Font2.font();
        draw_text_gnw(
            frame,
            area.ulx,
            area.uly,
            font,
            &unit_name(unit),
            palette.rgba(GREEN),
        );
        draw_text_gnw(
            frame,
            area.ulx,
            area.uly + font.height(),
            font,
            if unit.hits > 0 {
                "Awaiting"
            } else {
                "Destroyed"
            },
            palette.rgba(TEXT),
        );

        painted
    }

    fn draw_pair(
        &self,
        frame: &mut RgbaBuffer,
        palette: &UiPalette,
        pair: &ButtonPair,
        rect: PixelRect,
        active: bool,
        painted: &mut u32,
    ) {
        let image = if active { &pair.down } else { &pair.up };

        *painted += self.blit(frame, palette, image, rect.ulx, rect.uly, true);
    }

    /// Minimap button: `*_ON` always, `*_OF` — only if there is no chrome
    /// (the released state is already drawn in `FRAMEPIC`).
    #[allow(clippy::too_many_arguments)]
    fn draw_minimap_button(
        &self,
        frame: &mut RgbaBuffer,
        palette: &UiPalette,
        pair: &ButtonPair,
        rect: PixelRect,
        active: bool,
        draw_up: bool,
        painted: &mut u32,
    ) {
        if !active && !draw_up {
            return;
        }

        let image = if active { &pair.down } else { &pair.up };

        *painted += self.blit(frame, palette, image, rect.ulx, rect.uly, true);
    }

    fn draw_caption(&self, frame: &mut RgbaBuffer, palette: &UiPalette, button: PanelButton) {
        let (base, outline, shadow) = if button == PanelButton::EndTurn {
            (END_TURN_TEXT, END_TURN_OUTLINE, SHADOW)
        } else {
            (TEXT, CAPTION_OUTLINE, SHADOW)
        };

        self.draw_caption_colored(frame, palette, button, base, outline, shadow);
    }

    /// Button label with the given palette indices — `Button::SetCaption`
    /// (utton.cpp:412-449) and the End Turn label baked into the resource
    /// (`Text_TextBox(..., 0, 0, w, h, true, true, FontColor(...))`).
    ///
    /// C++ 1:1 geometry: `SetCaption(caption, x, y, …)` shrinks the
    /// centering area to `width - x` / `height - y`, then `Text_TextLine`
    /// (`text.cpp:772`) centers the line in this area:
    /// `x = (swidth - (Text_GetWidth + 1)) / 2` (a shift only if > 0),
    /// `y = ((height - y) - Text_GetHeight()) / 2 + y`. Earlier the port centered
    /// in the full button rectangle — the 3×3 labels drifted 1-2 px down and
    /// 1 px right.
    fn draw_caption_colored(
        &self,
        frame: &mut RgbaBuffer,
        palette: &UiPalette,
        button: PanelButton,
        base: u8,
        outline: u8,
        shadow: u8,
    ) {
        let Some(caption) = button.caption() else {
            return;
        };
        let font = button.caption_font().font();
        let (x, y) = button.caption_position();

        draw_text_gnw_shadowed(
            frame,
            x,
            y,
            font,
            caption,
            palette.rgba(base),
            palette.rgba(outline),
            palette.rgba(shadow),
        );
    }

    /// Copies a `SimpleImage` with transparency (the `image.transparent` index).
    fn blit(
        &self,
        frame: &mut RgbaBuffer,
        palette: &UiPalette,
        image: &SimpleImage,
        ulx: i32,
        uly: i32,
        transparent: bool,
    ) -> u32 {
        self.blit_clipped(frame, palette, image, ulx, uly, transparent, frame.bounds())
    }

    /// The same, but clipped by `clip` (the panel window).
    #[allow(clippy::too_many_arguments)]
    fn blit_clipped(
        &self,
        frame: &mut RgbaBuffer,
        palette: &UiPalette,
        image: &SimpleImage,
        ulx: i32,
        uly: i32,
        transparent: bool,
        clip: PixelRect,
    ) -> u32 {
        let mut painted = 0;

        for y in 0..image.height as u32 {
            for x in 0..image.width as u32 {
                let Some(index) = image.pixel(x, y) else {
                    continue;
                };

                if transparent && index == image.transparent {
                    continue;
                }

                let px = ulx + x as i32;
                let py = uly + y as i32;

                if !clip.contains(px, py) {
                    continue;
                }

                if frame.set(px, py, palette.rgba(index)) {
                    painted += 1;
                }
            }
        }

        painted
    }
}

/// Zoom pointer offset.
///
/// C++ formula (`GameManager_UpdateZoomSlider`): `((130-28) * (64-zoom)) / 60 +
/// 14` for the 4…64 range. Our `Viewport` keeps 8…64, so the denominator is
/// `56`; at `zoom == 64` the pointer stands at 14, at `zoom == 8` — at 116.
pub fn zoom_slider_offset(zoom: u32) -> i32 {
    let zoom = zoom.clamp(8, 64);
    ((102 * (64 - zoom) as i32) / 56) + 14
}

/// The inverse conversion: slider x → scale (`8..=64`).
pub fn zoom_from_slider_x(x: i32) -> u32 {
    let relative = (x - layout::ZOOM_SLIDER.ulx - 14).clamp(0, 102);
    let zoom = 64 - (relative * 56 / 102);

    zoom.clamp(8, 64) as u32
}

/// Panel button under the window point (`None` — the point is not on a button).
pub fn panel_button_at(x: i32, y: i32) -> Option<PanelButton> {
    PanelButton::ALL
        .into_iter()
        .find(|button| button.rect().contains(x, y))
}

/// Window point → local map coordinates (`None` — not in [`layout::MAIN_MAP`]).
pub fn screen_to_map(x: i32, y: i32) -> Option<(i32, i32)> {
    if !layout::MAIN_MAP.contains(x, y) {
        return None;
    }

    Some((x - layout::MAIN_MAP.ulx, y - layout::MAIN_MAP.uly))
}

/// 448×448 map window camera: the same center/zoom as the application's.
pub fn map_viewport(viewport: &Viewport) -> Viewport {
    Viewport::with_center(
        layout::MAIN_MAP.width as u32,
        layout::MAIN_MAP.height as u32,
        viewport.center(),
        viewport.zoom(),
    )
}

/// Unit name for the panels (`UnitInfo::GetDisplayName`): the name or the User-Friendly Name.
///
/// If the unit has no own name (`unit.name`), the panel shows the type
/// UFN — [`crate::unit_names::unit_user_friendly_name`], not the variant's `Debug`
/// name (`MiningStation` → «Mining Station»).
pub fn unit_name(unit: &UnitInfo) -> String {
    if unit.name.is_empty() {
        crate::unit_names::unit_user_friendly_name(unit.unit_type).to_string()
    } else {
        unit.name.clone()
    }
}

/// Statistics row label — `_(id)` from `game_manager.cpp` (`language.json`).
///
/// C++ identifiers → en-US strings: `c02c` Hits, `5027` Ammo, `1274` Cargo,
/// `c372` Speed, `2bce` Shots, `8d59`/`3aea`/`81c5`/`160c` Usage, `848e` Power,
/// `1e1a` Teams, `d28b`/`c9b3`/`d4ca`/`7be8`/`5f25` Total, `c54a` points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatLabel {
    /// Row 1 — `_(c02c)`.
    Hits,
    /// Row 2 for armed units — `_(5027)`.
    Ammo,
    /// Row 2 for storages/transport — `_(1274)`; raw-material storage,
    /// icon `SI_RAW`.
    Cargo,
    /// Row 2 for a fuel storage — `_(1274)`, icon `SI_FUEL`.
    CargoFuel,
    /// Row 2 for a gold storage — `_(1274)`, icon `SI_GOLD`.
    CargoGold,
    /// Row 3 for mobile units — `_(c372)`.
    Speed,
    /// Row 4 — `_(2bce)`.
    Shots,
    /// `DrawInfoDisplayType1/2` — `_(8d59)`/`_(3aea)`/`_(81c5)`/`_(160c)`.
    Usage,
    /// `DrawInfoDisplayType2` for a generator — `_(848e)`.
    Power,
    /// `DrawInfoDisplayType1` for a worker generator — `_(1e1a)`.
    Teams,
    /// `DrawInfoDisplayType1/2/3` and the complex stock — `_(d28b)`/`_(c9b3)`/
    /// `_(d4ca)`/`_(7be8)`/`_(5f25)`.
    Total,
    /// `DrawInfoDisplayType3` (greenhouse) — `_(c54a)`.
    Points,
}

impl StatLabel {
    /// Row label (the `en-US` value from `language.json` — as in the game).
    pub const fn caption(self) -> &'static str {
        match self {
            Self::Hits => "Hits",
            Self::Ammo => "Ammo",
            Self::Cargo | Self::CargoFuel | Self::CargoGold => "Cargo",
            Self::Speed => "Speed",
            Self::Shots => "Shots",
            Self::Usage => "Usage",
            Self::Power => "Power",
            Self::Teams => "Teams",
            Self::Total => "Total",
            Self::Points => "points",
        }
    }
}

/// Icon family of a statistics row — the `SI_*`/`EI_*` pair repeated after the label
/// (`GameManager_DrawInfoDisplayRowIcons`, `game_manager.cpp:7290`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatIcon {
    /// `SI_HITSB`; red/yellow variants are chosen by the row colour.
    Hits,
    /// `SI_AMMO`.
    Ammo,
    /// `SI_RAW`.
    Raw,
    /// `SI_FUEL`.
    Fuel,
    /// `SI_GOLD`.
    Gold,
    /// `SI_SPEED`.
    Speed,
    /// `SI_SHOTS`.
    Shots,
    /// `SI_POWER` (`DrawInfoDisplayType2`).
    Power,
    /// `SI_WORK` (`DrawInfoDisplayType1/3`).
    Work,
}

impl StatIcon {
    /// `(full, empty)` resource names — C++ `icon` / `icon + 1`
    /// (`resourcetable.hpp:658-687`).
    const fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Hits => ("SI_HITSB", "EI_HITSB"),
            Self::Ammo => ("SI_AMMO", "EI_AMMO"),
            Self::Raw => ("SI_RAW", "EI_RAW"),
            Self::Fuel => ("SI_FUEL", "EI_FUEL"),
            Self::Gold => ("SI_GOLD", "EI_GOLD"),
            Self::Speed => ("SI_SPEED", "EI_SPEED"),
            Self::Shots => ("SI_SHOTS", "EI_SHOTS"),
            Self::Power => ("SI_POWER", "EI_POWER"),
            Self::Work => ("SI_WORK", "EI_WORK"),
        }
    }
}

/// One drawn statistics row (`GameManager_DrawInfoDisplayRow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatRow {
    /// Label (the left box 45…75 px).
    pub label: StatLabel,
    /// Pips after the label (75 px onwards) — `icon` of the C++ call.
    pub icon: StatIcon,
    /// Current value (`current_value` after `std::min(current, base)`).
    pub current: i32,
    /// Base (`base_value`); the row is drawn only if `base != 0`.
    pub base: i32,
    /// Step of the C++ pips icons (`factor`); `0` — C++ does not draw the row at all.
    pub factor: i32,
}

impl StatRow {
    /// Value text: `sprintf(text, "%i/%i", current_value, base_value)`.
    pub fn text(&self) -> String {
        format!("{}/{}", self.current, self.base)
    }

    /// Value color — `GameManager_DrawInfoDisplayRow`:
    /// red `≤ base/4`, yellow `≤ base/2`, otherwise green.
    pub fn color(&self) -> u8 {
        if self.base <= 0 {
            return TEXT;
        }
        if self.current <= self.base / 4 {
            RED
        } else if self.current <= self.base / 2 {
            YELLOW
        } else {
            GREEN
        }
    }
}

/// The "Enemy %s in radar range." notice — `GameManager_NotifyEvent(unit, 0)`
/// (`game_manager.cpp:3458-3462`), called from `UnitInfo::SpotByTeam` →
/// `RadarPing` (`unitinfo.cpp:2810-2839/3711-3717`) when the **player's** team
/// spots an enemy. `%s` is `GetSingularName()` → [`crate::buildmenu::unit_name`].
///
/// Shown as `MESSAGE_BOX_NOTICE` (`MessageManager_DrawMessage`) and repeated in
/// the log (the turn report of `game_manager.cpp:8180` also goes there).
pub fn enemy_spotted_message(unit_type: UnitType) -> String {
    format!(
        "Enemy {} in radar range.",
        crate::buildmenu::unit_name(unit_type)
    )
}

/// The "Finished building …" line — `language.json` `1200`/`276c` (three variants
/// by the building gender; in the original all three coincide in text).
const FINISHED_CONSTRUCTION: &str =
    "Finished building {building}.  To move the {builder} out of the construction site, click on a square with a symbol in it.";
/// `FinishedBuilding` (`1200`/`74d3`/`8c62`) — factory: the unit leaves the factory.
const FINISHED_FACTORY: &str =
    "Finished building {building}.  To move the {building} out of the factory, click on a square with a symbol in it.";

/// Selected unit hint text — `GameManager_DrawUnitStatusMessage` +
/// `GameManager_DrawBuilderUnitStatusMessage` (`game_manager.cpp:7893/8038`).
///
/// Returns `None` if there is no hint for the unit (C++ draws the message
/// only for `ORDER_BUILD`/`CLEAR`/`HALT_BUILDING`/`HALT_BUILDING_2`).
pub fn builder_status_message(state: StateRef<'_>, unit: &UnitInfo) -> Option<String> {
    if !matches!(
        unit.orders,
        UnitOrder::Build | UnitOrder::Clear | UnitOrder::HaltBuilding | UnitOrder::HaltBuilding2
    ) {
        return None;
    }

    if unit.unit_type == UnitType::Bulldozer {
        return Some(format!(
            "Number of turns to clear site: {}.",
            unit.build_time
        ));
    }

    if unit.state == UnitOrderState::UnitReady {
        let building = unit
            .parent_unit
            .and_then(|id| state.unit(id))
            .map(|building| crate::buildmenu::unit_name(building.unit_type))
            .unwrap_or_default();
        let builder = crate::buildmenu::unit_name(unit.unit_type);

        return Some(if !unit.is_mobile() {
            FINISHED_FACTORY
                .replace("{building}", &building)
                .replace("{builder}", &builder)
        } else {
            FINISHED_CONSTRUCTION
                .replace("{building}", &building)
                .replace("{builder}", &builder)
        });
    }

    let &unit_type = unit.build_list.first()?;
    let name = crate::buildmenu::unit_name(unit_type);
    let build_rate = i32::from(unit.build_rate.max(1));

    if matches!(
        unit.orders,
        UnitOrder::HaltBuilding | UnitOrder::HaltBuilding2
    ) {
        return Some(format!(
            "Was building {name}, with {} turns to completion.",
            unit.build_time
        ));
    }

    let (turns, _) = state.turns_and_consumption(unit, unit_type, build_rate);

    Some(format!(
        "Currently building: {name}.\nTurns to completion: {turns}."
    ))
}

/// `MessageBoxStyle` (`message_manager.hpp:37`, `MESSAGE_BOX_STYLE_COUNT`):
/// which background colour table the message box uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageBoxStyle {
    /// `MESSAGE_BOX_INFO` — `ResourceManager_DarkeningColorIndexTable`.
    #[default]
    Info,
    /// `MESSAGE_BOX_NOTICE` — `Color_GenerateIntensityTable2`
    /// (`message_manager.cpp:92`): neutral grey, red on the snow tilesets.
    Notice,
    /// `MESSAGE_BOX_WARNING` — `Color_GenerateIntensityTable3`
    /// (`message_manager.cpp:88`): red, the tileset only changes the factor.
    Warning,
}

/// `MessageManager_DrawMessage` + `MessageManager_DrawMessageBox`
/// (`message_manager.cpp:144/206`).
///
/// Background — the style table (`MessageManager_MessageBox_BgColorArray`,
/// `message_manager.cpp:50-54`): `INFO` darkens, `NOTICE`/`WARNING` tint the pixels
/// under the box (the box is transparent over the map, not an opaque fill). Text —
/// `Font5` in `0xFF` with `GNW_TEXT_OUTLINE` (`MessageManager_DrawMessageBoxText`,
/// `message_manager.cpp:98-122`): a black copy shifted by one pixel and the white
/// fill on top. Rectangle: `(area.ulx, area.uly + 10)`, width = the longest line + 20,
/// height = `20 + 10 * lines`.
///
/// `snow` — the tileset is one of `SNOW_1..SNOW_6` (`World::
/// GetMessageBoxNoticeTintParameters`/`GetMessageBoxWarningTintParameters`,
/// `world.cpp:1003-1026`).
pub fn draw_status_message(
    frame: &mut RgbaBuffer,
    area: PixelRect,
    palette: &UiPalette,
    text: &str,
    style: MessageBoxStyle,
    snow: bool,
) -> u32 {
    if text.is_empty() || area.width <= 0 {
        return 0;
    }

    let font = GnwFontId::Font5.font();
    // `MessageManager_WrapText(text, width - 20)`: lines wrap by words.
    let max_line = (area.width - 20).max(20);
    let mut lines: Vec<String> = Vec::new();

    for line in text.split('\n') {
        let mut current = String::new();

        for word in line.split(' ') {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };

            if font.text_width(&candidate) > max_line && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current.push_str(word);
            } else {
                current = candidate;
            }
        }

        lines.push(current);
    }

    let width = (lines
        .iter()
        .map(|line| font.text_width(line))
        .max()
        .unwrap_or(0)
        + 20)
        .min(area.width);
    let height = 20 + 10 * lines.len() as i32;
    let rect = PixelRect::new(area.ulx, area.uly + 10, width, height);
    let mut painted = 0;

    let mut cache: std::collections::HashMap<[u8; 3], Rgba> = std::collections::HashMap::new();

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            let Some(color) = frame.get(x, y) else {
                continue;
            };
            let key = [color.r, color.g, color.b];
            let mapped = match cache.get(&key) {
                Some(mapped) => *mapped,
                None => {
                    let mapped = palette.message_box_color(style, snow, color);
                    cache.insert(key, mapped);
                    mapped
                }
            };

            if frame.set(x, y, mapped) {
                painted += 1;
            }
        }
    }

    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }

        // `MessageManager_DrawMessageBoxText`: `Text_Blit` with `GNW_TEXT_OUTLINE`
        // (`color = 0xFF`): the black copy at (+1, +1) first, the white fill on top.
        let drawn = draw_text_gnw_outlined(
            frame,
            rect.ulx + 10,
            rect.uly + 10 + 10 * index as i32,
            font,
            line,
            palette.rgba(0xFF),
            palette.rgba(0),
        );

        painted += drawn.max(0) as u32;
    }
    painted
}

/// Icon family of a label when it is unambiguous (`Hits`/`Ammo`/`Cargo`/`Speed`/
/// `Shots`); `Usage`/`Total`/`Teams`/`Power` get the family of the row from the
/// display type ([`make_row_icon`]).
const fn default_icon(label: StatLabel) -> StatIcon {
    match label {
        StatLabel::Hits => StatIcon::Hits,
        StatLabel::Ammo => StatIcon::Ammo,
        StatLabel::Cargo => StatIcon::Raw,
        StatLabel::CargoFuel => StatIcon::Fuel,
        StatLabel::CargoGold => StatIcon::Gold,
        StatLabel::Speed => StatIcon::Speed,
        StatLabel::Shots => StatIcon::Shots,
        StatLabel::Usage
        | StatLabel::Power
        | StatLabel::Teams
        | StatLabel::Total
        | StatLabel::Points => StatIcon::Work,
    }
}

/// C++ line: `if (base_value && factor)` — otherwise `DrawInfoDisplayRow`
/// draws nothing, and the row remains empty (`FRAMEPIC` background).
fn make_row(label: StatLabel, current: i32, base: i32, factor: i32) -> Option<StatRow> {
    make_row_icon(default_icon(label), label, current, base, factor)
}

/// [`make_row`] with an explicit icon family (`DrawInfoDisplayType1/2` share the
/// `Usage`/`Total` labels but use `SI_WORK` and `SI_POWER` respectively).
fn make_row_icon(
    icon: StatIcon,
    label: StatLabel,
    current: i32,
    base: i32,
    factor: i32,
) -> Option<StatRow> {
    if base == 0 || factor == 0 {
        return None;
    }

    Some(StatRow {
        label,
        icon,
        current: current.min(base),
        base,
        factor,
    })
}

/// Overlay of a line over the already drawn one (C++ draws over the buffer
/// `WINDOW_STAT_ROW_N`; a skipped call leaves the previous content).
fn assign_row(rows: &mut [Option<StatRow>; 4], index: usize, row: Option<StatRow>) {
    if let Some(row) = row {
        rows[index] = Some(row);
    }
}

/// `Cargo_GetNetProduction(unit)` in the `DrawInfoDisplayType1/2` loop: for a unit in
/// `ORDER_UPGRADE` C++ computes production by the **previous** order
/// (temporarily `SetOrder(prior)`), so we clone with `prior_orders`.
fn net_production_of(state: StateRef<'_>, unit: &UnitInfo) -> NetProduction {
    if unit.orders == UnitOrder::Upgrade && unit.state == UnitOrderState::ExecutingOrder {
        let mut simulated = unit.clone();

        simulated.orders = unit.prior_orders;

        state.cargo_get_net_production(&simulated, false)
    } else {
        state.cargo_get_net_production(unit, false)
    }
}

/// `GameManager_DrawInfoDisplayType2` — power: row 2 `Usage`/`Power`,
/// rows 3/4 — `Total`/`Usage` by the complex (own units only).
fn power_rows(
    state: StateRef<'_>,
    unit: &UnitInfo,
    player_team: TeamId,
    rows: &mut [Option<StatRow>; 4],
) {
    let power_need = cargo_power_consumption_rate(unit.unit_type);

    if power_need == 0
        || unit.team != player_team
        || cargo_life_consumption_rate(unit.unit_type) != 0
    {
        return;
    }

    let cargo = state.cargo_get_net_production(unit, false);
    let current_power_need = cargo.power;

    if power_need >= 0 {
        // `_(8d59)` «Usage»: how much power the unit consumes.
        assign_row(
            rows,
            1,
            make_row_icon(
                StatIcon::Power,
                StatLabel::Usage,
                -current_power_need,
                power_need,
                1,
            ),
        );
    } else {
        // `_(848e)` «Power»: how much power the unit produces.
        assign_row(
            rows,
            1,
            make_row_icon(
                StatIcon::Power,
                StatLabel::Power,
                current_power_need,
                -power_need,
                1,
            ),
        );
    }

    let mut power_need_total = 0;
    let mut current_power_need = 0;
    let mut current_value = 0;
    let mut base_value = 0;

    for &id in state.unit_list(UnitListKind::Stationary) {
        let Some(other) = state.unit(id) else {
            continue;
        };

        if other.complex != unit.complex || other.team != unit.team {
            continue;
        }

        let rate = cargo_power_consumption_rate(other.unit_type);

        if rate >= 0 {
            base_value += rate;
        } else {
            power_need_total -= rate;
        }

        let cargo = net_production_of(state, other);

        if cargo.power >= 0 {
            current_power_need += cargo.power;
        } else {
            current_value -= cargo.power;
        }
    }

    if cargo_power_consumption_rate(unit.unit_type) > 0 {
        assign_row(
            rows,
            2,
            make_row_icon(
                StatIcon::Power,
                StatLabel::Total,
                current_value,
                base_value,
                1,
            ),
        );
    } else {
        assign_row(
            rows,
            2,
            make_row_icon(
                StatIcon::Power,
                StatLabel::Total,
                current_power_need,
                power_need_total,
                1,
            ),
        );
        assign_row(
            rows,
            3,
            make_row_icon(
                StatIcon::Power,
                StatLabel::Usage,
                current_value,
                base_value,
                1,
            ),
        );
    }
}

/// `GameManager_DrawInfoDisplayType1` — workers: row 2 `Usage`/`Teams`,
/// rows 3/4 — `Total`/`Usage` by the complex (own units only).
fn life_rows(
    state: StateRef<'_>,
    unit: &UnitInfo,
    player_team: TeamId,
    rows: &mut [Option<StatRow>; 4],
) {
    let life_need = cargo_life_consumption_rate(unit.unit_type);

    if life_need == 0 || unit.team != player_team {
        return;
    }

    let cargo = state.cargo_get_net_production(unit, false);
    let current_life_need = cargo.life;

    if life_need >= 0 {
        assign_row(
            rows,
            1,
            make_row(StatLabel::Usage, -current_life_need, life_need, 1),
        );
    } else {
        assign_row(
            rows,
            1,
            make_row(StatLabel::Teams, current_life_need, -life_need, 1),
        );
    }

    let mut life_need_total = 0;
    let mut current_life_need = 0;
    let mut current_value = 0;
    let mut base_value = 0;

    for &id in state.unit_list(UnitListKind::Stationary) {
        let Some(other) = state.unit(id) else {
            continue;
        };

        if other.complex != unit.complex || other.team != unit.team {
            continue;
        }

        let rate = cargo_life_consumption_rate(other.unit_type);

        if rate >= 0 {
            base_value += rate;
        } else {
            life_need_total -= rate;
        }

        let cargo = net_production_of(state, other);

        if cargo.life >= 0 {
            current_life_need += cargo.life;
        } else {
            current_value -= cargo.life;
        }
    }

    if cargo_life_consumption_rate(unit.unit_type) > 0 {
        assign_row(
            rows,
            2,
            make_row(StatLabel::Total, current_value, base_value, 1),
        );
    } else {
        assign_row(
            rows,
            2,
            make_row(StatLabel::Total, current_life_need, life_need_total, 1),
        );
        assign_row(
            rows,
            3,
            make_row(StatLabel::Usage, current_value, base_value, 1),
        );
    }
}

/// `GameManager_DrawInfoDisplayType3` — the player's greenhouse: match points.
fn greenhouse_rows(state: StateRef<'_>, unit: &UnitInfo, rows: &mut [Option<StatRow>; 4]) {
    let mut current_value = if state.victory_type() == VictoryType::Score {
        state.victory_limit()
    } else {
        let mut value = 0;

        for team in 0..PLAYER_TEAM_COUNT {
            let info = &state.teams()[team];

            if info.team_type != TeamKind::None && info.team_type != TeamKind::Eliminated {
                value = value.max(state.teams()[unit.team.0 as usize].team_points as i32);
            }
        }

        value
    };

    if current_value < 1 {
        current_value = 10;
    }

    let value_limit = (current_value + 14) / 15;
    let storage = i32::from(unit.storage);

    assign_row(
        rows,
        1,
        make_row(StatLabel::Points, storage, storage, value_limit),
    );
    assign_row(
        rows,
        2,
        make_row(
            StatLabel::Total,
            state.teams()[unit.team.0 as usize].team_points as i32,
            current_value,
            value_limit,
        ),
    );
}

/// Port enhancement (not in the oracle): the `USAGE` row of a unit-producing factory
/// shows the raw material it **actually** spends per turn.
///
/// The original fills the `USAGE` row with the worker (`DrawInfoDisplayType1`) or
/// power (`DrawInfoDisplayType2`) need of the factory. This enhancement replaces
/// that row for the five factories with the real raw charge: the same
/// `Cargo_GetRawConsumptionRate(type, rate)` that the production manager deducts
/// from the complex every turn (`charge_units`, `cargo.cpp:140`) at the effective
/// rate `min(build_rate, BuildMenu_GetMaxPossibleBuildRate)`.
///
/// A factory that is **off** — not in `ORDER_BUILD`, waiting for the finished unit
/// to leave, or paused by the power optimizer — spends no raw, so the `USAGE` row
/// is not drawn at all (`None`). The condition is exactly the
/// `Cargo_GetNetProduction` charging condition (`cargo.cpp:88-95`): `ORDER_BUILD`,
/// not `UNIT_READY`, not `build_paused`.
///
/// Presentation only: the row is computed from the state and never written back.
fn factory_raw_usage_row(unit: &UnitInfo) -> Option<StatRow> {
    if !max_core::production::is_factory(unit.unit_type) {
        return None;
    }

    // `Cargo_GetNetProduction`/`ProductionManager::charge_units`: raw is charged
    // only while the construction runs — `ORDER_BUILD`, not `UNIT_READY`, not
    // paused. The predicate is shared with the simulation, so "off" means the same
    // in the panel and in the economy.
    if !max_core::production::is_construction_running(unit) {
        return None;
    }

    // The rate is capped by the remaining turns like `charge_units`
    // (`max_possible_build_rate` with the unlimited storage of a building).
    let max_rate = max_core::production::max_possible_build_rate(
        unit.unit_type,
        i32::from(unit.build_time),
        i32::from(i16::MAX),
    );
    let rate = i32::from(unit.build_rate.max(1)).min(max_rate.max(1)).max(1);
    let cost = cargo_raw_consumption_rate(unit.unit_type, rate);

    if cost <= 0 {
        return None;
    }

    // The panel always prints `current/base`; the actual per-turn charge is both.
    Some(StatRow {
        label: StatLabel::Usage,
        icon: StatIcon::Raw,
        current: cost,
        base: cost,
        factor: 1,
    })
}

/// Complex stock — `Complex::GetCargoInfo` + `Cargo_GetInventory`/
/// `Cargo_GetCargoCapacity`: the sum of `storage` and `ATTRIB_STORAGE` of stationary
/// units of the same complex with the same cargo type (the `_(d28b)` «Total» row).
///
/// The team check is mandatory: C++ compares `Complex*` pointers, while the port's
/// [`max_core::team::ComplexId`] is unique only **within a team**. Without it the
/// `Total` row of a mine also summed the capacity of every enemy complex with the
/// same numeric id — the starting bases of both teams own complex 2, so a mine with
/// Cargo `12/25` showed `Total 12/50` (manual defect "mine Cargo 25, complex Total 50").
fn complex_cargo(state: StateRef<'_>, unit: &UnitInfo, cargo_type: CargoType) -> (i32, i32) {
    let mut value = 0;
    let mut capacity = 0;

    for &id in state.unit_list(UnitListKind::Stationary) {
        let Some(other) = state.unit(id) else {
            continue;
        };

        if other.complex != unit.complex
            || other.team != unit.team
            || cargo_type_of(other.unit_type) != cargo_type
        {
            continue;
        }

        value += i32::from(other.storage);
        capacity += other.base_values.get_attribute(ATTRIB_STORAGE);
    }

    (value, capacity)
}

/// Four statistics lines of the selected unit — a port of `GameManager_UpdateInfoDisplay`
/// (game_manager.cpp:7667), the call order preserved:
///
/// 1. row 1 — `Hits` (`unit->hits` / `ATTRIB_HITS`, factor 4);
/// 2. row 2 — `Ammo` if `ATTRIB_ATTACK != 0` (the base `ATTRIB_AMMO` only
///    for own team), otherwise `Cargo` (`unit->storage` / `ATTRIB_STORAGE` scaled
///    by the cargo type; for a foreign unit — factor 0, the row is not drawn);
/// 3. row 3 — `Speed` (`unit->speed` / `ATTRIB_SPEED`);
/// 4. row 4 — `Shots` (`unit->shots` / `min(ROUNDS, AMMO)`).
///
/// Then over them (like C++, only for `player_team`):
/// [`power_rows`] if `ATTRIB_STORAGE == 0`, [`greenhouse_rows`] for `GREENHSE`
/// or [`life_rows`], and the complex stock row [`complex_cargo`] for own
/// stationary storages (`RAW`/`FUEL`/`GOLD`).
///
/// Port enhancement: for an own unit-producing factory the `USAGE` row is replaced
/// by [`factory_raw_usage_row`] — the actual raw per turn while the factory is on,
/// no `USAGE` row while it is off.
///
/// A row with a zero base or zero factor remains empty — exactly like
/// `GameManager_DrawInfoDisplayRow` (`if (base_value && factor)`).
pub fn rows_for(state: StateRef<'_>, unit: &UnitInfo, player_team: TeamId) -> [Option<StatRow>; 4] {
    let values = &unit.base_values;
    let own = unit.team == player_team;
    let mut rows: [Option<StatRow>; 4] = [None; 4];

    // Row 1: `Hits` — with a non-zero base (factor 4).
    rows[0] = make_row(
        StatLabel::Hits,
        i32::from(unit.hits),
        values.get_attribute(ATTRIB_HITS),
        4,
    );

    // Row 2: `Ammo` for armed units / `Cargo` for the others.
    let storage_value = values.get_attribute(ATTRIB_STORAGE);
    let cargo_type = cargo_type_of(unit.unit_type);
    let mut scaling_factor = 0;

    if values.get_attribute(ATTRIB_ATTACK) != 0 {
        // We do not show ammo to the enemy (`ammo = 0` in C++).
        let ammo = if own {
            values.get_attribute(ATTRIB_AMMO)
        } else {
            0
        };

        rows[1] = make_row(StatLabel::Ammo, i32::from(unit.ammo), ammo, 1);
    } else {
        scaling_factor = if storage_value <= 4 {
            1
        } else if storage_value <= 8 {
            2
        } else {
            3
        };

        if (cargo_type as u8) < (CargoType::Land as u8) {
            scaling_factor = 10;
        }

        if !own {
            scaling_factor = 0;
        }

        rows[1] = make_row(
            match cargo_type {
                CargoType::Fuel => StatLabel::CargoFuel,
                CargoType::Gold => StatLabel::CargoGold,
                _ => StatLabel::Cargo,
            },
            i32::from(unit.storage),
            storage_value,
            scaling_factor,
        );
    }

    // Row 3: `Speed`, row 4: `Shots` (`min(ROUNDS, AMMO)`).
    rows[2] = make_row(
        StatLabel::Speed,
        i32::from(unit.speed),
        values.get_attribute(ATTRIB_SPEED),
        1,
    );
    rows[3] = make_row(
        StatLabel::Shots,
        i32::from(unit.shots),
        values
            .get_attribute(ATTRIB_ROUNDS)
            .min(values.get_attribute(ATTRIB_AMMO)),
        1,
    );

    // `GameManager_DrawInfoDisplayType2` — only if `ATTRIB_STORAGE == 0`.
    if storage_value == 0 {
        power_rows(state, unit, player_team, &mut rows);
    }

    // The player's greenhouse — Type3 instead of Type1.
    if unit.unit_type == UnitType::EcoSphere && own {
        greenhouse_rows(state, unit, &mut rows);
    } else {
        life_rows(state, unit, player_team, &mut rows);
    }

    // Port enhancement: the `USAGE` row of a unit-producing factory is the actual
    // raw per turn; a factory that is off shows no `USAGE` row. Overrides the
    // worker/power `Usage` line written by `life_rows`/`power_rows` above.
    if own && max_core::production::is_factory(unit.unit_type) {
        rows[1] = factory_raw_usage_row(unit);
    }

    // Complex stock — `_(d28b)` over row 3.
    if own
        && storage_value > 0
        && !unit.is_mobile()
        && (cargo_type as u8) >= (CargoType::Raw as u8)
        && (cargo_type as u8) <= (CargoType::Gold as u8)
    {
        let (value, capacity) = complex_cargo(state, unit, cargo_type);

        // The icon family of this row is the **cargo** family, not the worker one:
        // `GameManager_UpdateInfoDisplay` (`game_manager.cpp:7772-7775`) draws
        // `_(d28b)` with `ReportStats_CargoIcons[cargo_type * 2]` — `SI_RAW` for a
        // mine (the manual defect «у шахты TOTAL индицируется иконками рабочих»).
        assign_row(
            &mut rows,
            2,
            make_row_icon(
                cargo_stat_icon(cargo_type),
                StatLabel::Total,
                value,
                capacity,
                scaling_factor,
            ),
        );
    }

    rows
}

/// The `(full, empty)` icon family of a cargo row — `ReportStats_CargoIcons`
/// (`reportstats.cpp:56-59`): `CARGO_TYPE_RAW/FUEL/GOLD` → `SI_RAW`/`SI_FUEL`/
/// `SI_GOLD`, anything else (transport cargo) keeps the worker pips.
fn cargo_stat_icon(cargo_type: CargoType) -> StatIcon {
    match cargo_type {
        CargoType::Raw => StatIcon::Raw,
        CargoType::Fuel => StatIcon::Fuel,
        CargoType::Gold => StatIcon::Gold,
        _ => StatIcon::Work,
    }
}

/// One drawn row (`GameManager_DrawInfoDisplayRow`): the `current/base` value
/// in the threshold color, the label `0xA2` and the pip icons after the 75 px
/// label area, font `GNW_TEXT_FONT_2`.
fn draw_stat_row(
    frame: &mut RgbaBuffer,
    row: PixelRect,
    stat: &StatRow,
    palette: &UiPalette,
    icons: &StatIcons,
) {
    let font = GnwFontId::Font2.font();
    let value_rect = PixelRect::new(row.ulx, row.uly, 45, row.height);

    draw_centered(
        frame,
        value_rect,
        font,
        &stat.text(),
        palette.rgba(stat.color()),
    );
    draw_text_gnw(
        frame,
        row.ulx + 45,
        row.uly + ((row.height + 1) - font.height()).max(0) / 2,
        font,
        stat.label.caption(),
        palette.rgba(TEXT),
    );
    draw_stat_row_icons(frame, row, stat, palette, icons);
}

/// `GameManager_DrawInfoDisplayRowIcons` (`game_manager.cpp:7290-7367`): `base`
/// scaled pips starting at 75 px, `current` of them in the "full" image and the rest
/// in the "empty" one (C++ `icon + 1`), five icons per column block.
///
/// The visible pip size is picked by the remaining width (`factor`), the scale of the
/// row (`factor` of the C++ call, e.g. 4 for hits) caps the number of pips at
/// 25 (`20` for fuel/gold).
fn draw_stat_row_icons(
    frame: &mut RgbaBuffer,
    row: PixelRect,
    stat: &StatRow,
    palette: &UiPalette,
    icons: &StatIcons,
) {
    let mut factor = stat.factor.max(1);
    let base = stat.base.max(0);
    let current = stat.current.clamp(0, base);

    // `size = (icon == SI_FUEL || icon == SI_GOLD) ? 20 : 25;`
    let size = match stat.icon {
        StatIcon::Fuel | StatIcon::Gold => 20,
        _ => 25,
    };

    if (factor + base - 1) / factor > size {
        factor = (size + base - 1) / size;
    }

    let current_scaled = (factor + current - 1) / factor;
    let base_scaled = (factor + base - 1) / factor;

    // Hits change colour with the value (`SI_HITSR`/`SI_HITSY`).
    let (normal, empty) = stat.icon.names();
    let (normal, empty) = if stat.icon == StatIcon::Hits {
        match stat.color() {
            RED => ("SI_HITSR", "EI_HITSR"),
            YELLOW => ("SI_HITSY", "EI_HITSY"),
            _ => (normal, empty),
        }
    } else {
        (normal, empty)
    };

    let Some((full, used)) = icons.pair(normal, empty) else {
        return;
    };

    let height = row.height;
    let icon_width = full.width as i32;
    let icon_height = full.height as i32;
    let mut width = row.width - 75;

    // `if (base_value > 5) height = (18 - image->height) / 4; else 0;`
    let height_offset = if base_scaled > 5 {
        ((height - 1) - icon_height) / 4
    } else {
        0
    };
    let origin_y = row.uly + (((height - 1) - height_offset * 4) - icon_height) / 2;
    let origin_x = row.ulx + 75;

    // `base_value_scaled = (base_value + 4) / 5` — the number of icon columns.
    let blocks = (base_scaled + 4) / 5;
    let diff = base_scaled - (blocks - 1) * 5;
    width -= icon_width * blocks;

    let width_offset = blocks * 4 - (5 - diff);
    let (pip_width, pip_offset) = if width_offset != 0 {
        if width >= width_offset {
            ((width / width_offset).min(icon_width), 0)
        } else {
            (
                1,
                (blocks + (width_offset - width) - 2) / (blocks - 1).max(1),
            )
        }
    } else {
        (icon_width, 0)
    };

    let pitch = icon_width + pip_width * 4 - pip_offset;

    // C++ `GameManager_DrawInfoDisplayRowIcons` (`game_manager.cpp:7304-7310`):
    // `if (current_value) image = icon; else image = icon + 1;` — a row with
    // nothing left is drawn entirely with the **empty** (`EI_*`, contour) pips,
    // and the switch to `icon + 1` happens after the filled ones
    // (`:7360-7365`). The empty part is what is spent/unfilled, so the full/empty
    // direction must not be inverted.
    let mut image = if current_scaled > 0 { full } else { used };
    let mut remaining = current_scaled;
    for index in 0..base_scaled {
        let icon_index = index % 5;
        let px = origin_x + pitch * (index / 5) + pip_width * icon_index;
        let py = origin_y + icon_index * height_offset;

        blit_icon(frame, palette, image, px, py);

        remaining -= 1;

        if remaining == 0 {
            image = used;
        }
    }
}

/// Transparent blit of a row pip (`UnitStats_DrawImage`, `unitstats.cpp:82`).
fn blit_icon(frame: &mut RgbaBuffer, palette: &UiPalette, image: &SimpleImage, ulx: i32, uly: i32) {
    for y in 0..image.height as u32 {
        for x in 0..image.width as u32 {
            let Some(index) = image.pixel(x, y) else {
                continue;
            };

            if index == image.transparent {
                continue;
            }

            frame.set(ulx + x as i32, uly + y as i32, palette.rgba(index));
        }
    }
}

/// Four statistics lines of the selected unit ([`rows_for`]).
///
/// Rows that C++ does not draw remain the window background — [``None``].
fn draw_stats(
    frame: &mut RgbaBuffer,
    state: StateRef<'_>,
    selected: Option<&UnitInfo>,
    palette: &UiPalette,
    icons: &StatIcons,
) -> u32 {
    let Some(unit) = selected else {
        return 0;
    };

    let rows = rows_for(state, unit, state.player_team());
    let rects = [
        layout::STAT_ROW_1,
        layout::STAT_ROW_2,
        layout::STAT_ROW_3,
        layout::STAT_ROW_4,
    ];

    for (rect, row) in rects.iter().zip(rows.iter()) {
        if let Some(row) = row {
            draw_stat_row(frame, *rect, row, palette, icons);
        }
    }

    (layout::STAT_WINDOW.width * layout::STAT_WINDOW.height) as u32
}

/// Minimap window size (`WINDOW_MINIMAP`, 112×112).
const MINIMAP_SLOT: i32 = layout::MINIMAP.width;

/// Map scaling into the minimap window — `World::CalculateMinimapScaling`
/// (`world.cpp:917`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapView {
    /// Offset of the drawn map inside the window (minimap pixels).
    pub offset_x: i32,
    /// Offset by y.
    pub offset_y: i32,
    /// Width of the drawn map in the window.
    pub width: i32,
    /// Height of the drawn map in the window.
    pub height: i32,
    /// Map → window scale (`ResourceManager_MinimapWindowScale`).
    pub scale: f64,
}

/// Computes the scale and offset of the map in the 112×112 window (a port of
/// `World::CalculateMinimapScaling`).
pub fn minimap_view(map_width: i32, map_height: i32) -> MinimapView {
    let slot = MINIMAP_SLOT;
    let mut offset_x = 0;
    let mut offset_y = 0;
    let mut width = map_width;
    let mut height = map_height;
    let mut scale = 1.0;

    if map_width != slot || map_height != slot {
        if map_width > map_height {
            if map_width == slot {
                width = slot;
                height = map_height;
                offset_y = (slot - height) / 2;
            } else {
                scale = f64::from(slot) / f64::from(map_width);
                width = slot;
                height = (f64::from(map_height) * scale) as i32;
                offset_y = (slot - height) / 2;
            }
        } else if map_width == map_height {
            width = slot;
            height = slot;
            scale = f64::from(slot) / f64::from(map_width);
        } else if map_height == slot {
            width = map_width;
            height = slot;
            offset_x = (slot - width) / 2;
        } else {
            scale = f64::from(slot) / f64::from(map_height);
            width = (f64::from(map_width) * scale) as i32;
            height = slot;
            offset_x = (slot - width) / 2;
        }
    }

    MinimapView {
        offset_x,
        offset_y,
        width,
        height,
        scale,
    }
}

/// `GameManager_GridCenterOffset`: the top-left corner of the 2x minimap area.
///
/// In C++ the offset is updated incrementally (±4); here its limit is taken:
/// `clamp(view_center - map/4, 0, map/2)`.
pub fn minimap_center_offset(viewport: &Viewport, map_width: i32, map_height: i32) -> (i32, i32) {
    let (center_x, center_y) = viewport.center();
    let grid_x = (center_x / 64).clamp(0, map_width - 1);
    let grid_y = (center_y / 64).clamp(0, map_height - 1);

    (
        (grid_x - map_width / 4).clamp(0, map_width / 2),
        (grid_y - map_height / 4).clamp(0, map_height / 2),
    )
}

/// Window point → minimap cell (`GameManager_GetMinimapPosition`).
///
/// Takes the `2x` mode into account: it shows a 56×56 area around the camera
/// center, so the coordinate is divided by 2 and shifted by
/// [`minimap_center_offset`]. `None` — the point is outside the minimap window.
pub fn minimap_grid_at(
    x: i32,
    y: i32,
    map_width: i32,
    map_height: i32,
    viewport: &Viewport,
    two_x: bool,
) -> Option<(i16, i16)> {
    let area = layout::MINIMAP;

    if !area.contains(x, y) || map_width <= 0 || map_height <= 0 {
        return None;
    }

    let view = minimap_view(map_width, map_height);
    let scale = if view.scale > 0.0 { view.scale } else { 1.0 };
    let map_scale = f64::from(map_width.max(map_height)) / f64::from(MINIMAP_SLOT);
    let local_x = f64::from(x - area.ulx - view.offset_x) / scale;
    let local_y = f64::from(y - area.uly - view.offset_y) / scale;
    let mut grid_x = (local_x * map_scale) as i32;
    let mut grid_y = (local_y * map_scale) as i32;

    grid_x = grid_x.clamp(0, map_width - 1);
    grid_y = grid_y.clamp(0, map_height - 1);

    if two_x {
        let (offset_x, offset_y) = minimap_center_offset(viewport, map_width, map_height);

        grid_x = (grid_x / 2 + offset_x).clamp(0, map_width - 1);
        grid_y = (grid_y / 2 + offset_y).clamp(0, map_height - 1);
    }

    Some((grid_x as i16, grid_y as i16))
}

/// Minimap index palette: the window system palette in which indices
/// 64…159 are replaced by the `.WRL` palette (`World::ApplyPaletteAndTints`).
fn minimap_palette(ui: &UiPalette, wrl: &[u8; 768]) -> max_render::terrain::PaletteRgba {
    let mut palette = ui.palette_rgba();
    let wrl_palette = max_render::terrain::palette_from_wrl(wrl);

    palette[64..160].copy_from_slice(&wrl_palette[64..160]);
    palette
}

/// `ResourceManager_DarkeningColorIndexTable` (`world.cpp:493-508`).
///
/// The index color is dimmed by half (`max(7, c/4) * 31 / 63`), then the
/// nearest palette index is searched (`Color_MapColor`, the 9..31 and 96..127 ranges
/// are skipped). The table is applied to unexplored FOV cells.
pub fn darkening_table(palette: &max_render::terrain::PaletteRgba) -> [u8; 256] {
    let mut palette6 = [[0u8; 3]; 256];

    for (index, entry) in palette6.iter_mut().enumerate() {
        *entry = [
            palette[index][0] / 4,
            palette[index][1] / 4,
            palette[index][2] / 4,
        ];
    }

    let mut table = [0u8; 256];

    for (index, slot) in table.iter_mut().enumerate() {
        let [r, g, b] = palette6[index];
        let dark = [
            ((i32::from(r.max(7)) * 31 / 63) as u8),
            ((i32::from(g.max(7)) * 31 / 63) as u8),
            ((i32::from(b.max(7)) * 31 / 63) as u8),
        ];

        *slot = map_color(&palette6, dark);
    }

    table
}

/// `Color_MapColor(..., full_scan = false)` — the nearest palette color.
fn map_color(palette: &[[u8; 3]; 256], target: [u8; 3]) -> u8 {
    let mut best = 0u8;
    let mut best_distance = i32::MAX;

    for (index, color) in palette.iter().enumerate() {
        // C++ skips the 9..=31 and 96..=127 ranges (in byte offsets —
        // the same indices after /3).
        if (9..=31).contains(&index) || (96..=127).contains(&index) {
            continue;
        }

        let dr = i32::from(color[0]) - i32::from(target[0]);
        let dg = i32::from(color[1]) - i32::from(target[1]);
        let db = i32::from(color[2]) - i32::from(target[2]);
        let distance = dr * dr + dg * dg + db * db;

        if distance < best_distance {
            best_distance = distance;
            best = index as u8;

            if distance == 0 {
                break;
            }
        }
    }

    best
}

/// Color index of the unit dot on the minimap (`DrawMap_RenderMiniMapUnitList`).
fn minimap_unit_color(team: max_core::TeamId) -> u8 {
    match team.0 {
        0 => 0x01, // COLOR_RED
        1 => 0x02, // COLOR_GREEN
        2 => 0x03, // COLOR_BLUE
        3 => 0xFF, // "gray" — white
        _ => 0x04, // COLOR_YELLOW (aliens/other)
    }
}

/// Minimap from the `minimap` layer of `.WRL` (112×112 for standard planets).
fn draw_minimap<D: max_core::universe::Dispatcher>(
    frame: &mut RgbaBuffer,
    app: &crate::App<D>,
    assets: &Assets,
    palette: &UiPalette,
) -> u32 {
    let world = &assets.world;
    let minimap = world.minimap();
    let map_width = i32::from(world.width());
    let map_height = i32::from(world.height());

    if minimap.len() < (map_width * map_height) as usize || map_width <= 0 || map_height <= 0 {
        return 0;
    }

    let state = app.state_ref();
    let panel = &app.hud().panels;
    let viewport = app.viewport();
    let combined = minimap_palette(palette, world.palette());
    let dark = darkening_table(&combined);

    // 1. FOV: `Access_UpdateMinimapFogOfWar` — cells with `HeatMap::complete == 0`
    // are darker; the fog **closes** again when the last scanner leaves
    // (`Access_OnCellHidden`, `access.cpp:576-584`). The debug `--no-fow` mode
    // lifts the shading (`GameManager_AllVisible`).
    //
    // While the frame follows the visual anchors, the live coverage is the
    // frame-side one ([`crate::vision::VisionView`]): the radar of a moving unit
    // reveals the cells it has **visibly** reached, as
    // `Access_UpdateMapStatus` does on the cell crossing in `UnitInfo::Move`
    // (`unitinfo.cpp:1787-1816`). `dt = 0` (QA/headless) uses the simulation heat
    // map verbatim, so the golden frames do not drift.
    let all_visible = state.ai().all_visible || app.no_fow();
    let vision = app.vision();
    let follow_visual = vision.follow_visual() && !all_visible;
    let heat = state
        .ai()
        .heat_maps
        .get(usize::from(state.player_team().0))
        .map(|map| map.cells())
        .unwrap_or(&[]);
    let mut layer = Vec::with_capacity((map_width * map_height) as usize);

    for (index, &color) in minimap.iter().enumerate() {
        let covered = if follow_visual {
            vision.covered_mask().get_index(index)
        } else {
            heat.get(index).is_some_and(|cell| cell.complete > 0)
        };

        layer.push(if all_visible || covered {
            color
        } else {
            dark[usize::from(color)]
        });
    }

    // 2. Unit dots (`DrawMap_RenderMiniMapUnitList`).
    paint_minimap_units(
        &mut layer,
        state,
        map_width,
        map_height,
        panel.minimap_tnt,
        panel.minimap_2x,
        app.no_fow(),
        Some(vision),
    );

    // 3. Map into the 112×112 window (`GameManager_RenderMinimap` + `cscale`).
    let view = minimap_view(map_width, map_height);
    let mut window = vec![0u8; (MINIMAP_SLOT * MINIMAP_SLOT) as usize];
    let inner_width = MINIMAP_SLOT - view.offset_x * 2;
    let inner_height = MINIMAP_SLOT - view.offset_y * 2;

    for y in 0..inner_height {
        let source_y = y * map_height / inner_height;

        for x in 0..inner_width {
            let source_x = x * map_width / inner_width;
            let value = layer[(source_y * map_width + source_x) as usize];
            let dest = ((y + view.offset_y) * MINIMAP_SLOT + x + view.offset_x) as usize;

            if let Some(slot) = window.get_mut(dest) {
                *slot = value;
            }
        }
    }

    // 4. Camera frame (`draw_box(..., COLOR_RED)`); in 2x mode C++ overwrites it
    //    by scaling, so we draw it before 2x.
    let map_view = map_viewport(viewport);

    if let Some(range) = map_view.visible_tiles() {
        let scale = view.scale;
        let x0 = view.offset_x + (f64::from(range.x0) * scale) as i32;
        let y0 = view.offset_y + (f64::from(range.y0) * scale) as i32;
        let x1 = view.offset_x + ((f64::from(range.x1) + 0.9) * scale) as i32;
        let y1 = view.offset_y + ((f64::from(range.y1) + 0.9) * scale) as i32;

        draw_box_indexed(&mut window, MINIMAP_SLOT, (x0, y0, x1, y1), 0x01);
    }

    // 5. 2x mode (`GameManager_RenderMinimap`): the 56×56 area around the camera
    //    center is stretched over the whole window.
    if panel.minimap_2x {
        let (center_x, center_y) = minimap_center_offset(&map_view, map_width, map_height);
        let start_x = view.offset_x / 2 + (f64::from(center_x) * view.scale) as i32;
        let start_y = view.offset_y / 2 + (f64::from(center_y) * view.scale) as i32;
        let mut scaled = vec![0u8; window.len()];

        for y in 0..MINIMAP_SLOT / 2 {
            for x in 0..MINIMAP_SLOT / 2 {
                let source_x = (start_x + x).clamp(0, MINIMAP_SLOT - 1);
                let source_y = (start_y + y).clamp(0, MINIMAP_SLOT - 1);
                let value = window[(source_y * MINIMAP_SLOT + source_x) as usize];

                for dy in 0..2 {
                    for dx in 0..2 {
                        scaled[((2 * y + dy) * MINIMAP_SLOT + 2 * x + dx) as usize] = value;
                    }
                }
            }
        }

        window = scaled;
    }

    // 6. Indices → RGBA into `WINDOW_MINIMAP`.
    let area = layout::MINIMAP;
    let mut painted = 0u32;

    for y in 0..MINIMAP_SLOT {
        for x in 0..MINIMAP_SLOT {
            let index = window[(y * MINIMAP_SLOT + x) as usize];
            let [r, g, b, a] = combined[usize::from(index)];

            if frame.set(area.ulx + x, area.uly + y, Rgba::from_rgba([r, g, b, a])) {
                painted += 1;
            }
        }
    }

    painted
}

/// Unit dots on the minimap layer (exactly the C++ conditions).
///
/// `no_fow` — the debug observation mode (`--no-fow`): every unit gets a dot,
/// visible or not (`GameManager_AllVisible`).
///
/// `vision` — the frame-side scan ([`crate::vision::VisionView`]). When it follows
/// the visual anchors: an own unit's dot stands on the cell its **sprite** is in
/// (`App::visual_position`, so the dot does not run ahead of the unit), and a
/// foreign one is drawn only inside the own scan at those anchors — exactly the
/// `IsVisibleToTeam(GameManager_PlayerTeam)` test of
/// `DrawMap_RenderMiniMapUnitList` (`drawmap.cpp:838`), whose `grid` in the
/// original moves with the sprite. `None`/`!follow_visual` — the simulation state
/// verbatim (`dt = 0`, QA/headless frames, goldens).
fn paint_minimap_units(
    layer: &mut [u8],
    state: StateRef<'_>,
    map_width: i32,
    map_height: i32,
    tnt: bool,
    two_x: bool,
    no_fow: bool,
    vision: Option<&crate::vision::VisionView>,
) {
    use max_core::enums::unit_flags;
    use max_core::enums::UnitOrder;

    let player_team = state.player_team();
    let follow_visual = vision.is_some_and(|view| view.follow_visual());

    for id in state.unit_ids_in_order() {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        let own = unit.team == player_team;
        let grid = (i32::from(unit.grid.x), i32::from(unit.grid.y));

        // `IsVisibleToTeam(GameManager_PlayerTeam)` (own units are always visible
        // to their team). While the frame follows the visual anchors, a foreign
        // unit needs the own scan at the visual cells — the radar of a moving unit
        // then reveals it on arrival, not when the simulation cell did
        // ([`crate::vision::presented_visible`]).
        let visible = if no_fow || own {
            true
        } else if !unit.is_visible_to_team(player_team) {
            false
        } else if follow_visual {
            vision.is_some_and(|view| crate::vision::presented_visible(state, view, unit))
        } else {
            true
        };

        if !visible || unit.orders == UnitOrder::Idle || unit.flags & unit_flags::SELECTABLE == 0 {
            continue;
        }

        if tnt && unit.flags & (unit_flags::HAS_FIRING_SPRITE | unit_flags::FIRES_MISSILES) == 0 {
            continue;
        }

        let color = minimap_unit_color(unit.team);
        // An own unit's dot follows the sprite; a foreign one — its simulation
        // cell (the original has no difference: its `grid` is the sprite cell).
        let (grid_x, grid_y) = if own && follow_visual {
            vision
                .and_then(|view| view.anchor(id))
                .map(|(x, y)| (i32::from(x), i32::from(y)))
                .unwrap_or(grid)
        } else {
            grid
        };

        if two_x && !unit.is_quad() {
            if grid_x >= 0 && grid_y >= 0 && grid_x < map_width && grid_y < map_height {
                layer[(grid_y * map_width + grid_x) as usize] = color;
            }
        } else {
            let grid_x = grid_x.clamp(0, map_width - 2);
            let grid_y = grid_y.clamp(0, map_height - 2);

            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                layer[((grid_y + dy) * map_width + grid_x + dx) as usize] = color;
            }
        }
    }
}

/// `draw_box` over an indexed buffer (the camera frame).
fn draw_box_indexed(buffer: &mut [u8], stride: i32, rect: (i32, i32, i32, i32), color: u8) {
    let (x0, y0, x1, y1) = rect;

    if x1 < x0 || y1 < y0 {
        return;
    }

    for x in x0..=x1 {
        set_indexed(buffer, stride, x, y0, color);
        set_indexed(buffer, stride, x, y1, color);
    }
    for y in y0..=y1 {
        set_indexed(buffer, stride, x0, y, color);
        set_indexed(buffer, stride, x1, y, color);
    }
}

fn set_indexed(buffer: &mut [u8], stride: i32, x: i32, y: i32, color: u8) {
    if x < 0 || y < 0 || x >= stride || y >= stride {
        return;
    }

    if let Some(slot) = buffer.get_mut((y * stride + x) as usize) {
        *slot = color;
    }
}

/// Draws a line centered in a rectangle with the GNW font.
fn draw_centered(
    frame: &mut RgbaBuffer,
    rect: PixelRect,
    font: &GnwFont,
    text: &str,
    color: Rgba,
) -> i32 {
    let width = font.text_width(text);
    let x = rect.ulx + (rect.width - (width + 1)).max(0) / 2;
    let y = rect.uly + ((rect.height + 1) - font.height()).max(0) / 2;

    draw_text_gnw(frame, x, y, font, text, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A palette in which the text (`0xA2`) and red (`0x01`) are distinguishable from the background.
    fn test_palette() -> UiPalette {
        let mut colors = [[0u8, 0, 0]; 256];
        colors[usize::from(TEXT)] = [252, 248, 244];
        colors[usize::from(RED)] = [252, 0, 0];
        colors[usize::from(END_TURN_TEXT)] = [252, 248, 244];
        colors[2usize] = [0, 252, 0];
        colors[usize::from(TEAM_TEXT_SHADOW)] = [36, 12, 4];

        UiPalette::from_colors(colors)
    }

    /// "Ink" bounds of the color `color` inside the rectangle (inclusive).
    fn ink_bounds(
        frame: &RgbaBuffer,
        rect: PixelRect,
        color: Rgba,
    ) -> Option<(i32, i32, i32, i32)> {
        let mut bounds: Option<(i32, i32, i32, i32)> = None;

        for y in rect.uly..rect.lry() {
            for x in rect.ulx..rect.lrx() {
                if frame.get(x, y) != Some(color) {
                    continue;
                }

                bounds = Some(match bounds {
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    None => (x, y, x, y),
                });
            }
        }

        bounds
    }

    /// `GameManager_DrawTurnTimer`: `sprintf(text, "%2.2i:%2.2i", t / 60, t % 60)`
    /// — leading zeros, minutes:seconds (`%2.2i` = width 2, precision 2).
    #[test]
    fn timer_text_matches_cpp_printf() {
        assert_eq!(timer_text(0), "00:00");
        assert_eq!(timer_text(1), "00:01");
        assert_eq!(timer_text(20), "00:20");
        assert_eq!(timer_text(21), "00:21");
        assert_eq!(timer_text(45), "00:45");
        assert_eq!(timer_text(101), "01:41");
        assert_eq!(timer_text(180), "03:00");
        assert_eq!(timer_text(600), "10:00");
        assert_eq!(timer_text(3599), "59:59");
    }

    /// Timer color: `turn_time <= 20` → `COLOR_RED`, otherwise `0xA2`
    /// (`game_manager.cpp:2197-2213`). Zero is red too.
    #[test]
    fn timer_is_red_at_twenty_seconds_or_less() {
        assert_eq!(timer_color_index(0), RED);
        assert_eq!(timer_color_index(1), RED);
        assert_eq!(timer_color_index(20), RED);
        assert_eq!(timer_color_index(21), TEXT);
        assert_eq!(timer_color_index(45), TEXT);
        assert_eq!(timer_color_index(180), TEXT);
    }

    /// Timer text geometry in the `WINDOW_TURN_TIMER_DISPLAY` window
    /// (`windows[]`: 534,2…594,22) — checked against the DOS original frame
    /// (`build/anim-dos/avicap/f1/f_0001.png`, timer «01:41»): the ink
    /// occupies x 551…575, y 9…15.
    #[test]
    fn timer_text_geometry_matches_the_dos_frame() {
        let palette = test_palette();
        let mut frame = RgbaBuffer::filled(640, 480, Rgba::from_rgb([0, 0, 0]));
        let font = GnwFontId::Font5.font();

        draw_centered(
            &mut frame,
            layout::TURN_TIMER,
            font,
            &timer_text(101),
            palette.rgba(TEXT),
        );

        let bounds = ink_bounds(&frame, layout::TURN_TIMER, palette.rgba(TEXT))
            .expect("текст «01:41» нарисован");

        assert_eq!(
            bounds,
            (551, 9, 575, 15),
            "таймер «01:41» обязан стоять как в оригинале"
        );
    }

    /// The End Turn button is pressed only on the computer turn.
    #[test]
    fn end_turn_is_pressed_only_on_a_computer_turn() {
        assert!(end_turn_pressed(TeamKind::Computer));
        assert!(!end_turn_pressed(TeamKind::Player));
        assert!(!end_turn_pressed(TeamKind::Remote));
        assert!(!end_turn_pressed(TeamKind::None));
        assert!(!end_turn_pressed(TeamKind::Eliminated));
    }

    /// Label color of the pressed button — `FontColor(team + 1, team + 1, 0x3F)`,
    /// for the gray team `0xFF` (`game_manager.cpp:7876-7882`).
    #[test]
    fn end_turn_caption_uses_the_active_team_color() {
        assert_eq!(end_turn_caption_color(TeamId::RED), 1);
        assert_eq!(end_turn_caption_color(TeamId::GREEN), 2);
        assert_eq!(end_turn_caption_color(TeamId::BLUE), 3);
        assert_eq!(end_turn_caption_color(TeamId::GRAY), GRAY_TEAM_TEXT);
    }

    #[test]
    fn caption_offsets_match_the_cpp_label_table() {
        // `GameManager_MenuItemLabelOffsets` (`game_manager.cpp:275-278`).
        assert_eq!(PanelButton::Files.caption_offset(), (0, 2));
        assert_eq!(PanelButton::Preferences.caption_offset(), (0, 2));
        assert_eq!(PanelButton::Prev.caption_offset(), (-10, 2));
        assert_eq!(PanelButton::Next.caption_offset(), (10, 2));
        assert_eq!(PanelButton::Done.caption_offset(), (0, 3));
        assert_eq!(PanelButton::Reports.caption_offset(), (0, 3));
        assert_eq!(PanelButton::Chat.caption_offset(), (0, 3));
        assert_eq!(PanelButton::Survey.caption_offset(), (2, 2));
        assert_eq!(PanelButton::Status.caption_offset(), (2, 2));
        assert_eq!(PanelButton::Colors.caption_offset(), (2, 4));
        assert_eq!(PanelButton::Hits.caption_offset(), (2, 2));
        assert_eq!(PanelButton::Ammo.caption_offset(), (2, 2));
        assert_eq!(PanelButton::Range.caption_offset(), (2, 4));
        assert_eq!(PanelButton::Scan.caption_offset(), (2, 2));
        assert_eq!(PanelButton::Grid.caption_offset(), (2, 2));
        assert_eq!(PanelButton::Names.caption_offset(), (2, 4));
        assert_eq!(PanelButton::EndTurn.caption_offset(), (0, 0));
    }

    #[test]
    fn captions_are_centered_inside_the_inset_button_rect() {
        // `Button::SetCaption` (`button.cpp:412-449`) + `Text_TextLine`
        // (`text.cpp:772`): centering inside `width - x` / `height - y`.
        for button in PanelButton::ALL {
            let Some(caption) = button.caption() else {
                continue;
            };
            let rect = button.rect();
            let font = button.caption_font().font();
            let (offset_x, offset_y) = button.caption_offset();
            let (x, y) = button.caption_position();
            let text_width = font.text_width(caption) + 1;

            assert!(
                x >= rect.ulx && x + text_width <= rect.lrx(),
                "{button:?}: подпись «{caption}» вылезает за кнопку"
            );
            assert_eq!(
                y,
                rect.uly + (rect.height - offset_y - font.height()) / 2 + offset_y,
                "{button:?}: вертикальная центровка 1:1 button.cpp:433"
            );

            let expected_x = rect.ulx + ((rect.width - offset_x - text_width) / 2).max(0);
            assert_eq!(x, expected_x, "{button:?}: горизонтальная центровка");
        }

        // Exact values for the top 3×3 row (`FONT_2` — 9 px, height 16):
        // C++ `((16 - 2) - 9) / 2 + 2 = 4`.
        assert_eq!(PanelButton::Survey.caption_position().1, 298 + 4);
        assert_eq!(PanelButton::Hits.caption_position().1, 298 + 4);
        // COLORS/RANGE/NAMES: `((18 - 4) - 9) / 2 + 4 = 6`.
        assert_eq!(PanelButton::Colors.caption_position().1, 330 + 6);
        assert_eq!(PanelButton::Names.caption_position().1, 330 + 6);
    }

    #[test]
    fn panel_buttons_have_the_original_window_rects() {
        assert_eq!(PanelButton::Files.rect(), PixelRect::new(17, 4, 67, 18));
        assert_eq!(PanelButton::EndTurn.rect(), PixelRect::new(390, 2, 72, 21));
        assert_eq!(
            PanelButton::ZoomSlider.rect(),
            PixelRect::new(20, 275, 130, 14)
        );
        assert_eq!(PanelButton::Survey.rect(), PixelRect::new(4, 298, 55, 16));
        assert_eq!(PanelButton::Names.rect(), PixelRect::new(114, 330, 55, 18));
    }

    #[test]
    fn panel_hit_testing_covers_each_button_and_ignores_the_map() {
        for button in PanelButton::ALL {
            let rect = button.rect();
            assert_eq!(
                panel_button_at(rect.ulx, rect.uly),
                Some(button),
                "{button:?}: левый верх"
            );
            assert_eq!(
                panel_button_at(rect.lrx() - 1, rect.lry() - 1),
                Some(button),
                "{button:?}: правый низ"
            );
        }

        assert_eq!(panel_button_at(300, 200), None, "центр карты — не кнопка");
    }

    #[test]
    fn end_turn_overlaps_the_map_but_wins_the_hit_test() {
        assert!(layout::END_TURN_BUTTON
            .intersect(layout::MAIN_MAP)
            .is_some());
        assert_eq!(panel_button_at(400, 10), Some(PanelButton::EndTurn));
        assert_eq!(screen_to_map(400, 10), None, "кнопка перекрывает карту");
    }

    #[test]
    fn screen_to_map_subtracts_the_map_origin() {
        assert_eq!(screen_to_map(180, 18), Some((0, 0)));
        assert_eq!(screen_to_map(627, 465), Some((447, 447)));
        assert_eq!(screen_to_map(179, 18), None);
        assert_eq!(screen_to_map(180, 17), None);
        assert_eq!(screen_to_map(628, 465), None);
    }

    #[test]
    fn toggles_are_local_flags() {
        let mut panel = PanelState::default();

        assert!(!panel.get(PanelButton::Grid));
        assert!(panel.toggle(PanelButton::Grid));
        assert!(panel.get(PanelButton::Grid));
        assert!(!panel.toggle(PanelButton::Grid));
        assert!(!panel.toggle(PanelButton::EndTurn), "не переключатель");
    }

    #[test]
    fn minimap_unit_dots_respect_fov() {
        use max_core::enums::UnitType;
        use max_core::geo::{Point, MAP_TILES};
        use max_core::team::TeamId;

        let mut state = max_core::universe::GameUniverse::default();
        let own = state
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
            .unwrap();
        let enemy = state
            .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(20, 20), 0)
            .unwrap();
        let mut layer = vec![0u8; MAP_TILES];

        paint_minimap_units(
            &mut layer,
            StateRef::from(&state),
            112,
            112,
            false,
            false,
            false,
            None,
        );
        assert_eq!(
            layer[10 * 112 + 10],
            minimap_unit_color(TeamId::RED),
            "свой юнит виден"
        );
        assert_eq!(layer[20 * 112 + 20], 0, "чужой вне FOV не рисуется");

        // Surveyed (got into the FOV/radar) — the dot appears.
        state
            .unit_mut(enemy)
            .unwrap()
            .set_visible_to_team(TeamId::RED, true);
        layer.fill(0);
        paint_minimap_units(
            &mut layer,
            StateRef::from(&state),
            112,
            112,
            false,
            false,
            false,
            None,
        );
        assert_eq!(
            layer[20 * 112 + 20],
            minimap_unit_color(TeamId::GREEN),
            "видимый чужой юнит появляется на миникарте"
        );
        assert_eq!(layer[10 * 112 + 10], minimap_unit_color(TeamId::RED));
        let _ = own;
    }

    #[test]
    fn minimap_unit_dots_use_the_no_fow_debug_mode() {
        use max_core::enums::UnitType;
        use max_core::geo::{Point, MAP_TILES};
        use max_core::team::TeamId;

        let mut state = max_core::universe::GameUniverse::default();
        let _own = state
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
            .unwrap();
        let _enemy = state
            .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(20, 20), 0)
            .unwrap();
        let mut layer = vec![0u8; MAP_TILES];

        // Debug `--no-fow`: the enemy that is outside the FOV is drawn too.
        paint_minimap_units(
            &mut layer,
            StateRef::from(&state),
            112,
            112,
            false,
            false,
            true,
            None,
        );

        assert_eq!(
            layer[10 * 112 + 10],
            minimap_unit_color(TeamId::RED),
            "own unit is drawn"
        );
        assert_eq!(
            layer[20 * 112 + 20],
            minimap_unit_color(TeamId::GREEN),
            "the debug mode draws the enemy without the fog of war"
        );
    }

    #[test]
    fn zoom_slider_maps_to_viewport_zoom() {
        assert_eq!(zoom_slider_offset(64), 14);
        assert_eq!(zoom_slider_offset(8), 116);
        assert!(zoom_slider_offset(8) > zoom_slider_offset(64));

        assert_eq!(zoom_from_slider_x(20), 64, "левый край — 1:1");
        assert_eq!(zoom_from_slider_x(149), 8, "правый край — мелкий");
        assert!(
            zoom_from_slider_x(20 + 14 + 51) < 64,
            "середина — между краями"
        );

        for zoom in [8u32, 16, 32, 48, 64] {
            let x = layout::ZOOM_SLIDER.ulx + zoom_slider_offset(zoom);
            let roundtrip = zoom_from_slider_x(x);

            assert!(
                roundtrip.abs_diff(zoom) <= 2,
                "zoom {zoom} -> x {x} -> {roundtrip}"
            );
        }
    }

    #[test]
    fn map_viewport_is_448_and_keeps_camera() {
        let mut viewport = Viewport::new(640, 480);
        viewport.center_on_grid(50, 50);

        let map = map_viewport(&viewport);

        assert_eq!(map.screen(), (448, 448));
        assert_eq!(map.center(), viewport.center());
        assert_eq!(map.zoom(), viewport.zoom());
        assert_eq!(map.screen_to_grid(224, 224), (50, 50));
    }

    #[test]
    fn captions_and_fonts_match_game_manager() {
        assert_eq!(PanelButton::Files.caption(), Some("Files"));
        assert_eq!(PanelButton::EndTurn.caption(), Some("End Turn"));
        assert_eq!(PanelButton::EndTurn.caption_font(), GnwFontId::Font5);
        assert_eq!(PanelButton::Survey.caption_font(), GnwFontId::Font2);
        assert_eq!(PanelButton::Names.caption(), Some("Names"));
        assert_eq!(PanelButton::ZoomPlus.caption(), None);
    }

    #[test]
    fn unit_name_falls_back_to_the_type() {
        use max_core::enums::UnitType;
        use max_core::unit::UnitInfo;

        let mut unit = UnitInfo::new(UnitType::Tank, max_core::TeamId::RED, max_core::UnitId(1));
        assert_eq!(unit_name(&unit), "Tank");
        unit.name = "AVENGER".to_string();
        assert_eq!(unit_name(&unit), "AVENGER");
    }

    /// The panels load from the real `MAX.RES`, and every zone changes pixels.
    #[test]
    fn minimap_view_matches_calculate_minimap_scaling() {
        let view = minimap_view(112, 112);
        assert_eq!((view.offset_x, view.offset_y), (0, 0));
        assert_eq!((view.width, view.height), (112, 112));
        assert_eq!(view.scale, 1.0);

        // A wide map 224×112: scale 0.5, centered by y.
        let view = minimap_view(224, 112);
        assert_eq!(view.scale, 0.5);
        assert_eq!((view.width, view.height), (112, 56));
        assert_eq!(view.offset_y, 28);

        // A tall map 56×112: centered by x.
        let view = minimap_view(56, 112);
        assert_eq!((view.width, view.height), (56, 112));
        assert_eq!(view.offset_x, 28);
    }

    #[test]
    fn minimap_click_maps_to_grid_and_accounts_for_2x() {
        let viewport = Viewport::new(640, 480);

        assert_eq!(
            minimap_grid_at(15, 356, 112, 112, &viewport, false),
            Some((0, 0))
        );
        assert_eq!(
            minimap_grid_at(15 + 56, 356 + 56, 112, 112, &viewport, false),
            Some((56, 56))
        );
        assert_eq!(
            minimap_grid_at(15 + 111, 356 + 111, 112, 112, &viewport, false),
            Some((111, 111))
        );

        // 2x: camera center (56,56) → offset 28, the top-left corner shows
        // cell 28, the center — 56.
        assert_eq!(
            minimap_grid_at(15, 356, 112, 112, &viewport, true),
            Some((28, 28))
        );
        assert_eq!(
            minimap_grid_at(15 + 56, 356 + 56, 112, 112, &viewport, true),
            Some((56, 56))
        );

        assert_eq!(minimap_grid_at(14, 356, 112, 112, &viewport, false), None);
        assert_eq!(minimap_grid_at(15, 468, 112, 112, &viewport, false), None);
    }

    #[test]
    fn darkening_table_halves_brightness_like_cpp() {
        let mut palette = [[0u8, 0, 0, 255]; 256];
        palette[2] = [100, 100, 100, 255];
        palette[128] = [48, 48, 48, 255];
        let table = darkening_table(&palette);

        // `max(7,100/4)*31/63 = 12`; the nearest palette color — index 128.
        assert_eq!(table[2], 128);
        assert_eq!(table[0], 0, "чёрный остаётся чёрным");
    }

    #[test]
    fn minimap_unit_dots_use_team_colors_and_tnt_filter() {
        use max_core::enums::{unit_flags, UnitOrder, UnitType};
        use max_core::geo::Point;

        let mut state = max_core::universe::GameUniverse::default();
        let id = state
            .spawn_unit(
                UnitType::Tank,
                max_core::TeamId(0),
                Point::new(10, 10),
                unit_flags::SELECTABLE,
            )
            .expect("юнит");
        state.unit_mut(id).expect("юнит").orders = UnitOrder::Move;

        let mut layer = vec![0u8; 112 * 112];
        paint_minimap_units(
            &mut layer,
            StateRef::from(&state),
            112,
            112,
            false,
            false,
            false,
            None,
        );

        // 2×2 unit dot, RED color (index 1).
        assert_eq!(layer[10 * 112 + 10], 1);
        assert_eq!(layer[11 * 112 + 11], 1);

        // TNT filter: the unit has no firing sprite — the dot disappears.
        {
            let unit = state.unit_mut(id).expect("юнит");
            unit.flags &= !(unit_flags::HAS_FIRING_SPRITE | unit_flags::FIRES_MISSILES);
        }
        let mut filtered = vec![0u8; 112 * 112];
        paint_minimap_units(
            &mut filtered,
            StateRef::from(&state),
            112,
            112,
            true,
            false,
            false,
            None,
        );
        assert_eq!(filtered[10 * 112 + 10], 0);

        // In 2x a non-building is drawn as one cell.
        let mut zoomed = vec![0u8; 112 * 112];
        paint_minimap_units(
            &mut zoomed,
            StateRef::from(&state),
            112,
            112,
            false,
            true,
            false,
            None,
        );
        assert_eq!(zoomed[10 * 112 + 10], 1);
        assert_eq!(zoomed[11 * 112 + 11], 0);
    }

    #[test]
    fn unit_dot_colors_follow_the_cpp_hash_teams() {
        assert_eq!(minimap_unit_color(max_core::TeamId(0)), 0x01);
        assert_eq!(minimap_unit_color(max_core::TeamId(1)), 0x02);
        assert_eq!(minimap_unit_color(max_core::TeamId(2)), 0x03);
        assert_eq!(minimap_unit_color(max_core::TeamId(3)), 0xFF);
        assert_eq!(minimap_unit_color(max_core::TeamId(4)), 0x04);
    }

    #[test]
    fn panels_load_and_paint_every_region() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let max_res = root.join("max-original/MAX.RES");
        let color_pal = root.join("max-original/COLOR.PAL");
        let patches = root.join("assets/PATCHES.RES");

        if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
            return;
        }

        let config = crate::AppConfig {
            max_res: max_res.clone(),
            color_pal,
            patches_res: patches.clone(),
            ..Default::default()
        };
        let mut app = crate::App::new(config);
        app.init_assets().expect("ассеты игры загружаются");
        assert!(
            app.assets().is_some_and(|assets| assets.panels.is_some()),
            "Panels::load из MAX.RES"
        );

        let panels = app
            .assets()
            .and_then(|assets| assets.panels.as_ref())
            .expect("панели");
        let mut frame = RgbaBuffer::filled(640, 480, Rgba::rgb(1, 2, 3));

        let painted = panels.draw(&mut frame, &app);
        assert!(painted > 0);

        for rect in [
            layout::FILES_BUTTON,
            layout::PREFS_BUTTON,
            layout::END_TURN_BUTTON,
            layout::TURN_COUNTER,
            layout::TURN_TIMER,
            layout::CORNER_FLIC,
            layout::ZOOM_SLIDER,
            layout::COORDINATES,
            layout::UNIT_DESCRIPTION,
            layout::MINIMAP,
        ] {
            assert!(
                (0..rect.height).any(|y| (0..rect.width).any(|x| {
                    frame.get(rect.ulx + x, rect.uly + y) != Some(Rgba::rgb(1, 2, 3))
                })),
                "зона {rect:?} не закрашена"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Statistics rows (`GameManager_UpdateInfoDisplay`)
    // -----------------------------------------------------------------------

    /// A match with real team attributes (`TeamUnits::with_clan(Random)`).
    fn stats_state() -> max_core::universe::GameUniverse<max_core::universe::LocalDispatcher> {
        use max_core::team::TeamUnits;

        let mut state = max_core::universe::GameUniverse::default();
        *state.team_units_mut(TeamId::RED).expect("RED economy") =
            TeamUnits::with_clan(max_core::enums::TeamClan::Random);
        state.set_player_team(TeamId::RED);

        state
    }
    /// A unit in the match: `spawn_unit` + `DeployUnit` (a new unit has `storage = 0`).
    fn deployed(
        state: &mut max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
        unit_type: UnitType,
    ) -> UnitInfo {
        let id = state
            .spawn_unit(unit_type, TeamId::RED, max_core::geo::Point::new(20, 20), 0)
            .expect("юнит создан");

        state.unit_mut(id).expect("юнит в состоянии").storage = 0;

        state.snapshot().unit(id).expect("юнит").clone()
    }

    fn labels(rows: &[Option<StatRow>; 4]) -> [Option<&'static str>; 4] {
        rows.map(|row| row.map(|row| row.label.caption()))
    }

    /// A mine (own stationary `RAW` storage): `Hits/Cargo/Total`, without `Speed`.
    #[test]
    fn mining_station_panel_shows_hits_cargo_total_without_speed() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::MiningStation);
        let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), Some("Cargo"), Some("Total"), None],
            "C++: Hits, Cargo (ATTRIB_STORAGE 25), запас комплекса (d28b); Speed 0 — пропущен"
        );
        assert!(
            rows.iter()
                .flatten()
                .all(|row| row.label != StatLabel::Speed),
            "у шахты не должно быть Speed: {rows:?}"
        );
        // `MiningStation`: 56 hits, storage 25, cargo type RAW.
        assert_eq!(rows[0].expect("Hits").text(), "56/56");
        assert_eq!(rows[1].expect("Cargo").text(), "0/25");
        assert_eq!(rows[2].expect("Total").text(), "0/25");
        assert_eq!(rows[0].expect("Hits").color(), GREEN);
        assert_eq!(rows[1].expect("Cargo").color(), RED);
        assert_eq!(rows[2].expect("Total").color(), RED);
        // Row 4 (Shots) is empty: `min(ROUNDS, AMMO) == 0`.
        assert_eq!(rows[3], None);
    }

    /// Regression for the manual defect «у шахты TOTAL индицируется иконками
    /// рабочих»: the complex stock row of a mine takes the **cargo** family.
    ///
    /// Oracle: `GameManager_UpdateInfoDisplay` draws `_(d28b)` with
    /// `ReportStats_CargoIcons[cargo_type * 2]` (`game_manager.cpp:7772-7775`,
    /// `reportstats.cpp:56-59`) — `SI_RAW`/`EI_RAW` for a mine, `SI_FUEL`/
    /// `EI_FUEL` for a fuel storage. `SI_WORK` belongs to the worker rows of
    /// `DrawInfoDisplayType1` only (`game_manager.cpp:7528-7576`).
    #[test]
    fn complex_total_row_takes_the_cargo_icon_family() {
        let mut state = stats_state();
        let mine = deployed(&mut state, UnitType::MiningStation);
        let rows = rows_for(StateRef::from(&state), &mine, TeamId::RED);

        assert_eq!(
            rows[2].expect("Total").icon,
            StatIcon::Raw,
            "_(d28b) шахты — SI_RAW (ReportStats_CargoIcons[RAW * 2])"
        );
        assert_eq!(
            rows[1].expect("Cargo").icon,
            StatIcon::Raw,
            "_(1274) шахты — SI_RAW"
        );

        let mut state = stats_state();
        let dump = deployed(&mut state, UnitType::FuelStorage);
        let rows = rows_for(StateRef::from(&state), &dump, TeamId::RED);

        assert_eq!(
            rows[2].expect("Total").icon,
            StatIcon::Fuel,
            "_(d28b) топливного склада — SI_FUEL"
        );
    }

    /// Regression for the «заполнение наоборот» half of the same defect: a row with
    /// nothing left is drawn with the **empty** (`EI_*`, contour) pips, not with the
    /// full ones.
    ///
    /// Oracle: `GameManager_DrawInfoDisplayRowIcons` (`game_manager.cpp:7304-7310`)
    /// picks `icon + 1` (the empty image) when `current_value == 0`, and switches
    /// to it after the filled pips (`:7360-7365`).
    #[test]
    fn a_spent_row_draws_the_empty_pips_and_a_full_one_the_filled_pips() {
        let mut images = std::collections::BTreeMap::new();
        images.insert("SI_RAW", solid_image(0x01));
        images.insert("EI_RAW", solid_image(0x02));
        let icons = StatIcons { images };
        let palette = test_palette();
        let row = layout::STAT_ROW_2;
        let strip = PixelRect::new(row.ulx + 75, row.uly, row.width - 75, row.height);

        let count = |frame: &RgbaBuffer, color: Rgba| -> usize {
            let mut count = 0;
            for y in strip.uly..strip.lry() {
                for x in strip.ulx..strip.lrx() {
                    if frame.get(x, y) == Some(color) {
                        count += 1;
                    }
                }
            }
            count
        };
        let draw = |current: i32| -> RgbaBuffer {
            let mut frame = RgbaBuffer::filled(640, 480, Rgba::from_rgb([0, 0, 0]));
            let stat = StatRow {
                label: StatLabel::Cargo,
                icon: StatIcon::Raw,
                current,
                base: 10,
                factor: 1,
            };

            draw_stat_row_icons(&mut frame, row, &stat, &palette, &icons);
            frame
        };

        let empty = palette.rgba(0x02);
        let full = palette.rgba(0x01);

        // Nothing stored: every pip is the contour one.
        let spent = draw(0);
        assert_eq!(count(&spent, full), 0, "заполненных иконок быть не должно");
        assert!(count(&spent, empty) > 0, "пустая строка — контурные иконки");

        // Full: the opposite.
        let filled = draw(10);
        assert_eq!(count(&filled, empty), 0, "пустых иконок быть не должно");
        assert!(
            count(&filled, full) > 0,
            "полная строка — заполненные иконки"
        );

        // Half: both families are present.
        let half = draw(5);
        assert!(count(&half, full) > 0, "половина — заполненные");
        assert!(count(&half, empty) > 0, "половина — контурные");
    }

    /// A solid `SimpleImage` of the given palette index (row pip stub).
    fn solid_image(color: u8) -> max_assets::SimpleImage {
        let mut bytes = vec![4u8, 0, 4, 0, 0, 0, 0, 0];
        // `pixels[0]` is the transparent index; the rest is the pip colour.
        bytes.push(0);
        bytes.extend(std::iter::repeat(color).take(15));

        max_assets::SimpleImage::decode(&bytes).expect("simple image")
    }

    /// A tank (mobile ground with an attack): `Hits/Ammo/Speed/Shots`.
    #[test]
    fn tank_panel_keeps_speed_ammo_and_shots() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::Tank);
        let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), Some("Ammo"), Some("Speed"), Some("Shots")]
        );
        assert_eq!(rows[0].expect("Hits").text(), "24/24");
        assert_eq!(rows[1].expect("Ammo").text(), "14/14");
        assert_eq!(rows[2].expect("Speed").text(), "6/6");
        assert_eq!(rows[3].expect("Shots").text(), "2/2");
        assert!(rows.iter().flatten().all(|row| row.color() == GREEN));
    }

    /// A turret (stationary, with an attack): `Hits/Ammo/Shots`, no `Speed`.
    #[test]
    fn turret_panel_shows_ammo_and_shots_without_speed() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::GunTurret);
        let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), Some("Ammo"), None, Some("Shots")]
        );
        assert!(rows
            .iter()
            .flatten()
            .all(|row| row.label != StatLabel::Speed));
        assert_eq!(rows[3].expect("Shots").text(), "2/2");
    }

    /// A scout: `Hits/Speed`, empty `Ammo` and `Shots` (ATTACK = 0, ROUNDS = 0).
    #[test]
    fn surveyor_panel_skips_empty_attribute_rows() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::Surveyor);
        let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), None, Some("Speed"), None],
            "C++: ATTRIB_STORAGE 0 → Cargo нет, ATTACK 0 → Ammo нет"
        );
        assert_eq!(rows[0].expect("Hits").text(), "16/16");
        assert_eq!(rows[2].expect("Speed").text(), "7/7");
    }

    /// A factory: `Hits/USAGE(RAW)/Total` of workers (`DrawInfoDisplayType1`),
    /// without `Speed`.
    ///
    /// Port enhancement: the `USAGE` row of a producing factory is the **actual raw
    /// per turn** (`Cargo_GetRawConsumptionRate`, `SI_RAW`), not the worker need;
    /// while the factory is off the row is empty.
    #[test]
    fn factory_panel_shows_raw_usage_only_while_producing() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::TrainingHall);
        let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

        // Off (no build order): no USAGE row; row 3 (`Total` of workers) still
        // reports the complex's worker balance `0/1`.
        assert_eq!(
            labels(&rows),
            [Some("Hits"), None, Some("Total"), None],
            "выключенный завод: USAGE не отображается"
        );
        assert!(rows
            .iter()
            .flatten()
            .all(|row| row.label != StatLabel::Speed));

        // Producing: USAGE = raw per turn at `build_rate = 1`
        // (`Cargo_GetRawConsumptionRate(TRAINHAL, 1) = 1`).
        let mut producing = unit.clone();
        producing.orders = UnitOrder::Build;
        producing.state = UnitOrderState::BuildInProgress;
        producing.build_time = 4;
        producing.build_rate = 1;
        producing.build_list = vec![UnitType::Infantry];
        let rows = rows_for(StateRef::from(&state), &producing, TeamId::RED);

        let usage = rows[1].expect("Usage raw");
        assert_eq!(usage.label, StatLabel::Usage);
        assert_eq!(usage.icon, StatIcon::Raw, "расход RAW — SI_RAW");
        assert_eq!(usage.text(), "1/1", "1 RAW в ход при build_rate = 1");
        assert_eq!(usage.color(), GREEN);

        // 2× rate: `Cargo_GetRawConsumptionRate(1 * 2 → 4)` — the actual rate.
        let mut fast = producing.clone();
        fast.build_rate = 2;
        let rows = rows_for(StateRef::from(&state), &fast, TeamId::RED);

        assert_eq!(rows[1].expect("Usage raw").text(), "4/4");

        // Paused by a shortage: the factory spends nothing — no USAGE row.
        let mut paused = producing.clone();
        paused.build_paused = true;
        let rows = rows_for(StateRef::from(&state), &paused, TeamId::RED);

        assert_eq!(rows[1], None, "пауза: фактического расхода нет");
    }

    /// A power plant: `Hits/Power/Total` (`DrawInfoDisplayType2`).
    #[test]
    fn power_station_panel_shows_power_and_total() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::PowerStation);
        let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), Some("Power"), Some("Total"), None],
            "POWERSTN: `Cargo_GetPowerConsumptionRate = -6`, приказ POWER_OFF"
        );
        assert_eq!(rows[1].expect("Power").text(), "0/6");
        assert_eq!(rows[2].expect("Total").text(), "0/6");
        assert_eq!(rows[3], None, "Usage: база (потребление) 0 — строка пустая");
    }

    /// A habitat: `Hits/Teams/Total` of workers (`life = -3`).
    #[test]
    fn habitat_panel_shows_teams_and_total() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::Habitat);
        let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), Some("Teams"), Some("Total"), None]
        );
        assert_eq!(rows[1].expect("Teams").text(), "3/3");
        assert_eq!(rows[2].expect("Total").text(), "3/3");
        assert_eq!(rows[1].expect("Teams").color(), GREEN);
    }

    /// A foreign unit: `Ammo`/`Cargo` are not drawn (`factor == 0` in C++).
    #[test]
    fn enemy_unit_hides_ammo_and_cargo_rows() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::Tank);
        let mut enemy = unit;
        enemy.team = TeamId::GREEN;

        let rows = rows_for(StateRef::from(&state), &enemy, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), None, Some("Speed"), Some("Shots")]
        );
    }

    /// The three `MessageBoxStyle` background tables (`MessageManager_
    /// MessageBox_BgColorArray`): info darkens, warning tints red, notice is neutral
    /// on a normal tileset and red on snow.
    #[test]
    fn message_box_styles_follow_the_cpp_colour_tables() {
        let mut colors = [[0u8; 3]; 256];
        let entries = [
            [0, 0, 0],
            [255, 0, 0],
            [100, 100, 100],
            [50, 50, 50],
            [25, 25, 25],
            [255, 255, 255],
            [200, 0, 0],
            [64, 0, 0],
            [81, 81, 81],
            [40, 40, 40],
        ];

        colors[..entries.len()].copy_from_slice(&entries);

        let palette = UiPalette::from_colors(colors);
        let gray = Rgba::rgb(100, 100, 100);

        // `max(7, c) * 31 / 63` → 49 → the nearest palette entry is (50, 50, 50).
        assert_eq!(
            palette.message_box_color(MessageBoxStyle::Info, false, gray),
            Rgba::rgb(50, 50, 50)
        );

        // `(max(c, 63/4) * 63) / 31` → (203, 0, 0) → (200, 0, 0).
        let warning = palette.message_box_color(MessageBoxStyle::Warning, false, gray);
        assert!(
            warning.r > warning.g && warning.g == warning.b,
            "{warning:?}"
        );

        // `GenerateIntensityTable2`: neutral grey on green/desert/crater.
        let notice = palette.message_box_color(MessageBoxStyle::Notice, false, gray);
        assert!(notice.r == notice.g && notice.g == notice.b, "{notice:?}");

        // …and red on the snow tilesets.
        let snow = palette.message_box_color(MessageBoxStyle::Notice, true, gray);
        assert!(snow.r > snow.g && snow.g == snow.b, "{snow:?}");
    }

    /// The message window paints its whole rectangle with the style table and the
    /// text in white with a black outline (`MessageManager_DrawMessageBox`).
    #[test]
    fn warning_message_box_paints_a_red_tint_behind_white_text() {
        let mut colors = [[0u8; 3]; 256];
        let entries = [
            [0, 0, 0],
            [255, 255, 255],
            [100, 100, 100],
            [49, 49, 49],
            [203, 0, 0],
            [203, 0, 0],
            [81, 81, 81],
            [40, 40, 40],
        ];

        colors[..entries.len()].copy_from_slice(&entries);
        // `MessageManager_DrawMessageBoxText` draws in palette index `0xFF`.
        colors[0xFF] = [255, 255, 255];

        let palette = UiPalette::from_colors(colors);
        let background = Rgba::rgb(100, 100, 100);
        let mut frame = RgbaBuffer::filled(160, 60, background);

        draw_status_message(
            &mut frame,
            PixelRect::new(0, 0, 160, 60),
            &palette,
            "HX",
            MessageBoxStyle::Warning,
            false,
        );

        // The box starts at `(area.ulx, area.uly + 10)`; its corner is the tinted
        // background, not the untouched map pixel.
        let corner = frame.get(0, 10).expect("пиксель окна");
        assert_eq!(
            corner,
            palette.message_box_color(MessageBoxStyle::Warning, false, background)
        );
        assert!(corner.r > corner.g + corner.b, "красный фон: {corner:?}");

        // `Text_Blit` with `GNW_TEXT_OUTLINE`: the white fill over the black copy.
        let white = frame
            .pixels()
            .chunks_exact(4)
            .filter(|pixel| pixel[0] == 255 && pixel[1] == 255 && pixel[2] == 255)
            .count();

        assert!(white > 0, "белый текст поверх красного фона");
    }

    /// Colors — the `GameManager_DrawInfoDisplayRow` thresholds (1/4 and 1/2 of the base).
    #[test]
    fn stat_row_colors_follow_the_cpp_thresholds() {
        let row = |current, base, factor| StatRow {
            label: StatLabel::Hits,
            icon: StatIcon::Hits,
            current,
            base,
            factor,
        };

        assert_eq!(row(1, 4, 1).color(), RED);
        assert_eq!(row(2, 4, 1).color(), YELLOW);
        assert_eq!(row(3, 4, 1).color(), GREEN);
        assert_eq!(row(0, 1, 1).color(), RED);
        assert_eq!(row(1, 1, 1).color(), GREEN);
        assert_eq!(row(0, 0, 1).color(), TEXT);
        // `%i/%i` and `std::min(current, base)` as in C++.
        assert_eq!(
            make_row(StatLabel::Hits, 99, 56, 4).expect("строка").text(),
            "56/56"
        );
        assert_eq!(make_row(StatLabel::Hits, 0, 0, 4), None);
        assert_eq!(make_row(StatLabel::Cargo, 0, 25, 0), None);
    }

    /// Regression for the manual defect "mine Cargo 25 Raw, complex Total 50":
    /// the `Total` row must sum `Cargo_GetCargoCapacity` of **every** complex
    /// member (`complex.cpp:136-147`) — the mine's 25-raw buffer plus the
    /// storage's 50, not the storage alone.
    #[test]
    fn complex_total_includes_the_mine_capacity() {
        let mut state = stats_state();
        let mine = state
            .spawn_unit(
                UnitType::MiningStation,
                TeamId::RED,
                max_core::geo::Point::new(20, 20),
                0,
            )
            .expect("mine");
        let dump = state
            .spawn_unit(
                UnitType::MaterialStorage,
                TeamId::RED,
                max_core::geo::Point::new(22, 20),
                0,
            )
            .expect("storage");
        state.unit_mut(mine).expect("mine").storage = 0;
        state.unit_mut(dump).expect("storage").storage = 0;

        let snapshot = state.snapshot();
        assert_eq!(
            snapshot.unit(mine).expect("mine").complex,
            snapshot.unit(dump).expect("storage").complex,
            "the mine and the storage must merge into one complex"
        );

        let unit = snapshot.unit(mine).expect("mine").clone();
        let rows = rows_for(snapshot, &unit, TeamId::RED);

        assert_eq!(rows[1].expect("Cargo").text(), "0/25", "mine Cargo");
        assert_eq!(
            rows[2].expect("Total").text(),
            "0/75",
            "MININGST 25 + ADUMP 50 in the complex capacity"
        );
    }

    /// The oracle of the manual defect: `ComplexId` is unique only within a team,
    /// while C++ compares `Complex*`. Both starting bases own complex 2, so a mine
    /// with Cargo `12/25` showed `Total 12/50` — its own 25 plus the **enemy**
    /// mine's 25 of the same numeric complex id.
    #[test]
    fn complex_total_ignores_a_foreign_complex_with_the_same_id() {
        use max_core::team::TeamUnits;

        let mut state = stats_state();
        *state.team_units_mut(TeamId::GREEN).expect("GREEN economy") =
            TeamUnits::with_clan(max_core::enums::TeamClan::Random);

        let own_mine = state
            .spawn_unit(
                UnitType::MiningStation,
                TeamId::RED,
                max_core::geo::Point::new(20, 20),
                0,
            )
            .expect("own mine");
        let enemy_mine = state
            .spawn_unit(
                UnitType::MiningStation,
                TeamId::GREEN,
                max_core::geo::Point::new(60, 60),
                0,
            )
            .expect("enemy mine");
        state.unit_mut(own_mine).expect("mine").storage = 12;
        state.unit_mut(enemy_mine).expect("mine").storage = 0;

        let snapshot = state.snapshot();
        assert_eq!(
            snapshot.unit(own_mine).expect("mine").complex,
            snapshot.unit(enemy_mine).expect("mine").complex,
            "the fixture reproduces the numeric id collision"
        );

        let unit = snapshot.unit(own_mine).expect("mine").clone();
        let rows = rows_for(snapshot, &unit, TeamId::RED);

        assert_eq!(rows[1].expect("Cargo").text(), "12/25");
        assert_eq!(
            rows[2].expect("Total").text(),
            "12/25",
            "the enemy complex with the same id must not count"
        );
    }

    /// The same team filter in the power/workers `Total` rows: two teams'
    /// `POWGEN`s share a numeric complex id, only the own one is counted.
    #[test]
    fn power_total_ignores_a_foreign_complex_with_the_same_id() {
        use max_core::team::TeamUnits;

        let mut state = stats_state();
        *state.team_units_mut(TeamId::GREEN).expect("GREEN economy") =
            TeamUnits::with_clan(max_core::enums::TeamClan::Random);

        let own = state
            .spawn_unit(
                UnitType::PowerGenerator,
                TeamId::RED,
                max_core::geo::Point::new(20, 20),
                0,
            )
            .expect("own generator");
        let enemy = state
            .spawn_unit(
                UnitType::PowerGenerator,
                TeamId::GREEN,
                max_core::geo::Point::new(60, 60),
                0,
            )
            .expect("enemy generator");

        let snapshot = state.snapshot();
        assert_eq!(
            snapshot.unit(own).expect("own").complex,
            snapshot.unit(enemy).expect("enemy").complex,
            "the fixture reproduces the numeric id collision"
        );

        let unit = snapshot.unit(own).expect("own").clone();
        let rows = rows_for(snapshot, &unit, TeamId::RED);
        let total = rows[2].expect("Total");

        assert_eq!(
            total.text(),
            "0/1",
            "only the own generator gives its 1 power"
        );
    }

    /// The `Speed` row exists exactly for types with `ATTRIB_SPEED > 0` (all 93 types).
    #[test]
    fn speed_row_appears_only_with_a_speed_attribute() {
        use max_core::unit_values::base_unit_values;

        let state = stats_state();

        for &unit_type in UnitType::ALL {
            let base = base_unit_values(unit_type);
            let mut unit = UnitInfo::new(unit_type, TeamId::RED, max_core::UnitId(1));
            unit.base_values = base;
            unit.hits = base.hits;
            unit.speed = base.speed;
            unit.shots = base.rounds as u8;
            unit.ammo = base.ammo as u8;
            unit.storage = 0;

            let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);
            let has_speed = rows
                .iter()
                .flatten()
                .any(|row| row.label == StatLabel::Speed);

            assert_eq!(has_speed, base.speed > 0, "{unit_type:?}");
        }
    }
}
