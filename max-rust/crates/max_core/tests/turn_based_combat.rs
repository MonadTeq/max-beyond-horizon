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
            UnitType::CommTower,
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
