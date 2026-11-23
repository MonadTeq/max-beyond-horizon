//! ENet **server** transport for the P2P host mode (`PLAN-host-mode.md` §5 H1).
//!
//! The host is the session authority of the star topology: every guest
//! connects to it and every application frame a guest produces is relayed to
//! the other guests. This module is **transport only** — it treats an APPL
//! payload as an opaque [`Packet`] envelope for [`HostEventKind::Packet`] and
//! never interprets a command. All game policy (lobby authority, barriers,
//! quorum, takeover) lives in `max_app::host` (H2–H4).
//!
//! # Responsibilities (H1)
//!
//! 1. **Accept loop.** [`HostSession`] binds the ENet server host
//!    ([`Host::server`], `enet.rs:292`) and services it every 10 ms, the same
//!    cadence as the C++ `TransportUdpDefault_ServerFunction`
//!    (`transport_udp_default.cpp:972-1042`) and the client loop in
//!    [`crate::session`] (`session.rs:529-545`).
//! 2. **Per-peer `TP 00`/`TP 03`.** On `ENET_EVENT_TYPE_CONNECT` the host
//!    immediately sends its `TP 00` (`protocol.md` §7); the guest's copy is
//!    validated against [`HostConfig::game_version`] and
//!    [`HostConfig::data_hash`]. A mismatch is answered with `TP 03` and the
//!    link is dropped; an incoming `TP 03` drops the link as well. APPL frames
//!    from a peer that has not passed `TP 00` are dropped (`protocol.md` §7).
//! 3. **Relay (multicast-except-source).** A verified guest's APPL frame is
//!    decoded for [`HostEventKind::Packet`] and the **original bytes** are
//!    forwarded to every other verified peer in receive order — the C++
//!    `REMOTE_MULTICAST` of `remote.cpp:826-862` minus the sender. The relay
//!    is copy-forward: no re-encoding, no reordering, no dropping of valid
//!    frames, so the host's receive order is the session's global order.
//! 4. **Heartbeat (packet 55).** [`HostHeartbeat`] is a transport frame of its
//!    own, *not* a [`Packet`] variant: the host driver publishes it through
//!    [`HostHandle::publish_heartbeat`], and the transport repeats it every
//!    [`HostConfig::heartbeat`] until a new one is published. Guests receive
//!    it as an unknown APPL kind 55 (the H3 driver decodes it).
//!
//! # Boundaries
//!
//! * no `max_core` / `GameState` / `Dispatcher` imports — only `max_types`
//!   DTOs (through [`crate::wire`]) and the wire types of this module;
//! * [`crate::session`] (the guest) is untouched: a guest is an ordinary
//!   client of this server, which is why the host reuses `Host::server` and
//!   the existing `TP 00`/`TP 03` codecs;
//! * no `unsafe` in this module (the `forbid` below); ENet stays in
//!   [`crate::enet`].
//!
//! # Event stream
//!
//! [`HostEvent`] carries the peer id and one of four kinds: `Connected` (ENet
//! handshake done), `Verified` (`TP 00` accepted), `Packet` (a decoded APPL
//! frame from that peer, already relayed) and `Disconnected` (the peer left
//! the roster for any reason — ENet disconnect, bad handshake or incoming
//! `TP 03`). The id is assigned in accept order and is stable for the life of
//! the connection; the host driver keeps its own `PeerId -> player` mapping
//! (H2).

#![forbid(unsafe_code)]

use std::fmt;
use std::net::Ipv4Addr;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use crate::enet::{
    EnetError, EventKind, Host, Peer, CHANNEL_APPL, CHANNEL_COUNT, CHANNEL_TP, PEER_COUNT,
};
use crate::handshake::{
    DataHash, Handshake, HandshakeError, Reject, TransportPacket, DEFAULT_GAME_VERSION,
};
use crate::wire::{Packet, Reader, WireError, Writer, TRANSPORT_APPL_PACKET_ID};

/// Default UDP port of a manually started host (`host_port` in the C++
/// settings, `transport_udp_default.cpp:63`).
pub const DEFAULT_HOST_PORT: u16 = 31554;
/// Default logical peer limit (ENet still allocates [`PEER_COUNT`] slots).
pub const DEFAULT_MAX_PEERS: usize = PEER_COUNT;
/// Default heartbeat repeat period.
///
/// It must stay well below the H4 `host_timeout` (5–300 s, default 90 s) so a
/// guest sees several heartbeats per timeout window.
pub const DEFAULT_HEARTBEAT_PERIOD: Duration = Duration::from_secs(1);
/// APPL number of the host heartbeat (`0x50 + 55` on the wire).
pub const HOST_HEARTBEAT_PACKET_NUMBER: u8 = 55;
/// Wire type byte of the host heartbeat (`0x87`).
pub const HOST_HEARTBEAT_TYPE: u8 = TRANSPORT_APPL_PACKET_ID + HOST_HEARTBEAT_PACKET_NUMBER;
/// ENet service cadence of the accept loop (C++ `ServiceTickPeriod`, 10 ms).
const HOST_SERVICE_TICK: Duration = Duration::from_millis(10);
/// Depth of the command channel ([`HostHandle`] → session task).
const HOST_COMMAND_QUEUE: usize = 64;
/// Depth of the event channel (session task → [`HostHandle`]).
const HOST_EVENT_QUEUE: usize = 256;
/// How long [`HostHandle::shutdown`] waits for the session task to end.
const HOST_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// Wire size of an encoded [`HostHeartbeat`] (type + body).
const HOST_HEARTBEAT_WIRE_LEN: usize = 1 + 4 + 4 + 1 + 4;

/// Identity of a connected peer inside one [`HostSession`].
///
/// Ids are allocated in accept order and are stable until the peer
/// disconnects. The numeric value is deliberately opaque: the host driver maps
/// it to its own player/node bookkeeping (H2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(u32);

impl PeerId {
    /// Numeric value (accept order, starting at 0).
    pub const fn value(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "peer#{}", self.0)
    }
}

/// Destination of [`HostHandle::send`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Recipient {
    /// One verified peer.
    Peer(PeerId),
    /// Every verified peer (the host itself is not a peer).
    Broadcast,
}

impl From<PeerId> for Recipient {
    fn from(peer: PeerId) -> Self {
        Self::Peer(peer)
    }
}

/// Configuration of one [`HostSession`].
///
/// The [`bind_ip`](Self::bind_ip)/[`port`](Self::port) pair is the ENet server
/// address; [`max_peers`](Self::max_peers) is the logical acceptance limit
/// (ENet always allocates [`PEER_COUNT`] slots); [`heartbeat`](Self::heartbeat)
/// is the repeat period of the last published [`HostHeartbeat`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConfig {
    /// Local IPv4 address to bind (`ENET_HOST_ANY`/`0.0.0.0` for all
    /// interfaces, like the C++ server that must not bind one interface).
    pub bind_ip: Ipv4Addr,
    /// UDP port ([`DEFAULT_HOST_PORT`] unless configured).
    pub port: u16,
    /// `GAME_VERSION` advertised in `TP 00` (`(major<<16)|(minor<<8)|patch`).
    pub game_version: u32,
    /// `SHA256(MAX.RES) ‖ SHA256(PATCHES.RES)` of the local data files.
    pub data_hash: DataHash,
    /// Maximum number of simultaneously accepted peers (`1..=PEER_COUNT`).
    pub max_peers: usize,
    /// Heartbeat repeat period; [`Duration::ZERO`] disables the repeat (a
    /// published heartbeat is then broadcast once).
    pub heartbeat: Duration,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            bind_ip: Ipv4Addr::UNSPECIFIED,
            port: DEFAULT_HOST_PORT,
            game_version: DEFAULT_GAME_VERSION,
            data_hash: DataHash {
                max_res: None,
                patches_res: None,
            },
            max_peers: DEFAULT_MAX_PEERS,
            heartbeat: DEFAULT_HEARTBEAT_PERIOD,
        }
    }
}

impl HostConfig {
    /// Config bound to `bind_ip:port` with default game version, peer limit
    /// and heartbeat period.
    pub fn new(bind_ip: Ipv4Addr, port: u16, data_hash: DataHash) -> Self {
        Self {
            bind_ip,
            port,
            game_version: DEFAULT_GAME_VERSION,
            data_hash,
            max_peers: DEFAULT_MAX_PEERS,
            heartbeat: DEFAULT_HEARTBEAT_PERIOD,
        }
    }

    /// Rejects values ENet or the transport cannot honour.
    fn validate(&self) -> Result<(), HostError> {
        if self.max_peers == 0 || self.max_peers > PEER_COUNT {
            return Err(HostError::Config(
                "max_peers must be within 1..=PEER_COUNT (32)",
            ));
        }
        Ok(())
    }
}

/// Transport heartbeat, APPL packet 55 (`PLAN-host-mode.md` §3, §5 H1).
///
/// This is **not** a [`Packet`] variant: it is host-to-guest transport data
/// used by the migration/quorum layer (H4), so it never reaches
/// `Dispatcher::route`.
///
/// Wire layout (all little-endian, 14 bytes, type byte included):
///
/// | offset | field | type |
/// |---|---|---|
/// | 0 | type | `u8` = [`HOST_HEARTBEAT_TYPE`] (`0x87`) |
/// | 1 | `host_epoch` | `u32` |
/// | 5 | `turn_counter` | `i32` (mirrors `GameManager_TurnCounter`) |
/// | 9 | `phase` | `u8` (opaque host FSM phase tag) |
/// | 10 | `reachable` | `u32` (bitmask of reachable player nodes) |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostHeartbeat {
    /// Monotonic host epoch (bumped on takeover, H4).
    pub host_epoch: u32,
    /// Host turn counter at the moment of the heartbeat.
    pub turn_counter: i32,
    /// Host barrier-FSM phase; opaque to the transport.
    pub phase: u8,
    /// Bitmask of player nodes the host can currently reach (`bit n` = node
    /// `n`). The transport only carries the value; the host driver fills it
    /// from its roster.
    pub reachable: u32,
}

impl HostHeartbeat {
    /// Encodes the complete APPL frame (type byte included).
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(HOST_HEARTBEAT_WIRE_LEN);
        w.write_u8(HOST_HEARTBEAT_TYPE);
        w.write_u32(self.host_epoch);
        w.write_i32(self.turn_counter);
        w.write_u8(self.phase);
        w.write_u32(self.reachable);
        w.finish()
    }

    /// Decodes a complete APPL frame, rejecting a different type byte,
    /// a truncated body and trailing bytes.
    pub fn decode(frame: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader::new(frame);
        let type_byte = r.read_u8()?;
        if type_byte != HOST_HEARTBEAT_TYPE {
            return Err(WireError::UnknownPacket(type_byte));
        }
        let heartbeat = Self {
            host_epoch: r.read_u32()?,
            turn_counter: r.read_i32()?,
            phase: r.read_u8()?,
            reachable: r.read_u32()?,
        };
        r.finish()?;
        Ok(heartbeat)
    }
}

/// Event delivered to the host driver by [`HostHandle`].
#[derive(Debug)]
pub struct HostEvent {
    /// Peer the event belongs to.
    pub peer: PeerId,
    /// What happened.
    pub kind: HostEventKind,
}

/// Kind of a [`HostEvent`].
///
/// `Packet` is not boxed on purpose: it mirrors
/// [`SessionEvent::Packet`](crate::session::SessionEvent::Packet) for the
/// unchanged guest client and the driver matches it per frame, so boxing would
/// add an allocation to every relayed packet (same trade-off the client event
/// already makes).
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum HostEventKind {
    /// The peer completed the ENet handshake (no `TP 00` yet).
    Connected,
    /// The peer's `TP 00` matched; APPL frames from it are accepted now.
    Verified,
    /// A decoded APPL frame from that peer; it has already been relayed to the
    /// other verified peers.
    Packet(Packet),
    /// The peer left the roster (ENet disconnect, rejected handshake or a
    /// `TP 03` from the peer).
    Disconnected,
}

/// Host transport failures.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// ENet-level failure (bind, service).
    #[error(transparent)]
    Enet(#[from] EnetError),
    /// The local `TP 00` could not be encoded.
    #[error(transparent)]
    Handshake(#[from] HandshakeError),
    /// A packet could not be encoded for sending.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// [`HostConfig::validate`] rejected the configuration.
    #[error("invalid host config: {0}")]
    Config(&'static str),
    /// A stage exceeded its timeout.
    #[error("{stage} timed out after {timeout:?}")]
    Timeout {
        /// Stage name (`"event"`, ...).
        stage: &'static str,
        /// Configured timeout.
        timeout: Duration,
    },
    /// The session task/handle channel was closed.
    #[error("host channel is closed")]
    Closed,
}

/// One accepted peer and its transport state.
#[derive(Debug)]
struct PeerEntry {
    id: PeerId,
    peer: Peer,
    verified: bool,
}

/// Commands accepted by a running [`HostSession`].
#[derive(Debug)]
enum HostCommand {
    /// Send an already encoded APPL frame on channel 1.
    Send { to: Recipient, frame: Vec<u8> },
    /// Broadcast and remember a heartbeat, repeating it on the config period.
    PublishHeartbeat(Vec<u8>),
    /// Flush queued ENet packets immediately.
    Flush,
    /// Disconnect every peer and stop the task.
    Shutdown,
}

/// A bound ENet server host (not yet running).
///
/// [`HostSession::bind`] only creates and binds the socket; the accept/relay
/// loop starts with [`HostSession::start`] (or through
/// [`HostHandle::bind`]). Keeping `bind` synchronous lets callers learn about a
/// bind failure before a runtime task exists and mirrors the
/// `Session::connect`/`Session::start` split of [`crate::session`].
#[derive(Debug)]
pub struct HostSession {
    config: HostConfig,
    local_handshake: Handshake,
    /// Encoded local `TP 00`, sent to every peer on `ENET_EVENT_TYPE_CONNECT`.
    hello: Vec<u8>,
    host: Host,
    /// Accepted peers in accept order: iteration order (broadcast and relay)
    /// must be deterministic, so no hash map is used here.
    peers: Vec<PeerEntry>,
    next_peer_id: u32,
    /// Last published heartbeat frame (already encoded).
    heartbeat_frame: Option<Vec<u8>>,
}

impl HostSession {
    /// Creates the ENet server host and binds [`HostConfig::bind_ip`] /
    /// [`HostConfig::port`].
    pub fn bind(config: HostConfig) -> Result<Self, HostError> {
        config.validate()?;
        let host = Host::server(config.bind_ip, config.port, CHANNEL_COUNT)?;
        let local_handshake = Handshake::new(config.game_version, String::new(), config.data_hash);
        let hello = local_handshake.encode()?;
        Ok(Self {
            config,
            local_handshake,
            hello,
            host,
            peers: Vec::new(),
            next_peer_id: 0,
            heartbeat_frame: None,
        })
    }

    /// Host configuration.
    pub const fn config(&self) -> &HostConfig {
        &self.config
    }

    /// The local `TP 00` copy sent to every peer (empty ticket: the ticket is
    /// a lobby concern, H2).
    pub const fn local_handshake(&self) -> &Handshake {
        &self.local_handshake
    }

    /// Starts the accept/relay loop on the current tokio runtime.
    pub fn start(self) -> HostHandle {
        let config = self.config.clone();
        let (command_tx, command_rx) = mpsc::channel(HOST_COMMAND_QUEUE);
        let (event_tx, event_rx) = mpsc::channel(HOST_EVENT_QUEUE);
        let task = tokio::spawn(self.run(command_rx, event_tx));
        HostHandle {
            config,
            commands: command_tx,
            events: event_rx,
            task,
        }
    }

    async fn run(
        mut self,
        mut commands: mpsc::Receiver<HostCommand>,
        events: mpsc::Sender<HostEvent>,
    ) {
        // The service tick is absolute, not restarted per iteration: a busy
        // command channel must never starve the socket, or relayed frames sit
        // unread (the regression documented in `session.rs:518-531`).
        let mut service = tokio::time::interval(HOST_SERVICE_TICK.max(Duration::from_millis(1)));
        service.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut heartbeat_timer = (!self.config.heartbeat.is_zero()).then(|| {
            let mut timer = tokio::time::interval(self.config.heartbeat);
            timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
            timer
        });

        loop {
            tokio::select! {
                biased;
                _ = service.tick() => {
                    match self.pump(&events).await {
                        Ok(true) => {}
                        // The peer channel or the event receiver is gone; the
                        // session is over either way.
                        Ok(false) | Err(_) => break,
                    }
                }
                _ = next_heartbeat(&mut heartbeat_timer) => {
                    self.broadcast_heartbeat();
                }
                command = commands.recv() => {
                    match command {
                        Some(HostCommand::Send { to, frame }) => self.send_frame(to, &frame),
                        Some(HostCommand::PublishHeartbeat(frame)) => {
                            send_to_verified(&mut self.host, &self.peers, None, &frame);
                            self.heartbeat_frame = Some(frame);
                        }
                        Some(HostCommand::Flush) => self.host.flush(),
                        Some(HostCommand::Shutdown) | None => break,
                    }
                }
            }
        }

        self.disconnect_all();
    }

    /// Drains ENet events; `Ok(false)` means the event receiver is gone.
    async fn pump(&mut self, events: &mpsc::Sender<HostEvent>) -> Result<bool, HostError> {
        loop {
            let Some(event) = self.host.service(0)? else {
                return Ok(true);
            };
            match event.kind {
                EventKind::Connect => {
                    let Some(peer) = event.peer else {
                        continue;
                    };
                    if !self.accept(peer, events).await {
                        return Ok(false);
                    }
                }
                EventKind::Disconnect => {
                    let Some(peer) = event.peer else {
                        continue;
                    };
                    if !self.disconnect(peer, events).await {
                        return Ok(false);
                    }
                }
                EventKind::Receive => {
                    let (Some(peer), Some(payload)) = (event.peer, event.payload.as_deref()) else {
                        continue;
                    };
                    let delivered = match event.channel {
                        CHANNEL_TP => self.process_tp(peer, payload, events).await,
                        CHANNEL_APPL => self.process_appl(peer, payload, events).await,
                        _ => true,
                    };
                    if !delivered {
                        return Ok(false);
                    }
                }
                EventKind::None => {}
            }
        }
    }

    /// Registers a new ENet connection and sends the local `TP 00`.
    async fn accept(&mut self, peer: Peer, events: &mpsc::Sender<HostEvent>) -> bool {
        if self.peers.len() >= self.config.max_peers {
            // No event: the peer never entered the roster.
            self.host.disconnect_now(peer, 0);
            return true;
        }

        // The C++ server answers CONNECT with TP 00 immediately
        // (`transport_udp_default.cpp:989-992`), so the guest can validate us
        // without a round trip.
        if self.host.send(peer, CHANNEL_TP, &self.hello).is_err() {
            self.host.disconnect_now(peer, 0);
            return true;
        }
        self.host.flush();

        let id = PeerId(self.next_peer_id);
        self.next_peer_id = self.next_peer_id.wrapping_add(1);
        self.peers.push(PeerEntry {
            id,
            peer,
            verified: false,
        });

        events
            .send(HostEvent {
                peer: id,
                kind: HostEventKind::Connected,
            })
            .await
            .is_ok()
    }

    /// Removes a peer that ENet reported as disconnected.
    async fn disconnect(&mut self, peer: Peer, events: &mpsc::Sender<HostEvent>) -> bool {
        match self.index_of(peer) {
            // A peer we already dropped (rejected handshake / incoming TP 03)
            // must not produce a second `Disconnected` event.
            None => true,
            Some(index) => self.drop_peer(index, events).await,
        }
    }

    /// `TP 00`/`TP 03` of one peer (`protocol.md` §7, `session.rs:411-484`).
    async fn process_tp(
        &mut self,
        peer: Peer,
        payload: &[u8],
        events: &mpsc::Sender<HostEvent>,
    ) -> bool {
        let Some(index) = self.index_of(peer) else {
            return true;
        };
        match TransportPacket::decode(payload) {
            Ok(TransportPacket::Hello(remote)) => {
                if self.peers[index].verified {
                    // A repeated TP 00 from a verified peer is a no-op.
                    return true;
                }
                // Version/enet/protocol/data-hash checks of `protocol.md` §7.
                // H1 carries no session ticket (`HostConfig` has none): the
                // empty-ticket server accepts every guest, so ticket matching
                // is lobby policy (H2).
                if let Err(reason) = self.local_handshake.validate_peer(&remote) {
                    let reject = Reject::new(&self.local_handshake, reason);
                    if let Ok(bytes) = reject.encode() {
                        let _ = self.host.send(peer, CHANNEL_TP, &bytes);
                    }
                    // The link is dropped after the rejection is flushed.
                    self.host.flush();
                    self.host.disconnect_later(peer, 0);
                    return self.drop_peer(index, events).await;
                }

                self.peers[index].verified = true;
                let id = self.peers[index].id;

                // A late joiner learns the current epoch/turn immediately;
                // later heartbeats follow on the repeat timer.
                if let Some(frame) = self.heartbeat_frame.as_deref() {
                    let _ = self.host.send(peer, CHANNEL_APPL, frame);
                    self.host.flush();
                }

                events
                    .send(HostEvent {
                        peer: id,
                        kind: HostEventKind::Verified,
                    })
                    .await
                    .is_ok()
            }
            Ok(TransportPacket::Reject(_)) => {
                // The guest refused our TP 00 (its version/data check failed);
                // there is nothing to negotiate, so drop the link.
                self.host.disconnect_later(peer, 0);
                self.host.flush();
                self.drop_peer(index, events).await
            }
            Err(_) => {
                // A malformed channel-0 frame is a per-packet error, exactly
                // like the C++ `ProtocolErrorMessage` path: log-free ignore.
                true
            }
        }
    }

    /// APPL frame of one peer: decode, emit, copy-forward (except the source).
    async fn process_appl(
        &mut self,
        peer: Peer,
        payload: &[u8],
        events: &mpsc::Sender<HostEvent>,
    ) -> bool {
        let Some(index) = self.index_of(peer) else {
            return true;
        };
        // `protocol.md` §7: APPL frames from an unverified peer are dropped.
        if !self.peers[index].verified {
            return true;
        }
        let source = self.peers[index].id;

        // Packet 55 is host -> guest transport data; a guest must not inject
        // it and it is never relayed.
        if payload.first() == Some(&HOST_HEARTBEAT_TYPE) {
            return true;
        }

        // Invalid frames are not relayed: only well-formed APPL data is part
        // of the global order (`PLAN-host-mode.md` §2).
        let Ok(packet) = Packet::decode(payload) else {
            return true;
        };

        if events
            .send(HostEvent {
                peer: source,
                kind: HostEventKind::Packet(packet),
            })
            .await
            .is_err()
        {
            return false;
        }

        // Copy-forward the original bytes in receive order; the sender is
        // excluded (`remote.cpp:826-862`, REMOTE_MULTICAST).
        send_to_verified(&mut self.host, &self.peers, Some(source), payload);
        true
    }

    /// Sends an already encoded frame to a command destination.
    fn send_frame(&mut self, to: Recipient, frame: &[u8]) {
        match to {
            Recipient::Peer(id) => {
                if let Some(entry) = self.peers.iter().find(|entry| entry.id == id) {
                    if entry.verified {
                        let _ = self.host.send(entry.peer, CHANNEL_APPL, frame);
                        self.host.flush();
                    }
                }
            }
            Recipient::Broadcast => {
                send_to_verified(&mut self.host, &self.peers, None, frame);
            }
        }
    }

    /// Repeats the last published heartbeat to every verified peer.
    fn broadcast_heartbeat(&mut self) {
        let Some(frame) = self.heartbeat_frame.as_deref() else {
            return;
        };
        send_to_verified(&mut self.host, &self.peers, None, frame);
    }

    /// Graceful disconnect of every accepted peer (best effort).
    fn disconnect_all(&mut self) {
        for entry in &self.peers {
            self.host.disconnect_later(entry.peer, 0);
        }
        self.host.flush();
    }

    fn index_of(&self, peer: Peer) -> Option<usize> {
        let key = peer.key();
        self.peers.iter().position(|entry| entry.peer.key() == key)
    }

    /// Removes `index` and emits its single `Disconnected` event.
    async fn drop_peer(&mut self, index: usize, events: &mpsc::Sender<HostEvent>) -> bool {
        let entry = self.peers.remove(index);
        events
            .send(HostEvent {
                peer: entry.id,
                kind: HostEventKind::Disconnected,
            })
            .await
            .is_ok()
    }
}

/// Sends `frame` to every verified peer, optionally skipping `except`.
///
/// Free function taking the fields separately: it lets callers hold a borrow
/// of `heartbeat_frame`/`payload` while mutably borrowing the ENet host.
fn send_to_verified(host: &mut Host, peers: &[PeerEntry], except: Option<PeerId>, frame: &[u8]) {
    let mut sent = false;
    for entry in peers {
        if !entry.verified || Some(entry.id) == except {
            continue;
        }
        // Best effort: a failed enqueue belongs to a peer whose disconnect is
        // already queued/serviced by ENet.
        let _ = host.send(entry.peer, CHANNEL_APPL, frame);
        sent = true;
    }
    if sent {
        host.flush();
    }
}

/// Awaits the heartbeat timer, or never resolves when heartbeats are disabled.
async fn next_heartbeat(timer: &mut Option<tokio::time::Interval>) {
    match timer {
        Some(timer) => {
            timer.tick().await;
        }
        None => std::future::pending::<()>().await,
    }
}

/// Handle of a running [`HostSession`].
#[derive(Debug)]
pub struct HostHandle {
    config: HostConfig,
    commands: mpsc::Sender<HostCommand>,
    events: mpsc::Receiver<HostEvent>,
    task: JoinHandle<()>,
}

impl HostHandle {
    /// Binds and starts the host on the current tokio runtime.
    pub fn bind(config: HostConfig) -> Result<Self, HostError> {
        Ok(HostSession::bind(config)?.start())
    }

    /// Host configuration.
    pub const fn config(&self) -> &HostConfig {
        &self.config
    }

    /// Encodes and queues an APPL packet for one peer or for broadcast.
    ///
    /// Packets are addressed explicitly: the host's own game traffic is not
    /// relayed (`PLAN-host-mode.md` §2 — the host driver calls
    /// `send(Recipient::Broadcast, …)`). A peer that is unknown or not yet
    /// verified is skipped by the session task; encoding errors surface here.
    pub async fn send(&self, to: impl Into<Recipient>, packet: Packet) -> Result<(), HostError> {
        let frame = packet.encode()?;
        self.commands
            .send(HostCommand::Send {
                to: to.into(),
                frame,
            })
            .await
            .map_err(|_| HostError::Closed)
    }

    /// Publishes the heartbeat: broadcasts it now and repeats it on
    /// [`HostConfig::heartbeat`] until a newer heartbeat is published.
    pub async fn publish_heartbeat(&self, heartbeat: HostHeartbeat) -> Result<(), HostError> {
        self.commands
            .send(HostCommand::PublishHeartbeat(heartbeat.encode()))
            .await
            .map_err(|_| HostError::Closed)
    }

    /// Flushes queued ENet packets (sends also flush; this is for the driver's
    /// own batching points).
    pub async fn flush(&self) -> Result<(), HostError> {
        self.commands
            .send(HostCommand::Flush)
            .await
            .map_err(|_| HostError::Closed)
    }

    /// Waits for the next host event; `None` when the task ended.
    pub async fn next_event(&mut self) -> Option<HostEvent> {
        self.events.recv().await
    }

    /// Receives with a timeout; `Ok(None)` means the task ended.
    pub async fn next_event_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<HostEvent>, HostError> {
        match tokio::time::timeout(timeout, self.events.recv()).await {
            Ok(event) => Ok(event),
            Err(_) => Err(HostError::Timeout {
                stage: "event",
                timeout,
            }),
        }
    }

    /// Closes the session, disconnecting every peer.
    pub async fn shutdown(mut self) {
        let _ = self.commands.send(HostCommand::Shutdown).await;
        if tokio::time::timeout(HOST_SHUTDOWN_TIMEOUT, &mut self.task)
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

impl Drop for HostHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enet;
    use crate::handshake::{HandshakeError, RejectReason};
    use crate::session::{Session, SessionConfig, SessionError, SessionEvent, SessionHandle};
    use max_types::input::{Command, Packet18Chat};
    use max_types::team::TeamId;
    use std::net::UdpSocket;
    use tokio::time::Instant;

    /// Blocking service timeout of the raw (non-tokio) test clients.
    const RAW_SERVICE_MS: u32 = 10;

    fn data_hash() -> DataHash {
        DataHash::from_bytes(b"max", b"patches")
    }

    fn wrong_data_hash() -> DataHash {
        DataHash::from_bytes(b"other", b"patches")
    }

    /// An ephemeral UDP port announced by the OS (same probe as `session.rs`'s
    /// integration test).
    fn free_udp_port() -> u16 {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe socket");
        socket.local_addr().expect("probe addr").port()
    }

    fn start_host(port: u16, heartbeat: Duration) -> HostHandle {
        let config = HostConfig {
            bind_ip: Ipv4Addr::LOCALHOST,
            port,
            game_version: DEFAULT_GAME_VERSION,
            data_hash: data_hash(),
            max_peers: DEFAULT_MAX_PEERS,
            heartbeat,
        };
        HostHandle::bind(config).expect("bind host")
    }

    fn chat(sender: TeamId, message: &str) -> Packet {
        Packet::Command(Command::Chat(Packet18Chat {
            sender,
            message: message.to_string(),
        }))
    }

    async fn connect_guest(port: u16, hash: DataHash) -> SessionHandle {
        let config = SessionConfig::new("127.0.0.1", port, "", hash);
        Session::connect(config)
            .await
            .expect("guest TP 00 exchange")
            .start()
    }

    async fn wait_host_event(host: &mut HostHandle, timeout: Duration) -> HostEvent {
        host.next_event_timeout(timeout)
            .await
            .expect("host event timeout")
            .expect("host task ended")
    }

    async fn wait_verified(host: &mut HostHandle, timeout: Duration) -> PeerId {
        loop {
            let event = wait_host_event(host, timeout).await;
            if let HostEventKind::Verified = event.kind {
                return event.peer;
            }
        }
    }

    async fn wait_host_packet(host: &mut HostHandle, timeout: Duration) -> (PeerId, Packet) {
        loop {
            let event = wait_host_event(host, timeout).await;
            if let HostEventKind::Packet(packet) = event.kind {
                return (event.peer, packet);
            }
        }
    }

    /// Asserts that no `Packet` event arrives within `timeout`.
    async fn assert_no_host_packet(host: &mut HostHandle, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return;
            }
            match host.next_event_timeout(remaining).await {
                Ok(Some(event)) => {
                    if let HostEventKind::Packet(packet) = event.kind {
                        panic!("unexpected host packet from {}: {packet:?}", event.peer);
                    }
                }
                Ok(None) => panic!("host task ended"),
                Err(_) => return,
            }
        }
    }

    async fn wait_guest_packet(client: &mut SessionHandle, timeout: Duration) -> Packet {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "guest packet did not arrive within {timeout:?}"
            );
            match client
                .next_event_timeout(remaining)
                .await
                .expect("guest event timeout")
            {
                Some(SessionEvent::Packet(packet)) => return packet,
                Some(_) => continue,
                None => panic!("guest task ended"),
            }
        }
    }

    async fn assert_no_guest_packet(client: &mut SessionHandle, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return;
            }
            match client.next_event_timeout(remaining).await {
                Ok(Some(SessionEvent::Packet(packet))) => {
                    panic!("unexpected guest packet: {packet:?}")
                }
                Ok(Some(_)) => continue,
                Ok(None) => panic!("guest task ended"),
                Err(_) => return,
            }
        }
    }

    /// Raw ENet client (no `Session`), used where the test must control the
    /// transport handshake byte by byte.
    fn raw_client(port: u16) -> Host {
        let mut host = Host::client(CHANNEL_COUNT).expect("raw ENet client");
        let address = enet::address(Ipv4Addr::LOCALHOST, port).expect("server address");
        host.connect(address, CHANNEL_COUNT).expect("raw connect");
        host.flush();
        host
    }

    fn wait_raw_connect(host: &mut Host, timeout: Duration) -> Peer {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let Some(event) = host.service(RAW_SERVICE_MS).expect("raw service") else {
                continue;
            };
            match event.kind {
                EventKind::Connect => return event.peer.expect("connect event has a peer"),
                EventKind::Disconnect => panic!("raw client disconnected during connect"),
                _ => {}
            }
        }
        panic!("ENet connect did not complete within {timeout:?}");
    }

    fn wait_tp_hello(host: &mut Host, timeout: Duration) -> Handshake {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let Some(event) = host.service(RAW_SERVICE_MS).expect("raw service") else {
                continue;
            };
            if event.kind == EventKind::Receive && event.channel == CHANNEL_TP {
                let payload = event.payload.expect("receive payload");
                if let Ok(TransportPacket::Hello(hello)) = TransportPacket::decode(&payload) {
                    return hello;
                }
            }
            if event.kind == EventKind::Disconnect {
                panic!("raw client disconnected before the host TP 00");
            }
        }
        panic!("host TP 00 did not arrive within {timeout:?}");
    }

    fn wait_tp_reject(host: &mut Host, timeout: Duration) -> Reject {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let Some(event) = host.service(RAW_SERVICE_MS).expect("raw service") else {
                continue;
            };
            if event.kind == EventKind::Receive && event.channel == CHANNEL_TP {
                let payload = event.payload.expect("receive payload");
                if let Ok(TransportPacket::Reject(reject)) = TransportPacket::decode(&payload) {
                    return reject;
                }
            }
            if event.kind == EventKind::Disconnect {
                panic!("raw client disconnected before TP 03");
            }
        }
        panic!("TP 03 did not arrive within {timeout:?}");
    }

    fn collect_appl_frames(host: &mut Host, count: usize, timeout: Duration) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        let deadline = std::time::Instant::now() + timeout;
        while frames.len() < count && std::time::Instant::now() < deadline {
            let Some(event) = host.service(RAW_SERVICE_MS).expect("raw service") else {
                continue;
            };
            if event.kind == EventKind::Receive && event.channel == CHANNEL_APPL {
                frames.push(event.payload.expect("receive payload"));
            }
        }
        assert_eq!(frames.len(), count, "expected {count} relayed APPL frames");
        frames
    }

    /// (a) The host accepts N peers and both `TP 00` directions are exercised:
    /// the guest validates the host's copy, and a guest with a mismatched data
    /// hash is answered with `TP 03` (reason 3) and removed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn host_accepts_three_guests_and_rejects_a_mismatched_tp00() {
        let port = free_udp_port();
        let mut host = start_host(port, Duration::ZERO);

        let mut guests = Vec::new();
        let mut ids = Vec::new();
        for _ in 0..3 {
            guests.push(connect_guest(port, data_hash()).await);
            ids.push(wait_verified(&mut host, Duration::from_secs(5)).await);
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 3, "each accepted peer gets its own id");

        // Server -> client: the raw client reads the host's TP 00 (correct
        // version/data), then presents a wrong data hash.
        let mut raw = raw_client(port);
        let peer = wait_raw_connect(&mut raw, Duration::from_secs(5));
        let hello = wait_tp_hello(&mut raw, Duration::from_secs(5));
        assert_eq!(hello.game_version, DEFAULT_GAME_VERSION);
        assert_eq!(hello.enet_version, crate::handshake::ENET_VERSION);
        assert_eq!(hello.protocol_version, crate::handshake::PROTOCOL_VERSION);
        assert_eq!(hello.data_hash, data_hash().to_hex());
        assert_eq!(hello.ticket, "", "the host advertises an empty ticket");

        let wrong = Handshake::new(DEFAULT_GAME_VERSION, "", wrong_data_hash());
        raw.send(peer, CHANNEL_TP, &wrong.encode().expect("wrong TP 00"))
            .expect("send wrong TP 00");
        raw.flush();
        let reject = wait_tp_reject(&mut raw, Duration::from_secs(5));
        assert_eq!(reject.reason, RejectReason::GameData);
        assert_eq!(reject.reason_text, "game data");

        // The host dropped the mismatched peer: exactly one Connected
        // (before any of its frames) followed by one Disconnected, never
        // Verified.
        let mut rejected_id = None;
        loop {
            let event = wait_host_event(&mut host, Duration::from_secs(5)).await;
            match event.kind {
                HostEventKind::Connected => {
                    assert!(rejected_id.is_none(), "second Connected for one peer");
                    rejected_id = Some(event.peer);
                }
                HostEventKind::Disconnected => {
                    assert_eq!(
                        Some(event.peer),
                        rejected_id,
                        "Disconnected before Connected"
                    );
                    break;
                }
                HostEventKind::Verified => panic!("a mismatched TP 00 must not verify"),
                HostEventKind::Packet(_) => {}
            }
        }

        // Client -> server: the unchanged `Session` guest rejects the host's
        // TP 00 on its own, sends TP 03 and surfaces the reason; the host must
        // drop that peer too (exactly one more Connected/Disconnected pair).
        let mismatched =
            Session::connect(SessionConfig::new("127.0.0.1", port, "", wrong_data_hash())).await;
        match mismatched {
            Err(SessionError::Handshake(HandshakeError::Rejected { reason, .. })) => {
                assert_eq!(reason, RejectReason::GameData)
            }
            other => panic!("expected a rejected handshake, got {other:?}"),
        }
        let mut rejected_id = None;
        loop {
            let event = wait_host_event(&mut host, Duration::from_secs(5)).await;
            match event.kind {
                HostEventKind::Connected => rejected_id = Some(event.peer),
                HostEventKind::Disconnected => {
                    assert_eq!(Some(event.peer), rejected_id);
                    break;
                }
                HostEventKind::Verified => panic!("a mismatched TP 00 must not verify"),
                HostEventKind::Packet(_) => {}
            }
        }

        for guest in guests {
            guest.shutdown().await;
        }
        host.shutdown().await;
    }

    /// (b) A verified guest's frame reaches every other verified guest and
    /// never the source ("multicast except source").
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn relay_reaches_every_verified_guest_except_the_source() {
        let port = free_udp_port();
        let mut host = start_host(port, Duration::ZERO);

        let mut guests = Vec::new();
        let mut ids = Vec::new();
        for _ in 0..3 {
            guests.push(connect_guest(port, data_hash()).await);
            ids.push(wait_verified(&mut host, Duration::from_secs(5)).await);
        }

        let frame = chat(TeamId::RED, "relay");
        guests[0].send(frame.clone()).await.expect("send chat");

        let (source, packet) = wait_host_packet(&mut host, Duration::from_secs(5)).await;
        assert_eq!(source, ids[0], "host sees the producer id");
        assert_eq!(packet, frame);

        for guest in guests.iter_mut().skip(1) {
            assert_eq!(
                wait_guest_packet(guest, Duration::from_secs(5)).await,
                frame,
                "relayed copy is unchanged"
            );
        }
        assert_no_guest_packet(&mut guests[0], Duration::from_millis(300)).await;

        for guest in guests {
            guest.shutdown().await;
        }
        host.shutdown().await;
    }

    /// (c) Relay is FIFO and byte-exact: a raw receiver sees the sender's
    /// encoded frames in order, with no re-encoding.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn relay_preserves_fifo_order_and_exact_bytes() {
        let port = free_udp_port();
        let mut host = start_host(port, Duration::ZERO);

        let sender = connect_guest(port, data_hash()).await;
        let _sender_id = wait_verified(&mut host, Duration::from_secs(5)).await;

        // Raw receiver: no Session decoding, so the test compares wire bytes.
        let mut receiver = raw_client(port);
        let receiver_peer = wait_raw_connect(&mut receiver, Duration::from_secs(5));
        receiver
            .send(
                receiver_peer,
                CHANNEL_TP,
                &Handshake::new(DEFAULT_GAME_VERSION, "", data_hash())
                    .encode()
                    .expect("TP 00"),
            )
            .expect("send TP 00");
        receiver.flush();
        let _receiver_id = wait_verified(&mut host, Duration::from_secs(5)).await;

        let mut expected = Vec::new();
        for counter in 0..5 {
            let frame = chat(TeamId::GREEN, &format!("fifo-{counter}"));
            let bytes = frame.encode().expect("encode chat");
            sender.send(frame).await.expect("send chat");
            expected.push(bytes);
        }

        let received = collect_appl_frames(&mut receiver, expected.len(), Duration::from_secs(5));
        assert_eq!(received, expected, "relay keeps bytes and order");

        sender.shutdown().await;
        host.shutdown().await;
    }

    /// (d) APPL frames from a peer that has not passed `TP 00` are dropped;
    /// once verified, the same peer's frames are accepted and relayed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn appl_from_unverified_peer_is_dropped() {
        let port = free_udp_port();
        let mut host = start_host(port, Duration::ZERO);

        let mut witness = connect_guest(port, data_hash()).await;
        let _witness_id = wait_verified(&mut host, Duration::from_secs(5)).await;

        let mut unverified = raw_client(port);
        let unverified_peer = wait_raw_connect(&mut unverified, Duration::from_secs(5));

        // APPL before TP 00: the host must neither emit nor relay it.
        let early = chat(TeamId::RED, "before-tp00");
        unverified
            .send(
                unverified_peer,
                CHANNEL_APPL,
                &early.encode().expect("encode early chat"),
            )
            .expect("send early APPL");
        unverified.flush();

        // Wait for the host's Connected event of this peer first, then make
        // sure no Packet event follows within the window.
        let unverified_id = loop {
            let event = wait_host_event(&mut host, Duration::from_secs(5)).await;
            if let HostEventKind::Connected = event.kind {
                break event.peer;
            }
        };
        assert_no_host_packet(&mut host, Duration::from_millis(300)).await;
        assert_no_guest_packet(&mut witness, Duration::from_millis(300)).await;

        // Verify the same peer, then send the same frame again.
        unverified
            .send(
                unverified_peer,
                CHANNEL_TP,
                &Handshake::new(DEFAULT_GAME_VERSION, "", data_hash())
                    .encode()
                    .expect("TP 00"),
            )
            .expect("send TP 00");
        unverified.flush();
        assert_eq!(
            wait_verified(&mut host, Duration::from_secs(5)).await,
            unverified_id
        );

        let (source, packet) = {
            unverified
                .send(
                    unverified_peer,
                    CHANNEL_APPL,
                    &early.encode().expect("encode chat"),
                )
                .expect("send verified APPL");
            unverified.flush();
            wait_host_packet(&mut host, Duration::from_secs(5)).await
        };
        assert_eq!(source, unverified_id);
        assert_eq!(packet, early);
        assert_eq!(
            wait_guest_packet(&mut witness, Duration::from_secs(5)).await,
            early,
            "a verified frame is relayed to the other guests"
        );

        witness.shutdown().await;
        host.shutdown().await;
    }

    /// `HostHandle::send` addresses one peer or broadcasts; unverified peers
    /// are skipped.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn host_send_unicast_and_broadcast_reach_verified_guests() {
        let port = free_udp_port();
        let mut host = start_host(port, Duration::ZERO);

        let mut guest_a = connect_guest(port, data_hash()).await;
        let id_a = wait_verified(&mut host, Duration::from_secs(5)).await;
        let mut guest_b = connect_guest(port, data_hash()).await;
        let id_b = wait_verified(&mut host, Duration::from_secs(5)).await;
        assert_ne!(id_a, id_b);

        let unicast = chat(TeamId::RED, "only-b");
        host.send(id_b, unicast.clone()).await.expect("unicast");
        host.flush().await.expect("flush");
        assert_eq!(
            wait_guest_packet(&mut guest_b, Duration::from_secs(5)).await,
            unicast
        );
        assert_no_guest_packet(&mut guest_a, Duration::from_millis(300)).await;

        let broadcast = chat(TeamId::RED, "everyone");
        host.send(Recipient::Broadcast, broadcast.clone())
            .await
            .expect("broadcast");
        assert_eq!(
            wait_guest_packet(&mut guest_a, Duration::from_secs(5)).await,
            broadcast
        );
        assert_eq!(
            wait_guest_packet(&mut guest_b, Duration::from_secs(5)).await,
            broadcast
        );

        guest_a.shutdown().await;
        guest_b.shutdown().await;
        host.shutdown().await;
    }

    /// An incoming `TP 03` (the guest refuses our copy) drops the peer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn incoming_tp03_drops_the_peer() {
        let port = free_udp_port();
        let mut host = start_host(port, Duration::ZERO);

        let mut raw = raw_client(port);
        let peer = wait_raw_connect(&mut raw, Duration::from_secs(5));
        raw.send(
            peer,
            CHANNEL_TP,
            &Handshake::new(DEFAULT_GAME_VERSION, "", data_hash())
                .encode()
                .expect("TP 00"),
        )
        .expect("send TP 00");
        raw.flush();
        let id = wait_verified(&mut host, Duration::from_secs(5)).await;

        let reject = Reject::new(
            &Handshake::new(DEFAULT_GAME_VERSION, "", data_hash()),
            RejectReason::Version,
        );
        raw.send(peer, CHANNEL_TP, &reject.encode().expect("TP 03"))
            .expect("send TP 03");
        raw.flush();

        let event = wait_host_event(&mut host, Duration::from_secs(5)).await;
        assert_eq!(event.peer, id);
        assert!(
            matches!(event.kind, HostEventKind::Disconnected),
            "a TP 03 removes the peer: {:?}",
            event.kind
        );

        host.shutdown().await;
    }

    /// Packet 55 is a transport frame with its own wire type, not a `Packet`.
    #[test]
    fn heartbeat_frame_is_packet_55_and_round_trips() {
        assert_eq!(HOST_HEARTBEAT_PACKET_NUMBER, 55);
        assert_eq!(HOST_HEARTBEAT_TYPE, 0x87);
        assert_eq!(HOST_HEARTBEAT_TYPE, TRANSPORT_APPL_PACKET_ID + 55);

        let heartbeat = HostHeartbeat {
            host_epoch: 3,
            turn_counter: 7,
            phase: 2,
            reachable: 0b101,
        };
        let frame = heartbeat.encode();
        assert_eq!(
            frame,
            [0x87, 3, 0, 0, 0, 7, 0, 0, 0, 2, 5, 0, 0, 0],
            "type, epoch u32, turn i32, phase u8, reachable u32 — all LE"
        );
        assert_eq!(HostHeartbeat::decode(&frame).expect("decode"), heartbeat);

        let mut wrong_type = frame.clone();
        wrong_type[0] = 0x86;
        assert!(HostHeartbeat::decode(&wrong_type).is_err());

        assert!(
            HostHeartbeat::decode(&frame[..frame.len() - 1]).is_err(),
            "truncated body"
        );

        let mut trailing = frame.clone();
        trailing.push(0);
        assert!(HostHeartbeat::decode(&trailing).is_err(), "trailing bytes");
    }

    /// The published heartbeat reaches guests immediately and is repeated on
    /// the configured period; a late joiner receives the current one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn published_heartbeat_reaches_guests_and_repeats() {
        let port = free_udp_port();
        let mut host = start_host(port, Duration::from_millis(100));

        let mut guest = connect_guest(port, data_hash()).await;
        let guest_id = wait_verified(&mut host, Duration::from_secs(5)).await;

        let heartbeat = HostHeartbeat {
            host_epoch: 1,
            turn_counter: 0,
            phase: 1,
            reachable: 0b1,
        };
        host.publish_heartbeat(heartbeat)
            .await
            .expect("publish heartbeat");

        // `Session` (unchanged, channel 1) decodes packet 55 as an unknown
        // APPL kind with the body preserved.
        let mut received = 0;
        let deadline = Instant::now() + Duration::from_secs(2);
        while received < 2 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "heartbeat was not repeated on the configured period"
            );
            match guest
                .next_event_timeout(remaining)
                .await
                .expect("guest event timeout")
            {
                Some(SessionEvent::Packet(Packet::Unknown { kind, payload })) => {
                    assert_eq!(kind, HOST_HEARTBEAT_PACKET_NUMBER);
                    let mut frame = vec![HOST_HEARTBEAT_TYPE];
                    frame.extend_from_slice(&payload);
                    assert_eq!(
                        HostHeartbeat::decode(&frame).expect("heartbeat body"),
                        heartbeat
                    );
                    received += 1;
                }
                Some(_) => {}
                None => panic!("guest task ended"),
            }
        }

        // A guest that verifies after the publish gets the current heartbeat.
        let mut late = connect_guest(port, data_hash()).await;
        let late_id = wait_verified(&mut host, Duration::from_secs(5)).await;
        assert_ne!(late_id, guest_id);
        match wait_guest_packet(&mut late, Duration::from_secs(5)).await {
            Packet::Unknown { kind, payload } => {
                assert_eq!(kind, HOST_HEARTBEAT_PACKET_NUMBER);
                let mut frame = vec![HOST_HEARTBEAT_TYPE];
                frame.extend_from_slice(&payload);
                assert_eq!(
                    HostHeartbeat::decode(&frame).expect("late heartbeat"),
                    heartbeat
                );
            }
            other => panic!("expected the heartbeat for the late joiner, got {other:?}"),
        }

        guest.shutdown().await;
        late.shutdown().await;
        host.shutdown().await;
    }

    #[test]
    fn invalid_config_is_rejected_before_binding() {
        let mut config = HostConfig {
            port: 0,
            ..HostConfig::default()
        };
        config.max_peers = 0;
        assert!(matches!(
            HostSession::bind(config),
            Err(HostError::Config(_))
        ));

        let mut config = HostConfig {
            port: 0,
            ..HostConfig::default()
        };
        config.max_peers = PEER_COUNT + 1;
        assert!(matches!(
            HostHandle::bind(config),
            Err(HostError::Config(_))
        ));

        // A valid configuration binds an ephemeral port without a runtime.
        let session = HostSession::bind(HostConfig::new(Ipv4Addr::LOCALHOST, 0, data_hash()))
            .expect("bind ephemeral host");
        assert_eq!(session.local_handshake().game_version, DEFAULT_GAME_VERSION);
        assert_eq!(session.local_handshake().ticket, "");
        assert_eq!(session.config().port, 0);
    }

    /// Compile-time guarantee that the host handle is usable from a spawned
    /// task (the session owns the ENet host, which is `Send` but not `Sync`).
    #[test]
    fn host_handle_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<HostHandle>();
        assert_send::<HostSession>();
        assert_send::<HostEvent>();
        assert_send::<HostHeartbeat>();
    }
}
