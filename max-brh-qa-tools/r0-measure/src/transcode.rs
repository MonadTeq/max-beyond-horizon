//! KTX2 → RGBA8 transcoder benchmark (R0, item 3).
//!
//! Two candidate run-time transcoders are measured on the same files:
//!
//! * `basisu` 0.1 — a pure-Rust port of the Basis Universal C++ transcoder
//!   (bit-exact, no C/C++, single dependency);
//! * `ktx-rs` 4.4.4 — a pure-Rust port of Khronos KTX-Software (`libktx`),
//!   the library the C `libktx` option would be built from.
//!
//! The test files are produced in-process by the `ktx-rs` encoder from real
//! game art (terrain tile, ×4 tile, `FRAMEPIC` chrome), in both source codecs
//! the pipeline plans to use — ETC1S (BasisLZ, small) and UASTC (high quality).
//! Everything is in memory: no file I/O and no CLI in the timed path, so the
//! numbers are the transcoder cost alone.
//!
//! Measured per (image, codec):
//!
//! * `.ktx2` size and bits per pixel (what ships in `target/remaster/`);
//! * cold start (parse container + first decode) — the cost of the first asset;
//! * warm decode (median of N runs, allocation included) — the load-time cost;
//! * byte equality of the two decoders' RGBA output.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use basisu::{DecodeFlags, TargetFormat, Transcoder};
use ktx::texture2::Texture2;
use ktx::TextureCreateInfo;
use ktx::{TextureCreateStorage, TranscodeFmt, VkFormat};
use max_assets::res::ResArchive;
use max_assets::sprite::decode_sprite;
use max_assets::wrl::WrlMap;
use max_render::palette::{BrightnessLut, ColorLut, Palette, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;
use max_render::terrain::palette_from_wrl;

/// One RGBA8 test image.
struct TestImage {
    name: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Source codecs the pipeline plans to choose between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    /// ETC1S + BasisLZ supercompression (small files, fast decode).
    Etc1s,
    /// UASTC LDR 4×4 (8 bpp, high quality, BC7/ASTC-class).
    Uastc,
}

impl Codec {
    fn label(self) -> &'static str {
        match self {
            Self::Etc1s => "ETC1S",
            Self::Uastc => "UASTC",
        }
    }
}

/// Result of one decoded file.
struct DecodeStats {
    cold: Duration,
    warm: Duration,
    output: Vec<u8>,
}

/// Runs the benchmark.
pub fn run(root: &Path, requested_iterations: Option<usize>, dump: Option<&Path>) -> Result<()> {
    let images = test_images(root)?;

    println!("## R0 transcoder benchmark (KTX2 → RGBA8)");
    println!();
    println!(
        "Files are encoded in-process by `ktx-rs` (port of KTX-Software) from real game art; \
         decoding is measured in memory, allocation included."
    );
    println!();

    let mut rows: Vec<Row> = Vec::new();

    for image in &images {
        for codec in [Codec::Etc1s, Codec::Uastc] {
            let (ktx2, encode_time) = encode(image, codec)?;
            let iterations =
                requested_iterations.unwrap_or_else(|| iterations_for(image.width, image.height));

            if let Some(dir) = dump {
                std::fs::create_dir_all(dir)?;
                let file_name = format!(
                    "{}_{}.ktx2",
                    image.name.replace([' ', '×'], "_"),
                    codec.label()
                );
                std::fs::write(dir.join(&file_name), &ktx2)
                    .with_context(|| format!("cannot write {}", dir.join(&file_name).display()))?;
            }

            let basis = decode_basisu(&ktx2, iterations)?;
            let ktx = decode_ktx_rs(&ktx2, iterations)?;

            let identical = basis.output == ktx.output;
            let differing = if identical {
                0
            } else {
                basis
                    .output
                    .iter()
                    .zip(ktx.output.iter())
                    .filter(|(left, right)| left != right)
                    .count()
            };

            rows.push(Row {
                image: image.name.clone(),
                width: image.width,
                height: image.height,
                codec,
                ktx2_bytes: ktx2.len(),
                encode_time,
                basisu_first: basis.cold,
                basisu_median: basis.warm,
                ktx_first: ktx.cold,
                ktx_median: ktx.warm,
                identical,
                differing,
                fnv: fnv1a(&basis.output),
                iterations,
            });
        }
    }

    print_table(&rows);
    print_summary(&rows);

    Ok(())
}

struct Row {
    image: String,
    width: u32,
    height: u32,
    codec: Codec,
    ktx2_bytes: usize,
    encode_time: Duration,
    basisu_first: Duration,
    basisu_median: Duration,
    ktx_first: Duration,
    ktx_median: Duration,
    identical: bool,
    differing: usize,
    fnv: u64,
    iterations: usize,
}

fn print_table(rows: &[Row]) {
    println!(
        "| image | codec | `.ktx2` | bpp | encode ms | basisu first ms | basisu median ms | ktx-rs first ms | ktx-rs median ms | RGBA identical | fnv |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|---|---|");

    for row in rows {
        let pixels = u64::from(row.width) * u64::from(row.height);

        println!(
            "| {} {}×{} | {} | {:.1} KiB | {:.2} | {:.1} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {:016x} |",
            row.image,
            row.width,
            row.height,
            row.codec.label(),
            row.ktx2_bytes as f64 / 1024.0,
            row.ktx2_bytes as f64 * 8.0 / pixels as f64,
            row.encode_time.as_secs_f64() * 1000.0,
            row.basisu_first.as_secs_f64() * 1000.0,
            row.basisu_median.as_secs_f64() * 1000.0,
            row.ktx_first.as_secs_f64() * 1000.0,
            row.ktx_median.as_secs_f64() * 1000.0,
            if row.identical {
                "yes".to_string()
            } else {
                format!("no ({} B differ)", row.differing)
            },
            row.fnv,
        );
    }

    println!();
    let runs = rows.first().map_or(0, |row| row.iterations);
    println!(
        "Each run is the whole load path (container parse + transcode), the way an asset \
         loader calls it; `first` is run 1, `median` is the median of {runs} runs (N from \
         the pixel count, `--iterations` overrides)."
    );
}

fn print_summary(rows: &[Row]) {
    let total_pixels: u64 = rows
        .iter()
        .filter(|row| row.codec == Codec::Etc1s)
        .map(|row| u64::from(row.width) * u64::from(row.height))
        .sum();

    let sum = |pick: fn(&Row) -> Duration| -> f64 {
        rows.iter()
            .filter(|row| row.codec == Codec::Etc1s)
            .map(|row| pick(row).as_secs_f64())
            .sum()
    };

    let basisu_total = sum(|row| row.basisu_median);
    let ktx_total = sum(|row| row.ktx_median);

    println!();
    println!("### Throughput (ETC1S files, median of the full load path)");
    println!();
    println!("| transcoder | total ms | MPix/s | ratio |",);
    println!("|---|---:|---:|---:|");
    println!(
        "| basisu (pure Rust) | {:.2} | {:.1} | 1.00× |",
        basisu_total * 1000.0,
        total_pixels as f64 / 1e6 / basisu_total
    );
    println!(
        "| ktx-rs (libktx port, pure Rust) | {:.2} | {:.1} | {:.2}× |",
        ktx_total * 1000.0,
        total_pixels as f64 / 1e6 / ktx_total,
        ktx_total / basisu_total
    );
    println!();
    println!(
        "All decoders agree byte for byte on every file: {}.",
        if rows.iter().all(|row| row.identical) {
            "yes"
        } else {
            "NO — see the table"
        }
    );
    println!();
    println!("Each decode allocates exactly one RGBA buffer (`w × h × 4`), so the output size is the texture's VRAM footprint.");
}

/// More pixels — fewer iterations, so the run stays under a few seconds.
fn iterations_for(width: u32, height: u32) -> usize {
    let pixels = u64::from(width) * u64::from(height);
    let target = 200_000_000_u64 / pixels.max(1);

    (target as usize).clamp(5, 300)
}

/// Encodes one RGBA image to `.ktx2` with the `ktx-rs` (KTX-Software port)
/// encoder.
fn encode(image: &TestImage, codec: Codec) -> Result<(Vec<u8>, Duration)> {
    let info = TextureCreateInfo {
        gl_internalformat: 0,
        vk_format: VkFormat::R8G8B8A8_UNORM,
        dfd: None,
        base_width: image.width,
        base_height: image.height,
        base_depth: 1,
        num_dimensions: 2,
        num_levels: 1,
        num_layers: 1,
        num_faces: 1,
        is_array: false,
        generate_mipmaps: false,
    };

    let started = Instant::now();
    let mut texture = Texture2::create(&info, TextureCreateStorage::AllocStorage)
        .with_context(|| format!("Texture2::create failed for {}", image.name))?;

    let data = texture
        .get_data_mut()
        .context("created texture has no storage")?;
    anyhow::ensure!(
        data.len() == image.rgba.len(),
        "texture storage {} != RGBA {}",
        data.len(),
        image.rgba.len()
    );
    data.copy_from_slice(&image.rgba);

    let params = ktx::encode_params::BasisParams {
        // The port validates this against `sizeof(ktxBasisParams)`, like the C API.
        struct_size: ktx::encode_params::KTX_BASIS_PARAMS_SIZE,
        codec: match codec {
            Codec::Etc1s => ktx::encode_params::KTX_BASIS_CODEC_ETC1S,
            Codec::Uastc => ktx::encode_params::KTX_BASIS_CODEC_UASTC_LDR_4X4,
        },
        // 0 — the encoder picks its own thread count; the benchmark measures
        // single-asset load-time decode, not encode throughput.
        thread_count: 0,
        quality_level: 128,
        uastc_flags: ktx::encode_params::KTX_PACK_UASTC_LEVEL_DEFAULT
            | ktx::encode_params::KTX_PACK_UASTC_FAVOR_UASTC_ERROR,
        ..Default::default()
    };

    texture
        .compress_basis_ex(&params)
        .with_context(|| format!("compress_basis_ex failed for {} / {:?}", image.name, codec))?;

    let bytes = texture
        .write_to_memory()
        .with_context(|| format!("write_to_memory failed for {}", image.name))?;

    Ok((bytes, started.elapsed()))
}

/// Median of the per-run durations, with the cold start measured separately.
fn measure(mut runs: usize, mut body: impl FnMut() -> Result<Vec<u8>>) -> Result<DecodeStats> {
    runs = runs.max(3);

    let cold_started = Instant::now();
    let output = body()?;
    let cold = cold_started.elapsed();

    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let started = Instant::now();
        let _ = body()?;
        samples.push(started.elapsed());
    }
    samples.sort();

    Ok(DecodeStats {
        cold,
        warm: samples[samples.len() / 2],
        output,
    })
}

/// Decodes with the `basisu` crate.
fn decode_basisu(bytes: &[u8], iterations: usize) -> Result<DecodeStats> {
    measure(iterations, || {
        // `basisu::Error` is a plain enum without a `std::error::Error` impl,
        // so it is stringified instead of chained.
        let texture = Transcoder::new(bytes).map_err(|error| anyhow!("basisu parse: {error:?}"))?;
        texture
            .transcode(0, TargetFormat::Rgba32, DecodeFlags::NONE)
            .map_err(|error| anyhow!("basisu transcode: {error:?}"))
    })
}

/// Decodes with the `ktx-rs` crate (KTX-Software port).
fn decode_ktx_rs(bytes: &[u8], iterations: usize) -> Result<DecodeStats> {
    measure(iterations, || {
        let mut texture =
            Texture2::create_from_memory(bytes, ktx::KTX_TEXTURE_CREATE_LOAD_IMAGE_DATA_BIT)
                .context("ktx-rs: cannot parse")?;
        texture
            .transcode_basis(TranscodeFmt::RGBA32, 0)
            .context("ktx-rs: transcode_basis failed")?;

        let offset = texture.level_data_offset(0) as usize;
        let data = texture.get_data().context("ktx-rs: no image data")?;
        let slice = data
            .get(offset..)
            .context("ktx-rs: level offset outside the image data")?;

        Ok(slice.to_vec())
    })
}

/// Builds the RGBA test images from the original game data.
fn test_images(root: &Path) -> Result<Vec<TestImage>> {
    let data_dir = root.join("max-original");
    let mut images = Vec::new();

    // 1. A real terrain tile (64×64) and its ×4 HD variant (256×256),
    //    the shape most HD work will be made of.
    let wrl_bytes = std::fs::read(data_dir.join("GREEN_1.WRL")).context("GREEN_1.WRL")?;
    let map = WrlMap::parse(&wrl_bytes).context("cannot parse GREEN_1.WRL")?;
    let tile = map.tile_pixels(0).context("map has no tiles")?;
    let palette = palette_from_wrl(map.palette());
    let tile_rgba = tile_rgba(tile, &palette);

    images.push(TestImage {
        name: "terrain tile".to_string(),
        width: 64,
        height: 64,
        rgba: tile_rgba.clone(),
    });
    images.push(TestImage {
        name: "terrain tile ×4".to_string(),
        width: 256,
        height: 256,
        rgba: upscale(&tile_rgba, 64, 64, 256, 256),
    });

    // 2. The full-screen window chrome (FRAMEPIC 640×480, own palette): the
    //    largest single UI texture of the remaster.
    let archive = ResArchive::open(data_dir.join("MAX.RES")).context("MAX.RES")?;
    let frame_pic = archive.read("FRAMEPIC")?;
    let big = max_assets::image::BigImage::decode(frame_pic).context("FRAMEPIC decode")?;
    let width = big.width as u32;
    let height = big.height as u32;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let rgb = big.rgb(x, y).unwrap_or([0, 0, 0]);
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    images.push(TestImage {
        name: "FRAMEPIC chrome".to_string(),
        width,
        height,
        rgba,
    });

    // 3. A unit sprite frame: the smallest, most numerous texture class.
    let palette_bytes = std::fs::read(data_dir.join("COLOR.PAL")).context("COLOR.PAL")?;
    let color_palette = Palette::from_color_pal(&palette_bytes).context("COLOR.PAL parse")?;
    let brightness = BrightnessLut::build(&color_palette);
    let tank = decode_sprite(archive.read("TANK")?).context("TANK sprite decode")?;
    let frame = tank.frame(0).context("TANK has no frames")?;
    let renderer = SpriteRenderer::new(
        &color_palette,
        &brightness,
        ColorLut::identity(),
        FULL_BRIGHTNESS_LEVEL,
    );
    let buffer = renderer.render_frame(frame);

    images.push(TestImage {
        name: "TANK frame".to_string(),
        width: buffer.width(),
        height: buffer.height(),
        rgba: buffer.pixels().to_vec(),
    });

    Ok(images)
}

fn tile_rgba(tile: &[u8], palette: &max_render::terrain::PaletteRgba) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(tile.len() * 4);

    for index in tile {
        let entry = palette[*index as usize];
        rgba.extend_from_slice(&entry);
    }

    rgba
}

/// FNV-1a 64-bit over the decoded RGBA — the same hash the C reference
/// harness prints, so all three decoders can be compared byte for byte.
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

/// Nearest-neighbour upscale of an RGBA image (the ×4 HD variant of a tile).
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
