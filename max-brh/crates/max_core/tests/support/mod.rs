//! Shared helpers for the phase 2 and phase 5 integration scenarios
//! (`phase2_*.rs`, `phase5_*.rs`; the AI harness lives in [`phase5`]).
//!
//! Every scenario has the same shape: a deterministic [`GameState`] (fixed
//! teams, seeded RNG), units placed by [`spawn`], orders issued through the
//! public state API, ticks, then assertions on the resulting state and the
//! canonical digests (`crc::state_checksum` / `snapshot_bytes`).

#![allow(dead_code)]

pub mod phase5;

use max_core::enums::{GameStateType, PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::game::GameState;
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::UnitId;

/// Two human teams (RED/GREEN), all units visible, turn-based mode.
pub fn game() -> GameState {
    let mut state = GameState::new();
    state.network_game = false;
    state.game_state = GameStateType::InGame;
    state.play_mode = PlayMode::TurnBased;
    state.active_turn_team = TeamId::RED;
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);
    // Scenario units are placed next to each other; visibility is an explicit
    // input fact, not a scan result (`GameManager_AllVisible`).
    state.ai.all_visible = true;
    state
}

/// The same game with fog of war (`GameManager_AllVisible = false`).
pub fn fog_game() -> GameState {
    let mut state = game();
    state.ai.all_visible = false;
    state
}

/// Spawns a unit with the type flags from `SC_UNITS`.
pub fn spawn(state: &mut GameState, unit_type: UnitType, team: TeamId, grid: Point) -> UnitId {
    state
        .spawn_unit(unit_type, team, grid, 0)
        .unwrap_or_else(|| panic!("spawn {unit_type:?} for {team:?} at {grid:?}"))
}

/// Issues `MOVE` to `target` (order data + order/state pair).
pub fn order_move(state: &mut GameState, id: UnitId, target: Point) {
    state.set_order_data(
        id,
        max_core::game::OrderData {
            move_to: target,
            fire_on: Point::ZERO,
        },
    );
    state.set_order(id, UnitOrder::Move, UnitOrderState::Init);
}

/// Issues `FIRE` at `target` (order data + order/state pair).
pub fn order_fire(state: &mut GameState, id: UnitId, target: Point) {
    state.set_order_data(
        id,
        max_core::game::OrderData {
            move_to: target,
            fire_on: target,
        },
    );
    state.set_order(id, UnitOrder::Fire, UnitOrderState::Init);
}

/// Runs up to `limit` ticks until `done` returns `true`; returns whether it did.
pub fn run_until(
    state: &mut GameState,
    limit: usize,
    mut done: impl FnMut(&GameState) -> bool,
) -> bool {
    for _ in 0..limit {
        if done(state) {
            return true;
        }
        state.tick();
    }
    done(state)
}

/// Runs exactly `count` ticks.
pub fn ticks(state: &mut GameState, count: usize) {
    for _ in 0..count {
        state.tick();
    }
}

/// Start of a team's turn (`GameManager_ManageEconomy`): restores speed/shots and
/// updates production. Scenarios that move farther than one speed allowance call
/// it between turn bursts, as the turn machine does.
pub fn start_turn(state: &mut GameState, team: TeamId) {
    state.manage_economy(team);
}

/// Runs up to `turns` team turns of `ticks_per_turn` ticks, calling
/// [`start_turn`] before each; stops as soon as `done` is satisfied.
pub fn run_turns(
    state: &mut GameState,
    team: TeamId,
    turns: usize,
    ticks_per_turn: usize,
    mut done: impl FnMut(&GameState) -> bool,
) -> bool {
    for _ in 0..turns {
        if done(state) {
            return true;
        }
        start_turn(state, team);
        for _ in 0..ticks_per_turn {
            state.tick();
            if done(state) {
                return true;
            }
        }
    }
    done(state)
}

/// Marks a unit as flying (`HOVERING`), as after a successful takeoff.
pub fn set_hovering(state: &mut GameState, id: UnitId, hovering: bool) {
    if let Some(unit) = state.unit_mut(id) {
        if hovering {
            unit.flags |= max_core::enums::unit_flags::HOVERING;
        } else {
            unit.flags &= !max_core::enums::unit_flags::HOVERING;
        }
    }
}
