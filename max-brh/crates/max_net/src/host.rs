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
/// APPL number of the takeover negotiation (`0x50 + 56` on the wire).
pub const HOST_TAKEOVER_PACKET_NUMBER: u8 = 56;
/// Wire type byte of the takeover negotiation (`0x88`).
pub const HOST_TAKEOVER_TYPE: u8 = TRANSPORT_APPL_PACKET_ID + HOST_TAKEOVER_PACKET_NUMBER;
/// Default `host_timeout` (`PLAN-host-mode.md` §6.2): a guest that hears no
/// heartbeat for this long considers the host failed.
pub const DEFAULT_HOST_TIMEOUT: Duration = Duration::from_secs(90);
/// Minimum accepted `host_timeout`.
pub const HOST_TIMEOUT_MIN: Duration = Duration::from_secs(5);
/// Maximum accepted `host_timeout`.
pub const HOST_TIMEOUT_MAX: Duration = Duration::from_secs(300);
/// Maximum roster entries a takeover frame may carry (the four team slots).
pub const TAKEOVER_MAX_ROSTER: usize = 4;

/// Rejects a `host_timeout` outside [`HOST_TIMEOUT_MIN`]..=[`HOST_TIMEOUT_MAX`].
///
/// `host_timeout` is deliberately **not** the `elimination_timeout`
/// (`PLAN-host-mode.md` §6.2): losing the host means a pause/migration, losing a
/// guest means a `SessionPolicy` decision.
pub fn validate_host_timeout(timeout: Duration) -> Result<(), HostError> {
    if (HOST_TIMEOUT_MIN..=HOST_TIMEOUT_MAX).contains(&timeout) {
        Ok(())
    } else {
        Err(HostError::Config(
            "host_timeout must be within 5..=300 seconds",
        ))
    }
}
/// ENet service cadence of the accept loop (C++ `ServiceTickPeriod`, 10 ms).
const HOST_SERVICE_TICK: Duration = Duration::from_millis(10);
/// Depth of the command channel ([`HostHandle`] → session task).
const HOST_COMMAND_QUEUE: usize = 64;
/// Depth of the event channel (session task → [`HostHandle`]).
const HOST_EVENT_QUEUE: usize = 256;
/// How long [`HostHandle::shutdown`] waits for the session task to end.
const HOST_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// Wire size of an encoded [`HostHeartbeat`] (type + body): 64 bytes.
const HOST_HEARTBEAT_WIRE_LEN: usize = 64;

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
    /// How long a guest may hear nothing from the host before it declares the
    /// host failed (H4 migration, `PLAN-host-mode.md` §4).
    pub host_timeout: Duration,
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
            host_timeout: DEFAULT_HOST_TIMEOUT,
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
            host_timeout: DEFAULT_HOST_TIMEOUT,
        }
    }

    /// Rejects values ENet or the transport cannot honour.
    fn validate(&self) -> Result<(), HostError> {
        if self.max_peers == 0 || self.max_peers > PEER_COUNT {
            return Err(HostError::Config(
                "max_peers must be within 1..=PEER_COUNT (32)",
            ));
        }
        validate_host_timeout(self.host_timeout)?;
        Ok(())
    }
}

/// Session policy carried by the heartbeat and the takeover frame
/// (`PLAN-host-mode.md` §4, §6.1).
///
/// This is the transport projection of the application-level `SessionPolicy`
/// (`max_app::host`): `Pause` is the deterministic default, `EliminateAfter`
/// removes a silent team after a number of **session ticks**
/// (`ticks_without_heartbeat`), never seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostSessionPolicy {
    /// Wait for the silent guest.
    #[default]
    Pause,
    /// Eliminate the team after `ticks` silent session ticks.
    EliminateAfter {
        /// Tick budget before elimination.
        ticks: u32,
    },
}

impl HostSessionPolicy {
    /// Wire tag (`0` — pause, `1` — eliminate).
    pub const fn tag(self) -> u8 {
        match self {
            Self::Pause => 0,
            Self::EliminateAfter { .. } => 1,
        }
    }

    /// Tick budget (`0` for `Pause`).
    pub const fn ticks(self) -> u32 {
        match self {
            Self::Pause => 0,
            Self::EliminateAfter { ticks } => ticks,
        }
    }

    /// Decodes the tag/tick pair; an unknown tag is `Pause`, a zero tick budget
    /// is clamped to one (an elimination tick always exists).
    pub const fn from_wire(tag: u8, ticks: u32) -> Self {
        match tag {
            1 => Self::EliminateAfter {
                ticks: if ticks == 0 { 1 } else { ticks },
            },
            _ => Self::Pause,
        }
    }
}

/// Transport heartbeat, APPL packet 55 (`PLAN-host-mode.md` §3, §5 H1/H4).
///
/// This is **not** a [`Packet`] variant: it is host-to-guest transport data
/// used by the migration/quorum layer (H4), so it never reaches
/// `Dispatcher::route`.
///
/// The numbered `session_tick` and the per-team `last_seen`/`eliminated_tick`
/// fields make elimination drift-independent: every client derives the same
/// elimination tick from the same heartbeat, no matter when (or in which order)
/// it receives it.
///
/// Wire layout (all little-endian, 64 bytes, type byte included):
///
/// | offset | field | type |
/// |---|---|---|
/// | 0 | type | `u8` = [`HOST_HEARTBEAT_TYPE`] (`0x87`) |
/// | 1 | `host_epoch` | `u32` |
/// | 5 | `session_tick` | `u32` (monotonic host heartbeat number) |
/// | 9 | `turn_counter` | `i32` (mirrors `GameManager_TurnCounter`) |
/// | 13 | `phase` | `u8` (opaque host barrier-FSM phase tag) |
/// | 14 | `reachable` | `u32` (bitmask of reachable player nodes) |
/// | 18 | `last_seen` | `[u32; 4]` (per team slot: last heard session tick) |
/// | 34 | `policy_tag` | `u8` |
/// | 35 | `policy_ticks` | `u32` |
/// | 39 | `host_timeout_ms` | `u32` |
/// | 43 | `quorum` | `u8` |
/// | 44 | `eliminated` | `u32` (bitmask of eliminated team slots) |
/// | 48 | `eliminated_tick` | `[u32; 4]` (per team slot: tick of elimination) |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostHeartbeat {
    /// Monotonic host epoch (bumped on takeover, H4).
    pub host_epoch: u32,
    /// Monotonic heartbeat number of this authority (starts at 1).
    pub session_tick: u32,
    /// Host turn counter at the moment of the heartbeat.
    pub turn_counter: i32,
    /// Host barrier-FSM phase; opaque to the transport.
    pub phase: u8,
    /// Bitmask of player nodes the host can currently reach (`bit n` = node
    /// `n`). The transport only carries the value; the host driver fills it
    /// from its roster.
    pub reachable: u32,
    /// Per team slot: the last `session_tick` at which that team was heard.
    pub last_seen: [u32; TAKEOVER_MAX_ROSTER],
    /// Session policy of the match.
    pub policy: HostSessionPolicy,
    /// `host_timeout` the host asks guests to use.
    pub host_timeout_ms: u32,
    /// `quorum` of the session (`floor(N/2)+1`, `PLAN-host-mode.md` §4).
    pub quorum: u8,
    /// Bitmask of eliminated team slots.
    pub eliminated: u32,
    /// Per team slot: the tick at which it was eliminated (`0` — never).
    pub eliminated_tick: [u32; TAKEOVER_MAX_ROSTER],
}

impl HostHeartbeat {
    /// Encodes the complete APPL frame (type byte included).
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(HOST_HEARTBEAT_WIRE_LEN);
        w.write_u8(HOST_HEARTBEAT_TYPE);
        w.write_u32(self.host_epoch);
        w.write_u32(self.session_tick);
        w.write_i32(self.turn_counter);
        w.write_u8(self.phase);
        w.write_u32(self.reachable);
        for tick in self.last_seen {
            w.write_u32(tick);
        }
        w.write_u8(self.policy.tag());
        w.write_u32(self.policy.ticks());
        w.write_u32(self.host_timeout_ms);
        w.write_u8(self.quorum);
        w.write_u32(self.eliminated);
        for tick in self.eliminated_tick {
            w.write_u32(tick);
        }
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
        Self::decode_body(&mut r)
    }

    /// Decodes the body that follows the type byte (already consumed).
    ///
    /// This is the shape a guest sees inside a
    /// [`Packet::Unknown`](crate::wire::Packet::Unknown) frame: the session
    /// codec strips the type byte before the application inspects the payload.
    pub fn decode_body(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let host_epoch = r.read_u32()?;
        let session_tick = r.read_u32()?;
        let turn_counter = r.read_i32()?;
        let phase = r.read_u8()?;
        let reachable = r.read_u32()?;
        let mut last_seen = [0u32; TAKEOVER_MAX_ROSTER];
        for slot in &mut last_seen {
            *slot = r.read_u32()?;
        }
        let policy_tag = r.read_u8()?;
        let policy_ticks = r.read_u32()?;
        let host_timeout_ms = r.read_u32()?;
        let quorum = r.read_u8()?;
        let eliminated = r.read_u32()?;
        let mut eliminated_tick = [0u32; TAKEOVER_MAX_ROSTER];
        for slot in &mut eliminated_tick {
            *slot = r.read_u32()?;
        }
        if !r.is_empty() {
            return Err(WireError::TrailingBytes(r.remaining()));
        }
        Ok(Self {
            host_epoch,
            session_tick,
            turn_counter,
            phase,
            reachable,
            last_seen,
            policy: HostSessionPolicy::from_wire(policy_tag, policy_ticks),
            host_timeout_ms,
            quorum,
            eliminated,
            eliminated_tick,
        })
    }

    /// Decodes the payload of a `Packet::Unknown { kind: 55, .. }`.
    pub fn decode_payload(payload: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader::new(payload);
        Self::decode_body(&mut r)
    }
}

/// Kind of a transport takeover frame (packet 56).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TakeoverKind {
    /// The successor offers its epoch and lockstep position.
    #[default]
    Proposal,
    /// A survivor agrees that its position matches (`TakeoverAck`).
    Accept,
    /// A survivor refuses: its lockstep position does not match.
    Reject,
    /// The new authority resumes the match for every accepted survivor.
    Resume,
}

impl TakeoverKind {
    /// Wire tag.
    pub const fn tag(self) -> u8 {
        match self {
            Self::Proposal => 0,
            Self::Accept => 1,
            Self::Reject => 2,
            Self::Resume => 3,
        }
    }

    /// Decodes a wire tag (`None` — unknown).
    pub const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Proposal),
            1 => Some(Self::Accept),
            2 => Some(Self::Reject),
            3 => Some(Self::Resume),
            _ => None,
        }
    }
}

/// Barrier counters carried by a takeover: the lockstep position of the
/// sender (`Remote_FrameSyncCounter`/`Remote_TurnIndices`/`Remote_StateSyncIndex`/
/// `Remote_NextTurnIndices`, own team).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TakeoverCounters {
    /// Packet 00 counter.
    pub begin: u8,
    /// Packet 52 counter.
    pub end: u8,
    /// Packet 54 index.
    pub sync: u8,
    /// Packet 45 index.
    pub crc: u8,
}

/// One roster entry of a takeover proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeoverRosterEntry {
    /// Player node id (`entity_id`).
    pub node: u16,
    /// Team slot 0..=3.
    pub team_slot: u8,
    /// Whether the node is reachable from the proposer.
    pub reachable: bool,
}

/// Transport takeover frame (APPL packet 56, `PLAN-host-mode.md` §3).
///
/// The frame is **transport only**: it never reaches `Dispatcher::route`. A
/// `Proposal` carries the new epoch, the lockstep position and the roster;
/// `Accept`/`Reject` carry the survivor's own position; `Resume` releases the
/// accepted survivors after the quorum is reached.
///
/// Wire layout (all little-endian):
///
/// | type `0x88` | kind `u8` | epoch `u32` | `turn_counter i32` | `phase u8` |
/// | `begin u8` | `end u8` | `sync u8` | `crc u8` | `node u16` | `len u8` |
/// | `(node u16, team_slot u8, reachable u8)` × len |
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Takeover {
    /// Frame kind.
    pub kind: TakeoverKind,
    /// Proposed authority epoch (`old + 1`).
    pub epoch: u32,
    /// Lockstep turn counter of the sender.
    pub turn_counter: i32,
    /// Barrier phase tag of the sender.
    pub phase: u8,
    /// Barrier counters of the sender.
    pub counters: TakeoverCounters,
    /// Sender player node.
    pub node: u16,
    /// Full roster (only a `Proposal` needs it; empty otherwise).
    pub roster: Vec<TakeoverRosterEntry>,
}

impl Takeover {
    /// Encodes the complete APPL frame (type byte included).
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(32 + self.roster.len() * 4);
        w.write_u8(HOST_TAKEOVER_TYPE);
        self.encode_body(&mut w);
        w.finish()
    }

    /// Body of the frame (type byte already written/consumed).
    pub fn encode_body(&self, w: &mut Writer) {
        w.write_u8(self.kind.tag());
        w.write_u32(self.epoch);
        w.write_i32(self.turn_counter);
        w.write_u8(self.phase);
        w.write_u8(self.counters.begin);
        w.write_u8(self.counters.end);
        w.write_u8(self.counters.sync);
        w.write_u8(self.counters.crc);
        w.write_u16(self.node);
        let len = self.roster.len().min(TAKEOVER_MAX_ROSTER);
        w.write_u8(len as u8);
        for entry in self.roster.iter().take(len) {
            w.write_u16(entry.node);
            w.write_u8(entry.team_slot);
            w.write_u8(u8::from(entry.reachable));
        }
    }

    /// Encodes the body into a fresh buffer (for `Packet::Unknown`).
    pub fn encode_payload(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(24 + self.roster.len() * 4);
        self.encode_body(&mut w);
        w.finish()
    }

    /// Decodes a complete frame (type byte included).
    pub fn decode(frame: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader::new(frame);
        let type_byte = r.read_u8()?;
        if type_byte != HOST_TAKEOVER_TYPE {
            return Err(WireError::UnknownPacket(type_byte));
        }
        Self::decode_body(&mut r)
    }

    /// Decodes the payload of `Packet::Unknown { kind: 56, .. }`.
    pub fn decode_payload(payload: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader::new(payload);
        Self::decode_body(&mut r)
    }

    /// Decodes the frame body (after the type byte).
    pub fn decode_body(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let tag = r.read_u8()?;
        let kind = TakeoverKind::from_tag(tag).ok_or(WireError::InvalidValue {
            kind: "takeover kind",
            value: u32::from(tag),
        })?;
        let epoch = r.read_u32()?;
        let turn_counter = r.read_i32()?;
        let phase = r.read_u8()?;
        let counters = TakeoverCounters {
            begin: r.read_u8()?,
            end: r.read_u8()?,
            sync: r.read_u8()?,
            crc: r.read_u8()?,
        };
        let node = r.read_u16()?;
        let len = usize::from(r.read_u8()?);
        if len > TAKEOVER_MAX_ROSTER {
            return Err(WireError::InvalidValue {
                kind: "takeover roster",
                value: len as u32,
            });
        }
        let mut roster = Vec::with_capacity(len);
        for _ in 0..len {
            roster.push(TakeoverRosterEntry {
                node: r.read_u16()?,
                team_slot: r.read_u8()?,
                reachable: r.read_u8()? != 0,
            });
        }
        if !r.is_empty() {
            return Err(WireError::TrailingBytes(r.remaining()));
        }
        Ok(Self {
            kind,
            epoch,
            turn_counter,
            phase,
            counters,
            node,
            roster,
        })
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

    /// Sends an already encoded **transport** frame (takeover 56, heartbeat
    /// 55, …) to one peer or to every verified peer.
    ///
    /// Unlike [`HostHandle::send`] the frame is not wrapped into a
    /// [`Packet`]: packets 55/56 are transport-only and never reach
    /// `Dispatcher::route` (`PLAN-host-mode.md` §5 guardrails).
    pub async fn send_frame(
        &self,
        to: impl Into<Recipient>,
        frame: Vec<u8>,
    ) -> Result<(), HostError> {
        if frame.len() > crate::wire::MAX_PACKET_SIZE {
            return Err(HostError::Wire(WireError::PacketTooLarge {
                size: frame.len(),
                max: crate::wire::MAX_PACKET_SIZE,
            }));
        }
        self.commands
            .send(HostCommand::Send {
                to: to.into(),
                frame,
            })
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
#[path = "host_tests.rs"]
mod tests;
