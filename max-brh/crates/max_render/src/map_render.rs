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

/// Draws the survey overlay from `MAX.RES` (`Survey_RenderMarkers` +
/// `Survey_RenderMarker`, `survey.cpp:32-155`).
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
/// the team's `hash_team_id`. In addition, C++ requires
/// `Access_GetModifiedSurfaceType(i, j) != SURFACE_TYPE_AIR`.
///
/// Every cell the team has **surveyed** gets its marker: `Survey_RenderMarkers`
/// calls `Survey_RenderMarker` for each surveyed cell, and the marker index is
/// `min(CargoMap & CARGO_MASK, 16)` — `*MK0` for a cell **without** a deposit
/// (the small circle that shows the scout has already explored the cell), `*MK1..16`
/// for a known deposit. At `zoom <= 16` C++ uses the small `SM*MK` set and returns
/// early for `resource_value == 0` (`survey.cpp:62-64`), so empty surveyed cells get
/// no marker there.
///
/// Returns `(pixels drawn, cells with a marker)`.
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

        // `CargoMap & mask`: team bit (or 0xFFFF with the MAXSURVEY cheat).
        if !max_survey && !entry.is_visible_to(team) {
            continue;
        }

        let rect = viewport.tile_rect(grid_x, grid_y);
        if rect.width <= 0 || rect.height <= 0 {
            continue;
        }

        // `resource_value = min(material_type & CARGO_MASK, 16)` (`survey.cpp:39`).
        let amount = entry.amount().min(16);
        // Marker family by the cargo bits: gold → `GOLDMK*`, fuel → `FUELMK*`,
        // otherwise `RAWMSK*` (`survey.cpp:41-50`). A cell without a deposit keeps
        // the `RAWMSK0` circle.
        let kind = entry.resource_type().unwrap_or(ResourceKind::Raw);

        // C++: `Gfx_ZoomLevel > 16` — large markers, otherwise small ones. At the small
        // zoom an empty surveyed cell has no marker at all (`survey.cpp:62-64`).
        if zoom <= 16 && amount == 0 {
            continue;
        }

        let centre = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);

        let (image, num, den) = if zoom > 16 {
            // Large marker — `zoom/64` (`WindowManager_RescaleSimpleImage`,
            // `Gfx_MapScalingFactor`).
            (markers.big(kind, amount), zoom, 64)
        } else {
            // Small marker — `zoom/16`.
            (markers.small(kind, amount), zoom, 16)
        };

        let drawn = if let Some(image) = image {
            blit_marker(dst, image, palette, centre, num, den)
        } else if let Some(icon) = markers.icon(kind) {
            // Fallback: the `I_*` interface icon at the center of the cell (not
            // used in the original, but better than nothing). Only for a known
            // deposit — an empty cell has no interface icon.
            if amount == 0 {
                0
            } else {
                let hotspot = (i32::from(icon.width) / 2, i32::from(icon.height) / 2);
                blit_marker_at(dst, icon, palette, centre, hotspot, 1, 1)
            }
        } else {
            0
        };

        if drawn > 0 {
            painted += drawn;
            cells += 1;
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
#[path = "map_render_tests.rs"]
mod tests;
