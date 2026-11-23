//! `enemy_halt` — «Halt movement when enemy is detected» (Preferences).
//!
//! Oracle: `Access_UpdateMapStatus` (`max-cpp-port/src/access.cpp:762-868`):
//! when scan coverage is added, the unit checks whether a cell with an enemy
//! it (or its group) can attack has been revealed; if so and the unit
//! has an active order (`order != ORDER_AWAIT`), `UnitEventEmergencyStop` is queued,
//! which `UnitsManager_ProcessOrders` applies
//! (`UnitInfo::StopMovement`, `unitinfo.cpp:3222`).
//!
//! The `enemy_halt` setting is read from `settings` at check time, so in
//! the port it is the [`GameUniverse::enemy_halt`] field; the tests check both branches.

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{PathStep, Point};
use max_core::team::{TeamId, TeamKind};
use max_core::unit::{GroundPath, UnitPath};
use max_core::universe::GameUniverse;

fn state() -> GameUniverse {
    let mut state = GameUniverse::with_setup(
        GameUniverse::default().state().map.clone(),
        [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
    );

    state.state_mut().player_team = TeamId::RED;
    state
}

/// Gives the tank unit an active movement order with a route.
fn order_move(state: &mut GameUniverse, id: max_core::unit::UnitId, to: Point) {
    let mut unit = state.snapshot().unit(id).cloned().expect("юнит");

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

fn setup_two_units(
    enemy_halt: bool,
) -> (GameUniverse, max_core::unit::UnitId, max_core::unit::UnitId) {
    let mut state = state();

    let mover = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 20), 0)
        .expect("танк RED");
    let enemy = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(21, 20), 0)
        .expect("танк GREEN");

    // The enemy is not immediately visible to the team: it is revealed by `update_map_status` (like
    // `Access_OnCellRevealed`), otherwise the `enemy_target_class` check would not fire.
    let mut enemy_unit = state.snapshot().unit(enemy).cloned().expect("враг");
    enemy_unit.visible_to_team = [false; 5];

    state.write_unit(enemy, enemy_unit);
    order_move(&mut state, mover, Point::new(26, 20));
    state.state_mut().enemy_halt = enemy_halt;

    // The unit "enters" an unscanned area: as in the game, where a step
    // reveals a new cell. Otherwise the cell is already covered by spawn and
    // `Access_OnCellRevealed` is not called (C++ behaves the same way).
    for map in &mut state.state_mut().ai.heat_maps {
        map.clear();
    }

    (state, mover, enemy)
}

#[test]
fn enemy_halt_queues_an_emergency_stop_for_a_moving_unit() {
    let (mut state, mover, _) = setup_two_units(true);

    state.state_mut().update_map_status(mover, true);

    assert!(
        state
            .state()
            .unit_events
            .iter()
            .any(|event| event.unit() == mover),
        "движущийся юнит, увидевший врага, должен получить EmergencyStop"
    );

    // Applying the event (which `UnitsManager_ProcessOrders` does) actually
    // stops the unit: `StopMovement` → `BlockedOnPathRequest`.
    let event = state.state_mut().unit_events.remove(0);
    max_core::events::process_unit_event(state.state_mut(), &event);

    let unit = state.snapshot().unit(mover).expect("юнит жив");
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

    state.state_mut().update_map_status(mover, true);

    assert!(
        state.state().unit_events.is_empty(),
        "при выключенной настройке событий остановки нет"
    );

    let unit = state.snapshot().unit(mover).expect("юнит жив");
    assert_eq!(unit.orders, UnitOrder::Move);
}

#[test]
fn enemy_halt_ignores_idle_units() {
    let (mut state, mover, _) = setup_two_units(true);

    if let Some(mut unit) = state.snapshot().unit(mover).cloned() {
        unit.orders = UnitOrder::Await;
        state.write_unit(mover, unit);
    }

    state.state_mut().update_map_status(mover, true);

    assert!(
        state.state().unit_events.is_empty(),
        "C++ проверяет `order != ORDER_AWAIT` (access.cpp:830)"
    );
}

#[test]
fn enemy_halt_ignores_remote_teams() {
    let (mut state, mover, _) = setup_two_units(true);
    state.state_mut().teams[0].team_type = TeamKind::Remote;

    state.state_mut().update_map_status(mover, true);

    assert!(
        state.state().unit_events.is_empty(),
        "C++ останавливает только TEAM_TYPE_PLAYER/COMPUTER (access.cpp:828)"
    );
}
