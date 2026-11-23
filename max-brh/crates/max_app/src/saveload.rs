//! Saving and loading V70 in game — C++ `saveloadmenu.cpp` (P3).
//!
//! # Oracle
//!
//! * `saveload.cpp` — file names (`SaveLoad_GetSaveFileName`: `save<slot>.<ext>`,
//!   the extension depends on the mission category: `dta`/`tra`/`cam`/`hot`/`mlt`/`dmo`/
//!   `sce`/`mps`), the directory (`FilePathGamePref` for custom/hot-seat/multi,
//!   `FilePathGameData` for campaigns) and autosave (`game_manager.cpp:2500`:
//!   slot **10**, enabled by the `SETUP.auto_save` setting, the name — «(Turn N)»);
//! * `saveloadmenu.cpp` — the window: `LOADPIC` background 640×480, title at (229,5)
//!   181×21, ten slots 200×70 (`FILE1_UP`…`FILE9_UP`, `FILE10UP`) in two
//!   columns: `x = 402*(i/5)+16`, `y = 76*(i%5)+44`, up to 10 slots per page,
//!   slot numbers from 1, the page shifts by 10 (`SaveLoadMenu_SaveSlotLimit`
//!   = 991 — the first slot of the last page);
//! * buttons in the bottom row y=438: `Cancel` (`MNUBTN6U`, 514), `Load`
//!   (`MNUBTN4U`, 354), `Save` (`MNUBTN2U`, 132, only in save mode),
//!   pages `MNUUAROU`/`MNUDAROU` (33/63), `Help` (`MNUBTN5U`, 465) and
//!   `Quit to menu` (`MNUBTN3U`, 243);
//! * the save name — `char[30]` with a text edit in the selected slot.
//!
//! # What is implemented and what is not
//!
//! Implemented: the slot grid with preview info (name, turn, planet, teams, number of
//! units), name input (30 bytes as in V70), pages, `Save`/`Load`/`Cancel`,
//! autosave. Not implemented (deliberately, so as not to drag in unrelated screens):
//! `Help` (`MNUBTN5U`) and `Quit to menu` (`MNUBTN3U`) — these are transitions to other
//! menus, not part of saving; the `FILESFLC` window animation.
//!
//! # Network
//!
//! In the original saving in the network is **allowed**: `SaveLoadMenu_Save` broadcasts
//! packet 16 (`Remote_SendNetPacket_16`) and writes the file, while loading is forbidden
//! («Save/Load is not available in a network game», `saveloadmenu.cpp:461`).
//! In our protocol there is no packet 16 (`max-brh-docs/architecture/client/protocol.md`), so
//! the menu **does not open** in a network session and writes to the log that network
//! saving is not supported — otherwise the clients would diverge (see [`crate::App::open_save_menu`]).

use std::path::{Path, PathBuf};

use max_assets::image::{BigImage, SimpleImage};
use max_core::enums::Planet;
use max_core::save::SaveFile;
use max_core::save_write::SaveMeta;
use max_core::team::TeamKind;
use max_core::victory::MissionCategory;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFontId};

use crate::assets::Assets;
use crate::menu::{draw_image_button, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};

/// Screen logical width (the original window).
pub const SCREEN_WIDTH: i32 = 640;
/// Screen logical height.
pub const SCREEN_HEIGHT: i32 = 480;

/// Slots per page (`SaveLoadMenu_SaveSlotsPerPage`).
pub const SLOTS_PER_PAGE: i32 = 10;
/// First slot (`SaveLoadMenu_FirstSaveSlot`).
pub const FIRST_SLOT: i32 = 1;
/// First slot of the last page (`SaveLoadMenu_SaveSlotLimit`).
pub const SLOT_LIMIT: i32 = 991;
/// Last reachable slot (`SaveLoadMenu_LastSaveSlot`).
pub const LAST_SLOT: i32 = SLOT_LIMIT + SLOTS_PER_PAGE - 1;
/// Autosave slot (`game_manager.cpp:2503`).
pub const AUTOSAVE_SLOT: i32 = 10;

/// Title rectangle (`Text_TextBox(…, 229, 5, 181, 21, …)`).
pub const TITLE: PixelRect = PixelRect::new(229, 5, 181, 21);
/// Save name field (V70 `save_name[30]`); in C++ — the slot's text edit.
pub const NAME_FIELD: PixelRect = PixelRect::new(16, 8, 200, 26);
/// Slot grid: two columns of five rows (C++ `402*(i/5)+16`, `76*(i%5)+44`).
pub const SLOT_WIDTH: i32 = 200;
/// Slot height (the `FILE*_UP` picture is 200×70).
pub const SLOT_HEIGHT: i32 = 70;
/// `Save` button (`MNUBTN2U`, 109×40) — only in save mode.
pub const SAVE_BUTTON: PixelRect = PixelRect::new(132, 438, 109, 40);
/// `Cancel` button (`MNUBTN4U`, 109×40): C++ `r_value = 1000` → exit without writing.
pub const CANCEL_BUTTON: PixelRect = PixelRect::new(354, 438, 109, 40);
/// `Load` button (`MNUBTN6U`, 109×40) — available in both modes.
pub const LOAD_BUTTON: PixelRect = PixelRect::new(514, 438, 109, 40);
/// Page up (`MNUUAROU`, 28×29).
pub const PAGE_UP: PixelRect = PixelRect::new(33, 438, 28, 29);
/// Page down (`MNUDAROU`, 28×29).
pub const PAGE_DOWN: PixelRect = PixelRect::new(63, 438, 28, 29);
/// Status line under the buttons.
pub const STATUS: PixelRect = PixelRect::new(16, 414, 600, 18);

/// Slot rectangle on the page (`i` = 0..9).
pub fn slot_rect(index: usize) -> PixelRect {
    let i = index as i32;
    PixelRect::new(402 * (i / 5) + 16, 76 * (i % 5) + 44, SLOT_WIDTH, SLOT_HEIGHT)
}

/// Window mode: save or load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveLoadMode {
    /// `is_saving_allowed = true` — the file can be written and the name edited.
    Save,
    /// Selection of an existing save only.
    Load,
}

impl SaveLoadMode {
    /// Window title (`_(5426)`/`_(63e7)`).
    pub const fn title(self) -> &'static str {
        match self {
            Self::Save => "Save Game",
            Self::Load => "Load Game",
        }
    }
}

/// Slot preview: what C++ shows in `SaveSlot::DrawSaveSlot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotPreview {
    /// Save name (the header `save_name`).
    pub save_name: String,
    /// Turn number.
    pub turn: i32,
    /// Planet.
    pub world: Planet,
    /// How many units are in the save.
    pub units: usize,
    /// «Team names separated by commas» for the preview line.
    pub teams: String,
    /// Format version.
    pub version: u16,
}

/// One page slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    /// Slot number in C++ terms (from 1).
    pub slot: i32,
    /// File name (`save3.hot`).
    pub file_name: String,
    /// Full path.
    pub path: PathBuf,
    /// The file exists and parses.
    pub in_use: bool,
    /// Preview, if the file was read.
    pub preview: Option<SlotPreview>,
}

/// What was pressed in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveLoadAction {
    /// Nothing (redraw).
    None,
    /// Save into the slot with this name.
    Save {
        /// Slot number (from 1).
        slot: i32,
        /// Save name (≤ 29 characters).
        name: String,
    },
    /// Load the slot.
    Load {
        /// Slot number (from 1).
        slot: i32,
    },
    /// Close the window.
    Cancel,
}

/// Save/load screen.
#[derive(Debug, Clone)]
pub struct SaveLoadMenu {
    /// Mode.
    pub mode: SaveLoadMode,
    /// Match category (defines the file extension and directory).
    pub category: MissionCategory,
    /// Save directory.
    pub dir: PathBuf,
    /// First slot of the page.
    pub first_slot: i32,
    /// Slots of the current page (always [`SLOTS_PER_PAGE`]).
    pub slots: Vec<SlotInfo>,
    /// Selected page slot (0..9).
    pub selected: Option<usize>,
    /// Editable name (`char[30]`, as in V70).
    pub name: String,
    /// The name is being edited (in save mode).
    pub editing: bool,
    /// Status/error line.
    pub status: String,
}

impl SaveLoadMenu {
    /// Opens the window on the first page (C++ `SaveLoadMenu_MenuLoop`).
    pub fn new(mode: SaveLoadMode, category: MissionCategory, dir: impl Into<PathBuf>) -> Self {
        let mut menu = Self {
            mode,
            category,
            dir: dir.into(),
            first_slot: FIRST_SLOT,
            slots: Vec::new(),
            selected: None,
            name: String::new(),
            editing: mode == SaveLoadMode::Save,
            status: String::new(),
        };
        menu.reload();
        menu
    }

    /// File extension of the category (`SaveLoad_GetSaveFileName`).
    pub fn extension(category: MissionCategory) -> &'static str {
        match category {
            MissionCategory::Custom => "dta",
            MissionCategory::Training => "tra",
            MissionCategory::Campaign => "cam",
            MissionCategory::HotSeat => "hot",
            MissionCategory::Multiplayer => "mlt",
            MissionCategory::Demo => "dmo",
            MissionCategory::Scenario => "sce",
            MissionCategory::MultiPlayerScenario => "mps",
        }
    }

    /// Slot file name (`save<slot>.<ext>`).
    pub fn file_name(category: MissionCategory, slot: i32) -> String {
        format!("save{slot}.{}", Self::extension(category))
    }

    /// Full path of the slot (lowercase first, like `SaveLoad_GetFilePath`).
    pub fn file_path(dir: &Path, category: MissionCategory, slot: i32) -> PathBuf {
        dir.join(Self::file_name(category, slot))
    }

    /// Re-reads the page slots.
    pub fn reload(&mut self) {
        self.slots.clear();
        for index in 0..SLOTS_PER_PAGE {
            let slot = self.first_slot + index;
            let path = Self::file_path(&self.dir, self.category, slot);
            let mut info = SlotInfo {
                slot,
                file_name: Self::file_name(self.category, slot),
                path: path.clone(),
                in_use: false,
                preview: None,
            };
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(save) = SaveFile::parse(&bytes) {
                    let teams = save
                        .header
                        .team_names
                        .iter()
                        .enumerate()
                        .filter(|(team, name)| {
                            save.header.team_type[*team] != TeamKind::None
                                && !name.iter().all(|byte| *byte == 0)
                        })
                        .map(|(team, _)| save.header.team_name(team))
                        .collect::<Vec<_>>()
                        .join(", ");
                    info.in_use = true;
                    info.preview = Some(SlotPreview {
                        save_name: save.header.save_name(),
                        turn: save.game.turn_counter,
                        world: save.header.world,
                        units: save.units.len(),
                        teams,
                        version: save.header.version,
                    });
                }
            }
            self.slots.push(info);
        }
        if self.selected.is_none() {
            // C++ does not select a slot itself (`save_slot_index = -1`); for the keyboard
            // we choose a useful default: a free slot when saving and
            // the first occupied one when loading.
            self.selected = if self.mode == SaveLoadMode::Save {
                self.slots.iter().position(|slot| !slot.in_use)
            } else {
                self.slots.iter().position(|slot| slot.in_use)
            }
            .or(Some(0));
        }
    }

    /// The selected slot as a whole.
    pub fn selected_slot(&self) -> Option<&SlotInfo> {
        self.selected.and_then(|index| self.slots.get(index))
    }

    /// Changes the page (`GNW_KB_KEY_PAGEUP`/`PAGEDOWN`); `false` — already at the edge.
    pub fn page(&mut self, delta: i32) -> bool {
        if delta < 0 {
            if self.first_slot <= FIRST_SLOT {
                return false;
            }
            self.first_slot = (self.first_slot - SLOTS_PER_PAGE).max(FIRST_SLOT);
        } else {
            if self.first_slot >= SLOT_LIMIT {
                return false;
            }
            self.first_slot = (self.first_slot + SLOTS_PER_PAGE).min(SLOT_LIMIT);
        }
        self.selected = Some(0);
        self.reload();
        true
    }

    /// Selects a slot by page index; when saving, substitutes the name of the
    /// existing save (overwrite), and leaves the name of an empty slot alone.
    pub fn select(&mut self, index: usize) {
        if index >= self.slots.len() {
            return;
        }
        self.selected = Some(index);
        if self.mode == SaveLoadMode::Save {
            if let Some(preview) = self.slots[index].preview.as_ref() {
                self.name = preview.save_name.clone();
            }
        }
    }

    /// The "Save" action for the current slot.
    pub fn save_action(&mut self) -> SaveLoadAction {
        let Some(index) = self.selected else {
            self.status = "Select a save slot first".to_string();
            return SaveLoadAction::None;
        };
        let name = self.name.trim();
        if name.is_empty() {
            self.status = "Enter a name for the save".to_string();
            return SaveLoadAction::None;
        }
        let slot = self.slots[index].slot;
        SaveLoadAction::Save {
            slot,
            name: name.to_string(),
        }
    }

    /// The "Load" action for the current slot.
    pub fn load_action(&mut self) -> SaveLoadAction {
        let Some(index) = self.selected else {
            self.status = "Select a save slot first".to_string();
            return SaveLoadAction::None;
        };
        let slot = &self.slots[index];
        if !slot.in_use {
            self.status = format!("Slot {} is empty", slot.slot);
            return SaveLoadAction::None;
        }
        SaveLoadAction::Load { slot: slot.slot }
    }

    /// Handles a mouse click in frame pixels.
    pub fn click(&mut self, x: i32, y: i32) -> SaveLoadAction {
        if PAGE_UP.contains(x, y) {
            self.page(-1);
            return SaveLoadAction::None;
        }
        if PAGE_DOWN.contains(x, y) {
            self.page(1);
            return SaveLoadAction::None;
        }
        if CANCEL_BUTTON.contains(x, y) {
            return SaveLoadAction::Cancel;
        }
        if LOAD_BUTTON.contains(x, y) {
            return self.load_action();
        }
        if self.mode == SaveLoadMode::Save && SAVE_BUTTON.contains(x, y) {
            return self.save_action();
        }

        for index in 0..self.slots.len() {
            if slot_rect(index).contains(x, y) {
                self.select(index);
                if self.mode == SaveLoadMode::Load {
                    return self.load_action();
                }
                return SaveLoadAction::None;
            }
        }

        SaveLoadAction::None
    }

    /// Handles a key.
    pub fn key(&mut self, key: Key) -> SaveLoadAction {
        match key {
            Key::Escape => return SaveLoadAction::Cancel,
            Key::PageUp => {
                self.page(-1);
            }
            Key::PageDown => {
                self.page(1);
            }
            Key::Up => self.step_selection(-1, 0),
            Key::Down => self.step_selection(1, 0),
            Key::Left => self.step_selection(0, -1),
            Key::Right => self.step_selection(0, 1),
            Key::Tab => self.editing = !self.editing,
            Key::Backspace => {
                if self.mode == SaveLoadMode::Save {
                    self.name.pop();
                }
            }
            Key::Enter => {
                return if self.mode == SaveLoadMode::Load {
                    self.load_action()
                } else {
                    self.save_action()
                }
            }
            Key::Char(character) if self.mode == SaveLoadMode::Save => {
                if self.name.len() < 29 && (character.is_ascii_graphic() || character == ' ') {
                    self.name.push(character);
                }
            }
            _ => {}
        }
        SaveLoadAction::None
    }

    /// Handles an input event as a whole.
    pub fn input(&mut self, event: InputEvent) -> SaveLoadAction {
        match event {
            InputEvent::Key(key) => self.key(key),
            InputEvent::MouseClick(x, y) => self.click(x, y),
            InputEvent::RightClick(_, _) | InputEvent::MouseMove(_, _) => SaveLoadAction::None,
        }
    }

    fn step_selection(&mut self, rows: i32, columns: i32) {
        let Some(current) = self.selected else {
            self.selected = Some(0);
            return;
        };
        let row = (current as i32 % 5 + rows).clamp(0, 4);
        let column = (current as i32 / 5 + columns).clamp(0, 1);
        let index = (column * 5 + row) as usize;
        self.select(index.min(self.slots.len().saturating_sub(1)));
    }

    /// Draws the window over the frame.
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &Assets) -> u32 {
        if frame.width() != SCREEN_WIDTH as u32 || frame.height() != SCREEN_HEIGHT as u32 {
            return 0;
        }

        // The frame under the window is black: in C++ the transparent pixels of `LOADPIC`
        // show the game, but it is simpler (and more stable for golden frames) to fill black.
        frame.fill_rect(
            PixelRect::new(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT),
            max_render::buffer::Rgba::rgb(0, 0, 0),
        );

        let menu_assets = assets.save_load.as_ref();
        let background = menu_assets.and_then(|menu| menu.background.as_ref());
        let mut painted = match background {
            Some(background) => crate::menu::draw_big_image(frame, background),
            None => 0,
        };
        let palette = background
            .map(UiPalette::from_big_image)
            .or_else(|| UiPalette::from_assets(assets))
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return painted;
        };
        let font5 = GnwFontId::Font5.font();
        let font1 = GnwFontId::Font1.font();
        let green = palette.rgba(crate::setup::GREEN);
        let text = palette.rgba(crate::setup::TEXT);
        let yellow = palette.rgba(crate::setup::YELLOW.0);
        let dim = palette.rgba(0x5A);

        painted += draw_text_gnw_centered(frame, TITLE, font5, self.mode.title(), green).max(0)
            as u32;

        // Name field (V70 `char save_name[30]`).
        frame.fill_rect(NAME_FIELD, palette.rgba(0x00));
        let label = if self.name.is_empty() {
            "Name: ".to_string()
        } else {
            format!("Name: {}", self.name)
        };
        painted += draw_text_gnw(
            frame,
            NAME_FIELD.ulx + 3,
            NAME_FIELD.uly + 6,
            font5,
            &label,
            if self.mode == SaveLoadMode::Load {
                dim
            } else {
                yellow
            },
        )
        .max(0) as u32;
        if self.editing && self.mode == SaveLoadMode::Save {
            let width = font5.text_width(&label);
            frame.fill_rect(
                PixelRect::new(NAME_FIELD.ulx + 3 + width + 1, NAME_FIELD.uly + 6, 2, 13),
                green,
            );
            painted += 1;
        }

        // Ten slots.
        for index in 0..self.slots.len() {
            let rect = slot_rect(index);
            let selected = self.selected == Some(index);
            let pair = menu_assets.and_then(|menu| menu.slot_pair(index));
            match pair {
                Some(pair) => {
                    crate::menu::blit_simple(
                        frame,
                        &palette,
                        pair.image(selected),
                        rect.ulx,
                        rect.uly,
                        false,
                    );
                    painted += 1;
                }
                None => {
                    frame.fill_rect(rect, palette.rgba(0x18));
                    frame.fill_rect(
                        PixelRect::new(rect.ulx, rect.uly, rect.width, 1),
                        palette.rgba(0x5A),
                    );
                    painted += 1;
                }
            }

            // Slot number (C++ `Text_TextBox(..., "%d", 0, 0, 40, 70)`).
            painted += draw_text_gnw(
                frame,
                rect.ulx + 6,
                rect.uly + 6,
                font1,
                &self.slots[index].slot.to_string(),
                if selected { green } else { text },
            )
            .max(0) as u32;

            match &self.slots[index].preview {
                Some(preview) => {
                    painted += draw_text_gnw(
                        frame,
                        rect.ulx + 44,
                        rect.uly + 6,
                        font5,
                        &fit_label(&preview.save_name, rect.width - 52, font5),
                        if selected { green } else { text },
                    )
                    .max(0) as u32;
                    painted += draw_text_gnw(
                        frame,
                        rect.ulx + 44,
                        rect.uly + 22,
                        font5,
                        &format!(
                            "Turn {} · {}",
                            preview.turn,
                            crate::ui::planet_name(preview.world)
                        ),
                        if selected { green } else { yellow },
                    )
                    .max(0) as u32;
                    painted += draw_text_gnw(
                        frame,
                        rect.ulx + 44,
                        rect.uly + 38,
                        font5,
                        &format!("{} units", preview.units),
                        dim,
                    )
                    .max(0) as u32;
                    painted += draw_text_gnw(
                        frame,
                        rect.ulx + 44,
                        rect.uly + 52,
                        font1,
                        &fit_label(&preview.teams, rect.width - 52, font1),
                        dim,
                    )
                    .max(0) as u32;
                }
                None => {
                    painted += draw_text_gnw(
                        frame,
                        rect.ulx + 44,
                        rect.uly + 6,
                        font5,
                        "Empty",
                        dim,
                    )
                    .max(0) as u32;
                }
            }
        }

        // Buttons in C++ order (`saveloadmenu.cpp:248-265`): pages on the left,
        // Save (save mode only), Cancel, Load.
        painted += draw_button(
            frame,
            &palette,
            menu_assets.and_then(|menu| menu.page_up.as_ref()),
            PAGE_UP,
            "",
            None,
            false,
        );
        painted += draw_button(
            frame,
            &palette,
            menu_assets.and_then(|menu| menu.page_down.as_ref()),
            PAGE_DOWN,
            "",
            None,
            false,
        );
        if self.mode == SaveLoadMode::Save {
            painted += draw_button(
                frame,
                &palette,
                menu_assets.and_then(|menu| menu.save.as_ref()),
                SAVE_BUTTON,
                "Save",
                None,
                false,
            );
        }
        painted += draw_button(
            frame,
            &palette,
            menu_assets.and_then(|menu| menu.cancel.as_ref()),
            CANCEL_BUTTON,
            "Cancel",
            None,
            false,
        );
        painted += draw_button(
            frame,
            &palette,
            menu_assets.and_then(|menu| menu.action.as_ref()),
            LOAD_BUTTON,
            "Load",
            None,
            false,
        );

        painted += draw_text_gnw(
            frame,
            STATUS.ulx,
            STATUS.uly,
            font5,
            &self.status,
            yellow,
        )
        .max(0) as u32;

        painted
    }
}

/// Save/load window resources from `MAX.RES`.
#[derive(Debug, Clone)]
pub struct SaveLoadAssets {
    /// `LOADPIC` background 640×480.
    pub background: Option<BigImage>,
    /// `FILE1_UP/DN`…`FILE10UP/DN` pairs (index — page slot 0..9).
    pub slots: [Option<MenuButtonPair>; 10],
    /// Page up (`MNUUAROU/D`).
    pub page_up: Option<MenuButtonPair>,
    /// Page down (`MNUDAROU/D`).
    pub page_down: Option<MenuButtonPair>,
    /// `Save` (`MNUBTN2U/D`).
    pub save: Option<MenuButtonPair>,
    /// `Load`/`Save` (`MNUBTN4U/D`).
    pub action: Option<MenuButtonPair>,
    /// `Cancel` (`MNUBTN6U/D`).
    pub cancel: Option<MenuButtonPair>,
}

impl SaveLoadAssets {
    /// Loads resources from the `MAX.RES` archive; a missing part is not an error.
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            let up = SimpleImage::decode(archive.read(up).ok()?).ok()?;
            let down = SimpleImage::decode(archive.read(down).ok()?).ok()?;
            Some(MenuButtonPair::new(up, down))
        };
        let mut slots: [Option<MenuButtonPair>; 10] = Default::default();
        for (index, slot) in slots.iter_mut().enumerate() {
            // C++ `FILE1_UP + i`: the tenth resource has no underscore in its name.
            *slot = if index < 9 {
                pair(
                    &format!("FILE{}_UP", index + 1),
                    &format!("FILE{}_DN", index + 1),
                )
            } else {
                pair("FILE10UP", "FILE10DN")
            };
        }
        Self {
            background: archive
                .read("LOADPIC")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            slots,
            page_up: pair("MNUUAROU", "MNUUAROD"),
            page_down: pair("MNUDAROU", "MNUDAROD"),
            save: pair("MNUBTN2U", "MNUBTN2D"),
            action: pair("MNUBTN4U", "MNUBTN4D"),
            cancel: pair("MNUBTN6U", "MNUBTN6D"),
        }
    }

    /// Slot picture pair of a page.
    pub fn slot_pair(&self, index: usize) -> Option<&MenuButtonPair> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    /// Whether there is a background or slot pictures (otherwise the window is drawn with frames).
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.slots.iter().any(Option::is_some)
    }
}

/// Button: the C++ picture or an honest frame with a label (like `buildmenu`).
fn draw_button(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    pair: Option<&MenuButtonPair>,
    rect: PixelRect,
    label: &str,
    mouse: Option<(i32, i32)>,
    active: bool,
) -> u32 {
    let hovered = mouse.is_some_and(|(x, y)| rect.contains(x, y));

    if let Some(pair) = pair {
        return draw_image_button(
            frame,
            palette,
            pair.image(active),
            pair.image(true),
            rect,
            label,
            hovered,
        );
    }

    frame.fill_rect(rect, palette.rgba(crate::setup::TEXT));
    let mut painted = 1;
    if !label.is_empty() {
        draw_text_gnw_centered(frame, rect, GnwFontId::Font1.font(), label, palette.rgba(crate::setup::GREEN));
        painted += 1;
    }
    painted
}

/// Clipping of a long name by width (like `mobilize::fit_label`).
fn fit_label(text: &str, max_width: i32, font: &max_render::text::GnwFont) -> String {
    if font.text_width(text) <= max_width {
        return text.to_string();
    }
    let mut out = text.to_string();
    while !out.is_empty() && font.text_width(&format!("{out}..")) > max_width {
        out.pop();
    }
    format!("{out}..")
}

// ---------------------------------------------------------------------------
// Integration with the application
// ---------------------------------------------------------------------------

// The save directory belongs to the `AppConfig`, not to the match: it is needed by
// every application type (the network application writes `preferences.ini` too).
impl<D: max_core::universe::Dispatcher> crate::App<D> {
    /// Save directory: `--save-dir`, otherwise the `MAX.RES` directory (`max-original`),
    /// like `FilePathGameData` for campaigns and `FilePathGamePref` for custom.
    pub fn save_dir(&self) -> PathBuf {
        self.config()
            .save_dir
            .clone()
            .or_else(|| {
                self.config()
                    .max_res
                    .parent()
                    .map(std::path::Path::to_path_buf)
            })
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

impl<D: max_core::universe::Dispatcher> crate::App<D> {
    /// Match category for file/slot names.
    pub fn save_category(&self) -> MissionCategory {
        if self.local().is_some_and(|local| local.is_hot_seat()) {
            MissionCategory::HotSeat
        } else {
            self.state_ref().mission_category()
        }
    }

    /// Open save/load window.
    pub fn save_load(&self) -> Option<&SaveLoadMenu> {
        self.save_load.as_ref()
    }

    /// The editable window.
    pub fn save_load_mut(&mut self) -> Option<&mut SaveLoadMenu> {
        self.save_load.as_mut()
    }

    /// Closes the window.
    pub fn close_save_load(&mut self) {
        self.save_load = None;
    }

    /// Opens the save window (`File` button, `Alt+F` in C++).
    ///
    /// In a network session returns `false` and writes the explanation to the log: in
    /// the original saving broadcasts packet 16, which does not exist in our protocol.
    pub fn open_save_menu(&mut self, online: bool) -> bool {
        if online {
            self.hud
                .push_log("Save is not available in a network game (packet 16 is not ported)");
            return false;
        }
        if !self.local_active() {
            self.hud
                .push_log("Save requires a local game (Single Player or Hot Seat)");
            return false;
        }
        let category = self.save_category();
        let dir = self.save_dir();
        let default = self
            .local()
            .map(|local| local.setup.player_name.clone())
            .unwrap_or_else(|| "Save".to_string());
        let mut menu = SaveLoadMenu::new(SaveLoadMode::Save, category, dir);
        menu.name = default.chars().take(29).collect();
        // The keyboard scenario "S → Enter" writes into the first free slot.
        if let Some(index) = menu.slots.iter().position(|slot| !slot.in_use) {
            menu.selected = Some(index);
        }
        self.save_load = Some(menu);
        true
    }

    /// Opens the load window (C++ `Alt+L`).
    ///
    /// In the network loading is forbidden in the original too (`saveloadmenu.cpp:461`).
    pub fn open_load_menu(&mut self, online: bool) -> bool {
        if online {
            self.hud
                .push_log("Load is not available in a network game");
            return false;
        }
        if !self.local_active() {
            self.hud
                .push_log("Load requires a local game (Single Player or Hot Seat)");
            return false;
        }
        let category = self.save_category();
        let dir = self.save_dir();
        self.save_load = Some(SaveLoadMenu::new(SaveLoadMode::Load, category, dir));
        true
    }

    /// Saves the match into a slot (`save<slot>.<ext>`).
    pub fn save_game_to_slot(&mut self, slot: i32, save_name: &str) -> Result<PathBuf, String> {
        let category = self.save_category();
        let path = SaveLoadMenu::file_path(&self.save_dir(), category, slot);
        self.save_v70_to(&path, save_name)?;
        Ok(path)
    }

    /// Writes the V70 save of the current match to the path.
    pub fn save_v70_to(
        &mut self,
        path: impl AsRef<Path>,
        save_name: &str,
    ) -> Result<usize, String> {
        let bytes = self.save_v70_bytes(save_name)?;
        let path = path.as_ref();
        std::fs::write(path, &bytes)
            .map_err(|error| format!("не удалось записать {}: {error}", path.display()))?;
        Ok(bytes.len())
    }

    /// V70 save bytes of the current match (with [`SaveMeta`] updated).
    pub fn save_v70_bytes(&mut self, save_name: &str) -> Result<Vec<u8>, String> {
        self.sync_save_meta(save_name);
        self.state_ref()
            .to_save_bytes()
            .map_err(|error| format!("V70-запись: {error}"))
    }

    /// Updates the "external" fields of the save before writing: name, turn timer,
    /// the player's UI fields and the team message log.
    fn sync_save_meta(&mut self, save_name: &str) {
        let team = usize::from(self.state_ref().player_team().0).min(3);
        let timer_left = self
            .local()
            .map(|local| local.timer_left.ceil().max(0.0) as u16)
            .unwrap_or(0);
        let (center_x, center_y) = self.viewport().center();
        let zoom = self.viewport().zoom();
        let messages: Vec<String> = self.hud().log.lines().to_vec();
        let camera_x = (center_x / 64).clamp(0, 111) as i16;
        let camera_y = (center_y / 64).clamp(0, 111) as i16;
        let existing = self.state_ref().save_meta().message_logs[team].len();
        let zoom_level = zoom.min(u32::from(u16::MAX)) as u16;

        let preferences = self.preferences().to_ini();
        // `SaveLoad_Save` (`saveload.cpp:426,502-511`): the team name —
        // `menu_team_name_setting` (edited by the in-game `OptionsMenu`), the timers —
        // the current `settings["timer"]`/`["endturn"]` (they could have been changed by
        // `Preferences` after the match started).
        let live_names: Vec<(usize, String)> = self
            .local()
            .map(|local| {
                (0..4)
                    .filter(|team| local.setup.team_types[*team] != TeamKind::None)
                    .map(|team| {
                        (
                            team,
                            local.display_name(max_core::TeamId(team as u8)),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (turn_timer, endturn) = (
            self.preferences().turn_timer.max(0),
            self.preferences().endturn.max(0),
        );
        let meta = self.universe.save_meta_mut();
        SaveMeta::set_name(&mut meta.save_name, save_name);
        meta.turn_timer = timer_left;
        meta.turn_timer_time = turn_timer.clamp(0, i32::from(u16::MAX)) as u16;
        meta.endturn_time = endturn.clamp(0, i32::from(u16::MAX)) as u16;
        meta.ini_options.turn_timer = turn_timer;
        meta.ini_options.endturn = endturn;
        for (team, name) in live_names {
            SaveMeta::set_name(&mut meta.team_names[team], &name);
        }
        // `IniPreferences` (7 × `signed int`): the current player settings, as
        // `SaveLoad_Save` (`saveload.cpp:547` — `settings->GetNumericValue`).
        meta.ini_preferences = preferences;
        // Camera/zoom of the player — like `GameManager_GuiSwitchTeam` + `CTInfo`.
        meta.team_ui[team].camera_position = max_core::geo::Point::new(camera_x, camera_y);
        meta.team_ui[team].zoom_level = zoom_level;
        // UI log → the team's `MessageLogList` (V70): only new lines.
        for line in messages.iter().skip(existing.min(messages.len())) {
            let mut text = line.chars().take(120).collect::<String>().into_bytes();
            text.push(0);
            meta.message_logs[team].push(max_core::save::MessageLogEntry {
                text,
                unit_object_index: 0,
                unit: None,
                point: max_core::geo::Point::new(camera_x, camera_y),
                is_alert_message: false,
                id: 0xFFFF,
            });
        }
    }

    /// End-of-turn autosave (C++ `SETUP.auto_save`, slot 10).
    ///
    /// C++ writes it on entering `GameManager_UpdateGui` of the round's first team
    /// (`game_manager.cpp:2500`) — see `local::local_finish_round`.
    pub fn autosave(&mut self) -> Option<PathBuf> {
        if !self.config().autosave || !self.preferences().auto_save || !self.local_active() {
            return None;
        }
        let name = format!("(Turn {})", self.state_ref().turn_counter());
        let path = match self.save_game_to_slot(AUTOSAVE_SLOT, &name) {
            Ok(path) => path,
            Err(error) => {
                self.hud.push_log(format!("Autosave failed: {error}"));
                return None;
            }
        };
        self.hud
            .push_log(format!("Autosave: {}", path.display()));
        Some(path)
    }

}

// Loading a V70 save starts a **local** match (`start_local_game_from_save`), so the
// two loaders stay on `App<LocalDispatcher>` (guardrail 6).
impl crate::App {
    /// Loads a V70 save into the current session: a local match restarts from
    /// the saved turn (C++ `GameManager_LoadGame`), otherwise the state is simply
    /// replaced.
    pub fn load_game_slot(&mut self, slot: i32) -> Result<PathBuf, String> {
        let category = self.save_category();
        let path = SaveLoadMenu::file_path(&self.save_dir(), category, slot);
        self.load_v70_path(&path)?;
        Ok(path)
    }

    /// Loads a V70 file by path.
    pub fn load_v70_path(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .map_err(|error| format!("не удалось прочитать {}: {error}", path.display()))?;
        let setup = self
            .local()
            .map(|local| local.setup.clone())
            .unwrap_or_default();
        self.start_local_game_from_save(&setup, &bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.hud.push_log(format!("Loaded {name}"));
        Ok(())
    }
}

/// Fills [`SaveMeta`] with the settings of a new local match.
///
/// `IniPreferences` defaults — from `SETUP` (as in `SAVE1.CAM`: effects=1,
/// quick_scroll=40, fast_movement=1, enemy_halt=1).
pub fn save_meta_for_local_game(setup: &crate::local::LocalSetup) -> SaveMeta {
    save_meta_for_local_game_with(
        setup,
        &max_core::save::IniPreferences {
            effects: 1,
            click_scroll: 0,
            quick_scroll: 40,
            fast_movement: 1,
            follow_unit: 0,
            auto_select: 0,
            enemy_halt: 1,
        },
    )
}

/// The same, but with the current player settings (`Preferences::to_ini`): they are
/// what goes into `SaveMeta::ini_preferences` of a new match, like `SaveLoad_Save`
/// (`saveload.cpp:547`).
pub fn save_meta_for_local_game_with(
    setup: &crate::local::LocalSetup,
    preferences: &max_core::save::IniPreferences,
) -> SaveMeta {
    let category = if setup.is_hot_seat() {
        MissionCategory::HotSeat
    } else {
        MissionCategory::Custom
    };
    let game_type = match category {
        MissionCategory::HotSeat => max_core::enums::FileType::HotSeat,
        _ => max_core::enums::FileType::Custom,
    };
    let opponent = max_core::enums::Opponent::try_from(setup.opponent)
        .unwrap_or(max_core::enums::Opponent::Average);
    let play_mode = setup.effective_play_mode();
    let mut meta = SaveMeta {
        save_game_type: game_type,
        world: crate::local::planet_for_world(setup.world),
        rng_seed: setup.seed,
        opponent,
        turn_timer_time: setup.timer.min(u32::from(u16::MAX)) as u16,
        endturn_time: setup.endturn.min(u32::from(u16::MAX)) as u16,
        ini_options: max_core::save::IniOptions {
            world: i32::try_from(setup.world.clamp(0, 23)).unwrap_or(0),
            turn_timer: setup.timer.min(i32::MAX as u32) as i32,
            endturn: setup.endturn.min(i32::MAX as u32) as i32,
            start_gold: setup.start_gold.min(i32::MAX as u32) as i32,
            play_mode,
            victory_type: setup.victory_type,
            victory_limit: setup.victory_limit,
            opponent,
            raw_resource: i32::from(setup.raw),
            fuel_resource: i32::from(setup.fuel),
            gold_resource: i32::from(setup.gold),
            alien_derelicts: i32::from(setup.derelicts),
        },
        ini_preferences: *preferences,
        ..SaveMeta::default()
    };

    SaveMeta::set_name(&mut meta.save_name, &setup.player_name);
    for team in 0..4 {
        if setup.team_types[team] == TeamKind::None {
            continue;
        }
        SaveMeta::set_name(&mut meta.team_names[team], &setup.display_name(max_core::TeamId(team as u8)));
    }
    // Our AI always keeps 112×112 maps (`AiPlayer::new`), so they get into the save —
    // unlike the V70 saves of the DOS original.
    meta.ai_has_info_map = [true; 4];
    meta.ai_has_mine_map = [true; 4];
    meta
}
