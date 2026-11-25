//! Codecs for the facade transport DTOs (stage B, `game-universe-stage-b.md` §2.1).
//!
//! `GameUniverse` talks to the transport with two DTOs (ADR §3):
//! [`NetworkCommand`] (outgoing) and [`RemoteCommand`] (incoming). This module is
//! the thin byte layer over them, not a second protocol model:
//!
//! * [`NetworkCommand::Command`]/[`RemoteCommand::Command`] — the existing
//!   [`command::decode_command`]/[`command::encode_command`] (`wire/command.rs`);
//! * [`NetworkCommand::StateSync`]/[`RemoteCommand::StateSync`] — the existing
//!   [`state::decode_state_sync`]/[`state::encode_state_sync`] (`wire/state.rs`),
//!   bridged through [`StateSync::from_core`].
//!
//! The DTOs come from the lower layer (`max_types::universe`): they are pure data
//! (`Command` or a packet-54 DTO), so this module needs no `max_core` import at
//! all — the `max_net → max_core` boundary lock in
//! `max-brh-cicd/scripts/check_maxnet_boundary.sh` stays reduced to the five
//! read-only projections.

use max_types::universe::{NetworkCommand, RemoteCommand};

use super::command::{decode_command, encode_command};
use super::reader::{Reader, WireResult, Writer};
use super::state::{decode_state_sync, encode_state_sync, StateSync};

/// APPL number of packet 54 — the only facade DTO that is not a `Command`.
pub const STATE_SYNC_KIND: u8 = 0x36;

/// Decodes a facade DTO body (the APPL type byte is already consumed).
pub fn decode_network_command(kind: u8, r: &mut Reader<'_>) -> WireResult<NetworkCommand> {
    if kind == STATE_SYNC_KIND {
        let packet = decode_state_sync(r)?.to_core();
        return Ok(NetworkCommand::StateSync(packet));
    }

    Ok(NetworkCommand::Command(decode_command(kind, r)?))
}

/// Encodes a facade DTO body (the APPL type byte is written by the envelope).
pub fn encode_network_command(command: &NetworkCommand, w: &mut Writer) -> WireResult<()> {
    match command {
        NetworkCommand::Command(command) => encode_command(command, w),
        NetworkCommand::StateSync(packet) => {
            encode_state_sync(&StateSync::from_core(packet.clone()), w)
        }
    }
}

/// Decodes an incoming facade DTO body (the APPL type byte is already consumed).
pub fn decode_remote_command(kind: u8, r: &mut Reader<'_>) -> WireResult<RemoteCommand> {
    if kind == STATE_SYNC_KIND {
        let packet = decode_state_sync(r)?.to_core();
        return Ok(RemoteCommand::StateSync(packet));
    }

    Ok(RemoteCommand::Command(decode_command(kind, r)?))
}

/// Encodes an incoming facade DTO body (the APPL type byte is written by the envelope).
pub fn encode_remote_command(command: &RemoteCommand, w: &mut Writer) -> WireResult<()> {
    match command {
        RemoteCommand::Command(command) => encode_command(command, w),
        RemoteCommand::StateSync(packet) => {
            encode_state_sync(&StateSync::from_core(packet.clone()), w)
        }
    }
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
