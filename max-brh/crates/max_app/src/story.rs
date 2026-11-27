//! Campaign story screens — `menu_draw_mission_story_screen` (`menu.cpp:766-864`).
//!
//! Oracle:
//! * `menu.cpp:2065-2071` — the **intro** story is shown after the mission file is
//!   chosen and before the match starts (`menu_draw_mission_story_screen`);
//! * `menu.cpp:495-512` — the **victory**/**defeat** story is shown after the
//!   game-over screen and before the campaign wrap-up returns to the main menu;
//! * `menu.cpp:766-864` — the layout: the background is one of the
//!   `menu_briefing_backgrounds` (`ENDGAME1`…`ENDGAME9`) or the story resource
//!   list; the text is `GNW_TEXT_FONT_1` wrapped to 600 px in the (20,20) 600×400
//!   box, colored `Fonts_BrightSilverColor`; the `ENDOK` button sits at (293,458);
//!   Enter/Esc closes the text window; then every video of `story.video` is played
//!   with `Movie_Play` (an MVE file reference from `MAX.RES`); optional background
//!   music is queued before the text window.
//!
//! # Layering (owner condition 1)
//!
//! Presentation only. [`StoryScreen`] holds decoded pictures and an MVE player —
//! no `App`, no `GameState`; it is not part of `snapshot_bytes`/CRC and never
//! touches the simulation RNG (C++ picks the background with `Randomizer_Generate`;
//! here the choice is deterministic from the mission index, like the victory screen
//! background of [`crate::winloss`]).
//!
//! # Visible fallbacks (owner condition 2)
//!
//! A story screen never blocks the mission: the drivers start/return the match on
//! their own, and the screen only gates the *screen* sequence. A missing background
//! resource, video file or music track is reported in [`StoryScreen::diagnostics`]
//! (drawn at the bottom of the screen, black backing so it stays readable) and in
//! the driver log; the remaining layers still show. A descriptor without a story
//! block has no screen at all, exactly like the oracle.
//!
//! One deliberate presentation deviation: the C++ text box is truncated to
//! `400 / (Text_GetHeight() + 1) = 23` rows (`Text_SplitText`). The campaign intro
//! of `SC_C0001` is longer, so the port paginates with Up/Down/PageUp/PageDown and
//! shows a page counter; the first page is laid out exactly like the oracle.
//!
//! The text is typed at the oracle's speed (`Text_TypeWriter_CharacterTimeMs = 10`,
//! `menu.cpp:795`): every character of the current page is revealed over time, any
//! input completes the page (the key is consumed, as in `Text_TypeWriter_TextBox`).
//! A `dt = 0` frame draws the complete page — the port's freeze-frame convention
//! keeps QA screenshots deterministic.

use std::fs;
use std::path::{Path, PathBuf};

use max_assets::image::BigImage;
use max_assets::mission::{Media, MediaResource, StoryBlock, DEFAULT_LANGUAGE};
use max_assets::{MissionDescriptor, ResArchive};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{
    draw_text_gnw, draw_text_gnw_centered_shadowed, draw_text_gnw_shadowed, GnwFontId,
};

use crate::intro::IntroScreen;
use crate::menu::{blit_simple, clean_status, draw_big_image, MenuAssets};

/// Story text box (`Text_TypeWriter_TextBoxMultiLineWrapText(..., 20, 20, 600, 400, 0)`).
pub const STORY_TEXT_RECT: PixelRect = PixelRect::new(20, 20, 600, 400);
/// Width of the wrapped story text.
pub const STORY_TEXT_WIDTH: i32 = 600;
/// Height of the story text box.
pub const STORY_TEXT_HEIGHT: i32 = 400;
/// Typewriter speed (`menu.cpp:795`: `Text_TypeWriter_CharacterTimeMs = 10`).
pub const TYPEWRITER_CHARACTER_MS: f32 = 10.0;
/// `ENDOK` button (`menu.cpp:821`: `Button(ENDOK_U, ENDOK_D, 293, 458)`).
pub const STORY_OK: PixelRect = PixelRect::new(293, 458, 72, 19);
/// Button caption (`_(ccca)` — "OK").
pub const STORY_OK_CAPTION: &str = "OK";
/// The status/fallback panel at the bottom of the screen.
pub const STORY_STATUS: PixelRect = PixelRect::new(8, 436, 624, 18);
/// Page counter line (only when the text has more than one page).
pub const STORY_PAGE_LINE: PixelRect = PixelRect::new(20, 424, 300, 12);
/// `Fonts_BrightSilverColor` (`fonts.cpp:31`): base `0xA2`, outline `0xAC`,
/// shadow `0xC7` — the three colours of `Text_TextLine`, not just the fill.
pub const SILVER: (u8, u8, u8) = (162, 172, 199);
/// Green text index (`COLOR_GREEN`).
pub const GREEN: u8 = crate::menu::GREEN;
/// The nine briefing backgrounds (`menu_briefing_backgrounds`, `menu.cpp:102`).
pub const BRIEFING_BACKGROUNDS: [&str; 9] = [
    "ENDGAME1", "ENDGAME2", "ENDGAME3", "ENDGAME4", "ENDGAME5", "ENDGAME6", "ENDGAME7", "ENDGAME8",
    "ENDGAME9",
];

/// Which story of a mission is being shown (`Mission::GetIntroInfo`/`GetVictoryInfo`/
/// `GetDefeatInfo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryKind {
    /// Before the match (`menu.cpp:2065-2071`).
    Intro,
    /// After a won match (`menu.cpp:498-503`).
    Victory,
    /// After a lost match (`menu.cpp:505-510`).
    Defeat,
}

impl StoryKind {
    /// Lower-case name for the log/status lines.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Intro => "intro",
            Self::Victory => "victory",
            Self::Defeat => "defeat",
        }
    }

    /// The story block of `descriptor` (`Mission::HasIntroInfo` & co).
    pub fn block<'a>(self, descriptor: &'a MissionDescriptor) -> Option<&'a StoryBlock> {
        match self {
            Self::Intro => descriptor.intro.as_ref(),
            Self::Victory => descriptor.victory.as_ref(),
            Self::Defeat => descriptor.defeat.as_ref(),
        }
    }
}

/// Phase of the story screen: the text window, or the trailing MVE movie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryPhase {
    /// The wrapped text + `ENDOK` (`menu.cpp:797-850`).
    Text,
    /// `Movie_Play` of the `story.video` list (`menu.cpp:852-863`).
    Video,
}

/// What the input did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryAction {
    /// Redraw only.
    None,
    /// The screen sequence is over — the driver resumes its own action.
    Finished,
}

/// The action a driver resumes after the story screen closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryContinuation {
    /// Enter the already-started match (`menu.cpp:2096-2098`).
    EnterSession,
    /// Return to the main menu and run the wrap-up (`menu.cpp:514-545`).
    LeaveSession,
}

/// A decoded story screen (text, background, optional videos and music).
#[derive(Debug, Clone)]
pub struct StoryScreen {
    kind: StoryKind,
    text: String,
    rows: Vec<String>,
    page: usize,
    /// Characters of the current page revealed by the typewriter.
    typed: usize,
    /// A positive `dt` has been applied: the typewriter is running. Until then the
    /// page is drawn complete, so a `dt = 0` freeze-frame stays deterministic.
    typing_started: bool,
    /// Time accumulated since the last revealed character.
    typing_elapsed: f32,
    /// Decoded background; `None` — the menu briefing fallback at draw time.
    background: Option<BigImage>,
    background_tag: Option<String>,
    /// Deterministic index into `menu_briefing_backgrounds` for the fallback.
    background_index: usize,
    /// Video file references resolved next to `MAX.RES` (in the oracle's order).
    videos: Vec<PathBuf>,
    video_index: usize,
    video: Option<IntroScreen>,
    music: Option<String>,
    phase: StoryPhase,
    diagnostics: Vec<String>,
}

impl PartialEq for StoryScreen {
    /// Comparison of the presentation state; the decoded pixels are not compared.
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.text == other.text
            && self.page == other.page
            && self.typed == other.typed
            && self.typing_started == other.typing_started
            && self.background_tag == other.background_tag
            && self.background_index == other.background_index
            && self.videos == other.videos
            && self.video_index == other.video_index
            && self.video == other.video
            && self.music == other.music
            && self.phase == other.phase
            && self.diagnostics == other.diagnostics
    }
}

impl Eq for StoryScreen {}

impl StoryScreen {
    /// Builds the screen of `block` (`menu_draw_mission_story_screen`).
    ///
    /// `seed` picks the background deterministically (the oracle uses
    /// `Randomizer_Generate`; the simulation RNG must not be advanced by the UI).
    /// `max_res` is `MAX.RES` (backgrounds and video file references); `data_dir`
    /// is the directory of `MAX.RES` (the `.MVE`/picture files live next to it).
    pub fn from_block(
        kind: StoryKind,
        block: &StoryBlock,
        language: &str,
        seed: usize,
        max_res: Option<&ResArchive>,
        data_dir: &Path,
    ) -> Self {
        let mut diagnostics = Vec::new();
        let text = block.text.get(language).to_string();
        let rows = wrap_story_text(&text);
        let background_index = seed % BRIEFING_BACKGROUNDS.len();
        let (background_tag, background) = resolve_background(
            block.background.as_ref(),
            seed,
            max_res,
            data_dir,
            &mut diagnostics,
        );
        let videos = resolve_videos(
            block.video.get(language).or_else(|| {
                block
                    .video
                    .get(DEFAULT_LANGUAGE)
                    .or_else(|| block.video.values().next())
            }),
            max_res,
            data_dir,
            &mut diagnostics,
        );
        let music = resolve_music(block.music.as_ref(), &mut diagnostics);

        let mut screen = Self {
            kind,
            text,
            rows,
            page: 0,
            typed: 0,
            typing_started: false,
            typing_elapsed: 0.0,
            background,
            background_tag,
            background_index,
            videos,
            video_index: 0,
            video: None,
            music,
            phase: StoryPhase::Text,
            diagnostics,
        };

        // The oracle shows no text window when the text is empty and goes straight
        // to the videos (`menu.cpp:797`/`852`).
        if screen.text.is_empty() {
            screen.begin_video();
        }

        screen
    }

    /// The screen of the `kind` story of `descriptor`; `None` when the mission has
    /// no such story, or when there is nothing to show (empty text and no video).
    pub fn for_mission(
        descriptor: Option<&MissionDescriptor>,
        kind: StoryKind,
        seed: usize,
        max_res: Option<&ResArchive>,
        data_dir: &Path,
    ) -> Option<Self> {
        let descriptor = descriptor?;
        let block = kind.block(descriptor)?;

        if block.text.is_empty() && block.video.is_empty() {
            return None;
        }

        let screen = Self::from_block(kind, block, DEFAULT_LANGUAGE, seed, max_res, data_dir);

        (!screen.is_empty()).then_some(screen)
    }

    /// Story kind.
    pub fn kind(&self) -> StoryKind {
        self.kind
    }

    /// Localized story text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Wrapped rows (`Text_SplitText` with `GNW_TEXT_FONT_1`).
    pub fn rows(&self) -> &[String] {
        &self.rows
    }

    /// Rows per page (`400 / (Text_GetHeight() + 1)`).
    pub fn rows_per_page() -> usize {
        (STORY_TEXT_HEIGHT / GnwFontId::Font1.font().line_height()).max(1) as usize
    }

    /// Number of pages (`1` when the text fits the oracle's box).
    pub fn page_count(&self) -> usize {
        self.rows.len().div_ceil(Self::rows_per_page()).max(1)
    }

    /// Current page (0-based).
    pub fn page(&self) -> usize {
        self.page
    }

    /// Rows of the current page.
    pub fn visible_rows(&self) -> &[String] {
        let per_page = Self::rows_per_page();
        let start = (self.page * per_page).min(self.rows.len());
        let end = (start + per_page).min(self.rows.len());

        &self.rows[start..end]
    }

    /// Characters of the current page revealed by the typewriter.
    ///
    /// Before the first positive `dt` the page is drawn complete (a deterministic
    /// freeze-frame), so the value is only meaningful once [`Self::typing`] is true.
    pub fn typed_chars(&self) -> usize {
        self.typed
    }

    /// Whether the typewriter is running (`false` before the first positive `dt`
    /// and after the page has been fully revealed).
    pub fn typing(&self) -> bool {
        self.typing_started && self.typed < self.page_chars()
    }

    /// Total characters of the current page (`Text_TypeWriter_TextBox` types every
    /// character of every row; row breaks do not consume time).
    fn page_chars(&self) -> usize {
        self.visible_rows()
            .iter()
            .map(|row| row.chars().count())
            .sum()
    }

    /// Moves to `page` and restarts the typewriter for it when the animation has
    /// already begun. A `dt = 0` freeze-frame keeps drawing complete pages.
    fn set_page(&mut self, page: usize) {
        self.page = page;
        self.typed = 0;
        self.typing_elapsed = 0.0;
    }

    /// Current phase.
    pub fn phase(&self) -> StoryPhase {
        self.phase
    }

    /// Decoded background resource tag, when the story named one.
    pub fn background_tag(&self) -> Option<&str> {
        self.background_tag.as_deref()
    }

    /// Whether a background picture was decoded.
    pub fn has_background(&self) -> bool {
        self.background.is_some()
    }

    /// Resolved video files.
    pub fn videos(&self) -> &[PathBuf] {
        &self.videos
    }

    /// Background music resource tag (`StoryBlock::music`).
    pub fn music(&self) -> Option<&str> {
        self.music.as_deref()
    }

    /// Visible fallback/diagnostic lines (missing media, decode errors).
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    /// Status line for the driver (`ctx.status`): the diagnostics, or empty.
    pub fn status_line(&self) -> String {
        self.diagnostics.join("; ")
    }

    /// Records a music failure the driver detected (the track is played through
    /// [`crate::music::MusicDirector`], so the screen cannot know it itself).
    pub fn note_music_error(&mut self, tag: &str, reason: &str) {
        self.diagnostics
            .push(format!("story music {tag}: {reason}"));
    }

    /// Whether the screen has anything to show.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.video.is_none() && self.videos.is_empty()
    }

    /// The movies that still have to be played (`Movie_Play` loop).
    pub fn remaining_videos(&self) -> usize {
        self.videos.len().saturating_sub(self.video_index)
    }

    /// Handles one input event.
    pub fn input(&mut self, event: crate::ui::InputEvent) -> StoryAction {
        use crate::ui::{InputEvent, Key};

        // `Text_TypeWriter_TextBox`: any input while the text is being typed only
        // completes the current page (the key is consumed), exactly like the oracle.
        //
        // A bare left-button **release** is not an input of the oracle here: during
        // the typewriter the briefing window has no registered buttons yet
        // (`button_end_ok` is created after `Text_TypeWriter_TextBoxMultiLineWrapText`,
        // `menu.cpp:816-826`), and `get_input()` only returns keys/button events.
        // The release of the click that started the mission would otherwise complete
        // the whole page on the first frame — the typewriter regression.
        if self.phase == StoryPhase::Text && self.typing() {
            if !matches!(
                event,
                InputEvent::MouseMove(..) | InputEvent::MouseRelease(..)
            ) {
                self.typed = self.page_chars();
                self.typing_elapsed = 0.0;

                return StoryAction::None;
            }
        }

        match event {
            InputEvent::MouseMove(..) | InputEvent::MouseRelease(..) => StoryAction::None,
            InputEvent::MouseWheel(..) => StoryAction::None,
            InputEvent::MouseClick(x, y) => match self.phase {
                StoryPhase::Text => {
                    if STORY_OK.contains(x, y) {
                        self.close_text()
                    } else {
                        StoryAction::None
                    }
                }
                StoryPhase::Video => self.next_video(),
            },
            InputEvent::RightClick(..) => match self.phase {
                StoryPhase::Video => self.next_video(),
                StoryPhase::Text => StoryAction::None,
            },
            InputEvent::Key(key) => match (self.phase, key) {
                (StoryPhase::Text, Key::Enter | Key::Escape) => self.close_text(),
                (StoryPhase::Text, Key::Up) => {
                    self.set_page(self.page.saturating_sub(1));
                    StoryAction::None
                }
                (StoryPhase::Text, Key::Down) => {
                    if self.page + 1 < self.page_count() {
                        self.set_page(self.page + 1);
                    }
                    StoryAction::None
                }
                (StoryPhase::Text, Key::PageUp) => {
                    self.set_page(0);
                    StoryAction::None
                }
                (StoryPhase::Text, Key::PageDown) => {
                    self.set_page(self.page_count() - 1);
                    StoryAction::None
                }
                // `mve_cb_ctl`: any key/click interrupts the movie; the oracle then
                // continues with the next video of the list.
                (StoryPhase::Video, _) => self.next_video(),
                (StoryPhase::Text, _) => StoryAction::None,
            },
        }
    }

    /// Advances the current phase by `dt` seconds; `true` — the whole story is over.
    ///
    /// In the text phase this is the typewriter (`10 ms` per character,
    /// `menu.cpp:795`); the first positive `dt` switches the complete freeze-frame to
    /// the animated reveal. `dt = 0` never changes the frame.
    pub fn advance(&mut self, dt: f32) -> bool {
        if self.phase == StoryPhase::Text {
            if dt > 0.0 && !self.text.is_empty() {
                self.typing_started = true;

                let total = self.page_chars();

                if self.typed < total {
                    self.typing_elapsed += dt;

                    let per_character = TYPEWRITER_CHARACTER_MS / 1000.0;
                    let due = (self.typing_elapsed / per_character + 1e-4) as usize;

                    if due > 0 {
                        let reveal = due.min(total - self.typed);

                        self.typed += reveal;
                        self.typing_elapsed -= reveal as f32 * per_character;
                    }

                    if self.typed >= total {
                        self.typed = total;
                        self.typing_elapsed = 0.0;
                    }
                }
            }

            return false;
        }

        if self.phase != StoryPhase::Video {
            return false;
        }

        let finished = self.video.as_mut().is_some_and(|video| video.advance(dt));

        if finished {
            return self.next_video() == StoryAction::Finished;
        }

        false
    }

    /// Audio chunks of the current movie frame (the driver queues them).
    pub fn take_audio(&mut self) -> Vec<max_assets::mve::MveAudioChunk> {
        self.video
            .as_mut()
            .map(IntroScreen::take_audio)
            .unwrap_or_default()
    }

    /// Closes the text window: plays the videos, or finishes the story.
    fn close_text(&mut self) -> StoryAction {
        if self.remaining_videos() == 0 {
            return StoryAction::Finished;
        }

        self.begin_video();

        if self.phase == StoryPhase::Video {
            StoryAction::None
        } else {
            StoryAction::Finished
        }
    }

    /// Starts the next resolved video; unreadable files are diagnostics and skipped.
    fn begin_video(&mut self) {
        while self.video_index < self.videos.len() {
            let path = self.videos[self.video_index].clone();
            let screen = IntroScreen::with_media(Some(path.clone()), None);

            if let Some(error) = screen.error() {
                self.diagnostics
                    .push(format!("story video {}: {error}", path.display()));
                self.video_index += 1;
                continue;
            }

            self.video = Some(screen);
            self.phase = StoryPhase::Video;

            return;
        }

        // Nothing playable is left: stay in the text phase (the next Enter finishes).
        self.video = None;
        self.phase = StoryPhase::Text;
    }

    /// Ends the current movie and starts the next one, or finishes the story.
    fn next_video(&mut self) -> StoryAction {
        self.video = None;
        self.video_index += 1;

        if self.video_index >= self.videos.len() {
            return StoryAction::Finished;
        }

        self.begin_video();

        if self.phase == StoryPhase::Video {
            StoryAction::None
        } else {
            StoryAction::Finished
        }
    }

    /// Draws the screen into the frame.
    ///
    /// `menu` is the loaded menu resource set; without it the screen still renders
    /// (text and diagnostics over a black background) — a visible fallback, never an
    /// empty frame.
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        menu: Option<&MenuAssets>,
        _mouse: Option<(i32, i32)>,
    ) -> u32 {
        if self.phase == StoryPhase::Video {
            if let (Some(video), Some(menu)) = (self.video.as_ref(), menu) {
                return video.draw(frame, menu);
            }
        }

        let mut painted = self.draw_background(frame, menu);

        if self.phase == StoryPhase::Text {
            painted += self.draw_text(frame, menu);
        }

        painted += self.draw_ok(frame, menu);
        painted += self.draw_footer(frame, menu);

        painted
    }

    /// Draws the story background or the menu briefing fallback.
    fn draw_background(&self, frame: &mut RgbaBuffer, menu: Option<&MenuAssets>) -> u32 {
        if let Some(background) = self.background.as_ref() {
            return draw_big_image(frame, background);
        }

        let Some(menu) = menu else {
            frame.fill(Rgba::BLACK);

            return 0;
        };

        if let Some(background) = menu.endgame_backgrounds.get(self.background_index) {
            return draw_big_image(frame, background);
        }

        if let Some(background) = menu.framepic.as_ref() {
            return draw_big_image(frame, background);
        }

        draw_big_image(frame, &menu.mainpic)
    }

    /// `Text_TypeWriter_TextBoxMultiLineWrapText`: every row is typed character by
    /// character (10 ms per character, `menu.cpp:795/816`). A `dt = 0` frame draws
    /// the complete page, so QA screenshots stay deterministic.
    ///
    /// Each character is drawn through `Text_TextLine` with `Fonts_BrightSilverColor`
    /// (`text.cpp:840`: `Text_TextLine(window, character, …, Fonts_BrightSilverColor)`),
    /// i.e. fill `0xA2` **plus** the outline `0xAC`/shadow `0xC7` relief — the typed
    /// text must not be a flat single-colour fill.
    fn draw_text(&self, frame: &mut RgbaBuffer, menu: Option<&MenuAssets>) -> u32 {
        let font = GnwFontId::Font1.font();
        let line_height = font.line_height();
        let rows = self.visible_rows();

        if rows.is_empty() {
            return 0;
        }

        let top = STORY_TEXT_RECT.uly
            + (STORY_TEXT_HEIGHT + 1 - rows.len() as i32 * line_height).max(0) / 2;
        let (base, outline, shadow) = match menu {
            Some(menu) => (
                menu.palette.rgba(SILVER.0),
                menu.palette.rgba(SILVER.1),
                menu.palette.rgba(SILVER.2),
            ),
            None => (
                Rgba::rgb(SILVER.0, SILVER.0, SILVER.0),
                Rgba::rgb(SILVER.1, SILVER.1, SILVER.1),
                Rgba::rgb(SILVER.2, SILVER.2, SILVER.2),
            ),
        };
        // Complete page until the first positive `dt` (deterministic freeze-frame).
        let mut budget = if self.typing_started {
            self.typed
        } else {
            usize::MAX
        };
        let mut painted = 0u32;

        for (row, line) in rows.iter().enumerate() {
            if line.is_empty() || budget == 0 {
                continue;
            }

            let shown: String = if budget >= line.chars().count() {
                line.clone()
            } else {
                line.chars().take(budget).collect()
            };

            budget = budget.saturating_sub(line.chars().count());

            painted += draw_text_gnw_shadowed(
                frame,
                STORY_TEXT_RECT.ulx,
                top + row as i32 * line_height,
                font,
                &shown,
                base,
                outline,
                shadow,
            )
            .max(0) as u32;
        }

        painted
    }

    /// `ENDOK` at (293,458) labelled "OK".
    fn draw_ok(&self, frame: &mut RgbaBuffer, menu: Option<&MenuAssets>) -> u32 {
        let font = GnwFontId::Font5.font();
        let (base, outline, shadow) = match menu {
            Some(menu) => (
                menu.palette.rgba(SILVER.0),
                menu.palette.rgba(0x00),
                menu.palette.rgba(0x00),
            ),
            None => (
                Rgba::rgb(SILVER.0, SILVER.1, SILVER.2),
                Rgba::BLACK,
                Rgba::BLACK,
            ),
        };
        let mut painted = 0u32;

        match menu {
            Some(menu) => {
                if let Some(pair) = menu.endok.as_ref() {
                    painted += blit_simple(
                        frame,
                        &menu.palette,
                        pair.image(false),
                        STORY_OK.ulx,
                        STORY_OK.uly,
                        true,
                    );
                } else {
                    frame.fill_rect(STORY_OK, Rgba::rgb(24, 28, 34));
                }
            }
            None => {
                frame.fill_rect(STORY_OK, Rgba::rgb(24, 28, 34));
            }
        }

        painted += draw_text_gnw_centered_shadowed(
            frame,
            STORY_OK,
            font,
            STORY_OK_CAPTION,
            base,
            outline,
            shadow,
        )
        .max(0) as u32;

        painted
    }

    /// Page counter and the visible fallback diagnostics.
    fn draw_footer(&self, frame: &mut RgbaBuffer, menu: Option<&MenuAssets>) -> u32 {
        let font = GnwFontId::Font5.font();
        let mut painted = 0u32;

        if self.page_count() > 1 {
            let green = match menu {
                Some(menu) => menu.palette.rgba(GREEN),
                None => Rgba::rgb(0, 0, 0),
            };
            let text = format!("Page {}/{} (Up/Down)", self.page + 1, self.page_count());

            painted += draw_text_gnw(
                frame,
                STORY_PAGE_LINE.ulx,
                STORY_PAGE_LINE.uly,
                font,
                &clean_status(&text),
                green,
            )
            .max(0) as u32;
        }

        if !self.diagnostics.is_empty() {
            let silver = match menu {
                Some(menu) => menu.palette.rgba(SILVER.0),
                None => Rgba::rgb(SILVER.0, SILVER.1, SILVER.2),
            };
            let text = clean_status(&self.diagnostics.join("; "));
            let text = truncate_to_width(font, &text, STORY_STATUS.width);

            // The panel keeps the diagnostics readable over any background
            // (deviation: the oracle has no status line here).
            frame.fill_rect(STORY_STATUS, Rgba::BLACK);
            painted += draw_text_gnw(
                frame,
                STORY_STATUS.ulx + 2,
                STORY_STATUS.uly + 2,
                font,
                &text,
                silver,
            )
            .max(0) as u32;
        }

        painted
    }
}

/// `ENDOK` hit (`StoryScreen::input` uses it for the click).
pub fn ok_at(x: i32, y: i32) -> bool {
    STORY_OK.contains(x, y)
}

/// Wraps `text` to [`STORY_TEXT_WIDTH`] with `GNW_TEXT_FONT_1` (`Text_SplitText`).
pub fn wrap_story_text(text: &str) -> Vec<String> {
    let font = GnwFontId::Font1.font();
    let mut lines = Vec::new();

    for paragraph in text.split('\n') {
        let mut line = String::new();

        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };

            if font.text_width(&candidate) > STORY_TEXT_WIDTH && !line.is_empty() {
                lines.push(clean_status(&line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }

        lines.push(clean_status(&line));
    }

    // The shipped descriptors end with blank lines (`…\n\n`); they are paragraph
    // separators, not an empty page. `Text_SplitText` drops the trailing rows too.
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    lines
}

/// Cuts `text` so it fits `width` pixels of `font`, appending an ellipsis.
fn truncate_to_width(font: &max_render::text::GnwFont, text: &str, width: i32) -> String {
    if font.text_width(text) <= width {
        return text.to_string();
    }

    let mut output = String::new();

    for character in text.chars() {
        let mut candidate = output.clone();
        candidate.push(character);

        if font.text_width(&format!("{candidate}...")) > width {
            break;
        }

        output = candidate;
    }

    output.push_str("...");

    output
}

// ---------------------------------------------------------------------------
// Story media resolution (`Mission_GetOptionalMedia`, `mission.cpp:118-156`)
// ---------------------------------------------------------------------------

/// Resolves `story.background` to a resource tag and a decoded picture.
///
/// The oracle picks a random id of the list (`Randomizer_Generate`); the port picks
/// deterministically from `seed` and falls through every id when a resource cannot
/// be decoded (a visible diagnostic instead of an empty screen).
fn resolve_background(
    media: Option<&Media>,
    seed: usize,
    max_res: Option<&ResArchive>,
    data_dir: &Path,
    diagnostics: &mut Vec<String>,
) -> (Option<String>, Option<BigImage>) {
    let Some(media) = media else {
        return (None, None);
    };
    let Some(resource) = media.resource.as_ref() else {
        return (None, None);
    };

    match resource {
        MediaResource::Ids(ids) => {
            if ids.is_empty() {
                return (None, None);
            }

            let start = seed % ids.len();

            for offset in 0..ids.len() {
                let id = &ids[(start + offset) % ids.len()];
                let Some(max_res) = max_res else {
                    diagnostics.push(format!(
                        "story background {id}: MAX.RES is not loaded - \
                         briefing background fallback"
                    ));
                    break;
                };

                match max_res
                    .read(id)
                    .ok()
                    .and_then(|bytes| BigImage::decode(bytes).ok())
                {
                    Some(image) => return (Some(id.clone()), Some(image)),
                    None => diagnostics.push(format!(
                        "story background {id}: the resource is missing or not a picture"
                    )),
                }
            }

            (None, None)
        }
        MediaResource::Files(files) => {
            for file in files {
                let Some(path) = resolve_media_file(data_dir, file) else {
                    diagnostics.push(format!(
                        "story background {file}: no such file in {}",
                        data_dir.display()
                    ));
                    continue;
                };

                match fs::read(&path)
                    .ok()
                    .and_then(|bytes| BigImage::decode(&bytes).ok())
                {
                    Some(image) => return (Some(path.display().to_string()), Some(image)),
                    None => diagnostics.push(format!(
                        "story background {}: the file is not a picture",
                        path.display()
                    )),
                }
            }

            (None, None)
        }
    }
}

/// Resolves the `story.video` entries to files next to `MAX.RES`.
///
/// The oracle's resource body is an ASCIIZ file name (`DEMO2FLC` → `maxmve2.mve`,
/// `ResourceManager_OpenFileResource`), so an id is followed by
/// [`ResArchive::file_reference`]. File entries are relative to the data directory.
fn resolve_videos(
    media: Option<&Media>,
    max_res: Option<&ResArchive>,
    data_dir: &Path,
    diagnostics: &mut Vec<String>,
) -> Vec<PathBuf> {
    let Some(media) = media else {
        return Vec::new();
    };
    let Some(resource) = media.resource.as_ref() else {
        return Vec::new();
    };

    let mut videos = Vec::new();

    match resource {
        MediaResource::Ids(ids) => {
            for id in ids {
                let Some(max_res) = max_res else {
                    diagnostics.push(format!(
                        "story video {id}: MAX.RES is not loaded - the video is skipped"
                    ));
                    continue;
                };
                let Some(name) = max_res.file_reference(id) else {
                    diagnostics.push(format!(
                        "story video {id}: the resource is not a file reference"
                    ));
                    continue;
                };

                match resolve_media_file(data_dir, name) {
                    Some(path) => videos.push(path),
                    None => diagnostics.push(format!(
                        "story video {id} -> {name}: the file is missing in {}",
                        data_dir.display()
                    )),
                }
            }
        }
        MediaResource::Files(files) => {
            for file in files {
                match resolve_media_file(data_dir, file) {
                    Some(path) => videos.push(path),
                    None => diagnostics.push(format!(
                        "story video {file}: no such file in {}",
                        data_dir.display()
                    )),
                }
            }
        }
    }

    videos
}

/// Resolves `story.music` to a resource tag for [`crate::music::MusicDirector`].
///
/// `Media.resource.file` is a `/// \todo` in the oracle (`menu.cpp:791-793`), so a
/// file entry is a visible diagnostic; the tag form is what the shipped missions use.
fn resolve_music(media: Option<&Media>, diagnostics: &mut Vec<String>) -> Option<String> {
    let media = media?;
    let resource = media.resource.as_ref()?;

    match resource {
        MediaResource::Ids(ids) => ids.first().cloned(),
        MediaResource::Files(files) => {
            diagnostics.push(format!(
                "story music file {}: the file form is not implemented by the oracle",
                files.join(", ")
            ));
            None
        }
    }
}

/// Finds `name` in `data_dir` (lowercase, uppercase, exact, then case-insensitively —
/// the same order as `ResourceManager_GetFileResourcePath`).
fn resolve_media_file(data_dir: &Path, name: &str) -> Option<PathBuf> {
    for candidate in [
        name.to_ascii_lowercase(),
        name.to_ascii_uppercase(),
        name.to_string(),
    ] {
        let path = data_dir.join(&candidate);

        if path.is_file() {
            return Some(path);
        }
    }

    // A path with sub-directories is not scanned for a case-insensitive match.
    if name.contains(['/', '\\']) {
        return None;
    }

    let entries = fs::read_dir(data_dir).ok()?;
    let mut matches: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
        })
        .map(|entry| entry.path())
        .collect();

    matches.sort();

    matches.into_iter().next()
}

#[cfg(test)]
#[path = "story_tests.rs"]
mod tests;
