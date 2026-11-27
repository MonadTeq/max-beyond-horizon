//! Lossless sample sources generated from the original game data.
//!
//! These are the same images R0 measures the transcoder on
//! (`r0-measure/src/transcode.rs::test_images`), decoded through the very decoders the
//! client uses, so the R2 sample exercises real art rather than synthetic gradients:
//!
//! * `GREEN_1.WRL` tile 0 — a 64x64 terrain tile (palette-applied RGBA) and its
//!   nearest-neighbour x4 HD variant (256x256, `scale = 4`, its own tag: one tag maps
//!   to one asset in the manifest);
//! * `FRAMEPIC` from `MAX.RES` — the 640x480 full-screen window chrome;
//! * `TANK` from `MAX.RES` — a unit sprite frame, the smallest and most numerous class.
//!
//! Everything is written as PNG: lossless, and the format the encode step reads back.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use max_assets::image::BigImage;
use max_assets::res::ResArchive;
use max_assets::sprite::decode_sprite;
use max_assets::wrl::WrlMap;
use max_render::palette::{BrightnessLut, ColorLut, Palette, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;
use max_render::terrain::{palette_from_wrl, PaletteRgba};

/// One generated lossless source.
#[derive(Debug, Clone)]
pub struct Generated {
    /// File name inside the sample directory (`<tag>[@<N>x].png`).
    pub file_name: String,
    /// Manifest tag.
    pub tag: String,
    /// Presentation scale the image was authored for.
    pub scale: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA8 pixels, row-major.
    pub rgba: Vec<u8>,
}

/// Builds the sample images from `root` (`max-original/`).
pub fn build(root: &Path) -> Result<Vec<Generated>> {
    let data_dir = root.join("max-original");
    let mut images = Vec::new();

    // 1. A real terrain tile (64x64) and its x4 HD variant (256x256).
    let wrl_bytes = std::fs::read(data_dir.join("GREEN_1.WRL"))
        .with_context(|| format!("cannot read {}", data_dir.join("GREEN_1.WRL").display()))?;
    let map = WrlMap::parse(&wrl_bytes).context("cannot parse GREEN_1.WRL")?;
    let tile = map.tile_pixels(0).context("GREEN_1.WRL has no tiles")?;
    let palette = palette_from_wrl(map.palette());
    let tile_rgba = tile_rgba(tile, &palette);

    images.push(Generated {
        file_name: "terrain_tile.png".to_string(),
        tag: "terrain_tile".to_string(),
        scale: 1,
        width: 64,
        height: 64,
        rgba: tile_rgba.clone(),
    });
    images.push(Generated {
        file_name: "terrain_tile_hd@4x.png".to_string(),
        tag: "terrain_tile_hd".to_string(),
        scale: 4,
        width: 256,
        height: 256,
        rgba: upscale(&tile_rgba, 64, 64, 256, 256),
    });

    // 2. The full-screen window chrome (FRAMEPIC 640x480, own palette).
    let archive = ResArchive::open(data_dir.join("MAX.RES"))
        .with_context(|| format!("cannot open {}", data_dir.join("MAX.RES").display()))?;
    let frame_pic = archive
        .read("FRAMEPIC")
        .context("MAX.RES has no FRAMEPIC")?;
    let big = BigImage::decode(frame_pic).context("cannot decode FRAMEPIC")?;
    let width = u32::from(big.width as u16);
    let height = u32::from(big.height as u16);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let rgb = big.rgb(x, y).unwrap_or([0, 0, 0]);
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }

    images.push(Generated {
        file_name: "framepic.png".to_string(),
        tag: "framepic".to_string(),
        scale: 1,
        width,
        height,
        rgba,
    });

    // 3. A unit sprite frame (palette + brightness LUT + command-color LUT, as drawn).
    let palette_bytes = std::fs::read(data_dir.join("COLOR.PAL"))
        .with_context(|| format!("cannot read {}", data_dir.join("COLOR.PAL").display()))?;
    let color_palette =
        Palette::from_color_pal(&palette_bytes).context("cannot parse COLOR.PAL")?;
    let brightness = BrightnessLut::build(&color_palette);
    let tank = decode_sprite(archive.read("TANK").context("MAX.RES has no TANK")?)
        .context("cannot decode the TANK sprite")?;
    let frame = tank.frame(0).context("TANK has no frames")?;
    let renderer = SpriteRenderer::new(
        &color_palette,
        &brightness,
        ColorLut::identity(),
        FULL_BRIGHTNESS_LEVEL,
    );
    let buffer = renderer.render_frame(frame);

    images.push(Generated {
        file_name: "tank_frame.png".to_string(),
        tag: "tank_frame".to_string(),
        scale: 1,
        width: buffer.width(),
        height: buffer.height(),
        rgba: buffer.pixels().to_vec(),
    });

    Ok(images)
}

/// Writes every generated image as PNG into `directory` (created if missing).
pub fn write(directory: &Path, images: &[Generated]) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(directory)
        .with_context(|| format!("cannot create {}", directory.display()))?;

    let mut written = Vec::with_capacity(images.len());

    for image in images {
        let path = directory.join(&image.file_name);
        image::save_buffer_with_format(
            &path,
            &image.rgba,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .with_context(|| format!("cannot write {}", path.display()))?;
        written.push(path);
    }

    Ok(written)
}

/// Expands palette indices to RGBA through the map palette.
fn tile_rgba(tile: &[u8], palette: &PaletteRgba) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(tile.len() * 4);

    for index in tile {
        rgba.extend_from_slice(&palette[*index as usize]);
    }

    rgba
}

/// Nearest-neighbour upscale of an RGBA image (the x4 HD variant of a tile).
fn upscale(source: &[u8], width: u32, height: u32, new_width: u32, new_height: u32) -> Vec<u8> {
    let mut out = vec![0_u8; new_width as usize * new_height as usize * 4];

    for y in 0..new_height {
        let source_y = (y * height / new_height).min(height - 1);
        for x in 0..new_width {
            let source_x = (x * width / new_width).min(width - 1);
            let from = ((source_y * width + source_x) * 4) as usize;
            let to = ((y * new_width + x) * 4) as usize;
            out[to..to + 4].copy_from_slice(&source[from..from + 4]);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::upscale;

    #[test]
    fn upscale_repeats_each_source_pixel() {
        let source = vec![
            1, 2, 3, 4, //
            5, 6, 7, 8,
        ];

        let upscaled = upscale(&source, 2, 1, 4, 2);
        let row = |y: usize| -> Vec<u8> {
            let start = y * 4 * 4;
            upscaled[start..start + 16].to_vec()
        };

        assert_eq!(row(0), vec![1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8, 5, 6, 7, 8]);
        assert_eq!(row(1), row(0), "the single source row is repeated");
    }
}
