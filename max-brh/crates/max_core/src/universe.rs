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
//! # Channels and consumers (SDL/UI Stage A, A3)
//!
//! Canonical contract of the facade, mirroring §3 A3 of the plan
//! (`max-brh-docs/development/sdl-ui-integration.md`).
//!
//! `GameState` is **not** re-exported: it stays `pub(crate)` in [`crate::game`],
//! and the only public entry into the simulation is this facade plus the read-only
//! [`StateRef`]. The facade exposes exactly **two** documented channels, and every
//! consumer picks one of them:
//!
//! 1. **Lockstep channel** — the command channel of a match, local or networked:
//!    * [`GameUniverse::submit`] — the single point of change: routes a command and
//!      queues it;
//!    * [`GameUniverse::tick`] — one simulation step; the queued commands are
//!      applied here, strictly in receive order;
//!    * [`GameUniverse::drain_outgoing`] — takes the transport DTOs planned by the
//!      last tick (plus the broadcast commands queued by `submit`);
//!    * [`GameUniverse::submit_remote`] — feeds a DTO that arrived from a peer (a
//!      replayed command is queued for the next tick; a barrier consent and an
//!      authoritative state sync are applied immediately).
//!
//!    Reading is separate and immutable: [`GameUniverse::snapshot`],
//!    [`GameUniverse::delta_since`], [`GameUniverse::subscribe`].
//!
//! 2. **Local machine** — the sequential team-turn machine of a single-player
//!    match (round entry, load entry, turn close) plus the immediate packet path a
//!    caller needs to observe before the next tick:
//!    * [`GameUniverse::begin_local_round`] — enters a fresh round at the first
//!      playing slot;
//!    * [`GameUniverse::resume_local_round`] — enters a round loaded from V70 at the
//!      saved active team, without a second economy pass;
//!    * [`GameUniverse::end_local_turn`] — closes the current team turn (`06`) and
//!      advances the machine to the next acting team;
//!    * [`GameUniverse::tick`] — the same one-step simulation step as above; `tick`
//!      is the shared clock of both channels;
//!    * [`GameUniverse::apply_now`] — applies an immediate packet (`06`/`01`,
//!      barrier consent `00`/`52`/`45`, state sync `54`) right away, for callers
//!      that must see the effect before the next tick. It is **not** a second
//!      lockstep API: it does not bump `seq`.
//!
//!    The machine runs only when [`Dispatcher::advances_turns`] is `true`
//!    (statically true for [`LocalDispatcher`]); a network dispatcher returns
//!    `false` and the barrier FSM owns the turn.
//!
//! **Consumer rules** — the reason the channels are named at all:
//!
//! * **UI code uses the command channel only**: `submit` for intents, `tick` for
//!   the clock, `drain_outgoing`/`submit_remote` for the transport, and the
//!   immutable snapshot/event accessors for drawing. UI code never calls
//!   `apply_now`, `set_*`, `*_mut` or the local-machine entries: a direct mutation
//!   is invisible to the peers, so it breaks determinism (the gates of Stage B
//!   close the remaining call sites).
//! * **Install code uses the install API only**: the configuration surface
//!   (`with_setup`, `with_setup_and_dispatcher`, `reset_world`, `spawn_unit`,
//!   `load_state`, `set_*`, `*_mut`). It never routes commands — a command needs a
//!   tick and a transport, which a world that is still being built does not have.
//!   The narrow `install_world`/`adopt_universe` API of Stage C3 replaces this
//!   surface; the split of responsibilities stays the same.
//! * There is **no third path**: anything that changes the simulation belongs to
//!   exactly one of the two channels, and the render layer is kept read-only by
//!   `max-brh-cicd/scripts/check_render_readonly.py` (Stage A2).
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

use crate::ai::AiState;
use crate::classification::Environment;
use crate::crc;
use crate::enums::{
    GameStateType, PlayMode, ResearchTopic, UnitOrder, UnitOrderState, UnitType, VictoryType,
    RESEARCH_TOPIC_COUNT,
};
use crate::events::{self, ApplyOutcome, CommandQueue, StateSyncPacket, UnitEvent};
use crate::game::{GameState, OrderData, TurnEvent, UnitSound, PLAYER_TEAM_COUNT, TEAM_SLOTS};
use crate::geo::Point;
use crate::input::{Command, Packet12MissionSupply};
use crate::pathfinding::PathRequest;
use crate::production::ProductionEvent;
use crate::research::ResearchEvent;
use crate::save_write::SaveMeta;
use crate::team::{Cargo, Complex, TeamId, TeamInfo, TeamKind, TeamUnits};
use crate::unit::{UnitId, UnitInfo, UnitListKind, UnitPresentation};
use crate::victory::{self, CargoType, MissionCategory, NetProduction, WinLossStatus};

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
    /// Apply the barrier-consent packet `00`/`52`/`45` **immediately** (A4).
    ///
    /// A barrier packet is not a lockstep command: [`GameState::begin_turn_barrier_reached`]
    /// and the other consent predicates are read by `net::wait_barrier`, which does not
    /// advance time. A `Replay`ed barrier would deadlock the barrier wait until its
    /// 30-second timeout and shift the CRC counters by one tick (decision A4,
    /// `game-universe-stage-b.md` §0.2). The facade applies it right away through
    /// `events::apply_command` and counts it as a state version.
    ApplyBarrier(Command),
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
    /// A team was eliminated by the host session policy
    /// (`PLAN-host-mode.md` §4, H4).
    ///
    /// `sequence` is the deterministic elimination tick carried by the host
    /// heartbeat — never a wall-clock stamp, so every client records the same
    /// value.
    PlayerEliminated {
        /// Eliminated team.
        team: TeamId,
        /// Deterministic elimination tick (`ticks_without_heartbeat`).
        sequence: u32,
    },
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

    /// The replay digest CRC (`crc.rs`) — the trailer of `replay_digest_bytes`.
    pub fn replay_digest_crc(&self) -> u16 {
        crc::replay_digest_crc(self.state)
    }

    /// The canonical replay digest (`crc.rs:242`) — end-to-end parity bytes.
    pub fn replay_digest_bytes(&self) -> Vec<u8> {
        crc::replay_digest_bytes(self.state)
    }

    /// The canonical state bytes.
    pub fn snapshot_bytes(&self) -> Vec<u8> {
        self.state.snapshot_bytes()
    }

    // — stage B §6: the rest of the read surface used by `max_app` —
    //
    // Every accessor below is a 1:1 read of a `GameState` field/method or of a
    // read-only `max_core` query helper that takes `&GameState`. They exist so that
    // `max_app` never has to name `GameState` (stage-b §6 item 1); none of them
    // mutates the state or advances `seq`.

    /// `CTInfo::team_units[PLAYER_TEAM_MAX]` — the team-economy table
    /// (`game.rs:224`).
    pub fn team_units(&self) -> &'a [TeamUnits; TEAM_SLOTS] {
        &self.state.team_units
    }

    /// `ini_setting_victory_type` (`game.rs:392`).
    pub fn victory_type(&self) -> VictoryType {
        self.state.victory_type
    }

    /// `ini_setting_victory_limit` (`game.rs:393`).
    pub fn victory_limit(&self) -> i32 {
        self.state.victory_limit
    }

    /// `MissionManager::GetMission()->GetCategory()` (`game.rs:389`).
    pub fn mission_category(&self) -> MissionCategory {
        self.state.mission_category
    }

    /// `SaveFormatHeader::mission_index` (`game.rs:387`).
    pub fn mission_index(&self) -> u16 {
        self.state.mission_index
    }

    /// `Remote_IsNetworkGame` (`game.rs:350`).
    pub fn network_game(&self) -> bool {
        self.state.network_game
    }

    /// The `enemy_halt` setting (`game.rs:373`).
    pub fn enemy_halt(&self) -> bool {
        self.state.enemy_halt
    }

    /// The `effects` setting (`game.rs:355`).
    pub fn settings_effects(&self) -> bool {
        self.state.settings_effects
    }

    /// The `disable_fire` setting (`game.rs:357`).
    pub fn disable_fire(&self) -> bool {
        self.state.disable_fire
    }

    /// `GameManager_RealTime` (`game.rs:360`).
    pub fn real_time(&self) -> bool {
        self.state.real_time
    }

    /// The AI state (players, task manager, heat maps) — `game.rs:366`.
    pub fn ai(&self) -> &'a AiState {
        &self.state.ai
    }

    /// V70 save data that is not simulation state (`game.rs:383`).
    pub fn save_meta(&self) -> &'a SaveMeta {
        &self.state.save_meta
    }

    /// The simulation RNG (`game.rs:226`) — read-only for the scenario digests.
    pub fn rng(&self) -> &'a crate::rng::Randomizer {
        &self.state.rng
    }

    /// `Hash_MapHash` — the cell → units index (`game.rs:240`).
    pub fn map_hash(&self) -> &'a crate::hash::MapHash {
        &self.state.map_hash
    }

    /// `UnitsManager_UnitIdCounter` (`game.rs:277`).
    pub fn unit_id_counter(&self) -> u32 {
        self.state.unit_id_counter
    }

    /// `GameState::to_save` (`save_write.rs:835`) — the live state as a V70 tree.
    pub fn to_save(&self) -> crate::save::SaveFile {
        self.state.to_save()
    }

    /// `GameState::is_visible_on_heat_map` (`ai/threat.rs:888`).
    pub fn is_visible_on_heat_map(&self, unit: &crate::unit::UnitInfo, team: TeamId) -> bool {
        self.state.is_visible_on_heat_map(unit, team)
    }

    /// `GameState::ai_heat_map_complete` (`ai/threat.rs:895`).
    pub fn ai_heat_map_complete(&self, team: TeamId, grid_x: i32, grid_y: i32) -> u32 {
        self.state.ai_heat_map_complete(team, grid_x, grid_y)
    }

    /// `GameState::ai_heat_map_stealth_sea` (`ai/threat.rs:902`).
    pub fn ai_heat_map_stealth_sea(&self, team: TeamId, grid_x: i32, grid_y: i32) -> u32 {
        self.state.ai_heat_map_stealth_sea(team, grid_x, grid_y)
    }

    /// `GameState::ai_heat_map_stealth_land` (`ai/threat.rs:909`).
    pub fn ai_heat_map_stealth_land(&self, team: TeamId, grid_x: i32, grid_y: i32) -> u32 {
        self.state.ai_heat_map_stealth_land(team, grid_x, grid_y)
    }

    /// `TeamUnits::CreateComplex`-computed research allocation (`game.rs:1994`).
    pub fn research_allocation(&self, team: TeamId) -> [u32; RESEARCH_TOPIC_COUNT] {
        self.state.research_allocation(team)
    }

    /// `GameManager_TeamTurnTurnBased` round cursor (`game.rs:265`) — not CRC state.
    pub fn round_team(&self) -> usize {
        self.state.round_team
    }

    /// `UnitsManager_OrdersPending` (`game.rs:294`).
    pub fn orders_pending(&self) -> bool {
        self.state.orders_pending
    }

    /// The complex industry message of the last production turn (`game.rs:336`).
    pub fn industry_message(&self) -> Option<&'a str> {
        self.state.industry_message.as_deref()
    }

    /// The path request queue (`game.rs:262`).
    pub fn path_requests(&self) -> &'a std::collections::VecDeque<PathRequest> {
        &self.state.path_requests
    }

    /// The incoming command queue (`game.rs:378`).
    pub fn commands(&self) -> &'a CommandQueue {
        &self.state.commands
    }

    /// `UnitEvent_UnitEvents` (`game.rs:312`).
    pub fn unit_events(&self) -> &'a [UnitEvent] {
        &self.state.unit_events
    }

    /// A team complex by id (`game.rs:3139`).
    pub fn complex(&self, team: TeamId, complex_id: u16) -> Option<&'a Complex> {
        self.state.complex(team, complex_id)
    }

    /// V70 save bytes of the state (`save_write.rs`).
    pub fn to_save_bytes(&self) -> Result<Vec<u8>, crate::save::SaveError> {
        self.state.to_save_bytes()
    }

    // — stage B §6: read-only `max_core` query helpers the UI layer needs —
    //
    // `max_app` calls these `max_core` functions with `&GameState` today; after
    // stage B the state is not nameable there, so the read-only subset is exposed
    // on `StateRef` as a 1:1 forward (`AI-хуки`/access/production/victory/transfer
    // queries, stage-b §6 item 1).

    /// `Paths_GetGroundPath` (`pathfinding.rs`) — the HUD route preview query.
    pub fn find_ground_path(
        &self,
        unit: &UnitInfo,
        destination: Point,
        flags: u32,
        minimum_distance: i32,
    ) -> Option<crate::unit::GroundPath> {
        crate::pathfinding::find_ground_path(self.state, unit, destination, flags, minimum_distance)
    }
    /// `Access_IsEngaged` (`access.rs:1145`).
    pub fn is_engaged(&self, unit: &UnitInfo) -> bool {
        crate::access::is_engaged(self.state, unit)
    }

    /// `Access_GetGroupMembers`-style group lookup (`access.rs:1532`).
    pub fn group_members_of(&self, id: UnitId) -> Option<Vec<UnitId>> {
        crate::access::group_members_of(self.state, id)
    }

    /// `Access_IsAccessible` (`access.rs:365`).
    pub fn is_accessible(&self, unit_type: UnitType, team: TeamId, grid: Point, flags: u32) -> u32 {
        crate::access::is_accessible(self.state, unit_type, team, grid, flags)
    }

    /// `Access_GetTeamUnit` (`access.rs:875`).
    pub fn get_team_unit(&self, grid: Point, team: TeamId, flags: u32) -> Option<UnitId> {
        crate::access::get_team_unit(self.state, grid, team, flags)
    }

    /// `Access_GetTeamUnitByEnvironment` (`access.rs:896`).
    pub fn get_team_unit_by_environments(
        &self,
        grid: Point,
        team: TeamId,
        environments: &[Environment],
    ) -> Option<UnitId> {
        crate::access::get_team_unit_by_environments(self.state, grid, team, environments)
    }

    /// `Access_GetAttackTarget` (`access.rs:790`).
    pub fn get_attack_target(&self, unit: &UnitInfo, grid: Point, mode: bool) -> Option<UnitId> {
        crate::access::get_attack_target(self.state, unit, grid, mode)
    }

    /// `Access_WithinAttackRange` (`access.rs:624`).
    pub fn is_within_attack_range(&self, unit: &UnitInfo, grid: Point, range: i32) -> bool {
        crate::access::is_within_attack_range(self.state, unit, grid, range)
    }

    /// `Access_IsValidAttackTarget` (`access.rs:769`).
    pub fn is_valid_attack_target(&self, attacker: &UnitInfo, target: &UnitInfo) -> bool {
        crate::access::is_valid_attack_target(self.state, attacker, target)
    }

    /// `Access_GetReceiverUnit` (`access.rs:1034`).
    pub fn get_receiver_unit(&self, unit: &UnitInfo, grid: Point) -> Option<UnitId> {
        crate::access::get_receiver_unit(self.state, unit, grid)
    }

    /// `UnitInfo::GetTargetUnitAngle` (`unit_order.rs:584`).
    pub fn target_unit_angle(&self, unit: &UnitInfo, move_to: Point) -> i32 {
        crate::unit_order::get_target_unit_angle(self.state, unit, move_to)
    }

    /// `UnitInfo::GetTurnsToBuild` (`production.rs`) — the team's upgrade value.
    pub fn turns_to_build(&self, team: TeamId, unit_type: UnitType) -> i32 {
        crate::production::turns_to_build(self.state, team, unit_type)
    }

    /// `UnitInfo::GetTurnsAndConsumption` (`production.rs`).
    pub fn turns_and_consumption(
        &self,
        builder: &UnitInfo,
        unit_type: UnitType,
        build_speed_multiplier: i32,
    ) -> (i32, i32) {
        crate::production::turns_and_consumption(
            self.state,
            builder,
            unit_type,
            build_speed_multiplier,
        )
    }

    /// `TaskCreateBuilding::FindBuildSite` (`production.rs`).
    pub fn find_build_site(
        &self,
        team: TeamId,
        unit_type: UnitType,
        origin: Point,
    ) -> Option<Point> {
        crate::production::find_build_site(self.state, team, unit_type, origin)
    }

    /// `Builder_AwaitingExit` (`production.rs`).
    pub fn builder_awaiting_exit(&self, builder_id: UnitId) -> Option<UnitId> {
        crate::production::builder_awaiting_exit(self.state, builder_id)
    }

    /// `UnitsManager_GetBuildSite` (`production.rs`).
    pub fn initial_build_site(
        &self,
        team: TeamId,
        unit_type: UnitType,
        builder_grid: Point,
        ignored: &[UnitId],
    ) -> Option<Point> {
        crate::production::initial_build_site(self.state, team, unit_type, builder_grid, ignored)
    }

    /// `UnitsManager_IsAccessible` (`production.rs`).
    pub fn is_site_accessible_ignoring(
        &self,
        team: TeamId,
        unit_type: UnitType,
        site: Point,
        ignored: &[UnitId],
    ) -> bool {
        crate::production::is_site_accessible_ignoring(self.state, team, unit_type, site, ignored)
    }

    /// `PrepareConstructionSite` (`production.rs`) — the construction tape at `site`.
    pub fn construction_utility(&self, team: TeamId, site: Point) -> Option<UnitId> {
        crate::production::construction_utility(self.state, team, site)
    }

    /// `UnitInfo::GetActivationExitCell` (`production.rs`).
    pub fn factory_exit_cell(&self, container: &UnitInfo, client: &UnitInfo) -> Option<Point> {
        crate::production::factory_exit_cell(self.state, container, client)
    }

    /// `Cargo_GetNetProduction` (`victory.rs:667`).
    pub fn cargo_get_net_production(&self, unit: &UnitInfo, current_order: bool) -> NetProduction {
        crate::victory::cargo_get_net_production(self.state, unit, current_order)
    }

    /// `WinLoss_EvaluateStatus` at the current turn counter (`victory.rs:1411`).
    pub fn evaluate_status(&self) -> WinLossStatus {
        victory::evaluate_status(self.state, self.state.turn_counter)
    }

    /// `Transfer_FindUnitWithCargoType` (`transfer.rs:57`).
    pub fn find_unit_with_cargo_type(
        &self,
        team: TeamId,
        complex: crate::team::ComplexId,
        cargo_type: CargoType,
    ) -> Option<UnitId> {
        crate::transfer::find_unit_with_cargo_type(self.state, team, complex, cargo_type)
    }

    /// `TransferMenu_GetCargoInfo` (`transfer.rs:327`).
    pub fn transfer_menu_cargo_info(
        &self,
        source: UnitId,
        destination: UnitId,
        cargo: CargoType,
    ) -> (i32, i32) {
        crate::transfer::transfer_menu_cargo_info(self.state, source, destination, cargo)
    }

    /// `Complex::GetCargoInfo` for one cargo type (`transfer.rs:363`).
    pub fn complex_stored(
        &self,
        team: TeamId,
        complex: crate::team::ComplexId,
        cargo: CargoType,
    ) -> i32 {
        crate::transfer::complex_stored(self.state, team, complex, cargo)
    }

    /// `Complex::GetCargoMining` (`alloc.rs`) — extraction and capacity.
    pub fn cargo_mining(
        &self,
        team: TeamId,
        complex: crate::team::ComplexId,
    ) -> (Cargo, Cargo) {
        crate::alloc::get_cargo_mining(self.state, team, complex)
    }

    /// `Complex::GetCargoMinable` (`alloc.rs`) — consumption.
    pub fn cargo_minable(&self, team: TeamId, complex: crate::team::ComplexId) -> Cargo {
        crate::alloc::get_cargo_minable(self.state, team, complex)
    }

    /// `Complex::GetCargoInfo` (`alloc.rs`) — materials and capacity.
    pub fn cargo_info(&self, team: TeamId, complex: crate::team::ComplexId) -> (Cargo, Cargo) {
        crate::alloc::get_cargo_info(self.state, team, complex)
    }

    // — stage B §7: read-only helper delegates for the integration scenarios —

    /// `alloc::get_cargo_info` (`alloc.rs:`) — the C++ name.
    pub fn get_cargo_info(&self, team: TeamId, complex: crate::team::ComplexId) -> (Cargo, Cargo) {
        crate::alloc::get_cargo_info(self.state, team, complex)
    }

    /// `alloc::get_cargo_mining` — extraction and capacity.
    pub fn get_cargo_mining(
        &self,
        team: TeamId,
        complex: crate::team::ComplexId,
    ) -> (Cargo, Cargo) {
        crate::alloc::get_cargo_mining(self.state, team, complex)
    }

    /// `alloc::get_cargo_minable` — consumption.
    pub fn get_cargo_minable(&self, team: TeamId, complex: crate::team::ComplexId) -> Cargo {
        crate::alloc::get_cargo_minable(self.state, team, complex)
    }

    /// `alloc::total_mining` (`alloc.rs:675`).
    pub fn total_mining(
        &self,
        team: TeamId,
        complex: crate::team::ComplexId,
        kind: crate::alloc::MiningKind,
    ) -> i32 {
        crate::alloc::total_mining(self.state, team, complex, kind)
    }

    /// `alloc::mining_survey` (`alloc.rs:197`).
    pub fn mining_survey(&self, grid: Point) -> (i32, i32, i32) {
        crate::alloc::mining_survey(self.state, grid)
    }

    /// `power::complex_totals` (`power.rs:306`).
    pub fn complex_totals(
        &self,
        team: TeamId,
        complex: crate::team::ComplexId,
    ) -> crate::power::EnergyTotals {
        crate::power::complex_totals(self.state, team, complex)
    }

    /// `access::modified_surface_type` (`access.rs:331`).
    pub fn modified_surface_type(&self, grid: Point, mode: bool) -> u8 {
        crate::access::modified_surface_type(self.state, grid, mode)
    }

    /// `production::complex_raw_materials` (`production.rs:1083`).
    pub fn complex_raw_materials(&self, team: TeamId, complex: crate::team::ComplexId) -> i32 {
        crate::production::complex_raw_materials(self.state, team, complex)
    }

    /// `victory::default_win_loss_conditions` (`victory.rs:880`).
    pub fn default_win_loss_conditions(&self, team: TeamId) -> crate::enums::WinLossState {
        crate::victory::default_win_loss_conditions(self.state, team)
    }

    /// `victory::count_ready_units` (`victory.rs:170`).
    pub fn count_ready_units(&self, team: TeamId, unit_type: UnitType) -> i32 {
        crate::victory::count_ready_units(self.state, team, unit_type)
    }

    /// `access::is_accessible_ignoring` (`access.rs:381`).
    pub fn is_accessible_ignoring(
        &self,
        unit_type: UnitType,
        team: TeamId,
        grid: Point,
        flags: u32,
        ignored: &[UnitId],
    ) -> u32 {
        crate::access::is_accessible_ignoring(self.state, unit_type, team, grid, flags, ignored)
    }

    /// `TaskManager::task_is_unit_usable` (`ai/task_manager.rs:1271`).
    pub fn task_is_unit_usable(&self, task_id: u32, unit: UnitId) -> bool {
        self.state
            .ai
            .task_manager
            .task_is_unit_usable(self.state, task_id, unit)
    }

    /// `access::surface_type` (`access.rs:324`).
    pub fn surface_type(&self, grid: Point) -> u8 {
        crate::access::surface_type(self.state, grid)
    }

    /// `deploy::survey_resources_in_area` (`deploy.rs:121`).
    pub fn survey_resources_in_area(&self, grid: Point, radius: i32) -> (i32, i32, i32) {
        crate::deploy::survey_resources_in_area(self.state, grid, radius)
    }

    /// `transfer::complex_free_capacity` (`transfer.rs:376`).
    pub fn complex_free_capacity(
        &self,
        team: TeamId,
        complex: crate::team::ComplexId,
        cargo: CargoType,
    ) -> i32 {
        crate::transfer::complex_free_capacity(self.state, team, complex, cargo)
    }

    /// `build_orders::get_filtered_weight_table` (`ai/build_orders.rs`).
    pub fn get_filtered_weight_table(
        &self,
        player: &crate::ai::AiPlayer,
        unit_type: Option<UnitType>,
        flags: u8,
    ) -> crate::ai::weight::WeightTable {
        crate::ai::build_orders::get_filtered_weight_table(player, self.state, unit_type, flags)
    }
}

impl<'a, D: Dispatcher> From<&'a GameUniverse<D>> for StateRef<'a> {
    /// The read view of a facade instance — the same as [`GameUniverse::snapshot`],
    /// written as a conversion for the scenario/menu tests.
    fn from(universe: &'a GameUniverse<D>) -> Self {
        universe.snapshot()
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
    /// Wraps an existing state with a dispatcher (crate-internal: the public
    /// constructors are [`Self::with_dispatcher`],
    /// [`Self::with_setup_and_dispatcher`] and `GameUniverse::<LocalDispatcher>`'s
    /// `with_setup`/`from_save`).
    pub(crate) fn new(state: GameState, dispatcher: D) -> Self {
        Self {
            state,
            dispatcher,
            seq: 0,
            outgoing: Vec::new(),
            bus: EventBus::default(),
        }
    }

    /// An empty world with the given dispatcher (network-FSM/test harness).
    pub fn with_dispatcher(dispatcher: D) -> Self {
        Self::new(GameState::new(), dispatcher)
    }

    /// A world from `GameState::with_setup` with an explicit dispatcher (A3).
    ///
    /// This is the common world generator of the local and the network
    /// constructors: only the dispatcher differs, so the canonical world bytes are
    /// the same. `GameUniverse::<LocalDispatcher>::with_setup` delegates here with
    /// [`LocalDispatcher`].
    pub fn with_setup_and_dispatcher(
        map: Map,
        team_kinds: [TeamKind; PLAYER_TEAM_COUNT],
        dispatcher: D,
    ) -> Self {
        Self::new(GameState::with_setup(map, team_kinds), dispatcher)
    }

    /// Replaces the world with a fresh `GameState::with_setup` state, keeping the
    /// dispatcher (stage C §4.1, A3).
    ///
    /// The counterpart of [`Self::with_setup_and_dispatcher`] for a universe that
    /// already owns its transport policy: the common world generator of the local
    /// and the network constructors writes the generated state **in place**, so the
    /// large state is never returned or moved by value. The facade counters
    /// (`seq`, `outgoing`) and the event bus start over — exactly the state after
    /// the constructor above, so the canonical bytes are identical.
    pub fn reset_world(&mut self, map: Map, team_kinds: [TeamKind; PLAYER_TEAM_COUNT]) {
        self.state = GameState::with_setup(map, team_kinds);
        self.seq = 0;
        self.outgoing.clear();
        self.bus = EventBus::default();
    }

    // — the dispatcher: the asynchronous driver's entry point (A2) —

    /// The transport policy stored in this universe (A2).
    ///
    /// This is the documented entry point of the asynchronous driver (the network
    /// game loop): the driver owns the barrier FSM, reads the world through
    /// [`Self::snapshot`] and pairs this accessor with [`Self::drain_outgoing`] and
    /// [`Self::submit_remote`]. The dispatcher is **facade** state: it never enters
    /// `GameState::snapshot_bytes` or any CRC (ADR §3, guardrails 6).
    pub fn dispatcher(&self) -> &D {
        &self.dispatcher
    }

    /// The transport policy stored in this universe, mutable (A2) — see
    /// [`Self::dispatcher`].
    pub fn dispatcher_mut(&mut self) -> &mut D {
        &mut self.dispatcher
    }

    // — lockstep channel (A3): submit → tick → drain_outgoing, plus submit_remote —
    //
    // The command channel of any match, local or networked: `submit` queues, `tick`
    // applies in receive order, `drain_outgoing` hands the DTOs to the transport and
    // `submit_remote` brings peer DTOs in. UI code writes here and nowhere else;
    // `apply_now`, `set_*` and `*_mut` are not part of this channel (see the module
    // docs, "Channels and consumers").

    /// Accepts a command: routes it and puts it into the simulation queue.
    ///
    /// **Channel:** lockstep (A3) — the command entry of a match. UI code writes
    /// here; the install API never routes commands.
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
    /// **Channel:** shared clock (A3) — the step of the lockstep channel and of the
    /// local machine alike; it is not a rendering or input entry.
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
        //
        // The plan is **extended**, not replaced: a `submit(Broadcast)` packet is
        // queued at submission time and must survive the following `tick`
        // (decision A2: `NetworkDispatcher::plan` returns `vec![]`). `drain_outgoing`
        // takes everything accumulated since the previous drain.
        self.outgoing.extend(self.dispatcher.plan(&facts));
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
    ///
    /// **Channel:** lockstep (A3) — the write half of the transport; the driver
    /// pairs it with [`Self::submit_remote`].
    pub fn drain_outgoing(&mut self) -> Vec<NetworkCommand> {
        std::mem::take(&mut self.outgoing)
    }

    /// Applies a DTO that arrived from a peer.
    ///
    /// **Channel:** lockstep (A3) — the read half of the transport; the driver
    /// pairs it with [`Self::drain_outgoing`].
    ///
    /// * a replayed command is queued (applied at the next tick);
    /// * a barrier-consent packet `00`/`52`/`45` is applied **immediately**
    ///   (decision A4): the barrier wait reads the consent counters without
    ///   ticking, so a queued barrier would time out;
    /// * an authoritative state sync (packet 54) is applied **immediately** and
    ///   counts as a state version (owner decision 2).
    pub fn submit_remote(&mut self, cmd: RemoteCommand) {
        match self.dispatcher.receive(cmd) {
            RemoteHandling::Replay(command) => {
                self.notify_chat(&command);
                self.state.enqueue(command);
            }
            RemoteHandling::ApplyBarrier(command) => {
                self.apply_barrier(&command);
            }
            RemoteHandling::ApplyStateSync(packet) => {
                self.apply_state_sync(&packet);
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

    /// The simulation RNG, mutable — the scenario harness sets a saved state.
    pub fn rng_mut(&mut self) -> &mut crate::rng::Randomizer {
        &mut self.state.rng
    }

    /// `GameManager_TeamTurnTurnBased` round cursor (`game.rs:269`) — setup write.
    pub fn set_round_team(&mut self, value: usize) {
        self.state.round_team = value;
    }

    /// `UnitsManager_UnitIdCounter` (`game.rs:277`) — setup write (V70/CRC tests).
    pub fn set_unit_id_counter(&mut self, value: u32) {
        self.state.unit_id_counter = value;
    }

    /// The pending unit-event queue (`game.rs:293`), mutable (scenario harness).
    pub fn unit_events_mut(&mut self) -> &mut Vec<UnitEvent> {
        &mut self.state.unit_events
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

    /// The whole team table, mutable (setup/test writer; per-slot `team_mut` is
    /// preferable in production code).
    pub fn teams_mut(&mut self) -> &mut [TeamInfo; TEAM_SLOTS] {
        &mut self.state.teams
    }

    /// The whole team-economy table, mutable (setup/test writer).
    pub fn team_units_table_mut(&mut self) -> &mut [TeamUnits; TEAM_SLOTS] {
        &mut self.state.team_units
    }

    /// Host-authoritative team elimination (`PLAN-host-mode.md` §4, H4).
    ///
    /// Sets the slot to [`TeamKind::Eliminated`] — the barrier FSM's
    /// `remote_teams` filter and the victory ranking then stop waiting for it —
    /// and emits the deterministic [`GameEvent::PlayerEliminated`] report.
    ///
    /// The mutation is applied by every client at the same lockstep point and
    /// with the same `sequence` (the host heartbeat tick), so it is part of the
    /// deterministic state (`snapshot_bytes`). Returns `false` for an empty or
    /// already eliminated slot.
    pub fn eliminate_team(&mut self, team: TeamId, sequence: u32) -> bool {
        let Some(info) = self.team_mut(team) else {
            return false;
        };
        if info.team_type == TeamKind::None || info.team_type == TeamKind::Eliminated {
            return false;
        }
        info.team_type = TeamKind::Eliminated;
        self.bus.push(GameEvent::PlayerEliminated { team, sequence });
        true
    }

    /// `GameManager_PlayerTeam` (`game.rs:270`) — setup write.
    pub fn set_player_team(&mut self, team: TeamId) {
        self.state.player_team = team;
    }

    /// `GameManager_ActiveTurnTeam` (`game.rs:269`) — setup write.
    pub fn set_active_turn_team(&mut self, team: TeamId) {
        self.state.active_turn_team = team;
    }

    /// `GameManager_TurnCounter` (`game.rs:258`) — match start/resume write.
    pub fn set_turn_counter(&mut self, value: i32) {
        self.state.turn_counter = value;
    }

    /// Packet 12 mission supplies (`game.rs:340`) — the lobby setup input.
    pub fn set_mission_supplies(&mut self, supplies: [Option<Packet12MissionSupply>; TEAM_SLOTS]) {
        self.state.mission_supplies = supplies;
    }

    /// The AI state (`game.rs:366`) — setup configuration (`ai.settings.opponent`, …).
    pub fn ai_mut(&mut self) -> &mut AiState {
        &mut self.state.ai
    }

    /// The V70 save data (`game.rs:383`) — save/load configuration.
    pub fn save_meta_mut(&mut self) -> &mut SaveMeta {
        &mut self.state.save_meta
    }

    /// `GameState::complex_mut` (`game.rs:3148`) — the alloc-dialog writer.
    pub fn complex_mut(
        &mut self,
        team: TeamId,
        complex_id: u16,
    ) -> Option<&mut Complex> {
        self.state.complex_mut(team, complex_id)
    }

    /// `GameState::set_research_allocation` (`game.rs:2014`); returns the actual
    /// allocation.
    pub fn set_research_allocation(
        &mut self,
        team: TeamId,
        targets: [u32; RESEARCH_TOPIC_COUNT],
    ) -> [u32; RESEARCH_TOPIC_COUNT] {
        self.state.set_research_allocation(team, targets)
    }

    /// `ResearchMenu_UpdateResearchProgress` (`game.rs:1972`) — the research-centre
    /// allocation of one topic.
    pub fn update_research_progress(&mut self, team: TeamId, topic: ResearchTopic, allocation: i32) {
        self.state.update_research_progress(team, topic, allocation);
    }

    /// `DeployConstructionSiteUtilities` (`production.rs`) — the setup/QA builder order.
    pub fn deploy_construction_site(
        &mut self,
        builder: UnitId,
        unit_type: UnitType,
        site: Point,
    ) -> Option<crate::production::ConstructionSite> {
        crate::production::deploy_construction_site(&mut self.state, builder, unit_type, site)
    }

    /// `ProductionManager::enqueue` (`production.rs`) — the QA/test build queue.
    pub fn enqueue_build(
        &mut self,
        team: TeamId,
        builder: UnitId,
        unit_type: UnitType,
    ) -> Result<usize, crate::production::BuildError> {
        let mut manager = crate::production::ProductionManager::new(&mut self.state, team);
        manager.enqueue(builder, unit_type)
    }

    /// `ProductionManager::commit` (`production.rs`) — the QA/test build order.
    pub fn commit_build(
        &mut self,
        team: TeamId,
        builder: UnitId,
    ) -> Result<crate::production::BuildReport, crate::production::BuildError> {
        let mut manager = crate::production::ProductionManager::new(&mut self.state, team);
        manager.commit(builder)
    }

    /// `GameManager_ProgressBuildState` (`production.rs`) — the QA production turn.
    pub fn progress_build_state(&mut self, team: TeamId) -> Vec<ProductionEvent> {
        crate::production::progress_build_state(&mut self.state, team)
    }

    /// `issue_builder_exit` (`unit_order.rs`) — the builder leaves the finished building.
    pub fn issue_builder_exit(&mut self, builder: UnitId) -> bool {
        crate::unit_order::issue_builder_exit(&mut self.state, builder)
    }

    /// `Ai_Init` (`ai/mod.rs:453`) — `GameManager_InitGame`/load entry.
    pub fn ai_init(&mut self) {
        crate::ai::ai_init(&mut self.state);
    }

    /// `Ai_SetupStrategy` (`ai/mod.rs:467`) — the computer-team strategy choice.
    pub fn ai_setup_strategy(&mut self, team: TeamId) -> bool {
        crate::ai::ai_setup_strategy(&mut self.state, team)
    }

    /// `AllocMenu_Optimize` (`alloc.rs`) — the mining allocation writer.
    pub fn optimize_allocation(
        &mut self,
        team: TeamId,
        complex: crate::team::ComplexId,
        kind: crate::alloc::MiningKind,
        material: i32,
    ) -> i32 {
        crate::alloc::optimize(&mut self.state, team, complex, kind, material)
    }

    /// `AllocMenu_ReduceProduction` (`alloc.rs`) — the mining allocation writer.
    pub fn reduce_production_allocation(
        &mut self,
        team: TeamId,
        complex: crate::team::ComplexId,
        kind: crate::alloc::MiningKind,
        amount: i32,
    ) {
        crate::alloc::reduce_production(&mut self.state, team, complex, kind, amount);
    }

    /// The game map, mutable (setup/test writer: resources, surfaces).
    pub fn map_mut(&mut self) -> &mut Map {
        &mut self.state.map
    }

    /// Barrier-counter write for the FSM/network tests (`game.rs:319-333`).
    pub fn set_frame_sync_counter(&mut self, team: TeamId, value: u8) {
        self.state.frame_sync_counter[team.0 as usize] = value;
    }

    /// `Remote_TurnIndices` write (packet 52 tests).
    pub fn set_turn_index(&mut self, team: TeamId, value: u8) {
        self.state.turn_indices[team.0 as usize] = value;
    }

    /// `Remote_NextTurnIndices` write (packet 45 tests).
    pub fn set_next_turn_index(&mut self, team: TeamId, value: u8) {
        self.state.next_turn_indices[team.0 as usize] = value;
    }

    /// `Remote_StateSyncIndices` write (packet 54 tests).
    pub fn set_state_sync_index(&mut self, team: TeamId, value: u8) {
        self.state.state_sync_indices[team.0 as usize] = value;
    }

    /// `Remote_TeamDataCrc16` write (packet 45 tests).
    pub fn set_team_data_crc16(&mut self, team: TeamId, value: u16) {
        self.state.team_data_crc16[team.0 as usize] = value;
    }

    /// `Remote_FrameSyncCounter` table, mutable (packet 00 tests).
    pub fn frame_sync_counters_mut(&mut self) -> &mut [u8; TEAM_SLOTS] {
        &mut self.state.frame_sync_counter
    }

    /// `Remote_TurnIndices` table, mutable (packet 52 tests).
    pub fn turn_indices_mut(&mut self) -> &mut [u8; TEAM_SLOTS] {
        &mut self.state.turn_indices
    }

    /// `Remote_NextTurnIndices` table, mutable (packet 45 tests).
    pub fn next_turn_indices_mut(&mut self) -> &mut [u8; TEAM_SLOTS] {
        &mut self.state.next_turn_indices
    }

    /// `Remote_TeamDataCrc16` table, mutable (packet 45 tests).
    pub fn team_data_crc16_mut(&mut self) -> &mut [u16; TEAM_SLOTS] {
        &mut self.state.team_data_crc16
    }
    /// `Complex::Transfer` (`transfer.rs`) — the QA complex-stock writer.
    pub fn complex_transfer(
        &mut self,
        team: TeamId,
        complex: crate::team::ComplexId,
        raw: i32,
        fuel: i32,
        gold: i32,
    ) {
        crate::transfer::complex_transfer(&mut self.state, team, complex, raw, fuel, gold);
    }

    /// `Randomizer_Generate(101)` — the stealth dice roll of packet 08/P3
    /// (`GameManager_StealUnit`/`GameManager_DisableUnit`, `game_manager.cpp:4680/4696`).
    ///
    /// The roll goes into the command payload, so it is generated by the application
    /// but consumes the simulation RNG stream exactly as before.
    pub fn generate_rng_i32(&mut self, bound: i32) -> i32 {
        self.state.rng.generate(bound)
    }
    /// `AllocMenu_Balance` (`alloc.rs`) — normalizes the complex distribution and
    /// returns the `cargo_2` balance.
    pub fn allocation_balance(
        &mut self,
        team: TeamId,
        complex: crate::team::ComplexId,
        kind: crate::alloc::MiningKind,
        material1: i32,
        material2: i32,
    ) -> i32 {
        crate::alloc::balance(&mut self.state, team, complex, kind, material1, material2)
    }

    // — stage C §5: the barrier-FSM channel —
    //
    // `apply_now`/`flush_commands`/`apply_state_sync`/`apply_barrier` are the
    // **immediate** channel of the barrier FSM (`max_app::NetworkDispatcher`), not
    // the lockstep `submit` path: the barrier wait (`net::wait_barrier`) and the
    // per-frame pacing packet (`01`, decision A5) observe the effect before the
    // next `tick`, and the consent counters they touch are simulation state
    // (`snapshot_bytes`), so a queued replay would shift them by a tick. Peer
    // packets reach this channel only through [`Self::submit_remote`]
    // (`RemoteHandling::ApplyBarrier`/`ApplyStateSync`); the application's
    // immediate commands (`06`/`01`) use `apply_now` directly. They are not a
    // second lockstep API: `apply_now`/`flush_commands` do not bump `seq`.

    /// Applies a command right away — the **FSM channel** (stage C §5).
    ///
    /// **Channel:** local machine (A3) — the immediate packet entry. UI code never
    /// calls it: a UI intent goes through [`Self::submit`], so that the peers see it.
    ///
    /// The barrier FSM and the immediate command paths (`06`/`01`, decision A5)
    /// need the effect before the next `tick`, which the lockstep [`Self::submit`]
    /// cannot give (it only queues). The outcome is observable by the caller —
    /// exactly like `Remote_ReceiveNetPacket` — and the call is not a state
    /// version (`seq` unchanged), so it never shifts the snapshot/CRC.
    pub fn apply_now(&mut self, command: Command) -> ApplyOutcome {
        events::apply_command(&mut self.state, &command)
    }

    /// Applies the already queued commands — the **FSM/config boundary** (§5).
    ///
    /// `GameState::apply_pending_commands` at a configuration boundary where the
    /// caller must observe the queued lobby packets before the world is regenerated
    /// (`net::initialize_game` reads the mission supplies); the regular path
    /// applies the queue inside [`Self::tick`].
    pub fn flush_commands(&mut self) -> usize {
        self.state.apply_pending_commands()
    }
    // — stage B §6: UI/QA report passthroughs for the local loop —
    //
    // The local match (`local.rs`) consumes the core's report queues at the turn
    // boundary, before the next `tick` — at that moment the facade has not drained
    // them into the `EventStream` yet. These 1:1 passthroughs keep that timing
    // (stage-b §6 item 3: `take_*` through the facade) without changing behavior;
    // a consumer that polls the stream after a tick uses `EventStream` instead.

    /// `GameState::take_production_events` (`game.rs:2695`).
    pub fn take_production_events(&mut self) -> Vec<ProductionEvent> {
        self.state.take_production_events()
    }

    /// `GameState::take_research_events` (`game.rs:2075`).
    pub fn take_research_events(&mut self) -> Vec<ResearchEvent> {
        self.state.take_research_events()
    }

    /// `GameState::take_unit_sounds` (`game.rs:2083`).
    pub fn take_unit_sounds(&mut self) -> Vec<UnitSound> {
        self.state.take_unit_sounds()
    }

    /// Applies an authoritative state sync (packet 54) immediately — the **FSM
    /// channel** (stage C §5).
    ///
    /// Packet 54 is authoritative state, not an intent, so [`Self::submit_remote`]
    /// (`RemoteHandling::ApplyStateSync`) applies it without waiting for a tick;
    /// it counts as a state version, like that branch (owner decision 2).
    pub fn apply_state_sync(&mut self, packet: &StateSyncPacket) -> ApplyOutcome {
        let outcome = events::apply_state_sync(&mut self.state, packet);
        self.seq += 1;
        outcome
    }

    /// Applies a barrier-consent packet (`00`/`52`/`45`) immediately — the **FSM
    /// channel** (stage C §5, decision A4).
    ///
    /// Barrier packets are read by the synchronous barrier wait
    /// (`net::wait_barrier`), so they cannot wait for the next `tick`; the state
    /// version is bumped, as for any observable state change.
    pub fn apply_barrier(&mut self, command: &Command) -> ApplyOutcome {
        let outcome = events::apply_command(&mut self.state, command);
        if outcome == ApplyOutcome::Applied {
            self.seq += 1;
        }
        outcome
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

    // — stage B §7: the remaining setup/test writers used by the integration
    // scenarios after the encapsulation cut (1:1 calls, no `seq` bump) —

    /// `GameState::take_industry_message` (`game.rs:2721`) — the UI report queue.
    pub fn take_industry_message(&mut self) -> Option<String> {
        self.state.take_industry_message()
    }

    /// `GameState::advance_research` (`game.rs:2064`) — the research turn.
    pub fn advance_research(&mut self, team: TeamId) -> Vec<crate::research::ResearchEvent> {
        self.state.advance_research(team)
    }

    /// `research::research_new_turn` (`research.rs`) — one research-centre turn
    /// over the team's economy and counters.
    pub fn research_new_turn(&mut self, team: TeamId) -> Vec<crate::research::ResearchEvent> {
        crate::research::research_new_turn(
            &mut self.state.team_units[team.0 as usize],
            &mut self.state.teams[team.0 as usize],
            team,
        )
    }

    /// `GameState::ai_begin_turn` (`ai/mod.rs:429`) — the AI turn start.
    pub fn ai_begin_turn(&mut self, team: TeamId) {
        self.state.ai_begin_turn(team);
    }

    /// `GameState::update_map_status` (`ai/threat.rs:708`) — the scan of one unit.
    pub fn update_map_status(&mut self, id: UnitId, mode: bool) {
        self.state.update_map_status(id, mode);
    }

    /// `GameState::ai_append_task` (`ai/mod.rs:240`) — the scenario task writer.
    pub fn ai_append_task(
        &mut self,
        team: TeamId,
        parent: Option<u32>,
        priority: u16,
        kind: crate::ai::TaskKind,
    ) -> u32 {
        self.state.ai_append_task(team, parent, priority, kind)
    }

    /// `GameState::ai_remind_move_finished` (`ai/mod.rs:260`).
    pub fn ai_remind_move_finished(&mut self, unit: UnitId, priority: bool) {
        self.state.ai_remind_move_finished(unit, priority);
    }

    /// `GameState::execute_reminders` (`ai/mod.rs:353`).
    pub fn execute_reminders(&mut self) -> bool {
        self.state.execute_reminders()
    }

    /// `TaskManager::begin_turn` through the temporary task-manager split
    /// (`GameState::with_task_manager`, `ai/mod.rs:229`) — the scenario harness
    /// drives the task manager turn directly.
    pub fn ai_task_begin_turn(&mut self, team: TeamId) {
        self.state
            .with_task_manager(|task_manager, world| task_manager.begin_turn(world, team));
    }

    /// `TaskManager::end_turn` through `GameState::with_task_manager`.
    pub fn ai_task_end_turn(&mut self, team: TeamId) {
        self.state
            .with_task_manager(|task_manager, world| task_manager.end_turn(world, team));
    }

    /// `TaskManager::attach_task_to_unit` through `GameState::with_task_manager`.
    pub fn ai_attach_task_to_unit(&mut self, unit: UnitId, task: u32) {
        self.state
            .with_task_manager(|task_manager, _| task_manager.attach_task_to_unit(unit, task));
    }

    /// `GameState::add_to_draw_list` (`game.rs:1117`) — the V70 draw-list writer.
    pub fn add_to_draw_list(&mut self, id: UnitId) {
        self.state.add_to_draw_list(id);
    }

    /// `GameState::process_orders` (`game.rs:1780`) — the rules phase of one tick.
    pub fn process_orders(&mut self) {
        self.state.process_orders();
    }

    /// `GameState::push_message` (`save_write.rs:857`) — the V70 message-log writer.
    pub fn push_message(&mut self, team: u8, text: &str, point: Point, unit: Option<UnitId>) {
        self.state.push_message(team, text, point, unit);
    }

    /// `combat::attack_unit` (`combat.rs:100`) — direct damage (scenario writer).
    pub fn attack_unit(
        &mut self,
        target_id: UnitId,
        attacker_id: UnitId,
        attack_potential: i32,
        direction: i32,
    ) {
        crate::combat::attack_unit(
            &mut self.state,
            target_id,
            attacker_id,
            attack_potential,
            direction,
        );
    }

    /// `combat::attack` (`combat.rs`) — the fire order of one unit.
    pub fn attack(&mut self, id: UnitId, grid: Point) {
        crate::combat::attack(&mut self.state, id, grid);
    }

    /// `combat::check_reaction` (`combat.rs:746`).
    pub fn check_reaction(&mut self, unit1_id: UnitId, unit2_id: UnitId) -> bool {
        crate::combat::check_reaction(&mut self.state, unit1_id, unit2_id)
    }

    /// `combat::spot_unit` (`combat.rs:207`).
    pub fn spot_unit(&mut self, id: UnitId, team: TeamId) {
        crate::combat::spot_unit(&mut self.state, id, team);
    }

    /// `events::process_unit_event` (`events.rs:86`).
    pub fn process_unit_event(&mut self, event: &UnitEvent) {
        events::process_unit_event(&mut self.state, event);
    }

    /// `deploy::deploy_starting_units` (`deploy.rs:654`).
    pub fn deploy_starting_units(
        &mut self,
        start_gold: u32,
        max_resources: i32,
        teams: &[crate::setup::TeamStart],
    ) -> usize {
        crate::deploy::deploy_starting_units(&mut self.state, start_gold, max_resources, teams)
    }

    /// `deploy::deploy_starting_units_with` (`deploy.rs:674`).
    pub fn deploy_starting_units_with(
        &mut self,
        max_resources: i32,
        deployments: &[crate::deploy::TeamDeployment],
    ) -> usize {
        crate::deploy::deploy_starting_units_with(&mut self.state, max_resources, deployments)
    }

    /// `victory::check_end_game_conditions` (`victory.rs:1506`).
    pub fn check_end_game_conditions(&mut self) -> bool {
        victory::check_end_game_conditions(&mut self.state)
    }

    /// `alloc::update_resources_total` (`alloc.rs:638`).
    pub fn update_resources_total(&mut self, team: TeamId, complex: crate::team::ComplexId) {
        crate::alloc::update_resources_total(&mut self.state, team, complex);
    }

    /// `transfer::transfer_cargo_kind` (`transfer.rs:183`).
    pub fn transfer_cargo_kind(&mut self, unit_id: UnitId, cargo: CargoType, amount: i32) {
        crate::transfer::transfer_cargo_kind(&mut self.state, unit_id, cargo, amount);
    }

    /// `transfer::process_transfer` (`transfer.rs:259`).
    pub fn process_transfer(&mut self, unit_id: UnitId) {
        crate::transfer::process_transfer(&mut self.state, unit_id);
    }

    /// `power::optimize_complex` (`power.rs:586`) — the industry allocator.
    pub fn optimize_complex(
        &mut self,
        team: TeamId,
        complex: crate::team::ComplexId,
        selected: Option<UnitId>,
        proposal: Option<UnitId>,
        show_messages: bool,
    ) -> crate::power::IndustryOutcome {
        crate::power::optimize_complex(
            &mut self.state,
            team,
            complex,
            selected,
            proposal,
            show_messages,
        )
    }

    /// `unit_order::issue_factory_exit` (`unit_order.rs:2380`).
    pub fn issue_factory_exit(&mut self, factory_id: UnitId) -> bool {
        crate::unit_order::issue_factory_exit(&mut self.state, factory_id)
    }

    /// `unit_order::process_order` (`unit_order.rs`) — one rules step of a unit.
    pub fn process_order(&mut self, id: UnitId) {
        crate::unit_order::process_order(&mut self.state, id);
    }

    /// `unit_order::position_in_tape` (`unit_order.rs`) — the builder tape step.
    pub fn position_in_tape(&mut self, unit: &mut UnitInfo, move_to: Point) {
        crate::unit_order::position_in_tape(&mut self.state, unit, move_to);
    }

    /// `access::build_clearing` (`access.rs`) — the terrain-clearing builder step.
    pub fn build_clearing(&mut self, unit_id: UnitId, mode: bool) -> Option<UnitId> {
        crate::access::build_clearing(&mut self.state, unit_id, mode)
    }

    /// `ai_unit_spotted` (`ai/mod.rs`) — the AI report of a spotted unit.
    pub fn ai_unit_spotted(&mut self, unit: UnitId, team: TeamId) {
        crate::ai::ai_unit_spotted(&mut self.state, unit, team);
    }

    /// The production queue of a team (`production.rs:1156`), borrowing the state.
    pub fn production_manager(&mut self, team: TeamId) -> crate::production::ProductionManager<'_> {
        crate::production::ProductionManager::new(&mut self.state, team)
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
    ///
    /// Delegates to [`Self::with_setup_and_dispatcher`] with [`LocalDispatcher`],
    /// so the local and the network constructors share one world generator (A3).
    pub fn with_setup(map: Map, team_kinds: [TeamKind; PLAYER_TEAM_COUNT]) -> Self {
        Self::with_setup_and_dispatcher(map, team_kinds, LocalDispatcher)
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

    // — local-machine channel (A3): begin_local_round / resume_local_round,
    // end_local_turn, tick and apply_now —
    //
    // The sequential team-turn machine of a single-player match, plus the immediate
    // packet path that must be observable before the next tick (`apply_now` lives in
    // the dispatcher-generic block above). Every entry is gated by
    // `Dispatcher::advances_turns`; a network match advances turns through the
    // barrier FSM instead, and the UI never closes a turn directly.

    /// Local End Turn: packet `06` **immediately**, then `enter_end_turn` and one
    /// run of the team-turn machine to the next acting team (ADR §1, §4).
    ///
    /// **Channel:** local machine (A3) — the turn close of a single-player match;
    /// a network dispatcher does not advance turns here (see
    /// [`Dispatcher::advances_turns`]).
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
    /// **Channel:** local machine (A3) — the round entry; the UI acts only after the
    /// machine has produced a [`GameEvent::Turn`].
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
    /// **Channel:** local machine (A3) — the load entry; a match loaded from V70
    /// enters here, not through [`Self::begin_local_round`].
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

/// Stage-A/B equivalence scenarios (`tests/universe*.rs` before §7): they need
/// the legacy `GameState` path next to the facade path, which is crate-private
/// after the encapsulation cut.
#[cfg(test)]
mod scenario_tests;
