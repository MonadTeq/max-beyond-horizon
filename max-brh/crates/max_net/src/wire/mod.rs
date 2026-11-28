//! Game protocol on ENet channel 1 (`APPL`) — `max-brh-docs/protocol.md` §8.
//!
//! Every APPL packet is a single ENet frame whose first byte is
//! `TRANSPORT_APPL_PACKET_ID + number` (`0x50 + n`, so packet 54 → `0x86`).
//! The body is a flat little-endian field dump (§8.1). This module is the
//! envelope: it dispatches the type byte to the command, lobby or state
//! codec and verifies the 1440-byte transport limit.
//!
//! The command variants deliberately *are* [`max_types::input::Command`]
//! values: `max_types` already owns the protocol model and
//! [`max_core::events::apply_command`] consumes it, so `max_net` does not
//! duplicate packet structs. Types without a simulation counterpart (lobby
//! nodes, audit snapshots) have their DTOs here.
//!
//! The dependency on max_core is intentional (read-only projection of the
//! simulation onto the wire, packets 23/45/53/54) — see
//! max-brh-docs/development/PLAN-refactor.md §5.3 /
//! architect-rules.md. Do NOT add new max_core calls without revisiting the
//! decision. Calling apply_* or mutating GameState from max_net voids it.

pub mod command;
pub mod lobby;
pub mod network;
pub mod reader;
pub mod state;

pub use command::{decode_command, encode_command};
pub use lobby::{
    decode_lobby, encode_lobby, LobbyGameSettings, LobbyPacket, LobbyWelcome, NetAddress, NetNode,
};
pub use network::{
    decode_network_command, decode_remote_command, encode_network_command, encode_remote_command,
};
pub use reader::{Reader, WireError, WireName, WireResult, Writer};
pub use state::{
    decode_audit, decode_game_options, decode_save_request, decode_state_sync,
    encode_audit, encode_game_options, encode_save_request, encode_state_sync, encode_team_param,
    state_sync_for, GameOptions, ResearchTopicWire, SaveRequest, StateSync, StateSyncTail, UnitState,
    STATE_SYNC_UNITS_PER_PACKET, UNIT_END,
};

use max_types::input::Command;
use max_types::team::TeamId;
use max_types::unit::UnitId;

/// `TRANSPORT_APPL_PACKET_ID` — base byte of APPL packets (`0x50`).
pub const TRANSPORT_APPL_PACKET_ID: u8 = 0x50;
/// `TRANSPORT_MAX_PACKET_SIZE` — maximum ENet application payload.
pub const MAX_PACKET_SIZE: usize = 1440;
/// Highest APPL number this crate dispatches as a command (packet 57 → type
/// `0x89`). Numbers 55/56 belong to the host-mode extensions (`max_net::host`).
pub const MAX_APPL_PACKET_NUMBER: u8 = 0x39;

/// One decoded APPL packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// Packets 00/01/06/08/09/0A/0B/0C/0D/0E/12/14/26/29/2B/2D/32/34/35 mapped
    /// to the simulation command model.
    Command(Command),
    /// Lobby packets (28/44/29/47/30/37/36/35/33/26/31/32/05/34).
    Lobby(LobbyPacket),
    /// 23 — audit snapshot of one unit.
    Audit {
        /// Audited unit.
        unit: UnitId,
        /// Authoritative state.
        state: UnitState,
    },
    /// 24 — audit acknowledgement.
    AuditAck {
        /// Team answering.
        team: TeamId,
        /// Whether the peer agrees with the snapshot.
        status: bool,
    },
    /// 49 — audit finished.
    AuditDone {
        /// Team that finished the audit.
        team: TeamId,
        /// Audit parameter.
        param: u8,
    },
    /// 54 — authoritative state sync of a team's units.
    StateSync(StateSync),
    /// 51 — desync detected.
    Desync {
        /// Team with the desync.
        team: TeamId,
        /// Desync parameter.
        param: u8,
    },
    /// 07 — leave request.
    Leave {
        /// Team leaving.
        team: TeamId,
        /// Request id.
        request_id: u8,
        /// Leave mode.
        mode: bool,
    },
    /// 48 — leave acknowledgement.
    LeaveAck {
        /// Team acknowledging.
        team: TeamId,
        /// Request id.
        request_id: u8,
    },
    /// 39 — pause request.
    PauseRequest {
        /// Team requesting the pause.
        team: TeamId,
        /// Pause parameter.
        param: u8,
    },
    /// 40 — unpause.
    PauseRelease {
        /// Team releasing the pause.
        team: TeamId,
        /// Pause parameter.
        param: u8,
    },
    /// 42 — site-select cancellation.
    CancelSite {
        /// Team cancelling.
        team: TeamId,
        /// Parameter.
        param: u8,
    },
    /// 46 — delayed-reaction synchronisation.
    DelayedReaction {
        /// Team.
        team: TeamId,
        /// State flag.
        state: bool,
        /// Counter.
        counter: u32,
    },
    /// 17 — full game options.
    GameOptions(GameOptions),
    /// 16 — save request.
    SaveRequest(SaveRequest),
    /// Unknown/unimplemented APPL number; payload preserved verbatim.
    Unknown {
        /// APPL number (`type - 0x50`).
        kind: u8,
        /// Body bytes after the type byte.
        payload: Vec<u8>,
    },
}

impl Packet {
    /// APPL packet number (`type - 0x50`).
    pub const fn kind(&self) -> u8 {
        match self {
            Self::Command(command) => command.kind() as u8,
            Self::Lobby(packet) => packet.kind(),
            Self::Audit { .. } => 0x17,           // 23
            Self::AuditAck { .. } => 0x18,        // 24
            Self::AuditDone { .. } => 0x31,       // 49
            Self::StateSync(_) => 0x36,           // 54
            Self::Desync { .. } => 0x33,          // 51
            Self::Leave { .. } => 0x07,           // 07
            Self::LeaveAck { .. } => 0x30,        // 48
            Self::PauseRequest { .. } => 0x27,    // 39
            Self::PauseRelease { .. } => 0x28,    // 40
            Self::CancelSite { .. } => 0x2A,      // 42
            Self::DelayedReaction { .. } => 0x2E, // 46
            Self::GameOptions(_) => 0x11,         // 17
            Self::SaveRequest(_) => 0x10,         // 16
            Self::Unknown { kind, .. } => *kind,
        }
    }

    /// ENet type byte (`0x50 + kind`), or `None` when it would overflow.
    pub fn wire_type(&self) -> Option<u8> {
        TRANSPORT_APPL_PACKET_ID.checked_add(self.kind())
    }

    /// Decodes a complete APPL frame (type byte included).
    pub fn decode(frame: &[u8]) -> WireResult<Self> {
        if frame.len() > MAX_PACKET_SIZE {
            return Err(WireError::PacketTooLarge {
                size: frame.len(),
                max: MAX_PACKET_SIZE,
            });
        }
        let (&type_byte, body) = frame
            .split_first()
            .ok_or(WireError::UnexpectedEof { offset: 0, need: 1 })?;
        let kind = type_byte
            .checked_sub(TRANSPORT_APPL_PACKET_ID)
            .ok_or(WireError::UnknownPacket(type_byte))?;

        let mut r = Reader::new(body);
        let packet = Self::decode_body(kind, &mut r)?;
        r.finish()?;
        Ok(packet)
    }

    /// Decodes a packet body for `kind` (type byte already consumed).
    pub fn decode_body(kind: u8, r: &mut Reader<'_>) -> WireResult<Self> {
        match kind {
            // Commands (max_types::input::Command).
            0x00 | 0x01 | 0x06 | 0x08 | 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x0E | 0x12 | 0x14
            | 0x26 | 0x29 | 0x2B | 0x2D | 0x32 | 0x34 | 0x35 | 0x39 => {
                Ok(Self::Command(command::decode_command(kind, r)?))
            }
            // Lobby.
            0x05 | 0x1A | 0x1C | 0x1D | 0x1E | 0x1F | 0x20 | 0x21 | 0x22 | 0x23 | 0x24 | 0x25
            | 0x2C | 0x2F => Ok(Self::Lobby(lobby::decode_lobby(kind, r)?)),
            // 23 audit.
            0x17 => {
                let (unit, state) = decode_audit(r)?;
                Ok(Self::Audit { unit, state })
            }
            // 24 audit ack.
            0x18 => Ok(Self::AuditAck {
                team: TeamId(r.read_u16()? as u8),
                status: r.read_bool()?,
            }),
            // 49 audit done, 39 pause request, 40 pause release, 42 cancel,
            // 51 desync: identical `team u16, param u8` shape.
            0x31 => Ok(Self::AuditDone {
                team: TeamId(r.read_u16()? as u8),
                param: r.read_u8()?,
            }),
            0x27 => Ok(Self::PauseRequest {
                team: TeamId(r.read_u16()? as u8),
                param: r.read_u8()?,
            }),
            0x28 => Ok(Self::PauseRelease {
                team: TeamId(r.read_u16()? as u8),
                param: r.read_u8()?,
            }),
            0x2A => Ok(Self::CancelSite {
                team: TeamId(r.read_u16()? as u8),
                param: r.read_u8()?,
            }),
            0x33 => Ok(Self::Desync {
                team: TeamId(r.read_u16()? as u8),
                param: r.read_u8()?,
            }),
            // 07 leave, 48 leave ack.
            0x07 => Ok(Self::Leave {
                team: TeamId(r.read_u16()? as u8),
                request_id: r.read_u8()?,
                mode: r.read_bool()?,
            }),
            0x30 => Ok(Self::LeaveAck {
                team: TeamId(r.read_u16()? as u8),
                request_id: r.read_u8()?,
            }),
            // 46 delayed reactions.
            0x2E => Ok(Self::DelayedReaction {
                team: TeamId(r.read_u16()? as u8),
                state: r.read_bool()?,
                counter: r.read_u32()?,
            }),
            // 54 state sync.
            0x36 => Ok(Self::StateSync(decode_state_sync(r)?)),
            // 17 game options.
            0x11 => Ok(Self::GameOptions(decode_game_options(r)?)),
            // 16 save request.
            0x10 => Ok(Self::SaveRequest(decode_save_request(r)?)),
            other => {
                let mut payload = Vec::with_capacity(r.remaining());
                while !r.is_empty() {
                    payload.push(r.read_u8()?);
                }
                Ok(Self::Unknown {
                    kind: other,
                    payload,
                })
            }
        }
    }

    /// Encodes the complete APPL frame (type byte included).
    pub fn encode(&self) -> WireResult<Vec<u8>> {
        let type_byte = self
            .wire_type()
            .ok_or(WireError::Unencodable { kind: self.kind() })?;
        let mut w = Writer::with_capacity(64);
        w.write_u8(type_byte);
        self.encode_body(&mut w)?;
        let bytes = w.finish();
        if bytes.len() > MAX_PACKET_SIZE {
            return Err(WireError::PacketTooLarge {
                size: bytes.len(),
                max: MAX_PACKET_SIZE,
            });
        }
        Ok(bytes)
    }

    /// Encodes the body (type byte excluded).
    pub fn encode_body(&self, w: &mut Writer) -> WireResult<()> {
        match self {
            Self::Command(command) => command::encode_command(command, w),
            Self::Lobby(packet) => lobby::encode_lobby(packet, w),
            Self::Audit { unit, state } => {
                encode_audit(*unit, state, w);
                Ok(())
            }
            Self::AuditAck { team, status } => {
                w.write_u16(u16::from(team.0));
                w.write_bool(*status);
                Ok(())
            }
            Self::AuditDone { team, param } => {
                encode_team_param(*team, *param, w);
                Ok(())
            }
            Self::PauseRequest { team, param } | Self::PauseRelease { team, param } => {
                encode_team_param(*team, *param, w);
                Ok(())
            }
            Self::CancelSite { team, param } | Self::Desync { team, param } => {
                encode_team_param(*team, *param, w);
                Ok(())
            }
            Self::Leave {
                team,
                request_id,
                mode,
            } => {
                w.write_u16(u16::from(team.0));
                w.write_u8(*request_id);
                w.write_bool(*mode);
                Ok(())
            }
            Self::LeaveAck { team, request_id } => {
                w.write_u16(u16::from(team.0));
                w.write_u8(*request_id);
                Ok(())
            }
            Self::DelayedReaction {
                team,
                state,
                counter,
            } => {
                w.write_u16(u16::from(team.0));
                w.write_bool(*state);
                w.write_u32(*counter);
                Ok(())
            }
            Self::StateSync(packet) => encode_state_sync(packet, w),
            Self::GameOptions(packet) => {
                encode_game_options(packet, w);
                Ok(())
            }
            Self::SaveRequest(packet) => encode_save_request(packet, w),
            Self::Unknown { payload, .. } => {
                w.write_bytes(payload);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
