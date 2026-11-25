//! Sentry reaction fire: a unit in `ORDER_SENTRY` must attack an enemy that
//! enters its attack range as soon as that enemy is visible to its team.
//!
//! # Oracle
//!
//! * `UnitsManager_CheckReaction` (`max-cpp-port/src/units_manager.cpp:4709`) вЂ”
//!   `shots > 0`, no delayed reaction, `Access_IsWithinAttackRange`,
//!   `Access_IsValidAttackTarget`, `UnitsManager_ShouldAttack` в†’ `ORDER_FIRE`;
//! * `UnitsManager_ShouldAttack` (`units_manager.cpp:4623`) вЂ” the sentry branch
//!   `GameManager_PlayMode == PLAY_MODE_TURN_BASED || order == ORDER_SENTRY`
//!   requires `unit2->IsVisibleToTeam(unit1->team)` first;
//! * `UnitsManager_AddToDelayedReactionList` (`units_manager.cpp:3435`) and
//!   `UnitsManager_CheckDelayedReactions` (`units_manager.cpp:4888`) вЂ” the
//!   trigger: a moving unit is queued and every other team checks reactions;
//! * `Access_UpdateMapStatus` (`access.cpp:762`) в†’ `Access_DrawUnit`
//!   (`access.cpp:691`) вЂ” the moved unit is spotted when its new cell lies
//!   inside a team's scan coverage; heat-map callouts alone
//!   (`Access_OnCellRevealed`) only fire for cells that become covered.
//!
//! # Defect (fixed)
//!
//! `GameState::update_map_status_unit` (`ai/threat.rs`) ported the scan-coverage
//! part of `Access_UpdateMapStatus` but not the per-unit visibility pass
//! (`Access_DrawUnit`, which uses `UnitInfo::Draw` for a cell outside the scan).
//! A unit that moved into an **already scanned** cell therefore kept
//! `visible_to_team = false`; `UnitsManager_ShouldAttack` rejected it and the
//! sentry never fired.

use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{GameStateType, PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::input::{Command, Packet41MoveFinished};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::unit_values::{ATTRIB_ARMOR, ATTRIB_ATTACK, ATTRIB_ROUNDS};
use max_core::universe::GameUniverse;
use max_core::UnitId;

fn scenario() -> GameUniverse {
    let mut state = GameUniverse::default();
    // Real-time (`PLAY_MODE_UNKNOWN`): the turn-based shortcut of
    // `UnitsManager_ShouldAttack` must not be what makes the sentry fire.
    state.set_play_mode(PlayMode::Unknown);
    state.set_game_state(GameStateType::InGame);
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.teams_mut()[1].team_type = TeamKind::Player;
    state.team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units_table_mut()[1] = TeamUnits::with_clan(TeamClan::Random);
    state.set_active_turn_team(TeamId::RED);
    state
}

fn spawn(state: &mut GameUniverse, unit_type: UnitType, team: TeamId, grid: Point) -> UnitId {
    state
        .spawn_unit(unit_type, team, grid, MOBILE_LAND_UNIT | SELECTABLE)
        .expect("spawn")
}

fn set_sentry(state: &mut GameUniverse, id: UnitId) {
    state.set_order(id, UnitOrder::Sentry, UnitOrderState::ExecutingOrder);
}

/// Teleports a unit (the result of a finished move) and refreshes its coverage
/// and visibility exactly like `UnitInfo::UpdateGridPosition` does
/// (`write_unit` в†’ `Access_UpdateMapStatus(unit, false)` then `(unit, true)`).
fn arrive(state: &mut GameUniverse, id: UnitId, grid: Point) {
    let mut unit = state.snapshot().unit(id).expect("unit").clone();
    unit.grid = grid;
    state.write_unit(id, unit);
}

/// A green tank moves into the range and the scan coverage of a red sentry tank;
/// the sentry must acquire `ORDER_FIRE` on that same movement (`MoveFinished`)
/// and damage the target.
#[test]
fn sentry_fires_at_an_enemy_that_moves_into_range_and_scan() {
    let mut state = scenario();
    let red = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5));
    // TANK: range 4, scan 4 вЂ” start well outside both.
    let green = spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(20, 5));
    set_sentry(&mut state, red);
    state.update_visibility_status(false);

    assert!(!state
        .snapshot()
        .unit(green)
        .expect("green")
        .is_visible_to_team(TeamId::RED));

    // The enemy finishes its move at distance 3: inside range 4 and inside the
    // red scan radius 4 of the already computed coverage.
    arrive(&mut state, green, Point::new(8, 5));
    assert!(
        state
            .snapshot()
            .unit(green)
            .expect("green")
            .is_visible_to_team(TeamId::RED),
        "Access_DrawUnit: the mover must be spotted inside the team's scanned cell"
    );

    // `MoveFinished(mode = true)` вЂ” the packet 41 path: `delayed_reaction` and
    // `AddToDelayedReactionList`.
    state.submit(Command::MoveFinished(Packet41MoveFinished::port_form(
        green, true,
    )));
    state.tick();

    let shooter = state.snapshot().unit(red).expect("red");
    assert_eq!(
        shooter.orders,
        UnitOrder::Fire,
        "a sentry must react immediately when the target enters range and is visible"
    );
    assert_eq!(shooter.state, UnitOrderState::Init);
    assert_eq!(state.snapshot().order_data(red).fire_on, Point::new(8, 5));

    let hits_before = state.snapshot().unit(green).expect("green").hits;
    for _ in 0..120 {
        state.tick();
    }
    let hits_after = state.snapshot().unit(green).expect("green").hits;
    assert!(
        hits_after < hits_before,
        "the reaction shot must resolve: {hits_before} -> {hits_after}"
    );
}

/// The visibility half of the condition: the same approach inside the attack
/// range but outside the scan coverage must not trigger reaction fire.
#[test]
fn sentry_does_not_fire_at_an_enemy_inside_range_but_unseen() {
    let mut state = scenario();
    // MISSILE CRAWLER: range 10, scan 4.
    let red = spawn(
        &mut state,
        UnitType::MissileCrawler,
        TeamId::RED,
        Point::new(5, 5),
    );
    let green = spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(20, 5));
    set_sentry(&mut state, red);
    state.update_visibility_status(false);

    // Distance 6: inside range 10, outside scan 4.
    arrive(&mut state, green, Point::new(11, 5));
    assert!(!state
        .snapshot()
        .unit(green)
        .expect("green")
        .is_visible_to_team(TeamId::RED));

    state.submit(Command::MoveFinished(Packet41MoveFinished::port_form(
        green, true,
    )));
    let hits_before = state.snapshot().unit(green).expect("green").hits;
    for _ in 0..10 {
        state.tick();
    }

    let shooter = state.snapshot().unit(red).expect("red");
    assert_eq!(
        shooter.orders,
        UnitOrder::Sentry,
        "an unseen target must not trigger reaction fire"
    );
    assert_eq!(
        state.snapshot().unit(green).expect("green").hits,
        hits_before,
        "no damage may be dealt to an unseen target"
    );
}

/// Reaction fire must **spend** `shots`: `UnitInfo::PrepareFire`
/// (`max-cpp-port/src/unitinfo.cpp:4805-4818`) takes one `ammo` and one `shots`
/// per shot outside the real-time debug setting, and `UnitsManager_CheckReaction`
/// (`units_manager.cpp:4712`) refuses a shooter with `shots == 0`. A `TANK` has
/// `ATTRIB_ROUNDS = 2`, so a sentry fires exactly twice per turn: two shots, then
/// the fire order is dropped and no further damage is dealt. There is no
/// unbounded fire loop, and a shot is never taken at `shots == 0` (which would
/// wrap the `uint8_t` counter to 255).
#[test]
fn reaction_fire_spends_shots_and_stops_when_they_are_gone() {
    let mut state = scenario();
    let red = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5));
    let green = spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(20, 5));
    set_sentry(&mut state, red);
    state.update_visibility_status(false);

    // In attack range (4) and inside the sentry's scan (4).
    arrive(&mut state, green, Point::new(8, 5));
    state.submit(Command::MoveFinished(Packet41MoveFinished::port_form(
        green, true,
    )));

    let tank = state.snapshot().unit(red).expect("red").base_values;
    let rounds = tank.get_attribute(ATTRIB_ROUNDS);
    let damage_per_shot = tank.get_attribute(ATTRIB_ATTACK)
        - state
            .snapshot()
            .unit(green)
            .expect("green")
            .base_values
            .get_attribute(ATTRIB_ARMOR);
    assert!(rounds >= 2, "the scenario needs a multi-shot unit");

    let ammo_before = state.snapshot().unit(red).expect("red").ammo;
    let hits_before = state.snapshot().unit(green).expect("green").hits;

    let mut shots_taken = 0;
    let mut previous = (
        state.snapshot().unit(red).expect("red").shots,
        state.snapshot().unit(red).expect("red").ammo,
    );

    for _ in 0..300 {
        state.tick();

        let shooter = state.snapshot().unit(red).expect("red").clone();
        let (shots, ammo) = (shooter.shots, shooter.ammo);

        if ammo < previous.1 {
            shots_taken += 1;
            assert!(
                previous.0 > 0,
                "a shot must not be taken at `shots == 0` (the counter would wrap)"
            );
            assert_eq!(
                shots,
                previous.0 - 1,
                "`UnitInfo::PrepareFire` takes exactly one shot"
            );
        }

        assert!(
            i32::from(shots) <= rounds,
            "`shots` never exceeds `ATTRIB_ROUNDS`"
        );
        previous = (shots, ammo);
    }

    assert_eq!(
        shots_taken, rounds,
        "a sentry fires `ATTRIB_ROUNDS` times per turn, not forever"
    );
    assert_eq!(
        state.snapshot().unit(red).expect("red").shots,
        0,
        "the shot budget is exhausted"
    );
    assert_eq!(
        state.snapshot().unit(red).expect("red").ammo,
        ammo_before - rounds as u8,
        "every shot is paid for in ammo"
    );
    assert_eq!(
        state.snapshot().unit(green).expect("green").hits,
        hits_before - (damage_per_shot * rounds) as u16,
        "exactly the `ATTRIB_ROUNDS` shots deal damage"
    );
}
