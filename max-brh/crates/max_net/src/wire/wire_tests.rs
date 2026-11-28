use super::*;
use max_types::enums::{UnitOrder, UnitOrderState, UnitType};
use max_types::geo::{PathStep, Point};
use max_types::input::{
    ComplexSummaryEntry, GroupSelectMode, MissionSupplyEntry, MovePathData, OrderPayload,
    Packet00TurnBarrier, Packet01FrameBarrier, Packet06FinishedTurn, Packet08Order,
    Packet09TeamState, Packet11ComplexOrder, Packet12MissionSupply, Packet13Seed,
    Packet14DeployUnit, Packet18Chat, Packet20UpgradeUnit, Packet38Move, Packet41MoveFinished,
    Packet43Rename, Packet45CrcBarrier, Packet50StopUnit, Packet52EndTurnBarrier,
    Packet53ComplexSummary, Packet57GroupSelect, PayloadP1, PayloadP2, PayloadP3, PayloadP4,
    PayloadP5,
};
use max_types::team::ResearchTopicInfo;

fn round_trip(packet: Packet) -> Vec<u8> {
    let bytes = packet.encode().expect("encode");
    let decoded = Packet::decode(&bytes).expect("decode");
    assert_eq!(decoded, packet, "round-trip mismatch for {bytes:02X?}");
    bytes
}

// Reference frames printed by `tools/barrier_vectors/main.cpp`, a copy of
// the C++ serializers (Remote_SendNetPacket_53 / Remote_SendUnitStates /
// Remote_SendNetPacket_45) built against the real Packet23Data layout.
// Blobs are also committed verbatim in `tools/barrier_vectors/vectors.txt`;
// `barrier_literals_match_cpp_generated_vectors` keeps the two in sync.
const PACKET_53_HEX: &str = "8501000700000001000300ffff02002c01fcff05000600";
const PACKET_54_HEX: &str = "860200030102000201020100010100002a000a000b000c000d000000feff0301020304050600330007080009000264000b0002d4fe0904030c011001003412fbff14001500160017002143070008090a0b0c0d0e0001000ffffffeff02c800100011120013d204000038000000000000000a00000014000000010000000b00000015000000020000000c00000016000000030000000d00000017000000040000000e00000018000000050000000f0000001900000006000000100000001a00000007000000110000001b00000007000100000004000000070000000a0000000d000000100000001300000016000000190000001c0000001f0000002200000025000000280000002b0000002e0000003100000034000000370000003a0000003d000000400000004300000046000000490000004c0000004f0000005200000055000000580000005b0000005e0000006100000064000000670000006a0000006d000000700000007300000076000000790000007c0000007f0000008200000085000000880000008b0000008e0000009100000094000000970000009a0000009d000000a0000000a3000000a6000000a9000000ac000000af000000b2000000b5000000b8000000bb000000be000000c1000000c4000000c7000000ca000000cd000000d0000000d3000000d6000000d9000000dc000000df000000e2000000e5000000e8000000eb000000ee000000f1000000f4000000f7000000fa000000fd000000000100000301000006010000090100000c0100000f0100001201000015010000";
const PACKET_45_HEX: &str = "7d030004cdab";

const BARRIER_VECTORS: &str = include_str!("../../../../tools/barrier_vectors/vectors.txt");

fn barrier_hex_bytes(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0, "odd hex literal length");
    (0..text.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).expect("hex literal"))
        .collect()
}

/// The `packet <n> <hex>` line emitted by the C++ barrier generator.
fn barrier_vector(number: u8) -> &'static str {
    for line in BARRIER_VECTORS.lines() {
        let Some(rest) = line.strip_prefix("packet ") else {
            continue;
        };
        let (candidate, hex) = rest.split_once(' ').expect("packet line");
        if candidate.parse::<u8>() == Ok(number) {
            return hex.split_whitespace().next().expect("hex blob");
        }
    }
    panic!("packet {number} vector missing from tools/barrier_vectors/vectors.txt");
}

fn decode_barrier_reference(number: u8, literal: &str) -> (Vec<u8>, Packet) {
    assert_eq!(
        literal,
        barrier_vector(number),
            "inline literal drifted from the C++ barrier vector file"
    );
    let frame = barrier_hex_bytes(literal);
    let packet = Packet::decode(&frame).expect("reference frame decodes");
    (frame, packet)
}

#[test]
fn barrier_literals_match_cpp_generated_vectors() {
    assert_eq!(PACKET_53_HEX, barrier_vector(53));
    assert_eq!(PACKET_54_HEX, barrier_vector(54));
    assert_eq!(PACKET_45_HEX, barrier_vector(45));
}

#[test]
fn packet_53_complex_summary_matches_cpp_bytes() {
    let (frame, packet) = decode_barrier_reference(53, PACKET_53_HEX);
    assert_eq!(
        packet,
        Packet::Command(Command::ComplexSummary(Packet53ComplexSummary {
            team: TeamId(1),
            turn: 7,
            complexes: vec![ComplexSummaryEntry {
                complex_id: 3,
                values: [-1, 2, 300, -4, 5, 6],
            }],
        }))
    );
    assert_eq!(packet.encode().unwrap(), frame);
}

#[test]
fn packet_54_state_sync_matches_cpp_bytes() {
    let (frame, packet) = decode_barrier_reference(54, PACKET_54_HEX);
    let Packet::StateSync(sync) = &packet else {
        panic!("packet 54 must decode as StateSync");
    };

    assert_eq!(sync.team, TeamId(2));
    assert_eq!(sync.sync_index, 3);
    assert!(sync.is_last);
    assert_eq!(sync.units.len(), 2);

    let (unit, first) = &sync.units[0];
    assert_eq!(*unit, UnitId(0x0102));
    assert_eq!(first.orders, 2);
    assert_eq!(first.state, 1);
    assert_eq!(first.prior_orders, 0);
    assert_eq!(first.prior_state, 1);
    assert!(first.disabled_reaction_fire);
    assert_eq!(first.experience, 42);
    assert_eq!(first.move_to_grid_x, 10);
    assert_eq!(first.move_to_grid_y, 11);
    assert_eq!(first.fire_on_grid_x, 12);
    assert_eq!(first.fire_on_grid_y, 13);
    assert_eq!(first.transfer_cargo, -2);
    assert_eq!(first.stealth_dice_roll, 3);
    assert_eq!(first.total_mining, 1);
    assert_eq!(first.raw_mining, 2);
    assert_eq!(first.fuel_mining, 3);
    assert_eq!(first.gold_mining, 4);
    assert_eq!(first.build_time, 5);
    assert_eq!(first.build_rate, 6);
    assert_eq!(first.unit_type, 0x33);
    assert_eq!(first.unit_id, 7);
    assert_eq!((first.grid_x, first.grid_y), (8, 9));
    assert_eq!(first.team, 2);
    assert_eq!(first.hits, 100);
    assert_eq!(first.speed, 11);
    assert_eq!(first.shots, 2);
    assert_eq!(first.storage, -300);
    assert_eq!(first.ammo, 9);

    let (unit, second) = &sync.units[1];
    assert_eq!(*unit, UnitId(0x0304));
    assert_eq!(second.orders, 0x0C);
    assert_eq!(second.prior_orders, 0x10);
    assert!(!second.disabled_reaction_fire);
    assert_eq!(second.parent_unit_id, 0x1234);
    assert_eq!(second.experience, -5);
    assert_eq!(second.enemy_unit_id, 0x4321);
    assert_eq!((second.grid_x, second.grid_y), (-1, -2));
    assert_eq!(second.hits, 200);
    assert_eq!(second.ammo, 19);

    let tail = sync.tail.as_ref().expect("54 last chunk carries a tail");
    assert_eq!(tail.gold, 1234);
    assert_eq!(tail.gold_spent_on_upgrades, 56);
    assert_eq!(
        tail.research[0],
        ResearchTopicWire {
            research_level: 0,
            turns_to_complete: 10,
            allocation: 20,
        }
    );
    assert_eq!(
        tail.research[7],
        ResearchTopicWire {
            research_level: 7,
            turns_to_complete: 17,
            allocation: 27,
        }
    );
    assert_eq!(tail.number_of_objects_created, 7);
    assert_eq!(tail.unit_counters[0], 1);
    assert_eq!(tail.unit_counters[92], 92 * 3 + 1);

    assert_eq!(packet.encode().unwrap(), frame);
}

#[test]
fn packet_45_crc_barrier_matches_cpp_bytes() {
    let (frame, packet) = decode_barrier_reference(45, PACKET_45_HEX);
    assert_eq!(
        packet,
        Packet::Command(Command::CrcBarrier(Packet45CrcBarrier {
            team: TeamId(3),
            next_turn_index: 4,
            crc16: 0xABCD,
        }))
    );
    assert_eq!(packet.encode().unwrap(), frame);
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
fn packet_18_decodes_a_cpp_smart_string_frame() {
    // The byte literal is assembled by hand from C++:
    //   `Remote_SendNetPacket_18` (remote.cpp:2675-2684) writes
    //   `uint8_t 0x12`, `uint16_t sender_team`, then `SmartString`;
    //   `operator<<(NetPacket&, const SmartString&)` (net_packet.cpp:179-184)
    //   writes `u16 length = strlen + 1` and `length` bytes, including the NUL.
    // The APPL type on the wire is `0x50 + 0x12 = 0x62`.
    let frame = [
        0x62, // APPL 18
        0x01, 0x00, // sender_team = 1 (GREEN)
        0x06, 0x00, // SmartString length = 6 (5 characters + NUL)
        b'H', b'e', b'l', b'l', b'o', 0x00,
    ];
    let decoded = Packet::decode(&frame).expect("C++ frame decodes");

    assert_eq!(
        decoded,
        Packet::Command(Command::Chat(Packet18Chat {
            sender: TeamId::GREEN,
            message: "Hello".into(),
        }))
    );
    assert_eq!(decoded.encode().unwrap(), frame);
}

#[test]
fn packet_18_empty_message_is_one_nul_byte() {
    // C++ `Remote_SendNetPacket_18` does not send empty messages, but
    // `SmartString("")` on the wire is `len = 1` + NUL; the decoder must
    // accept such a frame without error.
    let frame = [0x62, 0x00, 0x00, 0x01, 0x00, 0x00];
    let decoded = Packet::decode(&frame).expect("empty chat decodes");

    assert_eq!(
        decoded,
        Packet::Command(Command::Chat(Packet18Chat {
            sender: TeamId::RED,
            message: String::new(),
        }))
    );
    assert_eq!(decoded.encode().unwrap(), frame);
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
            raw: 1,
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

/// Packet **57** (BRH extension): the group-selection command.
///
/// The type byte is `0x50 + 57 = 0x89`; the body is
/// `team u16, mode u8, unit u16, member u16, upper i16×2, lower i16×2`.
#[test]
fn packet_57_group_select_round_trips_in_every_mode() {
    for mode in [
        GroupSelectMode::Box,
        GroupSelectMode::Toggle,
        GroupSelectMode::SelectUnit,
    ] {
        let packet = Packet::Command(Command::GroupSelect(Packet57GroupSelect {
            team: TeamId::GREEN,
            mode,
            unit: Some(UnitId(7)),
            member: Some(UnitId(9)),
            upper: Point::new(320, 320),
            lower: Point::new(511, 447),
        }));
        let bytes = round_trip(packet);

        assert_eq!(bytes[0], 0x89, "APPL type of packet 57");
        assert_eq!(bytes.len(), 1 + 2 + 1 + 2 + 2 + 4 + 4);
    }

    // A frame without units (`unit`/`member` = 0 on the wire).
    round_trip(Packet::Command(Command::GroupSelect(
        Packet57GroupSelect {
            team: TeamId::RED,
            mode: GroupSelectMode::Box,
            unit: None,
            member: None,
            upper: Point::ZERO,
            lower: Point::ZERO,
        },
    )));

    // An unknown mode byte is rejected, not silently coerced.
    let mut body = vec![0x89];
    body.extend_from_slice(&[0x00, 0x00, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert!(Packet::decode(&body).is_err(), "mode 9 is not a selection");
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
        mission_category: max_types::victory::MissionCategory::Multiplayer,
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

/// Packet 09 is byte-pinned against the oracle layout
/// (`Remote_SendNetPacket_09`, `remote.cpp:2067-2086`):
/// `type u8 | team u16 | gold u32 | gold_spent u32 | 8 × (i32, i32, i32) |
/// SmartString(u16 len incl. NUL + bytes + NUL)`.
#[test]
fn packet_09_matches_the_oracle_byte_layout() {
    let research = core::array::from_fn(|index| ResearchTopicInfo {
        research_level: index as u32,
        turns_to_complete: index as u32 * 3,
        allocation: index as u32 * 5,
    });
    let packet = Packet::Command(Command::TeamState(Packet09TeamState {
        team: TeamId::GREEN,
        gold: 300,
        gold_spent_on_upgrades: 15,
        research,
        name: "GREEN".to_string(),
    }));

    // The frame below is the oracle's field dump, assembled by hand.
    let mut expected = Vec::new();
    expected.push(0x59); // REMOTE_PACKET_09 = TRANSPORT_APPL_PACKET_ID + 9
    expected.extend_from_slice(&1u16.to_le_bytes()); // team GREEN
    expected.extend_from_slice(&300u32.to_le_bytes()); // gold
    expected.extend_from_slice(&15u32.to_le_bytes()); // stats_gold_spent_on_upgrades
    for index in 0..8i32 {
        expected.extend_from_slice(&index.to_le_bytes()); // research_level
        expected.extend_from_slice(&(index * 3).to_le_bytes()); // turns_to_complete
        expected.extend_from_slice(&(index * 5).to_le_bytes()); // allocation
    }
    expected.extend_from_slice(&6u16.to_le_bytes()); // SmartString length incl. NUL
    expected.extend_from_slice(b"GREEN\0");

    let bytes = round_trip(packet);
    assert_eq!(bytes, expected, "packet 09 frame: {bytes:02X?}");
    // 1 type + 2 team + 4 gold + 4 spent + 96 research + 2 length + 6 name = 115.
    assert_eq!(bytes.len(), 115, "packet 09 frame size");
    assert_eq!(bytes[0], super::TRANSPORT_APPL_PACKET_ID + 9);
}
