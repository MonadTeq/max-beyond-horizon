//! Encoding RGBA8 pixels to `.ktx2` with the `ktx-rs` encoder (a pure-Rust port of
//! Khronos KTX-Software / libktx), and the directory-level driver that turns a source
//! directory into `.ktx2` files plus [`crate::manifest`].
//!
//! The encoder call mirrors the one measured in R0 (`r0-measure/src/transcode.rs`):
//! `Texture2::create` + `compress_basis_ex` + `write_to_memory`, ETC1S or UASTC LDR 4x4.
//! The one deliberate difference is `thread_count = 1`: the gate requires the output to
//! be byte-identical between runs, and a single job-pool thread removes scheduling from
//! the equation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ktx::texture2::Texture2;
use ktx::{TextureCreateInfo, TextureCreateStorage, VkFormat};

use crate::hash::sha256_hex;
use crate::manifest::{AssetEntry, Encoder, Manifest, SourceInfo, Variant};
use crate::sources::{self, Source};

/// `ktx-rs` version this tool encodes with (must match `Cargo.toml`).
pub const KTX_RS_VERSION: &str = "4.4.4";

/// ETC1S quality level: the endpoint/selector cluster budget (0 = encoder default).
pub const QUALITY_LEVEL: u32 = 128;

/// UASTC LDR 4x4 pack flags — the default level, favouring UASTC error.
pub const UASTC_FLAGS: u32 = ktx::encode_params::KTX_PACK_UASTC_LEVEL_DEFAULT
    | ktx::encode_params::KTX_PACK_UASTC_FAVOR_UASTC_ERROR;

/// Job-pool threads. One: a deterministic encoder must not depend on scheduling.
pub const THREAD_COUNT: u32 = 1;

/// The source codecs the remaster pipeline produces for every asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// ETC1S + BasisLZ (small; the R0 recommendation and the primary codec).
    Etc1s,
    /// UASTC LDR 4x4 (8 bpp, high quality).
    Uastc,
}

impl Codec {
    /// Both codecs, in a fixed order.
    pub const ALL: [Self; 2] = [Self::Etc1s, Self::Uastc];

    /// File-name and manifest key of the codec.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Etc1s => "etc1s",
            Self::Uastc => "uastc",
        }
    }

    /// Human-readable name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Etc1s => "ETC1S",
            Self::Uastc => "UASTC",
        }
    }

    /// Parse the `--primary` argument.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "etc1s" => Some(Self::Etc1s),
            "uastc" => Some(Self::Uastc),
            _ => None,
        }
    }

    /// `ktxBasisCodecE` value passed to the encoder.
    fn basis_codec(self) -> u32 {
        match self {
            Self::Etc1s => ktx::encode_params::KTX_BASIS_CODEC_ETC1S,
            Self::Uastc => ktx::encode_params::KTX_BASIS_CODEC_UASTC_LDR_4X4,
        }
    }
}

/// Encodes one RGBA8 image to `.ktx2` bytes.
///
/// Every parameter is a crate constant, so the same `(pixels, codec)` always produces
/// the same bytes on the same machine with the same pinned dependency versions.
pub fn encode_rgba(rgba: &[u8], width: u32, height: u32, codec: Codec) -> Result<Vec<u8>> {
    anyhow::ensure!(width > 0 && height > 0, "cannot encode an empty image");

    let expected = width as usize * height as usize * 4;
    anyhow::ensure!(
        rgba.len() == expected,
        "RGBA buffer is {} bytes, expected {expected} for {width}x{height}",
        rgba.len()
    );

    let info = TextureCreateInfo {
        gl_internalformat: 0,
        vk_format: VkFormat::R8G8B8A8_UNORM,
        dfd: None,
        base_width: width,
        base_height: height,
        base_depth: 1,
        num_dimensions: 2,
        num_levels: 1,
        num_layers: 1,
        num_faces: 1,
        is_array: false,
        generate_mipmaps: false,
    };

    let mut texture = Texture2::create(&info, TextureCreateStorage::AllocStorage)
        .with_context(|| format!("Texture2::create failed for {width}x{height}"))?;

    let data = texture
        .get_data_mut()
        .context("created texture has no storage")?;
    anyhow::ensure!(
        data.len() == rgba.len(),
        "texture storage {} != RGBA {}",
        data.len(),
        rgba.len()
    );
    data.copy_from_slice(rgba);

    let params = ktx::encode_params::BasisParams {
        // The port validates this against `sizeof(ktxBasisParams)`, like the C API.
        struct_size: ktx::encode_params::KTX_BASIS_PARAMS_SIZE,
        codec: codec.basis_codec(),
        thread_count: THREAD_COUNT,
        quality_level: QUALITY_LEVEL,
        uastc_flags: UASTC_FLAGS,
        ..Default::default()
    };

    texture.compress_basis_ex(&params).with_context(|| {
        format!(
            "compress_basis_ex failed for {} {width}x{height}",
            codec.label()
        )
    })?;

    texture
        .write_to_memory()
        .with_context(|| format!("write_to_memory failed for {}", codec.label()))
}

/// One row of the tool's stdout table.
#[derive(Debug, Clone)]
pub struct EncodedAsset {
    /// Manifest tag.
    pub tag: String,
    /// Source size in pixels.
    pub width: u32,
    /// Source size in pixels.
    pub height: u32,
    /// Presentation scale.
    pub scale: u32,
    /// `etc1s` / `uastc` -> file record.
    pub variants: BTreeMap<String, Variant>,
}

/// What a run produced.
#[derive(Debug, Clone)]
pub struct RunSummary {
    /// Manifest path.
    pub manifest: PathBuf,
    /// SHA-256 of the manifest file bytes (`hex`) — the cheapest determinism check.
    pub manifest_sha256: String,
    /// One row per tag, sorted by tag.
    pub assets: Vec<EncodedAsset>,
    /// Total bytes of all generated `.ktx2` files.
    pub total_bytes: u64,
}

/// Encodes every source in `source_dir` into `output_dir` and writes
/// `remaster-manifest.json` next to the `.ktx2` files.
///
/// With `verify`, every file is encoded twice and the two byte strings are compared —
/// an in-process determinism self-check. The gate itself still compares two full CLI
/// runs on disk.
///
/// Stale `.ktx2` files in `output_dir` are removed first, so the directory always
/// matches the source directory (no leftovers from a renamed or deleted source).
pub fn run(
    source_dir: &Path,
    output_dir: &Path,
    primary: Codec,
    verify: bool,
) -> Result<RunSummary> {
    let sources = sources::discover(source_dir)?;

    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("cannot create {}", output_dir.display()))?;
    remove_stale_ktx2(output_dir)?;

    let mut assets = Vec::with_capacity(sources.len());
    let mut manifest_assets = BTreeMap::new();
    let mut total_bytes = 0_u64;

    for source in &sources {
        let mut variants = BTreeMap::new();

        for codec in Codec::ALL {
            let bytes = encode_rgba(&source.rgba, source.width, source.height, codec)
                .with_context(|| format!("cannot encode `{}` as {}", source.tag, codec.label()))?;

            if verify {
                let again = encode_rgba(&source.rgba, source.width, source.height, codec)
                    .with_context(|| {
                        format!("cannot re-encode `{}` as {}", source.tag, codec.label())
                    })?;
                anyhow::ensure!(
                    bytes == again,
                    "non-deterministic encoder: `{}` / {} differs between two in-process runs",
                    source.tag,
                    codec.label()
                );
            }

            let file_name = format!("{}.{}.ktx2", source.tag, codec.slug());
            write_file(&output_dir.join(&file_name), &bytes)?;
            total_bytes += bytes.len() as u64;

            variants.insert(
                codec.slug().to_string(),
                Variant {
                    path: file_name,
                    bytes: bytes.len() as u64,
                    sha256: sha256_hex(&bytes),
                },
            );
        }

        let primary_variant = variants
            .get(primary.slug())
            .with_context(|| format!("no `{}` variant for `{}`", primary.slug(), source.tag))?;
        let entry = AssetEntry {
            path: primary_variant.path.clone(),
            scale: source.scale,
            hash: format!("sha256:{}", primary_variant.sha256),
            source: source_info(source),
            variants: variants.clone(),
        };

        anyhow::ensure!(
            manifest_assets.insert(source.tag.clone(), entry).is_none(),
            "duplicate tag `{}`",
            source.tag
        );
        assets.push(EncodedAsset {
            tag: source.tag.clone(),
            width: source.width,
            height: source.height,
            scale: source.scale,
            variants,
        });
    }

    let manifest = Manifest::new(Encoder::new(primary), manifest_assets);
    let manifest_path = manifest.write(output_dir)?;
    let manifest_bytes = std::fs::read(&manifest_path)
        .with_context(|| format!("cannot read back {}", manifest_path.display()))?;

    Ok(RunSummary {
        manifest: manifest_path,
        manifest_sha256: sha256_hex(&manifest_bytes),
        assets,
        total_bytes,
    })
}

fn source_info(source: &Source) -> SourceInfo {
    SourceInfo {
        path: source.file_name.clone(),
        width: source.width,
        height: source.height,
        sha256: source.sha256.clone(),
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

/// Removes `*.ktx2` files left over by an earlier run; the manifest is overwritten.
fn remove_stale_ktx2(output_dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(output_dir)
        .with_context(|| format!("cannot list {}", output_dir.display()))?
    {
        let path = entry?.path();

        if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ktx2"))
        {
            std::fs::remove_file(&path)
                .with_context(|| format!("cannot remove stale {}", path.display()))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x4 red/green checkerboard: enough structure for both encoders.
    fn checkerboard(width: u32, height: u32) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);

        for y in 0..height {
            for x in 0..width {
                let odd = (x + y) % 2 == 1;
                rgba.extend_from_slice(&[
                    if odd { 220 } else { 10 },
                    40,
                    if odd { 30 } else { 200 },
                    255,
                ]);
            }
        }

        rgba
    }

    #[test]
    fn encoding_is_reproducible_in_process() {
        let rgba = checkerboard(16, 16);

        for codec in Codec::ALL {
            let first = encode_rgba(&rgba, 16, 16, codec).unwrap();
            let second = encode_rgba(&rgba, 16, 16, codec).unwrap();

            assert_eq!(first, second, "{} is not deterministic", codec.label());
            assert!(
                first.starts_with(&[0xAB, 0x4B, 0x54, 0x58]),
                "{} is not a KTX2 file",
                codec.label()
            );
            assert!(first.len() > 80);
        }
    }

    #[test]
    fn the_two_codecs_produce_different_files() {
        let rgba = checkerboard(16, 16);

        assert_ne!(
            encode_rgba(&rgba, 16, 16, Codec::Etc1s).unwrap(),
            encode_rgba(&rgba, 16, 16, Codec::Uastc).unwrap()
        );
    }

    #[test]
    fn a_different_image_produces_a_different_file() {
        let first = checkerboard(16, 16);
        // A structural change (a solid blue quadrant), not a one-LSB edit: ETC1S is
        // lossy and is allowed to quantize a single altered texel away — the source
        // SHA-256 in the manifest is what makes even that edit visible.
        let mut second = first.clone();
        for y in 0..8 {
            for x in 0..8 {
                let at = (y * 16 + x) * 4;
                second[at..at + 4].copy_from_slice(&[0, 0, 255, 255]);
            }
        }

        assert_ne!(
            encode_rgba(&first, 16, 16, Codec::Etc1s).unwrap(),
            encode_rgba(&second, 16, 16, Codec::Etc1s).unwrap()
        );
        assert_ne!(
            encode_rgba(&first, 16, 16, Codec::Uastc).unwrap(),
            encode_rgba(&second, 16, 16, Codec::Uastc).unwrap()
        );
    }

    #[test]
    fn a_wrong_buffer_length_is_rejected() {
        assert!(encode_rgba(&[0; 15], 4, 4, Codec::Etc1s).is_err());
        assert!(encode_rgba(&[], 0, 0, Codec::Etc1s).is_err());
    }
}
