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

        // Кнопки миникарты.
        self.draw_pair(
            frame,
            palette,
            &self.minimap_2x,
            layout::MINIMAP_2X,
            panel.minimap_2x,
            &mut painted,
        );
        self.draw_pair(
            frame,
            palette,
            &self.minimap_tnt,
            layout::MINIMAP_TNT,
            panel.minimap_tnt,
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
        painted += draw_minimap(frame, assets, viewport, palette);

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

/// Миникарта из `minimap`-слоя `.WRL` (112×112 у стандартных планет).
fn draw_minimap(
    frame: &mut RgbaBuffer,
    assets: &Assets,
    viewport: &Viewport,
    _palette: &UiPalette,
) -> u32 {
    let world = &assets.world;
    let minimap = world.minimap();
    let width = i32::from(world.width());
    let height = i32::from(world.height());

    if minimap.len() < (width * height) as usize || width <= 0 || height <= 0 {
        return 0;
    }

    let area = layout::MINIMAP;
    let wrl_palette = max_render::terrain::palette_from_wrl(world.palette());
    let mut painted = 0u32;

    for y in 0..area.height {
        for x in 0..area.width {
            let source_x = x * width / area.width;
            let source_y = y * height / area.height;
            let index = minimap[(source_y * width + source_x) as usize];
            let [r, g, b, a] = wrl_palette[usize::from(index)];

            if frame.set(area.ulx + x, area.uly + y, Rgba::from_rgba([r, g, b, a])) {
                painted += 1;
            }
        }
    }

    // Рамка камеры (как `GameManager_RenderMinimap`).
    let map_view = map_viewport(viewport);

    if let Some(range) = map_view.visible_tiles() {
        let x0 = area.ulx + i32::from(range.x0) * area.width / width;
        let y0 = area.uly + i32::from(range.y0) * area.height / height;
        let x1 = area.ulx + (i32::from(range.x1) + 1) * area.width / width;
        let y1 = area.uly + (i32::from(range.y1) + 1) * area.height / height;
        let color = Rgba::rgb(252, 0, 0);

        for x in x0..x1.min(area.lrx()) {
            frame.set(x, y0, color);
            frame.set(x, y1.min(area.lry() - 1), color);
        }
        for y in y0..y1.min(area.lry()) {
            frame.set(x0, y, color);
            frame.set(x1.min(area.lrx() - 1), y, color);
        }
    }

    painted
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
