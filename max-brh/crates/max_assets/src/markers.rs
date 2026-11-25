//! Resource markers on the map — `RAWMSK*`/`FUELMK*`/`GOLDMK*` and the small
//! `SMRWMK*`/`SMFLMK*`/`SMGDMK*` from `MAX.RES`.
//!
//! Oracle — `max-cpp-port/src/survey.cpp` (`Survey_RenderMarker`):
//!
//! ```cpp
//! resource_value = min(material_type & CARGO_MASK, 16);
//! if (Gfx_ZoomLevel > 16) marker = RAWMSK0 + resource_value;   // large 62×62
//! else                    marker = SMRWMK1 + resource_value - 1; // small 14×16
//! ```
//!
//! Large markers are simple images with hotspot `(31,31)` (center of the cell), small ones —
//! `(7,8)`. At a zoom other than 1:1 C++ scales the picture (`WindowManager_
//! RescaleSimpleImage`), the renderer does that. `I_RAW`/`I_FUEL`/`I_GOLD` are interface
//! icons (statistics/menu), and **not** map markers; here they are kept only
//! as a fallback if the sprite marker is missing from the archive.

use crate::image::{ImageError, SimpleImage};
use crate::res::ResArchive;
use max_types::map::ResourceKind;

/// Large markers per type: `*MK0..*MK16` (0 — an empty cell).
pub const BIG_MARKER_COUNT: usize = 17;
/// Small markers per type: `SM*MK1..SM*MK16`.
pub const SMALL_MARKER_COUNT: usize = 16;

/// Indices of the resource types in the marker tables.
const RAW: usize = 0;
const FUEL: usize = 1;
const GOLD: usize = 2;

fn kind_index(kind: ResourceKind) -> usize {
    match kind {
        ResourceKind::Raw => RAW,
        ResourceKind::Fuel => FUEL,
        ResourceKind::Gold => GOLD,
    }
}

/// Loaded resource markers.
#[derive(Debug, Clone, Default)]
pub struct ResourceMarkers {
    big: [Vec<SimpleImage>; 3],
    small: [Vec<SimpleImage>; 3],
    /// `I_RAW`/`I_FUEL`/`I_GOLD` — fallback if there are no markers.
    icons: [Option<SimpleImage>; 3],
    loaded: usize,
}

impl ResourceMarkers {
    /// Loads all markers present in the archive.
    pub fn load(archive: &ResArchive) -> Self {
        let read = |name: &str| -> Option<SimpleImage> {
            archive
                .read(name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
        };

        let mut markers = Self::default();

        for (index, prefix) in [(RAW, "RAWMSK"), (FUEL, "FUELMK"), (GOLD, "GOLDMK")] {
            for frame in 0..BIG_MARKER_COUNT {
                if let Some(image) = read(&format!("{prefix}{frame}")) {
                    markers.big[index].push(image);
                    markers.loaded += 1;
                }
            }
        }

        for (index, prefix) in [(RAW, "SMRWMK"), (FUEL, "SMFLMK"), (GOLD, "SMGDMK")] {
            for frame in 1..=SMALL_MARKER_COUNT {
                if let Some(image) = read(&format!("{prefix}{frame}")) {
                    markers.small[index].push(image);
                    markers.loaded += 1;
                }
            }
        }

        for (index, name) in [(RAW, "I_RAW"), (FUEL, "I_FUEL"), (GOLD, "I_GOLD")] {
            markers.icons[index] = read(name);
        }

        markers
    }

    /// How many marker images were loaded.
    pub fn loaded(&self) -> usize {
        self.loaded
    }

    /// Large marker for the amount `amount` (C++: `min(amount, 16)`).
    pub fn big(&self, kind: ResourceKind, amount: u8) -> Option<&SimpleImage> {
        let amount = usize::from(amount.min(16));
        let table = &self.big[kind_index(kind)];

        // If part of the frames is missing from the archive, the nearest smaller index is a reasonable
        // fallback (the reference MAX.RES has all 17).
        table.get(amount).or_else(|| table.first())
    }

    /// Small marker for the amount `amount` (C++: frame `amount - 1`, 1..=16).
    pub fn small(&self, kind: ResourceKind, amount: u8) -> Option<&SimpleImage> {
        let amount = amount.clamp(1, 16);
        let table = &self.small[kind_index(kind)];
        let index = usize::from(amount - 1);

        table.get(index).or_else(|| table.first())
    }

    /// `I_*` interface icon (fallback).
    pub fn icon(&self, kind: ResourceKind) -> Option<&SimpleImage> {
        self.icons[kind_index(kind)].as_ref()
    }

    /// Whether there is at least one marker/icon for the map.
    pub fn is_ready(&self) -> bool {
        self.loaded > 0 || self.icons.iter().any(Option::is_some)
    }
}

/// Parses one marker image (for tests/reports).
pub fn decode_marker(bytes: &[u8]) -> Result<SimpleImage, ImageError> {
    SimpleImage::decode(bytes)
}

#[cfg(test)]
#[path = "markers_tests.rs"]
mod tests;
