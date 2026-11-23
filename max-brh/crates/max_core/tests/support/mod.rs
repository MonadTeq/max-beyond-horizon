//! Shared helpers for the phase 2 and phase 5 integration scenarios
//! (`phase2_*.rs`, `phase5_*.rs`; the AI harness lives in [`phase5`]).
//!
//! Every scenario has the same shape: a deterministic [`GameUniverse`] (fixed
//! teams, seeded RNG), units placed by [`spawn`], orders issued through the
//! facade API, ticks through [`GameUniverse::tick`], then assertions on the
//! resulting state and the canonical digests
//! ([`max_core::crc::state_checksum`] / [`GameUniverse::snapshot`]).
//!
//! Stage B §4.1: the scenario harness drives the facade; `GameState` itself is
//! reached only for the setup fields the facade config layer does not expose yet
//! (`active_turn_team`, `ai.all_visible`).

#![allow(dead_code)]

pub mod phase5;

use max_core::enums::{GameStateType, PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::universe::GameUniverse;
use max_core::UnitId;

/// Two human teams (RED/GREEN), all units visible, turn-based mode.
pub fn game() -> GameUniverse {
    let mut universe = GameUniverse::default();
    universe.set_network_game(false);
    universe.set_game_state(GameStateType::InGame);
    universe.set_play_mode(PlayMode::TurnBased);
    universe.state_mut().active_turn_team = TeamId::RED;
    universe.team_mut(TeamId::RED).expect("RED slot").team_type = TeamKind::Player;
    universe
        .team_mut(TeamId::GREEN)
        .expect("GREEN slot")
        .team_type = TeamKind::Player;
    *universe.team_units_mut(TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(TeamClan::Random);
    *universe
        .team_units_mut(TeamId::GREEN)
        .expect("GREEN economy") = TeamUnits::with_clan(TeamClan::Random);
    // Scenario units are placed next to each other; visibility is an explicit
    // input fact, not a scan result (`GameManager_AllVisible`).
    universe.state_mut().ai.all_visible = true;
    universe
}

/// The same game with fog of war (`GameManager_AllVisible = false`).
pub fn fog_game() -> GameUniverse {
    let mut universe = game();
    universe.state_mut().ai.all_visible = false;
    universe
}

/// Spawns a unit with the type flags from `SC_UNITS`.
pub fn spawn(
    universe: &mut GameUniverse,
    unit_type: UnitType,
    team: TeamId,
    grid: Point,
) -> UnitId {
    universe
        .spawn_unit(unit_type, team, grid, 0)
        .unwrap_or_else(|| panic!("spawn {unit_type:?} for {team:?} at {grid:?}"))
}

/// Issues `MOVE` to `target` (order data + order/state pair).
pub fn order_move(universe: &mut GameUniverse, id: UnitId, target: Point) {
    universe.set_order_data(
        id,
        max_core::game::OrderData {
            move_to: target,
            fire_on: Point::ZERO,
        },
    );
    universe.set_order(id, UnitOrder::Move, UnitOrderState::Init);
}

/// Issues `FIRE` at `target` (order data + order/state pair).
pub fn order_fire(universe: &mut GameUniverse, id: UnitId, target: Point) {
    universe.set_order_data(
        id,
        max_core::game::OrderData {
            move_to: target,
            fire_on: target,
        },
    );
    universe.set_order(id, UnitOrder::Fire, UnitOrderState::Init);
}

/// Runs up to `limit` ticks until `done` returns `true`; returns whether it did.
pub fn run_until(
    universe: &mut GameUniverse,
    limit: usize,
    mut done: impl FnMut(&GameUniverse) -> bool,
) -> bool {
    for _ in 0..limit {
        if done(universe) {
            return true;
        }
        universe.tick();
    }
    done(universe)
}

/// Runs exactly `count` ticks.
pub fn ticks(universe: &mut GameUniverse, count: usize) {
    for _ in 0..count {
        universe.tick();
    }
}

/// Start of a team's turn (`GameManager_ManageEconomy`): restores speed/shots and
/// updates production. Scenarios that move farther than one speed allowance call
/// it between turn bursts, as the turn machine does.
pub fn start_turn(universe: &mut GameUniverse, team: TeamId) {
    universe.manage_economy(team);
}

/// Runs up to `turns` team turns of `ticks_per_turn` ticks, calling
/// [`start_turn`] before each; stops as soon as `done` is satisfied.
pub fn run_turns(
    universe: &mut GameUniverse,
    team: TeamId,
    turns: usize,
    ticks_per_turn: usize,
    mut done: impl FnMut(&GameUniverse) -> bool,
) -> bool {
    for _ in 0..turns {
        if done(universe) {
            return true;
        }
        start_turn(universe, team);
        for _ in 0..ticks_per_turn {
            universe.tick();
            if done(universe) {
                return true;
            }
        }
    }
    done(universe)
}

/// Marks a unit as flying (`HOVERING`), as after a successful takeoff.
pub fn set_hovering(universe: &mut GameUniverse, id: UnitId, hovering: bool) {
    if let Some(unit) = universe.unit_mut(id) {
        if hovering {
            unit.flags |= max_core::enums::unit_flags::HOVERING;
        } else {
            unit.flags &= !max_core::enums::unit_flags::HOVERING;
        }
    }
}
