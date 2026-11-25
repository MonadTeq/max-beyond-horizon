//! Host migration, quorum gate and `SessionPolicy` enforcement
//! (`PLAN-host-mode.md` §3/§4, H4).
//!
//! # Responsibilities
//!
//! * **Heartbeat / `host_timeout`.** A guest that hears no heartbeat 55 for
//!   [`P2pTransport`]'s `host_timeout` declares the host failed. The timeout is
//!   a transport property, validated in `5..=300 s` by
//!   [`max_net::validate_host_timeout`], and is deliberately separate from the
//!   elimination budget (§6.2).
//! * **Deterministic election.** The successor is `min(player_node)` among the
//!   registered players other than the current host. Every client computes the
//!   same candidate from the roster it received in the lobby (packet 30), so the
//!   election needs no vote round.
//! * **Takeover (packet 56).** The candidate binds the session's ENet port and
//!   sends a [`Takeover`] proposal with `epoch + 1`, the lockstep position and
//!   the roster. Survivors validate the proposal against their own position and
//!   answer `Accept`/`Reject`; the candidate resumes only after
//!   `reachable + 1 >= quorum`.
//! * **Resume-54.** Migration is reported to the driver as
//!   [`DisconnectOutcome::Restored`], so the ordinary reconnect path re-publishes
//!   the survivor's own packet 54 and the barriers continue from the same point.
//! * **Quorum.** `quorum = floor(N/2)+1`. The host only closes a barrier when
//!   `reachable_peers + 1 >= quorum`; a lost guest is eliminated
//!   (`TeamEliminated`) only with quorum, so a minority section never continues.
//! * **SessionPolicy.** `Pause` (default) keeps the match frozen; `EliminateAfter
//!   { ticks }` removes a silent team after `ticks_without_heartbeat`. The
//!   counter is evaluated in **numbered session ticks** carried by the
//!   heartbeat, never in wall-clock seconds: two clients with different clock
//!   drift or heartbeat latency record the same elimination tick.
//!
//! # Tick model
//!
//! The host numbers every heartbeat: `session_tick` starts at 1 and is
//! incremented on each publish. The heartbeat also carries `last_seen[team]`
//! (the last tick at which the host heard the team) and the resulting
//! `eliminated`/`eliminated_tick` decisions. Elimination is therefore a pure
//! function of `last_seen + ticks`; a client that receives a heartbeat late (or
//! misses an intermediate one) applies exactly the tick the authority decided.
//!
//! # Boundaries
//!
//! This module is application policy: it lives in `max_app`, uses the
//! [`HostIo`] transport surface and never touches the barrier FSM
//! ([`crate::net::dispatcher`]) directly. Packets 55/56 are transport frames and
//! never reach `Dispatcher::route`.
//!
//! # H5: standalone guest reconnect and split-brain transport rules
//!
//! A guest that loses its ENet link while the authority is **alive** (the H5
//! gate) does not wait for a takeover: it probes the endpoint with `TP 00`, and
//! a live authority means an ordinary reconnect without a new lobby
//! ([`P2pTransport::reconnect_guest`]). The successor candidate takes over only
//! when the probe fails. A guest whose link is up but silent keeps that link —
//! during a split-brain the ENet session still holds the delayed traffic, and
//! the driver re-publishes the last closed and the pending barrier on
//! `Restored`/`PeerReconnected`, so no closed barrier packet is lost.

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use max_core::game::PLAYER_TEAM_COUNT;
use max_core::team::TeamId;
use max_net::{
    validate_host_timeout, HostConfig, HostError, HostEvent, HostEventKind, HostHandle,
    HostHeartbeat, HostSessionPolicy, Packet, Recipient, Session, SessionConfig, Takeover,
    TakeoverCounters, TakeoverKind, TakeoverRosterEntry, DEFAULT_HEARTBEAT_PERIOD,
    DEFAULT_HOST_TIMEOUT, DEFAULT_MAX_PEERS, TAKEOVER_MAX_ROSTER,
};

use crate::host::SessionPolicy;
use crate::net::dispatcher::BarrierPhase;
use crate::net::{DirectEndpoint, DisconnectOutcome, HostIo, IoEvent, ReconnectPolicy, SessionIo};

/// Backoff between migration attempts.
const MIGRATION_RETRY: Duration = Duration::from_millis(200);
/// Timeout of one survivor reconnect attempt.
const MIGRATION_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// `TP 00` liveness probe of the old authority before a takeover.
const MIGRATION_PROBE_TIMEOUT: Duration = Duration::from_millis(500);
/// How long a survivor may stay paused without a quorum before the driver ends
/// the paused match (the state is consistent; nothing is advanced).
const MIGRATION_WINDOW: Duration = Duration::from_secs(3);
/// Maximum blocking wait of one transport step; the driver polls every ~2 ms.
const STEP_MAX_WAIT: Duration = Duration::from_millis(20);
/// Upper bound of the frames held during the test-only split-brain injection.
#[cfg(test)]
const BLACKOUT_MAX_FRAMES: usize = 32;

/// One team the session authority removed (`TeamEliminated`, §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TeamEliminated {
    /// Removed team.
    pub team: TeamId,
    /// Deterministic session tick of the decision (also the `sequence` of
    /// `PlayerEliminated`).
    pub tick: u32,
}

impl TeamEliminated {
    /// `sequence` reported to the simulation (`PlayerEliminated{team, sequence}`).
    pub const fn sequence(self) -> u32 {
        self.tick
    }
}

/// Lockstep position of one client, used to validate a takeover.
///
/// All values are read-only projections of the simulation
/// (`Remote_FrameSyncCounter`/`Remote_TurnIndices`/`Remote_StateSyncIndex`/
/// `Remote_NextTurnIndices` of the own team), so the takeover is validated
/// without inspecting the private barrier FSM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct LockstepPosition {
    /// `GameState::turn_counter`.
    pub turn_counter: i32,
    /// Barrier-FSM phase tag (see [`phase_tag`]).
    pub phase: u8,
    /// Packet 00 counter of the own team.
    pub begin: u8,
    /// Packet 52 counter of the own team.
    pub end: u8,
    /// Packet 54 index of the own team.
    pub sync: u8,
    /// Packet 45 index of the own team.
    pub crc: u8,
}

impl LockstepPosition {
    /// Counter block carried by a takeover frame.
    pub const fn counters(self) -> TakeoverCounters {
        TakeoverCounters {
            begin: self.begin,
            end: self.end,
            sync: self.sync,
            crc: self.crc,
        }
    }

    /// Whether `takeover` describes exactly this lockstep point.
    ///
    /// A mismatch means the two clients are not at the same barrier; resuming
    /// would diverge, so the survivor refuses the takeover and pauses
    /// (`PLAN-host-mode.md` §3: "a mismatch = STOP").
    pub fn matches(self, takeover: &Takeover) -> bool {
        self.turn_counter == takeover.turn_counter
            && self.phase == takeover.phase
            && self.counters() == takeover.counters
    }
}

/// Opaque phase tag of the barrier FSM (`00/53/52/54/45`).
///
/// The FSM itself is untouched: the driver maps its `BarrierPhase` to this
/// stable wire value and hands the position to the transport.
pub(crate) const fn phase_tag(phase: BarrierPhase) -> u8 {
    match phase {
        BarrierPhase::Begin => 1,
        BarrierPhase::Play => 2,
        BarrierPhase::End => 3,
        BarrierPhase::Sync => 4,
        BarrierPhase::Crc => 5,
    }
}

/// Registered players of the session (packet 30 `team_nodes` + `host_node`).
#[derive(Debug, Clone)]
pub(crate) struct P2pRoster {
    /// Own player node.
    pub local_node: u16,
    /// Current authority node (`host_node`).
    pub host_node: u16,
    /// Team slot → player node (`0` — empty slot).
    pub team_nodes: [u16; PLAYER_TEAM_COUNT],
}

impl P2pRoster {
    /// Roster of a lobby snapshot.
    pub fn new(local_node: u16, host_node: u16, team_nodes: [u16; PLAYER_TEAM_COUNT]) -> Self {
        Self {
            local_node,
            host_node,
            team_nodes,
        }
    }

    /// Number of players registered at the start (`node > 4`, like
    /// `Remote_SetupPlayers`).
    pub fn active_players(&self) -> usize {
        self.team_nodes.iter().filter(|&&node| node > 4).count()
    }

    /// `floor(N_active / 2) + 1` (`PLAN-host-mode.md` §4).
    pub fn quorum(&self) -> usize {
        self.active_players() / 2 + 1
    }

    /// Team slot holding `node`.
    pub fn team_slot(&self, node: u16) -> Option<usize> {
        self.team_nodes
            .iter()
            .position(|&candidate| candidate == node)
    }

    /// Whether a team slot is occupied by a client.
    pub fn occupied(&self, team: usize) -> bool {
        self.team_nodes.get(team).copied().unwrap_or(0) > 4
    }

    /// Deterministic successor: `min(player_node)` among the registered
    /// players except the current authority (`None` when nobody qualifies).
    ///
    /// Every client computes the same candidate from the lobby roster, so no
    /// vote is needed. A candidate that is dead simply never takes over, which
    /// is safe: the survivors pause instead of electing a minority section.
    pub fn successor(&self) -> Option<u16> {
        self.team_nodes
            .iter()
            .copied()
            .filter(|&node| node > 4 && node != self.host_node)
            .min()
    }

    /// Whether the local node is the deterministic successor.
    pub fn is_successor(&self) -> bool {
        self.successor() == Some(self.local_node)
    }

    /// Full roster for a takeover proposal.
    pub fn takeover_roster(&self) -> Vec<TakeoverRosterEntry> {
        let mut entries: Vec<TakeoverRosterEntry> = self
            .team_nodes
            .iter()
            .enumerate()
            .filter(|(_, &node)| node > 4)
            .map(|(team, &node)| TakeoverRosterEntry {
                node,
                team_slot: team as u8,
                reachable: node != self.host_node,
            })
            .collect();
        entries.truncate(TAKEOVER_MAX_ROSTER);
        entries
    }

    /// Whether `entries` describes the same player set as this roster.
    pub fn roster_matches(&self, entries: &[TakeoverRosterEntry]) -> bool {
        if entries.len() != self.active_players() {
            return false;
        }
        entries
            .iter()
            .all(|entry| self.team_slot(entry.node) == Some(usize::from(entry.team_slot)))
    }
}

/// Application policy → heartbeat wire value.
pub(crate) const fn policy_to_wire(policy: SessionPolicy) -> HostSessionPolicy {
    match policy {
        SessionPolicy::Pause => HostSessionPolicy::Pause,
        SessionPolicy::EliminateAfter { ticks } => HostSessionPolicy::EliminateAfter { ticks },
    }
}

/// Inverse of [`policy_to_wire`].
pub(crate) const fn policy_from_wire(policy: HostSessionPolicy) -> SessionPolicy {
    match policy {
        HostSessionPolicy::Pause => SessionPolicy::Pause,
        HostSessionPolicy::EliminateAfter { ticks } => SessionPolicy::EliminateAfter { ticks },
    }
}

/// Host session clock: numbered heartbeat ticks, liveness and eliminations.
///
/// Both the host (which decides) and the unit tests (which compare two clients
/// with different drift) use this clock. The authority increments `tick` once
/// per published heartbeat; a team is eliminated when
/// `tick - last_seen[team] >= ticks` **and** `reachable + 1 >= quorum`.
#[derive(Debug, Clone)]
pub(crate) struct SessionClock {
    /// Authority epoch.
    pub epoch: u32,
    /// Current heartbeat number (`0` before the first publish).
    pub tick: u32,
    /// Session policy.
    pub policy: SessionPolicy,
    /// `host_timeout` advertised to guests.
    pub host_timeout: Duration,
    /// Heartbeat period (to convert the timeout into ticks).
    pub heartbeat: Duration,
    /// Quorum of the roster.
    pub quorum: usize,
    /// Own team (never eliminated).
    pub local_team: TeamId,
    /// Whether the team slot is registered (from the roster).
    pub occupied: [bool; PLAYER_TEAM_COUNT],
    /// Whether the team slot is eliminated.
    pub eliminated: [bool; PLAYER_TEAM_COUNT],
    /// Elimination tick per team slot (`0` — not eliminated).
    pub eliminated_tick: [u32; PLAYER_TEAM_COUNT],
    /// The last session tick at which each team was heard (`1` at match start).
    pub last_seen: [u32; PLAYER_TEAM_COUNT],
}

impl SessionClock {
    /// A fresh host clock: every remote player is assumed reachable at tick 1.
    pub fn new(
        roster: &P2pRoster,
        local_team: TeamId,
        policy: SessionPolicy,
        host_timeout: Duration,
        heartbeat: Duration,
    ) -> Self {
        let mut occupied = [false; PLAYER_TEAM_COUNT];
        for (team, slot) in occupied.iter_mut().enumerate() {
            *slot = roster.occupied(team);
        }
        Self {
            epoch: 1,
            tick: 0,
            policy,
            host_timeout,
            heartbeat: heartbeat.max(Duration::from_millis(1)),
            quorum: roster.quorum(),
            local_team,
            occupied,
            eliminated: [false; PLAYER_TEAM_COUNT],
            eliminated_tick: [0; PLAYER_TEAM_COUNT],
            last_seen: fresh_last_seen(&occupied),
        }
    }

    /// Restarts the clock for a new authority epoch (takeover).
    pub fn reset_for_epoch(&mut self, epoch: u32) {
        self.epoch = epoch;
        self.tick = 0;
        self.eliminated = [false; PLAYER_TEAM_COUNT];
        self.eliminated_tick = [0; PLAYER_TEAM_COUNT];
        self.last_seen = fresh_last_seen(&self.occupied);
    }

    /// Marks the team as heard at the current tick.
    pub fn heard(&mut self, team: usize) {
        if team < PLAYER_TEAM_COUNT {
            self.last_seen[team] = self.tick.max(1);
        }
    }

    /// Number of `host_timeout` heartbeat ticks.
    fn timeout_ticks(&self) -> u32 {
        let period = self.heartbeat.as_millis().max(1);
        let ticks = self.host_timeout.as_millis().div_ceil(period);
        ticks.min(u128::from(u32::MAX)) as u32
    }

    /// Silence threshold that marks a team unreachable.
    fn reachable_threshold(&self) -> u32 {
        match self.policy {
            SessionPolicy::EliminateAfter { ticks } => ticks.max(1),
            SessionPolicy::Pause => self.timeout_ticks(),
        }
    }

    /// Mask of reachable players, indexed by **team slot** (`bit team`),
    /// excluding eliminated teams and the host's own team.
    pub fn reachable_mask(&self) -> u32 {
        let threshold = self.reachable_threshold();
        let mut mask = 0u32;
        for team in 0..PLAYER_TEAM_COUNT {
            if !self.occupied[team] || team == self.local_team.0 as usize {
                continue;
            }
            if self.eliminated[team] {
                continue;
            }
            let silence = self.tick.saturating_sub(self.last_seen[team]);
            if silence < threshold {
                mask |= 1 << team;
            }
        }
        mask
    }

    /// Number of remote teams currently reachable.
    pub fn reachable_peers(&self) -> usize {
        self.reachable_mask().count_ones() as usize
    }

    /// `reachable_peers + 1 >= quorum` (`PLAN-host-mode.md` §4).
    pub fn quorum_allows(&self) -> bool {
        self.reachable_peers() + 1 >= self.quorum
    }

    /// Advances the numbered session tick and returns the teams that must be
    /// eliminated at this tick.
    ///
    /// The quorum is checked against the reachable set **before** the
    /// elimination, so a two-player session (`quorum = 2`) never eliminates
    /// its only peer: `reachable = 0 ⇒ 0 + 1 < 2`. The decision itself is the
    /// shared pure rule [`deterministic_eliminations`], so the authority and
    /// its guests cannot disagree.
    pub fn advance(&mut self) -> Vec<TeamEliminated> {
        self.tick = self.tick.wrapping_add(1);
        let rule = HostHeartbeat {
            host_epoch: self.epoch,
            session_tick: self.tick,
            turn_counter: 0,
            phase: 0,
            reachable: self.reachable_mask(),
            last_seen: self.last_seen,
            policy: policy_to_wire(self.policy),
            host_timeout_ms: 0,
            quorum: self.quorum.min(u8::MAX as usize) as u8,
            eliminated: 0,
            eliminated_tick: [0; PLAYER_TEAM_COUNT],
        };
        let eliminations = deterministic_eliminations(&rule, self.policy, self.local_team);
        let mut fresh = Vec::with_capacity(eliminations.len());
        for elimination in eliminations {
            let slot = elimination.team.0 as usize;
            if self.eliminated[slot] {
                continue;
            }
            self.eliminated[slot] = true;
            self.eliminated_tick[slot] = elimination.tick;
            fresh.push(elimination);
        }
        fresh
    }

    /// Forced (authority) elimination: the departed host cannot come back, so
    /// the new epoch removes its team at tick 1 — independent of the policy and
    /// only after the takeover reached quorum.
    pub fn force_eliminate(&mut self, team: TeamId) -> Option<TeamEliminated> {
        let slot = team.0 as usize;
        if slot >= PLAYER_TEAM_COUNT || !self.occupied[slot] || self.eliminated[slot] {
            return None;
        }
        self.eliminated[slot] = true;
        self.eliminated_tick[slot] = 1;
        Some(TeamEliminated { team, tick: 1 })
    }

    /// Bitmask of eliminated team slots.
    pub fn eliminated_mask(&self) -> u32 {
        let mut mask = 0u32;
        for (team, eliminated) in self.eliminated.iter().enumerate() {
            if *eliminated {
                mask |= 1 << team;
            }
        }
        mask
    }

    /// Builds the heartbeat for the current tick and lockstep position.
    pub fn heartbeat(&self, position: LockstepPosition) -> HostHeartbeat {
        HostHeartbeat {
            host_epoch: self.epoch,
            session_tick: self.tick,
            turn_counter: position.turn_counter,
            phase: position.phase,
            reachable: self.reachable_mask(),
            last_seen: self.last_seen,
            policy: policy_to_wire(self.policy),
            host_timeout_ms: self.host_timeout.as_millis().min(u128::from(u32::MAX)) as u32,
            quorum: self.quorum.min(u8::MAX as usize) as u8,
            eliminated: self.eliminated_mask(),
            eliminated_tick: self.eliminated_tick,
        }
    }
}

/// Client view of the session authority: applies heartbeat decisions once.
///
/// The view trusts the host's `eliminated`/`eliminated_tick` fields but rejects
/// a syntactically impossible quorum; [`deterministic_eliminations`] is the
/// pure tick rule the determinism gate uses to show that drift cannot move the
/// elimination tick.
#[derive(Debug, Clone)]
pub(crate) struct GuestSessionView {
    /// Known authority epoch.
    pub epoch: u32,
    /// Effective policy advertised by the host.
    pub policy: SessionPolicy,
    /// Effective host timeout advertised by the host.
    pub host_timeout: Duration,
    /// Eliminated teams already reported to the simulation.
    applied: [bool; PLAYER_TEAM_COUNT],
}

impl Default for GuestSessionView {
    fn default() -> Self {
        Self {
            epoch: 1,
            policy: SessionPolicy::Pause,
            host_timeout: DEFAULT_HOST_TIMEOUT,
            applied: [false; PLAYER_TEAM_COUNT],
        }
    }
}

impl GuestSessionView {
    /// Applies one heartbeat and returns the eliminations that must reach the
    /// simulation exactly once.
    pub fn apply(&mut self, heartbeat: &HostHeartbeat) -> Vec<TeamEliminated> {
        // A stale authority (a minority-side host after a merge) is ignored:
        // the higher epoch wins (`PLAN-host-mode.md` §4).
        if heartbeat.host_epoch < self.epoch {
            return Vec::new();
        }
        self.epoch = heartbeat.host_epoch;
        self.policy = policy_from_wire(heartbeat.policy);
        let timeout = Duration::from_millis(u64::from(heartbeat.host_timeout_ms));
        if validate_host_timeout(timeout).is_ok() {
            self.host_timeout = timeout;
        }

        let mut eliminations = Vec::new();
        // The authority may only eliminate with a valid quorum; a malformed
        // claim is ignored instead of desyncing the clients.
        if heartbeat.quorum == 0 || usize::from(heartbeat.quorum) > self.quorum_bound() {
            return eliminations;
        }
        for team in 0..PLAYER_TEAM_COUNT {
            if self.applied[team] || heartbeat.eliminated & (1 << team) == 0 {
                continue;
            }
            let tick = heartbeat.eliminated_tick[team];
            if tick == 0 {
                continue;
            }
            self.applied[team] = true;
            eliminations.push(TeamEliminated {
                team: TeamId(team as u8),
                tick,
            });
        }
        eliminations
    }

    /// Upper bound of a plausible quorum (`PLAYER_TEAM_COUNT`).
    const fn quorum_bound(&self) -> usize {
        PLAYER_TEAM_COUNT
    }
}

/// Pure deterministic elimination rule (`ticks_without_heartbeat`).
///
/// Given the same heartbeat, two clients compute exactly the same
/// `(team, tick)` pairs regardless of when the heartbeat arrived: the tick is
/// `last_seen[team] + ticks`, not the local clock.
pub(crate) fn deterministic_eliminations(
    heartbeat: &HostHeartbeat,
    policy: SessionPolicy,
    local_team: TeamId,
) -> Vec<TeamEliminated> {
    let SessionPolicy::EliminateAfter { ticks } = policy else {
        return Vec::new();
    };
    let ticks = ticks.max(1);
    if heartbeat.quorum == 0 || (heartbeat.reachable.count_ones() + 1) < u32::from(heartbeat.quorum)
    {
        return Vec::new();
    }

    let mut eliminations = Vec::new();
    for team in 0..PLAYER_TEAM_COUNT {
        if team == local_team.0 as usize {
            continue;
        }
        let last_seen = heartbeat.last_seen[team];
        if last_seen == 0 {
            continue; // an unused slot never has a team to eliminate
        }
        let eliminated_at = last_seen.saturating_add(ticks);
        if heartbeat.session_tick >= eliminated_at {
            eliminations.push(TeamEliminated {
                team: TeamId(team as u8),
                tick: eliminated_at,
            });
        }
    }
    eliminations
}

/// Initial per-team `last_seen`: `1` for occupied slots, `0` for empty ones
/// (the elimination rule treats `0` as "no team").
fn fresh_last_seen(occupied: &[bool; PLAYER_TEAM_COUNT]) -> [u32; PLAYER_TEAM_COUNT] {
    std::array::from_fn(|team| u32::from(occupied[team]))
}

/// Role-specific state of [`P2pTransport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum P2pMode {
    /// Acting as the session authority (the initial host or a completed
    /// successor).
    Host,
    /// Ordinary guest of a live authority.
    Guest,
    /// The authority went silent; deciding between takeover and reconnect.
    HostFailure,
    /// The candidate bound the server and is collecting `Accept` frames.
    SuccessorCollecting,
    /// Reconnected; waiting for the proposal (or a heartbeat of the old
    /// authority, which means a healed split-brain).
    AwaitingTakeover,
    /// The survivor refused the takeover (position mismatch) or lost quorum;
    /// the match stays frozen.
    Paused,
}

/// Transport of one P2P participant: a guest session or a host server, plus the
/// H4 migration/quorum state machine (`PLAN-host-mode.md` §5 H4).
pub(crate) struct P2pTransport {
    mode: P2pMode,
    guest: Option<SessionIo>,
    host: Option<HostHandle>,
    /// Endpoint the guests (and a reconnecting survivor) connect to.
    endpoint: DirectEndpoint,
    /// Session config of the guest link (reconnect timeouts/hashes).
    session_config: SessionConfig,
    /// Reconnect policy of the guest link.
    reconnect: ReconnectPolicy,
    /// Registered players and the deterministic election.
    roster: P2pRoster,
    /// Own lockstep position (updated by the driver via `note_lockstep`).
    position: LockstepPosition,
    /// Current authority epoch.
    epoch: u32,
    /// Session policy (host: configured; guest: advertised).
    policy: SessionPolicy,
    /// `host_timeout` (host: advertised; guest: adopted from the heartbeat).
    host_timeout: Duration,
    /// Heartbeat period (host publishing cadence).
    heartbeat_period: Duration,
    /// Host session clock (used once the transport becomes an authority).
    clock: SessionClock,
    /// Heartbeat/elimination view of the current authority (guest side).
    view: GuestSessionView,
    /// Last heartbeat received from an authority (guest side).
    last_heartbeat: Instant,
    /// Last heartbeat published (host side).
    last_publish: Instant,
    /// Last bind/reconnect attempt (migration throttle).
    last_attempt: Instant,
    /// Reconnect attempts of the current failure episode.
    attempts: u32,
    /// Accepted survivor nodes of the candidate.
    accepted: Vec<u16>,
    /// The candidate was rejected: stay paused.
    rejected: bool,
    /// The survivor accepted a proposal (before `Resume`).
    accepted_takeover: bool,
    /// This failure episode changed the authority epoch (a real takeover, not
    /// a healed relay break).
    takeover_completed: bool,
    /// The transport resumed and the driver must publish resume-54.
    resume_pending: bool,
    /// Migration window start.
    failure_since: Instant,
    /// Pending events produced by the transport (eliminations, etc.).
    pending: VecDeque<IoEvent>,
    /// Test-only relay breaker: while set, the host neither publishes
    /// heartbeats nor reads guest packets (split-brain injection). Outgoing
    /// game frames are held in `blackout_out` and released when the relay
    /// heals — a break delays the lockstep traffic, it does not destroy it.
    #[cfg(test)]
    blackout: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Test-only frames produced during a `blackout` (delivered on heal).
    #[cfg(test)]
    blackout_out: Vec<Packet>,
    /// Test-only link breaker: while set, the guest drops its ENet session and
    /// takes the ordinary reconnect path (H5 standalone reconnect).
    #[cfg(test)]
    link_drop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl P2pTransport {
    /// Host side of a match that already completed the lobby (`run_host`).
    ///
    /// `heartbeat_period` is the publishing cadence of the numbered
    /// heartbeats; the ENet transport itself must be configured with
    /// `heartbeat: Duration::ZERO` so that only numbered frames are sent.
    pub fn host(
        handle: HostHandle,
        roster: P2pRoster,
        local_team: TeamId,
        policy: SessionPolicy,
        host_timeout: Duration,
        heartbeat_period: Duration,
    ) -> Self {
        let clock = SessionClock::new(&roster, local_team, policy, host_timeout, heartbeat_period);
        let now = Instant::now();
        Self {
            mode: P2pMode::Host,
            guest: None,
            host: Some(handle),
            endpoint: DirectEndpoint {
                host: Ipv4Addr::LOCALHOST.to_string(),
                port: 0,
            },
            session_config: SessionConfig::new(
                Ipv4Addr::LOCALHOST.to_string(),
                0,
                "",
                max_net::DataHash {
                    max_res: None,
                    patches_res: None,
                },
            ),
            reconnect: ReconnectPolicy {
                attempts: 0,
                ..ReconnectPolicy::default()
            },
            roster,
            position: LockstepPosition::default(),
            epoch: 1,
            policy,
            host_timeout,
            heartbeat_period: heartbeat_period.max(Duration::from_millis(1)),
            clock,
            view: GuestSessionView::default(),
            last_heartbeat: now,
            last_publish: now,
            last_attempt: now,
            attempts: 0,
            accepted: Vec::new(),
            rejected: false,
            accepted_takeover: false,
            takeover_completed: false,
            resume_pending: false,
            failure_since: now,
            pending: VecDeque::new(),
            #[cfg(test)]
            blackout: None,
            #[cfg(test)]
            blackout_out: Vec::new(),
            #[cfg(test)]
            link_drop: None,
        }
    }

    /// Guest side of a direct P2P session (after its lobby).
    #[allow(clippy::too_many_arguments)]
    pub fn guest(
        io: SessionIo,
        session_config: SessionConfig,
        endpoint: DirectEndpoint,
        roster: P2pRoster,
        local_team: TeamId,
        policy: SessionPolicy,
        host_timeout: Duration,
        reconnect: ReconnectPolicy,
    ) -> Self {
        Self::with_link(
            Some(io),
            session_config,
            endpoint,
            roster,
            local_team,
            policy,
            host_timeout,
            reconnect,
        )
    }

    /// Common constructor of the guest role (also used by the unit tests with
    /// `io = None`).
    #[allow(clippy::too_many_arguments)]
    fn with_link(
        io: Option<SessionIo>,
        session_config: SessionConfig,
        endpoint: DirectEndpoint,
        roster: P2pRoster,
        local_team: TeamId,
        policy: SessionPolicy,
        host_timeout: Duration,
        reconnect: ReconnectPolicy,
    ) -> Self {
        let clock = SessionClock::new(
            &roster,
            local_team,
            policy,
            host_timeout,
            DEFAULT_HEARTBEAT_PERIOD,
        );
        let now = Instant::now();
        Self {
            mode: P2pMode::Guest,
            guest: io,
            host: None,
            endpoint,
            session_config,
            reconnect,
            roster,
            position: LockstepPosition::default(),
            epoch: 1,
            policy,
            host_timeout,
            heartbeat_period: DEFAULT_HEARTBEAT_PERIOD,
            clock,
            view: GuestSessionView::default(),
            last_heartbeat: now,
            last_publish: now,
            last_attempt: now,
            attempts: 0,
            accepted: Vec::new(),
            rejected: false,
            accepted_takeover: false,
            takeover_completed: false,
            resume_pending: false,
            failure_since: now,
            pending: VecDeque::new(),
            #[cfg(test)]
            blackout: None,
            #[cfg(test)]
            blackout_out: Vec::new(),
            #[cfg(test)]
            link_drop: None,
        }
    }

    /// Current epoch (diagnostics/tests).
    #[cfg(test)]
    pub const fn epoch(&self) -> u32 {
        self.epoch
    }

    /// Queues an event for the driver.
    fn push(&mut self, event: IoEvent) {
        self.pending.push_back(event);
    }

    /// Pops a queued event.
    fn pop(&mut self) -> Option<IoEvent> {
        self.pending.pop_front()
    }

    /// Hands the current lockstep position to the transport.
    pub fn note_lockstep(&mut self, position: LockstepPosition) {
        self.position = position;
    }

    /// Whether the test-only relay breaker is active.
    fn blacked_out(&self) -> bool {
        #[cfg(test)]
        if let Some(flag) = &self.blackout {
            return flag.load(std::sync::atomic::Ordering::Relaxed);
        }
        false
    }

    /// Sends one frame through the active role (no blackout gate).
    async fn send_to_peers(&mut self, packet: Packet) -> Result<()> {
        match self.mode {
            P2pMode::Host => {
                if let Some(host) = self.host.as_ref() {
                    host.send(Recipient::Broadcast, packet)
                        .await
                        .context("p2p: broadcast пакета")?;
                }
            }
            P2pMode::Guest => {
                if let Some(guest) = self.guest.as_mut() {
                    guest
                        .send_packet_untracked(packet)
                        .await
                        .context("p2p: отправка пакета")?;
                }
            }
            // Frozen: nothing may reach a diverged authority.
            P2pMode::HostFailure
            | P2pMode::SuccessorCollecting
            | P2pMode::AwaitingTakeover
            | P2pMode::Paused => {}
        }
        Ok(())
    }

    /// Releases the frames held during a test-only blackout (relay heal).
    #[cfg(test)]
    async fn flush_blackout(&mut self) -> Result<()> {
        if self.blacked_out() || self.blackout_out.is_empty() {
            return Ok(());
        }
        let held = std::mem::take(&mut self.blackout_out);
        tracing::debug!(
            "p2p: раскол устранён — отправка {} отложенных пакетов",
            held.len()
        );
        for packet in held {
            self.send_to_peers(packet).await?;
        }
        Ok(())
    }

    /// Installs the test-only split-brain breaker (host side).
    #[cfg(test)]
    pub fn set_blackout(&mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.blackout = Some(flag);
    }

    /// Installs the test-only guest-link breaker (H5 standalone reconnect).
    #[cfg(test)]
    pub fn set_link_drop(&mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.link_drop = Some(flag);
    }

    /// Drops the guest ENet link if the test breaker fired.
    #[cfg(test)]
    fn take_link_drop(&mut self) -> bool {
        let Some(flag) = self.link_drop.as_ref() else {
            return false;
        };
        if !flag.swap(false, std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        if let Some(guest) = self.guest.take() {
            guest.abort();
        }
        true
    }

    /// Whether the host may close the current barrier
    /// (`reachable_peers + 1 >= quorum`, `PLAN-host-mode.md` §4).
    pub fn quorum_allows_barrier(&self) -> bool {
        match self.mode {
            P2pMode::Host => self.clock.quorum_allows(),
            P2pMode::SuccessorCollecting | P2pMode::Paused => false,
            _ => true,
        }
    }

    // -- host side -----------------------------------------------------//

    /// Publishes one numbered heartbeat and queues the eliminations decided at
    /// this tick.
    async fn publish_heartbeat(&mut self) -> Result<()> {
        let eliminations = self.clock.advance();
        for elimination in &eliminations {
            tracing::warn!(
                "p2p: команда {} не подаёт признаков {} тиков — устранена (tick {})",
                elimination.team.0,
                match self.policy {
                    SessionPolicy::EliminateAfter { ticks } => ticks,
                    SessionPolicy::Pause => 0,
                },
                elimination.tick
            );
            self.push(IoEvent::TeamEliminated(*elimination));
        }
        let heartbeat = self.clock.heartbeat(self.position);
        if let Some(host) = self.host.as_ref() {
            host.publish_heartbeat(heartbeat)
                .await
                .context("p2p: публикация heartbeat 55")?;
        }
        self.last_publish = Instant::now();
        Ok(())
    }

    /// One host transport step.
    async fn host_step(&mut self, wait: Duration) -> Result<()> {
        if self.blacked_out() {
            // Split-brain injection: the relay is down, nothing is published
            // and no guest packet is read (the socket buffers them).
            return Ok(());
        }
        if self.last_publish.elapsed() >= self.heartbeat_period {
            self.publish_heartbeat().await?;
            return Ok(());
        }
        let Some(host) = self.host.as_mut() else {
            self.mode = P2pMode::Paused;
            return Ok(());
        };
        let wait = wait
            .min(
                self.heartbeat_period
                    .saturating_sub(self.last_publish.elapsed()),
            )
            .clamp(Duration::from_millis(1), STEP_MAX_WAIT);
        match host.next_event_timeout(wait).await {
            Ok(Some(event)) => self.on_host_event(event).await?,
            Ok(None) => {
                self.push(IoEvent::Closed);
                self.mode = P2pMode::Paused;
            }
            Err(HostError::Timeout { .. }) => {}
            Err(error) => return Err(error).context("p2p: ожидание события хоста"),
        }
        Ok(())
    }

    /// One candidate step: only the takeover exchange, the clock is not
    /// running yet.
    async fn successor_step(&mut self, wait: Duration) -> Result<()> {
        if self.blacked_out() {
            return Ok(());
        }
        if self.failure_since.elapsed() >= self.host_timeout + MIGRATION_WINDOW {
            tracing::error!(
                "p2p: takeover epoch {} не собрал кворум {}/{} — пауза",
                self.epoch,
                self.accepted.len() + 1,
                self.roster.quorum()
            );
            self.mode = P2pMode::Paused;
            self.push(IoEvent::Closed);
            return Ok(());
        }
        let Some(host) = self.host.as_mut() else {
            self.mode = P2pMode::Paused;
            return Ok(());
        };
        let wait = wait.clamp(Duration::from_millis(1), STEP_MAX_WAIT);
        match host.next_event_timeout(wait).await {
            Ok(Some(event)) => self.on_host_event(event).await?,
            Ok(None) => {
                self.push(IoEvent::Closed);
                self.mode = P2pMode::Paused;
            }
            Err(HostError::Timeout { .. }) => {}
            Err(error) => return Err(error).context("p2p: ожидание события кандидата"),
        }
        Ok(())
    }

    /// Handles one event of the host/candidate transport.
    async fn on_host_event(&mut self, event: HostEvent) -> Result<()> {
        match event.kind {
            HostEventKind::Packet(packet) => {
                if self.mode == P2pMode::SuccessorCollecting {
                    self.on_takeover_packet(&packet).await?;
                    return Ok(());
                }
                // Liveness: any packet of a team is its heartbeat
                // (`ticks_without_heartbeat` counts silent teams).
                if let Some(team) = max_net::payload_team(&packet) {
                    let slot = team.0 as usize;
                    if slot < PLAYER_TEAM_COUNT && self.clock.occupied[slot] {
                        self.clock.heard(slot);
                    }
                }
                self.push(IoEvent::Packet(packet));
            }
            HostEventKind::Verified => {
                if self.mode == P2pMode::SuccessorCollecting {
                    self.send_proposal().await?;
                    self.push(IoEvent::Control);
                } else if self.mode == P2pMode::Host {
                    // H5: a guest (re)connected to the live authority. The driver
                    // re-publishes the last closed and the pending barrier so the
                    // guest can catch up without waiting for the 3 s retransmit.
                    self.push(IoEvent::PeerReconnected);
                } else {
                    self.push(IoEvent::Control);
                }
            }
            HostEventKind::Connected => self.push(IoEvent::Control),
            HostEventKind::Disconnected => {
                if self.mode == P2pMode::SuccessorCollecting {
                    // A survivor that is not needed for the quorum may leave;
                    // the candidate keeps collecting.
                    self.push(IoEvent::Control);
                } else {
                    self.push(IoEvent::Disconnected);
                }
            }
        }
        Ok(())
    }

    // -- successor side ------------------------------------------------//

    /// Liveness probe of the current authority: a `TP 00` exchange on a fresh
    /// temporary session.
    ///
    /// A relay break does not stop the ENet server from answering the transport
    /// handshake, so a live host is detected even when its heartbeats cannot
    /// reach the survivors. In that case the candidate must not bind a second
    /// authority: both sides stay at the barrier until the relay heals
    /// (`PLAN-host-mode.md` §4, split-brain).
    async fn old_host_is_alive(&self) -> bool {
        let mut config = self.session_config.clone();
        config.connect_timeout = MIGRATION_PROBE_TIMEOUT;
        config.handshake_timeout = MIGRATION_PROBE_TIMEOUT;
        Session::connect(config).await.is_ok()
    }

    /// Binds the session port as the new authority and starts collecting
    /// `Accept` frames. `authority_alive` is the result of the `TP 00` liveness
    /// probe; when the old authority answers, the candidate must not bind a
    /// second one (`PLAN-host-mode.md` §4, split-brain). `Ok(false)` means the
    /// port is still held: the caller stays paused/reconnects.
    async fn try_start_takeover(&mut self, authority_alive: bool) -> Result<bool> {
        if authority_alive {
            tracing::debug!(
                "p2p: старый хост {}:{} отвечает на TP 00 — takeover отменён (пауза)",
                self.endpoint.host,
                self.endpoint.port
            );
            return Ok(false);
        }
        let Some(bind_ip) = resolve_bind_ip(&self.endpoint.host) else {
            return Ok(false);
        };
        let config = HostConfig {
            bind_ip,
            port: self.endpoint.port,
            game_version: self.session_config.game_version,
            data_hash: self.session_config.data_hash,
            max_peers: DEFAULT_MAX_PEERS,
            heartbeat: Duration::ZERO,
            host_timeout: self.host_timeout,
        };
        match HostHandle::bind(config) {
            Ok(handle) => {
                self.host = Some(handle);
                self.accepted.clear();
                self.rejected = false;
                self.accepted_takeover = false;
                self.epoch = self.epoch.saturating_add(1);
                self.clock.reset_for_epoch(self.epoch);
                self.takeover_completed = true;
                self.mode = P2pMode::SuccessorCollecting;
                tracing::warn!(
                    "p2p: хост молчит; узел {} поднимает сервер на {}:{} (epoch {})",
                    self.roster.local_node,
                    bind_ip,
                    self.endpoint.port,
                    self.epoch
                );
                // The previous guest link is dropped: the survivors reconnect
                // to the new authority (`TP 00` without a new lobby).
                self.guest = None;
                self.send_proposal().await?;
                Ok(true)
            }
            Err(error) => {
                tracing::debug!(
                    "p2p: порт {}:{} занят ({error}); раскол не подтверждён — пауза",
                    bind_ip,
                    self.endpoint.port
                );
                Ok(false)
            }
        }
    }

    /// Sends the proposal to every verified survivor.
    async fn send_proposal(&mut self) -> Result<()> {
        let proposal = Takeover {
            kind: TakeoverKind::Proposal,
            epoch: self.epoch,
            turn_counter: self.position.turn_counter,
            phase: self.position.phase,
            counters: self.position.counters(),
            node: self.roster.local_node,
            roster: self.roster.takeover_roster(),
        };
        if let Some(host) = self.host.as_ref() {
            host.send_frame(Recipient::Broadcast, proposal.encode())
                .await
                .context("p2p: отправка Takeover 56")?;
        }
        Ok(())
    }

    /// Handles a takeover frame addressed to the candidate.
    async fn on_takeover_packet(&mut self, packet: &Packet) -> Result<()> {
        let Packet::Unknown { kind, payload } = packet else {
            return Ok(());
        };
        if *kind != max_net::HOST_TAKEOVER_PACKET_NUMBER {
            return Ok(());
        }
        let Ok(takeover) = Takeover::decode_payload(payload) else {
            return Ok(());
        };
        match takeover.kind {
            TakeoverKind::Accept => {
                if takeover.epoch != self.epoch || self.roster.team_slot(takeover.node).is_none() {
                    return Ok(());
                }
                if self.position.matches(&takeover) {
                    if !self.accepted.contains(&takeover.node) {
                        self.accepted.push(takeover.node);
                    }
                } else {
                    tracing::error!(
                        "p2p: узел {} на другой lockstep-позиции — takeover отклонён",
                        takeover.node
                    );
                    self.rejected = true;
                    self.mode = P2pMode::Paused;
                }
                self.maybe_finish_takeover().await?;
            }
            TakeoverKind::Reject => {
                tracing::error!("p2p: узел {} отверг takeover — пауза", takeover.node);
                self.rejected = true;
                self.mode = P2pMode::Paused;
            }
            TakeoverKind::Proposal | TakeoverKind::Resume => {}
        }
        Ok(())
    }

    /// Completes the takeover once the quorum of survivors accepted.
    async fn maybe_finish_takeover(&mut self) -> Result<()> {
        if self.mode != P2pMode::SuccessorCollecting || self.rejected {
            return Ok(());
        }
        // The candidate itself counts as one reachable player.
        if self.accepted.len() + 1 < self.roster.quorum() {
            tracing::info!(
                "p2p: takeover epoch {} ждёт кворум ({}/{})",
                self.epoch,
                self.accepted.len() + 1,
                self.roster.quorum()
            );
            return Ok(());
        }

        // The departed host cannot come back: its team is removed with the
        // authority's quorum, independent of the session policy.
        if let Some(host_team) = self
            .roster
            .team_slot(self.roster.host_node)
            .map(|slot| TeamId(slot as u8))
        {
            if let Some(elimination) = self.clock.force_eliminate(host_team) {
                tracing::warn!(
                    "p2p: команда {} устранена (узел {} покинул сессию, tick {})",
                    elimination.team.0,
                    self.roster.host_node,
                    elimination.tick
                );
                self.push(IoEvent::TeamEliminated(elimination));
            }
        }

        self.mode = P2pMode::Host;
        self.last_publish = Instant::now();
        self.resume_pending = true;
        self.publish_heartbeat().await?;

        let resume = Takeover {
            kind: TakeoverKind::Resume,
            epoch: self.epoch,
            ..Takeover::default()
        };
        if let Some(host) = self.host.as_ref() {
            host.send_frame(Recipient::Broadcast, resume.encode())
                .await
                .context("p2p: отправка Takeover::Resume")?;
        }
        tracing::warn!(
            "p2p: takeover завершён — epoch {}, кворум {}/{}; матч продолжен",
            self.epoch,
            self.accepted.len() + 1,
            self.roster.quorum()
        );
        Ok(())
    }

    // -- guest side ----------------------------------------------------//

    /// One guest transport step.
    async fn guest_step(&mut self, wait: Duration) -> Result<()> {
        // Detection is wall-clock based and must still run while the relay is
        // broken; only the actual I/O of the degraded link is skipped.
        if self.last_heartbeat.elapsed() >= self.host_timeout {
            self.enter_failure()?;
            self.push(IoEvent::HostTimeout);
            return Ok(());
        }
        if self.blacked_out() {
            return Ok(());
        }
        #[cfg(test)]
        if self.take_link_drop() {
            // H5 test hook: a lost link while the authority is alive.
            self.enter_failure()?;
            self.push(IoEvent::Disconnected);
            return Ok(());
        }
        let Some(guest) = self.guest.as_mut() else {
            self.enter_failure()?;
            self.push(IoEvent::Disconnected);
            return Ok(());
        };
        let wait = wait.clamp(Duration::from_millis(1), STEP_MAX_WAIT);
        match guest.next_io_event(wait).await? {
            IoEvent::Packet(packet) => self.on_guest_packet(packet).await?,
            IoEvent::Idle => {}
            IoEvent::Closed | IoEvent::Disconnected | IoEvent::HostTimeout => {
                self.enter_failure()?;
                self.push(IoEvent::Disconnected);
            }
            event => self.push(event),
        }
        Ok(())
    }

    /// Handles one packet received by a guest.
    async fn on_guest_packet(&mut self, packet: Packet) -> Result<()> {
        match packet {
            Packet::Unknown { kind, payload } if kind == max_net::HOST_HEARTBEAT_PACKET_NUMBER => {
                if let Ok(heartbeat) = HostHeartbeat::decode_payload(&payload) {
                    self.on_heartbeat(heartbeat);
                }
            }
            Packet::Unknown { kind, payload } if kind == max_net::HOST_TAKEOVER_PACKET_NUMBER => {
                if let Ok(takeover) = Takeover::decode_payload(&payload) {
                    self.on_takeover_frame(takeover).await?;
                }
            }
            other => self.push(IoEvent::Packet(other)),
        }
        Ok(())
    }

    /// Applies one host heartbeat (or detects a healed split-brain).
    fn on_heartbeat(&mut self, heartbeat: HostHeartbeat) {
        if heartbeat.host_epoch < self.epoch {
            // A lower epoch is a minority authority: ignore and stay frozen.
            tracing::warn!(
                "p2p: heartbeat устаревшего epoch {} (у нас {}) — игнорирован",
                heartbeat.host_epoch,
                self.epoch
            );
            return;
        }
        // A higher epoch without a validated proposal is not trusted: the
        // candidate sends its proposal before it starts heartbeating.
        if heartbeat.host_epoch > self.epoch && !self.accepted_takeover {
            tracing::warn!(
                "p2p: heartbeat epoch {} без валидированного takeover — игнорирован",
                heartbeat.host_epoch
            );
            return;
        }

        self.last_heartbeat = Instant::now();
        self.epoch = heartbeat.host_epoch;
        self.policy = policy_from_wire(heartbeat.policy);
        let timeout = Duration::from_millis(u64::from(heartbeat.host_timeout_ms));
        if validate_host_timeout(timeout).is_ok() {
            self.host_timeout = timeout;
        }

        // A heartbeat of the *known* authority during a failure means the
        // relay is healed: cancel the pending takeover and resume as a guest.
        if matches!(self.mode, P2pMode::HostFailure | P2pMode::AwaitingTakeover)
            && !self.accepted_takeover
        {
            self.attempts = 0;
            self.mode = P2pMode::Guest;
            self.resume_pending = true;
            tracing::warn!(
                "p2p: heartbeat epoch {} снова получен — раскол устранён, пауза снята",
                heartbeat.host_epoch
            );
        }

        let eliminations = self.view.apply(&heartbeat);
        self.push(IoEvent::Heartbeat {
            heartbeat,
            eliminated: eliminations,
        });
    }

    /// Handles the candidate's proposal / resume.
    async fn on_takeover_frame(&mut self, takeover: Takeover) -> Result<()> {
        match takeover.kind {
            TakeoverKind::Proposal => {
                if matches!(self.mode, P2pMode::SuccessorCollecting | P2pMode::Paused) {
                    return Ok(());
                }
                let expected = self.roster.successor();
                if expected != Some(takeover.node) {
                    tracing::error!(
                        "p2p: proposal от узла {} (ожидался {:?}) — отклонён",
                        takeover.node,
                        expected
                    );
                    self.reject_takeover(&takeover).await?;
                    return Ok(());
                }
                if takeover.epoch <= self.epoch
                    || !self.roster.roster_matches(&takeover.roster)
                    || !self.position.matches(&takeover)
                {
                    tracing::error!(
                        "p2p: takeover epoch {} не совпал с позицией/ростером — отказ + пауза",
                        takeover.epoch
                    );
                    self.reject_takeover(&takeover).await?;
                    return Ok(());
                }

                self.epoch = takeover.epoch;
                self.accepted_takeover = true;
                self.takeover_completed = true;
                self.mode = P2pMode::AwaitingTakeover;
                self.send_ack(TakeoverKind::Accept).await?;
                tracing::warn!(
                    "p2p: takeover epoch {} принят (позиция turn={} phase={})",
                    takeover.epoch,
                    takeover.turn_counter,
                    takeover.phase
                );
            }
            TakeoverKind::Resume => {
                if takeover.epoch == self.epoch && self.accepted_takeover {
                    self.mode = P2pMode::Guest;
                    self.resume_pending = true;
                    self.last_heartbeat = Instant::now();
                    tracing::warn!("p2p: Resume epoch {} — матч продолжен", takeover.epoch);
                }
            }
            TakeoverKind::Accept | TakeoverKind::Reject => {}
        }
        Ok(())
    }

    /// Sends a rejection and freezes forever (position mismatch = STOP).
    async fn reject_takeover(&mut self, takeover: &Takeover) -> Result<()> {
        self.rejected = true;
        self.send_ack(TakeoverKind::Reject).await?;
        self.mode = P2pMode::Paused;
        let _ = takeover;
        Ok(())
    }

    /// Sends `Accept`/`Reject` with this client's own position.
    async fn send_ack(&mut self, kind: TakeoverKind) -> Result<()> {
        let frame = Takeover {
            kind,
            epoch: self.epoch,
            turn_counter: self.position.turn_counter,
            phase: self.position.phase,
            counters: self.position.counters(),
            node: self.roster.local_node,
            roster: Vec::new(),
        }
        .encode();
        if let Some(guest) = self.guest.as_mut() {
            guest
                .send_frame(frame)
                .await
                .context("p2p: отправка Takeover 56")?;
        }
        Ok(())
    }

    // -- failure handling ----------------------------------------------//

    /// Starts (or continues) the host-failure episode.
    fn enter_failure(&mut self) -> Result<()> {
        if matches!(
            self.mode,
            P2pMode::HostFailure | P2pMode::Host | P2pMode::Paused | P2pMode::SuccessorCollecting
        ) {
            return Ok(());
        }
        tracing::warn!(
            "p2p: хост молчит дольше {:?}; узел {} — {}",
            self.host_timeout,
            self.roster.local_node,
            if self.roster.is_successor() {
                "кандидат на takeover"
            } else {
                "ждёт нового хоста"
            }
        );
        self.mode = P2pMode::HostFailure;
        self.failure_since = Instant::now();
        self.attempts = 0;
        self.last_attempt = Instant::now() - MIGRATION_RETRY;
        self.resume_pending = false;
        self.takeover_completed = false;
        Ok(())
    }

    /// Reconnects the guest link to the session endpoint.
    ///
    /// This is the ordinary H5 reconnect (no lobby, no Horizon): the lockstep
    /// point is kept, and once a heartbeat of the live authority arrives the
    /// transport reports `Restored`, so the driver publishes its resume-54 and
    /// the barriers continue (`protocol.md` §9.1). Returns whether a new `TP 00`
    /// session was established.
    async fn reconnect_guest(&mut self) -> bool {
        self.attempts = self.attempts.saturating_add(1);
        let mut config = self.session_config.clone();
        config.connect_timeout = MIGRATION_CONNECT_TIMEOUT;
        config.handshake_timeout = MIGRATION_CONNECT_TIMEOUT;
        match Session::connect(config).await {
            Ok(session) => {
                let handle = session.start();
                self.guest = Some(SessionIo::new(
                    handle,
                    self.session_config.clone(),
                    self.reconnect,
                ));
                // The new link is fresh: do not immediately time it out with
                // the age of the dead authority's heartbeat.
                self.last_heartbeat = Instant::now();
                self.accepted_takeover = false;
                self.mode = P2pMode::AwaitingTakeover;
                tracing::info!(
                    "p2p: узел {} переподключён к {}:{} (попыток {})",
                    self.roster.local_node,
                    self.endpoint.host,
                    self.endpoint.port,
                    self.attempts
                );
                true
            }
            Err(error) => {
                tracing::debug!("p2p: переподключение не удалось: {error}");
                false
            }
        }
    }

    /// Polls the still-open guest link during a failure episode.
    ///
    /// A heartbeat of the (possibly healed) authority resumes the session. Any
    /// other packet is **preserved** in the pending queue: during a relay break
    /// the link is up but silent, and once it heals ENet delivers exactly the
    /// barrier packets the client missed, so dropping them here would deadlock
    /// the lockstep (`PLAN-host-mode.md` §4, split-brain). Returns `true` when a
    /// heartbeat was applied.
    async fn poll_live_link(&mut self) -> Result<bool> {
        let Some(guest) = self.guest.as_mut() else {
            return Ok(false);
        };
        let event = guest.next_io_event(Duration::from_millis(5)).await?;
        match event {
            IoEvent::Packet(Packet::Unknown { kind, payload })
                if kind == max_net::HOST_HEARTBEAT_PACKET_NUMBER =>
            {
                if let Ok(heartbeat) = HostHeartbeat::decode_payload(&payload) {
                    self.on_heartbeat(heartbeat);
                    return Ok(true);
                }
            }
            IoEvent::Closed | IoEvent::Disconnected => {
                self.guest = None;
            }
            IoEvent::Idle => {}
            event => self.push(event),
        }
        Ok(false)
    }

    /// One step of the failure episode.
    async fn failure_step(&mut self, _wait: Duration) -> Result<()> {
        if self.blacked_out() {
            return Ok(());
        }
        if self.failure_since.elapsed() >= self.host_timeout + MIGRATION_WINDOW {
            tracing::error!(
                "p2p: сессия не восстановлена за {:?} — матч остаётся на паузе",
                self.host_timeout + MIGRATION_WINDOW
            );
            self.mode = P2pMode::Paused;
            self.push(IoEvent::Closed);
            return Ok(());
        }

        // The candidate is the deterministic successor, but a lost guest link
        // is not necessarily a lost authority (H5 standalone reconnect): probe
        // the old host first. A live `TP 00` means the ordinary reconnect, not
        // a takeover; only a dead endpoint starts the election.
        if self.roster.is_successor() {
            // Keep polling the old guest link for a healed heartbeat; only a
            // failed `TP 00` probe proves the host is really gone.
            if self.poll_live_link().await? {
                return Ok(());
            }
            if self.last_attempt.elapsed() >= MIGRATION_RETRY && self.guest.is_none() {
                // The old link is gone (a dead session or an aborted one), so
                // the authority is only reachable through a new `TP 00`:
                // a live probe means an ordinary reconnect, silence — takeover.
                self.last_attempt = Instant::now();
                if self.old_host_is_alive().await {
                    // The authority is alive: reconnect as an ordinary guest
                    // (the successor only takes over when the host is silent).
                    self.reconnect_guest().await;
                    return Ok(());
                }
                if self.try_start_takeover(false).await? {
                    return Ok(());
                }
                // The port was still held (a dying authority): a reconnect may
                // already work through the same endpoint.
                self.reconnect_guest().await;
            }
        } else {
            // A survivor that is not the candidate also keeps its live ENet
            // session: during a relay break the session is up but silent (a
            // split-brain, not a dead link), and only ENet's reliable queue can
            // still deliver the packets of the other survivors when the relay
            // heals. Reconnecting would throw that queue away.
            if self.poll_live_link().await? {
                return Ok(());
            }
            if self.last_attempt.elapsed() >= MIGRATION_RETRY && self.guest.is_none() {
                self.last_attempt = Instant::now();
                self.reconnect_guest().await;
            }
        }
        Ok(())
    }

    /// One step while waiting for the proposal/resume after a reconnect.
    async fn awaiting_step(&mut self, wait: Duration) -> Result<()> {
        if self.last_heartbeat.elapsed() >= self.host_timeout {
            self.enter_failure()?;
            self.push(IoEvent::HostTimeout);
            return Ok(());
        }
        if self.blacked_out() {
            return Ok(());
        }
        let Some(guest) = self.guest.as_mut() else {
            self.enter_failure()?;
            self.push(IoEvent::Disconnected);
            return Ok(());
        };
        let wait = wait.clamp(Duration::from_millis(1), STEP_MAX_WAIT);
        match guest.next_io_event(wait).await? {
            IoEvent::Packet(packet) => self.on_guest_packet(packet).await?,
            IoEvent::Idle => {}
            IoEvent::Closed | IoEvent::Disconnected | IoEvent::HostTimeout => {
                self.enter_failure()?;
                self.push(IoEvent::Disconnected);
            }
            // Game traffic stays frozen until the proposal is validated.
            _ => {}
        }
        Ok(())
    }

    /// Completes a migration episode: `Restored` when the transport is usable
    /// again, `Exhausted` when the match must stay frozen.
    pub async fn drive_migration(&mut self) -> Result<DisconnectOutcome> {
        if matches!(self.mode, P2pMode::Host | P2pMode::Paused) {
            return Ok(DisconnectOutcome::Ignored);
        }
        if self.mode != P2pMode::HostFailure {
            self.enter_failure()?;
        }
        loop {
            if self.blacked_out() {
                // Split-brain injection: no progress is possible, but the
                // migration episode must keep its wall-clock window.
                tokio::time::sleep(STEP_MAX_WAIT).await;
                if self.failure_since.elapsed() >= self.host_timeout + MIGRATION_WINDOW {
                    self.mode = P2pMode::Paused;
                    self.push(IoEvent::Closed);
                }
                continue;
            }
            if self.resume_pending {
                self.resume_pending = false;
                return Ok(DisconnectOutcome::Restored {
                    attempts: self.attempts,
                    migrated: self.takeover_completed,
                });
            }
            match self.mode {
                P2pMode::Host => {
                    return Ok(DisconnectOutcome::Restored {
                        attempts: self.attempts,
                        migrated: self.takeover_completed,
                    });
                }
                P2pMode::Paused => {
                    return Ok(DisconnectOutcome::Exhausted {
                        attempts: self.attempts,
                        migrated: false,
                    });
                }
                P2pMode::HostFailure => self.failure_step(STEP_MAX_WAIT).await?,
                P2pMode::SuccessorCollecting => self.successor_step(STEP_MAX_WAIT).await?,
                P2pMode::AwaitingTakeover => self.awaiting_step(STEP_MAX_WAIT).await?,
                P2pMode::Guest => {
                    self.step(Duration::from_millis(1)).await?;
                }
            }
            // Queue everything except control traffic; the driver applies it
            // after `on_disconnect` returns (eliminations, heartbeats, …).
            // Requeue through a temporary buffer: pushing straight back into
            // the same queue would spin forever.
            let mut deferred = Vec::new();
            while let Some(event) = self.pop() {
                if !matches!(event, IoEvent::Control | IoEvent::Idle) {
                    deferred.push(event);
                }
            }
            for event in deferred {
                self.pending.push_back(event);
            }
        }
    }

    /// One non-blocking transport step (no pending-queue pop).
    async fn step(&mut self, wait: Duration) -> Result<()> {
        match self.mode {
            P2pMode::Host => self.host_step(wait).await,
            P2pMode::Guest => self.guest_step(wait).await,
            P2pMode::HostFailure => self.failure_step(wait).await,
            P2pMode::SuccessorCollecting => self.successor_step(wait).await,
            P2pMode::AwaitingTakeover => self.awaiting_step(wait).await,
            P2pMode::Paused => Ok(()),
        }
    }

    /// Closes the guest session and/or the host server.
    pub async fn shutdown(mut self) {
        if let Some(guest) = self.guest.take() {
            guest.shutdown().await;
        }
        if let Some(host) = self.host.take() {
            host.shutdown().await;
        }
    }
}

/// Resolves the ENet session endpoint into the address the successor binds.
///
/// On localhost/single-machine deployments (the H4/H5 gates) every participant
/// resolves the same loopback address; a real LAN host address belongs to the
/// dead machine, so a successor cannot bind it — it then stays paused, which is
/// the safe behaviour (`PLAN-host-mode.md` §6: LAN/NAT is out of MVP scope).
fn resolve_bind_ip(host: &str) -> Option<Ipv4Addr> {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Some(ip);
    }
    (host, 0)
        .to_socket_addrs()
        .ok()?
        .find_map(|address| match address {
            SocketAddr::V4(v4) => Some(*v4.ip()),
            SocketAddr::V6(_) => None,
        })
}

impl HostIo for P2pTransport {
    async fn send_packet(&mut self, packet: Packet) -> Result<()> {
        self.send_packet_untracked(packet).await
    }

    async fn send_packet_untracked(&mut self, packet: Packet) -> Result<()> {
        #[cfg(test)]
        if self.blacked_out() {
            // Split-brain injection: the relay is down, so the frame is held
            // (frame pacing 01 is dropped — it is not a lockstep fact) and
            // released by `flush_blackout` when the relay heals.
            if !matches!(
                packet,
                Packet::Command(max_core::input::Command::FrameBarrier(_))
            ) && self.blackout_out.len() < BLACKOUT_MAX_FRAMES
            {
                self.blackout_out.push(packet);
            }
            return Ok(());
        }
        self.send_to_peers(packet).await
    }

    async fn clear_barriers(&mut self) -> Result<()> {
        if let Some(guest) = self.guest.as_mut() {
            guest.clear_barriers().await?;
        }
        Ok(())
    }

    async fn next_io_event(&mut self, wait: Duration) -> Result<IoEvent> {
        #[cfg(test)]
        self.flush_blackout().await?;
        if let Some(event) = self.pop() {
            return Ok(event);
        }
        self.step(wait).await?;
        Ok(self.pop().unwrap_or(IoEvent::Idle))
    }

    async fn on_disconnect(&mut self) -> Result<DisconnectOutcome> {
        self.drive_migration().await
    }

    fn note_lockstep(&mut self, position: LockstepPosition) {
        P2pTransport::note_lockstep(self, position);
    }

    fn quorum_allows_barrier(&self) -> bool {
        P2pTransport::quorum_allows_barrier(self)
    }

    fn is_authority(&self) -> bool {
        matches!(self.mode, P2pMode::Host)
    }
}

#[cfg(test)]
#[path = "p2p_tests.rs"]
mod tests;
