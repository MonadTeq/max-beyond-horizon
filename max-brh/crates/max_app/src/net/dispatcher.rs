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
//! [`NetworkDispatcher`] implements [`Dispatcher`] on top: simulation commands are
//! broadcast, packet 54 is applied as authoritative state, and the facade turn
//! machine is off (`advances_turns = false`) — a networked match advances the turn
//! through this FSM, not through `next_team` (ADR §2). `plan()` is empty: the
//! barrier packets are asynchronous and are delivered by the driver through
//! `SessionHandle`, not by `tick()` (decision A2).

use std::time::Duration;

use max_core::enums::PlayMode;
use max_core::game::PLAYER_TEAM_COUNT;
use max_core::input::{Command, Packet00TurnBarrier, Packet52EndTurnBarrier};
use max_core::team::{TeamId, TeamKind};
use max_core::universe::{
    BarrierFacts, Dispatcher, GameUniverse, NetworkCommand, RemoteCommand, RemoteHandling,
    RoutedCommand,
};
use max_net::session::{DEFAULT_BARRIER_REPEAT, DEFAULT_BARRIER_TIMEOUT};
use max_net::{complex_summary_for, crc_barrier_for, state_sync_for, Packet};
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

    /// C++ turn number (1-based `GameManager_TurnCounter`).
    fn cxx_turn<D: Dispatcher>(universe: &GameUniverse<D>) -> i32 {
        universe.snapshot().turn_counter() + 1
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
    /// The counter lives in `GameState` (`snapshot_bytes`), so the increment is a
    /// core transition through the facade passthrough
    /// ([`GameUniverse::step_begin_turn_barrier`]); the FSM keeps the packet, the
    /// wait and the retransmit.
    fn begin<D: Dispatcher>(&mut self, universe: &mut GameUniverse<D>) -> Vec<Packet> {
        let counter = universe.step_begin_turn_barrier(self.local_team);
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

    /// `Remote_WaitComplexTotals`: own team's 53 for the current turn.
    fn complex<D: Dispatcher>(&mut self, universe: &GameUniverse<D>) -> Vec<Packet> {
        self.complex_turn = Self::cxx_turn(universe);
        self.phase = BarrierPhase::Play;

        // Read-only projection: `complex_summary_for` only reads `TeamUnits`
        // (stage B step 1 keeps the stage-A facade read access; §6/§7 move the
        // projection onto a complete `StateRef`).
        vec![complex_summary_for(
            universe.state(),
            self.local_team,
            self.complex_turn,
        )]
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
    fn end<D: Dispatcher>(&mut self, universe: &mut GameUniverse<D>) -> Vec<Packet> {
        let counter = universe.step_end_turn_barrier(self.local_team);
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
    fn sync<D: Dispatcher>(&mut self, universe: &GameUniverse<D>) -> Vec<Packet> {
        self.sync_index = self.sync_index.wrapping_add(1);
        self.phase = BarrierPhase::Sync;

        state_sync_for(universe.state(), self.local_team, self.sync_index)
            .into_iter()
            .map(Packet::StateSync)
            .collect()
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
        state_sync_for(universe.state(), self.local_team, self.sync_index)
            .into_iter()
            .map(Packet::StateSync)
            .collect()
    }

    /// `Remote_CheckDesync`: `++Remote_NextTurnIndices[own]` → 45.
    fn crc<D: Dispatcher>(&mut self, universe: &mut GameUniverse<D>) -> Vec<Packet> {
        let index = universe.step_crc_barrier(self.local_team);
        self.crc_index = index;
        self.crc_checksum = universe.snapshot().state_checksum();
        self.phase = BarrierPhase::Crc;

        vec![crc_barrier_for(self.local_team, index, universe.state())]
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

    /// `Remote_WaitBeginTurnAcknowledge`: `++Remote_FrameSyncCounter[own]` → 00.
    pub(crate) fn begin<D: Dispatcher>(&mut self, universe: &mut GameUniverse<D>) -> Vec<Packet> {
        self.fsm.begin(universe)
    }

    /// `Remote_WaitComplexTotals`: own team's 53 for the current turn.
    pub(crate) fn complex<D: Dispatcher>(&mut self, universe: &GameUniverse<D>) -> Vec<Packet> {
        self.fsm.complex(universe)
    }

    /// Remembers an incoming 53 (called for all teams, including own).
    pub(crate) fn note_complex(&mut self, team: TeamId, turn: i32) {
        self.fsm.note_complex(team, turn);
    }

    /// `Remote_WaitEndTurnAcknowledge`: `++Remote_TurnIndices[own]` → 52.
    pub(crate) fn end<D: Dispatcher>(&mut self, universe: &mut GameUniverse<D>) -> Vec<Packet> {
        self.fsm.end(universe)
    }

    /// `Remote_SynchronizeUnitStates`: `++Remote_StateSyncIndex` → 54.
    pub(crate) fn sync<D: Dispatcher>(&mut self, universe: &GameUniverse<D>) -> Vec<Packet> {
        self.fsm.sync(universe)
    }

    /// Own team state for restoration after a reconnect (resume-54).
    pub(crate) fn resume_state_sync<D: Dispatcher>(
        &self,
        universe: &GameUniverse<D>,
    ) -> Vec<Packet> {
        self.fsm.resume_state_sync(universe)
    }

    /// `Remote_CheckDesync`: `++Remote_NextTurnIndices[own]` → 45.
    pub(crate) fn crc<D: Dispatcher>(&mut self, universe: &mut GameUniverse<D>) -> Vec<Packet> {
        self.fsm.crc(universe)
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
            other => RoutedCommand::Broadcast(other),
        }
    }

    /// A peer's command is replayed in receive order; packet 54 is authoritative
    /// state and is applied immediately (owner decision 2).
    fn receive(&mut self, cmd: RemoteCommand) -> RemoteHandling {
        match cmd {
            RemoteCommand::Command(command) => RemoteHandling::Replay(command),
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

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::crc;
    use max_core::enums::{TeamClan, UnitType};
    use max_core::game::GameState;
    use max_core::geo::Point;
    use max_core::input::{
        Packet06FinishedTurn, Packet10UpgradeUnitType, Packet13Seed, Packet18Chat,
        Packet45CrcBarrier, Packet53ComplexSummary,
    };
    use max_core::team::TeamUnits;
    use max_core::universe::{LocalDispatcher, RemoteCommand, SubmitOutcome};
    use max_core::UnitId;

    /// Synthetic world: the local team is `Player`, `remotes` are `Remote`, the rest
    /// are `None` (`Remote_SetupPlayers` team types).
    fn synthetic_state(local: TeamId, remotes: &[TeamId]) -> GameState {
        let mut state = GameState::new();
        for team in 0..PLAYER_TEAM_COUNT {
            state.teams[team].team_type = TeamKind::None;
            state.team_units[team] = TeamUnits::with_clan(TeamClan::Random);
        }
        state.teams[local.0 as usize].team_type = TeamKind::Player;
        for team in remotes {
            state.teams[team.0 as usize].team_type = TeamKind::Remote;
        }
        state.player_team = local;
        state
    }

    fn synthetic_universe(local: TeamId, remotes: &[TeamId]) -> GameUniverse<LocalDispatcher> {
        GameUniverse::new(synthetic_state(local, remotes), LocalDispatcher)
    }

    fn barrier_counter(packet: &Packet) -> u8 {
        match packet {
            Packet::Command(Command::TurnBarrier(barrier)) => barrier.counter,
            Packet::Command(Command::EndTurnBarrier(barrier)) => barrier.counter,
            Packet::Command(Command::CrcBarrier(barrier)) => barrier.next_turn_index,
            other => panic!("not a counter barrier: {other:?}"),
        }
    }

    #[test]
    fn counters_start_at_one_and_are_pre_incremented_per_team() {
        let local = TeamId::RED;
        let mut universe = synthetic_universe(local, &[TeamId::GREEN, TeamId::BLUE]);
        let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);

        let begin = fsm.begin(&mut universe);
        assert_eq!(begin.len(), 1);
        assert_eq!(barrier_counter(&begin[0]), 1, "the first 00 is 1");
        assert_eq!(universe.state().frame_sync_counter[local.0 as usize], 1);
        // The own counter is not yet agreed with the peers.
        assert!(!fsm.begin_reached(&universe));
        universe.state_mut().frame_sync_counter[TeamId::GREEN.0 as usize] = 1;
        assert!(!fsm.begin_reached(&universe), "all peers are required");
        universe.state_mut().frame_sync_counter[TeamId::BLUE.0 as usize] = 1;
        assert!(fsm.begin_reached(&universe));

        let end = fsm.end(&mut universe);
        assert_eq!(barrier_counter(&end[0]), 1, "the first 52 is 1");
        assert_eq!(universe.state().turn_indices[local.0 as usize], 1);
        assert!(!fsm.end_reached(&universe));
        universe.state_mut().turn_indices[TeamId::GREEN.0 as usize] = 1;
        universe.state_mut().turn_indices[TeamId::BLUE.0 as usize] = 1;
        assert!(fsm.end_reached(&universe));

        let sync = fsm.sync(&universe);
        assert!(!sync.is_empty());
        match &sync[0] {
            Packet::StateSync(packet) => {
                assert_eq!(packet.sync_index, 1, "the first 54 is 1");
                assert_eq!(packet.team, local);
                assert!(packet.is_last, "an empty team is one last chunk");
            }
            other => panic!("expected 54, got {other:?}"),
        }
        assert!(!fsm.sync_reached(&universe));
        universe.state_mut().state_sync_indices[TeamId::GREEN.0 as usize] = 1;
        universe.state_mut().state_sync_indices[TeamId::BLUE.0 as usize] = 1;
        assert!(fsm.sync_reached(&universe));

        let crc = fsm.crc(&mut universe);
        assert_eq!(barrier_counter(&crc[0]), 1, "the first 45 is 1");
        assert_eq!(universe.state().next_turn_indices[local.0 as usize], 1);
        let own_crc = match &crc[0] {
            Packet::Command(Command::CrcBarrier(barrier)) => barrier.crc16,
            other => panic!("expected 45, got {other:?}"),
        };
        assert_eq!(own_crc, crc::state_checksum(universe.state()));
        assert!(!fsm.crc_reached(&universe));
        universe.state_mut().next_turn_indices[TeamId::GREEN.0 as usize] = 1;
        universe.state_mut().next_turn_indices[TeamId::BLUE.0 as usize] = 1;
        assert!(fsm.crc_reached(&universe));
        assert!(
            !fsm.crc_matches(&universe),
            "the peer CRCs are not here yet"
        );
        universe.state_mut().team_data_crc16[TeamId::GREEN.0 as usize] = own_crc;
        universe.state_mut().team_data_crc16[TeamId::BLUE.0 as usize] = own_crc;
        assert!(fsm.crc_matches(&universe));
    }

    #[test]
    fn fsm_walks_the_full_turn_sequence() {
        let local = TeamId::RED;
        let mut universe = synthetic_universe(local, &[TeamId::GREEN]);
        let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);

        // 00
        let begin = fsm.begin(&mut universe);
        assert_eq!(fsm.phase(), BarrierPhase::Begin);
        assert_eq!(barrier_counter(&begin[0]), 1);
        universe.state_mut().frame_sync_counter[TeamId::GREEN.0 as usize] = 1;
        assert!(fsm.reached(&universe));

        // 53
        let play = fsm.complex(&universe);
        assert_eq!(fsm.phase(), BarrierPhase::Play);
        assert!(matches!(
            &play[0],
            Packet::Command(Command::ComplexSummary(summary)) if summary.turn == 1
        ));
        fsm.note_complex(TeamId::GREEN, 1);
        assert!(fsm.reached(&universe));

        // 06 — local end of the turn (exactly once).
        fsm.mark_local_finished();
        assert!(fsm.local_finished_sent());

        // 52
        let end = fsm.end(&mut universe);
        assert_eq!(fsm.phase(), BarrierPhase::End);
        assert_eq!(barrier_counter(&end[0]), 1);
        universe.state_mut().turn_indices[TeamId::GREEN.0 as usize] = 1;
        assert!(fsm.reached(&universe));

        // 54
        let sync = fsm.sync(&universe);
        assert_eq!(fsm.phase(), BarrierPhase::Sync);
        assert!(matches!(&sync[0], Packet::StateSync(packet) if packet.sync_index == 1));
        universe.state_mut().state_sync_indices[TeamId::GREEN.0 as usize] = 1;
        assert!(fsm.reached(&universe));

        // 45
        let crc = fsm.crc(&mut universe);
        assert_eq!(fsm.phase(), BarrierPhase::Crc);
        assert_eq!(barrier_counter(&crc[0]), 1);
        universe.state_mut().next_turn_indices[TeamId::GREEN.0 as usize] = 1;
        universe.state_mut().team_data_crc16[TeamId::GREEN.0 as usize] = fsm.crc_checksum();
        assert!(fsm.reached(&universe));
        assert!(fsm.crc_matches(&universe));

        // finish_turn → the next turn starts with a clean 06.
        fsm.on_turn_finished();
        assert!(!fsm.local_finished_sent());
    }

    #[test]
    fn same_kind_barriers_use_separate_counters() {
        let local = TeamId::RED;
        let mut universe = synthetic_universe(local, &[TeamId::GREEN]);
        let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);

        fsm.begin(&mut universe);
        fsm.end(&mut universe);
        fsm.sync(&universe);
        fsm.crc(&mut universe);
        assert_eq!(fsm.begin_counter(), 1);
        assert_eq!(fsm.end_counter(), 1);
        assert_eq!(fsm.sync_index(), 1);
        assert_eq!(fsm.crc_index(), 1);

        // Next turn: each counter has its own pre-increment (2).
        fsm.on_turn_finished();
        let begin = fsm.begin(&mut universe);
        assert_eq!(barrier_counter(&begin[0]), 2);
        let end = fsm.end(&mut universe);
        assert_eq!(barrier_counter(&end[0]), 2);
        let sync = fsm.sync(&universe);
        assert!(matches!(
            &sync[0],
            Packet::StateSync(packet) if packet.sync_index == 2
        ));
        let crc = fsm.crc(&mut universe);
        assert_eq!(barrier_counter(&crc[0]), 2);

        // 52 and 45 are different C++ arrays, not one shared counter.
        let state = universe.state();
        assert_eq!(state.turn_indices[local.0 as usize], 2);
        assert_eq!(state.next_turn_indices[local.0 as usize], 2);
        assert_eq!(state.frame_sync_counter[local.0 as usize], 2);
    }

    #[test]
    fn complex_barrier_needs_53_from_every_peer_for_the_current_turn() {
        let local = TeamId::RED;
        let mut universe = synthetic_universe(local, &[TeamId::GREEN, TeamId::BLUE]);
        // Rust turn_counter = 0 → C++ GameManager_TurnCounter = 1.
        let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);
        let packet = fsm.complex(&universe);
        assert!(matches!(
            &packet[0],
            Packet::Command(Command::ComplexSummary(summary)) if summary.turn == 1
        ));
        assert!(!fsm.reached(&universe));

        fsm.note_complex(TeamId::GREEN, 1);
        assert!(!fsm.reached(&universe), "BLUE has not sent 53 yet");
        fsm.note_complex(TeamId::BLUE, 1);
        assert!(fsm.reached(&universe));

        // The 53 of the previous turn does not close the new one.
        universe.state_mut().turn_counter = 1;
        let packet = fsm.complex(&universe);
        assert!(matches!(
            &packet[0],
            Packet::Command(Command::ComplexSummary(summary)) if summary.turn == 2
        ));
        assert!(!fsm.reached(&universe));
        fsm.note_complex(TeamId::GREEN, 2);
        fsm.note_complex(TeamId::BLUE, 2);
        assert!(fsm.reached(&universe));
    }

    #[test]
    fn single_player_barriers_pass_immediately() {
        let local = TeamId::RED;
        let mut universe = synthetic_universe(local, &[]);
        let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);

        let packets = fsm.begin(&mut universe);
        assert!(!packets.is_empty());
        assert!(fsm.reached(&universe), "00 with no peers");

        let packets = fsm.complex(&universe);
        assert!(!packets.is_empty());
        assert!(fsm.reached(&universe), "53 with no peers");

        let packets = fsm.end(&mut universe);
        assert!(!packets.is_empty());
        assert!(fsm.reached(&universe), "52 with no peers");

        let packets = fsm.sync(&universe);
        assert!(!packets.is_empty());
        assert!(fsm.reached(&universe), "54 with no peers");

        let packets = fsm.crc(&mut universe);
        assert!(!packets.is_empty());
        assert!(fsm.reached(&universe), "45 with no peers");
        assert!(fsm.crc_matches(&universe));
    }

    #[test]
    fn turn_based_runs_begin_once_simultaneous_every_turn() {
        let local = TeamId::RED;

        assert!(!NetworkDispatcher::new(local, PlayMode::TurnBased).simultaneous());
        assert!(NetworkDispatcher::new(local, PlayMode::SimultaneousMoves).simultaneous());
    }

    #[test]
    fn local_06_is_sent_once_per_turn_and_reset_on_finish() {
        let local = TeamId::RED;
        let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);

        assert!(!fsm.local_finished_sent());
        fsm.mark_local_finished();
        assert!(fsm.local_finished_sent());

        // A new turn resets the flag: 06 will be sent again.
        fsm.on_turn_finished();
        assert!(!fsm.local_finished_sent(), "a new turn — a new 06");
    }

    #[test]
    fn barrier_timings_are_the_cpp_constants() {
        assert_eq!(
            BARRIER_RETRY,
            Duration::from_secs(3),
            "REMOTE_PING_TIME_PERIOD must stay 3 s"
        );
        assert_eq!(
            BARRIER_TIMEOUT,
            Duration::from_secs(30),
            "REMOTE_RESPONSE_TIMEOUT must stay 30 s"
        );
        assert_eq!(BARRIER_RETRY, DEFAULT_BARRIER_REPEAT);
        assert_eq!(BARRIER_TIMEOUT, DEFAULT_BARRIER_TIMEOUT);
    }

    #[test]
    fn barrier_pending_repeats_every_three_seconds_and_reports_timeout_once() {
        let start = Instant::now();
        let packet = Packet::Command(Command::CrcBarrier(Packet45CrcBarrier {
            team: TeamId::RED,
            next_turn_index: 1,
            crc16: 0,
        }));
        let mut pending = BarrierPending::new(vec![packet], start);

        assert!(pending.poll(start).is_none());
        assert!(matches!(
            pending.poll(start + Duration::from_secs(3)),
            Some(BarrierPoll::Retransmit)
        ));
        assert_eq!(pending.attempts, 1);
        assert!(matches!(
            pending.poll(start + Duration::from_secs(6)),
            Some(BarrierPoll::Retransmit)
        ));
        assert_eq!(pending.attempts, 2);

        // The timeout is logged once, but the record does not disappear — the barrier
        // keeps repeating, like a C++ ping.
        assert!(matches!(
            pending.poll(start + Duration::from_secs(30)),
            Some(BarrierPoll::TimedOut)
        ));
        assert!(matches!(
            pending.poll(start + Duration::from_secs(31)),
            Some(BarrierPoll::Retransmit)
        ));
        assert!(pending.poll(start + Duration::from_secs(32)).is_none());
    }

    /// The same three-client world as the `net.rs` disconnect scenario: one tank per
    /// team, clients only differ in the team kinds.
    fn three_team_state() -> (GameState, [UnitId; 3]) {
        let mut state = GameState::new();
        let teams = [TeamId::RED, TeamId::GREEN, TeamId::BLUE];
        let mut ids: [UnitId; 3] = [UnitId::NO_ID; 3];

        for (index, team) in teams.iter().copied().enumerate() {
            state.team_units[team.0 as usize] = TeamUnits::with_clan(TeamClan::Random);
            ids[index] = state
                .spawn_unit(
                    UnitType::Tank,
                    team,
                    Point::new(20 + index as i16 * 5, 20),
                    0,
                )
                .expect("tank spawns");
        }
        state.play_mode = PlayMode::SimultaneousMoves;
        (state, ids)
    }

    /// Resume-54 after a reconnect: own team, the last sync_index and the full
    /// set of team units (including the economy tail in the last chunk).
    #[test]
    fn resume_state_sync_publishes_the_last_completed_index() {
        let (base, ids) = three_team_state();
        let universe = GameUniverse::new(base, LocalDispatcher);
        let mut fsm = NetworkDispatcher::new(TeamId::GREEN, PlayMode::SimultaneousMoves);
        fsm.set_sync_index(7);

        let packets = fsm.resume_state_sync(&universe);
        assert_eq!(packets.len(), 1, "one unit — one last chunk");
        match &packets[0] {
            Packet::StateSync(sync) => {
                assert_eq!(sync.team, TeamId::GREEN);
                assert_eq!(sync.sync_index, 7, "the last completed round");
                assert!(sync.is_last);
                assert_eq!(sync.units.len(), 1);
                assert_eq!(sync.units[0].0, ids[1], "only the own team's units");
            }
            other => panic!("expected 54, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Dispatcher policy (ADR §2)
    // -----------------------------------------------------------------------

    fn barrier_facts() -> BarrierFacts {
        BarrierFacts {
            begin_turn: Some(0),
            end_turn: Some(0),
            crc: Some(0),
            state_sync: [0; max_core::TEAM_SLOTS],
            local_team: TeamId::RED,
            play_mode: PlayMode::TurnBased,
        }
    }

    #[test]
    fn dispatcher_routes_simulation_broadcast_barriers_local_and_seed_drop() {
        let mut dispatcher = NetworkDispatcher::new(TeamId::RED, PlayMode::TurnBased);

        let simulation = Command::FinishedTurn(Packet06FinishedTurn { team: TeamId::RED });
        assert_eq!(
            dispatcher.route(simulation.clone()),
            RoutedCommand::Broadcast(simulation)
        );
        let move_order = Command::UpgradeUnitType(Packet10UpgradeUnitType {
            team: TeamId::RED,
            unit_type: UnitType::Tank,
            values: [0; 13],
        });
        assert_eq!(
            dispatcher.route(move_order.clone()),
            RoutedCommand::Broadcast(move_order)
        );
        let chat = Command::Chat(Packet18Chat {
            sender: TeamId::RED,
            message: "hi".into(),
        });
        assert_eq!(
            dispatcher.route(chat.clone()),
            RoutedCommand::Broadcast(chat),
            "18 is a broadcast command (ADR §2)"
        );

        for barrier in [
            Command::TurnBarrier(Packet00TurnBarrier {
                team: TeamId::RED,
                counter: 1,
            }),
            Command::EndTurnBarrier(Packet52EndTurnBarrier {
                team: TeamId::RED,
                counter: 1,
            }),
            Command::CrcBarrier(Packet45CrcBarrier {
                team: TeamId::RED,
                next_turn_index: 1,
                crc16: 0,
            }),
            Command::ComplexSummary(Packet53ComplexSummary {
                team: TeamId::RED,
                turn: 1,
                complexes: Vec::new(),
            }),
        ] {
            assert_eq!(
                dispatcher.route(barrier.clone()),
                RoutedCommand::Local(barrier),
                "a barrier stays local"
            );
        }

        assert_eq!(
            dispatcher.route(Command::Seed(Packet13Seed { seed: 7 })),
            RoutedCommand::Drop,
            "a mid-match 13 is forbidden"
        );
    }

    #[test]
    fn dispatcher_receive_replays_commands_and_applies_state_sync() {
        let mut dispatcher = NetworkDispatcher::new(TeamId::RED, PlayMode::TurnBased);

        let command = Command::FinishedTurn(Packet06FinishedTurn {
            team: TeamId::GREEN,
        });
        assert_eq!(
            dispatcher.receive(RemoteCommand::Command(command.clone())),
            RemoteHandling::Replay(command)
        );

        let packet = max_core::events::StateSyncPacket {
            team: TeamId::GREEN,
            sync_index: 2,
            is_last: false,
            units: Vec::new(),
            tail: None,
        };
        assert_eq!(
            dispatcher.receive(RemoteCommand::StateSync(packet.clone())),
            RemoteHandling::ApplyStateSync(packet)
        );
    }

    #[test]
    fn dispatcher_never_advances_turns_and_has_no_barrier_plan() {
        let mut dispatcher = NetworkDispatcher::new(TeamId::RED, PlayMode::TurnBased);
        assert!(!dispatcher.advances_turns());
        assert!(dispatcher.plan(&barrier_facts()).is_empty());
    }

    #[test]
    fn facade_universe_with_network_dispatcher_broadcasts_and_drops() {
        let local = TeamId::RED;
        let mut universe = GameUniverse::new(
            synthetic_state(local, &[TeamId::GREEN]),
            NetworkDispatcher::new(local, PlayMode::TurnBased),
        );

        let simulation = Command::FinishedTurn(Packet06FinishedTurn { team: local });
        assert_eq!(universe.submit(simulation.clone()), SubmitOutcome::Accepted);
        assert_eq!(
            universe.drain_outgoing(),
            vec![NetworkCommand::Command(simulation)]
        );

        // A barrier submitted locally is not broadcast.
        let barrier = Command::TurnBarrier(Packet00TurnBarrier {
            team: local,
            counter: 1,
        });
        assert_eq!(universe.submit(barrier), SubmitOutcome::Accepted);
        assert!(universe.drain_outgoing().is_empty());

        assert_eq!(
            universe.submit(Command::Seed(Packet13Seed { seed: 7 })),
            SubmitOutcome::Dropped
        );
        assert!(
            universe.drain_outgoing().is_empty(),
            "a dropped command never reaches the transport"
        );
    }
}
