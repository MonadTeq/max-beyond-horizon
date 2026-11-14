//! Камера и выбор видимого: преобразование координат карта → экран.
//!
//! Карта M.A.X. — сетка [`MAP_WIDTH`]×[`MAP_HEIGHT`] клеток по 64 пикселя
//! (`GFX_MAP_TILE_SIZE`). Камера хранит центр в «мировых» пикселях карты
//! (`0..MAP_WIDTH*64`) и масштаб — сколько **экранных** пикселей приходится на
//! одну клетку (`zoom`); в оригинале это `Gfx_ZoomLevel`, где 64 — масштаб 1:1.
//!
//! Вся арифметика целочисленная (как в C++-порте: 16.16 fixed point), поэтому
//! преобразования детерминированы и тестируются без окна. Углы экрана и карты
//! клиппуются: камера не уезжает за карту.
//!
//! Порядок отрисовки — по «земле» юнита: сначала верхние (меньший `y`), потом
//! нижние, чтобы нижний юнит перекрывал верхнего (§5.4 spw.md).

use max_core::geo::{MAP_HEIGHT, MAP_WIDTH};

use crate::buffer::PixelRect;

/// Пикселей карты в одной клетке (`GFX_MAP_TILE_SIZE`).
pub const MAP_TILE_PIXELS: i32 = 64;
/// Минимальный масштаб: 8 экранных пикселей на клетку.
pub const MIN_ZOOM: u32 = 8;
/// Максимальный масштаб: 128 экранных пикселей на клетку.
pub const MAX_ZOOM: u32 = 128;
/// Масштаб по умолчанию — 1:1 (как `Gfx_ZoomLevel` в оригинале).
pub const DEFAULT_ZOOM: u32 = 64;
/// Шаг изменения масштаба (кнопки приближения/отдаления).
pub const ZOOM_STEP: u32 = 2;

/// Диапазон видимых клеток (включительно по обоим углам).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRange {
    /// Минимальный x.
    pub x0: i16,
    /// Минимальный y.
    pub y0: i16,
    /// Максимальный x (включительно).
    pub x1: i16,
    /// Максимальный y (включительно).
    pub y1: i16,
}

impl TileRange {
    /// Диапазон из двух углов в любом порядке; пустой диапазон даёт `None`.
    pub fn new(x0: i16, y0: i16, x1: i16, y1: i16) -> Option<Self> {
        if x0 > x1 || y0 > y1 {
            return None;
        }

        Some(Self { x0, y0, x1, y1 })
    }

    /// Ширина в клетках.
    #[inline]
    pub fn width(&self) -> i16 {
        self.x1 - self.x0 + 1
    }

    /// Высота в клетках.
    #[inline]
    pub fn height(&self) -> i16 {
        self.y1 - self.y0 + 1
    }

    /// Число клеток.
    #[inline]
    pub fn count(&self) -> usize {
        self.width() as usize * self.height() as usize
    }

    /// Клетка входит в диапазон.
    #[inline]
    pub fn contains(&self, x: i16, y: i16) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    /// Клетки диапазона в порядке отрисовки (строки сверху вниз, внутри
    /// строки — слева направо).
    pub fn cells(&self) -> impl Iterator<Item = (i16, i16)> + '_ {
        (self.y0..=self.y1).flat_map(move |y| (self.x0..=self.x1).map(move |x| (x, y)))
    }
}

/// Камера: размер экрана, центр в мировых пикселях и масштаб.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    screen_width: u32,
    screen_height: u32,
    center_x: i32,
    center_y: i32,
    zoom: u32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new(640, 480)
    }
}

impl Viewport {
    /// Экран `width × height`, центр — центр карты, масштаб [`DEFAULT_ZOOM`].
    pub fn new(width: u32, height: u32) -> Self {
        let mut viewport = Self {
            screen_width: width,
            screen_height: height,
            center_x: MAP_WIDTH as i32 * MAP_TILE_PIXELS / 2,
            center_y: MAP_HEIGHT as i32 * MAP_TILE_PIXELS / 2,
            zoom: DEFAULT_ZOOM,
        };

        viewport.clamp_center();

        viewport
    }

    /// Экран с явным центром (в мировых пикселях).
    pub fn with_center(width: u32, height: u32, center: (i32, i32), zoom: u32) -> Self {
        let mut viewport = Self {
            screen_width: width,
            screen_height: height,
            center_x: center.0,
            center_y: center.1,
            zoom: zoom.clamp(MIN_ZOOM, MAX_ZOOM),
        };

        viewport.clamp_center();

        viewport
    }

    /// Размер экрана `(width, height)`.
    #[inline]
    pub fn screen(&self) -> (u32, u32) {
        (self.screen_width, self.screen_height)
    }

    /// Центр в мировых пикселях.
    #[inline]
    pub fn center(&self) -> (i32, i32) {
        (self.center_x, self.center_y)
    }

    /// Масштаб: экранных пикселей на клетку.
    #[inline]
    pub fn zoom(&self) -> u32 {
        self.zoom
    }

    /// Размер клетки на экране в пикселях.
    #[inline]
    pub fn tile_pixels(&self) -> i32 {
        self.zoom as i32
    }

    /// Масштаб 1:1 (клетка 64 экранных пикселя).
    #[inline]
    pub fn is_native_zoom(&self) -> bool {
        self.zoom == DEFAULT_ZOOM
    }

    /// Устанавливает масштаб с клампом; возвращает фактический масштаб.
    pub fn set_zoom(&mut self, zoom: u32) -> u32 {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.clamp_center();
        self.zoom
    }

    /// Приближает на [`ZOOM_STEP`] (с клампом).
    pub fn zoom_in(&mut self) -> u32 {
        self.set_zoom(self.zoom * ZOOM_STEP)
    }

    /// Отдаляет на [`ZOOM_STEP`] (с клампом).
    pub fn zoom_out(&mut self) -> u32 {
        self.set_zoom(self.zoom / ZOOM_STEP)
    }

    /// Мировой пиксель, который виден в левом верхнем углу экрана.
    pub fn origin(&self) -> (i32, i32) {
        let half_width_world = (i64::from(self.screen_width) * i64::from(MAP_TILE_PIXELS)
            / (2 * i64::from(self.zoom))) as i32;
        let half_height_world = (i64::from(self.screen_height) * i64::from(MAP_TILE_PIXELS)
            / (2 * i64::from(self.zoom))) as i32;

        (
            self.center_x - half_width_world,
            self.center_y - half_height_world,
        )
    }

    /// Мировые пиксели -> пиксели экрана.
    pub fn world_to_screen(&self, world_x: i32, world_y: i32) -> (i32, i32) {
        let (origin_x, origin_y) = self.origin();
        let zoom = i64::from(self.zoom);

        (
            (((i64::from(world_x) - i64::from(origin_x)) * zoom) / i64::from(MAP_TILE_PIXELS))
                as i32,
            (((i64::from(world_y) - i64::from(origin_y)) * zoom) / i64::from(MAP_TILE_PIXELS))
                as i32,
        )
    }

    /// Пиксели экрана -> мировые пиксели.
    pub fn screen_to_world(&self, screen_x: i32, screen_y: i32) -> (i32, i32) {
        let (origin_x, origin_y) = self.origin();
        let zoom = i64::from(self.zoom);

        (
            origin_x + ((i64::from(screen_x) * i64::from(MAP_TILE_PIXELS)) / zoom) as i32,
            origin_y + ((i64::from(screen_y) * i64::from(MAP_TILE_PIXELS)) / zoom) as i32,
        )
    }

    /// Левый верхний угол клетки на экране.
    pub fn grid_to_screen(&self, grid_x: i16, grid_y: i16) -> (i32, i32) {
        self.world_to_screen(
            i32::from(grid_x) * MAP_TILE_PIXELS,
            i32::from(grid_y) * MAP_TILE_PIXELS,
        )
    }

    /// Клетка под точкой экрана (может быть за картой — не клампится).
    pub fn screen_to_grid(&self, screen_x: i32, screen_y: i32) -> (i16, i16) {
        let (world_x, world_y) = self.screen_to_world(screen_x, screen_y);

        (
            (world_x.div_euclid(MAP_TILE_PIXELS)) as i16,
            (world_y.div_euclid(MAP_TILE_PIXELS)) as i16,
        )
    }

    /// Прямоугольник клетки на экране (размер — [`Viewport::tile_pixels`]).
    pub fn tile_rect(&self, grid_x: i16, grid_y: i16) -> PixelRect {
        let (screen_x, screen_y) = self.grid_to_screen(grid_x, grid_y);

        PixelRect::new(screen_x, screen_y, self.tile_pixels(), self.tile_pixels())
    }

    /// Центрирует камеру на клетке.
    pub fn center_on_grid(&mut self, grid_x: i16, grid_y: i16) {
        self.center_x = i32::from(grid_x) * MAP_TILE_PIXELS + MAP_TILE_PIXELS / 2;
        self.center_y = i32::from(grid_y) * MAP_TILE_PIXELS + MAP_TILE_PIXELS / 2;
        self.clamp_center();
    }

    /// Центрирует камеру на мировом пикселе.
    pub fn center_on_world(&mut self, world_x: i32, world_y: i32) {
        self.center_x = world_x;
        self.center_y = world_y;
        self.clamp_center();
    }

    /// Сдвигает камеру на экранную дельту (например, мышью).
    ///
    /// Экранные пиксели переводятся в мировые по текущему масштабу, поэтому
    /// содержимое «едет» ровно за курсором; результат клампится по карте.
    pub fn scroll_by_screen(&mut self, delta_x: i32, delta_y: i32) {
        let world_x =
            (i64::from(delta_x) * i64::from(MAP_TILE_PIXELS) / i64::from(self.zoom)) as i32;
        let world_y =
            (i64::from(delta_y) * i64::from(MAP_TILE_PIXELS) / i64::from(self.zoom)) as i32;

        self.center_x += world_x;
        self.center_y += world_y;
        self.clamp_center();
    }

    /// Не даёт камере выйти за карту (с учётом половины экрана).
    pub fn clamp_center(&mut self) {
        let map_width_world = i32::from(MAP_WIDTH) * MAP_TILE_PIXELS;
        let map_height_world = i32::from(MAP_HEIGHT) * MAP_TILE_PIXELS;
        let half_width_world = (i64::from(self.screen_width) * i64::from(MAP_TILE_PIXELS)
            / (2 * i64::from(self.zoom))) as i32;
        let half_height_world = (i64::from(self.screen_height) * i64::from(MAP_TILE_PIXELS)
            / (2 * i64::from(self.zoom))) as i32;

        if map_width_world <= 2 * half_width_world {
            self.center_x = map_width_world / 2;
        } else {
            self.center_x = self
                .center_x
                .clamp(half_width_world, map_width_world - half_width_world);
        }

        if map_height_world <= 2 * half_height_world {
            self.center_y = map_height_world / 2;
        } else {
            self.center_y = self
                .center_y
                .clamp(half_height_world, map_height_world - half_height_world);
        }
    }

    /// Видимый диапазон клеток (обрезан по карте); `None`, если камера смотрит
    /// мимо карты.
    pub fn visible_tiles(&self) -> Option<TileRange> {
        let (x0, y0) = self.screen_to_grid(0, 0);
        let (x1, y1) =
            self.screen_to_grid(self.screen_width as i32 - 1, self.screen_height as i32 - 1);

        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(MAP_WIDTH - 1);
        let y1 = y1.min(MAP_HEIGHT - 1);

        TileRange::new(x0, y0, x1, y1)
    }

    /// Клетка видна на экране.
    pub fn is_tile_visible(&self, grid_x: i16, grid_y: i16) -> bool {
        self.visible_tiles()
            .is_some_and(|range| range.contains(grid_x, grid_y))
    }

    /// Экранный прямоугольник пересекается с экраном.
    pub fn is_rect_visible(&self, rect: PixelRect) -> bool {
        rect.intersect(PixelRect::new(
            0,
            0,
            self.screen_width as i32,
            self.screen_height as i32,
        ))
        .is_some()
    }

    /// Индексы юнитов, видимых в текущем кадре, в порядке отрисовки §5.4.
    ///
    /// `margin_tiles` расширяет видимую область, чтобы спрайты, чья горячая
    /// точка за экраном, но «тело» на экране, не исчезали.
    pub fn visible_units(&self, units: &[(i16, i16)], margin_tiles: i16) -> Vec<usize> {
        let Some(range) = self.visible_tiles() else {
            return Vec::new();
        };
        let expanded = TileRange {
            x0: range.x0.saturating_sub(margin_tiles),
            y0: range.y0.saturating_sub(margin_tiles),
            x1: range.x1.saturating_add(margin_tiles),
            y1: range.y1.saturating_add(margin_tiles),
        };

        let mut indices: Vec<usize> = units
            .iter()
            .enumerate()
            .filter(|(_, &(x, y))| expanded.contains(x, y))
            .map(|(index, _)| index)
            .collect();

        indices.sort_by_key(|&index| (units[index].1, units[index].0));

        indices
    }
}

/// Порядок отрисовки набора позиций: по «земле» (`y`), затем по `x`.
///
/// Возвращает индексы исходного среза; сортировка устойчивая, поэтому юниты
/// с одинаковой позицией сохраняют порядок списка.
pub fn draw_order(units: &[(i16, i16)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..units.len()).collect();

    order.sort_by_key(|&index| (units[index].1, units[index].0));

    order
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Экран 512×384 при масштабе 64: половина экрана — (256, 192) мировых
    /// пикселей, поэтому центр карты (3584, 3584) даёт ровно клеточный угол
    /// (3328, 3392) = (52, 53).
    fn test_viewport() -> Viewport {
        Viewport::new(512, 384)
    }

    #[test]
    fn default_viewport_shows_the_map_centre_at_native_zoom() {
        let viewport = test_viewport();

        assert_eq!(viewport.screen(), (512, 384));
        assert_eq!(viewport.zoom(), DEFAULT_ZOOM);
        assert!(viewport.is_native_zoom());
        assert_eq!(viewport.tile_pixels(), 64);
        // Центр карты 112*64/2 = 3584.
        assert_eq!(viewport.center(), (3584, 3584));
        assert_eq!(viewport.origin(), (3328, 3392));
        assert_eq!(viewport.grid_to_screen(52, 53), (0, 0));
        assert_eq!(viewport.grid_to_screen(53, 53), (64, 0));

        // Видимы клетки 52..=59 по x и 53..=58 по y (8 × 6 = 48 клеток).
        let range = viewport.visible_tiles().unwrap();

        assert_eq!((range.x0, range.y0, range.x1, range.y1), (52, 53, 59, 58));
        assert_eq!(range.count(), 48);
        assert!(viewport.is_tile_visible(52, 53));
        assert!(!viewport.is_tile_visible(51, 53));
        assert!(!viewport.is_tile_visible(52, 59));
    }

    #[test]
    fn zoom_scales_tiles_and_changes_the_visible_range() {
        let mut viewport = test_viewport();

        assert_eq!(viewport.zoom_out(), 32);
        assert_eq!(viewport.tile_pixels(), 32);
        // При zoom 32 в кадр входит 512/32 = 16 клеток по x и 384/32 = 12 по y.
        let range = viewport.visible_tiles().unwrap();

        assert_eq!(range.width(), 16);
        assert_eq!(range.height(), 12);
        assert_eq!((range.x0, range.y0), (48, 50));

        assert_eq!(viewport.zoom_in(), 64);
        assert_eq!(viewport.zoom_in(), 128);
        assert_eq!(viewport.zoom_in(), MAX_ZOOM, "масштаб клампится сверху");
        assert_eq!(viewport.zoom_out(), 64);
        assert_eq!(viewport.set_zoom(1), MIN_ZOOM, "масштаб клампится снизу");
        assert_eq!(viewport.set_zoom(1000), MAX_ZOOM);
    }

    #[test]
    fn tile_rects_and_screen_to_grid_roundtrip() {
        let viewport = test_viewport();

        let rect = viewport.tile_rect(52, 53);

        assert_eq!(rect, PixelRect::new(0, 0, 64, 64));
        assert_eq!(viewport.screen_to_grid(0, 0), (52, 53));
        assert_eq!(viewport.screen_to_grid(63, 63), (52, 53));
        assert_eq!(viewport.screen_to_grid(64, 64), (53, 54));
        assert!(viewport.is_rect_visible(rect));

        // Клетка за правой границей экрана не видна.
        assert_eq!(viewport.tile_rect(60, 53), PixelRect::new(512, 0, 64, 64));
        assert!(!viewport.is_rect_visible(viewport.tile_rect(60, 53)));
        assert!(!viewport.is_rect_visible(PixelRect::new(1000, 1000, 64, 64)));
        assert!(!viewport.is_tile_visible(10, 10));
    }

    #[test]
    fn camera_clamps_to_the_map() {
        let mut viewport = test_viewport();

        viewport.center_on_grid(0, 0);
        // Половина экрана — (256, 192) мировых пикселей, ближе к краю нельзя.
        assert_eq!(viewport.origin(), (0, 0));
        assert_eq!(viewport.center(), (256, 192));

        viewport.center_on_grid(MAP_WIDTH - 1, MAP_HEIGHT - 1);
        let range = viewport.visible_tiles().unwrap();

        assert_eq!(range.x1, MAP_WIDTH - 1);
        assert_eq!(range.y1, MAP_HEIGHT - 1);

        // Сдвиг за пределы карты тоже клампится.
        viewport.scroll_by_screen(-100_000, -100_000);
        assert_eq!(viewport.origin(), (0, 0));
    }

    #[test]
    fn scroll_by_screen_moves_the_origin_by_the_delta() {
        let mut viewport = test_viewport();
        let before = viewport.origin();

        viewport.scroll_by_screen(64, 32);

        assert_eq!(viewport.origin(), (before.0 + 64, before.1 + 32));
        assert_eq!(viewport.center(), (3584 + 64, 3584 + 32));

        // При zoom 32 один экранный пиксель — два мировых.
        viewport.set_zoom(32);
        let before = viewport.origin();

        viewport.scroll_by_screen(32, 0);

        assert_eq!(viewport.origin(), (before.0 + 64, before.1));
    }

    #[test]
    fn draw_order_is_by_ground_then_x() {
        let units = [(10, 5), (3, 9), (7, 5), (1, 9), (0, 1)];

        // y = 1: (0,1); y = 5: (7,5) и (10,5); y = 9: (1,9) и (3,9).
        assert_eq!(draw_order(&units), vec![4, 2, 0, 3, 1]);
    }

    #[test]
    fn visible_units_filters_and_orders() {
        let viewport = test_viewport();

        // Видимы клетки 52..=59 x 53..=58.
        let units = [
            (55, 55), // виден
            (10, 10), // не виден
            (52, 57), // виден, ниже
            (56, 54), // виден, выше
            (52, 53), // виден (угол)
            (60, 55), // за правой границей
        ];

        let visible = viewport.visible_units(&units, 0);

        assert_eq!(visible, vec![4, 3, 0, 2]);
        // С запасом в клетку появляется юнит на x = 60 (тот же y, ниже по x).
        assert_eq!(viewport.visible_units(&units, 1), vec![4, 3, 0, 5, 2]);
    }

    #[test]
    fn tile_range_iterates_row_major() {
        let range = TileRange::new(2, 3, 3, 4).unwrap();
        let cells: Vec<(i16, i16)> = range.cells().collect();

        assert_eq!(cells, vec![(2, 3), (3, 3), (2, 4), (3, 4)]);
        assert_eq!(range.width(), 2);
        assert_eq!(range.height(), 2);
        assert!(TileRange::new(3, 3, 2, 4).is_none());
    }
}
