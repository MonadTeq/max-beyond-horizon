use super::*;
use crate::enums::{Surface, TeamClan};
use crate::map::{ResourceKind, ResourceMapEntry};
use crate::team::TeamUnits;

/// `max_resources` for levels 1/1/1 (see `GenerationSettings::from_menu_levels`).
const MAX_RESOURCES: i32 = 20;

/// A land map with resources sufficient for `GetStartPositionMiningStation`:
/// every 2×2 square has 16 materials and 16 fuel.
fn land_state(teams: &[TeamId]) -> GameState {
    let mut state = GameState::new();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            state.map.surface.set(x, y, Surface::LAND);

            let kind = if x % 2 == 0 {
                ResourceKind::Raw
            } else {
                ResourceKind::Fuel
            };
            let entry = ResourceMapEntry::new(16, Some(kind), 0);
            state.map.resources.set(x, y, entry);
        }
    }

    for &team in teams {
        let index = team.0 as usize;
        state.teams[index].team_type = TeamKind::Player;
        state.team_units[index] = TeamUnits::with_clan(TeamClan::Random);
    }

    state
}

fn unit_types(state: &GameState, team: TeamId) -> Vec<UnitType> {
    state
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.unit(id))
        .filter(|unit| unit.team == team)
        .map(|unit| unit.unit_type)
        .collect()
}

#[test]
fn default_loadout_matches_custom_game_script() {
    assert_eq!(DEFAULT_MISSION_LOADOUT.len(), 3);
    assert_eq!(DEFAULT_MISSION_LOADOUT[0].unit_type, UnitType::Constructor);
    assert_eq!(DEFAULT_MISSION_LOADOUT[0].cargo, 40);
    assert_eq!(DEFAULT_MISSION_LOADOUT[1].unit_type, UnitType::Engineer);
    assert_eq!(DEFAULT_MISSION_LOADOUT[1].cargo, 20);
    assert_eq!(DEFAULT_MISSION_LOADOUT[2].unit_type, UnitType::Surveyor);
    assert_eq!(DEFAULT_MISSION_LOADOUT[2].cargo, 0);
}

#[test]
fn survey_sees_both_resource_types() {
    let state = land_state(&[]);
    let (raw, gold, fuel) = survey_resources_in_area(&state, Point::new(10, 10), 1);

    assert_eq!(raw, 32);
    assert_eq!(gold, 0);
    assert_eq!(fuel, 32);
}

#[test]
fn deploy_team_builds_base_and_loadout() {
    let mut state = land_state(&[TeamId::RED]);
    let start = Point::new(20, 20);
    let deployed = deploy_team_supplies(
        &mut state,
        TeamId::RED,
        200,
        &DEFAULT_MISSION_LOADOUT,
        start,
        MAX_RESOURCES,
    );

    assert_eq!(deployed.len(), 4 + DEFAULT_MISSION_LOADOUT.len());

    let types = unit_types(&state, TeamId::RED);
    for expected in [
        UnitType::MiningStation,
        UnitType::LargeSlab,
        UnitType::PowerGenerator,
        UnitType::SmallSlab,
        UnitType::Constructor,
        UnitType::Engineer,
        UnitType::Surveyor,
    ] {
        assert_eq!(
            types
                .iter()
                .filter(|&&unit_type| unit_type == expected)
                .count(),
            1,
                "нет юнита {expected:?} в {types:?}"
        );
    }

    assert_eq!(state.team_units[TeamId::RED.0 as usize].gold, 200);

    let mining_station = deployed[0];
    let unit = state.unit(mining_station).expect("шахта");
    assert_eq!(unit.storage, 0);
    assert!(unit.raw_mining_max > 0, "шахте нужны материалы: {unit:?}");
    assert_eq!(unit.orders, UnitOrder::PowerOn);
    assert_eq!(unit.state, UnitOrderState::Init);

    let generator = deployed[2];
    let unit = state.unit(generator).expect("генератор");
    assert_eq!(unit.orders, UnitOrder::PowerOn);
    assert_eq!(unit.state, UnitOrderState::Init);

    // Loadout: storage = cargo, orders as in the C++ constructor.
    for (index, supply) in DEFAULT_MISSION_LOADOUT.iter().enumerate() {
        let unit = state.unit(deployed[4 + index]).expect("юнит загрузки");
        assert_eq!(unit.unit_type, supply.unit_type);
        assert_eq!(unit.storage, supply.cargo as i16);
        assert_eq!(unit.orders, UnitOrder::Await);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    }

    let info = &state.teams[TeamId::RED.0 as usize];
    assert_eq!(info.stats_units_built, DEFAULT_MISSION_LOADOUT.len() as i16);
    assert_eq!(info.stats_mines_built, 1);
    assert_eq!(info.stats_buildings_built, 1);
}

/// `GameManager_GetStartingPositionPowerGenerator` (`game_manager.cpp:1833`):
/// the generator — on a cell separate from the mine.
///
/// The start point `(20,20)` is valid on this map, so the mine stays in
/// it. The generator search starts from `(mining.x - 1, mining.y + 2)` and
/// goes north (`DIRECTION_OFFSETS[0]`), the first available candidate is
/// `(19,21)`. `SMLSLAB` (the slab under the generator) is placed there too — on the same
/// cell, as in C++. The "generator overlaps the mine" regression is caught
/// by comparing cells.
#[test]
fn power_generator_starts_next_to_the_mining_station_but_not_on_it() {
    let mut state = land_state(&[TeamId::RED]);
    let deployed = deploy_team_supplies(
        &mut state,
        TeamId::RED,
        200,
        &DEFAULT_MISSION_LOADOUT,
        Point::new(20, 20),
        MAX_RESOURCES,
    );

    assert_eq!(deployed.len(), 4 + DEFAULT_MISSION_LOADOUT.len());

    let mining_station = state.unit(deployed[0]).expect("шахта").grid;
    let power_generator = state.unit(deployed[2]).expect("генератор").grid;
    let small_slab = state.unit(deployed[3]).expect("плита").grid;

    assert_eq!(mining_station, Point::new(20, 20));
    assert_eq!(
        power_generator,
        Point::new(mining_station.x - 1, mining_station.y + 1),
            "C++ GetStartingPositionPowerGenerator: point.x -= 1; point.y += 2; \
             первый шаг на север"
    );
    assert_ne!(
        power_generator, mining_station,
            "генератор не должен занимать клетку шахты"
    );
    assert_eq!(
        small_slab, power_generator,
            "SMLSLAB ставится в клетку генератора (как C++ DeployUnit)"
    );

    // The cells do not coincide even by the 2×2 footprint of the `BUILDING` (mine).
    assert!(
        power_generator.x < mining_station.x && power_generator.y > mining_station.y,
            "генератор слева-снизу от шахты: {power_generator:?} vs {mining_station:?}"
    );
}

#[test]
fn deployment_consumes_two_rng_draws_per_team() {
    // Two teams → exactly two slab frames per team.
    const TEAMS: usize = 2;

    let mut expected_rng = crate::rng::Randomizer::new(0);
    let expected: Vec<i32> = (0..TEAMS)
        .map(|_| {
            let a = expected_rng.generate_i32(LARGE_SLAB_IMAGE_COUNT);
            let b = expected_rng.generate_i32(SMALL_SLAB_IMAGE_COUNT);
            a + b
        })
        .collect();

    let mut state = land_state(&[TeamId::RED, TeamId::BLUE]);
    let teams = [
        TeamStart::new(TeamId::RED, Point::new(20, 20)),
        TeamStart::new(TeamId::BLUE, Point::new(70, 70)),
    ];
    deploy_starting_units(&mut state, 150, MAX_RESOURCES, &teams);

    let mut rng = crate::rng::Randomizer::new(0);
    let actual: Vec<i32> = (0..TEAMS)
        .map(|_| {
            let a = rng.generate_i32(LARGE_SLAB_IMAGE_COUNT);
            let b = rng.generate_i32(SMALL_SLAB_IMAGE_COUNT);
            a + b
        })
        .collect();

    assert_eq!(actual, expected);
    assert_eq!(rng.state(), state.rng.state(), "поток ГПСЧ должен совпасть");
}

#[test]
fn deployment_is_deterministic() {
    let teams = [TeamStart::new(TeamId::RED, Point::new(20, 20))];

    let mut first = land_state(&[TeamId::RED]);
    deploy_starting_units(&mut first, 150, MAX_RESOURCES, &teams);

    let mut second = land_state(&[TeamId::RED]);
    deploy_starting_units(&mut second, 150, MAX_RESOURCES, &teams);

    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(first.unit_count(), second.unit_count());
    assert_eq!(first.rng.state(), second.rng.state());
}

#[test]
fn deployment_keeps_the_game_alive_after_turns() {
    let mut state = land_state(&[TeamId::RED, TeamId::GREEN]);
    let teams = [
        TeamStart::new(TeamId::RED, Point::new(20, 20)),
        TeamStart::new(TeamId::GREEN, Point::new(80, 20)),
    ];
    let deployed = deploy_starting_units(&mut state, 150, MAX_RESOURCES, &teams);

    assert!(deployed > 0);
    assert!(!crate::victory::check_end_game_conditions(&mut state));
    assert_eq!(
        crate::victory::default_win_loss_conditions(&state, TeamId::RED),
        crate::enums::WinLossState::Generic
    );
    assert_eq!(
        crate::victory::default_win_loss_conditions(&state, TeamId::GREEN),
        crate::enums::WinLossState::Generic
    );

    for turn in 0..3 {
        state.game_state = GameStateType::EndTurn;

        assert!(
            !state.finish_turn(),
                "партия не должна завершаться на ходу {turn}"
        );
        assert_eq!(state.turn_counter, turn + 1);
    }
}

// -----------------------------------------------------------------------
// Phase 1: deployment boundaries (U)
// -----------------------------------------------------------------------

/// `Survey_GetTotalResourcesInArea`: radius 0 sees exactly one cell, a
/// negative radius sees none, map edges are clipped and the sum is clamped
/// to 255 (`SURVEY_RESOURCE_LIMIT`).
#[test]
fn survey_resources_in_area_respects_radius_edges_and_limit() {
    let state = land_state(&[]);

    // Radius 0 — the starting cell only (`even x` is materials).
    assert_eq!(
        survey_resources_in_area(&state, Point::new(10, 10), 0),
        (16, 0, 0)
    );
    assert_eq!(
        survey_resources_in_area(&state, Point::new(11, 10), 0),
        (0, 0, 16)
    );

    // A negative radius yields an empty range (no cells).
    assert_eq!(
        survey_resources_in_area(&state, Point::new(10, 10), -1),
        (0, 0, 0)
    );

    // The map corner is clipped to the two visible cells per axis.
    assert_eq!(
        survey_resources_in_area(&state, Point::new(0, 0), 1),
        (32, 0, 32)
    );

    // Saturation: a radius covering the whole map clamps at 255 per type.
    let mut rich = land_state(&[]);
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            rich.map
                .resources
                .set(x, y, ResourceMapEntry::new(16, Some(ResourceKind::Raw), 0));
        }
    }
    assert_eq!(
        survey_resources_in_area(&rich, Point::new(10, 10), MAP_WIDTH as i32),
        (SURVEY_RESOURCE_LIMIT, 0, 0)
    );
}

/// `GameManager_IsValidStartingPosition`: the 4×4 area must be pure land and
/// the rectangle must stay inside the map (0 and MAX cell boundaries).
#[test]
fn valid_starting_position_rejects_edges_and_mixed_surface() {
    let mut state = GameState::new();

    assert!(is_valid_starting_position(&state, Point::new(1, 1)));
    assert!(is_valid_starting_position(&state, Point::new(109, 109)));
    assert!(!is_valid_starting_position(&state, Point::new(0, 1)));
    assert!(!is_valid_starting_position(&state, Point::new(1, 0)));
    assert!(!is_valid_starting_position(&state, Point::new(110, 1)));
    assert!(!is_valid_starting_position(&state, Point::new(1, 110)));
    assert!(!is_valid_starting_position(&state, Point::new(-1, 5)));

    // A single water cell inside the 4×4 square invalidates the position.
    state.map.surface.set(2, 2, Surface::WATER);
    assert!(!is_valid_starting_position(&state, Point::new(1, 1)));
}

/// `Access_FindReachableSpot` with `range = 0` performs no iteration:
/// it reports failure and leaves the point untouched (boundary 0).
#[test]
fn find_reachable_spot_with_zero_range_is_a_no_op() {
    let state = land_state(&[TeamId::RED]);
    let mut position = Point::new(20, 20);

    assert!(!find_reachable_spot(
        &state,
        UnitType::Constructor,
        TeamId::RED,
        &mut position,
        0,
        0
    ));
    assert_eq!(position, Point::new(20, 20));
}

/// `GameManager_GetStartPositionMiningStation`: a barren starting cell makes
/// the search move to a cell that satisfies the materials/fuel minimums.
#[test]
fn mining_station_search_leaves_a_barren_start_cell() {
    let mut state = land_state(&[TeamId::RED]);

    // Clear a 5×5 area around the requested start.
    for x in 18..=22 {
        for y in 18..=22 {
            state
                .map
                .resources
                .set(x, y, ResourceMapEntry::new(0, None, 0));
        }
    }

    let start = Point::new(20, 20);
    let found = start_position_mining_station(&state, start, MAX_RESOURCES);

    assert_ne!(found, start, "the barren start must be rejected");
    assert!(is_valid_starting_position(&state, found));

    let max_resources = MAX_RESOURCES;
    let minimum_fuel = (max_resources * 8 + 10) / 20;
    let minimum_materials = (max_resources * 12 + 10) / 20;
    let (raw, _gold, fuel) = survey_resources_in_area(&state, found, SURVEY_RADIUS);

    assert!(raw >= minimum_materials, "raw={raw} at {found:?}");
    assert!(fuel >= minimum_fuel, "fuel={fuel} at {found:?}");
}

/// `UnitsManager_SetInitialMining` fills the 16-unit storage: fuel for the
/// generator first, then materials, then the rest. A rich cell saturates the
/// capacity (MAX), a barren one yields zeros (0).
#[test]
fn set_initial_mining_caps_at_the_sixteen_unit_capacity() {
    let mut state = land_state(&[TeamId::RED]);
    let id = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(20, 20), 0)
        .expect("mining station");

    set_initial_mining(&mut state, id, Point::new(20, 20));

    let unit = state.unit(id).expect("mining station");
    // 2×2 neighborhood: 32 materials and 32 fuel; the generator consumes 2
    // fuel, so the 16 units of capacity are 2 fuel + 14 materials.
    assert_eq!(unit.fuel_mining, 2);
    assert_eq!(unit.raw_mining, 14);
    assert_eq!(unit.gold_mining, 0);
    assert_eq!(unit.total_mining, 16, "storage capacity is saturated");
    assert_eq!(unit.raw_mining_max, 32);
    assert_eq!(unit.fuel_mining_max, 32);
    assert_eq!(unit.gold_mining_max, 0);

    // Barren cell: nothing to mine, capacity stays unused.
    let mut barren = land_state(&[TeamId::RED]);
    for x in 0..MAP_WIDTH {
        for y in 0..MAP_HEIGHT {
            barren
                .map
                .resources
                .set(x, y, ResourceMapEntry::new(0, None, 0));
        }
    }
    let id = barren
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(20, 20), 0)
        .expect("mining station");
    set_initial_mining(&mut barren, id, Point::new(20, 20));

    let unit = barren.unit(id).expect("mining station");
    assert_eq!(unit.total_mining, 0);
    assert_eq!(unit.raw_mining_max, 0);
    assert_eq!(unit.fuel_mining_max, 0);
}

/// `UnitsManager_DeployUnit` (`units_manager.cpp:2403-2410`, `2452-2454`):
/// **every** new mine — including one built mid-game by a builder — starts
/// `ORDER_POWER_ON` and pre-allocated by `UnitsManager_SetInitialMining`.
///
/// The spawn path is checked directly (no explicit `set_initial_mining` call):
/// a mine that joins a complex through connectors must already be ON and
/// mining, otherwise it stays at 0/0/0 forever (`spawn_unit` used to skip the
/// C++ call, which only the landing flow repeated by hand).
#[test]
fn spawned_mining_station_is_powered_and_pre_allocated() {
    let mut state = land_state(&[TeamId::RED]);
    let id = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(20, 20), 0)
        .expect("mining station");

    let unit = state.unit(id).expect("mining station");
    assert_eq!(unit.orders, UnitOrder::PowerOn);
    assert_eq!(unit.state, UnitOrderState::Init);
    assert!(unit.complex.is_some(), "the mine joins a complex");
    // Rich 2×2 cell: 2 fuel for the generator first, then all remaining
    // capacity to raw materials (32 available), then extra fuel, then gold.
    assert_eq!(
        (
            unit.raw_mining,
            unit.fuel_mining,
            unit.gold_mining,
            unit.total_mining
        ),
        (14, 2, 0, 16)
    );
    let complex = unit.complex;

    // A storage built next to the mine connects the two into one complex;
    // the connection must not reset the mine's ON status or allocation.
    let dump = state
        .spawn_unit(
            UnitType::MaterialStorage,
            TeamId::RED,
            Point::new(22, 20),
            0,
        )
        .expect("material storage");
    assert_eq!(state.unit(dump).expect("dump").complex, complex);
    let unit = state.unit(id).expect("mining station");
    assert_eq!(unit.orders, UnitOrder::PowerOn);
    assert_eq!(
        (
            unit.raw_mining,
            unit.fuel_mining,
            unit.gold_mining,
            unit.total_mining
        ),
        (14, 2, 0, 16),
            "connecting to the complex keeps the default split"
    );
}

/// Priority of the default split (`units_manager.cpp:1496-1510`): the
/// generator's fuel share, then **raw materials to the cell maximum**, then
/// the remaining capacity to fuel, then gold.
#[test]
fn spawned_mining_station_prioritizes_raw_then_fuel_then_gold() {
    let mut state = land_state(&[TeamId::RED]);

    // Only the 2×2 footprint is surveyed (`SURVEY_RADIUS = 1`): 3 raw,
    // 4 fuel and 16 gold available.
    for x in 20..=21 {
        for y in 20..=21 {
            state
                .map
                .resources
                .set(x, y, ResourceMapEntry::new(0, None, 0));
        }
    }
    state
        .map
        .resources
        .set(20, 20, ResourceMapEntry::new(3, Some(ResourceKind::Raw), 0));
    state.map.resources.set(
        20,
        21,
        ResourceMapEntry::new(4, Some(ResourceKind::Fuel), 0),
    );
    state.map.resources.set(
        21,
        20,
        ResourceMapEntry::new(16, Some(ResourceKind::Gold), 0),
    );

    let id = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(20, 20), 0)
        .expect("mining station");

    let unit = state.unit(id).expect("mining station");
    assert_eq!(
        (
            unit.raw_mining_max,
            unit.fuel_mining_max,
            unit.gold_mining_max
        ),
        (3, 4, 16)
    );
    // fuel baseline 2, raw 3 (all of the cell), extra fuel 2, gold 9.
    assert_eq!(
        (unit.raw_mining, unit.fuel_mining, unit.gold_mining),
        (3, 4, 9)
    );
    assert_eq!(unit.total_mining, 16);
}

/// The derelict `ALIEN` slot is not a playing team: deployment rejects it
/// without touching the state or the RNG stream (boundary slot).
#[test]
fn deploy_ignores_the_derelict_team_slot() {
    let mut state = land_state(&[TeamId::RED]);
    let rng_before = state.rng.state();
    let deployed = deploy_team_supplies(
        &mut state,
        TeamId::ALIEN,
        200,
        &DEFAULT_MISSION_LOADOUT,
        Point::new(20, 20),
        MAX_RESOURCES,
    );

    assert!(deployed.is_empty());
    assert_eq!(state.unit_count(), 0);
    assert_eq!(state.rng.state(), rng_before, "no RNG draws for ALIEN");
    assert_eq!(state.game_state, GameStateType::InGame);
}

/// Teams whose `team_type` is `None` are skipped by the landing loop
/// (`GameManager_InitGame`: only `team_type != TEAM_TYPE_NONE`).
#[test]
fn deploy_skips_teams_that_do_not_play() {
    let mut state = GameState::new();
    for team in 0..PLAYER_TEAM_COUNT {
        state.teams[team].team_type = TeamKind::None;
    }
    let rng_before = state.rng.state();
    let teams = [TeamStart::new(TeamId::RED, Point::new(20, 20))];

    let deployed = deploy_starting_units(&mut state, 150, MAX_RESOURCES, &teams);

    assert_eq!(deployed, 0);
    assert_eq!(state.unit_count(), 0);
    assert_eq!(state.rng.state(), rng_before);
}
