//! Repair shop screen — C++ `RepairShopMenu` (`max-cpp-port/src/repairshopmenu.cpp`).
//!
//! # Why it exists
//!
//! The `activate` item of the command menu opens the repair shop of a container
//! (`UnitsManager_Popup_InitRepairShops` → `UnitsManager_Popup_OnClick_Activate`,
//! `units_manager.cpp:997-1009` → `RepairShopMenu_Menu`). The screen lists the
//! **stored units** in numbered slots and every slot has its own `Activate`
//! button (`RepairShopSlot_OnClick_Activate`, `repairshopmenu.cpp:737-745`), so
//! the player chooses *which* unit leaves the hold. The port used to release the
//! first stored unit automatically (the manual-run defect: "Activate unloads the
//! first one without asking").
//!
//! # Containers
//!
//! `CLNTRANS`/`SEATRANS`/`AIRTRANS` (transports) and `BARRACKS`/`DEPOT`/`HANGAR`/
//! `DOCK` (repair-shop buildings) — `InitKind::RepairShops` in
//! [`crate::unitmenu`]. The slot list is built exactly like
//! `RepairShopMenu::RepairShopMenu` (`:527-538`): every unit whose parent is the
//! container and whose order is `ORDER_IDLE`/`ORDER_STATE_STORE`, in
//! `Access_IsHeldByUnit` order — the mobile lists walked from `Begin()`
//! (`GameState::unit_ids_in_order` walks the same lists in the same order).
//!
//! # Layout (`repairshopmenu.cpp:363-563`)
//!
//! The window is `HANGRFRM` for a `HANGAR`, otherwise `DEPOTFRM` — both are
//! 640×480 big images at `(0,0)` (the header's `ulx/uly` are zero), i.e. the
//! dialogue covers the whole frame like `ALLOCFRM`.
//!
//! | Slots | `ulx` | `uly` | picture | stats |
//! |---|---|---|---|---|
//! | 6 (generic) | `(i % 3) * 155 + 17` | `(i / 3) * 235 + 9` | 128×128 | `(ulx+3, uly+140)-(ulx+125, uly+170)` |
//! | 4 (`HANGAR`) | `(i & 1) * 226 + 18` | `(i / 2) * 235 + 9` | 200×128 | `(ulx+38, uly+140)-(ulx+160, uly+170)` |
//!
//! The `Activate` button of a slot sits at `(stats.ulx − 12, stats.uly + 42)`
//! (`DPTBAY_U/D`, caption `_(2443)` "Activate", caption box 70×21).
//! `Done` is at `(511,372)` (`DPTMNUUP/DN`, `_(27af)`), the page arrows at
//! `(504,426)`/`(530,426)` (`DPTUP_*`/`DPTDN_*`), help at `(584,426)`
//! (`DPTHP_*`). `unit_slot_index` pages by `unit_slots_per_screen`; an arrow is
//! disabled when it cannot move (`UpdateButtons`, `:1008-1025`).
//!
//! # Differences from the oracle
//!
//! * the slot content is drawn as text (`name`, `Hits h/m`, `Ammo a/m` or
//!   `Cargo c/m`) instead of the `ReportStats` rows with `SI_*` icons — the
//!   mechanics (which unit is listed and released) are the same, the decoration
//!   is simplified;
//! * the `Reload`/`Repair`/`Upgrade` (and their `… All`) buttons of the
//!   repair-shop **buildings** (`repairshopmenu.cpp:173-202,474-524`) are not
//!   ported: this screen is the "which stored unit" selector of the manual-run
//!   defect. The oracle's `Order` mechanics for them are a separate feature;
//! * the keyboard (`1..6` — activate a slot, `Up`/`Down` — page, `H`/`?` — help,
//!   `Esc`/`Enter` — Done) is a Rust client extension: in C++ the dialogue is
//!   mouse-only (`Esc`/`KP_ENTER` close it), and headless QA has no mouse.
//!
//! # State
//!
//! Everything here is UI: [`RepairShopMenu`] is not part of the snapshot/CRC.
//! Choosing a slot only *selects* the stored unit / issues the oracle's order
//! (`RepairShopMenu::Activate`), so the simulation changes exactly as in C++.

use max_assets::image::{BigImage, SimpleImage};
use max_core::enums::{UnitOrder, UnitType};
use max_core::unit::UnitId;
use max_core::unit_values::{ATTRIB_AMMO, ATTRIB_HITS, ATTRIB_STORAGE};
use max_core::universe::StateRef;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered, GnwFontId};

use crate::menu::{blit_simple, draw_big_image, draw_image_button, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::Key;

/// Number of slot pictures a `RepairShopMenu` object has
/// (`REPAIRSHOP_SLOT_COUNT`, `repairshopmenu.cpp:43`).
pub const SLOT_COUNT: usize = 6;
/// Slots per screen for a generic shop (`unit_slots_per_screen = 6`,
/// `repairshopmenu.cpp:406`).
pub const SLOTS_PER_SCREEN: usize = 6;
/// Slots per screen for a `HANGAR` (`REPAIRSHOP_SLOT_COUNT - 2`,
/// `repairshopmenu.cpp:382`).
pub const HANGAR_SLOTS_PER_SCREEN: usize = 4;
/// Slot picture for a generic shop — 128×128 (`:418-420`).
pub const SLOT_IMAGE: (i32, i32) = (128, 128);
/// Slot picture for a `HANGAR` — 200×128 (`:390-391`).
pub const HANGAR_SLOT_IMAGE: (i32, i32) = (200, 128);
/// Slot statistics box width (`:421-422`: `ulx+3 .. ulx+125`).
pub const SLOT_STATS_WIDTH: i32 = 122;
/// Slot statistics box height (`:421-422`: `uly+140 .. uly+170`).
pub const SLOT_STATS_HEIGHT: i32 = 30;
/// `Activate` caption box (`rect_init(&unit_slot_button_area, 0, 0, 70, 21)`).
pub const ACTIVATE_CAPTION: (i32, i32) = (70, 21);
/// `Activate` caption — `_(2443)` (`language.json`, en-US).
pub const CAPTION_ACTIVATE: &str = "Activate";
/// `Done` caption — `_(27af)` (`language.json`, en-US).
pub const CAPTION_DONE: &str = "Done";
/// `Done` button — `(511, 372)`, `DPTMNUUP/DN`.
pub const DONE_RECT: PixelRect = PixelRect::new(511, 372, 90, 23);
/// Page up — `(504, 426)`, `DPTUP_UP/DPTUP_DN`.
pub const PAGE_UP_RECT: PixelRect = PixelRect::new(504, 426, 26, 23);
/// Page down — `(530, 426)`, `DPTDN_UP/DPTDN_DN`.
pub const PAGE_DOWN_RECT: PixelRect = PixelRect::new(530, 426, 26, 23);
/// Help — `(584, 426)`, `DPTHP_UP/DPTHP_DN`.
pub const HELP_RECT: PixelRect = PixelRect::new(584, 426, 26, 23);
/// Window size of `DEPOTFRM`/`HANGRFRM` (`ImageBigHeader.width/height`).
pub const WINDOW_SIZE: (i32, i32) = (640, 480);

/// Resources of the repair shop screen (`DEPOTFRM`/`HANGRFRM` + `DPT*`).
#[derive(Debug, Clone)]
pub struct RepairShopAssets {
    /// `DEPOTFRM` 640×480 — the generic shop background.
    pub depot_background: Option<BigImage>,
    /// `HANGRFRM` 640×480 — the `HANGAR` background.
    pub hangar_background: Option<BigImage>,
    /// `DPTBAYUP/DN` — the per-slot `Activate` button.
    pub activate: Option<MenuButtonPair>,
    /// `DPTUP_UP/DN` — page up.
    pub page_up: Option<MenuButtonPair>,
    /// `DPTUP_X` — the disabled (gray) picture of page up.
    pub page_up_disabled: Option<SimpleImage>,
    /// `DPTDN_UP/DN` — page down.
    pub page_down: Option<MenuButtonPair>,
    /// `DPTDN_X` — the disabled (gray) picture of page down.
    pub page_down_disabled: Option<SimpleImage>,
    /// `DPTHP_UP/DN` — help.
    pub help: Option<MenuButtonPair>,
    /// `DPTMNUUP/DN` — `Done`.
    pub done: Option<MenuButtonPair>,
}

impl RepairShopAssets {
    /// Loads the resources; a missing part is not an error (the screen is skipped).
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
            depot_background: big("DEPOTFRM"),
            hangar_background: big("HANGRFRM"),
            activate: pair("DPTBAYUP", "DPTBAYDN"),
            page_up: pair("DPTUP_UP", "DPTUP_DN"),
            page_up_disabled: simple("DPTUP_X"),
            page_down: pair("DPTDN_UP", "DPTDN_DN"),
            page_down_disabled: simple("DPTDN_X"),
            help: pair("DPTHP_UP", "DPTHP_DN"),
            done: pair("DPTMNUUP", "DPTMNUDN"),
        }
    }

    /// The window can be drawn: the background and the slot button are present.
    pub fn is_ready(&self) -> bool {
        self.depot_background.is_some() && self.activate.is_some() && self.done.is_some()
    }

    /// Background by shop type (`HANGR` → `HANGRFRM`, otherwise `DEPOTFRM`).
    pub fn background(&self, hangar: bool) -> Option<&BigImage> {
        if hangar {
            self.hangar_background
                .as_ref()
                .or(self.depot_background.as_ref())
        } else {
            self.depot_background.as_ref()
        }
    }
}

/// What to do after input in the repair shop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairShopAction {
    /// Change nothing (paging happened inside the menu).
    None,
    /// `RepairShopSlot_OnClick_Activate` — release this stored unit.
    Activate(UnitId),
    /// `RepairShopMenu_OnClick_Done`/`Esc` — close the window.
    Done,
    /// `RepairShopMenu_OnClick_Help` — `HelpMenu_Menu("TRANSFER_SETUP")`.
    Help,
}

/// Is the type a repair shop (`UnitsManager_Popup_InitRepairShops`)?
pub fn is_repair_shop(unit_type: UnitType) -> bool {
    matches!(
        unit_type,
        UnitType::AirTransport
            | UnitType::ArmouredPersonnelCarrier
            | UnitType::SeaTransport
            | UnitType::Barracks
            | UnitType::Depot
            | UnitType::Hangar
            | UnitType::Dock
    )
}

/// Is the type one of the two ported slot-screen variants? (for the menu item)
pub fn has_slot_screen(unit_type: UnitType) -> bool {
    is_repair_shop(unit_type)
}

/// `RepairShopMenu` — the slot list of one container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairShopMenu {
    /// The container (`repairshop`).
    pub shop: UnitId,
    /// Stored units in `Access_IsHeldByUnit` order (`units`, `:527-538`).
    pub units: Vec<UnitId>,
    /// First visible slot (`unit_slot_index`).
    pub slot_index: usize,
    /// Visible slots (`unit_slots_per_screen`).
    pub slots_per_screen: usize,
    /// `repairshop->GetUnitType() == HANGAR` (`HANGRFRM` + 4 slots).
    pub is_hangar: bool,
}

impl RepairShopMenu {
    /// Builds the slot list for a container; `None` — not a repair-shop type.
    pub fn new(state: StateRef<'_>, shop: UnitId) -> Option<Self> {
        let unit = state.unit(shop)?;
        if !is_repair_shop(unit.unit_type) {
            return None;
        }

        let is_hangar = unit.unit_type == UnitType::Hangar;
        let units: Vec<UnitId> = state
            .unit_ids_in_order()
            .into_iter()
            .filter(|id| {
                state.unit(*id).is_some_and(|stored| {
                    stored.orders == UnitOrder::Idle && stored.parent_unit == Some(shop)
                })
            })
            .collect();

        Some(Self {
            shop,
            units,
            slot_index: 0,
            slots_per_screen: if is_hangar {
                HANGAR_SLOTS_PER_SCREEN
            } else {
                SLOTS_PER_SCREEN
            },
            is_hangar,
        })
    }

    /// `RepairShopSlot::Draw` picture rectangle of a **screen** slot (`0..slots_per_screen`).
    pub fn slot_image_rect(&self, slot: usize) -> PixelRect {
        let (ulx, uly) = if self.is_hangar {
            ((slot as i32 & 1) * 226 + 18, (slot as i32 / 2) * 235 + 9)
        } else {
            ((slot as i32 % 3) * 155 + 17, (slot as i32 / 3) * 235 + 9)
        };
        let (width, height) = if self.is_hangar {
            HANGAR_SLOT_IMAGE
        } else {
            SLOT_IMAGE
        };

        PixelRect::new(ulx, uly, width, height)
    }

    /// Statistics box of a screen slot (`unit_stats_bounds`).
    pub fn slot_stats_rect(&self, slot: usize) -> PixelRect {
        let image = self.slot_image_rect(slot);
        let offset = if self.is_hangar { 38 } else { 3 };

        PixelRect::new(
            image.ulx + offset,
            image.uly + 140,
            SLOT_STATS_WIDTH,
            SLOT_STATS_HEIGHT,
        )
    }

    /// `Activate` button of a screen slot (`unit_stats_bounds->ulx - 12`,
    /// `unit_stats_bounds->uly + 42`).
    pub fn activate_rect(&self, slot: usize) -> PixelRect {
        let stats = self.slot_stats_rect(slot);

        PixelRect::new(
            stats.ulx - 12,
            stats.uly + 42,
            ACTIVATE_CAPTION.0,
            ACTIVATE_CAPTION.1,
        )
    }

    /// Stored unit shown in a screen slot (relative to `slot_index`).
    pub fn visible_unit(&self, slot: usize) -> Option<UnitId> {
        self.units.get(self.slot_index + slot).copied()
    }

    /// `unit_slot_index + unit_slots_per_screen < units.GetCount()`.
    pub fn can_page_down(&self) -> bool {
        self.slot_index + self.slots_per_screen < self.units.len()
    }

    /// `unit_slot_index > 0`.
    pub fn can_page_up(&self) -> bool {
        self.slot_index > 0
    }

    /// `RepairShopMenu_OnClick_UpArrow`: `slow_index -= slots_per_screen`, clipped at 0.
    pub fn page_up(&mut self) {
        self.slot_index = self.slot_index.saturating_sub(self.slots_per_screen);
    }

    /// `RepairShopMenu_OnClick_DownArrow`: `+= slots_per_screen` if the next page exists.
    pub fn page_down(&mut self) {
        if self.can_page_down() {
            self.slot_index += self.slots_per_screen;
        }
    }

    /// Mouse click in the 640×480 frame.
    pub fn click(&mut self, x: i32, y: i32) -> RepairShopAction {
        if DONE_RECT.contains(x, y) {
            return RepairShopAction::Done;
        }

        if HELP_RECT.contains(x, y) {
            return RepairShopAction::Help;
        }

        if PAGE_UP_RECT.contains(x, y) {
            self.page_up();
            return RepairShopAction::None;
        }

        if PAGE_DOWN_RECT.contains(x, y) {
            self.page_down();
            return RepairShopAction::None;
        }

        for slot in 0..self.slots_per_screen {
            if self.activate_rect(slot).contains(x, y) {
                if let Some(unit) = self.visible_unit(slot) {
                    return RepairShopAction::Activate(unit);
                }
            }
        }

        RepairShopAction::None
    }

    /// Keyboard (Rust client; see the module header).
    pub fn key(&mut self, key: Key) -> RepairShopAction {
        match key {
            Key::Escape | Key::Enter => RepairShopAction::Done,
            Key::Char('h' | 'H' | '?') => RepairShopAction::Help,
            Key::Up | Key::PageUp => {
                self.page_up();
                RepairShopAction::None
            }
            Key::Down | Key::PageDown => {
                self.page_down();
                RepairShopAction::None
            }
            Key::Char(digit @ '1'..='9') => {
                let slot = (digit as usize) - ('1' as usize);

                if slot < self.slots_per_screen {
                    if let Some(unit) = self.visible_unit(slot) {
                        return RepairShopAction::Activate(unit);
                    }
                }

                RepairShopAction::None
            }
            Key::Backspace
            | Key::Char(_)
            | Key::Alt(_)
            | Key::Other
            | Key::Left
            | Key::Right
            | Key::Tab => RepairShopAction::None,
        }
    }

    /// Input event.
    pub fn input(&mut self, event: crate::ui::InputEvent) -> RepairShopAction {
        match event {
            crate::ui::InputEvent::MouseClick(x, y) => self.click(x, y),
            crate::ui::InputEvent::Key(key) => self.key(key),
            _ => RepairShopAction::None,
        }
    }

    /// `RepairShopMenu::Draw`: the background, the visible slots and the buttons.
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        assets: Option<&RepairShopAssets>,
        palette: &UiPalette,
        state: StateRef<'_>,
    ) -> u32 {
        let Some(assets) = assets else {
            return 0;
        };
        let Some(background) = assets.background(self.is_hangar) else {
            return 0;
        };

        let mut painted = draw_big_image(frame, background);
        let font = GnwFontId::Font5.font();

        for slot in 0..self.slots_per_screen {
            let Some(id) = self.visible_unit(slot) else {
                continue;
            };
            let Some(unit) = state.unit(id) else {
                continue;
            };
            let image = self.slot_image_rect(slot);
            let stats = self.slot_stats_rect(slot);

            // `RepairShopSlot::Draw` writes the display name into the picture area
            // (`Text_TextBox(..., 10, 3, width - 20, height - 6, GNW_TEXT_OUTLINE)`).
            let name = crate::unit_names::unit_user_friendly_name(unit.unit_type);
            let name_rect = PixelRect::new(image.ulx + 10, image.uly + 4, image.width - 20, 12);
            painted += draw_text_gnw_centered(
                frame,
                name_rect,
                font,
                name,
                palette.rgba(crate::panels::GREEN),
            )
            .max(0) as u32;

            // `RepairShopSlot::DrawStats`: hits in the upper half, ammo/cargo in the
            // lower half. The `SI_*` icons are not drawn (see "Differences").
            let hits = format!(
                "Hits {}/{}",
                unit.hits,
                unit.base_values.get_attribute(ATTRIB_HITS)
            );
            painted += draw_text_gnw_centered(
                frame,
                PixelRect::new(stats.ulx, stats.uly, stats.width, stats.height / 2),
                font,
                &hits,
                palette.rgba(crate::panels::TEXT),
            )
            .max(0) as u32;

            let second = if unit.base_values.get_attribute(ATTRIB_AMMO) > 0 {
                format!(
                    "Ammo {}/{}",
                    unit.ammo,
                    unit.base_values.get_attribute(ATTRIB_AMMO)
                )
            } else {
                format!(
                    "Cargo {}/{}",
                    unit.storage,
                    unit.base_values.get_attribute(ATTRIB_STORAGE)
                )
            };
            painted += draw_text_gnw_centered(
                frame,
                PixelRect::new(
                    stats.ulx,
                    stats.uly + stats.height / 2,
                    stats.width,
                    stats.height / 2,
                ),
                font,
                &second,
                palette.rgba(crate::panels::TEXT),
            )
            .max(0) as u32;

            // `RepairShopSlot::UpdateButtons`: enabled only when `IsActivateViable`.
            if let Some(pair) = assets.activate.as_ref() {
                if activate_viable(unit) {
                    painted += blit_simple(
                        frame,
                        palette,
                        pair.image(false),
                        self.activate_rect(slot).ulx,
                        self.activate_rect(slot).uly,
                        true,
                    );
                    // The oracle swaps the up picture for `DPTBAYDN`
                    // (`CopyUpDisabled(DPTBAYDN)`, `repairshopmenu.cpp:167`).
                    painted += blit_simple(
                        frame,
                        palette,
                        pair.image(true),
                        self.activate_rect(slot).ulx,
                        self.activate_rect(slot).uly,
                        true,
                    );
                }
            }
        }

        // The `Activate` caption is drawn over every slot button (the picture is gray
        // and small; the centered `GNW_TEXT_FONT_2` label is the C++ `SetCaption`).
        for slot in 0..self.slots_per_screen {
            if self.visible_unit(slot).is_some() {
                let rect = self.activate_rect(slot);
                painted += draw_text_gnw_centered(
                    frame,
                    rect,
                    GnwFontId::Font2.font(),
                    CAPTION_ACTIVATE,
                    palette.rgba(crate::panels::TEXT),
                )
                .max(0) as u32;
            }
        }

        if let Some(pair) = assets.page_up.as_ref() {
            let image = if self.can_page_up() {
                pair.image(false)
            } else {
                assets
                    .page_up_disabled
                    .as_ref()
                    .unwrap_or(pair.image(false))
            };
            painted += blit_simple(
                frame,
                palette,
                image,
                PAGE_UP_RECT.ulx,
                PAGE_UP_RECT.uly,
                true,
            );
        }

        if let Some(pair) = assets.page_down.as_ref() {
            let image = if self.can_page_down() {
                pair.image(false)
            } else {
                assets
                    .page_down_disabled
                    .as_ref()
                    .unwrap_or(pair.image(false))
            };
            painted += blit_simple(
                frame,
                palette,
                image,
                PAGE_DOWN_RECT.ulx,
                PAGE_DOWN_RECT.uly,
                true,
            );
        }

        if let Some(pair) = assets.done.as_ref() {
            painted += draw_image_button(
                frame,
                palette,
                pair.image(false),
                pair.image(true),
                DONE_RECT,
                CAPTION_DONE,
                false,
            );
        }

        if let Some(pair) = assets.help.as_ref() {
            painted += blit_simple(
                frame,
                palette,
                pair.image(false),
                HELP_RECT.ulx,
                HELP_RECT.uly,
                true,
            );
        }

        painted
    }
}

/// `RepairShopMenu::IsActivateViable` (`repairshopmenu.cpp:657-659`):
/// `!(target_unit->flags & MOBILE_AIR_UNIT) || target_unit->speed > 0`.
pub fn activate_viable(unit: &max_core::unit::UnitInfo) -> bool {
    !unit.is_air() || unit.speed > 0
}

#[cfg(test)]
#[path = "repairshopmenu_tests.rs"]
mod tests;
