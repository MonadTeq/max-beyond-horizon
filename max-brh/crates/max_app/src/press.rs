//! Visual press feedback of the mouse buttons — the GNW `*D` picture.
//!
//! Oracle: `button.c` (`button_draw(..., b->down, ...)` while the pointer is held
//! over the button, `:479-531`), and `Button::SetCaption` (`button.cpp:412-449`):
//! the released label is `Fonts_GoldColor (165,175,199)`, the pressed one
//! `Fonts_DarkOrageColor (5,56,199)`.
//!
//! The port had no pressed state at all: `draw_image_button` ignored its
//! `down_image`/`pressed` arguments, so a click never pressed the picture
//! (manual-run defect 4).
//!
//! # Model
//!
//! The current press is a *point* plus a pulse counter:
//!
//! * a left button press (`MouseClick`) arms the point for as long as the button is
//!   held — the oracle's own behavior (`MOUSE_PRESS`/`MOUSE_RELEASE`);
//! * a release (`MouseRelease`) leaves a short pulse of [`RELEASE_FRAMES`] drawn
//!   frames: the port acts on the press (C++ acts on the release), so without the
//!   pulse the picture of a button that changes the screen could never be seen;
//! * every drawn frame (`Screen::render`, `App::render_frame` callers) calls
//!   [`advance`], which decrements the pulse.
//!
//! Headless QA sends a click and a release together (the `.qa` `click` command is a
//! complete click), so `shot` right after it captures the pressed picture
//! deterministically; `dt = 0` does not matter — the pulse counts drawn frames.
//!
//! The state is presentation-only: it is not part of the snapshot/CRC. Rendering is
//! single-threaded, so a thread-local is enough and keeps the read side
//! (`draw_image_button` and friends, which have no `App` at hand) unchanged.

use std::cell::Cell;

use max_render::buffer::PixelRect;

/// Frames a released button stays pressed (4 frames ≈ 66 ms at 60 Hz).
pub const RELEASE_FRAMES: u8 = 4;

/// Held counter: the button is pressed until the release event.
const HELD: u8 = u8::MAX;

#[derive(Debug, Clone, Copy, Default)]
struct Press {
    x: i32,
    y: i32,
    frames: u8,
}

thread_local! {
    static PRESS: Cell<Press> = const { Cell::new(Press { x: 0, y: 0, frames: 0 }) };
}

/// Arms the feedback at `(x, y)` for as long as the button is held.
pub fn arm(x: i32, y: i32) {
    PRESS.with(|press| {
        press.set(Press {
            x,
            y,
            frames: HELD,
        });
    });
}

/// Ends the hold and keeps the picture visible for [`RELEASE_FRAMES`] drawn frames.
pub fn release() {
    PRESS.with(|press| {
        let mut state = press.get();

        if state.frames > RELEASE_FRAMES {
            state.frames = RELEASE_FRAMES;
        }

        press.set(state);
    });
}

/// Drops the feedback at once (e.g. the screen changed on this click: the pressed
/// picture of the old screen must not light up a same-position button of the new one).
pub fn clear() {
    PRESS.with(|press| press.set(Press::default()));
}

/// One drawn frame: the release pulse shrinks (a held button stays pressed).
pub fn advance() {
    PRESS.with(|press| {
        let mut state = press.get();

        if state.frames > 0 && state.frames != HELD {
            state.frames -= 1;
        }

        press.set(state);
    });
}

/// The pressed point, if the feedback is still visible.
pub fn point() -> Option<(i32, i32)> {
    PRESS.with(|press| {
        let state = press.get();

        (state.frames > 0).then_some((state.x, state.y))
    })
}

/// Whether `rect` is the pressed button.
pub fn pressed(rect: PixelRect) -> bool {
    point().is_some_and(|(x, y)| rect.contains(x, y))
}

#[cfg(test)]
#[path = "press_tests.rs"]
mod tests;
