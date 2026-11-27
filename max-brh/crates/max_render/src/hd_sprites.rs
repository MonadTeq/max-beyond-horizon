//! HD unit sprites: index-domain textures recolored by a CPU palette LUT
//! (`max-brh-docs/development/remaster-hd.md` §R4, fork 1).
//!
//! # Why index-domain
//!
//! A unit sprite is not a picture: the palette index of every pixel is
//! recolored at draw time — team slots 32–39/48–63 through [`ColorLut`],
//! brightness levels 0..=7 through [`BrightnessLut`], and the palette cycles
//! (`ColorCycles`/`rotate_palette`) rotate the DAC slots 2–10 times per second
//! so that beacons and the drill indicator blink. Baking RGBA would freeze
//! all of that, so an HD sprite ships as a **high-resolution index map**: one
//! palette index per texel, recolored on the CPU into RGBA8 exactly like
//! [`SpriteRenderer::color_for`] does
//! (`color = palette[brightness[team[index]]]`).
//!
//! # Transport convention
//!
//! The R1 loader ([`max_assets::remaster::RemasterCache::resolve`]) hands out a
//! decoded RGBA8 texture; for an index map the channels carry:
//!
//! | channel | meaning |
//! |---|---|
//! | `R` ([`PAYLOAD_CHANNEL`]) | palette index of the texel (sprite) or the darkening mask level (shadow) |
//! | `A` ([`COVERAGE_CHANNEL`]) | texel coverage: `0` — transparent, `255` — opaque |
//!
//! `G`/`B` are unused. The payload must be encoded **losslessly** (UASTC or
//! uncompressed): a lossy ETC1S payload would shift palette indices and hence
//! colors.
//!
//! # What this module provides
//!
//! * [`HdIndexMap`] — the index texture itself (`from_rgba`/`from_texture`),
//!   [`HdIndexFrame`] — an index map with a hot spot (a body frame),
//!   [`HdShadowFrame`] — an index map used as a darkening mask;
//! * [`IndexPaletteLut`] — the 256-entry CPU palette LUT
//!   (`team → brightness → palette`), including the cycle-rotated snapshot;
//! * [`HdSpriteTexture`] + [`HdSpriteCache`] — the recolored RGBA8 texture and
//!   its cache, keyed by `(sprite, team, brightness, cycle frame)`;
//! * [`HdSpriteBank`] — the HD frames of one `MAX.RES` sprite resource with the
//!   original sheet as the **per-asset fallback**;
//! * [`HdSpriteRenderer`] — the drawer: HD geometry when a frame has an HD
//!   asset, the original [`SpriteRenderer`] path otherwise; HD shadows darken
//!   the already drawn pixels (`Rgba::darken`, level 5) through the mask.
//!
//! # Geometry
//!
//! `zoom` keeps the [`SpriteRenderer`] unit — render-frame pixels per 64-texel
//! tile, where `64` is 1:1. An HD source of `L` texels drawn at
//! `zoom` in a frame of scale `S` covers `L · zoom / (64 · S)` render pixels
//! and its hot spot is shifted by the same formula, so at the world scale of
//! `SpriteRenderer` (logical `zoom = 64`, i.e. `zoom = 64 · S` in the HD frame)
//! the picture is exactly the `×S` nearest-neighbour upscale of the original
//! 1:1 render. A frame without an HD asset is drawn by [`SpriteRenderer`] with
//! the HD `zoom`, which produces the same `×S` upscale from the low-res source.
//!
//! This is presentation only. Nothing here is imported by `max_core`, and the
//! original 640×480 path ([`RenderScale::PixelPerfect`]) never calls this
//! module: `S = 1` stays byte for byte the output of [`SpriteRenderer`].

use std::collections::{HashMap, VecDeque};

use max_assets::remaster::{sprite_frame_tag, sprite_shadow_tag, RemasterCache, RemasterTexture};
use max_assets::sprite::{ShadowSheet, SpriteFrame, SpriteSheet};

use crate::buffer::{Rgba, RgbaBuffer};
use crate::palette::{
    rotate_palette, BrightnessLut, ColorCycles, ColorLut, Palette, TeamColors, COLOR_CYCLES,
    FULL_BRIGHTNESS_LEVEL, PALETTE_SIZE, SHADOW_BRIGHTNESS_LEVEL,
};
use crate::render_scale::RenderScale;
use crate::sprite_atlas::{scale_pixels, DrawStats, SpriteRenderer};

/// Channel of the payload byte in an index-map texel: the palette index of a
/// sprite texel, or the darkening mask level of a shadow texel.
pub const PAYLOAD_CHANNEL: usize = 0;
/// Channel of the texel coverage: `0` — transparent, `255` — opaque.
pub const COVERAGE_CHANNEL: usize = 3;
/// Render pixels covered by one tile at 1:1 (`Gfx_ZoomLevel` = 64).
const TILE_PIXELS: i64 = 64;
/// Largest `zoom` at which the original draws units as solid team-colour
/// silhouettes (`UnitInfo::RenderSprite`: `zoom_level < 8`, §5.2).
pub const SILHOUETTE_ZOOM: u32 = 8;
/// Entries [`HdSpriteCache::new`] keeps before evicting the oldest one.
///
/// The cache keys include the palette-cycle frame, so a blinking beacon or a
/// working drill would otherwise grow it without bound.
pub const DEFAULT_CACHE_LIMIT: usize = 64;

/// `true` when the original renders units as team-colour silhouettes.
///
/// `UnitInfo::RenderSprite` (`unitinfo.cpp:3111`) switches to
/// `Gfx_TeamColorIndexBase` while `zoom_level < 8`; with the port's 1:1
/// `scaling_factor` (`2`) `zoom_level` equals our `zoom`, so the threshold is
/// [`SILHOUETTE_ZOOM`].
pub const fn is_silhouette_zoom(zoom: u32) -> bool {
    zoom < SILHOUETTE_ZOOM
}

/// Render-space offset of a source-sprite offset at `zoom`: `offset · zoom / 64`.
///
/// The original scales the shadow (`RenderShadow`) and the turret
/// (`angle_offsets`) offsets together with the map scale, so both paths of the
/// HD renderer use this rule and never place a shadow differently from
/// [`SpriteRenderer`].
pub fn scaled_offset(offset: (i32, i32), zoom: u32) -> (i32, i32) {
    (scale_pixels(offset.0, zoom), scale_pixels(offset.1, zoom))
}

// ---------------------------------------------------------------------------
// Index maps
// ---------------------------------------------------------------------------

/// A high-resolution index map: one payload byte plus one coverage byte per
/// texel, row-major.
///
/// The map carries **no colors**: it is the HD counterpart of the palette
/// indices of [`SpriteFrame`]. Colors appear only after [`Self::recolor`] with
/// an [`IndexPaletteLut`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdIndexMap {
    width: u32,
    height: u32,
    payload: Vec<u8>,
    coverage: Vec<u8>,
}

impl HdIndexMap {
    /// Builds a map from separate payload/coverage arrays; `None` for an empty
    /// map or a length mismatch.
    pub fn from_indices(
        width: u32,
        height: u32,
        payload: Vec<u8>,
        coverage: Vec<u8>,
    ) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }

        let texels = width as usize * height as usize;

        if payload.len() != texels || coverage.len() != texels {
            return None;
        }

        Some(Self {
            width,
            height,
            payload,
            coverage,
        })
    }

    /// Builds a fully opaque map (coverage `255` everywhere).
    pub fn from_opaque_indices(width: u32, height: u32, payload: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }

        let texels = width as usize * height as usize;

        if payload.len() != texels {
            return None;
        }

        Some(Self {
            width,
            height,
            payload,
            coverage: vec![u8::MAX; texels],
        })
    }

    /// Decodes the transport convention ([`PAYLOAD_CHANNEL`] /
    /// [`COVERAGE_CHANNEL`]); `None` for an empty or truncated texture.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }

        let texels = width as usize * height as usize;

        if rgba.len() < texels * 4 {
            return None;
        }

        let mut payload = Vec::with_capacity(texels);
        let mut coverage = Vec::with_capacity(texels);

        for texel in rgba.chunks_exact(4).take(texels) {
            payload.push(texel[PAYLOAD_CHANNEL]);
            coverage.push(texel[COVERAGE_CHANNEL]);
        }

        Some(Self {
            width,
            height,
            payload,
            coverage,
        })
    }

    /// Decodes a texture resolved by the R1 loader.
    pub fn from_texture(texture: &RemasterTexture) -> Option<Self> {
        Self::from_rgba(texture.width(), texture.height(), texture.pixels())
    }

    /// Width in texels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in texels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Number of texels.
    pub fn len(&self) -> usize {
        self.payload.len()
    }

    /// No texels (impossible for a successfully built map).
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }

    /// Payload byte of a texel: the palette index (sprite) or the darkening
    /// mask level (shadow); `None` outside the map.
    pub fn payload(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }

        Some(self.payload[y as usize * self.width as usize + x as usize])
    }

    /// Coverage of a texel (`0` — transparent); `None` outside the map.
    pub fn coverage(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }

        Some(self.coverage[y as usize * self.width as usize + x as usize])
    }

    /// The texel is painted at all.
    pub fn is_opaque(&self, x: u32, y: u32) -> bool {
        self.coverage(x, y).is_some_and(|coverage| coverage != 0)
    }

    /// Recolors the whole map through the palette LUT (CPU): `Rgba` of
    /// `palette[brightness[team[payload]]]` with the texel coverage as alpha.
    pub fn recolor(&self, lut: &IndexPaletteLut) -> HdSpriteTexture {
        HdSpriteTexture::from_index_map(self, lut)
    }
}

/// An HD sprite frame: an index map plus the hot spot (anchor) in HD texels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdIndexFrame {
    map: HdIndexMap,
    hotx: i16,
    hoty: i16,
}

impl HdIndexFrame {
    /// Frame from an index map and its hot spot.
    pub fn new(map: HdIndexMap, hotx: i16, hoty: i16) -> Self {
        Self { map, hotx, hoty }
    }

    /// The index map of the frame.
    pub fn map(&self) -> &HdIndexMap {
        &self.map
    }

    /// Width in HD texels.
    pub fn width(&self) -> u32 {
        self.map.width()
    }

    /// Height in HD texels.
    pub fn height(&self) -> u32 {
        self.map.height()
    }

    /// Hot spot `(x, y)` in HD texels.
    pub fn hot_spot(&self) -> (i16, i16) {
        (self.hotx, self.hoty)
    }

    /// Recolored RGBA8 texture of the frame.
    pub fn recolor(&self, lut: &IndexPaletteLut) -> HdSpriteTexture {
        self.map.recolor(lut)
    }

    /// Size the frame covers at `zoom` in a frame of scale `scale`.
    pub fn render_size(&self, zoom: u32, scale: u32) -> (i32, i32) {
        (
            hd_length(self.width(), zoom, scale),
            hd_length(self.height(), zoom, scale),
        )
    }

    /// Top-left corner the frame occupies for `anchor` at `zoom`.
    pub fn render_origin(&self, anchor: (i32, i32), zoom: u32, scale: u32) -> (i32, i32) {
        (
            anchor.0 - hd_pixels(i32::from(self.hotx), zoom, scale),
            anchor.1 - hd_pixels(i32::from(self.hoty), zoom, scale),
        )
    }
}

/// An HD shadow frame: an index map whose payload is the darkening mask level
/// (`0` — no shadow, `255` — the full [`SHADOW_BRIGHTNESS_LEVEL`] darkening)
/// plus the hot spot in HD texels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdShadowFrame {
    mask: HdIndexMap,
    hotx: i16,
    hoty: i16,
}

impl HdShadowFrame {
    /// Shadow from a mask map and its hot spot.
    pub fn new(mask: HdIndexMap, hotx: i16, hoty: i16) -> Self {
        Self { mask, hotx, hoty }
    }

    /// The mask map.
    pub fn mask(&self) -> &HdIndexMap {
        &self.mask
    }

    /// Width in HD texels.
    pub fn width(&self) -> u32 {
        self.mask.width()
    }

    /// Height in HD texels.
    pub fn height(&self) -> u32 {
        self.mask.height()
    }

    /// Hot spot `(x, y)` in HD texels.
    pub fn hot_spot(&self) -> (i16, i16) {
        (self.hotx, self.hoty)
    }

    /// Number of texels that darken the map at all.
    pub fn darkened_texels(&self) -> usize {
        self.mask
            .payload
            .iter()
            .filter(|&&level| level != 0)
            .count()
    }

    /// Size the mask covers at `zoom` in a frame of scale `scale`.
    pub fn render_size(&self, zoom: u32, scale: u32) -> (i32, i32) {
        (
            hd_length(self.width(), zoom, scale),
            hd_length(self.height(), zoom, scale),
        )
    }

    /// Top-left corner the mask occupies for `anchor` at `zoom`.
    pub fn render_origin(&self, anchor: (i32, i32), zoom: u32, scale: u32) -> (i32, i32) {
        (
            anchor.0 - hd_pixels(i32::from(self.hotx), zoom, scale),
            anchor.1 - hd_pixels(i32::from(self.hoty), zoom, scale),
        )
    }
}

// ---------------------------------------------------------------------------
// Palette LUT
// ---------------------------------------------------------------------------

/// The 256-entry CPU palette LUT of one recolor state.
///
/// Entry `i` is `palette[brightness[team[i]]]` — the exact expression
/// [`SpriteRenderer::color_for`] evaluates per pixel, evaluated once for every
/// index. The palette snapshot is passed in already rotated by
/// [`IndexPaletteLut::with_cycles`], so the LUT also covers the palette cycles
/// (beacons, drill indicator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexPaletteLut {
    colors: [Rgba; PALETTE_SIZE],
}

impl IndexPaletteLut {
    /// LUT of `team` and brightness `level` (0..=7) over a palette snapshot.
    pub fn new(palette: &Palette, brightness: &BrightnessLut, team: ColorLut, level: u8) -> Self {
        let level = level.min(FULL_BRIGHTNESS_LEVEL);
        let mut colors = [Rgba::TRANSPARENT; PALETTE_SIZE];

        for (index, color) in colors.iter_mut().enumerate() {
            let team_index = team.map(index as u8);
            let level_index = brightness.map(level, team_index);

            *color = palette.rgba(level_index);
        }

        Self { colors }
    }

    /// LUT over the palette snapshot of `cycles` — [`rotate_palette`] first,
    /// then the team/brightness LUT, exactly like the app passes its animated
    /// palette to [`SpriteRenderer`].
    pub fn with_cycles(
        palette: &Palette,
        cycles: &ColorCycles,
        brightness: &BrightnessLut,
        team: ColorLut,
        level: u8,
    ) -> Self {
        let snapshot = rotate_palette(palette, cycles);

        Self::new(&snapshot, brightness, team, level)
    }

    /// LUT of an existing renderer (the fallback path and the tests use it to
    /// prove both paths agree).
    pub fn from_renderer(renderer: &SpriteRenderer<'_>) -> Self {
        let mut colors = [Rgba::TRANSPARENT; PALETTE_SIZE];

        for (index, color) in colors.iter_mut().enumerate() {
            *color = renderer.color_for(index as u8);
        }

        Self { colors }
    }

    /// Color of a palette index after team, brightness and cycles.
    pub fn color(&self, index: u8) -> Rgba {
        self.colors[usize::from(index)]
    }

    /// The whole table.
    pub fn colors(&self) -> &[Rgba; PALETTE_SIZE] {
        &self.colors
    }
}

// ---------------------------------------------------------------------------
// Recolored texture and its cache
// ---------------------------------------------------------------------------

/// A recolored HD sprite: RGBA8 texels, ready to be uploaded or blitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdSpriteTexture {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl HdSpriteTexture {
    /// Recolors an index map through the LUT: transparent texels stay
    /// transparent, painted ones take the LUT color with the texel coverage as
    /// alpha (`255` for a hard-edged asset, so the result matches
    /// [`SpriteRenderer`] byte for byte).
    pub fn from_index_map(map: &HdIndexMap, lut: &IndexPaletteLut) -> Self {
        let mut rgba = Vec::with_capacity(map.len() * 4);

        for texel in 0..map.len() {
            let coverage = map.coverage[texel];

            if coverage == 0 {
                rgba.extend_from_slice(&Rgba::TRANSPARENT.to_rgba());
                continue;
            }

            let color = lut.color(map.payload[texel]);

            rgba.extend_from_slice(&[color.r, color.g, color.b, coverage]);
        }

        Self {
            width: map.width,
            height: map.height,
            rgba,
        }
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

    /// Texel color; `None` outside the texture.
    pub fn texel(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let offset = (y as usize * self.width as usize + x as usize) * 4;

        Some(Rgba::from_rgba([
            self.rgba[offset],
            self.rgba[offset + 1],
            self.rgba[offset + 2],
            self.rgba[offset + 3],
        ]))
    }
}

/// The recolor state of a unit sprite: team, brightness level and the palette
/// cycle frame.
///
/// [`Self::cycles`] is [`ColorCycles::step_signature`] — the palette snapshot
/// depends only on the step counts, so the array is the complete identity of
/// the "cycle frame" part of a cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HdRecolorState {
    /// Team whose [`ColorLut`] recolors the command slots.
    pub team: TeamColors,
    /// Brightness level (clamped to 0..=7 on use).
    pub brightness: u8,
    /// Palette cycle frame ([`ColorCycles::step_signature`]).
    pub cycles: [u64; COLOR_CYCLES.len()],
}

impl Default for HdRecolorState {
    fn default() -> Self {
        Self {
            team: TeamColors::Gray,
            brightness: FULL_BRIGHTNESS_LEVEL,
            cycles: [0; COLOR_CYCLES.len()],
        }
    }
}

impl HdRecolorState {
    /// State with the palette cycles at their initial position (`dt = 0`).
    pub fn new(team: TeamColors, brightness: u8) -> Self {
        Self {
            team,
            brightness,
            cycles: [0; COLOR_CYCLES.len()],
        }
    }

    /// The same state at the cycle frame of `cycles`.
    pub fn with_cycles(self, cycles: &ColorCycles) -> Self {
        Self {
            cycles: cycles.step_signature(),
            ..self
        }
    }

    /// The cycle clock of the state (for [`IndexPaletteLut::with_cycles`]).
    pub fn clock(self) -> ColorCycles {
        ColorCycles::from_step_counts(&self.cycles)
    }

    /// Brightness level clamped to 0..=7 (as [`SpriteRenderer`] does).
    pub fn level(self) -> u8 {
        self.brightness.min(FULL_BRIGHTNESS_LEVEL)
    }
}

/// Identity of a cached recolored texture: `(sprite, team, brightness,
/// cycle frame)`.
///
/// `sprite` is the caller-assigned stable id of the sprite resource
/// ([`HdSpriteBank::with_id`]), `frame` the frame inside that bank: one cache
/// may serve several banks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HdSpriteKey {
    /// Stable id of the sprite resource (`0` when the caller has only one bank).
    pub sprite: u16,
    /// Frame index inside the bank.
    pub frame: u16,
    /// Team of the recolor.
    pub team: TeamColors,
    /// Brightness level of the recolor.
    pub brightness: u8,
    /// Palette cycle frame ([`ColorCycles::step_signature`]).
    pub cycles: [u64; COLOR_CYCLES.len()],
}

impl HdSpriteKey {
    /// Key of frame `frame` of sprite `sprite` in `state`.
    pub fn new(sprite: u16, frame: u16, state: HdRecolorState) -> Self {
        Self {
            sprite,
            frame,
            team: state.team,
            brightness: state.level(),
            cycles: state.cycles,
        }
    }
}

/// Cache of recolored HD textures, keyed by [`HdSpriteKey`].
///
/// Recoloring is a full pass over the HD index map, so the result is kept: a
/// unit of one team at one brightness inside one palette-cycle frame is
/// recolored once. The cache is bounded ([`Self::with_limit`], default
/// [`DEFAULT_CACHE_LIMIT`]) and evicts the oldest entry, because the cycle
/// frame changes many times per second and an unbounded cache would grow
/// forever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdSpriteCache {
    entries: HashMap<HdSpriteKey, HdSpriteTexture>,
    order: VecDeque<HdSpriteKey>,
    limit: usize,
    hits: u64,
    misses: u64,
}

impl Default for HdSpriteCache {
    fn default() -> Self {
        Self::new()
    }
}

impl HdSpriteCache {
    /// Empty cache with [`DEFAULT_CACHE_LIMIT`] entries.
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_CACHE_LIMIT)
    }

    /// Empty cache with an explicit entry limit (at least 1).
    pub fn with_limit(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            limit: limit.max(1),
            hits: 0,
            misses: 0,
        }
    }

    /// How many recolored textures are held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The cache holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entry limit (`0` is never returned).
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Cache hits since creation (`resolve` found the texture).
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Cache misses since creation (`resolve` had to recolor).
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// Drops every entry (the counters keep counting).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// A cached texture, if the key is present.
    pub fn get(&self, key: &HdSpriteKey) -> Option<&HdSpriteTexture> {
        self.entries.get(key)
    }

    /// The recolored texture of `key`, recoloring `map` on a miss.
    pub fn resolve(
        &mut self,
        key: HdSpriteKey,
        map: &HdIndexMap,
        lut: &IndexPaletteLut,
    ) -> &HdSpriteTexture {
        if self.entries.contains_key(&key) {
            self.hits += 1;
        } else {
            self.misses += 1;
            let texture = HdSpriteTexture::from_index_map(map, lut);
            self.insert(key, texture);
        }

        self.entries
            .get(&key)
            .expect("the entry was just returned or inserted")
    }

    fn insert(&mut self, key: HdSpriteKey, texture: HdSpriteTexture) {
        while self.entries.len() >= self.limit {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };

            self.entries.remove(&oldest);
        }

        self.order.push_back(key);
        self.entries.insert(key, texture);
    }
}

// ---------------------------------------------------------------------------
// Sprite bank and loader
// ---------------------------------------------------------------------------

/// HD frames of one `MAX.RES` sprite resource, with the original sheets as the
/// per-asset fallback.
///
/// Mirrors [`crate::hd_terrain::HdTerrainAtlas`]: the original data is kept, so
/// every frame without an HD asset still draws through [`SpriteRenderer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdSpriteBank {
    id: u16,
    source: SpriteSheet,
    source_shadow: Option<ShadowSheet>,
    scale: RenderScale,
    /// `source.len()` entries: `Some` — an HD index frame for that index.
    frames: Vec<Option<HdIndexFrame>>,
    /// `source.len()` entries: `Some` — an HD shadow mask for that index.
    shadows: Vec<Option<HdShadowFrame>>,
}

impl HdSpriteBank {
    /// A bank with no HD assets: every frame falls back to `source`.
    pub fn new(
        source: SpriteSheet,
        source_shadow: Option<ShadowSheet>,
        scale: RenderScale,
    ) -> Self {
        let frames = vec![None; source.len()];
        let shadows = vec![None; source.len()];

        Self {
            id: 0,
            source,
            source_shadow,
            scale,
            frames,
            shadows,
        }
    }

    /// A bank from already decoded frames (`tests`, or callers that resolve
    /// assets themselves). Both vectors are aligned to `source.len()`: missing
    /// entries become `None` (fallback), extra entries are dropped.
    pub fn with_frames(
        source: SpriteSheet,
        source_shadow: Option<ShadowSheet>,
        scale: RenderScale,
        mut frames: Vec<Option<HdIndexFrame>>,
        mut shadows: Vec<Option<HdShadowFrame>>,
    ) -> Self {
        frames.resize(source.len(), None);
        frames.truncate(source.len());
        shadows.resize(source.len(), None);
        shadows.truncate(source.len());

        Self {
            id: 0,
            source,
            source_shadow,
            scale,
            frames,
            shadows,
        }
    }

    /// Loads every HD frame and shadow of `source` from the remaster root.
    ///
    /// For frame `i` the R1 cache is asked for
    /// [`sprite_frame_tag(resource, i, scale)`](sprite_frame_tag) and
    /// [`sprite_shadow_tag(resource, i, scale)](sprite_shadow_tag). A tag the
    /// manifest does not list, a missing `.ktx2`, a transcode failure or a
    /// truncated texture leaves that frame on the original sheet — the fallback
    /// is per asset and never aborts the load.
    ///
    /// The hot spot of an HD frame (and shadow) is the original hot spot scaled
    /// by the resolution ratio of the asset, so an asset authored exactly `×S`
    /// keeps its anchor. A shadow whose original frame is absent is not loaded:
    /// its hot spot would be unknown.
    pub fn load(
        cache: &RemasterCache,
        resource: &str,
        source: SpriteSheet,
        source_shadow: Option<ShadowSheet>,
        scale: RenderScale,
    ) -> Self {
        let multiplier = scale.scale();
        let mut frames = Vec::with_capacity(source.len());
        let mut shadows = Vec::with_capacity(source.len());

        for index in 0..source.len() {
            let frame_index = u16::try_from(index).unwrap_or(u16::MAX);
            let frame = source.frame(index);
            let frame_tag = sprite_frame_tag(resource, frame_index, multiplier);

            let hd_frame = cache
                .resolve(&frame_tag)
                .and_then(|texture| HdIndexMap::from_texture(&texture))
                .map(|map| {
                    let hot = frame
                        .map(|frame| scaled_hot_spot(frame, map.width(), map.height()))
                        .unwrap_or((0, 0));

                    HdIndexFrame::new(map, hot.0, hot.1)
                });

            frames.push(hd_frame);

            let shadow_frame = source_shadow.as_ref().and_then(|sheet| sheet.frame(index));
            let shadow_tag = sprite_shadow_tag(resource, frame_index, multiplier);
            let hd_shadow = match shadow_frame {
                Some(shadow) => cache
                    .resolve(&shadow_tag)
                    .and_then(|texture| HdIndexMap::from_texture(&texture))
                    .map(|map| {
                        let hot = scaled_hot_spot(shadow, map.width(), map.height());

                        HdShadowFrame::new(map, hot.0, hot.1)
                    }),
                None => None,
            };

            shadows.push(hd_shadow);
        }

        Self::with_frames(source, source_shadow, scale, frames, shadows)
    }
    /// Assigns the stable sprite id used in [`HdSpriteKey`] when one
    /// [`HdSpriteCache`] serves several banks.
    pub fn with_id(mut self, id: u16) -> Self {
        self.id = id;
        self
    }

    /// Stable sprite id of the bank.
    pub fn id(&self) -> u16 {
        self.id
    }

    /// The original sheet (the fallback source).
    pub fn source(&self) -> &SpriteSheet {
        &self.source
    }

    /// The original shadow sheet, if the resource has one.
    pub fn source_shadow(&self) -> Option<&ShadowSheet> {
        self.source_shadow.as_ref()
    }

    /// Presentation scale the bank was loaded for.
    pub fn scale(&self) -> RenderScale {
        self.scale
    }

    /// Number of frames (from the original sheet).
    pub fn frame_count(&self) -> usize {
        self.source.len()
    }

    /// HD index frame, if one was loaded for `index`.
    pub fn hd_frame(&self, index: usize) -> Option<&HdIndexFrame> {
        self.frames.get(index)?.as_ref()
    }

    /// HD shadow mask, if one was loaded for `index`.
    pub fn hd_shadow(&self, index: usize) -> Option<&HdShadowFrame> {
        self.shadows.get(index)?.as_ref()
    }

    /// How many frames have an HD asset (`0..=frame_count()`).
    pub fn hd_frame_count(&self) -> usize {
        self.frames.iter().filter(|frame| frame.is_some()).count()
    }

    /// How many frames have an HD shadow (`0..=frame_count()`).
    pub fn hd_shadow_count(&self) -> usize {
        self.shadows
            .iter()
            .filter(|shadow| shadow.is_some())
            .count()
    }

    /// At least one frame has an HD asset.
    pub fn is_hd(&self) -> bool {
        self.hd_frame_count() > 0 || self.hd_shadow_count() > 0
    }
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

/// Draws HD unit sprites with the recolor semantics of [`SpriteRenderer`].
///
/// The renderer owns the palette snapshot with the palette cycles applied and
/// the resulting [`IndexPaletteLut`]; the [`SpriteRenderer`] used for fallback
/// is built from the same state, so an HD frame and an original frame of the
/// same unit differ only in resolution.
#[derive(Debug, Clone)]
pub struct HdSpriteRenderer<'a> {
    bank: &'a HdSpriteBank,
    brightness: &'a BrightnessLut,
    palette: Palette,
    team: ColorLut,
    state: HdRecolorState,
    lut: IndexPaletteLut,
}

impl<'a> HdSpriteRenderer<'a> {
    /// Renderer for `bank` in `state` (team, brightness, palette cycle frame).
    ///
    /// `palette` is the **static** game palette; the cycle snapshot is computed
    /// here with [`rotate_palette`], the brightness LUT (built once from the
    /// static palette) is applied on top, and the team LUT comes first — the
    /// `palette[brightness[team[index]]]` order of `Gfx_DecodeSprite`.
    pub fn new(
        bank: &'a HdSpriteBank,
        palette: &Palette,
        brightness: &'a BrightnessLut,
        state: HdRecolorState,
    ) -> Self {
        let snapshot = rotate_palette(palette, &state.clock());
        let team = state.team.lut();
        let lut = IndexPaletteLut::new(&snapshot, brightness, team, state.level());

        Self {
            bank,
            brightness,
            palette: snapshot,
            team,
            state,
            lut,
        }
    }

    /// The bank being drawn.
    pub fn bank(&self) -> &'a HdSpriteBank {
        self.bank
    }

    /// The recolor state (team, brightness, cycle frame).
    pub fn state(&self) -> HdRecolorState {
        self.state
    }

    /// The palette LUT of the state.
    pub fn lut(&self) -> &IndexPaletteLut {
        &self.lut
    }

    /// The palette snapshot with the palette cycles applied.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The original-path renderer used for frames without an HD asset.
    pub fn fallback(&self) -> SpriteRenderer<'_> {
        SpriteRenderer::new(
            &self.palette,
            self.brightness,
            self.team,
            self.state.level(),
        )
    }

    /// Cache key of frame `index` in the current state.
    pub fn key(&self, index: usize) -> HdSpriteKey {
        HdSpriteKey::new(
            self.bank.id(),
            u16::try_from(index).unwrap_or(u16::MAX),
            self.state,
        )
    }

    /// Recolored texture of frame `index`, or `None` when the bank has no HD
    /// asset for it (the caller then uses [`Self::fallback`]).
    pub fn recolor_frame<'c>(
        &self,
        cache: &'c mut HdSpriteCache,
        index: usize,
    ) -> Option<&'c HdSpriteTexture> {
        let frame = self.bank.hd_frame(index)?;
        let key = self.key(index);

        Some(cache.resolve(key, frame.map(), &self.lut))
    }

    /// Draws a body frame: the HD index frame when the bank has one, otherwise
    /// the original [`SpriteRenderer`] path.
    ///
    /// `anchor` is in render-frame pixels, `zoom` in the [`SpriteRenderer`]
    /// unit (render pixels per 64-texel tile).
    pub fn draw_frame_scaled(
        &self,
        dst: &mut RgbaBuffer,
        cache: &mut HdSpriteCache,
        index: usize,
        anchor: (i32, i32),
        zoom: u32,
    ) -> DrawStats {
        let scale = self.bank.scale().scale();

        if let Some(frame) = self.bank.hd_frame(index) {
            let key = self.key(index);
            let texture = cache.resolve(key, frame.map(), &self.lut);

            return draw_texture_scaled(dst, texture, frame.hot_spot(), anchor, zoom, scale);
        }

        match self.bank.source().frame(index) {
            Some(frame) => self.fallback().draw_frame_scaled(dst, frame, anchor, zoom),
            None => DrawStats::default(),
        }
    }

    /// Draws a body frame as a solid team-colour silhouette — the
    /// `zoom_level < 8` mode, see [`is_silhouette_zoom`].
    ///
    /// The HD path paints every covered texel in `color` without consulting the
    /// palette LUT at all; the fallback is
    /// [`SpriteRenderer::draw_frame_silhouette_scaled`].
    pub fn draw_frame_silhouette_scaled(
        &self,
        dst: &mut RgbaBuffer,
        index: usize,
        anchor: (i32, i32),
        zoom: u32,
        color: Rgba,
    ) -> DrawStats {
        let scale = self.bank.scale().scale();

        if let Some(frame) = self.bank.hd_frame(index) {
            return draw_mask_scaled(
                dst,
                frame.map(),
                frame.hot_spot(),
                anchor,
                zoom,
                scale,
                |_, _| color,
            );
        }

        match self.bank.source().frame(index) {
            Some(frame) => self
                .fallback()
                .draw_frame_silhouette_scaled(dst, frame, anchor, zoom, color),
            None => DrawStats::default(),
        }
    }

    /// Darkens the already drawn map by the shadow of frame `index`.
    ///
    /// The HD mask level scales the level-5 darkening
    /// (`Rgba::darken(SHADOW_BRIGHTNESS_LEVEL)`), so `255` is exactly the
    /// original `draw_shadow` and `0` leaves the pixel untouched; the fallback
    /// is [`SpriteRenderer::draw_shadow_scaled`].
    pub fn draw_shadow_scaled(
        &self,
        dst: &mut RgbaBuffer,
        index: usize,
        anchor: (i32, i32),
        zoom: u32,
    ) -> DrawStats {
        let scale = self.bank.scale().scale();

        if let Some(shadow) = self.bank.hd_shadow(index) {
            return draw_mask_scaled(
                dst,
                shadow.mask(),
                shadow.hot_spot(),
                anchor,
                zoom,
                scale,
                |level, color| color.mix(color.darken(SHADOW_BRIGHTNESS_LEVEL), level),
            );
        }

        match self
            .bank
            .source_shadow()
            .and_then(|sheet| sheet.frame(index))
        {
            Some(shadow) => self
                .fallback()
                .draw_shadow_scaled(dst, shadow, anchor, zoom),
            None => DrawStats::default(),
        }
    }

    /// Draws a unit in the original's order: shadow (shifted by
    /// `shadow_offset`), then the body — `UnitInfo::RenderWithConnectors`.
    ///
    /// `shadow_offset` is in **source sprite pixels**, exactly like
    /// [`SpriteRenderer::draw_unit_scaled`]: it is scaled by `zoom / 64`
    /// ([`scaled_offset`]) so the HD and the fallback path place the shadow
    /// identically.
    pub fn draw_unit_scaled(
        &self,
        dst: &mut RgbaBuffer,
        cache: &mut HdSpriteCache,
        index: usize,
        anchor: (i32, i32),
        shadow_offset: (i32, i32),
        zoom: u32,
    ) -> DrawStats {
        let offset = scaled_offset(shadow_offset, zoom);
        let shadow_anchor = (anchor.0 + offset.0, anchor.1 + offset.1);
        let shadow = self.draw_shadow_scaled(dst, index, shadow_anchor, zoom);

        shadow.merge(self.draw_frame_scaled(dst, cache, index, anchor, zoom))
    }
}

// ---------------------------------------------------------------------------
// Geometry and blitting
// ---------------------------------------------------------------------------

/// Render pixels covered by `length` HD texels at `zoom` in a frame of scale
/// `scale`: `length · zoom / (64 · S)`.
fn hd_length(length: u32, zoom: u32, scale: u32) -> i32 {
    let divisor = TILE_PIXELS * i64::from(scale.max(1));

    (((i64::from(length) * i64::from(zoom.max(1))) / divisor) as i32).max(1)
}

/// Render pixels of an HD offset (hot spot): `value · zoom / (64 · S)`.
fn hd_pixels(value: i32, zoom: u32, scale: u32) -> i32 {
    let divisor = TILE_PIXELS * i64::from(scale.max(1));

    ((i64::from(value) * i64::from(zoom.max(1))) / divisor) as i32
}

/// HD source texel for a destination pixel (nearest neighbour):
/// `destination · 64 · S / zoom`.
fn hd_source_texel(destination: i32, zoom: u32, scale: u32, source_len: u32) -> i32 {
    let numerator = i64::from(destination) * TILE_PIXELS * i64::from(scale.max(1));
    let source = numerator / i64::from(zoom.max(1));
    let last = i64::from(source_len).saturating_sub(1);

    source.clamp(0, last) as i32
}

/// Blits a recolored texture at `zoom`, skipping fully transparent texels.
fn draw_texture_scaled(
    dst: &mut RgbaBuffer,
    texture: &HdSpriteTexture,
    hot: (i16, i16),
    anchor: (i32, i32),
    zoom: u32,
    scale: u32,
) -> DrawStats {
    let zoom = zoom.max(1);
    let width = hd_length(texture.width(), zoom, scale);
    let height = hd_length(texture.height(), zoom, scale);
    let ulx = anchor.0 - hd_pixels(i32::from(hot.0), zoom, scale);
    let uly = anchor.1 - hd_pixels(i32::from(hot.1), zoom, scale);
    let mut stats = DrawStats::default();

    for y in 0..height {
        let source_y = hd_source_texel(y, zoom, scale, texture.height());

        for x in 0..width {
            let source_x = hd_source_texel(x, zoom, scale, texture.width());
            let Some(color) = texture.texel(source_x as u32, source_y as u32) else {
                continue;
            };

            if color.is_transparent() {
                continue;
            }

            if dst.set(ulx + x, uly + y, color) {
                stats.drawn += 1;
            } else {
                stats.clipped += 1;
            }
        }
    }

    stats
}

/// Samples a mask map and applies `effect(level, current)` to every texel with
/// a non-zero level: the shadow darkening and the silhouette use the same
/// sampling, only the effect differs.
fn draw_mask_scaled(
    dst: &mut RgbaBuffer,
    mask: &HdIndexMap,
    hot: (i16, i16),
    anchor: (i32, i32),
    zoom: u32,
    scale: u32,
    effect: impl Fn(u8, Rgba) -> Rgba,
) -> DrawStats {
    let zoom = zoom.max(1);
    let width = hd_length(mask.width(), zoom, scale);
    let height = hd_length(mask.height(), zoom, scale);
    let ulx = anchor.0 - hd_pixels(i32::from(hot.0), zoom, scale);
    let uly = anchor.1 - hd_pixels(i32::from(hot.1), zoom, scale);
    let mut stats = DrawStats::default();

    for y in 0..height {
        let source_y = hd_source_texel(y, zoom, scale, mask.height());

        for x in 0..width {
            let source_x = hd_source_texel(x, zoom, scale, mask.width());
            let Some(level) = mask.payload(source_x as u32, source_y as u32) else {
                continue;
            };

            if level == 0 {
                continue;
            }

            if dst.modify(ulx + x, uly + y, |color| effect(level, color)) {
                stats.drawn += 1;
            } else {
                stats.clipped += 1;
            }
        }
    }

    stats
}

/// Hot spot of an HD asset: the original hot spot scaled by the resolution
/// ratio of the asset (exactly `×S` for a correct asset), saturated to `i16`.
fn scaled_hot_spot(source: &impl HotSpot, width: u32, height: u32) -> (i16, i16) {
    (
        scaled_hot(source.hotx(), source.width(), width),
        scaled_hot(source.hoty(), source.height(), height),
    )
}

fn scaled_hot(hot: i16, source_len: u16, target_len: u32) -> i16 {
    if source_len == 0 {
        return hot;
    }

    let scaled = i64::from(hot) * i64::from(target_len) / i64::from(source_len);

    scaled.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

/// Hot spot and frame size of a source sheet frame (the loader scales the hot
/// spot of both body and shadow frames the same way).
trait HotSpot {
    fn hotx(&self) -> i16;
    fn hoty(&self) -> i16;
    fn width(&self) -> u16;
    fn height(&self) -> u16;
}

impl HotSpot for SpriteFrame {
    fn hotx(&self) -> i16 {
        self.hotx
    }

    fn hoty(&self) -> i16 {
        self.hoty
    }

    fn width(&self) -> u16 {
        self.width
    }

    fn height(&self) -> u16 {
        self.height
    }
}

impl HotSpot for max_assets::sprite::ShadowFrame {
    fn hotx(&self) -> i16 {
        self.hotx
    }

    fn hoty(&self) -> i16 {
        self.hoty
    }

    fn width(&self) -> u16 {
        self.width
    }

    fn height(&self) -> u16 {
        self.height
    }
}

#[cfg(test)]
#[path = "hd_sprites_tests.rs"]
mod tests;
