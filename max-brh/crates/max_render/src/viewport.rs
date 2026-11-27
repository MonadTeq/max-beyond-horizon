//! Camera and visible selection: map -> screen coordinate transform.
//!
//! The M.A.X. map is a grid of [`MAP_WIDTH`]×[`MAP_HEIGHT`] cells of 64 pixels
//! (`GFX_MAP_TILE_SIZE`). The camera stores the center in "world" map pixels
//! (`0..MAP_WIDTH*64`) and the scale — how many **screen** pixels fall on
//! one cell (`zoom`); in the original this is `Gfx_ZoomLevel`, where 64 is the 1:1 scale.
//!
//! All arithmetic is integer (as in the C++ port: 16.16 fixed point), so the
//! transforms are deterministic and tested without a window. The screen and map corners
//! are clipped: the camera does not move beyond the map.
//!
//! The drawing order is by the unit's "ground": first the upper ones (smaller `y`), then
//! the lower ones, so that the lower unit overlaps the upper one (§5.4 spw.md).

use max_core::geo::{MAP_HEIGHT, MAP_WIDTH};

use crate::buffer::PixelRect;

/// Map pixels in one cell (`GFX_MAP_TILE_SIZE`).
pub const MAP_TILE_PIXELS: i32 = 64;
/// Minimum scale: 8 screen pixels per cell.
pub const MIN_ZOOM: u32 = 8;
/// Maximum scale: 128 screen pixels per cell.
pub const MAX_ZOOM: u32 = 128;
/// Default scale — 1:1 (like `Gfx_ZoomLevel` in the original).
pub const DEFAULT_ZOOM: u32 = 64;
/// Zoom change step (zoom-in/zoom-out buttons).
pub const ZOOM_STEP: u32 = 2;

/// Range of visible cells (inclusive at both corners).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRange {
    /// Minimum x.
    pub x0: i16,
    /// Minimum y.
    pub y0: i16,
    /// Maximum x (inclusive).
    pub x1: i16,
    /// Maximum y (inclusive).
    pub y1: i16,
}

impl TileRange {
    /// Range from two corners in any order; an empty range yields `None`.
    pub fn new(x0: i16, y0: i16, x1: i16, y1: i16) -> Option<Self> {
        if x0 > x1 || y0 > y1 {
            return None;
        }

        Some(Self { x0, y0, x1, y1 })
    }

    /// Width in cells.
    #[inline]
    pub fn width(&self) -> i16 {
        self.x1 - self.x0 + 1
    }

    /// Height in cells.
    #[inline]
    pub fn height(&self) -> i16 {
        self.y1 - self.y0 + 1
    }

    /// Number of cells.
    #[inline]
    pub fn count(&self) -> usize {
        self.width() as usize * self.height() as usize
    }

    /// The cell is within the range.
    #[inline]
    pub fn contains(&self, x: i16, y: i16) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    /// Cells of the range in drawing order (rows top to bottom, inside a
    /// row — left to right).
    pub fn cells(&self) -> impl Iterator<Item = (i16, i16)> + '_ {
        (self.y0..=self.y1).flat_map(move |y| (self.x0..=self.x1).map(move |x| (x, y)))
    }
}

/// Camera: screen size, center in world pixels and scale.
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

/// Screen-space positions of the cell-grid lines: [`Viewport::grid_lines`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GridLines {
    /// Vertical line positions (buffer columns), ascending.
    pub xs: Vec<i32>,
    /// Horizontal line positions (buffer rows), ascending.
    pub ys: Vec<i32>,
}

impl GridLines {
    /// No lines (empty viewport).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.xs.is_empty() && self.ys.is_empty()
    }
}

impl Viewport {
    /// Screen `width × height`, center — the center of the map, scale [`DEFAULT_ZOOM`].
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

    /// Screen with an explicit center (in world pixels).
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

    /// Screen size `(width, height)`.
    #[inline]
    pub fn screen(&self) -> (u32, u32) {
        (self.screen_width, self.screen_height)
    }

    /// The same camera on a `×factor` render frame (`remaster-hd.md` §R7).
    ///
    /// Both the screen size and `zoom` are multiplied, so the world origin and
    /// every cell boundary land on exactly `×factor` the logical coordinate: the
    /// HD frame is the same view at a higher resolution, and the mouse transform
    /// keeps working in 640×480 (`ScreenGeometry::window_to_frame` divides by
    /// `S`).
    ///
    /// `zoom` is **not** clamped to [`MAX_ZOOM`]: the multiplier is a
    /// presentation scale, not a zoom level the player can reach, so `S = 4` at
    /// the 1:1 `zoom = 64` needs `256`. A zero factor returns the camera
    /// unchanged.
    pub fn scaled(self, factor: u32) -> Self {
        if factor == 0 || factor == 1 {
            return self;
        }

        let mut viewport = Self {
            screen_width: self.screen_width.saturating_mul(factor),
            screen_height: self.screen_height.saturating_mul(factor),
            center_x: self.center_x,
            center_y: self.center_y,
            zoom: self.zoom.saturating_mul(factor),
        };

        viewport.clamp_center();

        viewport
    }

    /// Center in world pixels.
    #[inline]
    pub fn center(&self) -> (i32, i32) {
        (self.center_x, self.center_y)
    }

    /// Scale: screen pixels per cell.
    #[inline]
    pub fn zoom(&self) -> u32 {
        self.zoom
    }

    /// Cell size on screen in pixels.
    #[inline]
    pub fn tile_pixels(&self) -> i32 {
        self.zoom as i32
    }

    /// 1:1 scale (a cell of 64 screen pixels).
    #[inline]
    pub fn is_native_zoom(&self) -> bool {
        self.zoom == DEFAULT_ZOOM
    }

    /// Sets the scale with clamping; returns the actual scale.
    pub fn set_zoom(&mut self, zoom: u32) -> u32 {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.clamp_center();
        self.zoom
    }

    /// Zooms in by [`ZOOM_STEP`] (with clamping).
    pub fn zoom_in(&mut self) -> u32 {
        self.set_zoom(self.zoom * ZOOM_STEP)
    }

    /// Zooms out by [`ZOOM_STEP`] (with clamping).
    pub fn zoom_out(&mut self) -> u32 {
        self.set_zoom(self.zoom / ZOOM_STEP)
    }

    /// The world pixel visible at the top-left corner of the screen.
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

    /// World pixels -> screen pixels.
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

    /// Screen pixels -> world pixels.
    pub fn screen_to_world(&self, screen_x: i32, screen_y: i32) -> (i32, i32) {
        let (origin_x, origin_y) = self.origin();
        let zoom = i64::from(self.zoom);

        (
            origin_x + ((i64::from(screen_x) * i64::from(MAP_TILE_PIXELS)) / zoom) as i32,
            origin_y + ((i64::from(screen_y) * i64::from(MAP_TILE_PIXELS)) / zoom) as i32,
        )
    }

    /// Top-left corner of the cell on screen.
    pub fn grid_to_screen(&self, grid_x: i16, grid_y: i16) -> (i32, i32) {
        self.world_to_screen(
            i32::from(grid_x) * MAP_TILE_PIXELS,
            i32::from(grid_y) * MAP_TILE_PIXELS,
        )
    }

    /// The cell under a screen point (may be beyond the map — not clamped).
    pub fn screen_to_grid(&self, screen_x: i32, screen_y: i32) -> (i16, i16) {
        let (world_x, world_y) = self.screen_to_world(screen_x, screen_y);

        (
            (world_x.div_euclid(MAP_TILE_PIXELS)) as i16,
            (world_y.div_euclid(MAP_TILE_PIXELS)) as i16,
        )
    }

    /// Rectangle of the cell on screen (size — [`Viewport::tile_pixels`]).
    pub fn tile_rect(&self, grid_x: i16, grid_y: i16) -> PixelRect {
        let (screen_x, screen_y) = self.grid_to_screen(grid_x, grid_y);

        PixelRect::new(screen_x, screen_y, self.tile_pixels(), self.tile_pixels())
    }

    /// Centers the camera on a cell.
    pub fn center_on_grid(&mut self, grid_x: i16, grid_y: i16) {
        self.center_x = i32::from(grid_x) * MAP_TILE_PIXELS + MAP_TILE_PIXELS / 2;
        self.center_y = i32::from(grid_y) * MAP_TILE_PIXELS + MAP_TILE_PIXELS / 2;
        self.clamp_center();
    }

    /// Centers the camera on a world pixel.
    pub fn center_on_world(&mut self, world_x: i32, world_y: i32) {
        self.center_x = world_x;
        self.center_y = world_y;
        self.clamp_center();
    }

    /// Moves the camera by a screen delta (e.g. with the mouse).
    ///
    /// Screen pixels are converted to world pixels at the current scale, so that the
    /// content moves exactly with the cursor; the result is clamped to the map.
    pub fn scroll_by_screen(&mut self, delta_x: i32, delta_y: i32) {
        let world_x =
            (i64::from(delta_x) * i64::from(MAP_TILE_PIXELS) / i64::from(self.zoom)) as i32;
        let world_y =
            (i64::from(delta_y) * i64::from(MAP_TILE_PIXELS) / i64::from(self.zoom)) as i32;

        self.center_x += world_x;
        self.center_y += world_y;
        self.clamp_center();
    }

    /// Prevents the camera from going beyond the map (accounting for half the screen).
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

    /// Visible cell range (clipped to the map); `None` if the camera looks
    /// past the map.
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

    /// The cell is visible on screen.
    pub fn is_tile_visible(&self, grid_x: i16, grid_y: i16) -> bool {
        self.visible_tiles()
            .is_some_and(|range| range.contains(grid_x, grid_y))
    }

    /// Screen positions of the cell-grid lines of the current view.
    ///
    /// # Oracle
    ///
    /// `DrawMap_RenderSimpleGridOverlay` (`drawmap.cpp:922-954`) walks the cells of
    /// the redrawn rectangle and draws a line at the top-left corner of each cell;
    /// `DrawMap_RenderComplexGridOverlay` (`:956-996`) does the same per cell in the
    /// surface color. The lines start at the first cell boundary inside the map
    /// window (positions are clipped to the pixel bounds).
    ///
    /// # Full-viewport coverage
    ///
    /// The map window is exactly 7×7 cells of 64 px (`WINDOW_MAIN_MAP`,
    /// `window_manager.cpp:89`; [`crate::layout::MAIN_MAP`]), so the last cell
    /// boundary of the window (`448`) lies one pixel past the map buffer
    /// (`0..=447`). A plain boundary walk therefore left the outer column and row
    /// of the playable area without a closing grid line — the border cells are
    /// partly covered by the game UI (panels, End Turn), and a unit that reached
    /// the edge of the map visually slid out of the grid. The port closes the
    /// grid at the last buffer row/column, so the lines cover the whole playable
    /// viewport (`max-brh-qa/plans/defects-manual-1.2.md`, defect 7).
    ///
    /// The closing is needed on **all four** sides, not only the right/bottom one:
    /// the camera origin is not cell-aligned in general (`scroll_by_screen` /
    /// `center_on_world` move it by arbitrary pixels), so the first cell boundary
    /// lies *inside* the map window and the border strip of the adjacent cells
    /// (`0..first_boundary`) had no line at the window edge. The UI panels abut the
    /// map on every side, which made those border cells look cut off. Closing the
    /// walk at `0` and `width - 1` / `height - 1` makes the grid cover the whole
    /// playable area at any camera position and zoom (defect 7 of the manual run
    /// 1.2, second pass).
    ///
    /// The returned positions are ascending and inside `0..width` / `0..height`.
    pub fn grid_lines(&self, width: i32, height: i32) -> GridLines {
        let mut lines = GridLines::default();

        if width <= 0 || height <= 0 {
            return lines;
        }

        let (x0, y0) = self.screen_to_grid(0, 0);
        let (x1, y1) = self.screen_to_grid(width - 1, height - 1);

        for grid_x in x0..=x1.saturating_add(1) {
            let (screen_x, _) = self.grid_to_screen(grid_x, 0);

            if (0..width).contains(&screen_x) {
                lines.xs.push(screen_x);
            }
        }

        for grid_y in y0..=y1.saturating_add(1) {
            let (_, screen_y) = self.grid_to_screen(0, grid_y);

            if (0..height).contains(&screen_y) {
                lines.ys.push(screen_y);
            }
        }

        // Close the playable viewport on all four sides: the top/left window edge
        // is a partial cell whenever the camera origin is not cell-aligned, and
        // the bottom/right one lies one pixel past an exact 7×64 buffer.
        if lines.xs.first().copied() != Some(0) {
            lines.xs.insert(0, 0);
        }

        if lines.ys.first().copied() != Some(0) {
            lines.ys.insert(0, 0);
        }

        if lines.xs.last().copied() != Some(width - 1) {
            lines.xs.push(width - 1);
        }

        if lines.ys.last().copied() != Some(height - 1) {
            lines.ys.push(height - 1);
        }

        lines
    }

    /// The screen rectangle intersects the screen.
    pub fn is_rect_visible(&self, rect: PixelRect) -> bool {
        rect.intersect(PixelRect::new(
            0,
            0,
            self.screen_width as i32,
            self.screen_height as i32,
        ))
        .is_some()
    }

    /// Indices of the units visible in the current frame, in §5.4 drawing order.
    ///
    /// `margin_tiles` expands the visible area so that sprites whose hot
    /// spot is off screen but whose "body" is on screen do not disappear.
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

/// Drawing order of a set of positions: by "ground" (`y`), then by `x`.
///
/// Returns indices of the source slice; the sort is stable, so units
/// with the same position keep the list order.
pub fn draw_order(units: &[(i16, i16)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..units.len()).collect();

    order.sort_by_key(|&index| (units[index].1, units[index].0));

    order
}

#[cfg(test)]
#[path = "viewport_tests.rs"]
mod tests;
