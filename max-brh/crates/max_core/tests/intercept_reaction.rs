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

    // The turn close is incremental (`GameState::ClosingTurn`): the drain walks the
    // route one visual budget window at a time, so the halt and the reaction fire
    // happen while the scenario ticks.
    let over = support::close_turn(&mut scenario.universe, TeamId::RED);
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

    support::close_turn(&mut scenario.universe, TeamId::RED);

    // The reaction fires during the same close (see the previous test).
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

    support::close_turn(&mut scenario.universe, TeamId::RED);

    // The destination shot is resolved after the walk; give the reaction pass its
    // ticks (the drain itself no longer runs ahead of the drawing).
    for _ in 0..60 {
        if scenario
            .universe
            .snapshot()
            .unit(scenario.mover)
            .is_some_and(|unit| unit.orders == UnitOrder::Await)
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

/// The explosion of a unit killed by the intercept is spawned on the **halt cell**,
/// not on the destination of the interrupted route.
///
/// Oracle: `UnitsManager_StartExplosion` (`units_manager.cpp:3991`) deploys the
/// effect at `unit->grid_x, unit->grid_y` — the cell the dead unit **occupies** at
/// death — and then aligns it with the unit's pixel position
/// (`explosion->OffsetDrawZones(unit->x - explosion->x, unit->y - explosion->y)`,
/// `:3991-3994`). The order data (`move_to`, the route destination) is never a
/// position source: the old manual-run symptom was "the unit is stopped and shot at
/// on the halt cell, but the blast is shown on the destination".
#[test]
fn a_killed_intercepted_unit_explodes_on_the_halt_cell() {
    let mut scenario = scenario(true, false);
    let range = enemy_range(&scenario.universe, scenario.enemy);
    let halt_cell = Point::new(20 - range, 10);

    {
        let mover = scenario.universe.unit_mut(scenario.mover).expect("mover");
        mover.hits = 1;
    }

    // The intercept is resolved inside the incremental drain; the explosion lives a
    // few ticks, so it is looked for on every tick of the close (`hits = 1`: the
    // first reaction shot kills the mover).
    scenario.universe.end_local_turn(TeamId::RED);

    let mut explosion = None;
    for _ in 0..support::TURN_CLOSE_TICK_LIMIT {
        scenario.universe.tick();

        explosion = scenario
            .universe
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| scenario.universe.snapshot().unit(id).cloned())
            .find(|unit| {
                matches!(
                    unit.unit_type,
                    UnitType::HitExplosion
                        | UnitType::LandExplosion
                        | UnitType::BuildingExplosion
                        | UnitType::AirExplosion
                        | UnitType::SeaExplosion
                )
            });

        if explosion.is_some() {
            break;
        }
    }

    let explosion = explosion.expect("the killed unit spawns an explosion effect");
    let dead = scenario
        .universe
        .snapshot()
        .unit(scenario.mover)
        .cloned()
        .expect("the corpse stays until the blast clears it");

    assert_eq!(dead.grid, halt_cell, "the unit died on the halt cell");
    assert_eq!(
        explosion.grid, halt_cell,
        "`StartExplosion` uses the dying unit's cell, not `move_to`"
    );
    assert_ne!(
        explosion.grid, scenario.destination,
        "the destination of the interrupted route is not a position source"
    );
    assert_eq!(
        (explosion.x, explosion.y),
        (
            (halt_cell.x as i32 * 64 + 32) as u16,
            (halt_cell.y as i32 * 64 + 32) as u16
        ),
        "`OffsetDrawZones(unit->x - explosion->x, ...)`: the blast is drawn at the \
         dying unit's pixel, which for a stopped unit is the halt cell center"
    );
}
