//! SDL3 host of the editor: window, input, frame presentation.
//!
//! This is a thin layer (a counterpart of `max_render::sdl_window`), but with the events the
//! editor needs: mouse wheel, dragging, F-keys and the text filter.
//! All drawing happens in [`crate::ui::EditorState::compose`], so the same frame
//! is saved to PNG on F12/`--shot` without SDL involved.

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use max_render::buffer::RgbaBuffer;
use max_render::frame_scale::FrameScale;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::rect::Rect as SdlRect;
use sdl3::render::{Canvas, ScaleMode, TextureCreator};
use sdl3::video::{Window, WindowContext};

use crate::ui::{EditorState, FRAME_HEIGHT, FRAME_WIDTH, WINDOW_TITLE};

/// Opens the window and runs the loop until Esc/closing.
pub fn run(state: EditorState, shot: Option<&Path>) -> Result<()> {
    let sdl = sdl3::init().map_err(|error| anyhow!("SDL_Init: {error}"))?;

    run_with_sdl(sdl, state, shot)
}

/// The same as [`run`], but with SDL already initialized.
///
/// Needed when starting without arguments: before the window a native dialog
/// picks `MAX.RES` (see `dialog.rs` in the binary), and the same `Sdl` is reused
/// for the window — no re-initialization/shutdown of SDL.
pub fn run_with_sdl(sdl: sdl3::Sdl, mut state: EditorState, shot: Option<&Path>) -> Result<()> {
    let video = sdl.video().map_err(|error| anyhow!("SDL video: {error}"))?;
    let window = video
        .window(WINDOW_TITLE, FRAME_WIDTH, FRAME_HEIGHT)
        .resizable()
        .position_centered()
        .build()
        .map_err(|error| anyhow!("SDL window: {error}"))?;
    let mut canvas = window.into_canvas();
    let creator = canvas.texture_creator();
    let mut events = sdl
        .event_pump()
        .map_err(|error| anyhow!("SDL event pump: {error}"))?;

    let (window_width, window_height) = canvas.window().size();
    let mut scale = FrameScale::fit(window_width, window_height, FRAME_WIDTH, FRAME_HEIGHT);
    let mut frame = state.compose();
    present(&mut canvas, &creator, &frame, &mut scale)?;

    if let Some(path) = shot {
        state.save_screenshot(path)?;
        return Ok(());
    }

    let mut running = true;
    let mut dirty = false;

    while running {
        for event in events.poll_iter() {
            handle_event(event, &mut state, &scale, &mut running, &mut dirty);
        }

        if dirty {
            frame = state.compose();
            present(&mut canvas, &creator, &frame, &mut scale)?;
            dirty = false;
        }

        std::thread::sleep(Duration::from_millis(8));
    }

    Ok(())
}

fn handle_event(
    event: Event,
    state: &mut EditorState,
    scale: &FrameScale,
    running: &mut bool,
    dirty: &mut bool,
) {
    match event {
        Event::Quit { .. } => *running = false,
        Event::KeyDown {
            keycode: Some(code),
            repeat: false,
            ..
        } => {
            *dirty = true;
            match code {
                Keycode::Escape => *running = false,
                Keycode::Up => state.select_delta(-1),
                Keycode::Down => state.select_delta(1),
                Keycode::PageUp => state.select_page(-1),
                Keycode::PageDown => state.select_page(1),
                Keycode::Home => state.select_edge(false),
                Keycode::End => state.select_edge(true),
                Keycode::Left => state.pan_by(32, 0),
                Keycode::Right => state.pan_by(-32, 0),
                Keycode::F3 => state.toggle_search(),
                Keycode::F5 => state.fit_zoom(),
                Keycode::F12 => {
                    let path = std::path::PathBuf::from("max-resources-viewer-screenshot.png");
                    if let Err(error) = state.save_screenshot(&path) {
                        state.status = format!("{error:#}");
                    }
                }
                Keycode::Backspace => state.filter_backspace(),
                Keycode::Return | Keycode::KpEnter => state.apply_filter(),
                Keycode::Delete => state.clear_filter(),
                other => {
                    let raw = other as u32;
                    match raw {
                        0x2B | 0x3D => state.zoom_by(1),  // + / =
                        0x2D | 0x5F => state.zoom_by(-1), // - / _
                        0x30 => state.reset_view(),       // 0
                        0x20..=0x7E => {
                            if let Some(ch) = char::from_u32(raw) {
                                if !ch.is_ascii_control() {
                                    state.push_filter_char(ch);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Event::MouseMotion { x, y, .. } => {
            if let Some((x, y)) = scale.window_to_frame(x.round() as i32, y.round() as i32) {
                state.mouse_move(x, y);
                *dirty = true;
            }
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } => {
            if let Some((x, y)) = scale.window_to_frame(x.round() as i32, y.round() as i32) {
                state.click(x, y);
                *dirty = true;
            }
        }
        Event::MouseButtonUp {
            mouse_btn: MouseButton::Left,
            ..
        } => {
            state.release();
            *dirty = true;
        }
        Event::MouseWheel {
            x,
            y,
            mouse_x,
            mouse_y,
            ..
        } => {
            if let Some((mouse_x, mouse_y)) =
                scale.window_to_frame(mouse_x.round() as i32, mouse_y.round() as i32)
            {
                state.mouse_move(mouse_x, mouse_y);
                state.wheel(x, y, false);
                *dirty = true;
            }
        }
        Event::Window {
            win_event: WindowEvent::Resized(_, _),
            ..
        } => *dirty = true,
        _ => {}
    }
}

fn present(
    canvas: &mut Canvas<Window>,
    creator: &TextureCreator<WindowContext>,
    frame: &RgbaBuffer,
    scale: &mut FrameScale,
) -> Result<()> {
    let (window_width, window_height) = canvas.window().size();
    *scale = FrameScale::fit(window_width, window_height, frame.width(), frame.height());

    let mut texture = creator
        .create_texture_streaming(PixelFormat::RGBA32, frame.width(), frame.height())
        .map_err(|error| anyhow!("SDL texture: {error}"))?;

    texture.set_scale_mode(ScaleMode::Nearest);
    texture
        .update(None, frame.pixels(), frame.stride())
        .map_err(|error| anyhow!("SDL texture update: {error}"))?;

    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();

    let content = scale.content_rect();
    let destination = SdlRect::new(
        content.ulx,
        content.uly,
        content.width.max(1) as u32,
        content.height.max(1) as u32,
    );

    canvas
        .copy(&texture, None, destination)
        .map_err(|error| anyhow!("SDL copy: {error}"))?;
    let _ = canvas.present();

    Ok(())
}
