//! Rasterization of resources into RGBA: palettes, transparency, frame strips.
//!
//! The conventions are the same as in `max-brh-assets` (`max-brh-qa-tools/max-resources-viewer`), so
//! the unit export stays compatible with the existing one:
//!
//! * sprites (`COMMTWR`…`WALDO`) are indexed by `COLOR.PAL`, whose
//!   components are already multiplied by 4 (`Palette::rgb`);
//! * structural sprite transparency (skipped RLE pixels) → alpha 0;
//! * the `S_*` shadow is a mask: the shaded pixel is black with alpha 160, everything else
//!   is transparent;
//! * `sheet.png` — a horizontal frame strip aligned by the hotspot
//!   (`hotx/hoty`), so the animation does not "jump" (the layout is 1:1 with
//!   `max-brh-assets/README.md`, the "Frame strip" section);
//! * the `P_*` portrait is a 300×240 big image: its indices are mapped through
//!   the **window palette** (`FRAMEPIC`), which is what `CargoMenu`/`BuildMenu` do
//!   (`WindowManager_LoadBigImage(..., palette_from_image = false)`); the frame
//!   is fully opaque;
//! * FLC frames — the frame indices + this FLC's `COLOR_256` palette; the components
//!   are stored as 6-bit and multiplied by 4 on output.

use max_assets::flic::FlicFrame;
use max_assets::image::BigImage;
use max_assets::sprite::{ShadowFrame, SpriteFrame};
use max_render::palette::Palette;

use crate::png::RgbaImage;

/// Shadow alpha in the export (as in `max-brh-assets`: a `_mask` PNG).
pub const SHADOW_ALPHA: u8 = 160;

/// The factor from the 6-bit palette to 8-bit (`Palette::SIX_BIT_SCALE`).
pub const SIX_BIT_SCALE: u16 = 4;

/// The `P_*` portrait size in the game (big image, `CargoMenu`/`BuildMenu`).
pub const PORTRAIT_WIDTH: u32 = 300;
/// The `P_*` portrait height.
pub const PORTRAIT_HEIGHT: u32 = 240;

/// An 8-bit color from a 6-bit palette component (`×4`, saturating).
#[inline]
pub fn six_bit(value: u8) -> u8 {
    (u16::from(value) * SIX_BIT_SCALE).min(255) as u8
}

/// The window palette (`FRAMEPIC`) → 256 RGB colors.
///
/// In C++ the system window palette is the background big image's palette (`UiPalette` in
/// the port); the `P_*` portraits and FLC frames are drawn with exactly it (`palette_from_image =
/// false`). A `P_*`'s own palette differs from it by units in the low bits,
/// so the image matches (verified on all 71 portraits).
pub fn window_palette(image: &BigImage) -> [[u8; 3]; 256] {
    let mut colors = [[0u8; 3]; 256];

    for (index, color) in colors.iter_mut().enumerate() {
        *color = image.color(index as u8);
    }

    colors
}

/// A 6-bit FLC palette → an 8-bit color table.
pub fn flic_palette(palette: &[[u8; 3]; 256]) -> [[u8; 3]; 256] {
    let mut colors = [[0u8; 3]; 256];

    for (index, color) in colors.iter_mut().enumerate() {
        let entry = palette[index];
        *color = [six_bit(entry[0]), six_bit(entry[1]), six_bit(entry[2])];
    }

    colors
}

/// One frame of a regular sprite → an RGBA image `width × height`.
pub fn sprite_frame_image(frame: &SpriteFrame, palette: &Palette) -> RgbaImage {
    let width = u32::from(frame.width);
    let height = u32::from(frame.height);
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let Some(index) = frame.pixel(x, y) else {
                continue;
            };
            let [red, green, blue] = palette.rgb(index);

            out.set_pixel(x, y, [red, green, blue, 255]);
        }
    }

    out
}

/// One shadow frame → an RGBA mask (black, alpha [`SHADOW_ALPHA`]).
pub fn shadow_frame_image(frame: &ShadowFrame) -> RgbaImage {
    let width = u32::from(frame.width);
    let height = u32::from(frame.height);
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            if frame.darkened(x, y) {
                out.set_pixel(x, y, [0, 0, 0, SHADOW_ALPHA]);
            }
        }
    }

    out
}

/// An FLC frame → RGBA via the 8-bit color table (all pixels are opaque).
pub fn flic_frame_image(frame: &FlicFrame, colors: &[[u8; 3]; 256]) -> RgbaImage {
    let width = frame.width as u32;
    let height = frame.height as u32;
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let index = frame.pixels[(y as usize) * frame.width + x as usize];
            let [red, green, blue] = colors[usize::from(index)];

            out.set_pixel(x, y, [red, green, blue, 255]);
        }
    }

    out
}

/// A big image (`P_*`) → RGBA via the window palette; the frame is fully opaque.
pub fn big_image_image(image: &BigImage, colors: &[[u8; 3]; 256]) -> RgbaImage {
    let width = u32::from(image.width as u16);
    let height = u32::from(image.height as u16);
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let Some(index) = image.pixel(x, y) else {
                continue;
            };
            let [red, green, blue] = colors[usize::from(index)];

            out.set_pixel(x, y, [red, green, blue, 255]);
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Frame strip (as in `max-brh-assets/README.md`, the "Frame strip" section)
// ---------------------------------------------------------------------------

/// The strip's common anchor: the maximum of the positive hotspots per axis.
fn anchor(frames: &[(u16, u16, i16, i16)]) -> (u32, u32) {
    let x = frames
        .iter()
        .map(|(_, _, hot, _)| u32::from((*hot).max(0) as u16))
        .max()
        .unwrap_or(0);
    let y = frames
        .iter()
        .map(|(_, _, _, hot)| u32::from((*hot).max(0) as u16))
        .max()
        .unwrap_or(0);

    (x, y)
}

/// How much wider/taller a cell is than the frame because of the common anchor (≥ 0).
fn extra(anchor: u32, hot: i16) -> u32 {
    (i64::from(anchor) - i64::from(hot)).max(0) as u32
}

/// Frame strip dimensions: the sum of cells along X, the maximum height along Y.
pub fn strip_dimensions(frames: &[(u16, u16, i16, i16)]) -> (u32, u32) {
    if frames.is_empty() {
        return (0, 0);
    }

    let (anchor_x, anchor_y) = anchor(frames);
    let width: u32 = frames
        .iter()
        .map(|(width, _, hot, _)| u32::from(*width) + extra(anchor_x, *hot))
        .sum();
    let height = frames
        .iter()
        .map(|(_, height, _, hot)| u32::from(*height) + extra(anchor_y, *hot))
        .max()
        .unwrap_or(0);

    (width, height)
}

/// Sprite frame metadata for the layout.
pub fn sprite_meta(frames: &[&SpriteFrame]) -> Vec<(u16, u16, i16, i16)> {
    frames
        .iter()
        .map(|frame| (frame.width, frame.height, frame.hotx, frame.hoty))
        .collect()
}

/// The sprite's frame strip (RGBA, aligned by the hotspot).
pub fn sprite_strip(frames: &[&SpriteFrame], palette: &Palette) -> RgbaImage {
    let meta = sprite_meta(frames);
    let (width, height) = strip_dimensions(&meta);
    let mut out = RgbaImage::new(width, height);
    let (anchor_x, anchor_y) = anchor(&meta);
    let mut cell_x = 0u32;

    for (frame, (frame_width, _, hot, hot_y)) in frames.iter().zip(meta.iter()) {
        let origin_x = cell_x + extra(anchor_x, *hot);
        let origin_y = extra(anchor_y, *hot_y);

        for y in 0..u32::from(frame.height) {
            for x in 0..u32::from(frame.width) {
                let Some(index) = frame.pixel(x, y) else {
                    continue;
                };
                let [red, green, blue] = palette.rgb(index);

                out.set_pixel(origin_x + x, origin_y + y, [red, green, blue, 255]);
            }
        }

        cell_x += u32::from(*frame_width) + extra(anchor_x, *hot);
    }

    out
}

/// Fallback portrait: the first sprite frame, upscaled by an integer factor and
/// fitted into 300×240 on a black background (like `FillOrRestoreWindow(..., BLACK)`).
///
/// The factor is the largest integer with which the frame fits into the portrait
/// (`max(1, min(300 / w, 240 / h))`).
pub fn portrait_fallback(frame: &SpriteFrame, palette: &Palette) -> RgbaImage {
    let source = sprite_frame_image(frame, palette);
    let mut out = RgbaImage::new(PORTRAIT_WIDTH, PORTRAIT_HEIGHT);

    // The portrait window's black background.
    for pixel in out.rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[0, 0, 0, 255]);
    }

    let scale = (PORTRAIT_WIDTH / source.width.max(1))
        .min(PORTRAIT_HEIGHT / source.height.max(1))
        .max(1);
    let scaled_width = source.width * scale;
    let scaled_height = source.height * scale;
    let offset_x = (PORTRAIT_WIDTH - scaled_width) / 2;
    let offset_y = (PORTRAIT_HEIGHT - scaled_height) / 2;

    for y in 0..scaled_height {
        for x in 0..scaled_width {
            let Some(color) = source.pixel(x / scale, y / scale) else {
                continue;
            };

            if color[3] == 0 {
                continue;
            }

            out.set_pixel(offset_x + x, offset_y + y, color);
        }
    }

    out
}

/// The fallback portrait factor for a frame (for `unit.json`).
pub fn fallback_scale(frame: &SpriteFrame) -> u32 {
    (PORTRAIT_WIDTH / u32::from(frame.width).max(1))
        .min(PORTRAIT_HEIGHT / u32::from(frame.height).max(1))
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u16, height: u16, hotx: i16, hoty: i16) -> SpriteFrame {
        let mut pixels = vec![None; usize::from(width) * usize::from(height)];

        pixels[0] = Some(1);

        SpriteFrame {
            width,
            height,
            hotx,
            hoty,
            rows: Vec::new(),
            pixels,
        }
    }

    #[test]
    fn strip_packs_frames_by_hot_spot() {
        // As in the max-resources-viewer test: cells 44+6=50 and 56, height max(50,57).
        let (width, height) = strip_dimensions(&[(44, 45, 20, 23), (56, 57, 26, 28)]);

        assert_eq!(width, 50 + 56);
        assert_eq!(height, 57);
    }

    #[test]
    fn negative_hot_spot_extends_the_cell() {
        let (width, height) = strip_dimensions(&[(6, 8, -13, -28), (28, 25, 3, -16)]);

        assert_eq!(width, 22 + 28);
        assert_eq!(height, 41);
    }

    #[test]
    fn six_bit_scales_to_eight_bit_without_overflow() {
        assert_eq!(six_bit(0), 0);
        assert_eq!(six_bit(63), 252);
        assert_eq!(six_bit(64), 255);
        assert_eq!(six_bit(255), 255);
    }

    #[test]
    fn shadow_image_is_black_with_alpha() {
        let shadow = ShadowFrame {
            width: 2,
            height: 1,
            hotx: 0,
            hoty: 0,
            rows: Vec::new(),
            mask: vec![true, false],
        };
        let image = shadow_frame_image(&shadow);

        assert_eq!(image.pixel(0, 0), Some([0, 0, 0, SHADOW_ALPHA]));
        assert_eq!(image.pixel(1, 0), Some([0, 0, 0, 0]));
    }

    #[test]
    fn fallback_portrait_fits_into_300x240() {
        let palette = Palette::from_color_pal(&[0u8; 768]).expect("палитра");
        let source = frame(44, 45, 20, 23);
        let portrait = portrait_fallback(&source, &palette);

        assert_eq!((portrait.width, portrait.height), (300, 240));
        assert_eq!(fallback_scale(&source), 5); // 44*5=220, 45*5=225 — it fits
    }

    #[test]
    fn sprite_strip_is_as_wide_as_the_sum_of_cells() {
        let palette = Palette::from_color_pal(&[0u8; 768]).expect("палитра");
        let frames = [frame(10, 4, 2, 2), frame(12, 6, 6, 3)];
        let strip = sprite_strip(&[&frames[0], &frames[1]], &palette);

        // Cells: 10 + (6-2) = 14 and 12 + 0 = 12.
        assert_eq!(strip.width, 26);
        assert_eq!(strip.height, 6);
    }
}
