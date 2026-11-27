//! Integration tests of the HD remaster loader through the public API
//! (`max-brh-docs/development/test-organization.md`: unit → `src/`, integration
//! → `tests/`).
//!
//! The sample `.ktx2` files in `tests/data/remaster/` are the R0 fixtures
//! (`build/r0-ktx2/`), and their decoded RGBA8 must match the C reference
//! byte-for-byte: the FNV-1a 64 goldens below come from
//! `max-brh-docs/development/r0-measurements.md` §3.3.

use std::path::{Path, PathBuf};

use max_assets::remaster::{RemasterCache, RemasterManifest, RemasterTexture};

fn data_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

fn remaster_root() -> PathBuf {
    data_path("remaster")
}

/// FNV-1a 64 — the same hash the R0 C-reference harness prints over the decoded
/// RGBA8, so the pure-Rust decoder is compared byte-for-byte (not pixel-approx).
fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

fn assert_tile(texture: &RemasterTexture) {
    assert_eq!(texture.width(), 64, "sample tile is 64x64");
    assert_eq!(texture.height(), 64, "sample tile is 64x64");
    assert_eq!(
        texture.pixels().len(),
        64 * 64 * 4,
        "RGBA8 is exactly width * height * 4 bytes"
    );
    assert_eq!(texture.scale, 2, "fixture manifest declares scale 2");
    assert_eq!(texture.palette.as_deref(), Some("GREEN_1"));
    assert_eq!(texture.recolor, None);
    assert_eq!(texture.shadow, None);
}

#[test]
fn etc1s_sample_matches_c_reference() {
    let cache = RemasterCache::open(remaster_root());
    let texture = cache
        .resolve("terrain_tile_etc1s")
        .expect("the sample ETC1S asset must resolve");

    assert_tile(&texture);
    // R0 §3.3: terrain tile 64x64, ETC1S — all three decoders agree on FNV-1a 64.
    assert_eq!(
        fnv1a64(texture.pixels()),
        0x959e_7da5_fdd4_b62d,
        "decoded RGBA8 must match the R0 C reference byte-for-byte"
    );
}

#[test]
fn uastc_sample_matches_c_reference() {
    let cache = RemasterCache::open(remaster_root());
    let texture = cache
        .resolve("terrain_tile_uastc")
        .expect("the sample UASTC asset must resolve");

    assert_tile(&texture);
    // R0 §3.3: terrain tile 64x64, UASTC.
    assert_eq!(
        fnv1a64(texture.pixels()),
        0x981a_030d_bd13_bfb7,
        "decoded RGBA8 must match the R0 C reference byte-for-byte"
    );
}

#[test]
fn unknown_tag_resolves_to_none() {
    let cache = RemasterCache::open(remaster_root());

    assert_eq!(cache.resolve("NO_SUCH_TAG"), None);
    assert_eq!(cache.resolve(""), None);
}

#[test]
fn missing_file_resolves_to_none() {
    // The fixture manifest lists `missing_file` → `does_not_exist.ktx2`.
    let cache = RemasterCache::open(remaster_root());

    assert_eq!(
        cache.resolve("missing_file"),
        None,
        "a listed but missing `.ktx2` must fall back, not panic"
    );
}

#[test]
fn missing_manifest_resolves_to_none() {
    // `tests/data/` has no `remaster-manifest.json`: the cache must open empty.
    let cache = RemasterCache::open(data_path("."));
    assert!(cache.manifest().is_empty());
    assert_eq!(cache.resolve("terrain_tile_etc1s"), None);
}

#[test]
fn corrupt_ktx2_resolves_to_none() {
    // Point a manifest entry at an existing non-Basis file (PATCHES.RES): the
    // transcode fails and `resolve` falls back instead of panicking.
    let manifest = RemasterManifest::parse(
        r#"{"assets":{"corrupt":{"path":"PATCHES.RES","scale":2}}}"#,
    )
    .unwrap();
    let cache = RemasterCache::with_manifest(data_path("."), manifest);

    assert_eq!(cache.resolve("corrupt"), None);
}
