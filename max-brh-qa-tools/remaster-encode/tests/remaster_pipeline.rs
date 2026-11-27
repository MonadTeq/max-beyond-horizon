//! Integration test of the R2 pipeline through the public API: sources -> `.ktx2` +
//! manifest, twice, and back through the R1 loader (`max_assets::remaster`).
//!
//! What is asserted:
//!
//! * two encodes of the same sources are byte-identical, manifest included (the gate);
//! * a changed source changes its `source.sha256` and the encoded file hash;
//! * the generated manifest parses with the real runtime loader and `resolve` returns
//!   the expected texture size — i.e. the encoder cannot drift from what the game reads.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use max_assets::remaster::RemasterCache;
use remaster_encode::encode::{self, Codec};
use remaster_encode::hash::sha256_hex;

/// A throwaway directory removed on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

        let path = std::env::temp_dir().join(format!(
            "remaster-encode-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).unwrap();

        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A deterministic checkerboard with a red block in one corner.
fn checkerboard(width: u32, height: u32, red: u8) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);

    for y in 0..height {
        for x in 0..width {
            let odd = (x + y) % 2 == 1;
            let corner = x < width / 4 && y < height / 4;
            rgba.extend_from_slice(&[
                if corner {
                    red
                } else if odd {
                    220
                } else {
                    10
                },
                if corner { 0 } else { 40 },
                if corner {
                    0
                } else if odd {
                    30
                } else {
                    200
                },
                255,
            ]);
        }
    }

    rgba
}

/// Writes a PNG source into `directory`.
fn write_source(directory: &Path, file_name: &str, width: u32, height: u32, red: u8) {
    image::save_buffer_with_format(
        directory.join(file_name),
        &checkerboard(width, height, red),
        width,
        height,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .unwrap();
}

/// Every file of a directory: name -> bytes.
fn snapshot(directory: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().to_string(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect()
}

#[test]
fn two_encodes_are_byte_identical_and_the_manifest_is_loader_compatible() {
    let sources = TempDir::new("sources");
    let first = TempDir::new("run1");
    let second = TempDir::new("run2");

    write_source(&sources.path, "terrain_tile.png", 16, 16, 200);
    write_source(&sources.path, "terrain_tile_hd@4x.png", 32, 32, 40);

    let summary = encode::run(&sources.path, &first.path, Codec::Etc1s, true).unwrap();
    encode::run(&sources.path, &second.path, Codec::Etc1s, true).unwrap();

    let left = snapshot(&first.path);
    let right = snapshot(&second.path);

    assert_eq!(left, right, "two runs must be byte-identical");
    assert_eq!(
        left.len(),
        5,
        "2 tags x 2 codecs + the manifest: {:?}",
        left.keys().collect::<Vec<_>>()
    );
    assert!(left.contains_key("remaster-manifest.json"));

    // The loader reads what was written, and `assets[].path` is the primary codec.
    let cache = RemasterCache::open(&first.path);
    assert_eq!(cache.manifest().assets.len(), 2);

    let terrain = cache
        .resolve("terrain_tile")
        .expect("terrain_tile must resolve");
    assert_eq!((terrain.width(), terrain.height()), (16, 16));
    assert_eq!(terrain.pixels().len(), 16 * 16 * 4);
    assert_eq!(terrain.scale, 1);

    let hd = cache
        .resolve("terrain_tile_hd")
        .expect("terrain_tile_hd must resolve");
    assert_eq!(
        (hd.width(), hd.height()),
        (32, 32),
        "the @4x source is 4x bigger"
    );
    assert_eq!(hd.scale, 4);

    // The manifest hashes are the real file hashes, and the entry hash is the
    // loader's `sha256:<hex>` convention over the primary variant.
    let entry = cache.manifest().get("terrain_tile").unwrap();
    let primary_bytes = std::fs::read(first.path.join(&entry.path)).unwrap();
    let expected_hash = format!("sha256:{}", sha256_hex(&primary_bytes));
    assert_eq!(entry.hash.as_deref(), Some(expected_hash.as_str()));
    assert!(entry.path.ends_with(".etc1s.ktx2"), "{}", entry.path);

    assert!(
        summary
            .assets
            .iter()
            .any(|asset| asset.tag == "terrain_tile_hd" && asset.scale == 4 && asset.width == 32),
        "{:?}",
        summary.assets
    );

    // The UASTC variants are written too (R2 produces both codecs).
    assert!(left.contains_key("terrain_tile.uastc.ktx2"));
    assert!(left.contains_key("terrain_tile_hd.uastc.ktx2"));
}

#[test]
fn a_changed_source_changes_the_hashes() {
    let sources = TempDir::new("sources-changed");
    let run = TempDir::new("run-changed");

    write_source(&sources.path, "tile.png", 16, 16, 10);
    let before = encode::run(&sources.path, &run.path, Codec::Etc1s, false).unwrap();
    let manifest_before = std::fs::read(&before.manifest).unwrap();

    // Same file name, same size, one different pixel value.
    write_source(&sources.path, "tile.png", 16, 16, 200);
    let after = encode::run(&sources.path, &run.path, Codec::Etc1s, false).unwrap();
    let manifest_after = std::fs::read(&after.manifest).unwrap();

    let source_hash_before = before.assets[0].variants["etc1s"].sha256.clone();
    let source_hash_after = after.assets[0].variants["etc1s"].sha256.clone();

    assert_ne!(
        before.manifest_sha256, after.manifest_sha256,
        "manifest must change"
    );
    assert_ne!(
        source_hash_before, source_hash_after,
        "the encoded file hash must change with the source"
    );
    assert_ne!(manifest_before, manifest_after);
}

#[test]
fn both_codecs_are_written_and_the_primary_codec_is_selectable() {
    let sources = TempDir::new("sources-primary");
    let etc1s_run = TempDir::new("primary-etc1s");
    let uastc_run = TempDir::new("primary-uastc");

    write_source(&sources.path, "ui.png", 8, 8, 100);

    let etc1s = encode::run(&sources.path, &etc1s_run.path, Codec::Etc1s, false).unwrap();
    let uastc = encode::run(&sources.path, &uastc_run.path, Codec::Uastc, false).unwrap();

    let etc1s_manifest = RemasterCache::open(&etc1s_run.path);
    let uastc_manifest = RemasterCache::open(&uastc_run.path);

    assert_eq!(
        etc1s_manifest.manifest().get("ui").unwrap().path,
        "ui.etc1s.ktx2"
    );
    assert_eq!(
        uastc_manifest.manifest().get("ui").unwrap().path,
        "ui.uastc.ktx2"
    );
    assert_eq!(etc1s.assets[0].variants.len(), 2);
    assert_eq!(uastc.assets[0].variants.len(), 2);
    assert_ne!(etc1s.manifest_sha256, uastc.manifest_sha256);

    // Both codecs decode with the runtime transcoder (`basisu`), so neither produced
    // file is a container the game cannot read.
    let etc1s_texture = etc1s_manifest.resolve("ui").expect("ETC1S must decode");
    assert_eq!((etc1s_texture.width(), etc1s_texture.height()), (8, 8));
    let uastc_texture = uastc_manifest.resolve("ui").expect("UASTC must decode");
    assert_eq!((uastc_texture.width(), uastc_texture.height()), (8, 8));
}
