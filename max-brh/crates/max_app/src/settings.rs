//! `settings.jsonc` — the window/display settings file of the client.
//!
//! The file is JSON with `//` and `/* … */` comments and trailing commas
//! allowed (JSONC); every section and parameter is documented with a comment so
//! the file itself is the reference. The parser is a two-step pipeline:
//!
//! 1. [`strip_jsonc`] removes comments and trailing commas **in place** (bytes
//!    are replaced by spaces, newlines are preserved), so a `serde_json` error
//!    still points at the original line/column;
//! 2. the cleaned text is deserialized into strictly typed sections; unknown
//!    keys are rejected, so a typo cannot silently fall back to a default.
//!
//! # Sections
//!
//! * `display` — the presentation resolution of the SDL window:
//!   `scale` (`1`/`2`/`4`) and/or `resolution` (`"640x480"`, `"1280x960"`,
//!   `"2560x1920"`). The two are aliases; naming both with different values is
//!   an error. `aa` (`"off"`, `"fxaa"`, `"smaa"`, `gpu-aa-design.md`) selects
//!   the GPU anti-aliasing pass of the output layer — it is not a render
//!   multiplier, the frame stays `640·S`; `"off"` is the default, and any other
//!   value (including the retired SSAA numbers `2`/`4`) is an error. At `S = 1`
//!   the mode is forced to `Off` — the pixel-perfect frame stays byte-for-byte.
//! * `window` — the window presentation: `scale` (`0` — fit the desktop,
//!   otherwise `1..=8`) and `fullscreen` (`true`/`false`).
//!
//! # Priority
//!
//! **CLI > `settings.jsonc` > default** ([`resolve`]). A CLI presentation flag
//! (`--hd`/`--hd-scale`, `--aa`, `--scale`, `--fullscreen`) replaces the value
//! of the same parameter from the file; a parameter absent from the file keeps
//! the built-in default (`RenderScale::PixelPerfect`, `AaMode::Off`, auto window
//! scale, windowed).
//!
//! The file is read once at launch ([`Settings::load`]). The video rows of the
//! game's `Setup` dialog ([`crate::preferences`]) change the running presentation
//! only; they do not rewrite the file — the next launch takes the value from the
//! file (and the CLI) again.
//!
//! # Why this module is pure
//!
//! Everything except [`Settings::load`] is a function of the file text: the
//! precedence and the JSONC rules are unit-tested without a window, an SDL
//! context or a real user profile.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use max_render::frame_scale::MAX_SCALE;
use max_render::render_scale::{AaMode, PresentationMode, PresentationModeError, RenderScale};

/// One video setting as the `Setup` dialog edits it: the render scale and the
/// output-layer anti-aliasing mode.
///
/// A **presentation** choice, not a saved preference: `settings.jsonc` and the
/// CLI select the value at launch (`CLI > jsonc > default`), the dialog changes
/// the running window only and never rewrites the file, so the next launch is
/// back to the configured value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VideoChoice {
    /// The render scale (`640x480`, `1280x960`, `2560x1920`).
    pub render_scale: RenderScale,
    /// The output-layer anti-aliasing mode; always [`AaMode::Off`] at `S = 1`
    /// (the pixel-perfect frame has no HD geometry to smooth).
    pub aa: AaMode,
}

impl VideoChoice {
    /// A choice with the `S = 1` rule applied: anti-aliasing is forced off
    /// unless the scale is HD.
    pub const fn new(render_scale: RenderScale, aa: AaMode) -> Self {
        Self {
            render_scale,
            aa: if render_scale.is_hd() {
                aa
            } else {
                AaMode::Off
            },
        }
    }

    /// The same choice at another render scale; the anti-aliasing mode is kept
    /// only if the new scale can carry it (`S > 1`).
    pub const fn with_scale(self, render_scale: RenderScale) -> Self {
        Self::new(render_scale, self.aa)
    }

    /// The same choice with another anti-aliasing mode; `None` at `S = 1`, where
    /// the mode is forced to [`AaMode::Off`].
    pub const fn with_aa(self, aa: AaMode) -> Option<Self> {
        if self.render_scale.is_hd() {
            Some(Self {
                render_scale: self.render_scale,
                aa,
            })
        } else {
            None
        }
    }

    /// The presentation mode this choice resolves to (the window scale and
    /// fullscreen are not part of the dialog).
    pub const fn is_hd(self) -> bool {
        self.render_scale.is_hd()
    }

    /// The [`PresentationSettings`] of this choice, with the window defaults; the
    /// driver keeps its own window scale/fullscreen and only reads the mode, the
    /// render scale and `aa`.
    pub fn presentation(self) -> PresentationSettings {
        PresentationSettings {
            mode: if self.render_scale.is_hd() {
                PresentationMode::Hd
            } else {
                PresentationMode::PixelPerfect
            },
            render_scale: self.render_scale,
            aa: self.aa,
            window_scale: None,
            fullscreen: false,
        }
    }
}

/// File name of the client window settings.
pub const SETTINGS_FILE_NAME: &str = "settings.jsonc";

/// Last-resort settings path: `settings.jsonc` in the working directory.
///
/// [`resolve_path`] is what the launch uses; this name remains the built-in
/// default when no executable/catalog context is known, and `--settings PATH`
/// overrides both.
pub fn default_path() -> PathBuf {
    PathBuf::from(SETTINGS_FILE_NAME)
}

/// Resolves the launch `settings.jsonc` path (`distro-installers.md` §B).
///
/// Priority:
/// 1. the directory of the running executable — in the portable layout
///    `game.exe` and `settings.jsonc` share the catalog root;
/// 2. the game catalog root ([`crate::layout::Catalog`]) when the executable
///    lives in a subdirectory;
/// 3. only for a development checkout (`repository_root` is known): the
///    repository root and the working directory (the historical behavior);
/// 4. the built-in default ([`default_path`]).
///
/// An installed build therefore never reads a `settings.jsonc` from a foreign
/// working directory. A missing file is not an error: the caller applies the
/// built-in defaults. `--settings PATH` is handled by the caller and skips this
/// search.
pub fn resolve_path(
    exe: Option<&Path>,
    game_root: Option<&Path>,
    cwd: Option<&Path>,
    repository_root: Option<&Path>,
) -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    };

    if let Some(dir) = exe.and_then(Path::parent) {
        push(dir.join(SETTINGS_FILE_NAME));
    }

    if let Some(root) = game_root {
        push(root.join(SETTINGS_FILE_NAME));
    }

    // A development checkout: `cargo run` keeps the file in the repository root
    // (or next to the working directory), like before the portable layout.
    if let Some(root) = repository_root {
        push(root.join(SETTINGS_FILE_NAME));

        if let Some(cwd) = cwd {
            push(cwd.join(SETTINGS_FILE_NAME));
        }
    }

    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .or_else(|| candidates.into_iter().next())
        .unwrap_or_else(default_path)
}

/// The `display` section: which resolution the window renders at.
///
/// Both parameters select a [`RenderScale`] (`S = 1`, `2` or `4`); see
/// [`Settings::render_scale`] for the resolution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DisplaySettings {
    /// `display.scale` — the multiplier (`1`/`2`/`4`).
    pub scale: Option<RenderScale>,
    /// `display.resolution` — the render-frame size (`"1280x960"` → `S = 2`).
    pub resolution: Option<RenderScale>,
    /// `display.aa` — the GPU anti-aliasing mode (`off`/`fxaa`/`smaa`).
    pub aa: Option<AaMode>,
}

/// The `window` section: window scale and fullscreen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowSettings {
    /// `window.scale`: `None` — fit the desktop, otherwise `1..=8`.
    pub scale: Option<u32>,
    /// `window.fullscreen`; `None` — not named in the file.
    pub fullscreen: Option<bool>,
}

/// Parsed `settings.jsonc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    /// Display/presentation section.
    pub display: DisplaySettings,
    /// Window section.
    pub window: WindowSettings,
}

impl Settings {
    /// The presentation scale selected by `display`, or `PixelPerfect`.
    ///
    /// `scale` wins over the alias `resolution`; the parser already rejects a
    /// file that names both with different values ([`SettingsError::ConflictingDisplay`]).
    pub fn render_scale(&self) -> RenderScale {
        self.display
            .scale
            .or(self.display.resolution)
            .unwrap_or(RenderScale::PixelPerfect)
    }

    /// Parses JSONC text into [`Settings`].
    ///
    /// Comments and trailing commas are removed first; an empty file (or a file
    /// with comments only) is the default configuration.
    pub fn parse(text: &str) -> Result<Self, SettingsError> {
        let stripped = strip_jsonc(text);

        if stripped.trim().is_empty() {
            return Ok(Self::default());
        }

        let file: SettingsFile =
            serde_json::from_str(&stripped).map_err(|error| SettingsError::Syntax {
                line: error.line(),
                column: error.column(),
                message: error.to_string(),
            })?;

        file.into_settings()
    }

    /// Reads and parses `path`; `Ok(None)` when the file does not exist.
    ///
    /// A missing file is not an error (the built-in defaults apply). Any other
    /// I/O error or a parse/validation error is returned to the caller: a
    /// broken configuration must be visible at launch, not silently ignored.
    pub fn load(path: impl AsRef<Path>) -> Result<Option<Self>, SettingsError> {
        let path = path.as_ref();

        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).map(Some),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(SettingsError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// [`Self::load`] with the defaults for a missing file.
    pub fn load_or_default(path: impl AsRef<Path>) -> Result<Self, SettingsError> {
        Ok(Self::load(path)?.unwrap_or_default())
    }
}

/// The CLI presentation parameters that override `settings.jsonc`.
///
/// A direct mirror of `--hd`/`--hd-scale`/`--scale`/`--fullscreen` (the binary
/// builds it in `Cli::presentation_settings`), so the precedence rule is a pure
/// function and is tested without the CLI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CliPresentation {
    /// `--hd`.
    pub hd: bool,
    /// `--hd-scale N`.
    pub hd_scale: Option<u32>,
    /// `--aa off|fxaa|smaa`.
    pub aa: Option<AaMode>,
    /// `--scale N` (`0` — fit the desktop).
    pub window_scale: Option<u32>,
    /// `--fullscreen`.
    pub fullscreen: bool,
}

/// The effective presentation settings after applying the CLI over the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationSettings {
    /// Presentation mode (`PixelPerfect`/`Hd`).
    pub mode: PresentationMode,
    /// The render scale `S` (`1`/`2`/`4`).
    pub render_scale: RenderScale,
    /// The GPU anti-aliasing mode of the output layer (`Off` at `S = 1`).
    pub aa: AaMode,
    /// Window scale; `None` — fit the desktop.
    pub window_scale: Option<u32>,
    /// Borderless desktop fullscreen at launch.
    pub fullscreen: bool,
}

impl PresentationSettings {
    /// The presentation mode is HD (`S > 1`).
    pub const fn is_hd(self) -> bool {
        self.mode.is_hd()
    }
}

/// The effective presentation: `CLI > settings.jsonc > default`.
///
/// * a CLI presentation flag (`--hd`, `--hd-scale`) wins over `display`;
/// * `--aa` wins over `display.aa`; the mode is forced to [`AaMode::Off`] when
///   the resolved scale is `S = 1` — the pixel-perfect frame and its golden
///   stay byte-for-byte (`gpu-aa-design.md`);
/// * `--scale` wins over `window.scale` (`--scale 0` is "fit the desktop");
/// * `--fullscreen` wins over `window.fullscreen` (a bare `false` in the file is
///   the default and can only be turned on by the flag).
pub fn resolve(
    cli: CliPresentation,
    settings: &Settings,
) -> Result<PresentationSettings, SettingsError> {
    let render_scale = if cli.hd || cli.hd_scale.is_some() {
        PresentationMode::from_flags(cli.hd, cli.hd_scale)?.1
    } else {
        settings.render_scale()
    };
    let mode = if render_scale.is_hd() {
        PresentationMode::Hd
    } else {
        PresentationMode::PixelPerfect
    };
    // `S = 1` forces anti-aliasing off: a pixel-perfect frame has no HD geometry
    // to smooth and must stay byte-for-byte.
    let aa = if render_scale.is_hd() {
        cli.aa.or(settings.display.aa).unwrap_or(AaMode::Off)
    } else {
        AaMode::Off
    };
    let window_scale = match cli.window_scale {
        Some(0) => None,
        Some(value) => Some(value.clamp(1, MAX_SCALE)),
        None => settings.window.scale,
    };
    let fullscreen = cli.fullscreen || settings.window.fullscreen.unwrap_or(false);

    Ok(PresentationSettings {
        mode,
        render_scale,
        aa,
        window_scale,
        fullscreen,
    })
}

/// Settings file as deserialized from JSONC (raw values).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsFile {
    #[serde(default)]
    display: DisplayFile,
    #[serde(default)]
    window: WindowFile,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayFile {
    #[serde(default)]
    scale: Option<u32>,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    aa: Option<AaFileValue>,
}

/// `display.aa` as written in the file: a mode name, or a number from the
/// retired SSAA syntax (which is reported as an error, not accepted).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum AaFileValue {
    Name(String),
    Number(u32),
}

impl std::fmt::Display for AaFileValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name(value) => write!(f, "{value}"),
            Self::Number(value) => write!(f, "{value}"),
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowFile {
    #[serde(default)]
    scale: Option<u32>,
    #[serde(default)]
    fullscreen: Option<bool>,
}

impl SettingsFile {
    fn into_settings(self) -> Result<Settings, SettingsError> {
        let scale = self
            .display
            .scale
            .map(render_scale_from_multiplier)
            .transpose()?;
        let resolution = self
            .display
            .resolution
            .as_deref()
            .map(render_scale_from_resolution)
            .transpose()?;
        let aa = self
            .display
            .aa
            .map(|value| {
                AaMode::parse_flag(&value.to_string()).map_err(|_| SettingsError::UnsupportedAa {
                    value: value.to_string(),
                })
            })
            .transpose()?;

        if let (Some(scale), Some(resolution)) = (scale, resolution) {
            if scale != resolution {
                let scale_value = scale.scale();
                let resolution_value = self.display.resolution.clone().unwrap_or_default();

                return Err(SettingsError::ConflictingDisplay {
                    scale: scale_value,
                    resolution: resolution_value,
                });
            }
        }

        let window_scale = match self.window.scale {
            Some(value) => window_scale_from_file(value)?,
            None => None,
        };

        Ok(Settings {
            display: DisplaySettings {
                scale,
                resolution,
                aa,
            },
            window: WindowSettings {
                scale: window_scale,
                fullscreen: self.window.fullscreen,
            },
        })
    }
}

/// `display.scale` → [`RenderScale`] (`1`, `2`, `4`).
fn render_scale_from_multiplier(value: u32) -> Result<RenderScale, SettingsError> {
    RenderScale::from_scale(value).ok_or(SettingsError::UnsupportedScale { value })
}

/// `display.resolution` (`"WxH"`) → [`RenderScale`].
fn render_scale_from_resolution(value: &str) -> Result<RenderScale, SettingsError> {
    let unsupported = || SettingsError::UnsupportedResolution {
        value: value.to_string(),
    };
    let trimmed = value.trim();
    let (width, height) = trimmed
        .split_once(['x', 'X', '×'])
        .ok_or_else(unsupported)?;
    let width: u32 = width.trim().parse().map_err(|_| unsupported())?;
    let height: u32 = height.trim().parse().map_err(|_| unsupported())?;

    [
        RenderScale::PixelPerfect,
        RenderScale::Hd2,
        RenderScale::Hd4,
    ]
    .into_iter()
    .find(|scale| scale.render_size() == (width, height))
    .ok_or_else(unsupported)
}

/// `window.scale`: `0` — fit the desktop, otherwise `1..=MAX_SCALE`.
fn window_scale_from_file(value: u32) -> Result<Option<u32>, SettingsError> {
    match value {
        0 => Ok(None),
        1..=MAX_SCALE => Ok(Some(value)),
        value => Err(SettingsError::WindowScale {
            value,
            max: MAX_SCALE,
        }),
    }
}

/// `settings.jsonc` error.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// The file exists but cannot be read.
    #[error("cannot read settings file {path}: {source}")]
    Io {
        /// The path that was read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The cleaned JSONC text is not valid JSON.
    #[error("settings.jsonc is not valid JSONC at line {line}, column {column}: {message}")]
    Syntax {
        /// Line in the original file (comments are replaced in place).
        line: usize,
        /// Column in the original file.
        column: usize,
        /// `serde_json` message.
        message: String,
    },
    /// `display.scale` names an unsupported multiplier.
    #[error(
        "display.scale: unsupported multiplier {value}; supported values are 1 (640x480), \
         2 (1280x960) and 4 (2560x1920)"
    )]
    UnsupportedScale {
        /// The rejected value.
        value: u32,
    },
    /// `display.resolution` names an unsupported size.
    #[error(
        "display.resolution: unsupported resolution \"{value}\"; supported values are \
         640x480, 1280x960 and 2560x1920"
    )]
    UnsupportedResolution {
        /// The rejected value.
        value: String,
    },
    /// `display.scale` and `display.resolution` disagree.
    #[error(
        "display.scale = {scale} and display.resolution = \"{resolution}\" select different \
         resolutions; name only one of them"
    )]
    ConflictingDisplay {
        /// The multiplier of `display.scale`.
        scale: u32,
        /// The raw value of `display.resolution`.
        resolution: String,
    },
    /// `display.aa` names an unsupported anti-aliasing mode.
    #[error(
        "display.aa: unsupported antialiasing mode {value}; supported values are \"off\", \
         \"fxaa\" and \"smaa\" (the SSAA multipliers 2 and 4 are retired)"
    )]
    UnsupportedAa {
        /// The rejected value.
        value: String,
    },
    /// `window.scale` is outside `0..=MAX_SCALE`.
    #[error("window.scale: expected 1..={max} (or 0 to fit the desktop), got {value}")]
    WindowScale {
        /// The rejected value.
        value: u32,
        /// [`MAX_SCALE`].
        max: u32,
    },
    /// The CLI multiplier is unsupported (delegated to the presentation switch).
    #[error(transparent)]
    Presentation(#[from] PresentationModeError),
}

/// Removes JSONC comments and trailing commas, preserving the byte layout.
///
/// Bytes of a comment (or of a removed trailing comma) are replaced with spaces,
/// and line breaks are kept, so `serde_json` line/column numbers point into the
/// original file. String literals are respected: `//` inside `"http://…"` is
/// part of the value, not a comment.
pub fn strip_jsonc(text: &str) -> String {
    let source = text.strip_prefix('\u{feff}').unwrap_or(text);
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];

        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }

            index += 1;
            continue;
        }

        match byte {
            b'"' => {
                in_string = true;
                index += 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    out[index] = b' ';
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                out[index] = b' ';
                out[index + 1] = b' ';
                index += 2;

                while index < bytes.len() {
                    if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                        out[index] = b' ';
                        out[index + 1] = b' ';
                        index += 2;
                        break;
                    }

                    if bytes[index] != b'\n' {
                        out[index] = b' ';
                    }

                    index += 1;
                }
            }
            _ => index += 1,
        }
    }

    remove_trailing_commas(&mut out);

    // Replacing whole comment bytes with ASCII spaces keeps the text valid
    // UTF-8: only ASCII bytes inside comments are touched.
    String::from_utf8(out).expect("comment stripping keeps valid UTF-8")
}

/// Replaces a comma that is followed only by whitespace and `}`/`]` with a space.
fn remove_trailing_commas(bytes: &mut [u8]) {
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];

        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }

            index += 1;
            continue;
        }

        match byte {
            b'"' => {
                in_string = true;
                index += 1;
            }
            b',' => {
                let mut lookahead = index + 1;

                while lookahead < bytes.len() && bytes[lookahead].is_ascii_whitespace() {
                    lookahead += 1;
                }

                if matches!(bytes.get(lookahead), Some(b'}') | Some(b']')) {
                    bytes[index] = b' ';
                }

                index += 1;
            }
            _ => index += 1,
        }
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
