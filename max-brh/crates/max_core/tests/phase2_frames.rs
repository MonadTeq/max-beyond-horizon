//! Phase 2 integration: `frames.rs` — animation frame cycles and timings.
//!
//! Oracle: `max-cpp-port/src/frames` data (`D_*` in `PATCHES.RES`),
//! `units_manager.cpp` (`UnitsManager_ProgressExplosion`,
//! `UnitsManager_ProcessOrderSentry`), `unitinfo.cpp` (`UnitInfo::UnitInfo`,
//! `UnitsManager_DeployUnit` turret frames).

mod support;

use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SPINNING_TURRET, TURRET_SPRITE};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::frames;
use max_core::geo::Point;
use max_core::team::TeamId;

use support::{game, spawn, ticks};

/// `UnitsManager_ProgressExplosion`: the explosion advances one frame per
/// [`max_core::unit_order::EXPLOSION_FRAME_STEPS`] simulation steps and is removed at
/// `image_index_max`. The whole sequence is deterministic.
///
/// Oracle timing: `units_manager.cpp:4030-4036` advances the frame when `moved & 1`
/// (one frame per two order ticks), and an order tick is `TIMER_FPS_TO_MS(24)` ≈ 42 ms
/// (`game_manager.cpp:5103`, `timer.h:27`) → one frame per ~83 ms. The port's step is
/// 60 Hz, so 5 steps per frame: 14 frames of `LNDEXPLD` ≈ 1.1 s (the playtest defect was
/// that explosions ran 2.5× too fast).
#[test]
fn explosion_frames_advance_every_five_ticks_and_despawn() {
    fn run() -> (Vec<i16>, Vec<Vec<u8>>) {
        let mut state = game();
        let explosion = state
            .spawn_unit(
                UnitType::LandExplosion,
                TeamId::RED,
                Point::new(10, 10),
                MOBILE_LAND_UNIT,
            )
            .expect("explosion spawns");

        let max = state.snapshot().unit(explosion).unwrap().image_index_max;
        assert_eq!(max, frames::image_index_max(UnitType::LandExplosion, 0));
        assert_eq!(max, 13, "D_LNDEXPD: 14 frames");

        let mut observed = Vec::new();
        let mut snapshots = Vec::new();
        let mut removed_at = None;

        for tick in 0..160 {
            state.tick();
            match state.snapshot().unit(explosion) {
                Some(effect) => {
                    observed.push(effect.image_index);
                    assert!(effect.image_index <= max);
                    snapshots.push(state.snapshot().snapshot_bytes());
                }
                None => {
                    removed_at = Some(tick);
                    break;
                }
            }
        }

        // 13 advances × 5 steps + the removal check step (60 Hz: `66 × 16.7 ms ≈ 1.1 s`,
        // the oracle's `28 × 42 ms ≈ 1.17 s`).
        assert_eq!(
            removed_at,
            Some(65),
            "13 frame advances, one per five ticks"
        );
        assert_eq!(observed.first().copied(), Some(0), "frame 0 is shown");
        assert_eq!(observed.last().copied(), Some(max));
        assert_eq!(observed.len(), 65);
        // Frames 1..=12 are held for five ticks; the first frame is held for four
        // (the counter starts at zero) and the last appears once.
        for frame in 1..max {
            assert_eq!(
                observed.iter().filter(|value| **value == frame).count(),
                5,
                "frame {frame} timing: {observed:?}"
            );
        }
        assert_eq!(observed.iter().filter(|value| **value == 0).count(), 4);
        assert_eq!(observed.iter().filter(|value| **value == max).count(), 1);
        assert!(
            observed
                .windows(2)
                .all(|pair| pair[1] == pair[0] || pair[1] == pair[0] + 1),
            "frames never jump or rewind: {observed:?}"
        );
        (observed, snapshots)
    }

    let (first, first_snapshots) = run();
    let (second, second_snapshots) = run();
    assert_eq!(first, second);
    assert_eq!(first_snapshots, second_snapshots);
}

/// `UnitsManager_ProcessOrderSentry`: an animated unit in `SENTRY` cycles
/// `image_base ..= image_index_max` and wraps back to the base frame; the cycle
/// is paused when the `Effects` setting is off.
#[test]
fn sentry_animation_cycles_and_wraps() {
    let mut state = game();
    state.set_settings_effects(true);
    let radar = spawn(&mut state, UnitType::Radar, TeamId::RED, Point::new(10, 10));
    {
        let unit = state.snapshot().unit(radar).unwrap();
        assert_eq!(unit.orders, UnitOrder::Sentry);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert_eq!(unit.image_base, 0);
        assert_eq!(unit.image_index_max, 15, "D_RADAR: 16 frames");
        assert_ne!(
            unit.flags & max_core::enums::unit_flags::ANIMATED,
            0,
            "RADAR is animated in SC_UNITS"
        );
    }

    let mut frames_seen = Vec::new();
    for _ in 0..17 {
        state.tick();
        frames_seen.push(state.snapshot().unit(radar).unwrap().image_index);
    }

    assert_eq!(
        frames_seen,
        (1..=15).chain([0, 1]).collect::<Vec<_>>(),
        "one frame per tick, then wrap to image_base"
    );

    // Effects off — the frame stays at the base.
    let mut quiet = game();
    quiet.set_settings_effects(false);
    let radar = spawn(&mut quiet, UnitType::Radar, TeamId::RED, Point::new(10, 10));
    ticks(&mut quiet, 6);
    assert_eq!(quiet.snapshot().unit(radar).unwrap().image_index, 0);
}

/// Frame headers from `SC_UNITS` are applied when the unit is deployed:
/// turret bases/rotations, spinning turrets and the infantry `103` exception.
#[test]
fn deployed_frames_match_the_resource_table() {
    let mut state = game();

    let tank = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    let tank_unit = state.snapshot().unit(tank).unwrap();
    assert_ne!(tank_unit.flags & TURRET_SPRITE, 0);
    assert_eq!(tank_unit.image_base, 0);
    assert_eq!(tank_unit.image_index, 0);
    assert_eq!(tank_unit.image_index_max, 7, "8 hull frames");
    assert_eq!(tank_unit.turret_image_base, 8);
    assert_eq!(tank_unit.turret_image_index, 8, "base + angle 0");

    let scanner = spawn(
        &mut state,
        UnitType::Scanner,
        TeamId::RED,
        Point::new(12, 10),
    );
    let scanner_unit = state.snapshot().unit(scanner).unwrap();
    assert_ne!(scanner_unit.flags & SPINNING_TURRET, 0);
    assert_eq!(scanner_unit.turret_image_base, 8);
    assert_eq!(scanner_unit.turret_image_index, 8);
    assert_eq!(
        scanner_unit.image_index_max, 23,
        "spinning turret: turret base + 16 frames - 1"
    );

    for infantry in [UnitType::Infantry, UnitType::Infiltrator] {
        let id = spawn(&mut state, infantry, TeamId::RED, Point::new(14, 10));
        let unit = state.snapshot().unit(id).unwrap();
        assert_eq!(unit.image_index_max, 103, "{infantry:?}");
    }

    // A freshly deployed explosion also gets its frame range immediately.
    let explosion = state
        .spawn_unit(
            UnitType::BuildingExplosion,
            TeamId::RED,
            Point::new(16, 10),
            0,
        )
        .unwrap();
    assert_eq!(
        state.snapshot().unit(explosion).unwrap().image_index_max,
        27
    );

    // Frame counters are presentation fields: the canonical simulation digest
    // (`snapshot_bytes`) deliberately does not carry them; animation determinism
    // is checked by the frame sequences above instead.
    let before = state.snapshot().state_checksum();
    state.unit_mut(tank).unwrap().image_index = 3;
    assert_eq!(before, state.snapshot().state_checksum());
}
