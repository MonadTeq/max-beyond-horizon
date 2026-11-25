//! X-FER screen — C++ `TransferMenu` (`max-cpp-port/src/transfermenu.cpp`).
//!
//! # What this is
//!
//! The `x-fer` item of the command menu enables cursor mode (`unit->cursor = 3`,
//! `UnitsManager_Popup_OnClick_Transfer`). A click on an own unit calls
//! `GameManager_TransferCargo` (`game_manager.cpp:4634`): it checks the cargo
//! types, if necessary replaces the target with a complex building with a suitable
//! stock (`GameManager_GetUnitWithCargoType`), calls `SetParent` and
//! opens this window. `Done`/`Enter` issues `ORDER_TRANSFER` (packet **08**,
//! processor **P3** — `transfer_cargo`), `Esc`/`Cancel` — nothing.
//!
//! # Layout (`Window(XFERPIC, WINDOW_MAIN_MAP)`)
//!
//! `XFERPIC` 308×162 is centered in the map window (448×448 at (180,18)); C++
//! `Window::Window(ResourceID, win_id)` computes by **inclusive** bounds
//! (`lrx - ulx = 447`), so the window lands at `(249,160)`, not `(250,161)`.
//!
//! Inside the window (`transfermenu.cpp:196-266`):
//!
//! | Element | Rectangle/point |
//! |---|---|
//! | amount (`xfer_amount`) | (141,15,29,20), `FONT5`, `0xFF`, centered |
//! | source stock | (28,28,60,16) |
//! | target stock | (223,28,60,16) |
//! | unit icons | centers (104,36) and (207,36), `ReportStats_DrawListItemIcon` |
//! | labels | (10,52,110,30) and (191,52,110,30), `COLOR_BLACK` |
//! | direction arrow | (141,43) `XFRLFARO`/`XFRRTARO` (31×16) |
//! | left button | (17,89) `XFRLFT_U/D` (18×17) |
//! | right button | (278,89) `XFRRGT_U/D` (18×17) |
//! | bar | (44,89,223,16), `SMBRRAW`/`SMBRFUEL`/`SMBRGOLD` |
//! | Cancel | (82,125) `XFRCAN_U/D` (64×24) |
//! | Help | (147,125) `XFRHLP_U/D` (26×24) |
//! | Done | (174,125) `XFRDNE_U/D` (57×24) |
//!
//! # Bar value
//!
//! `LimitedScrollbar` (`scrollbar.cpp:298`) with `free_capacity = target_capacity
//! − target_materials`, `zero_offset = -target_materials`, `xfer_give_max =
//! min(target_free, source_materials)`, `xfer_take_max = -min(source_capacity −
//! source_materials, target_materials)`, `value = give_max`. A positive
//! value — from the unit into the target, a negative one — back. A click on the bar:
//! `new = (w/2 + capacity·x)/w + zero_offset`, then clipping
//! `[zero_offset, free_capacity]` and `[take_max, give_max]` (`scrollbar.cpp:327`).
//! Fill: `fill = (value − zero_offset)·223 / (free_capacity − zero_offset)`,
//! `LoadHorizontalBar` copies the **right** `fill` pixels of the texture per row.
//!
//! # Keys (`TransferMenu::ProcessKey`)
//!
//! `Enter` — Done, `Esc`/`Ctrl+Esc` — Cancel (`value = 0`), `Shift+/`/`?` —
//! `HelpMenu_Menu("TRANSFER_SETUP")`, `←/→` (`PageUp/PageDown`) — the bar step.
//! `KP_Enter` in C++ is bound to Done (`SetRValue`).

use max_assets::image::{BigImage, SimpleImage};
use max_core::enums::{UnitOrder, UnitOrderState};
use max_core::universe::StateRef;
use max_core::team::TeamId;
// Explicit import of the classification extension type (`UnitTypeClassify` — sealed in
// max_core); no glob re-exports (phase 5.2).
use max_core::UnitTypeClassify;
use max_core::unit::UnitId;
use max_core::victory::{cargo_type_of, CargoType};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::layout::MAIN_MAP;
use max_render::text::{draw_text_gnw, draw_text_gnw_centered_shadowed, GnwFontId};

use crate::menu::{blit_simple, draw_big_image_at, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::Key;

/// `XFERPIC` window size.
pub const WINDOW_SIZE: (i32, i32) = (308, 162);

/// Amount rectangle (141,15,29,20).
pub const AMOUNT_RECT: PixelRect = PixelRect::new(141, 15, 29, 20);
/// Source stock (28,28,60,16).
pub const SOURCE_RECT: PixelRect = PixelRect::new(28, 28, 60, 16);
/// Target stock (223,28,60,16).
pub const TARGET_RECT: PixelRect = PixelRect::new(223, 28, 60, 16);
/// Source icon — center (104,36).
pub const SOURCE_ICON: (i32, i32) = (104, 36);
/// Target icon — center (207,36).
pub const TARGET_ICON: (i32, i32) = (207, 36);

/// Source label (10,52,110,30).
pub const SOURCE_NAME_RECT: PixelRect = PixelRect::new(10, 52, 110, 30);
/// Target label (191,52,110,30).
pub const TARGET_NAME_RECT: PixelRect = PixelRect::new(191, 52, 110, 30);

/// Direction arrow (141,43) 31×16.
pub const ARROW_RECT: PixelRect = PixelRect::new(141, 43, 31, 16);
/// Left button (17,89) 18×17.
pub const LEFT_RECT: PixelRect = PixelRect::new(17, 89, 18, 17);
/// Right button (278,89) 18×17.
pub const RIGHT_RECT: PixelRect = PixelRect::new(278, 89, 18, 17);
/// Bar (44,89,267,105) — 223×16.
pub const SLIDER_RECT: PixelRect = PixelRect::new(44, 89, 223, 16);
/// Bar button — `Rect(ulx-3, uly, width+6, height)` (`scrollbar.cpp:145`).
pub const SLIDER_BUTTON_RECT: PixelRect = PixelRect::new(41, 89, 229, 16);

/// Cancel (82,125) 64×24.
pub const CANCEL_RECT: PixelRect = PixelRect::new(82, 125, 64, 24);
/// Help (147,125) 26×24.
pub const HELP_RECT: PixelRect = PixelRect::new(147, 125, 26, 24);
/// Done (174,125) 57×24.
pub const DONE_RECT: PixelRect = PixelRect::new(174, 125, 57, 24);

/// `Fonts_GoldColor` — `Cancel`/`Done` labels (palette indices).
pub const GOLD: (u8, u8, u8) = (165, 175, 199);

/// X-FER window resources from `MAX.RES` (`TransferMenu`).
#[derive(Debug, Clone)]
pub struct TransferMenuAssets {
    /// `XFERPIC` background 308×162 (big image; drawn with the window system palette,
    /// because `Window` is created with `palette_from_image = false`).
    pub background: Option<BigImage>,
    /// `Cancel` (`XFRCAN_U/D`, 64×24).
    pub cancel: Option<MenuButtonPair>,
    /// `?` (`XFRHLP_U/D`, 26×24) — the picture already has the question mark.
    pub help: Option<MenuButtonPair>,
    /// `Done` (`XFRDNE_U/D`, 57×24).
    pub done: Option<MenuButtonPair>,
    /// `−` (`XFRLFT_U/D`, 18×17).
    pub left: Option<MenuButtonPair>,
    /// `+` (`XFRRGT_U/D`, 18×17).
    pub right: Option<MenuButtonPair>,
    /// `XFRLFARO` 31×16 — the **left** arrow, `total < 0` (`button_arrow`
    /// released: `rest_state = total >= 0` = `false`, the `up` frame is drawn).
    pub arrow_left: Option<SimpleImage>,
    /// `XFRRTARO` 31×16 — the **right** arrow, `total >= 0` (pressed
    /// `rest_state`: `win_set_button_rest_state` draws the `down` frame of the button,
    /// and `down` of `Button(XFRLFARO, XFRRTARO, …)` is `XFRRTARO`).
    pub arrow_right: Option<SimpleImage>,
    /// `SMBRRAW` 285×16 — the raw bar.
    pub bar_raw: Option<SimpleImage>,
    /// `SMBRFUEL` 342×17 — the fuel bar.
    pub bar_fuel: Option<SimpleImage>,
    /// `SMBRGOLD` 342×17 — the gold bar.
    pub bar_gold: Option<SimpleImage>,
}

impl TransferMenuAssets {
    /// Loads resources; a missing part is not an error (the window is not drawn).
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let simple = |name: &str| -> Option<SimpleImage> {
            archive
                .read(name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
        };
        let big = |name: &str| -> Option<BigImage> {
            archive
                .read(name)
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok())
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair::new(simple(up)?, simple(down)?))
        };

        Self {
            background: big("XFERPIC"),
            cancel: pair("XFRCAN_U", "XFRCAN_D"),
            help: pair("XFRHLP_U", "XFRHLP_D"),
            done: pair("XFRDNE_U", "XFRDNE_D"),
            left: pair("XFRLFT_U", "XFRLFT_D"),
            right: pair("XFRRGT_U", "XFRRGT_D"),
            arrow_left: simple("XFRLFARO"),
            arrow_right: simple("XFRRTARO"),
            bar_raw: simple("SMBRRAW"),
            bar_fuel: simple("SMBRFUEL"),
            bar_gold: simple("SMBRGOLD"),
        }
    }

    /// The background and the main buttons are in place.
    pub fn is_ready(&self) -> bool {
        self.background.is_some()
            && self.cancel.is_some()
            && self.done.is_some()
            && self.bar_raw.is_some()
    }

    /// The bar by cargo type (`material_id` from `TransferMenu`).
    pub fn bar(&self, cargo: CargoType) -> Option<&SimpleImage> {
        match cargo {
            CargoType::Raw => self.bar_raw.as_ref(),
            CargoType::Fuel => self.bar_fuel.as_ref(),
            CargoType::Gold => self.bar_gold.as_ref(),
            _ => None,
        }
    }
}

/// What to do after input in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferAction {
    /// Change nothing.
    None,
    /// `Done`/`Enter` — issue `ORDER_TRANSFER`.
    Confirm,
    /// `Cancel`/`Esc` — close without an order (`value = 0`).
    Cancel,
    /// `Help`/`?` — `TRANSFER_SETUP`.
    Help,
}

/// Open transfer window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferMenu {
    /// The unit from whose menu x-fer was opened (`source_unit` in C++ before the possible
    /// swap — the swap happens only inside `ProcessTransfer`).
    pub unit: UnitId,
    /// Target (`unit->GetParent()`).
    pub target: UnitId,
    /// Team (both sides are own).
    pub team: TeamId,
    /// Cargo type (`ResourceManager_GetUnit(...).GetCargoType()`).
    pub cargo: CargoType,
    /// `source_unit_materials`.
    pub source_materials: i32,
    /// `source_unit_capacity`.
    pub source_capacity: i32,
    /// `target_unit_materials`.
    pub target_materials: i32,
    /// `target_unit_capacity`.
    pub target_capacity: i32,
    /// `source_unit_free_capacity` — the minimum of the source's free capacity and the target's stock.
    pub source_free: i32,
    /// `target_unit_free_capacity` — `xfer_give_max`.
    pub give_max: i32,
    /// `xfer_take_max` (negative).
    pub take_max: i32,
    /// `total_materials_transferred` — the current bar value.
    pub amount: i32,
}

impl TransferMenu {
    /// `TransferMenu::TransferMenu` (`transfermenu.cpp:148`).
    ///
    /// `None` if one of the sides is missing, the cargo is not `RAW/FUEL/GOLD`, or the sides
    /// are of different teams (C++ closes such a window immediately in `Run`).
    pub fn new(state: StateRef<'_>, source: UnitId, target: UnitId) -> Option<Self> {
        let source_unit = state.unit(source)?;
        let target_unit = state.unit(target)?;

        if source_unit.team != target_unit.team {
            return None;
        }

        let cargo = cargo_type_of(source_unit.unit_type);
        if !matches!(cargo, CargoType::Raw | CargoType::Fuel | CargoType::Gold) {
            return None;
        }

        let team = source_unit.team;
        let (source_materials, source_capacity) =
            state.transfer_menu_cargo_info( source, target, cargo);
        let (target_materials, target_capacity) =
            state.transfer_menu_cargo_info( target, source, cargo);

        let mut give_max = target_capacity - target_materials;
        give_max = give_max.min(source_materials);

        let mut source_free = source_capacity - source_materials;
        source_free = source_free.min(target_materials);

        Some(Self {
            unit: source,
            target,
            team,
            cargo,
            source_materials,
            source_capacity,
            target_materials,
            target_capacity,
            source_free,
            give_max,
            take_max: -source_free,
            amount: give_max,
        })
    }

    /// Window position in the frame: center of `WINDOW_MAIN_MAP` by inclusive bounds
    /// (`Window(ResourceID, WINDOW_MAIN_MAP)`).
    pub fn layout(area: PixelRect) -> PixelRect {
        let ulx = area.ulx + (area.width - 1 - WINDOW_SIZE.0) / 2;
        let uly = area.uly + (area.height - 1 - WINDOW_SIZE.1) / 2;

        PixelRect::new(ulx, uly, WINDOW_SIZE.0, WINDOW_SIZE.1)
    }

    /// Window rectangle in the 640×480 frame.
    pub fn window_rect(&self) -> PixelRect {
        Self::layout(MAIN_MAP)
    }

    /// How much resource is shown for the source: `source_materials − amount`.
    pub fn source_shown(&self) -> i32 {
        self.source_materials - self.amount
    }

    /// How much resource is shown for the target: `target_materials + amount`.
    pub fn target_shown(&self) -> i32 {
        self.target_materials + self.amount
    }

    /// Value limits: `[max(take_max, zero_offset), min(give_max, free_capacity)]`.
    pub fn value_limits(&self) -> (i32, i32) {
        let zero_offset = -self.target_materials;
        let free_capacity = self.target_capacity - self.target_materials;

        (
            self.take_max.max(zero_offset),
            self.give_max.min(free_capacity),
        )
    }

    /// Sets the value with clipping — like `LimitedScrollbar::ProcessKey`
    /// after a click on the bar.
    pub fn set_amount(&mut self, value: i32) {
        let zero_offset = -self.target_materials;
        let free_capacity = self.target_capacity - self.target_materials;

        let mut value = value.max(zero_offset).min(free_capacity);

        if value > self.give_max {
            value = self.give_max;
        }
        if value < self.take_max {
            value = self.take_max;
        }

        self.amount = value;
    }

    /// Value by a click at a frame point: the `LimitedScrollbar` formula for
    /// a horizontal bar.
    pub fn click_value(&self, window_x: i32) -> i32 {
        let capacity = self.target_capacity - self.target_materials;
        let width = SLIDER_RECT.width;
        let x = window_x - SLIDER_RECT.ulx;
        let zero_offset = -self.target_materials;

        let value = if capacity != 0 {
            (width / 2 + capacity * x) / width + zero_offset
        } else {
            zero_offset
        };

        value
    }

    /// One bar step (`Scrollbar::ProcessKey` with `1000`/`1001`).
    pub fn adjust(&mut self, increase: bool) -> bool {
        let before = self.amount;

        if increase {
            let (_, high) = self.value_limits();
            if self.amount < self.give_max && self.amount < high {
                self.amount += 1;
            }
        } else {
            let (low, _) = self.value_limits();
            if self.amount > self.take_max && self.amount > low {
                self.amount -= 1;
            }
        }

        self.amount != before
    }

    /// Mouse click in frame coordinates.
    pub fn click(&mut self, x: i32, y: i32) -> TransferAction {
        let window = self.window_rect();
        let local_x = x - window.ulx;
        let local_y = y - window.uly;

        if DONE_RECT.contains(local_x, local_y) {
            return TransferAction::Confirm;
        }
        if CANCEL_RECT.contains(local_x, local_y) {
            return TransferAction::Cancel;
        }
        if HELP_RECT.contains(local_x, local_y) {
            return TransferAction::Help;
        }
        if LEFT_RECT.contains(local_x, local_y) {
            self.adjust(false);
            return TransferAction::None;
        }
        if RIGHT_RECT.contains(local_x, local_y) {
            self.adjust(true);
            return TransferAction::None;
        }
        if SLIDER_BUTTON_RECT.contains(local_x, local_y) {
            let value = self.click_value(local_x);
            self.set_amount(value);
            return TransferAction::None;
        }

        TransferAction::None
    }

    /// Key: `TransferMenu::ProcessKey`.
    pub fn key(&mut self, key: Key) -> TransferAction {
        match key {
            Key::Enter => TransferAction::Confirm,
            Key::Escape => {
                self.amount = 0;
                TransferAction::Cancel
            }
            Key::Char('?') | Key::Char('/') | Key::Char('h') | Key::Char('H') => {
                TransferAction::Help
            }
            Key::Left | Key::PageUp | Key::Char('-') | Key::Char('_') => {
                self.adjust(false);
                TransferAction::None
            }
            Key::Right | Key::PageDown | Key::Char('+') | Key::Char('=') => {
                self.adjust(true);
                TransferAction::None
            }
            _ => TransferAction::None,
        }
    }

    /// Input event ([`crate::ui::InputEvent`]).
    pub fn input(&mut self, event: crate::ui::InputEvent) -> TransferAction {
        match event {
            crate::ui::InputEvent::MouseClick(x, y) => self.click(x, y),
            crate::ui::InputEvent::Key(key) => self.key(key),
            _ => TransferAction::None,
        }
    }

    /// Length of the bar fill in pixels (`Scrollbar::RefreshScreen`).
    pub fn bar_fill(&self) -> i32 {
        let zero_offset = -self.target_materials;
        let denominator = self.target_capacity - self.target_materials;

        if denominator == 0 {
            return 0;
        }

        ((self.amount - zero_offset) * SLIDER_RECT.width / denominator)
            .clamp(0, SLIDER_RECT.width)
    }

    /// Which way the direction arrow points: `true` — right (from source to
    /// receiver), `false` — left (from receiver to source).
    ///
    /// Oracle — `TransferMenu::UpdateIndicators` (`transfermenu.cpp:94-96`):
    /// `button_arrow->SetRestState(total_materials_transferred >= 0)`. The button
    /// is created as `Button(XFRLFARO, XFRRTARO, 141, 43)`
    /// (`transfermenu.cpp:239`), i.e. `up = XFRLFARO` (left arrow) and
    /// `down = XFRRTARO` (right arrow). `win_set_button_rest_state`
    /// (`button.c:1022`) with `rest_down != 0` draws `b->down` — so with
    /// `total >= 0` (transfer from source to receiver) the **right** arrow is visible.
    /// Earlier the port drew the opposite (`amount >= 0` → `XFRLFARO`), i.e.
    /// the direction was inverted.
    pub fn arrow_points_right(&self) -> bool {
        self.amount >= 0
    }

    /// Draws the window over the frame.
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &crate::Assets, state: StateRef<'_>) -> u32 {
        let Some(menu_assets) = assets.transfer.as_ref() else {
            return 0;
        };
        let palette = UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return 0;
        };

        let window = self.window_rect();
        let mut painted = 0;

        // `Window(XFERPIC, WINDOW_MAIN_MAP)`: the window background.
        if let Some(background) = menu_assets.background.as_ref() {
            painted += draw_big_image_at(
                frame,
                background,
                &palette,
                (window.ulx, window.uly),
                window,
            );
        }

        let font = GnwFontId::Font5.font();
        let white = palette.rgba(0xFF);
        let black = palette.rgba(0x00);
        let gold_base = palette.rgba(GOLD.0);
        let gold_outline = palette.rgba(GOLD.1);
        let gold_shadow = palette.rgba(GOLD.2);
        let team_lut = crate::mobilize::team_lut(self.team);

        // Unit icons (`ReportStats_DrawListItemIcon`): buildings — 1:4,
        // mobile — 1:2 (the same `I_*` as in `BuildMenu`).
        if let Some(cargo) = assets.cargo.as_ref() {
            for (unit_id, anchor) in [(self.unit, SOURCE_ICON), (self.target, TARGET_ICON)] {
                let Some(unit) = state.unit(unit_id) else {
                    continue;
                };
                let Some(icon) = cargo.icon(unit.unit_type) else {
                    continue;
                };
                let stationary = !unit.unit_type.is_mobile();
                let zoom = if stationary { 16 } else { 32 };

                painted += crate::mobilize::draw_icon(
                    frame,
                    &palette,
                    unit.unit_type,
                    icon,
                    (window.ulx + anchor.0, window.uly + anchor.1),
                    zoom,
                    &team_lut,
                );
            }
        }

        // `SingularName` labels, `COLOR_BLACK`, `FONT5`.
        for (unit_id, rect) in [
            (self.unit, SOURCE_NAME_RECT),
            (self.target, TARGET_NAME_RECT),
        ] {
            if let Some(unit) = state.unit(unit_id) {
                let name = crate::buildmenu::unit_name(unit.unit_type);

                painted += draw_text_hcentered(
                    frame,
                    window.ulx + rect.ulx,
                    window.uly + rect.uly,
                    rect.width,
                    font,
                    name,
                    black,
                );
            }
        }

        // `UpdateIndicators`: the amount and the stocks.
        painted += draw_text_hcentered(
            frame,
            window.ulx + AMOUNT_RECT.ulx,
            window.uly + AMOUNT_RECT.uly,
            AMOUNT_RECT.width,
            font,
            &self.amount.abs().to_string(),
            white,
        );
        painted += draw_text_hcentered(
            frame,
            window.ulx + SOURCE_RECT.ulx,
            window.uly + SOURCE_RECT.uly,
            SOURCE_RECT.width,
            font,
            &self.source_shown().abs().to_string(),
            white,
        );
        painted += draw_text_hcentered(
            frame,
            window.ulx + TARGET_RECT.ulx,
            window.uly + TARGET_RECT.uly,
            TARGET_RECT.width,
            font,
            &self.target_shown().abs().to_string(),
            white,
        );

        // The `SMBR*` bar: `LoadHorizontalBar` — the right `fill` pixels of the row.
        if let Some(bar) = menu_assets.bar(self.cargo) {
            painted += draw_horizontal_bar(
                frame,
                &palette,
                bar,
                window.ulx + SLIDER_RECT.ulx,
                window.uly + SLIDER_RECT.uly,
                self.bar_fill().max(0),
                SLIDER_RECT.height,
            );
        }

        // Buttons: picture + `Fonts_GoldColor` label (`SetCaption(c,1,2)`).
        for (pair, rect, caption) in [
            (&menu_assets.cancel, CANCEL_RECT, "Cancel"),
            (&menu_assets.help, HELP_RECT, ""),
            (&menu_assets.done, DONE_RECT, "Done"),
        ] {
            let Some(pair) = pair.as_ref() else {
                continue;
            };
            let frame_rect =
                PixelRect::new(window.ulx + rect.ulx, window.uly + rect.uly, rect.width, rect.height);

            painted += blit_simple(frame, &palette, pair.image(false), frame_rect.ulx, frame_rect.uly, true);

            if !caption.is_empty() {
                painted += draw_caption(
                    frame,
                    frame_rect,
                    font,
                    caption,
                    gold_base,
                    gold_outline,
                    gold_shadow,
                );
            }
        }

        for (pair, rect) in [(&menu_assets.left, LEFT_RECT), (&menu_assets.right, RIGHT_RECT)] {
            let Some(pair) = pair.as_ref() else {
                continue;
            };
            painted += blit_simple(
                frame,
                &palette,
                pair.image(false),
                window.ulx + rect.ulx,
                window.uly + rect.uly,
                true,
            );
        }

        // Direction arrow: `Button::SetRestState(total_materials_transferred
        // >= 0)` (`transfermenu.cpp:95`). The button is created as
        // `Button(XFRLFARO, XFRRTARO, 141, 43)` (`transfermenu.cpp:239`), i.e.
        // `up = XFRLFARO` (left arrow), `down = XFRRTARO` (right arrow), and
        // `win_set_button_rest_state` (`button.c:1022`) with `rest_down != 0`
        // draws exactly `b->down`. So a positive sum (transfer from
        // source to receiver, source drawn on the left, receiver on the right)
        // shows the **right** arrow, a negative one — the left.
        let arrow = if self.arrow_points_right() {
            menu_assets.arrow_right.as_ref()
        } else {
            menu_assets.arrow_left.as_ref()
        };
        if let Some(arrow) = arrow {
            painted += blit_simple(
                frame,
                &palette,
                arrow,
                window.ulx + ARROW_RECT.ulx,
                window.uly + ARROW_RECT.uly,
                true,
            );
        }

        painted
    }
}

/// `Text_TextBox(..., horizontal_align = true)` without vertical centering:
/// the line at `y`, centered within `width`.
fn draw_text_hcentered(
    frame: &mut RgbaBuffer,
    x: i32,
    y: i32,
    width: i32,
    font: &max_render::text::GnwFont,
    text: &str,
    color: Rgba,
) -> u32 {
    let text_width = font.text_width(text) + 1;
    let spacing = (width - text_width).max(0) / 2;

    draw_text_gnw(frame, x + spacing, y, font, text, color).max(0) as u32
}

/// `Button::SetCaption(caption, 1, 2)`: rectangle `{0,0,w-1,h-2}` with
/// `Fonts_GoldColor` (palette indices 165/175/199).
fn draw_caption(
    frame: &mut RgbaBuffer,
    rect: PixelRect,
    font: &max_render::text::GnwFont,
    caption: &str,
    base: Rgba,
    outline: Rgba,
    shadow: Rgba,
) -> u32 {
    let bounds = PixelRect::new(
        rect.ulx + 1,
        rect.uly + 2,
        (rect.width - 1).max(1),
        (rect.height - 2).max(1),
    );
    let y = bounds.uly + ((bounds.height - font.height()) / 2).max(0);
    let text_width = font.text_width(caption) + 1;
    let x = bounds.ulx + (bounds.width - text_width).max(0) / 2;

    let mut painted = 0;
    if x < frame.width() as i32 && y < frame.height() as i32 {
        // `draw_text_gnw_centered_shadowed` centers vertically too, so
        // we draw the shadow manually: the same cell computed per C++.
        let cell = PixelRect::new(x, y, text_width, font.height());
        painted += draw_text_gnw_centered_shadowed(
            frame,
            cell,
            font,
            caption,
            base,
            outline,
            shadow,
        )
        .max(0) as u32;
    }

    painted
}

/// `LoadHorizontalBar` (`scrollbar.cpp:36`): `height` rows, each — the right
/// `fill` pixels of the texture (`src_x = width - fill + column`), transparent
/// pixels are skipped.
fn draw_horizontal_bar(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    bar: &SimpleImage,
    x: i32,
    y: i32,
    fill: i32,
    height: i32,
) -> u32 {
    let width = i32::from(bar.width);
    let fill = fill.clamp(0, width);
    let mut painted = 0;

    for row in 0..height {
        for column in 0..fill {
            let src_x = (width - fill + column) as u32;
            let src_y = row as u32;

            let Some(index) = bar.pixel(src_x, src_y) else {
                continue;
            };
            if index == bar.transparent {
                continue;
            }

            if frame.set(x + column, y + row, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// `GameManager_TransferCargo` (`game_manager.cpp:4634`) as far as target selection goes.
///
/// * the target is in `ORDER_DISABLE` — `Err` (C++ shows `NCANC0` and a message);
/// * the cargo type matches — the target as is;
/// * the target is a stationary complex building that has a building with the
///   needed cargo — the target is replaced with that building (`GetUnitWithCargoType`);
/// * otherwise `Ok(None)` — C++ silently does nothing.
pub fn resolve_target(
    state: StateRef<'_>,
    source: UnitId,
    clicked: UnitId,
) -> Result<Option<UnitId>, String> {
    let Some(source_unit) = state.unit(source) else {
        return Ok(None);
    };
    let Some(target_unit) = state.unit(clicked) else {
        return Ok(None);
    };

    if source == clicked || target_unit.team != source_unit.team {
        return Ok(None);
    }

    if target_unit.orders == UnitOrder::Disable {
        return Err("x-fer: цель отключена".to_string());
    }

    let cargo = cargo_type_of(source_unit.unit_type);
    if !matches!(cargo, CargoType::Raw | CargoType::Fuel | CargoType::Gold) {
        return Ok(None);
    }

    if cargo_type_of(target_unit.unit_type) == cargo {
        return Ok(Some(clicked));
    }

    if let Some(complex) = target_unit.complex {
        if !target_unit.is_mobile() {
            if let Some(id) =
                state.find_unit_with_cargo_type( target_unit.team, complex, cargo)
            {
                return Ok(Some(id));
            }
        }
    }

    Ok(None)
}

/// `GameManager_TransferCargo` → `unit1->transfer_cargo = cargo_transferred;
/// UnitsManager_SetNewOrder(unit1, ORDER_TRANSFER, ORDER_STATE_INIT)`.
///
/// Packet **08/P3** (`max-brh-docs/architecture/client/protocol.md` §8.4): `parent_unit` —
/// the target, `transfer_cargo` — a signed amount. `prior_order`/`prior_state`
/// are taken from the unit, like `Remote_SendNetPacket_08` after `SetNewOrder`.
pub fn transfer_order(
    state: StateRef<'_>,
    unit_id: UnitId,
    target: UnitId,
    amount: i16,
) -> Option<max_core::input::Command> {
    let unit = state.unit(unit_id)?;
    let data = state.order_data(unit_id);

    Some(max_core::input::Command::Order(
        max_core::input::Packet08Order {
            unit: unit_id,
            payload: max_core::input::OrderPayload::P3(max_core::input::PayloadP3 {
                p2: max_core::input::PayloadP2 {
                    p1: max_core::input::PayloadP1 {
                        order: UnitOrder::Transfer,
                        state: UnitOrderState::Init,
                        prior_order: unit.orders,
                        prior_state: unit.state,
                        disabled_reaction_fire: unit.disabled_reaction_fire,
                    },
                    parent_unit: Some(target),
                    experience: unit.experience,
                },
                move_to: data.move_to,
                fire_on: data.fire_on,
                enemy_unit: None,
                transfer_cargo: amount,
                stealth_dice_roll: unit.stealth_dice_roll,
            }),
        },
    ))
}

/// `UTILITY`: the signed transfer amount from the window state.
pub fn amount_to_transfer(menu: &TransferMenu) -> i16 {
    menu.amount.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// QA check: the target is valid for x-fer (for tests/log).
pub fn can_transfer_between(
    state: StateRef<'_>,
    source: UnitId,
    target: UnitId,
) -> bool {
    resolve_target(state, source, target)
        .ok()
        .flatten()
        .is_some()
}

/// Unit cargo by type (for QA/log).
pub fn unit_storage_of(state: StateRef<'_>, unit_id: UnitId) -> i32 {
    state.unit(unit_id).map_or(0, |unit| i32::from(unit.storage))
}

/// Cargo units in the complex by type — a wrapper for QA lines.
pub fn stored_in_complex(
    state: StateRef<'_>,
    team: TeamId,
    complex: max_core::team::ComplexId,
    cargo: CargoType,
) -> i32 {
    state.complex_stored( team, complex, cargo)
}

// For tests and the future bar "dragging": the icon resource type.

#[cfg(test)]
#[path = "transfermenu_tests.rs"]
mod tests;
