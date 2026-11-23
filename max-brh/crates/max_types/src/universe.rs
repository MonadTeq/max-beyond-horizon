//! Facade transport DTOs (stage B): the only types `GameUniverse` exchanges with
//! the transport.
//!
//! Architecture: `max-brh-docs/architecture/client/game-universe-facade.md`
//! (ADR §3); the facade itself and its `Dispatcher` policy stay in
//! `max_core::universe`, because they touch `GameState`.
//!
//! The DTOs live in the lower layer (phase 5.2 principle) so that `max_net` can
//! encode them without importing `max_core::universe`: both payloads
//! ([`crate::input::Command`] and [`crate::events::StateSyncPacket`]) are already
//! `max_types` types. `max_core` re-exports the two enums unchanged
//! (`pub use max_types::universe::{NetworkCommand, RemoteCommand};`), so the
//! facade signatures and the application call sites keep their paths.
//!
//! Relocation principle (rule 2): the types were moved **verbatim** — derives,
//! field order and variants were not changed, so determinism/CRC are unaffected
//! (the DTOs never enter `snapshot_bytes` or any CRC; ADR §3).

use crate::events::StateSyncPacket;
use crate::input::Command;

/// A DTO the facade hands to the transport (before encoding).
// The size difference is dictated by the approved ADR signatures (packet 54 is a
// full DTO); boxing would change the shape `max_net` will encode in stage B.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkCommand {
    /// A command packet.
    Command(Command),
    /// Packet 54 — the authoritative team state.
    StateSync(StateSyncPacket),
}

/// A DTO the transport hands to the facade (after decoding).
// The size difference is dictated by the approved ADR signatures (see above).
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCommand {
    /// A command packet.
    Command(Command),
    /// Packet 54 — the authoritative team state.
    StateSync(StateSyncPacket),
}
