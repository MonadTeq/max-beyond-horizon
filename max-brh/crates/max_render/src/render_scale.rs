//! Presentation scale `S` (`max-brh-docs/development/remaster-hd.md` §2.2, R3).
//!
//! The simulation, the camera and every UI coordinate stay in the original
//! 640×480 space for every `S`. The scale only changes the *render target*:
//!
//! * [`RenderScale::PixelPerfect`] — `S = 1`, a 640×480 frame, the byte-for-byte
//!   path of the original game (default; the golden 1:1 references are drawn in
//!   this mode);
//! * [`RenderScale::Hd2`] / [`RenderScale::Hd4`] — `S = 2` / `S = 4`, a
//!   1280×960 / 2560×1920 frame with HD textures.
//!
//! # Coordinate chain
//!
//! ```text
//! window points -> output pixels -> render frame (640·S × 480·S) -> / S -> logical 640×480
//! ```
//!
//! [`crate::presentation::ScreenGeometry`] owns the chain: the integer window
//! scale is computed from the render frame, and a mouse point is divided by `S`
//! after the reverse window transform, so clicks and UI hit tests never see the
//! scaled space.
//!
//! # Filtering
//!
//! The filter is global per mode (guardrail 5 of the remaster plan): nearest
//! neighbour in [`RenderScale::PixelPerfect`], linear in the HD modes. The pure
//! tile sampler is not affected — it keeps the exact `Gfx_DecodeMapTile`
//! coordinate mapping; the filter belongs to the final texture upload in the
//! SDL layer.

use crate::frame_scale::{FRAME_HEIGHT, FRAME_WIDTH};

/// Scale of the byte-for-byte presentation (`S = 1`).
pub const PIXEL_PERFECT_SCALE: u32 = 1;
/// First HD scale (`S = 2`).
pub const HD_SCALE_X2: u32 = 2;
/// Second HD scale (`S = 4`).
pub const HD_SCALE_X4: u32 = 4;
/// Every presentation scale the remaster ships (`×2` and `×4`, owner decision
/// of 2026-11-26; `×3` is not an asset scale and is rejected by
/// [`RenderScale::from_scale`]).
pub const HD_SCALES: [u32; 2] = [HD_SCALE_X2, HD_SCALE_X4];

/// Texture filtering of the final frame upload.
///
/// A single global choice per presentation mode, never per asset: the remaster
/// guardrails forbid mixing nearest and linear inside one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FilterMode {
    /// Nearest neighbour — every logical pixel is an exact block; the
    /// PixelPerfect path.
    #[default]
    Nearest,
    /// Linear interpolation — the HD path (zoom of the frame away from 1:1).
    Linear,
}

/// Presentation mode: pixel-perfect `S = 1` or HD `S ∈ {2, 4}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RenderScale {
    /// `S = 1`: the original 640×480 frame, byte-for-byte.
    #[default]
    PixelPerfect,
    /// `S = 2`: a 1280×960 render frame.
    Hd2,
    /// `S = 4`: a 2560×1920 render frame.
    Hd4,
}

impl RenderScale {
    /// The multiplier `S`.
    pub const fn scale(self) -> u32 {
        match self {
            Self::PixelPerfect => PIXEL_PERFECT_SCALE,
            Self::Hd2 => HD_SCALE_X2,
            Self::Hd4 => HD_SCALE_X4,
        }
    }

    /// Scale from a raw multiplier; `None` for an unsupported `S` (e.g. 3).
    pub const fn from_scale(scale: u32) -> Option<Self> {
        match scale {
            PIXEL_PERFECT_SCALE => Some(Self::PixelPerfect),
            HD_SCALE_X2 => Some(Self::Hd2),
            HD_SCALE_X4 => Some(Self::Hd4),
            _ => None,
        }
    }

    /// The HD modes (`S > 1`).
    pub const fn is_hd(self) -> bool {
        matches!(self, Self::Hd2 | Self::Hd4)
    }

    /// The HD multiplier, or `None` in [`Self::PixelPerfect`].
    pub const fn hd_scale(self) -> Option<u32> {
        match self {
            Self::PixelPerfect => None,
            Self::Hd2 => Some(HD_SCALE_X2),
            Self::Hd4 => Some(HD_SCALE_X4),
        }
    }

    /// Filter of the final frame upload for this mode.
    pub const fn filter(self) -> FilterMode {
        if self.is_hd() {
            FilterMode::Linear
        } else {
            FilterMode::Nearest
        }
    }

    /// Logical frame size in original pixels: always 640×480.
    pub const fn frame_size(self) -> (u32, u32) {
        (FRAME_WIDTH, FRAME_HEIGHT)
    }

    /// Render frame size in pixels: `640·S × 480·S`.
    pub const fn render_size(self) -> (u32, u32) {
        (FRAME_WIDTH * self.scale(), FRAME_HEIGHT * self.scale())
    }

    /// Size of one map tile at this scale (`64·S`): the expected size of a
    /// pre-baked HD terrain texture.
    pub const fn tile_pixels(self) -> u32 {
        crate::viewport::MAP_TILE_PIXELS as u32 * self.scale()
    }

    /// Name of the mode for logs/diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::PixelPerfect => "pixel-perfect",
            Self::Hd2 => "hd-2x",
            Self::Hd4 => "hd-4x",
        }
    }
}

/// Presentation mode — which pipeline a frame is drawn through (§R7).
///
/// The remaster is a switch, not a replacement: the historical 1:1 renderer
/// stays the default and must remain byte-for-byte, while the HD pipeline is
/// opt-in and every asset of it falls back to the historical path.
///
/// * [`PresentationMode::PixelPerfect`] — `S = 1`, the original 640×480 frame,
///   the golden concept of the project;
/// * [`PresentationMode::Hd`] — `S ∈ {2, 4}`, the `640·S × 480·S` render frame
///   with tiles/sprites/chrome/fonts resolved from the remaster layers.
///
/// The multiplier itself is a [`RenderScale`]; the mode only says whether the
/// HD layers are consulted at all. In `PixelPerfect` nothing in the HD layer is
/// even loaded ([`PresentationMode::render_scale`] returns
/// [`RenderScale::PixelPerfect`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PresentationMode {
    /// `S = 1`: the historical byte-for-byte presentation (default).
    #[default]
    PixelPerfect,
    /// `S > 1`: the HD presentation over the `640·S × 480·S` render frame.
    Hd,
}

/// Why a `--hd-scale` value was rejected ([`PresentationMode::from_flags`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationModeError {
    /// The scale is neither 1 nor a shipped HD multiplier
    /// ([`HD_SCALES`]).
    UnsupportedScale(u32),
}

impl PresentationModeError {
    /// The rejected multiplier.
    pub const fn scale(self) -> u32 {
        match self {
            Self::UnsupportedScale(scale) => scale,
        }
    }
}

impl std::fmt::Display for PresentationModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedScale(scale) => write!(
                f,
                "unsupported presentation scale {scale}: the remaster ships ×2 and ×4 \
                 (a bypass asset is not authored for another multiplier)"
            ),
        }
    }
}

impl std::error::Error for PresentationModeError {}

impl PresentationMode {
    /// The HD scale `--hd` selects when no explicit `--hd-scale` is given.
    ///
    /// R0 measured `S = 2` as the largest multiplier the CPU renderer can carry
    /// (~47 fps, ~105 MiB working set; `r0-measurements.md` §1.3/§2.1), so it is
    /// the default HD mode.
    pub const DEFAULT_HD_SCALE: RenderScale = RenderScale::Hd2;

    /// The mode is HD (`S > 1`).
    pub const fn is_hd(self) -> bool {
        matches!(self, Self::Hd)
    }

    /// Name of the mode for logs/diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::PixelPerfect => "pixel-perfect",
            Self::Hd => "hd",
        }
    }

    /// The render scale of the mode: [`RenderScale::PixelPerfect`] for
    /// [`Self::PixelPerfect`], the requested HD scale otherwise.
    pub const fn render_scale(self, hd_scale: RenderScale) -> RenderScale {
        match self {
            Self::PixelPerfect => RenderScale::PixelPerfect,
            Self::Hd => hd_scale,
        }
    }

    /// The mode and the render scale for the CLI flags `--hd` / `--hd-scale N`.
    ///
    /// `--hd-scale` implies `--hd` (there is no reason to name a multiplier
    /// without asking for HD); `--hd` alone takes
    /// [`Self::DEFAULT_HD_SCALE`]. A scale of 1 is the pixel-perfect mode — the
    /// historical pipeline must stay reachable through the same switch — while
    /// any other unsupported value (`3`, `8`, …) is an error: a tag, and an
    /// asset, exists only for the shipped multipliers (R0 fork 3).
    pub fn from_flags(
        hd: bool,
        hd_scale: Option<u32>,
    ) -> Result<(Self, RenderScale), PresentationModeError> {
        let Some(scale) = hd_scale else {
            return if hd {
                Ok((Self::Hd, Self::DEFAULT_HD_SCALE))
            } else {
                Ok((Self::PixelPerfect, RenderScale::PixelPerfect))
            };
        };
        let Some(render_scale) = RenderScale::from_scale(scale) else {
            return Err(PresentationModeError::UnsupportedScale(scale));
        };

        if render_scale.is_hd() {
            Ok((Self::Hd, render_scale))
        } else {
            Ok((Self::PixelPerfect, RenderScale::PixelPerfect))
        }
    }
}

#[cfg(test)]
#[path = "render_scale_tests.rs"]
mod tests;
