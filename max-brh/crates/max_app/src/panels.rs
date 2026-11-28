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
/// Pressed/checked label outline — `FontColor(0xA2, 0xA3, 0x24)` of the down
/// image (`game_manager.cpp:7840-7845`, `Button::SetCaption`, `button.cpp:412-449`).
pub const CAPTION_OUTLINE_DOWN: u8 = 0xA3;
/// Pressed/checked label shadow — the `0x24` of the same `FontColor`.
pub const CAPTION_SHADOW_DOWN: u8 = 0x24;
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

    /// Label colours `(base, outline, shadow)` of the released (`pressed == false`)
    /// or pressed/checked (`pressed == true`) picture.
    ///
    /// Oracle: `GameManager_MenuInitButtons` (`game_manager.cpp:7834-7846`) gives
    /// every button **two** `SetCaption` colours — the up image gets
    /// `FontColor(0xA2, 0xA7, 0xE6)`, the down image `FontColor(0xA2, 0xA3, 0x24)`;
    /// End Turn gets `FontColor(0xA0, 0xA2, 0xE6)` for both (its down image is
    /// replaced by the team-coloured baked label, `game_manager.cpp:7860-7883`).
    /// `Button::SetCaption` (`button.cpp:412-449`) bakes the label into both image
    /// buffers, so the port has to switch the colours together with the picture.
    pub const fn caption_colors(self, pressed: bool) -> (u8, u8, u8) {
        match self {
            Self::EndTurn => (END_TURN_TEXT, END_TURN_OUTLINE, SHADOW),
            _ if pressed => (TEXT, CAPTION_OUTLINE_DOWN, CAPTION_SHADOW_DOWN),
            _ => (TEXT, CAPTION_OUTLINE, SHADOW),
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
    const NAMES: [&'static str; 28] = [
        "SI_HITSB", "EI_HITSB", "SI_HITSY", "EI_HITSY", "SI_HITSR", "EI_HITSR", "SI_AMMO",
        "EI_AMMO", "SI_RAW", "EI_RAW", "SI_FUEL", "EI_FUEL", "SI_GOLD", "EI_GOLD", "SI_LAND",
        "EI_LAND", "SI_SEA", "EI_SEA", "SI_AIR", "EI_AIR", "SI_SPEED", "EI_SPEED", "SI_SHOTS",
        "EI_SHOTS", "SI_POWER", "EI_POWER", "SI_WORK", "EI_WORK",
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

/// Coordinate display text — `sprintf(text, "%3.3i-%3.3i", x, y)`
/// (`GameManager_DrawMouseCoordinates`, `game_manager.cpp:4116-4121`).
///
/// The oracle passes the **1-based** mouse position: both call sites add one
/// (`game_manager.cpp:6570/6595`: `GameManager_MousePosition.x + 1`), so the
/// top-left cell is `001-001`.
pub fn coordinates_text(x: i32, y: i32) -> String {
    format!("{:03}-{:03}", x.max(0) + 1, y.max(0) + 1)
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
    /// Other buttons: the released/pressed pair and its position in the window.
    ///
    /// The pressed picture (`*_ON`) is the GNW `*D` look of `Button` while the
    /// pointer is held; the released state is already baked into `FRAMEPIC` for the
    /// `*_OF` pictures that lie exactly where the chrome draws them.
    aux: Vec<(ButtonPair, PixelRect)>,
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
        for (up, down, rect) in [
            ("PLAY_OF", "PLAY_ON", layout::PLAY_BUTTON),
            ("PAUSE_OF", "PAUSE_ON", layout::PAUSE_BUTTON),
            ("FIND_OFF", "FIND_ON", layout::CENTER_BUTTON),
            ("LOCK_OF", "LOCK_ON", layout::LOCK_BUTTON),
            ("PREV_OF", "PREV_ON", layout::PREV_BUTTON),
            ("UDONE_OF", "UDONE_ON", layout::DONE_BUTTON),
            ("NEXT_OF", "NEXT_ON", layout::NEXT_BUTTON),
            ("HELP_OF", "HELP_ON", layout::HELP_BUTTON),
            ("REPT_OFF", "REPT_ON", layout::REPORTS_BUTTON),
            ("CHAT_OFF", "CHAT_ON", layout::CHAT_BUTTON),
        ] {
            if let Some(pair) = pair(up, down) {
                aux.push((pair, rect));
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

        // Top buttons: the label colours follow the drawn picture
        // (`Button::SetCaption` bakes `color_up` into `*_OF`, `color_down` into
        // `*_ON` — `game_manager.cpp:7840-7845`).
        let files_pressed = self.draw_pair(
            frame,
            palette,
            &self.files,
            layout::FILES_BUTTON,
            false,
            &mut painted,
        );
        draw_panel_caption(frame, palette, PanelButton::Files, files_pressed);
        let prefs_pressed = self.draw_pair(
            frame,
            palette,
            &self.preferences,
            layout::PREFS_BUTTON,
            false,
            &mut painted,
        );
        draw_panel_caption(frame, palette, PanelButton::Preferences, prefs_pressed);

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
        let pressed = end_turn_pressed(active_kind)
            || crate::press::pressed(max_render::layout::END_TURN_BUTTON);

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
            draw_panel_caption_colored(
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
            draw_panel_caption(frame, palette, PanelButton::EndTurn, false);
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

        // Other buttons (Play/Pause/Center/Prev/Done/Next/Help/Reports/Chat): the
        // pressed picture of the held button (`GNW` `*D` — `button.c:479-531`) and
        // the pressed label colours baked into `*_ON` (`game_manager.cpp:7840-7845`).
        for (pair, rect) in &self.aux {
            let image = if crate::press::pressed(*rect) {
                &pair.down
            } else {
                &pair.up
            };
            painted += self.blit(frame, palette, image, rect.ulx, rect.uly, true);
        }
        for button in [
            PanelButton::Prev,
            PanelButton::Done,
            PanelButton::Next,
            PanelButton::Reports,
            PanelButton::Chat,
        ] {
            draw_panel_caption(frame, palette, button, crate::press::pressed(button.rect()));
        }

        // Toggle grid: the checked/pressed picture and its `FontColor(0xA2, 0xA3, 0x24)`
        // label (`GameManager_MenuClick*Button` → `SetRestState(rest_state)`).
        for (index, button) in PanelButton::TOGGLES.into_iter().enumerate() {
            let pair = &self.toggles[index];
            let active = panel.get(button);
            let pressed = self.draw_pair(frame, palette, pair, button.rect(), active, &mut painted);
            draw_panel_caption(frame, palette, button, pressed);
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
        let (pointer_x, pointer_y) = zoom_pointer_position(
            viewport.zoom(),
            i32::from(self.zoom_pointer.ulx),
            i32::from(self.zoom_pointer.uly),
        );
        painted += self.blit_clipped(
            frame,
            palette,
            &self.zoom_pointer,
            pointer_x,
            pointer_y,
            true,
            layout::ZOOM_SLIDER,
        );

        // Minimap from the `minimap` layer of `.WRL`.
        painted += draw_minimap(frame, app, assets, palette);

        // Portrait and statistics.
        painted += self.draw_portrait(frame, app, assets, palette);
        painted += draw_stats(
            frame,
            state,
            app.selected_unit(),
            palette,
            &self.stat_icons,
            app.hud().selected.and_then(|id| app.displayed_speed(id)),
        );

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
            &coordinates_text(i32::from(grid.0), i32::from(grid.1)),
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
        // `GameManager_GetWindowCursor` (`game_manager.cpp:4229`): the description strip
        // shows `UnitInfo::GetDisplayName` of the unit under the cursor
        // (`"Mk II Scout 7"`); the port also falls back to the selected unit so
        // that the strip is not empty when the pointer is off the map.
        let described = app.selected_unit().map(unit_display_name).or_else(|| {
            panel.cursor_grid.and_then(|(x, y)| {
                state
                    .units_at(max_core::geo::Point::new(x, y))
                    .into_iter()
                    .find_map(|id| state.unit(*id))
                    .map(unit_display_name)
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

        // `WINDOW_CORNER_FLIC` is filled by `GameManager_MenuUnitSelect` for a
        // unit that reached the selection only if visible to the player team
        // (`Access_GetEnemyUnit` → `IsVisibleToTeam`); the port re-checks it every
        // frame, so a hidden enemy does not keep its portrait.
        if !unit.is_visible_to_team(app.player_team()) {
            return painted;
        }

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
        //
        // The text is `UnitInfo::GetDisplayName` (`game_manager.cpp:7179`):
        // `"Mk I Tank 7"`. While the in-place rename is open the `Mk <roman> `
        // prefix stays and the rest of the line is the editable field —
        // `GameManager_FlicButtonRFunction` (`game_manager.cpp:4004-4023`): the
        // edit box starts at `Text_GetWidth(display) - Text_GetWidth(name)`,
        // i.e. right after the prefix.
        let font = GnwFontId::Font2.font();
        let editing = app
            .hud()
            .rename
            .as_ref()
            .filter(|editor| editor.unit == unit.id);

        match editing {
            Some(editor) => {
                let prefix = format!(
                    "Mk {} ",
                    crate::unit_names::roman_version(u32::from(unit.base_values.version))
                );

                draw_text_gnw(
                    frame,
                    area.ulx,
                    area.uly,
                    font,
                    &prefix,
                    palette.rgba(GREEN),
                );
                // C++ `TextEdit` keeps the field text white while it is fully
                // selected (`DrawFullText`: `is_selected` → color `0xFF`), then the
                // window color (green) once the player types; the blinking cursor
                // `|` (`DrawTillCursor`) is drawn statically at the end of the text.
                let field = if editor.selected {
                    editor.text.clone()
                } else {
                    format!("{}|", editor.text)
                };

                draw_text_gnw(
                    frame,
                    area.ulx + font.text_width(&prefix),
                    area.uly,
                    font,
                    &field,
                    palette.rgba(if editor.selected { 0xFF } else { GREEN }),
                );
            }
            None => {
                draw_text_gnw(
                    frame,
                    area.ulx,
                    area.uly,
                    font,
                    &unit_display_name(unit),
                    palette.rgba(GREEN),
                );
            }
        }
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
    ) -> bool {
        // `Button::Run` draws `*D` while the pointer is held over the button
        // (`button.c:479-531`); `active` is the toggle rest state.
        let pressed = active || crate::press::pressed(rect);
        let image = if pressed { &pair.down } else { &pair.up };

        *painted += self.blit(frame, palette, image, rect.ulx, rect.uly, true);

        pressed
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
        // A pressed minimap button shows `*_ON` even when the mode is off.
        let pressed = crate::press::pressed(rect);

        if !active && !pressed && !draw_up {
            return;
        }

        let image = if active || pressed {
            &pair.down
        } else {
            &pair.up
        };

        *painted += self.blit(frame, palette, image, rect.ulx, rect.uly, true);
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

/// Button label with the released or pressed/checked palette indices —
/// `Button::SetCaption` (`button.cpp:412-449`) with the two `FontColor`s of
/// `GameManager_MenuInitButtons` (`game_manager.cpp:7834-7846`).
pub fn draw_panel_caption(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    button: PanelButton,
    pressed: bool,
) {
    let (base, outline, shadow) = button.caption_colors(pressed);

    draw_panel_caption_colored(frame, palette, button, base, outline, shadow);
}

/// Button label with explicit palette indices — `Button::SetCaption`
/// (button.cpp:412-449) and the End Turn label baked into the resource
/// (`Text_TextBox(..., 0, 0, w, h, true, true, FontColor(...))`).
///
/// C++ 1:1 geometry: `SetCaption(caption, x, y, …)` shrinks the centering area to
/// `width - x` / `height - y`, then `Text_TextLine` (`text.cpp:772`) centers the line
/// in it: `x = (swidth - (Text_GetWidth + 1)) / 2` (a shift only if > 0),
/// `y = ((height - y) - Text_GetHeight()) / 2 + y`.
pub fn draw_panel_caption_colored(
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

/// Zoom slider travel — click `x` minus `window.ulx + 14`, clamped to
/// `window.lrx - window.ulx - 30` (`game_manager.cpp:6657-6667`; the inclusive
/// window rect (20,275)…(149,288) gives `149 - 20 - 30 = 99`).
pub const ZOOM_SLIDER_TRAVEL: i32 = 99;
/// Zoom slider knob offset from the window left edge — `width / 2` with
/// `width = (25 + 3) * scale` (`game_manager.cpp:1734`).
pub const ZOOM_SLIDER_KNOB_OFFSET: i32 = 14;
/// Zoom slider knob span — `window.lrx - window.ulx - width`
/// (`game_manager.cpp:1734-1736`: `149 - 20 - 28 = 101`).
pub const ZOOM_SLIDER_KNOB_SPAN: i32 = 101;
/// Zoom range of the original slider — `64 - 4` (`game_manager.cpp:1669`).
pub const ZOOM_RANGE: i32 = 60;
/// Zoom increment of the `+`/`−` panel buttons — `Gfx_ZoomLevel ± 10`
/// (`game_manager.cpp:6645-6679`).
pub const ZOOM_BUTTON_STEP: u32 = 10;
/// Mouse-wheel zoom sensitivity — `mouse_wheel_sensitivity` (`settings.cpp:109`):
/// one wheel notch changes `Gfx_ZoomLevel` by 3 (`mouse_get_wheel_delta`,
/// `mouse.c:323-325`; `GameManager_ProcessInput`, `game_manager.cpp:6412`).
pub const WHEEL_ZOOM_STEP: i32 = 3;

/// Zoom pointer offset.
///
/// C++ `GameManager_UpdateZoomSlider` (`game_manager.cpp:1731-1740`):
/// `offset = ((window.lrx - window.ulx - width) * (64 - zoom)) / 60 + width / 2`,
/// i.e. `101 * (64 - zoom) / 60 + 14` in the original 640×480 frame (`width = 28`).
/// The port keeps the slider graphics and this exact mapping; only the smallest
/// zoom is clamped to [`max_render::viewport::MIN_ZOOM`] (8) instead of the
/// original 4.
pub fn zoom_slider_offset(zoom: u32) -> i32 {
    let zoom = zoom.clamp(max_render::viewport::MIN_ZOOM, 64);

    (ZOOM_SLIDER_KNOB_SPAN * (64 - zoom) as i32) / ZOOM_RANGE + ZOOM_SLIDER_KNOB_OFFSET
}

/// Screen position of the zoom pointer's top-left corner.
///
/// Oracle: `GameManager_UpdateZoomSlider` (`game_manager.cpp:1731-1740`) loads
/// `ZOOMPTR` at `offset + window.ulx` / `window.uly + 1`, and
/// `WindowManager_DecodeSimpleImage` (`window_manager.cpp:784-785`) subtracts the
/// image's own hotspot: `ulx -= image->ulx; uly -= image->uly;`. `ZOOMPTR` is
/// 25×14 with `ulx = 12` (hot spot — a half of the 25 px knob), so the knob's
/// *hot spot* lands on `window.ulx + offset` and the pointer's left edge is 12 px
/// to its left. Without the subtraction the port drew the knob at the hot-spot
/// position and it sat 12 px right of the slider position — at the maximum zoom
/// (the leftmost position) the knob looked shifted off the track.
pub fn zoom_pointer_position(zoom: u32, pointer_ulx: i32, pointer_uly: i32) -> (i32, i32) {
    (
        layout::ZOOM_SLIDER.ulx + zoom_slider_offset(zoom) - pointer_ulx,
        layout::ZOOM_SLIDER.uly + 1 - pointer_uly,
    )
}

/// The inverse conversion: slider x → scale.
///
/// C++ `GameManager_ProcessInput`, `WINDOW_ZOOM_SLIDER_BUTTON`
/// (`game_manager.cpp:6651-6672`): on `MOUSE_PRESS_LEFT`
/// `mouse_x -= window.ulx + 14`, clamped to `[0, lrx - ulx - 30]`, then
/// `zoom = 64 - (mouse_x * 60) / (lrx - ulx - 30)` — 99 px of travel over the
/// 60 zoom steps. The result is clamped to the port's viewport range (8…64).
pub fn zoom_from_slider_x(x: i32) -> u32 {
    let relative =
        (x - layout::ZOOM_SLIDER.ulx - ZOOM_SLIDER_KNOB_OFFSET).clamp(0, ZOOM_SLIDER_TRAVEL);
    let zoom = 64 - (relative * ZOOM_RANGE) / ZOOM_SLIDER_TRAVEL;

    (zoom as u32).clamp(max_render::viewport::MIN_ZOOM, 64)
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

/// Unit name for the panels — `UnitInfo::GetName` (`unitinfo.cpp:757-779`).
///
/// The stored user name ([`UnitInfo::name`], `None` = never renamed) or
/// `"{Type} {unit_id}"` — the UFN of the type and the instance number
/// (`"Tank 7"`). The formatting lives in [`crate::unit_names::unit_name`];
/// this wrapper keeps the panel call sites short.
///
/// The type part is the UFN ([`crate::unit_names::unit_user_friendly_name`]),
/// not the variant's `Debug` name (`MiningStation` → «Mining Station»).
pub fn unit_name(unit: &UnitInfo) -> String {
    crate::unit_names::unit_name(unit)
}

/// Unit caption of the original — `UnitInfo::GetDisplayName`
/// (`unitinfo.cpp:781-803`): `"Mk I Tank 7"`.
///
/// The corner portrait, the description bar (`hover`), the `UnitStats` title,
/// the repair-shop list and the map name overlay all use this form.
pub fn unit_display_name(unit: &UnitInfo) -> String {
    crate::unit_names::unit_display_name(unit)
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
    /// `SI_LAND` — land transport cargo (`CARGO_TYPE_LAND`).
    Land,
    /// `SI_SEA` — sea transport cargo (`CARGO_TYPE_SEA`).
    Sea,
    /// `SI_AIR` — air transport cargo (`CARGO_TYPE_AIR`).
    Air,
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
            Self::Land => ("SI_LAND", "EI_LAND"),
            Self::Sea => ("SI_SEA", "EI_SEA"),
            Self::Air => ("SI_AIR", "EI_AIR"),
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

/// The HD twin of [`draw_status_message`] (`remaster-hd.md` §R6/§R7).
///
/// The same message box, laid out and drawn at the render scale: `S` multiplies
/// every logical metric (the `-20` of the wrap width, the `+20`/`+10` box
/// margins, the `10` per line and the `Text_TextLine` outline shift), and the
/// glyphs come from the HD `Font5` atlas ([`max_render::hd_text`]). With an HD
/// atlas that is the exact `×S` upscale of the original bitmap the result is
/// byte-identical to the `×S` upscale of the 1:1 box (the R6 golden invariant).
///
/// The background is the same colour table applied to the already drawn pixels
/// (`MessageManager_MessageBox_BgColorArray`), exactly like the 1:1 function:
/// the box is transparent over the map, not an opaque fill.
///
/// Anti-aliasing is not a parameter here (`gpu-aa-design.md`): the box is laid
/// out at `S` only and the output layer smooths the finished frame.
#[allow(clippy::too_many_arguments)]
pub fn draw_status_message_hd(
    frame: &mut RgbaBuffer,
    area: PixelRect,
    palette: &UiPalette,
    text: &str,
    style: MessageBoxStyle,
    snow: bool,
    font: &max_render::hd_text::HdFontAtlas,
) -> u32 {
    if text.is_empty() || area.width <= 0 {
        return 0;
    }

    let factor = font.scale().scale().max(1) as i32;
    // `MessageManager_WrapText(text, width - 20)` at `×S`.
    let max_line = (area.width - 20 * factor).max(20 * factor);
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
        + 20 * factor)
        .min(area.width);
    let height = 20 * factor + 10 * factor * lines.len() as i32;
    let rect = PixelRect::new(area.ulx, area.uly + 10 * factor, width, height);
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

        // `MessageManager_DrawMessageBoxText`: the black copy at `+S, +S`
        // first, the white fill on top.
        let drawn = max_render::hd_text::draw_text_hd_outlined(
            frame,
            rect.ulx + 10 * factor,
            rect.uly + 10 * factor + 10 * factor * index as i32,
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
    let rate = i32::from(unit.build_rate.max(1))
        .min(max_rate.max(1))
        .max(1);
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
    rows_for_with_speed(state, unit, player_team, None)
}

/// [`rows_for`] with the frame-synchronized `Speed` value ([`crate::App::displayed_speed`]).
///
/// C++ draws `unit->speed`, which its pixel walk decrements as the sprite moves;
/// the port simulation paces the route itself and draws the sprite on the simulation
/// cell, so the panel passes the simulation value (plus the points of a fallback
/// frame route, if any). The `base` (and therefore the pips) stays the unit's
/// `ATTRIB_SPEED`.
pub fn rows_for_with_speed(
    state: StateRef<'_>,
    unit: &UnitInfo,
    player_team: TeamId,
    display_speed: Option<u16>,
) -> [Option<StatRow>; 4] {
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

        // `ReportStats_CargoIcons[cargo_type * 2]`: a land transport (the Personnel
        // Carrier) uses `SI_LAND`, not the raw-material pips (`game_manager.cpp:7722`).
        let cargo_label = match cargo_type {
            CargoType::Fuel => StatLabel::CargoFuel,
            CargoType::Gold => StatLabel::CargoGold,
            _ => StatLabel::Cargo,
        };

        rows[1] = make_row_icon(
            cargo_stat_icon(cargo_type),
            cargo_label,
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

    // Row 3 is `Speed`; the visual value replaces the simulation one so the
    // number and the sprite reach zero together (`App::displayed_speed`).
    if let (Some(speed), Some(row)) = (display_speed, rows[2].as_mut()) {
        if row.label == StatLabel::Speed {
            row.current = i32::from(speed).min(row.base);
        }
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
        CargoType::Land => StatIcon::Land,
        CargoType::Sea => StatIcon::Sea,
        CargoType::Air => StatIcon::Air,
        CargoType::None => StatIcon::Work,
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
///
/// The panel is drawn only for a unit visible to the player team:
/// `GameManager_UpdateInfoDisplay` (`game_manager.cpp:7668`) opens with
/// `if (unit->IsVisibleToTeam(GameManager_PlayerTeam) || GameManager_MaxSpy)`.
/// The debug `MaxSpy` mode is not ported, so a foreign unit that left the
/// player's scan loses the panel (the own team always sees its units,
/// `UnitInfo::InitStealthStatus`).
fn draw_stats(
    frame: &mut RgbaBuffer,
    state: StateRef<'_>,
    selected: Option<&UnitInfo>,
    palette: &UiPalette,
    icons: &StatIcons,
    display_speed: Option<u16>,
) -> u32 {
    let Some(unit) = selected else {
        return 0;
    };

    if !unit.is_visible_to_team(state.player_team()) {
        return 0;
    }

    let rows = rows_for_with_speed(state, unit, state.player_team(), display_speed);
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
#[path = "panels_tests.rs"]
mod tests;
