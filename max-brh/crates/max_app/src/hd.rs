//! HD presentation pipeline (`max-brh-docs/development/remaster-hd.md` §R7).
//!
//! R3–R6 delivered the presentation adapters of the remaster in `max_render`
//! ([`HdTerrainAtlas`], [`HdSpriteBank`], [`HdChrome`], [`HdFonts`]). This
//! module is the application-side **switch and owner**: one [`HdPresenter`]
//! holds every layer of one [`RenderScale`], resolved once through the R1
//! loader ([`max_assets::remaster::RemasterCache`]), and hands the render
//! pipeline the pieces it needs.
//!
//! # Invariants
//!
//! * **Presentation only.** Nothing here is imported by `max_core`; the layers
//!   carry pixels, never simulation state. [`crate::App::render_frame`] — the
//!   1:1 frame every golden is taken from — never touches this module.
//! * **`S = 1` never loads the remaster.** [`HdPresenter::load`] with
//!   [`RenderScale::PixelPerfect`], and every layer constructor, resolve
//!   nothing: the byte-for-byte path stays byte-for-byte.
//! * **Per-asset fallback.** A tag the manifest does not list, a missing file,
//!   an undecodable `.ktx2` leaves that asset on the original `MAX.RES` path;
//!   [`Self::stats`] reports how many assets were actually substituted, so a
//!   partial remaster is visible in the log instead of silently ignored.
//!
//! # VRAM accounting
//!
//! [`HdStats`] sums the resident bytes of every decoded texture — HD tiles, HD
//! sprite index maps, HD chrome, font atlases and the bounded recolor cache.
//! The R0 quota (`r0-measurements.md` §2.1: ≈105 MiB at `S = 2`, ≈420 MiB at
//! `S = 4` for a match working set) is checked against it by the R7 gates.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use max_assets::remaster::RemasterCache;
use max_core::enums::UnitType;
use max_render::buffer::RgbaBuffer;
use max_render::hd_chrome::{ChromeSource, HdChrome};
use max_render::hd_sprites::{HdRecolorState, HdSpriteBank, HdSpriteCache, HdSpriteRenderer};
use max_render::hd_terrain::HdTerrainAtlas;
use max_render::hd_text::HdFonts;
use max_render::palette::{BrightnessLut, Palette};
use max_render::render_scale::RenderScale;

use crate::assets::Assets;

/// Directory name of the remaster root inside `target/` (R1/R2 convention).
pub const REMASTER_DIR: &str = "remaster";

/// VRAM quota of the match working set at `S = 2` (105 MiB).
///
/// R0 measured the working set of a match — one map's tiles, every unit sprite
/// and shadow, the window chrome, the panels and the fonts — at ≈105 MiB for
/// `×2` (`r0-measurements.md` §2.1). The §R7 gate keeps the decoded HD layers
/// below that number.
pub const VRAM_QUOTA_X2_BYTES: usize = 105 * 1024 * 1024;

/// VRAM quota of the match working set at `S = 4` (420 MiB, R0 §2.1).
pub const VRAM_QUOTA_X4_BYTES: usize = 420 * 1024 * 1024;

/// The VRAM quota of `scale`; `0` for [`RenderScale::PixelPerfect`] — the
/// pixel-perfect mode keeps no HD layer resident at all.
pub const fn vram_quota(scale: RenderScale) -> usize {
    match scale {
        RenderScale::PixelPerfect => 0,
        RenderScale::Hd2 => VRAM_QUOTA_X2_BYTES,
        RenderScale::Hd4 => VRAM_QUOTA_X4_BYTES,
    }
}

/// Default remaster root: `<workspace>/target/remaster` (R2 artifact location).
///
/// `repository_root` is the application's repository lookup (the same heuristic
/// `main.rs` uses for the game data); when it is absent the path stays relative
/// to the working directory, which never blocks the launch — a missing
/// manifest simply means "no HD assets" and every layer falls back.
pub fn default_remaster_root(repository_root: Option<&Path>) -> PathBuf {
    match repository_root {
        Some(root) => root.join("max-brh").join("target").join(REMASTER_DIR),
        None => PathBuf::from("target").join(REMASTER_DIR),
    }
}

/// An HD sprite bank of one `MAX.RES` sprite resource.
struct UnitBank {
    bank: HdSpriteBank,
}

/// Every HD layer of one presentation scale.
///
/// Constructed once per launch (HD mode only) and passed to
/// [`crate::App::render_frame_hd`]. The presenter is the only owner of the
/// decoded remaster textures; the render pipeline borrows it.
pub struct HdPresenter {
    /// Presentation scale of the layers (`S ∈ {2, 4}`; `PixelPerfect` = empty).
    scale: RenderScale,
    /// Remaster root the layers were resolved from (diagnostics).
    root: PathBuf,
    /// HD terrain atlas of the loaded map, if the map has any HD tile.
    terrain: Option<HdTerrainAtlas>,
    /// HD window chrome (`FRAMEPIC`/`PANELTOP`/`PANELBTM`).
    chrome: HdChrome,
    /// The original palette chrome — the per-asset fallback source.
    chrome_source: Option<ChromeSource>,
    /// HD raster GNW font atlases.
    fonts: HdFonts,
    /// HD sprite banks, one per resolved `MAX.RES` sprite resource.
    banks: Vec<UnitBank>,
    /// `UnitType` → index in `banks`.
    bank_index: HashMap<UnitType, usize>,
    /// Recolored sprite textures, bounded and shared by every bank.
    cache: HdSpriteCache,
}

impl std::fmt::Debug for HdPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HdPresenter")
            .field("scale", &self.scale)
            .field("root", &self.root)
            .field("banks", &self.banks.len())
            .field("stats", &self.stats())
            .finish()
    }
}

impl HdPresenter {
    /// An empty presenter: every drawer falls back to the original path.
    pub fn empty(scale: RenderScale) -> Self {
        Self {
            scale,
            root: PathBuf::new(),
            terrain: None,
            chrome: HdChrome::empty(scale),
            chrome_source: None,
            fonts: HdFonts::empty(scale),
            banks: Vec::new(),
            bank_index: HashMap::new(),
            cache: HdSpriteCache::new(),
        }
    }

    /// Resolves every HD layer of `scale` from `root` for the loaded assets.
    ///
    /// In [`RenderScale::PixelPerfect`] nothing is read: the 1:1 path must not
    /// depend on the remaster being present.
    ///
    /// The terrain tags use the map resource name (`GREEN_1`, …), the sprite
    /// tags the `MAX.RES` resource of each loaded sprite (`TANK`, …), the chrome
    /// tags the interface image names (`FRAMEPIC`, …) and the font tags the GNW
    /// file stems (`FONT1`, …) — the conventions of
    /// [`max_assets::remaster`].
    pub fn load(assets: &Assets, scale: RenderScale, root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();

        if !scale.is_hd() {
            let mut presenter = Self::empty(scale);
            presenter.root = root;

            return presenter;
        }

        let cache = RemasterCache::open(&root);
        let terrain =
            HdTerrainAtlas::load(&cache, &assets.world_name, assets.terrain.clone(), scale);
        let chrome = HdChrome::load(&cache, scale);
        let fonts = HdFonts::load(&cache, scale);
        let mut banks = Vec::new();
        let mut bank_index = HashMap::new();

        for sprite in assets.sprites() {
            let bank = HdSpriteBank::load(
                &cache,
                &sprite.sprite_name,
                sprite.sheet.clone(),
                sprite.shadow.clone(),
                scale,
            )
            .with_id(banks.len() as u16);

            if bank.is_hd() {
                bank_index.insert(sprite.unit_type, banks.len());
                banks.push(UnitBank { bank });
            }
        }

        Self {
            scale,
            root,
            terrain: Some(terrain),
            chrome,
            chrome_source: assets.chrome.as_ref().map(chrome_source),
            fonts,
            banks,
            bank_index,
            cache: HdSpriteCache::new(),
        }
    }

    /// A presenter from already decoded layers (tests, or a caller that
    /// resolves the remaster itself). Banks are added with [`Self::with_bank`].
    pub fn with_layers(
        scale: RenderScale,
        terrain: Option<HdTerrainAtlas>,
        chrome: HdChrome,
        fonts: HdFonts,
    ) -> Self {
        Self {
            scale,
            root: PathBuf::new(),
            terrain,
            chrome,
            chrome_source: None,
            fonts,
            banks: Vec::new(),
            bank_index: HashMap::new(),
            cache: HdSpriteCache::new(),
        }
    }

    /// Adds an HD sprite bank to a presenter built with [`Self::with_layers`].
    pub fn with_bank(mut self, unit_type: UnitType, bank: HdSpriteBank) -> Self {
        let bank = bank.with_id(self.banks.len() as u16);
        self.bank_index.insert(unit_type, self.banks.len());
        self.banks.push(UnitBank { bank });

        self
    }

    /// Adds the original palette chrome as the per-asset fallback source.
    pub fn with_chrome_source(mut self, source: ChromeSource) -> Self {
        self.chrome_source = Some(source);

        self
    }

    /// Presentation scale of the layers.
    pub fn scale(&self) -> RenderScale {
        self.scale
    }

    /// `true` in the HD modes (`S > 1`).
    pub fn is_hd(&self) -> bool {
        self.scale.is_hd()
    }

    /// Remaster root the layers were resolved from.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// HD terrain atlas, if one was built.
    pub fn terrain(&self) -> Option<&HdTerrainAtlas> {
        self.terrain.as_ref()
    }

    /// HD window chrome.
    pub fn chrome(&self) -> &HdChrome {
        &self.chrome
    }

    /// The original palette chrome used as the fallback source.
    pub fn chrome_source(&self) -> Option<&ChromeSource> {
        self.chrome_source.as_ref()
    }

    /// HD font atlases.
    pub fn fonts(&self) -> &HdFonts {
        &self.fonts
    }

    /// Whether `unit_type` has an HD sprite bank (at least one HD frame).
    pub fn has_sprite(&self, unit_type: UnitType) -> bool {
        self.bank_index.contains_key(&unit_type)
    }

    /// `true` when the map can be drawn from HD layers at all (tiles or
    /// sprites). Without it the HD frame keeps the `×S` upscale of the 1:1 map.
    pub fn has_map_content(&self) -> bool {
        !self.banks.is_empty()
            || self
                .terrain
                .as_ref()
                .is_some_and(|terrain| terrain.hd_tile_count() > 0)
    }

    /// `true` when at least one layer substituted an original asset.
    pub fn has_any(&self) -> bool {
        self.has_map_content() || self.chrome.hd_count() > 0 || self.fonts.hd_count() > 0
    }

    /// The recolor cache (diagnostics and the VRAM accounting).
    pub fn sprite_cache(&self) -> &HdSpriteCache {
        &self.cache
    }

    /// Draws the window background: the HD `FRAMEPIC` when resolved, otherwise
    /// the original palette `FRAMEPIC` nearest-scaled to the render frame.
    ///
    /// Returns `false` when neither is available (the caller keeps the frame it
    /// has). The fallback is the per-asset rule of the remaster: a partial
    /// remaster still renders.
    pub fn draw_chrome_frame(&self, frame: &mut RgbaBuffer) -> bool {
        if !self.is_hd() {
            return false;
        }

        match self.chrome_source.as_ref() {
            Some(source) => self.chrome.draw_frame_with_fallback(frame, source) > 0,
            None => self.chrome.draw_frame(frame).is_some(),
        }
    }

    /// The HD bank of `unit_type` and the shared recolor cache, ready to draw.
    ///
    /// The two borrows are disjoint fields of the presenter so the caller can
    /// hold the renderer (which borrows the bank) while passing the cache to
    /// every draw call — [`HdSpriteRenderer`] needs both. `None` when the type
    /// has no HD asset: the caller then draws the original sprite.
    pub fn sprite_render_context<'a>(
        &'a mut self,
        unit_type: UnitType,
        palette: &'a Palette,
        brightness: &'a BrightnessLut,
        state: HdRecolorState,
    ) -> Option<(HdSpriteRenderer<'a>, &'a mut HdSpriteCache)> {
        let index = *self.bank_index.get(&unit_type)?;
        let bank: &'a HdSpriteBank = &self.banks.get(index)?.bank;
        let renderer = HdSpriteRenderer::new(bank, palette, brightness, state);
        let cache: &'a mut HdSpriteCache = &mut self.cache;

        Some((renderer, cache))
    }

    /// Resident bytes and asset counts of every HD layer.
    pub fn stats(&self) -> HdStats {
        let tiles_bytes = self.terrain.as_ref().map(terrain_bytes).unwrap_or(0);
        let hd_tiles = self
            .terrain
            .as_ref()
            .map(HdTerrainAtlas::hd_tile_count)
            .unwrap_or(0);
        let hd_sprites: usize = self
            .banks
            .iter()
            .map(|bank| bank.bank.hd_frame_count())
            .sum();
        let hd_shadows: usize = self
            .banks
            .iter()
            .map(|bank| bank.bank.hd_shadow_count())
            .sum();
        let sprites_bytes: usize = self.banks.iter().map(|bank| bank_bytes(&bank.bank)).sum();

        HdStats {
            hd_tiles,
            hd_sprites,
            hd_shadows,
            hd_chrome: self.chrome.hd_count(),
            hd_fonts: self.fonts.hd_count(),
            tiles_bytes,
            sprites_bytes,
            chrome_bytes: self.chrome.bytes(),
            fonts_bytes: fonts_bytes(&self.fonts),
            cache_bytes: self.cache.bytes(),
        }
    }

    /// Resident bytes of every decoded HD texture (the VRAM gate of §R7).
    pub fn vram_bytes(&self) -> usize {
        self.stats().total_bytes()
    }

    /// Number of HD sprite banks (`0..=sprite count`).
    pub fn bank_count(&self) -> usize {
        self.banks.len()
    }
}

/// The original palette chrome of `MAX.RES` as the HD fallback source.
fn chrome_source(chrome: &crate::chrome::Chrome) -> ChromeSource {
    ChromeSource::with_panels(
        chrome.frame.clone(),
        chrome.panel_top.clone(),
        chrome.panel_bottom.clone(),
    )
}

/// Resident bytes of the HD terrain atlas (RGBA8 tiles).
fn terrain_bytes(atlas: &HdTerrainAtlas) -> usize {
    (0..atlas.tile_count())
        .filter_map(|tile_id| atlas.hd_tile(tile_id))
        .map(|tile| tile.pixels().len())
        .sum()
}

/// Resident bytes of the HD index maps of one sprite bank.
///
/// An index map keeps two bytes per texel — the palette index and the coverage
/// — not four: the HD sprite asset is a map, not a picture
/// (`max_render::hd_sprites`).
fn bank_bytes(bank: &HdSpriteBank) -> usize {
    let frames: usize = (0..bank.frame_count())
        .filter_map(|index| bank.hd_frame(index))
        .map(|frame| frame.map().len() * 2)
        .sum();
    let shadows: usize = (0..bank.frame_count())
        .filter_map(|index| bank.hd_shadow(index))
        .map(|shadow| shadow.mask().len() * 2)
        .sum();

    frames + shadows
}

/// Resident bytes of the font atlases (one coverage byte per texel).
fn fonts_bytes(fonts: &HdFonts) -> usize {
    max_render::text::GnwFontId::ALL
        .into_iter()
        .filter_map(|id| fonts.font(id).atlas())
        .map(|atlas| atlas.width() as usize * atlas.height() as usize)
        .sum()
}

/// Asset counts and resident bytes of the HD layers of one presentation scale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HdStats {
    /// HD terrain tiles substituted (`0..=tile count`).
    pub hd_tiles: usize,
    /// HD sprite frames substituted.
    pub hd_sprites: usize,
    /// HD shadow masks substituted.
    pub hd_shadows: usize,
    /// HD chrome images (`FRAMEPIC`/panels/named images).
    pub hd_chrome: usize,
    /// Fonts with an HD atlas (`0..=3`).
    pub hd_fonts: usize,
    /// Bytes of the decoded HD tiles.
    pub tiles_bytes: usize,
    /// Bytes of the HD sprite index maps.
    pub sprites_bytes: usize,
    /// Bytes of the decoded HD chrome.
    pub chrome_bytes: usize,
    /// Bytes of the font atlases.
    pub fonts_bytes: usize,
    /// Bytes of the recolored sprite cache.
    pub cache_bytes: usize,
}

impl HdStats {
    /// Resident bytes of every HD texture.
    pub fn total_bytes(&self) -> usize {
        self.tiles_bytes
            + self.sprites_bytes
            + self.chrome_bytes
            + self.fonts_bytes
            + self.cache_bytes
    }

    /// Resident bytes in MiB (`f64`, for logs and the quota check).
    pub fn total_mib(&self) -> f64 {
        self.total_bytes() as f64 / (1024.0 * 1024.0)
    }

    /// Total number of substituted assets.
    pub fn substituted(&self) -> usize {
        self.hd_tiles + self.hd_sprites + self.hd_shadows + self.hd_chrome + self.hd_fonts
    }

    /// The decoded layers fit the VRAM quota of `scale` (`r0-measurements.md` §2.1).
    pub fn fits_quota(&self, scale: RenderScale) -> bool {
        self.total_bytes() <= vram_quota(scale)
    }
}

#[cfg(test)]
#[path = "hd_tests.rs"]
mod tests;
