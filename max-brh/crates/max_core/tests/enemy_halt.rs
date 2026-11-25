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
        GameUniverse::default().snapshot().map().clone(),
        [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
    );

    state.set_player_team(TeamId::RED);
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
    state.set_enemy_halt(enemy_halt);

    // The unit "enters" an unscanned area: as in the game, where a step
    // reveals a new cell. Otherwise the cell is already covered by spawn and
    // `Access_OnCellRevealed` is not called (C++ behaves the same way).
    for map in &mut state.ai_mut().heat_maps {
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
            .snapshot()
            .unit_events()
            .iter()
            .any(|event| event.unit() == mover),
        "движущийся юнит, увидевший врага, должен получить EmergencyStop"
    );

    // Applying the event (which `UnitsManager_ProcessOrders` does) actually
    // stops the unit: `StopMovement` → `BlockedOnPathRequest`.
    let event = state.unit_events_mut().remove(0);
    state.process_unit_event(&event);

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

    state.update_map_status(mover, true);

    assert!(
        state.snapshot().unit_events().is_empty(),
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

    state.update_map_status(mover, true);

    assert!(
        state.snapshot().unit_events().is_empty(),
        "C++ проверяет `order != ORDER_AWAIT` (access.cpp:830)"
    );
}

#[test]
fn enemy_halt_ignores_remote_teams() {
    let (mut state, mover, _) = setup_two_units(true);
    state.teams_mut()[0].team_type = TeamKind::Remote;

    state.update_map_status(mover, true);

    assert!(
        state.snapshot().unit_events().is_empty(),
        "C++ останавливает только TEAM_TYPE_PLAYER/COMPUTER (access.cpp:828)"
    );
}

/// The scan rule of `Access_UpdateMapStatus` (`access.cpp:762-796`) is exposed once
/// (`GameState::scan_radius`) and shared with the frame-side vision: a unit that
/// does not scan must report `None`, a scanning one — its `ATTRIB_SCAN` radius.
#[test]
fn scan_radius_mirrors_the_map_status_rule() {
    let (mut state, mover, _) = setup_two_units(true);
    let unit = state.snapshot().unit(mover).cloned().expect("юнит");
    let scan = unit
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_SCAN);

    assert_eq!(
        state.snapshot().scan_radius(&unit),
        Some(scan),
        "ORDER_MOVE + SELECTABLE → сканирует"
    );

    // `ORDER_IDLE` units are skipped by C++ (`if (unit->GetOrder() != ORDER_IDLE)`).
    let mut idle = unit.clone();
    idle.orders = UnitOrder::Idle;
    assert_eq!(state.snapshot().scan_radius(&idle), None);

    // `GameManager_AllVisible` replaces the scan with `SpotByTeam`.
    state.ai_mut().all_visible = true;
    assert_eq!(state.snapshot().scan_radius(&unit), None);
}

/// `UnitInfo::SpotByTeam` → `RadarPing` (`unitinfo.cpp:2833-2834`): when the
/// **player's** team spots an enemy, C++ calls `GameManager_NotifyEvent(this, 0)`
/// («Enemy %s in radar range.», `game_manager.cpp:3458`). The report travels as a
/// [`max_core::universe::GameEvent::EnemySpotted`] and never enters the state.
#[test]
fn the_player_team_spotting_an_enemy_emits_a_radar_ping() {
    use max_core::universe::GameEvent;

    let (mut state, _, enemy) = setup_two_units(true);
    let mut events = state.subscribe();

    state.spot_by_team(enemy, TeamId::RED);
    state.tick();

    let reported = events.drain(&mut state);

    assert!(
        reported
            .iter()
            .any(|event| matches!(event, GameEvent::EnemySpotted { unit } if *unit == enemy)),
        "SpotByTeam команды игрока обязан дать radar ping: {reported:?}"
    );
}

/// Only the **player's** team pings: a computer team spotting a unit is an AI
/// report (`Ai_UnitSpotted`), not a message box for the human
/// (`if (team == GameManager_PlayerTeam)`, `unitinfo.cpp:2833`).
#[test]
fn a_foreign_team_spotting_a_unit_does_not_ping_the_player() {
    use max_core::universe::GameEvent;

    let (mut state, mover, _) = setup_two_units(true);

    // Deployment is itself an `Access_UpdateMapStatus(unit, true)`: a unit placed
    // inside the player's already scanned area is reported right away, as in the
    // oracle. Consume that setup-time report before isolating the foreign spot.
    state.tick();

    let mut events = state.subscribe();

    state.spot_by_team(mover, TeamId::GREEN);
    state.tick();

    let reported = events.drain(&mut state);

    assert!(
        !reported
            .iter()
            .any(|event| matches!(event, GameEvent::EnemySpotted { .. })),
        "чужой скан не рисует сообщение игроку: {reported:?}"
    );
}

/// Effects are **not units**: `UnitInfo::RadarPing` (`unitinfo.cpp:3711-3717`)
/// announces a spotted enemy only when
/// `orders != ORDER_BUILD/CLEAR && (!(flags & STATIONARY) || mine) && (flags & SELECTABLE)`.
///
/// Explosions, rocket smoke and torpedo bubbles are `STATIONARY` without
/// `SELECTABLE` in `SC_UNITS`, so the original never showed «Enemy Land Explosion
/// in radar range». The port announced every `SpotByTeam` of the player's team —
/// every shot and every blast produced a bogus «unit "explosion" in radar range»
/// message (the playtest defect).
#[test]
fn spotted_effects_do_not_ping_the_radar() {
    use max_core::enums::unit_flags::STATIONARY;
    use max_core::universe::GameEvent;

    for effect in [
        UnitType::LandExplosion,
        UnitType::AirExplosion,
        UnitType::SeaExplosion,
        UnitType::BuildingExplosion,
        UnitType::HitExplosion,
        UnitType::RocketSmoke,
        UnitType::TorpedoBubble,
    ] {
        let mut state = state();
        // An own unit with scan coverage creates the heat map the reveal runs on.
        let watcher = state
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 20), 0)
            .expect("танк");
        let effect_id = state.spawn_unit(effect, TeamId::GREEN, Point::new(20, 21), 0);

        let Some(effect_id) = effect_id else {
            // Some effect types are rejects for `spawn_unit` in a bare state;
            // the flags check below is then the only probe available.
            let flags = max_core::access::unit_type_flags(effect);
            assert!(flags & STATIONARY != 0, "{effect:?}: STATIONARY");
            assert_eq!(
                flags & max_core::enums::unit_flags::SELECTABLE,
                0,
                "{effect:?}: эффекты не SELECTABLE"
            );
            continue;
        };

        let mut effect_unit = state.snapshot().unit(effect_id).cloned().expect("эффект");
        effect_unit.visible_to_team = [false; 5];
        state.write_unit(effect_id, effect_unit);

        // The reveal path: the own unit scans the effect's cell.
        state.update_map_status(watcher, true);
        state.spot_by_team(effect_id, TeamId::RED);

        let mut events = state.subscribe();
        state.tick();
        let reported = events.drain(&mut state);

        assert!(
            !reported.iter().any(
                |event| matches!(event, GameEvent::EnemySpotted { unit } if *unit == effect_id)
            ),
            "{effect:?}: эффект не должен давать radar ping: {reported:?}"
        );
    }
}

/// The filter must not silence real units: a visible enemy tank still pings.
#[test]
fn a_spotted_enemy_tank_still_pings_the_radar() {
    use max_core::universe::GameEvent;

    let (mut state, _, enemy) = setup_two_units(true);
    let mut events = state.subscribe();

    state.spot_by_team(enemy, TeamId::RED);
    state.tick();

    let reported = events.drain(&mut state);

    assert!(
        reported
            .iter()
            .any(|event| matches!(event, GameEvent::EnemySpotted { unit } if *unit == enemy)),
        "боевой юнит обязан дать radar ping: {reported:?}"
    );
}
