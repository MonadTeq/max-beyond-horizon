//! HD remaster asset loader (`max-brh-docs/development/remaster-hd.md` §R1).
//!
//! The remaster is presentation-only: it never imports or mutates `max_core`
//! (and `max_assets` itself has no `max_core` dependency, phase 5.2). The
//! asset pipeline ships derived `.ktx2` textures plus a
//! `remaster-manifest.json` into a remaster root (`target/remaster/`); this
//! module reads that directory and, for any tag the manifest maps, transcodes
//! the KTX2 payload to RGBA8 through the pure-Rust `basisu` transcoder
//! (selected in R0).
//!
//! Fallback is per-asset: a missing manifest, an unknown tag, a missing file,
//! or an undecodable `.ktx2` all make [`RemasterCache::resolve`] return `None`,
//! so the caller keeps the original `MAX.RES` path. Nothing here panics on a
//! missing remaster asset.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use basisu::{DecodeFlags, TargetFormat, Transcoder};
use serde::{Deserialize, Serialize};

/// Name of the remaster manifest file in the remaster root.
pub const MANIFEST_FILE: &str = "remaster-manifest.json";
/// Current manifest schema version.
pub const MANIFEST_VERSION: u32 = 1;
/// Bytes per RGBA8 texel — the only target the SDL3 `Canvas` backend consumes (R0).
pub const RGBA_BYTES_PER_PIXEL: usize = 4;

/// Manifest tag of a pre-baked HD terrain tile (`remaster-hd.md` §R3).
///
/// Terrain tiles are the one asset class with no team colours, no brightness
/// levels and no palette cycles: the `.WRL` palette is baked into the pixels, so
/// exactly one RGBA texture is needed per `(map, tile_id)` pair. R3 derives the
/// tag from the map name (`GREEN_1`), the tile identifier and the presentation
/// scale, e.g. `terrain_tile_GREEN_1_000_x2`.
///
/// The scale is part of the tag because one tag maps to one asset in the
/// manifest (R2: `tile@4x.png` → tag `tile`, `scale = 4`): a ×2 and a ×4 bank
/// for the same tile must be separate entries.
///
/// The tag is a plain manifest key and part of the `.ktx2` file name, so it
/// contains only ASCII letters, digits and underscores.
pub fn terrain_tile_tag(world: &str, tile_id: u16, scale: u32) -> String {
    format!("terrain_tile_{world}_{tile_id:03}_x{scale}")
}

/// Remaster asset error, returned as a value (never a panic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemasterError {
    /// The manifest is not valid JSON or does not match the schema.
    InvalidManifest(String),
    /// The `.ktx2` payload is not a decodable Basis container, or the
    /// `(source, target)` pair is unsupported by the transcoder.
    Transcode(String),
    /// The file system refused a read (missing file, permissions, ...).
    Io(String),
}

impl std::fmt::Display for RemasterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(detail) => write!(f, "invalid remaster manifest: {detail}"),
            Self::Transcode(detail) => write!(f, "KTX2 transcode failed: {detail}"),
            Self::Io(detail) => write!(f, "cannot read remaster asset: {detail}"),
        }
    }
}

impl std::error::Error for RemasterError {}

/// One entry of the remaster manifest: how to substitute an original asset tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemasterEntry {
    /// `.ktx2` path relative to the remaster root.
    pub path: String,
    /// Presentation scale the asset was authored for (1, 2, 4).
    #[serde(default = "default_scale")]
    pub scale: u32,
    /// Palette tag for recolor semantics (refined in R3/R4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    /// Recolor model tag (index-domain HD + palette LUT; refined in R4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recolor: Option<String>,
    /// Shadow mask tag (`S_*`), if the asset has a separate shadow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<String>,
    /// Content hash of the `.ktx2` file (integrity; enforced in R2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

fn default_scale() -> u32 {
    1
}

/// The parsed `remaster-manifest.json` (tag → [`RemasterEntry`]).
///
/// Unknown JSON fields are ignored (forward-compatible: R2–R4 may extend the
/// schema without invalidating older loaders).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemasterManifest {
    /// Schema version (reserved for forward-compatible evolution).
    #[serde(default = "default_version")]
    pub version: u32,
    /// Tag → remaster asset entry.
    #[serde(default)]
    pub assets: BTreeMap<String, RemasterEntry>,
}

fn default_version() -> u32 {
    MANIFEST_VERSION
}

impl RemasterManifest {
    /// Parse a manifest from JSON text.
    pub fn parse(json: &str) -> Result<Self, RemasterError> {
        serde_json::from_str(json).map_err(|error| {
            RemasterError::InvalidManifest(error.to_string())
        })
    }

    /// Read and parse a manifest file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, RemasterError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .map_err(|error| RemasterError::Io(error.to_string()))?;
        let text = String::from_utf8(bytes).map_err(|error| {
            RemasterError::InvalidManifest(error.to_string())
        })?;

        Self::parse(&text)
    }

    /// The entry mapped to `tag`, if the manifest lists it.
    pub fn get(&self, tag: &str) -> Option<&RemasterEntry> {
        self.assets.get(tag)
    }

    /// Whether the manifest lists no assets.
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }
}

/// A decoded remaster texture: RGBA8 pixels plus the manifest metadata the
/// presentation layer needs to substitute it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemasterTexture {
    /// Presentation scale the texture was authored for.
    pub scale: u32,
    /// Whether the payload carries an alpha channel.
    pub has_alpha: bool,
    /// Palette tag for recolor semantics (if any).
    pub palette: Option<String>,
    /// Recolor model tag (if any).
    pub recolor: Option<String>,
    /// Shadow mask tag (if any).
    pub shadow: Option<String>,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl RemasterTexture {
    /// Width in texels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in texels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Decoded RGBA8 pixels, row-major: exactly `width * height * 4` bytes.
    pub fn pixels(&self) -> &[u8] {
        &self.rgba
    }
}

/// In-memory cache of a remaster asset root.
#[derive(Debug, Clone)]
pub struct RemasterCache {
    /// Root directory of the remaster assets (`target/remaster/`).
    root: PathBuf,
    /// The parsed manifest; empty when the manifest is missing/unreadable.
    manifest: RemasterManifest,
}

impl RemasterCache {
    /// Open the remaster asset root. Never fails: a missing or unreadable
    /// manifest yields an empty cache, so every [`Self::resolve`] returns `None`.
    pub fn open(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let manifest = RemasterManifest::load(root.join(MANIFEST_FILE)).unwrap_or_else(|_| {
            RemasterManifest {
                version: MANIFEST_VERSION,
                assets: BTreeMap::new(),
            }
        });

        Self { root, manifest }
    }

    /// Build a cache from an already-parsed manifest (tests, or callers that
    /// load the manifest themselves).
    pub fn with_manifest(root: impl AsRef<Path>, manifest: RemasterManifest) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            manifest,
        }
    }

    /// The remaster asset root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The parsed manifest (possibly empty).
    pub fn manifest(&self) -> &RemasterManifest {
        &self.manifest
    }

    /// Resolve `tag` to a decoded RGBA8 texture, or `None` when the remaster
    /// has no such asset — the caller then falls back to the original
    /// `MAX.RES` path. Never panics on a missing manifest/entry/file or an
    /// undecodable payload.
    pub fn resolve(&self, tag: &str) -> Option<RemasterTexture> {
        let entry = self.manifest.get(tag)?;
        let bytes = std::fs::read(self.root.join(&entry.path)).ok()?;

        decode_ktx2(&bytes, entry).ok()
    }
}

/// Transcodes a `.ktx2` payload to RGBA8 and attaches the entry metadata.
fn decode_ktx2(bytes: &[u8], entry: &RemasterEntry) -> Result<RemasterTexture, RemasterError> {
    // `basisu::Error` has no `std::error::Error` impl, so it is stringified
    // (its `Debug` is stable) instead of chained.
    let transcoder = Transcoder::new(bytes)
        .map_err(|error| RemasterError::Transcode(format!("{error:?}")))?;
    let (width, height) = transcoder.base_dimensions();
    let rgba = transcoder
        .transcode(0, TargetFormat::Rgba32, DecodeFlags::NONE)
        .map_err(|error| RemasterError::Transcode(format!("{error:?}")))?;

    Ok(RemasterTexture {
        scale: entry.scale,
        has_alpha: transcoder.has_alpha(),
        palette: entry.palette.clone(),
        recolor: entry.recolor.clone(),
        shadow: entry.shadow.clone(),
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
#[path = "remaster_tests.rs"]
mod tests;
