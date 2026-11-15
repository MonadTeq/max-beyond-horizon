//! Наполнение панелей окна игры 1:1 (пункт A `docs/visual-1to1-status.md`).
//!
//! Поверх `FRAMEPIC` оригинал (`game_manager.cpp`) дорисовывает картинки
//! кнопок/панелей из `MAX.RES` и текст шрифтами GNW. Здесь это собрано в
//! [`Panels`]:
//!
//! * верх: `FILES_OF/ON` (17,4) 67×18, `PREF_OFF/ON` (86,4) 67×18,
//!   `ENDTRN_U` (390,2) 72×21, `TURNS` (468,2) 61×21, `TIMER` (534,2) 61×21;
//! * портрет `CORNER_FLIC` (10,29) 128×128 — статичный кадр спрайта юнита;
//! * статистика `STAT_WINDOW` (10,173) 153×48 — четыре строки `Hits/Ammo/
//!   Speed/Shots` шрифтом `GNW_TEXT_FONT_2`;
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
use max_core::game::GameState;
use max_core::unit::UnitInfo;
use max_core::unit_values::{ATTRIB_AMMO, ATTRIB_HITS, ATTRIB_ROUNDS, ATTRIB_SPEED};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::layout;
use max_render::palette::{ColorLut, TeamColors, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;
use max_render::text::{
    draw_text_gnw, draw_text_gnw_shadowed, GnwFont, GnwFontId,
};
use max_render::viewport::Viewport;

use crate::assets::Assets;

/// Цвет обычного текста панелей (`0xA2`).
pub const TEXT: u8 = 0xA2;
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
        self.colors
            .map(|color| [color[0], color[1], color[2], 255])
    }
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
        self.draw_pair(frame, palette, &self.files, layout::FILES_BUTTON, false, &mut painted);
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
        painted += self.blit(frame, palette, &self.end_turn, layout::END_TURN_BUTTON.ulx, layout::END_TURN_BUTTON.uly, true);
        self.draw_caption(frame, palette, PanelButton::EndTurn);

        // Номер хода и таймер.
        painted += self.blit(frame, palette, &self.turns, layout::TURN_COUNTER.ulx, layout::TURN_COUNTER.uly, true);
        draw_centered(
            frame,
            layout::TURN_COUNTER,
            GnwFontId::Font5.font(),
            &format!("{}", state.turn_counter.max(0)),
            palette.rgba(TEXT),
        );
        painted += self.blit(frame, palette, &self.timer, layout::TURN_TIMER.ulx, layout::TURN_TIMER.uly, true);
        let timer_color = if panel.timer_seconds <= 20 && panel.timer_seconds > 0 {
            palette.rgba(0x01)
        } else {
            palette.rgba(TEXT)
        };
        draw_centered(
            frame,
            layout::TURN_TIMER,
            GnwFontId::Font5.font(),
            &format!("{:02}:{:02}", panel.timer_seconds / 60, panel.timer_seconds % 60),
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

        if let Some(sprite) = assets.sprite(unit.unit_type) {
            let Some(sprite_frame) = sprite.frame_for(unit.image_base, unit.angle) else {
                return painted;
            };
            let shadow = sprite.shadow_for(unit.image_base, unit.angle);
            let colors = TeamColors::ALL
                .get(unit.team.0 as usize)
                .copied()
                .unwrap_or(TeamColors::Derelict);
            let level = unit.brightness.min(FULL_BRIGHTNESS_LEVEL);
            let renderer = SpriteRenderer::new(
                &assets.palette,
                &assets.brightness,
                ColorLut::for_team(colors),
                level,
            );
            // Спрайт рисуется в буфер портрета и увеличивается вдвое вокруг
            // центра: статичная замена FLIC-анимации, но заметно крупнее
            // нативной иконки 50×50.
            let mut portrait = RgbaBuffer::filled(area.width as u32, area.height as u32, Rgba::BLACK);
            let anchor = (area.width / 2, area.height / 2);

            renderer.draw_unit(
                &mut portrait,
                sprite_frame,
                shadow,
                anchor,
                (
                    i32::from(unit.shadow_offset.x),
                    i32::from(unit.shadow_offset.y),
                ),
            );

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

        // Имя и статус в верхней части портрета (`GameManager_UpdateInfoDisplay`).
        let font = GnwFontId::Font2.font();
        draw_text_gnw(
            frame,
            area.ulx,
            area.uly,
            font,
            &unit_name(unit),
            palette.rgba(TEXT),
        );
        draw_text_gnw(
            frame,
            area.ulx,
            area.uly + font.height(),
            font,
            if unit.hits > 0 { "Awaiting" } else { "Destroyed" },
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

/// Одна строка статистики (`GameManager_DrawInfoDisplayRow`).
fn draw_stat_row(
    frame: &mut RgbaBuffer,
    row: PixelRect,
    label: &str,
    current: i32,
    base: i32,
    palette: &UiPalette,
) {
    let base_value = base.max(0);
    let current_value = current.clamp(0, base_value.max(0));
    let color = if base_value > 0 {
        if current_value <= base_value / 4 {
            0x01
        } else if current_value <= base_value / 2 {
            0x04
        } else {
            0x02
        }
    } else {
        TEXT
    };
    let font = GnwFontId::Font2.font();
    let value = format!("{current_value}/{base_value}");
    let value_rect = PixelRect::new(row.ulx, row.uly, 45, row.height);

    draw_centered(frame, value_rect, font, &value, palette.rgba(color));
    draw_text_gnw(
        frame,
        row.ulx + 45,
        row.uly + ((row.height + 1) - font.height()).max(0) / 2,
        font,
        label,
        palette.rgba(TEXT),
    );
}

/// Четыре строки статистики выбранного юнита.
fn draw_stats(
    frame: &mut RgbaBuffer,
    _state: &GameState,
    selected: Option<&UnitInfo>,
    palette: &UiPalette,
) -> u32 {
    let Some(unit) = selected else {
        return 0;
    };
    let values = &unit.base_values;
    let max_hits = values.get_attribute(ATTRIB_HITS);
    let max_ammo = values.get_attribute(ATTRIB_AMMO);
    let max_speed = values.get_attribute(ATTRIB_SPEED);
    let max_shots = values
        .get_attribute(ATTRIB_ROUNDS)
        .min(values.get_attribute(ATTRIB_AMMO));

    draw_stat_row(
        frame,
        layout::STAT_ROW_1,
        "Hits",
        i32::from(unit.hits),
        max_hits,
        palette,
    );
    draw_stat_row(
        frame,
        layout::STAT_ROW_2,
        "Ammo",
        i32::from(unit.ammo),
        max_ammo,
        palette,
    );
    draw_stat_row(
        frame,
        layout::STAT_ROW_3,
        "Speed",
        i32::from(unit.speed),
        max_speed,
        palette,
    );
    draw_stat_row(
        frame,
        layout::STAT_ROW_4,
        "Shots",
        i32::from(unit.shots),
        max_shots,
        palette,
    );

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
pub fn minimap_center_offset(
    viewport: &Viewport,
    map_width: i32,
    map_height: i32,
) -> (i32, i32) {
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

        draw_box_indexed(
            &mut window,
            MINIMAP_SLOT,
            (x0, y0, x1, y1),
            0x01,
        );
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
        assert_eq!(
            PanelButton::Files.rect(),
            PixelRect::new(17, 4, 67, 18)
        );
        assert_eq!(
            PanelButton::EndTurn.rect(),
            PixelRect::new(390, 2, 72, 21)
        );
        assert_eq!(
            PanelButton::ZoomSlider.rect(),
            PixelRect::new(20, 275, 130, 14)
        );
        assert_eq!(
            PanelButton::Survey.rect(),
            PixelRect::new(4, 298, 55, 16)
        );
        assert_eq!(
            PanelButton::Names.rect(),
            PixelRect::new(114, 330, 55, 18)
        );
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
}
