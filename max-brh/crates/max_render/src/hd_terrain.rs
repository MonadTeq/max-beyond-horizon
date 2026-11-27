//! HD map tiles (`max-brh-docs/development/remaster-hd.md` §R3).
//!
//! `.WRL` terrain tiles carry **no** team colours, no brightness levels and no
//! palette cycles: the map palette is baked into the pixels, so an HD remaster
//! ships one pre-baked RGBA8 texture per `(map, tile_id)` pair. This module is
//! the presentation-side substitute for [`TerrainAtlas`]:
//!
//! * [`HdTerrainAtlas::load`] resolves every bank tile through the R1 loader
//!   ([`max_assets::remaster::RemasterCache::resolve`]) using the tag convention
//!   of [`max_assets::remaster::terrain_tile_tag`];
//! * [`HdTerrainAtlas::render_tile_into`] and
//!   [`HdTerrainAtlas::render_tile_cpp_into`] sample the HD source with exactly
//!   the coordinate mapping of [`TerrainAtlas::render_tile_into`] /
//!   [`TerrainAtlas::render_tile_cpp_into`], only the source dimensions change
//!   (`64·S` instead of 64);
//! * a tile without an HD asset — an unknown tag, a missing file, a corrupt
//!   `.ktx2`, or a non-square texture — is drawn from the original palette
//!   atlas. Fallback is **per asset**: a partial remaster still renders.
//!
//! This is presentation only. Nothing here is imported by `max_core`, and the
//! original 640×480 path (`RenderScale::PixelPerfect`) never calls this module:
//! it draws [`TerrainAtlas`] directly, byte for byte.

use max_assets::remaster::{terrain_tile_tag, RemasterCache, RemasterTexture};

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};
use crate::render_scale::RenderScale;
use crate::terrain::{cpp_tile_sample_offsets_for, TerrainAtlas};
use crate::viewport::Viewport;

/// A decoded HD tile: square RGBA8 pixels.
///
/// The pre-baked terrain texture has no per-team variants, so a plain RGBA8
/// buffer is enough (no `ColorLut`/`BrightnessLut`/cycles at draw time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdTile {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl HdTile {
    /// Builds a tile from RGBA8 bytes; `None` for a non-square, empty or
    /// truncated texture (the caller then falls back to the original tile).
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 || width != height {
            return None;
        }

        if rgba.len() != width as usize * height as usize * 4 {
            return None;
        }

        Some(Self {
            width,
            height,
            rgba,
        })
    }

    /// Builds a tile from an R1 decoded texture; `None` for a non-square texture.
    pub fn from_texture(texture: &RemasterTexture) -> Option<Self> {
        Self::from_rgba(texture.width(), texture.height(), texture.pixels().to_vec())
    }

    /// Side of the tile in texels (the tile is square).
    pub fn side(&self) -> u32 {
        self.width
    }

    /// Width in texels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in texels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// RGBA8 texels, row-major.
    pub fn pixels(&self) -> &[u8] {
        &self.rgba
    }

    /// Texel color; `None` outside the tile.
    pub fn texel(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let offset = (y as usize * self.width as usize + x as usize) * 4;
        let pixel = &self.rgba[offset..offset + 4];

        Some(Rgba::from_rgba([pixel[0], pixel[1], pixel[2], pixel[3]]))
    }
}

/// Terrain atlas with an optional pre-baked HD texture per tile.
///
/// The original [`TerrainAtlas`] is kept as the fallback source, so every tile
/// without an HD asset still draws exactly as before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdTerrainAtlas {
    source: TerrainAtlas,
    scale: RenderScale,
    /// `source.tile_count()` entries: `Some` — an HD texture for that tile id.
    tiles: Vec<Option<HdTile>>,
}

impl HdTerrainAtlas {
    /// An HD atlas with no HD assets: every tile falls back to `source`.
    pub fn new(source: TerrainAtlas, scale: RenderScale) -> Self {
        let tiles = vec![None; source.tile_count() as usize];

        Self {
            source,
            scale,
            tiles,
        }
    }

    /// An HD atlas from already decoded tiles (tests and callers that resolve
    /// assets themselves). The vector is aligned to `source.tile_count()`:
    /// missing entries become `None` (fallback), extra entries are dropped.
    pub fn with_tiles(
        source: TerrainAtlas,
        scale: RenderScale,
        mut tiles: Vec<Option<HdTile>>,
    ) -> Self {
        tiles.resize(source.tile_count() as usize, None);
        tiles.truncate(source.tile_count() as usize);

        Self {
            source,
            scale,
            tiles,
        }
    }

    /// Loads every HD tile of `source` from the remaster root.
    ///
    /// For each tile id the R1 cache is asked for
    /// [`terrain_tile_tag(world, tile_id, scale.scale())`](terrain_tile_tag).
    /// A tag the manifest does not list, a missing `.ktx2`, a transcode failure
    /// or a non-square texture leaves that tile on the original atlas — the
    /// fallback is per asset and never aborts the load.
    ///
    /// The texture resolution is taken from the decoded asset, so an asset
    /// authored at `×2` or `×4` is sampled at its native size. `scale` names the
    /// tag's multiplier and is stored for diagnostics.
    pub fn load(
        cache: &RemasterCache,
        world: &str,
        source: TerrainAtlas,
        scale: RenderScale,
    ) -> Self {
        let mut tiles = Vec::with_capacity(source.tile_count() as usize);

        for tile_id in 0..source.tile_count() {
            let tag = terrain_tile_tag(world, tile_id, scale.scale());
            let tile = cache.resolve(&tag).as_ref().and_then(HdTile::from_texture);

            tiles.push(tile);
        }

        Self::with_tiles(source, scale, tiles)
    }

    /// The original palette atlas used for fallback.
    pub fn source(&self) -> &TerrainAtlas {
        &self.source
    }

    /// Presentation scale the atlas was loaded for.
    pub fn scale(&self) -> RenderScale {
        self.scale
    }

    /// Tile side at this presentation scale (`64·S`).
    pub fn tile_size(&self) -> i32 {
        crate::viewport::MAP_TILE_PIXELS * self.scale.scale() as i32
    }

    /// HD texture of a tile id, if one was loaded.
    pub fn hd_tile(&self, tile_id: u16) -> Option<&HdTile> {
        self.tiles.get(tile_id as usize)?.as_ref()
    }

    /// Number of tiles with an HD texture (`0..=tile_count()`).
    pub fn hd_tile_count(&self) -> usize {
        self.tiles.iter().filter(|tile| tile.is_some()).count()
    }

    /// `true` when there is nothing to draw at all.
    pub fn is_empty(&self) -> bool {
        self.source.is_empty() && self.hd_tile_count() == 0
    }

    /// Map size in cells (from the original atlas).
    pub fn size(&self) -> (i16, i16) {
        self.source.size()
    }

    /// Number of tiles in the map bank.
    pub fn tile_count(&self) -> u16 {
        self.source.tile_count()
    }

    /// Draws the visible map cells; returns the number of cells.
    ///
    /// Mirrors [`TerrainAtlas::render_into`]: the HD source is used per tile
    /// where available, otherwise the original tile.
    pub fn render_into(&self, dst: &mut RgbaBuffer, viewport: &Viewport) -> u32 {
        let Some(range) = viewport.visible_tiles() else {
            return 0;
        };

        let mut painted = 0;

        for (grid_x, grid_y) in range.cells() {
            let rect = viewport.tile_rect(grid_x, grid_y);

            if self.render_tile_into(dst, (grid_x, grid_y), rect) > 0 {
                painted += 1;
            }
        }

        painted
    }

    /// Draws one cell into `rect`, sampling the HD source where present.
    ///
    /// The coordinate mapping is the same as
    /// [`TerrainAtlas::render_tile_into`]: every screen pixel takes the start of
    /// an equal source block (`local · source / rect`). Only `source` is the HD
    /// tile size instead of 64.
    pub fn render_tile_into(&self, dst: &mut RgbaBuffer, grid: (i16, i16), rect: PixelRect) -> u32 {
        if rect.is_empty() {
            return 0;
        }

        let Some(tile_id) = self.source.tile_id_at(grid.0, grid.1) else {
            return 0;
        };

        if tile_id >= self.source.tile_count() {
            return 0;
        }

        match self.hd_tile(tile_id) {
            Some(tile) => render_hd_tile_into(dst, tile, rect),
            None => self.source.render_tile_into(dst, grid, rect),
        }
    }

    /// Draws one cell with the exact `Gfx_DecodeMapTile` sampling of the
    /// original, but from the HD source where present.
    ///
    /// The difference from [`Self::render_tile_into`] is the same as in
    /// [`TerrainAtlas::render_tile_cpp_into`]: the original stretches the tile
    /// at the edges (`(source - 1) << 16 / (size - 1) + 8`) instead of taking
    /// the start of an equal block.
    pub fn render_tile_cpp_into(
        &self,
        dst: &mut RgbaBuffer,
        grid: (i16, i16),
        rect: PixelRect,
    ) -> u32 {
        if rect.is_empty() {
            return 0;
        }

        let Some(tile_id) = self.source.tile_id_at(grid.0, grid.1) else {
            return 0;
        };

        if tile_id >= self.source.tile_count() {
            return 0;
        }

        match self.hd_tile(tile_id) {
            Some(tile) => render_hd_tile_cpp_into(dst, tile, rect),
            None => self.source.render_tile_cpp_into(dst, grid, rect),
        }
    }
}

/// Equal-block sampling of an HD tile (the HD counterpart of
/// `TerrainAtlas::render_tile_into`).
fn render_hd_tile_into(dst: &mut RgbaBuffer, tile: &HdTile, rect: PixelRect) -> u32 {
    let source_width = tile.width() as usize;
    let source_height = tile.height() as usize;
    let rows: Vec<usize> = (0..rect.height as usize)
        .map(|local_y| (local_y * source_height / rect.height as usize) * source_width)
        .collect();
    let columns: Vec<usize> = (0..rect.width as usize)
        .map(|local_x| local_x * source_width / rect.width as usize)
        .collect();

    let mut painted = 0;

    for (local_y, row) in rows.iter().enumerate() {
        for (local_x, column) in columns.iter().enumerate() {
            let offset = (*row + *column) * 4;

            if let Some(pixel) = tile.pixels().get(offset..offset + 4) {
                if dst.set(
                    rect.ulx + local_x as i32,
                    rect.uly + local_y as i32,
                    Rgba::from_rgba([pixel[0], pixel[1], pixel[2], pixel[3]]),
                ) {
                    painted += 1;
                }
            }
        }
    }

    painted
}

/// `Gfx_DecodeMapTile` sampling of an HD tile (the HD counterpart of
/// `TerrainAtlas::render_tile_cpp_into`).
fn render_hd_tile_cpp_into(dst: &mut RgbaBuffer, tile: &HdTile, rect: PixelRect) -> u32 {
    let source_width = tile.width() as usize;
    let columns = cpp_tile_sample_offsets_for(source_width, rect.width);
    let rows: Vec<usize> = cpp_tile_sample_offsets_for(tile.height() as usize, rect.height)
        .into_iter()
        .map(|row| row * source_width)
        .collect();
    let mut painted = 0;

    for (local_y, row) in rows.iter().enumerate() {
        for (local_x, column) in columns.iter().enumerate() {
            let offset = (*row + *column) * 4;

            if let Some(pixel) = tile.pixels().get(offset..offset + 4) {
                if dst.set(
                    rect.ulx + local_x as i32,
                    rect.uly + local_y as i32,
                    Rgba::from_rgba([pixel[0], pixel[1], pixel[2], pixel[3]]),
                ) {
                    painted += 1;
                }
            }
        }
    }

    painted
}

#[cfg(test)]
#[path = "hd_terrain_tests.rs"]
mod tests;
