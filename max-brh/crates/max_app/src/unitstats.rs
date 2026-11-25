//! Unit statistics window — a port of `UnitStats_Menu` (`unitstats.cpp`).
//!
//! Oracle: `max-cpp-port/src/unitstats.cpp:460-590`, opened from
//! `GameManager_ProcessPlayerInput` on a **right** click on the selected unit
//! (`game_manager.cpp:6747-6753`):
//!
//! ```text
//! if (GameManager_MouseButtons & MOUSE_RELEASE_RIGHT) {
//!     GameManager_ManagePlayerAction();
//!     if (GameManager_SelectedUnit != nullptr &&
//!         (GameManager_SelectedUnit == unit || GameManager_SelectedUnit == unit2)) {
//!         UnitStats_Menu(&*GameManager_SelectedUnit);
//! ```
//!
//! # Layout (oracle `UnitStatsMenu::UnitStatsMenu`)
//!
//! * window `STATS` 640×480 — the whole frame, palette of the window
//!   (`Window(STATS, ...)`);
//! * unit portrait `P_*` 300×240 at `(11, 13)` (`WindowManager_LoadBigImage`);
//! * the name — `Text_TextBox(..., 336, 60, 287, 11, 0xA2, true)`;
//! * the description — `Text_TextBox(..., 345, 90, 270, 117, 0xA2)`;
//! * the "Unit Status Screen" caption (`_(94b7)`, green) at `(327, 7)` 158×18;
//! * the characteristics table `UnitStats_DrawStats` at `(11, 293)` 245×174 —
//!   the same rows/`I_*` pips as the build/mobilization screens
//!   ([`crate::mobilize::Mobilization::stats_rows_for`]);
//! * `Done` (`BLDONE_U/D`, 54×23 at `(484, 452)`) and `Help`
//!   (`BLDHLP_U/D`, 26×23 at `(457, 452)`).
//!
//! # Behaviour
//!
//! Modal: `GameManager_DisableMainMenu` around the window, `Esc`/`Enter`/`Done`
//! close it, `?`/`Help` open the `STATS_SETUP` help section
//! (`HelpMenu_Menu("STATS_SETUP", WINDOW_MAIN_WINDOW)`).
//!
//! The window is drawn once when opened, like the C++ constructor (the game is
//! paused while it is up); the stored rows are a snapshot of the unit's values.

use max_assets::image::BigImage;
use max_core::enums::UnitType;
use max_core::team::TeamId;
use max_core::unit::{UnitId, UnitInfo};
use max_core::unit_values::UnitValues;
use max_core::universe::StateRef;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFontId};

use crate::assets::Assets;
use crate::menu::{draw_big_image_at, draw_image_button, wrap_gnw_text, MenuButtonPair};
use crate::mobilize::{self, StatRow};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};

/// `Window(STATS, ...)` — the full 640×480 frame.
pub const WINDOW: PixelRect = PixelRect::new(0, 0, 640, 480);
/// `debug_window.window = (11, 13) - (311, 253)` — the unit portrait.
pub const PORTRAIT: PixelRect = PixelRect::new(11, 13, 300, 240);
/// `Text_TextBox(text, 336, 60, 287, 11)` — the unit name.
pub const NAME_RECT: PixelRect = PixelRect::new(336, 60, 287, 11);
/// `Text_TextBox(description, 345, 90, 270, 117)`.
pub const DESCRIPTION_RECT: PixelRect = PixelRect::new(345, 90, 270, 117);
/// `Text_TextBox(_(94b7), 327, 7, 158, 18, COLOR_GREEN, true)`.
pub const CAPTION_RECT: PixelRect = PixelRect::new(327, 7, 158, 18);
/// `Button(BLDONE_U, BLDONE_D, 484, 452)` — 54×23.
pub const DONE_RECT: PixelRect = PixelRect::new(484, 452, 54, 23);
/// `Button(BLDHLP_U, BLDHLP_D, 457, 452)` — 26×23.
pub const HELP_RECT: PixelRect = PixelRect::new(457, 452, 26, 23);
/// `_(94b7)` in the shipped `en-US` catalog.
pub const CAPTION: &str = "Unit Status Screen";

/// What to do with the window input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitStatsAction {
    /// The window stays open.
    None,
    /// `Esc`/`Enter`/`Done` — close (`UnitStatsMenu::Run`).
    Close,
    /// `?`/`Help` — `HelpMenu_Menu("STATS_SETUP")`.
    Help,
}

/// Assets of the window: `STATS`, `BLDONE`, `BLDHLP`.
#[derive(Debug, Clone)]
pub struct UnitStatsAssets {
    /// `STATS` background 640×480 (own palette).
    pub background: Option<BigImage>,
    /// `Done` (`BLDONE_U/D`).
    pub done: Option<MenuButtonPair>,
    /// `Help` (`BLDHLP_U/D`).
    pub help: Option<MenuButtonPair>,
}

impl UnitStatsAssets {
    /// Loads the resources; missing ones are `None`.
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let simple = |name: &str| -> Option<max_assets::image::SimpleImage> {
            let bytes = archive.read(name).ok()?;
            max_assets::image::SimpleImage::decode(bytes).ok()
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair::new(simple(up)?, simple(down)?))
        };

        Self {
            background: archive
                .read("STATS")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            done: pair("BLDONE_U", "BLDONE_D"),
            help: pair("BLDHLP_U", "BLDHLP_D"),
        }
    }

    /// `STATS` loaded — the window can be drawn.
    pub fn is_ready(&self) -> bool {
        self.background.is_some()
    }
}

/// Open unit statistics window (`UnitStats_Menu`).
#[derive(Debug, Clone)]
pub struct UnitStatsMenu {
    /// Whose statistics (`GameManager_SelectedUnit`).
    pub unit: UnitId,
    /// Unit type (the portrait/description lookup).
    pub unit_type: UnitType,
    /// Team (the base-values column and the portrait palette).
    pub team: TeamId,
    /// Display name (`UnitInfo::GetDisplayName`, the port's user-friendly name).
    pub name: String,
    /// `Unit::GetDescription()` (`""` — none in the data).
    pub description: &'static str,
    /// Rows of `UnitStats_DrawStats` — drawn once, like the C++ constructor.
    pub rows: Vec<StatRow>,
    /// `P_*` portrait (`WindowManager_LoadBigImage`).
    ///
    /// `Arc`: the decoded image owns a 768-byte palette; keeping it inline would
    /// grow `App` (which tests place on the stack) by a kilobyte.
    pub portrait: Option<std::sync::Arc<BigImage>>,
}

impl UnitStatsMenu {
    /// Builds the window state for `unit` — `UnitStatsMenu::UnitStatsMenu`.
    ///
    /// `team_base` — `team_units->GetBaseUnitValues`; `current` — the unit's own
    /// `GetBaseValues()` (the statistics show the unit, not the production type).
    pub fn new(unit: &UnitInfo, team_base: &[UnitValues], assets: Option<&Assets>) -> Option<Self> {
        let index = unit.unit_type as usize;

        // Without the team table the base column cannot be built (a demo/QA world
        // has no economy): `UnitStats_DrawStats` reads it by type index.
        if team_base.len() <= index {
            return None;
        }

        let mut current = team_base.to_vec();
        current[index] = unit.base_values;

        // `UnitStats_DrawStats(..., I_RAW, I_RAWE)` (`unitstats.cpp:515-516`):
        // the cost row is paid in raw materials.
        let rows = mobilize::Mobilization::stats_rows_for(
            team_base,
            &current,
            unit.unit_type,
            "I_RAW",
            "I_RAWE",
        );

        let portrait = assets.and_then(|assets| {
            let name = crate::cargo_unit_data::unit_portrait(unit.unit_type)?;
            let bytes = assets.max_res.read(name).ok()?;
            BigImage::decode(bytes).ok().map(std::sync::Arc::new)
        });

        Some(Self {
            unit: unit.id,
            unit_type: unit.unit_type,
            team: unit.team,
            name: crate::unit_names::unit_user_friendly_name(unit.unit_type).to_string(),
            description: crate::cargo_unit_data::unit_description(unit.unit_type),
            rows,
            portrait,
        })
    }

    /// Input — `UnitStatsMenu::Run` (`Esc`/`Enter`/`Done`/`?`/`Help`).
    pub fn input(&self, event: InputEvent) -> UnitStatsAction {
        match event {
            InputEvent::Key(Key::Escape) | InputEvent::Key(Key::Enter) => UnitStatsAction::Close,
            InputEvent::Key(Key::Char('?')) => UnitStatsAction::Help,
            InputEvent::MouseClick(x, y) => {
                if DONE_RECT.contains(x, y) {
                    UnitStatsAction::Close
                } else if HELP_RECT.contains(x, y) {
                    UnitStatsAction::Help
                } else {
                    UnitStatsAction::None
                }
            }
            _ => UnitStatsAction::None,
        }
    }

    /// Draws the window (`UnitStatsMenu` constructor + `win_draw`).
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        assets: Option<&UnitStatsAssets>,
        cargo: Option<&crate::mobilize::CargoMenuAssets>,
        fallback: Option<UiPalette>,
    ) -> u32 {
        let palette = assets
            .and_then(|assets| assets.background.as_ref())
            .map(UiPalette::from_big_image)
            .or(fallback);

        let Some(palette) = palette else {
            return 0;
        };
        let font = GnwFontId::Font5.font();
        let mut painted = 0;

        if let Some(background) = assets.and_then(|assets| assets.background.as_ref()) {
            painted += draw_big_image_at(frame, background, &palette, (0, 0), WINDOW);
        } else {
            frame.fill_rect(WINDOW, palette.rgba(0x22));
            painted += 1;
        }

        // Unit portrait: `WindowManager_LoadBigImage` reads the `P_*` indices with
        // the window palette (`palette_from_image = false`).
        if let Some(portrait) = self.portrait.as_ref() {
            painted += draw_big_image_at(
                frame,
                portrait.as_ref(),
                &palette,
                (PORTRAIT.ulx, PORTRAIT.uly),
                PORTRAIT,
            );
        }

        // Caption `_(94b7)` — green, centered (`Text_TextBox(..., true)`).
        painted += draw_text_gnw_centered(
            frame,
            CAPTION_RECT,
            font,
            CAPTION,
            palette.rgba(COLOR_GREEN),
        )
        .max(0) as u32;

        // Name (`0xA2`) and description (`0xA2`), the C++ text areas.
        painted +=
            draw_text_clipped_centered(frame, NAME_RECT, font, &self.name, palette.rgba(0xA2));

        let mut y = DESCRIPTION_RECT.uly;
        for line in wrap_gnw_text(self.description, DESCRIPTION_RECT.width) {
            if y + font.height() > DESCRIPTION_RECT.uly + DESCRIPTION_RECT.height {
                break;
            }
            painted += draw_text_gnw(
                frame,
                DESCRIPTION_RECT.ulx,
                y,
                font,
                &line,
                palette.rgba(0xA2),
            )
            .max(0) as u32;
            y += font.height();
        }

        // Characteristics (`UnitStats_DrawStats` at (11,293) 245×174).
        if let Some(cargo) = cargo {
            painted += mobilize::draw_stats(frame, cargo, &palette, &self.rows);
        }

        // Buttons: `Done` (caption `_(cf39)`) and `Help`.
        painted += self.draw_button(
            frame,
            &palette,
            assets.and_then(|a| a.done.as_ref()),
            DONE_RECT,
            "Done",
        );
        painted += self.draw_button(
            frame,
            &palette,
            assets.and_then(|a| a.help.as_ref()),
            HELP_RECT,
            "Help",
        );

        painted
    }

    fn draw_button(
        &self,
        frame: &mut RgbaBuffer,
        palette: &UiPalette,
        pair: Option<&MenuButtonPair>,
        rect: PixelRect,
        label: &str,
    ) -> u32 {
        match pair {
            Some(pair) => draw_image_button(
                frame,
                palette,
                pair.image(false),
                pair.image(true),
                rect,
                label,
                false,
            ),
            None => {
                frame.fill_rect(rect, palette.rgba(0x17));
                1
            }
        }
    }
}

/// `COLOR_GREEN` index of the game window palette (`GameManager_MarkerColor`).
const COLOR_GREEN: u8 = 0x02;

/// One centered line clipped to `rect` — `Text_TextBox(..., horizontal_align,
/// true)`: the width is limited by the box, the line is centered.
fn draw_text_clipped_centered(
    frame: &mut RgbaBuffer,
    rect: PixelRect,
    font: &max_render::text::GnwFont,
    text: &str,
    color: max_render::buffer::Rgba,
) -> u32 {
    // `Text_TextBox` clips at the box width: keep the prefix that fits.
    let mut clipped = String::new();
    for ch in text.chars() {
        let mut candidate = clipped.clone();
        candidate.push(ch);
        if font.text_width(&candidate) > rect.width {
            break;
        }
        clipped = candidate;
    }

    if clipped.is_empty() {
        return 0;
    }

    let x = rect.ulx + (rect.width - (font.text_width(&clipped) + 1)).max(0) / 2;
    let y = rect.uly + ((rect.height + 1) - font.height()).max(0) / 2;

    draw_text_gnw(frame, x, y, font, &clipped, color).max(0) as u32
}

/// Opens the window for the selected unit of `state` (`UnitStats_Menu`).
///
/// `None` — the unit does not exist or the team table is missing.
pub fn open_for(
    state: StateRef<'_>,
    unit: UnitId,
    assets: Option<&Assets>,
) -> Option<UnitStatsMenu> {
    let unit = state.unit(unit)?.clone();
    let team_base = state
        .team_units()
        .get(unit.team.0 as usize)
        .map(|units| units.base_values.as_slice())
        .unwrap_or(&[]);

    UnitStatsMenu::new(&unit, team_base, assets)
}

#[cfg(test)]
#[path = "unitstats_tests.rs"]
mod tests;
