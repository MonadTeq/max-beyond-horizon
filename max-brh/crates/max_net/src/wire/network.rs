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
//! The `max_core` import is DTO-only: the facade enum types are re-exported data,
//! there is no simulation call and no `GameState` access here (the max_net→max_core
//! boundary lock in `max-brh-cicd/scripts/check_maxnet_boundary.sh` allowlists it).

use max_core::universe::{NetworkCommand, RemoteCommand};

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
mod tests {
    use super::*;
    use max_types::enums::{UnitOrder, UnitOrderState, UnitType};
    use max_types::events::{StateSyncPacket, TeamStateSyncTail, UnitStatePacket};
    use max_types::geo::{PathStep, Point};
    use max_types::input::{
        Command, ComplexSummaryEntry, MissionSupplyEntry, MovePathData, OrderPayload,
        Packet00TurnBarrier, Packet01FrameBarrier, Packet06FinishedTurn, Packet08Order,
        Packet10UpgradeUnitType, Packet11ComplexOrder, Packet12MissionSupply, Packet13Seed,
        Packet14DeployUnit, Packet18Chat, Packet20UpgradeUnit, Packet38Move, Packet41MoveFinished,
        Packet43Rename, Packet45CrcBarrier, Packet50StopUnit, Packet52EndTurnBarrier,
        Packet53ComplexSummary, PayloadP1, PayloadP2, PayloadP3, PayloadP4,
    };
    use max_types::team::{ResearchTopicInfo, TeamId};
    use max_types::unit::UnitId;
    use max_types::UNIT_COUNT;

    use crate::wire::{Packet, TRANSPORT_APPL_PACKET_ID};

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
            parent_unit: Some(UnitId(5)),
            experience: -2,
        }
    }

    fn p3(order: UnitOrder) -> PayloadP3 {
        PayloadP3 {
            p2: p2(order),
            move_to: Point::new(10, 20),
            fire_on: Point::new(30, 40),
            enemy_unit: Some(UnitId(9)),
            transfer_cargo: -3,
            stealth_dice_roll: 7,
        }
    }

    /// One representative payload of every [`Command`] variant — the complete
    /// surface a facade DTO can carry.
    fn all_commands() -> Vec<Command> {
        vec![
            Command::Order(Packet08Order {
                unit: UnitId(0x1234),
                payload: OrderPayload::P4(PayloadP4 {
                    p3: p3(UnitOrder::Build),
                    repeat_build: true,
                    build_time: 5,
                    build_rate: 0x0102,
                    build_list: vec![UnitType::Tank, UnitType::Barracks],
                }),
            }),
            Command::Move(Packet38Move {
                unit: UnitId(1),
                order: UnitOrder::Move,
                state: UnitOrderState::Init,
                grid: Point::new(10, 20),
                move_to: Point::new(13, 20),
                group_speed: 0,
                path: Some(MovePathData::Ground(vec![
                    PathStep::new(1, 0),
                    PathStep::new(1, 0),
                ])),
            }),
            Command::MoveFinished(Packet41MoveFinished {
                unit: UnitId(7),
                mode: true,
                grid: Some(Point::new(11, 10)),
                speed: Some(5),
                group_speed: Some(0),
                shots: Some(2),
                move_fraction: Some(1),
            }),
            Command::Stop(Packet50StopUnit {
                unit: UnitId(0x1234),
            }),
            Command::FinishedTurn(Packet06FinishedTurn {
                team: TeamId::GREEN,
            }),
            Command::EndTurnBarrier(Packet52EndTurnBarrier {
                team: TeamId::GREEN,
                counter: 9,
            }),
            Command::CrcBarrier(Packet45CrcBarrier {
                team: TeamId::GREEN,
                next_turn_index: 3,
                crc16: 0xABCD,
            }),
            Command::TurnBarrier(Packet00TurnBarrier {
                team: TeamId::GREEN,
                counter: 7,
            }),
            Command::FrameBarrier(Packet01FrameBarrier {
                team: TeamId::BLUE,
                counter: 0x3F,
            }),
            Command::Chat(Packet18Chat {
                sender: TeamId::BLUE,
                message: "hello".to_string(),
            }),
            Command::ComplexOrder(Packet11ComplexOrder {
                team: TeamId::RED,
                complex_id: 3,
                raw: -1,
                fuel: 2,
                gold: 300,
                power: -4,
                workers: 5,
                buildings: 6,
            }),
            Command::ComplexSummary(Packet53ComplexSummary {
                team: TeamId::GREEN,
                turn: 7,
                complexes: vec![ComplexSummaryEntry {
                    complex_id: 3,
                    values: [-1, 2, 300, -4, 5, 6],
                }],
            }),
            Command::UpgradeUnitType(Packet10UpgradeUnitType {
                team: TeamId::RED,
                unit_type: UnitType::Tank,
                values: core::array::from_fn(|index| index as u16),
            }),
            Command::UpgradeUnit(Packet20UpgradeUnit {
                unit: UnitId(0x4321),
                values: core::array::from_fn(|index| (index as u16) * 2),
            }),
            Command::Rename(Packet43Rename {
                unit: UnitId(0x1234),
                name: "Tank".to_string(),
            }),
            Command::Seed(Packet13Seed { seed: 0x0102_0304 }),
            Command::Deploy(Packet14DeployUnit {
                team: TeamId::RED,
                unit_type: UnitType::Tank,
                grid: Point::new(20, 30),
            }),
            Command::MissionSupply(Packet12MissionSupply {
                team: TeamId::RED,
                round: 2,
                start_gold: 200,
                team_gold: 150,
                units: vec![MissionSupplyEntry {
                    unit_type: UnitType::Tank,
                    cargo: 3,
                }],
                gold_spent_on_upgrades: 17,
                starting_position: Point::new(20, 20),
                proximity_alert_ack: 1,
            }),
        ]
    }

    fn command_kind(command: &Command) -> u8 {
        command.kind() as u8
    }

    fn state_sync_packet(is_last: bool, units: Vec<(UnitId, UnitStatePacket)>) -> StateSyncPacket {
        let mut tail = TeamStateSyncTail {
            gold: 1234,
            gold_spent_on_upgrades: 56,
            number_of_objects_created: 7,
            ..TeamStateSyncTail::default()
        };
        tail.research[7] = ResearchTopicInfo {
            research_level: 7,
            turns_to_complete: 17,
            allocation: 27,
        };
        tail.unit_counters[0] = 1;
        tail.unit_counters[UNIT_COUNT - 1] = 99;

        StateSyncPacket {
            team: TeamId::BLUE,
            sync_index: 3,
            is_last,
            units,
            tail: is_last.then_some(tail),
        }
    }

    fn unit_state() -> UnitStatePacket {
        UnitStatePacket {
            orders: UnitOrder::Sentry as u8,
            state: UnitOrderState::ExecutingOrder as u8,
            disabled_reaction_fire: true,
            parent_unit_id: 0x1234,
            experience: -5,
            move_to_grid_x: 10,
            move_to_grid_y: 11,
            fire_on_grid_x: 12,
            fire_on_grid_y: 13,
            enemy_unit_id: 0x4321,
            transfer_cargo: -2,
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
            team: TeamId::BLUE.0,
            hits: 100,
            speed: 11,
            shots: 2,
            storage: -300,
            ammo: 9,
            ..UnitStatePacket::default()
        }
    }

    /// Round-trips a DTO body through a complete APPL frame (type byte included).
    fn frame_and_decode(command: &NetworkCommand) -> (Vec<u8>, NetworkCommand) {
        let kind = match command {
            NetworkCommand::Command(command) => command_kind(command),
            NetworkCommand::StateSync(_) => STATE_SYNC_KIND,
        };
        let mut w = Writer::with_capacity(64);
        w.write_u8(TRANSPORT_APPL_PACKET_ID + kind);
        encode_network_command(command, &mut w).expect("encode network command");
        let frame = w.finish();

        let (&type_byte, body) = frame.split_first().expect("frame has a type byte");
        assert_eq!(type_byte, TRANSPORT_APPL_PACKET_ID + kind);
        let mut r = Reader::new(body);
        let decoded = decode_network_command(kind, &mut r).expect("decode network command");
        r.finish().expect("no trailing bytes");
        (frame, decoded)
    }

    fn frame_and_decode_remote(command: &RemoteCommand) -> (Vec<u8>, RemoteCommand) {
        let kind = match command {
            RemoteCommand::Command(command) => command_kind(command),
            RemoteCommand::StateSync(_) => STATE_SYNC_KIND,
        };
        let mut w = Writer::with_capacity(64);
        w.write_u8(TRANSPORT_APPL_PACKET_ID + kind);
        encode_remote_command(command, &mut w).expect("encode remote command");
        let frame = w.finish();

        let (&type_byte, body) = frame.split_first().expect("frame has a type byte");
        assert_eq!(type_byte, TRANSPORT_APPL_PACKET_ID + kind);
        let mut r = Reader::new(body);
        let decoded = decode_remote_command(kind, &mut r).expect("decode remote command");
        r.finish().expect("no trailing bytes");
        (frame, decoded)
    }

    #[test]
    fn every_command_variant_round_trips_as_network_command() {
        let commands = all_commands();
        assert_eq!(commands.len(), 18, "every Command variant is covered");

        for command in commands {
            let dto = NetworkCommand::Command(command);
            let (frame, decoded) = frame_and_decode(&dto);
            assert_eq!(decoded, dto, "network round-trip mismatch: {frame:02X?}");

            // No parity drift with the packet envelope: the frame is byte-identical
            // to the one `Packet::encode` produces for the same command.
            let NetworkCommand::Command(command) = &dto else {
                unreachable!()
            };
            assert_eq!(
                frame,
                Packet::Command(command.clone())
                    .encode()
                    .expect("Packet encode"),
                "network codec drifted from the packet envelope"
            );
        }
    }

    #[test]
    fn every_command_variant_round_trips_as_remote_command() {
        for command in all_commands() {
            let dto = RemoteCommand::Command(command);
            let (_, decoded) = frame_and_decode_remote(&dto);
            assert_eq!(decoded, dto, "remote round-trip mismatch");
        }
    }

    #[test]
    fn state_sync_with_last_chunk_and_tail_round_trips() {
        let packet = state_sync_packet(true, vec![(UnitId(0x0102), unit_state())]);
        let dto = NetworkCommand::StateSync(packet.clone());
        let (frame, decoded) = frame_and_decode(&dto);
        assert_eq!(decoded, dto);

        // The DTO frame is the same as the wire `Packet::StateSync` frame.
        let wire = StateSync::from_core(packet.clone());
        assert_eq!(wire.to_core(), packet, "from_core/to_core is an involution");
        assert_eq!(
            frame,
            Packet::StateSync(wire).encode().expect("Packet encode")
        );
        assert_eq!(frame[0], TRANSPORT_APPL_PACKET_ID + STATE_SYNC_KIND);
    }

    #[test]
    fn state_sync_with_empty_units_round_trips() {
        let packet = state_sync_packet(true, Vec::new());
        assert!(packet.units.is_empty());
        assert!(packet.tail.is_some());

        for dto in [
            NetworkCommand::StateSync(packet.clone()),
            NetworkCommand::StateSync(state_sync_packet(false, Vec::new())),
        ] {
            let (_, decoded) = frame_and_decode(&dto);
            assert_eq!(decoded, dto, "empty 54 chunk round-trip");
            let NetworkCommand::StateSync(packet) = &dto else {
                unreachable!()
            };
            assert_eq!(StateSync::from_core(packet.clone()).to_core(), *packet);
        }
    }

    #[test]
    fn state_sync_round_trips_as_remote_command() {
        let packet = state_sync_packet(true, vec![(UnitId(0x0102), unit_state())]);
        let dto = RemoteCommand::StateSync(packet);
        let (_, decoded) = frame_and_decode_remote(&dto);
        assert_eq!(decoded, dto);
    }

    #[test]
    fn state_sync_tail_projects_every_field() {
        let packet = state_sync_packet(true, Vec::new());
        let tail = packet.tail.as_ref().expect("tail");
        let wire = StateSync::from_core(packet.clone());
        let wire_tail = wire.tail.as_ref().expect("wire tail");

        assert_eq!(wire_tail.gold, tail.gold);
        assert_eq!(
            wire_tail.gold_spent_on_upgrades,
            tail.gold_spent_on_upgrades
        );
        assert_eq!(
            wire_tail.number_of_objects_created,
            tail.number_of_objects_created
        );
        assert_eq!(wire_tail.research[7].research_level, 7);
        assert_eq!(wire_tail.research[7].turns_to_complete, 17);
        assert_eq!(wire_tail.research[7].allocation, 27);
        assert_eq!(wire_tail.unit_counters[0], 1);
        assert_eq!(wire_tail.unit_counters[UNIT_COUNT - 1], 99);
    }

    #[test]
    fn unknown_kind_is_unencodable_and_truncated_54_is_an_error() {
        let mut empty = Reader::new(&[]);
        assert!(decode_network_command(0x7F, &mut empty).is_err());

        // 54 without a body: the state-sync decoder reports the missing fields.
        let mut truncated = Reader::new(&[]);
        assert!(decode_remote_command(STATE_SYNC_KIND, &mut truncated).is_err());
    }
}
