//! Фаза выбора точки высадки — порт `GameManager_SelectSite`
//! (`game_manager.cpp:1324`, состояние `GAME_STATE_7_SITE_SELECT`).
//!
//! # Что делает оригинал
//!
//! После мобилизации (если у команды были кредиты) C++ открывает карту в
//! состоянии `GAME_STATE_7_SITE_SELECT` на зуме 4: вся карта 112×112 клеток
//! занимает область карты `448×448` (клетка 4 px). Игрок водит курсор и
//! подтверждает точку; `GameManager_IsValidStartingPosition` требует, чтобы
//! 4×4 клетки вокруг (`x-1 … x+2`) были **ровно сушей**. Esc отменяет старт
//! (`GAME_STATE_14_EXIT_SITE_SELECT`).
//!
//! # Что портировано
//!
//! * Карта 112×112 в области [`max_render::layout::MAIN_MAP`] с клеткой 4 px —
//!   те же координаты, что у C++ на зуме 4.
//! * Проверка точки — [`max_core::setup::is_valid_starting_position`]
//!   (`GameManager_IsValidStartingPosition`): 4×4 вокруг выбранной клетки.
//! * `Enter`/`Done` — подтверждение, `Esc`/`Cancel` — отмена.
//! * Стрелки двигают курсор на клетку.
//!
//! # Упрощения
//!
//! * Рисуется **поверхностная** карта (суша/вода/берег/воздух), а не
//!   изометрические тайлы `.WRL` из `TerrainAtlas`: на зуме C++ 4 px на клетку
//!   разница всё равно сводится к цвету клетки, а ассеты нужны не всегда.
//!   Цвета берутся из [`max_core::enums::Surface`] через
//!   [`max_render::map_render::TileStyle`].
//! * В C++ курсор — юнит `MASTER` с зоной обзора; здесь — рамка 4×4 клетки и
//!   маркер в центре.
//! * В C++ клик по валидной клетке сразу подтверждает выбор; здесь клик только
//!   двигает курсор, а подтверждают `Done`/`Enter` (удобнее и проверяемо в QA).
//! * Зоны близости чужих высадок (`GameManager_CheckLandingZones`) и
//!   `GameManager_HandleProximityOverlaps` пока не портированы.

use max_core::enums::Surface;
use max_core::geo::{Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::map::SurfaceMap;
use max_core::setup::{find_valid_starting_position, is_valid_starting_position};
use max_core::team::TeamId;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::layout::{MAIN_MAP, WINDOW_HEIGHT, WINDOW_WIDTH};
use max_render::map_render::TileStyle;
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFontId};

use crate::menu::MenuAssets;

/// Размер клетки на экране выбора точки — зум 4 оригинала
/// (`GameManager_UpdateMainMapView(MAP_VIEW_ZOOM, 4, …)`).
pub const LANDING_CELL: i32 = 4;

/// Область карты на экране: 112 × 4 = 448 — ровно `MAIN_MAP`.
pub const LANDING_AREA: PixelRect = MAIN_MAP;

/// Кнопка `Cancel` нижней строки (как на экранах настройки).
pub const CANCEL_BUTTON: PixelRect = PixelRect::new(354, 438, 109, 40);
/// Кнопка `Done` нижней строки.
pub const DONE_BUTTON: PixelRect = PixelRect::new(514, 438, 109, 40);

/// Рамка сообщения «Select starting location.» (`MessageManager_DrawMessage`).
///
/// В C++ рамка уже (190 px) и содержит одну строку; здесь в неё добавлена
/// вторая строка с подсказкой управления, поэтому она шире.
pub const MESSAGE_BOX: PixelRect = PixelRect::new(73, 30, 300, 36);

/// Итог ввода на экране высадки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingAction {
    /// Ничего не произошло.
    None,
    /// Точка выбрана и подтверждена.
    Confirm,
    /// Игрок отменил выбор.
    Cancel,
}

/// Экран выбора точки высадки одной команды.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandingScreen {
    /// Чья высадка.
    pub team: TeamId,
    /// Планета `0..=23` (для подписи).
    pub world: i64,
    /// Поверхность карты.
    pub surface: SurfaceMap,
    /// Текущая точка под курсором (`starting_position`).
    pub cursor: Point,
    /// Строка статуса.
    pub status: String,
}

impl LandingScreen {
    /// Экран с курсором в стартовой точке команды (`START_POSITIONS`).
    ///
    /// Если точка невалидна, [`find_valid_starting_position`] двигает её по
    /// спирали — ровно как C++ при инициализации миссии.
    pub fn new(team: TeamId, world: i64, surface: SurfaceMap) -> Self {
        let mut cursor = crate::local::START_POSITIONS[usize::from(team.0) % 4];
        find_valid_starting_position(&surface, &mut cursor);

        Self {
            team,
            world,
            surface,
            cursor,
            status: String::new(),
        }
    }

    /// Загружает поверхность планеты из `world_dir` (`*.WRL`).
    ///
    /// `None` — данных планеты нет; UI тогда пропускает фазу выбора точки и
    /// оставляет стандартную стартовую позицию.
    pub fn load_surface(world: i64, world_dir: &std::path::Path) -> Option<SurfaceMap> {
        let index = usize::try_from(world.clamp(0, 23)).ok()?;
        let name = crate::assets::WORLD_ASSETS.get(index)?;
        let path = world_dir.join(format!("{name}.WRL"));

        if !path.is_file() {
            return None;
        }

        let map = max_assets::wrl::WrlMap::open(&path).ok()?;

        Some(map.surface_map())
    }

    /// Загружает поверхность планеты из `world_dir` (`*.WRL`) и создаёт экран.
    ///
    /// `None` — данных планеты нет; UI тогда показывает честную ошибку.
    pub fn load(team: TeamId, world: i64, world_dir: &std::path::Path) -> Option<Self> {
        Some(Self::new(team, world, Self::load_surface(world, world_dir)?))
    }

    /// Валидна ли текущая точка (`GameManager_IsValidStartingPosition`).
    pub fn cursor_valid(&self) -> bool {
        is_valid_starting_position(&self.surface, i32::from(self.cursor.x), i32::from(self.cursor.y))
    }

    /// Двигает курсор (с клампом по карте).
    pub fn step(&mut self, dx: i32, dy: i32) {
        self.cursor.x = (i32::from(self.cursor.x) + dx).clamp(1, i32::from(MAP_WIDTH) - 4) as i16;
        self.cursor.y = (i32::from(self.cursor.y) + dy).clamp(1, i32::from(MAP_HEIGHT) - 4) as i16;
        self.status.clear();
    }

    /// Переводит точку кадра в клетку карты (область [`LANDING_AREA`]).
    pub fn screen_to_grid(x: i32, y: i32) -> Option<Point> {
        if !LANDING_AREA.contains(x, y) {
            return None;
        }

        Some(Point::new(
            ((x - LANDING_AREA.ulx) / LANDING_CELL) as i16,
            ((y - LANDING_AREA.uly) / LANDING_CELL) as i16,
        ))
    }

    /// Обработка клика: кнопки, затем карта.
    pub fn click(&mut self, x: i32, y: i32) -> LandingAction {
        if DONE_BUTTON.contains(x, y) {
            return self.confirm();
        }
        if CANCEL_BUTTON.contains(x, y) {
            return LandingAction::Cancel;
        }

        if let Some(point) = Self::screen_to_grid(x, y) {
            self.cursor = point;
            self.status = if self.cursor_valid() {
                format!("Landing site: {},{}", self.cursor.x + 1, self.cursor.y + 1)
            } else {
                "Invalid landing site: need 4x4 land".to_string()
            };
        }

        LandingAction::None
    }

    /// Подтверждение текущей точки (`Enter`/`Done`).
    pub fn confirm(&mut self) -> LandingAction {
        if self.cursor_valid() {
            self.status = format!("Landing site: {},{}", self.cursor.x + 1, self.cursor.y + 1);
            LandingAction::Confirm
        } else {
            self.status = "Invalid landing site: need 4x4 land".to_string();
            LandingAction::None
        }
    }

    /// Отрисовка: карта 4 px/клетка, рамка сообщения, курсор, кнопки.
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        menu: &MenuAssets,
        chrome: Option<&crate::chrome::Chrome>,
        status: &str,
        mouse: Option<(i32, i32)>,
    ) -> u32 {
        let palette = &menu.palette;
        let style = TileStyle::default();
        let mut painted = 0;

        frame.fill_rect(
            PixelRect::new(0, 0, WINDOW_WIDTH, WINDOW_HEIGHT),
            Rgba::rgb(12, 12, 14),
        );

        if let Some(chrome) = chrome.filter(|chrome| chrome.is_ready()) {
            painted += chrome.draw(frame);
        }

        // Карта: 112 × 4 = 448 px ровно в область MAIN_MAP.
        for grid_y in 0..MAP_HEIGHT {
            for grid_x in 0..MAP_WIDTH {
                let surface = self.surface.get(grid_x, grid_y);
                let color = if surface == Surface::NONE {
                    style.unknown
                } else {
                    style.base_color(surface)
                };
                let rect = PixelRect::new(
                    LANDING_AREA.ulx + i32::from(grid_x) * LANDING_CELL,
                    LANDING_AREA.uly + i32::from(grid_y) * LANDING_CELL,
                    LANDING_CELL,
                    LANDING_CELL,
                );

                frame.fill_rect(rect, color);
            }
        }

        // Курсор: рамка 4×4 клетки (след юнита MASTER в C++) + маркер центра.
        let valid = self.cursor_valid();
        let color = if valid {
            Rgba::rgb(64, 232, 96)
        } else {
            Rgba::rgb(232, 64, 64)
        };
        let cursor = PixelRect::new(
            LANDING_AREA.ulx + i32::from(self.cursor.x) * LANDING_CELL,
            LANDING_AREA.uly + i32::from(self.cursor.y) * LANDING_CELL,
            LANDING_CELL * 4,
            LANDING_CELL * 4,
        );

        draw_box(frame, cursor, color, 1);
        let marker = PixelRect::new(cursor.ulx + 6, cursor.uly + 6, 4, 4);
        frame.fill_rect(marker, color);

        // Рамка сообщения и текст — как `MessageManager_DrawMessage`. Вторая
        // строка — подсказка по управлению (в C++ её нет: там только текст
        // «Select starting location.»).
        frame.fill_rect(MESSAGE_BOX, Rgba::rgb(0, 0, 0));
        draw_box(frame, MESSAGE_BOX, Rgba::rgb(240, 240, 240), 1);
        draw_text_gnw(
            frame,
            MESSAGE_BOX.ulx + 4,
            MESSAGE_BOX.uly + 4,
            GnwFontId::Font5.font(),
            "Select starting location.",
            Rgba::rgb(240, 240, 240),
        );
        draw_text_gnw(
            frame,
            MESSAGE_BOX.ulx + 4,
            MESSAGE_BOX.uly + 16,
            GnwFontId::Font5.font(),
            "Click = move, Enter/Done = confirm, Esc = cancel",
            Rgba::rgb(180, 200, 180),
        );

        let team = crate::hotseat::team_display_name(self.team);
        let line = if status.is_empty() {
            self.status.as_str()
        } else {
            status
        };
        let text = if line.is_empty() {
            format!("{team}: select landing site")
        } else {
            line.chars().take(26).collect()
        };

        draw_text_gnw(
            frame,
            11,
            424,
            GnwFontId::Font5.font(),
            &text,
            palette.rgba(crate::setup::GREEN),
        );

        // Confirm/Cancel: те же кнопки MNUBTN, что на экранах настройки.
        for (rect, label, resource) in [
            (CANCEL_BUTTON, "Cancel", 4usize),
            (DONE_BUTTON, "Done", 6usize),
        ] {
            if let Some(pair) = menu.mnubtn_pair(resource) {
                let hovered = mouse.is_some_and(|(x, y)| rect.contains(x, y));
                painted += crate::menu::draw_image_button(
                    frame,
                    palette,
                    pair.image(false),
                    pair.image(true),
                    rect,
                    label,
                    hovered,
                );
            } else {
                frame.fill_rect(rect, palette.rgba(crate::setup::TEXT));
                draw_text_gnw_centered(
                    frame,
                    rect,
                    GnwFontId::Font1.font(),
                    label,
                    palette.rgba(crate::setup::GREEN),
                );
                painted += 1;
            }
        }

        painted
    }
}

/// Рамка `thickness` внутри прямоугольника (как в `setup.rs`).
fn draw_box(frame: &mut RgbaBuffer, rect: PixelRect, color: Rgba, thickness: i32) {
    for inset in 0..thickness.max(1) {
        let x = rect.ulx + inset;
        let y = rect.uly + inset;
        let width = rect.width - inset * 2;
        let height = rect.height - inset * 2;

        if width <= 0 || height <= 0 {
            break;
        }

        for dx in 0..width {
            frame.set(x + dx, y, color);
            frame.set(x + dx, y + height - 1, color);
        }
        for dy in 0..height {
            frame.set(x, y + dy, color);
            frame.set(x + width - 1, y + dy, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Карта: всё суша, кроме вертикальной полосы воды в колонках 50..52.
    fn surface() -> SurfaceMap {
        let mut surface = SurfaceMap::new();

        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                let tile = if (50..53).contains(&x) {
                    Surface::WATER
                } else {
                    Surface::LAND
                };
                *surface.tiles_mut().get_mut(y as usize * MAP_WIDTH as usize + x as usize).unwrap() =
                    tile;
            }
        }

        surface
    }

    #[test]
    fn landing_accepts_only_4x4_land() {
        let mut landing = LandingScreen::new(TeamId::RED, 0, surface());

        landing.cursor = Point::new(20, 20);
        assert!(landing.cursor_valid());
        assert_eq!(landing.confirm(), LandingAction::Confirm);

        landing.cursor = Point::new(51, 20);
        assert!(!landing.cursor_valid());
        assert_eq!(landing.confirm(), LandingAction::None);
        assert!(landing.status.contains("Invalid"));
    }

    #[test]
    fn clicks_map_frame_coordinates_to_grid() {
        assert_eq!(
            LandingScreen::screen_to_grid(LANDING_AREA.ulx, LANDING_AREA.uly),
            Some(Point::new(0, 0))
        );
        assert_eq!(
            LandingScreen::screen_to_grid(
                LANDING_AREA.ulx + 4 * 10,
                LANDING_AREA.uly + 4 * 20
            ),
            Some(Point::new(10, 20))
        );
        assert_eq!(LandingScreen::screen_to_grid(0, 0), None, "вне карты");
    }

    #[test]
    fn buttons_confirm_and_cancel() {
        let mut landing = LandingScreen::new(TeamId::RED, 0, surface());
        landing.cursor = Point::new(30, 30);

        assert_eq!(
            landing.click(DONE_BUTTON.ulx + 2, DONE_BUTTON.uly + 2),
            LandingAction::Confirm
        );
        assert_eq!(
            landing.click(CANCEL_BUTTON.ulx + 2, CANCEL_BUTTON.uly + 2),
            LandingAction::Cancel
        );
    }

    #[test]
    fn arrows_move_the_cursor_within_the_map() {
        let mut landing = LandingScreen::new(TeamId::RED, 0, surface());

        landing.cursor = Point::new(5, 5);
        landing.step(1, 0);
        assert_eq!(landing.cursor, Point::new(6, 5));

        landing.step(-100, -100);
        assert_eq!(landing.cursor, Point::new(1, 1), "кламп по краю карты");
    }

    #[test]
    fn default_cursor_is_a_valid_site() {
        let landing = LandingScreen::new(TeamId::BLUE, 0, surface());

        assert!(
            landing.cursor_valid(),
            "find_valid_starting_position обязан дать валидную точку"
        );
    }
}
