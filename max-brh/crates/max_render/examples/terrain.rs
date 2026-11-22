//! Renders real surface tiles from `.WRL` (no SDL).
//!
//! ```sh
//! cargo run -p max_render --example terrain -- ../max-original/SNOW_1.WRL /tmp/out
//! ```
//!
//! Writes two BMPs: an overview of the whole map (zoom 8 → 8 px per cell) and a detailed
//! fragment in the center (zoom 64 → 64 px, like `Gfx_ZoomLevel` in the original).
//! This verifies that the `.WRL` palette/tiles are read correctly — the same
//! image that the C++ port draws (`Gfx_DecodeMapTile`).

use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

use max_assets::wrl::WrlMap;
use max_render::buffer::RgbaBuffer;
use max_render::terrain::TerrainAtlas;
use max_render::viewport::Viewport;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let wrl_path = args
        .next()
        .unwrap_or_else(|| "../../max-original/SNOW_1.WRL".to_string());
    let out_prefix = args.next().unwrap_or_else(|| "terrain".to_string());

    let wrl = WrlMap::open(&wrl_path)?;
    let atlas = TerrainAtlas::from_wrl(&wrl);

    println!(
        "wrl: {wrl_path} {}x{} клеток, тайлов {} ({} байт пикселей)",
        atlas.size().0,
        atlas.size().1,
        atlas.tile_count(),
        atlas.tile_count() as usize * max_assets::wrl::TILE_PIXELS
    );

    // 1. Whole-map overview: 112×112 cells at 8 px = 896×896.
    let (w, h) = (atlas.size().0.max(1) as u32, atlas.size().1.max(1) as u32);
    let zoom = 8u32;
    let mut frame = RgbaBuffer::filled(w * zoom, h * zoom, max_render::buffer::Rgba::BLACK);
    let viewport = Viewport::with_center(w * zoom, h * zoom, (0, 0), zoom);
    let painted = atlas.render_into(&mut frame, &viewport);
    let overview = PathBuf::from(format!("{out_prefix}-map.bmp"));
    write_bmp(&overview, &frame)?;
    println!("overview: {} (клеток {painted})", overview.display());

    // 2. Detail fragment 512×384, zoom 1:1 (64 px per cell). The center is
    //    configurable (by default the map center; for SNOW_1 that is water with land at the edges).
    let center = match (args.next(), args.next()) {
        (Some(x), Some(y)) => (x.parse::<i32>()?, y.parse::<i32>()?),
        _ => (0, 0),
    };
    let detail = Viewport::with_center(512, 384, center, 64);
    let mut frame = RgbaBuffer::filled(512, 384, max_render::buffer::Rgba::BLACK);
    let painted = atlas.render_into(&mut frame, &detail);
    let detail_path = PathBuf::from(format!("{out_prefix}-detail.bmp"));
    write_bmp(&detail_path, &frame)?;
    println!(
        "detail: {} (клеток {painted}, центр {:?})",
        detail_path.display(),
        center
    );

    Ok(())
}

/// 24-bit BMP (bottom-up) — no external crates.
fn write_bmp(path: &PathBuf, buffer: &RgbaBuffer) -> io::Result<()> {
    let width = buffer.width();
    let height = buffer.height();
    let stride = (width * 3 + 3) & !3;
    let image_size = stride * height;
    let file_size = 54 + image_size;
    let mut out = Vec::with_capacity(file_size as usize);

    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&image_size.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    for y in (0..height).rev() {
        let mut row = Vec::with_capacity(stride as usize);

        for x in 0..width {
            let pixel = buffer
                .get(x as i32, y as i32)
                .unwrap_or(max_render::buffer::Rgba::BLACK);
            row.extend_from_slice(&[pixel.b, pixel.g, pixel.r]);
        }

        while row.len() < stride as usize {
            row.push(0);
        }

        out.extend_from_slice(&row);
    }

    let mut file = File::create(path)?;
    file.write_all(&out)
}
