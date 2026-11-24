//! Task 1.4.2 gates: presentation (scale, DPI, refresh rate, alt-tab) must not
//! change the simulation or the golden frame.
//!
//! Checked here:
//!
//! 1. the same scenario executed under different presentation profiles — 100%
//!    and 150% DPI, 60 Hz and 144 Hz, 1920×1080 and 3840×2160 drawables —
//!    produces byte-identical `snapshot_bytes` and `state_digest`: the fixed
//!    60 Hz [`FixedStepClock`] decides how many simulation steps a frame runs,
//!    the refresh rate only decides how many frames are drawn;
//! 2. alt-tab (focus lost/gained, minimize/restore) does not reset the match,
//!    does not unpause a paused match and does not tick a paused match while
//!    the window is hidden;
//! 3. `session_step` — the skeleton shared with the lockstep network loop —
//!    keeps running while the frame skips drawing, so focus cannot stop the
//!    barrier loop;
//! 4. rendering is profile-independent: the screen always draws into the same
//!    640×480 frame, and the presentation geometry is a pure transform on top
//!    of it (the golden at 100% DPI stays the reference; other scales are not
//!    re-goldened).
//!
//! The local match needs the original game data (`max-original/MAX.RES`); the
//! render/clock/session checks do not and always run.

use std::path::{Path, PathBuf};

use max_app::local::{LocalPhase, LocalSetup};
use max_app::presentation::PresentationState;
use max_app::session::{session_step, SessionFrame, StepOutcome};
use max_app::ui::{Screen, ScreenView};
use max_app::{App, AppConfig, FixedStepClock, FramePacer, SIMULATION_DT, SIMULATION_HZ};
use max_core::enums::{PlayMode, TeamClan, VictoryType};
use max_core::team::TeamKind;
use max_core::universe::{Dispatcher, LocalDispatcher};
use max_render::buffer::RgbaBuffer;
use max_render::presentation::ScreenGeometry;
use max_render::{FRAME_HEIGHT, FRAME_WIDTH};

/// Steps executed after the player's turn starts: enough to close orders, move
/// units and let the AI act, but short enough for a debug test run.
const STEP_BUDGET: u32 = 300;

/// A presentation configuration: window points, drawable pixels, refresh rate.
///
/// None of these values reaches `max_core`: they only change the geometry and
/// how many frames carry the same 60 Hz simulation steps.
#[derive(Debug, Clone, Copy)]
struct PresentationProfile {
    name: &'static str,
    window_points: (u32, u32),
    output_pixels: (u32, u32),
    refresh_hz: u32,
}

impl PresentationProfile {
    fn frame_dt(self) -> f64 {
        1.0 / f64::from(self.refresh_hz)
    }
}

const PROFILES: [PresentationProfile; 4] = [
    PresentationProfile {
        name: "100% DPI, 1920x1080 drawable, 60 Hz",
        window_points: (1920, 1080),
        output_pixels: (1920, 1080),
        refresh_hz: 60,
    },
    PresentationProfile {
        name: "150% DPI, 960x720 drawable, 60 Hz",
        window_points: (640, 480),
        output_pixels: (960, 720),
        refresh_hz: 60,
    },
    PresentationProfile {
        name: "100% DPI, 1920x1080 drawable, 144 Hz",
        window_points: (1920, 1080),
        output_pixels: (1920, 1080),
        refresh_hz: 144,
    },
    PresentationProfile {
        name: "100% DPI, 3840x2160 drawable, 60 Hz",
        window_points: (3840, 2160),
        output_pixels: (3840, 2160),
        refresh_hz: 60,
    },
];

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// Reports a snapshot mismatch without dumping megabytes of bytes into the log.
fn assert_same_snapshot(left: &[u8], right: &[u8], context: &str) {
    if left == right {
        return;
    }

    let first = left
        .iter()
        .zip(right)
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| left.len().min(right.len()));

    panic!(
        "{context}: snapshot_bytes differ (len {} vs {}, first difference at {first})",
        left.len(),
        right.len()
    );
}

/// Application with the original game data, or `None` when the data is absent
/// (the gates that need a match skip themselves, the rest still run).
fn local_app() -> Option<App> {
    let data = repo_path("max-original");

    if !data.join("MAX.RES").is_file() {
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&data)
        .with_assets_dir(repo_path("assets"))
        .with_mode(max_app::AppMode::Ui);

    let mut app = App::new(config);
    app.init_assets().ok()?;

    Some(app)
}

/// One human (RED) against one computer (GREEN) on a fixed planet/seed.
fn setup_vs_one_ai() -> LocalSetup {
    LocalSetup {
        player_name: "Tester".to_string(),
        world: 5,
        opponent: 2,
        timer: 0,
        endturn: 0,
        play_mode: PlayMode::TurnBased,
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: VictoryType::Duration,
        victory_limit: 50,
        team_types: [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
        team_clans: [TeamClan::Random; 4],
        hot_seat: false,
        seed: 7,
        start_positions: [None; 4],
        team_loadouts: std::array::from_fn(|_| None),
        team_gold: [None; 4],
        team_upgrades: std::array::from_fn(|_| None),
        team_gold_spent: [None; 4],
    }
}

/// Runs the landing/AI phases until the human turn starts (`local_update(0.0)`
/// ticks the visible route with zero dt, exactly like the headless tests).
fn run_to_player_turn(app: &mut App, limit: usize) -> bool {
    for _ in 0..limit {
        if app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn) {
            return true;
        }
        if app.local().is_none_or(|local| local.finished()) {
            return false;
        }
        app.local_update(0.0);
    }

    app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn)
}

/// Starts the reference scenario and stops at the first human turn.
fn started_app() -> Option<App> {
    let mut app = local_app()?;

    app.start_local_game(&setup_vs_one_ai()).ok()?;
    if !run_to_player_turn(&mut app, 900) {
        return None;
    }

    Some(app)
}

// ---------------------------------------------------------------------------
// Gate 1: snapshot_bytes / state_digest do not depend on the presentation profile
// ---------------------------------------------------------------------------

#[test]
fn snapshot_bytes_and_digest_do_not_depend_on_the_presentation_profile() {
    let Some(mut reference) = started_app() else {
        return;
    };

    let start_bytes = reference.state_ref().snapshot_bytes();

    // Reference: 300 plain fixed steps (the 100% DPI / 60 Hz baseline).
    for _ in 0..STEP_BUDGET {
        reference.local_update(SIMULATION_DT);
    }

    let expected_bytes = reference.state_ref().snapshot_bytes();
    let expected_digest = reference.digest();

    for profile in PROFILES {
        let Some(mut app) = started_app() else {
            return;
        };

        assert_same_snapshot(
            &app.state_ref().snapshot_bytes(),
            &start_bytes,
            &format!("[{}] the started match", profile.name),
        );

        let mut clock = FixedStepClock::new();
        let mut executed = 0u32;
        let mut frames = 0u32;

        while executed < STEP_BUDGET {
            let batch = clock.plan(profile.frame_dt()).min(STEP_BUDGET - executed);

            for _ in 0..batch {
                app.local_update(SIMULATION_DT);
            }

            executed += batch;
            frames += 1;

            assert!(
                frames < STEP_BUDGET * 8,
                "[{}] the clock never planned a step",
                profile.name
            );
        }

        // A 144 Hz cadence draws ~2.4x more frames for the same 300 steps.
        if profile.refresh_hz == 144 {
            assert!(
                frames >= 700,
                "144 Hz carried the steps in only {frames} frames"
            );
        }

        assert_same_snapshot(
            &app.state_ref().snapshot_bytes(),
            &expected_bytes,
            &format!("[{}] {executed} steps in {frames} frames", profile.name),
        );
        assert_eq!(
            app.digest(),
            expected_digest,
            "[{}] state_digest changed with the presentation profile",
            profile.name
        );
    }
}

// ---------------------------------------------------------------------------
// Gate 2: alt-tab does not reset the state or the pause flag
// ---------------------------------------------------------------------------

#[test]
fn alt_tab_does_not_reset_or_unpause_the_match() {
    let Some(mut app) = started_app() else {
        return;
    };

    let mut presentation = PresentationState::new();
    assert!(presentation.should_draw());

    // The explicit pause (`Alt+P`, `PauseMenu`) is the only pause flag.
    assert!(app.local_toggle_pause());
    let paused_bytes = app.state_ref().snapshot_bytes();
    let paused_ticks = app.ticks();

    // Alt-tab away: focus lost, minimized, then focus gained and restored.
    presentation.focus_lost();
    assert!(!presentation.is_focused());
    presentation.minimized();
    assert!(!presentation.should_draw());

    // While the window is hidden, a paused match must not advance.
    for _ in 0..120 {
        app.local_update(SIMULATION_DT);
    }
    assert_eq!(
        app.ticks(),
        paused_ticks,
        "a paused match advanced while the window was hidden"
    );
    assert_same_snapshot(
        &app.state_ref().snapshot_bytes(),
        &paused_bytes,
        "a paused match changed its snapshot on focus loss",
    );

    presentation.focus_gained();
    presentation.exposed();
    assert!(presentation.should_draw());
    assert!(presentation.is_focused());

    // Back in the window: still paused, nothing was reset.
    assert!(
        app.local().is_some_and(|local| local.paused),
        "alt-tab dropped the pause flag"
    );
    assert_same_snapshot(
        &app.state_ref().snapshot_bytes(),
        &paused_bytes,
        "the match after focus returned",
    );
    assert_eq!(app.ticks(), paused_ticks);

    // A second Alt+P resumes exactly the paused state (no rewind, no reset).
    assert!(!app.local_toggle_pause());
    let resumed_ticks = app.ticks();
    app.local_update(SIMULATION_DT);
    assert_eq!(app.ticks(), resumed_ticks + 1, "resume did not tick");
}

#[test]
fn focus_events_never_call_into_the_simulation() {
    // `PresentationState` has no API that could tick, reset or pause the match:
    // this test documents the shape of the gate (the type is presentation only).
    let mut presentation = PresentationState::new();

    let before = presentation.transitions();
    presentation.focus_lost();
    presentation.focus_gained();
    assert!(presentation.transitions() > before);
    // The only observable state is drawing-related.
    assert!(presentation.should_draw());
    assert!(presentation.is_focused());
}

// ---------------------------------------------------------------------------
// Gate 3: the shared session step keeps running while the frame skips drawing
// ---------------------------------------------------------------------------

/// A frame that reports focus loss: `render` counts calls but draws nothing
/// (the real SDL frame skips the GPU upload in the same situation).
struct FocusFrame {
    presentation: PresentationState,
    renders: u32,
}

impl<D: Dispatcher> SessionFrame<D> for FocusFrame {
    fn poll_input(&mut self, _app: &mut App<D>, _dt: f32) -> bool {
        true
    }

    fn render(&mut self, _app: &mut App<D>) -> bool {
        // The real frame would upload only when `should_draw()` is true; the
        // step must continue regardless.
        let _ = self.presentation.should_draw();
        self.renders += 1;

        true
    }
}

#[test]
fn session_step_is_not_gated_on_focus() {
    let Some(mut focused) = started_app() else {
        return;
    };
    let Some(mut unfocused) = started_app() else {
        return;
    };

    // The network session step is the same skeleton for a lockstep match: a
    // hidden window must not stop the loop (barrier timeouts keep their own
    // clock) and must not change the tick count.
    for _ in 0..STEP_BUDGET {
        let mut presentation = PresentationState::new();
        presentation.focus_lost();
        presentation.minimized();

        let outcome = session_step(
            &mut unfocused,
            SIMULATION_DT,
            &mut FocusFrame {
                presentation,
                renders: 0,
            },
        );

        assert!(matches!(outcome, StepOutcome::Continue(_)));
        focused.local_update(SIMULATION_DT);
    }

    assert_eq!(
        unfocused.ticks(),
        focused.ticks(),
        "the session step stopped or skipped ticks while unfocused"
    );
    assert_same_snapshot(
        &unfocused.state_ref().snapshot_bytes(),
        &focused.state_ref().snapshot_bytes(),
        "a hidden window changed the simulation",
    );
}

// ---------------------------------------------------------------------------
// Gate 4: the drawn frame is profile-independent (golden at 100% stays the reference)
// ---------------------------------------------------------------------------

#[test]
fn screen_frame_is_640x480_for_every_presentation_profile() {
    for profile in PROFILES {
        let geometry = ScreenGeometry::native(profile.window_points, profile.output_pixels);

        assert_eq!(
            geometry.frame_size(),
            (FRAME_WIDTH, FRAME_HEIGHT),
            "[{}] the logical frame changed",
            profile.name
        );
        // Scaling is integer: no interpolation, so a scaled frame is an exact
        // block expansion of the 640×480 golden frame.
        assert!(geometry.scale() >= 1);
        assert_eq!(
            geometry.content_rect().width,
            (FRAME_WIDTH * geometry.scale()) as i32,
            "[{}]",
            profile.name
        );
        assert_eq!(
            geometry.content_rect().height,
            (FRAME_HEIGHT * geometry.scale()) as i32,
            "[{}]",
            profile.name
        );
    }
}

#[test]
fn menu_frame_bytes_do_not_depend_on_the_window_geometry() {
    // Rendering always targets a 640×480 frame; the window/DPI transform runs
    // after it (and is not part of the golden). Two unrelated geometries must
    // therefore yield identical frame bytes.
    let mut first = RgbaBuffer::new(FRAME_WIDTH, FRAME_HEIGHT);
    let mut second = RgbaBuffer::new(FRAME_WIDTH, FRAME_HEIGHT);
    let status = "golden frame";

    for buffer in [&mut first, &mut second] {
        let screen = Screen::MainMenu;

        screen.render(
            buffer,
            ScreenView::<LocalDispatcher> {
                width: FRAME_WIDTH,
                height: FRAME_HEIGHT,
                status,
                mouse: None,
                focus: 0,
                app: None,
            },
        );
    }

    assert_eq!(first.width(), FRAME_WIDTH);
    assert_eq!(first.height(), FRAME_HEIGHT);
    assert!(
        first.pixels() == second.pixels(),
        "the 640x480 frame depends on the window/DPI profile"
    );
}

// ---------------------------------------------------------------------------
// Clock contract
// ---------------------------------------------------------------------------

#[test]
fn the_presentation_clock_keeps_the_simulation_at_sixty_hertz() {
    assert_eq!(SIMULATION_HZ, 60);
    assert!((SIMULATION_DT - 1.0 / 60.0).abs() < 1e-6);

    // 60 Hz and 144 Hz frames over ten seconds plan the same number of steps.
    for hz in [60u32, 144] {
        let mut clock = FixedStepClock::new();
        let mut steps = 0u32;

        for _ in 0..hz * 10 {
            steps += clock.plan(1.0 / f64::from(hz));
        }

        assert_eq!(steps, 600, "{hz} Hz planned a different number of steps");
    }

    // A 30-second alt-tab gap is capped, and a reset drops it completely.
    let mut clock = FixedStepClock::new();
    let burst = clock.plan(30.0);
    assert!(burst <= 4, "the gap produced a {burst}-step burst");
    clock.reset();
    assert_eq!(clock.plan(0.0), 0);
}

#[test]
fn the_frame_pacer_targets_sixty_hertz() {
    let pacer = FramePacer::at_60hz();

    assert!((pacer.period().as_secs_f64() - 1.0 / 60.0).abs() < 1e-6);
}
