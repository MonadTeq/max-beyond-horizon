//! `Credits` main menu screen — a line-by-line port of `menu_credits_menu_loop`
//! (`max-cpp-port/src/menu.cpp:1332`).
//!
//! What the original does:
//!
//! 1. loads the `CREDITS` big image 640×480 (`palette_from_image = true`, that is,
//!    the system palette — the picture palette) and shows it as the background;
//! 2. allocates the central `450×230` area (`WindowManager_ScaleUlx/Uly`,
//!    with a 640×480 window — exactly 450×230): `ulx = (640-450)/2 = 95`,
//!    `uly = (480-230)/2 = 125`;
//! 3. keeps a "scroll buffer" `230+18 = 248` lines high: each step
//!    shifts it up by 1 pixel, and when `height_offset <= 355`
//!    (the bottom of the area) — draws the next credits line with the `FONT1` font at line
//!    `231` (`Text_TextBox(…, uly = 230, height = 0x12 = 18, outline)`:
//!    `font_height = 16+1 = 17`, `uly += ((18+1) - 17)/2 = 1` → 231) and
//!    resets `height_offset = 355 + 20 = 375`;
//! 4. a step lasts `TIMER_FPS_TO_MS(scroll_speed)` = `1000/100 = 10 ms`;
//!    `KP-`/`KP+` change the speed by 9 (within 9…1000), any other
//!    key — exit;
//! 5. when `height_offset <= 125` (the top of the area) — the screen closes.
//!
//! The line table is `data/credits.json` (the generator
//! `max-brh/tools/gen_credits_data.py`): 171 lines, color `0xFF` for the headers
//! and `0x04` for the names; `CREDITS_SEPARATOR` is an empty line, it draws nothing.
//!
//! # Determinism
//!
//! The state is a pure function of the number of steps: line `k` is drawn at step
//! `20·k` and at step `t` lies at line `231 - (t - 20·k)`. No pixel buffer
//! and no dependence on FPS: in QA `dt = 1/60`
//! ([`Self::advance`]), and the frame is fully determined by the number of steps.
//!
//! # Differences from C++
//!
//! * Text is drawn with the raster GNW `FONT1` font (like the whole Rust client), not
//!   with FreeType; the outline is a "shadow" at +1 pixel right/down with index `0x00`
//!   (`Text_BlitTTF` draws it with the color `Color_RGB2Color(0)`).
//! * `KP-`/`KP+` are unavailable in the client (no numpad events): the speed is changed
//!   with the `-`/`+` keys.

use std::sync::OnceLock;

use max_assets::image::BigImage;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered_shadowed, GnwFontId};
use serde::Deserialize;

use crate::panels::UiPalette;

/// JSON with the credits table (generated from `menu.cpp` + `language.json`).
pub const CREDITS_LINES_JSON: &str = include_str!("../data/credits.json");

/// Width of the scroll area (`WindowManager_ScaleUlx(window, 450)`).
pub const CREDITS_WIDTH: i32 = 450;
/// Height of the scroll area (`WindowManager_ScaleUly(window, 230)`).
pub const CREDITS_HEIGHT: i32 = 230;
/// Left edge of the area: `(640-450)/2`.
pub const CREDITS_ULX: i32 = 95;
/// Top edge of the area: `(480-230)/2`.
pub const CREDITS_ULY: i32 = 125;
/// Bottom of the area = `CREDITS_ULY + CREDITS_HEIGHT` (in C++ — `bounds.lry`).
pub const CREDITS_BOTTOM: i32 = CREDITS_ULY + CREDITS_HEIGHT;
/// Buffer line where the new line is drawn (`window_height + 1`).
pub const TEXT_ROW: i32 = CREDITS_HEIGHT + 1;
/// After how many steps the next line appears (`bounds.lry + 20`).
pub const LINE_STEP: i32 = 20;
/// Speed change step (`KP-`/`KP+`).
pub const SPEED_RATE: u32 = 9;
/// Default speed: `TIMER_FPS_TO_MS(100)` = 10 ms per step.
pub const DEFAULT_SPEED: u32 = 100;
/// Maximum speed (as in C++: `scroll_speed < 1000`).
pub const MAX_SPEED: u32 = 1000;
/// How many steps [`CreditsScreen::advance`] does at most per frame.
///
/// Protection against "catch-up" after a pause: without it a single frame with `dt = 10 s`
/// would scroll the whole screen at once, though determinism would be preserved. The limit
/// keeps the frame bounded in time.
pub const MAX_STEPS_PER_FRAME: u32 = 600;

/// One credits line.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CreditsLine {
    /// Color index in the `CREDITS` palette (`0xFF` — heading, `0x04` — text).
    pub color: u8,
    /// Text (en-US).
    pub text: String,
}

#[derive(Debug, Deserialize)]
struct CreditsFile {
    lines: Vec<CreditsLine>,
}

/// Credits table (`menu_credits_lines[]`).
pub fn lines() -> &'static [CreditsLine] {
    static LINES: OnceLock<Vec<CreditsLine>> = OnceLock::new();

    LINES
        .get_or_init(|| {
            serde_json::from_str::<CreditsFile>(CREDITS_LINES_JSON)
                .map(|file| file.lines)
                .unwrap_or_default()
        })
        .as_slice()
}

/// How many credits lines (171 in the original).
pub fn line_count() -> usize {
    lines().len()
}

/// Credits scroll state (equivalent of the `menu_credits_menu_loop` local
/// `menu_credits_menu_loop`: `line_index`, `height_offset`, `scroll_speed`,
/// `play`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditsScreen {
    steps: u32,
    line_index: usize,
    height_offset: i32,
    scroll_speed: u32,
    elapsed_us: u64,
    finished: bool,
}

impl Default for CreditsScreen {
    fn default() -> Self {
        Self {
            steps: 0,
            line_index: 0,
            height_offset: CREDITS_BOTTOM,
            scroll_speed: DEFAULT_SPEED,
            elapsed_us: 0,
            finished: false,
        }
    }
}

impl CreditsScreen {
    /// New screen: the first line appears on the very first step.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many scroll steps have been made.
    #[inline]
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// How many lines are already shown.
    #[inline]
    pub fn line_index(&self) -> usize {
        self.line_index
    }

    /// Current `height_offset` (for tests).
    #[inline]
    pub fn height_offset(&self) -> i32 {
        self.height_offset
    }

    /// Current speed (steps per second; 100 — as in the original).
    #[inline]
    pub fn scroll_speed(&self) -> u32 {
        self.scroll_speed
    }

    /// Scrolling reached the top of the area (`height_offset <= bounds.uly`).
    #[inline]
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// `KP-`: slower (`scroll_speed -= 9`, while `>= 9`).
    pub fn slower(&mut self) {
        if self.scroll_speed >= SPEED_RATE {
            self.scroll_speed -= SPEED_RATE;
        }
    }

    /// `KP+`: faster (`scroll_speed += 9`, while `< 1000`).
    pub fn faster(&mut self) {
        if self.scroll_speed < MAX_SPEED {
            self.scroll_speed += SPEED_RATE;
        }
    }

    /// One scroll step (`while (play)` in C++ without waiting).
    fn step(&mut self) {
        self.steps = self.steps.saturating_add(1);
        self.height_offset -= 1;

        if self.line_index < lines().len() && self.height_offset <= CREDITS_BOTTOM {
            self.line_index += 1;
            self.height_offset = CREDITS_BOTTOM + LINE_STEP;
        }

        if self.height_offset <= CREDITS_ULY {
            self.finished = true;
        }
    }

    /// Advances the scrolling by `dt` seconds (`dt <= 0` — stays, like the portrait).
    pub fn advance(&mut self, dt: f32) {
        if self.finished || !(dt > 0.0) {
            return;
        }

        let micros = (f64::from(dt) * 1_000_000.0).round() as u64;

        self.elapsed_us = self.elapsed_us.saturating_add(micros);

        let frame_us = 1_000_000 / u64::from(self.scroll_speed.max(1));
        let mut budget = MAX_STEPS_PER_FRAME;

        while self.elapsed_us >= frame_us && !self.finished && budget > 0 {
            self.elapsed_us -= frame_us;
            self.step();
            budget -= 1;
        }

        if budget == 0 {
            // Do not accumulate debt: the next frame starts from a clean slate.
            self.elapsed_us = 0;
        }
    }

    /// Buffer line where the credits line `index` currently is
    /// (may be negative — the line has already gone above).
    pub fn row_of(&self, index: usize) -> i32 {
        // Line `k` is drawn at step `1 + 20·k` (the first step draws line 0
        // and resets `height_offset` to 375, the next 20 steps lower it
        // to 355); since then the buffer has moved up by `steps - (1 + 20k)`.
        let drawn_at = 1 + (index as i32).saturating_mul(LINE_STEP);

        TEXT_ROW - (self.steps as i32 - drawn_at)
    }

    /// Visible lines: `(line number, buffer line)`.
    pub fn visible_lines(&self) -> Vec<(usize, i32)> {
        let font = GnwFontId::Font1.font();
        let height = font.height();
        let mut visible = Vec::new();

        for index in 0..self.line_index {
            let row = self.row_of(index);

            if row < CREDITS_HEIGHT && row + height > 0 {
                visible.push((index, row));
            }
        }

        visible
    }

    /// Draws the screen: `CREDITS` background + visible credits lines in the 450×230 area.
    ///
    /// Returns the number of filled pixels (for render statistics).
    pub fn draw(&self, frame: &mut RgbaBuffer, background: &BigImage) -> u32 {
        let painted = crate::menu::draw_big_image(frame, background);
        let palette = UiPalette::from_big_image(background);
        let font = GnwFontId::Font1.font();
        let outline = palette.rgba(0x00);

        // The text is drawn into a separate area buffer (transparent background) so that
        // lines beyond the 450×230 boundary do not spill onto the background around the area —
        // exactly like clipping the scroll buffer in C++.
        let mut region = RgbaBuffer::new(CREDITS_WIDTH as u32, CREDITS_HEIGHT as u32);

        for (index, row) in self.visible_lines() {
            let line = &lines()[index];

            if line.text.is_empty() {
                continue;
            }

            let rect = PixelRect::new(0, row - 1, CREDITS_WIDTH, 18);

            draw_text_gnw_centered_shadowed(
                &mut region,
                rect,
                font,
                &line.text,
                palette.rgba(line.color),
                outline,
                outline,
            );
        }

        let mut additional = 0;

        for y in 0..CREDITS_HEIGHT {
            for x in 0..CREDITS_WIDTH {
                let Some(color) = region.get(x, y) else {
                    continue;
                };

                if color.a == 0 {
                    continue;
                }

                if frame.set(CREDITS_ULX + x, CREDITS_ULY + y, color) {
                    additional += 1;
                }
            }
        }

        painted + additional
    }
}

/// Background/text color for tests: transparency must not leak into the frame.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_has_171_lines_in_menu_cpp_order() {
        assert_eq!(line_count(), 171, "menu_credits_lines: 171 непустая строка");
        assert_eq!(lines()[0].text, "Project Manager");
        assert_eq!(lines()[0].color, 0xFF);
        assert_eq!(lines()[1].text, "Ali Atabek");
        assert_eq!(lines()[1].color, 0x04);
        assert_eq!(lines()[2].text, "Producers");
        // Credits at the end of the table (Parallax Software — installer).
        assert!(lines().iter().any(|line| line.text.contains("Parallax")));
        assert!(lines().iter().all(|line| !line.text.is_empty()));
    }

    #[test]
    fn dt_zero_keeps_the_screen_still() {
        let mut screen = CreditsScreen::new();

        for _ in 0..600 {
            screen.advance(0.0);
        }

        assert_eq!(screen.steps(), 0);
        assert_eq!(screen.line_index(), 0);
        assert!(!screen.finished());
        assert!(screen.visible_lines().is_empty());
    }

    #[test]
    fn one_second_scrolls_one_hundred_steps_at_default_speed() {
        let mut screen = CreditsScreen::new();

        for _ in 0..60 {
            screen.advance(1.0 / 60.0);
        }

        // 10 ms per step: 99-100 steps per second (microsecond rounding).
        assert!(
            matches!(screen.steps(), 99 | 100),
            "шагов: {}",
            screen.steps()
        );
        // In 100 steps 6 lines appear (step 20) = line_index 6.
        assert!(
            matches!(screen.line_index(), 5 | 6),
            "{}",
            screen.line_index()
        );
    }

    #[test]
    fn lines_appear_every_twenty_steps_and_scroll_up() {
        let mut screen = CreditsScreen::new();

        // Step 1: the first line is drawn at line 231 (still below the window).
        screen.advance(0.01);

        assert_eq!(screen.steps(), 1);
        assert_eq!(screen.line_index(), 1);
        assert_eq!(screen.row_of(0), TEXT_ROW);
        assert!(screen.visible_lines().is_empty());

        // Step 2: the line at 230 does not fit into the window yet (lines 0..229 are visible).
        screen.advance(0.01);
        assert_eq!(screen.row_of(0), TEXT_ROW - 1);
        assert!(screen.visible_lines().is_empty());

        // Step 3: the line at 229 — the bottom visible line of the window.
        screen.advance(0.01);
        assert_eq!(screen.visible_lines(), vec![(0, TEXT_ROW - 2)]);

        // By step 21 a second line was added, the first moved up by 20.
        for _ in 0..18 {
            screen.advance(0.01);
        }

        assert_eq!(screen.steps(), 21);
        assert_eq!(screen.line_index(), 2);
        assert_eq!(screen.row_of(0), TEXT_ROW - 20);
        assert_eq!(screen.row_of(1), TEXT_ROW);
        // The second line does not fit into the window yet (line 231 > 229).
        assert_eq!(screen.visible_lines(), vec![(0, TEXT_ROW - 20)]);

        for _ in 0..20 {
            screen.advance(0.01);
        }

        assert_eq!(screen.steps(), 41);
        assert_eq!(screen.row_of(2), TEXT_ROW);
        // The third line is still below the window — only the first two are visible.
        assert_eq!(
            screen.visible_lines(),
            vec![(0, TEXT_ROW - 40), (1, TEXT_ROW - 20)]
        );
    }

    #[test]
    fn screen_finishes_after_the_last_line_leaves_the_top() {
        let mut screen = CreditsScreen::new();
        // The last line is drawn at step `1 + 20·(N-1)`, then 250 steps,
        // until `height_offset` goes from 375 to 125 (`bounds.uly`).
        let total = 1 + 20 * (line_count() as u32 - 1) + 250;

        // We go through to the end: 3651 steps at 100 steps/s.
        for _ in 0..(total * 11) {
            screen.advance(0.01);
        }

        assert!(screen.finished(), "экран должен закрыться сам");
        assert_eq!(screen.line_index(), line_count());
        assert!(screen.height_offset() <= CREDITS_ULY);
        assert!(
            screen.steps() >= total,
            "шагов {} < {total}",
            screen.steps()
        );
        // After closing, no steps run.
        let steps = screen.steps();
        screen.advance(1.0);
        assert_eq!(screen.steps(), steps);
    }

    #[test]
    fn speed_changes_by_nine_like_the_keypad_controls() {
        let mut screen = CreditsScreen::new();

        assert_eq!(screen.scroll_speed(), 100);

        screen.faster();
        assert_eq!(screen.scroll_speed(), 109);

        for _ in 0..200 {
            screen.faster();
        }
        assert_eq!(screen.scroll_speed(), MAX_SPEED);

        screen.slower();
        assert_eq!(screen.scroll_speed(), MAX_SPEED - SPEED_RATE);

        for _ in 0..200 {
            screen.slower();
        }
        // Stops at the first speed less than a step (as in C++:
        // `if (scroll_speed >= scroll_rate) scroll_speed -= scroll_rate`).
        assert!(
            screen.scroll_speed() < SPEED_RATE,
            "{}",
            screen.scroll_speed()
        );
        assert!(screen.scroll_speed() > 0);

        // Speed affects scrolling: 109 steps/s → 1 step per 10 ms.
        let mut fast = CreditsScreen::new();
        fast.faster();
        fast.advance(0.01);
        assert_eq!(fast.steps(), 1);
    }

    #[test]
    fn draw_paints_background_and_text_only_inside_the_region() {
        let menu =
            crate::menu::MenuAssets::load(&max_assets::ResArchive::open(repo_max_res()).unwrap());

        let Some(menu) = menu else {
            eprintln!("пропуск: нет MAX.RES");
            return;
        };

        let Some(background) = menu.credits.as_ref() else {
            eprintln!("пропуск: нет CREDITS");
            return;
        };

        let mut screen = CreditsScreen::new();
        let mut frame = RgbaBuffer::new(640, 480);
        let painted = screen.draw(&mut frame, background);

        assert!(painted >= 640 * 480, "фон должен занять весь кадр");

        // At the very beginning the text is not visible yet — the area matches the background.
        let area = |frame: &RgbaBuffer, y0: i32, y1: i32| {
            let mut out = Vec::new();

            for y in y0..y1 {
                for x in CREDITS_ULX..(CREDITS_ULX + CREDITS_WIDTH) {
                    out.push(frame.get(x, y));
                }
            }

            out
        };

        let before = area(&frame, CREDITS_ULY, CREDITS_ULY + CREDITS_HEIGHT);

        for _ in 0..30 {
            screen.advance(0.01);
        }

        let mut with_text = RgbaBuffer::new(640, 480);
        screen.draw(&mut with_text, background);
        let after = area(&with_text, CREDITS_ULY, CREDITS_ULY + CREDITS_HEIGHT);

        assert_ne!(before, after, "строки титров должны изменить область");

        // Nothing changes outside the area.
        for y in 0..480 {
            for x in 0..640 {
                let inside = (CREDITS_ULX..CREDITS_ULX + CREDITS_WIDTH).contains(&x)
                    && (CREDITS_ULY..CREDITS_ULY + CREDITS_HEIGHT).contains(&y);

                if !inside {
                    assert_eq!(
                        frame.get(x, y),
                        with_text.get(x, y),
                        "пиксель вне области ({x},{y}) изменился"
                    );
                }
            }
        }
    }

    fn repo_max_res() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("max-original")
            .join("MAX.RES")
    }
}
