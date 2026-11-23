//! `GameUniverse` — the facade over [`GameState`] (stage A).
//!
//! Architecture: `max-brh-docs/architecture/client/game-universe-facade.md`
//! (approved 2026-11-23); step-by-step tasks:
//! `max-brh-docs/development/game-universe-stage-a.md`.
//!
//! Stage A is a **thin owning wrapper**: [`GameState`] stays `pub` with all its
//! `pub` fields and methods (guardrail 1), the facade only adds the five pillars:
//!
//! 1. [`GameUniverse::submit`] — the single point of change (routing + queue);
//! 2. [`GameUniverse::tick`] — the single point of time advance (one step, not a
//!    loop);
//! 3. [`GameUniverse::snapshot`] / [`GameUniverse::delta_since`] — pull state out;
//! 4. [`GameUniverse::subscribe`] — pull events out (push channel, drained by
//!    request);
//! 5. [`GameUniverse::drain_outgoing`] / [`GameUniverse::submit_remote`] — the only
//!    contact with the transport (DTOs, not protocol).
//!
//! # Stages
//!
//! * **Stage A (this module):** the facade lives **on top of** `GameState`; the
//!   turn machine (`GameState::next_team`) is called **only from this module**
//!   ([`GameUniverse::end_local_turn`] / [`GameUniverse::begin_local_round`] and
//!   [`GameUniverse::resume_local_round`], gated by [`Dispatcher::advances_turns`]),
//!   the barrier steppers are **not** called
//!   from [`GameUniverse::tick`], `StateDelta` is coarse, and
//!   [`GameUniverse::state`]/[`GameUniverse::state_mut`] are temporary accesses.
//! * **Stage B:** `GameState` becomes private, the barrier FSM (`NetworkDispatcher`)
//!   moves in, the temporary accesses disappear.
//!
//! # Determinism
//!
//! `seq`, [`EventBus`] and `outgoing` are **facade** values: they never enter
//! [`GameState::snapshot_bytes`] or any CRC (ADR §3, guardrails 6). The facade
//! calls exactly the same `GameState` methods as the old code path, so a
//! `submit`→`tick` run and an `enqueue`→`tick` run produce byte-identical
//! snapshots (see `tests/universe.rs`).
//!
//! The module is I/O-, thread- and float-free (`max_core` rules).

use crate::crc;
use crate::enums::{GameStateType, PlayMode, UnitOrder, UnitOrderState, UnitType, VictoryType};
use crate::events::{self, ApplyOutcome, StateSyncPacket};
use crate::game::{GameState, OrderData, TurnEvent, UnitSound, PLAYER_TEAM_COUNT, TEAM_SLOTS};
use crate::geo::Point;
use crate::input::Command;
use crate::production::ProductionEvent;
use crate::research::ResearchEvent;
use crate::team::{TeamId, TeamInfo, TeamKind, TeamUnits};
use crate::unit::{UnitId, UnitInfo, UnitListKind, UnitPresentation};
use crate::victory::{self, MissionCategory, WinLossStatus};

use max_types::map::Map;

/// How many events the push buffer keeps before the oldest ones are dropped.
///
/// Events are a UI/QA report, not game state: trimming them cannot affect
/// determinism (ADR §3). The value is a stage-A compromise between memory and
/// how far behind a consumer (render, QA log) may lag.
const MAX_BUFFERED_EVENTS: usize = 8192;

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// Transport policy: what happens around a command.
///
/// The dispatcher is **pure policy and never touches [`GameState`]** — the facade
/// performs every mutation. That keeps `max_core` transport-agnostic (ADR §2).
pub trait Dispatcher {
    /// Route of a locally submitted command.
    fn route(&mut self, cmd: Command) -> RoutedCommand;

    /// What to do with a command that arrived from a peer.
    fn receive(&mut self, cmd: RemoteCommand) -> RemoteHandling;

    /// Barrier plan after a tick: which DTOs to send.
    ///
    /// `facts` is a read-only slice of the barrier counters (the facade builds it
    /// from `GameState` right after the rules phase and before the checksum).
    fn plan(&mut self, facts: &BarrierFacts) -> Vec<NetworkCommand>;

    /// Whether the facade owns the sequential team-turn machine (`next_team`).
    ///
    /// [`LocalDispatcher`] returns `true`: the facade advances the turn itself,
    /// triggered by [`GameUniverse::end_local_turn`]. A network dispatcher
    /// (stage B) returns `false`, because a networked match advances the turn
    /// through the barrier FSM (`max_app::net`) and the simultaneous mode needs a
    /// synchronous `finished_turn` for `are_teams_finished_turn` (ADR §1 item 5,
    /// §2). The default is `false` — fail-closed: a dispatcher that has not
    /// declared the machine does not start it.
    fn advances_turns(&mut self) -> bool {
        false
    }
}

/// Routing decision for a locally submitted command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutedCommand {
    /// Apply locally only (`00`/`52`/`45`, single-player).
    Local(Command),
    /// Apply locally and broadcast to peers
    /// (`06`/`08`/`10`/`11`/`12`/`14`/`18`/`20`/`38`/`41`/`43`/`50`; the route
    /// table is ADR §2, `game-universe-facade.md`). Packet `53` is **not** here:
    /// it is a `Local` barrier/projection packet like `00`/`01`/`45`/`52`.
    Broadcast(Command),
    /// Drop (a forbidden order of an inactive command and the like).
    Drop,
}

/// What to do with a command received from a peer.
// The variant sizes follow the approved ADR signatures
// (`game-universe-facade.md` §2): `StateSyncPacket` is a full packet-54 DTO, so
// boxing it would change the public shape the transport layer expects.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteHandling {
    /// Put into the queue in receive order.
    Replay(Command),
    /// Apply the authoritative state immediately (packet 54).
    ApplyStateSync(StateSyncPacket),
    /// Ignore.
    Ignore,
}

/// Read-only slice of the barrier counters (the values are `GameState` fields,
/// CRC-relevant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarrierFacts {
    /// `GameState::begin_turn_barrier` — the common packet 00 counter.
    pub begin_turn: Option<u8>,
    /// `GameState::end_turn_barrier` — the common packet 52 counter.
    pub end_turn: Option<u8>,
    /// `GameState::crc_barrier` — the common packet 45 index.
    pub crc: Option<u8>,
    /// `GameState::state_sync_indices` — the last applied 54 index per team.
    pub state_sync: [u8; TEAM_SLOTS],
    /// The local team (`GameState::player_team`).
    pub local_team: TeamId,
    /// The play mode (`GameState::play_mode`).
    pub play_mode: PlayMode,
}

/// The single-player dispatcher: no network, deterministic, zero-sized.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalDispatcher;

impl Dispatcher for LocalDispatcher {
    /// There is no network: every command is local.
    fn route(&mut self, cmd: Command) -> RoutedCommand {
        RoutedCommand::Local(cmd)
    }

    /// Single-player never receives remote commands.
    fn receive(&mut self, _cmd: RemoteCommand) -> RemoteHandling {
        RemoteHandling::Ignore
    }

    /// No barrier plan in a local match — and **no barrier steppers** are stepped.
    ///
    /// The steppers (`step_begin_turn_barrier`/`step_end_turn_barrier`/
    /// `step_crc_barrier`) belong to the network path only: in the original
    /// `Remote_IsNetworkGame = false` skips the barrier path, and the single-player
    /// snapshot must stay identical to the current one (ADR §2, owner item 4).
    fn plan(&mut self, _facts: &BarrierFacts) -> Vec<NetworkCommand> {
        Vec::new()
    }

    /// The facade owns the sequential team-turn machine in a local match
    /// (ADR §2: `LocalDispatcher` → `true`).
    fn advances_turns(&mut self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Transport DTOs
// ---------------------------------------------------------------------------

// The DTOs are `max_types` data (stage B refinement): they are pure wrappers over
// `Command`/`StateSyncPacket`, so the transport layer encodes them without
// importing this module. The re-export keeps the facade API and every existing
// `max_core::universe::{NetworkCommand, RemoteCommand}` path unchanged.
pub use max_types::universe::{NetworkCommand, RemoteCommand};

/// Result of a local submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitOutcome {
    /// The command was accepted into the queue (or applied).
    Accepted,
    /// The command was dropped by the dispatcher route.
    Dropped,
}

/// Where a rejected command was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectSource {
    /// The dispatcher route returned [`RoutedCommand::Drop`].
    DroppedByRoute,
    /// The simulation rejected the command while applying it.
    ///
    /// Reserved for stage B: `GameState::tick` applies the queue through
    /// [`crate::events::apply_pending`], which reports only a count, so the facade
    /// cannot observe a per-command outcome yet. The variant exists so that the
    /// event shape does not change later.
    RejectedBySimulation,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// One step of the team-turn machine **with the values the application peeks
/// before the transition** (ADR §3).
///
/// The step itself is `GameState::next_team`; the two extra fields are exactly
/// what `local.rs` read before calling it (C++ `GameLoop` reads
/// `WinLoss_Status` before `GameManager_TeamTurnFinish` and
/// `GameManager_PlayerTeam` before `GameManager_UpdateGui` switches it), now
/// computed inside the facade because the machine lives here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnNotice {
    /// The core transition (`Human`/`Computer`/`RoundClosed`/`GameOver`).
    pub event: TurnEvent,
    /// `GameManager_PlayerTeam` **before** the switch — the hot-seat «Begin turn»
    /// window is shown when `previous_player_team != team` (`local.rs:790-804`).
    pub previous_player_team: TeamId,
    /// `WinLoss_EvaluateStatus` **before** `finish_turn` — only on the closing step
    /// (`upcoming_team().is_none()`); `None` for `Human`/`Computer`/`RoundClosed`.
    pub status: Option<WinLossStatus>,
}

/// One facade event — a UI/QA report, never simulation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    /// The team-turn machine produced a step, with the pre-transition peek data.
    Turn(TurnNotice),
    /// A production report of the last turn (`spawn`/`interrupt`).
    Production(ProductionEvent),
    /// A research report of the last turn (completed topic levels).
    Research(ResearchEvent),
    /// A combat sound.
    Sound(UnitSound),
    /// A complex power message.
    Industry(String),
    /// In-game chat (packet 18).
    Chat(TeamId, String),
    /// A command was rejected — full context is kept (owner decision 7).
    CommandRejected {
        /// The command.
        cmd: Command,
        /// The full application outcome, not only its kind.
        outcome: ApplyOutcome,
        /// The state version (`seq`) at which the command was rejected.
        tick: u64,
        /// Who rejected it.
        source: RejectSource,
    },
}

/// The append-only event buffer behind [`EventStream`] cursors.
///
/// One buffer, N independent cursors (owner decision 6); trimming is by size and
/// never affects determinism.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EventBus {
    events: Vec<GameEvent>,
    /// Absolute number of the first buffered event (grows when the buffer is trimmed).
    base: u64,
}

impl EventBus {
    fn push(&mut self, event: GameEvent) {
        self.events.push(event);

        if self.events.len() > MAX_BUFFERED_EVENTS {
            let excess = self.events.len() - MAX_BUFFERED_EVENTS;
            self.events.drain(..excess);
            self.base += excess as u64;
        }
    }

    /// Absolute number of the next event to be pushed.
    fn position(&self) -> u64 {
        self.base + self.events.len() as u64
    }
}

/// An independent event cursor (owner decision 6: one buffer, N cursors).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EventStream {
    cursor: u64,
}

impl EventStream {
    /// A cursor that starts at the current end of the buffer.
    pub fn new() -> Self {
        Self { cursor: 0 }
    }

    /// Takes every event after the cursor and advances it.
    ///
    /// Events dropped by trimming are reported as absent — they are a report, not
    /// state (ADR §3). A second stream is not affected: the buffer is shared, the
    /// cursors are not.
    pub fn drain<D: Dispatcher>(&mut self, universe: &mut GameUniverse<D>) -> Vec<GameEvent> {
        let bus = &universe.bus;
        let start = self.cursor.saturating_sub(bus.base) as usize;
        let events = if start < bus.events.len() {
            bus.events[start..].to_vec()
        } else {
            Vec::new()
        };
        self.cursor = bus.position();
        events
    }

    /// The events after the cursor without consuming them.
    pub fn try_drain<'a, D: Dispatcher>(
        &mut self,
        universe: &'a GameUniverse<D>,
    ) -> &'a [GameEvent] {
        let bus = &universe.bus;
        let start = self.cursor.saturating_sub(bus.base) as usize;
        if start < bus.events.len() {
            &bus.events[start..]
        } else {
            &[]
        }
    }

    /// The cursor position (absolute event number).
    pub const fn cursor(&self) -> u64 {
        self.cursor
    }
}

// ---------------------------------------------------------------------------
// StateRef / StateDelta
// ---------------------------------------------------------------------------

/// A borrowed snapshot — no clone, no `Arc`, no buffer (Q20).
///
/// The borrow ends before the next `submit`/`tick` (the borrow checker enforces
/// it). `UnitPresentation` is part of the snapshot (Q22) even though it is not
/// part of `snapshot_bytes`/CRC.
#[derive(Debug, Clone, Copy)]
pub struct StateRef<'a> {
    state: &'a GameState,
}

impl<'a> StateRef<'a> {
    /// `UnitInfo` by id.
    pub fn unit(&self, id: UnitId) -> Option<&'a crate::unit::UnitInfo> {
        self.state.unit(id)
    }

    /// Whether the unit exists.
    pub fn has_unit(&self, id: UnitId) -> bool {
        self.state.has_unit(id)
    }

    /// Number of live units.
    pub fn unit_count(&self) -> usize {
        self.state.unit_count()
    }

    /// All unit ids in canonical order (five lists + units outside them).
    pub fn unit_ids_in_order(&self) -> Vec<UnitId> {
        self.state.unit_ids_in_order()
    }

    /// One of the five unit lists in insertion order.
    pub fn unit_list(&self, kind: UnitListKind) -> &'a [UnitId] {
        self.state.unit_list(kind)
    }

    /// Units on a cell (`Hash_MapHash`).
    pub fn units_at(&self, grid: Point) -> &'a [UnitId] {
        self.state.units_at(grid)
    }

    /// The unit presentation fields (Q22) — the renderer's input.
    pub fn presentation(&self, id: UnitId) -> Option<&'a UnitPresentation> {
        self.state.presentation(id)
    }

    /// The unit's order data (`game.rs:861`): `move_to`/`fire_on`.
    pub fn order_data(&self, id: UnitId) -> OrderData {
        self.state.order_data(id)
    }

    /// `GameManager_TurnCounter`.
    pub fn turn_counter(&self) -> i32 {
        self.state.turn_counter
    }

    /// `GameManager_ActiveTurnTeam`.
    pub fn active_turn_team(&self) -> TeamId {
        self.state.active_turn_team
    }

    /// `GameManager_PlayerTeam` (the local player).
    pub fn player_team(&self) -> TeamId {
        self.state.player_team
    }

    /// `GameManager_GameState`.
    pub fn game_state(&self) -> crate::enums::GameStateType {
        self.state.game_state
    }

    /// `GameManager_PlayMode`.
    pub fn play_mode(&self) -> PlayMode {
        self.state.play_mode
    }

    /// `GameManager_IsActiveTurn` (`game.rs:2634`).
    pub fn is_active_turn(&self, team: TeamId) -> bool {
        self.state.is_active_turn(team)
    }

    /// The team the next turn-machine step would start; `None` — the round would
    /// close (`game.rs:2319`).
    pub fn upcoming_team(&self) -> Option<TeamId> {
        self.state.upcoming_team()
    }

    /// `GameManager_AreTeamsFinishedTurn` — the concurrent round barrier
    /// (`game.rs:2293`).
    pub fn are_teams_finished_turn(&self) -> bool {
        self.state.are_teams_finished_turn()
    }

    /// All team slots.
    pub fn teams(&self) -> &'a [TeamInfo; TEAM_SLOTS] {
        &self.state.teams
    }

    /// One team slot.
    pub fn team(&self, team: TeamId) -> Option<&'a TeamInfo> {
        self.state.teams.get(team.0 as usize)
    }

    /// The game map.
    pub fn map(&self) -> &'a Map {
        &self.state.map
    }

    /// Packet 45 CRC of the units (`crc.rs:179`).
    pub fn state_checksum(&self) -> u16 {
        crc::state_checksum(self.state)
    }

    /// CRC16 of [`GameState::snapshot_bytes`] (`crc.rs:205`).
    pub fn state_digest(&self) -> u16 {
        crc::state_digest(self.state)
    }

    /// The common packet 00 counter (`GameState::begin_turn_barrier`).
    pub fn begin_turn_barrier(&self) -> Option<u8> {
        self.state.begin_turn_barrier()
    }

    /// The common packet 52 counter (`GameState::end_turn_barrier`).
    pub fn end_turn_barrier(&self) -> Option<u8> {
        self.state.end_turn_barrier()
    }

    /// The common packet 45 index (`GameState::crc_barrier`).
    pub fn crc_barrier(&self) -> Option<u8> {
        self.state.crc_barrier()
    }

    /// Packet 00 consent predicate (`GameState::begin_turn_barrier_reached`).
    pub fn begin_turn_barrier_reached(&self, counter: u8) -> bool {
        self.state.begin_turn_barrier_reached(counter)
    }

    /// Packet 52 consent predicate (`GameState::end_turn_barrier_reached`).
    pub fn end_turn_barrier_reached(&self, counter: u8) -> bool {
        self.state.end_turn_barrier_reached(counter)
    }

    /// Packet 45 consent predicate (`GameState::crc_barrier_reached`).
    pub fn crc_barrier_reached(&self, index: u8) -> bool {
        self.state.crc_barrier_reached(index)
    }

    /// `GameState::state_sync_indices` — the last applied 54 index per team.
    pub fn state_sync_indices(&self) -> [u8; TEAM_SLOTS] {
        self.state.state_sync_indices
    }

    /// `Remote_TurnIndices` (packet 52 counters, `game.rs:319`).
    pub fn turn_indices(&self) -> [u8; TEAM_SLOTS] {
        self.state.turn_indices
    }

    /// `Remote_FrameSyncCounter` (packet 00 counters, `game.rs:325`).
    pub fn frame_sync_counter(&self) -> [u8; TEAM_SLOTS] {
        self.state.frame_sync_counter
    }

    /// `Remote_NextTurnIndices` (packet 45 indices, `game.rs:321`).
    pub fn next_turn_indices(&self) -> [u8; TEAM_SLOTS] {
        self.state.next_turn_indices
    }

    /// `GameState::team_data_crc16` — the peer CRCs of the current 45 round.
    pub fn team_data_crc16(&self) -> [u16; TEAM_SLOTS] {
        self.state.team_data_crc16
    }

    /// In-game chat (packet 18) — `game.rs:338`; a UI/log report.
    pub fn chat_log(&self) -> &'a [(TeamId, String)] {
        &self.state.chat_log
    }

    /// Mission supplies per team (packet 12) — `game.rs:340`; setup input data.
    pub fn mission_supplies(
        &self,
    ) -> &'a [Option<crate::input::Packet12MissionSupply>; TEAM_SLOTS] {
        &self.state.mission_supplies
    }

    /// The canonical replay digest (`crc.rs:242`) — end-to-end parity bytes.
    pub fn replay_digest_bytes(&self) -> Vec<u8> {
        crc::replay_digest_bytes(self.state)
    }

    /// The canonical state bytes.
    pub fn snapshot_bytes(&self) -> Vec<u8> {
        self.state.snapshot_bytes()
    }
}

/// A coarse change envelope: from `from_seq` to `seq` with the current snapshot.
///
/// `from_seq`/`seq`/`checksum` are **facade** values and never enter
/// `snapshot_bytes`/CRC (ADR §3). A field-level diff (`DirtySet`) is stage B.
#[derive(Debug, Clone, Copy)]
pub struct StateDelta<'a> {
    /// The excluding base version.
    pub from_seq: u64,
    /// The version the snapshot belongs to.
    pub seq: u64,
    /// `crc::state_digest` of the state at `seq`.
    pub checksum: u16,
    /// The full current snapshot (borrowed, not cloned).
    pub state: StateRef<'a>,
}

// ---------------------------------------------------------------------------
// GameUniverse
// ---------------------------------------------------------------------------

/// The owning facade over [`GameState`] (stage A).
#[derive(Debug, Clone)]
pub struct GameUniverse<D: Dispatcher = LocalDispatcher> {
    state: GameState,
    dispatcher: D,
    seq: u64,
    outgoing: Vec<NetworkCommand>,
    bus: EventBus,
}

impl<D: Dispatcher> GameUniverse<D> {
    /// Wraps an existing state with a dispatcher.
    pub fn new(state: GameState, dispatcher: D) -> Self {
        Self {
            state,
            dispatcher,
            seq: 0,
            outgoing: Vec::new(),
            bus: EventBus::default(),
        }
    }

    // — the single point of change —

    /// Accepts a command: routes it and puts it into the simulation queue.
    ///
    /// `submit` is **not** `apply_command`: the mutation happens in [`Self::tick`]
    /// through `events::apply_pending` → `apply_command`, strictly in receive order
    /// (ADR §1 item 1). The one-tick shift of `06`/lobby commands is the accepted
    /// price of the single path (owner decision 2).
    pub fn submit(&mut self, cmd: Command) -> SubmitOutcome {
        // The clone keeps the full command for `CommandRejected` diagnostics
        // (owner decision 7); submissions are user input, not a per-tick cost.
        match self.dispatcher.route(cmd.clone()) {
            RoutedCommand::Local(command) => {
                self.notify_chat(&command);
                self.state.enqueue(command);
                SubmitOutcome::Accepted
            }
            RoutedCommand::Broadcast(command) => {
                self.notify_chat(&command);
                self.state.enqueue(command.clone());
                self.outgoing.push(NetworkCommand::Command(command));
                SubmitOutcome::Accepted
            }
            RoutedCommand::Drop => {
                self.bus.push(GameEvent::CommandRejected {
                    cmd,
                    outcome: ApplyOutcome::Ignored,
                    tick: self.seq,
                    source: RejectSource::DroppedByRoute,
                });
                SubmitOutcome::Dropped
            }
        }
    }

    /// One simulation step (a step, not a loop; Q23).
    ///
    /// Phases (ADR §5), all after the simulation itself:
    ///
    /// 1. **rules/counters** — [`GameState::tick`] wraps the whole existing
    ///    sequence (path results, `apply_pending_commands`, `process_orders`, AI).
    ///    The facade must not decompose it: `poll_path_results`/`dispatch_path_jobs`
    ///    are private, and stage A forbids changes in `game.rs`;
    /// 2. **barriers** — [`BarrierFacts`] is built right after the rules phase and
    ///    before the checksum (owner item 3);
    /// 3. **version** — `seq += 1`;
    /// 4. **events** — the UI/QA reports of the step are taken from the state and
    ///    pushed into the buffer;
    /// 5. **outgoing** — `dispatcher.plan(&facts)`.
    ///
    /// The barrier steppers (`step_begin_turn_barrier`/`step_end_turn_barrier`/
    /// `step_crc_barrier`) are **not** called here: their only production callers
    /// are in `max_app::net` (the barrier FSM). Stepping them in a local match would
    /// change `snapshot_bytes` (ADR §2, guardrail 7).
    pub fn tick(&mut self) {
        // Phases 1-3 of the owner table: the unchanged simulation tick.
        self.state.tick();

        // Barrier facts: after the rules phase, before the checksum (owner item 3).
        let facts = self.barrier_facts();

        // Phase 4: the facade version (never part of the snapshot/CRC).
        self.seq += 1;

        // Phase 5: the UI/QA reports of this step.
        for event in self.state.take_production_events() {
            self.bus.push(GameEvent::Production(event));
        }
        for event in self.state.take_research_events() {
            self.bus.push(GameEvent::Research(event));
        }
        for sound in self.state.take_unit_sounds() {
            self.bus.push(GameEvent::Sound(sound));
        }
        if let Some(message) = self.state.take_industry_message() {
            self.bus.push(GameEvent::Industry(message));
        }

        // Phase 6: the transport plan.
        self.outgoing = self.dispatcher.plan(&facts);
    }

    // — pull state out —

    /// The current borrowed snapshot.
    pub fn snapshot(&self) -> StateRef<'_> {
        StateRef { state: &self.state }
    }

    /// The changes since `seq`; `None` when the caller is already up to date.
    pub fn delta_since(&self, seq: u64) -> Option<StateDelta<'_>> {
        if seq >= self.seq {
            return None;
        }

        Some(StateDelta {
            from_seq: seq,
            seq: self.seq,
            checksum: crc::state_digest(&self.state),
            state: self.snapshot(),
        })
    }

    /// The current state version.
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    // — pull events out —

    /// A fresh cursor at the current end of the event buffer.
    pub fn subscribe(&self) -> EventStream {
        EventStream {
            cursor: self.bus.position(),
        }
    }

    // — the only contact with the transport —

    /// Takes the outgoing DTOs planned by the last [`Self::tick`].
    pub fn drain_outgoing(&mut self) -> Vec<NetworkCommand> {
        std::mem::take(&mut self.outgoing)
    }

    /// Applies a DTO that arrived from a peer.
    ///
    /// A replayed command is queued (applied at the next tick); an authoritative
    /// state sync (packet 54) is applied **immediately** and counts as a state
    /// version (owner decision 2).
    pub fn submit_remote(&mut self, cmd: RemoteCommand) {
        match self.dispatcher.receive(cmd) {
            RemoteHandling::Replay(command) => {
                self.notify_chat(&command);
                self.state.enqueue(command);
            }
            RemoteHandling::ApplyStateSync(packet) => {
                events::apply_state_sync(&mut self.state, &packet);
                self.seq += 1;
            }
            RemoteHandling::Ignore => {}
        }
    }

    // — stage B: configuration/test layer (stage-b §3, ADR §6 risk 2) —
    //
    // Setup mutators have no `Command` equivalent: they configure a match (or a
    // test fixture) instead of moving the lockstep simulation forward. They stay
    // `pub` on the facade as a documented config/test surface and are 1:1 calls,
    // so the behavior of `GameState` and every golden digest is unchanged. None of
    // them advances `seq`: configuration is not an observable simulation step.

    /// Spawns a unit — `UnitsManager_DeployUnit` (`game.rs:1311`).
    pub fn spawn_unit(
        &mut self,
        unit_type: UnitType,
        team: TeamId,
        grid: Point,
        flags: u32,
    ) -> Option<UnitId> {
        self.state.spawn_unit(unit_type, team, grid, flags)
    }

    /// Destroys a unit — `UnitsManager_DestroyUnit` (`game.rs:1562`).
    pub fn destroy_unit(&mut self, id: UnitId) {
        self.state.destroy_unit(id);
    }

    /// Writes a modified unit copy back (`game.rs:880`).
    pub fn write_unit(&mut self, id: UnitId, unit: UnitInfo) {
        self.state.write_unit(id, unit);
    }

    /// Mutable unit by id — the setup counterpart of [`StateRef::unit`]
    /// (`game.rs:818`).
    pub fn unit_mut(&mut self, id: UnitId) -> Option<&mut UnitInfo> {
        self.state.unit_mut(id)
    }

    /// Writes the unit presentation (`game.rs:841`).
    pub fn write_presentation(&mut self, id: UnitId, presentation: UnitPresentation) {
        self.state.write_presentation(id, presentation);
    }

    /// Mutable unit presentation — the setup counterpart of
    /// [`StateRef::presentation`] (`game.rs:835`).
    pub fn presentation_mut(&mut self, id: UnitId) -> Option<&mut UnitPresentation> {
        self.state.presentation_mut(id)
    }

    /// `UnitsManager_SetNewOrderInt` (`game.rs:1626`); returns `true` if a path
    /// request was removed.
    pub fn set_order(&mut self, id: UnitId, order: UnitOrder, state: UnitOrderState) -> bool {
        self.state.set_order(id, order, state)
    }

    /// Order-data write (`game.rs:868`).
    pub fn set_order_data(&mut self, id: UnitId, data: OrderData) {
        self.state.set_order_data(id, data);
    }

    /// Applies a parsed V70 save to the wrapped state (`game.rs:547`) — the
    /// setup entry that is not a facade version bump (see [`Self::from_save`]).
    pub fn load_state(&mut self, save: &crate::save::SaveFile) {
        self.state.load_state(save);
    }

    /// Replaces the V70 save/service fields (`save_write.rs:851`).
    pub fn set_save_meta(&mut self, meta: crate::save_write::SaveMeta) {
        self.state.set_save_meta(meta);
    }

    /// `Remote_IsNetworkGame` — network mode flag (`game.rs:350`).
    pub fn set_network_game(&mut self, value: bool) {
        self.state.network_game = value;
    }

    /// The `enemy_halt` setting (`game.rs:373`).
    pub fn set_enemy_halt(&mut self, value: bool) {
        self.state.enemy_halt = value;
    }

    /// `GameManager_PlayMode` (`game.rs:258`).
    pub fn set_play_mode(&mut self, value: PlayMode) {
        self.state.play_mode = value;
    }

    /// `GameManager_GameState` (`game.rs:256`).
    pub fn set_game_state(&mut self, value: GameStateType) {
        self.state.game_state = value;
    }

    /// `ini_setting_victory_type` (`game.rs:392`).
    pub fn set_victory_type(&mut self, value: VictoryType) {
        self.state.victory_type = value;
    }

    /// `ini_setting_victory_limit` (`game.rs:393`).
    pub fn set_victory_limit(&mut self, value: i32) {
        self.state.victory_limit = value;
    }

    /// `SaveFormatHeader::mission_index` (`game.rs:387`).
    pub fn set_mission_index(&mut self, value: u16) {
        self.state.mission_index = value;
    }

    /// `MissionManager::GetMission()->GetCategory()` (`game.rs:389`).
    pub fn set_mission_category(&mut self, value: MissionCategory) {
        self.state.mission_category = value;
    }

    /// The `effects` setting (`game.rs:355`).
    pub fn set_settings_effects(&mut self, value: bool) {
        self.state.settings_effects = value;
    }

    /// The `disable_fire` setting (`game.rs:357`).
    pub fn set_disable_fire(&mut self, value: bool) {
        self.state.disable_fire = value;
    }

    /// The `GameManager_RealTime` setting (`game.rs:360`).
    pub fn set_real_time(&mut self, value: bool) {
        self.state.real_time = value;
    }

    /// Re-seeds the simulation RNG — `Randomizer_SetSeed` (`game.rs:226`), the
    /// same call packet 13 makes (`events.rs:216`).
    pub fn set_rng_seed(&mut self, seed: u32) {
        self.state.rng.set_seed(seed);
    }

    /// `UnitsManager_TeamInfo[PLAYER_TEAM_MAX]` — replaces the team table
    /// (`game.rs:222`).
    pub fn set_teams(&mut self, teams: [TeamInfo; TEAM_SLOTS]) {
        self.state.teams = teams;
    }

    /// `CTInfo::team_units[PLAYER_TEAM_MAX]` — replaces the team-economy table
    /// (`game.rs:224`).
    pub fn set_team_units(&mut self, team_units: [TeamUnits; TEAM_SLOTS]) {
        self.state.team_units = team_units;
    }

    /// Per-slot team configuration (`game.rs:222`): the setup counterpart of
    /// [`StateRef::team`], so a single slot can be edited without rebuilding the
    /// whole table (the reason [`Self::set_teams`] alone is not enough for §6).
    pub fn team_mut(&mut self, team: TeamId) -> Option<&mut TeamInfo> {
        self.state.teams.get_mut(team.0 as usize)
    }

    /// Per-slot team-economy configuration (`game.rs:224`): the setup counterpart
    /// of a single `team_units` slot.
    pub fn team_units_mut(&mut self, team: TeamId) -> Option<&mut TeamUnits> {
        self.state.team_units.get_mut(team.0 as usize)
    }

    // — stage B: barrier steppers, reachable only through the facade (A1) —

    /// `Remote_WaitBeginTurnAcknowledge`: `++Remote_FrameSyncCounter[team]` → 00.
    ///
    /// Stage B decision A1 (`game-universe-stage-b.md` §0.2): `GameState`'s
    /// steppers become `pub(crate)` in the final visibility cut, so the barrier FSM
    /// (`max_app::NetworkDispatcher`) steps them **only through this facade
    /// passthrough**. The body is a 1:1 call — the counter stays simulation state
    /// (`snapshot_bytes`). Returns the new counter.
    pub fn step_begin_turn_barrier(&mut self, team: TeamId) -> u8 {
        self.state.step_begin_turn_barrier(team)
    }

    /// `Remote_WaitEndTurnAcknowledge`: `++Remote_TurnIndices[team]` → 52.
    ///
    /// See [`Self::step_begin_turn_barrier`] (A1). Returns the new counter.
    pub fn step_end_turn_barrier(&mut self, team: TeamId) -> u8 {
        self.state.step_end_turn_barrier(team)
    }

    /// `Remote_CheckDesync`: `++Remote_NextTurnIndices[team]` → 45.
    ///
    /// See [`Self::step_begin_turn_barrier`] (A1). Returns the new index.
    pub fn step_crc_barrier(&mut self, team: TeamId) -> u8 {
        self.state.step_crc_barrier(team)
    }

    // — stage B: simulation/turn passthroughs for the scenario harness (§4.1) —
    //
    // The phase 1-5 integration scenarios drive the simulation in steps that are
    // not a `Command` and not a plain `tick()`: the start-of-turn economy pass,
    // the team-turn machine, the end-turn phase and the visibility scan. Stage B
    // §4.1 moves these test harness calls onto the facade, so each one is a 1:1
    // passthrough with the same behavior as the `GameState` call it wraps. They
    // are **not** lockstep mutations of the `submit` path (no `seq` bump, no
    // outgoing plan): the caller observes the returned value, exactly as the
    // old harness did.

    /// `GameManager_ManageEconomy` (`game.rs:1886`) — the start-of-turn economy
    /// pass (`start_turn` in `tests/support/mod.rs`).
    pub fn manage_economy(&mut self, team: TeamId) {
        self.state.manage_economy(team);
    }

    /// `GameManager_TeamTurnTurnBased` step (`game.rs:2400`); returns the
    /// transition so the harness can observe `Human`/`Computer`/`RoundClosed`/
    /// `GameOver` without touching the state.
    pub fn next_team(&mut self) -> TurnEvent {
        self.state.next_team()
    }

    /// Round entry `GameManager_TeamTurnTurnBased` (`game.rs:2336`).
    pub fn begin_round(&mut self) {
        self.state.begin_round();
    }

    /// `GameManager_TeamTurnFinish` (`game.rs:2486`); `true` when the match is
    /// over.
    pub fn end_round(&mut self) -> bool {
        self.state.end_round()
    }

    /// Load entry (`game.rs:2361`): starts the saved active team without a second
    /// economy pass; `None` when no playing slot is left.
    pub fn resume_round(&mut self) -> Option<TurnEvent> {
        self.state.resume_round()
    }

    /// Enters `GAME_STATE_9_END_TURN` (`game.rs:2607`) — the phase
    /// [`Self::finish_turn`] requires.
    pub fn enter_end_turn(&mut self) {
        self.state.enter_end_turn();
    }

    /// `GameManager_TeamTurnFinish` (`game.rs:2649`); `true` when the match is
    /// over.
    pub fn finish_turn(&mut self) -> bool {
        self.state.finish_turn()
    }

    /// `Access_UpdateVisibilityStatus` (`ai/threat.rs:859`).
    pub fn update_visibility_status(&mut self, reset_visibility: bool) {
        self.state.update_visibility_status(reset_visibility);
    }

    /// `UnitInfo::SpotByTeam` + `Ai_UnitSpotted` (`ai/mod.rs:296`).
    pub fn spot_by_team(&mut self, id: UnitId, team: TeamId) {
        self.state.spot_by_team(id, team);
    }

    // — stage A only: temporary accesses, removed in stage B —

    /// The wrapped state (stage A only, guardrail 2).
    ///
    /// It exists so that the existing `max_app` code keeps compiling while the
    /// renderer migrates to [`Self::snapshot`]. Stage B removes it.
    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// The wrapped state, mutable (stage A only, guardrail 2).
    pub fn state_mut(&mut self) -> &mut GameState {
        &mut self.state
    }

    // — internals —

    /// The barrier counters right after the rules phase.
    fn barrier_facts(&self) -> BarrierFacts {
        BarrierFacts {
            begin_turn: self.state.begin_turn_barrier(),
            end_turn: self.state.end_turn_barrier(),
            crc: self.state.crc_barrier(),
            state_sync: self.state.state_sync_indices,
            local_team: self.state.player_team,
            play_mode: self.state.play_mode,
        }
    }

    /// Reports an incoming chat line as an event.
    ///
    /// Chat is a UI report, so it is announced at submission time (the state also
    /// keeps `chat_log` for the HUD); the simulation application itself stays in
    /// the queue like every other command.
    fn notify_chat(&mut self, command: &Command) {
        if let Command::Chat(packet) = command {
            self.bus
                .push(GameEvent::Chat(packet.sender, packet.message.clone()));
        }
    }
}

impl GameUniverse<LocalDispatcher> {
    /// A local universe with a map and a set of teams — `GameState::with_setup`
    /// (`game.rs:486`), the usual test/scenario constructor.
    pub fn with_setup(map: Map, team_kinds: [TeamKind; PLAYER_TEAM_COUNT]) -> Self {
        Self::new(GameState::with_setup(map, team_kinds), LocalDispatcher)
    }

    /// A local universe from a V70 save (`from_save` counts as a state version).
    pub fn from_save(save: &crate::save::SaveFile) -> Self {
        Self {
            state: GameState::from_save(save),
            dispatcher: LocalDispatcher,
            seq: 1,
            outgoing: Vec::new(),
            bus: EventBus::default(),
        }
    }

    /// The wrapped state, consuming the facade (stage A migration helper).
    pub fn into_state(self) -> GameState {
        self.state
    }

    // — the local team-turn machine (stage A, ADR §1/§4) —

    /// Local End Turn: packet `06` **immediately**, then `enter_end_turn` and one
    /// run of the team-turn machine to the next acting team (ADR §1, §4).
    ///
    /// Together with [`Self::begin_local_round`] (the round entry) this is the only
    /// way the sequential machine advances in a single-player match, and the only
    /// trigger of a turn close. `06` is applied right away (not queued): in the
    /// simultaneous mode the round is closed by `are_teams_finished_turn`, which
    /// needs a synchronous `finished_turn` (stage-A task §5 item 5). Every machine
    /// step is emitted as [`GameEvent::Turn`] with the pre-transition peek data
    /// ([`TurnNotice`]).
    ///
    /// The dispatcher policy gates the machine: it runs only when
    /// [`Dispatcher::advances_turns`] is `true` (statically true for
    /// [`LocalDispatcher`]; a network dispatcher returns `false`).
    pub fn end_local_turn(&mut self, team: TeamId) {
        if !self.dispatcher.advances_turns() {
            return;
        }

        // `GameManager_RefreshOrders` → `Remote_SendNetPacket_06`: the turn of the
        // team is over, applied synchronously (as `local.rs` did before, ADR §4).
        let _ = events::apply_command(&mut self.state, &events::command_finished_turn(team));
        // The sequential loop closes every team turn through `GAME_STATE_9_END_TURN`
        // (`finish_turn` requires the phase; `06` itself sets it only in `TurnBased`).
        self.state.enter_end_turn();
        self.run_turn_machine();
        // The turn transition is an observable state change (ADR §1.3): bump the
        // facade version once per atomic facade operation.
        self.seq += 1;
    }

    /// Enters a round of the local team-turn machine and advances it to the first
    /// acting team (match start entry, stage-A task §6 item 4).
    ///
    /// `GameState::begin_round` stays `pub` for configuration scenarios, but
    /// `next_team` is facade-only (stage-A task §6 item 4): the round cursor reset
    /// and the first step therefore belong together here. The state before the call
    /// is already configured by the application (`team_types`, `play_mode`, …),
    /// exactly as `start_local_game` does.
    ///
    /// A match **loaded from V70** enters through [`Self::resume_local_round`]
    /// instead: its first slot is the saved `active_turn_team`, not
    /// `PLAYER_TEAM_RED`, and its economy must not run for a second time.
    pub fn begin_local_round(&mut self) {
        if !self.dispatcher.advances_turns() {
            return;
        }

        self.state.begin_round();
        self.run_turn_machine();
        // Round entry is an observable state change (ADR §1.3): bump the facade
        // version once per atomic facade operation.
        self.seq += 1;
    }

    /// Resumes the local team-turn machine after a V70 load —
    /// `GAME_STATE_10_LOAD_GAME` (`game_manager.cpp:458-480`).
    ///
    /// C++ starts the team-turn loop at `GameManager_ActiveTurnTeam` (the active
    /// team restored by `SaveLoad_Load`, `saveload.cpp:905-910`) and, because the
    /// phase is the load phase, skips `GameManager_ManageEconomy`
    /// (`game_manager.cpp:462-464`) and `GameManager_ProgressBuildState`
    /// (`:2450/2468/2484`) for that team: its economy was applied **before** the
    /// save. The task's ADR §1 item 6 — the same one-step entry without a second
    /// economy pass — is implemented by `GameState::resume_round`; the facade owns
    /// the single `TurnNotice` (the application's hot-seat/UI start, ADR §4).
    ///
    /// Unlike [`Self::begin_local_round`] exactly **one** team is started (C++ breaks
    /// out of the loop after the first `GameManager_UpdateGui`, `:476-480`), and
    /// `finished_turn` of that team is reset as in `:472`; no round is closed.
    ///
    /// Returns `true` when a team was started. `false` means no playing slot is left
    /// at or after the saved active team (C++ then never enters the loop body); the
    /// caller has to decide how to continue, the facade leaves the state untouched.
    pub fn resume_local_round(&mut self) -> bool {
        if !self.dispatcher.advances_turns() {
            return false;
        }

        // `GameManager_UpdateGui` compares `GameManager_PlayerTeam` with the started
        // team; that value is peeked before the step, as in `run_turn_machine`.
        let previous_player_team = self.state.player_team;
        let Some(event) = self.state.resume_round() else {
            return false;
        };

        self.bus.push(GameEvent::Turn(TurnNotice {
            event,
            previous_player_team,
            // The load entry never closes a round, so there is no `finish_turn`
            // status to peek (`GAME_STATE_10_LOAD_GAME` breaks out before it).
            status: None,
        }));
        // The resumed turn start is an observable state change (ADR §1.3): bump the
        // facade version once per atomic facade operation.
        self.seq += 1;
        true
    }

    /// Runs `GameState::next_team` until a team has to act (`Human`/`Computer`) or
    /// the match is over; `RoundClosed` continues into the next round (ADR §4).
    ///
    /// The peek values are taken **before** each step, as the application did in
    /// `local.rs:790-804`:
    ///
    /// * `previous_player_team` — `GameManager_PlayerTeam` before `next_team`
    ///   switches it for a human team;
    /// * `status` — `WinLoss_EvaluateStatus` before `finish_turn`, only when the
    ///   step is the closing one (`upcoming_team().is_none()`), and only reported
    ///   on [`TurnEvent::GameOver`] (ADR §3).
    fn run_turn_machine(&mut self) {
        loop {
            let previous_player_team = self.state.player_team;
            let closing = self.state.upcoming_team().is_none();
            let status =
                closing.then(|| victory::evaluate_status(&self.state, self.state.turn_counter));

            let event = self.state.next_team();

            self.bus.push(GameEvent::Turn(TurnNotice {
                event,
                previous_player_team,
                status: if event == TurnEvent::GameOver {
                    status
                } else {
                    None
                },
            }));

            match event {
                TurnEvent::Human(_) | TurnEvent::Computer(_) | TurnEvent::GameOver => return,
                TurnEvent::RoundClosed => {}
            }
        }
    }
}

impl Default for GameUniverse<LocalDispatcher> {
    fn default() -> Self {
        Self::new(GameState::new(), LocalDispatcher)
    }
}
