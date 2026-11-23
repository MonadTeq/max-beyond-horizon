//! Shared harness for the phase 5 AI integration scenarios (`phase5_*.rs`).
//!
//! Every scenario has the same shape (engine-coverage-plan.md §3, phase 5):
//!
//! > an AI-controlled base (RED, `TeamKind::Computer`) + an enemy base
//! > (GREEN, `TeamKind::Player`, passive) → N rounds through the core turn
//! > machine → expected tasks / units / CRC.
//!
//! # Turn model
//!
//! The rounds run through the real team-turn machine
//! ([`GameState::begin_round`] → [`GameState::next_team`] →
//! [`GameState::end_round`], `GameManager_TeamTurnTurnBased`/`TeamTurnFinish`).
//! [`GameState::next_team`] itself performs `GameManager_ManageEconomy` and
//! `Ai_BeginTurn` for a computer slot (`game.rs:2373`, `:2392`), so the harness
//! never calls `ai_begin_turn` a second time — that would be a different
//! (double-planned) turn than the engine's.
//!
//! A team turn is closed the way the engine closes it: packet 06
//! ([`events::command_finished_turn`]) when the AI has not closed its own turn
//! yet (`AiPlayer::CheckEndTurn` inside the tick loop), then the end-turn phase
//! ([`GameState::enter_end_turn`]) that [`GameState::finish_turn`] requires.
//!
//! # Transient tasks
//!
//! `TaskAttack`, `TaskKillUnit`, `TaskMove` and the reserve pool live only a
//! few ticks of the AI turn, so observations of them are sampled **inside** the
//! tick loop ([`AiObservation`]); assertions on persistent state (units, team
//! gold, research topics, digests) read the state at a round boundary.
//!
//! # Visibility
//!
//! A unit whose order is `ORDER_IDLE` does not scan
//! (`access.cpp:762`: `Access_UpdateMapStatus` returns early for `ORDER_IDLE`),
//! so a scenario that needs spotting puts its observer on `ORDER_SENTRY` and
//! recomputes the coverage exactly as a landing does
//! (`Access_UpdateVisibilityStatus` → [`GameState::update_visibility_status`]).

#![allow(dead_code)]

use max_core::ai::TaskKind;
use max_core::crc;
use max_core::enums::{Opponent, PlayMode, Surface, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::events;
use max_core::game::{GameState, TurnEvent};
use max_core::geo::Point;
use max_core::map::{Map, ResourceKind, ResourceMapEntry};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::UnitId;

/// Simulation ticks given to every team turn. The AI closes its own turn when
/// its task manager is idle (`CheckEndTurn` retry counter 10, ~10 ticks), so
/// 16 ticks per team turn is a full turn with movement.
pub const ROUND_TICKS: usize = 16;

// ---------------------------------------------------------------------------
// Maps and states
// ---------------------------------------------------------------------------

/// Land in the top-left `size × size` square, water elsewhere.
///
/// `SelectStrategy` needs at least one viable continent with a safe landing
/// site; the enclosing water keeps the map a single island, which is the
/// "TankHorde-friendly" shape of `aiplayer.cpp:3765-3805`.
pub fn land_map(size: i16) -> Map {
    let mut map = Map::new();

    for y in 0..max_core::geo::MAP_HEIGHT {
        for x in 0..max_core::geo::MAP_WIDTH {
            let surface = if x < size && y < size {
                Surface::LAND
            } else {
                Surface::WATER
            };
            map.surface.set(x, y, surface);
        }
    }

    map
}

/// A red AI base against a passive green base.
///
/// RED is the computer team under test; GREEN is a human slot so the enemy base
/// takes no AI decisions and the scenario stays about one AI player.
pub fn ai_state(map: Map, clan: TeamClan, opponent: Opponent) -> GameState {
    let mut state = GameState::with_setup(
        map,
        [
            TeamKind::Computer,
            TeamKind::Player,
            TeamKind::None,
            TeamKind::None,
        ],
    );

    state.network_game = false;
    state.play_mode = PlayMode::TurnBased;
    state.player_team = TeamId::GREEN;

    let red = TeamId::RED.0 as usize;
    state.teams[red].team_type = TeamKind::Computer;
    state.teams[red].team_clan = clan;
    state.team_units[red] = TeamUnits::with_clan(clan);

    let green = TeamId::GREEN.0 as usize;
    state.teams[green].team_type = TeamKind::Player;
    state.team_units[green] = TeamUnits::with_clan(TeamClan::Random);

    state.ai.settings.opponent = opponent as u8;
    max_core::ai_init(&mut state);

    state
}

/// A state with the strategy fixed directly on the player.
///
/// The strategy is a public `AiPlayer` field, so the scenario can name the
/// strategy under test without depending on the score roll of
/// `SelectStrategy` (which the strategy scenarios cover separately). A
/// non-`Random` strategy survives the clan mapping of `AiPlayer::init_tasks`.
pub fn strategy_state(strategy: max_core::enums::AiStrategy, opponent: Opponent) -> GameState {
    let mut state = ai_state(land_map(40), TeamClan::Musashi, opponent);
    state.ai.settings.team_strategy[0] = Some(strategy);
    state.ai.players[TeamId::RED.0 as usize].strategy = strategy;
    state
}

/// Places a raw-materials deposit on a cell (`Survey`-visible ore).
pub fn put_ore(state: &mut GameState, site: Point, amount: u8) {
    put_resource(state, site, amount, ResourceKind::Raw);
}

/// Places a fuel deposit on a cell.
pub fn put_fuel(state: &mut GameState, site: Point, amount: u8) {
    put_resource(state, site, amount, ResourceKind::Fuel);
}

/// Places a deposit of one resource kind on a cell.
pub fn put_resource(state: &mut GameState, site: Point, amount: u8, kind: ResourceKind) {
    state.map.resources.set(
        site.x,
        site.y,
        ResourceMapEntry::new(amount, Some(kind), 0x0F),
    );
}

/// Spawns a unit with the type flags from `SC_UNITS`.
pub fn spawn(state: &mut GameState, unit_type: UnitType, team: TeamId, site: Point) -> UnitId {
    state
        .spawn_unit(unit_type, team, site, 0)
        .unwrap_or_else(|| panic!("spawn {unit_type:?} for {team:?} at {site:?}"))
}

/// The red starting base: a constructor with raw material, two ore cells, a
/// tank (a team without a combat unit is "lost" for the victory check, which
/// would close the match after the first round) and a team treasury.
pub struct RedBase {
    /// The builder.
    pub constructor: UnitId,
    /// The base defender.
    pub tank: UnitId,
}

/// See [`RedBase`].
pub fn red_base(state: &mut GameState, site: Point, gold: u32) -> RedBase {
    put_ore(state, Point::new(site.x + 2, site.y + 1), 20);
    put_ore(state, Point::new(site.x + 3, site.y + 1), 20);

    let constructor = spawn(state, UnitType::Constructor, TeamId::RED, site);
    state.unit_mut(constructor).expect("constructor").storage = 60;

    let tank = spawn(
        state,
        UnitType::Tank,
        TeamId::RED,
        Point::new(site.x, site.y + 5),
    );

    state.team_units[TeamId::RED.0 as usize].set_gold(gold);

    RedBase { constructor, tank }
}

/// The enemy base: a key-facility building plus one defender four cells east.
///
/// Returns `(building, defender)`.
pub fn green_base(state: &mut GameState, site: Point) -> (UnitId, UnitId) {
    let building = spawn(state, UnitType::TrainingHall, TeamId::GREEN, site);
    let defender = spawn(
        state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(site.x + 4, site.y),
    );

    (building, defender)
}

/// Puts a unit on sentry so that it scans (`Access_UpdateMapStatus` skips
/// `ORDER_IDLE`).
pub fn sentry(state: &mut GameState, unit: UnitId) {
    state.set_order(unit, UnitOrder::Sentry, UnitOrderState::ExecutingOrder);
}

/// Recomputes the map coverage of every unit — `Access_UpdateVisibilityStatus`
/// after a landing (`access.cpp:880`).
pub fn refresh_visibility(state: &mut GameState) {
    state.update_visibility_status(false);
}

/// Marks a world unit as visible to a team and notifies the AI
/// (`UnitInfo::SpotByTeam` + `Ai_UnitSpotted`).
pub fn spot(state: &mut GameState, unit: UnitId, team: TeamId) {
    state.spot_by_team(unit, team);
}

/// Number of cells the team has explored (`AiPlayer::info_map`).
pub fn explored_count(state: &GameState, team: TeamId) -> usize {
    state.ai.players[team.0 as usize]
        .info_map
        .iter()
        .filter(|value| **value & max_core::enums::INFO_MAP_EXPLORED != 0)
        .count()
}

// ---------------------------------------------------------------------------
// Scenario building blocks
// ---------------------------------------------------------------------------

/// A heavy vehicle plant with a full raw/fuel stockpile (a material dump and a
/// fuel dump in the plant's complex).
///
/// The stock lives in the complex cache, which `ManageEconomy` recomputes from
/// the complex members, so the dumps are real units — setting
/// `Complex::raw` directly is undone by the next economy pass.
pub fn add_vehicle_plant(state: &mut GameState, site: Point) -> UnitId {
    let plant = spawn(state, UnitType::HeavyVehiclePlant, TeamId::RED, site);
    state.unit_mut(plant).expect("plant").orders = UnitOrder::PowerOn;

    let material = spawn(
        state,
        UnitType::MaterialStorage,
        TeamId::RED,
        Point::new(site.x + 2, site.y),
    );
    state.unit_mut(material).expect("material dump").storage = 100;

    let fuel = spawn(
        state,
        UnitType::FuelStorage,
        TeamId::RED,
        Point::new(site.x, site.y + 2),
    );
    state.unit_mut(fuel).expect("fuel dump").storage = 100;

    plant
}

/// A powered research centre: a mine on a fuel deposit, the centre and three
/// power generators in one complex.
///
/// One research centre plus the mine consume three power; the station-selection
/// formula of `ProductionManager_OptimizeProduction` only turns a station on
/// for a demand of six or more (three generators are the cheapest exact fit),
/// and without a positive fuel balance the generators are not started at all
/// (`power::settle_producers`). The centre is `ORDER_POWER_ON` when
/// `DetermineResearchProjects` runs, which is what the topic assignment needs.
pub fn add_research_lab(state: &mut GameState, site: Point) -> UnitId {
    put_fuel(state, site, 50);

    let mine = spawn(state, UnitType::MiningStation, TeamId::RED, site);
    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.fuel_mining = 10;
    }

    let centre = spawn(
        state,
        UnitType::ResearchCentre,
        TeamId::RED,
        Point::new(site.x + 2, site.y),
    );
    {
        let unit = state.unit_mut(centre).expect("centre");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::UnitReady;
    }

    for generator_site in [
        Point::new(site.x + 4, site.y),
        Point::new(site.x + 2, site.y + 2),
        Point::new(site.x, site.y + 2),
    ] {
        let generator = spawn(state, UnitType::PowerGenerator, TeamId::RED, generator_site);
        let unit = state.unit_mut(generator).expect("generator");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    centre
}

// ---------------------------------------------------------------------------
// Turn machine
// ---------------------------------------------------------------------------

/// A team turn selected by the machine: the economy and `Ai_BeginTurn` of a
/// computer slot have already run inside [`GameState::next_team`].
pub fn start_team_turn(state: &mut GameState) -> TeamId {
    match state.next_team() {
        TurnEvent::Human(team) | TurnEvent::Computer(team) => {
            assert_eq!(state.active_turn_team, team, "active team of {team:?}");
            team
        }
        other => panic!("a team turn was expected, got {other:?}"),
    }
}

/// Closes a team turn: packet 06 when the AI left the turn open, then the
/// end-turn phase `finish_turn` requires.
pub fn close_team_turn(state: &mut GameState, team: TeamId) {
    if !state.teams[team.0 as usize].finished_turn {
        state.enqueue(events::command_finished_turn(team));
        state.tick();
    }

    state.enter_end_turn();
}

/// One full round: every playing team gets `ticks` ticks, then
/// `GameManager_TeamTurnFinish`. Returns `true` when the match is over.
pub fn play_round(state: &mut GameState, ticks: usize) -> bool {
    let mut observation = AiObservation::default();
    play_round_observed(state, ticks, TeamId::RED, &mut observation)
}

/// One full round while sampling the red AI task list on every tick.
///
/// Returns `true` when the match is over.
pub fn play_round_observed(
    state: &mut GameState,
    ticks: usize,
    observed_team: TeamId,
    observation: &mut AiObservation,
) -> bool {
    state.begin_round();

    // RED, GREEN, … — `PLAYER_TEAM_COUNT` slots; the guard catches a machine
    // that never closes the round.
    for _ in 0..8 {
        match state.next_team() {
            TurnEvent::Human(team) | TurnEvent::Computer(team) => {
                for _ in 0..ticks {
                    state.tick();
                    observation.sample(state, observed_team);
                }
                observation.sample(state, observed_team);
                close_team_turn(state, team);
            }
            TurnEvent::RoundClosed => return false,
            TurnEvent::GameOver => return true,
        }
    }

    panic!("the team-turn machine did not close the round");
}

/// Plays up to `rounds` rounds; returns the number of closed rounds and whether
/// the match ended. Stops early on game over.
pub fn play_rounds(state: &mut GameState, rounds: usize, ticks: usize) -> (usize, bool) {
    for played in 0..rounds {
        if play_round(state, ticks) {
            return (played + 1, true);
        }
    }

    (rounds, false)
}

/// Plays up to `rounds` rounds while sampling the observed team (see
/// [`play_round_observed`]).
pub fn play_rounds_observed(
    state: &mut GameState,
    rounds: usize,
    ticks: usize,
    observed_team: TeamId,
    observation: &mut AiObservation,
) -> (usize, bool) {
    for played in 0..rounds {
        if play_round_observed(state, ticks, observed_team, observation) {
            return (played + 1, true);
        }
    }

    (rounds, false)
}

// ---------------------------------------------------------------------------
// Observations
// ---------------------------------------------------------------------------

/// Mid-round AI facts. Tasks such as `TaskAttack`, `TaskKillUnit`, `TaskMove`
/// and the reserve pool exist only for a few ticks of the AI turn, so they are
/// collected while the turn runs, not at the round boundary.
#[derive(Debug, Default, Clone)]
pub struct AiObservation {
    /// `TaskAttack` was present at least once.
    pub saw_attack_task: bool,
    /// `TaskKillUnit` was present at least once.
    pub saw_kill_unit_task: bool,
    /// `TaskExplore` was present at least once.
    pub saw_explore_task: bool,
    /// `TaskMove` was present at least once.
    pub saw_move_task: bool,
    /// `TaskAttackReserve` (the reserve pool) was present at least once.
    pub saw_attack_reserve_task: bool,
    /// Largest `TaskAttackReserve::units` count seen.
    pub max_attack_reserve: usize,
    /// Largest `TaskDefenseReserve::units` count seen.
    pub max_defense_reserve: usize,
    /// Largest `TaskObtainUnits` type count seen.
    pub max_obtain_types: usize,
    /// Largest `TaskCreateBuilding` task count seen.
    pub max_create_building_tasks: usize,
    /// Largest `AiPlayer::spotted_units` count seen.
    pub max_spotted: usize,
    /// Largest explored-cell count seen.
    pub max_explored: usize,
    /// Largest `stats_gold_spent_on_upgrades` value seen.
    pub max_gold_spent: u32,
    /// `ORDER_BUILD` was seen on a red constructor.
    pub saw_constructor_build_order: bool,
}

impl AiObservation {
    /// Samples the AI tasks of `team` and the persistent counters.
    pub fn sample(&mut self, state: &GameState, team: TeamId) {
        for task in state
            .ai
            .task_manager
            .tasks()
            .iter()
            .filter(|task| task.team == team)
        {
            match &task.kind {
                TaskKind::Attack(_) => self.saw_attack_task = true,
                TaskKind::KillUnit(_) => self.saw_kill_unit_task = true,
                TaskKind::Explore { .. } => self.saw_explore_task = true,
                TaskKind::Move(_) => self.saw_move_task = true,
                TaskKind::AttackReserve { units, .. } => {
                    self.saw_attack_reserve_task = true;
                    self.max_attack_reserve = self.max_attack_reserve.max(units.len());
                }
                TaskKind::DefenseReserve { units, .. } => {
                    self.max_defense_reserve = self.max_defense_reserve.max(units.len());
                }
                TaskKind::ObtainUnits { units, .. } => {
                    self.max_obtain_types = self.max_obtain_types.max(units.len());
                }
                TaskKind::CreateBuilding { .. } => {
                    self.max_create_building_tasks += 1;
                }
                _ => {}
            }
        }

        self.max_spotted = self
            .max_spotted
            .max(state.ai.players[team.0 as usize].spotted_units.len());
        self.max_explored = self.max_explored.max(explored_count(state, team));
        self.max_gold_spent = self
            .max_gold_spent
            .max(state.teams[team.0 as usize].stats_gold_spent_on_upgrades);

        if state.unit_ids_in_order().into_iter().any(|id| {
            state.unit(id).is_some_and(|unit| {
                unit.team == team
                    && unit.unit_type == UnitType::Constructor
                    && unit.orders == UnitOrder::Build
            })
        }) {
            self.saw_constructor_build_order = true;
        }
    }
}

// ---------------------------------------------------------------------------
// State queries
// ---------------------------------------------------------------------------

/// Team task ids of one task type (`TASK_TYPE_*`).
pub fn tasks_of_type(state: &GameState, team: TeamId, task_type: u8) -> Vec<u32> {
    state
        .ai
        .task_manager
        .tasks()
        .iter()
        .filter(|task| task.team == team && task.get_type() == task_type)
        .map(|task| task.id)
        .collect()
}

/// Unit types ordered by `TaskCreateBuilding` tasks of a team (in task order).
pub fn create_building_types(state: &GameState, team: TeamId) -> Vec<UnitType> {
    state
        .ai
        .task_manager
        .tasks()
        .iter()
        .filter(|task| task.team == team)
        .filter_map(|task| match &task.kind {
            TaskKind::CreateBuilding { unit_type, .. } => Some(*unit_type),
            _ => None,
        })
        .collect()
}

/// `TaskObtainUnits` requests grouped per task (insertion order). One group is
/// one order: `TaskExplore` requests a list, the factory production roll
/// requests exactly one type.
pub fn obtain_unit_requests(state: &GameState, team: TeamId) -> Vec<Vec<UnitType>> {
    state
        .ai
        .task_manager
        .tasks()
        .iter()
        .filter(|task| task.team == team)
        .filter_map(|task| match &task.kind {
            TaskKind::ObtainUnits { units, .. } => Some(units.clone()),
            _ => None,
        })
        .collect()
}

/// Unit types requested by `TaskObtainUnits` tasks of a team (flattened).
pub fn obtain_units_types(state: &GameState, team: TeamId) -> Vec<UnitType> {
    obtain_unit_requests(state, team)
        .into_iter()
        .flatten()
        .collect()
}

/// Single-type production requests whose type the given factory builds.
///
/// This is the `plan_unit_production` order: `GetFilteredWeightTable(INVALID_ID,
/// 3)` → `RollUnitType` → `TaskObtainUnits(type)`.
pub fn factory_unit_requests(state: &GameState, team: TeamId, factory: UnitType) -> Vec<UnitType> {
    obtain_unit_requests(state, team)
        .into_iter()
        .filter(|group| {
            group.len() == 1 && max_core::production::builder_type(group[0]) == Some(factory)
        })
        .flatten()
        .collect()
}

/// Units accumulated by the team's `TaskAttackReserve`.
pub fn attack_reserve_units(state: &GameState, team: TeamId) -> Vec<UnitId> {
    state
        .ai
        .task_manager
        .tasks()
        .iter()
        .filter(|task| task.team == team)
        .find_map(|task| match &task.kind {
            TaskKind::AttackReserve { units, .. } => Some(units.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Units accumulated by the team's `TaskDefenseReserve`.
pub fn defense_reserve_units(state: &GameState, team: TeamId) -> Vec<UnitId> {
    state
        .ai
        .task_manager
        .tasks()
        .iter()
        .filter(|task| task.team == team)
        .find_map(|task| match &task.kind {
            TaskKind::DefenseReserve { units, .. } => Some(units.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Whether the team holds an active `TaskAttack`.
pub fn has_attack_task(state: &GameState, team: TeamId) -> bool {
    state
        .ai
        .task_manager
        .tasks()
        .iter()
        .any(|task| task.team == team && matches!(task.kind, TaskKind::Attack(_)))
}

/// Units of a team with a given type currently alive.
pub fn count_units(state: &GameState, team: TeamId, unit_type: UnitType) -> usize {
    state
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.unit_type == unit_type)
        })
        .count()
}

/// `(state_checksum, replay_digest_crc, state_digest)` — packet 45, the
/// canonical replay digest CRC and the full-snapshot CRC (the three D-layer
/// values pinned for every phase 5 scenario).
pub fn crcs(state: &GameState) -> (u16, u16, u16) {
    (
        crc::state_checksum(state),
        crc::replay_digest_crc(state),
        crc::state_digest(state),
    )
}
