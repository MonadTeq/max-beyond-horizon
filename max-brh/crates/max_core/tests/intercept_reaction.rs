//! Reaction-fire intercept at the turn close: a unit that walks into an enemy's
//! weapon range is stopped there and fired upon, instead of finishing its route
//! and exploding on the destination cell.
//!
//! # Oracle
//!
//! * `GameManager_TeamTurnTurnBased` / `GameManager_TeamTurnConcurrent`
//!   (`max-cpp-port/src/game_manager.cpp:488-490`, `:556`) close a team turn with
//!   `while (Access_AreTaskEventsPending()) GameManager_ProgressTurn();`;
//!   `GameManager_ProgressTurn` (`:2586`) is one `GameManager_ProcessTick(false)`,
//!   so every pass runs `UnitsManager_ProcessOrders` — movement **and**
//!   `UnitsManager_AssessAttacks` (reaction fire) **and** `UnitEvent_UnitEvents`
//!   (the emergency stops).
//! * `Access_AreTaskEventsPending` (`access.cpp:2269`) reports `true` while any
//!   `UnitsManager_DelayedAttackTargets[team]` list is non-empty.
//! * `Access_UpdateMapStatus` (`access.cpp:827-868`) queues
//!   `UnitEventEmergencyStop` for a moving unit that spots an enemy able to attack
//!   it; `UnitEventEmergencyStop::Process` (`unitevents.cpp:38`) → `StopMovement`
//!   (`unitinfo.cpp:3222`) → `GroundPath::CancelMovement` (`groundpath.cpp:161`) →
//!   `BlockedOnPathRequest()` (`unitinfo.cpp:3628`) → `MoveFinished(true)`
//!   (`unitinfo.cpp:3654`) → `UnitsManager_AddToDelayedReactionList`
//!   (`unitinfo.cpp:3692-3694`). The halted unit is a reaction target at the
//!   halt cell.
//! * `UnitsManager_CheckReaction` (`units_manager.cpp:4709`) is the reaction
//!   predicate: shots, no delayed reaction, `Access_IsWithinAttackRange`,
//!   `Access_IsValidAttackTarget`, `UnitsManager_ShouldAttack` (visibility).
//!
//! # Defect (fixed)
//!
//! The port's turn-close drain called `UnitInfo::ExecuteMove` only: it did not
//! process the queued emergency stops and did not run the reaction pass, so a
//! unit kept walking to its destination and only then became a reaction target.
//! The stopped unit was also never added to the delayed reaction list because the
//! event handler used the unit-level `StopMovement` without the `MoveFinished`
//! side effects. Both paths are covered here.

mod support;

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{PathStep, Point};
use max_core::team::TeamId;
use max_core::unit::{GroundPath, UnitPath};
use max_core::universe::GameUniverse;
use max_core::UnitId;

/// Gives the unit a ready route east `steps` cells long and `speed` movement
/// points, as the path manager leaves it after `PathRequest::Finish`
/// (`pathrequest.cpp:145`: `state = ORDER_STATE_IN_PROGRESS`).
fn give_route(universe: &mut GameUniverse, id: UnitId, steps: usize, speed: u16) {
    let end = {
        let unit = universe.unit_mut(id).expect("unit");
        let mut end = unit.grid;

        for _ in 0..steps {
            end.x += 1;
        }

        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::InProgress;
        unit.speed = speed;
        unit.path = Some(UnitPath::Ground(GroundPath {
            end_x: end.x,
            end_y: end.y,
            step_index: 0,
            steps: vec![PathStep::new(1, 0); steps],
        }));

        end
    };

    universe.set_order_data(
        id,
        max_core::game::OrderData {
            move_to: end,
            fire_on: Point::ZERO,
        },
    );
}

struct Scenario {
    universe: GameUniverse,
    mover: UnitId,
    enemy: UnitId,
    destination: Point,
}

/// A RED tank ordered 8 cells east, a GREEN tank 10 cells ahead (range 4, scan 4).
///
/// `fog` — the enemy is initially unseen: the mover reveals it while walking.
/// Otherwise the enemy is scouted first (`SpotByTeam`), so the mover cannot
/// reveal it again and the `enemy_halt` reveal check stays silent — exactly the
/// case where the original port ignored the enemy and walked to the destination.
fn scenario(enemy_halt: bool, fog: bool) -> Scenario {
    let mut universe = support::fog_game();
    universe.set_enemy_halt(enemy_halt);
    universe.begin_local_round();

    let start = Point::new(10, 10);
    let mover = support::spawn(&mut universe, UnitType::Tank, TeamId::RED, start);
    let enemy = support::spawn(
        &mut universe,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(20, 10),
    );

    if !fog {
        universe.update_visibility_status(false);
        universe.spot_by_team(enemy, TeamId::RED);
    }

    give_route(&mut universe, mover, 8, 8);

    Scenario {
        universe,
        mover,
        enemy,
        destination: Point::new(start.x + 8, start.y),
    }
}

/// The distance at which the enemy's `ATTRIB_RANGE` starts, in cells: the mover
/// must be stopped exactly there (`UnitsManager_CheckReaction` range check).
fn enemy_range(universe: &GameUniverse, enemy: UnitId) -> i16 {
    let unit = universe.snapshot().unit(enemy).expect("enemy");
    unit.base_values
        .get_attribute(max_core::unit_values::ATTRIB_RANGE) as i16
}

#[test]
fn the_turn_close_stops_a_mover_in_the_enemy_range_and_fires_at_the_halt_cell() {
    let mut scenario = scenario(true, true);
    let range = enemy_range(&scenario.universe, scenario.enemy);
    let hits_before = scenario
        .universe
        .snapshot()
        .unit(scenario.mover)
        .expect("mover")
        .hits;

    let over = scenario.universe.end_local_turn(TeamId::RED);
    assert!(!over, "the match is not over");

    // The halted unit is already on `UnitsManager_DelayedAttackTargets`;
    // `UnitsManager_AssessAttacks` opens fire on the next order pass.
    for _ in 0..60 {
        if scenario
            .universe
            .snapshot()
            .unit(scenario.mover)
            .is_some_and(|unit| unit.hits < hits_before)
        {
            break;
        }
        scenario.universe.tick();
    }

    let mover = scenario
        .universe
        .snapshot()
        .unit(scenario.mover)
        .cloned()
        .expect("mover survives the intercept");

    assert_eq!(
        mover.grid,
        Point::new(20 - range, 10),
        "the mover must stop where it enters the enemy's weapon range, not at the destination"
    );
    assert_ne!(
        mover.grid, scenario.destination,
        "the route is interrupted: the destination is never reached"
    );
    assert!(
        mover.hits < hits_before,
        "the enemy must open reaction fire at the halt cell: {hits_before} -> {}",
        mover.hits
    );
    assert_eq!(
        mover.orders,
        UnitOrder::Await,
        "the halted unit waits after `BlockedOnPathRequest`"
    );
}

/// The reported case: the enemy was already scouted (`SpotByTeam`), so the
/// `enemy_halt` reveal check has nothing to reveal. The reaction check from the
/// enemy side still stops the mover at the range boundary — the intercept.
#[test]
fn a_scouted_enemy_still_intercepts_the_mover() {
    let mut scenario = scenario(true, false);
    let range = enemy_range(&scenario.universe, scenario.enemy);
    let hits_before = scenario
        .universe
        .snapshot()
        .unit(scenario.mover)
        .expect("mover")
        .hits;

    scenario.universe.end_local_turn(TeamId::RED);

    // The reaction fires on the next order pass (see the previous test).
    for _ in 0..60 {
        if scenario
            .universe
            .snapshot()
            .unit(scenario.mover)
            .is_some_and(|unit| unit.hits < hits_before)
        {
            break;
        }
        scenario.universe.tick();
    }

    let mover = scenario
        .universe
        .snapshot()
        .unit(scenario.mover)
        .cloned()
        .expect("mover survives the intercept");

    assert_eq!(
        mover.grid,
        Point::new(20 - range, 10),
        "a scouted enemy must still interrupt the movement at the range boundary"
    );
    assert!(
        mover.hits < hits_before,
        "the scouted enemy fires at the halted unit: {hits_before} -> {}",
        mover.hits
    );
}

/// Control: with `enemy_halt` off the unit does not stop (the preference is exactly
/// the "halt movement on a detected enemy" switch of `settings.cpp:134`) and is
/// fired upon only after it reaches the destination — the behaviour the fix
/// removes from the default configuration.
#[test]
fn enemy_halt_off_lets_the_mover_reach_the_destination() {
    let mut scenario = scenario(false, false);

    scenario.universe.end_local_turn(TeamId::RED);

    let mover = scenario
        .universe
        .snapshot()
        .unit(scenario.mover)
        .cloned()
        .expect("mover survives the destination shot");

    assert_eq!(
        mover.grid, scenario.destination,
        "without `enemy_halt` the route is walked to its end"
    );
    assert_eq!(
        mover.orders,
        UnitOrder::Await,
        "the destination shot leaves the unit waiting there"
    );
}
