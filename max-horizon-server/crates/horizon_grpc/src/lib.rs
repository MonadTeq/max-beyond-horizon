//! # horizon_grpc
//!
//! The Horizon lobby/admin **gRPC server** (`horizon.v1.Horizon`), a thin
//! adapter over the existing session services. It runs next to the WebSocket
//! listener during the transition described in `docs/protocol-grpc.md` §4.4,
//! and is the server side of the same `.proto` contract the `max_net` client
//! already speaks.
//!
//! ## What it is
//!
//! * the 10 unary RPCs the proto defines — `Hello`, the four lobby calls, the
//!   five admin calls and `Ping` — with exactly the semantics of their WS
//!   counterparts (`docs/protocol.md` §5, `docs/protocol-grpc.md` §3/§5);
//! * the production backend [`MaxBackend`] reuses the very same services the
//!   `max_lobby` and `max_admin` plugins use: [`max_session::SessionManager`]
//!   for process lifecycle/ports and [`max_auth`] for signed per-player tickets;
//! * errors are standard gRPC statuses whose `message` is the verbatim legacy WS
//!   text, plus the inline `horizon.v1.Error` of the three admin `ok:false`
//!   responses (`docs/protocol-grpc.md` §6);
//! * the listener is configured by `[server.grpc]` in `config.toml`, defaults to
//!   `0.0.0.0:8082`, and is disabled unless `enabled = true`.
//!
//! ## Layout
//!
//! | Module | Contents |
//! |---|---|
//! | [`proto`] | generated `horizon.v1` messages + client/server stubs |
//! | [`error`] | [`ServiceError`] and the WS-text/close-code mapping tables |
//! | [`backend`] | [`HorizonBackend`], [`MaxBackend`], [`SessionControl`] |
//! | [`service`] | the `horizon.v1.Horizon` implementation (protobuf shell) |
//! | [`server`] | [`GrpcSettings`] + listener bootstrap |
//! | [`testing`] | [`testing::MockSessions`] for tests |
//!
//! ## Example
//!
//! ```no_run
//! use horizon_grpc::{serve, GrpcSettings};
//! use std::path::Path;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let settings = GrpcSettings {
//!     enabled: true,
//!     bind_address: "0.0.0.0:8082".to_string(),
//! };
//!
//! // Resolves when the listener shuts down (e.g. on Ctrl+C).
//! serve(&settings, Path::new("config.toml"), std::future::pending()).await
//! # }
//! ```
//!
//! ## Relationship to the WebSocket server
//!
//! WS is untouched and stays available on its own port. The gRPC adapter owns
//! its own [`max_session::SessionManager`] instance configured from the same
//! `[max]` section, because sharing one handle between a `cdylib` plugin and
//! the host binary is a plugin-loader refactor (documented in
//! `docs/protocol-grpc.md` §4.4 and in the server README).

#![forbid(unsafe_code)]

pub mod admin;
pub mod backend;
pub mod error;
pub mod proto;
pub mod server;
pub mod service;
pub mod testing;

pub use backend::{
    AdminSession, AdminStartParams, CreateGameParams, CreatedGame, GameInfo, GameOptions, GameStatus,
    HorizonBackend, JoinedGame, MaxBackend, RestartOutcome, SessionControl, SessionManagerControl,
    StartOutcome, StopOutcome, TicketInfo, TicketKind, TICKET_TTL,
};
pub use error::{classify_ws_error, code_for_legacy_close, ServiceError};
pub use server::{
    serve, serve_with_backend, serve_with_listener, GrpcSettings, DEFAULT_GRPC_BIND_ADDRESS,
};
pub use service::{HorizonService, PROTOCOL_VERSION};
