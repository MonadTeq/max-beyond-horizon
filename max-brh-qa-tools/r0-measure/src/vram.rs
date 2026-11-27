//! Render memory of the current 640×480 mode (R0, item 1).
//!
//! Three groups, measured separately so that the numbers stay comparable:
//!
//! 1. **the frame path** — the RGBA frame the renderer produces, the SDL
//!    streaming texture it is uploaded to, and the window drawable. The first
//!    is allocated (and measured) here; the two GPU buffers are computed from
//!    the exact SDL calls in `max_render::sdl_window::SdlWindow::present`
//!    (`create_texture_streaming(RGBA32, buffer.width, buffer.height)` and the
//!    drawable behind `Canvas`), because there is no GPU to query headlessly;
//! 2. **the render working set** the application holds while a match runs —
//!    `max_app::assets::Assets::load`, the same call the client makes;
//! 3. **the isolated components** (unit sprite sheets, the terrain atlas, the
//!    UI image library) measured by decoding them again, so the totals can be
//!    attributed without reading private fields.
//!
//! The same 1:1 frame is then rendered at S ∈ {1, 2, 4} through the *current*
//! per-pixel CPU renderer (logical frame `640·S × 480·S`, `zoom = 64·S`), so
//! the scale decision has a measured cost rather than a guess.
//!
//! Everything is heap-measured with [`crate::alloc`], so the tool prints the
//! numbers it allocated rather than a `size_of` estimate.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use max_app::assets::{Assets, UNIT_SPRITES};
use max_app::AppConfig;
use max_assets::image::{BigImage, SimpleImage};
use max_assets::res::ResArchive;
use max_assets::sprite::{decode_shadow, decode_sprite, ShadowSheet, SpriteSheet};
use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::palette::{ColorLut, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;
use max_render::terrain::TerrainAtlas;
use max_render::viewport::Viewport;
use max_resources_viewer::model::{classify_archive, Kind};

use crate::alloc;

/// Frame size of the current mode (the original 640×480 screen).
const FRAME_WIDTH: u32 = 640;
/// Frame height of the current mode.
const FRAME_HEIGHT: u32 = 480;

/// Render scales compared with the current CPU renderer.
const SCALES: [u32; 3] = [1, 2, 4];

/// Frame renders timed per scale.
const TIMED_FRAMES: usize = 20;

/// Window drawables the GPU table is computed for (`points × DPI`).
const DRAWABLES: [(u32, u32, &str); 5] = [
    (640, 480, "640×480 (1:1 window)"),
    (1280, 960, "1280×960 (S=2)"),
    (1920, 1080, "1920×1080 (full HD)"),
    (2560, 1920, "2560×1920 (S=4)"),
    (3840, 2160, "3840×2160 (4K)"),
];

/// Runs the render-memory measurement.
pub fn run(root: &Path) -> Result<()> {
    let data_dir = root.join("max-original");
    let config = AppConfig {
        max_res: data_dir.join("MAX.RES"),
        color_pal: data_dir.join("COLOR.PAL"),
        patches_res: root
            .join("max-brh/crates/max_assets/tests/data")
            .join("PATCHES.RES"),
        world_dir: Some(data_dir),
        ..AppConfig::default()
    };

    let baseline = alloc::current();

    // 1. The working set exactly as the client loads it (headless).
    alloc::reset_peak();
    let started = Instant::now();
    let assets = Assets::load(&config).context("Assets::load failed")?;
    let assets_phase = alloc::Phase {
        label: "Assets::load (a match's working set)",
        before: baseline,
        after: alloc::current(),
        peak: alloc::peak(),
    };
    let assets_seconds = started.elapsed().as_secs_f64();

    // 2. The frame buffer the renderer draws into.
    let (frame_phase, _frame) = alloc::phase("frame RGBA 640×480", || {
        RgbaBuffer::new(FRAME_WIDTH, FRAME_HEIGHT)
    });

    // 3. A real 1:1 frame: terrain + every loaded sprite frame 0 + shadows.
    let (render_phase, render_stats) = alloc::phase("1:1 frame (terrain + sprites)", || {
        render_frames(&assets, FRAME_WIDTH, FRAME_HEIGHT, 64)
    });

    // 4. The same scene through the current renderer at S = 2 and S = 4.
    let mut scale_rows = Vec::new();
    for scale in SCALES {
        let width = FRAME_WIDTH * scale;
        let height = FRAME_HEIGHT * scale;
        let (phase, stats) = alloc::phase("HD scale frame", || {
            render_frames(&assets, width, height, 64 * scale)
        });
        scale_rows.push((scale, width, height, stats, phase));
    }

    // 5. Isolated components (decoded again, same functions the app uses).
    let (sprites_phase, sheets) = alloc::phase("unit sprite sheets (92 units)", || {
        decode_sprite_sheets(&assets.max_res).unwrap_or_else(|_| (0, 0, 0, Vec::new()))
    });
    let (sprite_units, sprite_frames, sprite_pixels, _kept_sheets) = sheets;

    let (terrain_phase, atlas) = alloc::phase("terrain atlas (one planet map)", || {
        TerrainAtlas::from_wrl(&assets.world)
    });
    let (tile_count, terrain_bytes, cells) = estimate_terrain_bytes(&atlas);

    let (ui_phase, ui) = alloc::phase("UI image library (simple + big)", || {
        decode_ui_images(&assets.max_res).unwrap_or_else(|_| (0, 0, Vec::new()))
    });
    let (ui_images, ui_pixels, _kept_images) = ui;

    print_header(root, &assets, &assets_phase, assets_seconds);
    print_frame_path(&frame_phase, &render_phase, &render_stats);
    print_scale_table(&scale_rows);
    print_working_set(
        &assets,
        &assets_phase,
        sprite_units,
        sprite_frames,
        sprite_pixels,
        sprites_phase.retained(),
        &terrain_phase,
        tile_count,
        terrain_bytes,
        cells,
    );
    print_ui_library(ui_images, ui_pixels, &ui_phase);
    print_gpu_table(&frame_phase);

    Ok(())
}

/// Result of a timed render loop.
struct RenderStats {
    cells: u32,
    sprites: usize,
    /// Median per-frame duration (robust against a single scheduler hiccup).
    median: f64,
    /// Fastest frame (the renderer's own cost without interference).
    best: f64,
}

/// Draws `TIMED_FRAMES` full frames (terrain + every sprite's frame 0) into a
/// `width × height` buffer, with `zoom` (64 — 1:1).
fn render_frames(assets: &Assets, width: u32, height: u32, zoom: u32) -> RenderStats {
    let mut buffer = RgbaBuffer::new(width, height);
    let mut viewport = Viewport::with_center(width, height, (224, 224), 64);
    viewport.set_zoom(zoom);

    let renderer = SpriteRenderer::new(
        &assets.palette,
        &assets.brightness,
        ColorLut::identity(),
        FULL_BRIGHTNESS_LEVEL,
    );

    let mut cells = 0;
    let mut drawn = 0;
    let mut samples = Vec::with_capacity(TIMED_FRAMES);

    for _ in 0..TIMED_FRAMES {
        buffer.fill(Rgba::TRANSPARENT);
        let started = Instant::now();
        cells = assets.terrain.render_into(&mut buffer, &viewport);

        drawn = 0;
        for (index, sprite) in assets.sprites().iter().enumerate() {
            let x = (32 + (index as i32 % 8) * 72) * zoom as i32 / 64;
            let y = (32 + (index as i32 / 8) * 72) * zoom as i32 / 64;
            if let Some(body) = sprite.frame_at(0) {
                renderer.draw_unit_scaled(
                    &mut buffer,
                    body,
                    sprite.shadow_at(0),
                    (x, y),
                    (0, 0),
                    zoom,
                );
                drawn += 1;
            }
        }
        samples.push(started.elapsed().as_secs_f64());
    }

    samples.sort_by(|left, right| left.total_cmp(right));

    RenderStats {
        cells,
        sprites: drawn,
        median: samples[samples.len() / 2],
        best: samples[0],
    }
}

fn print_header(root: &Path, assets: &Assets, phase: &alloc::Phase, seconds: f64) {
    println!("## R0 render memory, current 640×480 mode");
    println!();
    println!("Root: `{}`", root.display());
    println!(
        "Planet: `{:?}`, world: `{}` ({}×{} cells), tile bank: {} tiles.",
        assets.planet,
        assets.world_name,
        assets.world.width(),
        assets.world.height(),
        assets.world.tile_count()
    );
    println!(
        "`{}`: retained {} in {:.2} s (peak +{}).",
        phase.label,
        alloc::human(phase.retained()),
        seconds,
        alloc::human(phase.peak_extra())
    );
    println!();
}

fn print_frame_path(frame_phase: &alloc::Phase, render_phase: &alloc::Phase, stats: &RenderStats) {
    println!("### 1. Frame path (per frame)");
    println!();
    println!("| buffer | size |");
    println!("|---|---:|");
    println!(
        "| frame RGBA 640×480 (`RgbaBuffer`) | {} ({} B) |",
        alloc::human(frame_phase.retained()),
        frame_phase.retained()
    );
    println!(
        "| SDL streaming texture (RGBA32 640×480, recreated per `present`) | {} B |",
        FRAME_WIDTH as usize * FRAME_HEIGHT as usize * 4
    );
    println!();
    println!(
        "Frame content: {} terrain cells, {} sprites (body + shadow); CPU {:.2} ms/frame \
         (median; best {:.2} ms → {:.0} fps ceiling, single-threaded, 1:1).",
        stats.cells,
        stats.sprites,
        stats.median * 1000.0,
        stats.best * 1000.0,
        1.0 / stats.best
    );
    println!(
        "Retained by the render pass itself: {} (the frame buffer is reused; drawing allocates nothing).",
        alloc::human(render_phase.retained())
    );
    println!();
}

fn print_scale_table(rows: &[(u32, u32, u32, RenderStats, alloc::Phase)]) {
    println!("### 2. Cost of the same scene at S = 1, 2, 4 (current CPU renderer)");
    println!();
    println!("| S | logical frame | RGBA frame | median ms | best ms | fps (best) | ×1:1 |");
    println!("|---:|---|---:|---:|---:|---:|---:|");
    let base = rows.first().map_or(1.0, |row| row.3.best.max(1e-9));

    for (scale, width, height, stats, _phase) in rows {
        let frame_bytes = *width as usize * *height as usize * 4;

        println!(
            "| {} | {}×{} | {} | {:.2} | {:.2} | {:.0} | {:.1}× |",
            scale,
            width,
            height,
            alloc::human(frame_bytes),
            stats.median * 1000.0,
            stats.best * 1000.0,
            1.0 / stats.best,
            stats.best / base
        );
    }
    println!();
    println!(
        "The current renderer is per-pixel on the CPU: the cost grows with the frame area \
         (S²) because every pixel of every tile and sprite is written individually."
    );
    println!();
}

#[allow(clippy::too_many_arguments)]
fn print_working_set(
    assets: &Assets,
    assets_phase: &alloc::Phase,
    sprite_units: usize,
    sprite_frames: usize,
    sprite_pixels: u64,
    sprite_bytes: usize,
    terrain_phase: &alloc::Phase,
    tile_count: usize,
    _terrain_bytes: usize,
    cells: usize,
) {
    let archives =
        assets.max_res.data().len() + assets.patches_res.data().len() + assets.world.data().len();
    let terrain = terrain_phase.retained();
    let total = assets_phase.retained();
    let other = total.saturating_sub(archives + sprite_bytes + terrain);

    println!("### 3. Render working set (CPU heap, measured)");
    println!();
    println!("| component | bytes | share |");
    println!("|---|---:|---:|");
    println!(
        "| MAX.RES + PATCHES.RES + WRL bytes kept resident | {} | {:.0}% |",
        alloc::human(archives),
        percent(archives, total)
    );
    println!(
        "| unit sprite sheets ({} units, {} frames, RLE runs) | {} | {:.0}% |",
        sprite_units,
        sprite_frames,
        alloc::human(sprite_bytes),
        percent(sprite_bytes, total)
    );
    println!(
        "| terrain atlas ({} tiles + {} cell ids + palette) | {} | {:.0}% |",
        tile_count,
        cells,
        alloc::human(terrain),
        percent(terrain, total)
    );
    println!(
        "| UI/menus/HUD images, cursors, portrait FLCs, audio hooks (remainder) | {} | {:.0}% |",
        alloc::human(other),
        percent(other, total)
    );
    println!(
        "| **total `{}`** | **{}** | 100% |",
        assets_phase.label,
        alloc::human(total)
    );
    println!();
    println!(
        "Sprite frame area is {} (RGBA ×1 {}); the sheets cost {} because `SpriteFrame` \
         stores RLE runs (`SpriteRow`/`SpriteRun`), not full pixel planes.",
        sprite_pixels,
        alloc::human(sprite_pixels as usize * 4),
        alloc::human(sprite_bytes)
    );
    println!(
        "Terrain atlas indexed bytes: {} tiles × 64×64 + {} × 2 B ids + 1 KiB palette.",
        tile_count, cells
    );
    println!();
}

fn print_ui_library(images: usize, pixels: u64, phase: &alloc::Phase) {
    println!("### 4. UI image library (all `simple` + `big` records, decoded)");
    println!();
    println!("| images | pixels | RGBA ×1 | decoded heap |");
    println!("|---:|---:|---:|---:|");
    println!(
        "| {} | {} | {} | {} |",
        images,
        pixels,
        alloc::human(pixels as usize * 4),
        alloc::human(phase.retained())
    );
    println!();
    println!(
        "This is the whole interface/menu/picture set, not the subset a single screen holds — \
         it is the upper bound an HD cache must fit (or stream) once every screen is HD."
    );
    println!();
}

fn print_gpu_table(frame_phase: &alloc::Phase) {
    println!("### 5. GPU side (computed from the exact SDL calls)");
    println!();
    println!("| drawable | drawable RGBA | frame texture | GPU total |");
    println!("|---|---:|---:|---:|");
    for (width, height, label) in DRAWABLES {
        let drawable = width as usize * height as usize * 4;

        println!(
            "| {} | {} | {} | {} |",
            label,
            alloc::human(drawable),
            alloc::human(frame_phase.retained()),
            alloc::human(drawable + frame_phase.retained())
        );
    }
    println!();
    println!(
        "`SdlWindow::present` recreates the frame texture on every frame \
         (`create_texture_streaming`), so exactly one 640×480 RGBA texture is resident."
    );
    println!();
}

fn percent(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

/// A decoded sprite sheet with its optional shadow sheet, kept alive so the
/// heap measurement sees it.
type SpriteSheets = Vec<(SpriteSheet, Option<ShadowSheet>)>;

/// Decoded UI images, kept alive so the heap measurement sees them.
type UiImages = Vec<Box<dyn std::any::Any>>;

/// Decodes every unit sprite sheet exactly like `Assets::load` does and returns
/// `(units, frames, frame pixels, sheets kept alive for the measurement)`.
fn decode_sprite_sheets(
    archive: &ResArchive,
) -> Result<(usize, usize, u64, SpriteSheets), max_assets::sprite::SpriteError> {
    let mut units = 0;
    let mut frames = 0;
    let mut pixels = 0_u64;
    let mut kept: SpriteSheets = Vec::new();

    for (_, sprite_name, shadow_name) in UNIT_SPRITES {
        let Ok(bytes) = archive.read(sprite_name) else {
            continue;
        };
        let Ok(sheet) = decode_sprite(bytes) else {
            continue;
        };

        frames += sheet.len();
        pixels += sheet_pixel_area(&sheet);
        units += 1;

        let shadow = shadow_name
            .and_then(|name| archive.read(name).ok())
            .and_then(|bytes| decode_shadow(bytes).ok());

        kept.push((sheet, shadow));
    }

    Ok((units, frames, pixels, kept))
}

fn sheet_pixel_area(sheet: &SpriteSheet) -> u64 {
    (0..sheet.len())
        .filter_map(|index| sheet.frame(index))
        .map(|frame| u64::from(frame.width) * u64::from(frame.height))
        .sum()
}

/// (tiles, indexed bytes, cells) — the atlas layout is `tile_count × 64×64`
/// + `width × height` tile ids + a 256-entry RGBA palette.
fn estimate_terrain_bytes(atlas: &TerrainAtlas) -> (usize, usize, usize) {
    let tiles = atlas.tile_count() as usize;
    let (width, height) = atlas.size();
    let cells = width as usize * height as usize;

    (tiles, tiles * 64 * 64 + cells * 2 + 256 * 4, cells)
}

/// Decodes every `simple`/`big` image (the UI/menus/HUD library) and returns
/// `(images, pixels, images kept alive for the measurement)`.
fn decode_ui_images(archive: &ResArchive) -> Result<(usize, u64, UiImages)> {
    let infos = classify_archive(archive);
    let mut images = 0;
    let mut pixels = 0_u64;
    let mut kept: UiImages = Vec::new();

    for info in &infos {
        let Ok(bytes) = archive.read(&info.name) else {
            continue;
        };

        match info.kind {
            Kind::Simple => {
                if let Ok(image) = SimpleImage::decode(bytes) {
                    pixels += u64::from(image.width as u16) * u64::from(image.height as u16);
                    images += 1;
                    kept.push(Box::new(image));
                }
            }
            Kind::Big => {
                if let Ok(image) = BigImage::decode(bytes) {
                    pixels += u64::from(image.width as u16) * u64::from(image.height as u16);
                    images += 1;
                    kept.push(Box::new(image));
                }
            }
            _ => {}
        }
    }

    Ok((images, pixels, kept))
}
