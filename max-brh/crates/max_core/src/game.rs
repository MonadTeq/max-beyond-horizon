//! Состояние партии и игровой цикл тика — C++ `GameManager_*` / `UnitsManager_*`.
//!
//! Оракул: `max-cpp-port/src/units_manager.cpp` (`UnitsManager_ProcessOrders`,
//! `UnitsManager_DeployUnit`, `UnitsManager_RemoveUnitFromUnitLists`,
//! `UnitsManager_EndAttack`), `game_manager.cpp` (`GameManager_ProcessTick`,
//! `GameManager_ManageEconomy`, `GameManager_UpdateScoreGraph`,
//! `GameManager_TeamTurnFinish`, `GameManager_AreTeamsFinishedTurn`),
//! `unitinfo.cpp` (`UnitInfo::AddToDrawList`, `UpdateProduction`).
//! Порядок фаз — DESIGN.md §5.1-§5.3, `max-brh-docs/state-model.md` §3.
//!
//! # Что здесь есть и чего нет
//!
//! P4 закрывает **машину приказов и цикл хода**: пять списков юнитов,
//! `ProcessOrders` в фиксированном порядке, конец хода (экономика, график
//! очков, смена команд), барьеры как факты сети и канонический дайджест
//! состояния для тестов детерминизма.
//!
//! P5 подключает бой (`combat`), проходимость (`access`) и пути
//! (`pathfinding`): `ProcessOrders` вызывает `AssessAttacks` для отложенных
//! реакций, `PendingAirGroupLeader` — `Access_ProcessNewGroupOrder`, тик —
//! синхронный `PollResults`; `MapHash` и `DelayedAttackTargets` входят в
//! состояние.
//!
//! P6 подключает ИИ (`crate::ai`): `Ai_ClearTasksPendingFlags` в
//! `ProcessOrders`, `Ai_CheckReactions`/`Ai_CheckEndTurn` в тике, покрытие
//! сканирования (`Access_UpdateMapStatus`) и `Ai_RemoveUnit` при
//! спавне/движении/уничтожении юнитов. Состояние ИИ — `GameState.ai` —
//! входит в канонический дайджест состояния.
//!
//! # Создание карты и юнитов
//!
//! * `GameManager_InitMap`/генерация карты (`CreateMap`) — в `setup.rs` (P2,
//!   `generate_map`/`derive_surface_map`); результат кладётся в [`GameState::map`].
//! * `GameManager_InitUnitsAndGameState`/`CreateUnits` — раскладка стартовых
//!   юнитов; в P4 её выполняет вызывающий слой через [`GameState::spawn_unit`]
//!   (порт `UnitsManager_DeployUnit` в объёме P4). Автоматическая выкладка по
//!   `TeamMissionSupplies` требует таблицы флагов 93 типов юнитов
//!   (`units.json`, `ResourceManager_InitUnits` — P3-данные) и миссионных
//!   правил (P7), поэтому пока не портирована.
//!
//! # Расхождения с C++-портом (осознанные)
//!
//! 1. Команды применяются в начале тика из очереди [`CommandQueue`] (DESIGN
//!    §5.1, шаг 1) — в C++ это делает `Remote_Synchronize()` /
//!    `Remote_ProcessNetPackets()` в те же моменты цикла; сама сеть вынесена в
//!    `max_net`.
//! 2. Тайм-бюджеты (`TickTimer_*`, лимиты `ExecuteReminders`) не портируются
//!    (DESIGN §5.1, риск 4): очередь обрабатывается полностью.
//! 3. В C++ `UnitsManager_ProcessOrder` может за один тик провести юнит через
//!    несколько клеток (цикл `while (state == IN_TRANSITION && !visible)`), а в
//!    сетевой игре — ровно одна клетка за вызов `Move()` (`if
//!    (Remote_IsNetworkGame) return;`). `max_core` — это lockstep-симуляция,
//!    поэтому принята сетевая ветка: один шаг пути за тик, и она
//!    задокументирована в `unit_order::unit_move`.
//! 4. `UnitInfo::x/y` (пиксельные координаты, UI) поддерживаются только как
//!    производные от клетки: `x = grid*64 + 32` (+31 для зданий).

use crate::enums::{
    unit_flags, GameStateType, PlayMode, ResearchTopic, Surface, UnitOrder, UnitOrderState,
    UnitType, VictoryType, RESEARCH_TOPIC_COUNT,
};
use crate::events::CommandQueue;
use crate::geo::Point;
use crate::hash::{MapHash, UnitHash, HASH_UNIT_HASH_SIZE};
use crate::input::Command;
use crate::map::Map;
use crate::pathfinding::PathRequest;
use crate::rng::Randomizer;
use crate::team::{Complex, TeamId, TeamInfo, TeamKind, TeamUnits, SCORE_GRAPH_LEN};
use crate::unit::{UnitId, UnitInfo, UnitListKind, UnitPath};
use crate::unit_order;
use crate::unit_values::{
    ATTRIB_AGENT_ADJUST, ATTRIB_AMMO, ATTRIB_ATTACK, ATTRIB_HITS, ATTRIB_MOVE_AND_FIRE,
    ATTRIB_ROUNDS, ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS,
};
use crate::victory::{
    cargo_gold_consumption_rate, cargo_raw_consumption_rate, cargo_type_of, CargoType,
};

/// `PLAYER_TEAM_MAX`: четыре игровые команды (`RED..GRAY`) + `ALIEN`.
pub const PLAYER_TEAM_MAX: u8 = 5;
/// Количество игровых команд без `ALIEN` (`PLAYER_TEAM_MAX - 1`).
pub const PLAYER_TEAM_COUNT: usize = PLAYER_TEAM_MAX as usize - 1;
/// Размер массива `TeamInfo`/`TeamUnits` (включая `ALIEN`).
pub const TEAM_SLOTS: usize = PLAYER_TEAM_MAX as usize;
/// `GFX_MAP_TILE_SIZE` — сторона клетки в пикселях.
pub const GFX_MAP_TILE_SIZE: i32 = 64;
/// Стартовое значение `UnitsManager_UnitIdCounter` (state-model §7.8, DESIGN риск 9).
pub const INITIAL_UNIT_ID_COUNTER: u32 = (PLAYER_TEAM_MAX as u32) << 13;

// ---------------------------------------------------------------------------
// Данные приказа, которых нет в `UnitInfo`
// ---------------------------------------------------------------------------

/// Runtime-данные приказа, которые C++ хранит в `UnitInfo`, но V70 — одним
/// полем `target_grid` (state-model §3.9, §7.6).
///
/// `move_to` и `fire_on` в C++ — разные поля (`move_to_grid_x/y`,
/// `fire_on_grid_x/y`), а V70 хранит один `Point`. Поэтому они живут в
/// параллельной таблице [`GameState`]; `UnitInfo::target_grid` зеркалит
/// `move_to` (это семантика поля в сейве).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrderData {
    /// `UnitInfo::move_to_grid_*`.
    pub move_to: Point,
    /// `UnitInfo::fire_on_grid_*`.
    pub fire_on: Point,
}

/// Живая атака — C++ `UnitsManager_LiveAttacks` (номер → список целей).
///
/// В C++ это `std::unordered_map<uint32_t, SmartList<UnitInfo>>`; итерация по
/// карте не выполняется (только поиск по номеру), поэтому детерминизм не
/// страдает. В Rust — `Vec` с линейным поиском: номеров атак мало, а порядок
/// вставки целей сохраняется.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAttack {
    /// Номер атаки (`live_attack`).
    pub id: u32,
    /// Захваченные цели (`pin_count` уже увеличен).
    pub targets: Vec<UnitId>,
}

/// Какой звук юнита проиграть — C++ `Unit::SfxType`
/// (`unit.hpp:49-68`), в объёме боя.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitSoundKind {
    /// `SFX_TYPE_FIRE` — `UnitInfo::PrepareFire` (`unitinfo.cpp:4725`).
    Fire,
    /// `SFX_TYPE_HIT` — попадание по живому (`units_manager.cpp:3922`).
    Hit,
    /// `SFX_TYPE_EXPLOAD` — взрыв уничтоженного (`units_manager.cpp:3929`).
    Explode,
}

/// Одно звуковое событие боя: кто и какой `SfxType`.
///
/// Ресурс `*.SPW` и громкость выбираются на клиенте по таблице `SC_UNITS`
/// (`SoundManager::PlaySfx(UnitInfo*, Unit::SfxType)`,
/// `sound_manager.cpp:413-490`): симуляция обязана оставаться без ссылок на
/// ресурсы (DESIGN §6.1), поэтому событие несёт только id юнита и тип звука.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitSound {
    /// Юнит, чей звук играет (`PlaySfx(unit, …)`).
    pub unit: UnitId,
    /// Тип юнита на момент события (`unit->GetUnitType()`): клиент выбирает по
    /// нему звук из `SC_UNITS`, даже если юнит к моменту чтения уже уничтожен.
    pub unit_type: crate::enums::UnitType,
    /// Тип звука.
    pub kind: UnitSoundKind,
}

// ---------------------------------------------------------------------------
// GameState
// ---------------------------------------------------------------------------

/// Состояние партии — аналог глобалов C++ (`UnitsManager_*`, `GameManager_*`,
/// `UnitEvent_UnitEvents`) в одном объекте (DESIGN §1.4 п.5).
#[derive(Debug, Clone)]
pub struct GameState {
    // --- карта и команды ---
    /// Карта партии.
    pub map: Map,
    /// `UnitsManager_TeamInfo[PLAYER_TEAM_MAX]`.
    pub teams: [TeamInfo; TEAM_SLOTS],
    /// `CTInfo::team_units` (экономика/технологии, включая `ALIEN`).
    pub team_units: [TeamUnits; TEAM_SLOTS],
    /// `Randomizer` симуляции (сид — только пакет 13).
    pub rng: Randomizer,

    // --- юниты ---
    units: Vec<Option<UnitInfo>>,
    free_slots: Vec<u16>,
    unit_hash: UnitHash<u16>,
    order_data: Vec<OrderData>,
    /// Пять списков юнитов в порядке `UnitListKind` (порядок вставки).
    lists: [Vec<UnitId>; 5],
    /// `Hash_MapHash` — клетка → юниты (`P5`; `access.rs`/`pathfinding.rs`).
    pub map_hash: MapHash,
    /// Групповой список юнита (`UnitInfo::unit_list` в C++ — `UnitInfoGroup`).
    /// Параллельно слотам `units`/`order_data`.
    unit_group: Vec<Option<u32>>,
    /// Групповые списки (`SmartList<UnitInfo> UnitInfoGroup` C++).
    pub groups: Vec<Option<Vec<UnitId>>>,
    /// Очередь запросов путей (`PathsManager::m_pending_requests`).
    pub path_requests: std::collections::VecDeque<PathRequest>,
    /// `UnitsManager_DelayedAttackTargets[team]` — цели реактивного огня.
    pub delayed_attack_targets: [Vec<UnitId>; TEAM_SLOTS],
    /// `UnitsManager_DelayedReactionsTeam` — команда, чья очередь проверять
    /// отложенные реакции.
    pub delayed_reactions_team: TeamId,

    // --- фаза и ход ---
    /// `GameManager_GameState`.
    pub game_state: GameStateType,
    /// `GameManager_PlayMode`.
    pub play_mode: PlayMode,
    /// `GameManager_TurnCounter`.
    pub turn_counter: i32,
    /// `GameManager_ActiveTurnTeam`.
    pub active_turn_team: TeamId,
    /// `GameManager_PlayerTeam` (локальный игрок).
    pub player_team: TeamId,
    /// `GameManager_UpdateFlag` — чередование `Ai_CheckReactions` (DESIGN риск 14).
    pub update_flag: bool,
    /// `UnitsManager_UnitIdCounter`.
    pub unit_id_counter: u32,
    /// `UnitsManager_LiveAttackCounter`.
    pub live_attack_counter: u32,
    /// `UnitsManager_OrdersPending` — в этом тике кто-то ещё в движении/бою.
    pub orders_pending: bool,
    /// `UnitsManager_CombatEffectsActive`.
    pub combat_effects_active: bool,
    /// `UnitsManager_BobEffectQuota`.
    pub bob_effect_quota: i8,
    /// `UnitsManager_ResetBobState`.
    pub reset_bob_state: bool,
    /// `UnitsManager_PendingAttacks` — порядок вставки.
    pub pending_attacks: Vec<UnitId>,
    /// `UnitsManager_PendingAirGroupLeader`.
    pub pending_air_group_leader: Option<UnitId>,
    /// `UnitEvent_UnitEvents` (`unitevents.cpp`).
    pub unit_events: Vec<crate::events::UnitEvent>,
    /// `UnitsManager_LiveAttacks` (`live_attack` → цели).
    pub live_attacks: Vec<LiveAttack>,
    /// `UnitsManager_DelayedReactionsPending` (флаг для сети/ИИ).
    pub delayed_reactions_pending: bool,
    /// События последнего хода производства (`GameManager_ProgressBuildState`):
    /// что выпущено/прервано. Не входит в дайджест — это отчёт для UI/QA.
    pub production_events: Vec<crate::production::ProductionEvent>,
    /// События последнего хода исследований (`ResearchMenu_NewTurn`):
    /// завершённые уровни тем. Не входит в дайджест — отчёт для UI/QA.
    pub research_events: Vec<crate::research::ResearchEvent>,
    /// Сообщение энергетики комплекса для верхней строки карты — C++
    /// `ProductionManager::DrawResourceMessage`/`ComposeIndustryMessage`
    /// (`production_manager.cpp:308-330/792-804`), `MESSAGE_BOX_NOTICE`.
    ///
    /// Отчёт для UI/QA, в снапшот/CRC не входит (как `production_events`).
    /// Копится за ход производства и забирается
    /// [`Self::take_industry_message`].
    pub industry_message: Option<String>,
    /// Звуки боя за тик — `SoundManager::PlaySfx(unit, SfxType)`
    /// (`unitinfo.cpp:4725` — выстрел, `units_manager.cpp:3922/3929` — попадание
    /// и взрыв). Симуляция только копит события, ресурс/громкость выбирает
    /// клиент (`max_app::sfx`, таблица `SC_UNITS`). В дайджест/CRC не входит.
    pub unit_sounds: Vec<UnitSound>,
    /// `UnitsManager_TeamInfo[*].finished_turn` дублируется в `teams`; здесь —
    /// счётчики барьеров, чтобы состояние было самодостаточным.
    pub turn_indices: [u8; TEAM_SLOTS],
    /// `Remote_NextTurnIndices` (пакет 45).
    pub next_turn_indices: [u8; TEAM_SLOTS],
    /// `Remote_TeamDataCrc16` (пакет 45).
    pub team_data_crc16: [u16; TEAM_SLOTS],
    /// `Remote_FrameSyncCounter` (пакет 00).
    pub frame_sync_counter: [u8; TEAM_SLOTS],
    /// `Remote_FrameSyncCounter2values` (пакет 01).
    pub frame_sync_counter2: [u8; TEAM_SLOTS],
    /// `Remote_StateSyncIndices` (пакет 54) — индекс последнего **полного**
    /// (`is_last`) состояния команды, по нему завершается 54-барьер.
    pub state_sync_indices: [u8; TEAM_SLOTS],
    /// `Remote_StateSyncReceivingIndices` (пакет 54) — индекс чанка, который
    /// сейчас принимается (сброс `state_sync_unit_ids` при смене индекса).
    pub state_sync_receiving_indices: [u8; TEAM_SLOTS],
    /// `Remote_StateSyncUnitIds` (пакет 54) — id юнитов, присланных владельцем
    /// в текущем раунде; по ним `Remote_RemoveExtraUnits` чистит лишних.
    pub state_sync_unit_ids: [Vec<UnitId>; TEAM_SLOTS],
    /// Чат в игре (пакет 18) — чистые данные для UI/логов, не влияет на сим.
    pub chat_log: Vec<(TeamId, String)>,
    /// Снабжение миссии по командам (пакет 12) — входные данные setup'а.
    pub mission_supplies: [Option<crate::input::Packet12MissionSupply>; TEAM_SLOTS],
    /// Кэш `UnitsManager_Units[team]` — юнит, для которого в этом проходе
    /// сработала отложенная реакция (`UnitsManager_PursueEnemy`).
    pub units_active: [Option<UnitId>; TEAM_SLOTS],
    /// Флаг сетевого режима (`Remote_IsNetworkGame`).
    ///
    /// `max_core` исполняет **lockstep-симуляцию** (DESIGN §1.1/§5.1), поэтому
    /// по умолчанию `true`: это включает сетевые ветки C++-кода (один шаг
    /// движения за тик, `finished_turn`-проверки, применение пакетов в начале
    /// тика).
    pub network_game: bool,
    /// Настройка `effects` (`ResourceManager_GetSettings()->"effects"`).
    ///
    /// В симуляции она влияет только на счётчик `moved` в `ORDER_BUILD`
    /// (`UnitsManager_ProcessOrderBuild`); UI-анимации не портируются.
    pub settings_effects: bool,
    /// Настройка `disable_fire` — запрет реактивного огня (DESIGN §6.5).
    pub disable_fire: bool,
    /// Настройка `GameManager_RealTime` (`paths.cpp:Paths_CalculateStep`):
    /// при истине шаг пути не тратит `speed`.
    pub real_time: bool,
    /// Настройка `enemy_halt` («Halt movement when enemy is detected»).
    ///
    /// `Access_UpdateMapStatus` (`access.cpp:827-868`) при обнаружении врага в
    /// радиусе сканирования ставит `UnitEventEmergencyStop` юниту (или его
    /// группе), если у него есть активный приказ. Настройка читается из
    /// `settings` на каждом шаге, поэтому в C++ она не входит в состояние
    /// партии; здесь — поле `GameState`, чтобы симуляция была самодостаточной.
    /// `App` пишет его из `Preferences` при старте/загрузке партии.
    ///
    /// По умолчанию `false`: чистое ядро (юнит-тесты) сохраняет прежнее
    /// поведение; приложение ставит значение из настроек (`settings.cpp`:
    /// `enemy_halt = 1`).
    pub enemy_halt: bool,
    /// Очередь входных команд (DESIGN §5.1 шаг 1).
    pub commands: CommandQueue,
    /// Состояние ИИ: игроки, менеджер задач, тепловые карты (P6, `ai.cpp`).
    pub ai: crate::ai::AiState,
    /// Данные V70, нужные только для записи сейва (P3): настройки партии,
    /// UI-поля `TeamInfo`, журналы сообщений, V70-поля `AiPlayer`.
    ///
    /// В канонический дайджест (`snapshot_bytes`) не входит — это не игровое
    /// состояние, а «то, что C++ хранит вне `GameState`».
    pub save_meta: crate::save_write::SaveMeta,

    // --- правила партии (setup-константы; P7) ---
    /// `SaveFormatHeader::mission_index` (0 — generic custom game).
    pub mission_index: u16,
    /// `MissionManager::GetMission()->GetCategory()` (`winloss.cpp:57`).
    pub mission_category: crate::victory::MissionCategory,
    /// `ini_setting_victory_type` (`VICTORY_TYPE_DURATION`/`SCORE`).
    pub victory_type: VictoryType,
    /// `ini_setting_victory_limit` — лимит ходов или очков.
    pub victory_limit: i32,
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

impl GameState {
    /// Пустая партия: карта по умолчанию, команды `None`, 0-й тик.
    pub fn new() -> Self {
        let mut teams = std::array::from_fn(|_| TeamInfo::new());
        for team in teams.iter_mut() {
            team.team_type = TeamKind::None;
        }
        let team_units = std::array::from_fn(|_| TeamUnits::new());
        // Пустая партия для юнит-тестов: поверхность `LAND`, иначе ни один
        // наземный юнит не смог бы двигаться (реальная карта приходит из
        // `setup::generate_map` и подменяет её целиком).
        let mut map = Map::new();
        for y in 0..crate::geo::MAP_HEIGHT {
            for x in 0..crate::geo::MAP_WIDTH {
                map.surface.set(x, y, Surface::LAND);
            }
        }
        Self {
            map,
            teams,
            team_units,
            rng: Randomizer::new(0),
            units: Vec::new(),
            free_slots: Vec::new(),
            unit_hash: UnitHash::new(HASH_UNIT_HASH_SIZE),
            order_data: Vec::new(),
            lists: std::array::from_fn(|_| Vec::new()),
            map_hash: MapHash::for_standard_map(),
            unit_group: Vec::new(),
            groups: Vec::new(),
            path_requests: std::collections::VecDeque::new(),
            delayed_attack_targets: std::array::from_fn(|_| Vec::new()),
            delayed_reactions_team: TeamId::RED,
            game_state: GameStateType::InGame,
            play_mode: PlayMode::TurnBased,
            turn_counter: 0,
            active_turn_team: TeamId::RED,
            player_team: TeamId::RED,
            update_flag: false,
            unit_id_counter: INITIAL_UNIT_ID_COUNTER,
            live_attack_counter: 0,
            orders_pending: false,
            combat_effects_active: false,
            bob_effect_quota: 5,
            reset_bob_state: false,
            pending_attacks: Vec::new(),
            pending_air_group_leader: None,
            unit_events: Vec::new(),
            live_attacks: Vec::new(),
            delayed_reactions_pending: false,
            production_events: Vec::new(),
            research_events: Vec::new(),
            industry_message: None,
            unit_sounds: Vec::new(),
            turn_indices: [0; TEAM_SLOTS],
            next_turn_indices: [0; TEAM_SLOTS],
            team_data_crc16: [0; TEAM_SLOTS],
            frame_sync_counter: [0; TEAM_SLOTS],
            frame_sync_counter2: [0; TEAM_SLOTS],
            state_sync_indices: [0; TEAM_SLOTS],
            state_sync_receiving_indices: [0; TEAM_SLOTS],
            state_sync_unit_ids: std::array::from_fn(|_| Vec::new()),
            chat_log: Vec::new(),
            mission_supplies: std::array::from_fn(|_| None),
            units_active: [None; TEAM_SLOTS],
            network_game: true,
            settings_effects: false,
            disable_fire: false,
            real_time: false,
            enemy_halt: false,
            commands: CommandQueue::new(),
            ai: crate::ai::AiState::new(),
            save_meta: crate::save_write::SaveMeta::default(),
            // `settings.cpp` defaults: generic custom game, duration, лимит 50.
            mission_index: 0,
            mission_category: crate::victory::MissionCategory::Custom,
            victory_type: VictoryType::Duration,
            victory_limit: 50,
        }
    }

    /// Партия с картой и набором команд (`TeamKind` за игровыми командами).
    pub fn with_setup(map: Map, team_kinds: [TeamKind; PLAYER_TEAM_COUNT]) -> Self {
        let mut state = Self::new();
        state.map = map;
        for (index, kind) in team_kinds.iter().enumerate() {
            state.teams[index].team_type = *kind;
            state.team_units[index] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
        }
        state.teams[PLAYER_TEAM_COUNT].team_type = TeamKind::Computer;
        state.team_units[PLAYER_TEAM_COUNT] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
        state
    }

    // -----------------------------------------------------------------------
    // Загрузка V70-сейва (P7): нормализации `saveload.cpp` / `unitinfo.cpp`
    // -----------------------------------------------------------------------

    /// Партия из V70-сейва: [`Self::new`] + [`Self::load_state`].
    pub fn from_save(save: &crate::save::SaveFile) -> Self {
        let mut state = Self::new();
        state.load_state(save);
        state
    }

    /// Применяет разобранный V70-сейв к состоянию, **с нормализациями C++**,
    /// чтобы загруженную партию сразу можно было тикать.
    ///
    /// # Порт `SaveLoad_LoadFormatV70` (`saveload.cpp:756`) и
    /// `UnitInfo::FileLoad` (`unitinfo.cpp:2489`)
    ///
    /// 1. `UnitsManager_UnitIdCounter = PLAYER_TEAM_MAX << 13`
    ///    (`saveload.cpp:834`) — новые id не пересекаются с V70-диапазоном.
    /// 2. `unit.movement_wait_count = 0` (V70-ветка `FileLoad`).
    /// 3. `unit.ai_state_bits &= ~(AI_STATE_MOVE_FINISHED_REMINDER |
    ///    AI_STATE_RETREAT_REMINDER)` (`unitinfo.cpp:2512`) — напоминания ИИ
    ///    живут только в текущем процессе и в сейв не переносятся.
    /// 4. `build_rate == 0 → 1` (`unitinfo.cpp:2514`).
    /// 5. `state ∈ {NEW_ORDER, MOVE_GETTING_PATH, ISSUING_PATH, IN_TRANSITION,
    ///    IN_PROGRESS} → EXECUTING_ORDER` (`unitinfo.cpp:2527`): после загрузки
    ///    незавершённых переходов нет.
    /// 6. `HeatMap::LoadV70` — отрицательные значения клеток клампятся в 0
    ///    (`heatmap.cpp:212`) для команд с `team_type != NONE`.
    /// 7. `stats_gold_spent_on_upgrades` — кламп ≥ 0 (это делает `save.rs` при
    ///    разборе, `read_team_info`).
    /// 8. `TeamUnits` и `TeamInfo` переносятся как есть (`TeamUnits::FileLoad`).
    ///
    /// # Осознанные расхождения
    ///
    /// * `Randomizer` в V70 не сериализуется: C++ читает `rng_seed` заголовка
    ///   только для лобби и **не** трогает состояние ГПСЧ при загрузке
    ///   (`saveload.cpp:776`, `Randomizer_SetSeed` вызывается лишь при старте
    ///   партии). Для детерминизма «загрузил → тикаешь» здесь поток
    ///   пере-сидируется `header.rng_seed`.
    /// * `Hash_MapHash`/`Hash_UnitHash` не копируются из файла как объекты: они
    ///   восстанавливаются по спискам и id (порядок вставки — порядок файла).
    /// * `MessageLog`, `selected_unit`, камера/зум и прочие UI-поля — вне
    ///   `max_core` (state-model §6).
    pub fn load_state(&mut self, save: &crate::save::SaveFile) {
        use crate::enums::{AI_STATE_MOVE_FINISHED_REMINDER, AI_STATE_RETREAT_REMINDER};

        // --- карта и заголовок партии ---
        self.map = Map::from_parts(save.surface(), save.resource_map());
        self.mission_index = save.header.mission_index;
        self.mission_category =
            crate::victory::MissionCategory::from_file_type(save.header.save_game_type);
        self.play_mode = save.header.play_mode;
        self.victory_type = save.ini_options.victory_type;
        self.victory_limit = save.ini_options.victory_limit;
        // Настройки/UI-поля/журналы и V70-поля ИИ — для записи сейва 1:1 (P3).
        self.save_meta = crate::save_write::SaveMeta::from_save(save);

        // --- глобальное состояние партии ---
        self.game_state = save.game.game_state;
        self.turn_counter = save.game.turn_counter;
        self.active_turn_team = save.game.active_turn_team;
        self.player_team = save.game.player_team;
        // V70 не хранит состояние ГПСЧ (см. «Осознанные расхождения»).
        self.rng = Randomizer::new(save.header.rng_seed);
        self.unit_id_counter = INITIAL_UNIT_ID_COUNTER;
        self.live_attack_counter = 0;
        self.orders_pending = false;
        self.combat_effects_active = false;
        self.delayed_reactions_pending = false;
        self.pending_attacks.clear();
        self.pending_air_group_leader = None;
        self.unit_events.clear();
        self.live_attacks.clear();
        self.path_requests.clear();
        self.chat_log.clear();
        for targets in &mut self.delayed_attack_targets {
            targets.clear();
        }
        self.delayed_reactions_team = TeamId::RED;
        self.units_active = [None; TEAM_SLOTS];
        self.turn_indices = [0; TEAM_SLOTS];
        self.next_turn_indices = [0; TEAM_SLOTS];
        self.team_data_crc16 = [0; TEAM_SLOTS];
        self.frame_sync_counter = [0; TEAM_SLOTS];
        self.frame_sync_counter2 = [0; TEAM_SLOTS];
        self.state_sync_indices = [0; TEAM_SLOTS];
        self.state_sync_receiving_indices = [0; TEAM_SLOTS];
        for ids in &mut self.state_sync_unit_ids {
            ids.clear();
        }
        self.commands = CommandQueue::new();

        // --- команды: TeamInfo × 4 из файла, ALIEN — служебный слот ---
        for team in 0..crate::save::SAVE_TEAM_COUNT {
            self.teams[team] = save.teams[team].clone();
        }
        self.teams[PLAYER_TEAM_COUNT].team_type = TeamKind::Computer;
        self.teams[PLAYER_TEAM_COUNT].team_clan = save.header.team_clan[PLAYER_TEAM_COUNT];

        // --- TeamUnits × 4 из файла; ALIEN — пустые значения ---
        let saved_team_units = save.team_units();
        for team in 0..crate::save::SAVE_TEAM_COUNT {
            self.team_units[team] = saved_team_units[team].clone();
        }
        self.team_units[PLAYER_TEAM_COUNT] = TeamUnits::new();

        // --- юниты ---
        self.units.clear();
        self.free_slots.clear();
        self.order_data.clear();
        self.unit_group.clear();
        self.groups.clear();
        self.lists = std::array::from_fn(|_| Vec::new());
        self.unit_hash = UnitHash::new(HASH_UNIT_HASH_SIZE);

        let mut object_ids: Vec<(u16, UnitId)> = Vec::with_capacity(save.units.len());

        for saved in &save.units {
            let mut unit = saved.info.clone();

            // `UnitInfo::FileLoad` (V70-ветка) + нормализации.
            unit.movement_wait_count = 0;
            unit.ai_state_bits &= !(AI_STATE_MOVE_FINISHED_REMINDER | AI_STATE_RETREAT_REMINDER);
            if unit.build_rate == 0 {
                unit.build_rate = 1;
            }
            if matches!(
                unit.state,
                UnitOrderState::NewOrder
                    | UnitOrderState::MoveGettingPath
                    | UnitOrderState::IssuingPath
                    | UnitOrderState::InTransition
                    | UnitOrderState::InProgress
            ) {
                unit.state = UnitOrderState::ExecutingOrder;
            }
            // Ссылки разрешаются вторым проходом по номеру объекта.
            unit.parent_unit = None;
            unit.enemy_unit = None;

            let slot = self.units.len();
            object_ids.push((saved.object_index, unit.id));
            self.units.push(Some(unit.clone()));
            self.order_data.push(OrderData {
                move_to: saved.info.target_grid,
                fire_on: Point::ZERO,
            });
            self.unit_group.push(None);
            self.unit_hash.push_at(u32::from(unit.id.0), slot as u16);
        }

        object_ids.sort_by_key(|(object_index, _)| *object_index);
        let resolve = |object_index: u16| -> Option<UnitId> {
            object_ids
                .binary_search_by_key(&object_index, |entry| entry.0)
                .ok()
                .map(|index| object_ids[index].1)
        };

        for (index, saved) in save.units.iter().enumerate() {
            let Some(entry) = self.units.get_mut(index) else {
                continue;
            };
            let Some(unit) = entry.as_mut() else { continue };

            unit.parent_unit = if saved.parent_object_index == 0 {
                None
            } else {
                resolve(saved.parent_object_index)
            };
            unit.enemy_unit = if saved.enemy_object_index == 0 {
                None
            } else {
                resolve(saved.enemy_object_index)
            };
        }

        // Пять списков — в порядке файла (`SmartList_UnitInfo_FileLoad`).
        for kind in UnitListKind::ALL {
            let mut members = Vec::new();
            for &object_index in save.unit_list(kind) {
                if let Some(id) = resolve(object_index) {
                    members.push(id);
                }
            }
            for &id in &members {
                if let Some(unit) = self.unit_mut(id) {
                    unit.unit_list = kind;
                }
            }
            self.lists[kind.index()] = members;
        }

        // `Hash_MapHash` — из файла (порядок ведёрок/клеток/юнитов сохраняется).
        self.map_hash = MapHash::for_standard_map();
        for bucket in save.map_hash.buckets() {
            for object in bucket {
                let mut units = Vec::with_capacity(object.units.len());
                for &object_index in &object.units {
                    if let Some(id) = resolve(object_index) {
                        units.push(id);
                    }
                }
                if !units.is_empty() {
                    self.map_hash
                        .bucket_mut(object.x, object.y)
                        .push(crate::hash::MapHashObject {
                            x: object.x,
                            y: object.y,
                            units,
                        });
                }
            }
        }
        // Страховка: юнит, которого нет в восстановленном хэше, добавляется по
        // своей клетке (иначе `Access_*` его «не увидит»).
        for id in self.snapshot_unit_ids() {
            let Some(unit) = self.unit(id) else { continue };
            let present = self
                .units_at_opt(unit.grid)
                .is_some_and(|ids| ids.contains(&id));
            if !present {
                self.map_hash_add(id, unit.grid, unit.flags);
            }
        }

        // --- тепловые карты: `HeatMap::LoadV70` с клампом отрицательных ---
        for team in 0..PLAYER_TEAM_COUNT {
            let Some(saved) = save.heat_maps[team].as_deref() else {
                continue;
            };
            self.ai.heat_maps[team].load_v70(
                &saved.complete,
                &saved.stealth_sea,
                &saved.stealth_land,
            );
        }

        // --- состояние ИИ: игроки из сейва, очереди задач пусты ---
        self.ai.task_manager = crate::ai::TaskManager::new();
        self.ai.reaction_state = crate::ai::AiReactionState::Idle;
        for team in 0..PLAYER_TEAM_COUNT {
            let mut player = crate::ai::AiPlayer::new(TeamId(team as u8));

            if let Some(saved) = save.ai_players[team].as_deref() {
                player.strategy = crate::enums::AiStrategy::try_from(saved.strategy)
                    .unwrap_or(crate::enums::AiStrategy::Random);
                player.greenhouse_ratio = saved.greenhouse_ratio;
                player.minefield_density = saved.minefield_density;
                player.target_location = saved.target_location;
                player.need_init = false;
                player.spotted_units = saved
                    .spotted_units
                    .iter()
                    .filter_map(|spotted| {
                        let unit = spotted.unit?;
                        Some(crate::ai::SpottedUnit {
                            unit: Some(unit),
                            task: None,
                            team: TeamId(team as u8),
                            last_position: spotted.last_position,
                            visible_to_team: spotted.visible_to_team,
                        })
                    })
                    .collect();
                if let Some(map) = saved.info_map.as_deref() {
                    player.info_map = map.to_vec();
                }
                if let Some(map) = saved.mine_map.as_deref() {
                    player.mine_map = map.to_vec();
                }
            }

            self.ai.players[team] = player;
        }
    }

    // -----------------------------------------------------------------------
    // Доступ к юнитам
    // -----------------------------------------------------------------------

    /// Поиск слота по id через `UnitHash`-ведёрко (`Hash_UnitHash[...]`).
    fn lookup(&self, id: UnitId) -> Option<usize> {
        let size = self.unit_hash.size();
        let bucket = self
            .unit_hash
            .bucket((u32::from(id.0) % u32::from(size)) as u16)?;
        bucket.iter().copied().find_map(|slot| {
            let unit = self.units.get(slot as usize)?.as_ref()?;
            (unit.id == id).then_some(slot as usize)
        })
    }

    /// Юнит по id (`Hash_UnitHash[id]`; `None` — юнита нет).
    #[inline]
    pub fn unit(&self, id: UnitId) -> Option<&UnitInfo> {
        self.lookup(id)
            .and_then(|slot| self.units.get(slot).and_then(Option::as_ref))
    }

    /// Изменяемый юнит по id.
    #[inline]
    pub fn unit_mut(&mut self, id: UnitId) -> Option<&mut UnitInfo> {
        let slot = self.lookup(id)?;
        self.units.get_mut(slot).and_then(Option::as_mut)
    }

    /// Есть ли юнит с таким id.
    #[inline]
    pub fn has_unit(&self, id: UnitId) -> bool {
        self.lookup(id).is_some()
    }

    /// Число живых юнитов.
    pub fn unit_count(&self) -> usize {
        self.units.iter().filter(|slot| slot.is_some()).count()
    }

    /// Данные приказа юнита.
    pub fn order_data(&self, id: UnitId) -> OrderData {
        self.lookup(id)
            .and_then(|slot| self.order_data.get(slot).copied())
            .unwrap_or_default()
    }

    /// Запись данных приказа.
    pub fn set_order_data(&mut self, id: UnitId, data: OrderData) {
        if let Some(slot) = self.lookup(id) {
            if let Some(entry) = self.order_data.get_mut(slot) {
                *entry = data;
            }
        }
    }

    /// Записывает изменённую копию юнита обратно (см. `unit_order`).
    ///
    /// Если клетка изменилась, обновляется `Hash_MapHash` — аналог
    /// `Hash_MapHash.Remove/Add` в `UnitInfo::Move`/`UpdateGridPosition`.
    pub fn write_unit(&mut self, id: UnitId, unit: UnitInfo) {
        if let Some(slot) = self.lookup(id) {
            let previous = self
                .units
                .get(slot)
                .and_then(|entry| entry.as_ref())
                .cloned();

            if let Some(previous) = &previous {
                if previous.grid != unit.grid {
                    self.map_hash.remove(
                        id,
                        previous.grid.x as u16,
                        previous.grid.y as u16,
                        previous.flags,
                    );
                    self.map_hash.add(
                        id,
                        unit.grid.x as u16,
                        unit.grid.y as u16,
                        unit.flags,
                        false,
                    );

                    // `Access_UpdateMapStatus` — покрытие сканирования
                    // переезжает вместе с юнитом (P6).
                    self.update_map_status_unit(previous, false);
                }
            }

            if let Some(entry) = self.units.get_mut(slot) {
                *entry = Some(unit.clone());
            }

            if let Some(previous) = &previous {
                if previous.grid != unit.grid {
                    self.update_map_status(id, true);
                }
            }
        }
    }

    /// `Hash_MapHash[Point]` — юниты клетки в порядке вставки.
    #[inline]
    pub fn units_at(&self, grid: Point) -> &[UnitId] {
        self.units_at_opt(grid).map(Vec::as_slice).unwrap_or(&[])
    }

    /// `Hash_MapHash[Point]`: `None`, если клетка пуста или вне карты.
    #[inline]
    pub fn units_at_opt(&self, grid: Point) -> Option<&Vec<UnitId>> {
        if grid.x < 0
            || grid.y < 0
            || grid.x >= crate::geo::MAP_WIDTH
            || grid.y >= crate::geo::MAP_HEIGHT
        {
            return None;
        }
        self.map_hash.get(grid.x as u16, grid.y as u16)
    }

    /// `Hash_MapHash.Add` для внешних модулей (`MoveInTransitUnitInMapHash`).
    pub fn map_hash_add(&mut self, id: UnitId, grid: Point, flags: u32) {
        if grid.x >= 0 && grid.y >= 0 {
            self.map_hash
                .add(id, grid.x as u16, grid.y as u16, flags, false);
        }
    }

    /// `Hash_MapHash.Remove` для внешних модулей.
    pub fn map_hash_remove(&mut self, id: UnitId, grid: Point, flags: u32) {
        if grid.x >= 0 && grid.y >= 0 {
            self.map_hash
                .remove(id, grid.x as u16, grid.y as u16, flags);
        }
    }

    /// `ResourceManager_GetPathsManager().RemoveRequest(unit)`.
    pub fn remove_path_request(&mut self, id: UnitId) {
        self.path_requests.retain(|request| request.unit != id);
    }

    // -----------------------------------------------------------------------
    // Групповые списки (`UnitInfoGroup`)
    // -----------------------------------------------------------------------

    /// Список группы юнита (`UnitInfo::GetUnitList`).
    pub fn group_members(&self, id: UnitId) -> Option<Vec<UnitId>> {
        let slot = self.lookup(id)?;
        let group = self.unit_group.get(slot).copied().flatten()?;
        self.groups.get(group as usize)?.clone()
    }

    /// Номер группы юнита (`None` — юнит вне группы).
    pub fn group_id_of(&self, id: UnitId) -> Option<u32> {
        let slot = self.lookup(id)?;
        self.unit_group.get(slot).copied().flatten()
    }

    /// `AllocateUnitList` + `AssignUnitList` для лидера: создаёт группу.
    pub fn create_group_with_leader(&mut self, leader: UnitId) -> u32 {
        let group_id = self.groups.len() as u32;
        self.groups.push(Some(vec![leader]));
        if let Some(slot) = self.lookup(leader) {
            if let Some(entry) = self.unit_group.get_mut(slot) {
                *entry = Some(group_id);
            }
        }
        group_id
    }

    /// Приписывает юнита к группе (`UnitInfo::AssignUnitList`).
    pub fn assign_group_member(&mut self, id: UnitId, group_id: u32) {
        if !self.has_unit(id) {
            return;
        }
        let Some(slot) = self.lookup(id) else { return };
        if let Some(entry) = self.unit_group.get_mut(slot) {
            *entry = Some(group_id);
        }
        if let Some(Some(group)) = self.groups.get_mut(group_id as usize) {
            if !group.contains(&id) {
                group.push(id);
            }
        }
    }

    /// `UnitInfo::ClearUnitList` — снимает юнита с группового списка.
    pub fn clear_group_member(&mut self, id: UnitId) {
        let Some(slot) = self.lookup(id) else { return };
        let Some(group_id) = self.unit_group.get(slot).copied().flatten() else {
            return;
        };
        if let Some(entry) = self.unit_group.get_mut(slot) {
            *entry = None;
        }
        if let Some(group) = self.groups.get_mut(group_id as usize) {
            match group {
                Some(members) => {
                    members.retain(|member| *member != id);
                    if members.is_empty() {
                        *group = None;
                    }
                }
                None => {}
            }
        }
    }

    /// Снимает цель из `UnitsManager_DelayedAttackTargets[team]`.
    pub fn remove_delayed_attack_target(&mut self, team: TeamId, id: UnitId) {
        if let Some(targets) = self.delayed_attack_targets.get_mut(team.0 as usize) {
            targets.retain(|unit| *unit != id);
        }
    }

    /// Снимает юнита из `UnitsManager_PendingAttacks`.
    pub fn remove_pending_attack(&mut self, id: UnitId) {
        self.pending_attacks.retain(|unit| *unit != id);
    }

    /// Юниты одного из пяти списков в порядке обхода (`UnitsManager_*Units`).
    #[inline]
    pub fn unit_list(&self, kind: UnitListKind) -> &[UnitId] {
        &self.lists[kind.index()]
    }

    /// Все живые юниты в каноническом порядке обхода пяти списков.
    pub fn unit_ids_in_order(&self) -> Vec<UnitId> {
        let mut result = Vec::with_capacity(self.unit_count());
        for kind in UnitListKind::ALL {
            result.extend_from_slice(self.unit_list(kind));
        }
        result
    }

    /// Все живые юниты для дайджеста: пять списков в каноническом порядке,
    /// затем юниты вне списков (в C++ такие юниты не обрабатываются, но в
    /// состоянии они есть) в порядке слотов.
    fn snapshot_unit_ids(&self) -> Vec<UnitId> {
        let mut result = self.unit_ids_in_order();
        let mut seen: Vec<UnitId> = result.clone();

        for slot in &self.units {
            let Some(unit) = slot else { continue };
            if !seen.contains(&unit.id) {
                seen.push(unit.id);
                result.push(unit.id);
            }
        }

        result
    }

    /// Индекс юнита в его списке (`None` — не в списке).
    pub fn list_position(&self, id: UnitId) -> Option<usize> {
        let kind = self.unit(id)?.unit_list;
        self.lists[kind.index()].iter().position(|item| *item == id)
    }

    // -----------------------------------------------------------------------
    // Списки юнитов: C++ `UnitInfo::AddToDrawList` / `RemoveUnitFromUnitLists`
    // -----------------------------------------------------------------------

    /// `UnitInfo::GetDrawLayer` — слой отрисовки ground cover (`unitinfo.cpp:1191`).
    pub fn draw_layer(unit_type: UnitType) -> i32 {
        use UnitType::*;
        match unit_type {
            Torpedo | TorpedoBubble => 1,
            WaterPlatform => 2,
            Road | Bridge | DeadWaldo | LargeSlab | SmallSlab => 3,
            SmallRubble | LargeRubble => 4,
            LandMine | SeaMine => 5,
            _ => 6,
        }
    }

    /// `UnitInfo::AddToDrawList` (`unitinfo.cpp:1230`) — вставка в один из пяти
    /// списков с сохранением порядка отрисовки.
    pub fn add_to_draw_list(&mut self, id: UnitId) {
        self.add_to_draw_list_with(id, 0);
    }

    /// `UnitInfo::AddToDrawList(override_flags)` — вариант с переопределением
    /// флагов (C++ использует его при подъёме моста).
    pub fn add_to_draw_list_with(&mut self, id: UnitId, override_flags: u32) {
        let Some(unit) = self.unit(id).cloned() else {
            return;
        };
        let flags = if override_flags != 0 {
            override_flags
        } else {
            unit.flags
        };
        use crate::enums::unit_flags::{
            EXPLODING, GROUND_COVER, HOVERING, MISSILE_UNIT, MOBILE_AIR_UNIT, MOBILE_LAND_UNIT,
            MOBILE_SEA_UNIT, STATIONARY,
        };

        let (kind, position) = if flags & (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT) != 0 {
            // `PushFront`.
            (UnitListKind::MobileLandSea, Some(0))
        } else if flags & STATIONARY != 0 {
            if flags & GROUND_COVER != 0 {
                let layer = Self::draw_layer(unit.unit_type);
                let position = self.lists[UnitListKind::GroundCover.index()]
                    .iter()
                    .position(|other| {
                        self.unit(*other)
                            .map(|other| Self::draw_layer(other.unit_type) >= layer)
                            .unwrap_or(true)
                    })
                    .unwrap_or_else(|| self.lists[UnitListKind::GroundCover.index()].len());
                (UnitListKind::GroundCover, Some(position))
            } else {
                let reference_y = unit.y;
                let position = self.lists[UnitListKind::Stationary.index()]
                    .iter()
                    .position(|other| {
                        self.unit(*other)
                            .map(|other| other.y >= reference_y)
                            .unwrap_or(true)
                    })
                    .unwrap_or_else(|| self.lists[UnitListKind::Stationary.index()].len());
                (UnitListKind::Stationary, Some(position))
            }
        } else if flags & MOBILE_AIR_UNIT != 0 {
            let position = self.lists[UnitListKind::MobileAir.index()]
                .iter()
                .position(|other| {
                    self.unit(*other)
                        .map(|other| other.flags & HOVERING != 0)
                        .unwrap_or(true)
                })
                .unwrap_or_else(|| self.lists[UnitListKind::MobileAir.index()].len());
            (UnitListKind::MobileAir, Some(position))
        } else if flags & MISSILE_UNIT != 0 {
            if flags & GROUND_COVER != 0 {
                (UnitListKind::GroundCover, Some(0))
            } else {
                let position = self.lists[UnitListKind::Particle.index()]
                    .iter()
                    .position(|other| {
                        self.unit(*other)
                            .map(|other| {
                                other.flags & EXPLODING != 0
                                    && other.unit_type != UnitType::RocketSmoke
                            })
                            .unwrap_or(true)
                    })
                    .unwrap_or_else(|| self.lists[UnitListKind::Particle.index()].len());
                (UnitListKind::Particle, Some(position))
            }
        } else {
            // Ни один класс не подошёл: в C++ юнит остаётся без списка.
            (UnitListKind::Particle, None)
        };

        if let Some(position) = position {
            let list = &mut self.lists[kind.index()];
            let position = position.min(list.len());
            list.insert(position, id);
            self.set_unit_list_kind(id, kind);
        }
    }

    /// Снимает юнита из конкретного списка (`SmartList::Remove`).
    pub fn remove_from_list(&mut self, kind: UnitListKind, id: UnitId) -> bool {
        let list = &mut self.lists[kind.index()];
        match list.iter().position(|item| *item == id) {
            Some(position) => {
                list.remove(position);
                true
            }
            None => false,
        }
    }

    fn set_unit_list_kind(&mut self, id: UnitId, kind: UnitListKind) {
        if let Some(unit) = self.unit_mut(id) {
            unit.unit_list = kind;
        }
    }

    /// `UnitsManager_RemoveUnitFromUnitLists` (`units_manager.cpp:2332`).
    pub fn remove_unit_from_unit_lists(&mut self, id: UnitId) {
        let Some(unit) = self.unit(id).cloned() else {
            return;
        };
        use unit_flags::*;
        let remove = |list: &mut Vec<UnitId>| {
            if let Some(position) = list.iter().position(|item| *item == id) {
                list.remove(position);
                true
            } else {
                false
            }
        };
        if unit.flags & GROUND_COVER != 0 {
            if !remove(&mut self.lists[UnitListKind::GroundCover.index()]) {
                if unit.unit_type == UnitType::Bridge {
                    remove(&mut self.lists[UnitListKind::Stationary.index()]);
                }
            }
        } else if unit.is_sea() || unit.is_ground() || unit.is_amphibious() {
            remove(&mut self.lists[UnitListKind::MobileLandSea.index()]);
        } else if !unit.is_mobile() {
            remove(&mut self.lists[UnitListKind::Stationary.index()]);
        } else if unit.is_air() {
            remove(&mut self.lists[UnitListKind::MobileAir.index()]);
        } else if unit.flags & MISSILE_UNIT != 0 {
            remove(&mut self.lists[UnitListKind::Particle.index()]);
        }
    }

    /// `UnitInfo::MoveToFrontInUnitList` (`unitinfo.cpp:1926`) — перенос в
    /// начало текущего списка.
    pub fn move_to_front_in_unit_list(&mut self, id: UnitId) {
        let Some(kind) = self.unit(id).map(|unit| unit.unit_list) else {
            return;
        };
        let list = &mut self.lists[kind.index()];
        if let Some(position) = list.iter().position(|item| *item == id) {
            list.remove(position);
            list.insert(0, id);
        }
    }

    // -----------------------------------------------------------------------
    // Спавн и уничтожение (P3-подмножество, нужное приказам)
    // -----------------------------------------------------------------------

    /// Выделяет следующий `UnitId` (`UnitsManager_DeployUnit`, `++UnitIdCounter`).
    fn allocate_unit_id(&mut self, team: TeamId) -> UnitId {
        self.unit_id_counter = self.unit_id_counter.wrapping_add(1);
        let index = (self.unit_id_counter & 0x1FFF_u32) as u16;
        UnitId::new(team, index)
    }

    /// Спавн юнита — `UnitsManager_DeployUnit` (`units_manager.cpp:2354`) в
    /// объёме P4: id, счётчики имён, стартовые приказы по типу, атрибуты,
    /// позиция и список.
    ///
    /// `flags` — дополнительные флаги вызывающего слоя; они объединяются с
    /// флагами типа из `SC_UNITS` ([`crate::access::unit_type_flags`]), как
    /// `UnitsManager_DeployUnit` берёт флаги из данных юнита.
    pub fn spawn_unit(
        &mut self,
        unit_type: UnitType,
        team: TeamId,
        grid: Point,
        flags: u32,
    ) -> Option<UnitId> {
        let id = self.allocate_unit_id(team);
        let team_index = team.0 as usize;
        let mut unit = UnitInfo::new(unit_type, team, id);
        // Флаги типа — из таблицы `SC_UNITS` (`Unit::m_flags`); явные флаги
        // вызывающего слоя объединяются с ними (P3-совместимый вход).
        // C++-код ниже (`DeployUnit`, `UnitInfo::SetPosition`) читает уже
        // собранные флаги `unit->flags`, поэтому дальше используется
        // `unit_flags`, а не только входной `flags`.
        //
        // Биты цвета команды (`HASH_TEAM_*`) в C++ добавляет конструктор
        // `UnitInfo` (`unitinfo.cpp:179`). Порт их в `flags` не пишет: набор
        // флагов входит в канонический дайджест состояния, и эталонный
        // C++-вектор (`tools/sim_vectors`) собран без них. Цвет команды
        // восстанавливается тем же отображением слот→цвет из `unit.team`
        // (`max_app::overlays::team_color_index`), а если юнит пришёл из сейва
        // с уже выставленными битами — они honoured.
        let merged_flags = flags | crate::access::unit_type_flags(unit_type);
        unit.flags = merged_flags;

        // `UnitsManager_DeployUnit` (`units_manager.cpp:2443-2450`): у башен
        // `UpdateTurretAngle(angle)`, у вращающейся башни `image_index_max`
        // считается по кадрам **башни**, а не корпуса.
        if merged_flags & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0 {
            unit.turret_image_index = unit.turret_image_base.wrapping_add(i16::from(unit.angle));
            if merged_flags & unit_flags::SPINNING_TURRET != 0 {
                let frames = crate::frames::frame_info(unit_type);
                unit.image_index_max = unit.turret_image_base + frames.turret_image_count - 1;
            }
        }

        // `UnitInfo::UnitInfo`: атрибуты из current values команды.
        if let Some(values) = self
            .team_units
            .get(team_index)
            .map(|units| *units.current_unit_values(unit_type as u16))
        {
            unit.base_values = values;
        }
        let base = unit.base_values;
        unit.hits = base.get_attribute(ATTRIB_HITS) as u16;
        unit.speed = base.get_attribute(ATTRIB_SPEED) as u16;
        unit.shots = base.get_attribute(ATTRIB_ROUNDS) as u8;
        unit.move_and_fire = base.get_attribute(ATTRIB_MOVE_AND_FIRE) as u8;
        unit.ammo = base.get_attribute(ATTRIB_AMMO) as u8;
        unit.storage = base.get_attribute(ATTRIB_STORAGE) as i16;
        unit.engine = 2;
        unit.weapon = if base.get_attribute(ATTRIB_ATTACK) != 0 {
            2
        } else {
            0
        };

        // `UnitsManager_DeployUnit` (`units_manager.cpp:2382-2384`): у нового
        // юнита склад пуст — `if (GameManager_GameState != GAME_STATE_12_DEPLOYING_UNITS
        // && !QuickBuild_MenuActive && !is_existing_unit) unit->storage = 0;`.
        // Раньше порт оставлял `ATTRIB_STORAGE`, и только что построенный
        // `ADUMP`/`FDUMP`/`GOLDSM` появлялся полным — «бесплатное» сырьё
        // попадало в котёл комплекса. При высадке (`DeployingUnits`) C++
        // склад не обнуляет; конкретные грузы задаёт
        // `GameManager_ProcessTeamMissionSupplyUnits` (`deploy.rs`).
        if self.game_state != GameStateType::DeployingUnits {
            unit.storage = 0;
        }

        // `UnitInfo::InitStealthStatus` (`unitinfo.cpp:2796`): мины, коммандос
        // и подлодки невидимы всем; остальные видны только при
        // `GameManager_AllVisible`, а своя команда видит всегда. Раньше здесь
        // все обычные юниты помечались видимыми **всем** командам — из-за
        // этого на карте и миникарте были видны чужие базы без разведки.
        crate::combat::init_stealth_status(&mut unit, self.ai.all_visible);

        // `UnitsManager_DeployUnit`: счётчик имён типа в команде.
        if let Some(team_info) = self.teams.get_mut(team_index) {
            let counter = team_info.unit_counter(unit_type as u16);
            unit.unit_id = counter as u8;
            if let Some(slot) = team_info.unit_counters.get_mut(unit_type as usize) {
                *slot = slot.wrapping_add(1);
            }
        }

        if unit_type == UnitType::Infiltrator {
            unit.experience = base.get_attribute(ATTRIB_AGENT_ADJUST) as i16;
        }

        if merged_flags & unit_flags::ANIMATED != 0 {
            // `UnitInfo::UnitInfo` (`unitinfo.cpp:288-294`) читает `image_index`
            // из кадровых данных типа и `UnitsManager_DeployUnit`
            // (`units_manager.cpp:2390`) переводит `ANIMATED` в
            // `ORDER_EXPLODE/ORDER_STATE_DESTROY`. Флаг берётся из **итоговых**
            // флагов: вызывающий слой (`UnitsManager_StartExplosion`) передаёт
            // только класс (MOBILE_LAND/…), а `ANIMATED` приходит из `SC_UNITS`.
            unit.orders = UnitOrder::Explode;
            unit.state = UnitOrderState::Destroy;
        }

        // Стартовые приказы по типу (`UnitsManager_DeployUnit`, switch).
        match unit_type {
            UnitType::SeaTransport
            | UnitType::AirTransport
            | UnitType::ArmouredPersonnelCarrier => {
                unit.storage = 0;
            }
            UnitType::GoldRefinery | UnitType::Habitat | UnitType::MiningStation => {
                unit.orders = UnitOrder::PowerOn;
                unit.state = UnitOrderState::Init;
                unit.prior_orders = UnitOrder::PowerOn;
                unit.prior_state = UnitOrderState::Init;
            }
            UnitType::LandMine
            | UnitType::SeaMine
            | UnitType::GunTurret
            | UnitType::AntiAir
            | UnitType::Artillery
            | UnitType::MissileLauncher
            | UnitType::Radar => {
                unit.orders = UnitOrder::Sentry;
                unit.state = UnitOrderState::ExecutingOrder;
            }
            UnitType::PowerStation | UnitType::PowerGenerator | UnitType::ResearchCentre => {
                unit.orders = UnitOrder::PowerOff;
                unit.state = UnitOrderState::ExecutingOrder;
            }
            UnitType::Barracks | UnitType::Depot | UnitType::Hangar | UnitType::Dock => {
                unit.storage = 0;
            }
            _ => {}
        }

        if team == TeamId::ALIEN && merged_flags & unit_flags::REGENERATING_UNIT != 0 {
            unit.orders = UnitOrder::Disable;
        }

        unit.target_grid = grid;
        unit.grid = grid;
        unit.x = (grid.x as i32 * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2) as u16;
        unit.y = (grid.y as i32 * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2) as u16;
        if merged_flags & unit_flags::BUILDING != 0 {
            unit.x = unit.x.wrapping_add((GFX_MAP_TILE_SIZE / 2 - 1) as u16);
            unit.y = unit.y.wrapping_add((GFX_MAP_TILE_SIZE / 2 - 1) as u16);
        }

        // `UnitInfo::UnitInfo` (`unitinfo.cpp:316`): здание сразу получает
        // **свой** комплекс — `AttachComplex(CreateComplex(team))`, а
        // `Complex::Grow` ставит `buildings = 1`. Без этого у шахты не было
        // `unit->GetComplex()`, и диалог `allocate` (`AllocMenu`) не открывался,
        // а `Access_UpdateResourcesTotal` не видел добычу комплекса.
        // Слияние с соседями по коннекторам делает
        // [`crate::complex::update_connectors`] ниже — как
        // `UnitsManager_UpdateConnectors` после завершения масштабирования
        // здания (`units_manager.cpp:3166`).
        if unit.is_complex_member() {
            unit.complex = self.attach_new_complex(team);
        }

        // Слот: свободный (LIFO, как переиспользование `SmartObjectArray`) или новый.
        let slot = match self.free_slots.pop() {
            Some(slot) => {
                self.units[slot as usize] = Some(unit);
                self.order_data[slot as usize] = OrderData {
                    move_to: grid,
                    fire_on: Point::ZERO,
                };
                if let Some(entry) = self.unit_group.get_mut(slot as usize) {
                    *entry = None;
                }
                slot as usize
            }
            None => {
                self.units.push(Some(unit));
                self.order_data.push(OrderData {
                    move_to: grid,
                    fire_on: Point::ZERO,
                });
                self.unit_group.push(None);
                self.units.len() - 1
            }
        };
        // `Hash_UnitHash.PushBack(unit)` — ведёрко по `id % hash_size`.
        self.unit_hash.push_at(u32::from(id.0), slot as u16);
        // `Hash_MapHash.Add` (`units_manager.cpp`): клетка (у зданий — 2×2).
        self.map_hash
            .add(id, grid.x as u16, grid.y as u16, merged_flags, false);

        self.add_to_draw_list(id);

        // `Access_UpdateMapStatus(unit, true)` — покрытие сканирования (P6).
        self.update_map_status(id, true);

        // `UnitsManager_UpdateConnectors(unit)` (`units_manager.cpp:2051`):
        // здание ищет соседей по восьми направлениям, сливает комплексы
        // (`AttachToPrimaryComplex`) и пересчитывает запасы. В C++ вызов идёт
        // по завершении `ORDER_AWAIT_SCALING`/`EXPAND`; в порте здание уже стоит
        // на карте, поэтому коннекторы считаются сразу — иначе завод до конца
        // анимации не видел бы материалов комплекса. Здание в `ORDER_IDLE`
        // (готовое, но ещё не раскрытое) в комплекс не входит: её приказ
        // восстановит `UnitsManager_BuildingReady`, и тогда `update_connectors`
        // вызовется из `process_order_await_scaling`.
        if self
            .unit(id)
            .is_some_and(|unit| unit.is_complex_member() && unit.orders != UnitOrder::Idle)
        {
            crate::complex::update_connectors(self, id);
        }

        Some(id)
    }

    /// `UnitInfo::AttachComplex` (`unitinfo.cpp:1978`) в объёме модели:
    /// новый комплекс команды (`TeamUnits::CreateComplex`) + `Complex::Grow`.
    ///
    /// Возвращает id комплекса; `None`, если у команды нет слота.
    fn attach_new_complex(&mut self, team: TeamId) -> Option<crate::team::ComplexId> {
        let team_units = self.team_units.get_mut(team.0 as usize)?;
        let id = team_units.create_complex();
        let complex = team_units.get_complex_mut(id)?;
        complex.grow();
        Some(id)
    }

    /// `UnitsManager_RemoveConnections` (`units_manager.cpp:1556`) в объёме
    /// модели: снятие парных битов у соседей и разделение комплекса
    /// (`UnitInfo::DetachComplex`).
    ///
    /// C++ вызывает `RemoveConnections` из `UnitsManager_BuildClearing` перед
    /// `DestroyUnit`; в порте уничтожение идёт через [`Self::destroy_unit`],
    /// поэтому разделение комплекса делается здесь же.
    fn remove_connections(&mut self, id: UnitId) {
        crate::complex::remove_connections(self, id);
    }

    /// Уничтожение юнита — подмножество `UnitsManager_DestroyUnit`
    /// (`units_manager.cpp`), достаточное циклу P4: снятие со списков, из
    /// хэшей, из атак и событий, покрытия сканирования и задач ИИ.
    ///
    /// TODO(P3): комплексы, грузы, следы/обломки, статистика потерь.
    pub fn destroy_unit(&mut self, id: UnitId) {
        let Some(slot) = self.lookup(id) else {
            return;
        };
        let placement = self.unit(id).map(|unit| (unit.grid, unit.flags));
        // Атаку завершаем до удаления из хэша (`UnitsManager_EndAttack` ищет
        // юнита по id).
        self.end_attack(id);

        // `Access_UpdateMapStatus(unit, false)` и `Ai_RemoveUnit` (P6).
        self.update_map_status(id, false);
        crate::ai::ai_remove_unit(self, id);

        // `UnitsManager_RemoveConnections` → `DetachComplex` (`units_manager.cpp:1569`):
        // здание покидает комплекс, оставшиеся связные группы получают свои
        // комплексы, у соседей снимаются биты коннекторов.
        if let Some(unit) = self.unit(id) {
            if unit.is_complex_member() {
                self.remove_connections(id);
            }
        }

        self.clear_group_member(id);
        self.remove_unit_from_unit_lists(id);
        self.unit_hash.remove_at(u32::from(id.0), &(slot as u16));
        if let Some((grid, flags)) = placement {
            self.map_hash
                .remove(id, grid.x as u16, grid.y as u16, flags);
        }
        if let Some(entry) = self.units.get_mut(slot) {
            *entry = None;
        }
        if let Some(entry) = self.unit_group.get_mut(slot) {
            *entry = None;
        }
        self.free_slots.push(slot as u16);
        self.pending_attacks.retain(|unit| *unit != id);
        self.pending_air_group_leader = self.pending_air_group_leader.filter(|unit| *unit != id);
        self.unit_events.retain(|event| event.unit() != id);
        self.path_requests.retain(|request| request.unit != id);
        for targets in &mut self.delayed_attack_targets {
            targets.retain(|unit| *unit != id);
        }
        for active in &mut self.units_active {
            if *active == Some(id) {
                *active = None;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Приказы: межъюнитные операции (C++ `UnitsManager_SetNewOrderInt` и др.)
    // -----------------------------------------------------------------------

    /// `UnitsManager_SetNewOrderInt` для произвольного юнита
    /// (`units_manager.cpp:1945`); возвращает `true`, если снят запрос пути.
    ///
    /// Новый приказ **не** отменяет стройку: в C++ строителя нельзя увести
    /// кликом с недостроенной площадки (`CURSOR_UNIT_NO_GO`,
    /// `game_manager.cpp:5927-5931`), а отмена идёт только явным `stop`
    /// (`ORDER_BUILD`/`BUILD_CANCEL` → `UnitInfo::CancelBuilding`).
    pub fn set_order(&mut self, id: UnitId, order: UnitOrder, state: UnitOrderState) -> bool {
        let Some(mut unit) = self.unit(id).cloned() else {
            return false;
        };
        // `UnitsManager_NewOrderWhileScaling` (`units_manager.cpp:2529`): если
        // приказ меняется на этапе `EXPAND`, C++ сразу зовёт
        // `UnitsManager_UpdateConnectors`, не дожидаясь конца анимации.
        let connectors_due =
            unit.orders == UnitOrder::AwaitScaling && unit.state == UnitOrderState::Expand;
        let delete_path = unit_order::set_new_order_int(&mut unit, order, state);
        if delete_path {
            self.remove_path_request(id);
        }
        self.write_unit(id, unit);
        if connectors_due {
            crate::complex::update_connectors(self, id);
        }
        delete_path
    }

    /// `UnitsManager_ScaleUnit` (`units_manager.cpp:1847`).
    pub fn scale_unit(&mut self, id: UnitId, state: UnitOrderState) -> bool {
        let Some(mut unit) = self.unit(id).cloned() else {
            return false;
        };
        let result = unit_order::scale_unit(&mut unit, state);
        self.write_unit(id, unit);
        result
    }

    /// `UnitsManager_EndAttack` (`units_manager.cpp:2568`).
    pub fn end_attack(&mut self, id: UnitId) {
        let Some(unit) = self.unit(id) else {
            return;
        };
        let attack = unit.live_attack;
        if attack == 0 {
            return;
        }
        if let Some(position) = self
            .live_attacks
            .iter()
            .position(|entry| entry.id == attack)
        {
            let targets = self.live_attacks.remove(position).targets;
            for target in targets {
                if let Some(target_unit) = self.unit_mut(target) {
                    // C++ `--pin_count` без насыщения; возможный «минус» —
                    // точное поведение оракула (DESIGN §6.3).
                    target_unit.pin_count = target_unit.pin_count.wrapping_sub(1);
                }
            }
        }
        if let Some(unit) = self.unit_mut(id) {
            unit.live_attack = 0;
        }
    }

    /// `UnitsManager_PinAttackTargets` (`units_manager.cpp:2542`).
    ///
    /// Захват целей — `Access_GetAreaAttackTargets` для площадной атаки,
    /// иначе `Access_GetAttackTarget` по `fire_on`. Каждая цель получает
    /// `pin_count += 1`; она не может сойти с клетки/карты до `EndAttack`.
    pub fn pin_attack_targets(&mut self, id: UnitId) {
        self.end_attack(id);
        self.live_attack_counter = self.live_attack_counter.wrapping_add(1);
        let attack = self.live_attack_counter;
        if let Some(unit) = self.unit_mut(id) {
            unit.live_attack = attack;
        }

        let Some(unit) = self.unit(id).cloned() else {
            return;
        };
        let fire_on = self.order_data(id).fire_on;
        let targets = if unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_ATTACK_RADIUS)
            != 0
        {
            let targets: Vec<UnitId> = crate::combat::get_area_attack_targets(self, &unit, fire_on)
                .into_iter()
                .map(|(target, _)| target)
                .collect();
            targets
        } else {
            crate::access::get_attack_target(self, &unit, fire_on, false)
                .into_iter()
                .collect()
        };

        for target in &targets {
            if let Some(target_unit) = self.unit_mut(*target) {
                target_unit.pin_count = target_unit.pin_count.wrapping_add(1);
            }
        }

        self.live_attacks.push(LiveAttack {
            id: attack,
            targets,
        });
    }

    /// `UnitsManager_AddToDelayedReactionList` (`units_manager.cpp:3435`).
    pub fn add_to_delayed_reaction_list(&mut self, id: UnitId) {
        // Настройка `disable_fire` выключает реактивный огонь целиком.
        if self.disable_fire {
            return;
        }

        let Some(unit) = self.unit(id) else {
            return;
        };
        let team = unit.team;
        if let Some(targets) = self.delayed_attack_targets.get_mut(team.0 as usize) {
            if !targets.contains(&id) {
                targets.push(id);
            }
        }
        self.delayed_reactions_pending = true;
    }

    /// `UnitsManager_ClearDelayedReactions` (`units_manager.cpp:3456`) —
    /// сброс флага, `delayed_reaction` живых юнитов и всех списков целей.
    pub fn clear_delayed_reactions(&mut self) {
        self.delayed_reactions_pending = false;

        for kind in [
            UnitListKind::MobileLandSea,
            UnitListKind::MobileAir,
            UnitListKind::Stationary,
        ] {
            let ids = self.lists[kind.index()].clone();
            for id in ids {
                let Some(unit) = self.unit(id) else { continue };
                if unit.delayed_reaction != 0 && unit.hits > 0 {
                    let mut unit = unit.clone();
                    unit.delayed_reaction = 0;
                    self.write_unit(id, unit);
                }
            }
        }

        for targets in &mut self.delayed_attack_targets {
            targets.clear();
        }
    }

    // -----------------------------------------------------------------------
    // Обход пяти списков (DESIGN §5.2, `units_manager.cpp:1860`)
    // -----------------------------------------------------------------------

    /// `UnitsManager_ProcessOrders` (`units_manager.cpp:1860`) — один проход по
    /// пяти спискам в фиксированном порядке.
    pub fn process_orders(&mut self) {
        self.bob_effect_quota = 5;

        // `Ai_ClearTasksPendingFlags` (`ai.cpp:273`).
        crate::ai::ai_clear_tasks_pending_flags(self);

        // `for team: UnitsManager_Units[team] = nullptr`.
        self.units_active = [None; TEAM_SLOTS];

        self.orders_pending = false;
        self.combat_effects_active = false;

        for kind in UnitListKind::ALL {
            self.process_unit_orders(kind);
        }

        // В C++ здесь `Remote_ProcessNetPackets()`; в max_core очередь команд
        // применяется в начале тика (см. `tick`), поэтому pass.

        if !self.orders_pending {
            self.process_pending_attacks();
        }

        // `UnitsManager_PendingAirGroupLeader` → `Access_ProcessNewGroupOrder`:
        // лидер остаётся в поле, пока путь группы не разрешён (C++).
        if let Some(leader) = self.pending_air_group_leader {
            if crate::access::process_new_group_order(self, leader) {
                self.pending_air_group_leader = None;
            }
        }

        // `UnitEvent_UnitEvents`: Process() каждого, затем Clear().
        let events = std::mem::take(&mut self.unit_events);
        for event in &events {
            crate::events::process_unit_event(self, event);
        }
        self.unit_events.clear();

        self.reset_bob_state = self.bob_effect_quota > 0;
    }

    /// `UnitsManager_ProcessUnitOrders` — обход одного списка.
    ///
    /// C++ итерирует `SmartList` от `Begin()` до зафиксированного `End()`;
    /// здесь — снимок id списка на начало обхода. Юниты, удалённые во время
    /// обхода, пропускаются (как и в C++: `SmartList`-итератор после
    /// `Remove` текущего узла не продолжают), добавленные в начало/конец
    /// списка — не посещаются в этом проходе.
    pub fn process_unit_orders(&mut self, kind: UnitListKind) {
        let ids = self.lists[kind.index()].clone();
        for id in ids {
            if self.has_unit(id) {
                unit_order::process_order(self, id);
            }
        }
    }

    /// Голова `UnitsManager_PendingAttacks` (`units_manager.cpp:1884-1930`).
    fn process_pending_attacks(&mut self) {
        if self.pending_attacks.is_empty() {
            // `UnitsManager_AssessAttacks()` — проверка отложенных реакций.
            crate::combat::assess_attacks(self);
            return;
        }

        let id = self.pending_attacks[0];
        let Some(unit) = self.unit(id).cloned() else {
            self.pending_attacks.remove(0);
            return;
        };

        if unit.hits == 0 || unit.state == UnitOrderState::Destroy {
            // Атакующий умирает — `DestroyUnit` завершает его атаку.
            self.pending_attacks.remove(0);
        } else if unit.orders != UnitOrder::Fire {
            // Анимация попадания лишь приостанавливает огонь; любой другой
            // приказ атаку отменяет.
            if unit.orders != UnitOrder::Explode || unit.prior_orders != UnitOrder::Fire {
                self.end_attack(id);
                self.pending_attacks.remove(0);
            }
        } else if unit.shots > 0 {
            if let Some(unit) = self.unit_mut(id) {
                unit.state = UnitOrderState::AttackBeginning;
            }
            self.pending_attacks.remove(0);
        } else {
            self.end_attack(id);
            let mut unit = unit;
            unit_order::restore_orders(&mut unit);
            if unit.orders == UnitOrder::Fire {
                // Ошибка: отложенным приказом был огонь.
                unit.orders = UnitOrder::Await;
                unit.state = UnitOrderState::ExecutingOrder;
            }
            self.write_unit(id, unit);
            self.pending_attacks.remove(0);
        }
    }

    // -----------------------------------------------------------------------
    // Конец хода (DESIGN §5.3, `game_manager.cpp:2042-2087`)
    // -----------------------------------------------------------------------

    /// `GameManager_ManageEconomy` — обновление производства, снабжения и
    /// исследований команды на начало её хода.
    pub fn manage_economy(&mut self, team: TeamId) {
        // `team_units->OptimizeComplexes(team)` (`teamunits.cpp:170`):
        // `Access_UpdateResourcesTotal(complex)` — запасы комплекса (включая
        // добычу шахт по их `raw/fuel/gold_mining`) пересчитываются на начало
        // хода; затем `ProductionManager_OptimizeProduction` — включение
        // производителей энергии, отключение потребителей при дефиците и
        // пауза строек (`production_manager.cpp:830-901`).
        let complexes: Vec<crate::team::ComplexId> = self
            .team_units
            .get(team.0 as usize)
            .map(|units| {
                units
                    .complexes
                    .iter()
                    .map(|complex| complex.complex_id())
                    .collect()
            })
            .unwrap_or_default();

        self.update_complex_resources(team);

        for complex in &complexes {
            let _ = crate::power::optimize_complex(self, team, *complex, None, None, false);
        }

        for kind in [
            UnitListKind::MobileLandSea,
            UnitListKind::Stationary,
            UnitListKind::MobileAir,
        ] {
            let ids = self.lists[kind.index()].clone();
            for id in ids {
                let Some(unit) = self.unit(id) else { continue };
                if unit.team != team {
                    continue;
                }
                let mut unit = unit.clone();
                self.update_production(&mut unit);
                self.write_unit(id, unit);
            }
        }

        for kind in [
            UnitListKind::MobileLandSea,
            UnitListKind::Stationary,
            UnitListKind::MobileAir,
        ] {
            let ids = self.lists[kind.index()].clone();
            for id in ids {
                let Some(unit) = self.unit(id) else { continue };
                if unit.team != team || unit.orders == UnitOrder::Disable {
                    continue;
                }
                let mut unit = unit.clone();
                self.resupply(&mut unit);
                self.write_unit(id, unit);
            }
        }

        // Повторная оптимизация здесь **не** нужна: `update_production` не
        // сбрасывает паузу стационарных заводов (её решение принято
        // `ProductionManager_OptimizeProduction` выше), а
        // `Access_UpdateResourcesTotal` после раздачи складов вернул бы кэш
        // комплекса к «инвентарь + добыча» (C++ `ManageEconomy` этого не делает).
        if let Some(team_info) = self.teams.get_mut(team.0 as usize) {
            for index in 0..RESEARCH_TOPIC_COUNT {
                let topic = team_info.research_topics[index];
                if topic.allocation != 0 && topic.turns_to_complete == 0 {
                    // `ResearchMenu_UpdateResearchProgress(team, i, 0)`:
                    // посчитать ходы для нового уровня (C++ `manage_economy`).
                    crate::research::update_research_progress(
                        &mut team_info.research_topics[index],
                        crate::research::RESEARCH_TOPIC_FACTORS[index],
                        0,
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Исследования (P6: `researchmenu.cpp` / `upgradecontrol.cpp`)
    // -----------------------------------------------------------------------

    /// `ResearchMenu_UpdateResearchProgress(team, topic, allocation)` —
    /// изменить число исследовательских центров темы и пересчитать прогресс.
    pub fn update_research_progress(
        &mut self,
        team: TeamId,
        topic: ResearchTopic,
        allocation: i32,
    ) {
        let index = topic as usize;
        if index >= RESEARCH_TOPIC_COUNT {
            return;
        }
        let Some(team_info) = self.teams.get_mut(team.0 as usize) else {
            return;
        };
        crate::research::update_research_progress(
            &mut team_info.research_topics[index],
            crate::research::RESEARCH_TOPIC_FACTORS[index],
            allocation,
        );
    }

    /// Текущее распределение исследовательских центров по темам (C++
    /// `TeamInfo::research_topics[i].allocation`).
    pub fn research_allocation(&self, team: TeamId) -> [u32; RESEARCH_TOPIC_COUNT] {
        let mut result = [0u32; RESEARCH_TOPIC_COUNT];
        if let Some(info) = self.teams.get(team.0 as usize) {
            for (index, topic) in info.research_topics.iter().enumerate() {
                result[index] = topic.allocation;
            }
        }
        result
    }

    /// `ResearchMenu_OnClick_Done` (`researchmenu.cpp:518`): применить новое
    /// распределение центров по темам.
    ///
    /// Для каждого включённого `RESEARCH`-центра: если в целевом распределении
    /// для его темы есть место — оставить; иначе перевести на первую тему с
    /// местом (обновив allocation `-1/+1`), а если мест нет — выключить
    /// (`ORDER_POWER_OFF`/`INIT`, как C++).
    ///
    /// Возвращает фактическое распределение (C++ не гарантирует сумму = числу
    /// центров, если цель больше числа центров).
    pub fn set_research_allocation(
        &mut self,
        team: TeamId,
        target: [u32; RESEARCH_TOPIC_COUNT],
    ) -> [u32; RESEARCH_TOPIC_COUNT] {
        let mut remaining = target;
        let ids = self.lists[UnitListKind::Stationary.index()].clone();

        for id in ids {
            let Some(unit) = self.unit(id).cloned() else {
                continue;
            };
            if unit.team != team
                || unit.unit_type != UnitType::ResearchCentre
                || unit.orders != UnitOrder::PowerOn
            {
                continue;
            }

            let old_index = unit.research_topic as usize;
            if old_index < RESEARCH_TOPIC_COUNT && remaining[old_index] > 0 {
                remaining[old_index] -= 1;
                continue;
            }

            let Some(new_index) = remaining.iter().position(|count| *count > 0) else {
                let mut unit = unit;
                unit_order::set_new_order_int(&mut unit, UnitOrder::PowerOff, UnitOrderState::Init);
                self.write_unit(id, unit);
                continue;
            };

            let mut unit = unit;
            if old_index < RESEARCH_TOPIC_COUNT {
                self.update_research_progress(team, unit.research_topic, -1);
            }
            let Some(new_topic) = ResearchTopic::try_from(new_index as u8).ok() else {
                continue;
            };
            unit.research_topic = new_topic;
            self.update_research_progress(team, new_topic, 1);
            remaining[new_index] -= 1;
            self.write_unit(id, unit);
        }

        self.research_allocation(team)
    }

    /// `ResearchMenu_NewTurn(team)` — ход исследований команды: списать
    /// allocation, завершить уровни, применить апгрейды к текущим значениям.
    pub fn advance_research(&mut self, team: TeamId) -> Vec<crate::research::ResearchEvent> {
        let index = team.0 as usize;
        if index >= TEAM_SLOTS {
            return Vec::new();
        }
        let teams = &mut self.teams;
        let team_units = &mut self.team_units;
        crate::research::research_new_turn(&mut team_units[index], &mut teams[index], team)
    }

    /// Забирает события исследований последнего хода (для HUD-лога).
    pub fn take_research_events(&mut self) -> Vec<crate::research::ResearchEvent> {
        std::mem::take(&mut self.research_events)
    }

    /// Забирает звуковые события боя (для `AudioPlayer`).
    ///
    /// Копятся `PrepareFire`/`UnitsManager_StartExplosion`; читает клиент
    /// (`max_app::App::collect_combat_sounds`) сразу после тика.
    pub fn take_unit_sounds(&mut self) -> Vec<UnitSound> {
        std::mem::take(&mut self.unit_sounds)
    }

    /// `UnitInfo::UpdateProduction` (`unitinfo.cpp:2904`) — сброс скоростей,
    /// выстрелов, снабжения и **разбор запаса комплекса по складам зданий** на
    /// начало хода.
    ///
    /// Точный порядок C++:
    /// 1. мобильный строитель в `ORDER_BUILD` платит сырьём
    ///    (`Cargo_GetRawConsumptionRate`), предварительно урезав `build_rate` до
    ///    `BuildMenu_GetMaxPossibleBuildRate`;
    /// 2. `COMMTWR` в `ORDER_POWER_ON` добавляет команде золото
    ///    (`Cargo_GetGoldConsumptionRate` = 5);
    /// 3. `GREENHSE` в `ORDER_POWER_ON` — `++storage` и `++team_points`;
    /// 4. выстрелы/скорость/отключение;
    /// 5. если у юнита есть комплекс — `storage = min(complex.<field>,
    ///    ATTRIB_STORAGE)`, `complex.<field> -= storage`.
    ///
    /// Шаг 5 и есть «добыча шахты → хранилища»: `Access_UpdateResourcesTotal`
    /// (см. [`Self::update_complex_resources`]) кладёт добычу в поле комплекса,
    /// а здесь она раскладывается по складам (`ADUMP`/`FDUMP`/`GOLDSM` и т.д.).
    /// Остаток сверх суммы `ATTRIB_STORAGE` остаётся в поле комплекса и теряется
    /// на следующем пересчёте — как в C++.
    pub fn update_production(&mut self, unit: &mut UnitInfo) {
        let team_index = unit.team.0 as usize;

        // Флаг паузы мобильного строителя пересчитывается каждый ход:
        // `true` ставит только ветка оплаты ниже. Пауза **стационарного**
        // завода принадлежит `ProductionManager::progress` (оптимизатор
        // энергии) и здесь не сбрасывается — иначе решение об отключении
        // терялось бы между `ManageEconomy` и `ProgressBuildState`.
        if unit.is_ground() || unit.is_amphibious() {
            unit.build_paused = false;
        }

        if unit.orders == UnitOrder::Build
            && unit.state != UnitOrderState::UnitReady
            && (unit.is_ground() || unit.is_amphibious())
        {
            let max_rate = crate::production::max_possible_build_rate(
                unit.unit_type,
                i32::from(unit.build_time),
                i32::from(unit.storage),
            );

            if max_rate >= 0 {
                unit.build_rate = unit.build_rate.min(max_rate as u16);
            }

            let target_turns = unit
                .build_list
                .first()
                .and_then(|unit_type| {
                    self.team_units.get(team_index).map(|units| {
                        units
                            .current_unit_values(*unit_type as u16)
                            .get_attribute(ATTRIB_TURNS)
                    })
                })
                .unwrap_or(0);

            let skip_for_ai = self
                .teams
                .get(team_index)
                .is_some_and(|info| info.team_type == TeamKind::Computer)
                && self.ai.settings.opponent >= crate::enums::Opponent::Master as u8
                && target_turns <= 1;

            if !skip_for_ai {
                // C++ `storage -= Cargo_GetRawConsumptionRate(unit_type,
                // build_rate)` (`unitinfo.cpp:2914`) списывает сырьё **всегда**
                // и позволяет складу уйти в минус; `GameManager_SpawnNewUnits`
                // затем всё равно двигает `build_time`, и постройка
                // «достраивается долгом». Порт обязан держать ресурс
                // неотрицательным, поэтому списание идёт только при
                // достаточном остатке, а неоплаченный ход помечается
                // `build_paused` — прогресс такого хода не делает
                // (`production::spawn_units`).
                let cost = cargo_raw_consumption_rate(unit.unit_type, i32::from(unit.build_rate));

                if cost > 0 {
                    if i32::from(unit.storage) >= cost {
                        unit.storage -= cost as i16;
                    } else {
                        unit.build_paused = true;
                    }
                }
            }
        }

        if unit.unit_type == UnitType::GoldRefinery && unit.orders == UnitOrder::PowerOn {
            if let Some(team_units) = self.team_units.get_mut(team_index) {
                let reserves = team_units.gold();
                let income = cargo_gold_consumption_rate(UnitType::GoldRefinery).max(0) as u32;
                team_units.set_gold(reserves.wrapping_add(income));
            }
        }

        if unit.unit_type == UnitType::EcoSphere && unit.orders == UnitOrder::PowerOn {
            unit.storage = unit.storage.wrapping_add(1);

            if let Some(info) = self.teams.get_mut(team_index) {
                info.team_points = info.team_points.wrapping_add(1);
            }
        }

        let base = unit.base_values;
        let rounds = base.get_attribute(ATTRIB_ROUNDS);

        if rounds > 0 {
            if rounds > i32::from(unit.ammo) {
                unit.shots = unit.ammo;
            } else if rounds != i32::from(unit.shots) {
                unit.shots = rounds as u8;
            }
        }

        unit.speed = base.get_attribute(ATTRIB_SPEED) as u16;

        if unit.speed > 0 {
            for spotted in unit.spotted_by_team.iter_mut() {
                *spotted = false;
            }
        }

        if unit.orders == UnitOrder::Disable
            || (unit.orders == UnitOrder::Idle && unit.prior_orders == UnitOrder::Disable)
        {
            if unit.team != TeamId::ALIEN {
                // Отключение, закончившееся на нуле, остаётся нулём.
                if unit.disabled_turns_remaining > 0 {
                    unit.disabled_turns_remaining -= 1;
                }

                if unit.disabled_turns_remaining == 0 {
                    if unit.orders == UnitOrder::Idle {
                        unit.prior_orders = UnitOrder::Await;
                        unit.prior_state = UnitOrderState::ExecutingOrder;
                    } else {
                        unit_order::restore_orders(unit);

                        // Попадание во время отключения оставило DISABLE
                        // отложенным приказом; `RestoreOrders` его сохраняет
                        // для зданий с питанием.
                        if unit.orders == UnitOrder::Disable {
                            unit.prior_orders = UnitOrder::PowerOff;
                            unit.prior_state = UnitOrderState::ExecutingOrder;

                            unit_order::restore_orders(unit);
                        }
                    }
                }
            }
        }

        if unit.state == UnitOrderState::ReadyToExecuteOrder {
            unit.state = UnitOrderState::ExecutingOrder;
        }

        // `if (complex) { storage = min(complex-><field>, ATTRIB_STORAGE);
        // complex-><field> -= storage; }` (`unitinfo.cpp:3004`).
        if let Some(complex) = unit.complex {
            let capacity = unit.base_values.get_attribute(ATTRIB_STORAGE);
            let cargo = cargo_type_of(unit.unit_type);

            let pulled = state_complex_pull(self, unit.team, complex, cargo, capacity);

            if let Some(value) = pulled {
                unit.storage = value as i16;
            }
        }
    }

    /// `UnitInfo::Resupply` (`unitinfo.cpp:2885`) — пополнение боезапаса из
    /// запаса комплекса (`Complex::GetCargoInfo` + `Complex::Transfer(-1,0,0)`)
    /// и регенерация.
    ///
    /// `Regenerate()` (зелёные юниты пришельцев) в модели отсутствует —
    /// TODO, как и в остальном порте.
    pub fn resupply(&mut self, unit: &mut UnitInfo) {
        if unit.is_complex_member()
            && unit.base_values.get_attribute(ATTRIB_ATTACK) > 0
            && unit.base_values.get_attribute(ATTRIB_ROUNDS) > i32::from(unit.ammo)
        {
            let Some(complex) = unit.complex else {
                return;
            };
            let (materials, _) = crate::alloc::get_cargo_info(self, unit.team, complex);

            if materials.raw > 0 {
                unit.ammo = unit.base_values.get_attribute(ATTRIB_AMMO) as u8;
                crate::transfer::complex_transfer(self, unit.team, complex, -1, 0, 0);
            }
        }
    }

    /// `GameManager_UpdateScoreGraph` (`game_manager.cpp:2078`): сдвиг графика
    /// очков на 1 влево, последний элемент — текущие очки команды.
    pub fn update_score_graph(&mut self) {
        for team in 0..PLAYER_TEAM_COUNT {
            let points = self.teams[team].team_points;
            let graph = &mut self.teams[team].score_graph;
            graph.copy_within(1..SCORE_GRAPH_LEN, 0);
            graph[SCORE_GRAPH_LEN - 1] = points as i16;
        }
    }

    /// `GameManager_AreTeamsFinishedTurn` (`game_manager.cpp:2520`).
    pub fn are_teams_finished_turn(&self) -> bool {
        for team in 0..PLAYER_TEAM_COUNT {
            let info = &self.teams[team];
            if info.team_type != TeamKind::None
                && info.team_type != TeamKind::Eliminated
                && !info.finished_turn
            {
                return false;
            }
        }
        true
    }

    /// Общий счётчик барьера конца хода (пакет 52), если **все** играющие
    /// команды прислали одинаковое значение — `Remote_ServerWaitBarrier`
    /// (`max-brh-docs/protocol.md` §8.6: «ждёт, пока все удалённые команды пришлют
    /// одинаковое значение счётчика»).
    ///
    /// `None` — барьер ещё не собран (или команды разошлись).
    pub fn end_turn_barrier(&self) -> Option<u8> {
        self.common_barrier(|state, team| state.turn_indices[team])
    }

    /// Общий счётчик барьера начала хода (пакет 00) — аналог
    /// [`Self::end_turn_barrier`] по `Remote_FrameSyncCounter`
    /// (`Remote_WaitBeginTurnAcknowledge`, `remote.cpp:1876-1933`).
    ///
    /// `None` — не все играющие команды вошли в раунд с одним счётчиком.
    pub fn begin_turn_barrier(&self) -> Option<u8> {
        self.common_barrier(|state, team| state.frame_sync_counter[team])
    }

    /// `Remote_StateSyncIndices[team]` — последний применённый индекс 54.
    #[inline]
    pub fn state_sync_index(&self, team: TeamId) -> u8 {
        self.state_sync_indices
            .get(team.0 as usize)
            .copied()
            .unwrap_or(0)
    }

    /// Общий номер следующего хода CRC-барьера (пакет 45): все играющие
    /// команды согласны. `None` — барьер не собран (десинк/ожидание).
    pub fn crc_barrier(&self) -> Option<u8> {
        self.common_barrier(|state, team| state.next_turn_indices[team])
    }

    /// Значение общего барьера по командам (`None` — нет согласия).
    ///
    /// Команды `None`/`Eliminated` не участвуют (как в `Remote_ServerWaitBarrier`).
    fn common_barrier(&self, value: impl Fn(&Self, usize) -> u8) -> Option<u8> {
        let mut common: Option<u8> = None;

        for team in 0..PLAYER_TEAM_COUNT {
            let kind = self.teams[team].team_type;
            if kind == TeamKind::None || kind == TeamKind::Eliminated {
                continue;
            }

            match common {
                None => common = Some(value(self, team)),
                Some(expected) if expected == value(self, team) => {}
                Some(_) => return None,
            }
        }

        common
    }

    /// `GameManager_IsActiveTurn` (`game_manager.cpp:8296`).
    pub fn is_active_turn(&self, team: TeamId) -> bool {
        self.active_turn_team == team
            || self.play_mode == PlayMode::SimultaneousMoves
            || self.play_mode == PlayMode::Unknown
            || self.teams[team.0 as usize].team_type == TeamKind::Remote
    }

    /// `GameManager_TeamTurnFinish` (`game_manager.cpp:583`) — закрытие хода:
    /// проверка победы/поражения (P7), сброс `finished_turn`, график очков,
    /// счётчик хода.
    ///
    /// Возвращает `true`, если партия завершена
    /// ([`crate::victory::check_end_game_conditions`] — порт
    /// `menu_check_end_game_conditions`): тогда C++ не сбрасывает
    /// `finished_turn` и не увеличивает счётчик хода.
    pub fn finish_turn(&mut self) -> bool {
        if self.game_state != GameStateType::EndTurn {
            return false;
        }
        self.game_state = GameStateType::InGame;

        // `menu_check_end_game_conditions(GameManager_TurnCounter, …, false)`.
        if crate::victory::check_end_game_conditions(self) {
            return true;
        }

        for team in 0..PLAYER_TEAM_COUNT {
            self.teams[team].finished_turn = false;
        }

        self.update_score_graph();
        self.turn_counter += 1;

        // `GameManager_ManageEconomy` → `GameManager_ProgressBuildState` для
        // каждой играющей команды в порядке RED..GRAY (C++ `UpdateGui`): ход
        // производства идёт сразу после смены хода, до ходов ИИ/игрока.
        // Порядок вызовов 1:1: списание стоимости хода → `SpawnNewUnits`
        // (`GroundCover → MobileLandSea → Stationary → MobileAir`).
        self.production_events.clear();
        self.research_events.clear();
        self.industry_message = None;

        for team in 0..PLAYER_TEAM_COUNT {
            let kind = self.teams[team].team_type;
            if kind == TeamKind::None || kind == TeamKind::Eliminated {
                continue;
            }

            // `GameManager_ProgressBuildState`: сначала `ResearchMenu_NewTurn`
            // (P6), затем `SpawnNewUnits` — порядок C++.
            let research = self.advance_research(TeamId(team as u8));
            self.research_events.extend(research);

            let events = crate::production::progress_build_state(self, TeamId(team as u8));
            self.production_events.extend(events);
        }

        false
    }

    /// Забирает события последнего хода производства (для HUD-лога).
    pub fn take_production_events(&mut self) -> Vec<crate::production::ProductionEvent> {
        std::mem::take(&mut self.production_events)
    }

    /// Добавляет строку в сообщение энергетики комплекса — C++
    /// `ComposeIndustryMessage`/`DrawResourceMessage`
    /// (`production_manager.cpp:308-330/792-804`).
    pub fn push_industry_message(&mut self, message: impl Into<String>) {
        let message = message.into();

        if message.is_empty() {
            return;
        }

        match &mut self.industry_message {
            Some(buffer) => {
                if !buffer.ends_with('\n') {
                    buffer.push('\n');
                }
                buffer.push_str(&message);
            }
            None => self.industry_message = Some(message),
        }
    }

    /// Забирает накопленное сообщение энергетики (для `hud.hint`).
    pub fn take_industry_message(&mut self) -> Option<String> {
        self.industry_message.take()
    }

    // -----------------------------------------------------------------------
    // Цикл тика (DESIGN §5.1, `GameManager_ProcessTick`, `game_manager.cpp:5144`)
    // -----------------------------------------------------------------------

    /// Один тик симуляции.
    ///
    /// Порядок (C++ `GameManager_ProcessTick`):
    /// 1. `PathsManager.PollResults()` — результаты путей (P5);
    /// 2. применить входные команды (`Remote_Synchronize`/`ProcessNetPackets`);
    /// 3. если фаза ≠ `TURN_ACTIVE` — `UnitsManager_ProcessOrders()`;
    /// 4. если фаза ≠ `TURN_ACTIVE` и нет `OrdersPending`: `Ai_CheckReactions`
    ///    через чередование `update_flag`, `DispatchJobs`, `Ai_CheckReactions`,
    ///    `Ai_CheckEndTurn`.
    pub fn tick(&mut self) {
        // Звуки боя копятся до чтения клиентом (`App::collect_combat_sounds`
        // сразу после тика). Headless-прогоны их не забирают, поэтому очередь
        // подрезается: события — отчёт для UI, а не состояние партии.
        const MAX_UNIT_SOUNDS: usize = 256;
        if self.unit_sounds.len() > MAX_UNIT_SOUNDS {
            let excess = self.unit_sounds.len() - MAX_UNIT_SOUNDS;
            self.unit_sounds.drain(0..excess);
        }

        // 1. `ResourceManager_GetPathsManager().PollResults()` — P5.
        self.poll_path_results();

        // 2. Входные команды: в порядке поступления.
        self.apply_pending_commands();

        // 3. Приказы.
        if self.game_state != GameStateType::TurnActive {
            self.process_orders();
        }

        // 4. Реакции ИИ и конец хода.
        if self.game_state != GameStateType::TurnActive && !self.orders_pending {
            if self.update_flag {
                self.check_reactions();
            }

            self.update_flag = !self.update_flag;

            self.dispatch_path_jobs();
            self.check_reactions();
            self.check_end_turn();
        }
    }

    /// Применяет все накопленные команды через [`crate::events::apply_command`].
    pub fn apply_pending_commands(&mut self) -> usize {
        crate::events::apply_pending(self)
    }

    /// Ставит команду в очередь (в порядке получения).
    pub fn enqueue(&mut self, command: Command) {
        self.commands.push(command);
    }

    /// `ResourceManager_GetPathsManager().PollResults()` — P5 pathfinding.
    ///
    /// Синхронно считает и применяет все запросы в порядке постановки
    /// (DESIGN риск 2): очередь `GameState::path_requests`.
    fn poll_path_results(&mut self) {
        crate::pathfinding::poll_results(self);
    }

    /// `ResourceManager_GetPathsManager().DispatchJobs()` — в синхронной модели
    /// работа выполнена в [`Self::poll_path_results`]; точка вызова сохранена,
    /// чтобы порядок тика совпадал с C++ (DESIGN §5.1).
    fn dispatch_path_jobs(&mut self) {
        crate::pathfinding::dispatch_jobs(self);
    }

    /// `Ai_CheckReactions` (`ai.cpp:407`) — реакции ИИ на замеченные цели,
    /// разбор очереди напоминаний через чередование `Ai_AiReactionState`.
    fn check_reactions(&mut self) {
        crate::ai::ai_check_reactions(self);
    }

    /// `Ai_CheckEndTurn` (`ai.cpp:239)` — проверка условий завершения хода.
    ///
    /// Для компьютерных команд решение принимает ИИ-игрок
    /// (`AiPlayer::CheckEndTurn`); люди и удалённые команды завершают ход
    /// пакетом 06.
    pub fn check_end_turn(&mut self) {
        crate::ai::ai_check_end_turn(self);
    }

    // -----------------------------------------------------------------------
    // Канонический дайджест состояния (для тестов детерминизма, задел P7/crc)
    // -----------------------------------------------------------------------

    /// Канонический байтовый дайджест состояния.
    ///
    /// Кодирует **только** симуляционные поля в детерминированном порядке:
    /// карта (поверхность + ресурсы), настройки, влияющие на сим, пять списков
    /// юнитов (порядок вставки), `OrderData` юнита, живые атаки, фазу и
    /// барьеры. Формат — явный little-endian writer без `serde` и без
    /// map-итерации (DESIGN §6.4): это заготовка `crc.rs` (P7) и снапшот для
    /// сравнения двух прогонов.
    ///
    /// Почему не `save.rs`: `save.rs` — V70-формат **файла сейва** реальной
    /// игры; записи `GameState → V70` пока нет (P7, DESIGN §8.2 п.5), а
    /// дайджест обязан быть стабильным между прогонами уже на P4.
    pub fn snapshot_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4096 + self.unit_count() * 128);

        out.extend_from_slice(self.map.surface.to_bytes().as_slice());
        out.extend_from_slice(self.map.resources.to_le_bytes().as_slice());
        out.push(u8::from(self.network_game));
        out.push(u8::from(self.settings_effects));
        out.push(u8::from(self.disable_fire));
        out.push(u8::from(self.real_time));

        out.push(self.game_state as u8);
        out.push(self.play_mode as u8);
        put_i32(&mut out, self.turn_counter);
        out.push(self.active_turn_team.0);
        out.push(self.player_team.0);
        out.push(u8::from(self.update_flag));
        out.push(u8::from(self.orders_pending));
        out.push(u8::from(self.combat_effects_active));
        out.push(u8::from(self.delayed_reactions_pending));
        put_u32(&mut out, self.unit_id_counter);
        put_u32(&mut out, self.live_attack_counter);
        for word in self.rng.state() {
            put_u32(&mut out, word);
        }

        for team in 0..TEAM_SLOTS {
            let info = &self.teams[team];
            out.push(team as u8);
            out.push(info.team_type as u8);
            out.push(u8::from(info.finished_turn));
            out.push(info.team_clan as u8);
            put_u32(&mut out, info.team_points);
            put_u16(&mut out, info.number_of_objects_created);
            put_u32(&mut out, self.team_units[team].gold);
            for topic in &info.research_topics {
                put_u32(&mut out, topic.research_level);
                put_u32(&mut out, topic.turns_to_complete);
                put_u32(&mut out, topic.allocation);
            }
            for value in &info.score_graph {
                put_i16(&mut out, *value);
            }
            for value in &info.unit_counters {
                put_u32(&mut out, *value);
            }
            for value in &info.casualties {
                put_u32(&mut out, *value);
            }
            put_u32(&mut out, info.stats_gold_spent_on_upgrades);
            out.push(self.turn_indices[team]);
            out.push(self.next_turn_indices[team]);
            put_u16(&mut out, self.team_data_crc16[team]);
            out.push(self.frame_sync_counter[team]);
            out.push(self.frame_sync_counter2[team]);

            // Комплексы команды — C++ `TeamUnits::complexes` (порядок по
            // возрастанию id, который держит `CreateComplex`): id, запасы и
            // число зданий. Поля `Complex::{material,fuel,gold,power,workers}`
            // — кэш `Access_UpdateResourcesTotal`, но он влияет на показ и на
            // `Complex::Transfer`, поэтому входит в снапшот/CRC (`state_digest`).
            let complexes = &self.team_units[team].complexes;
            put_u32(&mut out, complexes.len() as u32);
            for complex in complexes {
                put_i16(&mut out, complex.id);
                put_i16(&mut out, complex.material);
                put_i16(&mut out, complex.fuel);
                put_i16(&mut out, complex.gold);
                put_i16(&mut out, complex.power);
                put_i16(&mut out, complex.workers);
                put_i16(&mut out, complex.buildings);
            }
        }

        put_u32(&mut out, self.pending_attacks.len() as u32);
        for id in &self.pending_attacks {
            put_u16(&mut out, id.0);
        }
        put_u32(&mut out, self.live_attacks.len() as u32);
        for attack in &self.live_attacks {
            put_u32(&mut out, attack.id);
            put_u32(&mut out, attack.targets.len() as u32);
            for target in &attack.targets {
                put_u16(&mut out, target.0);
            }
        }
        out.push(u8::from(self.pending_air_group_leader.is_some()));
        put_u32(&mut out, self.unit_events.len() as u32);

        // Отложенные реакции (`UnitsManager_DelayedAttackTargets`).
        out.push(self.delayed_reactions_team.0);
        for targets in &self.delayed_attack_targets {
            put_u32(&mut out, targets.len() as u32);
            for id in targets {
                put_u16(&mut out, id.0);
            }
        }

        // Очередь запросов путей (порядок = порядок постановки).
        put_u32(&mut out, self.path_requests.len() as u32);
        for request in &self.path_requests {
            put_u16(&mut out, request.unit.0);
            put_i16(&mut out, request.destination.x);
            put_i16(&mut out, request.destination.y);
            put_u32(&mut out, request.flags);
            put_i32(&mut out, request.minimum_distance);
            out.push(u8::from(request.board_transport));
        }

        // Групповые списки (`UnitInfoGroup`): номера групп по слотам.
        put_u32(&mut out, self.groups.len() as u32);
        for group in &self.groups {
            match group {
                Some(members) => {
                    out.push(1);
                    put_u32(&mut out, members.len() as u32);
                    for id in members {
                        put_u16(&mut out, id.0);
                    }
                }
                None => out.push(0),
            }
        }
        put_u32(&mut out, self.unit_group.len() as u32);
        for group in &self.unit_group {
            put_u32(&mut out, group.unwrap_or(u32::MAX));
        }

        // Юниты: канонический порядок — пять списков (+ юниты вне списков).
        let ids = self.snapshot_unit_ids();
        put_u32(&mut out, ids.len() as u32);
        for id in ids {
            let Some(unit) = self.unit(id) else { continue };
            let data = self.order_data(id);
            put_u16(&mut out, unit.id.0);
            put_u16(&mut out, unit.unit_type as u16);
            out.push(unit.team.0);
            out.push(unit.unit_id);
            out.push(unit.unit_list as u8);
            put_u32(&mut out, unit.flags);
            put_i16(&mut out, unit.grid.x);
            put_i16(&mut out, unit.grid.y);
            put_i16(&mut out, data.move_to.x);
            put_i16(&mut out, data.move_to.y);
            put_i16(&mut out, data.fire_on.x);
            put_i16(&mut out, data.fire_on.y);
            put_i16(&mut out, unit.target_grid.x);
            put_i16(&mut out, unit.target_grid.y);
            out.push(unit.orders as u8);
            out.push(unit.state as u8);
            out.push(unit.prior_orders as u8);
            out.push(unit.prior_state as u8);
            put_u16(&mut out, unit.hits);
            put_u16(&mut out, unit.speed);
            put_u16(&mut out, unit.group_speed);
            out.push(unit.shots);
            out.push(unit.move_and_fire);
            put_i16(&mut out, unit.storage);
            put_i16(&mut out, unit.experience);
            put_i16(&mut out, unit.transfer_cargo);
            out.push(unit.stealth_dice_roll);
            out.push(unit.ammo);
            out.push(unit.targeting_mode);
            out.push(unit.enter_mode);
            out.push(unit.firing_recoil_frames);
            out.push(unit.disabled_turns_remaining);
            out.push(unit.delayed_reaction);
            out.push(u8::from(unit.damaged_this_turn));
            out.push(u8::from(unit.disabled_reaction_fire));
            out.push(u8::from(unit.auto_survey));
            out.push(unit.research_topic as u8);
            out.push(unit.moved);
            out.push(unit.laying_state);
            put_u16(&mut out, unit.movement_wait_count);
            out.push(unit.build_time);
            out.push(unit.total_mining);
            out.push(unit.raw_mining);
            out.push(unit.fuel_mining);
            out.push(unit.gold_mining);
            put_u16(&mut out, unit.build_rate);
            out.push(u8::from(unit.repeat_build));
            out.push(unit.angle);
            out.push(unit.max_velocity);
            out.push(unit.velocity);
            out.push(unit.move_fraction);
            out.push(u8::from(unit.in_transit));
            put_i16(&mut out, unit.pin_count);
            put_u32(&mut out, unit.live_attack);
            out.push(u8::from(unit.tasks_enabled));
            put_u32(&mut out, unit.ai_state_bits);
            put_u16(&mut out, unit.connectors);
            put_u16(
                &mut out,
                unit.complex
                    .map(|complex| complex.0 as u16)
                    .unwrap_or(u16::MAX),
            );
            put_u16(&mut out, unit.parent_unit.map(|p| p.0).unwrap_or(u16::MAX));
            put_u16(&mut out, unit.enemy_unit.map(|p| p.0).unwrap_or(u16::MAX));
            put_u32(&mut out, unit.build_list.len() as u32);
            for unit_type in &unit.build_list {
                put_u16(&mut out, *unit_type as u16);
            }
            for visible in &unit.visible_to_team {
                out.push(u8::from(*visible));
            }
            for spotted in &unit.spotted_by_team {
                out.push(u8::from(*spotted));
            }
            match &unit.path {
                None => out.push(0),
                Some(UnitPath::Ground(path)) => {
                    out.push(1);
                    put_i16(&mut out, path.end_x);
                    put_i16(&mut out, path.end_y);
                    put_u32(&mut out, path.step_index);
                    put_u32(&mut out, path.steps.len() as u32);
                    for step in &path.steps {
                        out.push(step.x as u8);
                        out.push(step.y as u8);
                    }
                }
                Some(UnitPath::Air(path)) => {
                    out.push(2);
                    put_i16(&mut out, path.end_x);
                    put_i16(&mut out, path.end_y);
                    put_i16(&mut out, path.length);
                    out.push(path.angle as u8);
                }
                Some(UnitPath::Builder(path)) => {
                    out.push(3);
                    put_i16(&mut out, path.direction_x);
                    put_i16(&mut out, path.direction_y);
                }
            }
            // Атрибуты юнита (`base_values`) — 14 полей.
            for attribute in 0..crate::unit_values::ATTRIB_COUNT {
                put_i16(&mut out, unit.base_values.get_attribute(attribute) as i16);
            }
        }

        // Состояние ИИ (P6): задачи, напоминания, игроки, тепловые карты.
        self.ai.digest_into(&mut out);

        out
    }
}

// ---------------------------------------------------------------------------
// Кодировщики дайджеста
// ---------------------------------------------------------------------------

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// `UnitInfo::UpdateProduction`, шаг 5: забрать из комплекса до `capacity`
/// единиц ресурса в склад здания (`storage = min(complex->field, ATTRIB_STORAGE)`,
/// `complex->field -= storage`). Возвращает новое значение `storage`.
///
/// Поля комплекса — `int16_t` (C++), поэтому присваивание усекается
/// (`wrapping_*`), даже если `capacity` больше.
fn state_complex_pull(
    state: &mut GameState,
    team: TeamId,
    complex: crate::team::ComplexId,
    cargo: CargoType,
    capacity: i32,
) -> Option<i32> {
    let target = state.complex_mut(team, complex.0 as u16)?;

    let value = match cargo {
        CargoType::Raw => {
            let value = i32::from(target.material).min(capacity);
            target.material = target.material.wrapping_sub(value as i16);
            value
        }
        CargoType::Fuel => {
            let value = i32::from(target.fuel).min(capacity);
            target.fuel = target.fuel.wrapping_sub(value as i16);
            value
        }
        CargoType::Gold => {
            let value = i32::from(target.gold).min(capacity);
            target.gold = target.gold.wrapping_sub(value as i16);
            value
        }
        _ => return None,
    };

    Some(value)
}

// ---------------------------------------------------------------------------
// Вспомогательное: комплексы
// ---------------------------------------------------------------------------

impl GameState {
    /// Комплекс команды по id (`TeamUnits::GetComplex`).
    pub fn complex(&self, team: TeamId, complex_id: u16) -> Option<&Complex> {
        self.team_units
            .get(team.0 as usize)?
            .complexes()
            .iter()
            .find(|complex| complex.id == complex_id as i16)
    }

    /// Изменяемый комплекс команды по id.
    pub fn complex_mut(&mut self, team: TeamId, complex_id: u16) -> Option<&mut Complex> {
        self.team_units
            .get_mut(team.0 as usize)?
            .complexes
            .iter_mut()
            .find(|complex| complex.id == complex_id as i16)
    }

    /// `TeamUnits::OptimizeComplexes(team)` (`teamunits.cpp:170`) в объёме
    /// экономики: для каждого комплекса команды — `Access_UpdateResourcesTotal`
    /// (`access.cpp:912`), то есть пересчёт `material/fuel/gold/power/workers`
    /// из зданий (`Cargo_GetNetProduction + Cargo_GetInventory`).
    ///
    /// Добыча шахты попадает сюда через `raw_mining`/`fuel_mining`/
    /// `gold_mining` (см. [`crate::alloc`]), поэтому распределение из диалога
    /// `allocate` напрямую меняет доход комплекса за ход.
    ///
    /// Порядок комплексов — порядок `TeamUnits::complexes` (тот же, что в
    /// C++-списке: по возрастанию id).
    pub fn update_complex_resources(&mut self, team: TeamId) {
        let Some(ids) = self.team_units.get(team.0 as usize).map(|units| {
            units
                .complexes
                .iter()
                .map(|complex| complex.id)
                .collect::<Vec<_>>()
        }) else {
            return;
        };

        for id in ids {
            // `UpdateResourcesTotal` затрагивает только поля комплекса, состав
            // зданий и их приказы не меняет — цикл по снимку id безопасен.
            crate::alloc::update_resources_total(self, team, crate::team::ComplexId(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::unit_flags;
    use crate::geo::PathStep;
    use crate::unit::{GroundPath, UnitPath};

    fn tank_state() -> (GameState, UnitId, UnitId) {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Player;
        state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
        state.team_units[1] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
        let flags = unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE;
        let first = state
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), flags)
            .expect("spawn");
        let second = state
            .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(20, 10), flags)
            .expect("spawn");
        (state, first, second)
    }

    #[test]
    fn spawn_assigns_ids_and_lists_in_cpp_order() {
        let (state, first, second) = tank_state();
        assert_eq!(first.team(), TeamId::RED);
        assert_eq!(second.team(), TeamId::GREEN);
        assert_eq!(first.object_index(), 1);
        assert_eq!(second.object_index(), 2);
        assert_eq!(state.unit_count(), 2);
        assert_eq!(
            state.unit_list(UnitListKind::MobileLandSea),
            &[second, first],
            "AddToDrawList кладёт мобильные наземные в начало списка (PushFront)"
        );
        let unit = state.unit(first).expect("unit");
        assert_eq!(unit.unit_list, UnitListKind::MobileLandSea);
        assert_eq!(
            unit.hits,
            unit.base_values.get_attribute(ATTRIB_HITS) as u16
        );
        // `CTInfo::Reset` стартует счётчики имён с 1 (`ctinfo.hpp:49-51`).
        assert_eq!(unit.unit_id, 1);
        assert_eq!(state.teams[0].unit_counter(UnitType::Tank as u16), 2);
    }

    #[test]
    fn destroy_removes_unit_from_every_structure() {
        let (mut state, first, _second) = tank_state();
        state.destroy_unit(first);
        assert!(!state.has_unit(first));
        assert_eq!(state.unit_count(), 1);
        assert!(!state
            .unit_list(UnitListKind::MobileLandSea)
            .contains(&first));
        assert_eq!(state.unit_ids_in_order().len(), 1);
    }

    #[test]
    fn stationary_list_keeps_y_order() {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
        let flags = unit_flags::STATIONARY | unit_flags::BUILDING;
        let high = state
            .spawn_unit(
                UnitType::PowerGenerator,
                TeamId::RED,
                Point::new(5, 30),
                flags,
            )
            .unwrap();
        let low = state
            .spawn_unit(
                UnitType::PowerGenerator,
                TeamId::RED,
                Point::new(6, 10),
                flags,
            )
            .unwrap();
        let middle = state
            .spawn_unit(
                UnitType::PowerGenerator,
                TeamId::RED,
                Point::new(7, 20),
                flags,
            )
            .unwrap();
        assert_eq!(
            state.unit_list(UnitListKind::Stationary),
            &[low, middle, high]
        );
    }

    #[test]
    fn score_graph_shifts_like_memmove() {
        let mut state = GameState::new();
        state.teams[0].score_graph = [7; SCORE_GRAPH_LEN];
        state.teams[0].team_points = 42;
        state.update_score_graph();
        assert_eq!(state.teams[0].score_graph[0], 7);
        assert_eq!(state.teams[0].score_graph[SCORE_GRAPH_LEN - 1], 42);
        for index in 0..SCORE_GRAPH_LEN - 2 {
            assert_eq!(state.teams[0].score_graph[index], 7);
        }
        assert_eq!(state.teams[0].score_graph[SCORE_GRAPH_LEN - 2], 7);
    }

    #[test]
    fn finish_turn_advances_counter_and_resets_flags() {
        let (mut state, _red, _green) = tank_state();
        // P7: конец хода сначала проверяет победу/поражение
        // (`menu_check_end_game_conditions`); живая команда + лимит не достигнут →
        // партия продолжается. Сетевая ветка выключена (локальная семантика).
        state.network_game = false;
        state.victory_type = VictoryType::Duration;
        state.victory_limit = 50;
        state.teams[0].finished_turn = true;
        state.game_state = GameStateType::EndTurn;
        assert!(!state.finish_turn());
        assert_eq!(state.turn_counter, 1);
        assert!(!state.teams[0].finished_turn);
        assert_eq!(state.game_state, GameStateType::InGame);
    }

    #[test]
    fn finish_turn_stops_when_victory_condition_met() {
        let (mut state, _red, _green) = tank_state();
        state.network_game = false;
        // `VICTORY_TYPE_DURATION` с нулевым лимитом: `limit <= turn_counter` → конец.
        state.victory_type = VictoryType::Duration;
        state.victory_limit = 0;
        state.game_state = GameStateType::EndTurn;
        assert!(state.finish_turn());
        assert_eq!(state.turn_counter, 0);
    }

    #[test]
    fn pending_attack_head_is_settled_like_cpp() {
        let (mut state, first, _second) = tank_state();
        // Атакующий с нулевыми hits — просто снимается из очереди.
        {
            let unit = state.unit_mut(first).unwrap();
            unit.hits = 0;
            unit.orders = UnitOrder::Fire;
            unit.state = UnitOrderState::AttackPending;
        }
        state.pending_attacks.push(first);
        state.process_orders();
        assert!(state.pending_attacks.is_empty());

        // Атакующий с приказом не FIRE — атака отменяется.
        {
            let unit = state.unit_mut(first).unwrap();
            unit.hits = 10;
            unit.orders = UnitOrder::Await;
            unit.prior_orders = UnitOrder::Await;
        }
        state.pending_attacks.push(first);
        state.process_orders();
        assert!(state.pending_attacks.is_empty());
    }

    #[test]
    fn process_orders_visits_five_lists_in_canonical_order() {
        use crate::enums::unit_flags::{
            GROUND_COVER, MISSILE_UNIT, MOBILE_AIR_UNIT, MOBILE_LAND_UNIT, STATIONARY,
        };

        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);

        // По одному юниту в каждом из пяти списков, в порядке-канарейке
        // (DESIGN §5.2: GroundCover → MobileLandSea → Stationary → MobileAir →
        // Particle).
        let ground_cover = state
            .spawn_unit(
                UnitType::SmallSlab,
                TeamId::RED,
                Point::new(1, 1),
                STATIONARY | GROUND_COVER,
            )
            .unwrap();
        let mobile_land = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::RED,
                Point::new(2, 2),
                MOBILE_LAND_UNIT,
            )
            .unwrap();
        let stationary = state
            .spawn_unit(
                UnitType::PowerGenerator,
                TeamId::RED,
                Point::new(3, 3),
                STATIONARY,
            )
            .unwrap();
        let mobile_air = state
            .spawn_unit(
                UnitType::Fighter,
                TeamId::RED,
                Point::new(4, 4),
                MOBILE_AIR_UNIT,
            )
            .unwrap();
        let particle = state
            .spawn_unit(
                UnitType::Rocket,
                TeamId::RED,
                Point::new(5, 5),
                MISSILE_UNIT,
            )
            .unwrap();

        let expected = [ground_cover, mobile_land, stationary, mobile_air, particle];
        assert_eq!(state.unit_ids_in_order(), expected);

        // `ORDER_FIRE` в состоянии INIT за один проход добавляет юнита в
        // `PendingAttacks` и в `LiveAttacks` — порядок вставки и есть порядок
        // обхода списков.
        for unit in expected {
            state.set_order(unit, UnitOrder::Fire, UnitOrderState::Init);
        }
        state.tick();

        assert_eq!(state.live_attacks.len(), expected.len());
        for (attack, unit) in state.live_attacks.iter().zip(expected.iter()) {
            assert_eq!(
                state.unit(*unit).unwrap().live_attack,
                attack.id,
                "живая атака юнита {unit:?} создана не в каноническом порядке"
            );
        }
        // Ракета (`MISSILE_UNIT`) в шапке `UnitsManager_ProcessOrder` включает
        // `OrdersPending`, поэтому очередь атак в этом тике не разбирается —
        // как в C++.
        assert!(state.orders_pending);
        assert_eq!(state.pending_attacks, expected.to_vec());
        for unit in expected {
            assert_eq!(
                state.unit(unit).unwrap().state,
                UnitOrderState::AttackPending
            );
        }
    }

    #[test]
    fn scripted_combat_replay_is_deterministic() {
        use crate::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE, UPGRADABLE};

        fn run() -> (Vec<u8>, u16, u16, u8) {
            let mut state = GameState::new();
            state.teams[0].team_type = TeamKind::Player;
            state.teams[1].team_type = TeamKind::Player;
            state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
            state.team_units[1] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
            state.rng.set_seed(0xDEAD_BEEF);

            let red = state
                .spawn_unit(
                    UnitType::Tank,
                    TeamId::RED,
                    Point::new(10, 10),
                    MOBILE_LAND_UNIT | SELECTABLE | UPGRADABLE,
                )
                .unwrap();
            let green = state
                .spawn_unit(
                    UnitType::Tank,
                    TeamId::GREEN,
                    Point::new(13, 10),
                    MOBILE_LAND_UNIT | SELECTABLE | UPGRADABLE,
                )
                .unwrap();

            // Серия выстрелов: каждый FIRE-приказ — один выстрел (после
            // выстрела `RestoreOrders` возвращает AWAIT); начало хода
            // восстанавливает `shots` (`UpdateProduction`).
            for _ in 0..8 {
                state.manage_economy(TeamId::RED);
                let mut data = state.order_data(red);
                data.fire_on = Point::new(13, 10);
                data.move_to = Point::new(13, 10);
                state.set_order_data(red, data);
                state.set_order(red, UnitOrder::Fire, UnitOrderState::Init);
                for _ in 0..8 {
                    state.tick();
                }
                if !state.has_unit(green)
                    || state.unit(green).map(|unit| unit.hits).unwrap_or(0) == 0
                {
                    break;
                }
            }
            for _ in 0..2 {
                state.tick();
            }

            let green_hits = state.unit(green).map(|unit| unit.hits).unwrap_or(0);
            let red_ammo = state.unit(red).map(|unit| unit.ammo).unwrap_or(0);
            let red_shots = state.unit(red).map(|unit| unit.shots).unwrap_or(0);
            (
                state.snapshot_bytes(),
                green_hits,
                u16::from(red_ammo),
                red_shots,
            )
        }

        let first = run();
        let second = run();
        assert_eq!(first.0, second.0, "боевой реплей недетерминирован");
        assert_eq!((first.1, first.2, first.3), (second.1, second.2, second.3));
        // 4 выстрела по 6 урона (24 hp) уничтожают цель; поведение
        // зафиксировано, чтобы регрессия формул была видна.
        assert_eq!(first.1, 0, "цель должна быть уничтожена");
        assert_eq!(first.2, 10, "израсходовано 4 патрона из 14");
    }

    #[test]
    fn scripted_replay_is_deterministic() {
        use crate::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};

        fn run() -> Vec<u8> {
            let mut state = GameState::new();
            state.teams[0].team_type = TeamKind::Player;
            state.teams[1].team_type = TeamKind::Computer;
            state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
            state.team_units[1] = TeamUnits::with_clan(crate::enums::TeamClan::Random);

            let red = state
                .spawn_unit(
                    UnitType::Tank,
                    TeamId::RED,
                    Point::new(10, 10),
                    MOBILE_LAND_UNIT | SELECTABLE,
                )
                .unwrap();
            let green = state
                .spawn_unit(
                    UnitType::Tank,
                    TeamId::GREEN,
                    Point::new(20, 10),
                    MOBILE_LAND_UNIT | SELECTABLE,
                )
                .unwrap();

            // 13: seed; 08: приказ с целями (payload P3); 38: готовый путь;
            // 50: аварийный стоп; 41: завершение движения; 06/52: конец хода.
            state.enqueue(Command::Seed(crate::input::Packet13Seed {
                seed: 0xDEAD_BEEF,
            }));
            state.enqueue(Command::Order(crate::input::Packet08Order {
                unit: red,
                payload: crate::input::OrderPayload::P3(crate::input::PayloadP3 {
                    p2: crate::input::PayloadP2 {
                        p1: crate::events::payload_p1_for(
                            UnitOrder::Move,
                            UnitOrderState::Init,
                            UnitOrder::Await,
                            UnitOrderState::ExecutingOrder,
                        ),
                        parent_unit: None,
                        experience: 0,
                    },
                    move_to: Point::new(14, 10),
                    fire_on: Point::new(14, 10),
                    enemy_unit: Some(green),
                    transfer_cargo: 0,
                    stealth_dice_roll: 0,
                }),
            }));
            for _ in 0..6 {
                state.tick();
            }
            state.enqueue(crate::events::command_move_linear(
                green,
                Point::new(20, 10),
                Point::new(20, 13),
                UnitOrder::Move,
                UnitOrderState::InProgress,
            ));
            state.tick();
            state.enqueue(crate::events::command_move_linear(
                red,
                Point::new(14, 10),
                Point::new(18, 10),
                UnitOrder::Move,
                UnitOrderState::InProgress,
            ));
            state.enqueue(Command::Stop(crate::input::Packet50StopUnit {
                unit: green,
            }));
            state.enqueue(Command::MoveFinished(
                crate::input::Packet41MoveFinished::port_form(red, true),
            ));
            for _ in 0..5 {
                state.tick();
            }
            state.enqueue(crate::events::command_finished_turn(TeamId::RED));
            state.enqueue(crate::events::command_finished_turn(TeamId::GREEN));
            state.enqueue(crate::events::command_end_turn_barrier(TeamId::RED, 1));
            state.tick();
            state.finish_turn();
            for _ in 0..3 {
                state.tick();
            }
            state.snapshot_bytes()
        }

        let first = run();
        let second = run();
        assert_eq!(first, second, "два прогона одного скрипта разошлись");
        assert!(!first.is_empty());
    }

    #[test]
    fn end_turn_restores_speed_and_updates_production() {
        let (mut state, id, _second) = tank_state();
        // Израсходуем скорость и выстрелы.
        {
            let unit = state.unit_mut(id).unwrap();
            unit.speed = 0;
            unit.shots = 0;
            unit.moved = 5;
        }
        let team = state.unit(id).unwrap().team;
        state.manage_economy(team);
        let unit = state.unit(id).unwrap();
        assert_eq!(
            unit.speed,
            unit.base_values.get_attribute(ATTRIB_SPEED) as u16,
            "UpdateProduction возвращает скорость на начало хода"
        );
        assert_eq!(
            unit.shots,
            unit.base_values.get_attribute(ATTRIB_ROUNDS) as u8
        );
    }

    #[test]
    fn barriers_require_agreement_of_playing_teams() {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Remote;
        state.teams[2].team_type = TeamKind::None;
        state.teams[3].team_type = TeamKind::Eliminated;

        // Барьера ещё нет: значения по умолчанию одинаковы (0), но это и есть
        // согласие «до начала хода» — как в C++ счётчики стартуют с 0.
        assert_eq!(state.end_turn_barrier(), Some(0));

        state.turn_indices[0] = 3;
        assert_eq!(
            state.end_turn_barrier(),
            None,
            "команды разошлись по счётчику"
        );
        state.turn_indices[1] = 3;
        assert_eq!(state.end_turn_barrier(), Some(3));
        // ALIEN/пустые слоты в барьере не участвуют.
        state.turn_indices[4] = 9;
        assert_eq!(state.end_turn_barrier(), Some(3));

        state.next_turn_indices[0] = 5;
        state.next_turn_indices[1] = 6;
        assert_eq!(state.crc_barrier(), None);
        state.next_turn_indices[1] = 5;
        assert_eq!(state.crc_barrier(), Some(5));
    }

    #[test]
    fn begin_turn_barrier_tracks_frame_sync_counter() {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Remote;
        state.teams[2].team_type = TeamKind::None;

        assert_eq!(state.begin_turn_barrier(), Some(0));
        state.frame_sync_counter[0] = 1;
        assert_eq!(state.begin_turn_barrier(), None, "пир ещё не вошёл в раунд");
        state.frame_sync_counter[1] = 1;
        assert_eq!(state.begin_turn_barrier(), Some(1));

        // 54-барьер завершается только по полному (`is_last`) состоянию.
        assert_eq!(state.state_sync_index(TeamId::GREEN), 0);
        state.state_sync_indices[1] = 4;
        assert_eq!(state.state_sync_index(TeamId::GREEN), 4);
    }

    #[test]
    fn snapshot_is_stable_across_clones() {
        let (state, first, _second) = tank_state();

        // Два независимых прогона одной и той же последовательности команд
        // дают побайтово одинаковое состояние.
        let mut first_run = state.clone();
        let mut second_run = state.clone();
        for _ in 0..5 {
            first_run.enqueue(Command::Stop(crate::input::Packet50StopUnit {
                unit: first,
            }));
            second_run.enqueue(Command::Stop(crate::input::Packet50StopUnit {
                unit: first,
            }));
            first_run.tick();
            second_run.tick();
        }
        assert_eq!(first_run.snapshot_bytes(), second_run.snapshot_bytes());

        // Изменение приказа меняет дайджест.
        let before = first_run.snapshot_bytes();
        first_run.set_order(first, UnitOrder::Sentry, UnitOrderState::ExecutingOrder);
        assert_ne!(before, first_run.snapshot_bytes());
    }

    #[test]
    fn path_kind_is_encoded_in_snapshot() {
        let (mut state, first, _second) = tank_state();
        let before = state.snapshot_bytes();
        {
            let unit = state.unit_mut(first).unwrap();
            unit.path = Some(UnitPath::Ground(GroundPath {
                end_x: 12,
                end_y: 10,
                step_index: 0,
                steps: vec![PathStep::new(1, 0), PathStep::new(1, 0)],
            }));
        }
        assert_ne!(before, state.snapshot_bytes());
    }
}
