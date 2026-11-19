//! Наполнение панелей окна игры 1:1 (пункт A `docs/visual-1to1-status.md`).
//!
//! Поверх `FRAMEPIC` оригинал (`game_manager.cpp`) дорисовывает картинки
//! кнопок/панелей из `MAX.RES` и текст шрифтами GNW. Здесь это собрано в
//! [`Panels`]:
//!
//! * верх: `FILES_OF/ON` (17,4) 67×18, `PREF_OFF/ON` (86,4) 67×18,
//!   `ENDTRN_U` (390,2) 72×21, `TURNS` (468,2) 61×21, `TIMER` (534,2) 61×21;
//! * портрет `CORNER_FLIC` (10,29) 128×128 — FLC-анимация юнита
//!   (`max_assets::flic`, 15 FPS, см. [`crate::portrait`]); без FLC/выделения —
//!   чёрный фон (или статичный спрайт ×2 как fallback);
//! * статистика `STAT_WINDOW` (10,173) 153×48 — четыре строки шрифтом
//!   `GNW_TEXT_FONT_2`, состав зависит от юнита ([`rows_for`], порт
//!   `GameManager_UpdateInfoDisplay`): у мобильных — `Hits/Ammo/Speed/Shots`,
//!   у зданий — `Hits/Cargo` и строки `Usage`/`Power`/`Total` экономики
//!   комплекса (никакого `Speed` у шахты);
//! * сетка переключателей `SURV_OFF/ON`…`NAMES_UP/DN` (9 кнопок 55×16/18);
//! * зум `ZOOMPNL1` + `ZOOMPTR` в (20,275) 130×19;
//! * низ: `XYPOS` (242,456) 90×21 и `UNITNAME` (338,456) 222×21;
//! * миникарта `WINDOW_MINIMAP` (15,356) 112×112 из `minimap`-слоя `.WRL`.
//!
//! Координаты — из `window_manager.cpp::windows[]` (см. [`max_render::layout`]),
//! подписи — из `language.json` (`Files`, `Preferences`, `End Turn`, `Survey`,
//! …), цвета — индексы палитры ровно как в `game_manager.cpp`/`button.cpp`
//! (`FontColor(0xA2, 0xA7, 0xE6)` для подписей, `(0xA0, 0xA2, 0xE6)` для
//! End Turn).
//!
//! ## Координаты мыши
//!
//! Карта занимает [`layout::MAIN_MAP`] = (180,18) 448×448. [`screen_to_map`]
//! переводит точку окна в локальные координаты карты, а [`panel_button_at`]
//! проверяет панели (включая End Turn, который намеренно заходит на карту).

use max_assets::image::SimpleImage;
use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType, VictoryType};
use max_core::game::{GameState, PLAYER_TEAM_COUNT};
use max_core::team::{TeamId, TeamKind};
use max_core::unit::{UnitInfo, UnitListKind};
use max_core::unit_values::{
    ATTRIB_AMMO, ATTRIB_ATTACK, ATTRIB_HITS, ATTRIB_ROUNDS, ATTRIB_SPEED, ATTRIB_STORAGE,
};
use max_core::victory::{
    cargo_get_net_production, cargo_life_consumption_rate, cargo_power_consumption_rate,
    cargo_type_of, CargoType, NetProduction,
};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::layout;
use max_render::text::{draw_text_gnw, draw_text_gnw_shadowed, GnwFont, GnwFontId};
use max_render::viewport::Viewport;

use crate::assets::Assets;

/// Цвет обычного текста панелей (`0xA2`).
pub const TEXT: u8 = 0xA2;
/// `COLOR_GREEN` (`0x02`) — имя юнита в портрете
/// (`GameManager_MenuUnitSelect`: `win_print(..., COLOR_GREEN)`).
pub const GREEN: u8 = 0x02;
/// `COLOR_RED` (`0x01`) — значение ≤ 1/4 базы (`GameManager_DrawInfoDisplayRow`).
pub const RED: u8 = 0x01;
/// `COLOR_YELLOW` (`0x04`) — значение ≤ 1/2 базы.
pub const YELLOW: u8 = 0x04;
/// Обводка подписей кнопок (`0xA7`).
pub const CAPTION_OUTLINE: u8 = 0xA7;
/// Тень подписей кнопок и текста (`0xE6`).
pub const SHADOW: u8 = 0xE6;
/// Светлый текст End Turn (`0xA0`).
pub const END_TURN_TEXT: u8 = 0xA0;
/// Кнопка End Turn нажата (не используется в статичном кадре).
pub const END_TURN_OUTLINE: u8 = 0xA2;

/// Палитра окна игры.
///
/// В партии системная палитра — это палитра `FRAMEPIC` (C++:
/// `WindowManager_LoadBigImage(FRAMEPIC, …, palette_from_image=true)`), а не
/// `COLOR.PAL` спрайтов. Индексы 0x01/0x02/0x04 в ней — яркие красный,
/// зелёный и жёлтый, а 0xA2/0xA7/0xE6 — цвета текста и тени; `COLOR.PAL` для
/// этих индексов даёт другие цвета.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiPalette {
    colors: [[u8; 3]; 256],
}

impl UiPalette {
    /// Палитра из `FRAMEPIC` (8-битные каналы, без `*4`).
    pub fn from_big_image(image: &max_assets::image::BigImage) -> Self {
        let mut colors = [[0u8; 3]; 256];

        for (index, color) in colors.iter_mut().enumerate() {
            *color = image.color(index as u8);
        }

        Self { colors }
    }

    /// Палитра из готовых цветов (для тестов и синтетических кадров).
    pub const fn from_colors(colors: [[u8; 3]; 256]) -> Self {
        Self { colors }
    }

    /// Системная палитра окна после загрузки мира: индексы 64…159 берутся из
    /// `.WRL` (`World::Load` → `WindowManager_ColorPalette[i] = palette[i]/4`
    /// в C++; здесь — уже 8-битные цвета `TerrainAtlas`).
    pub fn with_wrl_palette(mut self, wrl: &max_render::terrain::PaletteRgba) -> Self {
        for (index, color) in wrl.iter().enumerate().skip(64).take(96) {
            self.colors[index] = [color[0], color[1], color[2]];
        }

        self
    }

    /// Палитра из загруженного хрома; `None`, если `FRAMEPIC` не загружен.
    pub fn from_assets(assets: &Assets) -> Option<Self> {
        assets
            .chrome
            .as_ref()
            .map(|chrome| Self::from_big_image(&chrome.frame))
    }

    /// Цвет по индексу палитры.
    pub fn rgba(&self, index: u8) -> Rgba {
        Rgba::from_rgb(self.colors[usize::from(index)])
    }

    /// Вся палитра в формате рендера (`[[u8; 4]; 256]`) — для simple image
    /// (маркеры ресурсов, курсоры), которые индексируются системной палитрой
    /// окна, а не палитрой `.WRL`/`COLOR.PAL`.
    pub fn palette_rgba(&self) -> max_render::terrain::PaletteRgba {
        self.colors.map(|color| [color[0], color[1], color[2], 255])
    }

    /// Ближайший цвет палитры — дословный `Color_MapColor(palette, r, g, b, true)`
    /// (`color.cpp:79`): перебор всех 256 цветов по сумме квадратов разностей,
    /// первый минимальный выигрывает.
    ///
    /// Нужен для мини-карт планет: `Menu_LoadPlanetMinimap` переводит цвета
    /// `.WRL` (делённые на 4) в палитру окна этим вызовом.
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

    /// Цвет палитры по индексу как `[r, g, b]`.
    pub fn color_rgb(&self, index: u8) -> [u8; 3] {
        self.colors[usize::from(index)]
    }

    /// Индекс палитры по точному цвету кадра (`None`, если цвета нет —
    /// например, пиксель пришёл не из этой палитры).
    ///
    /// Нужен `Color_RecolorPixels`: перекраска подложки идёт по **индексам**
    /// палитры, а кадр хранит уже RGB.
    pub fn index_of(&self, color: Rgba) -> Option<u8> {
        self.colors
            .iter()
            .position(|candidate| {
                candidate[0] == color.r && candidate[1] == color.g && candidate[2] == color.b
            })
            .map(|index| index as u8)
    }

    /// `Color_GenerateIntensityTable1(red, green, blue, factor1, factor2, table)`
    /// (`color.cpp:132`): таблица «приглушения» палитры — каждый цвет палитры
    /// смешивается с уровнем и заменяется ближайшим цветом палитры
    /// (`Color_MapColor` с полным перебором, первый минимальный выигрывает).
    ///
    /// Диалог распределения шахты (`AllocMenu`) строит такую таблицу
    /// `(0, 0, 25, 1, 1)` и ею перекрашивает подложку под числом на полосе
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

            // `Color_GetColorDistance` до собственного цвета; при нулевом
            // расстоянии C++ цикл поиска не выполняется вовсе.
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

/// `Color_GetColorDistance` (`color.cpp:120`) — сумма квадратов разностей.
fn palette_distance(color: [u8; 3], target: [i32; 3]) -> i32 {
    let red = i32::from(color[0]) - target[0];
    let green = i32::from(color[1]) - target[1];
    let blue = i32::from(color[2]) - target[2];

    red * red + green * green + blue * blue
}

/// Переключатель панели или кнопка, обрабатываемая хромом.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelButton {
    /// Кнопка Files (17,4) 67×18.
    Files,
    /// Кнопка Preferences (86,4) 67×18.
    Preferences,
    /// Кнопка End Turn (390,2) 72×21 (заходит на карту).
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
    /// Кнопка `2x` миникарты (136,387) 27×26.
    Minimap2x,
    /// Кнопка `TNT` миникарты (136,413) 27×27.
    MinimapTnt,
    /// Кнопка `+` зума (2,275) 18×14.
    ZoomPlus,
    /// Ползунок зума (20,275) 130×14.
    ZoomSlider,
    /// Кнопка `−` зума (150,275) 20×14.
    ZoomMinus,
    /// Переключатель Survey.
    Survey,
    /// Переключатель Status.
    Status,
    /// Переключатель Colors.
    Colors,
    /// Переключатель Hits.
    Hits,
    /// Переключатель Ammo.
    Ammo,
    /// Переключатель Range.
    Range,
    /// Переключатель Scan.
    Scan,
    /// Переключатель Grid.
    Grid,
    /// Переключатель Names.
    Names,
}

impl PanelButton {
    /// Все кнопки в порядке проверки попадания.
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

    /// Девять переключателей сетки (левый столбец → правый).
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

    /// Прямоугольник кнопки в окне (inclusive-координаты `windows[]` → размеры).
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

    /// Подпись кнопки (`language.json`, en-US), если она есть у оригинала.
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

    /// Шрифт подписи: End Turn — FONT_5, остальные — FONT_2
    /// (`GameManager_MenuInitButtons`).
    pub const fn caption_font(self) -> GnwFontId {
        match self {
            Self::EndTurn => GnwFontId::Font5,
            _ => GnwFontId::Font2,
        }
    }

    /// Вертикальный сдвиг подписи (`GameManager_MenuItemLabelOffsets`).
    pub const fn caption_offset_y(self) -> i32 {
        match self {
            Self::Files | Self::Preferences => 2,
            Self::Prev | Self::Next => 2,
            Self::Done | Self::Reports | Self::Chat => 3,
            Self::Colors | Self::Range | Self::Names => 4,
            Self::Survey | Self::Status | Self::Hits | Self::Ammo | Self::Scan | Self::Grid => 2,
            _ => 0,
        }
    }

    /// Это переключатель HUD (рисуется `*_ON` при включении).
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

/// Локальное состояние панелей (`HudState::panels`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelState {
    /// Нажат Survey.
    pub survey: bool,
    /// Нажат Status.
    pub status: bool,
    /// Нажат Colors.
    pub colors: bool,
    /// Нажат Hits.
    pub hits: bool,
    /// Нажат Ammo.
    pub ammo: bool,
    /// Нажат Range.
    pub range: bool,
    /// Нажат Scan.
    pub scan: bool,
    /// Нажат Grid.
    pub grid: bool,
    /// Нажат Names.
    pub names: bool,
    /// Миникарта в режиме `2x`.
    pub minimap_2x: bool,
    /// Миникарта в режиме `TNT`.
    pub minimap_tnt: bool,
    /// Клетка под курсором (для координат/описания внизу), если мышь на карте.
    pub cursor_grid: Option<(i16, i16)>,
    /// Остаток времени хода в секундах (таймер в верхней строке).
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
    /// Включён ли переключатель.
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

    /// Переключает состояние; возвращает новое значение.
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

/// Пара картинок «отжата/нажата».
#[derive(Debug, Clone)]
pub struct ButtonPair {
    up: SimpleImage,
    down: SimpleImage,
}

/// Загруженные картинки панелей.
#[derive(Debug, Clone)]
pub struct Panels {
    files: ButtonPair,
    preferences: ButtonPair,
    end_turn: SimpleImage,
    turns: SimpleImage,
    timer: SimpleImage,
    coordinates: SimpleImage,
    unit_description: SimpleImage,
    zoom_background: SimpleImage,
    zoom_pointer: SimpleImage,
    toggles: [ButtonPair; 9],
    minimap_2x: ButtonPair,
    minimap_tnt: ButtonPair,
    /// Прочие кнопки: готовая картинка и положение в окне.
    aux: Vec<(SimpleImage, PixelRect)>,
}

impl Panels {
    /// Грузит картинки панелей из `MAX.RES`.
    ///
    /// Обязательны картинки верхних кнопок/счётчиков, нижней строки, зума и
    /// девяти переключателей. Если чего-то нет, возвращается `None` — хром
    /// продолжает рисовать только `FRAMEPIC`.
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
        })
    }

    /// Рисует панели поверх кадра (после `FRAMEPIC` и карты).
    ///
    /// Возвращает число закрашенных пикселей.
    pub fn draw(&self, frame: &mut RgbaBuffer, app: &crate::App) -> u32 {
        let Some(assets) = app.assets() else {
            return 0;
        };
        let state = app.state();
        let hud = app.hud();
        let viewport = app.viewport();
        let panel = &hud.panels;
        let Some(palette) = UiPalette::from_assets(assets) else {
            return 0;
        };
        let palette = &palette;
        let mut painted = 0;

        // Верхние кнопки.
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

        // End Turn (заходит на карту — рисуется поверх).
        painted += self.blit(
            frame,
            palette,
            &self.end_turn,
            layout::END_TURN_BUTTON.ulx,
            layout::END_TURN_BUTTON.uly,
            true,
        );
        self.draw_caption(frame, palette, PanelButton::EndTurn);

        // Номер хода и таймер.
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
            &format!("{}", state.turn_counter.max(0)),
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
        let timer_color = if panel.timer_seconds <= 20 && panel.timer_seconds > 0 {
            palette.rgba(0x01)
        } else {
            palette.rgba(TEXT)
        };
        draw_centered(
            frame,
            layout::TURN_TIMER,
            GnwFontId::Font5.font(),
            &format!(
                "{:02}:{:02}",
                panel.timer_seconds / 60,
                panel.timer_seconds % 60
            ),
            timer_color,
        );

        // Прочие кнопки (Play/Pause/Center/Prev/Done/Next/Help/Reports/Chat).
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

        // Сетка переключателей.
        for (index, button) in PanelButton::TOGGLES.into_iter().enumerate() {
            let pair = &self.toggles[index];
            let active = panel.get(button);
            self.draw_pair(frame, palette, pair, button.rect(), active, &mut painted);
            self.draw_caption(frame, palette, button);
        }

        // Кнопки миникарты. В оригинале их отжатое состояние уже нарисовано в
        // `FRAMEPIC` (как и весь хром), а ресурс `MIN2X_OF`/`MINFL_OF` лежит на
        // несколько пикселей ниже — при отрисовке поверх получался бы дубль.
        // Поэтому `*_OF` рисуется только без хрома, `*_ON` — всегда.
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

        // Зум: фон и указатель (смещение — `GameManager_UpdateZoomSlider`).
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

        // Миникарта из `minimap`-слоя `.WRL`.
        painted += draw_minimap(frame, app, assets, palette);

        // Портрет и статистика.
        painted += self.draw_portrait(frame, app, assets, palette);
        painted += draw_stats(frame, state, app.selected_unit(), palette);

        // Нижняя строка: координаты и описание юнита.
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

    fn draw_portrait(
        &self,
        frame: &mut RgbaBuffer,
        app: &crate::App,
        assets: &Assets,
        palette: &UiPalette,
    ) -> u32 {
        let mut painted = 0u32;
        let area = layout::CORNER_FLIC;

        // Чёрный фон окна портрета (как `FillOrRestoreWindow(CORNER_FLIC, BLACK)`).
        frame.fill_rect(area, Rgba::BLACK);
        painted += (area.width * area.height) as u32;

        let Some(unit) = app.selected_unit() else {
            return painted;
        };

        // Основной путь — FLC-анимация (`GameManager_MenuCreateFlic` +
        // `AdvanceFlic`): кадр 1:1 в окно 128×128, индексы — палитрой окна
        // (`FRAMEPIC`), как C++ (`load_flic_palette = 0`).
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

                    frame.set(area.ulx + x as i32, area.uly + y as i32, palette.rgba(index));
                }
            }

            painted += (width * height) as u32;
        } else {
            // Fallback: FLC нет (22 типа) или файл не загрузился — спрайт ×2.
            let mut portrait = RgbaBuffer::filled(area.width as u32, area.height as u32, Rgba::BLACK);
            let anchor = (area.width / 2, area.height / 2);

            let image_base = crate::render::image_base_for(
                unit,
                app.state()
                    .teams
                    .get(unit.team.0 as usize)
                    .map(|team| team.team_clan),
            );

            if crate::render::draw_unit_sprites_with_base(
                assets,
                unit,
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

        // Имя и статус в верхней части портрета (`GameManager_MenuUnitSelect`:
        // `win_print` именем `COLOR_GREEN`, затем `GameManager_UpdateInfoDisplay`
        // — статус `0xA2` строкой ниже). Оригинал рисует их поверх кадра FLC и
        // при анимации перерисовывает только строки ниже `text_height` — у нас
        // панель рисуется целиком, поэтому текст просто наносится последним.
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

    /// Кнопка миникарты: `*_ON` всегда, `*_OF` — только если хрома нет
    /// (в `FRAMEPIC` отжатое состояние уже нарисовано).
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
        let Some(caption) = button.caption() else {
            return;
        };
        let rect = button.rect();
        let font = button.caption_font().font();
        let width = font.text_width(caption) + 1;
        let x = rect.ulx + (rect.width - width).max(0) / 2;
        let y = rect.uly + (rect.height - font.height()) / 2 + button.caption_offset_y();
        let (base, outline, shadow) = if button == PanelButton::EndTurn {
            (END_TURN_TEXT, END_TURN_OUTLINE, SHADOW)
        } else {
            (TEXT, CAPTION_OUTLINE, SHADOW)
        };

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

    /// Копирует `SimpleImage` с прозрачностью (индекс `image.transparent`).
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

    /// То же, но с обрезкой по `clip` (окно панели).
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

/// Смещение указателя зума.
///
/// C++-формула (`GameManager_UpdateZoomSlider`): `((130-28) * (64-zoom)) / 60 +
/// 14` для диапазона 4…64. Наш `Viewport` держит 8…64, поэтому знаменатель —
/// `56`; при `zoom == 64` указатель стоит на 14, при `zoom == 8` — на 116.
pub fn zoom_slider_offset(zoom: u32) -> i32 {
    let zoom = zoom.clamp(8, 64);
    ((102 * (64 - zoom) as i32) / 56) + 14
}

/// Обратное преобразование: x ползунка → масштаб (`8..=64`).
pub fn zoom_from_slider_x(x: i32) -> u32 {
    let relative = (x - layout::ZOOM_SLIDER.ulx - 14).clamp(0, 102);
    let zoom = 64 - (relative * 56 / 102);

    zoom.clamp(8, 64) as u32
}

/// Кнопка панели под точкой окна (`None` — точка не на кнопке).
pub fn panel_button_at(x: i32, y: i32) -> Option<PanelButton> {
    PanelButton::ALL
        .into_iter()
        .find(|button| button.rect().contains(x, y))
}

/// Точка окна → локальные координаты карты (`None` — не в [`layout::MAIN_MAP`]).
pub fn screen_to_map(x: i32, y: i32) -> Option<(i32, i32)> {
    if !layout::MAIN_MAP.contains(x, y) {
        return None;
    }

    Some((x - layout::MAIN_MAP.ulx, y - layout::MAIN_MAP.uly))
}

/// Камера окна карты 448×448: тот же центр/зум, что у приложения.
pub fn map_viewport(viewport: &Viewport) -> Viewport {
    Viewport::with_center(
        layout::MAIN_MAP.width as u32,
        layout::MAIN_MAP.height as u32,
        viewport.center(),
        viewport.zoom(),
    )
}

/// Имя юнита для панелей (`UnitInfo::GetDisplayName`): имя или тип.
pub fn unit_name(unit: &UnitInfo) -> String {
    if unit.name.is_empty() {
        format!("{:?}", unit.unit_type)
    } else {
        unit.name.clone()
    }
}

/// Подпись строки статистики — `_(id)` из `game_manager.cpp` (`language.json`).
///
/// Идентификаторы C++ → строки en-US: `c02c` Hits, `5027` Ammo, `1274` Cargo,
/// `c372` Speed, `2bce` Shots, `8d59`/`3aea`/`81c5`/`160c` Usage, `848e` Power,
/// `1e1a` Teams, `d28b`/`c9b3`/`d4ca`/`7be8`/`5f25` Total, `c54a` points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatLabel {
    /// Строка 1 — `_(c02c)`.
    Hits,
    /// Строка 2 у вооружённых — `_(5027)`.
    Ammo,
    /// Строка 2 у складов/транспорта — `_(1274)`.
    Cargo,
    /// Строка 3 у мобильных — `_(c372)`.
    Speed,
    /// Строка 4 — `_(2bce)`.
    Shots,
    /// `DrawInfoDisplayType1/2` — `_(8d59)`/`_(3aea)`/`_(81c5)`/`_(160c)`.
    Usage,
    /// `DrawInfoDisplayType2` у генератора — `_(848e)`.
    Power,
    /// `DrawInfoDisplayType1` у генератора рабочих — `_(1e1a)`.
    Teams,
    /// `DrawInfoDisplayType1/2/3` и запас комплекса — `_(d28b)`/`_(c9b3)`/
    /// `_(d4ca)`/`_(7be8)`/`_(5f25)`.
    Total,
    /// `DrawInfoDisplayType3` (теплица) — `_(c54a)`.
    Points,
}

impl StatLabel {
    /// Подпись строки (значение `en-US` из `language.json` — как в игре).
    pub const fn caption(self) -> &'static str {
        match self {
            Self::Hits => "Hits",
            Self::Ammo => "Ammo",
            Self::Cargo => "Cargo",
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

/// Одна нарисованная строка статистики (`GameManager_DrawInfoDisplayRow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatRow {
    /// Подпись (левый бокс 45…75 px).
    pub label: StatLabel,
    /// Текущее значение (`current_value` после `std::min(current, base)`).
    pub current: i32,
    /// База (`base_value`); строка рисуется только при `base != 0`.
    pub base: i32,
    /// Шаг pips-иконок C++ (`factor`); `0` — C++ не рисует строку вовсе.
    pub factor: i32,
}

impl StatRow {
    /// Текст значения: `sprintf(text, "%i/%i", current_value, base_value)`.
    pub fn text(&self) -> String {
        format!("{}/{}", self.current, self.base)
    }

    /// Цвет значения — `GameManager_DrawInfoDisplayRow`:
    /// красный `≤ base/4`, жёлтый `≤ base/2`, иначе зелёный.
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

/// Строка «Finished building …» — `language.json` `1200`/`276c` (три варианта
/// по роду здания; в оригинале все три совпадают по тексту).
const FINISHED_CONSTRUCTION: &str =
    "Finished building {building}.  To move the {builder} out of the construction site, click on a square with a symbol in it.";
/// `FinishedBuilding` (`1200`/`74d3`/`8c62`) — завод: юнит выходит из завода.
const FINISHED_FACTORY: &str =
    "Finished building {building}.  To move the {building} out of the factory, click on a square with a symbol in it.";

/// Текст подсказки выделенного юнита — `GameManager_DrawUnitStatusMessage` +
/// `GameManager_DrawBuilderUnitStatusMessage` (`game_manager.cpp:7893/8038`).
///
/// Возвращает `None`, если для юнита подсказки нет (C++ рисует сообщение
/// только для `ORDER_BUILD`/`CLEAR`/`HALT_BUILDING`/`HALT_BUILDING_2`).
pub fn builder_status_message(state: &GameState, unit: &UnitInfo) -> Option<String> {
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

        return Some(if unit.flags & unit_flags::STATIONARY != 0 {
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

    if matches!(unit.orders, UnitOrder::HaltBuilding | UnitOrder::HaltBuilding2) {
        return Some(format!(
            "Was building {name}, with {} turns to completion.",
            unit.build_time
        ));
    }

    let (turns, _) = max_core::production::turns_and_consumption(state, unit, unit_type, build_rate);

    Some(format!(
        "Currently building: {name}.\nTurns to completion: {turns}."
    ))
}

/// Окно сообщения над картой — `MessageManager_DrawMessage` +
/// `MessageManager_DrawMessageBox` (`message_manager.cpp:144/206`).
///
/// Фон — таблица затемнения `MESSAGE_BOX_INFO` (`ResourceManager_
/// DarkeningColorIndexTable`: `max(7, c/4)*31/63` и ближайший цвет палитры),
/// текст — `Font5` цветом `0xFF` с обводкой/тенью. Прямоугольник: `(area.ulx,
/// area.uly + 10)`, ширина = самая длинная строка + 20, высота = `20 + 10 *
/// строк`.
pub fn draw_status_message(
    frame: &mut RgbaBuffer,
    area: PixelRect,
    palette: &UiPalette,
    text: &str,
) -> u32 {
    if text.is_empty() || area.width <= 0 {
        return 0;
    }

    let font = GnwFontId::Font5.font();
    // `MessageManager_WrapText(text, width - 20)`: строки переносятся по словам.
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
                    let dark = color.darken(1);
                    let mapped = palette.map_color([dark.r, dark.g, dark.b]);
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

        let drawn = draw_text_gnw_shadowed(
            frame,
            rect.ulx + 10,
            rect.uly + 10 + 10 * index as i32,
            font,
            line,
            palette.rgba(0xFF),
            Rgba::BLACK,
            Rgba::BLACK,
        );

        painted += drawn.max(0) as u32;
    }
    painted
}

/// Строка C++: `if (base_value && factor)` — иначе `DrawInfoDisplayRow`
/// не рисует ничего, и строка остаётся пустой (фон `FRAMEPIC`).
fn make_row(label: StatLabel, current: i32, base: i32, factor: i32) -> Option<StatRow> {
    if base == 0 || factor == 0 {
        return None;
    }

    Some(StatRow {
        label,
        current: current.min(base),
        base,
        factor,
    })
}

/// Наложение строки поверх уже нарисованной (C++ рисует поверх буфера
/// `WINDOW_STAT_ROW_N`; пропущенный вызов оставляет прежнее содержимое).
fn assign_row(rows: &mut [Option<StatRow>; 4], index: usize, row: Option<StatRow>) {
    if let Some(row) = row {
        rows[index] = Some(row);
    }
}

/// `Cargo_GetNetProduction(unit)` в цикле `DrawInfoDisplayType1/2`: у юнита в
/// `ORDER_UPGRADE` C++ считает производство по **предыдущему** приказу
/// (временно `SetOrder(prior)`), поэтому клонируем с `prior_orders`.
fn net_production_of(state: &GameState, unit: &UnitInfo) -> NetProduction {
    if unit.orders == UnitOrder::Upgrade && unit.state == UnitOrderState::ExecutingOrder {
        let mut simulated = unit.clone();

        simulated.orders = unit.prior_orders;

        cargo_get_net_production(state, &simulated, false)
    } else {
        cargo_get_net_production(state, unit, false)
    }
}

/// `GameManager_DrawInfoDisplayType2` — энергия: строка 2 `Usage`/`Power`,
/// строки 3/4 — `Total`/`Usage` по комплексу (только свои юниты).
fn power_rows(
    state: &GameState,
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

    let cargo = cargo_get_net_production(state, unit, false);
    let current_power_need = cargo.power;

    if power_need >= 0 {
        // `_(8d59)` «Usage»: сколько энергии юнит потребит.
        assign_row(
            rows,
            1,
            make_row(StatLabel::Usage, -current_power_need, power_need, 1),
        );
    } else {
        // `_(848e)` «Power»: сколько энергии юнит даёт.
        assign_row(
            rows,
            1,
            make_row(StatLabel::Power, current_power_need, -power_need, 1),
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

        if other.complex != unit.complex {
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
            make_row(StatLabel::Total, current_value, base_value, 1),
        );
    } else {
        assign_row(
            rows,
            2,
            make_row(StatLabel::Total, current_power_need, power_need_total, 1),
        );
        assign_row(
            rows,
            3,
            make_row(StatLabel::Usage, current_value, base_value, 1),
        );
    }
}

/// `GameManager_DrawInfoDisplayType1` — рабочие: строка 2 `Usage`/`Teams`,
/// строки 3/4 — `Total`/`Usage` по комплексу (только свои юниты).
fn life_rows(
    state: &GameState,
    unit: &UnitInfo,
    player_team: TeamId,
    rows: &mut [Option<StatRow>; 4],
) {
    let life_need = cargo_life_consumption_rate(unit.unit_type);

    if life_need == 0 || unit.team != player_team {
        return;
    }

    let cargo = cargo_get_net_production(state, unit, false);
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

        if other.complex != unit.complex {
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

/// `GameManager_DrawInfoDisplayType3` — теплица игрока: очки партии.
fn greenhouse_rows(state: &GameState, unit: &UnitInfo, rows: &mut [Option<StatRow>; 4]) {
    let mut current_value = if state.victory_type == VictoryType::Score {
        state.victory_limit
    } else {
        let mut value = 0;

        for team in 0..PLAYER_TEAM_COUNT {
            let info = &state.teams[team];

            if info.team_type != TeamKind::None && info.team_type != TeamKind::Eliminated {
                value = value.max(state.teams[unit.team.0 as usize].team_points as i32);
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
            state.teams[unit.team.0 as usize].team_points as i32,
            current_value,
            value_limit,
        ),
    );
}

/// Запас комплекса — `Complex::GetCargoInfo` + `Cargo_GetInventory`/
/// `Cargo_GetCargoCapacity`: сумма `storage` и `ATTRIB_STORAGE` стационарных
/// юнитов того же комплекса с тем же типом груза (строка `_(d28b)` «Total»).
fn complex_cargo(state: &GameState, unit: &UnitInfo, cargo_type: CargoType) -> (i32, i32) {
    let mut value = 0;
    let mut capacity = 0;

    for &id in state.unit_list(UnitListKind::Stationary) {
        let Some(other) = state.unit(id) else {
            continue;
        };

        if other.complex != unit.complex || cargo_type_of(other.unit_type) != cargo_type {
            continue;
        }

        value += i32::from(other.storage);
        capacity += other.base_values.get_attribute(ATTRIB_STORAGE);
    }

    (value, capacity)
}

/// Четыре строки статистики выбранного юнита — порт `GameManager_UpdateInfoDisplay`
/// (`game_manager.cpp:7667`), порядок вызовов сохранён:
///
/// 1. строка 1 — `Hits` (`unit->hits` / `ATTRIB_HITS`, factor 4);
/// 2. строка 2 — `Ammo` при `ATTRIB_ATTACK != 0` (база `ATTRIB_AMMO` только
///    для своей команды), иначе `Cargo` (`unit->storage` / `ATTRIB_STORAGE` с
///    масштабом по типу груза; чужому юниту — factor 0, строка не рисуется);
/// 3. строка 3 — `Speed` (`unit->speed` / `ATTRIB_SPEED`);
/// 4. строка 4 — `Shots` (`unit->shots` / `min(ROUNDS, AMMO)`).
///
/// Затем поверх (как C++, только для `player_team`):
/// [`power_rows`] при `ATTRIB_STORAGE == 0`, [`greenhouse_rows`] для `GREENHSE`
/// либо [`life_rows`], и строка запаса комплекса [`complex_cargo`] для своих
/// стационарных складов (`RAW`/`FUEL`/`GOLD`).
///
/// Строка с нулевой базой или нулевым factor остаётся пустой — ровно как
/// `GameManager_DrawInfoDisplayRow` (`if (base_value && factor)`).
pub fn rows_for(state: &GameState, unit: &UnitInfo, player_team: TeamId) -> [Option<StatRow>; 4] {
    let values = &unit.base_values;
    let own = unit.team == player_team;
    let mut rows: [Option<StatRow>; 4] = [None; 4];

    // Строка 1: `Hits` — при ненулевой базе (factor 4).
    rows[0] = make_row(
        StatLabel::Hits,
        i32::from(unit.hits),
        values.get_attribute(ATTRIB_HITS),
        4,
    );

    // Строка 2: `Ammo` у вооружённых / `Cargo` у остальных.
    let storage_value = values.get_attribute(ATTRIB_STORAGE);
    let cargo_type = cargo_type_of(unit.unit_type);
    let mut scaling_factor = 0;

    if values.get_attribute(ATTRIB_ATTACK) != 0 {
        // Врагу боезапас не показываем (`ammo = 0` в C++).
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
            StatLabel::Cargo,
            i32::from(unit.storage),
            storage_value,
            scaling_factor,
        );
    }

    // Строка 3: `Speed`, строка 4: `Shots` (`min(ROUNDS, AMMO)`).
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

    // `GameManager_DrawInfoDisplayType2` — только если `ATTRIB_STORAGE == 0`.
    if storage_value == 0 {
        power_rows(state, unit, player_team, &mut rows);
    }

    // Теплица игрока — Type3 вместо Type1.
    if unit.unit_type == UnitType::Greenhouse && own {
        greenhouse_rows(state, unit, &mut rows);
    } else {
        life_rows(state, unit, player_team, &mut rows);
    }

    // Запас комплекса — `_(d28b)` поверх строки 3.
    if own
        && storage_value > 0
        && unit.flags & unit_flags::STATIONARY != 0
        && (cargo_type as u8) >= (CargoType::Raw as u8)
        && (cargo_type as u8) <= (CargoType::Gold as u8)
    {
        let (value, capacity) = complex_cargo(state, unit, cargo_type);

        assign_row(
            &mut rows,
            2,
            make_row(StatLabel::Total, value, capacity, scaling_factor),
        );
    }

    rows
}

/// Одна нарисованная строка (`GameManager_DrawInfoDisplayRow`): значение
/// `current/base` цветом порога и подпись `0xA2`, шрифт `GNW_TEXT_FONT_2`.
fn draw_stat_row(frame: &mut RgbaBuffer, row: PixelRect, stat: &StatRow, palette: &UiPalette) {
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
}

/// Четыре строки статистики выбранного юнита ([`rows_for`]).
///
/// Строки, которые C++ не рисует, остаются фоном окна — [``None``].
fn draw_stats(
    frame: &mut RgbaBuffer,
    state: &GameState,
    selected: Option<&UnitInfo>,
    palette: &UiPalette,
) -> u32 {
    let Some(unit) = selected else {
        return 0;
    };

    let rows = rows_for(state, unit, state.player_team);
    let rects = [
        layout::STAT_ROW_1,
        layout::STAT_ROW_2,
        layout::STAT_ROW_3,
        layout::STAT_ROW_4,
    ];

    for (rect, row) in rects.iter().zip(rows.iter()) {
        if let Some(row) = row {
            draw_stat_row(frame, *rect, row, palette);
        }
    }

    (layout::STAT_WINDOW.width * layout::STAT_WINDOW.height) as u32
}

/// Размер окна миникарты (`WINDOW_MINIMAP`, 112×112).
const MINIMAP_SLOT: i32 = layout::MINIMAP.width;

/// Масштабирование карты в окно миникарты — `World::CalculateMinimapScaling`
/// (`world.cpp:917`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapView {
    /// Смещение нарисованной карты внутри окна (пиксели миникарты).
    pub offset_x: i32,
    /// Смещение по y.
    pub offset_y: i32,
    /// Ширина нарисованной карты в окне.
    pub width: i32,
    /// Высота нарисованной карты в окне.
    pub height: i32,
    /// Масштаб карта → окно (`ResourceManager_MinimapWindowScale`).
    pub scale: f64,
}

/// Считает масштаб и смещение карты в окне 112×112 (порт
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

/// `GameManager_GridCenterOffset`: левый верхний угол области 2x-миникарты.
///
/// В C++ смещение обновляется инкрементально (±4), здесь берётся его предел:
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

/// Точка окна → клетка миникарты (`GameManager_GetMinimapPosition`).
///
/// Учитывает `2x`-режим: в нём показывается область 56×56 вокруг центра
/// камеры, поэтому координата делится на 2 и смещается на
/// [`minimap_center_offset`]. `None` — точка вне окна миникарты.
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

/// Палитра индексов миникарты: системная палитра окна, в которой индексы
/// 64…159 заменены палитрой `.WRL` (`World::ApplyPaletteAndTints`).
fn minimap_palette(ui: &UiPalette, wrl: &[u8; 768]) -> max_render::terrain::PaletteRgba {
    let mut palette = ui.palette_rgba();
    let wrl_palette = max_render::terrain::palette_from_wrl(wrl);

    palette[64..160].copy_from_slice(&wrl_palette[64..160]);
    palette
}

/// `ResourceManager_DarkeningColorIndexTable` (`world.cpp:493-508`).
///
/// Цвет индекса затемняется вдвое (`max(7, c/4) * 31 / 63`), затем ищется
/// ближайший индекс палитры (`Color_MapColor`, диапазоны 9..31 и 96..127
/// пропускаются). Таблица применяется к неисследованным клеткам FOV.
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

/// `Color_MapColor(..., full_scan = false)` — ближайший цвет палитры.
fn map_color(palette: &[[u8; 3]; 256], target: [u8; 3]) -> u8 {
    let mut best = 0u8;
    let mut best_distance = i32::MAX;

    for (index, color) in palette.iter().enumerate() {
        // C++ пропускает диапазоны 9..=31 и 96..=127 (в байтовых смещениях —
        // те же индексы после /3).
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

/// Индекс цвета точки юнита на миникарте (`DrawMap_RenderMiniMapUnitList`).
fn minimap_unit_color(team: max_core::TeamId) -> u8 {
    match team.0 {
        0 => 0x01, // COLOR_RED
        1 => 0x02, // COLOR_GREEN
        2 => 0x03, // COLOR_BLUE
        3 => 0xFF, // «серый» — белый
        _ => 0x04, // COLOR_YELLOW (пришельцы/прочее)
    }
}

/// Миникарта из `minimap`-слоя `.WRL` (112×112 у стандартных планет).
fn draw_minimap(
    frame: &mut RgbaBuffer,
    app: &crate::App,
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

    let state = app.state();
    let panel = &app.hud().panels;
    let viewport = app.viewport();
    let combined = minimap_palette(palette, world.palette());
    let dark = darkening_table(&combined);

    // 1. FOV: `Access_UpdateMinimapFogOfWar` — неисследованные клетки темнее.
    let all_visible = state.ai.all_visible;
    let heat = state
        .ai
        .heat_maps
        .get(usize::from(state.player_team.0))
        .map(|map| map.cells())
        .unwrap_or(&[]);
    let mut layer = Vec::with_capacity((map_width * map_height) as usize);

    for (index, &color) in minimap.iter().enumerate() {
        let explored = heat.get(index).is_some_and(|cell| cell.complete > 0);

        layer.push(if all_visible || explored {
            color
        } else {
            dark[usize::from(color)]
        });
    }

    // 2. Точки юнитов (`DrawMap_RenderMiniMapUnitList`).
    paint_minimap_units(
        &mut layer,
        state,
        map_width,
        map_height,
        panel.minimap_tnt,
        panel.minimap_2x,
    );

    // 3. Карта в окно 112×112 (`GameManager_RenderMinimap` + `cscale`).
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

    // 4. Рамка камеры (`draw_box(..., COLOR_RED)`); в 2x-режиме C++ затирает её
    //    масштабированием, поэтому рисуем до 2x.
    let map_view = map_viewport(viewport);

    if let Some(range) = map_view.visible_tiles() {
        let scale = view.scale;
        let x0 = view.offset_x + (f64::from(range.x0) * scale) as i32;
        let y0 = view.offset_y + (f64::from(range.y0) * scale) as i32;
        let x1 = view.offset_x + ((f64::from(range.x1) + 0.9) * scale) as i32;
        let y1 = view.offset_y + ((f64::from(range.y1) + 0.9) * scale) as i32;

        draw_box_indexed(&mut window, MINIMAP_SLOT, (x0, y0, x1, y1), 0x01);
    }

    // 5. 2x-режим (`GameManager_RenderMinimap`): область 56×56 вокруг центра
    //    камеры растягивается на всё окно.
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

    // 6. Индексы → RGBA в окно `WINDOW_MINIMAP`.
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

/// Точки юнитов на слое миникарты (ровно условия C++).
fn paint_minimap_units(
    layer: &mut [u8],
    state: &GameState,
    map_width: i32,
    map_height: i32,
    tnt: bool,
    two_x: bool,
) {
    use max_core::enums::unit_flags;
    use max_core::enums::UnitOrder;

    let player_team = state.player_team;

    for id in state.unit_ids_in_order() {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        if !unit.is_visible_to_team(player_team)
            || unit.orders == UnitOrder::Idle
            || unit.flags & unit_flags::SELECTABLE == 0
        {
            continue;
        }

        if tnt && unit.flags & (unit_flags::HAS_FIRING_SPRITE | unit_flags::FIRES_MISSILES) == 0 {
            continue;
        }

        let color = minimap_unit_color(unit.team);
        let grid_x = i32::from(unit.grid.x);
        let grid_y = i32::from(unit.grid.y);

        if two_x && unit.flags & unit_flags::BUILDING == 0 {
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

/// `draw_box` по индексированному буферу (рамка камеры).
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

/// Рисует строку по центру прямоугольника шрифтом GNW.
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

        let mut state = GameState::new();
        let own = state
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
            .unwrap();
        let enemy = state
            .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(20, 20), 0)
            .unwrap();
        let mut layer = vec![0u8; MAP_TILES];

        paint_minimap_units(&mut layer, &state, 112, 112, false, false);
        assert_eq!(
            layer[10 * 112 + 10],
            minimap_unit_color(TeamId::RED),
            "свой юнит виден"
        );
        assert_eq!(layer[20 * 112 + 20], 0, "чужой вне FOV не рисуется");

        // Разведан (попал в FOV/радар) — точка появляется.
        state
            .unit_mut(enemy)
            .unwrap()
            .set_visible_to_team(TeamId::RED, true);
        layer.fill(0);
        paint_minimap_units(&mut layer, &state, 112, 112, false, false);
        assert_eq!(
            layer[20 * 112 + 20],
            minimap_unit_color(TeamId::GREEN),
            "видимый чужой юнит появляется на миникарте"
        );
        assert_eq!(layer[10 * 112 + 10], minimap_unit_color(TeamId::RED));
        let _ = own;
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

    /// Панели грузятся из настоящего `MAX.RES`, и каждая зона меняет пиксели.
    #[test]
    fn minimap_view_matches_calculate_minimap_scaling() {
        let view = minimap_view(112, 112);
        assert_eq!((view.offset_x, view.offset_y), (0, 0));
        assert_eq!((view.width, view.height), (112, 112));
        assert_eq!(view.scale, 1.0);

        // Широкая карта 224×112: масштаб 0.5, по y — центрирование.
        let view = minimap_view(224, 112);
        assert_eq!(view.scale, 0.5);
        assert_eq!((view.width, view.height), (112, 56));
        assert_eq!(view.offset_y, 28);

        // Высокая карта 56×112: по x — центрирование.
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

        // 2x: центр камеры (56,56) → offset 28, левый верхний угол показывает
        // клетку 28, центр — 56.
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

        // `max(7,100/4)*31/63 = 12`; ближайший цвет палитры — индекс 128.
        assert_eq!(table[2], 128);
        assert_eq!(table[0], 0, "чёрный остаётся чёрным");
    }

    #[test]
    fn minimap_unit_dots_use_team_colors_and_tnt_filter() {
        use max_core::enums::{unit_flags, UnitOrder, UnitType};
        use max_core::geo::Point;

        let mut state = GameState::new();
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
        paint_minimap_units(&mut layer, &state, 112, 112, false, false);

        // Точка юнита 2×2, цвет RED (индекс 1).
        assert_eq!(layer[10 * 112 + 10], 1);
        assert_eq!(layer[11 * 112 + 11], 1);

        // TNT-фильтр: у юнита нет стреляющего спрайта — точка пропадает.
        {
            let unit = state.unit_mut(id).expect("юнит");
            unit.flags &= !(unit_flags::HAS_FIRING_SPRITE | unit_flags::FIRES_MISSILES);
        }
        let mut filtered = vec![0u8; 112 * 112];
        paint_minimap_units(&mut filtered, &state, 112, 112, true, false);
        assert_eq!(filtered[10 * 112 + 10], 0);

        // В 2x не-здание рисуется одной клеткой.
        let mut zoomed = vec![0u8; 112 * 112];
        paint_minimap_units(&mut zoomed, &state, 112, 112, false, true);
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
    // Строки статистики (`GameManager_UpdateInfoDisplay`)
    // -----------------------------------------------------------------------

    /// Партия с настоящими атрибутами команды (`TeamUnits::with_clan(Random)`).
    fn stats_state() -> GameState {
        use max_core::enums::TeamClan;
        use max_core::team::TeamUnits;

        let mut state = GameState::new();
        state.team_units[TeamId::RED.0 as usize] = TeamUnits::with_clan(TeamClan::Random);
        state.player_team = TeamId::RED;

        state
    }

    /// Юнит в партии: `spawn_unit` + `DeployUnit` (у нового юнита `storage = 0`).
    fn deployed(state: &mut GameState, unit_type: UnitType) -> UnitInfo {
        let id = state
            .spawn_unit(unit_type, TeamId::RED, max_core::geo::Point::new(20, 20), 0)
            .expect("юнит создан");

        state.unit_mut(id).expect("юнит в состоянии").storage = 0;

        state.unit(id).expect("юнит").clone()
    }

    fn labels(rows: &[Option<StatRow>; 4]) -> [Option<&'static str>; 4] {
        rows.map(|row| row.map(|row| row.label.caption()))
    }

    /// Шахта (свой стационарный склад `RAW`): `Hits/Cargo/Total`, без `Speed`.
    #[test]
    fn mining_station_panel_shows_hits_cargo_total_without_speed() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::MiningStation);
        let rows = rows_for(&state, &unit, TeamId::RED);

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
        // Строка 4 (Shots) пустая: `min(ROUNDS, AMMO) == 0`.
        assert_eq!(rows[3], None);
    }

    /// Танк (мобильный наземный с атакой): `Hits/Ammo/Speed/Shots`.
    #[test]
    fn tank_panel_keeps_speed_ammo_and_shots() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::Tank);
        let rows = rows_for(&state, &unit, TeamId::RED);

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

    /// Турель (стационарная, с атакой): `Hits/Ammo/Shots`, `Speed` нет.
    #[test]
    fn turret_panel_shows_ammo_and_shots_without_speed() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::GunTurret);
        let rows = rows_for(&state, &unit, TeamId::RED);

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

    /// Разведчик: `Hits/Speed`, пустые `Ammo` и `Shots` (ATTACK = 0, ROUNDS = 0).
    #[test]
    fn surveyor_panel_skips_empty_attribute_rows() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::Surveyor);
        let rows = rows_for(&state, &unit, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), None, Some("Speed"), None],
            "C++: ATTRIB_STORAGE 0 → Cargo нет, ATTACK 0 → Ammo нет"
        );
        assert_eq!(rows[0].expect("Hits").text(), "16/16");
        assert_eq!(rows[2].expect("Speed").text(), "7/7");
    }

    /// Завод: `Hits/Usage/Total` рабочих (`DrawInfoDisplayType1`), без `Speed`.
    #[test]
    fn training_hall_panel_shows_worker_usage_and_total() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::TrainingHall);
        let rows = rows_for(&state, &unit, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), Some("Usage"), Some("Total"), None],
            "C++: ATTRIB_STORAGE 0 → Type1 (life 1), Speed 0 — пропущен"
        );
        assert!(rows
            .iter()
            .flatten()
            .all(|row| row.label != StatLabel::Speed));
        // `Cargo_GetNetProduction` без приказа = 0; база life_need = 1.
        assert_eq!(rows[1].expect("Usage").text(), "0/1");
        assert_eq!(rows[1].expect("Usage").color(), RED);
        assert_eq!(rows[2].expect("Total").text(), "0/1");
    }

    /// Электростанция: `Hits/Power/Total` (`DrawInfoDisplayType2`).
    #[test]
    fn power_station_panel_shows_power_and_total() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::PowerStation);
        let rows = rows_for(&state, &unit, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), Some("Power"), Some("Total"), None],
            "POWERSTN: `Cargo_GetPowerConsumptionRate = -6`, приказ POWER_OFF"
        );
        assert_eq!(rows[1].expect("Power").text(), "0/6");
        assert_eq!(rows[2].expect("Total").text(), "0/6");
        assert_eq!(rows[3], None, "Usage: база (потребление) 0 — строка пустая");
    }

    /// Жилой блок: `Hits/Teams/Total` рабочих (`life = -3`).
    #[test]
    fn habitat_panel_shows_teams_and_total() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::Habitat);
        let rows = rows_for(&state, &unit, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), Some("Teams"), Some("Total"), None]
        );
        assert_eq!(rows[1].expect("Teams").text(), "3/3");
        assert_eq!(rows[2].expect("Total").text(), "3/3");
        assert_eq!(rows[1].expect("Teams").color(), GREEN);
    }

    /// Чужой юнит: `Ammo`/`Cargo` не рисуются (`factor == 0` в C++).
    #[test]
    fn enemy_unit_hides_ammo_and_cargo_rows() {
        let mut state = stats_state();
        let unit = deployed(&mut state, UnitType::Tank);
        let mut enemy = unit;
        enemy.team = TeamId::GREEN;

        let rows = rows_for(&state, &enemy, TeamId::RED);

        assert_eq!(
            labels(&rows),
            [Some("Hits"), None, Some("Speed"), Some("Shots")]
        );
    }

    /// Цвета — пороги `GameManager_DrawInfoDisplayRow` (1/4 и 1/2 базы).
    #[test]
    fn stat_row_colors_follow_the_cpp_thresholds() {
        let row = |current, base, factor| StatRow {
            label: StatLabel::Hits,
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
        // `%i/%i` и `std::min(current, base)` как в C++.
        assert_eq!(
            make_row(StatLabel::Hits, 99, 56, 4).expect("строка").text(),
            "56/56"
        );
        assert_eq!(make_row(StatLabel::Hits, 0, 0, 4), None);
        assert_eq!(make_row(StatLabel::Cargo, 0, 25, 0), None);
    }

    /// `Speed`-строка есть ровно у типов с `ATTRIB_SPEED > 0` (все 93 типа).
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

            let rows = rows_for(&state, &unit, TeamId::RED);
            let has_speed = rows
                .iter()
                .flatten()
                .any(|row| row.label == StatLabel::Speed);

            assert_eq!(has_speed, base.speed > 0, "{unit_type:?}");
        }
    }
}
