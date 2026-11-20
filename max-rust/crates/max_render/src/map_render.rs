//! Отрисовка тайла карты в RGBA: поверхность + ресурсы.
//!
//! Полноценные тайлы `.WRL` — это 64×64 пикселя индексов палитры на каждый
//! tile-id (`max_assets::wrl`), но для клиента достаточно упрощённой схемы,
//! которая уже даёт читаемую карту:
//!
//! * база клетки — цвет её поверхности (`LAND`/`WATER`/`COAST`/`AIR`);
//! * поверх базы — шахматный паттерн (8×8 ячеек), чтобы соседние клетки
//!   различались без текстур;
//! * ресурсная точка в центре клетки — цвет по типу ресурса
//!   (материалы/топливо/золото), если в клетке есть ненулевое количество.
//!
//! `Surface` — битовая маска, поэтому приоритет такой: `WATER` → `COAST` →
//! `LAND` → `AIR` → «неизвестно». В штатных картах в клетке ровно один бит
//! (`pass_table` даёт одно из четырёх значений), так что приоритет важен только
//! для синтетических/смешанных карт.
//!
//! Всё — чистые функции над [`RgbaBuffer`] и [`Map`]: SDL не требуется.

use max_assets::{ResourceMarkers, SimpleImage, WrlMap};
use max_core::enums::Surface;
use max_core::map::{Map, ResourceKind, ResourceMap};

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};
use crate::viewport::Viewport;

/// Пикселей карты в клетке (`GFX_MAP_TILE_SIZE`).
pub const TILE_PIXELS: i32 = 64;
/// Сколько ячеек паттерна по каждой оси (8×8 на клетку).
pub const PATTERN_CELLS: i32 = 8;

/// Строит [`Map`] из разобранного `.WRL`.
///
/// Поверхности берутся по формуле оригинала
/// `surface[i] = PassData[pass_table[tile_ids[i]]]` (`WrlMap::surface_map`,
/// §3.1 `formats/wrl.md`); карта ресурсов пустая — в `.WRL` ресурсов нет,
/// их расставляет генерация партии (`max_core::setup`).
///
/// Сами пиксели тайлов `.WRL` здесь не используются: упрощённый рендер
/// рисует цвет поверхности и ресурсную точку (см. модульную документацию).
pub fn map_from_wrl(wrl: &WrlMap) -> Map {
    Map::from_parts(wrl.surface_map(), ResourceMap::new())
}

/// Цвета и параметры упрощённой отрисовки карты.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileStyle {
    /// База суши.
    pub land: Rgba,
    /// База воды.
    pub water: Rgba,
    /// База побережья.
    pub coast: Rgba,
    /// База «только воздух» (обрыв/пропасть).
    pub air: Rgba,
    /// База клетки без поверхности (`Surface::NONE`).
    pub unknown: Rgba,
    /// Ресурс «материалы» (`ResourceKind::Raw`).
    pub materials: Rgba,
    /// Ресурс «топливо».
    pub fuel: Rgba,
    /// Ресурс «золото».
    pub gold: Rgba,
    /// Радиус ресурсной точки в пикселях при масштабе 1:1.
    pub dot_radius: i32,
    /// Уровень затемнения нечётных ячеек паттерна (0..=7).
    pub pattern_level: u8,
}

impl Default for TileStyle {
    fn default() -> Self {
        Self {
            // Палитра-независимые цвета: карта не обязана совпадать с оригиналом
            // пиксель-в-пиксель, важно различать поверхности и ресурсы.
            land: Rgba::rgb(86, 124, 58),
            water: Rgba::rgb(38, 74, 148),
            coast: Rgba::rgb(198, 186, 122),
            air: Rgba::rgb(92, 84, 104),
            unknown: Rgba::rgb(24, 24, 28),
            materials: Rgba::rgb(196, 196, 204),
            fuel: Rgba::rgb(226, 96, 42),
            gold: Rgba::rgb(240, 202, 62),
            dot_radius: 6,
            pattern_level: 6,
        }
    }
}

impl TileStyle {
    /// Цвет базы для поверхности-маски.
    pub fn base_color(&self, surface: Surface) -> Rgba {
        if surface.intersects(Surface::WATER) {
            self.water
        } else if surface.intersects(Surface::COAST) {
            self.coast
        } else if surface.intersects(Surface::LAND) {
            self.land
        } else if surface.intersects(Surface::AIR) {
            self.air
        } else {
            self.unknown
        }
    }

    /// Цвет ресурсной точки.
    pub fn resource_color(&self, kind: ResourceKind) -> Rgba {
        match kind {
            ResourceKind::Raw => self.materials,
            ResourceKind::Fuel => self.fuel,
            ResourceKind::Gold => self.gold,
        }
    }
}

/// Цвет базы клетки карты (с учётом её поверхности).
pub fn tile_base_color(map: &Map, grid_x: i16, grid_y: i16, style: &TileStyle) -> Rgba {
    style.base_color(map.surface_at(grid_x, grid_y))
}

/// Цвет пикселя клетки внутри её прямоугольника на экране.
///
/// `local_x`/`local_y` — координаты пикселя внутри прямоугольника клетки;
/// `cell` — размер ячейки паттерна в пикселях (масштабируется вместе с тайлом).
pub fn tile_pixel_color(
    base: Rgba,
    local_x: i32,
    local_y: i32,
    cell: i32,
    style: &TileStyle,
) -> Rgba {
    if cell <= 0 {
        return base;
    }

    let parity = ((local_x / cell) + (local_y / cell)) & 1;

    if parity == 0 {
        base
    } else {
        base.darken(style.pattern_level)
    }
}

/// Размер ячейки паттерна для прямоугольника клетки заданного размера.
pub fn pattern_cell_size(tile_size: i32) -> i32 {
    (tile_size / PATTERN_CELLS).max(1)
}

/// Радиус ресурсной точки для прямоугольника клетки заданного размера.
pub fn dot_radius_for(tile_size: i32, style: &TileStyle) -> i32 {
    if tile_size <= 0 {
        0
    } else {
        (style.dot_radius * tile_size / TILE_PIXELS).max(1)
    }
}

/// Рисует одну клетку карты в `rect` буфера `dst`.
///
/// Возвращает число изменённых пикселей (0 — клетка вне карты или прямоугольник
/// вне буфера). Ресурсная точка рисуется только при ненулевом количестве.
pub fn render_tile_into(
    dst: &mut RgbaBuffer,
    map: &Map,
    grid: (i16, i16),
    rect: PixelRect,
    style: &TileStyle,
) -> u32 {
    if grid.0 < 0
        || grid.1 < 0
        || grid.0 >= map.size.width
        || grid.1 >= map.size.height
        || rect.is_empty()
    {
        return 0;
    }

    let base = tile_base_color(map, grid.0, grid.1, style);
    let cell = pattern_cell_size(rect.width);
    let entry = map.resource_at(grid.0, grid.1);
    let dot = entry
        .resource_type()
        .filter(|_| entry.amount() > 0)
        .map(|kind| style.resource_color(kind));
    let radius = dot_radius_for(rect.width, style);
    let centre = (rect.width / 2, rect.height / 2);
    let mut painted = 0;

    for local_y in 0..rect.height {
        for local_x in 0..rect.width {
            let mut color = tile_pixel_color(base, local_x, local_y, cell, style);

            if let Some(dot_color) = dot {
                let dx = local_x - centre.0;
                let dy = local_y - centre.1;

                if dx * dx + dy * dy <= radius * radius {
                    color = dot_color;
                }
            }

            if dst.set(rect.ulx + local_x, rect.uly + local_y, color) {
                painted += 1;
            }
        }
    }

    painted
}

/// Клетка карты в собственный буфер `size × size`.
///
/// `None`, если клетка вне карты.
pub fn render_tile(
    map: &Map,
    grid: (i16, i16),
    size: i32,
    style: &TileStyle,
) -> Option<RgbaBuffer> {
    if size <= 0
        || grid.0 < 0
        || grid.1 < 0
        || grid.0 >= map.size.width
        || grid.1 >= map.size.height
    {
        return None;
    }

    let mut buffer = RgbaBuffer::new(size as u32, size as u32);

    render_tile_into(
        &mut buffer,
        map,
        grid,
        PixelRect::new(0, 0, size, size),
        style,
    );

    Some(buffer)
}

/// Рисует все видимые клетки карты в `dst` через камеру.
///
/// Возвращает число нарисованных клеток. Буфер не очищается: фон задаёт
/// вызывающий (обычно — [`TileStyle::unknown`]).
pub fn render_map_into(
    dst: &mut RgbaBuffer,
    map: &Map,
    viewport: &Viewport,
    style: &TileStyle,
) -> u32 {
    let Some(range) = viewport.visible_tiles() else {
        return 0;
    };
    let mut painted = 0;

    for (grid_x, grid_y) in range.cells() {
        let rect = viewport.tile_rect(grid_x, grid_y);

        if render_tile_into(dst, map, (grid_x, grid_y), rect, style) > 0 {
            painted += 1;
        }
    }

    painted
}

/// Рисует только маркеры ресурсов поверх уже нарисованных тайлов.
///
/// Нужен, когда поверхность рисуется настоящими пикселями из `.WRL`
/// ([`crate::terrain::TerrainAtlas`]): шахматная заглушка [`TileStyle`] тогда не
/// нужна, но точки ресурсов должны остаться.
pub fn render_resources_into(
    dst: &mut RgbaBuffer,
    map: &Map,
    viewport: &Viewport,
    style: &TileStyle,
) -> u32 {
    let Some(range) = viewport.visible_tiles() else {
        return 0;
    };
    let mut painted = 0;

    for (grid_x, grid_y) in range.cells() {
        if grid_x < 0 || grid_y < 0 || grid_x >= map.size.width || grid_y >= map.size.height {
            continue;
        }

        let entry = map.resource_at(grid_x, grid_y);
        let Some(kind) = entry.resource_type().filter(|_| entry.amount() > 0) else {
            continue;
        };

        let rect = viewport.tile_rect(grid_x, grid_y);
        if rect.width <= 0 || rect.height <= 0 {
            continue;
        }

        let color = style.resource_color(kind);
        let radius = dot_radius_for(rect.width, style);
        let centre = (rect.width / 2, rect.height / 2);

        for local_y in 0..rect.height {
            for local_x in 0..rect.width {
                let dx = local_x - centre.0;
                let dy = local_y - centre.1;

                if dx * dx + dy * dy <= radius * radius
                    && dst.set(rect.ulx + local_x, rect.uly + local_y, color)
                {
                    painted += 1;
                }
            }
        }
    }

    painted
}

/// Рисует маркеры ресурсов из `MAX.RES` (`Survey_RenderMarker`).
///
/// В отличие от [`render_resources_into`] (цветные точки для упрощённого
/// рендера без данных игры), здесь используются настоящие картинки
/// `RAWMSK*`/`FUELMK*`/`GOLDMK*` (при `zoom > 16`) или малые
/// `SMRWMK*`/`SMFLMK*`/`SMGDMK*` (при `zoom <= 16`). Координаты — центр клетки,
/// hotspot картинки (`ulx/uly`) — как `WindowManager_DecodeSimpleImage`.
///
/// `palette` — системная палитра окна (`FRAMEPIC`): маркеры — simple image и
/// индексируются именно ею, а не палитрой `.WRL`, которой рисуются тайлы.
/// `team`/`show_all` — маска исследованности (`ResourceManager_CargoMap &
/// hash_team_id` и `GameManager_MaxSurvey` в C++).
#[allow(clippy::too_many_arguments)]
pub fn render_resource_icons_into(
    dst: &mut RgbaBuffer,
    map: &Map,
    viewport: &Viewport,
    markers: &ResourceMarkers,
    palette: &crate::terrain::PaletteRgba,
    team: u8,
    show_all: bool,
) -> u32 {
    let Some(range) = viewport.visible_tiles() else {
        return 0;
    };
    let zoom = viewport.zoom() as i32;
    let mut painted = 0;

    for (grid_x, grid_y) in range.cells() {
        if grid_x < 0 || grid_y < 0 || grid_x >= map.size.width || grid_y >= map.size.height {
            continue;
        }

        let entry = map.resource_at(grid_x, grid_y);
        let Some(kind) = entry.resource_type().filter(|_| entry.amount() > 0) else {
            continue;
        };

        if !show_all && !entry.is_visible_to(team) {
            continue;
        }

        let rect = viewport.tile_rect(grid_x, grid_y);
        if rect.width <= 0 || rect.height <= 0 {
            continue;
        }

        let centre = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);

        // C++: `Gfx_ZoomLevel > 16` — большие маркеры, иначе малые.
        let image = if zoom > 16 {
            markers.big(kind, entry.amount())
        } else {
            markers.small(kind, entry.amount())
        };

        if let Some(image) = image {
            // Масштаб оригинала: большой маркер — `zoom/64`, малый — `zoom/16`.
            let (num, den) = if zoom > 16 { (zoom, 64) } else { (zoom, 16) };

            painted += blit_marker(dst, image, palette, centre, num, den);
        } else if let Some(icon) = markers.icon(kind) {
            // Fallback: иконка интерфейса `I_*` по центру клетки (в оригинале не
            // используется, но лучше цветной точки).
            let hotspot = (i32::from(icon.width) / 2, i32::from(icon.height) / 2);

            painted += blit_marker_at(dst, icon, palette, centre, hotspot, 1, 1);
        }
    }

    painted
}

/// Блит simple image с hotspot по центру и масштабом `num/den`.
fn blit_marker(
    dst: &mut RgbaBuffer,
    image: &SimpleImage,
    palette: &crate::terrain::PaletteRgba,
    centre: (i32, i32),
    num: i32,
    den: i32,
) -> u32 {
    let hotspot = (
        scale_round(i32::from(image.ulx), num, den),
        scale_round(i32::from(image.uly), num, den),
    );

    blit_marker_at(dst, image, palette, centre, hotspot, num, den)
}

/// Блит simple image с hotspot из заголовка ресурса
/// (`WindowManager_DecodeSimpleImage`: `top_left = centre - ulx/uly`),
/// масштабированный `num/den`.
///
/// Используется и маркерами ресурсов, и маркерами стройки `BLDMRK1..5`
/// (`DrawMap_Callback2`, `drawmap.cpp:339`).
pub fn blit_simple_image(
    dst: &mut RgbaBuffer,
    image: &SimpleImage,
    palette: &crate::terrain::PaletteRgba,
    centre: (i32, i32),
    num: i32,
    den: i32,
) -> u32 {
    blit_marker(dst, image, palette, centre, num, den)
}

/// Блит simple image: `top_left = centre - hotspot`, размеры `w*num/den`.
fn blit_marker_at(
    dst: &mut RgbaBuffer,
    image: &SimpleImage,
    palette: &crate::terrain::PaletteRgba,
    centre: (i32, i32),
    hotspot: (i32, i32),
    num: i32,
    den: i32,
) -> u32 {
    let num = num.max(1);
    let den = den.max(1);
    let width = scale_round(i32::from(image.width), num, den).max(1);
    let height = scale_round(i32::from(image.height), num, den).max(1);
    let ulx = centre.0 - hotspot.0;
    let uly = centre.1 - hotspot.1;
    let mut painted = 0;

    for dy in 0..height {
        let sy = (dy * den / num) as u32;

        for dx in 0..width {
            let sx = (dx * den / num) as u32;
            let Some(index) = image.pixel(sx, sy) else {
                continue;
            };

            if index == image.transparent {
                continue;
            }

            let [r, g, b, a] = palette[usize::from(index)];

            if dst.set(ulx + dx, uly + dy, Rgba::from_rgba([r, g, b, a])) {
                painted += 1;
            }
        }
    }

    painted
}

/// `(value * num / den)` с округлением к ближайшему.
fn scale_round(value: i32, num: i32, den: i32) -> i32 {
    if den == 0 {
        return value;
    }

    (value * num + den / 2) / den
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::map::{ResourceMap, ResourceMapEntry, SurfaceMap};

    /// Карта, целиком заполненная `surface`.
    fn uniform_map(surface: Surface) -> Map {
        let mut surface_map = SurfaceMap::new();

        for y in 0..max_core::geo::MAP_HEIGHT {
            for x in 0..max_core::geo::MAP_WIDTH {
                surface_map.set(x, y, surface);
            }
        }

        Map::from_parts(surface_map, ResourceMap::new())
    }

    fn style() -> TileStyle {
        TileStyle::default()
    }

    #[test]
    fn surface_colors_are_distinct() {
        let style = style();
        let colors = [
            style.base_color(Surface::LAND),
            style.base_color(Surface::WATER),
            style.base_color(Surface::COAST),
            style.base_color(Surface::AIR),
            style.base_color(Surface::NONE),
        ];

        for (index, color) in colors.iter().enumerate() {
            for other in colors.iter().skip(index + 1) {
                assert_ne!(color, other, "цвета поверхностей должны различаться");
            }
        }

        // Приоритет для смешанной маски: WATER важнее COAST, COAST важнее LAND.
        assert_eq!(
            style.base_color(Surface::LAND | Surface::COAST),
            style.coast
        );
        assert_eq!(
            style.base_color(Surface::LAND | Surface::WATER),
            style.water
        );
        assert_eq!(style.base_color(Surface::AIR | Surface::LAND), style.land);
    }

    #[test]
    fn resource_colors_are_distinct() {
        let style = style();

        assert_ne!(
            style.resource_color(ResourceKind::Raw),
            style.resource_color(ResourceKind::Fuel)
        );
        assert_ne!(
            style.resource_color(ResourceKind::Raw),
            style.resource_color(ResourceKind::Gold)
        );
        assert_ne!(
            style.resource_color(ResourceKind::Fuel),
            style.resource_color(ResourceKind::Gold)
        );
    }

    #[test]
    fn tile_base_and_pattern_alternate_cells() {
        let style = style();
        let map = uniform_map(Surface::LAND);
        let buffer = render_tile(&map, (10, 10), TILE_PIXELS, &style).unwrap();
        let land = style.land;

        assert_eq!(buffer.width(), 64);
        assert_eq!(buffer.height(), 64);
        // Ячейка паттерна — 8 пикселей: (0,0) и (8,0) разной чётности.
        assert_eq!(buffer.get(0, 0), Some(land));
        assert_eq!(buffer.get(7, 0), Some(land));
        assert_eq!(buffer.get(8, 0), Some(land.darken(style.pattern_level)));
        assert_eq!(buffer.get(16, 0), Some(land));
        assert_eq!(buffer.get(8, 8), Some(land));
        // (63, 63) — ячейка (7, 7): чётная, значит база без затемнения.
        assert_eq!(buffer.get(63, 63), Some(land));
        // (0, 8) — ячейка (0, 1): нечётная, затемнена.
        assert_eq!(buffer.get(0, 8), Some(land.darken(style.pattern_level)));
    }

    #[test]
    fn different_surfaces_give_different_tiles() {
        let style = style();
        let land = render_tile(&uniform_map(Surface::LAND), (1, 1), 16, &style).unwrap();
        let water = render_tile(&uniform_map(Surface::WATER), (1, 1), 16, &style).unwrap();
        let coast = render_tile(&uniform_map(Surface::COAST), (1, 1), 16, &style).unwrap();
        let air = render_tile(&uniform_map(Surface::AIR), (1, 1), 16, &style).unwrap();

        assert_eq!(land.get(0, 0), Some(style.land));
        assert_eq!(water.get(0, 0), Some(style.water));
        assert_eq!(coast.get(0, 0), Some(style.coast));
        assert_eq!(air.get(0, 0), Some(style.air));
    }

    #[test]
    fn resource_markers_are_drawn_from_max_res() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAX.RES");

        if !path.is_file() {
            return;
        }

        let archive = max_assets::ResArchive::open(&path).expect("MAX.RES");
        let markers = max_assets::ResourceMarkers::load(&archive);
        assert!(markers.is_ready());

        let mut resources = ResourceMap::new();
        resources.set(
            52,
            53,
            ResourceMapEntry::new(4, Some(ResourceKind::Gold), 0b1),
        );
        let map = Map::from_parts(uniform_map(Surface::LAND).surface.clone(), resources);
        let mut palette = [[0u8, 0, 0, 255]; 256];

        for (index, entry) in palette.iter_mut().enumerate() {
            *entry = [index as u8, 255 - index as u8, 128, 255];
        }

        let viewport = Viewport::new(448, 448);
        let mut buffer = RgbaBuffer::new(448, 448);
        let painted =
            render_resource_icons_into(&mut buffer, &map, &viewport, &markers, &palette, 0, false);

        assert!(painted > 0, "маркер золота должен нарисоваться");
        assert_eq!(
            render_resource_icons_into(
                &mut buffer,
                &map,
                &viewport,
                &markers,
                &palette,
                0b10,
                false
            ),
            0,
            "клетка не разведана другой командой"
        );
        assert!(
            render_resource_icons_into(
                &mut buffer,
                &map,
                &viewport,
                &markers,
                &palette,
                0b10,
                true
            ) > 0,
            "SURVEY (show_all) показывает маркеры всех клеток"
        );
    }

    #[test]
    fn resource_dot_is_painted_at_the_centre() {
        let style = style();
        let map = uniform_map(Surface::LAND);
        let mut resources = ResourceMap::new();

        resources.set(
            10,
            10,
            ResourceMapEntry::new(5, Some(ResourceKind::Gold), 0),
        );

        let map = Map::from_parts(map.surface.clone(), resources);
        let buffer = render_tile(&map, (10, 10), TILE_PIXELS, &style).unwrap();

        // Центр клетки — золото, угол — суша (шахматка не важна).
        assert_eq!(buffer.get(32, 32), Some(style.gold));
        assert_eq!(buffer.get(0, 0), Some(style.land));

        // Нулевое количество ресурса точки не рисует.
        let mut empty = ResourceMap::new();
        empty.set(
            10,
            10,
            ResourceMapEntry::new(0, Some(ResourceKind::Gold), 0),
        );

        let map = Map::from_parts(uniform_map(Surface::LAND).surface, empty);
        let buffer = render_tile(&map, (10, 10), TILE_PIXELS, &style).unwrap();

        assert_eq!(buffer.get(32, 32), Some(style.land));
    }

    #[test]
    fn resource_kinds_paint_different_dots() {
        let style = style();
        let mut surface_map = SurfaceMap::new();

        surface_map.set(1, 1, Surface::LAND);
        surface_map.set(2, 1, Surface::LAND);
        surface_map.set(3, 1, Surface::LAND);

        let mut resources = ResourceMap::new();

        resources.set(1, 1, ResourceMapEntry::new(4, Some(ResourceKind::Raw), 0));
        resources.set(2, 1, ResourceMapEntry::new(4, Some(ResourceKind::Fuel), 0));
        resources.set(3, 1, ResourceMapEntry::new(4, Some(ResourceKind::Gold), 0));

        let map = Map::from_parts(surface_map, resources);
        let raw = render_tile(&map, (1, 1), TILE_PIXELS, &style).unwrap();
        let fuel = render_tile(&map, (2, 1), TILE_PIXELS, &style).unwrap();
        let gold = render_tile(&map, (3, 1), TILE_PIXELS, &style).unwrap();

        assert_eq!(raw.get(32, 32), Some(style.materials));
        assert_eq!(fuel.get(32, 32), Some(style.fuel));
        assert_eq!(gold.get(32, 32), Some(style.gold));
    }

    #[test]
    fn out_of_map_tiles_are_not_drawn() {
        let style = style();
        let map = uniform_map(Surface::LAND);
        let mut buffer = RgbaBuffer::new(32, 32);

        assert_eq!(render_tile(&map, (-1, 0), 32, &style), None);
        assert_eq!(
            render_tile_into(
                &mut buffer,
                &map,
                (-1, 0),
                PixelRect::new(0, 0, 32, 32),
                &style
            ),
            0
        );
        assert_eq!(
            render_tile_into(
                &mut buffer,
                &map,
                (max_core::geo::MAP_WIDTH, 0),
                PixelRect::new(0, 0, 32, 32),
                &style
            ),
            0
        );
    }

    #[test]
    fn render_map_into_paints_exactly_the_visible_tiles() {
        let style = style();
        let map = uniform_map(Surface::WATER);
        let viewport = Viewport::new(512, 384);
        let mut buffer = RgbaBuffer::filled(512, 384, style.unknown);
        let range = viewport.visible_tiles().unwrap();

        let painted = render_map_into(&mut buffer, &map, &viewport, &style);

        assert_eq!(painted, range.count() as u32);
        // Клетка (52,53) начинается в (0,0) экрана.
        assert_eq!(buffer.get(0, 0), Some(style.water));
        assert_eq!(
            buffer.get(63, 0),
            Some(style.water.darken(style.pattern_level))
        );
        // Правый нижний угол экрана тоже покрыт картой.
        assert_ne!(buffer.get(511, 383), Some(style.unknown));
    }
}
