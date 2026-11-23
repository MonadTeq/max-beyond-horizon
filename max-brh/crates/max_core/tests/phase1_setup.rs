//! Phase 1 integration: match setup (`setup.rs`).
//!
//! Oracle: `max-cpp-port/src/game_manager.cpp` (`GameManager_InitGame`,
//! `GameManager_PopulateMapWithResources`), `resource_allocator.cpp`,
//! `menu.cpp` (`menu_update_resource_levels`) and `world.cpp` (pass table).
//!
//! Reference vectors from the C++ generator (`tools/map_vectors/vectors.txt`)
//! are covered by the unit tests in `setup.rs`; here the scenarios are
//! "menu settings + planet + teams → `generate_map` → expected map/positions",
//! plus the determinism (D) of the whole generation.

use max_core::crc;
use max_core::enums::{Planet, Surface};
use max_core::geo::{MAP_HEIGHT, MAP_TILES, MAP_WIDTH};
use max_core::map::{ResourceKind, SurfaceMap};
use max_core::setup::{
    generate_map, is_valid_starting_position, AlienUnitCosts, GameSetup, GenerationSettings,
    ResourceLevel, TeamStart,
};
use max_core::team::TeamId;
use max_core::Randomizer;

const SEED: u32 = 0x5EED_1234;

/// Flat land, as the `.wrl` loader would produce for a land planet.
fn flat_land() -> SurfaceMap {
    SurfaceMap::from_tiles([Surface::LAND; MAP_TILES])
}

fn alien_costs() -> AlienUnitCosts {
    AlienUnitCosts {
        alien_tank: 4,
        juggernaut: 8,
        alien_assault_gun: 5,
        alien_plane: 6,
    }
}

/// A custom game on the default menu levels (1/1/1, derelicts off), as
/// `settings.cpp` writes them.
fn custom_setup(teams: Vec<TeamStart>) -> GameSetup {
    GameSetup {
        planet: Planet::Snowcrab,
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

/// `generate_map` is a pure function of `(setup, surface, seed)`: the same seed
/// reproduces the map, the resource map and the RNG state byte for byte (D).
#[test]
fn generated_map_is_deterministic_for_the_same_seed() {
    let surface = flat_land();
    let teams = vec![
        TeamStart::new(TeamId::RED, max_core::geo::Point::new(20, 20)),
        TeamStart::new(TeamId::GREEN, max_core::geo::Point::new(90, 20)),
    ];

    let mut first_setup = custom_setup(teams.clone());
    let mut first_rng = Randomizer::new(SEED);
    let first = generate_map(&mut first_setup, &surface, &mut first_rng);

    let mut second_setup = custom_setup(teams);
    let mut second_rng = Randomizer::new(SEED);
    let second = generate_map(&mut second_setup, &surface, &mut second_rng);

    assert_eq!(first, second, "the whole generation result must match");
    assert_eq!(first_rng.state(), second_rng.state());
    assert_eq!(
        crc::crc16(&first.map.resources.to_le_bytes()),
        crc::crc16(&second.map.resources.to_le_bytes())
    );
    assert_eq!(
        first.map.surface.tiles(),
        surface.tiles(),
        "generation keeps the input surface"
    );
    assert!(first.aliens.is_empty(), "derelicts are disabled");
    assert_eq!(first_setup.teams, second_setup.teams);
}

/// Another seed must change the resource map and the RNG stream
/// (`Randomizer_SetSeed(Remote_RngSeed)` at the start of generation).
#[test]
fn another_seed_changes_the_resource_map() {
    let surface = flat_land();
    let teams = vec![TeamStart::new(
        TeamId::RED,
        max_core::geo::Point::new(20, 20),
    )];

    let mut first_setup = custom_setup(teams.clone());
    let mut first_rng = Randomizer::new(SEED);
    let first = generate_map(&mut first_setup, &surface, &mut first_rng);

    let mut other_setup = custom_setup(teams);
    let mut other_rng = Randomizer::new(SEED.wrapping_add(1));
    let other = generate_map(&mut other_setup, &surface, &mut other_rng);

    assert_ne!(
        first.map.resources.tiles(),
        other.map.resources.tiles(),
        "the seed must drive the resource map"
    );
    assert_ne!(first_rng.state(), other_rng.state());
}

/// `GameManager_InitGame` with four playing teams (MAX): every requested
/// position stays a valid 4×4 land square and teams stay in `RED..GRAY` order.
#[test]
fn four_team_setup_keeps_valid_starting_positions() {
    let surface = flat_land();
    let requested = [
        (TeamId::RED, max_core::geo::Point::new(10, 10)),
        (TeamId::GREEN, max_core::geo::Point::new(100, 10)),
        (TeamId::BLUE, max_core::geo::Point::new(10, 100)),
        (TeamId::GRAY, max_core::geo::Point::new(100, 100)),
    ];
    let mut setup = custom_setup(
        requested
            .iter()
            .map(|&(team, point)| TeamStart::new(team, point))
            .collect(),
    );

    let mut rng = Randomizer::new(SEED);
    let output = generate_map(&mut setup, &surface, &mut rng);

    assert_eq!(setup.teams.len(), 4);
    for (index, start) in setup.teams.iter().enumerate() {
        assert_eq!(start.team, requested[index].0, "team order is preserved");
        assert_eq!(
            start.position, requested[index].1,
            "a valid position must not be moved"
        );
        assert!(
            is_valid_starting_position(
                &surface,
                i32::from(start.position.x),
                i32::from(start.position.y)
            ),
            "team {:?} ends on an invalid start",
            start.team
        );
        assert!(start.position.x > 0 && start.position.y > 0);
        assert!(i32::from(start.position.x) < i32::from(MAP_WIDTH));
        assert!(i32::from(start.position.y) < i32::from(MAP_HEIGHT));
    }

    // Resource generation always fills all three resource types.
    let counts = output.map.resources.counts();
    assert!(counts.raw_tiles > 0);
    assert!(counts.fuel_tiles > 0);
    assert!(counts.gold_tiles > 0);
}

/// All three resource levels (0/1/MAX of the menu) produce a complete map with
/// all resource kinds; only the amounts differ (`menu_update_resource_levels`).
#[test]
fn every_resource_level_generates_all_resource_kinds() {
    for level in [
        ResourceLevel::LOW,
        ResourceLevel::NORMAL,
        ResourceLevel::HIGH,
    ] {
        let surface = flat_land();
        let mut setup = GameSetup {
            planet: Planet::Frigia,
            settings: GenerationSettings::from_menu_levels(level, level, level, ResourceLevel::LOW),
            teams: vec![TeamStart::new(
                TeamId::RED,
                max_core::geo::Point::new(50, 50),
            )],
            alien_costs: alien_costs(),
        };
        let mut rng = Randomizer::new(SEED);
        let output = generate_map(&mut setup, &surface, &mut rng);

        let counts = output.map.resources.counts();
        assert!(counts.raw_tiles > 0, "level {level:?}: no materials");
        assert!(counts.fuel_tiles > 0, "level {level:?}: no fuel");
        assert!(counts.gold_tiles > 0, "level {level:?}: no gold");

        for entry in output.map.resources.tiles() {
            assert_eq!(
                entry.visibility(),
                0,
                "generation must not reveal resources"
            );
            let amount = u16::from(entry.amount());
            let kind = entry.resource_type();
            assert!(
                amount == 0 || kind.is_some_and(|kind| ResourceKind::ALL.contains(&kind)),
                "amount without a valid resource kind: {entry:?}"
            );
        }
    }
}
