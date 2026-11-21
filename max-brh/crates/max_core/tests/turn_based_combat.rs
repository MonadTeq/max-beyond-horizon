//! Turn Based: боевые эффекты не должны держать ход ИИ.
//!
//! Регресс: снаряд (`ROCKET`/`TORPEDO`/…) создаётся с `speed = 0` из
//! `SC_ATTRI`, потому что это юнит-эффект. `AirPath::Execute` в C++ проверяет
//! скорость только у `MOBILE_AIR_UNIT` (`airpath.cpp:186-199`); порт применял
//! проверку ко всем, и снаряд навсегда оставался в `ORDER_MOVE`/
//! `ORDER_STATE_EXECUTING_ORDER`. `UnitsManager_ProcessOrder` каждый тик
//! выставляет `OrdersPending` для `MISSILE_UNIT`, поэтому
//! `Ai_CheckEndTurn` не вызывался и ход ИИ не закрывался до аварийного
//! `LOCAL_AI_TICK_LIMIT` (12 000 кадров) — «зависание на ходу ИИ».
//!
//! Второй регресс: `UnitsManager_StartExplosion` создаёт юнит-эффект с флагами
//! класса (`MOBILE_LAND`/…), а `ANIMATED` приходит из `SC_UNITS`;
//! `spawn_unit` проверял только явные флаги и не переводил эффект в
//! `ORDER_EXPLODE`/`ORDER_STATE_DESTROY`, поэтому взрыв оставался на карте
//! навсегда.

use max_core::combat;
use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::game::GameState;
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};

fn combat_state() -> GameState {
    let mut state = GameState::new();
    state.play_mode = PlayMode::TurnBased;
    state.game_state = max_core::enums::GameStateType::InGame;
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Computer;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);
    // Цель видна стрелку: в партии это даёт разведка, здесь — детерминизм.
    state.ai.all_visible = true;
    state
}

fn spawn(
    state: &mut GameState,
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

/// Ход ИИ закрывается, когда его ракета попала и взрыв доиграл.
#[test]
fn turn_based_ai_turn_ends_after_its_missile_impact() {
    let mut state = combat_state();
    state.active_turn_team = TeamId::GREEN;
    let launcher = spawn(
        &mut state,
        UnitType::MissileCrawler,
        TeamId::GREEN,
        Point::new(5, 5),
    );
    let target = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(13, 5));
    state.update_visibility_status(false);

    state.ai_begin_turn(TeamId::GREEN);
    let mut data = state.order_data(launcher);
    data.fire_on = Point::new(13, 5);
    state.set_order_data(launcher, data);
    state.set_order(launcher, UnitOrder::Fire, UnitOrderState::Init);

    let mut saw_rocket = false;
    let mut saw_explosion = false;

    for tick in 0..400 {
        state.tick();

        for id in state.unit_ids_in_order() {
            if let Some(unit) = state.unit(id) {
                saw_rocket |= unit.unit_type == UnitType::Rocket;
                saw_explosion |=
                    is_combat_effect(unit.unit_type) && unit.unit_type != UnitType::Rocket;
            }
        }

        if state.ai.players[TeamId::GREEN.0 as usize].end_turn_retry_counter > 120 {
            assert!(tick < 300, "ход ИИ закрылся только на тике {tick}");

            let leftovers: Vec<_> = state
                .unit_ids_in_order()
                .into_iter()
                .filter_map(|id| state.unit(id))
                .filter(|unit| is_combat_effect(unit.unit_type))
                .map(|unit| unit.unit_type)
                .collect();
            assert!(
                leftovers.is_empty(),
                "юниты-эффекты остались на карте: {leftovers:?}"
            );
            assert!(
                state.unit(target).is_none_or(|unit| unit.hits < 24),
                "ракета должна была попасть в цель"
            );
            assert!(saw_rocket, "снаряд ROCKET должен был появиться");
            assert!(saw_explosion, "взрыв должен был появиться");
            return;
        }
    }

    panic!(
        "ход ИИ не закрылся за 400 тиков: counter={}, units={:?}",
        state.ai.players[TeamId::GREEN.0 as usize].end_turn_retry_counter,
        state
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.unit(id))
            .map(|unit| (unit.unit_type, unit.orders, unit.state, unit.grid))
            .collect::<Vec<_>>()
    );
}

/// Промах (нет цели в клетке) даёт `HITEXPLD` (`unitinfo.cpp:2713-2723`) —
/// так же юнитом-эффектом с анимацией и удалением.
#[test]
fn a_miss_spawns_a_hit_explosion_that_animates_and_despawns() {
    let mut state = combat_state();
    let shooter = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5));
    state.update_visibility_status(false);

    // Стреляем по пустой клетке в радиусе: `Fire` без цели.
    combat::attack(&mut state, shooter, Point::new(6, 5));

    let mut explosion = None;
    for _ in 0..30 {
        state.tick();

        for id in state.unit_ids_in_order() {
            if let Some(unit) = state.unit(id) {
                if unit.unit_type == UnitType::HitExplosion {
                    explosion = Some(unit.image_index_max);
                }
            }
        }

        if explosion.is_some()
            && !state
                .unit_ids_in_order()
                .into_iter()
                .filter_map(|id| state.unit(id))
                .any(|unit| unit.unit_type == UnitType::HitExplosion)
        {
            break;
        }
    }

    assert_eq!(explosion, Some(4), "HITEXPLD: image_index_max = 4");
    assert!(
        !state
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.unit(id))
            .any(|unit| unit.unit_type == UnitType::HitExplosion),
        "HITEXPLD удаляется после анимации"
    );
}

/// `Paths_DrawMissile` (`airpath.cpp:362-406`): за снарядом тянется след из
/// трёх юнитов-эффектов на каждую покинутую клетку — `RKTSMOKE` у ракеты
/// (`D_RKTSMOKE`: 5 кадров) и `TRPBUBLE` у торпеды (`D_TRPBUBLE`: 3 кадра).
///
/// Проверяется точный порт: тип частицы, позиция (покинутая клетка, но
/// пиксельный сдвиг 0/⅓/⅔ к следующей — `OffsetDrawZones`), флаги
/// (`EXPLODING|ANIMATED|MISSILE_UNIT`), `image_index_max` из `UnitInfo`, кадры
/// по тику через `UnitsManager_ProgressExplosion` и удаление на последнем кадре.
#[test]
fn missiles_leave_three_particle_trails_behind_them() {
    // (тип выстрела, частица, image_index_max, тик появления)
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
        // Цель — в 4 клетках: снаряд должен проехать несколько клеток и
        // оставить несколько следов.
        let launcher = spawn(&mut state, case.launcher, TeamId::GREEN, Point::new(5, 5));
        spawn(&mut state, case.target, TeamId::RED, Point::new(9, 5));
        state.update_visibility_status(false);

        let mut data = state.order_data(launcher);
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

            for id in state.unit_ids_in_order() {
                let Some(unit) = state.unit(id) else { continue };
                if unit.unit_type != case.particle {
                    continue;
                }
                alive += 1;

                if seen_ids.insert(id) {
                    fresh.push(unit.clone());
                    frames.push(unit.image_index);
                }

                assert_eq!(
                    unit.image_index_max, case.image_index_max,
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
                spawn_ticks.push(state.turn_counter);

                // Все три — в клетке, которую снаряд покинул, а пиксельные
                // `x/y` разнесены на 0/⅓/⅔ отрезка к новой клетке.
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
        assert!(checked_position, "{:?}: три частицы на клетку", case.particle);
        assert!(!frames.is_empty(), "{:?}: кадры частицы", case.particle);
        assert!(
            despawned,
            "{:?}: частицы удаляются после последнего кадра",
            case.particle
        );
    }
}

/// Детерминизм: след ракеты входит в состояние (id/позиции/кадры), поэтому
/// одинаковый сценарий с одним seed обязан давать одинаковый дайджест.
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

        let mut data = state.order_data(launcher);
        data.fire_on = Point::new(9, 5);
        state.set_order_data(launcher, data);
        state.set_order(launcher, UnitOrder::Fire, UnitOrderState::Init);

        for _ in 0..40 {
            state.tick();
        }

        max_core::crc::state_digest(&state)
    }

    assert_eq!(run(), run(), "следы ракеты должны быть детерминированы");
}

/// Каждый тип попадания даёт свой взрыв (`UnitsManager_StartExplosion`,
/// `units_manager.cpp:3948-3987`) и удаляется после анимации.
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
        // Юнит погиб: `StartExplosion` спавнит эффект по классу юнита.
        {
            let unit = state.unit_mut(dead).expect("unit");
            unit.hits = 0;
            unit.orders = UnitOrder::Explode;
            unit.state = UnitOrderState::Init;
        }

        let mut seen_frames = Vec::new();
        let mut explosion = None;

        for _ in 0..80 {
            state.tick();

            for id in state.unit_ids_in_order() {
                if let Some(unit) = state.unit(id) {
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
                    .unit_ids_in_order()
                    .into_iter()
                    .filter_map(|id| state.unit(id))
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
                .unit_ids_in_order()
                .into_iter()
                .filter_map(|id| state.unit(id))
                .any(|unit| unit.unit_type == expected),
            "{expected:?} должен исчезнуть после анимации"
        );
    }
}
