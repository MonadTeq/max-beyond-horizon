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
