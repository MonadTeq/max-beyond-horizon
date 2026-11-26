//! Recording a `.qa` scenario manually (QA layer 2.5): player input in the window → driver
//! commands of [`crate::qa::runner`].
//!
//! Idea: in `main.rs` the SDL event is already translated into [`InputEvent`] **in pixels
//! of the 640×480 frame** (`SdlWindow::poll_input` does the reverse window scale
//! conversion), so the recorder needs neither SDL nor knowledge of the window layout. It
//! receives the same event stream that goes to `Screen::handle_input` /
//! `handle_session_input`, and turns it into `.qa` language lines:
//!
//! | Input | Command |
//! |---|---|
//! | LMB at a frame point | `click X Y` |
//! | RMB at a frame point | `rclick X Y` |
//! | Enter/Esc/Tab/Backspace/arrows | `key ENTER`, `key ESC`, … |
//! | printable character outside text input | `key A` (exactly the press) |
//! | characters inside an open text input (chat: T/Enter in a session) | one `type "…"` |
//! | pause between events | `wait N`, `N = min(frames, cap)` |
//!
//! # Why the recording is reproducible
//!
//! Coordinates are frame pixels, not window pixels: with `--scale 3` a click in the
//! 1920×1440 window is still recorded as a 640×480 point. The script is run by the same
//! `qa_run` and the same initial state (same planet/seed/settings),
//! so it goes through the same player path.
//!
//! # Limitations (deliberate)
//!
//! * **`type` is only insertion of characters**: Backspace is not part of the `type`
//!   grammar. The recorder flushes the accumulated text as a separate `type`, and
//!   Backspace itself is written as `key BACKSPACE`; the input order is preserved, but
//!   a deletion "in the middle" is reproduced as a separate command rather than as
//!   an edit inside `type`.
//! * **Mouse movement is not recorded**: `MouseMove` does not affect the game state
//!   (only highlighting), otherwise the script would swell by a thousand lines.
//! * **`Key::Other`** (modifiers, F-keys) has no command in the language —
//!   it is skipped.
//! * **Space and control characters outside text input** are not expressible
//!   by a single-letter `key` — they are skipped (inside `type` a space is written).
//! * **A click outside the frame** (black bars when scaling) yields `(-1, -1)`
//!   from `poll_input` and is not written — the parser would reject such coordinates.
//! * **A network session** can be recorded, but `qa_run` does not bring up the network:
//!   local matches are reproducible (`--ui` → New Game). For the network the recording is
//!   a reference artifact.
//! * If the exit happened with an open text input, the file ends with
//!   `type` without a closing `key` — on replay the line will stay open;
//!   the QA engineer adds `key ENTER`/`key ESC` manually.
//!
//! The file header is comments (`#`), so `qa::script` parses the file
//! as a whole, without preprocessing.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::qa::script::{FRAME_HEIGHT, FRAME_WIDTH};
use crate::ui::{InputEvent, Key};

/// How many pause frames at most are written into `wait N`.
///
/// The player "thinks" in seconds, while a CI run must not stretch: 120 frames is
/// about 2 seconds at 60 FPS. Anything longer is clipped to this value.
pub const DEFAULT_PAUSE_CAP: u32 = 120;

/// What the recorder knows about the UI state at the moment of the event.
///
/// `text_open` — the authoritative sign of an open text input
/// (`App::hud().chat.is_active()` in a session). While it is `true`, printable
/// characters accumulate into one `type` command; opening/closing the input (`T`/`Enter`/`Esc`)
/// remains separate `key`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecordContext {
    /// A match is in progress (`Screen::InSession` / network `FrameSink`).
    pub in_session: bool,
    /// The text input was open **before** this event.
    pub text_open: bool,
}

/// Recording of player actions into a `.qa` file.
///
/// Writes line by line and flushes the buffer to disk after each command: if the
/// application crashes, the already recorded steps remain in the file. [`Recorder::finish`]
/// appends the tail (an unclosed `type`, the pause after the last event,
/// the final comment).
pub struct Recorder {
    path: Option<PathBuf>,
    out: Option<BufWriter<File>>,
    header: Vec<String>,
    commands: Vec<String>,
    frame: u64,
    last_command_frame: Option<u64>,
    text: String,
    text_start_frame: Option<u64>,
    text_last_frame: Option<u64>,
    pause_cap: u32,
    events: usize,
    write_error: Option<String>,
    finished: bool,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Recorder")
            .field("path", &self.path)
            .field("frame", &self.frame)
            .field("events", &self.events)
            .field("commands", &self.commands.len())
            .finish()
    }
}

impl Recorder {
    /// Creates the recording file.
    ///
    /// If `path` already exists, a free name with a suffix is taken:
    /// `run.qa` → `run-1.qa` → `run-2.qa` … (an existing file is never
    /// overwritten without explicit user consent).
    pub fn new(path: impl Into<PathBuf>) -> io::Result<Self> {
        let requested = path.into();
        let path = unique_path(&requested);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let mut recorder = Self::with_path(Some(path), Some(BufWriter::new(file)));

        recorder.write_header()?;

        Ok(recorder)
    }

    /// Recorder into memory (for tests and APIs without a file).
    pub fn in_memory() -> Self {
        Self::with_path(None, None)
    }

    fn with_path(path: Option<PathBuf>, out: Option<BufWriter<File>>) -> Self {
        Self {
            path,
            out,
            header: header_lines(),
            commands: Vec::new(),
            frame: 0,
            last_command_frame: None,
            text: String::new(),
            text_start_frame: None,
            text_last_frame: None,
            pause_cap: DEFAULT_PAUSE_CAP,
            events: 0,
            write_error: None,
            finished: false,
        }
    }

    /// Another pause cap (frames; `0` — do not write pauses).
    pub fn with_pause_cap(mut self, cap: u32) -> Self {
        self.pause_cap = cap;
        self
    }

    /// The file being recorded into (`None` — recording into memory).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// File header (comment lines without a trailing `\n`).
    pub fn header(&self) -> &[String] {
        &self.header
    }

    /// Recorded commands (without the header and the final comment).
    pub fn commands(&self) -> &[String] {
        &self.commands
    }

    /// How many significant events the recorder has seen.
    pub fn events(&self) -> usize {
        self.events
    }

    /// Current frame number (for diagnostics/tests).
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// The disk write error, if any (the first one).
    pub fn write_error(&self) -> Option<&str> {
        self.write_error.as_deref()
    }

    /// A new application frame has started.
    ///
    /// Called exactly once per game loop iteration: pauses in `wait`
    /// are computed as the difference of frame numbers between events.
    pub fn begin_frame(&mut self) {
        self.frame = self.frame.saturating_add(1);
    }

    /// Records one input event.
    ///
    /// `context` must reflect the UI state **before** the event is applied
    /// (in `main.rs` — `app.hud().chat.is_active()`). Insignificant events
    /// (mouse movement, a click outside the frame, `Key::Other`) create no commands and do not
    /// shift the pause countdown.
    pub fn record(&mut self, event: InputEvent, context: RecordContext) {
        match event {
            InputEvent::MouseMove(..) | InputEvent::MouseRelease(..) => {}
            // While the chat is open, the UI itself ignores the mouse — so we do not write it either.
            InputEvent::MouseClick(..) | InputEvent::RightClick(..) if context.text_open => {}
            InputEvent::MouseClick(x, y) => self.record_click("click", x, y),
            InputEvent::RightClick(x, y) => self.record_click("rclick", x, y),
            InputEvent::Key(key) if context.text_open => self.record_text_key(key),
            InputEvent::Key(key) => self.record_key(key),
        }
    }

    /// Finishes the recording: flushes the open `type`, writes the pause after
    /// the last event and the final comment.
    ///
    /// Returns the path of the recorded file (`None` for recording into memory).
    pub fn finish(&mut self) -> io::Result<Option<PathBuf>> {
        if !self.finished {
            self.flush_text();
            self.flush_trailing_wait();

            let summary = format!(
                "# конец записи: событий {}, команд {}, кадров {}",
                self.events,
                self.commands.len(),
                self.frame
            );
            self.push_comment(summary);

            self.finished = true;
        }

        if let Some(out) = self.out.as_mut() {
            out.flush()?;
        }

        if let Some(error) = self.write_error.take() {
            return Err(io::Error::other(error));
        }

        Ok(self.path.clone())
    }

    // -----------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------

    fn record_click(&mut self, name: &str, x: i32, y: i32) {
        if !(0..FRAME_WIDTH).contains(&x) || !(0..FRAME_HEIGHT).contains(&y) {
            return;
        }

        self.flush_text();
        self.push_timed(format!("{name} {x} {y}"));
        self.events += 1;
    }

    fn record_key(&mut self, key: Key) {
        let Some(line) = key_line(key) else {
            return;
        };

        self.flush_text();
        self.push_timed(line);
        self.events += 1;
    }

    fn record_text_key(&mut self, key: Key) {
        match key {
            Key::Char(character) if is_typeable(character) => {
                self.push_text_char(character);
            }
            Key::Char(_) => {}
            Key::Backspace => {
                self.flush_text();
                self.push_timed("key BACKSPACE".to_string());
                self.events += 1;
            }
            Key::Enter => {
                self.flush_text();
                self.push_timed("key ENTER".to_string());
                self.events += 1;
            }
            Key::Escape => {
                self.flush_text();
                self.push_timed("key ESC".to_string());
                self.events += 1;
            }
            Key::Tab | Key::Up | Key::Down | Key::Left | Key::Right | Key::PageUp | Key::PageDown => {
                if let Some(line) = key_line(key) {
                    self.flush_text();
                    self.push_timed(line);
                    self.events += 1;
                }
            }
            Key::Alt(_) => {}
            Key::Other => {}
        }
    }

    fn push_text_char(&mut self, character: char) {
        if self.text.is_empty() {
            self.text_start_frame = Some(self.frame);
        }

        self.text.push(character);
        self.text_last_frame = Some(self.frame);
        self.events += 1;
    }

    /// Flushes the accumulated text into `type "…"`.
    ///
    /// The pause before typing starts is written **before** `type`, and the pause after
    /// the last character stays with the next command — this way the replay
    /// preserves both "thought for a long time before typing" and "thought for a long time after".
    fn flush_text(&mut self) {
        if self.text.is_empty() {
            return;
        }

        let text = std::mem::take(&mut self.text);
        let Some(start) = self.text_start_frame.take() else {
            return;
        };
        let end = self.text_last_frame.take().unwrap_or(start);

        self.push_wait_to(start);
        // parse_text keeps quotes inside the text: it strips only the
        // outer ones, so `type "…"` encodes any ASCII from the chat.
        self.push_line(format!("type \"{text}\""));
        self.last_command_frame = Some(end);
    }

    fn push_timed(&mut self, line: String) {
        self.push_wait_to(self.frame);
        self.push_line(line);
        self.last_command_frame = Some(self.frame);
    }

    fn push_wait_to(&mut self, target: u64) {
        let Some(last) = self.last_command_frame else {
            return;
        };

        let delta = target.saturating_sub(last);

        if delta == 0 || self.pause_cap == 0 {
            return;
        }

        self.push_line(format!("wait {}", delta.min(u64::from(self.pause_cap))));
    }

    fn flush_trailing_wait(&mut self) {
        self.push_wait_to(self.frame);
    }

    // -----------------------------------------------------------------------
    // File
    // -----------------------------------------------------------------------

    fn write_header(&mut self) -> io::Result<()> {
        let header = self.header.clone();

        for line in &header {
            self.write_raw(line)?;
        }

        Ok(())
    }

    fn push_line(&mut self, line: String) {
        self.commands.push(line.clone());
        self.push_comment(line);
    }

    /// Comment/header: written to the file but not to [`Self::commands`].
    fn push_comment(&mut self, line: String) {
        if self.write_error.is_none() {
            if let Err(error) = self.write_raw(&line) {
                self.write_error = Some(error.to_string());
                self.out = None;
            }
        }
    }

    fn write_raw(&mut self, line: &str) -> io::Result<()> {
        let Some(out) = self.out.as_mut() else {
            return Ok(());
        };

        writeln!(out, "{line}")?;
        out.flush()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish();
        }
    }
}

/// The `key` line for a key, or `None` if the language does not express it.
fn key_line(key: Key) -> Option<String> {
    let name = match key {
        Key::Enter => "ENTER",
        Key::Escape => "ESC",
        Key::Tab => "TAB",
        Key::Backspace => "BACKSPACE",
        Key::Up => "UP",
        Key::Down => "DOWN",
        Key::Left => "LEFT",
        Key::Right => "RIGHT",
        Key::PageUp => "PAGEUP",
        Key::PageDown => "PAGEDOWN",
        Key::Char(character) if character.is_ascii_graphic() => {
            return Some(format!("key {character}"));
        }
        Key::Char(_) | Key::Alt(_) | Key::Other => return None,
    };

    Some(format!("key {name}"))
}

/// The character can be written inside `type "…"`: printable ASCII (space included).
fn is_typeable(character: char) -> bool {
    character.is_ascii() && !character.is_ascii_control()
}

/// A free name: an existing file is not overwritten, `-N` is appended.
fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("record");
    let extension = path.extension().and_then(|extension| extension.to_str());

    for index in 1..10_000 {
        let name = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        let candidate = parent.join(name);

        if !candidate.exists() {
            return candidate;
        }
    }

    path.to_path_buf()
}

fn header_lines() -> Vec<String> {
    vec![
        "# QA-сценарий записан вручную (max_app --ui --record)".to_string(),
        format!("# дата: {}", utc_timestamp()),
        format!("# билд: {}", build_label()),
        format!(
            "# кадр: {}x{}; координаты кликов — пиксели кадра",
            FRAME_WIDTH, FRAME_HEIGHT
        ),
        "# формат: max-brh-qa/drivers/README.md, раздел «Запись сценариев вручную»".to_string(),
        "# реплей: cargo run -p max_app --example qa_run -- <этот файл> <out_dir>".to_string(),
        "# ассерты и shot QA-инженер добавляет вручную после записи".to_string(),
    ]
}

fn build_label() -> String {
    format!(
        "max_app {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// UTC time `YYYY-MM-DD HH:MM:SS UTC` without external crates.
fn utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let days = (seconds / 86_400) as i64;
    let time = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);

    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        time / 3600,
        (time / 60) % 60,
        time % 60
    )
}

/// Days from 1970-01-01 → (year, month, day) by Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = (days - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;

    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
#[path = "recorder_tests.rs"]
mod tests;
