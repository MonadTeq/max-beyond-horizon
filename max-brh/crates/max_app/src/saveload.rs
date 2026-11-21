//! Сохранение и загрузка V70 в игре — C++ `saveloadmenu.cpp` (P3).
//!
//! # Оракул
//!
//! * `saveload.cpp` — имена файлов (`SaveLoad_GetSaveFileName`: `save<slot>.<ext>`,
//!   расширение зависит от категории миссии: `dta`/`tra`/`cam`/`hot`/`mlt`/`dmo`/
//!   `sce`/`mps`), каталог (`FilePathGamePref` для custom/hot-seat/multi,
//!   `FilePathGameData` для кампаний) и автосейв (`game_manager.cpp:2500`:
//!   слот **10**, включён настройкой `SETUP.auto_save`, имя — «(Turn N)»);
//! * `saveloadmenu.cpp` — окно: фон `LOADPIC` 640×480, заголовок в (229,5)
//!   181×21, десять слотов 200×70 (`FILE1_UP`…`FILE9_UP`, `FILE10UP`) в две
//!   колонки: `x = 402*(i/5)+16`, `y = 76*(i%5)+44`, до 10 слотов на страницу,
//!   номера слотов с 1, страница сдвигается на 10 (`SaveLoadMenu_SaveSlotLimit`
//!   = 991 — первый слот последней страницы);
//! * кнопки в нижней строке y=438: `Cancel` (`MNUBTN6U`, 514), `Load`
//!   (`MNUBTN4U`, 354), `Save` (`MNUBTN2U`, 132, только в режиме сохранения),
//!   страницы `MNUUAROU`/`MNUDAROU` (33/63), `Help` (`MNUBTN5U`, 465) и
//!   `Quit to menu` (`MNUBTN3U`, 243);
//! * имя сохранения — `char[30]` с text edit в выбранном слоте.
//!
//! # Что реализовано, а что нет
//!
//! Реализованы: сетка слотов с превью-инфо (имя, ход, планета, команды, число
//! юнитов), ввод имени (30 байт как в V70), страницы, `Save`/`Load`/`Cancel`,
//! автосейв. Не реализованы (осознанно, чтобы не тащить неотносящиеся экраны):
//! `Help` (`MNUBTN5U`) и `Quit to menu` (`MNUBTN3U`) — это переходы в другие
//! меню, а не часть сохранения; `FILESFLC`-анимация окна.
//!
//! # Сеть
//!
//! В оригинале сохранение в сети **разрешено**: `SaveLoadMenu_Save` рассылает
//! пакет 16 (`Remote_SendNetPacket_16`) и пишет файл, а загрузка запрещена
//! («Save/Load is not available in a network game», `saveloadmenu.cpp:461`).
//! В нашем протоколе пакета 16 нет (`max-brh-docs/architecture/protocol.md`), поэтому
//! меню в сетевой сессии **не открывается** и пишет в журнал, что сохранение
//! сети не поддержано — иначе клиенты разошлись бы (см. [`crate::App::open_save_menu`]).

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

/// Логическая ширина экрана (окно оригинала).
pub const SCREEN_WIDTH: i32 = 640;
/// Логическая высота экрана.
pub const SCREEN_HEIGHT: i32 = 480;

/// Слотов на страницу (`SaveLoadMenu_SaveSlotsPerPage`).
pub const SLOTS_PER_PAGE: i32 = 10;
/// Первый слот (`SaveLoadMenu_FirstSaveSlot`).
pub const FIRST_SLOT: i32 = 1;
/// Первый слот последней страницы (`SaveLoadMenu_SaveSlotLimit`).
pub const SLOT_LIMIT: i32 = 991;
/// Последний достижимый слот (`SaveLoadMenu_LastSaveSlot`).
pub const LAST_SLOT: i32 = SLOT_LIMIT + SLOTS_PER_PAGE - 1;
/// Слот автосейва (`game_manager.cpp:2503`).
pub const AUTOSAVE_SLOT: i32 = 10;

/// Прямоугольник заголовка (`Text_TextBox(…, 229, 5, 181, 21, …)`).
pub const TITLE: PixelRect = PixelRect::new(229, 5, 181, 21);
/// Поле имени сохранения (V70 `save_name[30]`); в C++ — text edit слота.
pub const NAME_FIELD: PixelRect = PixelRect::new(16, 8, 200, 26);
/// Сетка слотов: две колонки по пять строк (C++ `402*(i/5)+16`, `76*(i%5)+44`).
pub const SLOT_WIDTH: i32 = 200;
/// Высота слота (картинка `FILE*_UP` — 200×70).
pub const SLOT_HEIGHT: i32 = 70;
/// Кнопка `Save` (`MNUBTN2U`, 109×40) — только в режиме сохранения.
pub const SAVE_BUTTON: PixelRect = PixelRect::new(132, 438, 109, 40);
/// Кнопка `Cancel` (`MNUBTN4U`, 109×40): C++ `r_value = 1000` → выход без записи.
pub const CANCEL_BUTTON: PixelRect = PixelRect::new(354, 438, 109, 40);
/// Кнопка `Load` (`MNUBTN6U`, 109×40) — доступна в обоих режимах.
pub const LOAD_BUTTON: PixelRect = PixelRect::new(514, 438, 109, 40);
/// Страница вверх (`MNUUAROU`, 28×29).
pub const PAGE_UP: PixelRect = PixelRect::new(33, 438, 28, 29);
/// Страница вниз (`MNUDAROU`, 28×29).
pub const PAGE_DOWN: PixelRect = PixelRect::new(63, 438, 28, 29);
/// Строка статуса под кнопками.
pub const STATUS: PixelRect = PixelRect::new(16, 414, 600, 18);

/// Прямоугольник слота на странице (`i` = 0..9).
pub fn slot_rect(index: usize) -> PixelRect {
    let i = index as i32;
    PixelRect::new(402 * (i / 5) + 16, 76 * (i % 5) + 44, SLOT_WIDTH, SLOT_HEIGHT)
}

/// Режим окна: сохранение или загрузка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveLoadMode {
    /// `is_saving_allowed = true` — можно писать файл и править имя.
    Save,
    /// Только выбор существующего сейва.
    Load,
}

impl SaveLoadMode {
    /// Заголовок окна (`_(5426)`/`_(63e7)`).
    pub const fn title(self) -> &'static str {
        match self {
            Self::Save => "Save Game",
            Self::Load => "Load Game",
        }
    }
}

/// Превью слота: то, что C++ показывает в `SaveSlot::DrawSaveSlot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotPreview {
    /// Имя сохранения (`save_name` заголовка).
    pub save_name: String,
    /// Номер хода.
    pub turn: i32,
    /// Планета.
    pub world: Planet,
    /// Сколько юнитов в сейве.
    pub units: usize,
    /// «Имена команд через запятую» для строки превью.
    pub teams: String,
    /// Версия формата.
    pub version: u16,
}

/// Один слот страницы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    /// Номер слота в терминах C++ (с 1).
    pub slot: i32,
    /// Имя файла (`save3.hot`).
    pub file_name: String,
    /// Полный путь.
    pub path: PathBuf,
    /// Файл существует и разбирается.
    pub in_use: bool,
    /// Превью, если файл прочитан.
    pub preview: Option<SlotPreview>,
}

/// Что нажали в окне.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveLoadAction {
    /// Ничего (перерисовать).
    None,
    /// Сохранить в слот с таким именем.
    Save {
        /// Номер слота (с 1).
        slot: i32,
        /// Имя сохранения (≤ 29 символов).
        name: String,
    },
    /// Загрузить слот.
    Load {
        /// Номер слота (с 1).
        slot: i32,
    },
    /// Закрыть окно.
    Cancel,
}

/// Экран сохранения/загрузки.
#[derive(Debug, Clone)]
pub struct SaveLoadMenu {
    /// Режим.
    pub mode: SaveLoadMode,
    /// Категория партии (задаёт расширение файлов и каталог).
    pub category: MissionCategory,
    /// Каталог сейвов.
    pub dir: PathBuf,
    /// Первый слот страницы.
    pub first_slot: i32,
    /// Слоты текущей страницы (всегда [`SLOTS_PER_PAGE`]).
    pub slots: Vec<SlotInfo>,
    /// Выбранный слот страницы (0..9).
    pub selected: Option<usize>,
    /// Редактируемое имя (`char[30]`, как в V70).
    pub name: String,
    /// Имя редактируется (в режиме сохранения).
    pub editing: bool,
    /// Строка статуса/ошибки.
    pub status: String,
}

impl SaveLoadMenu {
    /// Открывает окно на первой странице (C++ `SaveLoadMenu_MenuLoop`).
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

    /// Расширение файлов категории (`SaveLoad_GetSaveFileName`).
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

    /// Имя файла слота (`save<slot>.<ext>`).
    pub fn file_name(category: MissionCategory, slot: i32) -> String {
        format!("save{slot}.{}", Self::extension(category))
    }

    /// Полный путь слота (сначала нижний регистр, как `SaveLoad_GetFilePath`).
    pub fn file_path(dir: &Path, category: MissionCategory, slot: i32) -> PathBuf {
        dir.join(Self::file_name(category, slot))
    }

    /// Перечитывает слоты страницы.
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
            // C++ не выбирает слот сам (`save_slot_index = -1`); для клавиатуры
            // выбираем полезный по умолчанию: свободный слот при сохранении и
            // первый занятый при загрузке.
            self.selected = if self.mode == SaveLoadMode::Save {
                self.slots.iter().position(|slot| !slot.in_use)
            } else {
                self.slots.iter().position(|slot| slot.in_use)
            }
            .or(Some(0));
        }
    }

    /// Выбранный слот целиком.
    pub fn selected_slot(&self) -> Option<&SlotInfo> {
        self.selected.and_then(|index| self.slots.get(index))
    }

    /// Меняет страницу (`GNW_KB_KEY_PAGEUP`/`PAGEDOWN`); `false` — уже край.
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

    /// Выбирает слот по индексу страницы; при сохранении подставляет имя
    /// существующего сейва (перезапись), у пустого слота имя не трогает.
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

    /// Действие «Сохранить» для текущего слота.
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

    /// Действие «Загрузить» для текущего слота.
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

    /// Обрабатывает клик мыши в пикселях кадра.
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

    /// Обрабатывает клавишу.
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

    /// Обрабатывает событие ввода целиком.
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

    /// Рисует окно поверх кадра.
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &Assets) -> u32 {
        if frame.width() != SCREEN_WIDTH as u32 || frame.height() != SCREEN_HEIGHT as u32 {
            return 0;
        }

        // Под окном — чёрный кадр: прозрачные пиксели `LOADPIC` в C++ показывают
        // игру, но проще (и стабильнее для golden-кадров) залить чёрным.
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

        // Поле имени (V70 `char save_name[30]`).
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

        // Десять слотов.
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

            // Номер слота (C++ `Text_TextBox(..., "%d", 0, 0, 40, 70)`).
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

        // Кнопки в порядке C++ (`saveloadmenu.cpp:248-265`): страницы слева,
        // Save (только сохранение), Cancel, Load.
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

/// Ресурсы окна сохранения/загрузки из `MAX.RES`.
#[derive(Debug, Clone)]
pub struct SaveLoadAssets {
    /// Фон `LOADPIC` 640×480.
    pub background: Option<BigImage>,
    /// Пары `FILE1_UP/DN`…`FILE10UP/DN` (индекс — слот страницы 0..9).
    pub slots: [Option<MenuButtonPair>; 10],
    /// Страница вверх (`MNUUAROU/D`).
    pub page_up: Option<MenuButtonPair>,
    /// Страница вниз (`MNUDAROU/D`).
    pub page_down: Option<MenuButtonPair>,
    /// `Save` (`MNUBTN2U/D`).
    pub save: Option<MenuButtonPair>,
    /// `Load`/`Save` (`MNUBTN4U/D`).
    pub action: Option<MenuButtonPair>,
    /// `Cancel` (`MNUBTN6U/D`).
    pub cancel: Option<MenuButtonPair>,
}

impl SaveLoadAssets {
    /// Грузит ресурсы из архива `MAX.RES`; отсутствие части — не ошибка.
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            let up = SimpleImage::decode(archive.read(up).ok()?).ok()?;
            let down = SimpleImage::decode(archive.read(down).ok()?).ok()?;
            Some(MenuButtonPair::new(up, down))
        };
        let mut slots: [Option<MenuButtonPair>; 10] = Default::default();
        for (index, slot) in slots.iter_mut().enumerate() {
            // C++ `FILE1_UP + i`: у десятого ресурса имя без подчёркивания.
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

    /// Пара картинок слота страницы.
    pub fn slot_pair(&self, index: usize) -> Option<&MenuButtonPair> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    /// Есть ли фон или картинки слотов (иначе окно рисуется рамками).
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.slots.iter().any(Option::is_some)
    }
}

/// Кнопка: картинка C++ или честная рамка с подписью (как `buildmenu`).
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

/// Обрезка длинного имени по ширине (как `mobilize::fit_label`).
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
// Интеграция с приложением
// ---------------------------------------------------------------------------

impl crate::App {
    /// Каталог сейвов: `--save-dir`, иначе каталог `MAX.RES` (`max-original`),
    /// как `FilePathGameData` для кампаний и `FilePathGamePref` для custom.
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

    /// Категория партии для имён файлов/слотов.
    pub fn save_category(&self) -> MissionCategory {
        if self.local().is_some_and(|local| local.is_hot_seat()) {
            MissionCategory::HotSeat
        } else {
            self.state().mission_category
        }
    }

    /// Открытое окно сохранения/загрузки.
    pub fn save_load(&self) -> Option<&SaveLoadMenu> {
        self.save_load.as_ref()
    }

    /// Изменяемое окно.
    pub fn save_load_mut(&mut self) -> Option<&mut SaveLoadMenu> {
        self.save_load.as_mut()
    }

    /// Закрывает окно.
    pub fn close_save_load(&mut self) {
        self.save_load = None;
    }

    /// Открывает окно сохранения (`File`-кнопка, `Alt+F` в C++).
    ///
    /// В сетевой сессии возвращает `false` и пишет в журнал объяснение: в
    /// оригинале сохранение рассылает пакет 16, которого в нашем протоколе нет.
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
        // Клавиатурный сценарий «S → Enter» пишет в первый свободный слот.
        if let Some(index) = menu.slots.iter().position(|slot| !slot.in_use) {
            menu.selected = Some(index);
        }
        self.save_load = Some(menu);
        true
    }

    /// Открывает окно загрузки (C++ `Alt+L`).
    ///
    /// В сети загрузка запрещена и в оригинале (`saveloadmenu.cpp:461`).
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

    /// Сохраняет партию в слот (`save<slot>.<ext>`).
    pub fn save_game_to_slot(&mut self, slot: i32, save_name: &str) -> Result<PathBuf, String> {
        let category = self.save_category();
        let path = SaveLoadMenu::file_path(&self.save_dir(), category, slot);
        self.save_v70_to(&path, save_name)?;
        Ok(path)
    }

    /// Записывает V70-сейв текущей партии по пути.
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

    /// Байты V70-сейва текущей партии (с обновлением [`SaveMeta`]).
    pub fn save_v70_bytes(&mut self, save_name: &str) -> Result<Vec<u8>, String> {
        self.sync_save_meta(save_name);
        self.state
            .to_save_bytes()
            .map_err(|error| format!("V70-запись: {error}"))
    }

    /// Обновляет «внешние» поля сейва перед записью: имя, таймер хода, UI-поля
    /// игрока и журнал сообщений команды.
    fn sync_save_meta(&mut self, save_name: &str) {
        let team = usize::from(self.state().player_team.0).min(3);
        let timer_left = self
            .local()
            .map(|local| local.timer_left.ceil().max(0.0) as u16)
            .unwrap_or(0);
        let (center_x, center_y) = self.viewport().center();
        let zoom = self.viewport().zoom();
        let messages: Vec<String> = self.hud().log.lines().to_vec();
        let camera_x = (center_x / 64).clamp(0, 111) as i16;
        let camera_y = (center_y / 64).clamp(0, 111) as i16;
        let existing = self.state().save_meta.message_logs[team].len();
        let zoom_level = zoom.min(u32::from(u16::MAX)) as u16;

        let preferences = self.preferences().to_ini();
        // `SaveLoad_Save` (`saveload.cpp:426,502-511`): имя команды —
        // `menu_team_name_setting` (его правит `OptionsMenu` в игре), таймеры —
        // текущие `settings["timer"]`/`["endturn"]` (их мог изменить
        // `Preferences` после старта партии).
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
        let meta = &mut self.state_mut().save_meta;
        SaveMeta::set_name(&mut meta.save_name, save_name);
        meta.turn_timer = timer_left;
        meta.turn_timer_time = turn_timer.clamp(0, i32::from(u16::MAX)) as u16;
        meta.endturn_time = endturn.clamp(0, i32::from(u16::MAX)) as u16;
        meta.ini_options.turn_timer = turn_timer;
        meta.ini_options.endturn = endturn;
        for (team, name) in live_names {
            SaveMeta::set_name(&mut meta.team_names[team], &name);
        }
        // `IniPreferences` (7 × `signed int`): текущие настройки игрока, как
        // `SaveLoad_Save` (`saveload.cpp:547` — `settings->GetNumericValue`).
        meta.ini_preferences = preferences;
        // Камера/зум игрока — как `GameManager_GuiSwitchTeam` + `CTInfo`.
        meta.team_ui[team].camera_position = max_core::geo::Point::new(camera_x, camera_y);
        meta.team_ui[team].zoom_level = zoom_level;
        // Журнал UI → `MessageLogList` команды (V70): только новые строки.
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

    /// Автосейв конца хода (C++ `SETUP.auto_save`, слот 10).
    ///
    /// C++ пишет его при входе в `GameManager_UpdateGui` первой команды раунда
    /// (`game_manager.cpp:2500`) — см. `local::local_finish_round`.
    pub fn autosave(&mut self) -> Option<PathBuf> {
        if !self.config().autosave || !self.preferences().auto_save || !self.local_active() {
            return None;
        }
        let name = format!("(Turn {})", self.state().turn_counter);
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

    /// Загружает V70-сейв в текущую сессию: локальная партия перезапускается с
    /// сохранённого хода (C++ `GameManager_LoadGame`), иначе состояние просто
    /// заменяется.
    pub fn load_game_slot(&mut self, slot: i32) -> Result<PathBuf, String> {
        let category = self.save_category();
        let path = SaveLoadMenu::file_path(&self.save_dir(), category, slot);
        self.load_v70_path(&path)?;
        Ok(path)
    }

    /// Загружает V70-файл по пути.
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

/// Заполняет [`SaveMeta`] настройками новой локальной партии.
///
/// Дефолты `IniPreferences` — из `SETUP` (как в `SAVE1.CAM`: effects=1,
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

/// То же, но с текущими настройками игрока (`Preferences::to_ini`): именно они
/// попадают в `SaveMeta::ini_preferences` новой партии, как `SaveLoad_Save`
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
    // Наш ИИ всегда держит карты 112×112 (`AiPlayer::new`), поэтому в сейв они
    // попадают — в отличие от V70-сейвов DOS-оригинала.
    meta.ai_has_info_map = [true; 4];
    meta.ai_has_mine_map = [true; 4];
    meta
}
