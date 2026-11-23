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
//!   turn machine (`next_team`) is **not** moved here yet, the barrier steppers are
//!   **not** called from [`GameUniverse::tick`], `StateDelta` is coarse, and
//!   [`GameUniverse::state`]/[`GameUniverse::state_mut`] are temporary accesses.
//! * **Stage B:** `GameState` becomes private, the turn machine and the barrier FSM
//!   move in, the temporary accesses disappear.
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
use crate::enums::PlayMode;
use crate::events::{self, ApplyOutcome, StateSyncPacket};
use crate::game::{GameState, TurnEvent, UnitSound, TEAM_SLOTS};
use crate::geo::Point;
use crate::input::Command;
use crate::production::ProductionEvent;
use crate::research::ResearchEvent;
use crate::team::{TeamId, TeamInfo};
use crate::unit::{UnitId, UnitListKind, UnitPresentation};

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
}

/// Routing decision for a locally submitted command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutedCommand {
    /// Apply locally only (`00`/`52`/`45`, single-player).
    Local(Command),
    /// Apply locally and broadcast to peers (`08`/`38`/`41`/`50`/`06`/`11`/`53`/`18`/`43`…).
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
}

// ---------------------------------------------------------------------------
// Transport DTOs
// ---------------------------------------------------------------------------

/// A DTO the facade hands to the transport (before encoding).
// The size difference is dictated by the approved ADR signatures (packet 54 is a
// full DTO); boxing would change the shape `max_net` will encode in stage B.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkCommand {
    /// A command packet.
    Command(Command),
    /// Packet 54 — the authoritative team state.
    StateSync(StateSyncPacket),
}

/// A DTO the transport hands to the facade (after decoding).
// The size difference is dictated by the approved ADR signatures (see above).
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCommand {
    /// A command packet.
    Command(Command),
    /// Packet 54 — the authoritative team state.
    StateSync(StateSyncPacket),
}

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

/// One facade event — a UI/QA report, never simulation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    /// The team-turn machine produced a step (`Human`/`Computer`/`RoundClosed`/`GameOver`).
    Turn(TurnEvent),
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
}

impl Default for GameUniverse<LocalDispatcher> {
    fn default() -> Self {
        Self::new(GameState::new(), LocalDispatcher)
    }
}
