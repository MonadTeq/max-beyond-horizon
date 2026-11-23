//! Phase 5 — determinism of the AI integration scenarios (D layer).
//!
//! Every scenario of the AI port is run **twice from scratch** and the two
//! states must be bit-identical: `snapshot_bytes` (the full simulation + AI
//! state), packet 45 `state_checksum`, `replay_digest_crc` and `state_digest`.
//! Each scenario also pins its three digests — the regression lock documented in
//! `max-brh-docs/analysis/ai-port-status.md` ("AI CRC сценариев"): a change in
//! the AI port (tasks, decisions, RNG path) or in the digest layout fails here.
//!
//! A single seed must be bit-for-bit (`engine-coverage-plan.md` §1 level D); a
//! different seed is not required to take the same decision path.
//!
//! The scenarios are the same base + enemy + N-round setups as the behavioural
//! `phase5_ai_*.rs` files, compressed to the round counts the CRC table uses.

mod support;

use max_core::enums::{AiStrategy, Opponent, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::GameState;
use support::phase5::{
    add_research_lab, add_vehicle_plant, ai_state, count_units, crcs, factory_unit_requests,
    green_base, land_map, play_rounds, red_base, refresh_visibility, sentry, spawn, strategy_state,
    ROUND_TICKS,
};

/// Pinned `(state_checksum, replay_digest_crc, state_digest)` of the strategy
/// scenario (A.2: clan selection + weight tables, 6 rounds).
const STRATEGY_CRCS: (u16, u16, u16) = (0x4C90, 0x26ED, 0xDB7B);
/// Pinned digests of the production scenario (A.3: base + first buildings, 12
/// rounds).
const PRODUCTION_CRCS: (u16, u16, u16) = (0x372E, 0x79A6, 0x2FAE);
/// Pinned digests of the factory scenario (A.1/A.3: spotted enemy tank → heavy
/// plant orders a tank, 4 rounds).
const FACTORY_CRCS: (u16, u16, u16) = (0x1DE6, 0x25CF, 0x9F84);
/// Pinned digests of the combat scenario (A.4: spotted facility attacked, 6
/// rounds).
const COMBAT_CRCS: (u16, u16, u16) = (0x56BC, 0xE313, 0xCE5B);
/// Pinned digests of the research scenario (A.5: upgrades + powered centre, 4
/// rounds).
const RESEARCH_CRCS: (u16, u16, u16) = (0xCC41, 0x0CA2, 0xADC0);
/// Pinned digests of the reconnaissance scenario (A.6: scout exploring, 8
/// rounds).
const RECON_CRCS: (u16, u16, u16) = (0x5387, 0x603B, 0xFD64);

/// A.2: clan `AyersHand` resolves to `Missiles`; the base plays six rounds.
fn strategy_scenario() -> GameState {
    let mut state = ai_state(land_map(40), TeamClan::AyersHand, Opponent::Average);
    red_base(&mut state, Point::new(8, 8), 500);
    green_base(&mut state, Point::new(32, 32));

    let (_, _over) = play_rounds(&mut state, 6, ROUND_TICKS);
    state
}

/// A.3: a bare TankHorde base builds its first buildings; 12 rounds.
fn production_scenario() -> GameState {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    red_base(&mut state, Point::new(8, 8), 500);
    green_base(&mut state, Point::new(32, 32));

    let (_, _over) = play_rounds(&mut state, 12, ROUND_TICKS);
    state
}

/// A.1/A.3: a heavy plant with a stockpile builds a tank because an enemy tank
/// is spotted; 4 rounds.
fn factory_scenario() -> GameState {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    let red = red_base(&mut state, Point::new(8, 8), 500);
    green_base(&mut state, Point::new(32, 32));

    spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(11, 13),
    );
    add_vehicle_plant(&mut state, Point::new(16, 16));

    sentry(&mut state, red.tank);
    refresh_visibility(&mut state);

    let (_, _over) = play_rounds(&mut state, 4, ROUND_TICKS);
    state
}

/// A.4: the base tank spots a key facility and the attack runs; 6 rounds.
fn combat_scenario() -> GameState {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    let red = red_base(&mut state, Point::new(8, 8), 500);

    spawn(
        &mut state,
        UnitType::TrainingHall,
        TeamId::GREEN,
        Point::new(11, 12),
    );
    spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(36, 36),
    );

    sentry(&mut state, red.tank);
    refresh_visibility(&mut state);

    let (_, _over) = play_rounds(&mut state, 6, ROUND_TICKS);
    state
}

/// A.5: the base buys tank upgrades and the powered centre gets the fastest
/// topic; 4 rounds.
fn research_scenario() -> GameState {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    let red = red_base(&mut state, Point::new(8, 8), 800);
    green_base(&mut state, Point::new(32, 32));

    {
        let unit = state.unit_mut(red.constructor).expect("constructor");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
        unit.build_list = vec![UnitType::Tank];
    }

    add_research_lab(&mut state, Point::new(14, 14));

    let (_, _over) = play_rounds(&mut state, 4, ROUND_TICKS);
    state
}

/// A.6: the scout explores the map; 8 rounds.
fn recon_scenario() -> GameState {
    let mut state = strategy_state(AiStrategy::CombinedArms, Opponent::Average);
    red_base(&mut state, Point::new(10, 10), 500);
    green_base(&mut state, Point::new(30, 30));
    spawn(&mut state, UnitType::Scout, TeamId::RED, Point::new(12, 12));

    let (_, _over) = play_rounds(&mut state, 8, ROUND_TICKS);
    state
}

/// Runs the scenario twice and asserts the bit-identical state plus the pinned
/// digests.
fn assert_deterministic(name: &str, run: fn() -> GameState, pinned: (u16, u16, u16)) -> GameState {
    let first = run();
    let second = run();

    assert_eq!(
        first.snapshot_bytes(),
        second.snapshot_bytes(),
        "{name}: two identical runs must produce the same snapshot"
    );
    assert_eq!(
        crcs(&first),
        crcs(&second),
        "{name}: packet 45 / replay digest / state digest must match"
    );
    assert_eq!(
        crcs(&first),
        pinned,
        "{name}: pinned CRC changed; update the table in ai-port-status.md"
    );

    first
}

#[test]
fn strategy_scenario_is_deterministic_and_pinned() {
    let state = assert_deterministic("strategy", strategy_scenario, STRATEGY_CRCS);
    assert_eq!(
        state.ai.players[TeamId::RED.0 as usize].strategy,
        AiStrategy::Missiles
    );
}

#[test]
fn production_scenario_is_deterministic_and_pinned() {
    let state = assert_deterministic("production", production_scenario, PRODUCTION_CRCS);
    assert!(count_units(&state, TeamId::RED, UnitType::MiningStation) >= 1);
}

#[test]
fn factory_scenario_is_deterministic_and_pinned() {
    let state = assert_deterministic("factory", factory_scenario, FACTORY_CRCS);
    let requested = factory_unit_requests(&state, TeamId::RED, UnitType::HeavyVehiclePlant);
    assert!(
        requested.contains(&UnitType::Tank),
        "the factory scenario must keep its tank order, requests: {requested:?}"
    );
}

#[test]
fn combat_scenario_is_deterministic_and_pinned() {
    let state = assert_deterministic("combat", combat_scenario, COMBAT_CRCS);
    assert!(state.ai.players[TeamId::RED.0 as usize].spotted_units.len() <= 2);
}

#[test]
fn research_scenario_is_deterministic_and_pinned() {
    let state = assert_deterministic("research", research_scenario, RESEARCH_CRCS);
    assert!(
        state.teams[TeamId::RED.0 as usize].stats_gold_spent_on_upgrades > 0,
        "the research scenario must have bought upgrades"
    );
}

#[test]
fn recon_scenario_is_deterministic_and_pinned() {
    let state = assert_deterministic("recon", recon_scenario, RECON_CRCS);
    assert!(count_units(&state, TeamId::RED, UnitType::Scout) == 1);
}

/// The scenarios cover different mechanics and must not collapse to the same
/// digest (a sanity check on the CRC table: six distinct rows).
#[test]
fn the_six_scenarios_have_distinct_digests() {
    // One scenario at a time: a `GameState` is large, six on one test-thread
    // stack overflow it.
    let scenarios: [(&str, fn() -> GameState); 6] = [
        ("strategy", strategy_scenario),
        ("production", production_scenario),
        ("factory", factory_scenario),
        ("combat", combat_scenario),
        ("research", research_scenario),
        ("recon", recon_scenario),
    ];

    let mut seen: Vec<(&str, u16)> = Vec::new();
    for (name, run) in scenarios {
        let state = run();
        let (_, replay, _) = crcs(&state);
        drop(state);

        for (other_name, other_replay) in seen.iter() {
            assert_ne!(
                replay, *other_replay,
                "{name} and {other_name} produced the same replay digest"
            );
        }
        seen.push((name, replay));
    }
}

/// A run resumed from a clone continues bit-identically to an uninterrupted
/// run: no AI state lives outside `GameState`.
#[test]
fn a_cloned_run_continues_bit_identically() {
    let mut interrupted = production_scenario_setup();
    let (_, over) = play_rounds(&mut interrupted, 4, ROUND_TICKS);
    assert!(!over);

    let mut resumed = interrupted.clone();

    let (_, over) = play_rounds(&mut interrupted, 8, ROUND_TICKS);
    assert!(!over);
    let (_, over) = play_rounds(&mut resumed, 8, ROUND_TICKS);
    assert!(!over);

    assert_eq!(interrupted.snapshot_bytes(), resumed.snapshot_bytes());
    assert_eq!(crcs(&interrupted), crcs(&resumed));
}

/// The production scenario's opening position (shared by the resume test).
fn production_scenario_setup() -> GameState {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    red_base(&mut state, Point::new(8, 8), 500);
    green_base(&mut state, Point::new(32, 32));
    state
}
