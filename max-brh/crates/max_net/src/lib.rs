//! `max_net` — networking of the Rust M.A.X. client.
//!
//! # Two channels
//!
//! 1. **Horizon lobby/admin — gRPC + protobuf** ([`horizon`]): `Hello`,
//!    `ListGames`, `CreateGame`, `JoinGame`, `Ping` from the
//!    `max-brh-server/proto` contract ([`proto`]). The client gets the
//!    session `host`/`port`/`ticket` from `JoinGame`.
//! 2. **Game session — ENet 1.3.18 over UDP**
//!    ([`session`], [`handshake`], [`wire`], [`enet`]):
//!    channel 0 carries the `TP 00`/`TP 03` handshake, channel 1 carries the
//!    lockstep game packets (`0x50 + n`, packets 00…54). Both channels use
//!    `ENET_PACKET_FLAG_RELIABLE`, packets stay within 1440 bytes.
//!
//! The legacy WS JSON client ([`lobby`], [`protocol`]) is kept so `max_app`
//! still builds while the migration described in `max-brh-docs/protocol-grpc.md` is in
//! progress; new code should use [`horizon`].
//!
//! # Layout
//!
//! * [`wire`] — byte-accurate codecs for the game protocol (`max-brh-docs/protocol.md`
//!   §8). Command packets reuse [`max_core::input`] types and convert to/from
//!   [`max_core::input::Command`]; lobby/audit packets have their own DTOs.
//! * [`handshake`] — `TP 00`/`TP 03`, `SHA256(MAX.RES)‖SHA256(PATCHES.RES)`.
//! * [`session`] — ENet connect, handshake, async command/event loop, barriers.
//! * [`enet`] — the only `unsafe` module: a minimal wrapper over the vendored
//!   C library (see its module docs for the safety argument).
//! * [`horizon`] — typed async gRPC façade over the generated `horizon.v1`
//!   stubs (mock-server integration tests included).

pub mod enet;
pub mod handshake;
pub mod horizon;
pub mod wire;

// Legacy WS JSON lobby (kept for `max_app`; superseded by `horizon`).
pub mod lobby;
pub mod protocol;
pub mod session;

pub use handshake::{
    DataHash, Handshake, HandshakeError, Reject, RejectReason, TransportPacket,
    DEFAULT_GAME_VERSION, ENET_VERSION as PROTOCOL_ENET_VERSION, PROTOCOL_VERSION,
};
pub use horizon::{
    CreateGameParams, CreateGameResult, GameInfo, GameJoined, GameStatus, HelloAck, HorizonClient,
    HorizonError, PingResult, Ticket, TicketKind, CLIENT_NAME,
    PROTOCOL_VERSION as HORIZON_PROTOCOL_VERSION,
};
pub use protocol::{GameListing, JoinRequest, LobbyEvent};
pub use session::{
    complex_summary_for, crc_barrier_for, payload_team, BarrierAction, BarrierTiming, BarrierTracker,
    Session, SessionCommand, SessionConfig, SessionError, SessionEvent, SessionHandle,
};
pub use wire::{
    state_sync_for, GameOptions, LobbyGameSettings, LobbyPacket, LobbyWelcome, NetAddress, NetNode,
    Packet, Reader, ResearchTopicWire, SaveRequest, StateSync, StateSyncTail, TeamState, UnitState,
    WireError, WireName, WireResult, Writer, STATE_SYNC_UNITS_PER_PACKET,
};
