//! `Introduction` main menu screen — a real MVE video (`INTROFLC`).
//!
//! What the original does (`max-cpp-port/src/menu.cpp:2666` →
//! `Movie_PlayIntro`, `movie.cpp`):
//!
//! * `Movie_PlayIntro()` reads the `intro_movie` setting (0 — off) and
//!   calls `movie_run(INTROFLC, track, INTROSUB)`;
//! * the `INTROFLC` resource in `MAX.RES` is not a video but an ASCIIZ reference to a file
//!   (`maxint.mve`, 11 bytes), so `ResourceManager_OpenFileResource`
//!   opens the `.MVE` container next to the archive;
//! * `movie_run` runs `MVE_rmStepMovie()` from `mvelib32.c` (Interplay MVE:
//!   its own video codec + audio tracks by language), the frame is output by
//!   `movie_cb_show_frame` scaled to the window (letterbox: 432×320 →
//!   640×474 in a 640×480 frame); any key interrupts (`mve_cb_ctl`).
//!
//! In the Rust client the video is decoded by [`max_assets::mve`] (an exact port of
//! `nfPkDecomp`: 4391 frames, 15 FPS, 432×320). The screen:
//!
//! * on entry it opens `maxint.mve` and decodes frame 0 — so `shot`
//!   in QA is deterministic, and `dt = 0` keeps a freeze-frame;
//! * in the GUI it advances frames by the real `dt` (15 FPS, like `TIMER_FPS_TO_MS`);
//! * at the end of the video and on Esc/click it returns to the main menu;
//! * if there is no data (unit tests without `MAX.RES`), it shows the former
//!   placeholder with the `MLOGO`/`MAINPIC` background, so the screen is not empty.
//!
//! Audio and subtitles (`INTROSUB`, `moviesubtitle.cpp`) are not ported — this is
//! honestly noted in `max-brh-docs/analysis/visual-1to1-status.md`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use max_assets::image::BigImage;
use max_assets::mve::{Mve, MveDecoder, MveError};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered, draw_text_gnw_centered_shadowed, GnwFontId};

use crate::menu::{draw_big_image, MenuAssets, TEXT};
use crate::panels::UiPalette;

/// Screen title (like the main menu button label).
pub const INTRO_TITLE: &str = "Introduction";
/// Title area: centered, in the upper quarter of the frame.
pub const INTRO_TITLE_RECT: PixelRect = PixelRect::new(120, 120, 400, 20);
/// Placeholder text area.
pub const INTRO_TEXT_RECT: PixelRect = PixelRect::new(90, 176, 460, 150);

/// Placeholder lines: what the original plays and why this is not a video here.
pub const INTRO_TEXT: [&str; 5] = [
    "The original plays an MVE movie: INTROFLC -> maxint.mve",
    "with subtitles from INTROSUB (Interplay MVE container).",
    "",
    "MVE data was not found next to MAX.RES -",
    "the video screen needs the original game files.",
];

/// Title color: menu gold (`Fonts_GoldColor` 165/175/199).
pub const TITLE_COLORS: (u8, u8, u8) = (165, 175, 199);
/// Body text color: green `COLOR_GREEN` (0x02) of the menu palette.
pub const TEXT_COLOR: u8 = crate::menu::GREEN;

/// Player state (shared by [`IntroScreen`] clones).
#[derive(Debug)]
struct Playback {
    /// MVE decoder (if the file opened and the first frame was decoded).
    decoder: Option<MveDecoder>,
    /// Current indexed frame (empty before loading/on error).
    frame: Vec<u8>,
    /// Palette of the current frame.
    palette: [[u8; 3]; 256],
    /// Video width/height in pixels.
    width: u32,
    height: u32,
    /// How many frames have been decoded (for status/tests).
    frame_index: usize,
    /// Accumulated time since the last shown frame.
    elapsed: f32,
    /// The video has ended.
    finished: bool,
    /// Open/decode error (the placeholder is drawn then).
    error: Option<String>,
}

impl Playback {
    fn new() -> Self {
        Self {
            decoder: None,
            frame: Vec::new(),
            palette: [[0u8; 3]; 256],
            width: 0,
            height: 0,
            frame_index: 0,
            elapsed: 0.0,
            finished: false,
            error: None,
        }
    }

    /// Opens the MVE and decodes frame 0.
    fn load(&mut self, path: &Path) {
        let mve = match Mve::open(path) {
            Ok(mve) => mve,
            Err(error) => {
                self.error = Some(format!("{}: {error}", path.display()));
                return;
            }
        };

        let info = mve.info();
        let mut decoder = mve.decoder();

        match decoder.next_frame() {
            Ok(Some(frame)) => {
                let pixels = frame.to_vec();
                self.width = u32::from(info.width);
                self.height = u32::from(info.height);
                self.palette = *decoder.palette();
                self.frame = pixels;
                self.frame_index = 1;
                self.decoder = Some(decoder);
            }
            Ok(None) => {
                self.error = Some(format!("{}: в MVE нет кадров", path.display()));
            }
            Err(error) => {
                self.error = Some(format!("{}: {error}", path.display()));
            }
        }
    }

    /// Advances the video by `dt` seconds. `true` — the video has ended.
    fn advance(&mut self, dt: f32) -> bool {
        // Decoding error — show the placeholder/last frame, but do not
        // automatically kick the player off the screen.
        if self.error.is_some() {
            return false;
        }

        if self.finished {
            return true;
        }

        let Some(decoder) = self.decoder.as_mut() else {
            return false;
        };

        if dt <= 0.0 {
            return false;
        }

        // Do not catch up more than 8 frames per call (like frame skipping in
        // `sync_late`): otherwise after a pause the window would "jump" through half the movie.
        self.elapsed += dt.min(0.5);
        let frame_delay = decoder.info().frame_delay_seconds().max(1.0 / 120.0);
        let mut decoded = 0;

        while self.elapsed >= frame_delay && decoded < 8 {
            match decoder.next_frame() {
                Ok(Some(frame)) => {
                    let pixels = frame.to_vec();
                    self.palette = *decoder.palette();
                    self.frame = pixels;
                    self.frame_index += 1;
                    self.elapsed -= frame_delay;
                    decoded += 1;
                }
                Ok(None) => {
                    self.finished = true;

                    return true;
                }
                Err(error) => {
                    self.error = Some(mve_error_line(error));
                    self.finished = true;

                    return true;
                }
            }
        }

        false
    }
}

fn mve_error_line(error: MveError) -> String {
    format!("MVE: {error}")
}

/// `Introduction` screen with a lazy MVE player.
#[derive(Debug, Clone)]
pub struct IntroScreen {
    path: Option<PathBuf>,
    state: Arc<Mutex<Playback>>,
}

impl PartialEq for IntroScreen {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for IntroScreen {}

impl IntroScreen {
    /// Creates the screen and immediately decodes frame 0 (if the file exists).
    ///
    /// `path` — path to `maxint.mve` next to `MAX.RES` (see
    /// [`crate::assets::Assets::intro_mve`]); `None` — placeholder.
    pub fn new(path: Option<PathBuf>) -> Self {
        let state = Arc::new(Mutex::new(Playback::new()));

        if let Some(path) = path.as_deref() {
            state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .load(path);
        }

        Self { path, state }
    }

    /// Path to the opened MVE (for tests/diagnostics).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Whether there is a decoded frame (the MVE opened).
    pub fn is_loaded(&self) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        state.decoder.is_some() && !state.frame.is_empty()
    }

    /// How many frames were decoded (0 — placeholder only).
    pub fn frame_index(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .frame_index
    }

    /// Open/decode error (for status/tests).
    pub fn error(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .error
            .clone()
    }

    /// One video step (`dt` seconds; `dt <= 0` — freeze-frame).
    ///
    /// `true` — the video has ended, the screen should return to the menu.
    pub fn advance(&mut self, dt: f32) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .advance(dt)
    }

    /// Draws the current video frame (or the placeholder if there is no data).
    pub fn draw(&self, frame: &mut RgbaBuffer, menu: &MenuAssets) -> u32 {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if state.decoder.is_some() && !state.frame.is_empty() {
            draw_video(frame, &state)
        } else {
            draw_placeholder(frame, menu)
        }
    }
}

/// Scales the indexed frame to 640×480 with letterbox, like
/// `movie_cb_show_frame` + `cscale` (integer nearest-neighbor without
/// smoothing; the background around the frame is black).
fn draw_video(frame: &mut RgbaBuffer, state: &Playback) -> u32 {
    frame.fill(Rgba::rgb(0, 0, 0));

    let source_width = state.width as i32;
    let source_height = state.height as i32;

    if source_width <= 0 || source_height <= 0 {
        return 0;
    }

    let screen_width = frame.width() as i32;
    let screen_height = frame.height() as i32;
    let scale_w = screen_width as f32 / source_width as f32;
    let scale_h = screen_height as f32 / source_height as f32;
    let scale = if scale_h >= scale_w { scale_w } else { scale_h };

    let mut target_width = (scale * source_width as f32) as i32;
    let mut target_height = (scale * source_height as f32) as i32;

    target_width = target_width.min(screen_width);
    target_height = target_height.min(screen_height);

    let offset_x = (screen_width - target_width) / 2;
    let offset_y = (screen_height - target_height) / 2;
    let mx = (target_width << 16) / source_width;
    let my = (target_height << 16) / source_height;
    let mut painted = 0u32;

    for source_y in 0..source_height {
        let start_y = (my * source_y) >> 16;
        let end_y = (my + my * source_y) >> 16;
        let row = source_y as usize * source_width as usize;

        for source_x in 0..source_width {
            let start_x = (mx * source_x) >> 16;
            let end_x = (mx + mx * source_x) >> 16;
            let index = state.frame[row + source_x as usize];
            let color = state.palette[usize::from(index)];
            let rgba = Rgba::rgb(color[0], color[1], color[2]);

            for y in start_y..end_y {
                for x in start_x..end_x {
                    if frame.set(offset_x + x, offset_y + y, rgba) {
                        painted += 1;
                    }
                }
            }
        }
    }

    painted
}

/// The former placeholder: `MLOGO`/`MAINPIC` background + title and honest text.
fn draw_placeholder(frame: &mut RgbaBuffer, menu: &MenuAssets) -> u32 {
    let background: &BigImage = menu.mlogo.as_ref().unwrap_or(&menu.mainpic);
    let painted = draw_big_image(frame, background);
    let palette = UiPalette::from_big_image(&menu.mainpic);

    // Darken the text area so that the caption is readable on any background
    // (in the original this is the video frame — the placeholder needs a backing).
    let shade = Rgba::rgb(8, 10, 14);
    let box_rect = PixelRect::new(80, 108, 480, 240);

    for y in box_rect.uly..box_rect.uly + box_rect.height {
        for x in box_rect.ulx..box_rect.ulx + box_rect.width {
            frame.set(x, y, shade);
        }
    }

    draw_text_gnw_centered_shadowed(
        frame,
        INTRO_TITLE_RECT,
        GnwFontId::Font1.font(),
        INTRO_TITLE,
        Rgba::rgb(TITLE_COLORS.0, TITLE_COLORS.1, TITLE_COLORS.2),
        palette.rgba(0x00),
        palette.rgba(0x00),
    );

    let font = GnwFontId::Font2.font();
    let line_height = font.line_height();
    let mut y = INTRO_TEXT_RECT.uly;

    for line in INTRO_TEXT {
        if !line.is_empty() {
            draw_text_gnw_centered(
                frame,
                PixelRect::new(
                    INTRO_TEXT_RECT.ulx,
                    y,
                    INTRO_TEXT_RECT.width,
                    font.height() + 1,
                ),
                font,
                line,
                palette.rgba(TEXT_COLOR),
            );
        }

        y += line_height + 6;
    }

    draw_text_gnw_centered(
        frame,
        PixelRect::new(
            INTRO_TEXT_RECT.ulx,
            304,
            INTRO_TEXT_RECT.width,
            font.height() + 1,
        ),
        font,
        "Press Esc to return to the main menu",
        palette.rgba(TEXT),
    );

    painted
}
