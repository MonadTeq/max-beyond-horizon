//! Turn Based: combat effects must not hold the AI turn.
//!
//! Regression: a projectile (`ROCKET`/`TORPEDO`/…) is created with `speed = 0` from
//! `SC_ATTRI`, because it is a unit effect. `AirPath::Execute` in C++ checks
//! speed only for `MOBILE_AIR_UNIT` (`airpath.cpp:186-199`); the port applied
//! the check to all, and the projectile stayed in `ORDER_MOVE`/
//! `ORDER_STATE_EXECUTING_ORDER` forever. `UnitsManager_ProcessOrder` set
//! `OrdersPending` every tick for `MISSILE_UNIT`, so
//! `Ai_CheckEndTurn` was not called and the AI turn did not close until the emergency
//! `LOCAL_AI_TICK_LIMIT` (12,000 frames) — a "hang on the AI turn".
//!
//! Second regression: `UnitsManager_StartExplosion` creates a unit effect with
//! class flags (`MOBILE_LAND`/…), while `ANIMATED` comes from `SC_UNITS`;
//! `spawn_unit` checked only explicit flags and did not move the effect to
//! `ORDER_EXPLODE`/`ORDER_STATE_DESTROY`, so the explosion stayed on the map
//! forever.

use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::universe::GameUniverse;

fn combat_state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.set_play_mode(PlayMode::TurnBased);
    state.set_game_state(max_core::enums::GameStateType::InGame);
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.teams_mut()[1].team_type = TeamKind::Computer;
    state.team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units_table_mut()[1] = TeamUnits::with_clan(TeamClan::Random);
    // The target is visible to the shooter: in game this comes from reconnaissance, here — determinism.
    state.ai_mut().all_visible = true;
    state
}

fn spawn(
    state: &mut GameUniverse,
    unit_type: UnitType,
    team: TeamId,
    grid: Point,
) -> max_core::UnitId {
    state
        .spawn_unit(unit_type, team, grid, MOBILE_LAND_UNIT | SELECTABLE)
        .expect("spawn")
}

fn is_combat_effect(unit_type: UnitType) -> bool {
    matches!(
        unit_type,
        UnitType::Rocket
            | UnitType::Torpedo
            | UnitType::AlienMissile
            | UnitType::AlienTankBall
            | UnitType::AlienArtilleryBall
            | UnitType::RocketSmoke
            | UnitType::TorpedoBubble
            | UnitType::LandExplosion
            | UnitType::AirExplosion
            | UnitType::SeaExplosion
            | UnitType::BuildingExplosion
            | UnitType::HitExplosion
    )
}

/// The AI turn closes when its missile hit and the explosion finished.
#[test]
fn turn_based_ai_turn_ends_after_its_missile_impact() {
    let mut state = combat_state();
    state.set_active_turn_team(TeamId::GREEN);
    let launcher = spawn(
        &mut state,
        UnitType::MissileCrawler,
        TeamId::GREEN,
        Point::new(5, 5),
    );
    let target = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(13, 5));
    state.update_visibility_status(false);

    state.ai_begin_turn(TeamId::GREEN);
    let mut data = state.snapshot().order_data(launcher);
    data.fire_on = Point::new(13, 5);
    state.set_order_data(launcher, data);
    state.set_order(launcher, UnitOrder::Fire, UnitOrderState::Init);

    let mut saw_rocket = false;
    let mut saw_explosion = false;

    for tick in 0..400 {
        state.tick();

        for id in state.snapshot().unit_ids_in_order() {
            if let Some(unit) = state.snapshot().unit(id) {
                saw_rocket |= unit.unit_type == UnitType::Rocket;
                saw_explosion |=
                    is_combat_effect(unit.unit_type) && unit.unit_type != UnitType::Rocket;
            }
        }

        if state.snapshot().ai().players[TeamId::GREEN.0 as usize].end_turn_retry_counter > 120 {
            assert!(tick < 300, "ход ИИ закрылся только на тике {tick}");

            let leftovers: Vec<_> = state
                .snapshot()
                .unit_ids_in_order()
                .into_iter()
                .filter_map(|id| state.snapshot().unit(id))
                .filter(|unit| is_combat_effect(unit.unit_type))
                .map(|unit| unit.unit_type)
                .collect();
            assert!(
                leftovers.is_empty(),
                "юниты-эффекты остались на карте: {leftovers:?}"
            );
            assert!(
                state
                    .snapshot()
                    .unit(target)
                    .is_none_or(|unit| unit.hits < 24),
                "ракета должна была попасть в цель"
            );
            assert!(saw_rocket, "снаряд ROCKET должен был появиться");
            assert!(saw_explosion, "взрыв должен был появиться");
            return;
        }
    }

    panic!(
        "ход ИИ не закрылся за 400 тиков: counter={}, units={:?}",
        state.snapshot().ai().players[TeamId::GREEN.0 as usize].end_turn_retry_counter,
        state
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.snapshot().unit(id))
            .map(|unit| (unit.unit_type, unit.orders, unit.state, unit.grid))
            .collect::<Vec<_>>()
    );
}

/// A miss (no target on the cell) gives `HITEXPLD` (`unitinfo.cpp:2713-2723`) —
/// also as a unit effect with animation and removal.
#[test]
fn a_miss_spawns_a_hit_explosion_that_animates_and_despawns() {
    let mut state = combat_state();
    let shooter = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5));
    state.update_visibility_status(false);

    // Shoot at an empty cell within range: `Fire` without a target.
    state.attack(shooter, Point::new(6, 5));

    let mut explosion = None;
    for _ in 0..30 {
        state.tick();

        for id in state.snapshot().unit_ids_in_order() {
            if let Some(unit) = state.snapshot().unit(id) {
                if unit.unit_type == UnitType::HitExplosion {
                    explosion = Some(unit.image_index_max);
                }
            }
        }

        if explosion.is_some()
            && !state
                .snapshot()
                .unit_ids_in_order()
                .into_iter()
                .filter_map(|id| state.snapshot().unit(id))
                .any(|unit| unit.unit_type == UnitType::HitExplosion)
        {
            break;
        }
    }

    assert_eq!(explosion, Some(4), "HITEXPLD: image_index_max = 4");
    assert!(
        !state
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.snapshot().unit(id))
            .any(|unit| unit.unit_type == UnitType::HitExplosion),
        "HITEXPLD удаляется после анимации"
    );
}

/// `Paths_DrawMissile` (`airpath.cpp:362-406`): a trail of
/// three effect units is left behind the projectile for every cell it leaves — `RKTSMOKE` for a rocket
/// (`D_RKTSMOKE`: 5 frames) and `TRPBUBLE` for a torpedo (`D_TRPBUBLE`: 3 frames).
///
/// The exact port is checked: particle type, position (the left cell, but
/// pixel offset 0/⅓/⅔ toward the next one — `OffsetDrawZones`), flags
/// (`EXPLODING|ANIMATED|MISSILE_UNIT`), `image_index_max` from `UnitInfo`, frames
/// per tick via `UnitsManager_ProgressExplosion` and removal on the last frame.
#[test]
fn missiles_leave_three_particle_trails_behind_them() {
    // (shot type, particle, image_index_max, spawn tick)
    struct Case {
        launcher: UnitType,
        target: UnitType,
        particle: UnitType,
        image_index_max: i16,
        flag_probe: u32,
    }

    for case in [
        Case {
            launcher: UnitType::MissileCrawler,
            target: UnitType::Tank,
            particle: UnitType::RocketSmoke,
            image_index_max: 4, // D_RKTSMOKE.image_count = 5
            flag_probe: 0,
        },
        Case {
            launcher: UnitType::Corvette,
            target: UnitType::Submarine,
            particle: UnitType::TorpedoBubble,
            image_index_max: 2, // D_TRPBUBLE.image_count = 3
            flag_probe: max_core::enums::unit_flags::GROUND_COVER,
        },
    ] {
        let mut state = combat_state();
        // The target is 4 cells away: the projectile must travel several cells and
        // leave several trails.
        let launcher = spawn(&mut state, case.launcher, TeamId::GREEN, Point::new(5, 5));
        spawn(&mut state, case.target, TeamId::RED, Point::new(9, 5));
        state.update_visibility_status(false);

        let mut data = state.snapshot().order_data(launcher);
        data.fire_on = Point::new(9, 5);
        state.set_order_data(launcher, data);
        state.set_order(launcher, UnitOrder::Fire, UnitOrderState::Init);

        let mut seen_ids = std::collections::BTreeSet::new();
        let mut spawn_ticks = Vec::new();
        let mut frames = Vec::new();
        let mut checked_position = false;
        let mut despawned = false;

        for _ in 0..200 {
            state.tick();

            let mut fresh = Vec::new();
            let mut alive = 0;

            for id in state.snapshot().unit_ids_in_order() {
                let Some(unit) = state.snapshot().unit(id) else {
                    continue;
                };
                if unit.unit_type != case.particle {
                    continue;
                }
                alive += 1;

                if seen_ids.insert(id) {
                    fresh.push(unit.clone());
                    frames.push(unit.image_index);
                }

                assert_eq!(
                    unit.image_index_max,
                    case.image_index_max,
                    "{:?}: кадров в D_* — {}",
                    case.particle,
                    case.image_index_max + 1
                );
                assert!(
                    unit.flags & max_core::enums::unit_flags::EXPLODING != 0
                        && unit.flags & max_core::enums::unit_flags::ANIMATED != 0
                        && unit.flags & max_core::enums::unit_flags::MISSILE_UNIT != 0,
                    "{:?}: флаги SC_UNITS (EXPLODING|ANIMATED|MISSILE_UNIT), получено {:#x}",
                    case.particle,
                    unit.flags
                );
                assert_eq!(
                    unit.flags & max_core::enums::unit_flags::GROUND_COVER,
                    case.flag_probe,
                    "{:?}: GROUND_COVER из SC_UNITS",
                    case.particle
                );
            }

            if fresh.len() == 3 {
                spawn_ticks.push(state.snapshot().turn_counter());

                // All three — in the cell the projectile left, and the pixel
                // `x/y` are spread by 0/⅓/⅔ of the segment toward the new cell.
                let mut xs: Vec<i32> = fresh.iter().map(|unit| i32::from(unit.x)).collect();
                xs.sort_unstable();
                let grid = fresh[0].grid;
                assert!(
                    fresh.iter().all(|unit| unit.grid == grid),
                    "{:?}: частицы стоят в покинутой клетке {grid:?}",
                    case.particle
                );
                assert_eq!(xs[0], i32::from(grid.x) * 64 + 32);
                assert!(xs[1] > xs[0], "след разнесён по x");
                assert!(xs[2] > xs[1], "след разнесён по x");

                if !checked_position {
                    checked_position = true;
                }
            }

            if alive == 0 && !seen_ids.is_empty() {
                despawned = true;
                break;
            }
        }

        assert!(
            spawn_ticks.len() >= 2,
            "{:?}: след обязан ставиться на каждую покинутую клетку, получено {}",
            case.particle,
            spawn_ticks.len()
        );
        assert!(
            checked_position,
            "{:?}: три частицы на клетку",
            case.particle
        );
        assert!(!frames.is_empty(), "{:?}: кадры частицы", case.particle);
        assert!(
            despawned,
            "{:?}: частицы удаляются после последнего кадра",
            case.particle
        );
    }
}

/// Determinism: the missile trail enters the state (ids/positions/frames), so
/// the same scenario with one seed must give the same digest.
#[test]
fn missile_trails_are_deterministic() {
    fn run() -> u16 {
        let mut state = combat_state();
        let launcher = spawn(
            &mut state,
            UnitType::MissileCrawler,
            TeamId::GREEN,
            Point::new(5, 5),
        );
        spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(9, 5));
        state.update_visibility_status(false);

        let mut data = state.snapshot().order_data(launcher);
        data.fire_on = Point::new(9, 5);
        state.set_order_data(launcher, data);
        state.set_order(launcher, UnitOrder::Fire, UnitOrderState::Init);

        for _ in 0..40 {
            state.tick();
        }

        state.snapshot().state_digest()
    }

    assert_eq!(run(), run(), "следы ракеты должны быть детерминированы");
}

/// Each hit type gives its own explosion (`UnitsManager_StartExplosion`,
/// `units_manager.cpp:3948-3987`) and is removed after the animation.
#[test]
fn explosions_use_the_cpp_unit_type_and_animate_until_destroyed() {
    for (target_type, flags, expected) in [
        (UnitType::Tank, MOBILE_LAND_UNIT, UnitType::LandExplosion),
        (
            UnitType::Fighter,
            max_core::enums::unit_flags::MOBILE_AIR_UNIT,
            UnitType::AirExplosion,
        ),
        (
            UnitType::Corvette,
            max_core::enums::unit_flags::MOBILE_SEA_UNIT,
            UnitType::SeaExplosion,
        ),
        (
            UnitType::GoldRefinery,
            max_core::enums::unit_flags::BUILDING,
            UnitType::BuildingExplosion,
        ),
    ] {
        let mut state = combat_state();
        let dead = state
            .spawn_unit(target_type, TeamId::GREEN, Point::new(10, 10), flags)
            .expect("spawn");
        // The unit died: `StartExplosion` spawns an effect by the unit class.
        {
            let unit = state.unit_mut(dead).expect("unit");
            unit.hits = 0;
            unit.orders = UnitOrder::Explode;
            unit.state = UnitOrderState::Init;
        }

        let mut seen_frames = Vec::new();
        let mut explosion = None;

        // The longest blast is `BLDEXPLD` (28 frames): `5 × 27 + 1 = 136`
        // simulation steps with `unit_order::EXPLOSION_FRAME_STEPS`.
        for _ in 0..200 {
            state.tick();

            for id in state.snapshot().unit_ids_in_order() {
                if let Some(unit) = state.snapshot().unit(id) {
                    if matches!(
                        unit.unit_type,
                        UnitType::LandExplosion
                            | UnitType::AirExplosion
                            | UnitType::SeaExplosion
                            | UnitType::BuildingExplosion
                            | UnitType::HitExplosion
                    ) {
                        explosion = Some(unit.unit_type);
                        if seen_frames.last() != Some(&unit.image_index) {
                            seen_frames.push(unit.image_index);
                        }
                    }
                }
            }

            if explosion.is_some()
                && !state
                    .snapshot()
                    .unit_ids_in_order()
                    .into_iter()
                    .filter_map(|id| state.snapshot().unit(id))
                    .any(|unit| {
                        matches!(
                            unit.unit_type,
                            UnitType::LandExplosion
                                | UnitType::AirExplosion
                                | UnitType::SeaExplosion
                                | UnitType::BuildingExplosion
                                | UnitType::HitExplosion
                        )
                    })
            {
                break;
            }
        }

        assert_eq!(explosion, Some(expected), "тип взрыва для {target_type:?}");
        assert!(
            seen_frames.len() >= 4,
            "{expected:?}: кадры взрыва должны листаться, получено {seen_frames:?}"
        );
        assert!(
            !state
                .snapshot()
                .unit_ids_in_order()
                .into_iter()
                .filter_map(|id| state.snapshot().unit(id))
                .any(|unit| unit.unit_type == expected),
            "{expected:?} должен исчезнуть после анимации"
        );
    }
}
