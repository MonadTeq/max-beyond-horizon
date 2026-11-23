//! Phase 5 — AI strategy selection and weight tables (areas A.2 and A.1).
//!
//! Scenarios: an AI base + a passive enemy base → rounds through the core turn
//! machine (`begin_round`/`next_team`/`end_round`) → the strategy the AI picked,
//! the clan mapping and the 18 weight tables it will plan production from.
//!
//! Oracles: `aiplayer.cpp` — `SelectStrategy` (`:3765-3990`), the clan mapping in
//! `AiPlayer::Init` (`:2285`), `UpdateWeightTables` (`:99-158`),
//! `RebuildWeightTables` (`:78-97`), `GetFilteredWeightTable` (`:3387`).

mod support;

use max_core::ai::build_orders;
use max_core::ai::weight::WeightTable;
use max_core::ai::AiPlayer;
use max_core::enums::unit_flags;
use max_core::enums::{AiStrategy, Opponent, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::Randomizer;
use support::phase5::{
    ai_state, green_base, land_map, play_rounds, red_base, strategy_state, ROUND_TICKS,
};

/// The clan → strategy mapping of `AiPlayer::init_tasks` (`aiplayer.cpp:2280`):
/// `AI_STRATEGY_RANDOM` is resolved by the team clan on the first turn.
#[test]
fn clan_mapping_selects_the_strategy_on_the_first_turn() {
    let cases = [
        (TeamClan::TheChosen, AiStrategy::Air),
        (TeamClan::CrimsonPath, AiStrategy::Sea),
        (TeamClan::AyersHand, AiStrategy::Missiles),
        (TeamClan::SacredEight, AiStrategy::FastAttack),
        (TeamClan::SevenKnights, AiStrategy::Defensive),
        (TeamClan::VonGriffin, AiStrategy::CombinedArms),
        (TeamClan::Musashi, AiStrategy::CombinedArms),
        (TeamClan::AxisInc, AiStrategy::CombinedArms),
    ];

    for (clan, expected) in cases {
        let mut state = ai_state(land_map(40), clan, Opponent::Average);
        assert_eq!(
            state.state().ai.players[TeamId::RED.0 as usize].strategy,
            AiStrategy::Random,
            "{clan:?}: the strategy starts unresolved"
        );

        red_base(&mut state, Point::new(8, 8), 500);
        green_base(&mut state, Point::new(32, 32));

        let (played, over) = play_rounds(&mut state, 1, ROUND_TICKS);
        assert_eq!(played, 1);
        assert!(!over, "{clan:?}: a base with an army is not eliminated");

        assert_eq!(
            state.state().ai.players[TeamId::RED.0 as usize].strategy,
            expected,
            "{clan:?}: `AiPlayer::init_tasks` must resolve RANDOM by the clan"
        );
    }
}

/// `menu_team_strategy_setting` forces a strategy only when its score is
/// non-zero (`aiplayer.cpp:3884-3907`); otherwise C++ falls back to the roll.
#[test]
fn forced_strategy_wins_only_when_its_score_is_non_zero() {
    // `Defensive` scores `count` (one continent) → the force is accepted.
    let mut forced = ai_state(land_map(40), TeamClan::Musashi, Opponent::Average);
    forced.state_mut().ai.settings.team_strategy[0] = Some(AiStrategy::Defensive);
    assert!(
        max_core::ai_setup_strategy(forced.state_mut(), TeamId::RED),
        "the map has a viable continent"
    );
    assert_eq!(
        forced.state().ai.players[TeamId::RED.0 as usize].strategy,
        AiStrategy::Defensive
    );

    // The following turn must not override the forced strategy.
    red_base(&mut forced, Point::new(8, 8), 500);
    green_base(&mut forced, Point::new(32, 32));
    let (_, over) = play_rounds(&mut forced, 1, ROUND_TICKS);
    assert!(!over);
    assert_eq!(
        forced.state().ai.players[TeamId::RED.0 as usize].strategy,
        AiStrategy::Defensive,
        "the forced strategy survives the turn"
    );
}

/// `Sea` scores `isolated_continents * 2`; on a single island its score is 0,
/// so the forced value is ignored and the score-weighted roll runs.
#[test]
fn forced_strategy_with_a_zero_score_falls_back_to_the_roll() {
    let mut state = ai_state(land_map(40), TeamClan::Musashi, Opponent::Average);
    state.state_mut().ai.settings.team_strategy[0] = Some(AiStrategy::Sea);

    assert!(max_core::ai_setup_strategy(state.state_mut(), TeamId::RED));

    let strategy = state.state().ai.players[TeamId::RED.0 as usize].strategy;
    assert_ne!(
        strategy,
        AiStrategy::Sea,
        "SEA has no isolated continent on this map and cannot be rolled"
    );
    assert_ne!(strategy, AiStrategy::Random, "the roll always resolves");
}

/// `SelectStrategy` consumes the RNG for the strategy choice: the same seed
/// gives the same choice and the same RNG tail, and a different seed starts a
/// different stream.
#[test]
fn strategy_selection_is_seeded_and_reproducible() {
    let mut first = ai_state(land_map(40), TeamClan::Musashi, Opponent::Average);
    let mut second = ai_state(land_map(40), TeamClan::Musashi, Opponent::Average);

    let before = first.state().rng.state();
    assert!(max_core::ai_setup_strategy(first.state_mut(), TeamId::RED));
    assert!(max_core::ai_setup_strategy(second.state_mut(), TeamId::RED));

    let after = first.state().rng.state();
    assert_ne!(
        before, after,
        "SelectStrategy must roll the strategy on the RNG"
    );
    assert_eq!(
        after,
        second.state().rng.state(),
        "same seed → same RNG path"
    );
    assert_eq!(
        first.state().ai.players[TeamId::RED.0 as usize].strategy,
        second.state().ai.players[TeamId::RED.0 as usize].strategy
    );

    let mut other_seed = ai_state(land_map(40), TeamClan::Musashi, Opponent::Average);
    other_seed.state_mut().rng = Randomizer::new(1);
    assert!(max_core::ai_setup_strategy(
        other_seed.state_mut(),
        TeamId::RED
    ));
    assert_ne!(
        other_seed.state().rng.state(),
        after,
        "a different seed starts a different RNG stream"
    );
}

/// All 18 `AiPlayer` weight tables in C++ order (`aiplayer.hpp:93-110`).
fn weight_tables(player: &AiPlayer) -> [(&'static str, &WeightTable); 18] {
    [
        ("ground_defense", &player.weight_table_ground_defense),
        ("scout", &player.weight_table_scout),
        ("tanks", &player.weight_table_tanks),
        ("assault_guns", &player.weight_table_assault_guns),
        ("missile_launcher", &player.weight_table_missile_launcher),
        ("air_defense", &player.weight_table_air_defense),
        ("aircrafts", &player.weight_table_aircrafts),
        ("bomber", &player.weight_table_bomber),
        ("table_9", &player.weight_table_9),
        ("fastboat", &player.weight_table_fastboat),
        ("corvette", &player.weight_table_corvette),
        ("submarine", &player.weight_table_submarine),
        ("battleships", &player.weight_table_battleships),
        ("missileboat", &player.weight_table_missileboat),
        ("support_ships", &player.weight_table_support_ships),
        ("commando", &player.weight_table_commando),
        ("infantry", &player.weight_table_infantry),
        ("generic", &player.weight_table_generic),
    ]
}

/// `UpdateWeightTables` (`aiplayer.cpp:99-158`): every table is filled, and the
/// strategy multiplies exactly its own unit types by its factor across all 18
/// tables (`RebuildWeightTables`), leaving the other entries untouched.
#[test]
fn weight_tables_are_filled_and_rebuilt_for_the_strategy() {
    let cases: &[(AiStrategy, &[(UnitType, i32)])] = &[
        (AiStrategy::TankHorde, &[(UnitType::Tank, 10)]),
        (
            AiStrategy::Defensive,
            &[
                (UnitType::GunTurret, 3),
                (UnitType::MissileLauncher, 3),
                (UnitType::AntiAir, 3),
            ],
        ),
        (
            AiStrategy::Missiles,
            &[(UnitType::MissileCrawler, 5), (UnitType::MissileCruiser, 5)],
        ),
        (
            AiStrategy::Air,
            &[(UnitType::Fighter, 5), (UnitType::Bomber, 5)],
        ),
        (
            AiStrategy::Sea,
            &[
                (UnitType::Escort, 2),
                (UnitType::Corvette, 2),
                (UnitType::Submarine, 2),
                (UnitType::Gunboat, 2),
                (UnitType::MissileCruiser, 2),
            ],
        ),
        (
            AiStrategy::CombinedArms,
            &[(UnitType::Tank, 5), (UnitType::MissileCrawler, 5)],
        ),
        (
            AiStrategy::FastAttack,
            &[
                (UnitType::AssaultGun, 3),
                (UnitType::Bomber, 2),
                (UnitType::MobileAntiAir, 2),
                (UnitType::Corvette, 2),
                (UnitType::Escort, 2),
            ],
        ),
    ];

    for (strategy, rebuilt) in cases {
        let mut state = strategy_state(*strategy, Opponent::Average);
        max_core::ai_begin_turn(state.state_mut(), TeamId::RED);

        let player = &state.state().ai.players[TeamId::RED.0 as usize];
        assert_eq!(player.strategy, *strategy);

        let mut baseline = AiPlayer::new(TeamId::RED);
        baseline.init_weight_tables(Opponent::Average as u8);

        for ((name, after), (_, base)) in weight_tables(player)
            .iter()
            .zip(weight_tables(&baseline).iter())
        {
            assert!(
                !after.is_empty(),
                "{strategy:?}: weight table {name} must be filled after BeginTurn"
            );

            for entry in base.entries() {
                let Some(unit_type) = entry.unit_type else {
                    continue;
                };
                let factor = rebuilt
                    .iter()
                    .find(|(rebuilt_type, _)| *rebuilt_type == unit_type)
                    .map(|(_, factor)| *factor)
                    .unwrap_or(1);
                let expected = i32::from(entry.weight) * factor;
                assert_eq!(
                    after.get_weight(unit_type),
                    expected,
                    "{strategy:?}: {name} weight of {unit_type:?} after RebuildWeightTables"
                );
            }
        }
    }
}

/// `GetFilteredWeightTable` (`aiplayer.cpp:3387`, flags `0x01`) drops the
/// stationary types and leaves a non-empty mobile pool for the factories.
#[test]
fn filtered_weight_table_drops_stationary_types() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    max_core::ai_begin_turn(state.state_mut(), TeamId::RED);

    let player = &state.state().ai.players[TeamId::RED.0 as usize];
    let filtered = build_orders::get_filtered_weight_table(player, state.state(), None, 0x01);

    let mut live = 0;
    for entry in filtered.entries() {
        if entry.weight == 0 {
            continue;
        }
        live += 1;
        let unit_type = entry.unit_type.expect("weight entry has a type");
        assert_eq!(
            max_core::access::unit_type_flags(unit_type) & unit_flags::STATIONARY,
            0,
            "stationary type {unit_type:?} survived flags 0x01"
        );
    }

    assert!(
        live > 0,
        "the filtered table must have at least one mobile type"
    );
}

/// The AI state (strategy + weight tables) is part of the canonical snapshot:
/// a change of strategy changes the digest even without ticks.
#[test]
fn strategy_and_weight_tables_enter_the_state_digest() {
    let mut first = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    max_core::ai_begin_turn(first.state_mut(), TeamId::RED);
    let baseline = first.snapshot().snapshot_bytes();

    let mut second = first.clone();
    second.state_mut().ai.players[TeamId::RED.0 as usize].strategy = AiStrategy::Air;
    second.state_mut().ai.players[TeamId::RED.0 as usize]
        .weight_table_tanks
        .clear();

    assert_ne!(baseline, second.snapshot().snapshot_bytes());
}
