//! P7 end-to-end tests: CRC16, a mini C++ ⇄ Rust replay, victory/defeat on
//! real V70 saves and loading `GameUniverse` from V70.
//!
//! Oracles: `max-cpp-port/src/{crc16.c,game_manager.cpp,winloss.cpp}`,
//! `assets/PATCHES.RES` (`SC_T*`/`SC_S*`/`SC_C*` — mission scripts),
//! `assets/PATCHES.RES` (`SC_L0002` — the generic script).
//!
//! Vectors: `max-brh/tools/sim_vectors/vectors.txt` (C++ generator, ubuntu:24.04)
//! and `max-brh/tools/map_vectors/vectors.txt` (P2: `.wrl` input + map parameters).

use max_core::crc;
use max_core::enums::{FileType, TeamClan, UnitOrderState, UnitType, WinLossState};
use max_core::geo::Point;
use max_core::save::SaveFile;
use max_core::setup::{
    derive_surface_map, generate_map, AlienUnitCosts, GameSetup, GenerationSettings, ResourceLevel,
    TeamStart,
};
use max_core::team::{TeamId, TeamInfo, TeamKind, TeamUnits};
use max_core::unit::UnitListKind;
use max_core::universe::GameUniverse;
use max_core::victory::MissionCategory;
use max_core::{Randomizer, INITIAL_UNIT_ID_COUNTER};

const SIM_VECTORS: &str = include_str!("../../../tools/sim_vectors/vectors.txt");
const MAP_VECTORS: &str = include_str!("../../../tools/map_vectors/vectors.txt");
const SAVE1_CAMPAIGN: &[u8] = include_bytes!("data/save1_v70.cam");
const SAVE18_SCENARIO: &[u8] = include_bytes!("data/save18_v70.sce");

fn hex_to_bytes(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    assert_eq!(bytes.len() % 2, 0, "hex string has odd length");

    (0..bytes.len() / 2)
        .map(|index| {
            let pair = std::str::from_utf8(&bytes[index * 2..index * 2 + 2]).expect("ascii hex");
            u8::from_str_radix(pair, 16).expect("valid hex byte")
        })
        .collect()
}

fn hex_to_u16(text: &str) -> Vec<u16> {
    hex_to_bytes(text)
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

// ---------------------------------------------------------------------------
// Parsing a P2 vector (`.wrl` input + generation parameters)
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct MapVectorCase {
    planet: u8,
    levels: [u8; 4],
    seed: u32,
    starts: Vec<(u8, i16, i16)>,
    pass_table: Vec<u8>,
    tile_ids: Vec<u16>,
    alien_costs: [i32; 4],
}

fn map_vector_case(text: &str, wanted: &str) -> MapVectorCase {
    let mut case = MapVectorCase::default();
    let mut active = false;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let key = parts.next().expect("record key");

        if key == "case" {
            active = parts.next() == Some(wanted);
            continue;
        }

        if !active {
            continue;
        }

        match key {
            "planet" => case.planet = parts.next().expect("planet").parse().expect("planet"),
            "levels" => {
                for (slot, value) in case.levels.iter_mut().zip(parts) {
                    *slot = value.parse().expect("level");
                }
            }
            "seed" => case.seed = parts.next().expect("seed").parse().expect("seed"),
            "start" => {
                case.starts.push((
                    parts.next().expect("team").parse().expect("team"),
                    parts.next().expect("x").parse().expect("x"),
                    parts.next().expect("y").parse().expect("y"),
                ));
            }
            "pass_table" => case.pass_table = hex_to_bytes(parts.next().expect("pass table")),
            "tile_ids" => case.tile_ids = hex_to_u16(parts.next().expect("tile ids")),
            "alien_costs" => {
                for (slot, value) in case.alien_costs.iter_mut().zip(parts) {
                    *slot = value.parse().expect("alien cost");
                }
            }
            _ => {}
        }
    }

    assert!(!case.tile_ids.is_empty(), "map vector case not found");
    case
}

// ---------------------------------------------------------------------------
// Parsing the replay section of sim_vectors/vectors.txt
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ReplayVector {
    case: String,
    levels: [u8; 4],
    seed: u32,
    starts: Vec<(u8, i16, i16)>,
    alien_costs: [i32; 4],
    deploys: Vec<(u16, u8, i16, i16)>,
    ticks: u32,
    checksum: u16,
    digest: Vec<u8>,
}

fn replay_vector() -> ReplayVector {
    let mut case = String::new();
    let mut levels = [0u8; 4];
    let mut seed = 0u32;
    let mut starts = Vec::new();
    let mut alien_costs = [0i32; 4];
    let mut deploys = Vec::new();
    let mut ticks = 0u32;
    let mut checksum = 0u16;
    let mut digest = Vec::new();

    for line in SIM_VECTORS.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        match parts.next().expect("record key") {
            "replay_case" => case = parts.next().expect("case name").to_owned(),
            "replay_levels" => {
                for (slot, value) in levels.iter_mut().zip(parts) {
                    *slot = value.parse().expect("level");
                }
            }
            "replay_seed" => seed = parts.next().expect("seed").parse().expect("seed"),
            "replay_start" => starts.push((
                parts.next().expect("team").parse().expect("team"),
                parts.next().expect("x").parse().expect("x"),
                parts.next().expect("y").parse().expect("y"),
            )),
            "replay_alien_costs" => {
                for (slot, value) in alien_costs.iter_mut().zip(parts) {
                    *slot = value.parse().expect("alien cost");
                }
            }
            "replay_deploy" => deploys.push((
                parts.next().expect("unit type").parse().expect("unit type"),
                parts.next().expect("team").parse().expect("team"),
                parts.next().expect("x").parse().expect("x"),
                parts.next().expect("y").parse().expect("y"),
            )),
            "replay_ticks" => ticks = parts.next().expect("ticks").parse().expect("ticks"),
            "replay_checksum" => {
                checksum =
                    u16::from_str_radix(parts.next().expect("checksum"), 16).expect("checksum")
            }
            "replay_digest" => digest = hex_to_bytes(parts.next().expect("digest")),
            _ => {}
        }
    }

    assert!(!case.is_empty(), "replay section missing");
    assert!(!digest.is_empty(), "replay digest missing");

    ReplayVector {
        case,
        levels,
        seed,
        starts,
        alien_costs,
        deploys,
        ticks,
        checksum,
        digest,
    }
}

fn level(value: u8) -> ResourceLevel {
    ResourceLevel::new(value).expect("menu resource level 0..=2")
}

/// Assembles the state exactly as the C++ generator does:
/// the map with P2 parameters, two units via `GameUniverse::spawn_unit`, N ticks.
fn build_replay_state(replay: &ReplayVector) -> GameUniverse {
    let case = map_vector_case(MAP_VECTORS, &replay.case);
    assert_eq!(case.seed, replay.seed);
    assert_eq!(case.levels, replay.levels);

    let surface = derive_surface_map(&case.tile_ids, &case.pass_table);
    let mut setup = GameSetup {
        planet: max_core::enums::Planet::try_from(case.planet).expect("planet"),
        settings: GenerationSettings::from_menu_levels(
            level(replay.levels[0]),
            level(replay.levels[1]),
            level(replay.levels[2]),
            level(replay.levels[3]),
        ),
        teams: replay
            .starts
            .iter()
            .map(|(team, x, y)| TeamStart::new(TeamId(*team), Point::new(*x, *y)))
            .collect(),
        alien_costs: AlienUnitCosts {
            alien_tank: replay.alien_costs[0],
            juggernaut: replay.alien_costs[1],
            alien_assault_gun: replay.alien_costs[2],
            alien_plane: replay.alien_costs[3],
        },
    };

    let mut rng = Randomizer::new(replay.seed);
    let output = generate_map(&mut setup, &surface, &mut rng);

    let mut state = GameUniverse::default();
    *state.map_mut() = output.map;

    // Teams: red and blue are humans, gray/green do not play, ALIEN is a computer.
    let kinds = [
        TeamKind::Player,
        TeamKind::None,
        TeamKind::Player,
        TeamKind::None,
    ];
    for (team, kind) in kinds.iter().enumerate() {
        state.teams_mut()[team] = TeamInfo::new();
        state.teams_mut()[team].team_type = *kind;
        state.team_units_table_mut()[team] = TeamUnits::with_clan(TeamClan::Random);
    }
    state.teams_mut()[4] = TeamInfo::new();
    state.teams_mut()[4].team_type = TeamKind::Computer;

    *state.rng_mut() = rng;
    state.set_unit_id_counter(INITIAL_UNIT_ID_COUNTER);
    state.set_game_state(max_core::enums::GameStateType::InGame);

    // Deployment happens in phase `GAME_STATE_12_DEPLOYING_UNITS`: C++
    // `UnitsManager_DeployUnit` (`units_manager.cpp:2382-2384`) in this phase
    // does **not** zero the storage (the engine expects mission loading), so
    // ENGINEER keeps `ATTRIB_STORAGE` (40) — exactly as in
    // `tools/sim_vectors` (`deploy_unit` writes `values.storage`).
    let deploy_state = state.snapshot().game_state();
    state.set_game_state(max_core::enums::GameStateType::DeployingUnits);

    for (unit_type, team, x, y) in &replay.deploys {
        let unit_type = UnitType::try_from(*unit_type).expect("unit type");
        state
            .spawn_unit(unit_type, TeamId(*team), Point::new(*x, *y), 0)
            .expect("unit deploys");
    }

    state.set_game_state(deploy_state);

    for _ in 0..replay.ticks {
        state.tick();
    }

    state
}

// ---------------------------------------------------------------------------
// (b) The key parity test: 10 C++ ⇄ Rust ticks, digest bit for bit
// ---------------------------------------------------------------------------

#[test]
fn mini_replay_matches_cpp_digest_bit_for_bit() {
    let replay = replay_vector();
    let state = build_replay_state(&replay);
    let digest = state.snapshot().replay_digest_bytes();

    assert_eq!(
        digest,
        replay.digest,
        "replay digest mismatch: rust {} bytes, cpp {} bytes",
        digest.len(),
        replay.digest.len()
    );
    assert_eq!(
        state.snapshot().replay_digest_crc(),
        state.snapshot().replay_digest_crc()
    );

    // Magic and CRC at the end.
    assert_eq!(&digest[..8], crc::REPLAY_DIGEST_MAGIC);
    assert_eq!(
        u16::from_le_bytes([digest[digest.len() - 2], digest[digest.len() - 1]]),
        crc::crc16(&digest[..digest.len() - 2])
    );
}

#[test]
fn mini_replay_is_deterministic_over_two_runs() {
    let replay = replay_vector();
    let first = build_replay_state(&replay);
    let second = build_replay_state(&replay);

    assert_eq!(
        first.snapshot().replay_digest_bytes(),
        second.snapshot().replay_digest_bytes()
    );
    assert_eq!(
        first.snapshot().snapshot_bytes(),
        second.snapshot().snapshot_bytes()
    );
    assert_eq!(
        first.snapshot().state_checksum(),
        second.snapshot().state_checksum()
    );
}

// ---------------------------------------------------------------------------
// (a) CRC16: the network CRC of packet 45
// ---------------------------------------------------------------------------

#[test]
fn network_crc_matches_cpp_vector_bytes() {
    // The `crc_bytes` section comparison is done in the `crc` unit test; here — one more
    // independent run on boundary values and on the replay state.
    assert_eq!(crc::crc16(&[]), crc::CRC16_INITIAL);
    assert_eq!(crc::crc16(&[0x00]), 0xE1F0);

    let replay = replay_vector();
    let state = build_replay_state(&replay);
    let checksum = state.snapshot().state_checksum();

    // `GameManager_CheckDesync` (packet 45) on the same state in C++.
    assert_eq!(checksum, replay.checksum, "packet 45 CRC mismatch");
    assert_eq!(checksum, state.snapshot().state_checksum());
    // There are units in the replay, so the CRC differs from the initial value.
    assert_ne!(checksum, crc::CRC16_INITIAL);
}

// ---------------------------------------------------------------------------
// (c) Victory/defeat on real saves
// ---------------------------------------------------------------------------

fn green_count(state: &GameUniverse, unit_type: UnitType) -> i32 {
    state.snapshot().count_ready_units(TeamId::GREEN, unit_type)
}

fn destroy_all(state: &mut GameUniverse, unit_type: UnitType) {
    let ids: Vec<_> = state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            state
                .snapshot()
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == unit_type)
        })
        .collect();

    for id in ids {
        state.destroy_unit(id);
    }
}

#[test]
fn campaign_1_real_save_evaluates_like_the_script() {
    let save = SaveFile::parse(SAVE1_CAMPAIGN).expect("campaign 1 fixture parses");
    assert_eq!(save.header.save_game_type, FileType::Campaign);
    assert_eq!(save.header.mission_index, 1);

    let mut state = GameUniverse::from_save(&save);
    assert_eq!(state.snapshot().mission_category(), MissionCategory::Campaign);
    assert_eq!(state.snapshot().mission_index(), 1);
    assert_eq!(state.snapshot().player_team(), TeamId::RED);
    assert_eq!(
        state.snapshot().teams()[TeamId::RED.0 as usize].team_type,
        TeamKind::Player
    );

    let research = green_count(&state, UnitType::ResearchCentre);
    let greenhouses = green_count(&state, UnitType::EcoSphere);

    // `SC_C0001`: RED — LOST when GREEN has 0 research centers, WON at 0
    // greenhouses (and a live research center), otherwise PENDING.
    let expected = if research == 0 {
        WinLossState::Lost
    } else if greenhouses > 0 {
        WinLossState::Pending
    } else {
        WinLossState::Won
    };

    let status = state.snapshot().evaluate_status();
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        expected,
        "campaign 1: research={research}, greenhouses={greenhouses}"
    );
    assert_eq!(status.mission_type, MissionCategory::Campaign as i32);

    // Destroy all green research centers → red loses.
    destroy_all(&mut state, UnitType::ResearchCentre);
    assert_eq!(green_count(&state, UnitType::ResearchCentre), 0);
    let status = state.snapshot().evaluate_status();
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Lost
    );
}

#[test]
fn mission_18_real_save_wins_when_green_research_centers_are_gone() {
    let save = SaveFile::parse(SAVE18_SCENARIO).expect("scenario 18 fixture parses");
    assert_eq!(save.header.save_game_type, FileType::Scenario);
    assert_eq!(save.header.mission_index, 18);

    let mut state = GameUniverse::from_save(&save);
    assert_eq!(state.snapshot().mission_category(), MissionCategory::Scenario);
    assert_eq!(state.snapshot().mission_index(), 18);

    // While green has research centers — the mission continues.
    assert!(green_count(&state, UnitType::ResearchCentre) > 0);
    let status = state.snapshot().evaluate_status();
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Pending
    );

    destroy_all(&mut state, UnitType::ResearchCentre);
    assert_eq!(green_count(&state, UnitType::ResearchCentre), 0);

    let status = state.snapshot().evaluate_status();
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Won
    );
}

// ---------------------------------------------------------------------------
// (d) Loading SAVE1.CAM → 5 ticks: determinism of two runs, no panic
// ---------------------------------------------------------------------------

#[test]
fn real_save_loads_normalized_and_ticks_deterministically() {
    let save = SaveFile::parse(SAVE1_CAMPAIGN).expect("fixture parses");
    let mut first = GameUniverse::from_save(&save);
    let mut second = GameUniverse::from_save(&save);

    // --- normalizations of `UnitInfo::FileLoad` / `SaveLoad_LoadFormatV70` ---
    assert_eq!(first.snapshot().unit_id_counter(), INITIAL_UNIT_ID_COUNTER);
    assert!(first.snapshot().unit_count() > 0, "fixture has units");

    for id in first.snapshot().unit_ids_in_order() {
        let unit = first.snapshot().unit(id).expect("unit exists");
        assert!(unit.build_rate >= 1, "build_rate normalized");
        assert_eq!(
            unit.ai_state_bits & (0x100 | 0x200),
            0,
            "AI remind bits cleared"
        );
        assert!(!matches!(
            unit.state,
            UnitOrderState::NewOrder
                | UnitOrderState::MoveGettingPath
                | UnitOrderState::IssuingPath
                | UnitOrderState::InTransition
                | UnitOrderState::InProgress
        ));
        assert_eq!(unit.movement_wait_count, 0);
    }

    // Every unit from the file got into the cell map (otherwise `Access_*` does not see it).
    for id in first.snapshot().unit_ids_in_order() {
        let grid = first.snapshot().unit(id).expect("unit").grid;
        assert!(
            first.snapshot().units_at(grid).contains(&id),
            "unit {id:?} missing from map hash at {grid:?}"
        );
    }

    // --- 5 ticks, two independent runs ---
    for _ in 0..5 {
        first.tick();
        second.tick();
    }

    assert_eq!(
        first.snapshot().snapshot_bytes(),
        second.snapshot().snapshot_bytes()
    );
    assert_eq!(
        first.snapshot().replay_digest_bytes(),
        second.snapshot().replay_digest_bytes()
    );
    assert_eq!(
        first.snapshot().state_checksum(),
        second.snapshot().state_checksum()
    );
}

#[test]
fn loaded_units_resolve_parent_and_enemy_references() {
    // The three real fixtures have no `parent_unit`/`enemy_unit` references (these are
    // early saves without transports and attacks), so the reference resolution path
    // is checked on a modified copy of the real save: the object numbers of
    // two units are linked, and the loader must return their `UnitId`.
    let mut save = SaveFile::parse(SAVE1_CAMPAIGN).expect("fixture parses");
    assert!(save.units.len() >= 2, "fixture has units");

    let parent_object_index = save.units[1].object_index;
    let parent_id = save.units[1].info.id;
    let enemy_object_index = save.units[0].object_index;
    let enemy_id = save.units[0].info.id;

    save.units[0].parent_object_index = parent_object_index;
    save.units[0].enemy_object_index = enemy_object_index;

    let state = GameUniverse::from_save(&save);
    let unit = state
        .snapshot()
        .unit(save.units[0].info.id)
        .expect("loaded unit exists");

    assert_eq!(unit.parent_unit, Some(parent_id));
    assert_eq!(unit.enemy_unit, Some(enemy_id));

    // References absent from the file (object number 0) remain empty.
    let other = state
        .snapshot()
        .unit(save.units[1].info.id)
        .expect("loaded unit exists");
    assert_eq!(other.parent_unit, None);
    assert_eq!(other.enemy_unit, None);
}

#[test]
fn loaded_lists_and_team_units_match_the_file() {
    let save = SaveFile::parse(SAVE1_CAMPAIGN).expect("fixture parses");
    let state = GameUniverse::from_save(&save);

    for kind in UnitListKind::ALL {
        let expected: Vec<_> = save
            .unit_list(kind)
            .iter()
            .filter_map(|object_index| {
                save.unit_by_object_index(*object_index)
                    .map(|saved| saved.info.id)
            })
            .collect();

        assert_eq!(state.snapshot().unit_list(kind), expected, "list {kind:?}");
    }

    let team_units = save.team_units();
    for team in 0..4 {
        assert_eq!(state.snapshot().team_units()[team].gold, team_units[team].gold);
        assert_eq!(
            state.snapshot().team_units()[team].complexes().len(),
            team_units[team].complexes().len()
        );
    }
}
