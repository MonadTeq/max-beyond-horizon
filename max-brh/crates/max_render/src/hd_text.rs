//! HD raster fonts (`max-brh-docs/development/remaster-hd.md` §R6).
//!
//! The game text is drawn with the **raster** GNW fonts `FONT1.FON`/`FONT2.FON`/
//! `FONT5.FON` ([`crate::text::gnw`]), not with a TTF: the per-glyph advance, the
//! cell height and the `Text_TextLine` relief are properties of those bitmaps.
//! The R6 remaster therefore keeps the glyphs raster and ships a **pre-baked
//! RGBA8 glyph atlas** per font and presentation scale `S`, resolved through the
//! R1 loader ([`max_assets::remaster::RemasterCache::resolve`]) with the
//! [`max_assets::remaster::font_tag`] convention (`font_FONT1_x2`, …).
//!
//! # Atlas layout
//!
//! One atlas holds all 256 glyphs of the `.FON` table (`0..=255`) in character
//! order, **row-major in one row**: the cell of character `c` starts at
//! `x = advance(0..c)·S` and has the size `advance(c)·S × height·S`. The metrics
//! are the original `.FON` metrics, so the layout is fully derived from the font
//! and nothing else is stored in the asset; an atlas of the wrong resolution is
//! rejected and the caller falls back (per font). Padding of the texture to a
//! multiple of the block size (`4` texels) on the right/bottom is tolerated.
//!
//! The payload is a **mask**, not a picture: `A` is the ink coverage (a texel is
//! ink at `A >= INK_THRESHOLD`, see [`INK_THRESHOLD`]), `R`/`G`/`B` are unused.
//! The atlas must be encoded losslessly/high quality (UASTC, as the R4 index
//! maps): a lossy ETC1S payload could shift the coverage around the threshold.
//! Glyphs stay crisp — the remaster keeps the pixel look on purpose.
//!
//! # Metrics and relief
//!
//! Everything this module reports and draws is in **render pixels** of the
//! `640·S × 480·S` frame ([`RenderScale`]): the advance, the cell height, the
//! line width (`Text_GetWidth`) and the line spacing (`Text_TextBox`) are exactly
//! `S` times the 1:1 value. The logical 640×480 layout is unchanged; convert a
//! point with [`HdFontAtlas::render_point`].
//!
//! [`draw_text_hd_shadowed`] reproduces `Text_TextLine` at HD: the fill first,
//! then the down-right overwrite — `outline` where the neighbour is fill,
//! `shadow` otherwise — with the one-pixel logical shift expressed as `S` render
//! pixels and the `+1` reserve of `Text_GetWidth + 1`/`Text_GetHeight + 1`
//! scaled to `+S`. An HD atlas that is the exact `×S` nearest upscale of the
//! original bitmap therefore gives exactly the `×S` upscale of the 1:1 relief
//! (that is the R6 golden invariant). [`draw_text_hd_outlined`] is the other
//! outline of the original (`Text_Blit` + `GNW_TEXT_OUTLINE`), also at `×S`.
//!
//! # Fallback
//!
//! Fallback is per font and never aborts a load:
//!
//! * no manifest entry, a missing `.ktx2`, a transcode failure, an atlas whose
//!   size does not match the original metrics or a `×1` atlas requested at
//!   `S > 1` → [`HdFontAtlas::has_atlas`] is `false` and the drawers sample the
//!   original GNW bitmap with nearest `×S` (visually the `×S` upscale of the
//!   current 1:1 text);
//! * [`draw_text_line_hd`] is the same `×S` path for the built-in 8×8 ASCII
//!   font used for service messages ([`crate::text::draw_text_line`]);
//! * [`RenderScale::PixelPerfect`] never enters the HD layer: [`HdFontAtlas::load`]
//!   resolves nothing and the drawers are byte-for-byte the 1:1 functions.
//!
//! This is presentation only. Nothing here is imported by `max_core`, and an SDL
//! window is not needed (the module is pure integer arithmetic).
//!
//! # Scope of an atlas
//!
//! [`HdGlyphAtlas`] is the decoded coverage mask. [`HdFontAtlas`] is one GNW font
//! at one scale with its optional atlas: metrics, [`HdFontAtlas::glyph_rect`]
//! (the cell in the canonical layout), [`HdFontAtlas::ink`] and the drawers.
//! [`HdFonts`] loads all three fonts of the game — the R7 caller draws a caption
//! with `fonts.font(GnwFontId::Font5)`.

use max_assets::remaster::{font_tag, RemasterCache, RemasterTexture};

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};
use crate::render_scale::RenderScale;
use crate::text::gnw::{GnwFont, GnwFontId, GNW_FALLBACK, GNW_GLYPHS};

/// Coverage at which an atlas texel counts as ink (`0` — empty, `255` — full).
///
/// The HD atlas is authored as a raster mask (`0`/`255`); the threshold only
/// absorbs round-off of a high-quality transcode. It is the same rule the
/// drawers use for every glyph pixel, so the relief stays a binary mask.
pub const INK_THRESHOLD: u8 = 128;

/// Number of glyph cells in a GNW table (`0..=255`, including the empty zero).
pub const GLYPH_COUNT: usize = GNW_GLYPHS + 1;

/// RGBA channel of the ink coverage in an HD font atlas.
pub const FONT_COVERAGE_CHANNEL: usize = 3;

/// Decoded HD glyph mask: one ink-coverage byte per texel.
///
/// Built from an R1 [`RemasterTexture`] ([`Self::from_texture`]) or directly
/// from coverage/RGBA bytes (tests and callers that decode the asset
/// themselves). The atlas is just a byte mask — the glyph colors are arguments
/// of the drawers, so one atlas serves every text color, outline and shadow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdGlyphAtlas {
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}

impl HdGlyphAtlas {
    /// Builds an atlas from one coverage byte per texel; `None` for an empty
    /// atlas or a length that does not match `width * height`.
    pub fn from_coverage(width: u32, height: u32, coverage: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }

        if coverage.len() != width as usize * height as usize {
            return None;
        }

        Some(Self {
            width,
            height,
            coverage,
        })
    }

    /// Decodes the transport convention (the [`FONT_COVERAGE_CHANNEL`] of every
    /// texel); `None` for an empty or truncated texture.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }

        let texels = width as usize * height as usize;

        if rgba.len() < texels * 4 {
            return None;
        }

        let coverage = rgba
            .chunks_exact(4)
            .take(texels)
            .map(|texel| texel[FONT_COVERAGE_CHANNEL])
            .collect();

        Some(Self {
            width,
            height,
            coverage,
        })
    }

    /// Decodes a texture resolved by the R1 loader; `None` for an empty or
    /// alpha-less texture.
    ///
    /// The mask channel is the alpha channel, so a texture without alpha cannot
    /// describe a glyph (every texel would count as ink) and is rejected.
    pub fn from_texture(texture: &RemasterTexture) -> Option<Self> {
        if !texture.has_alpha {
            return None;
        }

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

    /// `true` when the atlas has no texels (never for a built one).
    pub fn is_empty(&self) -> bool {
        self.coverage.is_empty()
    }

    /// Ink coverage of a texel; `None` outside the atlas.
    pub fn coverage(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }

        Some(self.coverage[y as usize * self.width as usize + x as usize])
    }

    /// The texel is ink (`coverage >= INK_THRESHOLD`); `false` outside.
    pub fn is_ink(&self, x: u32, y: u32) -> bool {
        self.coverage(x, y)
            .is_some_and(|coverage| coverage >= INK_THRESHOLD)
    }
}

/// One GNW font at one presentation scale, with an optional HD atlas.
///
/// The original [`GnwFont`] is kept as the fallback (and as the source of the
/// metrics), so a font without an HD atlas still draws — nearest `×S` of the
/// original bitmap. Metrics and drawing are in render pixels of the
/// `640·S × 480·S` frame; [`Self::render_point`] converts a logical 640×480
/// point. `S = 1` ([`RenderScale::PixelPerfect`]) never loads an atlas and
/// draws exactly like [`crate::text::gnw`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdFontAtlas {
    id: GnwFontId,
    scale: RenderScale,
    atlas: Option<HdGlyphAtlas>,
    /// Start of every glyph cell in the canonical atlas layout, in texels.
    offsets: [u32; GLYPH_COUNT],
    /// Width required by the canonical layout (`sum(advance) · S`), in texels.
    layout_width: u32,
}

impl HdFontAtlas {
    /// The font without an HD atlas: every drawer falls back to the original
    /// GNW bitmap (nearest `×S`).
    pub fn new(id: GnwFontId, scale: RenderScale) -> Self {
        let source = id.font();
        let (offsets, layout_width) = glyph_layout(source, scale.scale());

        Self {
            id,
            scale,
            atlas: None,
            offsets,
            layout_width,
        }
    }

    /// The font with an HD atlas; `None` when the atlas does not match the
    /// original metrics.
    ///
    /// The canonical layout requires `atlas.width() >= sum(advance) · S` and
    /// `atlas.height() >= height · S`; a smaller atlas would shift or clip
    /// glyphs, so it is rejected and the caller keeps the fallback. Extra texels
    /// on the right/bottom (block-size padding of the `.ktx2`) are ignored.
    pub fn with_atlas(id: GnwFontId, scale: RenderScale, atlas: HdGlyphAtlas) -> Option<Self> {
        let source = id.font();
        let (offsets, layout_width) = glyph_layout(source, scale.scale());
        let required_height = source.height() as u32 * scale.scale();

        if atlas.width() < layout_width || atlas.height() < required_height {
            return None;
        }

        Some(Self {
            id,
            scale,
            atlas: Some(atlas),
            offsets,
            layout_width,
        })
    }

    /// Loads the HD atlas of `id` from the remaster root.
    ///
    /// The tag is [`font_tag(id.file_stem(), scale.scale())`](font_tag). A tag
    /// the manifest does not list, a missing `.ktx2`, a transcode failure or an
    /// atlas of the wrong size leaves that font without an HD atlas — the
    /// fallback is per font. In [`RenderScale::PixelPerfect`] nothing is
    /// resolved: `S = 1` is the byte-for-byte path and never enters this layer.
    pub fn load(cache: &RemasterCache, id: GnwFontId, scale: RenderScale) -> Self {
        if !scale.is_hd() {
            return Self::new(id, scale);
        }

        cache
            .resolve(&font_tag(id.file_stem(), scale.scale()))
            .as_ref()
            .and_then(HdGlyphAtlas::from_texture)
            .and_then(|atlas| Self::with_atlas(id, scale, atlas))
            .unwrap_or_else(|| Self::new(id, scale))
    }

    /// Which GNW font this is.
    pub fn id(&self) -> GnwFontId {
        self.id
    }

    /// Presentation scale this font was loaded for.
    pub fn scale(&self) -> RenderScale {
        self.scale
    }

    /// The original font (metrics and fallback bitmaps).
    pub fn source(&self) -> &'static GnwFont {
        self.id.font()
    }

    /// The HD atlas, if one was loaded (and accepted).
    pub fn atlas(&self) -> Option<&HdGlyphAtlas> {
        self.atlas.as_ref()
    }

    /// `true` when the font draws from an HD atlas (`false` — GNW fallback).
    pub fn has_atlas(&self) -> bool {
        self.atlas.is_some()
    }

    /// Number of glyph cells in the table (always 256).
    pub fn glyph_count(&self) -> usize {
        GLYPH_COUNT
    }

    /// Glyph cell height in render pixels (`Text_GetHeight · S`).
    pub fn height(&self) -> i32 {
        self.source().height() * self.scale.scale() as i32
    }

    /// Line spacing in render pixels (`Text_TextBox`: `(height + 1) · S`).
    pub fn line_height(&self) -> i32 {
        self.source().line_height() * self.scale.scale() as i32
    }

    /// Advance of a character in render pixels (`width · S`).
    pub fn advance(&self, ch: char) -> i32 {
        self.source().char_width(ch) * self.scale.scale() as i32
    }

    /// Line width in render pixels (`Text_GetWidth · S`).
    pub fn text_width(&self, text: &str) -> i32 {
        self.source().text_width(text) * self.scale.scale() as i32
    }

    /// Logical 640×480 point → render pixel of the `640·S × 480·S` frame.
    ///
    /// The only place `S` enters text placement: layout, hit tests and alignment
    /// stay in the logical coordinates.
    pub fn render_point(&self, x: i32, y: i32) -> (i32, i32) {
        let factor = self.scale.scale() as i32;

        (x * factor, y * factor)
    }

    /// Minimum size of the HD atlas in texels: `(sum(advance) · S, height · S)`.
    ///
    /// This is the canonical layout the decoder expects; a `.ktx2` may be a few
    /// texels larger (padding), never smaller.
    pub fn atlas_size(&self) -> (u32, u32) {
        (
            self.layout_width,
            self.source().height() as u32 * self.scale.scale(),
        )
    }

    /// Cell of a character in the canonical atlas layout, in atlas texels.
    ///
    /// The rectangle is derived from the original metrics (the glyphs are packed
    /// in character order in one row), so it does not depend on whether an HD
    /// atlas is actually loaded; a character outside Latin-1 maps to the `?`
    /// cell, exactly like [`GnwFont::glyph`].
    pub fn glyph_rect(&self, ch: char) -> PixelRect {
        let index = glyph_index(ch);

        PixelRect::new(
            self.offsets[index] as i32,
            0,
            self.advance(ch),
            self.height(),
        )
    }

    /// Glyph pixel in the HD cell (`x`/`y` are render pixels inside the cell).
    ///
    /// With an atlas the texel of the canonical cell is sampled; without one the
    /// original GNW bitmap is sampled at `(x / S, y / S)` — the nearest `×S`
    /// upscale, i.e. the pixel look of the current font.
    pub fn ink(&self, ch: char, x: i32, y: i32) -> bool {
        let factor = self.scale.scale() as i32;
        let glyph = self.source().glyph(ch);

        if x < 0
            || y < 0
            || x >= i32::from(glyph.width) * factor
            || y >= i32::from(glyph.height) * factor
        {
            return false;
        }

        match &self.atlas {
            Some(atlas) => {
                let cell = self.glyph_rect(ch);
                let atlas_x = cell.ulx as u32 + x as u32;
                let atlas_y = y as u32;

                atlas.is_ink(atlas_x, atlas_y)
            }
            None => self.source().glyph_ink(ch, x / factor, y / factor),
        }
    }
}

/// All three GNW fonts of the game at one presentation scale.
///
/// [`Self::load`] resolves the HD atlas of every font independently, so a
/// partial remaster keeps the fonts it has and falls back for the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdFonts {
    scale: RenderScale,
    fonts: [HdFontAtlas; 3],
}

impl HdFonts {
    /// Loads every GNW font of [`GnwFontId::ALL`] from the remaster root.
    ///
    /// In [`RenderScale::PixelPerfect`] nothing is resolved (the 1:1 path).
    pub fn load(cache: &RemasterCache, scale: RenderScale) -> Self {
        Self {
            scale,
            fonts: GnwFontId::ALL.map(|id| HdFontAtlas::load(cache, id, scale)),
        }
    }

    /// Presentation scale of the set.
    pub fn scale(&self) -> RenderScale {
        self.scale
    }

    /// The font `id` (with or without an HD atlas).
    pub fn font(&self, id: GnwFontId) -> &HdFontAtlas {
        &self.fonts[font_index(id)]
    }

    /// `true` when at least one font draws from an HD atlas.
    pub fn has_any(&self) -> bool {
        self.fonts.iter().any(HdFontAtlas::has_atlas)
    }

    /// Number of fonts with an HD atlas (`0..=3`).
    pub fn hd_count(&self) -> usize {
        self.fonts.iter().filter(|font| font.has_atlas()).count()
    }
}

/// Index of a font in [`GnwFontId::ALL`] (the storage order of [`HdFonts`]).
const fn font_index(id: GnwFontId) -> usize {
    match id {
        GnwFontId::Font1 => 0,
        GnwFontId::Font2 => 1,
        GnwFontId::Font5 => 2,
    }
}

/// Atlas index of a character: `0..=255`, everything else is `?`.
fn glyph_index(ch: char) -> usize {
    let code = ch as usize;

    if code <= GNW_GLYPHS {
        code
    } else {
        GNW_FALLBACK as usize
    }
}

/// Canonical atlas layout: the start of every glyph cell and the total width.
fn glyph_layout(source: &GnwFont, factor: u32) -> ([u32; GLYPH_COUNT], u32) {
    let mut offsets = [0u32; GLYPH_COUNT];
    let mut cursor = 0u32;

    for (index, offset) in offsets.iter_mut().enumerate() {
        *offset = cursor;
        cursor += source.char_width(char::from(index as u8)) as u32 * factor;
    }

    (offsets, cursor)
}

/// Draws one line at `(x, y)` in render pixels (top-left of the cell).
///
/// Mirrors [`crate::text::draw_text_gnw`] with HD glyphs and metrics; returns
/// the line width in render pixels (`font.text_width(text)`). Drawing is
/// clipped by the buffer bounds; outside Latin-1 `?` is used.
pub fn draw_text_hd(
    buffer: &mut RgbaBuffer,
    x: i32,
    y: i32,
    font: &HdFontAtlas,
    text: &str,
    color: Rgba,
) -> i32 {
    let mut pen_x = x;
    let height = font.height();

    for ch in text.chars() {
        let advance = font.advance(ch);

        if pen_x >= buffer.width() as i32 {
            // The pen only moves right: nothing further will be visible.
            break;
        }

        for row in 0..height {
            let pixel_y = y + row;

            if pixel_y < 0 || pixel_y >= buffer.height() as i32 {
                continue;
            }

            for column in 0..advance {
                if font.ink(ch, column, row) {
                    buffer.set(pen_x + column, pixel_y, color);
                }
            }
        }

        pen_x += advance;
    }

    font.text_width(text)
}

/// The HD [`crate::text::draw_text_gnw_shadowed`]: the `Text_TextLine` relief.
///
/// The oracle order is preserved exactly: the glyph fill is painted first, then
/// the buffer is post-processed bottom-to-top — every fill pixel overwrites its
/// down-right neighbour with `outline` when that neighbour is also fill (the
/// lower-right diagonal of a stroke) and with `shadow` otherwise. At scale `S`
/// the logical one-pixel shift is `S` render pixels and the `+1` reserve of
/// `Text_GetWidth + 1`/`Text_GetHeight + 1` is `+S`, so an HD atlas that is the
/// exact `×S` upscale of the original bitmap yields exactly the `×S` upscale of
/// the 1:1 relief.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_hd_shadowed(
    buffer: &mut RgbaBuffer,
    x: i32,
    y: i32,
    font: &HdFontAtlas,
    text: &str,
    base: Rgba,
    outline: Rgba,
    shadow: Rgba,
) -> i32 {
    let shift = font.scale().scale() as i32;
    let width = font.text_width(text) + shift;
    let height = font.height() + shift;
    let stride = width.max(1) as usize;

    // 1 — glyph ink (the fill), 0 — empty.
    let mut mask = vec![0u8; stride * height.max(1) as usize];
    let mut pen_x = 0i32;

    for ch in text.chars() {
        let advance = font.advance(ch);

        for row in 0..font.height() {
            for column in 0..advance {
                if !font.ink(ch, column, row) {
                    continue;
                }

                let px = pen_x + column;

                if px >= 0 && px < width && row < height {
                    mask[row as usize * stride + px as usize] = 1;
                }
            }
        }

        pen_x += advance;
    }

    // 1) The fill (`Text_Blit(buffer, str, swidth, text_width, color.base)`).
    for row in 0..height {
        for column in 0..width {
            if mask[row as usize * stride + column as usize] == 1 {
                buffer.set(x + column, y + row, base);
            }
        }
    }

    // 2) The shifted overwrite of `Text_TextLine` (bottom-to-top, left-to-right):
    //    the lower-right diagonal of the fill becomes the outline, the pixel just
    //    outside the glyph the shadow. The `+S` reserve keeps every write in the
    //    box, exactly like `+1` at 1:1.
    for row in (0..height.saturating_sub(shift)).rev() {
        for column in 0..width.saturating_sub(shift) {
            let index = row as usize * stride + column as usize;

            if mask[index] != 1 {
                continue;
            }

            let target = index + shift as usize * stride + shift as usize;
            let color = if mask[target] == 1 { outline } else { shadow };

            buffer.set(x + column + shift, y + row + shift, color);
        }
    }

    font.text_width(text)
}

/// The HD [`crate::text::draw_text_gnw_outlined`] (`Text_Blit` with
/// `GNW_TEXT_OUTLINE`).
///
/// At scale `S` the outline copy is shifted by `S` render pixels instead of one
/// pixel; everything else (black-equivalent outline under the fill) is the same.
pub fn draw_text_hd_outlined(
    buffer: &mut RgbaBuffer,
    x: i32,
    y: i32,
    font: &HdFontAtlas,
    text: &str,
    base: Rgba,
    outline: Rgba,
) -> i32 {
    let shift = font.scale().scale() as i32;

    draw_text_hd(buffer, x + shift, y + shift, font, text, outline);
    draw_text_hd(buffer, x, y, font, text, base)
}

/// Draws one line of the built-in 8×8 ASCII font at `scale` in render pixels.
///
/// This is the `×S` path of [`crate::text::draw_text_line`] for the service
/// messages that have no GNW font: every logical glyph pixel becomes an `S×S`
/// block, so the text is the nearest `×S` upscale of the 1:1 line. Returns the
/// line width in render pixels ([`line_width_hd`]).
pub fn draw_text_line_hd(
    buffer: &mut RgbaBuffer,
    x: i32,
    y: i32,
    scale: RenderScale,
    text: &str,
    color: Rgba,
) -> i32 {
    let factor = scale.scale() as i32;
    let mut pen_x = x;

    for ch in text.chars() {
        if pen_x >= buffer.width() as i32 {
            break;
        }

        let rows = crate::text::glyph(ch);

        for (row, bits) in rows.iter().enumerate() {
            if *bits == 0 {
                continue;
            }

            for column in 0..crate::text::GLYPH_WIDTH {
                // Bit 0 is the left pixel (font8x8/VGA), as in `draw_text_line`.
                if bits & (0x01 << column) == 0 {
                    continue;
                }

                let block_x = pen_x + column * factor;
                let block_y = y + row as i32 * factor;

                for dy in 0..factor {
                    for dx in 0..factor {
                        buffer.set(block_x + dx, block_y + dy, color);
                    }
                }
            }
        }

        pen_x += crate::text::GLYPH_WIDTH * factor;
    }

    line_width_hd(text, scale)
}

/// Line width of the built-in 8×8 font at `scale`, in render pixels.
pub fn line_width_hd(text: &str, scale: RenderScale) -> i32 {
    crate::text::line_width(text) * scale.scale() as i32
}

#[cfg(test)]
#[path = "hd_text_tests.rs"]
mod tests;
