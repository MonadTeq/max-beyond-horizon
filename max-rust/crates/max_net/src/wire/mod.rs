//! Game protocol on ENet channel 1 (`APPL`) — `docs/protocol.md` §8.
//!
//! Every APPL packet is a single ENet frame whose first byte is
//! `TRANSPORT_APPL_PACKET_ID + number` (`0x50 + n`, so packet 54 → `0x86`).
//! The body is a flat little-endian field dump (§8.1). This module is the
//! envelope: it dispatches the type byte to the command, lobby or state
//! codec and verifies the 1440-byte transport limit.
//!
//! The command variants deliberately *are* [`max_core::input::Command`]
//! values: `max_core` already owns the protocol model and
//! [`max_core::events::apply_command`] consumes it, so `max_net` does not
//! duplicate packet structs. Types without a simulation counterpart (lobby
//! nodes, audit snapshots) have their DTOs here.

pub mod command;
pub mod lobby;
pub mod reader;
pub mod state;

pub use command::{decode_command, encode_command};
pub use lobby::{
    decode_lobby, encode_lobby, LobbyGameSettings, LobbyPacket, LobbyWelcome, NetAddress, NetNode,
};
pub use reader::{Reader, WireError, WireName, WireResult, Writer};
pub use state::{
    decode_audit, decode_game_options, decode_save_request, decode_state_sync, decode_team_state,
    encode_audit, encode_game_options, encode_save_request, encode_state_sync, encode_team_param,
    encode_team_state, GameOptions, ResearchTopicWire, SaveRequest, StateSync, StateSyncTail,
    TeamState, UnitState,
};

use max_core::input::Command;
use max_core::team::TeamId;
use max_core::unit::UnitId;

/// `TRANSPORT_APPL_PACKET_ID` — base byte of APPL packets (`0x50`).
pub const TRANSPORT_APPL_PACKET_ID: u8 = 0x50;
/// `TRANSPORT_MAX_PACKET_SIZE` — maximum ENet application payload.
pub const MAX_PACKET_SIZE: usize = 1440;
/// Highest defined APPL number in this crate (packet 54 → type `0x86`).
pub const MAX_APPL_PACKET_NUMBER: u8 = 0x36;

/// One decoded APPL packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// Packets 00/01/06/08/0A/0B/0C/0D/0E/12/14/26/29/2B/2D/32/34/35 mapped
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
    /// 09 — team state (gold/research/name).
    TeamState(TeamState),
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
            Self::TeamState(_) => 0x09,           // 09
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
            // Commands (max_core::input::Command).
            0x00 | 0x01 | 0x06 | 0x08 | 0x0A | 0x0B | 0x0C | 0x0D | 0x0E | 0x12 | 0x14 | 0x26
            | 0x29 | 0x2B | 0x2D | 0x32 | 0x34 | 0x35 => {
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
            // 09 team state.
            0x09 => Ok(Self::TeamState(decode_team_state(r)?)),
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
            Self::TeamState(packet) => encode_team_state(packet, w),
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
mod tests {
    use super::*;
    use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
    use max_core::geo::{PathStep, Point};
    use max_core::input::{
        ComplexSummaryEntry, MissionSupplyEntry, MovePathData, OrderPayload, Packet00TurnBarrier,
        Packet01FrameBarrier, Packet06FinishedTurn, Packet08Order, Packet11ComplexOrder,
        Packet12MissionSupply, Packet13Seed, Packet14DeployUnit, Packet18Chat, Packet20UpgradeUnit,
        Packet38Move, Packet41MoveFinished, Packet43Rename, Packet45CrcBarrier, Packet50StopUnit,
        Packet52EndTurnBarrier, Packet53ComplexSummary, PayloadP1, PayloadP2, PayloadP3, PayloadP4,
        PayloadP5,
    };

    fn round_trip(packet: Packet) -> Vec<u8> {
        let bytes = packet.encode().expect("encode");
        let decoded = Packet::decode(&bytes).expect("decode");
        assert_eq!(decoded, packet, "round-trip mismatch for {bytes:02X?}");
        bytes
    }

    fn p1(order: UnitOrder) -> PayloadP1 {
        PayloadP1 {
            order,
            state: UnitOrderState::ExecutingOrder,
            prior_order: UnitOrder::Await,
            prior_state: UnitOrderState::ExecutingOrder,
            disabled_reaction_fire: false,
        }
    }

    fn p2(order: UnitOrder) -> PayloadP2 {
        PayloadP2 {
            p1: p1(order),
            parent_unit: None,
            experience: 0,
        }
    }

    fn p3(order: UnitOrder) -> PayloadP3 {
        PayloadP3 {
            p2: p2(order),
            move_to: Point::new(1, 2),
            fire_on: Point::new(3, 4),
            enemy_unit: None,
            transfer_cargo: 0,
            stealth_dice_roll: 0,
        }
    }

    #[test]
    fn packet_00_turn_barrier_is_exact() {
        let bytes = round_trip(Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
            team: TeamId::GREEN,
            counter: 7,
        })));
        assert_eq!(bytes, [0x50, 0x01, 0x00, 0x07]);
    }

    #[test]
    fn packet_01_frame_barrier_is_exact() {
        let bytes = round_trip(Packet::Command(Command::FrameBarrier(
            Packet01FrameBarrier {
                team: TeamId::BLUE,
                counter: 0x3F,
            },
        )));
        assert_eq!(bytes, [0x51, 0x02, 0x00, 0x3F]);
    }

    #[test]
    fn packet_06_finished_turn_is_exact() {
        let bytes = round_trip(Packet::Command(Command::FinishedTurn(
            Packet06FinishedTurn { team: TeamId::GRAY },
        )));
        assert_eq!(bytes, [0x56, 0x03, 0x00]);
    }

    #[test]
    fn packet_08_p0_order17_is_empty_payload() {
        let bytes = round_trip(Packet::Command(Command::Order(Packet08Order {
            unit: UnitId(0x1234),
            payload: OrderPayload::Empty,
        })));
        assert_eq!(bytes, [0x58, 0x34, 0x12, 0x12]);
    }

    #[test]
    fn packet_08_p1_is_exact() {
        let bytes = round_trip(Packet::Command(Command::Order(Packet08Order {
            unit: UnitId(0x1234),
            payload: OrderPayload::P1(p1(UnitOrder::Sentry)),
        })));
        assert_eq!(
            bytes,
            [
                0x58, 0x34, 0x12, // type, unit_id
                0x0C, 0x01, 0x00, 0x01, 0x00 // order, state, prior, prior_state, disabled
            ]
        );
    }

    #[test]
    fn packet_08_p2_is_exact() {
        let bytes = round_trip(Packet::Command(Command::Order(Packet08Order {
            unit: UnitId(0x1234),
            payload: OrderPayload::P2(PayloadP2 {
                p1: p1(UnitOrder::Repair),
                parent_unit: Some(UnitId(5)),
                experience: -2,
            }),
        })));
        assert_eq!(
            bytes,
            [
                0x58, 0x34, 0x12, 0x11, 0x01, 0x00, 0x01, 0x00, // P1
                0x05, 0x00, // parent_unit_id
                0xFE, 0xFF // experience
            ]
        );
    }

    #[test]
    fn packet_08_p3_is_exact() {
        let bytes = round_trip(Packet::Command(Command::Order(Packet08Order {
            unit: UnitId(0x1234),
            payload: OrderPayload::P3(PayloadP3 {
                p2: PayloadP2 {
                    p1: p1(UnitOrder::Move),
                    parent_unit: None,
                    experience: 0,
                },
                move_to: Point::new(10, 20),
                fire_on: Point::new(30, 40),
                enemy_unit: Some(UnitId(9)),
                transfer_cargo: -3,
                stealth_dice_roll: 7,
            }),
        })));
        assert_eq!(
            bytes,
            [
                0x58, 0x34, 0x12, 0x02, 0x01, 0x00, 0x01, 0x00, // P1
                0x00, 0x00, 0x00, 0x00, // parent, experience
                0x0A, 0x00, 0x14, 0x00, // move_to
                0x1E, 0x00, 0x28, 0x00, // fire_on
                0x09, 0x00, // enemy
                0xFD, 0xFF, // transfer_cargo
                0x07  // stealth
            ]
        );
    }

    #[test]
    fn packet_08_p4_is_exact() {
        let bytes = round_trip(Packet::Command(Command::Order(Packet08Order {
            unit: UnitId(1),
            payload: OrderPayload::P4(PayloadP4 {
                p3: p3(UnitOrder::Build),
                repeat_build: true,
                build_time: 5,
                build_rate: 0x0102,
                build_list: vec![UnitType::Tank, UnitType::Barracks],
            }),
        })));
        let mut expected = vec![
            0x58, 0x01, 0x00, // type, unit
            0x04, 0x01, 0x00, 0x01, 0x00, // P1
            0x00, 0x00, 0x00, 0x00, // parent, experience
            0x01, 0x00, 0x02, 0x00, // move_to
            0x03, 0x00, 0x04, 0x00, // fire_on
            0x00, 0x00, // enemy
            0x00, 0x00, // transfer
            0x00, // stealth
            0x01, 0x05, 0x02, 0x01, // repeat, build_time, build_rate
            0x02, 0x00, 0x00, 0x00, // build_count
        ];
        expected.extend_from_slice(&[0x33, 0x00, 0x03, 0x00]); // unit types
        assert_eq!(bytes, expected);
    }

    #[test]
    fn packet_08_p5_is_exact() {
        let bytes = round_trip(Packet::Command(Command::Order(Packet08Order {
            unit: UnitId(1),
            payload: OrderPayload::P5(PayloadP5 {
                p3: p3(UnitOrder::NewAllocate),
                total_mining: 1,
                raw_mining: 2,
                fuel_mining: 3,
                gold_mining: 4,
            }),
        })));
        assert_eq!(&bytes[bytes.len() - 4..], &[1, 2, 3, 4]);
    }

    #[test]
    fn packet_38_ground_path_is_exact() {
        let bytes = round_trip(Packet::Command(Command::Move(Packet38Move {
            unit: UnitId(1),
            order: UnitOrder::Move,
            state: UnitOrderState::Init,
            grid: Point::new(10, 20),
            move_to: Point::new(13, 20),
            group_speed: 0,
            path: Some(MovePathData::Ground(vec![
                PathStep::new(1, 0),
                PathStep::new(1, 0),
                PathStep::new(1, 0),
            ])),
        })));
        assert_eq!(
            bytes,
            [
                0x76, 0x01, 0x00, // type, unit
                0x02, 0x00, // order Move, state Init
                0x0A, 0x00, 0x14, 0x00, // grid (10,20)
                0x0D, 0x00, 0x14, 0x00, // move_to (13,20)
                0x00, 0x00, // group_speed
                0x03, 0x00, // steps_count
                0x01, 0x00, 0x01, 0x00, 0x01, 0x00 // steps
            ]
        );
    }

    #[test]
    fn packet_38_air_path_is_exact() {
        let bytes = round_trip(Packet::Command(Command::Move(Packet38Move {
            unit: UnitId(2),
            order: UnitOrder::Move,
            state: UnitOrderState::InProgress,
            grid: Point::new(1, 2),
            move_to: Point::new(30, 40),
            group_speed: 5,
            path: Some(MovePathData::Air {
                end_x: 30,
                end_y: 40,
                distance_x: 29,
                distance_y: 38,
                euclidean_distance: 47,
                speed: 9,
                move_fraction: 3,
                max_velocity: 4,
            }),
        })));
        assert_eq!(
            bytes,
            [
                0x76, 0x02, 0x00, 0x02, 0x05, // unit, order, state
                0x01, 0x00, 0x02, 0x00, // grid
                0x1E, 0x00, 0x28, 0x00, // move_to
                0x05, 0x00, // group_speed
                0x01, 0x00, // steps_count = 1
                0x1E, 0x00, 0x28, 0x00, // end
                0x1D, 0x00, 0x00, 0x00, 0x26, 0x00, 0x00, 0x00, // distances
                0x2F, 0x00, // euclidean distance
                0x09, 0x00, // speed
                0x03, 0x04 // move_fraction, max_velocity
            ]
        );
    }

    #[test]
    fn packet_41_full_form_is_exact() {
        let bytes = round_trip(Packet::Command(Command::MoveFinished(
            Packet41MoveFinished {
                unit: UnitId(7),
                mode: true,
                grid: Some(Point::new(11, 10)),
                speed: Some(5),
                group_speed: Some(0),
                shots: Some(2),
                move_fraction: Some(1),
            },
        )));
        assert_eq!(
            bytes,
            [
                0x79, 0x07, 0x00, 0x01, // type, unit, mode
                0x0B, 0x00, 0x0A, 0x00, // grid
                0x05, 0x00, // speed
                0x00, 0x00, // group_speed
                0x02, // shots
                0x01  // move_fraction
            ]
        );
    }

    #[test]
    fn packet_41_short_form_is_accepted() {
        let bytes = [0x79, 0x07, 0x00, 0x01];
        let decoded = Packet::decode(&bytes).expect("decode short form");
        assert_eq!(
            decoded,
            Packet::Command(Command::MoveFinished(Packet41MoveFinished::port_form(
                UnitId(7),
                true
            )))
        );
        // Re-encoding the short form stays short.
        assert_eq!(decoded.encode().unwrap(), bytes);
    }

    #[test]
    fn packet_50_stop_is_exact() {
        let bytes = round_trip(Packet::Command(Command::Stop(Packet50StopUnit {
            unit: UnitId(0x1234),
        })));
        assert_eq!(bytes, [0x82, 0x34, 0x12]);
    }

    #[test]
    fn packet_52_end_turn_barrier_is_exact() {
        let bytes = round_trip(Packet::Command(Command::EndTurnBarrier(
            Packet52EndTurnBarrier {
                team: TeamId::GREEN,
                counter: 9,
            },
        )));
        assert_eq!(bytes, [0x84, 0x01, 0x00, 0x09]);
    }

    #[test]
    fn packet_45_crc_barrier_is_exact() {
        let bytes = round_trip(Packet::Command(Command::CrcBarrier(Packet45CrcBarrier {
            team: TeamId::GREEN,
            next_turn_index: 3,
            crc16: 0xABCD,
        })));
        assert_eq!(bytes, [0x7D, 0x01, 0x00, 0x03, 0xCD, 0xAB]);
    }

    #[test]
    fn packet_18_chat_uses_smart_string() {
        let bytes = round_trip(Packet::Command(Command::Chat(Packet18Chat {
            sender: TeamId::BLUE,
            message: "hi".into(),
        })));
        assert_eq!(bytes, [0x62, 0x02, 0x00, 0x03, 0x00, b'h', b'i', 0x00]);
    }

    #[test]
    fn packet_12_mission_supply_matches_documented_layout() {
        let bytes = round_trip(Packet::Command(Command::MissionSupply(
            Packet12MissionSupply {
                team: TeamId::RED,
                round: 2,
                start_gold: 200,
                team_gold: 150,
                units: vec![MissionSupplyEntry {
                    unit_type: UnitType::Tank,
                    cargo: 3,
                }],
                gold_spent_on_upgrades: 10,
                starting_position: Point::new(4, 5),
                proximity_alert_ack: 1,
            },
        )));
        assert_eq!(
            bytes,
            [
                0x5C, 0x00, 0x00, 0x02, // type, team, round
                0xC8, 0x00, // start_gold
                0x96, 0x00, // team_gold
                0x01, 0x00, 0x00, 0x00, // unit_count
                0x0A, 0x00, 0x00, 0x00, // gold spent
                0x04, 0x00, 0x05, 0x00, // starting_position
                0x01, // proximity_alert_ack
                0x33, 0x00, 0x03, 0x00 // unit_type, cargo
            ]
        );
    }

    #[test]
    fn packet_53_complex_summary_round_trips() {
        round_trip(Packet::Command(Command::ComplexSummary(
            Packet53ComplexSummary {
                team: TeamId::RED,
                turn: 7,
                complexes: vec![ComplexSummaryEntry {
                    complex_id: 1,
                    values: [1, 2, 3, 4, 5, 6],
                }],
            },
        )));
    }

    #[test]
    fn packet_11_complex_order_round_trips() {
        round_trip(Packet::Command(Command::ComplexOrder(
            Packet11ComplexOrder {
                team: TeamId::GREEN,
                complex_id: 2,
                material: 1,
                fuel: 2,
                gold: 3,
                power: 4,
                workers: 5,
                buildings: 6,
            },
        )));
    }

    #[test]
    fn packet_13_seed_and_14_deploy_round_trip() {
        round_trip(Packet::Command(Command::Seed(Packet13Seed {
            seed: 0xDEAD_BEEF,
        })));
        round_trip(Packet::Command(Command::Deploy(Packet14DeployUnit {
            team: TeamId::RED,
            unit_type: UnitType::Tank,
            grid: Point::new(4, 5),
        })));
    }

    #[test]
    fn packet_20_upgrade_round_trips() {
        let mut values = [0u16; 13];
        values[1] = 99;
        round_trip(Packet::Command(Command::UpgradeUnit(Packet20UpgradeUnit {
            unit: UnitId(3),
            values,
        })));
    }

    #[test]
    fn packet_43_rename_uses_smart_string() {
        let bytes = round_trip(Packet::Command(Command::Rename(Packet43Rename {
            unit: UnitId(1),
            name: "Tank 1".into(),
        })));
        assert_eq!(&bytes[..3], &[0x7B, 0x01, 0x00]);
        assert_eq!(&bytes[3..5], &[7, 0]);
    }

    #[test]
    fn packet_23_audit_round_trips() {
        let state = UnitState {
            orders: UnitOrder::Move as u8,
            state: UnitOrderState::InProgress as u8,
            prior_orders: UnitOrder::Await as u8,
            prior_state: UnitOrderState::InProgress as u8,
            disabled_reaction_fire: true,
            parent_unit_id: 0,
            experience: 12,
            move_to_grid_x: 10,
            move_to_grid_y: 20,
            fire_on_grid_x: 30,
            fire_on_grid_y: 40,
            enemy_unit_id: 55,
            transfer_cargo: -1,
            stealth_dice_roll: 3,
            total_mining: 1,
            raw_mining: 2,
            fuel_mining: 3,
            gold_mining: 4,
            build_time: 5,
            build_rate: 6,
            unit_type: UnitType::Tank as u16,
            unit_id: 7,
            grid_x: 8,
            grid_y: 9,
            team: 1,
            hits: 100,
            speed: 11,
            shots: 2,
            storage: -100,
            ammo: 9,
        };
        round_trip(Packet::Audit {
            unit: UnitId(0x1234),
            state,
        });
    }

    #[test]
    fn packet_54_state_sync_round_trips() {
        let sync = StateSync {
            team: TeamId::GREEN,
            sync_index: 3,
            is_last: true,
            units: vec![(UnitId(1), UnitState::default())],
            tail: Some(StateSyncTail {
                gold: 100,
                gold_spent_on_upgrades: 5,
                number_of_objects_created: 7,
                ..StateSyncTail::default()
            }),
        };
        let bytes = round_trip(Packet::StateSync(sync));
        assert_eq!(bytes[0], 0x86);
    }

    #[test]
    fn lobby_welcome_round_trips() {
        use super::lobby::{LobbyGameSettings, LobbyWelcome, NetAddress, NetNode};
        let settings = LobbyGameSettings {
            mission_category: max_core::victory::MissionCategory::Multiplayer,
            multi_scenario_id: Some(3),
            mission_hashes: Vec::new(),
            world_name: WireName::from("world"),
            ini_world_index: 0,
            ini_play_mode: 1,
            ini_timer: 30,
            ini_endturn: 15,
            ini_opponent: 2,
            ini_start_gold: 200,
            ini_raw_resource: 1,
            ini_fuel_resource: 1,
            ini_gold_resource: 1,
            ini_alien_derelicts: 0,
            ini_victory_type: 0,
            ini_victory_limit: 100,
            is_map_changed: false,
            is_multi_scenario: false,
            default_team_names: [
                WireName::from("RED"),
                WireName::from("GREEN"),
                WireName::from("BLUE"),
                WireName::from("GRAY"),
            ],
            rng_seed: 42,
        };
        let welcome = LobbyWelcome {
            player_node: 11,
            host_node: 10,
            team_nodes: [0, 0, 0, 0],
            team_names: [WireName::EMPTY; 4],
            nodes: vec![NetNode {
                entity_id: 10,
                address: NetAddress {
                    host: u32::from_be(0x7F00_0001),
                    port: 31554,
                },
                name: WireName::from("host"),
                is_host: true,
            }],
            settings,
        };
        let packet = Packet::Lobby(LobbyPacket::Welcome(Box::new(welcome)));
        let bytes = round_trip(packet);
        assert_eq!(bytes[0], 0x6E);
    }

    #[test]
    fn unknown_packets_are_preserved() {
        let bytes = [0x50 + 0x40, 1, 2, 3];
        let decoded = Packet::decode(&bytes).unwrap();
        assert_eq!(
            decoded,
            Packet::Unknown {
                kind: 0x40,
                payload: vec![1, 2, 3]
            }
        );
        assert_eq!(decoded.encode().unwrap(), bytes);
    }

    #[test]
    fn invalid_type_byte_and_oversize_are_rejected() {
        assert!(matches!(
            Packet::decode(&[0x4F]),
            Err(WireError::UnknownPacket(0x4F))
        ));
        let huge = vec![0u8; MAX_PACKET_SIZE + 1];
        assert!(matches!(
            Packet::decode(&huge),
            Err(WireError::PacketTooLarge { .. })
        ));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let bytes = [0x50, 0x01, 0x00, 0x07, 0xFF];
        assert!(matches!(
            Packet::decode(&bytes),
            Err(WireError::TrailingBytes(1))
        ));
    }

    #[test]
    fn every_command_kind_has_a_stable_number() {
        // The envelope number must be the max_core packet kind, never renumbered.
        let stop = Packet::Command(Command::Stop(Packet50StopUnit { unit: UnitId(1) }));
        assert_eq!(stop.wire_type(), Some(0x82));
        assert_eq!(stop.kind(), 0x32);
        let seed = Packet::Command(Command::Seed(Packet13Seed { seed: 1 }));
        assert_eq!(seed.wire_type(), Some(0x5D));
        assert_eq!(seed.kind(), 0x0D);
    }
}
