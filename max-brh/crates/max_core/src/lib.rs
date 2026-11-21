//! max_core — чистая детерминированная модель M.A.X. без I/O.
//!
//! Крейт повторяет поведение C++-порта (`max-cpp-port/src`) бит-в-бит: типы и
//! константы оригинала (P0), детерминированный RNG (P1), карта и генерация
//! (P2), далее — юниты, команды, комплексы, экономика (P3), приказы, бой и ИИ
//! (DESIGN.md §7).
//!
//! Текущее состояние:
//! * [`enums`] — все перечисления/битовые маски C++ с точными `#[repr]` и
//!   `TryFrom`-преобразованиями;
//! * [`geo`] — `Point`, `PathStep`, `Direction`, `DIRECTION_OFFSETS`, размеры
//!   карты;
//! * [`rng`] — `Randomizer` (SplitMix64 + Xoshiro128\*\*) с эталонными
//!   векторами из C++ (P1);
//! * [`map`] — `SurfaceMap` (112×112 битмаска `Surface`), `ResourceMapEntry`
//!   (u16-битфилд количества/типа/видимости) и `Map` (state-model §3.10);
//! * [`setup`] — детерминированная генерация карты при старте партии: вывод
//!   `SurfaceMap` из tile-data `.wrl`, ресурсы (`ResourceAllocator`), стартовые
//!   позиции и alien-обломки, с порядком RNG 1:1 к C++;
//! * [`deploy`] — развёртывание стартовых армий при создании партии
//!   (`GameManager_ProcessTeamMissionSupplyUnits`): шахта/генератор/плиты и
//!   загрузка из `SC_L0001` (`CONSTRCT`/`ENGINEER`/`SURVEYOR`), с двумя
//!   RNG-вызовами на команду;
//! * [`unit_values`] — `UnitValues`, индексы `ATTRIB_*`, таблица базовых
//!   значений 93 типов (`SC_ATTRI`) и клановые модификаторы (`SC_CLANS`);
//! * [`team`] — `TeamInfo`/`ResearchTopicInfo`/`TeamUnits`, `Complex`, `Cargo`;
//! * [`unit`] — `UnitId`, `UnitInfo` (симуляционные `S`-поля), пути юнита;
//! * [`hash`] — `UnitHash`/`MapHash` с упорядоченными ведёрками;
//! * [`save`] — V70 `&[u8]` ⇄ [`save::SaveFile`] (полный разбор и запись);
//! * [`order`] — каркас клиентских команд (входной API, DESIGN §2.4);
//! * [`unit_order`] — машина приказов `UnitOrder`×`UnitOrderState` (32×47):
//!   `SetNewOrderInt`, `RestoreOrders`, `BlockedOnPathRequest`, движение
//!   (`ExecuteMove`/`Move`), 32 процессора `ProcessOrder*` (P4);
//! * [`game`] — `GameState`: пять списков юнитов, `ProcessOrders`, тик-цикл,
//!   конец хода, барьеры, канонический дайджест состояния (P4);
//! * [`access`] — проходимость клетки (`Access_IsAccessible` с флагами и
//!   `land_type` 93 типов из `SC_UNITS`), `MapHash`-занятость, цели атаки,
//!   приём в транспорт, `IsEngaged`, групповые списки (P5);
//! * [`classification`] — типобезопасный «вид» над флагами (Ф2): классифицирующие
//!   перечисления (`CombatRole`, `Mobility`, `Environment`, `CargoKind`) для
//!   `UnitType` и `UnitInfo` (решение C: маркерные типажи не используются).
//!   `UnitInfo` (Ф3 переводит логику на этот слой);
//! * [`combat`] — `GetAttackDamage`/`AttackUnit`, `Attack` (площадная атака),
//!   `PrepareFire`/`ProgressFire`/`AimAtTarget`, реактивный огонь
//!   (`AssessAttacks`), стелс-действия диверсанта (P5);
//! * [`pathfinding`] — `Paths_RequestPath` (очередь в порядке постановки),
//!   детерминированный поиск по `AccessMap`, `Paths_GetAirPath`, ракеты,
//!   `PathFill`, `BuilderPath` (P5, синхронно);
//! * [`input`] — структуры входящих пакетов (08 с процессорами P0–P5, 38, 41,
//!   50, 06, 52, 45, 00/01, 10/20, 11/12/13/14/18/43/53) как чистые данные;
//! * [`events`] — очередь команд, события юнитов и применение пакетов к
//!   `GameState` в порядке получения (P4/P5);
//! * [`frames`] — кадровые данные типов (`Unit::GetFrameInfo` из `D_*`):
//!   `image_index_max` взрывов/башен/пехоты для `ProgressExplosion`,
//!   `ProcessOrderSentry` и `ORDER_STATE_ELEVATE`;
//! * [`ai`] — ИИ: `AiPlayer` (стратегия, карты угроз, приоритеты атак),
//!   `TaskManager` (очереди напоминаний `priority→normal→deferred`),
//!   49 классов задач `task*.cpp`, `HeatMap`/`ThreatMap`, `WeightTable`,
//!   решения `aiattack.cpp` (P6);
//! * [`victory`] — победа/поражение (P7): generic-правила (`winloss.cpp`,
//!   `menu_check_end_game_conditions`), предикаты Lua-API `max_*`
//!   (`scripter.cpp`), правила миссий 1:1 из `assets/PATCHES.RES`
//!   (`SC_T*`/`SC_S*`/`SC_C*`), сортировка и ранги команд;
//! * [`crc`] — CRC16 пакета 45 (`crc16.c`, `GameManager_CheckDesync`) и
//!   канонические дайджесты состояния для сквозных тестов паритета (P7).
//!
//! Запрещено (DESIGN.md §1.4, §6.4): файловый I/O, сеть, время, потоки,
//! `f32`/`f64` в симуляции, итерация по `HashMap`.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]

pub mod access;
pub mod ai;
pub mod alloc;
pub mod classification;
pub mod combat;
pub mod complex;
pub mod crc;
pub mod deploy;
pub mod enums;
pub mod events;
pub mod frames;
pub mod game;
pub mod geo;
pub mod hash;
pub mod input;
pub mod map;
pub mod order;
pub mod pathfinding;
pub mod power;
pub mod production;
pub mod research;
pub mod rng;
pub mod save;
pub mod save_write;
pub mod setup;
pub mod team;
pub mod transfer;
pub mod unit;
pub mod unit_order;
pub mod unit_values;
pub mod victory;

pub use access::{
    get_attack_target, is_accessible, is_engaged, is_valid_attack_target, unit_type_flags,
    unit_type_land_type, unit_type_properties, UnitTypeProperties, ACCESS_MODIFIER_NONE,
    ACCESS_MODIFIER_SAME_CLASS_BLOCKS, UNIT_TYPE_PROPERTIES,
};
pub use ai::HeatMap as AiHeatMap;
pub use ai::{
    ai_begin_turn, ai_check_end_turn, ai_check_mines, ai_check_reactions,
    ai_clear_tasks_pending_flags, ai_determine_caution_level, ai_enable_auto_survey,
    ai_evaluate_attack_targets, ai_init, ai_is_dangerous_location, ai_remove_unit,
    ai_select_starting_position, ai_set_info_map_point, ai_set_tasks_pending_flag,
    ai_setup_strategy, ai_unit_hidden, ai_unit_spotted, task_manager_need_to_reserve_raw_materials,
    AiPlayer, AiReactionState, AiSettings, AiState, Reminder, Task, TaskHeader, TaskKind,
    TaskManager, ThreatMap, WeightTable,
};
pub use alloc::{
    balance as alloc_balance, cargo_get_cargo_capacity, cargo_get_inventory,
    get_cargo_info as complex_get_cargo_info, get_cargo_minable as complex_get_cargo_minable,
    get_cargo_mining as complex_get_cargo_mining, is_active_mine, mining_survey,
    optimize as alloc_optimize, optimize_masked as alloc_optimize_masked, reduce_production,
    total_mining as complex_total_mining, unit_complex, update_resources_total, MiningKind,
    MINING_CAPACITY, MINING_SURVEY_LIMIT, MINING_SURVEY_RADIUS,
};
pub use classification::{
    is_turret, CargoKind, CombatRole, Environment, Mobility, PRODUCING_UNIT_TYPES,
    TURRET_UNIT_TYPES,
};
pub use combat::{
    aim_at_target, assess_attacks, attack, attack_unit, get_attack_damage, prepare_fire,
    progress_fire, ATTACK_DAMAGE_MIN,
};
pub use complex::{
    attach_complex, attach_to_primary_complex, connected_building, detach_complex,
    first_untested_connection, remove_connections, team_building_at, test_connections,
    unit_size as complex_unit_size, update_connectors, CONNECTOR_ORDER,
};
pub use crc::{
    crc16, crc16_update_byte, get_crc16_word, get_unit_list_checksum, replay_digest_bytes,
    replay_digest_crc, state_checksum, state_digest, CRC16_INITIAL, CRC16_POLYNOMIAL,
    REPLAY_DIGEST_MAGIC,
};
pub use deploy::{
    deploy_starting_units, deploy_starting_units_with, deploy_team_supplies, find_reachable_spot,
    is_valid_starting_position, set_initial_mining, start_position_mining_station,
    starting_position_power_generator, survey_resources_in_area, TeamDeployment, TeamSupply,
    DEFAULT_MISSION_LOADOUT, DEFAULT_START_GOLD, DEPLOY_SEARCH_RANGE, LARGE_SLAB_IMAGE_COUNT,
    SMALL_SLAB_IMAGE_COUNT,
};
pub use enums::{
    AiStrategy, CautionLevel, FileType, GameStateType, InvalidEnumValue, Opponent, OpponentType,
    Planet, PlanetType, PlayMode, ResearchTopic, ResearchTopicType, Surface, SurfaceType, TeamClan,
    TeamClanType, TeamIndex, TeamType, UnitOrder, UnitOrderState, UnitOrderStateType,
    UnitOrderType, UnitType, VictoryType, WinLossState,
};
pub use events::{apply_command, ApplyOutcome, CommandQueue, UnitEvent};
pub use game::{
    GameState, LiveAttack, OrderData, UnitSound, UnitSoundKind,
    GFX_MAP_TILE_SIZE as GAME_GFX_MAP_TILE_SIZE, INITIAL_UNIT_ID_COUNTER, PLAYER_TEAM_COUNT,
    PLAYER_TEAM_MAX, TEAM_SLOTS,
};
pub use geo::{
    Direction, PathStep, Point, DIRECTION_OFFSETS, MAP_HEIGHT, MAP_SIZE, MAP_TILES, MAP_WIDTH,
};
pub use hash::{MapHash, MapHashObject, UnitHash};
pub use input::{
    Command, ComplexSummaryEntry, MovePathData, OrderPayload, Packet00TurnBarrier,
    Packet01FrameBarrier, Packet06FinishedTurn, Packet08Order, Packet10UpgradeUnitType,
    Packet11ComplexOrder, Packet12MissionSupply, Packet13Seed, Packet14DeployUnit, Packet18Chat,
    Packet20UpgradeUnit, Packet38Move, Packet41MoveFinished, Packet43Rename, Packet45CrcBarrier,
    Packet50StopUnit, Packet52EndTurnBarrier, Packet53ComplexSummary, PacketKind, PayloadLevel,
    PayloadP1, PayloadP2, PayloadP3, PayloadP4, PayloadP5, UPGRADE_VALUE_COUNT,
};
pub use map::{Map, MapSize, ResourceKind, ResourceMap, ResourceMapEntry, SurfaceMap};
pub use order::Order;
pub use pathfinding::{
    find_ground_path, get_missile_path, paths_get_air_path, request_path, PathRequest,
};
pub use power::{
    complex_totals as complex_energy_totals, industry_unit_name, is_power_producer,
    is_switchable_consumer, optimize_complex, shortage_build_error, EnergyTotals, IndustryOutcome,
    Shortage, CONSUMER_OFF_ORDER,
};
pub use production::{
    buildable_units, builder_type, can_afford_turn, can_build, complex_raw_materials,
    cost_per_turn, find_build_site, initial_build_site, is_builder, is_factory, is_mobile_builder,
    is_site_accessible, is_site_accessible_ignoring, max_possible_build_rate, progress_build_state,
    site_anchor_for_cursor, site_covers, total_cost, turns_to_build as production_turns_to_build,
    BuildError, BuildReport, ProductionEvent, ProductionEventKind, ProductionManager, BUILDABLE,
    FACTORIES, MOBILE_BUILDERS,
};
pub use research::{
    apply_research_upgrades, research_attribute_values, research_factor, research_new_turn,
    research_turns_to_complete, team_upgrade_cost, team_upgrade_cost_step,
    update_research_progress, upgrade_amount, upgrade_amount_for_base, upgrade_control_cost,
    upgrade_cost_step, upgrade_packet_values, ResearchEvent, UpgradeQuote, RESEARCH_TOPIC_FACTORS,
    UPGRADE_CONTROL_COUNT, UPGRADE_CONTROL_FACTORS, UPGRADE_COST_LIMIT, UPGRADE_PACKET_ATTRIBUTES,
};
pub use rng::{GenerateValue, Randomizer};
pub use save::{
    AiPlayerState, GameManagerState, HeatMap, IniOptions, IniPreferences, MessageLogEntry,
    SaveError, SaveFile, SaveFormatHeader, SaveTeamUnits, SaveUnit, SavedComplex, SavedUnitValues,
    SpottedUnit, SAVE_VERSION_V70,
};
pub use save_write::{build_save_file, SaveMeta, SAVE_UNIT_HASH_SIZE};
pub use setup::{
    ai_choose_starting_position, ai_is_safe_starting_position, derive_surface_map, generate_map,
    AlienPlacement, AlienUnitCosts, GameSetup, GenerationSettings, ResourceLevel, SetupOutput,
    TeamStart,
};
pub use team::{
    Cargo, Complex, ComplexId, ResearchTopicInfo, ScreenLocation, Team, TeamId, TeamInfo,
    TeamInfoUi, TeamKind, TeamUnits,
};
pub use transfer::{
    complex_free_capacity, complex_income, complex_stored, complex_transfer,
    find_unit_with_cargo_type, process_transfer, transfer_cargo, transfer_cargo_kind,
    transfer_menu_cargo_info, unit_cargo,
};
pub use unit::{AirPath, BuilderPath, GroundPath, Rect, UnitId, UnitInfo, UnitListKind, UnitPath};
pub use unit_order::{
    blocked_on_path_request, move_finished, restore_orders, set_new_order_int, OrderContext,
    MOVEMENT_WAIT_LIMIT,
};
pub use unit_values::{
    apply_clan_tradeoffs, base_unit_values, base_unit_values_by_index, clan_tradeoff,
    team_base_values, upgrade_offset_factor, UnitValues, ATTRIB_AGENT_ADJUST, ATTRIB_AMMO,
    ATTRIB_ARMOR, ATTRIB_ATTACK, ATTRIB_ATTACK_RADIUS, ATTRIB_COUNT, ATTRIB_FUEL, ATTRIB_HITS,
    ATTRIB_MOVE_AND_FIRE, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN, ATTRIB_SPEED, ATTRIB_STORAGE,
    ATTRIB_TURNS, BASE_UNIT_VALUES, CLAN_TRADEOFFS, UNIT_COUNT,
};
pub use victory::{
    cargo_fuel_consumption_rate, cargo_get_net_production, cargo_gold_consumption_rate,
    cargo_life_consumption_rate, cargo_power_consumption_rate, cargo_raw_consumption_rate,
    cargo_type_of, check_end_game_conditions, count_casualties, count_damaged_units,
    count_ready_units, count_total_mining, count_units_above_attrib_level,
    count_units_with_reduced_attrib, default_win_loss_conditions, evaluate_status,
    get_normal_rate_build_cost, get_research_level, get_total_consumption, get_total_mining,
    get_total_power_consumption, get_total_units_being_constructed, get_total_worth_of_assets,
    get_worth_of_assets, has_attack_power, has_construction_infrastructure, has_materials,
    has_production_infrastructure, has_unit_above_mark_level, has_unit_experience,
    mission_win_loss, relevant_list, CargoType, MissionCategory, NetProduction, UnitClass,
    WinLossStatus, PLAYING_TEAMS,
};

#[cfg(test)]
mod tests {
    use crate::enums::{UnitOrder, UnitOrderState, UnitType};
    use crate::geo::{Direction, Point, DIRECTION_OFFSETS, MAP_TILES};
    use crate::rng::Randomizer;
    use crate::{Team, TeamId, TeamKind};

    #[test]
    fn core_smoke() {
        let team = Team::new(TeamId(0), TeamKind::Player);
        assert_eq!(team.kind, TeamKind::Player);
    }

    #[test]
    fn p0_constants_are_visible_from_crate_root() {
        assert_eq!(UnitType::COUNT, 93);
        assert_eq!(UnitOrder::COUNT, 32);
        assert_eq!(UnitOrderState::COUNT, 47);
        assert_eq!(MAP_TILES, 12_544);
        assert_eq!(Direction::COUNT, DIRECTION_OFFSETS.len());
        assert_eq!(Point::new(1, 2).x, 1);
    }

    #[test]
    fn p1_rng_is_deterministic() {
        let mut first = Randomizer::new(0xDEAD_BEEF);
        let mut second = Randomizer::new(0xDEAD_BEEF);
        for _ in 0..1000 {
            assert_eq!(first.next_u32(), second.next_u32());
        }
    }

    #[test]
    fn p3_types_are_visible_from_crate_root() {
        // Таблица атрибутов, доменные команды и сейв-слой доступны с корня.
        assert_eq!(crate::BASE_UNIT_VALUES.len(), crate::UNIT_COUNT);
        assert_eq!(crate::UNIT_COUNT, 93);
        let units = crate::TeamUnits::with_clan(crate::TeamClan::Musashi);
        assert_eq!(units.complexes().len(), 0);
        assert_eq!(
            crate::SaveFile::parse(SAVE_BYTES).map(|save| save.header.version),
            Ok(70)
        );
        assert_eq!(
            crate::SaveFile::parse(SAVE_BYTES)
                .expect("fixture parses")
                .units
                .len(),
            312
        );
    }

    const SAVE_BYTES: &[u8] = include_bytes!("../tests/data/save1_v70.cam");
}
