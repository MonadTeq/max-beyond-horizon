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
//! # option 1 РІР‚вЂќ system SDL3 (Linux: apt install libsdl3-dev; Windows: vcpkg):
//! cargo run -p max_render --features sdl --example window
//!
//! # option 2 РІР‚вЂќ SDL3 is built from source (cmake required; docker/CI):
//! cargo run -p max_render --features sdl-from-source --example window
//! ```
//!
//! Running without the `sdl` feature gives a compilation error in the calling code
//! (the module simply does not exist) РІР‚вЂќ this is deliberate: SDL must not leak into pure tests.
//!
//! # High DPI, fullscreen and focus (task 1.4.2)
//!
//! The window is created with `SDL_WINDOW_HIGH_PIXEL_DENSITY`, so the renderer owns
//! a real pixel drawable: at 150% Windows scaling a 640Р“вЂ”480-point window has a
//! 960Р“вЂ”720-pixel output. [`FrameScale`] is computed from the **pixel** output
//! ([`ScreenGeometry`]), the frame stays nearest-neighbour and letterboxed, and a
//! mouse point is mapped points РІвЂ вЂ™ pixels РІвЂ вЂ™ frame. That keeps the picture crisp
//! instead of letting the OS stretch a 640Р“вЂ”480 back buffer bilinearly.
//!
//! `Alt+Enter` toggles borderless desktop fullscreen, as in the C++ oracle
//! (`Svga_ToggleFullscreen`, `input.cpp`); `--fullscreen` opens the window in it.
//! A fullscreen transition, a DPI/display-scale change, a resize and a monitor
//! move only recompute the geometry РІР‚вЂќ the logical 640Р“вЂ”480 frame is unchanged.
//!
//! Focus/minimize/occlusion change **drawing only** ([`PresentationState`]):
//! [`SdlWindow::present`] skips the upload while the window is hidden, but
//! [`SdlWindow::poll_input`] keeps returning `running = true` and no simulation,
//! pause or network state is touched (the oracle also keeps the loop alive on
//! `SDL_EVENT_WINDOW_FOCUS_LOST`; `input.cpp:277-288`).

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
use crate::presentation::{PresentationState, ScreenGeometry};
use crate::render_scale::{FilterMode, RenderScale};

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
    /// Printable ASCII character with Alt held (`Alt+P` РІР‚вЂќ pause in the game).
    Alt(char),
    /// Any other key (modifiers, F-keys, РІР‚В¦).
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
    /// Left mouse button release (ends the pressed picture, `GNW` `MOUSE_RELEASE`).
    MouseRelease {
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

/// Translates an SDL key with modifiers: `Alt+<printable>` РІвЂ вЂ™ [`WindowKey::Alt`]
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

/// `Alt+Enter` / `Alt+KpEnter` РІР‚вЂќ the fullscreen shortcut of the C++ oracle
/// (`input.cpp:307-310` РІвЂ вЂ™ `Svga_ToggleFullscreen`).
///
/// Handled in [`SdlWindow::poll_input`] as a presentation action: it is not
/// forwarded to the UI as a key, so a menu cannot interpret it as `Enter`.
pub fn is_toggle_fullscreen_shortcut(code: Keycode, modifiers: Mod) -> bool {
    let alt = modifiers.intersects(Mod::LALTMOD | Mod::RALTMOD);

    alt && matches!(code, Keycode::Return | Keycode::Return2 | Keycode::KpEnter)
}

/// Window/frame output initialization error.
#[derive(Debug)]
pub struct WindowError {
    /// The stage at which the error occurred (`init`, `video`, `window`, РІР‚В¦).
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
/// The texture is created on every [`SdlWindow::present`] РІР‚вЂќ the `sdl3` crate yields a
/// `Texture<'creator>` that cannot be stored next to its `TextureCreator` in
/// one structure without `unsafe` (and `unsafe` is forbidden in this crate). For
/// the client this does not matter: SDL reuses the driver buffer, and the frame is
/// redrawn entirely anyway.
pub struct SdlWindow {
    canvas: Canvas<Window>,
    creator: TextureCreator<WindowContext>,
    events: EventPump,
    title: String,
    /// Logical frame size (640Р“вЂ”480 in UI mode).
    base_size: (u32, u32),
    /// Presentation scale of the render frame (`PixelPerfect` = 640×480).
    render_scale: RenderScale,
    /// Window points / drawable pixels / frame layout in pixels.
    geometry: ScreenGeometry,
    /// Focus/minimize/occlusion РІР‚вЂќ drawing decisions only.
    presentation: PresentationState,
    /// The window is in (borderless desktop) fullscreen.
    fullscreen: bool,
    /// Display scale reported by SDL at the last layout refresh.
    display_scale: f32,
    /// Refresh rate of the display at creation (Hz; diagnostics only).
    display_refresh_rate: f32,
    /// A layout or visibility change asks for one extra frame.
    /// Cursor cache: key (`CursorId | frame << 8`) РІвЂ вЂ™ SDL cursor.
    cursors: HashMap<u32, Cursor>,
    /// Which key is currently installed (the SDL cursor lives in this cache).
    active_cursor: Option<u32>,
    /// The SDL context is kept last: Rust fields are dropped in declaration
    /// order, so the window/cursors are destroyed **before** `SDL_Quit`.
    _sdl: sdl3::Sdl,
}

impl SdlWindow {
    /// Creates a `width Р“вЂ” height` window with the title `title`.
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, WindowError> {
        Self::new_scaled(title, width, height, Some(1))
    }

    /// Creates a `width*scale Р“вЂ” height*scale` window (points); `scale = None` РІР‚вЂќ
    /// pick an integer scale for the desktop (the default, item F).
    ///
    /// The window requests `SDL_WINDOW_HIGH_PIXEL_DENSITY`: the drawable is the
    /// real pixel buffer (at 150% DPI a 640Р“вЂ”480-point window has 960Р“вЂ”720 pixels),
    /// and [`Self::present`] lays the frame out in those pixels nearest-neighbour
    /// with black bars. The window is resizable.
    pub fn new_scaled(
        title: &str,
        width: u32,
        height: u32,
        scale: Option<u32>,
    ) -> Result<Self, WindowError> {
        Self::new_with_render_scale(title, width, height, RenderScale::PixelPerfect, scale)
    }

    /// Creates a window for the logical `width × height` frame presented at
    /// `render_scale` (`max-brh-docs/development/remaster-hd.md` §R7).
    ///
    /// `width`/`height` are the **logical** frame size (640×480 in the client),
    /// never the render frame: the window opens at the render frame —
    /// [`crate::presentation::window_points_for`] gives `width·S × height·S`
    /// points — times the optional window `scale`, so the logical frame keeps its
    /// coordinates and [`Self::window_to_frame`] divides by `S` exactly once.
    /// `scale = None` picks an integer scale for the desktop.
    ///
    /// This is the constructor that knows the presentation scale. Feeding it a
    /// size that is already `×S` makes the geometry multiply it again
    /// (`640·S² × 480·S²`), which crops the interface to its centre in the window
    /// and shifts every click; [`Self::present`] guards the buffer against
    /// [`Self::render_frame_size`] in debug builds.
    pub fn new_with_render_scale(
        title: &str,
        width: u32,
        height: u32,
        render_scale: RenderScale,
        scale: Option<u32>,
    ) -> Result<Self, WindowError> {
        let width = width.max(1);
        let height = height.max(1);
        let factor = render_scale.scale().max(1);
        let render_width = width.saturating_mul(factor);
        let render_height = height.saturating_mul(factor);
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
                    .map(|rect| (rect.width() / render_width).min(rect.height() / render_height))
                    .unwrap_or(1);

                auto.clamp(1, 4)
            }
        };
        let (window_width, window_height) =
            crate::presentation::window_points_for(width, height, render_scale, scale);
        let window = video
            .window(title, window_width, window_height)
            .resizable()
            .high_pixel_density()
            .position_centered()
            .build()
            .map_err(|error| WindowError::new("window", error))?;
        let canvas: Canvas<Window> = window.into_canvas();
        let creator = canvas.texture_creator();
        let events = sdl
            .event_pump()
            .map_err(|error| WindowError::new("event_pump", error))?;
        let window_points = canvas.window().size();
        let output_pixels = canvas
            .output_size()
            .unwrap_or_else(|_| canvas.window().size_in_pixels());
        let display_scale = canvas.window().display_scale();
        let display_refresh_rate = canvas
            .window()
            .get_display()
            .ok()
            .and_then(|display| display.get_mode().ok())
            .map(|mode| mode.refresh_rate)
            .unwrap_or(0.0);

        Ok(Self {
            canvas,
            creator,
            events,
            title: title.to_owned(),
            base_size: (width, height),
            render_scale,
            geometry: ScreenGeometry::fit_with_render_scale(
                window_points,
                output_pixels,
                width,
                height,
                render_scale,
            ),
            presentation: PresentationState::new(),
            fullscreen: false,
            display_scale,
            display_refresh_rate,
            cursors: HashMap::new(),
            active_cursor: None,
            _sdl: sdl,
        })
    }

    /// Window size in screen coordinates (points): what SDL mouse events carry.
    pub fn size(&self) -> (u32, u32) {
        self.canvas.window().size()
    }

    /// Drawable size in pixels (`SDL_GetRenderOutputSize`).
    pub fn output_size(&self) -> (u32, u32) {
        self.canvas
            .output_size()
            .unwrap_or_else(|_| self.canvas.window().size_in_pixels())
    }

    /// Logical frame size.
    pub fn base_size(&self) -> (u32, u32) {
        self.base_size
    }

    /// Presentation scale of the render frame (`max-brh-docs/development/remaster-hd.md` §2.2).
    pub fn render_scale(&self) -> RenderScale {
        self.render_scale
    }

    /// Render frame size in pixels (`640·S × 480·S`).
    pub fn render_frame_size(&self) -> (u32, u32) {
        self.geometry.render_frame_size()
    }

    /// Switches between the pixel-perfect (`S = 1`) and HD (`S = 2/4`) presentation.
    ///
    /// Presentation only: the window layout is recomputed, the logical 640×480
    /// frame, the camera and the simulation are untouched. In HD mode the caller
    /// hands [`Self::present`] a render frame of [`Self::render_frame_size`] and
    /// the frame is uploaded with linear filtering; `PixelPerfect` keeps nearest
    /// neighbour and the historical 640×480 frame.
    /// [`Self::new_with_render_scale`] opens the window for a presentation scale
    /// once; this switch keeps the window as it is, and the render frame is laid
    /// out in it (never shrunk), so a larger scale may letterbox or crop until
    /// the window is resized.
    pub fn set_render_scale(&mut self, render_scale: RenderScale) {
        if self.render_scale == render_scale {
            return;
        }

        self.render_scale = render_scale;
        self.refresh_layout();
    }

    /// Current integer frame scale (in drawable pixels).
    pub fn frame_scale(&self) -> FrameScale {
        self.geometry.frame_scale()
    }

    /// Current geometry: window points, drawable pixels, frame layout.
    pub fn geometry(&self) -> ScreenGeometry {
        self.geometry
    }

    /// Pixels per point at the last layout refresh (`1.0` at 100% DPI).
    pub fn dpi_scale(&self) -> f32 {
        self.display_scale
    }

    /// Display refresh rate reported by SDL (Hz; `0.0` if unknown).
    pub fn display_refresh_rate(&self) -> f32 {
        self.display_refresh_rate
    }

    /// Whether the window currently has the input focus.
    pub fn is_focused(&self) -> bool {
        self.presentation.is_focused()
    }

    /// Whether the window is minimized.
    pub fn is_minimized(&self) -> bool {
        self.presentation.is_minimized()
    }

    /// Whether a frame should be uploaded to the window.
    ///
    /// A hidden/occluded window still runs its simulation and its network loop;
    /// only the GPU upload is skipped (task 1.4.2, alt-tab gate).
    pub fn should_present(&self) -> bool {
        self.presentation.should_draw()
    }

    /// Whether the window is in borderless desktop fullscreen.
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Enters or leaves borderless desktop fullscreen.
    ///
    /// Matches the oracle (`Svga_ToggleFullscreen`): the window is sized to the
    /// display bounds first, then switched, so leaving fullscreen restores a
    /// sane window. The logical frame is unchanged: only the geometry (and with
    /// it the integer scale and the letterbox) is recomputed.
    pub fn set_fullscreen(&mut self, fullscreen: bool) -> Result<(), WindowError> {
        if fullscreen == self.fullscreen {
            return Ok(());
        }

        if fullscreen {
            let bounds = self
                .canvas
                .window()
                .get_display()
                .ok()
                .and_then(|display| display.get_bounds().ok());

            if let Some(bounds) = bounds {
                self.canvas
                    .window_mut()
                    .set_size(bounds.width(), bounds.height())
                    .map_err(|error| WindowError::new("fullscreen_size", error))?;
            }
        }

        self.canvas
            .window_mut()
            .set_fullscreen(fullscreen)
            .map_err(|error| WindowError::new("fullscreen", error))?;
        self.fullscreen = fullscreen;
        self.refresh_layout();
        eprintln!(
            "SDL: fullscreen={fullscreen}, Р С•Р С”Р Р…Р С• {}x{} РЎвЂљР С•РЎвЂЎР ВµР С” / {}x{} Р С—Р С‘Р С”РЎРѓР ВµР В»Р ВµР в„–, Р СР В°РЎРѓРЎв‚¬РЎвЂљР В°Р В± {}",
            self.size().0,
            self.size().1,
            self.output_size().0,
            self.output_size().1,
            self.geometry.scale()
        );
        Ok(())
    }

    /// Toggles fullscreen; returns the new state.
    pub fn toggle_fullscreen(&mut self) -> Result<bool, WindowError> {
        self.set_fullscreen(!self.fullscreen)?;
        Ok(self.fullscreen)
    }

    /// Recomputes the layout for the current window size and **logical** frame size.
    ///
    /// The render frame follows from [`Self::render_scale`]; in HD mode the
    /// renderer must hand [`Self::present`] a buffer of
    /// [`Self::render_frame_size`].
    pub fn update_frame_scale(&mut self, frame_width: u32, frame_height: u32) -> FrameScale {
        let (window_width, window_height) = self.size();
        let output = self.output_size();

        if (frame_width, frame_height) != self.geometry.frame_size()
            || (window_width, window_height) != self.geometry.window_points()
            || output != self.geometry.output_pixels()
        {
            self.base_size = (frame_width, frame_height);
            self.geometry = ScreenGeometry::fit_with_render_scale(
                (window_width, window_height),
                output,
                frame_width,
                frame_height,
                self.render_scale,
            );
        }

        self.geometry.frame_scale()
    }

    /// Refreshes geometry from the current window/drawable sizes and display scale.
    fn refresh_layout(&mut self) {
        let (width, height) = self.base_size;

        self.display_scale = self.canvas.window().display_scale();
        self.geometry = ScreenGeometry::fit_with_render_scale(
            self.size(),
            self.output_size(),
            width,
            height,
            self.render_scale,
        );
    }

    /// Window point (points) РІвЂ вЂ™ frame coordinates (for clicks at scale РІвЂ°В  1).
    pub fn window_to_frame(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        self.geometry.window_to_frame(x, y)
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
                return Err(WindowError::new("cursor", "Р Р…РЎС“Р В»Р ВµР Р†Р С•Р в„– РЎР‚Р В°Р В·Р СР ВµРЎР‚ Р С”РЎС“РЎР‚РЎРѓР С•РЎР‚Р В°"));
            }

            let expected = width as usize * height as usize * 4;

            if rgba.len() < expected {
                return Err(WindowError::new(
                    "cursor",
                    format!("Р С”Р В°Р Т‘РЎР‚ Р С”РЎС“РЎР‚РЎРѓР С•РЎР‚Р В° Р С”Р С•РЎР‚Р С•РЎвЂЎР Вµ: {} < {expected}", rgba.len()),
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
    /// pixel of the 640Р“вЂ”480 frame remains a square `NР“вЂ”N` block on any
    /// monitor (item F), and the mouse is translated back through
    /// [`Self::window_to_frame`].
    ///
    /// The upload is skipped while the window is minimized/occluded: a hidden
    /// window is not drawn, but the caller's loop keeps running (no state reset).
    ///
    /// With [`RenderScale::Hd2`]/[`RenderScale::Hd4`] the buffer is the
    /// `640·S × 480·S` render frame, the content rectangle is `640·S × 480·S`
    /// (centred, letterboxed on the same rule) and the upload is linear; the
    /// mouse transform still returns logical 640×480 coordinates.
    pub fn present(&mut self, buffer: &RgbaBuffer) -> Result<(), WindowError> {
        if buffer.is_empty() {
            return Ok(());
        }

        if self.render_scale.is_hd() {
            // HD: the buffer is the render frame (640·S × 480·S) of the logical
            // 640×480 frame; its size does not change the geometry.
            self.refresh_layout();
            debug_assert_eq!(
                (buffer.width(), buffer.height()),
                self.render_frame_size(),
                "HD present expects the logical frame times S; another buffer size means the \
                 window was opened for a different scale (S applied twice), which crops the \
                 interface in the window and breaks mouse hit testing"
            );
        } else {
            // PixelPerfect keeps the historical behavior: the presented buffer
            // is the logical frame, so a different buffer size re-lays out it.
            let _ = self.update_frame_scale(buffer.width(), buffer.height());
        }

        if !self.should_present() {
            return Ok(());
        }

        let mut texture = self
            .creator
            .create_texture_streaming(PixelFormat::RGBA32, buffer.width(), buffer.height())
            .map_err(|error| WindowError::new("texture", error))?;

        // Guardrail 5: the filter is global per mode, never per asset.
        texture.set_scale_mode(match self.render_scale.filter() {
            FilterMode::Nearest => ScaleMode::Nearest,
            FilterMode::Linear => ScaleMode::Linear,
        });
        texture
            .update(None, buffer.pixels(), buffer.stride())
            .map_err(|error| WindowError::new("texture_update", error))?;

        self.canvas
            .set_draw_color(sdl3::pixels::Color::RGBA(0, 0, 0, 255));
        self.canvas.clear();

        let content = self.geometry.content_rect();
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
    /// Mouse coordinates are translated from window points to the logical frame
    /// through [`Self::window_to_frame`] (points РІвЂ вЂ™ pixels РІвЂ вЂ™ frame): at 150% DPI a
    /// click in a 640Р“вЂ”480-point window still arrives at the UI as a 640Р“вЂ”480-frame
    /// point. A click on a black bar outside the frame yields `(-1, -1)` РІР‚вЂќ the UI
    /// ignores it.
    ///
    /// `running == false` means the window was closed (`Quit`); Esc does **not** close
    /// the window at this level РІР‚вЂќ the UI decides itself (in the menu РІР‚вЂќ exit, in a subscreen РІР‚вЂќ
    /// back). For the old "Esc closes the window" behavior there is [`Self::poll`].
    ///
    /// `Alt+Enter` toggles fullscreen here (presentation only, the oracle's
    /// `Svga_ToggleFullscreen`) and does not reach the UI as a key. Focus,
    /// minimize, occlusion, resize, DPI and monitor changes update the
    /// presentation state/geometry only.
    pub fn poll_input(&mut self) -> (bool, Vec<WindowInput>) {
        let mut running = true;
        let mut inputs = Vec::new();
        // The window may have been resized since the previous frame РІР‚вЂќ update the layout before
        // recomputing the mouse coordinates.
        self.refresh_layout();
        let mut geometry = self.geometry;
        // Drain the queue first: the iterator borrows `self.events`, while the
        // handlers below need `&mut self` (fullscreen toggle, layout refresh).
        let events: Vec<sdl3::event::Event> = self.events.poll_iter().collect();

        for event in events {
            match event {
                sdl3::event::Event::Quit { .. } => running = false,
                sdl3::event::Event::KeyDown {
                    keycode: Some(code),
                    repeat: false,
                    keymod,
                    ..
                } if is_toggle_fullscreen_shortcut(code, keymod) => {
                    if let Err(error) = self.toggle_fullscreen() {
                        eprintln!("SDL: {error}");
                    }
                    geometry = self.geometry;
                }
                sdl3::event::Event::KeyDown {
                    keycode: Some(code),
                    repeat: false,
                    keymod,
                    ..
                } => inputs.push(WindowInput::Key(window_key_mod(code, keymod))),
                sdl3::event::Event::MouseMotion { x, y, .. } => {
                    let (x, y) = geometry
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
                    let (x, y) = geometry
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
                    let (x, y) = geometry
                        .window_to_frame(x.round() as i32, y.round() as i32)
                        .unwrap_or((-1, -1));
                    inputs.push(WindowInput::MouseRightClick { x, y });
                }
                sdl3::event::Event::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    let (x, y) = geometry
                        .window_to_frame(x.round() as i32, y.round() as i32)
                        .unwrap_or((-1, -1));
                    inputs.push(WindowInput::MouseRelease { x, y });
                }
                // P4 (TC-1107), 1.4.2: alt-tab/minimize/restore do not stop the loop.
                // Presentation state only; the geometry is refreshed when the
                // drawable or the display changes. An explicit branch so that the
                // behavior is visible and covered by the tests.
                sdl3::event::Event::Window { win_event, .. } => {
                    match win_event {
                        WindowEvent::FocusLost => {
                            self.presentation.focus_lost();
                            eprintln!(
                                "SDL: Р С•Р С”Р Р…Р С• Р С—Р С•РЎвЂљР ВµРЎР‚РЎРЏР В»Р С• РЎвЂћР С•Р С”РЎС“РЎРѓ РІР‚вЂќ РЎвЂ Р С‘Р С”Р В» Р С‘ РЎРѓР С‘Р СРЎС“Р В»РЎРЏРЎвЂ Р С‘РЎРЏ Р С—РЎР‚Р С•Р Т‘Р С•Р В»Р В¶Р В°РЎР‹РЎвЂљРЎРѓРЎРЏ, \
                                 Р С”Р В°Р Т‘РЎР‚ Р Р…Р Вµ РЎР‚Р С‘РЎРѓРЎС“Р ВµРЎвЂљРЎРѓРЎРЏ РЎвЂљР С•Р В»РЎРЉР С”Р С• Р С—РЎР‚Р С‘ РЎРѓР Р†Р С•РЎР‚Р В°РЎвЂЎР С‘Р Р†Р В°Р Р…Р С‘Р С‘"
                            );
                        }
                        WindowEvent::FocusGained => {
                            self.presentation.focus_gained();
                            // A focused window is visible: do not stay in the
                            // "occluded" state if the expose event was missed.
                            self.presentation.exposed();
                            eprintln!("SDL: РЎвЂћР С•Р С”РЎС“РЎРѓ Р Р†Р С•Р В·Р Р†РЎР‚Р В°РЎвЂ°РЎвЂР Р… РІР‚вЂќ Р С”Р В°Р Т‘РЎР‚ Р С—Р ВµРЎР‚Р ВµРЎР‚Р С‘РЎРѓР С•Р Р†РЎвЂ№Р Р†Р В°Р ВµРЎвЂљРЎРѓРЎРЏ");
                        }
                        WindowEvent::Minimized | WindowEvent::Hidden => {
                            self.presentation.minimized();
                            eprintln!("SDL: Р С•Р С”Р Р…Р С• РЎРѓР Р†РЎвЂРЎР‚Р Р…РЎС“РЎвЂљР С•/РЎРѓР С”РЎР‚РЎвЂ№РЎвЂљР С• РІР‚вЂќ Р С”Р В°Р Т‘РЎР‚РЎвЂ№ Р Р…Р Вµ Р Р†РЎвЂ№Р Р†Р С•Р Т‘РЎРЏРЎвЂљРЎРѓРЎРЏ");
                        }
                        WindowEvent::Restored
                        | WindowEvent::Exposed
                        | WindowEvent::Shown
                        | WindowEvent::Maximized => {
                            self.presentation.restored();
                            self.presentation.exposed();
                        }
                        WindowEvent::Occluded => self.presentation.occluded(),
                        WindowEvent::Resized(..)
                        | WindowEvent::PixelSizeChanged(..)
                        | WindowEvent::DisplayChanged(..) => {
                            self.refresh_layout();
                            geometry = self.geometry;
                        }
                        WindowEvent::EnterFullscreen => self.fullscreen = true,
                        WindowEvent::LeaveFullscreen => {
                            self.fullscreen = false;
                            self.refresh_layout();
                            geometry = self.geometry;
                        }
                        _ => {}
                    }
                }
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
/// stop the loop РІР‚вЂќ focus loss/gain (alt-tab), minimize,
/// restore, show/hide/redraw, cursor enter/leave.
///
/// With them the window stays alive: `poll_input` returns `running = true`, and
/// the next frame is drawn as usual. An explicit predicate РІР‚вЂќ so that this behavior
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
#[path = "sdl_window_tests.rs"]
mod tests;
