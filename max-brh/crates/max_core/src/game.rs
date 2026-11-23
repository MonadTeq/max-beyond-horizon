//! Game state and the tick loop — C++ `GameManager_*` / `UnitsManager_*`.
//!
//! Oracle: `max-cpp-port/src/units_manager.cpp` (`UnitsManager_ProcessOrders`,
//! `UnitsManager_DeployUnit`, `UnitsManager_RemoveUnitFromUnitLists`,
//! `UnitsManager_EndAttack`), `game_manager.cpp` (`GameManager_ProcessTick`,
//! `GameManager_ManageEconomy`, `GameManager_UpdateScoreGraph`,
//! `GameManager_TeamTurnFinish`, `GameManager_AreTeamsFinishedTurn`),
//! `unitinfo.cpp` (`UnitInfo::AddToDrawList`, `UpdateProduction`).
//! Phase order — DESIGN.md §5.1-§5.3, `max-brh-docs/state-model.md` §3.
//!
//! # What is here and what is not
//!
//! P4 covers the **order machine and the turn loop**: five unit lists,
//! `ProcessOrders` in fixed order, end of turn (economy, score
//! graph, team switching), barriers as network facts and the canonical state
//! digest for determinism tests.
//!
//! P5 connects combat (`combat`), passability (`access`) and paths
//! (`pathfinding`): `ProcessOrders` calls `AssessAttacks` for delayed
//! reactions, `PendingAirGroupLeader` — `Access_ProcessNewGroupOrder`, the tick —
//! synchronous `PollResults`; `MapHash` and `DelayedAttackTargets` enter the
//! state.
//!
//! P6 connects the AI (`crate::ai`): `Ai_ClearTasksPendingFlags` in
//! `ProcessOrders`, `Ai_CheckReactions`/`Ai_CheckEndTurn` in the tick, scan
//! coverage (`Access_UpdateMapStatus`) and `Ai_RemoveUnit` on
//! unit spawn/movement/destruction. The AI state — `GameState.ai` —
//! enters the canonical state digest.
//!
//! # Map and unit creation
//!
//! * `GameManager_InitMap`/map generation (`CreateMap`) — in `setup.rs` (P2,
//!   `generate_map`/`derive_surface_map`); the result goes into [`GameState::map`].
//! * `GameManager_InitUnitsAndGameState`/`CreateUnits` — the starting unit
//!   layout; in P4 it is performed by the caller layer via [`GameState::spawn_unit`]
//!   (a port of `UnitsManager_DeployUnit` in the P4 scope). Automatic placement by
//!   `TeamMissionSupplies` requires the flag table of the 93 unit types
//!   (`units.json`, `ResourceManager_InitUnits` — P3 data) and mission
//!   rules (P7), so it is not ported yet.
//!
//! # Divergences from the C++ port (intentional)
//!
//! 1. Commands are applied at the start of the tick from the [`CommandQueue`] (DESIGN
//!    §5.1, step 1) — in C++ this is done by `Remote_Synchronize()` /
//!    `Remote_ProcessNetPackets()` at the same points of the loop; the network itself is moved to
//!    `max_net`.
//! 2. Time budgets (`TickTimer_*`, `ExecuteReminders` limits) are not ported
//!    (DESIGN §5.1, risk 4): the queue is processed fully.
//! 3. In C++ `UnitsManager_ProcessOrder` may move a unit through
//!    several cells in one tick (loop `while (state == IN_TRANSITION && !visible)`), while in
//!    a network game — exactly one cell per `Move()` call (`if
//!    (Remote_IsNetworkGame) return;`). `max_core` is a lockstep simulation,
//!    so the network branch is adopted: one path step per tick, and it is
//!    documented in `unit_order::unit_move`.
//! 4. `UnitInfo::x/y` are pixel coordinates. They are simulation fields: `AirPath`
//!    steps pixel by pixel, `Access_IsWithinAttackRange` checks the line of fire, and
//!    spawn/movement derive them from the cell (`x = grid*64 + 32`, +31 for buildings).
//!
//! # Unit presentation (F5.3/B)
//!
//! C++ `UnitInfo` fields not participating in the simulation (shadow, sprite bounds,
//! brightness, sound, turret offsets/frames, cursor, bobbing) are moved to
//! [`UnitPresentation`] and stored in `GameState` as a parallel table
//! `presentations` next to the `units` slots. The V70 bytes did not change:
//! `save.rs` writes both objects in the original order.

use crate::classification::Footprint;
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
use crate::unit::{UnitId, UnitInfo, UnitListKind, UnitPath, UnitPresentation};
use crate::unit_order;
use crate::unit_values::{
    ATTRIB_AGENT_ADJUST, ATTRIB_AMMO, ATTRIB_ATTACK, ATTRIB_HITS, ATTRIB_MOVE_AND_FIRE,
    ATTRIB_ROUNDS, ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS,
};
use crate::victory::{
    cargo_gold_consumption_rate, cargo_raw_consumption_rate, cargo_type_of, CargoType,
};

/// `PLAYER_TEAM_MAX`: four playing teams (`RED..GRAY`) + `ALIEN`.
pub const PLAYER_TEAM_MAX: u8 = 5;
/// Number of playing teams without `ALIEN` (`PLAYER_TEAM_MAX - 1`).
pub const PLAYER_TEAM_COUNT: usize = PLAYER_TEAM_MAX as usize - 1;
/// Array size of `TeamInfo`/`TeamUnits` (including `ALIEN`).
pub const TEAM_SLOTS: usize = PLAYER_TEAM_MAX as usize;
/// `GFX_MAP_TILE_SIZE` — the cell side in pixels.
pub const GFX_MAP_TILE_SIZE: i32 = 64;
/// Initial `UnitsManager_UnitIdCounter` value (state-model §7.8, DESIGN risk 9).
pub const INITIAL_UNIT_ID_COUNTER: u32 = (PLAYER_TEAM_MAX as u32) << 13;

// ---------------------------------------------------------------------------
// Order data that is not in `UnitInfo`
// ---------------------------------------------------------------------------

/// Order runtime data that C++ stores in `UnitInfo`, but V70 — as one
/// `target_grid` field (state-model §3.9, §7.6).
///
/// `move_to` and `fire_on` are different fields in C++ (`move_to_grid_x/y`,
/// `fire_on_grid_x/y`), while V70 stores one `Point`. Therefore they live in
/// a parallel [`GameState`] table; `UnitInfo::target_grid` mirrors
/// `move_to` (that is the field's semantics in the save).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrderData {
    /// `UnitInfo::move_to_grid_*`.
    pub move_to: Point,
    /// `UnitInfo::fire_on_grid_*`.
    pub fire_on: Point,
}

/// A live attack — C++ `UnitsManager_LiveAttacks` (number → target list).
///
/// In C++ this is `std::unordered_map<uint32_t, SmartList<UnitInfo>>`; iteration over
/// the map is not performed (only lookup by number), so determinism does not
/// suffer. In Rust — a `Vec` with linear search: there are few attack numbers, and the
/// insertion order of targets is preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAttack {
    /// Attack number (`live_attack`).
    pub id: u32,
    /// Pinned targets (`pin_count` is already incremented).
    pub targets: Vec<UnitId>,
}

/// Which unit sound to play — C++ `Unit::SfxType`
/// (`unit.hpp:49-68`), in the combat scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitSoundKind {
    /// `SFX_TYPE_FIRE` — `UnitInfo::PrepareFire` (`unitinfo.cpp:4725`).
    Fire,
    /// `SFX_TYPE_HIT` — a hit on a live target (`units_manager.cpp:3922`).
    Hit,
    /// `SFX_TYPE_EXPLOAD` — the explosion of a destroyed unit (`units_manager.cpp:3929`).
    Explode,
}

/// One combat sound event: who and which `SfxType`.
///
/// The `*.SPW` resource and volume are chosen on the client by the `SC_UNITS` table
/// (`SoundManager::PlaySfx(UnitInfo*, Unit::SfxType)`,
/// `sound_manager.cpp:413-490`): the simulation must remain free of references to
/// resources (DESIGN §6.1), so the event carries only the unit id and the sound type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitSound {
    /// The unit whose sound plays (`PlaySfx(unit, …)`).
    pub unit: UnitId,
    /// Unit type at event time (`unit->GetUnitType()`): the client picks the sound
    /// from `SC_UNITS` by it, even if the unit is already destroyed when read.
    pub unit_type: crate::enums::UnitType,
    /// Sound type.
    pub kind: UnitSoundKind,
}

// ---------------------------------------------------------------------------
// GameState
// ---------------------------------------------------------------------------

/// Game state — the analog of the C++ globals (`UnitsManager_*`, `GameManager_*`,
/// `UnitEvent_UnitEvents`) in one object (DESIGN §1.4 item 5).
#[derive(Debug, Clone)]
pub struct GameState {
    // --- map and teams ---
    /// The game map.
    pub map: Map,
    /// `UnitsManager_TeamInfo[PLAYER_TEAM_MAX]`.
    pub teams: [TeamInfo; TEAM_SLOTS],
    /// `CTInfo::team_units` (economy/technologies, including `ALIEN`).
    pub team_units: [TeamUnits; TEAM_SLOTS],
    /// The simulation `Randomizer` (the seed comes only from packet 13).
    pub rng: Randomizer,

    // --- units ---
    units: Vec<Option<UnitInfo>>,
    /// Unit presentation fields (F5.3/B) — parallel to the `units` slots.
    /// They do not enter the simulation/snapshot/CRC; needed by `save.rs` (byte-exact V70) and
    /// by the `max_app` renderer.
    presentations: Vec<Option<UnitPresentation>>,
    free_slots: Vec<u16>,
    unit_hash: UnitHash<u16>,
    order_data: Vec<OrderData>,
    /// Five unit lists in `UnitListKind` order (insertion order).
    lists: [Vec<UnitId>; 5],
    /// `Hash_MapHash` — cell → units (`P5`; `access.rs`/`pathfinding.rs`).
    pub map_hash: MapHash,
    /// The unit's group list (`UnitInfo::unit_list` in C++ — `UnitInfoGroup`).
    /// Parallel to the `units`/`order_data` slots.
    unit_group: Vec<Option<u32>>,
    /// Group lists (C++ `SmartList<UnitInfo> UnitInfoGroup`).
    pub groups: Vec<Option<Vec<UnitId>>>,
    /// Path request queue (`PathsManager::m_pending_requests`).
    pub path_requests: std::collections::VecDeque<PathRequest>,
    /// `UnitsManager_DelayedAttackTargets[team]` — reaction fire targets.
    pub delayed_attack_targets: [Vec<UnitId>; TEAM_SLOTS],
    /// `UnitsManager_DelayedReactionsTeam` — the team whose turn it is to check
    /// delayed reactions.
    pub delayed_reactions_team: TeamId,

    // --- phase and turn ---
    /// `GameManager_GameState`.
    pub game_state: GameStateType,
    /// `GameManager_PlayMode`.
    pub play_mode: PlayMode,
    /// `GameManager_TurnCounter`.
    pub turn_counter: i32,
    /// `GameManager_ActiveTurnTeam`.
    pub active_turn_team: TeamId,
    /// `GameManager_PlayerTeam` (the local player).
    pub player_team: TeamId,
    /// `GameManager_UpdateFlag` — alternation of `Ai_CheckReactions` (DESIGN risk 14).
    pub update_flag: bool,
    /// `UnitsManager_UnitIdCounter`.
    pub unit_id_counter: u32,
    /// `UnitsManager_LiveAttackCounter`.
    pub live_attack_counter: u32,
    /// `UnitsManager_OrdersPending` — someone is still moving/fighting this tick.
    pub orders_pending: bool,
    /// `UnitsManager_CombatEffectsActive`.
    pub combat_effects_active: bool,
    /// `UnitsManager_BobEffectQuota`.
    pub bob_effect_quota: i8,
    /// `UnitsManager_ResetBobState`.
    pub reset_bob_state: bool,
    /// `UnitsManager_PendingAttacks` — insertion order.
    pub pending_attacks: Vec<UnitId>,
    /// `UnitsManager_PendingAirGroupLeader`.
    pub pending_air_group_leader: Option<UnitId>,
    /// `UnitEvent_UnitEvents` (`unitevents.cpp`).
    pub unit_events: Vec<crate::events::UnitEvent>,
    /// `UnitsManager_LiveAttacks` (`live_attack` → targets).
    pub live_attacks: Vec<LiveAttack>,
    /// `UnitsManager_DelayedReactionsPending` (a flag for the network/AI).
    pub delayed_reactions_pending: bool,
    /// Events of the last production turn (`GameManager_ProgressBuildState`):
    /// what was spawned/interrupted. Not part of the digest — it is a UI/QA report.
    pub production_events: Vec<crate::production::ProductionEvent>,
    /// Events of the last research turn (`ResearchMenu_NewTurn`):
    /// completed topic levels. Not part of the digest — a UI/QA report.
    pub research_events: Vec<crate::research::ResearchEvent>,
    /// Complex power message for the top map line — C++
    /// `ProductionManager::DrawResourceMessage`/`ComposeIndustryMessage`
    /// (`production_manager.cpp:308-330/792-804`), `MESSAGE_BOX_NOTICE`.
    ///
    /// A UI/QA report, not part of the snapshot/CRC (like `production_events`).
    /// Accumulated during the production turn and taken
    /// [`Self::take_industry_message`].
    pub industry_message: Option<String>,
    /// Combat sounds per tick — `SoundManager::PlaySfx(unit, SfxType)`
    /// (`unitinfo.cpp:4725` — shot, `units_manager.cpp:3922/3929` — hit
    /// and explosion). The simulation only accumulates events; the resource/volume is chosen by
    /// the client (`max_app::sfx`, `SC_UNITS` table). Not part of the digest/CRC.
    pub unit_sounds: Vec<UnitSound>,
    /// `UnitsManager_TeamInfo[*].finished_turn` is duplicated in `teams`; here —
    /// barrier counters so that the state is self-contained.
    pub turn_indices: [u8; TEAM_SLOTS],
    /// `Remote_NextTurnIndices` (packet 45).
    pub next_turn_indices: [u8; TEAM_SLOTS],
    /// `Remote_TeamDataCrc16` (packet 45).
    pub team_data_crc16: [u16; TEAM_SLOTS],
    /// `Remote_FrameSyncCounter` (packet 00).
    pub frame_sync_counter: [u8; TEAM_SLOTS],
    /// `Remote_FrameSyncCounter2values` (packet 01).
    pub frame_sync_counter2: [u8; TEAM_SLOTS],
    /// `Remote_StateSyncIndices` (packet 54) — the index of the last **full**
    /// (`is_last`) team state, which completes the 54 barrier.
    pub state_sync_indices: [u8; TEAM_SLOTS],
    /// `Remote_StateSyncReceivingIndices` (packet 54) — the index of the chunk
    /// currently being received (resets `state_sync_unit_ids` on index change).
    pub state_sync_receiving_indices: [u8; TEAM_SLOTS],
    /// `Remote_StateSyncUnitIds` (packet 54) — ids of units sent by the owner
    /// in the current round; `Remote_RemoveExtraUnits` removes extras by them.
    pub state_sync_unit_ids: [Vec<UnitId>; TEAM_SLOTS],
    /// In-game chat (packet 18) — pure data for the UI/logs, does not affect the sim.
    pub chat_log: Vec<(TeamId, String)>,
    /// Mission supplies per team (packet 12) — setup input data.
    pub mission_supplies: [Option<crate::input::Packet12MissionSupply>; TEAM_SLOTS],
    /// `UnitsManager_Units[team]` cache — the unit for which a delayed reaction
    /// fired in this pass (`UnitsManager_PursueEnemy`).
    pub units_active: [Option<UnitId>; TEAM_SLOTS],
    /// Network mode flag (`Remote_IsNetworkGame`).
    ///
    /// `max_core` runs a **lockstep simulation** (DESIGN §1.1/§5.1), so
    /// by default `true`: this enables the network branches of the C++ code (one path
    /// step per tick, `finished_turn` checks, packet application at the start of
    /// the tick).
    pub network_game: bool,
    /// The `effects` setting (`ResourceManager_GetSettings()->"effects"`).
    ///
    /// In the simulation it affects only the `moved` counter in `ORDER_BUILD`
    /// (`UnitsManager_ProcessOrderBuild`); UI animations are not ported.
    pub settings_effects: bool,
    /// The `disable_fire` setting — disable reaction fire (DESIGN §6.5).
    pub disable_fire: bool,
    /// The `GameManager_RealTime` setting (`paths.cpp:Paths_CalculateStep`):
    /// when true, a path step does not spend `speed`.
    pub real_time: bool,
    /// The `enemy_halt` setting ("Halt movement when enemy is detected").
    ///
    /// `Access_UpdateMapStatus` (`access.cpp:827-868`), on detecting an enemy in
    /// the scan radius, queues `UnitEventEmergencyStop` for the unit (or its
    /// group) if it has an active order. The setting is read from
    /// `settings` on every step, so in C++ it is not part of the game
    /// state; here it is a `GameState` field so that the simulation is self-contained.
    /// `App` writes it from `Preferences` at game start/load.
    ///
    /// Defaults to `false`: the pure core (unit tests) keeps the previous
    /// behavior; the application sets the value from settings (`settings.cpp`:
    /// `enemy_halt = 1`).
    pub enemy_halt: bool,
    /// Input command queue (DESIGN §5.1 step 1).
    pub commands: CommandQueue,
    /// AI state: players, task manager, heat maps (P6, `ai.cpp`).
    pub ai: crate::ai::AiState,
    /// V70 data needed only for save writing (P3): game settings,
    /// `TeamInfo` UI fields, message logs, `AiPlayer` V70 fields.
    ///
    /// Not part of the canonical digest (`snapshot_bytes`) — it is not game
    /// state but "what C++ keeps outside `GameState`".
    pub save_meta: crate::save_write::SaveMeta,

    // --- game rules (setup constants; P7) ---
    /// `SaveFormatHeader::mission_index` (0 — generic custom game).
    pub mission_index: u16,
    /// `MissionManager::GetMission()->GetCategory()` (`winloss.cpp:57`).
    pub mission_category: crate::victory::MissionCategory,
    /// `ini_setting_victory_type` (`VICTORY_TYPE_DURATION`/`SCORE`).
    pub victory_type: VictoryType,
    /// `ini_setting_victory_limit` — the turn or point limit.
    pub victory_limit: i32,
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

impl GameState {
    /// Empty game: default map, `None` teams, tick 0.
    pub fn new() -> Self {
        let mut teams = std::array::from_fn(|_| TeamInfo::new());
        for team in teams.iter_mut() {
            team.team_type = TeamKind::None;
        }
        let team_units = std::array::from_fn(|_| TeamUnits::new());
        // Empty game for unit tests: the surface is `LAND`, otherwise no
        // ground unit could move (the real map comes from
        // `setup::generate_map` and replaces it entirely).
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
            presentations: Vec::new(),
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
            // `settings.cpp` defaults: generic custom game, duration, limit 50.
            mission_index: 0,
            mission_category: crate::victory::MissionCategory::Custom,
            victory_type: VictoryType::Duration,
            victory_limit: 50,
        }
    }

    /// A game with a map and a set of teams (`TeamKind` per playing team).
    pub fn with_setup(map: Map, team_kinds: [TeamKind; PLAYER_TEAM_COUNT]) -> Self {
        let mut state = Self::new();
        state.map = map;
        for (index, kind) in team_kinds.iter().enumerate() {
            state.teams[index].team_type = *kind;
            state.team_units[index] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
        }
        state.teams[PLAYER_TEAM_COUNT].team_type = TeamKind::Computer;
        // `ResourceManager_InitClanUnitValues` (`resource_manager.cpp:1275-1277`):
        // the derelict team (`PLAYER_TEAM_ALIEN`) always gets `TEAM_CLAN_THE_CHOSEN`
        // regardless of the menu settings — alien units from the debris use it.
        state.teams[PLAYER_TEAM_COUNT].team_clan = crate::enums::TeamClan::TheChosen;
        state.team_units[PLAYER_TEAM_COUNT] =
            TeamUnits::with_clan(crate::enums::TeamClan::TheChosen);
        state
    }

    // -----------------------------------------------------------------------
    // V70 save loading (P7): normalizations of `saveload.cpp` / `unitinfo.cpp`
    // -----------------------------------------------------------------------

    /// A game from a V70 save: [`Self::new`] + [`Self::load_state`].
    pub fn from_save(save: &crate::save::SaveFile) -> Self {
        let mut state = Self::new();
        state.load_state(save);
        state
    }

    /// Applies a parsed V70 save to the state, **with the C++ normalizations**,
    /// so that the loaded game can be ticked immediately.
    ///
    /// # Port of `SaveLoad_LoadFormatV70` (`saveload.cpp:756`) and
    /// `UnitInfo::FileLoad` (`unitinfo.cpp:2489`)
    ///
    /// 1. `UnitsManager_UnitIdCounter = PLAYER_TEAM_MAX << 13`
    ///    (`saveload.cpp:834`) — new ids do not overlap the V70 range.
    /// 2. `unit.movement_wait_count = 0` (the `FileLoad` V70 branch).
    /// 3. `unit.ai_state_bits &= ~(AI_STATE_MOVE_FINISHED_REMINDER |
    ///    AI_STATE_RETREAT_REMINDER)` (`unitinfo.cpp:2512`) — AI reminders
    ///    live only in the current process and are not carried into the save.
    /// 4. `build_rate == 0 → 1` (`unitinfo.cpp:2514`).
    /// 5. `state ∈ {NEW_ORDER, MOVE_GETTING_PATH, ISSUING_PATH, IN_TRANSITION,
    ///    IN_PROGRESS} → EXECUTING_ORDER` (`unitinfo.cpp:2527`): after loading
    ///    there are no unfinished transitions.
    /// 6. `HeatMap::LoadV70` — negative cell values are clamped to 0
    ///    (`heatmap.cpp:212`) for teams with `team_type != NONE`.
    /// 7. `stats_gold_spent_on_upgrades` — clamped ≥ 0 (`save.rs` does this when
    ///    parsing, `read_team_info`).
    /// 8. `TeamUnits` and `TeamInfo` are transferred as is (`TeamUnits::FileLoad`).
    ///
    /// # Intentional divergences
    ///
    /// * `Randomizer` is not serialized in V70: C++ reads the header's `rng_seed`
    ///   only for the lobby and does **not** touch the RNG state on load
    ///   (`saveload.cpp:776`, `Randomizer_SetSeed` is called only at game
    ///   start). For "load → tick" determinism the stream here
    ///   is re-seeded with `header.rng_seed`.
    /// * `Hash_MapHash`/`Hash_UnitHash` are not copied from the file as objects: they
    ///   are rebuilt from lists and ids (insertion order — file order).
    /// * `MessageLog`, `selected_unit`, camera/zoom and other UI fields are outside
    ///   `max_core` (state-model §6).
    pub fn load_state(&mut self, save: &crate::save::SaveFile) {
        use crate::enums::{AI_STATE_MOVE_FINISHED_REMINDER, AI_STATE_RETREAT_REMINDER};

        // --- map and game header ---
        self.map = Map::from_parts(save.surface(), save.resource_map());
        self.mission_index = save.header.mission_index;
        self.mission_category =
            crate::victory::MissionCategory::from_file_type(save.header.save_game_type);
        self.play_mode = save.header.play_mode;
        self.victory_type = save.ini_options.victory_type;
        self.victory_limit = save.ini_options.victory_limit;
        // Settings/UI fields/logs and AI V70 fields — for 1:1 save writing (P3).
        self.save_meta = crate::save_write::SaveMeta::from_save(save);

        // --- global game state ---
        self.game_state = save.game.game_state;
        self.turn_counter = save.game.turn_counter;
        self.active_turn_team = save.game.active_turn_team;
        self.player_team = save.game.player_team;
        // V70 does not store the RNG state (see "Intentional divergences").
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

        // --- teams: TeamInfo × 4 from the file, ALIEN is a service slot ---
        for team in 0..crate::save::SAVE_TEAM_COUNT {
            self.teams[team] = save.teams[team].clone();
        }
        self.teams[PLAYER_TEAM_COUNT].team_type = TeamKind::Computer;
        self.teams[PLAYER_TEAM_COUNT].team_clan = save.header.team_clan[PLAYER_TEAM_COUNT];

        // --- TeamUnits × 4 from the file; ALIEN — the service derelict team ---
        let saved_team_units = save.team_units();
        for team in 0..crate::save::SAVE_TEAM_COUNT {
            self.team_units[team] = saved_team_units[team].clone();
        }
        // The save has no derelict `TeamUnits`: C++ `GameManager_InitUnitsAndGameState`
        // initializes all five teams (`ResourceManager_InitClanUnitValues`, where
        // `PLAYER_TEAM_ALIEN` is `TEAM_CLAN_THE_CHOSEN`) before the file is loaded.
        self.team_units[PLAYER_TEAM_COUNT] =
            TeamUnits::with_clan(crate::enums::TeamClan::TheChosen);

        // --- units ---
        self.units.clear();
        self.presentations.clear();
        self.free_slots.clear();
        self.order_data.clear();
        self.unit_group.clear();
        self.groups.clear();
        self.lists = std::array::from_fn(|_| Vec::new());
        self.unit_hash = UnitHash::new(HASH_UNIT_HASH_SIZE);

        let mut object_ids: Vec<(u16, UnitId)> = Vec::with_capacity(save.units.len());

        for saved in &save.units {
            let mut unit = saved.info.clone();

            // `UnitInfo::FileLoad` (V70 branch) + normalizations.
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
            // References are resolved in a second pass by object number.
            unit.parent_unit = None;
            unit.enemy_unit = None;

            let slot = self.units.len();
            object_ids.push((saved.object_index, unit.id));
            self.units.push(Some(unit.clone()));
            // Presentation of the V70 object — parallel to the slot (F5.3/B).
            self.presentations.push(Some(saved.presentation.clone()));
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

        // Five lists — in file order (`SmartList_UnitInfo_FileLoad`).
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

        // `Hash_MapHash` — from the file (bucket/cell/unit order is preserved).
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
        // Safety net: a unit absent from the rebuilt hash is added by
        // its cell (otherwise `Access_*` would "not see" it).
        for id in self.snapshot_unit_ids() {
            let Some(unit) = self.unit(id) else { continue };
            let present = self
                .units_at_opt(unit.grid)
                .is_some_and(|ids| ids.contains(&id));
            if !present {
                self.map_hash_add(id, unit.grid, unit.flags);
            }
        }

        // --- heat maps: `HeatMap::LoadV70` with negative clamping ---
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

        // --- AI state: players from the save, task queues empty ---
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
    // Unit access
    // -----------------------------------------------------------------------

    /// Slot lookup by id via the `UnitHash` bucket (`Hash_UnitHash[...]`).
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

    /// A unit by id (`Hash_UnitHash[id]`; `None` — no unit).
    #[inline]
    pub fn unit(&self, id: UnitId) -> Option<&UnitInfo> {
        self.lookup(id)
            .and_then(|slot| self.units.get(slot).and_then(Option::as_ref))
    }

    /// Mutable unit by id.
    #[inline]
    pub fn unit_mut(&mut self, id: UnitId) -> Option<&mut UnitInfo> {
        let slot = self.lookup(id)?;
        self.units.get_mut(slot).and_then(Option::as_mut)
    }

    /// Unit presentation fields (`None` — no unit).
    ///
    /// Read only by `save.rs` (V70) and the `max_app` renderer; not part of the
    /// snapshot/CRC (F5.3/B).
    #[inline]
    pub fn presentation(&self, id: UnitId) -> Option<&UnitPresentation> {
        let slot = self.lookup(id)?;
        self.presentations.get(slot).and_then(Option::as_ref)
    }

    /// Mutable unit presentation.
    #[inline]
    pub fn presentation_mut(&mut self, id: UnitId) -> Option<&mut UnitPresentation> {
        let slot = self.lookup(id)?;
        self.presentations.get_mut(slot).and_then(Option::as_mut)
    }

    /// Writes the unit presentation (the slot must exist).
    pub fn write_presentation(&mut self, id: UnitId, presentation: UnitPresentation) {
        if let Some(slot) = self.lookup(id) {
            if let Some(entry) = self.presentations.get_mut(slot) {
                *entry = Some(presentation);
            }
        }
    }

    /// Whether a unit with this id exists.
    #[inline]
    pub fn has_unit(&self, id: UnitId) -> bool {
        self.lookup(id).is_some()
    }

    /// Number of live units.
    pub fn unit_count(&self) -> usize {
        self.units.iter().filter(|slot| slot.is_some()).count()
    }

    /// The unit's order data.
    pub fn order_data(&self, id: UnitId) -> OrderData {
        self.lookup(id)
            .and_then(|slot| self.order_data.get(slot).copied())
            .unwrap_or_default()
    }

    /// Order-data write.
    pub fn set_order_data(&mut self, id: UnitId, data: OrderData) {
        if let Some(slot) = self.lookup(id) {
            if let Some(entry) = self.order_data.get_mut(slot) {
                *entry = data;
            }
        }
    }

    /// Writes the modified unit copy back (see `unit_order`).
    ///
    /// If the cell changed, `Hash_MapHash` is updated — the analog of
    /// `Hash_MapHash.Remove/Add` in `UnitInfo::Move`/`UpdateGridPosition`.
    pub fn write_unit(&mut self, id: UnitId, unit: UnitInfo) {
        // The presentation table is parallel to the `units` slots (F5.3/B): when
        // a slot is added outside spawn, we align the length and provide a default.
        if self.presentations.len() < self.units.len() {
            self.presentations.resize(self.units.len(), None);
        }
        if let Some(slot) = self.lookup(id) {
            if self.presentations.get(slot).is_some_and(Option::is_none) {
                self.presentations[slot] = Some(UnitPresentation::new());
            }
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
                        previous.footprint(),
                    );
                    self.map_hash.add(
                        id,
                        unit.grid.x as u16,
                        unit.grid.y as u16,
                        unit.footprint(),
                        false,
                    );

                    // `Access_UpdateMapStatus` — scan coverage
                    // moves together with the unit (P6).
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

    /// `Hash_MapHash[Point]` — the cell's units in insertion order.
    #[inline]
    pub fn units_at(&self, grid: Point) -> &[UnitId] {
        self.units_at_opt(grid).map(Vec::as_slice).unwrap_or(&[])
    }

    /// `Hash_MapHash[Point]`: `None` if the cell is empty or off the map.
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

    /// `Hash_MapHash.Add` for external modules (`MoveInTransitUnitInMapHash`).
    /// The footprint is computed from flags (the C++ layout API takes flags).
    pub fn map_hash_add(&mut self, id: UnitId, grid: Point, flags: u32) {
        if grid.x >= 0 && grid.y >= 0 {
            self.map_hash.add(
                id,
                grid.x as u16,
                grid.y as u16,
                Footprint::from_flags(flags),
                false,
            );
        }
    }

    /// `Hash_MapHash.Remove` for external modules.
    pub fn map_hash_remove(&mut self, id: UnitId, grid: Point, flags: u32) {
        if grid.x >= 0 && grid.y >= 0 {
            self.map_hash.remove(
                id,
                grid.x as u16,
                grid.y as u16,
                Footprint::from_flags(flags),
            );
        }
    }

    /// `ResourceManager_GetPathsManager().RemoveRequest(unit)`.
    pub fn remove_path_request(&mut self, id: UnitId) {
        self.path_requests.retain(|request| request.unit != id);
    }

    // -----------------------------------------------------------------------
    // Group lists (`UnitInfoGroup`)
    // -----------------------------------------------------------------------

    /// The unit's group list (`UnitInfo::GetUnitList`).
    pub fn group_members(&self, id: UnitId) -> Option<Vec<UnitId>> {
        let slot = self.lookup(id)?;
        let group = self.unit_group.get(slot).copied().flatten()?;
        self.groups.get(group as usize)?.clone()
    }

    /// The unit's group number (`None` — the unit is outside a group).
    pub fn group_id_of(&self, id: UnitId) -> Option<u32> {
        let slot = self.lookup(id)?;
        self.unit_group.get(slot).copied().flatten()
    }

    /// `AllocateUnitList` + `AssignUnitList` for the leader: creates a group.
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

    /// Assigns a unit to a group (`UnitInfo::AssignUnitList`).
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

    /// `UnitInfo::ClearUnitList` — removes a unit from the group list.
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

    /// Removes a target from `UnitsManager_DelayedAttackTargets[team]`.
    pub fn remove_delayed_attack_target(&mut self, team: TeamId, id: UnitId) {
        if let Some(targets) = self.delayed_attack_targets.get_mut(team.0 as usize) {
            targets.retain(|unit| *unit != id);
        }
    }

    /// Removes a unit from `UnitsManager_PendingAttacks`.
    pub fn remove_pending_attack(&mut self, id: UnitId) {
        self.pending_attacks.retain(|unit| *unit != id);
    }

    /// Units of one of the five lists in traversal order (`UnitsManager_*Units`).
    #[inline]
    pub fn unit_list(&self, kind: UnitListKind) -> &[UnitId] {
        &self.lists[kind.index()]
    }

    /// All live units in the canonical order of the five lists.
    pub fn unit_ids_in_order(&self) -> Vec<UnitId> {
        let mut result = Vec::with_capacity(self.unit_count());
        for kind in UnitListKind::ALL {
            result.extend_from_slice(self.unit_list(kind));
        }
        result
    }

    /// All live units for the digest: the five lists in canonical order,
    /// then units outside lists (C++ does not process such units, but they
    /// exist in the state) in slot order.
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

    /// The unit's index in its list (`None` — not in a list).
    pub fn list_position(&self, id: UnitId) -> Option<usize> {
        let kind = self.unit(id)?.unit_list;
        self.lists[kind.index()].iter().position(|item| *item == id)
    }

    // -----------------------------------------------------------------------
    // Unit lists: C++ `UnitInfo::AddToDrawList` / `RemoveUnitFromUnitLists`
    // -----------------------------------------------------------------------

    /// `UnitInfo::GetDrawLayer` — the ground cover draw layer (`unitinfo.cpp:1191`).
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

    /// `UnitInfo::AddToDrawList` (`unitinfo.cpp:1230`) — insertion into one of the five
    /// lists while preserving draw order.
    pub fn add_to_draw_list(&mut self, id: UnitId) {
        self.add_to_draw_list_with(id, 0);
    }

    /// `UnitInfo::AddToDrawList(override_flags)` — the variant with flag
    /// override (C++ uses it when raising a bridge).
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
            // No class matched: in C++ the unit stays without a list.
            (UnitListKind::Particle, None)
        };

        if let Some(position) = position {
            let list = &mut self.lists[kind.index()];
            let position = position.min(list.len());
            list.insert(position, id);
            self.set_unit_list_kind(id, kind);
        }
    }

    /// Removes a unit from a concrete list (`SmartList::Remove`).
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

    /// `UnitInfo::MoveToFrontInUnitList` (`unitinfo.cpp:1926`) — moving to
    /// the front of the current list.
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
    // Spawn and destruction (the P3 subset needed by orders)
    // -----------------------------------------------------------------------

    /// Allocates the next `UnitId` (`UnitsManager_DeployUnit`, `++UnitIdCounter`).
    fn allocate_unit_id(&mut self, team: TeamId) -> UnitId {
        self.unit_id_counter = self.unit_id_counter.wrapping_add(1);
        let index = (self.unit_id_counter & 0x1FFF_u32) as u16;
        UnitId::new(team, index)
    }

    /// Unit spawn — `UnitsManager_DeployUnit` (`units_manager.cpp:2354`) in
    /// the P4 scope: id, name counters, starting orders by type, attributes,
    /// position and list.
    ///
    /// `flags` — extra caller-layer flags; they are merged with the
    /// type flags from `SC_UNITS` ([`crate::access::unit_type_flags`]), as
    /// `UnitsManager_DeployUnit` takes flags from the unit data.
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
        // C++ `UnitInfo::UnitInfo`: presentation fields (`brightness = 255`,
        // zeros) are created with the unit and live parallel to the slot (F5.3/B).
        let presentation = UnitPresentation::new();
        // Type flags — from the `SC_UNITS` table (`Unit::m_flags`); explicit caller
        // flags are merged with them (a P3-compatible input).
        // The C++ code below (`DeployUnit`, `UnitInfo::SetPosition`) reads the already
        // assembled `unit->flags`, so from here on
        // `unit_flags` is used, not just the input `flags`.
        //
        // The team color bits (`HASH_TEAM_*`) are added in C++ by the `UnitInfo`
        // constructor (`unitinfo.cpp:179`). The port does not write them into `flags`: the
        // set of flags enters the canonical state digest, and the reference
        // C++ vector (`tools/sim_vectors`) was built without them. The team color
        // is restored by the same slot→color mapping from `unit.team`
        // (`max_app::overlays::team_color_index`), and if the unit came from a save
        // with the bits already set — they are honoured.
        let merged_flags = flags | crate::access::unit_type_flags(unit_type);
        unit.flags = merged_flags;

        // `UnitsManager_DeployUnit` (`units_manager.cpp:2443-2450`): for turrets
        // `UpdateTurretAngle(angle)`, for a spinning turret `image_index_max`
        // is computed from the **turret** frames, not the hull.
        if merged_flags & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0 {
            unit.turret_image_index = unit.turret_image_base.wrapping_add(i16::from(unit.angle));
            if merged_flags & unit_flags::SPINNING_TURRET != 0 {
                let frames = crate::frames::frame_info(unit_type);
                unit.image_index_max = unit.turret_image_base + frames.turret_image_count - 1;
            }
        }

        // `UnitInfo::UnitInfo`: attributes from the team's current values.
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

        // `UnitsManager_DeployUnit` (`units_manager.cpp:2382-2384`): a new
        // unit has empty storage — `if (GameManager_GameState != GAME_STATE_12_DEPLOYING_UNITS
        // && !QuickBuild_MenuActive && !is_existing_unit) unit->storage = 0;`.
        // Previously the port kept `ATTRIB_STORAGE`, and a just-built
        // `ADUMP`/`FDUMP`/`GOLDSM` appeared full — "free" raw material
        // entered the complex pool. During landing (`DeployingUnits`) C++
        // does not zero the storage; concrete cargo is set by
        // `GameManager_ProcessTeamMissionSupplyUnits` (`deploy.rs`).
        if self.game_state != GameStateType::DeployingUnits {
            unit.storage = 0;
        }

        // `UnitInfo::InitStealthStatus` (`unitinfo.cpp:2796`): mines, commandos
        // and submarines are invisible to all; the rest are visible only with
        // `GameManager_AllVisible`, and the own team always sees them. Previously here
        // all ordinary units were marked visible to **all** teams — because of
        // this, foreign bases were visible on the map and minimap without reconnaissance.
        crate::combat::init_stealth_status(&mut unit, self.ai.all_visible);

        // `UnitsManager_DeployUnit`: the type name counter in the team.
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
            // `UnitInfo::UnitInfo` (`unitinfo.cpp:288-294`) reads `image_index`
            // from the type's frame data, and `UnitsManager_DeployUnit`
            // (`units_manager.cpp:2390`) translates `ANIMATED` into
            // `ORDER_EXPLODE/ORDER_STATE_DESTROY`. The flag is taken from the **final**
            // flags: the caller layer (`UnitsManager_StartExplosion`) passes
            // only the class (MOBILE_LAND/…), while `ANIMATED` comes from `SC_UNITS`.
            unit.orders = UnitOrder::Explode;
            unit.state = UnitOrderState::Destroy;
        }

        // Starting orders by type (`UnitsManager_DeployUnit`, switch).
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
        if unit.is_quad() {
            unit.x = unit.x.wrapping_add((GFX_MAP_TILE_SIZE / 2 - 1) as u16);
            unit.y = unit.y.wrapping_add((GFX_MAP_TILE_SIZE / 2 - 1) as u16);
        }

        // `UnitInfo::UnitInfo` (`unitinfo.cpp:316`): a building immediately gets
        // **its own** complex — `AttachComplex(CreateComplex(team))`, and
        // `Complex::Grow` sets `buildings = 1`. Without this the mine had no
        // `unit->GetComplex()`, the `allocate` dialog (`AllocMenu`) did not open,
        // and `Access_UpdateResourcesTotal` did not see the complex mining.
        // Merging with connector neighbors is done by
        // [`crate::complex::update_connectors`] below — like
        // `UnitsManager_UpdateConnectors` after the building scaling
        // completes (`units_manager.cpp:3166`).
        if unit.is_complex_member() {
            unit.complex = self.attach_new_complex(team);
        }

        // Footprint — before moving `unit` into the slot (the 2×2 layout goes to `MapHash`).
        let footprint = unit.footprint();

        // Slot: free (LIFO, like `SmartObjectArray` reuse) or new.
        let slot = match self.free_slots.pop() {
            Some(slot) => {
                self.units[slot as usize] = Some(unit);
                self.presentations[slot as usize] = Some(presentation);
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
                self.presentations.push(Some(presentation));
                self.order_data.push(OrderData {
                    move_to: grid,
                    fire_on: Point::ZERO,
                });
                self.unit_group.push(None);
                self.units.len() - 1
            }
        };
        // `Hash_UnitHash.PushBack(unit)` — bucket by `id % hash_size`.
        self.unit_hash.push_at(u32::from(id.0), slot as u16);
        // `Hash_MapHash.Add` (`units_manager.cpp`): the cell (for 2×2 structures —
        // three more cells).
        self.map_hash
            .add(id, grid.x as u16, grid.y as u16, footprint, false);

        self.add_to_draw_list(id);

        // `Access_UpdateMapStatus(unit, true)` — scan coverage (P6).
        self.update_map_status(id, true);

        // `UnitsManager_UpdateConnectors(unit)` (`units_manager.cpp:2051`):
        // the building searches neighbors in eight directions, merges complexes
        // (`AttachToPrimaryComplex`) and recomputes stocks. In C++ the call happens
        // after `ORDER_AWAIT_SCALING`/`EXPAND` completes; in the port the building already stands
        // on the map, so connectors are computed immediately — otherwise the factory would not see
        // the complex materials until the end of the animation. A building in `ORDER_IDLE`
        // (ready but not yet unfolded) does not join the complex: its order
        // is restored by `UnitsManager_BuildingReady`, and then `update_connectors`
        // is called from `process_order_await_scaling`.
        if self
            .unit(id)
            .is_some_and(|unit| unit.is_complex_member() && unit.orders != UnitOrder::Idle)
        {
            crate::complex::update_connectors(self, id);
        }

        Some(id)
    }

    /// `UnitInfo::AttachComplex` (`unitinfo.cpp:1978`) in the model scope:
    /// a new team complex (`TeamUnits::CreateComplex`) + `Complex::Grow`.
    ///
    /// Returns the complex id; `None` if the team has no slot.
    fn attach_new_complex(&mut self, team: TeamId) -> Option<crate::team::ComplexId> {
        let team_units = self.team_units.get_mut(team.0 as usize)?;
        let id = team_units.create_complex();
        let complex = team_units.get_complex_mut(id)?;
        complex.grow();
        Some(id)
    }

    /// `UnitsManager_RemoveConnections` (`units_manager.cpp:1556`) in the
    /// model scope: clearing paired bits on neighbors and splitting the complex
    /// (`UnitInfo::DetachComplex`).
    ///
    /// C++ calls `RemoveConnections` from `UnitsManager_BuildClearing` before
    /// `DestroyUnit`; in the port destruction goes through [`Self::destroy_unit`],
    /// so the complex split is done here too.
    fn remove_connections(&mut self, id: UnitId) {
        crate::complex::remove_connections(self, id);
    }

    /// Unit destruction — a subset of `UnitsManager_DestroyUnit`
    /// (`units_manager.cpp`) sufficient for the P4 loop: removal from lists,
    /// hashes, attacks and events, scan coverage and AI tasks.
    ///
    /// TODO(P3): complexes, cargo, trails/rubble, loss statistics.
    pub fn destroy_unit(&mut self, id: UnitId) {
        let Some(slot) = self.lookup(id) else {
            return;
        };
        let placement = self.unit(id).map(|unit| (unit.grid, unit.footprint()));
        // End the attack before removing from the hash (`UnitsManager_EndAttack` finds
        // the unit by id).
        self.end_attack(id);

        // `Access_UpdateMapStatus(unit, false)` and `Ai_RemoveUnit` (P6).
        self.update_map_status(id, false);
        crate::ai::ai_remove_unit(self, id);

        // `UnitsManager_RemoveConnections` → `DetachComplex` (`units_manager.cpp:1569`):
        // the building leaves the complex, the remaining connected groups get their
        // own complexes, connector bits are cleared on neighbors.
        if let Some(unit) = self.unit(id) {
            if unit.is_complex_member() {
                self.remove_connections(id);
            }
        }

        self.clear_group_member(id);
        self.remove_unit_from_unit_lists(id);
        self.unit_hash.remove_at(u32::from(id.0), &(slot as u16));
        if let Some((grid, footprint)) = placement {
            self.map_hash
                .remove(id, grid.x as u16, grid.y as u16, footprint);
        }
        if let Some(entry) = self.units.get_mut(slot) {
            *entry = None;
        }
        if let Some(entry) = self.presentations.get_mut(slot) {
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
    // Orders: inter-unit operations (C++ `UnitsManager_SetNewOrderInt` etc.)
    // -----------------------------------------------------------------------

    /// `UnitsManager_SetNewOrderInt` for an arbitrary unit
    /// (`units_manager.cpp:1945`); returns `true` if a path request was removed.
    ///
    /// A new order does **not** cancel construction: in C++ the builder cannot be moved
    /// by a click from an unfinished site (`CURSOR_UNIT_NO_GO`,
    /// `game_manager.cpp:5927-5931`), and cancellation goes only through an explicit `stop`
    /// (`ORDER_BUILD`/`BUILD_CANCEL` → `UnitInfo::CancelBuilding`).
    pub fn set_order(&mut self, id: UnitId, order: UnitOrder, state: UnitOrderState) -> bool {
        let Some(mut unit) = self.unit(id).cloned() else {
            return false;
        };
        // `UnitsManager_NewOrderWhileScaling` (`units_manager.cpp:2529`): if
        // the order changes during `EXPAND`, C++ immediately calls
        // `UnitsManager_UpdateConnectors`, without waiting for the animation to end.
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
                    // C++ `--pin_count` without saturation; a possible "minus" is
                    // the exact oracle behavior (DESIGN §6.3).
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
    /// Target pinning — `Access_GetAreaAttackTargets` for an area attack,
    /// otherwise `Access_GetAttackTarget` by `fire_on`. Each target gets
    /// `pin_count += 1`; it cannot leave the cell/map until `EndAttack`.
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
        // The `disable_fire` setting disables reaction fire entirely.
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
    /// flag reset, `delayed_reaction` of live units and all target lists.
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
    // Walk over the five lists (DESIGN §5.2, `units_manager.cpp:1860`)
    // -----------------------------------------------------------------------

    /// `UnitsManager_ProcessOrders` (`units_manager.cpp:1860`) — one pass over
    /// the five lists in fixed order.
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

        // In C++ this is `Remote_ProcessNetPackets()`; in max_core the command queue
        // is applied at the start of the tick (see `tick`), hence pass.

        if !self.orders_pending {
            self.process_pending_attacks();
        }

        // `UnitsManager_PendingAirGroupLeader` → `Access_ProcessNewGroupOrder`:
        // the leader stays in the field until the group path is allowed (C++).
        if let Some(leader) = self.pending_air_group_leader {
            if crate::access::process_new_group_order(self, leader) {
                self.pending_air_group_leader = None;
            }
        }

        // `UnitEvent_UnitEvents`: Process() each, then Clear().
        let events = std::mem::take(&mut self.unit_events);
        for event in &events {
            crate::events::process_unit_event(self, event);
        }
        self.unit_events.clear();

        self.reset_bob_state = self.bob_effect_quota > 0;
    }

    /// `UnitsManager_ProcessUnitOrders` — a walk over one list.
    ///
    /// C++ iterates `SmartList` from `Begin()` to a fixed `End()`;
    /// here — a snapshot of the list ids at the start of the walk. Units removed during
    /// the walk are skipped (as in C++: the `SmartList` iterator after
    /// `Remove` of the current node is not continued), ones added to the start/end
    /// of the list are not visited in this pass.
    pub fn process_unit_orders(&mut self, kind: UnitListKind) {
        let ids = self.lists[kind.index()].clone();
        for id in ids {
            if self.has_unit(id) {
                unit_order::process_order(self, id);
            }
        }
    }

    /// The head of `UnitsManager_PendingAttacks` (`units_manager.cpp:1884-1930`).
    fn process_pending_attacks(&mut self) {
        if self.pending_attacks.is_empty() {
            // `UnitsManager_AssessAttacks()` — checking delayed reactions.
            crate::combat::assess_attacks(self);
            return;
        }

        let id = self.pending_attacks[0];
        let Some(unit) = self.unit(id).cloned() else {
            self.pending_attacks.remove(0);
            return;
        };

        if unit.hits == 0 || unit.state == UnitOrderState::Destroy {
            // The attacker dies — `DestroyUnit` ends its attack.
            self.pending_attacks.remove(0);
        } else if unit.orders != UnitOrder::Fire {
            // The hit animation only pauses fire; any other
            // order cancels the attack.
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
                // Failure: the deferred order was fire.
                unit.orders = UnitOrder::Await;
                unit.state = UnitOrderState::ExecutingOrder;
            }
            self.write_unit(id, unit);
            self.pending_attacks.remove(0);
        }
    }

    // -----------------------------------------------------------------------
    // End of turn (DESIGN §5.3, `game_manager.cpp:2042-2087`)
    // -----------------------------------------------------------------------

    /// `GameManager_ManageEconomy` — updating the team's production, supply and
    /// research at the start of its turn.
    pub fn manage_economy(&mut self, team: TeamId) {
        // `team_units->OptimizeComplexes(team)` (`teamunits.cpp:170`):
        // `Access_UpdateResourcesTotal(complex)` — the complex stocks (including
        // mine mining by their `raw/fuel/gold_mining`) are recomputed at the start
        // of the turn; then `ProductionManager_OptimizeProduction` — turning
        // power producers on, turning consumers off on a deficit and
        // pausing constructions (`production_manager.cpp:830-901`).
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

        // Repeated optimization is **not** needed here: `update_production` does not
        // reset the pause of stationary factories (its decision was made by
        // `ProductionManager_OptimizeProduction` above), and
        // `Access_UpdateResourcesTotal` after distributing storages would restore the
        // complex cache to "inventory + mining" (C++ `ManageEconomy` does not do this).
        if let Some(team_info) = self.teams.get_mut(team.0 as usize) {
            for index in 0..RESEARCH_TOPIC_COUNT {
                let topic = team_info.research_topics[index];
                if topic.allocation != 0 && topic.turns_to_complete == 0 {
                    // `ResearchMenu_UpdateResearchProgress(team, i, 0)`:
                    // compute turns for the new level (C++ `manage_economy`).
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
    // Research (P6: `researchmenu.cpp` / `upgradecontrol.cpp`)
    // -----------------------------------------------------------------------

    /// `ResearchMenu_UpdateResearchProgress(team, topic, allocation)` —
    /// change the number of a topic's research centers and recompute progress.
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

    /// The current allocation of research centers by topic (C++
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

    /// `ResearchMenu_OnClick_Done` (`researchmenu.cpp:518`): apply the new
    /// allocation of centers by topic.
    ///
    /// For each enabled `RESEARCH` center: if the target allocation
    /// has room for its topic — keep it; otherwise move it to the first topic with
    /// room (updating allocation `-1/+1`), and if there is no room — turn it off
    /// (`ORDER_POWER_OFF`/`INIT`, as in C++).
    ///
    /// Returns the actual allocation (C++ does not guarantee the sum equals the number
    /// of centers if the target exceeds the number of centers).
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

    /// `ResearchMenu_NewTurn(team)` — the team's research turn: deduct
    /// allocation, complete levels, apply upgrades to current values.
    pub fn advance_research(&mut self, team: TeamId) -> Vec<crate::research::ResearchEvent> {
        let index = team.0 as usize;
        if index >= TEAM_SLOTS {
            return Vec::new();
        }
        let teams = &mut self.teams;
        let team_units = &mut self.team_units;
        crate::research::research_new_turn(&mut team_units[index], &mut teams[index], team)
    }

    /// Takes the research events of the last turn (for the HUD log).
    pub fn take_research_events(&mut self) -> Vec<crate::research::ResearchEvent> {
        std::mem::take(&mut self.research_events)
    }

    /// Takes combat sound events (for `AudioPlayer`).
    ///
    /// Accumulated by `PrepareFire`/`UnitsManager_StartExplosion`; the client reads
    /// them (`max_app::App::collect_combat_sounds`) right after the tick.
    pub fn take_unit_sounds(&mut self) -> Vec<UnitSound> {
        std::mem::take(&mut self.unit_sounds)
    }

    /// `UnitInfo::UpdateProduction` (`unitinfo.cpp:2904`) — reset of speeds,
    /// shots, supply and **distribution of the complex stock over building storages** at
    /// the start of the turn.
    ///
    /// The exact C++ order:
    /// 1. a mobile builder in `ORDER_BUILD` pays with raw material
    ///    (`Cargo_GetRawConsumptionRate`), having first cut `build_rate` to
    ///    `BuildMenu_GetMaxPossibleBuildRate`;
    /// 2. `COMMTWR` in `ORDER_POWER_ON` adds gold to the team
    ///    (`Cargo_GetGoldConsumptionRate` = 5);
    /// 3. `GREENHSE` in `ORDER_POWER_ON` — `++storage` and `++team_points`;
    /// 4. shots/speed/disable;
    /// 5. if the unit has a complex — `storage = min(complex.<field>,
    ///    ATTRIB_STORAGE)`, `complex.<field> -= storage`.
    ///
    /// Step 5 is the "mine mining → storages" transfer: `Access_UpdateResourcesTotal`
    /// (see [`Self::update_complex_resources`]) puts the mining into the complex field,
    /// and here it is distributed over storages (`ADUMP`/`FDUMP`/`GOLDSM` etc.).
    /// The remainder beyond the `ATTRIB_STORAGE` sum stays in the complex field and is lost
    /// on the next recomputation — as in C++.
    pub fn update_production(&mut self, unit: &mut UnitInfo) {
        let team_index = unit.team.0 as usize;

        // The mobile builder's pause flag is recomputed every turn:
        // only the payment branch below sets `true`. The pause of a **stationary**
        // factory belongs to `ProductionManager::progress` (the power
        // optimizer) and is not reset here — otherwise the shutdown decision
        // would be lost between `ManageEconomy` and `ProgressBuildState`.
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
                // build_rate)` (`unitinfo.cpp:2914`) charges raw material **always**
                // and lets the storage go negative; `GameManager_SpawnNewUnits`
                // then advances `build_time` anyway, and the construction
                // "completes on debt". The port must keep the resource
                // non-negative, so charging happens only with
                // a sufficient remainder, and an unpaid turn is marked
                // `build_paused` — such a turn makes no progress
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
                // A disable that ended at zero stays zero.
                if unit.disabled_turns_remaining > 0 {
                    unit.disabled_turns_remaining -= 1;
                }

                if unit.disabled_turns_remaining == 0 {
                    if unit.orders == UnitOrder::Idle {
                        unit.prior_orders = UnitOrder::Await;
                        unit.prior_state = UnitOrderState::ExecutingOrder;
                    } else {
                        unit_order::restore_orders(unit);

                        // A hit during disable left DISABLE
                        // as a deferred order; `RestoreOrders` preserves it
                        // for powered buildings.
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

    /// `UnitInfo::Resupply` (`unitinfo.cpp:2885`) — ammo replenishment from
    /// the complex stock (`Complex::GetCargoInfo` + `Complex::Transfer(-1,0,0)`)
    /// and regeneration.
    ///
    /// `Regenerate()` (green alien units) is absent from the model —
    /// TODO, as in the rest of the port.
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

    /// `GameManager_UpdateScoreGraph` (`game_manager.cpp:2078`): shift the score
    /// graph one place left, the last element — the team's current points.
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

    /// The common end-of-turn barrier counter (packet 52) if **all** playing
    /// teams sent the same value — `Remote_ServerWaitBarrier`
    /// (`max-brh-docs/protocol.md` §8.6: "waits until all remote teams send
    /// the same counter value").
    ///
    /// `None` — the barrier is not assembled yet (or the teams diverged).
    pub fn end_turn_barrier(&self) -> Option<u8> {
        self.common_barrier(|state, team| state.turn_indices[team])
    }

    /// The common start-of-turn barrier counter (packet 00) — the analog of
    /// [`Self::end_turn_barrier`] by `Remote_FrameSyncCounter`
    /// (`Remote_WaitBeginTurnAcknowledge`, `remote.cpp:1876-1933`).
    ///
    /// `None` — not all playing teams entered the round with one counter.
    pub fn begin_turn_barrier(&self) -> Option<u8> {
        self.common_barrier(|state, team| state.frame_sync_counter[team])
    }

    /// `Remote_StateSyncIndices[team]` — the last applied index of 54.
    #[inline]
    pub fn state_sync_index(&self, team: TeamId) -> u8 {
        self.state_sync_indices
            .get(team.0 as usize)
            .copied()
            .unwrap_or(0)
    }

    /// The common next-turn number of the CRC barrier (packet 45): all playing
    /// teams agree. `None` — the barrier is not assembled (desync/waiting).
    pub fn crc_barrier(&self) -> Option<u8> {
        self.common_barrier(|state, team| state.next_turn_indices[team])
    }

    /// The common barrier value by teams (`None` — no agreement).
    ///
    /// `None`/`Eliminated` teams do not participate (as in `Remote_ServerWaitBarrier`).
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

    /// `GameManager_TeamTurnFinish` (`game_manager.cpp:583`) — closing the turn:
    /// victory/defeat check (P7), `finished_turn` reset, score graph,
    /// turn counter.
    ///
    /// Returns `true` if the game is over
    /// ([`crate::victory::check_end_game_conditions`] — a port of
    /// `menu_check_end_game_conditions`): then C++ does not reset
    /// `finished_turn` and does not increment the turn counter.
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

        // `GameManager_ManageEconomy` → `GameManager_ProgressBuildState` for
        // each playing team in RED..GRAY order (C++ `UpdateGui`): the production
        // turn runs right after the turn switch, before AI/player turns.
        // Call order 1:1: turn cost charging → `SpawnNewUnits`
        // (`GroundCover → MobileLandSea → Stationary → MobileAir`).
        self.production_events.clear();
        self.research_events.clear();
        self.industry_message = None;

        for team in 0..PLAYER_TEAM_COUNT {
            let kind = self.teams[team].team_type;
            if kind == TeamKind::None || kind == TeamKind::Eliminated {
                continue;
            }

            // `GameManager_ProgressBuildState`: first `ResearchMenu_NewTurn`
            // (P6), then `SpawnNewUnits` — the C++ order.
            let research = self.advance_research(TeamId(team as u8));
            self.research_events.extend(research);

            let events = crate::production::progress_build_state(self, TeamId(team as u8));
            self.production_events.extend(events);
        }

        false
    }

    /// Takes the production events of the last turn (for the HUD log).
    pub fn take_production_events(&mut self) -> Vec<crate::production::ProductionEvent> {
        std::mem::take(&mut self.production_events)
    }

    /// Appends a line to the complex power message — C++
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

    /// Takes the accumulated power message (for `hud.hint`).
    pub fn take_industry_message(&mut self) -> Option<String> {
        self.industry_message.take()
    }

    // -----------------------------------------------------------------------
    // Tick loop (DESIGN §5.1, `GameManager_ProcessTick`, `game_manager.cpp:5144`)
    // -----------------------------------------------------------------------

    /// One simulation tick.
    ///
    /// Order (C++ `GameManager_ProcessTick`):
    /// 1. `PathsManager.PollResults()` — path results (P5);
    /// 2. apply input commands (`Remote_Synchronize`/`ProcessNetPackets`);
    /// 3. if the phase ≠ `TURN_ACTIVE` — `UnitsManager_ProcessOrders()`;
    /// 4. if the phase ≠ `TURN_ACTIVE` and there is no `OrdersPending`: `Ai_CheckReactions`
    ///    via the `update_flag` alternation, `DispatchJobs`, `Ai_CheckReactions`,
    ///    `Ai_CheckEndTurn`.
    pub fn tick(&mut self) {
        // Combat sounds accumulate until the client reads them (`App::collect_combat_sounds`
        // right after the tick). Headless runs do not take them, so the queue
        // is trimmed: events are a UI report, not game state.
        const MAX_UNIT_SOUNDS: usize = 256;
        if self.unit_sounds.len() > MAX_UNIT_SOUNDS {
            let excess = self.unit_sounds.len() - MAX_UNIT_SOUNDS;
            self.unit_sounds.drain(0..excess);
        }

        // 1. `ResourceManager_GetPathsManager().PollResults()` — P5.
        self.poll_path_results();

        // 2. Input commands: in arrival order.
        self.apply_pending_commands();

        // 3. Orders.
        if self.game_state != GameStateType::TurnActive {
            self.process_orders();
        }

        // 4. AI reactions and end of turn.
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

    /// Applies all accumulated commands via [`crate::events::apply_command`].
    pub fn apply_pending_commands(&mut self) -> usize {
        crate::events::apply_pending(self)
    }

    /// Enqueues a command (in receive order).
    pub fn enqueue(&mut self, command: Command) {
        self.commands.push(command);
    }

    /// `ResourceManager_GetPathsManager().PollResults()` — P5 pathfinding.
    ///
    /// Synchronously computes and applies all requests in submission order
    /// (DESIGN risk 2): the `GameState::path_requests` queue.
    fn poll_path_results(&mut self) {
        crate::pathfinding::poll_results(self);
    }

    /// `ResourceManager_GetPathsManager().DispatchJobs()` — in the synchronous model
    /// the work is done in [`Self::poll_path_results`]; the call site is kept
    /// so that the tick order matches C++ (DESIGN §5.1).
    fn dispatch_path_jobs(&mut self) {
        crate::pathfinding::dispatch_jobs(self);
    }

    /// `Ai_CheckReactions` (`ai.cpp:407`) — AI reactions to spotted targets,
    /// processing the reminder queue via the `Ai_AiReactionState` alternation.
    fn check_reactions(&mut self) {
        crate::ai::ai_check_reactions(self);
    }

    /// `Ai_CheckEndTurn` (`ai.cpp:239)` — checking end-of-turn conditions.
    ///
    /// For computer teams the decision is made by the AI player
    /// (`AiPlayer::CheckEndTurn`); humans and remote teams finish the turn
    /// with packet 06.
    pub fn check_end_turn(&mut self) {
        crate::ai::ai_check_end_turn(self);
    }

    // -----------------------------------------------------------------------
    // Canonical state digest (for determinism tests, groundwork for P7/crc)
    // -----------------------------------------------------------------------

    /// The canonical byte digest of the state.
    ///
    /// Encodes **only** simulation fields in a deterministic order:
    /// map (surface + resources), settings affecting the sim, the five unit
    /// lists (insertion order), the unit's `OrderData`, live attacks, the phase and
    /// barriers. The format is an explicit little-endian writer without `serde` and without
    /// map iteration (DESIGN §6.4): it is the groundwork for `crc.rs` (P7) and a snapshot for
    /// comparing two runs.
    ///
    /// Why not `save.rs`: `save.rs` is the V70 format of the real game's **save
    /// file**; there is no `GameState → V70` writer yet (P7, DESIGN §8.2 item 5), and
    /// the digest must be stable between runs already at P4.
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

            // Team complexes — C++ `TeamUnits::complexes` (ascending id
            // order maintained by `CreateComplex`): id, stocks and
            // building count. The `Complex::{raw,fuel,gold,power,workers}` fields
            // are the `Access_UpdateResourcesTotal` cache, but they affect the display and
            // `Complex::Transfer`, so they enter the snapshot/CRC (`state_digest`).
            let complexes = &self.team_units[team].complexes;
            put_u32(&mut out, complexes.len() as u32);
            for complex in complexes {
                put_i16(&mut out, complex.id);
                put_i16(&mut out, complex.raw);
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

        // Delayed reactions (`UnitsManager_DelayedAttackTargets`).
        out.push(self.delayed_reactions_team.0);
        for targets in &self.delayed_attack_targets {
            put_u32(&mut out, targets.len() as u32);
            for id in targets {
                put_u16(&mut out, id.0);
            }
        }

        // Path request queue (order = submission order).
        put_u32(&mut out, self.path_requests.len() as u32);
        for request in &self.path_requests {
            put_u16(&mut out, request.unit.0);
            put_i16(&mut out, request.destination.x);
            put_i16(&mut out, request.destination.y);
            put_u32(&mut out, request.flags);
            put_i32(&mut out, request.minimum_distance);
            out.push(u8::from(request.board_transport));
        }

        // Group lists (`UnitInfoGroup`): group numbers by slot.
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

        // Units: canonical order — five lists (+ units outside lists).
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
            // Unit attributes (`base_values`) — 14 fields.
            for attribute in 0..crate::unit_values::ATTRIB_COUNT {
                put_i16(&mut out, unit.base_values.get_attribute(attribute) as i16);
            }
        }

        // AI state (P6): tasks, reminders, players, heat maps.
        self.ai.digest_into(&mut out);

        out
    }
}

// ---------------------------------------------------------------------------
// Digest encoders
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

/// `UnitInfo::UpdateProduction`, step 5: take from the complex up to `capacity`
/// resource units into the building storage (`storage = min(complex->field, ATTRIB_STORAGE)`,
/// `complex->field -= storage`). Returns the new `storage` value.
///
/// Complex fields are `int16_t` (C++), so the assignment truncates
/// (`wrapping_*`), even if `capacity` is greater.
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
            let value = i32::from(target.raw).min(capacity);
            target.raw = target.raw.wrapping_sub(value as i16);
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
// Helpers: complexes
// ---------------------------------------------------------------------------

impl GameState {
    /// A team complex by id (`TeamUnits::GetComplex`).
    pub fn complex(&self, team: TeamId, complex_id: u16) -> Option<&Complex> {
        self.team_units
            .get(team.0 as usize)?
            .complexes()
            .iter()
            .find(|complex| complex.id == complex_id as i16)
    }

    /// Mutable team complex by id.
    pub fn complex_mut(&mut self, team: TeamId, complex_id: u16) -> Option<&mut Complex> {
        self.team_units
            .get_mut(team.0 as usize)?
            .complexes
            .iter_mut()
            .find(|complex| complex.id == complex_id as i16)
    }

    /// `TeamUnits::OptimizeComplexes(team)` (`teamunits.cpp:170`) in the
    /// economy scope: for each team complex — `Access_UpdateResourcesTotal`
    /// (`access.cpp:912`), i.e. recomputation of `raw/fuel/gold/power/workers`
    /// from buildings (`Cargo_GetNetProduction + Cargo_GetInventory`).
    ///
    /// Mine mining enters here via `raw_mining`/`fuel_mining`/
    /// `gold_mining` (see [`crate::alloc`]), so the allocation from the
    /// `allocate` dialog directly changes the complex income per turn.
    ///
    /// Complex order — the `TeamUnits::complexes` order (the same as in the
    /// C++ list: ascending id).
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
            // `UpdateResourcesTotal` touches only complex fields; the composition
            // of buildings and their orders does not change — a loop over an id snapshot is safe.
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
        // `CTInfo::Reset` starts name counters at 1 (`ctinfo.hpp:49-51`).
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
        // P7: end of turn first checks victory/defeat
        // (`menu_check_end_game_conditions`); a live team + the limit not reached →
        // the game continues. The network branch is disabled (local semantics).
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
        // `VICTORY_TYPE_DURATION` with a zero limit: `limit <= turn_counter` → over.
        state.victory_type = VictoryType::Duration;
        state.victory_limit = 0;
        state.game_state = GameStateType::EndTurn;
        assert!(state.finish_turn());
        assert_eq!(state.turn_counter, 0);
    }

    #[test]
    fn pending_attack_head_is_settled_like_cpp() {
        let (mut state, first, _second) = tank_state();
        // An attacker with zero hits is simply removed from the queue.
        {
            let unit = state.unit_mut(first).unwrap();
            unit.hits = 0;
            unit.orders = UnitOrder::Fire;
            unit.state = UnitOrderState::AttackPending;
        }
        state.pending_attacks.push(first);
        state.process_orders();
        assert!(state.pending_attacks.is_empty());

        // An attacker whose order is not FIRE — the attack is cancelled.
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

        // One unit in each of the five lists, in the canary order
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

        // `ORDER_FIRE` in INIT state adds the unit in one pass to
        // `PendingAttacks` and `LiveAttacks` — insertion order is the order
        // of list traversal.
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
        // A missile (`MISSILE_UNIT`) at the head of `UnitsManager_ProcessOrder` sets
        // `OrdersPending`, so the attack queue is not processed this tick —
        // as in C++.
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

            // A series of shots: each FIRE order is one shot (after
            // the shot `RestoreOrders` returns AWAIT); the start of the turn
            // restores `shots` (`UpdateProduction`).
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
        // 4 shots of 6 damage (24 hp) destroy the target; the behavior
        // is pinned so that a formula regression is visible.
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

            // 13: seed; 08: order with targets (P3 payload); 38: ready path;
            // 50: emergency stop; 41: movement finished; 06/52: end of turn.
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
        // Spend speed and shots.
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

        // The barrier is not there yet: default values are equal (0), but this is
        // the "before turn start" agreement — as in C++, counters start at 0.
        assert_eq!(state.end_turn_barrier(), Some(0));

        state.turn_indices[0] = 3;
        assert_eq!(
            state.end_turn_barrier(),
            None,
            "команды разошлись по счётчику"
        );
        state.turn_indices[1] = 3;
        assert_eq!(state.end_turn_barrier(), Some(3));
        // ALIEN/empty slots do not participate in the barrier.
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

        // The 54 barrier completes only on a full (`is_last`) state.
        assert_eq!(state.state_sync_index(TeamId::GREEN), 0);
        state.state_sync_indices[1] = 4;
        assert_eq!(state.state_sync_index(TeamId::GREEN), 4);
    }

    #[test]
    fn snapshot_is_stable_across_clones() {
        let (state, first, _second) = tank_state();

        // Two independent runs of the same command sequence
        // give a byte-identical state.
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

        // An order change changes the digest.
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

    /// `ResourceManager_InitClanUnitValues` (`resource_manager.cpp:1275-1290`):
    /// the derelict team is always `TEAM_CLAN_THE_CHOSEN`, independent of the
    /// menu clan settings.
    #[test]
    fn derelict_team_uses_the_chosen_clan() {
        let mut state = GameState::with_setup(
            crate::map::Map::new(),
            [
                TeamKind::Player,
                TeamKind::None,
                TeamKind::None,
                TeamKind::None,
            ],
        );
        let alien = PLAYER_TEAM_COUNT;
        assert_eq!(
            state.teams[alien].team_clan,
            crate::enums::TeamClan::TheChosen
        );
        let expected = crate::unit_values::team_base_values(crate::enums::TeamClan::TheChosen);
        assert_eq!(state.team_units[alien].base_values, expected);
        assert_eq!(state.team_units[alien].current_values, expected);
        assert!(state.team_units[alien].complexes().is_empty());

        // The Chosen bonuses touch air units only: alien debris types keep the
        // `SC_ATTRI` values (the same observable result as in C++).
        assert_eq!(
            state.team_units[alien].base_values[UnitType::AlienTank as usize],
            crate::unit_values::base_unit_values(UnitType::AlienTank)
        );
    }

    // -----------------------------------------------------------------------
    // Phase 1: turn state machine (U)
    // -----------------------------------------------------------------------

    /// `GameManager_TeamTurnFinish` closes the turn only from `GAME_STATE_END_TURN`.
    /// Any other phase is a no-op: the counter and the phase stay untouched.
    #[test]
    fn finish_turn_is_guarded_by_the_end_turn_phase() {
        let (mut state, _red, _green) = tank_state();
        state.network_game = false;
        state.victory_limit = 50;

        for phase in [
            GameStateType::InGame,
            GameStateType::TurnActive,
            GameStateType::DeployingUnits,
            GameStateType::SiteSelect,
        ] {
            state.game_state = phase;
            assert!(
                !state.finish_turn(),
                "phase {phase:?} must not close the turn"
            );
            assert_eq!(state.turn_counter, 0, "phase {phase:?}");
            assert_eq!(state.game_state, phase, "phase {phase:?} is left as is");
        }

        state.game_state = GameStateType::EndTurn;
        assert!(!state.finish_turn(), "no victory condition is met");
        assert_eq!(state.turn_counter, 1);
        assert_eq!(state.game_state, GameStateType::InGame);
    }

    /// Boundary: a game with no playing teams has no barrier agreement, yet
    /// `menu_check_end_game_conditions` finds nothing to end and the turn
    /// counter still advances (0 participants is not "everyone is out").
    #[test]
    fn empty_game_has_no_barrier_and_still_advances_the_turn() {
        let mut state = GameState::new();

        assert_eq!(
            state.end_turn_barrier(),
            None,
            "no playing teams — no agreement"
        );
        assert_eq!(state.begin_turn_barrier(), None);
        assert_eq!(state.crc_barrier(), None);
        assert!(state.are_teams_finished_turn());

        state.game_state = GameStateType::EndTurn;
        assert!(!state.finish_turn(), "empty game has no end condition");
        assert_eq!(state.turn_counter, 1);
        assert_eq!(state.game_state, GameStateType::InGame);
    }

    /// `GameManager_IsActiveTurn` (`game_manager.cpp:8296`): the active team in
    /// turn-based play, everyone in simultaneous play, and always a remote team.
    #[test]
    fn is_active_turn_follows_the_play_mode() {
        let (mut state, _red, _green) = tank_state();
        state.active_turn_team = TeamId::RED;
        state.play_mode = PlayMode::TurnBased;
        assert!(state.is_active_turn(TeamId::RED));
        assert!(!state.is_active_turn(TeamId::GREEN));

        state.teams[TeamId::GREEN.0 as usize].team_type = TeamKind::Remote;
        assert!(
            state.is_active_turn(TeamId::GREEN),
            "a remote team always processes its input"
        );

        state.play_mode = PlayMode::SimultaneousMoves;
        assert!(state.is_active_turn(TeamId::BLUE));
        assert!(state.is_active_turn(TeamId::GRAY));

        state.play_mode = PlayMode::Unknown;
        assert!(state.is_active_turn(TeamId::GRAY));
    }

    /// The barrier needs the agreement of all four playing teams; `ALIEN` and
    /// empty/eliminated slots never take part (`Remote_ServerWaitBarrier`).
    #[test]
    fn barrier_requires_all_four_playing_teams_to_agree() {
        let mut state = GameState::new();
        for team in 0..PLAYER_TEAM_COUNT {
            state.teams[team].team_type = TeamKind::Player;
            state.turn_indices[team] = 9;
        }
        assert_eq!(state.end_turn_barrier(), Some(9));

        state.turn_indices[PLAYER_TEAM_COUNT - 1] = 8;
        assert_eq!(state.end_turn_barrier(), None, "one team diverged");

        state.turn_indices[PLAYER_TEAM_COUNT] = 1;
        assert_eq!(
            state.end_turn_barrier(),
            None,
            "ALIEN does not repair a disagreement"
        );

        state.turn_indices[PLAYER_TEAM_COUNT - 1] = 9;
        assert_eq!(state.end_turn_barrier(), Some(9));

        // An eliminated team drops out of the agreement (as in C++).
        state.teams[PLAYER_TEAM_COUNT - 1].team_type = TeamKind::Eliminated;
        state.turn_indices[PLAYER_TEAM_COUNT - 1] = 0;
        assert_eq!(state.end_turn_barrier(), Some(9));
    }

    /// Commands are applied strictly in receive order: conflicting orders produce
    /// the last delivered one, and the reverse delivery produces the other state.
    /// Both runs are bit-identical to their own repetition (D).
    #[test]
    fn command_delivery_order_decides_the_final_order() {
        fn order_command(id: UnitId, order: UnitOrder) -> Command {
            Command::Order(crate::input::Packet08Order {
                unit: id,
                payload: crate::input::OrderPayload::P1(crate::events::payload_p1_for(
                    order,
                    UnitOrderState::ExecutingOrder,
                    UnitOrder::Await,
                    UnitOrderState::ExecutingOrder,
                )),
            })
        }

        fn run(base: &GameState, id: UnitId, first: UnitOrder, second: UnitOrder) -> GameState {
            let mut state = base.clone();
            state.enqueue(order_command(id, first));
            state.enqueue(order_command(id, second));
            assert_eq!(state.apply_pending_commands(), 2);
            state
        }

        let (state, id, _second) = tank_state();
        let sentry = UnitOrder::Sentry;
        let await_order = UnitOrder::Await;

        let forward = run(&state, id, sentry, await_order);
        assert_eq!(
            forward.unit(id).expect("unit").orders,
            await_order,
            "the last delivered order wins"
        );

        let backward = run(&state, id, await_order, sentry);
        assert_eq!(backward.unit(id).expect("unit").orders, sentry);
        assert_ne!(
            forward.snapshot_bytes(),
            backward.snapshot_bytes(),
            "delivery order is part of the state"
        );

        // Repetition of the same delivery order stays bit-identical.
        let repeated = run(&state, id, sentry, await_order);
        assert_eq!(repeated.snapshot_bytes(), forward.snapshot_bytes());
        assert_eq!(
            crate::crc::state_digest(&repeated),
            crate::crc::state_digest(&forward)
        );
    }

    /// Phase, active team and turn counter are simulated fields: they enter the
    /// canonical digest but not the packet-45 unit-list CRC (D).
    #[test]
    fn phase_team_and_turn_counter_enter_the_digest() {
        let (mut state, _red, _green) = tank_state();
        state.network_game = false;

        let baseline = state.snapshot_bytes();
        let baseline_crc = crate::crc::state_digest(&state);
        let unit_crc = crate::crc::state_checksum(&state);

        state.active_turn_team = TeamId::GREEN;
        let after_team = state.snapshot_bytes();
        assert_ne!(baseline, after_team, "active team is digested");
        assert_ne!(baseline_crc, crate::crc::state_digest(&state));

        state.game_state = GameStateType::EndTurn;
        assert_ne!(after_team, state.snapshot_bytes(), "phase is digested");

        state.game_state = GameStateType::InGame;
        state.turn_counter += 1;
        assert_ne!(baseline, state.snapshot_bytes(), "turn is digested");

        // Packet 45 covers only the four unit lists (no phase/turn fields).
        assert_eq!(
            crate::crc::state_checksum(&state),
            unit_crc,
            "unit-list CRC must not depend on the phase"
        );
    }

    /// `UnitsManager_UnitIdCounter` ids carry a 13-bit object index
    /// (`counter & 0x1FFF`); a long run of spawns never leaves that space and
    /// keeps the counter monotonic (boundary/MAX).
    #[test]
    fn unit_ids_stay_inside_the_thirteen_bit_index_space() {
        let (mut state, _red, _green) = tank_state();
        let before = state.unit_id_counter;
        let flags = unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE;
        let mut seen = std::collections::BTreeSet::new();

        for index in 0..64i16 {
            let grid = Point::new(2 + index % 60, 2 + index / 60);
            let id = state
                .spawn_unit(UnitType::Tank, TeamId::RED, grid, flags)
                .expect("spawn");
            assert!(
                id.object_index() < 0x2000,
                "object index {} escapes the 13-bit space",
                id.object_index()
            );
            assert!(seen.insert(id), "duplicate unit id {id:?}");
        }

        assert_eq!(state.unit_id_counter, before + 64);
        assert_eq!(state.unit_count(), 2 + 64);
    }
}
