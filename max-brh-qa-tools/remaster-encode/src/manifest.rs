//! `remaster-manifest.json`: the schema the R1 loader reads, plus the R2 build-time
//! detail.
//!
//! The loader (`max_assets::remaster::RemasterManifest`) deserializes `{ "version": N,
//! "assets": { tag: { "path", "scale", "hash" } } }` and ignores unknown fields, so the
//! encoder emits exactly those keys and adds the rest (`encoder`, `source`, `variants`,
//! `width`/`height`) alongside them. `tests/manifest_compat.rs` parses a generated
//! manifest with the real loader, so the two cannot drift silently.
//!
//! `assets[tag].hash` follows the loader's convention (`sha256:<hex>`); the `sha256`
//! fields of `source`/`variants` are the bare hex digest. `variants` lists both codecs
//! because R2 produces ETC1S and UASTC for every source, while `path`/`hash` point at
//! the primary one the runtime loads.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

/// Name of the manifest file in the remaster root.
pub const MANIFEST_FILE: &str = "remaster-manifest.json";
/// Manifest schema version (must match `max_assets::remaster::MANIFEST_VERSION`).
pub const MANIFEST_VERSION: u32 = 1;

/// The whole manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Manifest {
    /// Schema version.
    pub version: u32,
    /// Exact encoder identity and knobs — everything needed to explain a difference.
    pub encoder: Encoder,
    /// Tag -> remaster asset, sorted (deterministic JSON key order).
    pub assets: BTreeMap<String, AssetEntry>,
}

/// Encoder identity and the knobs that affect the produced bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Encoder {
    /// Tool name.
    pub tool: String,
    /// Tool version (`CARGO_PKG_VERSION`).
    pub tool_version: String,
    /// Encoding library.
    pub library: String,
    /// Encoding library version, pinned in `Cargo.toml`/`Cargo.lock`.
    pub library_version: String,
    /// Job-pool threads (`1` — no scheduling-dependent output).
    pub thread_count: u32,
    /// ETC1S quality level.
    pub quality_level: u32,
    /// UASTC LDR 4x4 pack flags.
    pub uastc_flags: u32,
    /// Codec `assets[].path` points at.
    pub primary_codec: String,
}

impl Encoder {
    /// The encoder identity for a run whose primary codec is `primary`.
    pub fn new(primary: crate::encode::Codec) -> Self {
        Self {
            tool: env!("CARGO_PKG_NAME").to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            library: "ktx-rs".to_string(),
            library_version: crate::encode::KTX_RS_VERSION.to_string(),
            thread_count: crate::encode::THREAD_COUNT,
            quality_level: crate::encode::QUALITY_LEVEL,
            uastc_flags: crate::encode::UASTC_FLAGS,
            primary_codec: primary.slug().to_string(),
        }
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new(crate::encode::Codec::Etc1s)
    }
}

/// One tag's remaster asset: what the runtime loads plus the R2 build record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetEntry {
    /// `.ktx2` path relative to the remaster root (the primary codec).
    pub path: String,
    /// Presentation scale the source was authored for (1, 2, 4).
    pub scale: u32,
    /// Content hash of `path` (`sha256:<hex>`), the loader's convention.
    pub hash: String,
    /// The lossless source this asset was encoded from.
    pub source: SourceInfo,
    /// Every produced codec: `etc1s` / `uastc` -> file record.
    pub variants: BTreeMap<String, Variant>,
}

/// The lossless source a tag was encoded from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceInfo {
    /// Source file name (relative to the source directory).
    pub path: String,
    /// Source width in pixels.
    pub width: u32,
    /// Source height in pixels.
    pub height: u32,
    /// SHA-256 of the source file bytes (`hex`): a changed source changes this.
    pub sha256: String,
}

/// One encoded `.ktx2` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Variant {
    /// File name relative to the remaster root.
    pub path: String,
    /// File size in bytes.
    pub bytes: u64,
    /// SHA-256 of the file bytes (`hex`).
    pub sha256: String,
}

impl Manifest {
    /// Assemble a manifest from already encoded assets.
    pub fn new(encoder: Encoder, assets: BTreeMap<String, AssetEntry>) -> Self {
        Self {
            version: MANIFEST_VERSION,
            encoder,
            assets,
        }
    }

    /// Pretty JSON with a trailing newline — the exact bytes written to disk.
    pub fn to_json(&self) -> Result<String> {
        let mut json =
            serde_json::to_string_pretty(self).context("cannot serialize the manifest")?;
        json.push('\n');

        Ok(json)
    }

    /// Write the manifest into `directory`.
    pub fn write(&self, directory: &Path) -> Result<std::path::PathBuf> {
        let path = directory.join(MANIFEST_FILE);
        let json = self.to_json()?;
        std::fs::write(&path, json).with_context(|| format!("cannot write {}", path.display()))?;

        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> Manifest {
        let mut variants = BTreeMap::new();
        variants.insert(
            "etc1s".to_string(),
            Variant {
                path: "tile.etc1s.ktx2".to_string(),
                bytes: 1122,
                sha256: "aa".repeat(32),
            },
        );
        variants.insert(
            "uastc".to_string(),
            Variant {
                path: "tile.uastc.ktx2".to_string(),
                bytes: 4096,
                sha256: "bb".repeat(32),
            },
        );

        let mut assets = BTreeMap::new();
        assets.insert(
            "tile".to_string(),
            AssetEntry {
                path: "tile.etc1s.ktx2".to_string(),
                scale: 1,
                hash: format!("sha256:{}", "aa".repeat(32)),
                source: SourceInfo {
                    path: "tile.png".to_string(),
                    width: 64,
                    height: 64,
                    sha256: "cc".repeat(32),
                },
                variants,
            },
        );

        Manifest::new(Encoder::default(), assets)
    }

    #[test]
    fn json_is_stable_across_calls() {
        let manifest = sample_manifest();

        assert_eq!(manifest.to_json().unwrap(), manifest.to_json().unwrap());
    }

    #[test]
    fn json_ends_with_exactly_one_newline() {
        let json = sample_manifest().to_json().unwrap();

        assert!(json.ends_with("}\n"), "{json}");
        assert!(!json.ends_with("}\n\n"), "{json}");
    }

    #[test]
    fn version_and_assets_are_what_the_r1_loader_expects() {
        let manifest = sample_manifest();
        let parsed =
            max_assets::remaster::RemasterManifest::parse(&manifest.to_json().unwrap()).unwrap();

        assert_eq!(parsed.version, max_assets::remaster::MANIFEST_VERSION);
        assert_eq!(parsed.assets.len(), 1);

        let entry = parsed.get("tile").expect("tag `tile` must resolve");
        assert_eq!(entry.path, "tile.etc1s.ktx2");
        assert_eq!(entry.scale, 1);
        assert_eq!(
            entry.hash.as_deref(),
            Some(manifest.assets["tile"].hash.as_str())
        );
    }

    #[test]
    fn assets_are_sorted_by_tag() {
        let mut assets = BTreeMap::new();
        for tag in ["zeta", "alpha", "mid"] {
            assets.insert(
                tag.to_string(),
                AssetEntry {
                    path: format!("{tag}.etc1s.ktx2"),
                    scale: 1,
                    hash: format!("sha256:{}", "00".repeat(32)),
                    source: SourceInfo {
                        path: format!("{tag}.png"),
                        width: 1,
                        height: 1,
                        sha256: "11".repeat(32),
                    },
                    variants: BTreeMap::new(),
                },
            );
        }

        let json = Manifest::new(Encoder::default(), assets).to_json().unwrap();
        let alpha = json.find("\"alpha\"").unwrap();
        let mid = json.find("\"mid\"").unwrap();
        let zeta = json.find("\"zeta\"").unwrap();

        assert!(alpha < mid && mid < zeta, "{json}");
    }
}
