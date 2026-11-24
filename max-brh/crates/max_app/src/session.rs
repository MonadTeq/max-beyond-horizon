//! SDL/UI Stage C1 — the single session step: `poll input → order → tick → render`.
//!
//! Every session runs this one skeleton:
//!
//! * the local match — [`crate::App::local_update`] (the SDL screen polls the
//!   input before and draws after, so the step runs with the headless frame);
//! * the lockstep driver — `net::App::run_game_loop`, for the guest and for the
//!   host alike, with the frame supplied by the session ([`FrameSink`]);
//! * a headless host — the same driver with the headless frame.
//!
//! What the loops used to decide with an `online: bool` is a capability of the
//! dispatcher, read through [`SessionLocal`] (SDL/UI Stage C2):
//!
//! * order stage — [`SessionLocal::session_orders`]: a local match applies the
//!   order through `apply_now`; a lockstep match submits it to the facade
//!   (`net::flush_hud_orders` → `submit`) for the transport;
//! * clock stage — [`SessionLocal::session_tick`]: the local phase machine or one
//!   `App::tick` of the lockstep driver.
//!
//! The step never touches the simulation directly: it only orders the phases.
//!
//! [`FrameSink`]: crate::net::FrameSink

use max_core::universe::Dispatcher;

use crate::local::{LocalUpdate, SessionLocal};
use crate::App;

/// What one [`session_step`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// The frame ran to the end; the session continues. The payload is the
    /// local-match report (`LocalUpdate::default` for a lockstep session).
    Continue(LocalUpdate),
    /// The frame asked to leave the session (the window closed, Esc to the menu).
    Leave,
}

/// Input and drawing of a session frame — phases 1 and 4 of [`session_step`].
///
/// The step owns the order of the phases; a frame owns the event source (an SDL
/// window, a QA script, nothing for a headless host) and the drawing.
pub trait SessionFrame<D: Dispatcher> {
    /// Phase 1 — polls this frame's input and passes it to the session input
    /// ([`crate::ui::handle_session_input`]). `false` — the session leaves.
    fn poll_input(&mut self, app: &mut App<D>, dt: f32) -> bool;

    /// Phase 4 — draws the current state. `false` — the frame is gone (the window
    /// was closed), the session leaves.
    fn render(&mut self, app: &mut App<D>) -> bool;
}

/// The frame of a session without a window.
///
/// Used by the headless host/guest (`run_game_loop` without a sink) and by a
/// caller that polls the input and draws outside the step: a local match has one
/// event queue for the menu screens and the session, so `run_ui` polls before
/// [`crate::App::local_update`] and draws after it.
pub struct HeadlessFrame;

impl<D: Dispatcher> SessionFrame<D> for HeadlessFrame {
    fn poll_input(&mut self, _app: &mut App<D>, _dt: f32) -> bool {
        true
    }

    fn render(&mut self, _app: &mut App<D>) -> bool {
        true
    }
}

/// The one session step: `poll input → order → tick → render` (SDL/UI Stage C1).
///
/// Dispatcher-generic: a local match and a lockstep match (guest or host) execute
/// exactly this function. The immediate/queued difference is resolved by the
/// dispatcher capability inside [`SessionLocal::session_orders`] — never by a flag
/// passed to the step.
pub fn session_step<D, F>(app: &mut App<D>, dt: f32, frame: &mut F) -> StepOutcome
where
    D: Dispatcher,
    App<D>: SessionLocal,
    F: SessionFrame<D> + ?Sized,
{
    // 1. Poll input: the session handlers build orders through the command channel.
    if !frame.poll_input(app, dt) {
        return StepOutcome::Leave;
    }

    // 2. Order: the dispatcher capability decides between `apply_now` (local) and
    //    `hud.pending` → `flush_hud_orders` → `submit` (lockstep).
    app.session_orders();

    // 3. Tick: the clock of this session.
    let update = app.session_tick(dt);

    // 4. Render: the frame draws the state the tick just produced.
    if !frame.render(app) {
        return StepOutcome::Leave;
    }

    StepOutcome::Continue(update)
}
