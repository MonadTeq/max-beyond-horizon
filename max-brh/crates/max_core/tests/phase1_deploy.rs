//! Phase 1 integration: landing the starting armies (`deploy.rs`).
//!
//! Oracle: `max-cpp-port/src/game_manager.cpp`
//! (`GameManager_ProcessTeamMissionSupplyUnits`, `GameManager_InitGame`),
//! `units_manager.cpp` (`UnitsManager_SetInitialMining`,
//! `UnitsManager_DeployUnit`) and `assets/PATCHES.RES` (`SC_L0001`).
//!
//! Scenario shape: menu setup → `generate_map` → `GameState` → landing →
//! turns/ticks → expected units, economy and determinism (D).

use max_core::crc;
use max_core::deploy::{deploy_starting_units, DEFAULT_MISSION_LOADOUT, DEFAULT_START_GOLD};
use max_core::enums::{GameStateType, Surface, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::events;
use max_core::game::{GameState, PLAYER_TEAM_COUNT};
use max_core::geo::{Point, MAP_HEIGHT, MAP_TILES, MAP_WIDTH};
use max_core::map::{Map, ResourceKind, ResourceMap, ResourceMapEntry, SurfaceMap};
use max_core::setup::{
    generate_map, AlienUnitCosts, GameSetup, GenerationSettings, ResourceLevel, TeamStart,
};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::Randomizer;

const SEED: u32 = 0x00DE_9101;
const UNITS_PER_TEAM: usize = 4 + DEFAULT_MISSION_LOADOUT.len();

fn alien_costs() -> AlienUnitCosts {
    AlienUnitCosts {
        alien_tank: 4,
        juggernaut: 8,
        alien_assault_gun: 5,
        alien_plane: 6,
    }
}

fn custom_setup(teams: Vec<TeamStart>) -> GameSetup {
    GameSetup {
        planet: max_core::enums::Planet::Snowcrab,
        settings: GenerationSettings::from_menu_levels(
            ResourceLevel::NORMAL,
            ResourceLevel::NORMAL,
            ResourceLevel::NORMAL,
            ResourceLevel::LOW,
        ),
        teams,
        alien_costs: alien_costs(),
    }
}

/// A new custom game: generated map + two human teams + landing
/// (`GameManager_InitGame`).
fn new_game() -> GameState {
    let surface = SurfaceMap::from_tiles([Surface::LAND; MAP_TILES]);
    let mut setup = custom_setup(vec![
        TeamStart::new(TeamId::RED, Point::new(20, 20)),
        TeamStart::new(TeamId::GREEN, Point::new(90, 20)),
    ]);

    let mut rng = Randomizer::new(SEED);
    let output = generate_map(&mut setup, &surface, &mut rng);

    let mut state = GameState::with_setup(
        output.map,
        [
            TeamKind::Player,
            TeamKind::Player,
            TeamKind::None,
            TeamKind::None,
        ],
    );
    state.network_game = false;
    state.rng = Randomizer::new(SEED);
    // `GameManager_InitGame`: the first turn is number 1.
    state.turn_counter = 1;

    let deployed = deploy_starting_units(
        &mut state,
        DEFAULT_START_GOLD,
        setup.settings.max_resources,
        &setup.teams,
    );
    assert_eq!(
        deployed,
        2 * UNITS_PER_TEAM,
        "each team lands a base plus the SC_L0001 loadout"
    );

    state
}

/// One round of team turns for the local harness (the switching layer lives in
/// `max_app`; `max_core` owns the phase and the counters).
fn play_round(state: &mut GameState, ticks_per_team: usize) {
    for team_index in 0..PLAYER_TEAM_COUNT {
        let kind = state.teams[team_index].team_type;
        if kind == TeamKind::None || kind == TeamKind::Eliminated {
            continue;
        }

        let team = TeamId(team_index as u8);
        state.teams[team_index].finished_turn = false;
        state.manage_economy(team);
        state.active_turn_team = team;
        state.game_state = GameStateType::InGame;

        for _ in 0..ticks_per_team {
            state.tick();
        }

        state.enqueue(events::command_finished_turn(team));
        state.tick();
    }

    assert!(
        !state.finish_turn(),
        "a peaceful round must not end the game"
    );
}

/// `setup → deploy → 3 rounds → ticks` is bit-identical across two independent
/// runs: units, economy and the canonical digests (D).
#[test]
fn setup_deploy_and_turn_ticks_are_bit_identical() {
    fn run() -> GameState {
        let mut state = new_game();
        for _ in 0..3 {
            play_round(&mut state, 3);
        }
        state
    }

    let first = run();
    let second = run();

    assert_eq!(first.unit_ids_in_order(), second.unit_ids_in_order());
    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(crc::state_checksum(&first), crc::state_checksum(&second));
    assert_eq!(crc::state_digest(&first), crc::state_digest(&second));
    assert_eq!(
        crc::replay_digest_bytes(&first),
        crc::replay_digest_bytes(&second)
    );
    assert_eq!(first.turn_counter, 4);
    assert_eq!(first.unit_count(), 2 * UNITS_PER_TEAM);
}

/// The landed base keeps both teams alive: the generic win/loss script reports
/// `GENERIC`, the mine has mining assigned and the credits are the start gold.
#[test]
fn deployed_base_keeps_both_teams_alive_after_a_turn() {
    let mut state = new_game();

    // Straight after landing (`GameManager_ProcessTeamMissionSupplyUnits`).
    for team in [TeamId::RED, TeamId::GREEN] {
        assert_eq!(
            state.team_units[team.0 as usize].gold, DEFAULT_START_GOLD,
            "team {team:?}"
        );

        let mine = state
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.unit(id))
            .find(|unit| unit.team == team && unit.unit_type == UnitType::MiningStation)
            .expect("a mining station per team");

        assert_eq!(mine.storage, 0, "team {team:?}: the mine lands empty");
        assert_eq!(mine.orders, UnitOrder::PowerOn);
        assert_eq!(mine.state, UnitOrderState::Init);
        assert!(
            mine.raw_mining_max > 0,
            "team {team:?}: the generator optimized the start cell"
        );
        assert!(mine.total_mining > 0);
    }

    play_round(&mut state, 2);

    for team in [TeamId::RED, TeamId::GREEN] {
        assert_eq!(
            max_core::victory::default_win_loss_conditions(&state, team),
            max_core::enums::WinLossState::Generic,
            "team {team:?}"
        );
        assert_eq!(
            state.team_units[team.0 as usize].gold, DEFAULT_START_GOLD,
            "team {team:?}"
        );
    }

    assert!(!max_core::victory::check_end_game_conditions(&mut state));
    assert_eq!(state.unit_count(), 2 * UNITS_PER_TEAM);
}

/// Landing on a map without any resources still places the full base (the
/// mine has zero mining) and does not leave the map bounds.
#[test]
fn landing_on_a_barren_map_keeps_every_unit_inside_the_map() {
    let mut state = GameState::new();
    for team in [TeamId::RED, TeamId::GREEN] {
        state.teams[team.0 as usize].team_type = TeamKind::Player;
        state.team_units[team.0 as usize] = TeamUnits::with_clan(TeamClan::Random);
    }
    state.network_game = false;

    let teams = [
        TeamStart::new(TeamId::RED, Point::new(20, 20)),
        TeamStart::new(TeamId::GREEN, Point::new(80, 20)),
    ];
    let deployed = deploy_starting_units(&mut state, DEFAULT_START_GOLD, 20, &teams);

    assert_eq!(deployed, 2 * UNITS_PER_TEAM);
    assert_eq!(state.unit_count(), 2 * UNITS_PER_TEAM);

    for id in state.unit_ids_in_order() {
        let unit = state.unit(id).expect("unit");
        assert!(
            unit.grid.x >= 0
                && unit.grid.y >= 0
                && i32::from(unit.grid.x) < i32::from(MAP_WIDTH)
                && i32::from(unit.grid.y) < i32::from(MAP_HEIGHT),
            "{:?} is outside the map: {:?}",
            unit.unit_type,
            unit.grid
        );
    }

    let mine = state
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.unit(id))
        .find(|unit| unit.unit_type == UnitType::MiningStation)
        .expect("mining station");
    assert_eq!(mine.total_mining, 0, "nothing to mine on a barren map");
    assert_eq!(mine.raw_mining_max, 0);
}

/// Landing at the first valid map cell (1, 1) stays inside the map: the
/// generator search and the loadout placement are clamped by the map access
/// rules.
#[test]
fn landing_at_the_map_corner_stays_inside_the_bounds() {
    let mut resources = ResourceMap::new();
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let kind = if x % 2 == 0 {
                ResourceKind::Raw
            } else {
                ResourceKind::Fuel
            };
            resources.set(x, y, ResourceMapEntry::new(16, Some(kind), 0));
        }
    }
    let map = Map::from_parts(
        SurfaceMap::from_tiles([Surface::LAND; MAP_TILES]),
        resources,
    );

    let mut state = GameState::with_setup(
        map,
        [
            TeamKind::Player,
            TeamKind::None,
            TeamKind::None,
            TeamKind::None,
        ],
    );
    state.network_game = false;

    let teams = [TeamStart::new(TeamId::RED, Point::new(1, 1))];
    let deployed = deploy_starting_units(&mut state, DEFAULT_START_GOLD, 20, &teams);

    assert_eq!(deployed, UNITS_PER_TEAM);
    for id in state.unit_ids_in_order() {
        let unit = state.unit(id).expect("unit");
        assert!(
            unit.grid.x >= 0 && unit.grid.y >= 0,
            "{:?} has a negative cell: {:?}",
            unit.unit_type,
            unit.grid
        );
        assert!(
            i32::from(unit.grid.x) < i32::from(MAP_WIDTH)
                && i32::from(unit.grid.y) < i32::from(MAP_HEIGHT),
            "{:?} is outside the map: {:?}",
            unit.unit_type,
            unit.grid
        );
        // Landed units are waiting for orders, as in `UnitsManager_DeployUnit`.
        assert!(
            unit.orders == UnitOrder::Await || unit.orders == UnitOrder::PowerOn,
            "{:?} kept {:?}/{:?}",
            unit.unit_type,
            unit.orders,
            unit.state
        );
        assert!(matches!(
            unit.state,
            UnitOrderState::Init | UnitOrderState::ExecutingOrder
        ));
    }
}
