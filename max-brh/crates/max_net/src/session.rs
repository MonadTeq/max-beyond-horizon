//! UDP session with the headless server (ENet channel 0 handshake + channel 1
//! game protocol).
//!
//! # Flow
//!
//! 1. [`Session::connect`] resolves the host, creates a client ENet host with
//!    two channels, calls `enet_host_connect` and waits for
//!    `ENET_EVENT_TYPE_CONNECT` (up to [`SessionConfig::connect_timeout`]).
//! 2. Both sides send `TP 00` on channel 0; the client validates the server's
//!    copy (`max-brh-docs/protocol.md` §7). A mismatch sends `TP 03` and drops the
//!    link; a server `TP 03` becomes [`SessionError::Handshake`].
//! 3. [`SessionHandle::start`] moves the session into a tokio task with a
//!    command/event channel pair. Incoming APPL frames are decoded into
//!    [`Packet`]s, malformed frames are reported and ignored (the C++ server
//!    does the same), and barrier packets (00/45/52) are retransmitted every
//!    3 s with a 30 s timeout.
//!
//! The ENet host is serviced with a non-blocking `enet_host_service(..., 0)`
//! every [`SessionConfig::service_tick`] (10 ms, matching the C++ client),
//! so the async runtime is never blocked inside the C library.
//!
//! The dependency on max_core is intentional (read-only projection of the
//! simulation onto the wire, packets 23/45/53/54) — see
//! max-brh-docs/development/PLAN-refactor.md §5.3 /
//! architect-rules.md. Do NOT add new max_core calls without revisiting the
//! decision. Calling apply_* or mutating GameState from max_net voids it.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use max_types::enums::TeamClan;
use max_types::input::Command;
use max_types::team::TeamId;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::enet::{
    self, EnetError, EventKind, Host, Peer, CHANNEL_APPL, CHANNEL_COUNT, CHANNEL_TP,
};
use crate::handshake::{
    DataHash, Handshake, HandshakeError, Reject, RejectReason, TransportPacket,
    DEFAULT_GAME_VERSION,
};
use crate::horizon::GameJoined;
use crate::wire::{Packet, WireError, MAX_PACKET_SIZE};

/// Default time allowed for the ENet connect handshake.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Default time allowed for the TP 00/TP 03 exchange.
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Default ENet service cadence (C++ client: 10 ms).
pub const DEFAULT_SERVICE_TICK: Duration = Duration::from_millis(10);
/// Default barrier retransmit period (`max-brh-docs/protocol.md` §8.6).
pub const DEFAULT_BARRIER_REPEAT: Duration = Duration::from_secs(3);
/// Default barrier timeout (`max-brh-docs/protocol.md` §8.6).
pub const DEFAULT_BARRIER_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration of one UDP session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Host from `game_joined.host`.
    pub host: String,
    /// UDP port from `game_joined.port`.
    pub port: u16,
    /// Ticket from `game_joined.ticket`.
    pub ticket: String,
    /// Server build's `GAME_VERSION` (`(major<<16)|(minor<<8)|patch`).
    pub game_version: u32,
    /// `SHA256(MAX.RES) ‖ SHA256(PATCHES.RES)` of the local data files.
    pub data_hash: DataHash,
    /// Player name sent in the lobby (packet 36).
    pub player_name: String,
    /// Preferred team slot 0..=3 (packet 35).
    pub team_slot: u8,
    /// Preferred clan (packet 26).
    pub clan: TeamClan,
    /// ENet connect timeout.
    pub connect_timeout: Duration,
    /// TP 00/TP 03 timeout.
    pub handshake_timeout: Duration,
    /// ENet service cadence.
    pub service_tick: Duration,
    /// Barrier retransmit period.
    pub barrier_repeat: Duration,
    /// Barrier timeout.
    pub barrier_timeout: Duration,
}

impl SessionConfig {
    /// Builds a config with default timings and team slot 0 / random clan.
    pub fn new(
        host: impl Into<String>,
        port: u16,
        ticket: impl Into<String>,
        data_hash: DataHash,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            ticket: ticket.into(),
            game_version: DEFAULT_GAME_VERSION,
            data_hash,
            player_name: "max_net".to_string(),
            team_slot: 0,
            clan: TeamClan::Random,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            service_tick: DEFAULT_SERVICE_TICK,
            barrier_repeat: DEFAULT_BARRIER_REPEAT,
            barrier_timeout: DEFAULT_BARRIER_TIMEOUT,
        }
    }

    /// Builds a config from a Horizon [`GameJoined`] response.
    pub fn from_joined(joined: &GameJoined, data_hash: DataHash) -> Self {
        Self::new(
            joined.host.clone(),
            joined.port,
            joined.ticket.value.clone(),
            data_hash,
        )
    }

    /// Sets the server's `GAME_VERSION`.
    pub fn with_game_version(mut self, game_version: u32) -> Self {
        self.game_version = game_version;
        self
    }

    /// Sets the player name / team slot / clan used by the lobby phase.
    pub fn with_player(mut self, name: impl Into<String>, team_slot: u8, clan: TeamClan) -> Self {
        self.player_name = name.into();
        self.team_slot = team_slot;
        self.clan = clan;
        self
    }
}

/// Session failures.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// ENet-level failure.
    #[error(transparent)]
    Enet(#[from] EnetError),
    /// Channel-0 handshake failure.
    #[error(transparent)]
    Handshake(#[from] HandshakeError),
    /// Malformed game packet (only fatal while sending).
    #[error(transparent)]
    Wire(#[from] WireError),
    /// Host name could not be resolved to an IPv4 address.
    #[error("cannot resolve {host}:{port} to IPv4: {reason}")]
    Resolution {
        /// Host that failed to resolve.
        host: String,
        /// Port that was queried.
        port: u16,
        /// OS/dialer message.
        reason: String,
    },
    /// A stage exceeded its timeout.
    #[error("{stage} timed out after {timeout:?}")]
    Timeout {
        /// Stage name (`"connect"`, `"handshake"`, ...).
        stage: &'static str,
        /// Configured timeout.
        timeout: Duration,
    },
    /// The peer disconnected during a stage.
    #[error("disconnected during {stage}")]
    Disconnected {
        /// Stage name.
        stage: &'static str,
    },
    /// The session task/handle was closed.
    #[error("session channel is closed")]
    Closed,
}

/// Event delivered to the application by [`SessionHandle`].
#[derive(Debug)]
pub enum SessionEvent {
    /// TP 00 exchange succeeded; APPL packets are now accepted.
    HandshakeOk,
    /// A decoded game/lobby packet.
    ///
    /// The ENet sender is deliberately not part of the event: in the star
    /// topology relayed copies arrive from the server, so a command's team can
    /// only come from its payload ([`payload_team`]).
    Packet(Packet),
    /// A tracked barrier is due for retransmission.
    BarrierDue {
        /// Packet to resend.
        packet: Box<Packet>,
        /// Retransmission number (1 = first repeat).
        attempt: u32,
    },
    /// A tracked barrier waited longer than the configured timeout.
    BarrierTimeout {
        /// Barrier packet that timed out.
        packet: Box<Packet>,
    },
    /// The server rejected the handshake (`TP 03`).
    HandshakeRejected {
        /// Reject reason.
        reason: RejectReason,
        /// Diagnostic text.
        reason_text: String,
    },
    /// The peer disconnected (ENet event).
    Disconnected,
    /// A channel-1 frame failed to decode; ignored like the C++ receiver.
    Malformed {
        /// Decode error.
        error: WireError,
    },
    /// Fatal session error; the task stops after this event.
    Error(SessionError),
}

/// Commands accepted by a running [`SessionHandle`].
#[derive(Debug)]
pub enum SessionCommand {
    /// Encode and send an APPL packet on channel 1.
    ///
    /// Barrier packets (00/45/52) are also registered for retransmission. A
    /// caller that repeats an already tracked barrier itself must use
    /// [`SessionCommand::SendUntracked`] instead, otherwise every repeat adds
    /// another tracker entry (`max-brh-docs/barrier-flow.md` §6.8).
    Send(Packet),
    /// Encode and send an APPL packet **without** registering it for
    /// retransmission.
    ///
    /// Used for repeats of a barrier that is already tracked: the tracker keeps
    /// exactly one entry per barrier instead of one per retransmit.
    SendUntracked(Packet),
    /// Send an already encoded transport frame (packets 55/56) on channel 1.
    SendFrame(Vec<u8>),
    /// Stop retransmitting tracked barriers (e.g. the turn completed).
    ClearBarriers,
    /// Disconnect and stop the task.
    Disconnect,
}

/// Barrier bookkeeping (`max-brh-docs/protocol.md` §8.6): repeat every 3 s, wait 30 s.
#[derive(Debug, Clone, Copy)]
pub struct BarrierTiming {
    /// Retransmit period.
    pub repeat: Duration,
    /// Give-up timeout.
    pub timeout: Duration,
}

impl Default for BarrierTiming {
    fn default() -> Self {
        Self {
            repeat: DEFAULT_BARRIER_REPEAT,
            timeout: DEFAULT_BARRIER_TIMEOUT,
        }
    }
}

/// Action produced by [`BarrierTracker::poll`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarrierAction {
    /// Retransmit the packet.
    Retransmit {
        /// Packet to resend.
        packet: Packet,
        /// Retransmission number.
        attempt: u32,
    },
    /// The barrier timed out.
    TimedOut {
        /// Packet that timed out.
        packet: Packet,
    },
}

#[derive(Debug)]
struct PendingBarrier {
    packet: Packet,
    first_sent: Instant,
    last_sent: Instant,
    attempts: u32,
}

/// Tracks outgoing barrier packets.
///
/// The server arbiter does not echo barriers back (`max-brh-docs/protocol.md` §8.6),
/// so the client cannot observe its own barrier being accepted. The tracker
/// therefore keeps retransmitting until the application calls
/// [`BarrierTracker::clear`] (exposed as [`SessionCommand::ClearBarriers`]),
/// and reports a timeout so the app can mark the turn as stalled.
#[derive(Debug)]
pub struct BarrierTracker {
    timing: BarrierTiming,
    pending: Vec<PendingBarrier>,
}

impl BarrierTracker {
    /// Creates a tracker with explicit timings.
    pub const fn new(timing: BarrierTiming) -> Self {
        Self {
            timing,
            pending: Vec::new(),
        }
    }

    /// Starts tracking a barrier packet at `now`.
    pub fn track(&mut self, packet: Packet, now: Instant) {
        self.pending.push(PendingBarrier {
            packet,
            first_sent: now,
            last_sent: now,
            attempts: 0,
        });
    }

    /// Number of tracked barriers.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// True when no barrier is tracked.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Drops all tracked barriers.
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// Returns due retransmits/timeouts and updates internal timestamps.
    pub fn poll(&mut self, now: Instant) -> Vec<BarrierAction> {
        let mut actions = Vec::new();
        let mut keep = Vec::with_capacity(self.pending.len());

        for mut pending in self.pending.drain(..) {
            if now.duration_since(pending.first_sent) >= self.timing.timeout {
                actions.push(BarrierAction::TimedOut {
                    packet: pending.packet,
                });
                continue;
            }
            if now.duration_since(pending.last_sent) >= self.timing.repeat {
                pending.last_sent = now;
                pending.attempts = pending.attempts.saturating_add(1);
                actions.push(BarrierAction::Retransmit {
                    packet: pending.packet.clone(),
                    attempt: pending.attempts,
                });
            }
            keep.push(pending);
        }

        self.pending = keep;
        actions
    }
}

/// A connected + handshaken session (not yet running).
#[derive(Debug)]
pub struct Session {
    config: SessionConfig,
    host: Host,
    peer: Peer,
    local_handshake: Handshake,
    barrier: BarrierTracker,
}

impl Session {
    /// Connects to the headless server and performs the TP 00 exchange.
    pub async fn connect(config: SessionConfig) -> Result<Self, SessionError> {
        enet::initialize()?;
        let ip = resolve_ipv4(&config.host, config.port).await?;
        let address = enet::address(ip, config.port)?;
        let mut host = Host::client(CHANNEL_COUNT)?;
        let mut peer = host.connect(address, CHANNEL_COUNT)?;
        host.flush();

        // 1. Wait for the ENet connect event.
        let deadline = Instant::now() + config.connect_timeout;
        let mut connected = false;
        while !connected {
            while let Some(event) = host.service(0)? {
                match event.kind {
                    EventKind::Connect => {
                        if let Some(connected_peer) = event.peer {
                            peer = connected_peer;
                        }
                        connected = true;
                    }
                    EventKind::Disconnect => {
                        return Err(SessionError::Disconnected { stage: "connect" })
                    }
                    EventKind::Receive | EventKind::None => {}
                }
            }
            if !connected {
                if Instant::now() >= deadline {
                    return Err(SessionError::Timeout {
                        stage: "connect",
                        timeout: config.connect_timeout,
                    });
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }

        // 2. Send TP 00 and wait for the peer's copy (or TP 03).
        let local_handshake =
            Handshake::new(config.game_version, config.ticket.clone(), config.data_hash);
        let hello = local_handshake.encode()?;
        host.send(peer, CHANNEL_TP, &hello)?;
        host.flush();

        let deadline = Instant::now() + config.handshake_timeout;
        loop {
            while let Some(event) = host.service(0)? {
                match event.kind {
                    EventKind::Disconnect => {
                        host.disconnect_now(peer, 0);
                        return Err(SessionError::Disconnected { stage: "handshake" });
                    }
                    EventKind::Receive if event.channel == CHANNEL_TP => {
                        let Some(payload) = event.payload.as_deref() else {
                            continue;
                        };
                        match TransportPacket::decode(payload) {
                            Ok(TransportPacket::Hello(remote)) => {
                                if let Err(reason) = local_handshake.validate_peer(&remote) {
                                    let reject = Reject::new(&local_handshake, reason);
                                    if let Ok(bytes) = reject.encode() {
                                        let _ = host.send(peer, CHANNEL_TP, &bytes);
                                        host.flush();
                                    }
                                    host.disconnect_later(peer, 0);
                                    return Err(SessionError::Handshake(
                                        HandshakeError::Rejected {
                                            reason,
                                            reason_text: reason.text().to_string(),
                                        },
                                    ));
                                }
                                return Ok(Self {
                                    barrier: BarrierTracker::new(BarrierTiming {
                                        repeat: config.barrier_repeat,
                                        timeout: config.barrier_timeout,
                                    }),
                                    config,
                                    host,
                                    peer,
                                    local_handshake,
                                });
                            }
                            Ok(TransportPacket::Reject(reject)) => {
                                host.disconnect_now(peer, 0);
                                return Err(SessionError::Handshake(HandshakeError::Rejected {
                                    reason: reject.reason,
                                    reason_text: reject.reason_text,
                                }));
                            }
                            Err(error) => {
                                return Err(SessionError::Handshake(error));
                            }
                        }
                    }
                    // APPL packets before verification are dropped
                    // (`max-brh-docs/protocol.md` §7).
                    EventKind::Receive | EventKind::Connect | EventKind::None => {}
                }
            }

            if Instant::now() >= deadline {
                host.disconnect_now(peer, 0);
                return Err(SessionError::Timeout {
                    stage: "handshake",
                    timeout: config.handshake_timeout,
                });
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    /// Session configuration.
    pub const fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// The local TP 00 copy sent to the server.
    pub const fn local_handshake(&self) -> &Handshake {
        &self.local_handshake
    }

    /// Starts the session on the current tokio runtime.
    pub fn start(self) -> SessionHandle {
        let (command_tx, command_rx) = mpsc::channel(64);
        let (event_tx, event_rx) = mpsc::channel(256);
        let task = tokio::spawn(self.run(command_rx, event_tx));
        SessionHandle {
            commands: command_tx,
            events: event_rx,
            task,
        }
    }

    async fn run(
        mut self,
        mut commands: mpsc::Receiver<SessionCommand>,
        events: mpsc::Sender<SessionEvent>,
    ) {
        if events.send(SessionEvent::HandshakeOk).await.is_err() {
            return;
        }

        let tick = self.config.service_tick.max(Duration::from_millis(1));
        // ENet must be serviced even while the application keeps sending.
        //
        // A `sleep(tick)` future created inside the `select!` restarts its
        // deadline on every iteration: during a barrier the game loop queues an
        // 01 frame barrier (and repeats every 3 s) far more often than `tick`,
        // so the command arm always won, the socket was never polled and the
        // relayed 00/52/45 packets sat unread in the ENet queue forever — the
        // barrier reported "no peers" after 30 s even though the server had
        // already forwarded every report. [`tokio::time::Interval`] keeps an
        // absolute deadline and `biased` lets a due tick win over an
        // always-ready command channel.
        let mut service = tokio::time::interval(tick);
        service.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                biased;
                _ = service.tick() => {
                    match self.pump(&events).await {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(error) => {
                            let _ = events.send(SessionEvent::Error(error)).await;
                            break;
                        }
                    }
                    self.poll_barriers(&events).await;
                }
                command = commands.recv() => {
                    match command {
                        Some(SessionCommand::Send(packet)) => {
                            if let Err(error) = self.send_packet(&packet) {
                                let _ = events.send(SessionEvent::Error(error)).await;
                            }
                        }
                        Some(SessionCommand::SendUntracked(packet)) => {
                            if let Err(error) = self.send_raw(&packet) {
                                let _ = events.send(SessionEvent::Error(error)).await;
                            }
                        }
                        Some(SessionCommand::SendFrame(frame)) => {
                            if let Err(error) = self.send_frame_bytes(&frame) {
                                let _ = events.send(SessionEvent::Error(error)).await;
                            }
                        }
                        Some(SessionCommand::ClearBarriers) => self.barrier.clear(),
                        Some(SessionCommand::Disconnect) | None => {
                            self.host.disconnect_later(self.peer, 0);
                            self.host.flush();
                            break;
                        }
                    }
                }
            }
        }

        let _ = events.send(SessionEvent::Disconnected).await;
    }

    /// Drains ENet events; returns `Ok(false)` when the peer disconnected.
    async fn pump(&mut self, events: &mpsc::Sender<SessionEvent>) -> Result<bool, SessionError> {
        loop {
            let Some(event) = self.host.service(0)? else {
                return Ok(true);
            };
            match event.kind {
                EventKind::Disconnect => {
                    let _ = events.send(SessionEvent::Disconnected).await;
                    return Ok(false);
                }
                EventKind::Connect => {}
                EventKind::Receive => {
                    let Some(payload) = event.payload.as_deref() else {
                        continue;
                    };
                    match event.channel {
                        CHANNEL_APPL => match Packet::decode(payload) {
                            Ok(packet) => {
                                if events.send(SessionEvent::Packet(packet)).await.is_err() {
                                    return Ok(false);
                                }
                            }
                            Err(error) => {
                                // A malformed frame is a per-packet error in
                                // the C++ receiver too: report and continue.
                                if events
                                    .send(SessionEvent::Malformed { error })
                                    .await
                                    .is_err()
                                {
                                    return Ok(false);
                                }
                            }
                        },
                        CHANNEL_TP => {
                            // Late TP 03 (e.g. a ticket rejected after a
                            // reconnect attempt) is surfaced; a late TP 00 is
                            // simply ignored.
                            if let Ok(TransportPacket::Reject(reject)) =
                                TransportPacket::decode(payload)
                            {
                                let _ = events
                                    .send(SessionEvent::HandshakeRejected {
                                        reason: reject.reason,
                                        reason_text: reject.reason_text,
                                    })
                                    .await;
                                return Ok(false);
                            }
                        }
                        _ => {}
                    }
                }
                EventKind::None => {}
            }
        }
    }

    /// Sends one encoded APPL packet and tracks barrier retransmission.
    fn send_packet(&mut self, packet: &Packet) -> Result<(), SessionError> {
        self.send_raw(packet)?;
        if records_barrier(true, packet) {
            self.barrier.track(packet.clone(), Instant::now());
        }
        Ok(())
    }

    fn send_raw(&mut self, packet: &Packet) -> Result<(), SessionError> {
        let bytes = packet.encode()?;
        debug_assert!(bytes.len() <= MAX_PACKET_SIZE);
        self.host.send(self.peer, CHANNEL_APPL, &bytes)?;
        self.host.flush();
        Ok(())
    }

    /// Sends an already encoded transport frame (packets 55/56).
    fn send_frame_bytes(&mut self, frame: &[u8]) -> Result<(), SessionError> {
        debug_assert!(frame.len() <= MAX_PACKET_SIZE);
        self.host.send(self.peer, CHANNEL_APPL, frame)?;
        self.host.flush();
        Ok(())
    }

    async fn poll_barriers(&mut self, events: &mpsc::Sender<SessionEvent>) {
        for action in self.barrier.poll(Instant::now()) {
            match action {
                BarrierAction::Retransmit { packet, attempt } => {
                    if let Err(error) = self.send_raw(&packet) {
                        let _ = events.send(SessionEvent::Error(error)).await;
                        return;
                    }
                    let _ = events
                        .send(SessionEvent::BarrierDue {
                            packet: Box::new(packet),
                            attempt,
                        })
                        .await;
                }
                BarrierAction::TimedOut { packet } => {
                    let _ = events
                        .send(SessionEvent::BarrierTimeout {
                            packet: Box::new(packet),
                        })
                        .await;
                }
            }
        }
    }
}

/// Barrier packets retransmitted by the session (00 turn, 45 CRC, 52 end-turn).
///
/// The per-frame barrier 01 is sent by the game loop every tick and is
/// deliberately not tracked here.
fn is_tracked_barrier(packet: &Packet) -> bool {
    matches!(
        packet,
        Packet::Command(
            Command::TurnBarrier(_) | Command::CrcBarrier(_) | Command::EndTurnBarrier(_)
        )
    )
}

/// Team that owns a barrier/command packet — **always from the payload**.
///
/// In the star topology a relayed packet reaches the client from the server,
/// so the ENet source address of a packet says nothing about the team that
/// produced it: `Remote_GetSenderTeam` would report -1. Every barrier handler
/// (`apply_packet_00/52/45`, `Remote_ReceiveNetPacket_00/52/45`) therefore
/// reads the `team u16` field of the payload instead. This helper exposes the
/// same rule for diagnostics and tests.
///
/// Returns `None` for packets without a team field (lobby/audit frames,
/// chat carries a sender rather than an owner, …).
pub fn payload_team(packet: &Packet) -> Option<TeamId> {
    match packet {
        Packet::Command(Command::TurnBarrier(p)) => Some(p.team),
        Packet::Command(Command::EndTurnBarrier(p)) => Some(p.team),
        Packet::Command(Command::CrcBarrier(p)) => Some(p.team),
        Packet::Command(Command::FrameBarrier(p)) => Some(p.team),
        Packet::Command(Command::FinishedTurn(p)) => Some(p.team),
        Packet::Command(Command::ComplexSummary(p)) => Some(p.team),
        Packet::StateSync(p) => Some(p.team),
        Packet::Desync { team, .. } => Some(*team),
        _ => None,
    }
}

/// Should `Send`/`SendUntracked` register the packet in
/// [`BarrierTracker`]?
///
/// `SendUntracked` is `tracked = false`: repeating an already tracked barrier
/// does not create a second entry (otherwise every retransmit would add a new
/// one and the number of concurrent repeats would grow without bound).
fn records_barrier(tracked: bool, packet: &Packet) -> bool {
    tracked && is_tracked_barrier(packet)
}

/// Read-only projection: sim → wire. Does not call apply_*, does not mutate GameState.
///
/// Builds the packet 45 CRC barrier from the authoritative local state.
///
/// The checksum is exactly what `max_core::crc::state_checksum` gives (the
/// same fields and order as `GameManager_CheckDesync`), so the owner team can
/// send its units' CRC without duplicating the algorithm in `max_net`.
pub fn crc_barrier_for(
    team: max_types::team::TeamId,
    next_turn_index: u8,
    state: max_core::universe::StateRef<'_>,
) -> Packet {
    Packet::Command(Command::CrcBarrier(max_types::input::Packet45CrcBarrier {
        team,
        next_turn_index,
        crc16: state.state_checksum(),
    }))
}

/// Read-only projection: sim → wire. Does not call apply_*, does not mutate GameState.
///
/// Builds packet 53 — the owner's complex totals at the start of a turn.
///
/// Port of `Remote_SendNetPacket_53` (`remote.cpp:4243-4258`): `turn` is the
/// C++ `GameManager_TurnCounter` (**1-based**), while `max_core` starts at 0 —
/// the caller maps it (`state.turn_counter + 1`). Complex order — the
/// `TeamUnits::complexes()` order (ascending id), values in `Complex::WritePacket`
/// order.
pub fn complex_summary_for(
    state: max_core::universe::StateRef<'_>,
    team: max_types::team::TeamId,
    turn: i32,
) -> Packet {
    let complexes = state.team_units()[team.0 as usize]
        .complexes()
        .iter()
        .map(|complex| max_types::input::ComplexSummaryEntry {
            complex_id: complex.id as u16,
            values: [
                complex.raw,
                complex.fuel,
                complex.gold,
                complex.power,
                complex.workers,
                complex.buildings,
            ],
        })
        .collect();

    Packet::Command(Command::ComplexSummary(
        max_types::input::Packet53ComplexSummary {
            team,
            turn,
            complexes,
        },
    ))
}

/// Handle of a running session.
#[derive(Debug)]
pub struct SessionHandle {
    commands: mpsc::Sender<SessionCommand>,
    events: mpsc::Receiver<SessionEvent>,
    task: JoinHandle<()>,
}

impl SessionHandle {
    /// Connects, handshakes and starts the session task.
    pub async fn start(config: SessionConfig) -> Result<Self, SessionError> {
        let session = Session::connect(config).await?;
        Ok(session.start())
    }

    /// Encodes and queues a packet. Encoding errors are returned immediately.
    pub async fn send(&self, packet: Packet) -> Result<(), SessionError> {
        packet.encode()?;
        self.commands
            .send(SessionCommand::Send(packet))
            .await
            .map_err(|_| SessionError::Closed)
    }

    /// Encodes and queues a packet **without** registering it for session
    /// retransmission.
    ///
    /// This is the retransmit path: a packet already present in the session tracker
    /// must not add another entry to it on every retransmit.
    pub async fn send_untracked(&self, packet: Packet) -> Result<(), SessionError> {
        packet.encode()?;
        self.commands
            .send(SessionCommand::SendUntracked(packet))
            .await
            .map_err(|_| SessionError::Closed)
    }

    /// Sends an already encoded **transport** frame (takeover 56/55) on
    /// channel 1.
    ///
    /// Like the host side, transport frames are outside the [`Packet`] model:
    /// packet 56 carries the takeover negotiation and is never routed through a
    /// `Dispatcher` (`PLAN-host-mode.md` §5 guardrails).
    pub async fn send_frame(&self, frame: Vec<u8>) -> Result<(), SessionError> {
        if frame.len() > MAX_PACKET_SIZE {
            return Err(SessionError::Wire(WireError::PacketTooLarge {
                size: frame.len(),
                max: MAX_PACKET_SIZE,
            }));
        }
        self.commands
            .send(SessionCommand::SendFrame(frame))
            .await
            .map_err(|_| SessionError::Closed)
    }

    /// Stops barrier retransmission (e.g. after the turn barrier was reached).
    pub async fn clear_barriers(&self) -> Result<(), SessionError> {
        self.commands
            .send(SessionCommand::ClearBarriers)
            .await
            .map_err(|_| SessionError::Closed)
    }

    /// Waits for the next event; `None` when the task ended.
    pub async fn next_event(&mut self) -> Option<SessionEvent> {
        self.events.recv().await
    }

    /// Receives with a timeout; `Ok(None)` means the task ended.
    pub async fn next_event_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<SessionEvent>, SessionError> {
        match tokio::time::timeout(timeout, self.events.recv()).await {
            Ok(event) => Ok(event),
            Err(_) => Err(SessionError::Timeout {
                stage: "event",
                timeout,
            }),
        }
    }

    /// Disconnects and waits briefly for the task to finish.
    pub async fn shutdown(mut self) {
        let _ = self.commands.send(SessionCommand::Disconnect).await;
        if tokio::time::timeout(Duration::from_secs(2), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
        }
    }

    /// Aborts the task without a graceful disconnect.
    pub fn abort(self) {
        self.task.abort();
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn resolve_ipv4(host: &str, port: u16) -> Result<Ipv4Addr, SessionError> {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok(ip);
    }
    let iter = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| SessionError::Resolution {
            host: host.to_string(),
            port,
            reason: error.to_string(),
        })?;
    for addr in iter {
        match addr {
            SocketAddr::V4(v4) => return Ok(*v4.ip()),
            SocketAddr::V6(v6) => {
                if let Some(mapped) = v6.ip().to_ipv4_mapped() {
                    return Ok(mapped);
                }
            }
        }
    }
    Err(SessionError::Resolution {
        host: host.to_string(),
        port,
        reason: "no IPv4 address found (ENet 1.3.18 is IPv4-only)".to_string(),
    })
}

/// Parses a host string that may already be an IPv4 literal.
pub fn parse_ipv4(value: &str) -> Option<Ipv4Addr> {
    value.parse::<Ipv4Addr>().ok()
}

/// Converts an IPv4 address into [`IpAddr`] (convenience for callers).
pub const fn ip_addr(ip: Ipv4Addr) -> IpAddr {
    IpAddr::V4(ip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_types::input::{
        Packet00TurnBarrier, Packet01FrameBarrier, Packet18Chat, Packet45CrcBarrier,
        Packet52EndTurnBarrier,
    };
    use max_types::team::TeamId;

    fn barrier_packet(counter: u8) -> Packet {
        Packet::Command(Command::EndTurnBarrier(Packet52EndTurnBarrier {
            team: TeamId::RED,
            counter,
        }))
    }

    #[test]
    fn barrier_tracker_retransmits_and_times_out() {
        let start = Instant::now();
        let mut tracker = BarrierTracker::new(BarrierTiming {
            repeat: Duration::from_secs(3),
            timeout: Duration::from_secs(30),
        });
        tracker.track(barrier_packet(1), start);
        assert_eq!(tracker.len(), 1);

        assert!(tracker.poll(start + Duration::from_secs(1)).is_empty());
        let actions = tracker.poll(start + Duration::from_secs(3));
        assert!(matches!(
            actions.as_slice(),
            [BarrierAction::Retransmit { attempt: 1, .. }]
        ));
        let actions = tracker.poll(start + Duration::from_secs(6));
        assert!(matches!(
            actions.as_slice(),
            [BarrierAction::Retransmit { attempt: 2, .. }]
        ));
        assert_eq!(tracker.len(), 1);

        let actions = tracker.poll(start + Duration::from_secs(30));
        assert!(matches!(
            actions.as_slice(),
            [BarrierAction::TimedOut { .. }]
        ));
        assert!(tracker.is_empty());
    }

    #[test]
    fn barrier_tracker_clear_stops_retransmission() {
        let start = Instant::now();
        let mut tracker = BarrierTracker::new(BarrierTiming::default());
        tracker.track(barrier_packet(1), start);
        tracker.clear();
        assert!(tracker.is_empty());
        assert!(tracker.poll(start + Duration::from_secs(60)).is_empty());
    }

    #[test]
    fn tracked_barriers_are_only_00_45_52() {
        let turn = Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
            team: TeamId::RED,
            counter: 1,
        }));
        let crc = Packet::Command(Command::CrcBarrier(Packet45CrcBarrier {
            team: TeamId::RED,
            next_turn_index: 1,
            crc16: 0,
        }));
        assert!(is_tracked_barrier(&turn));
        assert!(is_tracked_barrier(&crc));
        assert!(is_tracked_barrier(&barrier_packet(1)));
        assert!(!is_tracked_barrier(&Packet::Command(Command::Chat(
            Packet18Chat {
                sender: TeamId::RED,
                message: "hi".into(),
            }
        ))));
    }

    /// Repeating a barrier through `SendUntracked` does not create a second tracker entry.
    ///
    /// The old path (`SessionCommand::Send` on every retransmit) added one
    /// entry per repeat — the number of concurrent repeats grew without bound.
    #[test]
    fn untracked_repeat_keeps_a_single_tracker_entry() {
        let now = Instant::now();
        let packet = barrier_packet(1);
        let mut tracker = BarrierTracker::new(BarrierTiming::default());

        assert!(records_barrier(true, &packet), "первый Send трекается");

        tracker.track(packet.clone(), now);
        assert_eq!(tracker.len(), 1);

        assert!(
            !records_barrier(false, &packet),
            "SendUntracked не должен трекать повтор"
        );

        // Exactly what `SessionCommand::SendUntracked` does: send only.
        assert_eq!(tracker.len(), 1, "повтор не добавил запись");

        // Whereas a repeat through Send would add one more — that was the double tracking.
        assert!(records_barrier(true, &packet));
        tracker.track(packet, now);
        assert_eq!(tracker.len(), 2);
    }

    #[test]
    fn complex_summary_for_matches_team_complexes() {
        use max_core::team::TeamUnits;
        use max_types::enums::TeamClan;

        let mut state = max_core::universe::GameUniverse::default();
        state
            .team_mut(TeamId::RED)
            .expect("RED slot")
            .team_type = max_types::team::TeamKind::Player;
        *state.team_units_mut(TeamId::RED).expect("RED economy") =
            TeamUnits::with_clan(TeamClan::Random);
        let id = state
            .team_units_mut(TeamId::RED)
            .expect("RED economy")
            .create_complex();
        let complex = state
            .team_units_mut(TeamId::RED)
            .expect("RED economy")
            .get_complex_mut(id)
            .expect("complex created");
        complex.raw = 10;
        complex.fuel = 20;
        complex.gold = 30;
        complex.power = 40;
        complex.workers = 50;
        complex.buildings = 60;

        match complex_summary_for(state.snapshot(), TeamId::RED, 1) {
            Packet::Command(Command::ComplexSummary(summary)) => {
                assert_eq!(summary.team, TeamId::RED);
                assert_eq!(
                    summary.turn, 1,
                    "turn — C++ GameManager_TurnCounter (1-based)"
                );
                assert_eq!(summary.complexes.len(), 1);
                assert_eq!(summary.complexes[0].complex_id, 1);
                assert_eq!(summary.complexes[0].values, [10, 20, 30, 40, 50, 60]);
            }
            other => panic!("expected packet 53, got {other:?}"),
        }
    }

    #[test]
    fn crc_barrier_uses_max_core_checksum() {
        let state = max_core::universe::GameUniverse::default();
        let packet = crc_barrier_for(TeamId::RED, 3, state.snapshot());
        match packet {
            Packet::Command(Command::CrcBarrier(barrier)) => {
                assert_eq!(barrier.team, TeamId::RED);
                assert_eq!(barrier.next_turn_index, 3);
                assert_eq!(barrier.crc16, state.snapshot().state_checksum());
            }
            other => panic!("expected CRC barrier, got {other:?}"),
        }
    }

    #[test]
    fn config_from_joined_copies_ticket_and_endpoint() {
        let joined = GameJoined {
            game_id: "g".into(),
            host: "127.0.0.1".into(),
            port: 31554,
            ticket: crate::horizon::Ticket {
                value: "sesame".into(),
                kind: crate::horizon::TicketKind::Legacy,
                session_id: None,
                player_id: None,
                expires_at_unix: None,
            },
        };
        let config = SessionConfig::from_joined(&joined, DataHash::from_bytes(b"a", b"b"));
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 31554);
        assert_eq!(config.ticket, "sesame");
        assert_eq!(config.game_version, DEFAULT_GAME_VERSION);
        assert_eq!(parse_ipv4(&config.host), Some(Ipv4Addr::LOCALHOST));
        assert_eq!(ip_addr(Ipv4Addr::LOCALHOST).to_string(), "127.0.0.1");
    }

    /// The barrier command is taken from the payload's `team` field, not from the sender.
    ///
    /// The relayed copy comes from the server (`Remote_GetSenderTeam` → -1 at the
    /// C++ client), so the only source of the command is the packet bytes.
    #[test]
    fn barrier_team_comes_from_the_payload_team_field() {
        let frame = [
            0x50, 0x01, 0x00, 0x01, // 00: team=1, counter=1
        ];
        let packet = Packet::decode(&frame).expect("decode 00");
        assert_eq!(payload_team(&packet), Some(TeamId(1)));

        let frame = [0x84, 0x02, 0x00, 0x05]; // 52: team=2, counter=5
        let packet = Packet::decode(&frame).expect("decode 52");
        assert_eq!(payload_team(&packet), Some(TeamId(2)));

        let frame = [0x7D, 0x03, 0x00, 0x04, 0xCD, 0xAB]; // 45: team=3
        let packet = Packet::decode(&frame).expect("decode 45");
        assert_eq!(payload_team(&packet), Some(TeamId(3)));

        // Packets 06 and 53 also have a team field.
        let frame = [0x56, 0x01, 0x00];
        let packet = Packet::decode(&frame).expect("decode 06");
        assert_eq!(payload_team(&packet), Some(TeamId(1)));
    }

    /// Regression: ENet is polled even while the application sends continuously.
    ///
    /// The old `select!` re-created `sleep(tick)` on every iteration, and
    /// the stream of outgoing 01 (every few milliseconds) kept the timer from
    /// firing: `pump` was not called, and relayed 00/52/45 sat in the socket until
    /// the 30-second barrier timeout. The test brings up a real ENet server,
    /// sends a relayed 00 and simultaneously floods the command channel.
    #[test]
    fn appl_frames_are_pumped_while_outgoing_commands_flood() {
        let port = {
            let socket = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe socket");
            socket.local_addr().expect("probe addr").port()
        };
        let data_hash = DataHash::from_bytes(b"max", b"patches");

        let server = std::thread::spawn(move || {
            let mut host =
                Host::server(Ipv4Addr::LOCALHOST, port, CHANNEL_COUNT).expect("test ENet server");
            let hello = Handshake::new(DEFAULT_GAME_VERSION, "", data_hash)
                .encode()
                .expect("server handshake");
            let relayed_00 = Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
                team: TeamId::GREEN,
                counter: 1,
            }))
            .encode()
            .expect("encode 00");

            let mut peer = None;
            let deadline = std::time::Instant::now() + Duration::from_secs(5);

            while std::time::Instant::now() < deadline {
                let Some(event) = host.service(5).expect("server service") else {
                    continue;
                };

                match event.kind {
                    EventKind::Connect => peer = event.peer.or(peer),
                    EventKind::Receive if event.channel == CHANNEL_TP => {
                        let Some(peer) = peer.or(event.peer) else {
                            continue;
                        };

                        host.send(peer, CHANNEL_TP, &hello).expect("send hello");
                        host.flush();

                        // Keep the channel busy: meanwhile the client floods
                        // the outgoing queue with 01 frames.
                        let until = std::time::Instant::now() + Duration::from_secs(3);
                        while std::time::Instant::now() < until {
                            host.send(peer, CHANNEL_APPL, &relayed_00)
                                .expect("send relayed 00");
                            host.flush();
                            let _ = host.service(10);
                        }

                        return;
                    }
                    _ => {}
                }
            }
        });

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        runtime.block_on(async move {
            let config = SessionConfig::new("127.0.0.1", port, "", data_hash);
            let mut handle = Session::connect(config).await.expect("connect").start();
            let frame_barrier = Packet::Command(Command::FrameBarrier(Packet01FrameBarrier {
                team: TeamId::RED,
                counter: 0,
            }));

            let deadline = tokio::time::Instant::now() + Duration::from_secs(3);

            while tokio::time::Instant::now() < deadline {
                handle
                    .send_untracked(frame_barrier.clone())
                    .await
                    .expect("send 01");

                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(1)) => {}
                    event = handle.next_event() => {
                        if let Some(SessionEvent::Packet(Packet::Command(
                            Command::TurnBarrier(barrier),
                        ))) = event
                        {
                            assert_eq!(barrier.team, TeamId::GREEN, "team из payload");
                            assert_eq!(barrier.counter, 1);
                            return;
                        }
                    }
                }
            }

            panic!("релейный 00 не дошёл: ENet не опрашивается при потоке исходящих команд");
        });

        server.join().expect("test ENet server thread");
    }
}
