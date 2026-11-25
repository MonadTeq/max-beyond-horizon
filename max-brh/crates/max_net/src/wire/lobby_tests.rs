use super::*;
use crate::wire::Packet;
use max_types::victory::MissionCategory;

// Reference frames printed by `tools/lobby_vectors/main.cpp`, a copy of the
// C++ serializers (Remote_SendNetPacket_30/35/37/44/47 + Remote_WriteNetNode
// + Remote_WriteGameSettings) built against the real `net_address.hpp`.
// Blobs are also committed verbatim in
// `tools/lobby_vectors/vectors.txt`; `literals_match_cpp_generated_vectors`
// keeps the two in sync.
const PACKET_28_HEX: &str = "6c0000";
const PACKET_29_HEX: &str = "6d0a00";
const PACKET_30_HEX: &str = "6e0b000a000b00000000000000527573740000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000020000000a007f000001427b0000486f73740000000000000000000000000000000000000000000000000000010b007f00000121cb00005275737400000000000000000000000000000000000000000000000000000004000000010000004d554c5449000000000000000000000000000000000000000000000000000000000001000000030000000000000001000000c8000000010000000100000001000000000000000000000000000000000052757374000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004030201";
const PACKET_35_HEX: &str = "730a000b00020106000000527573742332";
const PACKET_37_HEX: &str = "750a000700000002000000400000006161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616140000000303132333435363738396162636465663031323334353637383961626364656630313233343536373839616263646566303132333435363738396162636465665343454e4152494f000000000000000000000000000000000000000000000000000001000000030000000000000001000000c80000000100000001000000010000000000000000000000000000000101524544000000000000000000000000000000000000000000000000000000475245454e00000000000000000000000000000000000000000000000000424c5545000000000000000000000000000000000000000000000000000047524159000000000000000000000000000000000000000000000000000007000000";
const PACKET_44_HEX: &str = "7c0a000008000004000000486f7374";
const PACKET_47_HEX: &str =
        "7f0a000b007f00000121cbcdcd52757374000000000000000000000000000000000000000000000000000000";

const LOBBY_VECTORS: &str = include_str!("../../../../tools/lobby_vectors/vectors.txt");

fn hex_bytes(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0, "odd hex literal length");
    (0..text.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).expect("hex literal"))
        .collect()
}

/// The `packet <n> <hex>` line emitted by the C++ vector generator.
fn generated_vector(number: u8) -> &'static str {
    for line in LOBBY_VECTORS.lines() {
        let Some(rest) = line.strip_prefix("packet ") else {
            continue;
        };
        let (candidate, hex) = rest.split_once(' ').expect("packet line");
        if candidate.parse::<u8>() == Ok(number) {
            return hex.split_whitespace().next().expect("hex blob");
        }
    }
    panic!("packet {number} vector missing from tools/lobby_vectors/vectors.txt");
}

fn decode_reference(number: u8, literal: &str) -> (Vec<u8>, LobbyPacket) {
    assert_eq!(
        literal,
        generated_vector(number),
            "inline literal drifted from the C++ vector file"
    );
    let frame = hex_bytes(literal);
    let packet = match Packet::decode(&frame).expect("reference frame decodes") {
        Packet::Lobby(packet) => packet,
        other => panic!("expected a lobby packet, got {other:?}"),
    };
    (frame, packet)
}

fn encode_frame(packet: &LobbyPacket) -> Vec<u8> {
    Packet::Lobby(packet.clone()).encode().unwrap()
}

fn multi_settings() -> LobbyGameSettings {
    LobbyGameSettings {
        mission_category: MissionCategory::Multiplayer,
        multi_scenario_id: Some(1),
        mission_hashes: Vec::new(),
        world_name: WireName::from("MULTI"),
        ini_world_index: 0,
        ini_play_mode: 1,
        ini_timer: 3,
        ini_endturn: 0,
        ini_opponent: 1,
        ini_start_gold: 200,
        ini_raw_resource: 1,
        ini_fuel_resource: 1,
        ini_gold_resource: 1,
        ini_alien_derelicts: 0,
        ini_victory_type: 0,
        ini_victory_limit: 0,
        is_map_changed: false,
        is_multi_scenario: false,
        default_team_names: [
            WireName::from("Rust"),
            WireName::EMPTY,
            WireName::EMPTY,
            WireName::EMPTY,
        ],
        rng_seed: 0x0102_0304,
    }
}

#[test]
fn literals_match_cpp_generated_vectors() {
    assert_eq!(PACKET_28_HEX, generated_vector(28));
    assert_eq!(PACKET_29_HEX, generated_vector(29));
    assert_eq!(PACKET_30_HEX, generated_vector(30));
    assert_eq!(PACKET_35_HEX, generated_vector(35));
    assert_eq!(PACKET_37_HEX, generated_vector(37));
    assert_eq!(PACKET_44_HEX, generated_vector(44));
    assert_eq!(PACKET_47_HEX, generated_vector(47));
}

#[test]
fn packet_28_and_29_carry_only_a_node_id() {
    let (frame, packet) = decode_reference(28, PACKET_28_HEX);
    assert_eq!(packet, LobbyPacket::EnterLobby { node: 0 });
    assert_eq!(encode_frame(&packet), frame);

    let (frame, packet) = decode_reference(29, PACKET_29_HEX);
    assert_eq!(packet, LobbyPacket::Register { host_node: 10 });
    assert_eq!(encode_frame(&packet), frame);
}

#[test]
fn packet_30_welcome_matches_cpp_bytes() {
    let (_frame, packet) = decode_reference(30, PACKET_30_HEX);
    let LobbyPacket::Welcome(welcome) = packet else {
        panic!("packet 30 must decode as Welcome");
    };

    assert_eq!(welcome.player_node, 11);
    assert_eq!(welcome.host_node, 10);
    assert_eq!(welcome.team_nodes, [11, 0, 0, 0]);
    assert_eq!(
        welcome.team_names,
        [
            WireName::from("Rust"),
            WireName::EMPTY,
            WireName::EMPTY,
            WireName::EMPTY
        ]
    );

    // Two NetNodes; each carries `entity_id u16, host u32, port u16,
    // 2 padding bytes, name char[30], is_host bool`.
    assert_eq!(welcome.nodes.len(), 2);
    let host = &welcome.nodes[0];
    assert_eq!(host.entity_id, 10);
    assert_eq!(host.address.host, 0x0100_007F);
    assert_eq!(host.address.port, 31554);
    assert_eq!(host.address.ipv4().to_string(), "127.0.0.1");
    assert_eq!(host.name.as_str(), "Host");
    assert!(host.is_host);

    let client = &welcome.nodes[1];
    assert_eq!(client.entity_id, 11);
    assert_eq!(client.address.host, 0x0100_007F);
    assert_eq!(client.address.port, 52001);
    assert_eq!(client.name.as_str(), "Rust");
    assert!(!client.is_host);

    assert_eq!(welcome.settings, multi_settings());
    assert_eq!(
        welcome.settings.mission_category,
        MissionCategory::Multiplayer
    );
    assert_eq!(welcome.settings.multi_scenario_id, Some(1));
}

#[test]
fn packet_30_encoder_reproduces_cpp_bytes() {
    let welcome = LobbyWelcome {
        player_node: 11,
        host_node: 10,
        team_nodes: [11, 0, 0, 0],
        team_names: [
            WireName::from("Rust"),
            WireName::EMPTY,
            WireName::EMPTY,
            WireName::EMPTY,
        ],
        nodes: vec![
            NetNode {
                entity_id: 10,
                address: NetAddress {
                    host: 0x0100_007F,
                    port: 31554,
                },
                name: WireName::from("Host"),
                is_host: true,
            },
            NetNode {
                entity_id: 11,
                address: NetAddress {
                    host: 0x0100_007F,
                    port: 52001,
                },
                name: WireName::from("Rust"),
                is_host: false,
            },
        ],
        settings: multi_settings(),
    };
    let packet = Packet::Lobby(LobbyPacket::Welcome(Box::new(welcome)));
    assert_eq!(packet.encode().unwrap(), hex_bytes(PACKET_30_HEX));
}

#[test]
fn packet_37_scenario_settings_match_cpp_bytes() {
    let (frame, packet) = decode_reference(37, PACKET_37_HEX);
    let LobbyPacket::HostSettings {
        player_node,
        settings,
    } = packet
    else {
        panic!("packet 37 must decode as HostSettings");
    };

    assert_eq!(player_node, 10);
    assert_eq!(
        settings.mission_category,
        MissionCategory::MultiPlayerScenario
    );
    assert_eq!(settings.multi_scenario_id, None);
    assert_eq!(
        settings.mission_hashes,
        vec![
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
        ]
    );
    assert_eq!(settings.world_name.as_str(), "SCENARIO");
    assert!(settings.is_map_changed);
    assert!(settings.is_multi_scenario);
    assert_eq!(
        settings
            .default_team_names
            .map(|name| name.as_str().into_owned()),
        [
                "RED".to_string(),
                "GREEN".to_string(),
                "BLUE".to_string(),
                "GRAY".to_string()
        ]
    );
    assert_eq!(settings.rng_seed, 7);
    assert_eq!(
        encode_frame(&LobbyPacket::HostSettings {
            player_node,
            settings
        }),
        frame
    );
}

#[test]
fn packet_44_host_announce_matches_cpp_bytes() {
    let (frame, packet) = decode_reference(44, PACKET_44_HEX);
    assert_eq!(
        packet,
        LobbyPacket::HostAnnounce {
            player_node: 10,
            game_version: 0x0000_0800,
            player_name: "Host".to_string(),
        }
    );
    assert_eq!(encode_frame(&packet), frame);
}

#[test]
fn packet_47_node_registered_skips_cpp_struct_padding() {
    let (frame, packet) = decode_reference(47, PACKET_47_HEX);
    let LobbyPacket::NodeRegistered { player_node, node } = packet else {
        panic!("packet 47 must decode as NodeRegistered");
    };
    assert_eq!(player_node, 10);
    assert_eq!(node.entity_id, 11);
    assert_eq!(node.address.host, 0x0100_007F);
    assert_eq!(node.address.port, 52001);
    assert_eq!(node.name.as_str(), "Rust");
    assert!(!node.is_host);

    // The C++ generator filled the two `NetAddress` padding bytes with 0xCD
    // on purpose. A parser that reads 6 bytes instead of 8 would consume
    // them as part of the name and fail here.
    assert_eq!(&frame[11..13], &[0xCD, 0xCD]);

    // Re-encoding is identical except that padding is normalised to zeros.
    let encoded = Packet::decode(&frame).unwrap().encode().unwrap();
    assert_eq!(encoded.len(), frame.len());
    assert_eq!(&encoded[..11], &frame[..11]);
    assert_eq!(&encoded[11..13], &[0, 0]);
    assert_eq!(&encoded[13..], &frame[13..]);
}

#[test]
fn packet_35_team_select_matches_cpp_bytes() {
    let (frame, packet) = decode_reference(35, PACKET_35_HEX);
    assert_eq!(
        packet,
        LobbyPacket::TeamSelect {
            host_node: 10,
            player_node: 11,
            team_slot: 2,
            ready: true,
            team_name: "Rust#2".to_string(),
        }
    );
    assert_eq!(encode_frame(&packet), frame);
}

#[test]
fn address_wire_length_matches_cpp_sizeof() {
    // C++ `sizeof(NetAddress)` is 8: two trailing padding bytes after port.
    assert_eq!(NET_ADDRESS_WIRE_LEN, 8);
    assert_eq!(NET_ADDRESS_PADDING, 2);
}
