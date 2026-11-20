//! `enemy_halt` — «Halt movement when enemy is detected» (Preferences).
//!
//! Оракул: `Access_UpdateMapStatus` (`max-cpp-port/src/access.cpp:762-868`):
//! при добавлении покрытия сканирования юнит проверяет, не открылась ли клетка
//! с врагом, которого он (или его группа) может атаковать; если да и у юнита
//! есть активный приказ (`order != ORDER_AWAIT`), ставится
//! `UnitEventEmergencyStop`, который `UnitsManager_ProcessOrders` применяет
//! (`UnitInfo::StopMovement`, `unitinfo.cpp:3222`).
//!
//! Настройка `enemy_halt` читается из `settings` в момент проверки, поэтому в
//! порте это поле [`GameState::enemy_halt`]; тесты проверяют обе ветки.

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::game::GameState;
use max_core::geo::{PathStep, Point};
use max_core::team::{TeamId, TeamKind};
use max_core::unit::{GroundPath, UnitPath};

fn state() -> GameState {
    let mut state = GameState::with_setup(
        GameState::new().map,
        [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
    );

    state.player_team = TeamId::RED;
    state
}

/// Ставит юниту-танку активный приказ движения с маршрутом.
fn order_move(state: &mut GameState, id: max_core::unit::UnitId, to: Point) {
    let mut unit = state.unit(id).cloned().expect("юнит");

    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::InProgress;
    unit.path = Some(UnitPath::Ground(GroundPath {
        end_x: to.x,
        end_y: to.y,
        step_index: 0,
        steps: vec![PathStep::new(1, 0), PathStep::new(2, 0)],
    }));

    state.write_unit(id, unit);
}

fn setup_two_units(enemy_halt: bool) -> (GameState, max_core::unit::UnitId, max_core::unit::UnitId) {
    let mut state = state();

    let mover = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 20), 0)
        .expect("танк RED");
    let enemy = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(21, 20), 0)
        .expect("танк GREEN");

    // Враг видим команде не сразу: его открывает `update_map_status` (как
    // `Access_OnCellRevealed`), иначе проверка `enemy_target_class` не сработает.
    let mut enemy_unit = state.unit(enemy).cloned().expect("враг");
    enemy_unit.visible_to_team = [false; 5];

    state.write_unit(enemy, enemy_unit);
    order_move(&mut state, mover, Point::new(26, 20));
    state.enemy_halt = enemy_halt;

    // Юнит «входит» в ещё не просканированную область: как в игре, где шаг
    // открывает новую клетку. Иначе клетка уже покрыта спавном и
    // `Access_OnCellRevealed` не вызывается (C++ ведёт себя так же).
    for map in &mut state.ai.heat_maps {
        map.clear();
    }

    (state, mover, enemy)
}

#[test]
fn enemy_halt_queues_an_emergency_stop_for_a_moving_unit() {
    let (mut state, mover, _) = setup_two_units(true);

    state.update_map_status(mover, true);

    assert!(
        state
            .unit_events
            .iter()
            .any(|event| event.unit() == mover),
        "движущийся юнит, увидевший врага, должен получить EmergencyStop"
    );

    // Применение события (это делает `UnitsManager_ProcessOrders`) реально
    // останавливает юнита: `StopMovement` → `BlockedOnPathRequest`.
    let event = state.unit_events.remove(0);
    max_core::events::process_unit_event(&mut state, &event);

    let unit = state.unit(mover).expect("юнит жив");
    assert_ne!(
        unit.orders,
        UnitOrder::Move,
        "приказ движения снят (ORDER_AWAIT после остановки)"
    );
    assert_ne!(unit.state, UnitOrderState::InProgress);
}

#[test]
fn enemy_halt_off_keeps_the_unit_moving() {
    let (mut state, mover, _) = setup_two_units(false);

    state.update_map_status(mover, true);

    assert!(
        state.unit_events.is_empty(),
        "при выключенной настройке событий остановки нет"
    );

    let unit = state.unit(mover).expect("юнит жив");
    assert_eq!(unit.orders, UnitOrder::Move);
}

#[test]
fn enemy_halt_ignores_idle_units() {
    let (mut state, mover, _) = setup_two_units(true);

    if let Some(mut unit) = state.unit(mover).cloned() {
        unit.orders = UnitOrder::Await;
        state.write_unit(mover, unit);
    }

    state.update_map_status(mover, true);

    assert!(
        state.unit_events.is_empty(),
        "C++ проверяет `order != ORDER_AWAIT` (access.cpp:830)"
    );
}

#[test]
fn enemy_halt_ignores_remote_teams() {
    let (mut state, mover, _) = setup_two_units(true);
    state.teams[0].team_type = TeamKind::Remote;

    state.update_map_status(mover, true);

    assert!(
        state.unit_events.is_empty(),
        "C++ останавливает только TEAM_TYPE_PLAYER/COMPUTER (access.cpp:828)"
    );
}
