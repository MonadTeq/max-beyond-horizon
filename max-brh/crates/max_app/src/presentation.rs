//! Presentation clock and focus policy of the SDL client (task 1.4.2).
//!
//! Rendering is presentation: it observes the simulation and never drives it
//! (guardrail A2, `max-brh-cicd/scripts/check_render_readonly.py`). This module
//! keeps that split true for **time** as well:
//!
//! * the simulation advances in fixed [`SIMULATION_DT`] steps at
//!   [`SIMULATION_HZ`], whatever the display refresh rate is — a 144 Hz frame
//!   plans zero or one 60 Hz step, and the same scenario produces byte-identical
//!   `snapshot_bytes`/`state_digest` at 60 Hz, 144 Hz or after a DPI change;
//! * [`FramePacer`] only decides when to draw the next frame; a late frame never
//!   injects extra simulation steps (the accumulated backlog is capped, and
//!   [`FixedStepClock::reset`] drops it after alt-tab);
//! * [`PresentationState`] records focus/minimize/occlusion. Alt-tab changes
//!   **drawing only**: it neither resets the state nor touches the explicit
//!   pause flag (`Alt+P`), and it is not visible to the network loop, so
//!   barrier timeouts keep their own clock.
//!
//! Everything here is pure except [`FramePacer::wait`], which sleeps; the
//! schedule itself is computed by [`FramePacer::plan_wait`] and is tested with
//! synthetic instants.

use std::time::{Duration, Instant};

/// Fixed simulation rate (the original game ticks at the 60 Hz frame rate).
pub const SIMULATION_HZ: u32 = 60;

/// One simulation step, seconds ([`SIMULATION_HZ`]).
pub const SIMULATION_DT: f32 = 1.0 / SIMULATION_HZ as f32;

/// The same step in `f64` — the clock accumulator uses it so that 6000 frames
/// of `1/60 s` average exactly 6000 steps (the `f32` constant is a rounded
/// value and would lose one step over a few minutes).
pub const SIMULATION_STEP_SECS: f64 = 1.0 / SIMULATION_HZ as f64;

/// Largest wall-clock delta a single frame may contribute, seconds.
///
/// Longer stalls (debugger, alt-tab, a slow first frame) are truncated: the
/// simulation does not try to replay an unbounded amount of time in one burst.
pub const MAX_FRAME_DT: f32 = 0.25;

/// Default cap of simulation steps executed for one rendered frame.
pub const MAX_STEPS_PER_FRAME: u32 = 4;

/// Converts wall-clock frame deltas into a whole number of fixed simulation steps.
#[derive(Debug, Clone)]
pub struct FixedStepClock {
    accumulator: f64,
    max_steps: u32,
    dropped: u64,
}

impl Default for FixedStepClock {
    fn default() -> Self {
        Self::new()
    }
}

impl FixedStepClock {
    /// Clock with the default step cap ([`MAX_STEPS_PER_FRAME`]).
    pub fn new() -> Self {
        Self::with_max_steps(MAX_STEPS_PER_FRAME)
    }

    /// Clock with an explicit cap of steps per frame (at least one).
    pub fn with_max_steps(max_steps: u32) -> Self {
        Self {
            accumulator: 0.0,
            max_steps: max_steps.max(1),
            dropped: 0,
        }
    }

    /// Registers `dt` seconds of wall-clock time and returns how many
    /// [`SIMULATION_DT`] steps a frame must run now (never more than the cap).
    ///
    /// The remainder stays in the accumulator, so 144 Hz frames alternate
    /// between zero and one step and average exactly 60 steps per second.
    pub fn plan(&mut self, dt: f64) -> u32 {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, f64::from(MAX_FRAME_DT))
        } else {
            0.0
        };

        self.accumulator += dt;

        let step = SIMULATION_STEP_SECS;
        // The epsilon keeps exact multiples (60 frames of 1/60 s) from losing a
        // step to floating-point rounding.
        let due = (self.accumulator / step + 1e-9).floor().max(0.0);
        let steps = (due as u64).min(u64::from(self.max_steps)) as u32;

        self.accumulator -= f64::from(steps) * step;

        if due > f64::from(steps) {
            self.dropped += due as u64 - u64::from(steps);
        }

        // Drain an alt-tab-sized backlog over the next few frames instead of one
        // spike: the accumulator never holds more than the cap.
        let ceiling = step * f64::from(self.max_steps);
        if self.accumulator > ceiling {
            self.accumulator = ceiling;
        }

        steps
    }

    /// Drops the accumulated time (focus regained, window restored): the next
    /// frame starts a new cadence instead of replaying the time spent away.
    pub fn reset(&mut self) {
        self.accumulator = 0.0;
    }

    /// Fraction of a step already accumulated, `0.0..1.0`.
    pub fn accumulated(&self) -> f32 {
        (self.accumulator / SIMULATION_STEP_SECS) as f32
    }

    /// Steps dropped because of the per-frame cap (diagnostics only).
    pub fn dropped_steps(&self) -> u64 {
        self.dropped
    }
}

/// Sleeps until the next frame deadline; never touches the simulation.
#[derive(Debug, Clone)]
pub struct FramePacer {
    period: Duration,
    deadline: Instant,
}

impl Default for FramePacer {
    fn default() -> Self {
        Self::at_hz(SIMULATION_HZ)
    }
}

impl FramePacer {
    /// Pacer at `hz` frames per second (clamped to `1..=1000`).
    pub fn at_hz(hz: u32) -> Self {
        let hz = hz.clamp(1, 1000);
        let period = Duration::from_secs_f64(1.0 / f64::from(hz));

        Self {
            period,
            deadline: Instant::now(),
        }
    }

    /// Pacer at [`SIMULATION_HZ`].
    pub fn at_60hz() -> Self {
        Self::at_hz(SIMULATION_HZ)
    }

    /// Frame period.
    pub fn period(&self) -> Duration {
        self.period
    }

    /// Time to sleep before the next frame, from `now` (`0` once the deadline
    /// has passed — a late frame is not compensated, there is no catch-up burst).
    pub fn plan_wait(&self, now: Instant) -> Duration {
        self.deadline.saturating_duration_since(now)
    }

    /// Restarts the cadence from `now` (the next frame is due immediately).
    ///
    /// Called after alt-tab / restore: the time spent away does not turn into a
    /// burst of frames or simulation steps.
    pub fn resync(&mut self, now: Instant) {
        self.deadline = now;
    }

    /// Advances the deadline after a frame that woke up at `now`.
    ///
    /// On time (`next > now`) the cadence is kept exactly; an overrun drops the
    /// debt and restarts from `now` (no catch-up burst).
    fn advance(&mut self, now: Instant) {
        let next = self.deadline + self.period;

        self.deadline = if next > now { next } else { now + self.period };
    }

    /// Sleeps until the next frame deadline and schedules the one after it.
    pub fn wait(&mut self) {
        let wait = self.plan_wait(Instant::now());

        if !wait.is_zero() {
            std::thread::sleep(wait);
        }

        self.advance(Instant::now());
    }
}

/// Wall-clock frame timings of the presentation loop (60 Hz budget evidence).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Rendered frames measured.
    pub frames: u64,
    /// Total frame time, nanoseconds.
    pub total_ns: u64,
    /// Longest frame, nanoseconds.
    pub max_ns: u64,
    /// Frames longer than the 60 Hz budget (16.67 ms).
    pub over_budget: u64,
}

impl FrameStats {
    /// Registers one frame.
    pub fn record(&mut self, duration: Duration) {
        let ns = duration.as_nanos() as u64;

        self.frames += 1;
        self.total_ns = self.total_ns.saturating_add(ns);
        self.max_ns = self.max_ns.max(ns);
        if ns > crate::profile::FRAME_BUDGET_NS {
            self.over_budget += 1;
        }
    }

    /// Average frame time in milliseconds (`0.0` when no frames were measured).
    pub fn avg_ms(&self) -> f64 {
        self.total_ns
            .checked_div(self.frames)
            .map(|ns| ns as f64 / 1_000_000.0)
            .unwrap_or(0.0)
    }

    /// Longest frame in milliseconds.
    pub fn max_ms(&self) -> f64 {
        self.max_ns as f64 / 1_000_000.0
    }

    /// One line for the log (`MAX_TICK_PROFILE=1`).
    pub fn summary(&self) -> String {
        format!(
            "PRESENT: frames={} avg={:.2}ms max={:.2}ms over_16.67ms={}",
            self.frames,
            self.avg_ms(),
            self.max_ms(),
            self.over_budget
        )
    }
}

/// Presentation-only state of the window: focus, minimize, occlusion.
///
/// Re-exported from `max_render::presentation`, where the SDL window lives:
/// the focus flags belong to presentation, next to the geometry.
pub use max_render::presentation::PresentationState;

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_seconds(clock: &mut FixedStepClock, hz: u32, seconds: u32) -> u32 {
        let frames = hz * seconds;
        let dt = 1.0 / f64::from(hz);
        let mut steps = 0;

        for _ in 0..frames {
            steps += clock.plan(dt);
        }

        steps
    }

    #[test]
    fn sixty_hertz_frames_give_one_step_per_frame() {
        let mut clock = FixedStepClock::new();

        for _ in 0..600 {
            assert_eq!(clock.plan(f64::from(SIMULATION_DT)), 1);
        }
        assert_eq!(clock.dropped_steps(), 0);
        // `SIMULATION_DT` is the rounded f32 step, so a small remainder stays.
        assert!(clock.accumulated() < 0.001);
    }

    #[test]
    fn one_second_of_any_refresh_rate_is_sixty_steps() {
        for hz in [30, 50, 60, 75, 120, 144, 165, 240] {
            let mut clock = FixedStepClock::new();
            let steps = plan_seconds(&mut clock, hz, 1);

            assert!(
                (59..=61).contains(&steps),
                "{hz} Hz: {steps} steps in one second"
            );
        }
    }

    #[test]
    fn refresh_rate_does_not_change_the_total_step_count_over_time() {
        // 10 seconds at 60 Hz and at 144 Hz: both cadences execute the same
        // number of simulation steps, so the snapshot cannot drift with fps.
        let mut sixty = FixedStepClock::new();
        let mut hundred_forty_four = FixedStepClock::new();

        let at_60 = plan_seconds(&mut sixty, 60, 10);
        let at_144 = plan_seconds(&mut hundred_forty_four, 144, 10);

        assert_eq!(at_60, 600);
        assert_eq!(at_144, 600);
    }

    #[test]
    fn alt_tab_gap_is_capped_and_reset_drops_the_backlog() {
        let mut clock = FixedStepClock::new();

        // A 30-second stall: at most MAX_STEPS_PER_FRAME now ...
        let steps = clock.plan(30.0);
        assert_eq!(steps, MAX_STEPS_PER_FRAME);
        // ... and the backlog drains over the next frames, not in one spike.
        assert!(clock.dropped_steps() > 0);

        clock.reset();
        assert_eq!(clock.plan(0.0), 0);
        assert_eq!(clock.accumulated(), 0.0);
    }

    #[test]
    fn negative_and_non_finite_deltas_are_ignored() {
        let mut clock = FixedStepClock::new();

        assert_eq!(clock.plan(-1.0), 0);
        assert_eq!(clock.plan(f64::NAN), 0);
        assert_eq!(clock.plan(f64::INFINITY), 0);
    }

    #[test]
    fn accumulated_fraction_tracks_the_remainder() {
        let mut clock = FixedStepClock::new();
        let half = f64::from(SIMULATION_DT) / 2.0;

        assert_eq!(clock.plan(half), 0);
        assert!((clock.accumulated() - 0.5).abs() < 1e-3);
        assert_eq!(clock.plan(half), 1);
        assert!(clock.accumulated() < 1e-3);
    }

    /// Durations from `f64` seconds are rounded to the nanosecond, so the tests
    /// compare periods with a tolerance instead of exact bits.
    fn assert_near(left: Duration, right: Duration) {
        let difference = left.as_secs_f64() - right.as_secs_f64();

        assert!(
            difference.abs() < 1e-6,
            "duration mismatch: {left:?} vs {right:?}"
        );
    }

    #[test]
    fn pacer_period_matches_the_requested_rate() {
        assert_near(
            FramePacer::at_60hz().period(),
            Duration::from_secs_f64(1.0 / 60.0),
        );
        assert_near(
            FramePacer::at_hz(144).period(),
            Duration::from_secs_f64(1.0 / 144.0),
        );
        // Out-of-range rates are clamped, never zero or infinite.
        assert!(FramePacer::at_hz(0).period() >= Duration::from_millis(1));
        assert!(FramePacer::at_hz(u32::MAX).period() >= Duration::from_millis(1));
    }

    #[test]
    fn pacer_deadline_is_relative_and_late_frames_do_not_rewind() {
        let mut pacer = FramePacer::at_60hz();
        let start = Instant::now();

        pacer.resync(start);
        // The next frame is due immediately after a resync (alt-tab return).
        assert_eq!(pacer.plan_wait(start), Duration::ZERO);

        // A frame that starts on time: the next deadline is one period later.
        pacer.advance(start);
        assert_near(pacer.plan_wait(start), pacer.period());

        // Half a period in: the rest of the period remains.
        assert_near(
            pacer.plan_wait(start + pacer.period() / 2),
            pacer.period() / 2,
        );

        // A frame that overran its slot (20 periods late) restarts from `now`
        // instead of producing a catch-up burst.
        pacer.advance(start + pacer.period() * 20);
        assert_near(pacer.plan_wait(start + pacer.period() * 20), pacer.period());

        // Resync after a 30-second alt-tab: a fresh cadence, no replay.
        pacer.resync(start + Duration::from_secs(30));
        assert_eq!(
            pacer.plan_wait(start + Duration::from_secs(30)),
            Duration::ZERO
        );
        pacer.advance(start + Duration::from_secs(30));
        assert_near(
            pacer.plan_wait(start + Duration::from_secs(30)),
            pacer.period(),
        );
    }

    #[test]
    fn frame_stats_report_the_budget_overruns() {
        let mut stats = FrameStats::default();

        stats.record(Duration::from_millis(5));
        stats.record(Duration::from_millis(25));
        stats.record(Duration::from_millis(10));

        assert_eq!(stats.frames, 3);
        assert_eq!(stats.over_budget, 1);
        assert_eq!(stats.max_ms(), 25.0);
        assert!((stats.avg_ms() - 13.333).abs() < 0.01);
        assert!(stats.summary().contains("over_16.67ms=1"));
    }

    #[test]
    fn focus_changes_only_drawing_decisions() {
        let mut state = PresentationState::new();

        assert!(state.should_draw());
        assert!(state.is_focused());

        state.focus_lost();
        assert!(!state.is_focused());
        // A focused-out window is still drawn (alt-tab, not minimize).
        assert!(state.should_draw());

        state.minimized();
        assert!(!state.should_draw());

        state.focus_gained();
        assert!(state.is_focused());
        assert!(!state.is_minimized());
        assert!(state.should_draw());

        state.occluded();
        assert!(!state.should_draw());
        state.exposed();
        assert!(state.should_draw());
    }

    #[test]
    fn repeated_focus_events_are_idempotent() {
        let mut state = PresentationState::new();

        state.focus_lost();
        state.focus_lost();
        state.focus_gained();
        state.focus_gained();

        assert_eq!(state.transitions(), 2);
        assert!(state.is_focused());
    }
}
