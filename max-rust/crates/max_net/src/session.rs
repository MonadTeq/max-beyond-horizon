//! UDP session with the headless server (ENet channel 0 handshake + channel 1
//! game protocol).
//!
//! # Flow
//!
//! 1. [`Session::connect`] resolves the host, creates a client ENet host with
//!    two channels, calls `enet_host_connect` and waits for
//!    `ENET_EVENT_TYPE_CONNECT` (up to [`SessionConfig::connect_timeout`]).
//! 2. Both sides send `TP 00` on channel 0; the client validates the server's
//!    copy (`docs/protocol.md` §7). A mismatch sends `TP 03` and drops the
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

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use max_core::enums::TeamClan;
use max_core::input::Command;
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
/// Default barrier retransmit period (`docs/protocol.md` §8.6).
pub const DEFAULT_BARRIER_REPEAT: Duration = Duration::from_secs(3);
/// Default barrier timeout (`docs/protocol.md` §8.6).
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
    Send(Packet),
    /// Stop retransmitting tracked barriers (e.g. the turn completed).
    ClearBarriers,
    /// Disconnect and stop the task.
    Disconnect,
}

/// Barrier bookkeeping (`docs/protocol.md` §8.6): repeat every 3 s, wait 30 s.
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
/// The server arbiter does not echo barriers back (`docs/protocol.md` §8.6),
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
                    // (`docs/protocol.md` §7).
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
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tick) => {
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
        if is_tracked_barrier(packet) {
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

/// Builds the packet 45 CRC barrier from the authoritative local state.
///
/// The checksum is exactly what `max_core::crc::state_checksum` gives (the
/// same fields and order as `GameManager_CheckDesync`), so the owner team can
/// send its units' CRC without duplicating the algorithm in `max_net`.
pub fn crc_barrier_for(
    team: max_core::team::TeamId,
    next_turn_index: u8,
    state: &max_core::game::GameState,
) -> Packet {
    Packet::Command(Command::CrcBarrier(
        max_core::input::Packet45CrcBarrier {
            team,
            next_turn_index,
            crc16: max_core::crc::state_checksum(state),
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
    use max_core::input::{
        Packet00TurnBarrier, Packet18Chat, Packet45CrcBarrier, Packet52EndTurnBarrier,
    };
    use max_core::team::TeamId;

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

    #[test]
    fn crc_barrier_uses_max_core_checksum() {
        use max_core::game::GameState;

        let state = GameState::new();
        let packet = crc_barrier_for(TeamId::RED, 3, &state);
        match packet {
            Packet::Command(Command::CrcBarrier(barrier)) => {
                assert_eq!(barrier.team, TeamId::RED);
                assert_eq!(barrier.next_turn_index, 3);
                assert_eq!(barrier.crc16, max_core::crc::state_checksum(&state));
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
}
