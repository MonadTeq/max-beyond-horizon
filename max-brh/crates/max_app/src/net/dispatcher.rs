//! Network transport policy + the barrier FSM (stage B, step 1).
//!
//! Source of truth: `max-brh-docs/development/game-universe-stage-b.md` §2
//! (`NetworkDispatcher` + the **verbatim** `TurnBarrierFsm` transfer) and ADR §2
//! (`max-brh-docs/architecture/client/game-universe-facade.md`).
//!
//! The wait/retransmit FSM (`net.rs:1393-1583` before the move) is transferred
//! unchanged: the phases, the separate pre-incremented counters (00/52/54/45),
//! a 3 s retransmit (`BARRIER_RETRY`) and a 30 s timeout (`BARRIER_TIMEOUT`),
//! the resume-54 and the `06`-once-per-turn flag are the same logic, line by line.
//!
//! The only mechanical change (task §0.2, decision A1): the parameter
//! `&mut GameState` became `&mut GameUniverse<D>`, and the counter step calls
//! `state.step_*_barrier(...)` became the facade passthroughs
//! [`GameUniverse::step_begin_turn_barrier`]/[`GameUniverse::step_end_turn_barrier`]/
//! [`GameUniverse::step_crc_barrier`]. The FSM never mutates `GameState` directly;
//! the counters stay simulation state (`snapshot_bytes`).
//!
//! Stage C §4.4 (A1) splits the FSM steps into a **counter step** (the facade
//! `step_*_barrier`) and a **clean record + packet** half
//! (`record_begin`/`record_end`/`record_crc`, plus `record_complex`/`record_sync`
//! for the read-only projections): the driver then never holds two `&mut` borrows
//! of the universe at once (`universe.dispatcher_mut().begin(&mut universe)`).
//! The packets, the counters, the phases and the timing constants are unchanged —
//! the split is mechanical.
//!
//! [`NetworkDispatcher`] implements [`Dispatcher`] on top: simulation commands are
//! broadcast, packet 54 is applied as authoritative state, and the facade turn
//! machine is off (`advances_turns = false`) — a networked match advances the turn
//! through this FSM, not through `next_team` (ADR §2). `plan()` is empty: the
//! barrier packets are asynchronous and are delivered by the driver through
//! `SessionHandle`, not by `tick()` (decision A2).

use std::time::Duration;

use max_core::enums::PlayMode;
use max_core::game::PLAYER_TEAM_COUNT;
use max_core::input::{Command, Packet00TurnBarrier, Packet45CrcBarrier, Packet52EndTurnBarrier};
use max_core::team::{TeamId, TeamKind};
use max_core::universe::{
    BarrierFacts, Dispatcher, GameUniverse, NetworkCommand, RemoteCommand, RemoteHandling,
    RoutedCommand,
};
use max_net::session::{DEFAULT_BARRIER_REPEAT, DEFAULT_BARRIER_TIMEOUT};
use max_net::{state_sync_for, Packet};
use tokio::time::Instant;

/// Barrier repeat (`REMOTE_PING_TIME_PERIOD`).
pub(crate) const BARRIER_RETRY: Duration = DEFAULT_BARRIER_REPEAT;
/// Peer wait timeout (`REMOTE_RESPONSE_TIMEOUT`).
pub(crate) const BARRIER_TIMEOUT: Duration = DEFAULT_BARRIER_TIMEOUT;

/// Barrier FSM phase of the current turn (`max-brh-docs/barrier-flow.md` §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BarrierPhase {
    /// 00 — entering the round.
    Begin,
    /// 53 — base totals at the start of the turn.
    Play,
    /// 52 — end of the turn.
    End,
    /// 54 — team state sync.
    Sync,
    /// 45 — CRC barrier.
    Crc,
}

impl BarrierPhase {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Begin => "00",
            Self::Play => "53",
            Self::End => "52",
            Self::Sync => "54",
            Self::Crc => "45",
        }
    }
}

/// Client barrier FSM of a turn: `00 → 53 → 06 → 52 → 54 → 45`.
///
/// The counters are separate (`Remote_FrameSyncCounter`, `Remote_TurnIndices`,
/// `Remote_StateSyncIndex`, `Remote_NextTurnIndices`), all pre-increment: the first
/// report is **1**. `GameManager_TurnCounter` in C++ starts at 1, while
/// `GameState::turn_counter` — at 0, so 53 carries `turn_counter + 1`.
#[derive(Debug)]
struct TurnBarrierFsm {
    local_team: TeamId,
    /// In `SimultaneousMoves` the 00 barrier is needed every turn, in `TurnBased` — once
    /// at the start (`game_manager.cpp:496`).
    simultaneous: bool,
    phase: BarrierPhase,
    begin_counter: u8,
    end_counter: u8,
    sync_index: u8,
    crc_index: u8,
    /// `GameManager_TurnCounter` for 53 (1-based).
    complex_turn: i32,
    /// Which `turn` the last 53 came with for each team.
    received_complex: [Option<i32>; PLAYER_TEAM_COUNT],
    /// Own world CRC in the current 45 round.
    crc_checksum: u16,
    /// 06 sent once per turn.
    local_finished_sent: bool,
}

impl TurnBarrierFsm {
    fn new(local_team: TeamId, play_mode: PlayMode) -> Self {
        Self {
            local_team,
            simultaneous: play_mode == PlayMode::SimultaneousMoves,
            phase: BarrierPhase::Begin,
            begin_counter: 0,
            end_counter: 0,
            sync_index: 0,
            crc_index: 0,
            complex_turn: 0,
            received_complex: [None; PLAYER_TEAM_COUNT],
            crc_checksum: 0,
            local_finished_sent: false,
        }
    }

    /// Peer teams: those whose barriers must be awaited (`TEAM_TYPE_REMOTE`).
    fn remote_teams<'a, D: Dispatcher>(
        &'a self,
        universe: &'a GameUniverse<D>,
    ) -> impl Iterator<Item = usize> + 'a {
        (0..PLAYER_TEAM_COUNT)
            .filter(move |&team| universe.snapshot().teams()[team].team_type == TeamKind::Remote)
    }

    /// `Remote_WaitBeginTurnAcknowledge`: `++Remote_FrameSyncCounter[own]` → 00.
    ///
    /// Stage C decision A1 (`game-universe-stage-c.md` §0.2): the counter step stays
    /// a core transition through the facade passthrough
    /// ([`GameUniverse::step_begin_turn_barrier`]), while this method is the clean
    /// **record + packet** half — it takes the already stepped counter, so the
    /// driver never needs two `&mut` borrows of the universe at once.
    fn record_begin(&mut self, counter: u8) -> Vec<Packet> {
        self.begin_counter = counter;
        self.phase = BarrierPhase::Begin;

        vec![Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
            team: self.local_team,
            counter,
        }))]
    }

    fn begin_reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        universe
            .snapshot()
            .begin_turn_barrier_reached(self.begin_counter)
    }

    /// `Remote_WaitComplexTotals`: records the C++ turn number of the own 53.
    ///
    /// Stage C A1: the record half of the former `complex`; the packet itself is a
    /// read-only projection over the snapshot
    /// ([`max_net::complex_summary_for`]) built by the driver.
    fn record_complex(&mut self, turn: i32) {
        self.complex_turn = turn;
        self.phase = BarrierPhase::Play;
    }

    /// Remembers an incoming 53 (called for all teams, including own).
    fn note_complex(&mut self, team: TeamId, turn: i32) {
        if let Some(slot) = self.received_complex.get_mut(team.0 as usize) {
            *slot = Some(turn);
        }
    }

    fn complex_reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        self.remote_teams(universe)
            .all(|team| self.received_complex[team] == Some(self.complex_turn))
    }

    /// `Remote_WaitEndTurnAcknowledge`: `++Remote_TurnIndices[own]` → 52.
    ///
    /// Stage C A1: the clean record half of the former `end` (see
    /// [`Self::record_begin`]).
    fn record_end(&mut self, counter: u8) -> Vec<Packet> {
        self.end_counter = counter;
        self.phase = BarrierPhase::End;

        vec![Packet::Command(Command::EndTurnBarrier(
            Packet52EndTurnBarrier {
                team: self.local_team,
                counter,
            },
        ))]
    }

    fn end_reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        universe
            .snapshot()
            .end_turn_barrier_reached(self.end_counter)
    }

    /// `Remote_SynchronizeUnitStates`: `++Remote_StateSyncIndex` → 54.
    ///
    /// Stage C A1: the record half of the former `sync`; it advances the own index
    /// (pre-increment, the first report is 1) and returns it — the packet itself is
    /// the read-only [`max_net::state_sync_for`] projection built by the driver.
    fn record_sync(&mut self) -> u8 {
        self.sync_index = self.sync_index.wrapping_add(1);
        self.phase = BarrierPhase::Sync;
        self.sync_index
    }

    fn sync_reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        self.remote_teams(universe)
            .all(|team| universe.snapshot().state_sync_indices()[team] == self.sync_index)
    }

    /// Own team state for restoration after a reconnect.
    ///
    /// Packet 54 is the only "snapshot" in the protocol (`max-brh-docs/protocol.md` §8.5,
    /// §9): it carries the units, gold, research and id counters of one
    /// team. After a disconnect the client re-publishes **its own** state with
    /// the last completed `sync_index`: the server and peers apply it as
    /// authoritative (`Remote_ReceiveNetPacket_54`), while the peers' 54
    /// correct the remote teams on our side. There is no full map/SNAP in the
    /// protocol — see §9 of protocol.md about the limits.
    fn resume_state_sync<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> Vec<Packet> {
        state_sync_for(universe.snapshot(), self.local_team, self.sync_index)
            .into_iter()
            .map(Packet::StateSync)
            .collect()
    }

    /// `Remote_CheckDesync`: `++Remote_NextTurnIndices[own]` → 45.
    ///
    /// Stage C A1: the clean record half of the former `crc` — the facade stepped
    /// the counter (`GameUniverse::step_crc_barrier`) and the driver read the
    /// checksum (`StateRef::state_checksum`), so this method only stores the facts
    /// and builds the packet (the same fields
    /// [`max_net::crc_barrier_for`] projected from the snapshot).
    fn record_crc(&mut self, index: u8, checksum: u16) -> Vec<Packet> {
        self.crc_index = index;
        self.crc_checksum = checksum;
        self.phase = BarrierPhase::Crc;

        vec![Packet::Command(Command::CrcBarrier(Packet45CrcBarrier {
            team: self.local_team,
            next_turn_index: index,
            crc16: checksum,
        }))]
    }

    fn crc_reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        universe.snapshot().crc_barrier_reached(self.crc_index)
    }

    /// CRC comparison after the 45 barrier: all peers have the same `crc16`.
    fn crc_matches<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        self.remote_teams(universe)
            .all(|team| universe.snapshot().team_data_crc16()[team] == self.crc_checksum)
    }

    /// The current phase is complete (all peers sent the expected value).
    fn reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        match self.phase {
            BarrierPhase::Begin => self.begin_reached(universe),
            BarrierPhase::Play => self.complex_reached(universe),
            BarrierPhase::End => self.end_reached(universe),
            BarrierPhase::Sync => self.sync_reached(universe),
            BarrierPhase::Crc => self.crc_reached(universe),
        }
    }

    fn mark_local_finished(&mut self) {
        self.local_finished_sent = true;
    }

    fn local_finished_sent(&self) -> bool {
        self.local_finished_sent
    }

    /// The turn is closed (`GameManager_TeamTurnFinish`): reset the local flag,
    /// the next 00 — only in `SimultaneousMoves`.
    fn on_turn_finished(&mut self) {
        self.local_finished_sent = false;
        self.phase = if self.simultaneous {
            BarrierPhase::Begin
        } else {
            BarrierPhase::Play
        };
    }
}

/// Outcome of waiting for a barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaitOutcome {
    /// The barrier is complete.
    Reached,
    /// The session ended.
    Stopped,
}

/// Deferred barrier send: retry every 3 s, timeout log after 30 s.
///
/// Unlike [`max_net::BarrierTracker`], the record does **not** disappear after
/// the timeout: C++ keeps pinging the lagging team, Rust logs and waits
/// (`max-brh-docs/barrier-flow.md` §3).
pub(crate) struct BarrierPending {
    pub(crate) packets: Vec<Packet>,
    first_sent: Instant,
    last_sent: Instant,
    pub(crate) attempts: u32,
    timeout_reported: bool,
}

impl BarrierPending {
    pub(crate) fn new(packets: Vec<Packet>, now: Instant) -> Self {
        Self {
            packets,
            first_sent: now,
            last_sent: now,
            attempts: 0,
            timeout_reported: false,
        }
    }

    pub(crate) fn poll(&mut self, now: Instant) -> Option<BarrierPoll> {
        if !self.timeout_reported && now.duration_since(self.first_sent) >= BARRIER_TIMEOUT {
            self.timeout_reported = true;
            return Some(BarrierPoll::TimedOut);
        }
        if now.duration_since(self.last_sent) >= BARRIER_RETRY {
            self.last_sent = now;
            self.attempts = self.attempts.saturating_add(1);
            return Some(BarrierPoll::Retransmit);
        }
        None
    }
}

pub(crate) enum BarrierPoll {
    Retransmit,
    TimedOut,
}

/// The network dispatcher: the barrier FSM plus the transport policy (stage B).
///
/// The FSM is driven by the async game loop (`net::run_game_loop`) and keeps the
/// barrier packets; the [`Dispatcher`] implementation classifies local/remote
/// commands for the facade. `advances_turns` is `false`, so the facade never runs
/// `next_team` in a network match (ADR §2).
#[derive(Debug)]
pub struct NetworkDispatcher {
    fsm: TurnBarrierFsm,
}

impl NetworkDispatcher {
    /// A dispatcher for `local_team` in the match `play_mode`.
    pub fn new(local_team: TeamId, play_mode: PlayMode) -> Self {
        Self {
            fsm: TurnBarrierFsm::new(local_team, play_mode),
        }
    }

    // — the barrier FSM, driven by the async game loop (verbatim transfer) —

    /// Records the stepped 00 counter and builds the packet (stage C A1).
    pub(crate) fn record_begin(&mut self, counter: u8) -> Vec<Packet> {
        self.fsm.record_begin(counter)
    }

    /// Records the C++ turn number of the own 53 (stage C A1).
    pub(crate) fn record_complex(&mut self, turn: i32) {
        self.fsm.record_complex(turn);
    }

    /// Remembers an incoming 53 (called for all teams, including own).
    pub(crate) fn note_complex(&mut self, team: TeamId, turn: i32) {
        self.fsm.note_complex(team, turn);
    }

    /// Records the stepped 52 counter and builds the packet (stage C A1).
    pub(crate) fn record_end(&mut self, counter: u8) -> Vec<Packet> {
        self.fsm.record_end(counter)
    }

    /// Advances the own 54 index and returns it (stage C A1).
    pub(crate) fn record_sync(&mut self) -> u8 {
        self.fsm.record_sync()
    }

    /// Own team state for restoration after a reconnect (resume-54).
    pub(crate) fn resume_state_sync<D: Dispatcher>(
        &self,
        universe: &GameUniverse<D>,
    ) -> Vec<Packet> {
        self.fsm.resume_state_sync(universe)
    }

    /// Records the stepped 45 index and checksum, builds the packet (stage C A1).
    pub(crate) fn record_crc(&mut self, index: u8, checksum: u16) -> Vec<Packet> {
        self.fsm.record_crc(index, checksum)
    }

    /// The current phase is complete (all peers sent the expected value).
    pub(crate) fn reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        self.fsm.reached(universe)
    }

    /// CRC comparison after the 45 barrier: all peers have the same `crc16`.
    pub(crate) fn crc_matches<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        self.fsm.crc_matches(universe)
    }

    /// 06 is sent once per turn (the flag resets on `on_turn_finished`).
    pub(crate) fn mark_local_finished(&mut self) {
        self.fsm.mark_local_finished();
    }

    /// Whether 06 was already sent in this turn.
    pub(crate) fn local_finished_sent(&self) -> bool {
        self.fsm.local_finished_sent()
    }

    /// The turn is closed: reset the 06 flag and re-arm 00 only in `SimultaneousMoves`.
    pub(crate) fn on_turn_finished(&mut self) {
        self.fsm.on_turn_finished();
    }

    /// Peer teams whose barriers must be awaited (`TEAM_TYPE_REMOTE`).
    pub(crate) fn remote_teams<'a, D: Dispatcher>(
        &'a self,
        universe: &'a GameUniverse<D>,
    ) -> impl Iterator<Item = usize> + 'a {
        self.fsm.remote_teams(universe)
    }

    // — read-only FSM facts for logs/reports —

    /// The local team of the match.
    pub(crate) fn local_team(&self) -> TeamId {
        self.fsm.local_team
    }

    /// Whether the 00 barrier is needed every turn (`PLAY_MODE_SIMULTANEOUS_MOVES`).
    pub(crate) fn simultaneous(&self) -> bool {
        self.fsm.simultaneous
    }

    /// `GameManager_TurnCounter` of the current 53.
    pub(crate) fn complex_turn(&self) -> i32 {
        self.fsm.complex_turn
    }

    /// The last completed (or in-flight) 54 index.
    pub(crate) fn sync_index(&self) -> u8 {
        self.fsm.sync_index
    }

    /// The own CRC of the current 45 round.
    pub(crate) fn crc_checksum(&self) -> u16 {
        self.fsm.crc_checksum
    }

    // — FSM instrumentation for the gate-5 tests (not shipped) —

    #[cfg(test)]
    pub(crate) fn phase(&self) -> BarrierPhase {
        self.fsm.phase
    }

    #[cfg(test)]
    pub(crate) fn begin_counter(&self) -> u8 {
        self.fsm.begin_counter
    }

    #[cfg(test)]
    pub(crate) fn end_counter(&self) -> u8 {
        self.fsm.end_counter
    }

    #[cfg(test)]
    pub(crate) fn crc_index(&self) -> u8 {
        self.fsm.crc_index
    }

    #[cfg(test)]
    pub(crate) fn set_sync_index(&mut self, index: u8) {
        self.fsm.sync_index = index;
    }

    #[cfg(test)]
    pub(crate) fn begin_reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        self.fsm.begin_reached(universe)
    }

    #[cfg(test)]
    pub(crate) fn end_reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        self.fsm.end_reached(universe)
    }

    #[cfg(test)]
    pub(crate) fn sync_reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        self.fsm.sync_reached(universe)
    }

    #[cfg(test)]
    pub(crate) fn crc_reached<D: Dispatcher>(&self, universe: &GameUniverse<D>) -> bool {
        self.fsm.crc_reached(universe)
    }
}

impl Dispatcher for NetworkDispatcher {
    /// Simulation commands are broadcast; barriers and 53 are applied locally.
    ///
    /// Packet 09 (team state: gold, upgrade spending, research allocation) is an
    /// explicit broadcast: the oracle sends it as `REMOTE_MULTICAST`
    /// (`remote.cpp:2085`) and the fields enter the CRC — routing it `Local` would
    /// make the owner's gold/research differ from every peer.
    ///
    /// A mid-match packet 13 (`Remote_RngSeed`) is a forbidden order: the world is
    /// seeded once at initialization (`net::initialize_game`), and a repeated
    /// `Randomizer_SetSeed` would reset the stream and diverge from the server
    /// (the driver drops an incoming 13 for the same reason, `net.rs:2085-2087`).
    fn route(&mut self, cmd: Command) -> RoutedCommand {
        match cmd {
            Command::TurnBarrier(_)
            | Command::FrameBarrier(_)
            | Command::EndTurnBarrier(_)
            | Command::CrcBarrier(_)
            | Command::ComplexSummary(_) => RoutedCommand::Local(cmd),
            Command::Seed(_) => RoutedCommand::Drop,
            // 09 — team state, a simulation command on the broadcast channel.
            Command::TeamState(_) => RoutedCommand::Broadcast(cmd),
            other => RoutedCommand::Broadcast(other),
        }
    }

    /// A peer's command is replayed in receive order; packet 54 is authoritative
    /// state and is applied immediately (owner decision 2).
    ///
    /// The barrier-consent packets `00`/`52`/`45` are the exception (decision A4,
    /// `game-universe-stage-b.md` §0.2): the barrier wait reads the consent
    /// counters synchronously and never ticks, so a queued barrier would block
    /// until the 30-second timeout and shift the CRC counters by one tick. They
    /// are therefore applied **immediately** through
    /// [`RemoteHandling::ApplyBarrier`]; `01`/`53` and the order packets keep the
    /// receive-order replay.
    fn receive(&mut self, cmd: RemoteCommand) -> RemoteHandling {
        match cmd {
            RemoteCommand::Command(command) => {
                if is_consent_barrier(&command) {
                    RemoteHandling::ApplyBarrier(command)
                } else {
                    RemoteHandling::Replay(command)
                }
            }
            RemoteCommand::StateSync(packet) => RemoteHandling::ApplyStateSync(packet),
        }
    }

    /// No synchronous barrier plan: the barrier FSM is asynchronous and its
    /// packets are delivered by the driver (decision A2).
    fn plan(&mut self, _facts: &BarrierFacts) -> Vec<NetworkCommand> {
        Vec::new()
    }

    /// A networked match advances the turn through the barrier FSM, not `next_team`.
    fn advances_turns(&mut self) -> bool {
        false
    }
}

/// Whether a command is a barrier-consent packet (`00`/`52`/`45`) — decision A4.
///
/// These three are the packets the barrier wait compares against (`*_barrier_reached`,
/// `net.rs::wait_barrier`); `01` (frame pacing) and `53` (complex summary) are not
/// consent facts and keep the receive-order replay.
fn is_consent_barrier(command: &Command) -> bool {
    matches!(
        command,
        Command::TurnBarrier(_) | Command::EndTurnBarrier(_) | Command::CrcBarrier(_)
    )
}

#[cfg(test)]
#[path = "dispatcher_tests.rs"]
mod tests;
