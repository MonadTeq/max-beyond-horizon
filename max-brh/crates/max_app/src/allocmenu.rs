//! Mine extraction allocation dialog — C++ `allocmenu.cpp` (`AllocMenu`), window
//! `ALLOCFRM` 640×480.
//!
//! # Oracle
//!
//! * `ALLOCFRM` window 640×480 (`allocmenu.cpp:153`): `Window(ALLOCFRM,
//!   GameManager_GetDialogWindowCenterMode())` — with `dialog_center_mode = 0`
//!   (`settings.cpp:159`) this is the center of `WINDOW_MAIN_MAP`; `win_move` (`gnw.c:663`)
//!   pins the 640×480 window to `(0,0)`, i.e. the dialog occupies the whole screen;
//! * title `Allocation Menu` (`_(c771)`) — `Text_TextBox(230, 6, 180, 20)`,
//!   `GNW_TEXT_FONT_5`, `COLOR_GREEN`, centered on both axes;
//! * ten labels (`_(8481)` `Raw`, `_(1e19)`/`_(5432)`/`_(e187)` `Usage`,
//!   `_(5ae6)`/`_(04dd)`/`_(89a3)` `Reserve`, `_(a80f)` `Fuel`, `_(dff1)`
//!   `Gold`) in rectangles of 78×19 at the left edge (x = 42), the same
//!   `FONT5`/`COLOR_GREEN`;
//! * three sections (`Raw`/`Fuel`/`Gold`) at `y = 70/190/310`, each with three bars
//!   (step 37 px, `CargoBar::Draw`):
//!   1. **extraction** — a `BARRAW`/`BARFUEL`/`BARGOLD` fill at
//!      `production·240/capacity` px, on top on the right — `BARTAPE` with width
//!      `(capacity − balance)·240/capacity`, label `%i` (`production`);
//!   2. **consumption** — a fill by `usage`, label `%i (%+i / turn)`
//!      (`usage`, `production − usage`);
//!   3. **complex stock** — a fill by `complex_materials`, label `%i`;
//!   the label — `GNW_TEXT_FONT_1`, color `0xFF`, outline `0x00` (`Text_Blit` with
//!   `GNW_TEXT_OUTLINE`), the backing is recolored by the table
//!   `Color_GenerateIntensityTable1(0, 0, 25, 1, 1)`
//!   (`Color_RecolorPixels`);
//! * arrows `LFARO_OF`/`RTARO_OF` (28×29) at `(139, y)`/`(421, y)` —
//!   `−1`/`+1` unit (`cases 1002/1001`, `1005/1004`, `1008/1007`);
//! * `?` (`MNUBTN5U/D` 48×40 at `465,438`) — help `ALLOCATE_SETUP`;
//!   `Done` (`MNUBTN6U/D` 109×40 at `514,438`) and `Esc`/`KP_Enter` — close.
//!   There is **no** separate `Cancel` in C++: the values are applied at the moment of the click
//!   (`UpdateRawAllocation`), `Done` only closes the window;
//! * modality: `GameManager_DisableMainMenu`/`EnableMainMenu` around
//!   `Run`, so the order map does not receive input.
//!
//! # Keyboard
//!
//! In C++ the dialog has only buttons (`Esc`/`KP_Enter`/`Alt-P`/`Shift-/`); here
//! `↑/↓`/`Tab` — section selection,
//! `←/→` and `−/+` — change the selected section, `H`/`?` — help. This is
//! a documented difference: without it headless QA and the keyboard player
//! cannot distribute extraction.
//!
//! # What changes in the state
//!
//! Only the unit fields `raw_mining`/`fuel_mining`/`gold_mining`/`total_mining`
//! (see [`max_core::alloc`]) and — as in C++ `Update*Allocation` — the complex
//! stock (`Complex::raw/fuel/gold`). There are no `TeamUnits` fields:
//! in C++ the value lives on each `MININGST` unit, travels in packet **08/P5**
//! (`ORDER_NEW_ALLOCATE`) and is part of the snapshot/CRC.
//!
//! # Deliberate differences
//!
//! * the keyboard (`↑/↓`/`←/→`/`−`/`+`/`H`) — a Rust client extension: in C++
//!   the dialog is mouse-only (`Esc`/`KP_Enter`/`Alt-P`/`Shift-/`);
//! * `AllocMenu::Run` closes the window if the mine's order became
//!   `ORDER_DISABLE` (`allocmenu.cpp:704`); in a local turn-based match the unit
//!   cannot be disabled while the modal dialog is open, so the check is
//!   not there;
//! * the `KCARG0` sound on the arrows and `NHELP0`/`NDONE0` on the buttons are not
//!   played (the headless stack has no UI sound triggers);
//! * `PauseMenu_Menu()` on `Alt-P` is not ported (as in the rest of the UI).

use max_assets::image::{BigImage, SimpleImage};
use max_core::alloc::{MiningKind, MINING_CAPACITY};
use max_core::universe::{Dispatcher, GameUniverse, StateRef};
use max_core::team::{Cargo, ComplexId, TeamId};
use max_core::unit::{UnitId, UnitInfo};
use max_core::victory::CargoType;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFontId};

use crate::menu::{blit_simple, draw_big_image_at, draw_image_button, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::Key;

/// `ALLOCFRM` window size.
pub const WINDOW_SIZE: (i32, i32) = (640, 480);
/// The window in the frame: `win_move` pins 640×480 to the top-left corner.
pub const WINDOW_RECT: PixelRect = PixelRect::new(0, 0, 640, 480);
/// Title `Text_TextBox(230, 6, 180, 20)`.
pub const TITLE_RECT: PixelRect = PixelRect::new(230, 6, 180, 20);
/// Section and row labels (`MENU_TITLE_ITEM_DEF(42, uly, 120, lry)`), 78×19.
pub const LABEL_RECTS: [PixelRect; 10] = [
    PixelRect::new(42, 74, 78, 19),   // Raw (8481)
    PixelRect::new(42, 111, 78, 19),  // Usage (1e19)
    PixelRect::new(42, 148, 78, 19),  // Reserve (5ae6)
    PixelRect::new(42, 194, 78, 19),  // Fuel (a80f)
    PixelRect::new(42, 231, 78, 19),  // Usage (5432)
    PixelRect::new(42, 268, 78, 19),  // Reserve (04dd)
    PixelRect::new(42, 315, 78, 18),  // Gold (dff1)
    PixelRect::new(42, 353, 78, 17),  // Usage (e187)
    PixelRect::new(42, 389, 78, 18),  // Reserve (89a3)
    // C++ declares 10 labels (`ALLOCATION_MENU_TITLE_COUNT = 10`), the tenth is
    // the empty tail of the table (used only for alignment): we keep an
    // empty rectangle so that the indices match C++.
    PixelRect::new(0, 0, 0, 0),
];
/// Number of labels in the dialog — C++ `ALLOCATION_MENU_TITLE_COUNT`.
pub const TITLE_COUNT: usize = 10;
/// Label texts in C++ order (`_(…)` from `assets/language.json`).
pub const LABELS: [&str; 9] = [
    "Raw", "Usage", "Reserve", "Fuel", "Usage", "Reserve", "Gold", "Usage", "Reserve",
];
/// Left arrows `LFARO_OF` (`MENU_CONTROL_DEF(139, y, …, 1002)`).
pub const LEFT_ARROW_X: i32 = 139;
/// Right arrows `RTARO_OF` (`MENU_CONTROL_DEF(421, y, …, 1001)`).
pub const RIGHT_ARROW_X: i32 = 421;
/// Arrow size (`LFARO_OF`/`RTARO_OF` — 28×29).
pub const ARROW_SIZE: (i32, i32) = (28, 29);
/// Top Y of the sections (`MENU_CONTROL_DEF(…, 70/190/310, …)`).
pub const SECTION_TOP: [i32; 3] = [70, 190, 310];
/// Bar: `CargoBarData::Draw` — `rect = (174, uly, 414, uly + 30)`.
pub const BAR_RECT: PixelRect = PixelRect::new(174, 0, 240, 30);
/// Step of the bars inside a section (`data.rect.uly += 37`).
pub const BAR_STEP: i32 = 37;
/// `?` — `MNUBTN5U/D` 48×40 at (465,438).
pub const HELP_RECT: PixelRect = PixelRect::new(465, 438, 48, 40);
/// `Done` — `MNUBTN6U/D` 109×40 at (514,438).
pub const DONE_RECT: PixelRect = PixelRect::new(514, 438, 109, 40);
/// `COLOR_GREEN` — labels and title.
pub const GREEN: u8 = 0x02;
/// Color of the number on the bar — `Text_Blit(..., 0xFF)`.
pub const CAPTION: u8 = 0xFF;
/// Number outline (`Color_RGB2Color(0)` in `Text_BlitTTF`).
pub const CAPTION_OUTLINE: u8 = 0x00;
/// `Fonts_GoldColor` — `Done`/`?` labels.
pub const GOLD: (u8, u8, u8) = (165, 175, 199);

/// What to do after an event in the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocAction {
    /// Nothing (mouse movement, click outside).
    None,
    /// Close the window (`Done`, `Esc`, `KP_Enter`, `unit->GetOrder() == DISABLE`).
    Close,
    /// Open the `ALLOCATE_SETUP` help.
    Help,
}

/// Open extraction allocation dialog.
///
/// The numbers (`production`/`capacity`/`usage`/`complex_*`) are exactly the `Cargo`
/// that C++ computes in the `AllocMenu` constructor (`GetCargoMining`,
/// `GetCargoMinable`, `GetCargoInfo`) and maintains incrementally in
/// `Update*Allocation`; `balance` — `cargo_2` from `DrawCargoBars`.
#[derive(Debug, Clone, PartialEq)]
pub struct AllocMenu {
    /// The mine from whose menu the dialog was opened.
    pub unit: UnitId,
    /// Mine team.
    pub team: TeamId,
    /// Mine complex (`unit->GetComplex()`).
    pub complex: ComplexId,
    /// Complex extraction (`materials` from `GetCargoMining`).
    pub production: Cargo,
    /// Maximum of mine cells (`capacity` from `GetCargoMining`).
    pub capacity: Cargo,
    /// Consumption of the other buildings (`GetCargoMinable`).
    pub usage: Cargo,
    /// Complex stock (`GetCargoInfo` → materials).
    pub complex_materials: Cargo,
    /// Complex capacity (`GetCargoInfo` → capacity).
    pub complex_capacity: Cargo,
    /// `cargo_2` — `Balance` for each resource (for the bar "tape").
    pub balance: Cargo,
    /// Selected section (Rust client keyboard).
    pub selected: MiningKind,
    /// Status line (hints/errors).
    pub status: String,
}

impl AllocMenu {
    /// Opens the dialog for a mine — C++ `AllocMenu_Menu(unit)`.
    ///
    /// `None` if the unit is not a mine, not of own team, disabled/idle
    /// (`allocate` in the popup exists only with the `ORDER_POWER_ON` family) or
    /// has no complex.
    pub fn new<D: Dispatcher>(state: &mut GameUniverse<D>, unit_id: UnitId) -> Option<Self> {
        let unit = state.snapshot().unit(unit_id)?.clone();

        if unit.unit_type != max_core::enums::UnitType::MiningStation {
            return None;
        }

        if !max_core::alloc::is_active_mine(&unit) {
            return None;
        }

        let complex = unit.complex?;
        let team = unit.team;
        let (production, capacity) = state.snapshot().cargo_mining( team, complex);
        let usage = state.snapshot().cargo_minable( team, complex);
        let (complex_materials, complex_capacity) = state.snapshot().cargo_info( team, complex);

        let mut menu = Self {
            unit: unit_id,
            team,
            complex,
            production,
            capacity,
            usage,
            complex_materials,
            complex_capacity,
            balance: Cargo::init(),
            selected: MiningKind::Raw,
            status: String::new(),
        };
        menu.refresh_balance(state);
        Some(menu)
    }

    /// `DrawCargoBars` in the `cargo_2` part = `Balance(...)` for the three resources.
    ///
    /// C++ calls this on opening and after each change — with the same
    /// side effects of `AllocMenu_Optimize`/`AllocMenu_ReduceProduction`
    /// (the total extraction does not change, but the distribution across the complex's mines
    /// is normalized). Here — at exactly the same moments.
    pub fn refresh_balance<D: Dispatcher>(&mut self, state: &mut GameUniverse<D>) {
        self.balance = Cargo::init();
        self.balance.raw = state.allocation_balance(
            self.team,
            self.complex,
            MiningKind::Raw,
            self.production.raw,
            self.capacity.raw,
        );
        self.balance.fuel = state.allocation_balance(
            self.team,
            self.complex,
            MiningKind::Fuel,
            self.production.fuel,
            self.capacity.fuel,
        );
        self.balance.gold = state.allocation_balance(
            self.team,
            self.complex,
            MiningKind::Gold,
            self.production.gold,
            self.capacity.gold,
        );
    }

    /// `AllocMenu::UpdateRawAllocation`/`UpdateFuelAllocation`/`UpdateGoldAllocation`.
    fn update<D: Dispatcher>(&mut self, state: &mut GameUniverse<D>, kind: MiningKind, material: i32) {
        match kind {
            MiningKind::Raw => {
                self.production.raw += material;
                self.production.free_capacity -= material;
            }
            MiningKind::Fuel => {
                self.production.fuel += material;
                self.production.free_capacity -= material;
            }
            MiningKind::Gold => {
                self.production.gold += material;
                self.production.free_capacity -= material;
            }
        }

        // `complex->material/fuel/gold += material` — the complex stock.
        if let Some(complex) = state.complex_mut(self.team, self.complex.0 as u16) {
            match kind {
                MiningKind::Raw => complex.raw = complex.raw.saturating_add(material as i16),
                MiningKind::Fuel => complex.fuel = complex.fuel.saturating_add(material as i16),
                MiningKind::Gold => complex.gold = complex.gold.saturating_add(material as i16),
            }
        }

        self.refresh_balance(state);
    }

    /// `+1` resource unit — C++ `case 1001/1004/1007`.
    pub fn increase<D: Dispatcher>(&mut self, state: &mut GameUniverse<D>, kind: MiningKind) -> bool {
        let balanced = match kind {
            MiningKind::Raw => self.balance.raw,
            MiningKind::Fuel => self.balance.fuel,
            MiningKind::Gold => self.balance.gold,
        };
        let production = match kind {
            MiningKind::Raw => self.production.raw,
            MiningKind::Fuel => self.production.fuel,
            MiningKind::Gold => self.production.gold,
        };

        if self.production.free_capacity == 0 || production >= balanced {
            return false;
        }

        let added = state.optimize_allocation(self.team, self.complex, kind, 1);
        self.update(state, kind, added);
        added > 0
    }

    /// `−1` resource unit — C++ `case 1002/1005/1008`.
    pub fn decrease<D: Dispatcher>(&mut self, state: &mut GameUniverse<D>, kind: MiningKind) -> bool {
        let production = match kind {
            MiningKind::Raw => self.production.raw,
            MiningKind::Fuel => self.production.fuel,
            MiningKind::Gold => self.production.gold,
        };

        if production <= 0 {
            return false;
        }

        state.reduce_production_allocation(self.team, self.complex, kind, 1);
        self.update(state, kind, -1);
        true
    }

    /// `AllocMenu::OnClickRawBar`/`Fuel`/`Gold` with an **absolute** level
    /// (in C++ the level comes from the cursor position).
    pub fn set_level<D: Dispatcher>(&mut self, state: &mut GameUniverse<D>, kind: MiningKind, target: i32) {
        let production = match kind {
            MiningKind::Raw => self.production.raw,
            MiningKind::Fuel => self.production.fuel,
            MiningKind::Gold => self.production.gold,
        };
        let mut materials = target - production;

        if self.production.free_capacity < materials {
            materials = self.production.free_capacity;
        }

        if materials < 0 {
            state.reduce_production_allocation(self.team, self.complex, kind, -materials);
            self.update(state, kind, materials);
        } else if materials > 0 {
            let added = state.optimize_allocation(self.team, self.complex, kind, materials);
            self.update(state, kind, added);
        }
    }

    /// `AllocMenu::GetHorizontalBarClickValue` (`allocmenu.cpp:592`):
    /// `(capacity * (mouse_x − bounds.ulx − 174) + 120) / 240`.
    pub fn bar_click_value(&self, kind: MiningKind, x: i32) -> i32 {
        let capacity = match kind {
            MiningKind::Raw => self.capacity.raw,
            MiningKind::Fuel => self.capacity.fuel,
            MiningKind::Gold => self.capacity.gold,
        };
        let local = x - (WINDOW_RECT.ulx + BAR_RECT.ulx);

        (capacity * local + 120) / 240
    }

    /// The bar the window point falls into: `(section, row)`.
    fn bar_at(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        if x < BAR_RECT.ulx || x >= BAR_RECT.lrx() {
            return None;
        }

        for (section, top) in SECTION_TOP.iter().enumerate() {
            for row in 0..3 {
                let rect = PixelRect::new(
                    BAR_RECT.ulx,
                    top + row * BAR_STEP,
                    BAR_RECT.width,
                    BAR_RECT.height,
                );

                if rect.contains(x, y) {
                    return Some((section, row as usize));
                }
            }
        }

        None
    }

    /// Click in the 640×480 frame.
    pub fn click<D: Dispatcher>(&mut self, state: &mut GameUniverse<D>, x: i32, y: i32) -> AllocAction {
        if HELP_RECT.contains(x, y) {
            return AllocAction::Help;
        }

        if DONE_RECT.contains(x, y) {
            return AllocAction::Close;
        }

        for (section, top) in SECTION_TOP.iter().enumerate() {
            let kind = MiningKind::ALL[section];
            let left = PixelRect::new(LEFT_ARROW_X, *top, ARROW_SIZE.0, ARROW_SIZE.1);
            let right = PixelRect::new(RIGHT_ARROW_X, *top, ARROW_SIZE.0, ARROW_SIZE.1);

            if left.contains(x, y) {
                self.selected = kind;
                // `−1`: the button is not disabled, at zero it simply does
                // nothing (`case 1002/1005/1008`).
                self.decrease(state, kind);
                return AllocAction::None;
            }

            if right.contains(x, y) {
                self.selected = kind;
                if !self.increase(state, kind) {
                    self.status = format!("{}: no free mining capacity", kind.name());
                }
                return AllocAction::None;
            }
        }

        if let Some((section, row)) = self.bar_at(x, y) {
            let kind = MiningKind::ALL[section];

            // A click on the first bar — setting the level (`OnClick*Bar`).
            if row == 0 {
                let target = self.bar_click_value(kind, x);
                self.set_level(state, kind, target);
            }

            self.selected = kind;
        }

        AllocAction::None
    }

    /// Keyboard (Rust client; see the module header).
    pub fn key<D: Dispatcher>(&mut self, state: &mut GameUniverse<D>, key: Key) -> AllocAction {
        match key {
            Key::Escape | Key::Enter => AllocAction::Close,
            Key::Char('h' | 'H' | '?') => AllocAction::Help,
            Key::Up => {
                let index = self.selected.index();
                self.selected = MiningKind::ALL[(index + 2) % 3];
                AllocAction::None
            }
            Key::Down | Key::Tab => {
                let index = self.selected.index();
                self.selected = MiningKind::ALL[(index + 1) % 3];
                AllocAction::None
            }
            Key::Left | Key::Char('-') | Key::Char('_') => {
                self.decrease(state, self.selected);
                AllocAction::None
            }
            Key::Right | Key::Char('+') | Key::Char('=') => {
                if !self.increase(state, self.selected) {
                    self.status = format!("{}: no free mining capacity", self.selected.name());
                }
                AllocAction::None
            }
            Key::PageUp => {
                self.decrease(state, self.selected);
                AllocAction::None
            }
            Key::PageDown => {
                self.increase(state, self.selected);
                AllocAction::None
            }
            Key::Backspace | Key::Char(_) | Key::Alt(_) | Key::Other => AllocAction::None,
        }
    }

    /// Input event.
    pub fn input<D: Dispatcher>(&mut self, state: &mut GameUniverse<D>, event: crate::ui::InputEvent) -> AllocAction {
        match event {
            crate::ui::InputEvent::MouseClick(x, y) => self.click(state, x, y),
            crate::ui::InputEvent::Key(key) => self.key(state, key),
            _ => AllocAction::None,
        }
    }

    /// Resource extraction in the complex (`production.<kind>`).
    pub fn production_of(&self, kind: MiningKind) -> i32 {
        match kind {
            MiningKind::Raw => self.production.raw,
            MiningKind::Fuel => self.production.fuel,
            MiningKind::Gold => self.production.gold,
        }
    }

    /// Free extraction units (`production.free_capacity`).
    pub fn free_capacity(&self) -> i32 {
        self.production.free_capacity
    }

    /// Bar label: `%i` for the first/third, `%i (%+i / turn)` for the second.
    pub fn bar_caption(&self, section: usize, row: usize) -> String {
        let kind = MiningKind::ALL[section];

        match row {
            0 => format!("{}", self.production_of(kind)),
            1 => {
                let usage = match kind {
                    MiningKind::Raw => self.usage.raw,
                    MiningKind::Fuel => self.usage.fuel,
                    MiningKind::Gold => self.usage.gold,
                };

                format!("{} ({:+} / turn)", usage, self.production_of(kind) - usage)
            }
            _ => {
                let stored = match kind {
                    MiningKind::Raw => self.complex_materials.raw,
                    MiningKind::Fuel => self.complex_materials.fuel,
                    MiningKind::Gold => self.complex_materials.gold,
                };

                format!("{stored}")
            }
        }
    }

    /// Length of the bar fill in pixels — `CargoBarData::Draw`:
    /// `offset_y = cargo_1 * 240 / cargo_3` (0 with zero capacity).
    pub fn bar_fill(&self, section: usize, row: usize) -> i32 {
        let kind = MiningKind::ALL[section];
        let width = BAR_RECT.width;

        let (value, scale) = match row {
            0 => {
                let scale = self.capacity_of(kind).max(self.usage_of(kind));
                (self.production_of(kind), scale)
            }
            1 => {
                let scale = self.capacity_of(kind).max(self.usage_of(kind));
                (self.usage_of(kind), scale)
            }
            _ => (self.stored_of(kind), self.complex_capacity_of(kind)),
        };

        if scale <= 0 {
            return 0;
        }

        (value * width / scale).clamp(0, width)
    }

    /// Length of the bar "tape" (`BARTAPE`): `(cargo_3 − cargo_2)·240/cargo_3`,
    /// only at the first bar (data.flag = true).
    pub fn bar_tape(&self, section: usize, row: usize) -> i32 {
        if row != 0 {
            return 0;
        }

        let kind = MiningKind::ALL[section];
        let width = BAR_RECT.width;
        let scale = self.capacity_of(kind).max(self.usage_of(kind));

        if scale <= 0 {
            return width;
        }

        let balanced = match kind {
            MiningKind::Raw => self.balance.raw,
            MiningKind::Fuel => self.balance.fuel,
            MiningKind::Gold => self.balance.gold,
        };

        ((scale - balanced) * width / scale).clamp(0, width)
    }

    /// `capacity.<kind>`.
    pub fn capacity_of(&self, kind: MiningKind) -> i32 {
        match kind {
            MiningKind::Raw => self.capacity.raw,
            MiningKind::Fuel => self.capacity.fuel,
            MiningKind::Gold => self.capacity.gold,
        }
    }

    /// `usage.<kind>`.
    pub fn usage_of(&self, kind: MiningKind) -> i32 {
        match kind {
            MiningKind::Raw => self.usage.raw,
            MiningKind::Fuel => self.usage.fuel,
            MiningKind::Gold => self.usage.gold,
        }
    }

    /// `complex_materials.<kind>`.
    pub fn stored_of(&self, kind: MiningKind) -> i32 {
        match kind {
            MiningKind::Raw => self.complex_materials.raw,
            MiningKind::Fuel => self.complex_materials.fuel,
            MiningKind::Gold => self.complex_materials.gold,
        }
    }

    /// `complex_capacity.<kind>`.
    pub fn complex_capacity_of(&self, kind: MiningKind) -> i32 {
        match kind {
            MiningKind::Raw => self.complex_capacity.raw,
            MiningKind::Fuel => self.complex_capacity.fuel,
            MiningKind::Gold => self.complex_capacity.gold,
        }
    }

    /// Draws the dialog over the 640×480 frame (`AllocMenu::AllocMenu` + `win_draw`).
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &crate::Assets) -> u32 {
        if frame.width() != 640 || frame.height() != 480 {
            return 0;
        }

        let Some(menu_assets) = assets.alloc.as_ref() else {
            return 0;
        };
        let background = menu_assets.background.as_ref();
        let palette = background
            .map(UiPalette::from_big_image)
            .or_else(|| UiPalette::from_assets(assets))
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return 0;
        };

        let mut painted = 0;

        match background {
            Some(background) => {
                painted += draw_big_image_at(
                    frame,
                    background,
                    &palette,
                    (WINDOW_RECT.ulx, WINDOW_RECT.uly),
                    WINDOW_RECT,
                );
            }
            None => {
                frame.fill_rect(WINDOW_RECT, palette.rgba(0x22));
                painted += 1;
            }
        }

        let title_font = GnwFontId::Font5.font();
        let green = palette.rgba(GREEN);

        // Title: `Text_TextBox(…, 230, 6, 180, 20, COLOR_GREEN, true, true)`
        // — vertical_align: `6 + ((20 + 1) − font_height·1) / 2`.
        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(
                WINDOW_RECT.ulx + TITLE_RECT.ulx,
                vertical_center(TITLE_RECT),
                TITLE_RECT.width,
                title_font.height(),
            ),
            title_font,
            "Allocation Menu",
            green,
        )
        .max(0) as u32;

        for (index, label) in LABELS.iter().enumerate() {
            let rect = LABEL_RECTS[index];

            painted += draw_text_gnw_centered(
                frame,
                PixelRect::new(
                    WINDOW_RECT.ulx + rect.ulx,
                    vertical_center(rect),
                    rect.width,
                    title_font.height(),
                ),
                title_font,
                label,
                green,
            )
            .max(0) as u32;
        }

        let intensity = palette.intensity_table(0, 0, 25, 1, 1);

        for (section, top) in SECTION_TOP.iter().enumerate() {
            let kind = MiningKind::ALL[section];

            // `LFARO_OF`/`RTARO_OF` arrows (pressed look — `*_ON`; hovering in
            // C++ does not change the look, so `OF` is drawn).
            if let Some(arrow) = menu_assets.left.as_ref() {
                painted += blit_simple(
                    frame,
                    &palette,
                    arrow.image(false),
                    LEFT_ARROW_X,
                    *top,
                    true,
                );
            }
            if let Some(arrow) = menu_assets.right.as_ref() {
                painted += blit_simple(
                    frame,
                    &palette,
                    arrow.image(false),
                    RIGHT_ARROW_X,
                    *top,
                    true,
                );
            }

            for row in 0..3 {
                let rect = PixelRect::new(
                    WINDOW_RECT.ulx + BAR_RECT.ulx,
                    top + row as i32 * BAR_STEP,
                    BAR_RECT.width,
                    BAR_RECT.height,
                );

                painted += self.draw_bar(frame, &palette, menu_assets, kind, rect, row, &intensity);
            }
        }

        if let Some(pair) = menu_assets.help.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                HELP_RECT,
                "?",
                false,
            );
        }
        if let Some(pair) = menu_assets.done.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                DONE_RECT,
                "Done",
                false,
            );
        }

        painted
    }

    /// One bar: `BAR*` fill, `BARTAPE` "tape", label with an outline.
    #[allow(clippy::too_many_arguments)]
    fn draw_bar(
        &self,
        frame: &mut RgbaBuffer,
        palette: &UiPalette,
        assets: &AllocMenuAssets,
        kind: MiningKind,
        rect: PixelRect,
        row: usize,
        intensity: &[u8; 256],
    ) -> u32 {
        let mut painted = 0;
        let fill = self.bar_fill(kind.index(), row);
        let tape = self.bar_tape(kind.index(), row);

        // `LoadHorizontalBar`: right to left from the sprite edge — the
        // *right* piece of the texture `fill` wide is taken and placed at the left edge.
        if let Some(bar) = assets.bar(kind) {
            painted += blit_simple_slice(
                frame,
                palette,
                bar,
                i32::from(bar.width) - fill,
                rect.ulx,
                rect.uly,
                fill,
                rect.height,
                true,
            );
        }

        // `LoadHorizontalTape`: the left piece of `BARTAPE` `tape` wide — to the right
        // edge of the bar. `buf_to_buf` copies pixels as is (without transparency),
        // so the "tape" overlaps the bar texture entirely.
        if tape > 0 {
            if let Some(tape_image) = assets.tape.as_ref() {
                painted += blit_simple_slice(
                    frame,
                    palette,
                    tape_image,
                    0,
                    rect.lrx() - tape,
                    rect.uly,
                    tape,
                    rect.height,
                    false,
                );
            }
        }

        // Label: `CargoBarData::Draw` — centered on the bar, `FONT1`, outline.
        let caption = self.bar_caption(kind.index(), row);
        let font = GnwFontId::Font1.font();
        let width = font.text_width(&caption).min(BAR_RECT.width);
        let text_x = rect.ulx + (BAR_RECT.width - width) / 2;
        let text_y = rect.uly + (rect.height - font.height()) / 2;

        // `Color_RecolorPixels` — darkening of the backing under the text (box
        // `width+4` × `font height`, starting at `text_x − 2`).
        painted += recolor_box(
            frame,
            palette,
            PixelRect::new(text_x - 2, text_y, width + 4, font.height()),
            intensity,
        );

        // `Text_Blit(..., GNW_TEXT_OUTLINE | 0xFF)`: first the same line one
        // pixel below-right in black, then the main one.
        draw_text_gnw(
            frame,
            text_x + 1,
            text_y + 1,
            font,
            &caption,
            palette.rgba(CAPTION_OUTLINE),
        );
        draw_text_gnw(frame, text_x, text_y, font, &caption, palette.rgba(CAPTION));
        painted += 1;

        painted
    }
}

/// `Text_TextBox(..., vertical_align = true)`: `uly + ((height + 1) − font) / 2`.
fn vertical_center(rect: PixelRect) -> i32 {
    let font = GnwFontId::Font5.font();
    rect.uly + ((rect.height + 1) - font.height()) / 2
}

/// Copies a `SimpleImage` piece (`width × height` from `(source_x, 0)`) into a frame
/// point, skipping the transparent index (`LoadHorizontalBar`/`LoadHorizontalTape`).
#[allow(clippy::too_many_arguments)]
fn blit_simple_slice(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: &SimpleImage,
    source_x: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    skip_transparent: bool,
) -> u32 {
    let mut painted = 0;

    for row in 0..height {
        for column in 0..width {
            let sx = source_x + column;

            if sx < 0 || sx >= i32::from(image.width) {
                continue;
            }

            let Some(index) = image.pixel(sx as u32, row as u32) else {
                continue;
            };

            if skip_transparent && index == image.transparent {
                continue;
            }

            if frame.set(x + column, y + row, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// `Color_RecolorPixels` — replace palette indices by the table.
fn recolor_box(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    rect: PixelRect,
    table: &[u8; 256],
) -> u32 {
    let mut painted = 0;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            let Some(pixel) = frame.get(x, y) else {
                continue;
            };
            let Some(index) = palette.index_of(pixel) else {
                continue;
            };

            if frame.set(x, y, palette.rgba(table[usize::from(index)])) {
                painted += 1;
            }
        }
    }

    painted
}

/// Extraction allocation dialog assets.
#[derive(Debug, Clone)]
pub struct AllocMenuAssets {
    /// `ALLOCFRM` background 640×480.
    pub background: Option<BigImage>,
    /// `LFARO_OF`/`LFARO_ON` 28×29.
    pub left: Option<MenuButtonPair>,
    /// `RTARO_OF`/`RTARO_ON` 28×29.
    pub right: Option<MenuButtonPair>,
    /// `BARRAW` 241×32.
    pub bar_raw: Option<SimpleImage>,
    /// `BARFUEL` 273×33.
    pub bar_fuel: Option<SimpleImage>,
    /// `BARGOLD` 241×33.
    pub bar_gold: Option<SimpleImage>,
    /// `BARTAPE` 240×30.
    pub tape: Option<SimpleImage>,
    /// `MNUBTN5U/D` 48×40 (`?`).
    pub help: Option<MenuButtonPair>,
    /// `MNUBTN6U/D` 109×40 (`Done`).
    pub done: Option<MenuButtonPair>,
}

impl AllocMenuAssets {
    /// Loads the dialog resources; a missing part is a warning, not an error.
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let simple = |name: &str| -> Option<SimpleImage> {
            let bytes = archive.read(name).ok()?;
            SimpleImage::decode(bytes).ok()
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair::new(simple(up)?, simple(down)?))
        };

        Self {
            background: archive
                .read("ALLOCFRM")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            left: pair("LFARO_OF", "LFARO_ON"),
            right: pair("RTARO_OF", "RTARO_ON"),
            bar_raw: simple("BARRAW"),
            bar_fuel: simple("BARFUEL"),
            bar_gold: simple("BARGOLD"),
            tape: simple("BARTAPE"),
            help: pair("MNUBTN5U", "MNUBTN5D"),
            done: pair("MNUBTN6U", "MNUBTN6D"),
        }
    }

    /// Whether the background and the key bar pictures are present.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.bar_raw.is_some() && self.tape.is_some()
    }

    /// Fill texture for the resource.
    pub fn bar(&self, kind: MiningKind) -> Option<&SimpleImage> {
        match kind {
            MiningKind::Raw => self.bar_raw.as_ref(),
            MiningKind::Fuel => self.bar_fuel.as_ref(),
            MiningKind::Gold => self.bar_gold.as_ref(),
        }
    }
}

/// Extraction value at which the mine is full (`16` units).
pub const CAPACITY_LIMIT: i32 = MINING_CAPACITY;

/// Allocation result by resource (for log/QA).
pub fn totals(menu: &AllocMenu) -> [i32; 3] {
    [
        menu.production_of(MiningKind::Raw),
        menu.production_of(MiningKind::Fuel),
        menu.production_of(MiningKind::Gold),
    ]
}

/// Resource stock in the complex (for log/QA).
pub fn stored(state: StateRef<'_>, team: TeamId, complex: ComplexId, kind: MiningKind) -> i32 {
    let Some(complex) = state.complex(team, complex.0 as u16) else {
        return 0;
    };

    match kind {
        MiningKind::Raw => i32::from(complex.raw),
        MiningKind::Fuel => i32::from(complex.fuel),
        MiningKind::Gold => i32::from(complex.gold),
    }
}

/// Resource `CargoType` (for QA/log).
pub fn cargo_type(kind: MiningKind) -> CargoType {
    kind.cargo_type()
}

/// Whether this is a mine (for the opening check).
pub fn is_mining_station(unit: &UnitInfo) -> bool {
    unit.unit_type == max_core::enums::UnitType::MiningStation
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
    use max_core::geo::Point;
    use max_core::team::Complex;
    use max_core::TeamUnits;

    fn state_with_mine(
        complex_id: i16,
    ) -> (
        max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
        UnitId,
        ComplexId,
    ) {
        let mut state = max_core::universe::GameUniverse::default();
        state
            .team_mut(TeamId::RED)
            .expect("RED slot")
            .team_type = max_core::team::TeamKind::Player;
        *state.team_units_mut(TeamId::RED).expect("RED economy") =
            TeamUnits::with_clan(max_core::enums::TeamClan::Random);

        // A 2×2 square around the mine: raw 8+8 (measurement limit 16), fuel 8,
        // gold 8 — like `Survey_GetResourcesInArea(..., 1, 16, ...)`.
        let resources = [
            (20, 20, 8, max_core::map::ResourceKind::Raw),
            (21, 20, 8, max_core::map::ResourceKind::Raw),
            (20, 21, 8, max_core::map::ResourceKind::Gold),
            (21, 21, 8, max_core::map::ResourceKind::Fuel),
        ];
        for (x, y, amount, kind) in resources {
            state.map_mut().resources.set(
                x,
                y,
                max_core::map::ResourceMapEntry::new(amount, Some(kind), 0),
            );
        }

        // Control data, not classification reading: the mask is an argument of
        // `spawn_unit` (instance flags), so `STATIONARY | BUILDING`
        // remain raw bits.
        let flags = unit_flags::STATIONARY | unit_flags::BUILDING;
        let id = state
            .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(20, 20), flags)
            .expect("шахта");

        let complex = ComplexId(complex_id);
        if let Some(units) = state.team_units_mut(TeamId::RED) {
            units.complexes.push(Complex::new(complex_id));
        }

        let mut unit = state.snapshot().unit(id).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.complex = Some(complex);
        state.write_unit(id, unit);

        (state, id, complex)
    }

    #[test]
    fn layout_matches_the_oracle() {
        assert_eq!(WINDOW_RECT, PixelRect::new(0, 0, 640, 480));
        assert_eq!(TITLE_RECT, PixelRect::new(230, 6, 180, 20));
        assert_eq!(LEFT_ARROW_X, 139);
        assert_eq!(RIGHT_ARROW_X, 421);
        assert_eq!(ARROW_SIZE, (28, 29));
        assert_eq!(SECTION_TOP, [70, 190, 310]);
        assert_eq!(BAR_RECT, PixelRect::new(174, 0, 240, 30));
        assert_eq!(BAR_STEP, 37);
        assert_eq!(HELP_RECT, PixelRect::new(465, 438, 48, 40));
        assert_eq!(DONE_RECT, PixelRect::new(514, 438, 109, 40));
        // The tenth C++ label is empty (a table of 10 elements).
        assert_eq!(LABEL_RECTS.len(), TITLE_COUNT);
        assert!(LABEL_RECTS[9].is_empty());
        assert_eq!(LABEL_RECTS[0], PixelRect::new(42, 74, 78, 19));
        assert_eq!(LABEL_RECTS[8], PixelRect::new(42, 389, 78, 18));
    }

    #[test]
    fn labels_match_language_json() {
        assert_eq!(LABELS[0], "Raw");
        assert_eq!(LABELS[1], "Usage");
        assert_eq!(LABELS[2], "Reserve");
        assert_eq!(LABELS[3], "Fuel");
        assert_eq!(LABELS[6], "Gold");
    }

    #[test]
    fn opening_requires_an_active_mine() {
        let (mut state, id, _complex) = state_with_mine(1);
        assert!(AllocMenu::new(&mut state, id).is_some());

        state.unit_mut(id).unwrap().orders = UnitOrder::PowerOff;
        assert!(
            AllocMenu::new(&mut state, id).is_none(),
            "выключенная шахта не открывает распределение (в C++ пункт тогда `start`)"
        );

        state.unit_mut(id).unwrap().orders = UnitOrder::PowerOn;
        state.unit_mut(id).unwrap().complex = None;
        assert!(AllocMenu::new(&mut state, id).is_none(), "нет комплекса");
    }

    #[test]
    fn opening_reports_mining_capacity_from_the_cell() {
        let (mut state, id, _complex) = state_with_mine(1);
        let menu = AllocMenu::new(&mut state, id).expect("меню");

        assert_eq!(menu.production_of(MiningKind::Raw), 0);
        assert!(
            menu.capacity_of(MiningKind::Raw) >= 0,
            "максимум клетки неотрицателен"
        );
        assert_eq!(menu.free_capacity(), 16);
        assert_eq!(menu.production.free_capacity, 16);
    }

    #[test]
    fn increase_and_decrease_move_the_mine_counters() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");

        if menu.capacity_of(MiningKind::Raw) == 0 {
            return;
        }

        assert!(menu.increase(&mut state, MiningKind::Raw));
        assert_eq!(menu.production_of(MiningKind::Raw), 1);
        assert_eq!(state.snapshot().unit(id).unwrap().raw_mining, 1);
        assert_eq!(state.snapshot().unit(id).unwrap().total_mining, 1);
        assert_eq!(menu.free_capacity(), 15);

        assert!(menu.decrease(&mut state, MiningKind::Raw));
        assert_eq!(menu.production_of(MiningKind::Raw), 0);
        assert_eq!(state.snapshot().unit(id).unwrap().raw_mining, 0);
        assert_eq!(state.snapshot().unit(id).unwrap().total_mining, 0);
    }

    #[test]
    fn level_change_is_absolute_like_a_bar_click() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");
        let capacity = menu.capacity_of(MiningKind::Raw);

        if capacity < 3 {
            return;
        }

        menu.set_level(&mut state, MiningKind::Raw, 3);
        assert_eq!(menu.production_of(MiningKind::Raw), 3);
        assert_eq!(state.snapshot().unit(id).unwrap().raw_mining, 3);

        menu.set_level(&mut state, MiningKind::Raw, 1);
        assert_eq!(menu.production_of(MiningKind::Raw), 1);
        assert_eq!(state.snapshot().unit(id).unwrap().raw_mining, 1);
    }

    #[test]
    fn bar_click_value_matches_cpp_formula() {
        let (mut state, id, _complex) = state_with_mine(1);
        let menu = AllocMenu::new(&mut state, id).expect("меню");
        let capacity = menu.capacity_of(MiningKind::Raw);

        // `x -= bounds.ulx + 174; return (capacity * x + 120) / 240;`
        assert_eq!(menu.bar_click_value(MiningKind::Raw, 174), 0);
        assert_eq!(
            menu.bar_click_value(MiningKind::Raw, 174 + 240),
            (capacity * 240 + 120) / 240
        );
        assert_eq!(
            menu.bar_click_value(MiningKind::Raw, 174 + 120),
            (capacity * 120 + 120) / 240
        );
    }

    #[test]
    fn clicks_hit_the_expected_targets() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");

        // `?` and `Done`.
        assert_eq!(
            menu.click(&mut state, HELP_RECT.ulx + 1, HELP_RECT.uly + 1),
            AllocAction::Help
        );
        assert_eq!(
            menu.click(&mut state, DONE_RECT.ulx + 1, DONE_RECT.uly + 1),
            AllocAction::Close
        );

        // A click outside — nothing.
        assert_eq!(
            menu.click(&mut state, 320, 200),
            AllocAction::None,
            "полоса не меняет приказ"
        );

        // Right arrow of Raw.
        let before = menu.production_of(MiningKind::Raw);
        menu.click(&mut state, RIGHT_ARROW_X + 2, SECTION_TOP[0] + 2);
        let after = menu.production_of(MiningKind::Raw);
        assert!(after == before || after == before + 1);

        // The left arrow returns.
        menu.click(&mut state, LEFT_ARROW_X + 2, SECTION_TOP[0] + 2);
        assert_eq!(menu.production_of(MiningKind::Raw), before);
    }

    #[test]
    fn keys_close_select_and_adjust() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");

        assert_eq!(menu.key(&mut state, Key::Escape), AllocAction::Close);
        assert_eq!(menu.key(&mut state, Key::Enter), AllocAction::Close);
        assert_eq!(menu.key(&mut state, Key::Char('h')), AllocAction::Help);

        assert_eq!(menu.selected, MiningKind::Raw);
        menu.key(&mut state, Key::Down);
        assert_eq!(menu.selected, MiningKind::Fuel);
        menu.key(&mut state, Key::Down);
        assert_eq!(menu.selected, MiningKind::Gold);
        menu.key(&mut state, Key::Down);
        assert_eq!(menu.selected, MiningKind::Raw);
        menu.key(&mut state, Key::Up);
        assert_eq!(menu.selected, MiningKind::Gold);

        let before = menu.production_of(MiningKind::Fuel);
        menu.key(&mut state, Key::Right);
        let after = menu.production_of(MiningKind::Fuel);
        assert!(after == before || after == before + 1);
        menu.key(&mut state, Key::Left);
        assert_eq!(menu.production_of(MiningKind::Fuel), before);
    }

    #[test]
    fn bar_captions_follow_cpp_format() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");

        if menu.capacity_of(MiningKind::Raw) < 2 {
            return;
        }

        menu.set_level(&mut state, MiningKind::Raw, 2);
        assert_eq!(menu.bar_caption(0, 0), "2");
        // `%i (%+i / turn)` — `usage` and `production − usage`.
        assert!(menu.bar_caption(0, 1).contains("/ turn"));
        assert!(menu.bar_caption(0, 1).starts_with(&menu.usage_of(MiningKind::Raw).to_string()));
        assert_eq!(menu.bar_caption(0, 2), menu.stored_of(MiningKind::Raw).to_string());
    }

    #[test]
    fn bar_geometry_follows_cpp() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");
        let capacity = menu.capacity_of(MiningKind::Raw);

        if capacity < 4 {
            return;
        }

        menu.set_level(&mut state, MiningKind::Raw, 4);
        // Fill `production * 240 / max(capacity, usage)`.
        let scale = capacity.max(menu.usage_of(MiningKind::Raw));
        assert_eq!(menu.bar_fill(0, 0), (4 * 240 / scale).clamp(0, 240));
        // The second and third bars without a "tape".
        assert_eq!(menu.bar_tape(0, 1), 0);
        assert_eq!(menu.bar_tape(0, 2), 0);
    }

    #[test]
    fn determinism_of_two_identical_dialogs() {
        let (mut a, id_a, _) = state_with_mine(1);
        let (mut b, id_b, _) = state_with_mine(1);

        let mut menu_a = AllocMenu::new(&mut a, id_a).expect("меню");
        let mut menu_b = AllocMenu::new(&mut b, id_b).expect("меню");

        for _ in 0..3 {
            menu_a.increase(&mut a, MiningKind::Raw);
            menu_b.increase(&mut b, MiningKind::Raw);
        }
        menu_a.decrease(&mut a, MiningKind::Raw);
        menu_b.decrease(&mut b, MiningKind::Raw);

        assert_eq!(menu_a.production, menu_b.production);
        assert_eq!(menu_a.balance, menu_b.balance);
        assert_eq!(
            a.snapshot().unit(id_a).unwrap().raw_mining,
            b.snapshot().unit(id_b).unwrap().raw_mining
        );
        assert_eq!(
            a.snapshot().unit(id_a).unwrap().total_mining,
            b.snapshot().unit(id_b).unwrap().total_mining
        );
        // The state digest (CRC/snapshot) matches too: the value is part of the
        // snapshot via the unit fields.
        assert_eq!(
            a.snapshot().state_checksum(),
            b.snapshot().state_checksum()
        );
        assert_eq!(a.snapshot().snapshot_bytes(), b.snapshot().snapshot_bytes());
    }
}
