//! HD window chrome and UI box skins (`max-brh-docs/development/remaster-hd.md`
//! §R5).
//!
//! The 1:1 window chrome and the client's menus are drawn from `MAX.RES`
//! interface images: [`CHROME_FRAME`] (`FRAMEPIC`, a 640×480 `BigImage` with its
//! own palette) plus `PANELTOP`/`PANELBTM` and the button/menu images over the
//! window palette. Chrome has no team colours, no brightness levels and no
//! palette cycles, so — like an R3 terrain tile — an HD remaster is a plain
//! **pre-baked RGBA8 texture** per image, resolved through the R1 loader with
//! the [`chrome_image_tag`] convention.
//!
//! This module is the presentation-side substitute:
//!
//! * [`HdChrome`] holds the HD frame, the two panel halves and any named
//!   button/menu image. [`HdChrome::load`] resolves `chrome_FRAMEPIC_x<S>` /
//!   `chrome_PANELTOP_x<S>` / `chrome_PANELBTM_x<S>`; [`HdChrome::load_with`]
//!   adds further `MAX.RES` images by name (buttons, menus, dialogs).
//! * [`HdChrome::draw_frame`], [`HdChrome::draw_panel`] and
//!   [`HdChrome::draw_image`] sample the HD texture into the render frame with
//!   the R3 equal-block mapping (identical to
//!   [`HdTerrainAtlas::render_tile_into`](crate::hd_terrain::HdTerrainAtlas::render_tile_into)),
//!   so a texture authored at `×S` lands 1:1 on the `640·S × 480·S` frame.
//!   Each returns `None` when that asset has no HD texture: the caller then
//!   draws the original palette image, i.e. the fallback is **per asset** — a
//!   partial remaster still renders. The `*_with_fallback` drawers do that
//!   themselves from a [`ChromeSource`] (the real `MAX.RES` `BigImage`/
//!   `SimpleImage` over the `FRAMEPIC` window palette): at `S = 1` they are the
//!   byte-for-byte 1:1 picture, in the HD modes the nearest `×S` upscale of it.
//! * [`HdUiSkin`] is the same idea for the procedural boxes of
//!   [`crate::ui::render_ui`] (buttons, fields, lists, panels, selection),
//!   resolved with [`ui_skin_tag`]; [`crate::ui::render_ui_with_skin`] draws it.
//!
//! Two invariants keep `S = 1` and input handling untouched:
//!
//! 1. **Layout is always 640×480.** [`crate::layout`] constants are logical; the
//!    only place `S` enters chrome drawing is [`HdChrome::render_rect`] /
//!    [`scaled_rect`] (`×S` at output). Hit tests and clicks keep using the
//!    logical rectangles, as in `ScreenGeometry::window_to_frame` (which divides
//!    by `S`).
//! 2. **`PixelPerfect` never touches HD.** A `PixelPerfect` [`HdChrome`] or
//!    [`HdUiSkin`] loads nothing (and any injected texture is ignored), so the
//!    byte-for-byte 1:1 path and the golden frames cannot drift.
//!
//! This is presentation only. Nothing here is imported by `max_core`.

use std::collections::BTreeMap;

use max_assets::image::{BigImage, SimpleImage};
use max_assets::remaster::{chrome_image_tag, ui_skin_tag, RemasterCache, RemasterTexture};

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};
use crate::layout;
use crate::render_scale::RenderScale;
use crate::ui::{Rect, UiBox, UiBoxPainter};

/// `MAX.RES` name of the full-screen window background (`BigImage`).
pub const CHROME_FRAME: &str = "FRAMEPIC";
/// `MAX.RES` name of the top half of the left panel (`SimpleImage`).
pub const CHROME_PANEL_TOP: &str = "PANELTOP";
/// `MAX.RES` name of the bottom half of the left panel (`SimpleImage`).
pub const CHROME_PANEL_BOTTOM: &str = "PANELBTM";

/// Logical rectangle of [`CHROME_PANEL_TOP`]: the top of the left side panel.
pub const PANEL_TOP_RECT: Rect = Rect::new(
    0,
    0,
    layout::SIDE_PANEL_WIDTH,
    layout::SIDE_PANEL_TOP_HEIGHT,
);
/// Logical rectangle of [`CHROME_PANEL_BOTTOM`]: everything below the top half
/// down to the window bottom (239 + 241 = the 480 px window).
pub const PANEL_BOTTOM_RECT: Rect = Rect::new(
    0,
    layout::SIDE_PANEL_TOP_HEIGHT,
    layout::SIDE_PANEL_WIDTH,
    layout::WINDOW_HEIGHT - layout::SIDE_PANEL_TOP_HEIGHT,
);

/// A chrome image of [`HdChrome`]: RGBA8 texels, row-major.
///
/// Unlike the square [`HdTile`](crate::hd_terrain::HdTile), chrome images have
/// arbitrary dimensions (`FRAMEPIC` is 640×480, a button 67×18, …).
///
/// The remaster pre-bakes the window palette into the texels, so no
/// `ColorLut`/`BrightnessLut`/cycles are applied at draw time. Alpha is the
/// transparency of the original image (a `SimpleImage` transparent index is
/// baked as `A = 0`); [`stretch_into`] does not write fully transparent texels,
/// so an HD button composes over the HD frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl HdImage {
    /// Builds an image from RGBA8 bytes; `None` for an empty or truncated image
    /// (the caller then falls back to the original image).
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 {
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

    /// Builds an image from an R1 decoded texture; `None` for an empty one.
    pub fn from_texture(texture: &RemasterTexture) -> Option<Self> {
        Self::from_rgba(texture.width(), texture.height(), texture.pixels().to_vec())
    }

    /// Width in texels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in texels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// `true` when the image has no texels (never the case for a constructed one).
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// RGBA8 texels, row-major.
    pub fn pixels(&self) -> &[u8] {
        &self.rgba
    }

    /// Texel color; `None` outside the image.
    pub fn texel(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let offset = (y as usize * self.width as usize + x as usize) * 4;
        let pixel = &self.rgba[offset..offset + 4];

        Some(Rgba::from_rgba([pixel[0], pixel[1], pixel[2], pixel[3]]))
    }
}

/// Render-pixel rectangle of a logical 640×480 rectangle at `scale` (`×S`).
///
/// The logical side of the remaster: layout constants stay 640×480 (and
/// hit tests keep using them), and this is the only translation into render
/// pixels. In [`RenderScale::PixelPerfect`] it is the identity.
pub fn scaled_rect(rect: Rect, scale: RenderScale) -> PixelRect {
    scaled_rect_by(rect, scale.scale())
}

/// [`scaled_rect`] by a raw render factor.
///
/// The SSAA pass renders the frame at `S·aa`
/// (`max-brh-docs/development/ssaa-design.md`), so the logical rectangle is
/// multiplied by the render factor, not by `S` alone: the HD assets of the
/// `×S` layers are then placed by their equal-block mapping, which expands every
/// texel to an `aa × aa` block (the nearest `×aa` of the HD picture).
pub fn scaled_rect_by(rect: Rect, factor: u32) -> PixelRect {
    let factor = factor.max(1) as i32;

    PixelRect::new(
        rect.x * factor,
        rect.y * factor,
        rect.width * factor,
        rect.height * factor,
    )
}

/// The same translation for a 640×480-space [`PixelRect`] (e.g. a `layout`
/// constant such as `MAIN_MAP`); identity in [`RenderScale::PixelPerfect`].
pub fn scaled_pixel_rect(rect: PixelRect, scale: RenderScale) -> PixelRect {
    scaled_pixel_rect_by(rect, scale.scale())
}

/// [`scaled_pixel_rect`] by a raw render factor (the SSAA pass, see
/// [`scaled_rect_by`]).
pub fn scaled_pixel_rect_by(rect: PixelRect, factor: u32) -> PixelRect {
    let factor = factor.max(1) as i32;

    PixelRect::new(
        rect.ulx * factor,
        rect.uly * factor,
        rect.width * factor,
        rect.height * factor,
    )
}

/// Stretches an HD image into `rect` with the R3 equal-block mapping.
///
/// The coordinate mapping is exactly that of
/// [`HdTerrainAtlas::render_tile_into`](crate::hd_terrain::HdTerrainAtlas::render_tile_into):
/// every destination pixel takes the start of an equal source block
/// (`local · source / rect`). At a matching resolution (a texture authored `×S`
/// into a `×S` destination) this is the identity.
///
/// Fully transparent texels (`A = 0`) are **not** written: the pre-baked chrome
/// keeps the transparent cut-outs of the original interface images, so a button
/// or a panel composes over what was drawn below it. Returns the number of
/// destination pixels actually written (clipped by the buffer bounds).
pub fn stretch_into(dst: &mut RgbaBuffer, image: &HdImage, rect: PixelRect) -> u32 {
    if rect.is_empty() || image.is_empty() {
        return 0;
    }

    let source_width = image.width() as usize;
    let source_height = image.height() as usize;
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

            let Some(pixel) = image.pixels().get(offset..offset + 4) else {
                continue;
            };

            if pixel[3] == 0 {
                continue;
            }

            if dst.set(
                rect.ulx + local_x as i32,
                rect.uly + local_y as i32,
                Rgba::from_rgba([pixel[0], pixel[1], pixel[2], pixel[3]]),
            ) {
                painted += 1;
            }
        }
    }

    painted
}

/// Stretches a sampled source into `rect` with the same equal-block mapping.
///
/// The generic form of [`stretch_into`] over a sampler: [`ChromeSource`] uses it
/// to draw the original palette images (`BigImage`/`SimpleImage`) when an asset
/// has no HD texture. `None` from `sample` leaves the destination pixel
/// untouched (a transparent `SimpleImage` index).
fn stretch_sampled(
    dst: &mut RgbaBuffer,
    rect: PixelRect,
    source_width: u32,
    source_height: u32,
    sample: impl Fn(u32, u32) -> Option<Rgba>,
) -> u32 {
    if rect.is_empty() || source_width == 0 || source_height == 0 {
        return 0;
    }

    let source_width_usize = source_width as usize;
    let source_height_usize = source_height as usize;
    let rows: Vec<usize> = (0..rect.height as usize)
        .map(|local_y| local_y * source_height_usize / rect.height as usize)
        .collect();
    let columns: Vec<usize> = (0..rect.width as usize)
        .map(|local_x| local_x * source_width_usize / rect.width as usize)
        .collect();
    let mut painted = 0;

    for (local_y, source_y) in rows.iter().enumerate() {
        for (local_x, source_x) in columns.iter().enumerate() {
            let Some(color) = sample(*source_x as u32, *source_y as u32) else {
                continue;
            };

            if dst.set(rect.ulx + local_x as i32, rect.uly + local_y as i32, color) {
                painted += 1;
            }
        }
    }

    painted
}

/// One half of the left side panel (§[`CHROME_PANEL_TOP`]/[`CHROME_PANEL_BOTTOM`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChromePanel {
    /// Upper half (`PANELTOP`, 171×239).
    Top,
    /// Lower half (`PANELBTM`, 171×241).
    Bottom,
}

impl ChromePanel {
    /// Both halves, top to bottom.
    pub const ALL: [Self; 2] = [Self::Top, Self::Bottom];

    /// `MAX.RES` resource name of the half.
    pub const fn resource(self) -> &'static str {
        match self {
            Self::Top => CHROME_PANEL_TOP,
            Self::Bottom => CHROME_PANEL_BOTTOM,
        }
    }

    /// Logical rectangle of the half inside the 640×480 window.
    pub const fn logical_rect(self) -> Rect {
        match self {
            Self::Top => PANEL_TOP_RECT,
            Self::Bottom => PANEL_BOTTOM_RECT,
        }
    }
}

/// The original palette chrome of `MAX.RES` — the fallback source of the HD
/// layer (`remaster-hd.md` §R5).
///
/// [`CHROME_FRAME`] is both the 640×480 background and the **window palette**
/// every `SimpleImage` panel/button is drawn with (the original does
/// `WindowManager_LoadBigImage(FRAMEPIC, …, palette_from_image = true)`, so
/// `FRAMEPIC`'s own palette becomes the system one). The fallback blit
/// reproduces the 1:1 renderer of `max_app::chrome`/`max_app::panels`:
/// `Rgba::rgb` from the palette color, and the `SimpleImage` transparent index
/// is skipped.
///
/// The panels are placed by the layout halves ([`PANEL_TOP_RECT`] and
/// [`PANEL_BOTTOM_RECT`]) rather than by their header origin: both `PANELTOP`
/// and `PANELBTM` headers say `(0, 0)`, and the two files stack as 239 + 241 px.
#[derive(Debug, Clone)]
pub struct ChromeSource {
    frame: BigImage,
    panel_top: Option<SimpleImage>,
    panel_bottom: Option<SimpleImage>,
    /// Further original `MAX.RES` images by resource name (buttons, menus).
    images: BTreeMap<String, SimpleImage>,
}

impl ChromeSource {
    /// A source with only the window frame (background + window palette).
    pub fn new(frame: BigImage) -> Self {
        Self {
            frame,
            panel_top: None,
            panel_bottom: None,
            images: BTreeMap::new(),
        }
    }

    /// A source with the frame and the two panel halves.
    pub fn with_panels(
        frame: BigImage,
        panel_top: Option<SimpleImage>,
        panel_bottom: Option<SimpleImage>,
    ) -> Self {
        Self {
            frame,
            panel_top,
            panel_bottom,
            images: BTreeMap::new(),
        }
    }

    /// Adds an original `SimpleImage` by `MAX.RES` name (a button, a menu
    /// background, …), drawn with the window palette.
    pub fn with_image(mut self, name: impl Into<String>, image: SimpleImage) -> Self {
        self.images.insert(name.into(), image);

        self
    }

    /// The original window background (and the window palette source).
    pub fn frame(&self) -> &BigImage {
        &self.frame
    }

    /// The original half of the left panel, if it was loaded.
    pub fn panel(&self, part: ChromePanel) -> Option<&SimpleImage> {
        match part {
            ChromePanel::Top => self.panel_top.as_ref(),
            ChromePanel::Bottom => self.panel_bottom.as_ref(),
        }
    }

    /// The original named interface image, if it was added.
    pub fn image(&self, name: &str) -> Option<&SimpleImage> {
        self.images.get(name)
    }

    /// Window palette color of an index (`FRAMEPIC` palette, 8-bit channels).
    pub fn window_color(&self, index: u8) -> Rgba {
        Rgba::from_rgb(self.frame.color(index))
    }

    /// Color of an original `SimpleImage` texel; `None` outside or transparent.
    fn image_color(&self, image: &SimpleImage, x: u32, y: u32) -> Option<Rgba> {
        let index = image.pixel(x, y)?;

        (index != image.transparent).then(|| Rgba::from_rgb(self.frame.color(index)))
    }
}

/// HD chrome assets of one presentation scale, with per-asset fallback.
///
/// A drawer without the `_with_fallback` suffix returns `None` for an asset
/// without an HD texture and leaves the original `MAX.RES` path to the caller.
/// The `_with_fallback` drawers ([`Self::draw_frame_with_fallback`], …) draw the
/// original palette image from a [`ChromeSource`] themselves — stretched by `S`
/// with the same mapping — so a partial remaster renders at every scale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdChrome {
    scale: RenderScale,
    frame: Option<HdImage>,
    panel_top: Option<HdImage>,
    panel_bottom: Option<HdImage>,
    /// Additional `MAX.RES` images by resource name (buttons, menus, dialogs).
    images: BTreeMap<String, HdImage>,
}

impl HdChrome {
    /// A chrome bank with no HD assets: every drawer falls back.
    pub fn empty(scale: RenderScale) -> Self {
        Self {
            scale,
            frame: None,
            panel_top: None,
            panel_bottom: None,
            images: BTreeMap::new(),
        }
    }

    /// A chrome bank from already decoded images (tests, or callers that resolve
    /// assets themselves).
    pub fn with_images(
        scale: RenderScale,
        frame: Option<HdImage>,
        panel_top: Option<HdImage>,
        panel_bottom: Option<HdImage>,
        images: BTreeMap<String, HdImage>,
    ) -> Self {
        Self {
            scale,
            frame,
            panel_top,
            panel_bottom,
            images,
        }
    }

    /// Loads `FRAMEPIC`/`PANELTOP`/`PANELBTM` from the remaster root.
    ///
    /// For each resource the R1 cache is asked for
    /// [`chrome_image_tag(resource, scale.scale())`](chrome_image_tag). A tag the
    /// manifest does not list, a missing `.ktx2`, a transcode failure or an
    /// empty texture leaves that asset on the original `MAX.RES` image — the
    /// fallback is per asset and never aborts the load.
    ///
    /// In [`RenderScale::PixelPerfect`] nothing is loaded: `S = 1` is the
    /// byte-for-byte path and never enters the HD layer.
    pub fn load(cache: &RemasterCache, scale: RenderScale) -> Self {
        Self::load_with(cache, scale, &[])
    }

    /// The same, plus the named `MAX.RES` interface images (`names`, e.g.
    /// `["ENDTRN_U", "MNUBTN"]`) resolved with the same tag convention.
    ///
    /// Names are stored verbatim and can be drawn by [`Self::draw_image`];
    /// names that do not resolve are simply absent (per-asset fallback).
    pub fn load_with(cache: &RemasterCache, scale: RenderScale, names: &[&str]) -> Self {
        if !scale.is_hd() {
            return Self::empty(scale);
        }

        let resolve = |resource: &str| {
            cache
                .resolve(&chrome_image_tag(resource, scale.scale()))
                .as_ref()
                .and_then(HdImage::from_texture)
        };
        let images = names
            .iter()
            .filter_map(|name| resolve(name).map(|image| ((*name).to_string(), image)))
            .collect();

        Self {
            scale,
            frame: resolve(CHROME_FRAME),
            panel_top: resolve(CHROME_PANEL_TOP),
            panel_bottom: resolve(CHROME_PANEL_BOTTOM),
            images,
        }
    }

    /// Presentation scale this bank was loaded for.
    pub fn scale(&self) -> RenderScale {
        self.scale
    }

    /// `true` in the HD modes (`S > 1`), where the drawers may substitute.
    pub fn is_hd(&self) -> bool {
        self.scale.is_hd()
    }

    /// HD window background, if loaded.
    pub fn frame(&self) -> Option<&HdImage> {
        self.frame.as_ref()
    }

    /// HD top half of the left panel, if loaded.
    pub fn panel_top(&self) -> Option<&HdImage> {
        self.panel_top.as_ref()
    }

    /// HD bottom half of the left panel, if loaded.
    pub fn panel_bottom(&self) -> Option<&HdImage> {
        self.panel_bottom.as_ref()
    }

    /// HD version of the named chrome image, if loaded.
    pub fn image(&self, name: &str) -> Option<&HdImage> {
        self.images.get(name)
    }

    /// Number of loaded HD assets (`0..=3 + named images`).
    pub fn hd_count(&self) -> usize {
        usize::from(self.frame.is_some())
            + usize::from(self.panel_top.is_some())
            + usize::from(self.panel_bottom.is_some())
            + self.images.len()
    }

    /// Resident bytes of the decoded HD chrome (RGBA8, 4 B per texel).
    ///
    /// The remaster VRAM budget (`remaster-hd.md` §R7, `r0-measurements.md`
    /// §2.1) counts every decoded texture that stays in memory for the match;
    /// the chrome is one of those classes.
    pub fn bytes(&self) -> usize {
        self.frame
            .iter()
            .chain(self.panel_top.iter())
            .chain(self.panel_bottom.iter())
            .chain(self.images.values())
            .map(|image| image.pixels().len())
            .sum()
    }

    /// The whole render frame: `640·S × 480·S` pixels.
    pub fn frame_rect(&self) -> PixelRect {
        scaled_rect(
            Rect::new(0, 0, layout::WINDOW_WIDTH, layout::WINDOW_HEIGHT),
            self.scale,
        )
    }

    /// The whole render frame at a raw render factor (`×S·aa` in the SSAA pass).
    pub fn frame_rect_at(&self, factor: u32) -> PixelRect {
        scaled_rect_by(
            Rect::new(0, 0, layout::WINDOW_WIDTH, layout::WINDOW_HEIGHT),
            factor,
        )
    }

    /// Render-pixel rectangle of a logical 640×480 rectangle at this bank's
    /// scale. This is the only place `S` changes layout.
    pub fn render_rect(&self, rect: Rect) -> PixelRect {
        scaled_rect(rect, self.scale)
    }

    /// [`Self::render_rect`] at a raw render factor (the SSAA pass).
    pub fn render_rect_at(&self, rect: Rect, factor: u32) -> PixelRect {
        scaled_rect_by(rect, factor)
    }

    /// Draws the HD `FRAMEPIC` over the whole render frame.
    ///
    /// Returns `None` when there is no HD frame (or at `S = 1`): the caller then
    /// draws the original palette `BigImage`. `Some(painted)` on success.
    pub fn draw_frame(&self, dst: &mut RgbaBuffer) -> Option<u32> {
        self.draw_frame_at(dst, self.scale.scale())
    }

    /// [`Self::draw_frame`] at a raw render factor: the `×S` HD texture is
    /// stretched over the `×S·aa` frame, i.e. every texel becomes an `aa × aa`
    /// block (the nearest `×aa` of the HD picture, SSAA pass).
    pub fn draw_frame_at(&self, dst: &mut RgbaBuffer, factor: u32) -> Option<u32> {
        if !self.is_hd() {
            return None;
        }

        let image = self.frame.as_ref()?;

        Some(stretch_into(dst, image, self.frame_rect_at(factor)))
    }

    /// Draws one HD panel half into its logical rectangle (`×S`).
    ///
    /// `None` — no HD panel: the original `SimpleImage` over the window palette
    /// is the fallback.
    pub fn draw_panel(&self, dst: &mut RgbaBuffer, part: ChromePanel) -> Option<u32> {
        self.draw_panel_at(dst, part, self.scale.scale())
    }

    /// [`Self::draw_panel`] at a raw render factor (the SSAA pass).
    pub fn draw_panel_at(
        &self,
        dst: &mut RgbaBuffer,
        part: ChromePanel,
        factor: u32,
    ) -> Option<u32> {
        if !self.is_hd() {
            return None;
        }

        let image = match part {
            ChromePanel::Top => self.panel_top.as_ref()?,
            ChromePanel::Bottom => self.panel_bottom.as_ref()?,
        };

        Some(stretch_into(
            dst,
            image,
            self.render_rect_at(part.logical_rect(), factor),
        ))
    }

    /// Draws the named HD chrome image (a button, a menu background, …)
    /// stretched into the logical `rect` (`×S`).
    ///
    /// `None` — no HD image for that name: the original palette image is the
    /// fallback.
    pub fn draw_image(&self, dst: &mut RgbaBuffer, name: &str, rect: Rect) -> Option<u32> {
        self.draw_image_at(dst, name, rect, self.scale.scale())
    }

    /// [`Self::draw_image`] at a raw render factor (the SSAA pass).
    pub fn draw_image_at(
        &self,
        dst: &mut RgbaBuffer,
        name: &str,
        rect: Rect,
        factor: u32,
    ) -> Option<u32> {
        if !self.is_hd() {
            return None;
        }

        let image = self.images.get(name)?;

        Some(stretch_into(dst, image, self.render_rect_at(rect, factor)))
    }

    /// Draws the window background: the HD frame when loaded, else the original
    /// palette `BigImage` from `source` (`S = 1` is the exact 1:1 picture).
    ///
    /// Always paints (when the source frame is valid) and returns the number of
    /// written pixels — this is the per-asset fallback of the full frame.
    pub fn draw_frame_with_fallback(&self, dst: &mut RgbaBuffer, source: &ChromeSource) -> u32 {
        self.draw_frame_with_fallback_at(dst, source, self.scale.scale())
    }

    /// [`Self::draw_frame_with_fallback`] at a raw render factor: both the HD
    /// texture and the original palette fallback are stretched to `×S·aa`, so
    /// the SSAA resolve sees the same picture either way.
    pub fn draw_frame_with_fallback_at(
        &self,
        dst: &mut RgbaBuffer,
        source: &ChromeSource,
        factor: u32,
    ) -> u32 {
        if let Some(painted) = self.draw_frame_at(dst, factor) {
            return painted;
        }

        let frame = source.frame();

        stretch_sampled(
            dst,
            self.frame_rect_at(factor),
            frame.width as u32,
            frame.height as u32,
            |x, y| frame.rgb(x, y).map(Rgba::from_rgb),
        )
    }

    /// Draws one panel half: the HD texture when loaded, else the original
    /// palette `SimpleImage` from `source` (`0` when that is missing too).
    pub fn draw_panel_with_fallback(
        &self,
        dst: &mut RgbaBuffer,
        part: ChromePanel,
        source: &ChromeSource,
    ) -> u32 {
        if let Some(painted) = self.draw_panel(dst, part) {
            return painted;
        }

        let Some(image) = source.panel(part) else {
            return 0;
        };

        stretch_sampled(
            dst,
            self.render_rect(part.logical_rect()),
            image.width as u32,
            image.height as u32,
            |x, y| source.image_color(image, x, y),
        )
    }

    /// Draws the named interface image: the HD texture when loaded, else the
    /// original palette `SimpleImage` from `source` (`0` when it is absent too).
    pub fn draw_image_with_fallback(
        &self,
        dst: &mut RgbaBuffer,
        name: &str,
        rect: Rect,
        source: &ChromeSource,
    ) -> u32 {
        if let Some(painted) = self.draw_image(dst, name, rect) {
            return painted;
        }

        let Some(image) = source.image(name) else {
            return 0;
        };

        stretch_sampled(
            dst,
            self.render_rect(rect),
            image.width as u32,
            image.height as u32,
            |x, y| source.image_color(image, x, y),
        )
    }
}

/// HD skins for the procedural UI boxes of [`crate::ui::render_ui`], one texture
/// per [`UiBox`] slot.
///
/// A slot without a texture falls back per part: the box is drawn procedurally
/// by the theme. The hover and disabled button states additionally fall back to
/// the base [`UiBox::Button`] skin, so one button texture already keeps the
/// buttons HD while the other states stay procedural (accent/border semantics
/// preserved).
///
/// Like [`HdChrome`], a `PixelPerfect` skin never paints — `S = 1` is exactly
/// [`crate::ui::render_ui`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdUiSkin {
    scale: RenderScale,
    parts: BTreeMap<UiBox, HdImage>,
}

impl HdUiSkin {
    /// A skin with no HD art: every box is procedural.
    pub fn empty(scale: RenderScale) -> Self {
        Self {
            scale,
            parts: BTreeMap::new(),
        }
    }

    /// A skin from already decoded textures (tests, or callers that resolve
    /// assets themselves).
    pub fn with_parts(
        scale: RenderScale,
        parts: impl IntoIterator<Item = (UiBox, HdImage)>,
    ) -> Self {
        Self {
            scale,
            parts: parts.into_iter().collect(),
        }
    }

    /// Loads every [`UiBox::ALL`] slot from the remaster root.
    ///
    /// The tag of a slot is [`ui_skin_tag(part.name(), scale.scale())`](ui_skin_tag);
    /// a slot that does not resolve stays procedural. In
    /// [`RenderScale::PixelPerfect`] nothing is loaded.
    pub fn load(cache: &RemasterCache, scale: RenderScale) -> Self {
        if !scale.is_hd() {
            return Self::empty(scale);
        }

        let parts = UiBox::ALL
            .into_iter()
            .filter_map(|part| {
                cache
                    .resolve(&ui_skin_tag(part.name(), scale.scale()))
                    .as_ref()
                    .and_then(HdImage::from_texture)
                    .map(|image| (part, image))
            })
            .collect();

        Self { scale, parts }
    }

    /// Presentation scale this skin was loaded for.
    pub fn scale(&self) -> RenderScale {
        self.scale
    }

    /// `true` in the HD modes (`S > 1`), where the skin may paint.
    pub fn is_hd(&self) -> bool {
        self.scale.is_hd()
    }

    /// `true` when no slot has HD art.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Number of loaded slot textures (`0..=UiBox::ALL.len()`).
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// HD texture of a slot, if loaded (no state fallback here).
    pub fn part(&self, part: UiBox) -> Option<&HdImage> {
        self.parts.get(&part)
    }

    /// Texture used to paint `part`, with the button-state fallback.
    fn image_for(&self, part: UiBox) -> Option<&HdImage> {
        self.parts.get(&part).or_else(|| match part {
            UiBox::ButtonHover | UiBox::ButtonDisabled => self.parts.get(&UiBox::Button),
            _ => None,
        })
    }
}

impl UiBoxPainter for HdUiSkin {
    fn paint_box(&self, dst: &mut RgbaBuffer, rect: Rect, part: UiBox) -> bool {
        if !self.is_hd() || rect.is_empty() {
            return false;
        }

        let Some(image) = self.image_for(part) else {
            return false;
        };

        stretch_into(dst, image, rect.into());

        true
    }
}

#[cfg(test)]
#[path = "hd_chrome_tests.rs"]
mod tests;
