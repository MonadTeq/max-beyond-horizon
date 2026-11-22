//! Drawing of a map tile into RGBA: surface + resources.
//!
//! Full `.WRL` tiles are 64×64 pixels of palette indices per
//! tile-id (`max_assets::wrl`), but for the client a simplified scheme is enough
//! that already yields a readable map:
//!
//! * the cell base is the color of its surface (`LAND`/`WATER`/`COAST`/`AIR`);
//! * on top of the base is a checkerboard pattern (8×8 cells) so that neighboring cells
//!   differ without textures;
//! * a resource dot in the center of the cell — color by resource type
//!   (materials/fuel/gold), if the cell has a non-zero amount.
//!
//! `Surface` is a bit mask, so the priority is: `WATER` → `COAST` →
//! `LAND` → `AIR` → "unknown". In stock maps a cell has exactly one bit
//! (`pass_table` yields one of four values), so the priority matters only
//! for synthetic/mixed maps.
//!
//! Everything is pure functions over [`RgbaBuffer`] and [`Map`]: SDL is not required.

use max_assets::{ResourceMarkers, SimpleImage, WrlMap};
use max_core::enums::Surface;
use max_core::map::{Map, ResourceKind, ResourceMap};

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};
use crate::viewport::Viewport;

/// Map pixels in a cell (`GFX_MAP_TILE_SIZE`).
pub const TILE_PIXELS: i32 = 64;
/// How many pattern cells per axis (8×8 per cell).
pub const PATTERN_CELLS: i32 = 8;

/// Builds a [`Map`] from a parsed `.WRL`.
///
/// The surfaces are taken by the original's formula
/// `surface[i] = PassData[pass_table[tile_ids[i]]]` (`WrlMap::surface_map`,
/// §3.1 of `formats/wrl.md`); the resource map is empty — there are no resources in `.WRL`,
/// they are placed by match generation (`max_core::setup`).
///
/// The `.WRL` tile pixels themselves are not used here: the simplified renderer
/// draws the surface color and the resource dot (see the module documentation).
pub fn map_from_wrl(wrl: &WrlMap) -> Map {
    Map::from_parts(wrl.surface_map(), ResourceMap::new())
}

/// Colors and parameters of the simplified map rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileStyle {
    /// Land base.
    pub land: Rgba,
    /// Water base.
    pub water: Rgba,
    /// Coast base.
    pub coast: Rgba,
    /// "Air only" base (cliff/abyss).
    pub air: Rgba,
    /// Base of a cell without a surface (`Surface::NONE`).
    pub unknown: Rgba,
    /// Resource "materials" (`ResourceKind::Raw`).
    pub materials: Rgba,
    /// Resource "fuel".
    pub fuel: Rgba,
    /// Resource "gold".
    pub gold: Rgba,
    /// Radius of the resource dot in pixels at 1:1 scale.
    pub dot_radius: i32,
    /// Darkening level of the odd pattern cells (0..=7).
    pub pattern_level: u8,
}

impl Default for TileStyle {
    fn default() -> Self {
        Self {
            // Palette-independent colors: the map does not have to match the original
            // pixel for pixel, it is important to distinguish surfaces and resources.
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
    /// Base color for a surface mask.
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

    /// Color of the resource dot.
    pub fn resource_color(&self, kind: ResourceKind) -> Rgba {
        match kind {
            ResourceKind::Raw => self.materials,
            ResourceKind::Fuel => self.fuel,
            ResourceKind::Gold => self.gold,
        }
    }
}

/// Base color of a map cell (taking its surface into account).
pub fn tile_base_color(map: &Map, grid_x: i16, grid_y: i16, style: &TileStyle) -> Rgba {
    style.base_color(map.surface_at(grid_x, grid_y))
}

/// Color of a cell pixel inside its rectangle on screen.
///
/// `local_x`/`local_y` are the pixel coordinates inside the cell rectangle;
/// `cell` is the size of a pattern cell in pixels (scaled together with the tile).
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

/// Pattern cell size for a cell rectangle of the given size.
pub fn pattern_cell_size(tile_size: i32) -> i32 {
    (tile_size / PATTERN_CELLS).max(1)
}

/// Resource dot radius for a cell rectangle of the given size.
pub fn dot_radius_for(tile_size: i32, style: &TileStyle) -> i32 {
    if tile_size <= 0 {
        0
    } else {
        (style.dot_radius * tile_size / TILE_PIXELS).max(1)
    }
}

/// Draws one map cell into `rect` of the `dst` buffer.
///
/// Returns the number of changed pixels (0 — the cell is outside the map or the rectangle
/// is outside the buffer). The resource dot is drawn only for a non-zero amount.
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

/// Map cell into its own `size × size` buffer.
///
/// `None` if the cell is outside the map.
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

/// Draws all visible map cells into `dst` through the camera.
///
/// Returns the number of drawn cells. The buffer is not cleared: the background is set by
/// the caller (usually [`TileStyle::unknown`]).
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

/// Draws only the resource markers on top of already drawn tiles.
///
/// Needed when the surface is drawn with real pixels from `.WRL`
/// ([`crate::terrain::TerrainAtlas`]): the checkerboard placeholder [`TileStyle`] is then not
/// needed, but the resource dots must remain.
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

/// Draws resource markers from `MAX.RES` (`Survey_RenderMarker`).
///
/// Unlike [`render_resources_into`] (colored dots for the simplified
/// render without game data), this uses the real images
/// `RAWMSK*`/`FUELMK*`/`GOLDMK*` (at `zoom > 16`) or the small
/// `SMRWMK*`/`SMFLMK*`/`SMGDMK*` (at `zoom <= 16`). The coordinates are the center of the cell,
/// the hotspot of the image (`ulx/uly`) — like `WindowManager_DecodeSimpleImage`.
///
/// `palette` is the system palette of the window (`FRAMEPIC`): the markers are simple images and
/// are indexed precisely by it, not by the `.WRL` palette used to draw the tiles.
///
/// `team`/`max_survey` — the survey mask (`Survey_RenderMarkers`,
/// `survey.cpp:137-151`): `mask = GameManager_MaxSurvey ? 0xFFFF : 0` plus
/// the team's `hash_team_id`. The icon is drawn only where
/// `ResourceManager_CargoMap & mask`, that is, **surveyed** by this team
/// (or the `MAXSURVEY` cheat is enabled). In addition, C++ requires
/// `Access_GetModifiedSurfaceType(i, j) != SURFACE_TYPE_AIR`.
///
/// Returns `(pixels drawn, cells with an icon)`.
#[allow(clippy::too_many_arguments)]
pub fn render_resource_icons_into(
    dst: &mut RgbaBuffer,
    map: &Map,
    viewport: &Viewport,
    markers: &ResourceMarkers,
    palette: &crate::terrain::PaletteRgba,
    team: u8,
    max_survey: bool,
) -> (u32, u32) {
    let Some(range) = viewport.visible_tiles() else {
        return (0, 0);
    };
    let zoom = viewport.zoom() as i32;
    let mut painted = 0;
    let mut cells = 0;

    for (grid_x, grid_y) in range.cells() {
        if grid_x < 0 || grid_y < 0 || grid_x >= map.size.width || grid_y >= map.size.height {
            continue;
        }

        if map.surface_at(grid_x, grid_y) == Surface::AIR {
            // `Access_GetModifiedSurfaceType(i, j) != SURFACE_TYPE_AIR`.
            continue;
        }

        let entry = map.resource_at(grid_x, grid_y);
        let Some(kind) = entry.resource_type().filter(|_| entry.amount() > 0) else {
            continue;
        };

        // `CargoMap & mask`: team bit (or 0xFFFF with the MAXSURVEY cheat).
        if !max_survey && !entry.is_visible_to(team) {
            continue;
        }

        let rect = viewport.tile_rect(grid_x, grid_y);
        if rect.width <= 0 || rect.height <= 0 {
            continue;
        }

        let centre = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);

        // C++: `Gfx_ZoomLevel > 16` — large markers, otherwise small ones.
        let image = if zoom > 16 {
            markers.big(kind, entry.amount())
        } else {
            markers.small(kind, entry.amount())
        };

        if let Some(image) = image {
            // Scale of the original: large marker — `zoom/64`, small — `zoom/16`.
            let (num, den) = if zoom > 16 { (zoom, 64) } else { (zoom, 16) };
            let drawn = blit_marker(dst, image, palette, centre, num, den);

            if drawn > 0 {
                painted += drawn;
                cells += 1;
            }
        } else if let Some(icon) = markers.icon(kind) {
            // Fallback: the `I_*` interface icon at the center of the cell (not
            // used in the original, but better than a colored dot).
            let hotspot = (i32::from(icon.width) / 2, i32::from(icon.height) / 2);
            let drawn = blit_marker_at(dst, icon, palette, centre, hotspot, 1, 1);

            if drawn > 0 {
                painted += drawn;
                cells += 1;
            }
        }
    }

    (painted, cells)
}

/// Blit of a simple image with the hotspot at the center and scale `num/den`.
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

/// Blit of a simple image with the hotspot from the resource header
/// (`WindowManager_DecodeSimpleImage`: `top_left = centre - ulx/uly`),
/// scaled by `num/den`.
///
/// Used by both resource markers and build markers `BLDMRK1..5`
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

/// Blit of a simple image: `top_left = centre - hotspot`, sizes `w*num/den`.
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

/// `(value * num / den)` rounded to the nearest.
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

    /// A map filled entirely with `surface`.
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

        // Priority for a mixed mask: WATER is more important than COAST, COAST than LAND.
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
        // Pattern cell is 8 pixels: (0,0) and (8,0) have different parity.
        assert_eq!(buffer.get(0, 0), Some(land));
        assert_eq!(buffer.get(7, 0), Some(land));
        assert_eq!(buffer.get(8, 0), Some(land.darken(style.pattern_level)));
        assert_eq!(buffer.get(16, 0), Some(land));
        assert_eq!(buffer.get(8, 8), Some(land));
        // (63, 63) — cell (7, 7): even, so the base without darkening.
        assert_eq!(buffer.get(63, 63), Some(land));
        // (0, 8) — cell (0, 1): odd, darkened.
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
            // `HASH_TEAM_RED = 0x2000` (bit 13) — surveyed by the reds.
            ResourceMapEntry::new(4, Some(ResourceKind::Gold), 0b1000),
        );
        let map = Map::from_parts(uniform_map(Surface::LAND).surface.clone(), resources);
        let mut palette = [[0u8, 0, 0, 255]; 256];

        for (index, entry) in palette.iter_mut().enumerate() {
            *entry = [index as u8, 255 - index as u8, 128, 255];
        }

        let viewport = Viewport::new(448, 448);
        let mut buffer = RgbaBuffer::new(448, 448);
        let (painted, cells) =
            render_resource_icons_into(&mut buffer, &map, &viewport, &markers, &palette, 0, false);

        assert!(painted > 0, "маркер золота должен нарисоваться");
        assert_eq!(cells, 1, "одна разведанная клетка — одна иконка");
        assert_eq!(
            render_resource_icons_into(&mut buffer, &map, &viewport, &markers, &palette, 1, false),
            (0, 0),
            "клетка не разведана другой командой"
        );
        assert_eq!(
            render_resource_icons_into(&mut buffer, &map, &viewport, &markers, &palette, 1, true).1,
            1,
            "чит MAXSURVEY (max_survey) показывает маркеры всех клеток"
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

        // Center of the cell is gold, the corner is land (the checkerboard does not matter).
        assert_eq!(buffer.get(32, 32), Some(style.gold));
        assert_eq!(buffer.get(0, 0), Some(style.land));

        // A zero resource amount draws no dot.
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
        // Cell (52,53) starts at (0,0) of the screen.
        assert_eq!(buffer.get(0, 0), Some(style.water));
        assert_eq!(
            buffer.get(63, 0),
            Some(style.water.darken(style.pattern_level))
        );
        // The bottom-right corner of the screen is also covered by the map.
        assert_ne!(buffer.get(511, 383), Some(style.unknown));
    }
}
