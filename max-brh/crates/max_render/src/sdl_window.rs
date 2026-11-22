//! Thin SDL3 layer: window, frame texture, events.
//!
//! The module builds only under the `sdl` feature (see `Cargo.toml`), so the
//! pure part of the crate and all its tests work without SDL3 installed.
//! There is no game logic and no drawing as such here: the layer takes a ready
//! [`RgbaBuffer`], uploads it into a streaming texture and hands it to the window.
//!
//! Why SDL3 and not SDL2: the C++ port (`max-cpp-port`) and the original game
//! use SDL3, so the window and input match the reference by API. The
//! `sdl3` crate (crates.io) requires either a system SDL3 (`libsdl3-dev`, vcpkg) or
//! a build from source:
//!
//! ```text
//! # option 1 — system SDL3 (Linux: apt install libsdl3-dev; Windows: vcpkg):
//! cargo run -p max_render --features sdl --example window
//!
//! # option 2 — SDL3 is built from source (cmake required; docker/CI):
//! cargo run -p max_render --features sdl-from-source --example window
//! ```
//!
//! Running without the `sdl` feature gives a compilation error in the calling code
//! (the module simply does not exist) — this is deliberate: SDL must not leak into pure tests.

use std::collections::HashMap;

use sdl3::event::WindowEvent;
use sdl3::keyboard::{Keycode, Mod};
use sdl3::mouse::{Cursor, MouseButton};
use sdl3::pixels::PixelFormat;
use sdl3::rect::Rect as SdlRect;
use sdl3::render::{Canvas, ScaleMode, TextureCreator};
use sdl3::surface::Surface;
use sdl3::video::{Window, WindowContext};
use sdl3::EventPump;

use crate::buffer::RgbaBuffer;
use crate::frame_scale::FrameScale;

/// A window key in UI terms: `max_app` does not depend on SDL types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKey {
    /// Esc.
    Escape,
    /// Enter / KpEnter.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Printable ASCII character (SDL reports the base layout: `a`, `0`, `.`).
    Char(char),
    /// Printable ASCII character with Alt held (`Alt+P` — pause in the game).
    Alt(char),
    /// Any other key (modifiers, F-keys, …).
    Other,
}

/// Window event suitable for a UI loop without leaking SDL types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowInput {
    /// Key press (auto-repeat filtered out).
    Key(WindowKey),
    /// Mouse movement in window pixels.
    MouseMove {
        /// x coordinate.
        x: i32,
        /// y coordinate.
        y: i32,
    },
    /// Left mouse button press.
    MouseClick {
        /// x coordinate.
        x: i32,
        /// y coordinate.
        y: i32,
    },
    /// Right mouse button press (move order/cancel in the game).
    MouseRightClick {
        /// x coordinate.
        x: i32,
        /// y coordinate.
        y: i32,
    },
}

/// Translates an SDL key into [`WindowKey`].
fn window_key(code: Keycode) -> WindowKey {
    window_key_mod(code, Mod::empty())
}

/// [`window_key`] with modifiers: `Alt+<printable>` → [`WindowKey::Alt`]
/// (C++ `GNW_KB_KEY_LALT_P`).
fn window_key_mod(code: Keycode, modifiers: Mod) -> WindowKey {
    let alt = modifiers.intersects(Mod::LALTMOD | Mod::RALTMOD);

    if alt {
        if let raw @ 0x20..=0x7E = code as u32 {
            return WindowKey::Alt(char::from_u32(raw).unwrap_or('?'));
        }
    }

    match code {
        Keycode::Escape => WindowKey::Escape,
        Keycode::Return | Keycode::Return2 | Keycode::KpEnter => WindowKey::Enter,
        Keycode::Tab | Keycode::KpTab => WindowKey::Tab,
        Keycode::Backspace | Keycode::KpBackspace => WindowKey::Backspace,
        Keycode::Up => WindowKey::Up,
        Keycode::Down => WindowKey::Down,
        Keycode::Left => WindowKey::Left,
        Keycode::Right => WindowKey::Right,
        Keycode::PageUp => WindowKey::PageUp,
        Keycode::PageDown => WindowKey::PageDown,
        _ => {
            // The basic SDL3 ASCII codes match ASCII (`A = 97`, `_0 = 48`).
            match code as u32 {
                raw @ 0x20..=0x7E => WindowKey::Char(char::from_u32(raw).unwrap_or('?')),
                _ => WindowKey::Other,
            }
        }
    }
}

/// Window/frame output initialization error.
#[derive(Debug)]
pub struct WindowError {
    /// The stage at which the error occurred (`init`, `video`, `window`, …).
    pub stage: &'static str,
    /// SDL message.
    pub message: String,
}

impl WindowError {
    fn new(stage: &'static str, error: impl std::fmt::Display) -> Self {
        Self {
            stage,
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SDL3 ({}): {}", self.stage, self.message)
    }
}

impl std::error::Error for WindowError {}

/// SDL3 window with a streaming texture for an RGBA8 frame.
///
/// The texture is created on every [`SdlWindow::present`] — the `sdl3` crate yields a
/// `Texture<'creator>` that cannot be stored next to its `TextureCreator` in
/// one structure without `unsafe` (and `unsafe` is forbidden in this crate). For
/// the client this does not matter: SDL reuses the driver buffer, and the frame is
/// redrawn entirely anyway.
pub struct SdlWindow {
    canvas: Canvas<Window>,
    creator: TextureCreator<WindowContext>,
    events: EventPump,
    title: String,
    /// Logical frame size (640×480 in UI mode).
    base_size: (u32, u32),
    /// Current layout of the frame in the window (integer scale + bars).
    frame_scale: FrameScale,
    /// Cursor cache: key (`CursorId | frame << 8`) → SDL cursor.
    cursors: HashMap<u32, Cursor>,
    /// Which key is currently installed (the SDL cursor lives in this cache).
    active_cursor: Option<u32>,
    /// The SDL context is kept last: Rust fields are dropped in declaration
    /// order, so the window/cursors are destroyed **before** `SDL_Quit`.
    _sdl: sdl3::Sdl,
}

impl SdlWindow {
    /// Creates a `width × height` window with the title `title`.
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, WindowError> {
        Self::new_scaled(title, width, height, Some(1))
    }

    /// Creates a `width*scale × height*scale` window; `scale = None` — pick an
    /// integer scale for the desktop (the default, item F).
    ///
    /// The window is resizable; [`Self::present`] recomputes
    /// [`FrameScale`] every frame and outputs the frame nearest-neighbour with black bars.
    pub fn new_scaled(
        title: &str,
        width: u32,
        height: u32,
        scale: Option<u32>,
    ) -> Result<Self, WindowError> {
        let width = width.max(1);
        let height = height.max(1);
        let sdl = sdl3::init().map_err(|error| WindowError::new("init", error))?;
        let video = sdl
            .video()
            .map_err(|error| WindowError::new("video", error))?;
        let scale = match scale {
            Some(scale) => scale.clamp(1, crate::frame_scale::MAX_SCALE),
            None => {
                let bounds = video
                    .get_primary_display()
                    .ok()
                    .and_then(|display| display.get_usable_bounds().ok());
                let auto = bounds
                    .map(|rect| (rect.width() / width).min(rect.height() / height))
                    .unwrap_or(1);

                auto.clamp(1, 4)
            }
        };
        let window_width = width * scale;
        let window_height = height * scale;
        let window = video
            .window(title, window_width, window_height)
            .resizable()
            .position_centered()
            .build()
            .map_err(|error| WindowError::new("window", error))?;
        let canvas: Canvas<Window> = window.into_canvas();
        let creator = canvas.texture_creator();
        let events = sdl
            .event_pump()
            .map_err(|error| WindowError::new("event_pump", error))?;
        let (actual_width, actual_height) = canvas.window().size();

        Ok(Self {
            canvas,
            creator,
            events,
            title: title.to_owned(),
            base_size: (width, height),
            frame_scale: FrameScale::fit(actual_width, actual_height, width, height),
            cursors: HashMap::new(),
            active_cursor: None,
            _sdl: sdl,
        })
    }

    /// Window size in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.canvas.window().size()
    }

    /// Logical frame size.
    pub fn base_size(&self) -> (u32, u32) {
        self.base_size
    }

    /// Current integer frame scale.
    pub fn frame_scale(&self) -> FrameScale {
        self.frame_scale
    }

    /// Recomputes the layout for the current window size and frame size.
    pub fn update_frame_scale(&mut self, frame_width: u32, frame_height: u32) -> FrameScale {
        let (window_width, window_height) = self.size();

        if (frame_width, frame_height) != self.frame_scale.frame_size()
            || (window_width, window_height) != self.frame_scale.window_size()
        {
            self.base_size = (frame_width, frame_height);
            self.frame_scale =
                FrameScale::fit(window_width, window_height, frame_width, frame_height);
        }

        self.frame_scale
    }

    /// Window point → frame coordinates (for clicks at scale ≠ 1).
    pub fn window_to_frame(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        self.frame_scale.window_to_frame(x, y)
    }

    /// Window title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Changes the window title.
    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_owned();
        self.canvas.window_mut().set_title(title).ok();
    }

    /// Sets the cursor from an RGBA8 frame with a hotspot (`SDL_CreateColorCursor`).
    ///
    /// `key` is the frame identifier (`CursorId | frame << 8`): SDL cursors
    /// are cached, so a repeated call with the same key does nothing. Thus
    /// the animated `PTR_PATH` (10 frames) does not recreate the cursor every tick.
    /// Transparent pixels (`alpha == 0`) remain transparent.
    #[allow(clippy::too_many_arguments)]
    pub fn set_cursor_rgba(
        &mut self,
        key: u32,
        rgba: &[u8],
        width: u32,
        height: u32,
        hot_x: i32,
        hot_y: i32,
    ) -> Result<(), WindowError> {
        if self.active_cursor == Some(key) {
            return Ok(());
        }

        if !self.cursors.contains_key(&key) {
            if width == 0 || height == 0 {
                return Err(WindowError::new("cursor", "нулевой размер курсора"));
            }

            let expected = width as usize * height as usize * 4;

            if rgba.len() < expected {
                return Err(WindowError::new(
                    "cursor",
                    format!("кадр курсора короче: {} < {expected}", rgba.len()),
                ));
            }

            let mut surface = Surface::new(width, height, PixelFormat::RGBA32)
                .map_err(|error| WindowError::new("cursor_surface", error))?;
            let pitch = surface.pitch() as usize;
            let row_bytes = width as usize * 4;

            surface.with_lock_mut(|pixels: &mut [u8]| {
                for y in 0..height as usize {
                    let source = &rgba[y * row_bytes..(y + 1) * row_bytes];
                    let start = y * pitch;

                    if start + row_bytes <= pixels.len() {
                        pixels[start..start + row_bytes].copy_from_slice(source);
                    }
                }
            });

            let cursor = Cursor::from_surface(surface, hot_x, hot_y)
                .map_err(|error| WindowError::new("cursor_create", error))?;

            self.cursors.insert(key, cursor);
        }

        if let Some(cursor) = self.cursors.get(&key) {
            cursor.set();
            self.active_cursor = Some(key);
        }

        Ok(())
    }

    /// Outputs the frame: uploads the texture and presents it to the window nearest-neighbour.
    ///
    /// The frame is not stretched over the whole window: [`FrameScale`] computes an integer
    /// scale and centers it, and black bars remain around it. Thus every
    /// pixel of the 640×480 frame remains a square `N×N` block on any
    /// monitor (item F), and the mouse is translated back through
    /// [`Self::window_to_frame`].
    pub fn present(&mut self, buffer: &RgbaBuffer) -> Result<(), WindowError> {
        if buffer.is_empty() {
            return Ok(());
        }

        let scale = self.update_frame_scale(buffer.width(), buffer.height());
        let mut texture = self
            .creator
            .create_texture_streaming(PixelFormat::RGBA32, buffer.width(), buffer.height())
            .map_err(|error| WindowError::new("texture", error))?;

        texture.set_scale_mode(ScaleMode::Nearest);
        texture
            .update(None, buffer.pixels(), buffer.stride())
            .map_err(|error| WindowError::new("texture_update", error))?;

        self.canvas
            .set_draw_color(sdl3::pixels::Color::RGBA(0, 0, 0, 255));
        self.canvas.clear();

        let content = scale.content_rect();
        let destination = SdlRect::new(
            content.ulx,
            content.uly,
            content.width.max(1) as u32,
            content.height.max(1) as u32,
        );

        self.canvas
            .copy(&texture, None, destination)
            .map_err(|error| WindowError::new("copy", error))?;
        let _ = self.canvas.present();

        Ok(())
    }

    /// Fills the window with a color (e.g. before the first frame).
    pub fn clear(&mut self, color: crate::buffer::Rgba) {
        self.canvas.set_draw_color(sdl3::pixels::Color::RGBA(
            color.r, color.g, color.b, color.a,
        ));
        self.canvas.clear();
    }

    /// Polls the event queue and returns the events to the UI layer.
    ///
    /// Mouse coordinates are translated from the window to the logical frame through
    /// [`FrameScale`]: at scale 2 a click in a 1280×960 window arrives at the UI as
    /// a click in a 640×480 frame (item F). A click on a black bar outside the frame yields
    /// `(-1, -1)` — the UI ignores it.
    ///
    /// `running == false` means the window was closed (`Quit`); Esc does **not** close
    /// the window at this level — the UI decides itself (in the menu — exit, in a subscreen —
    /// back). For the old "Esc closes the window" behavior there is [`Self::poll`].
    pub fn poll_input(&mut self) -> (bool, Vec<WindowInput>) {
        let mut running = true;
        let mut inputs = Vec::new();
        // The window may have been resized since the previous frame — update the layout before
        // recomputing the mouse coordinates.
        let (window_width, window_height) = self.size();
        self.frame_scale = FrameScale::fit(
            window_width,
            window_height,
            self.base_size.0,
            self.base_size.1,
        );
        let frame_scale = self.frame_scale;

        for event in self.events.poll_iter() {
            match event {
                sdl3::event::Event::Quit { .. } => running = false,
                sdl3::event::Event::KeyDown {
                    keycode: Some(code),
                    repeat: false,
                    keymod,
                    ..
                } => inputs.push(WindowInput::Key(window_key_mod(code, keymod))),
                sdl3::event::Event::MouseMotion { x, y, .. } => {
                    let (x, y) = frame_scale
                        .window_to_frame(x.round() as i32, y.round() as i32)
                        .unwrap_or((-1, -1));
                    inputs.push(WindowInput::MouseMove { x, y });
                }
                sdl3::event::Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    let (x, y) = frame_scale
                        .window_to_frame(x.round() as i32, y.round() as i32)
                        .unwrap_or((-1, -1));
                    inputs.push(WindowInput::MouseClick { x, y });
                }
                sdl3::event::Event::MouseButtonDown {
                    mouse_btn: MouseButton::Right,
                    x,
                    y,
                    ..
                } => {
                    let (x, y) = frame_scale
                        .window_to_frame(x.round() as i32, y.round() as i32)
                        .unwrap_or((-1, -1));
                    inputs.push(WindowInput::MouseRightClick { x, y });
                }
                // P4 (TC-1107): alt-tab/minimize/restore do not
                // stop the loop. SDL3 sends Window events, no input comes
                // from them, the window stays alive, and the next frame is drawn as
                // usual. The branch is explicit so that the behavior is visible and covered by
                // the test [`window_event_is_input_neutral`].
                sdl3::event::Event::Window { win_event, .. }
                    if window_event_is_input_neutral(win_event) => {}
                _ => {}
            }
        }

        (running, inputs)
    }

    /// Polls the event queue.
    ///
    /// Returns `false` when it is time to close the window (window close or Esc).
    /// This is the former behavior of `--connect --sdl`; the UI loop uses
    /// [`Self::poll_input`] to handle Esc as navigation.
    pub fn poll(&mut self) -> bool {
        let (running, inputs) = self.poll_input();

        running
            && !inputs
                .iter()
                .any(|input| matches!(input, WindowInput::Key(WindowKey::Escape)))
    }
}

/// P4 (TC-1107): window events that carry no input and must not
/// stop the loop — focus loss/gain (alt-tab), minimize,
/// restore, show/hide/redraw, cursor enter/leave.
///
/// With them the window stays alive: `poll_input` returns `running = true`, and
/// the next frame is drawn as usual. An explicit predicate — so that this behavior
/// is visible in the code and is checked by a test without a real window.
pub fn window_event_is_input_neutral(event: WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::FocusLost
            | WindowEvent::FocusGained
            | WindowEvent::Minimized
            | WindowEvent::Restored
            | WindowEvent::Maximized
            | WindowEvent::Shown
            | WindowEvent::Hidden
            | WindowEvent::Exposed
            | WindowEvent::Occluded
            | WindowEvent::MouseEnter
            | WindowEvent::MouseLeave
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_and_minimize_events_are_input_neutral() {
        for event in [
            WindowEvent::FocusLost,
            WindowEvent::FocusGained,
            WindowEvent::Minimized,
            WindowEvent::Restored,
            WindowEvent::Maximized,
            WindowEvent::Shown,
            WindowEvent::Hidden,
            WindowEvent::Exposed,
            WindowEvent::Occluded,
            WindowEvent::MouseEnter,
            WindowEvent::MouseLeave,
        ] {
            assert!(
                window_event_is_input_neutral(event),
                "{event:?} не должен останавливать цикл или давать ввод"
            );
        }
    }

    #[test]
    fn resize_move_and_close_are_not_neutral_input_events() {
        for event in [
            WindowEvent::Resized(640, 480),
            WindowEvent::Moved(10, 20),
            WindowEvent::CloseRequested,
            WindowEvent::HitTest(1, 2),
        ] {
            assert!(
                !window_event_is_input_neutral(event),
                "{event:?} не должен считаться нейтральным"
            );
        }
    }
}

