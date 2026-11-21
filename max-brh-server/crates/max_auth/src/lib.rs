//! # max_auth
//!
//! Signed session tickets for M.A.X. Port dedicated servers.
//!
//! The game client presents a "session ticket" during the ENet transport handshake
//! (`TRANSPORT_PACKET_00`); the C++ engine compares that string with the
//! `server_session_ticket` setting. `max_auth` is the server-side authority that mints and
//! verifies those tickets, so the lobby can hand out a per-player, time-limited ticket for
//! every join instead of reusing one static password.
//!
//! ## Ticket format
//!
//! A ticket is a plain ASCII string, safe to embed in game configuration files:
//!
//! ```text
//! v1.<session_id>.<player_id>.<unix_expiry>.<hex_hmac>
//! ```
//!
//! * `session_id` / `player_id` — hyphenated, lowercase UUIDs;
//! * `unix_expiry` — expiration time in whole Unix seconds;
//! * `hex_hmac` — lowercase hex encoding of an HMAC-SHA256 over the string
//!   `v1.<session_id>.<player_id>.<unix_expiry>` keyed with the shared secret.
//!
//! Verification is fuzz-safe: every parsing step is a checked operation, [`Auth::verify`]
//! never panics on arbitrary input, and the signature is compared in constant time.
//!
//! ## Usage
//!
//! ```
//! use max_auth::Auth;
//! use std::time::Duration;
//! use uuid::Uuid;
//!
//! let auth = Auth::new(b"shared-secret");
//! let session = Uuid::new_v4();
//! let player = Uuid::new_v4();
//!
//! let ticket = auth.issue(session, player, Duration::from_secs(3600));
//! match auth.verify(&ticket, session) {
//!     Ok(t) => assert_eq!(t.player_id, player),
//!     Err(e) => panic!("fresh ticket must verify: {e}"),
//! }
//! ```
//!
//! The crate deliberately depends only on small, workspace-managed libraries (`serde`,
//! `uuid`, `hmac`, `sha2`, `thiserror`) — no `horizon_event_system` — so it can be linked
//! into both cdylib plugins and plain tests.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod ticket;

pub use ticket::{Auth, Ticket, VerifyError};
