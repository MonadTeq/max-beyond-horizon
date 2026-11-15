//! Сквозные тесты P7: CRC16, мини-реплей C++ ⇄ Rust, победа/поражение на
//! реальных сейвах V70 и загрузка `GameState` из V70.
//!
//! Оракулы: `max-cpp-port/src/{crc16.c,game_manager.cpp,winloss.cpp}`,
//! `assets/PATCHES.RES` (`SC_T*`/`SC_S*`/`SC_C*` — скрипты миссий),
//! `assets/PATCHES.RES` (`SC_L0002` — generic-скрипт).
//!
//! Векторы: `max-rust/tools/sim_vectors/vectors.txt` (C++-генератор, ubuntu:24.04)
//! и `max-rust/tools/map_vectors/vectors.txt` (P2: вход `.wrl` + параметры карты).

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
use max_core::victory::{self, MissionCategory};
use max_core::{GameState, Randomizer, INITIAL_UNIT_ID_COUNTER};

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
// Разбор P2-вектора (вход `.wrl` + параметры генерации)
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
// Разбор replay-секции sim_vectors/vectors.txt
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

/// Собирает состояние ровно так, как его собирает C++-генератор:
/// карта P2-параметрами, два юнита через `GameState::spawn_unit`, N тиков.
fn build_replay_state(replay: &ReplayVector) -> GameState {
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

    let mut state = GameState::new();
    state.map = output.map;

    // Команды: красные и синие — люди, серые/зелёные не играют, ALIEN — компьютер.
    let kinds = [
        TeamKind::Player,
        TeamKind::None,
        TeamKind::Player,
        TeamKind::None,
    ];
    for (team, kind) in kinds.iter().enumerate() {
        state.teams[team] = TeamInfo::new();
        state.teams[team].team_type = *kind;
        state.team_units[team] = TeamUnits::with_clan(TeamClan::Random);
    }
    state.teams[4] = TeamInfo::new();
    state.teams[4].team_type = TeamKind::Computer;

    state.rng = rng;
    state.unit_id_counter = INITIAL_UNIT_ID_COUNTER;
    state.game_state = max_core::enums::GameStateType::InGame;

    for (unit_type, team, x, y) in &replay.deploys {
        let unit_type = UnitType::try_from(*unit_type).expect("unit type");
        state
            .spawn_unit(unit_type, TeamId(*team), Point::new(*x, *y), 0)
            .expect("unit deploys");
    }

    for _ in 0..replay.ticks {
        state.tick();
    }

    state
}

// ---------------------------------------------------------------------------
// (б) Ключевой тест паритета: 10 тиков C++ ⇄ Rust, дайджест бит-в-бит
// ---------------------------------------------------------------------------

#[test]
fn mini_replay_matches_cpp_digest_bit_for_bit() {
    let replay = replay_vector();
    let state = build_replay_state(&replay);
    let digest = crc::replay_digest_bytes(&state);

    assert_eq!(
        digest,
        replay.digest,
        "replay digest mismatch: rust {} bytes, cpp {} bytes",
        digest.len(),
        replay.digest.len()
    );
    assert_eq!(
        crc::replay_digest_crc(&state),
        crc::replay_digest_crc(&state.clone())
    );

    // Магия и CRC в конце.
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
        crc::replay_digest_bytes(&first),
        crc::replay_digest_bytes(&second)
    );
    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(crc::state_checksum(&first), crc::state_checksum(&second));
}

// ---------------------------------------------------------------------------
// (а) CRC16: сетевой CRC пакета 45
// ---------------------------------------------------------------------------

#[test]
fn network_crc_matches_cpp_vector_bytes() {
    // Сверка `crc_bytes`-секции выполняется в юнит-тесте `crc`; здесь — ещё
    // один независимый прогон на граничных значениях и на состоянии реплея.
    assert_eq!(crc::crc16(&[]), crc::CRC16_INITIAL);
    assert_eq!(crc::crc16(&[0x00]), 0xE1F0);

    let replay = replay_vector();
    let state = build_replay_state(&replay);
    let checksum = crc::state_checksum(&state);

    // `GameManager_CheckDesync` (пакет 45) на том же состоянии в C++.
    assert_eq!(checksum, replay.checksum, "packet 45 CRC mismatch");
    assert_eq!(checksum, crc::state_checksum(&state.clone()));
    // Юниты в реплее есть, поэтому CRC отличается от начального значения.
    assert_ne!(checksum, crc::CRC16_INITIAL);
}

// ---------------------------------------------------------------------------
// (в) Победа/поражение на реальных сейвах
// ---------------------------------------------------------------------------

fn green_count(state: &GameState, unit_type: UnitType) -> i32 {
    victory::count_ready_units(state, TeamId::GREEN, unit_type)
}

fn destroy_all(state: &mut GameState, unit_type: UnitType) {
    let ids: Vec<_> = state
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            state
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

    let mut state = GameState::from_save(&save);
    assert_eq!(state.mission_category, MissionCategory::Campaign);
    assert_eq!(state.mission_index, 1);
    assert_eq!(state.player_team, TeamId::RED);
    assert_eq!(
        state.teams[TeamId::RED.0 as usize].team_type,
        TeamKind::Player
    );

    let research = green_count(&state, UnitType::ResearchCenter);
    let greenhouses = green_count(&state, UnitType::Greenhouse);

    // `SC_C0001`: RED — LOST при 0 research center у GREEN, WON при 0
    // экосфер (и живом research center), иначе PENDING.
    let expected = if research == 0 {
        WinLossState::Lost
    } else if greenhouses > 0 {
        WinLossState::Pending
    } else {
        WinLossState::Won
    };

    let status = victory::evaluate_status(&state, state.turn_counter);
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        expected,
        "campaign 1: research={research}, greenhouses={greenhouses}"
    );
    assert_eq!(status.mission_type, MissionCategory::Campaign as i32);

    // Уничтожаем все центры исследований зелёных → поражение красных.
    destroy_all(&mut state, UnitType::ResearchCenter);
    assert_eq!(green_count(&state, UnitType::ResearchCenter), 0);
    let status = victory::evaluate_status(&state, state.turn_counter);
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

    let mut state = GameState::from_save(&save);
    assert_eq!(state.mission_category, MissionCategory::Scenario);
    assert_eq!(state.mission_index, 18);

    // Пока у зелёных есть центры исследований — миссия продолжается.
    assert!(green_count(&state, UnitType::ResearchCenter) > 0);
    let status = victory::evaluate_status(&state, state.turn_counter);
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Pending
    );

    destroy_all(&mut state, UnitType::ResearchCenter);
    assert_eq!(green_count(&state, UnitType::ResearchCenter), 0);

    let status = victory::evaluate_status(&state, state.turn_counter);
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Won
    );
}

// ---------------------------------------------------------------------------
// (г) Загрузка SAVE1.CAM → 5 тиков: детерминизм двух прогонов, нет паники
// ---------------------------------------------------------------------------

#[test]
fn real_save_loads_normalized_and_ticks_deterministically() {
    let save = SaveFile::parse(SAVE1_CAMPAIGN).expect("fixture parses");
    let mut first = GameState::from_save(&save);
    let mut second = GameState::from_save(&save);

    // --- нормализации `UnitInfo::FileLoad` / `SaveLoad_LoadFormatV70` ---
    assert_eq!(first.unit_id_counter, INITIAL_UNIT_ID_COUNTER);
    assert!(first.unit_count() > 0, "fixture has units");

    for id in first.unit_ids_in_order() {
        let unit = first.unit(id).expect("unit exists");
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

    // Каждый юнит из файла попал в карту клеток (иначе `Access_*` его не видит).
    for id in first.unit_ids_in_order() {
        let grid = first.unit(id).expect("unit").grid;
        assert!(
            first.units_at(grid).contains(&id),
            "unit {id:?} missing from map hash at {grid:?}"
        );
    }

    // --- 5 тиков, два независимых прогона ---
    for _ in 0..5 {
        first.tick();
        second.tick();
    }

    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(
        crc::replay_digest_bytes(&first),
        crc::replay_digest_bytes(&second)
    );
    assert_eq!(crc::state_checksum(&first), crc::state_checksum(&second));
}

#[test]
fn loaded_units_resolve_parent_and_enemy_references() {
    // В трёх реальных фикстурах ссылок `parent_unit`/`enemy_unit` нет (это
    // ранние сейвы без транспортов и атак), поэтому путь разрешения ссылок
    // проверяется на модифицированной копии настоящего сейва: номера объектов
    // двух юнитов связываются, загрузчик обязан вернуть их `UnitId`.
    let mut save = SaveFile::parse(SAVE1_CAMPAIGN).expect("fixture parses");
    assert!(save.units.len() >= 2, "fixture has units");

    let parent_object_index = save.units[1].object_index;
    let parent_id = save.units[1].info.id;
    let enemy_object_index = save.units[0].object_index;
    let enemy_id = save.units[0].info.id;

    save.units[0].parent_object_index = parent_object_index;
    save.units[0].enemy_object_index = enemy_object_index;

    let state = GameState::from_save(&save);
    let unit = state
        .unit(save.units[0].info.id)
        .expect("loaded unit exists");

    assert_eq!(unit.parent_unit, Some(parent_id));
    assert_eq!(unit.enemy_unit, Some(enemy_id));

    // Ссылки, которых нет в файле (номер объекта 0), остаются пустыми.
    let other = state
        .unit(save.units[1].info.id)
        .expect("loaded unit exists");
    assert_eq!(other.parent_unit, None);
    assert_eq!(other.enemy_unit, None);
}

#[test]
fn loaded_lists_and_team_units_match_the_file() {
    let save = SaveFile::parse(SAVE1_CAMPAIGN).expect("fixture parses");
    let state = GameState::from_save(&save);

    for kind in UnitListKind::ALL {
        let expected: Vec<_> = save
            .unit_list(kind)
            .iter()
            .filter_map(|object_index| {
                save.unit_by_object_index(*object_index)
                    .map(|saved| saved.info.id)
            })
            .collect();

        assert_eq!(state.unit_list(kind), expected, "list {kind:?}");
    }

    let team_units = save.team_units();
    for team in 0..4 {
        assert_eq!(state.team_units[team].gold, team_units[team].gold);
        assert_eq!(
            state.team_units[team].complexes().len(),
            team_units[team].complexes().len()
        );
    }
}
