//! In-game lobby packets on channel 1 (`docs/protocol.md` §8.2).
//!
//! These packets exchange node identities and session settings before the
//! first game turn: 28 → 44 → 29 → 47/30 → 36 → 35 → 32 → 05 → 34.
//! They are not [`max_core::input::Command`]s (they never reach the
//! simulation), so `max_net` owns their wire DTOs.
//!
//! String flavours follow §8.1: `std::string` (u32 length) in 33/35/36/44,
//! raw `char[30]` names in NetNode and game settings (see [`WireName`]).
//!
//! One field is a C++ aggregate rather than a scalar: `Remote_WriteNetNode`
//! sends `packet << node.address`, i.e. the whole `NetAddress` struct, so
//! `sizeof(NetAddress)` — 8 bytes on the game's ABI, not the 6 of its
//! `host`/`port` members — goes on the wire. The trailing two bytes are
//! harmless struct padding, but they are still bytes: a decoder that skips
//! them shifts every following field by two bytes per node (see
//! [`NET_ADDRESS_WIRE_LEN`]).

use max_core::enums::TeamClan;
use max_core::team::TeamId;
use max_core::victory::MissionCategory;

use super::reader::{Reader, WireError, WireName, WireResult, Writer, MAX_VECTOR_LEN};

/// Wire size of a C++ `NetAddress` aggregate.
///
/// `net_address.hpp` declares `{ uint32_t host; uint16_t port; }`, so
/// `sizeof(NetAddress)` is 8 with two trailing alignment bytes.
/// `Remote_WriteNetNode` writes the aggregate (`packet << node.address`),
/// which copies those padding bytes too. Their value is indeterminate on the
/// sender and ignored by the C++ receiver, but their **length** is part of the
/// format: this is `NET_ADDRESS_WIRE_LEN - 6`.
pub const NET_ADDRESS_WIRE_LEN: usize = 8;
/// Trailing `NetAddress` struct-padding bytes present on the wire.
pub const NET_ADDRESS_PADDING: usize = NET_ADDRESS_WIRE_LEN - 6;

/// A node address as carried by NetNode.
///
/// `host` is in network byte order (the raw `ENetAddress.host` field), `port`
/// is in host byte order — exactly as the C++ port copies the aggregate.
/// The two bytes of C++ struct padding that follow on the wire are dropped by
/// [`read_node`] and written as zeros by [`write_node`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NetAddress {
    /// IPv4 address in network byte order.
    pub host: u32,
    /// UDP port in host byte order.
    pub port: u16,
}

impl NetAddress {
    /// Decodes `host` into a printable IPv4 address.
    ///
    /// `host` holds the raw `struct in_addr` bytes in network byte order, so
    /// the native byte representation is already the dotted-quad order.
    pub fn ipv4(self) -> std::net::Ipv4Addr {
        std::net::Ipv4Addr::from(self.host.to_ne_bytes())
    }
}

/// One lobby peer (`NetNode` in `remote.cpp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetNode {
    /// Server-generated node id (`Randomizer(31991)+10`).
    pub entity_id: u16,
    /// ENet address of the peer.
    pub address: NetAddress,
    /// Player name, raw `char[30]`.
    pub name: WireName,
    /// True for the hosting peer.
    pub is_host: bool,
}

/// Session settings block (packets 30 and 37).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyGameSettings {
    /// Mission category; selects the optional field below.
    pub mission_category: MissionCategory,
    /// `multi_scenario_id` (only for [`MissionCategory::Multiplayer`]).
    pub multi_scenario_id: Option<i32>,
    /// SHA-256 mission hashes (only for
    /// [`MissionCategory::MultiPlayerScenario`]).
    pub mission_hashes: Vec<String>,
    /// World name, raw `char[30]`.
    pub world_name: WireName,
    /// `ini_world_index`.
    pub ini_world_index: i32,
    /// `ini_play_mode`.
    pub ini_play_mode: i32,
    /// `ini_timer`.
    pub ini_timer: i32,
    /// `ini_endturn`.
    pub ini_endturn: i32,
    /// `ini_opponent`.
    pub ini_opponent: i32,
    /// `ini_start_gold`.
    pub ini_start_gold: i32,
    /// `ini_raw_resource`.
    pub ini_raw_resource: i32,
    /// `ini_fuel_resource`.
    pub ini_fuel_resource: i32,
    /// `ini_gold_resource`.
    pub ini_gold_resource: i32,
    /// `ini_alien_derelicts`.
    pub ini_alien_derelicts: i32,
    /// `ini_victory_type`.
    pub ini_victory_type: i32,
    /// `ini_victory_limit`.
    pub ini_victory_limit: i32,
    /// `is_map_changed`.
    pub is_map_changed: bool,
    /// `is_multi_scenario`.
    pub is_multi_scenario: bool,
    /// Default team names, raw `char[4][30]`.
    pub default_team_names: [WireName; 4],
    /// RNG seed chosen by the host (also sent by packet 13).
    pub rng_seed: u32,
}

/// Packet 30 — "welcome" reply to a registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyWelcome {
    /// Node id assigned to this client.
    pub player_node: u16,
    /// Node id of the host.
    pub host_node: u16,
    /// Team node ids (`u16[4]`).
    pub team_nodes: [u16; 4],
    /// Team names (`char[4][30]`).
    pub team_names: [WireName; 4],
    /// All registered nodes.
    pub nodes: Vec<NetNode>,
    /// Session settings.
    pub settings: LobbyGameSettings,
}

/// One lobby-channel packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LobbyPacket {
    /// 28 — broadcast lobby entry request (`node = 0` from a client).
    EnterLobby {
        /// Node id in the request (0 from a client).
        node: u16,
    },
    /// 29 — unicast registration at the host.
    Register {
        /// Host node id taken from packet 44.
        host_node: u16,
    },
    /// 44 — host announcement (unicast to a newcomer).
    HostAnnounce {
        /// Host node id.
        player_node: u16,
        /// Game version, must match the server build.
        game_version: u32,
        /// Host player name (`std::string`).
        player_name: String,
    },
    /// 47 — one node registered (multicast).
    NodeRegistered {
        /// Node id the packet describes.
        player_node: u16,
        /// Node details.
        node: NetNode,
    },
    /// 30 — full welcome snapshot (unicast).
    Welcome(Box<LobbyWelcome>),
    /// 37 — game settings sent by the human host.
    HostSettings {
        /// Host node id.
        player_node: u16,
        /// Session settings (same block as in packet 30).
        settings: LobbyGameSettings,
    },
    /// 36 — player name announcement.
    PlayerName {
        /// Node whose name is announced.
        player_node: u16,
        /// Player name (`std::string`).
        player_name: String,
    },
    /// 35 — team slot / READY.
    TeamSelect {
        /// Host node id.
        host_node: u16,
        /// Node selecting the slot.
        player_node: u16,
        /// Team slot 0..=3.
        team_slot: u8,
        /// READY flag.
        ready: bool,
        /// Team name (`std::string`).
        team_name: String,
    },
    /// 33 — lobby chat.
    Chat {
        /// Host node id.
        host_node: u16,
        /// Chat text (`std::string`, sent as "name: text").
        text: String,
    },
    /// 26 — clan selection.
    Clan {
        /// Team index.
        team: TeamId,
        /// Selected clan.
        clan: TeamClan,
    },
    /// 31 — the human host left the lobby.
    HostLeft {
        /// Host node id.
        node: u16,
    },
    /// 32 — server broadcasts setup/start.
    Setup {
        /// Host node id.
        host_node: u16,
    },
    /// 05 — client acknowledges setup.
    SetupAck {
        /// Host node id.
        host_node: u16,
    },
    /// 34 — game start confirmation.
    Start {
        /// Host node id.
        host_node: u16,
    },
}

impl LobbyPacket {
    /// APPL packet number (`type - 0x50`).
    pub const fn kind(&self) -> u8 {
        match self {
            Self::EnterLobby { .. } => 0x1C,     // 28
            Self::Register { .. } => 0x1D,       // 29
            Self::HostAnnounce { .. } => 0x2C,   // 44
            Self::NodeRegistered { .. } => 0x2F, // 47
            Self::Welcome(_) => 0x1E,            // 30
            Self::HostSettings { .. } => 0x25,   // 37
            Self::PlayerName { .. } => 0x24,     // 36
            Self::TeamSelect { .. } => 0x23,     // 35
            Self::Chat { .. } => 0x21,           // 33
            Self::Clan { .. } => 0x1A,           // 26
            Self::HostLeft { .. } => 0x1F,       // 31
            Self::Setup { .. } => 0x20,          // 32
            Self::SetupAck { .. } => 0x05,       // 05
            Self::Start { .. } => 0x22,          // 34
        }
    }
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

fn read_node(r: &mut Reader<'_>) -> WireResult<NetNode> {
    let entity_id = r.read_u16()?;
    let host = r.read_u32()?;
    let port = r.read_u16()?;
    // `packet << node.address` copies sizeof(NetAddress): consume the trailing
    // struct padding (indeterminate on the sender) before the name.
    let _padding: [u8; NET_ADDRESS_PADDING] = r.read_array()?;
    Ok(NetNode {
        entity_id,
        address: NetAddress { host, port },
        name: r.read_name()?,
        is_host: r.read_bool()?,
    })
}

fn write_node(w: &mut Writer, node: &NetNode) {
    w.write_u16(node.entity_id);
    w.write_u32(node.address.host);
    w.write_u16(node.address.port);
    // The C++ sender copies two indeterminate padding bytes here; any value is
    // accepted by its receiver, so emit deterministic zeros.
    w.write_bytes(&[0u8; NET_ADDRESS_PADDING]);
    w.write_name(&node.name);
    w.write_bool(node.is_host);
}

fn read_settings(r: &mut Reader<'_>) -> WireResult<LobbyGameSettings> {
    let category_raw = r.read_u32()?;
    let mission_category = match category_raw {
        0 => MissionCategory::Custom,
        1 => MissionCategory::Training,
        2 => MissionCategory::Campaign,
        3 => MissionCategory::HotSeat,
        4 => MissionCategory::Multiplayer,
        5 => MissionCategory::Demo,
        6 => MissionCategory::Scenario,
        7 => MissionCategory::MultiPlayerScenario,
        other => {
            return Err(WireError::InvalidValue {
                kind: "MissionCategory",
                value: other,
            })
        }
    };

    let mut multi_scenario_id = None;
    let mut mission_hashes = Vec::new();
    match mission_category {
        MissionCategory::Multiplayer => multi_scenario_id = Some(r.read_i32()?),
        MissionCategory::MultiPlayerScenario => {
            let count = r.read_vec_len()?;
            mission_hashes.reserve(count);
            for _ in 0..count {
                mission_hashes.push(r.read_string()?);
            }
        }
        _ => {
            return Err(WireError::InvalidValue {
                kind: "MissionCategory (wire)",
                value: category_raw,
            })
        }
    }

    let world_name = r.read_name()?;
    let ini_world_index = r.read_i32()?;
    let ini_play_mode = r.read_i32()?;
    let ini_timer = r.read_i32()?;
    let ini_endturn = r.read_i32()?;
    let ini_opponent = r.read_i32()?;
    let ini_start_gold = r.read_i32()?;
    let ini_raw_resource = r.read_i32()?;
    let ini_fuel_resource = r.read_i32()?;
    let ini_gold_resource = r.read_i32()?;
    let ini_alien_derelicts = r.read_i32()?;
    let ini_victory_type = r.read_i32()?;
    let ini_victory_limit = r.read_i32()?;
    let is_map_changed = r.read_bool()?;
    let is_multi_scenario = r.read_bool()?;
    let default_team_names = [
        r.read_name()?,
        r.read_name()?,
        r.read_name()?,
        r.read_name()?,
    ];
    let rng_seed = r.read_u32()?;

    Ok(LobbyGameSettings {
        mission_category,
        multi_scenario_id,
        mission_hashes,
        world_name,
        ini_world_index,
        ini_play_mode,
        ini_timer,
        ini_endturn,
        ini_opponent,
        ini_start_gold,
        ini_raw_resource,
        ini_fuel_resource,
        ini_gold_resource,
        ini_alien_derelicts,
        ini_victory_type,
        ini_victory_limit,
        is_map_changed,
        is_multi_scenario,
        default_team_names,
        rng_seed,
    })
}

fn write_settings(w: &mut Writer, settings: &LobbyGameSettings) -> WireResult<()> {
    w.write_u32(settings.mission_category as u32);
    match settings.mission_category {
        MissionCategory::Multiplayer => {
            w.write_i32(settings.multi_scenario_id.ok_or(WireError::InvalidValue {
                kind: "multi_scenario_id (missing)",
                value: 0,
            })?);
        }
        MissionCategory::MultiPlayerScenario => {
            if settings.mission_hashes.len() > MAX_VECTOR_LEN {
                return Err(WireError::VectorTooLong {
                    len: settings.mission_hashes.len(),
                    max: MAX_VECTOR_LEN,
                });
            }
            w.write_u32(settings.mission_hashes.len() as u32);
            for hash in &settings.mission_hashes {
                w.write_string(hash)?;
            }
        }
        _ => {
            return Err(WireError::InvalidValue {
                kind: "MissionCategory (not a lobby category)",
                value: settings.mission_category as u32,
            })
        }
    }

    w.write_name(&settings.world_name);
    w.write_i32(settings.ini_world_index);
    w.write_i32(settings.ini_play_mode);
    w.write_i32(settings.ini_timer);
    w.write_i32(settings.ini_endturn);
    w.write_i32(settings.ini_opponent);
    w.write_i32(settings.ini_start_gold);
    w.write_i32(settings.ini_raw_resource);
    w.write_i32(settings.ini_fuel_resource);
    w.write_i32(settings.ini_gold_resource);
    w.write_i32(settings.ini_alien_derelicts);
    w.write_i32(settings.ini_victory_type);
    w.write_i32(settings.ini_victory_limit);
    w.write_bool(settings.is_map_changed);
    w.write_bool(settings.is_multi_scenario);
    for name in &settings.default_team_names {
        w.write_name(name);
    }
    w.write_u32(settings.rng_seed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Decodes a lobby packet body (the APPL type byte is already consumed).
pub fn decode_lobby(kind: u8, r: &mut Reader<'_>) -> WireResult<LobbyPacket> {
    match kind {
        0x1C => Ok(LobbyPacket::EnterLobby {
            node: r.read_u16()?,
        }),
        0x1D => Ok(LobbyPacket::Register {
            host_node: r.read_u16()?,
        }),
        0x2C => Ok(LobbyPacket::HostAnnounce {
            player_node: r.read_u16()?,
            game_version: r.read_u32()?,
            player_name: r.read_string()?,
        }),
        0x2F => Ok(LobbyPacket::NodeRegistered {
            player_node: r.read_u16()?,
            node: read_node(r)?,
        }),
        0x1E => {
            let player_node = r.read_u16()?;
            let host_node = r.read_u16()?;
            let team_nodes = [r.read_u16()?, r.read_u16()?, r.read_u16()?, r.read_u16()?];
            let team_names = [
                r.read_name()?,
                r.read_name()?,
                r.read_name()?,
                r.read_name()?,
            ];
            let count = r.read_vec_len()?;
            let mut nodes = Vec::with_capacity(count);
            for _ in 0..count {
                nodes.push(read_node(r)?);
            }
            let settings = read_settings(r)?;
            Ok(LobbyPacket::Welcome(Box::new(LobbyWelcome {
                player_node,
                host_node,
                team_nodes,
                team_names,
                nodes,
                settings,
            })))
        }
        0x25 => Ok(LobbyPacket::HostSettings {
            player_node: r.read_u16()?,
            settings: read_settings(r)?,
        }),
        0x24 => Ok(LobbyPacket::PlayerName {
            player_node: r.read_u16()?,
            player_name: r.read_string()?,
        }),
        0x23 => Ok(LobbyPacket::TeamSelect {
            host_node: r.read_u16()?,
            player_node: r.read_u16()?,
            team_slot: r.read_u8()?,
            ready: r.read_bool()?,
            team_name: r.read_string()?,
        }),
        0x21 => Ok(LobbyPacket::Chat {
            host_node: r.read_u16()?,
            text: r.read_string()?,
        }),
        0x1A => {
            let team = TeamId(r.read_u16()? as u8);
            let raw = r.read_u8()?;
            let clan = TeamClan::try_from(raw).map_err(|_| WireError::InvalidValue {
                kind: "TeamClan",
                value: u32::from(raw),
            })?;
            Ok(LobbyPacket::Clan { team, clan })
        }
        0x1F => Ok(LobbyPacket::HostLeft {
            node: r.read_u16()?,
        }),
        0x20 => Ok(LobbyPacket::Setup {
            host_node: r.read_u16()?,
        }),
        0x05 => Ok(LobbyPacket::SetupAck {
            host_node: r.read_u16()?,
        }),
        0x22 => Ok(LobbyPacket::Start {
            host_node: r.read_u16()?,
        }),
        other => Err(WireError::Unencodable { kind: other }),
    }
}

/// Encodes a lobby packet body (the APPL type byte is written by the
/// envelope).
pub fn encode_lobby(packet: &LobbyPacket, w: &mut Writer) -> WireResult<()> {
    match packet {
        LobbyPacket::EnterLobby { node } => {
            w.write_u16(*node);
            Ok(())
        }
        LobbyPacket::Register { host_node } => {
            w.write_u16(*host_node);
            Ok(())
        }
        LobbyPacket::HostAnnounce {
            player_node,
            game_version,
            player_name,
        } => {
            w.write_u16(*player_node);
            w.write_u32(*game_version);
            w.write_string(player_name)
        }
        LobbyPacket::NodeRegistered { player_node, node } => {
            w.write_u16(*player_node);
            write_node(w, node);
            Ok(())
        }
        LobbyPacket::Welcome(welcome) => {
            w.write_u16(welcome.player_node);
            w.write_u16(welcome.host_node);
            for node in welcome.team_nodes {
                w.write_u16(node);
            }
            for name in &welcome.team_names {
                w.write_name(name);
            }
            if welcome.nodes.len() > MAX_VECTOR_LEN {
                return Err(WireError::VectorTooLong {
                    len: welcome.nodes.len(),
                    max: MAX_VECTOR_LEN,
                });
            }
            w.write_u32(welcome.nodes.len() as u32);
            for node in &welcome.nodes {
                write_node(w, node);
            }
            write_settings(w, &welcome.settings)
        }
        LobbyPacket::HostSettings {
            player_node,
            settings,
        } => {
            w.write_u16(*player_node);
            write_settings(w, settings)
        }
        LobbyPacket::PlayerName {
            player_node,
            player_name,
        } => {
            w.write_u16(*player_node);
            w.write_string(player_name)
        }
        LobbyPacket::TeamSelect {
            host_node,
            player_node,
            team_slot,
            ready,
            team_name,
        } => {
            w.write_u16(*host_node);
            w.write_u16(*player_node);
            w.write_u8(*team_slot);
            w.write_bool(*ready);
            w.write_string(team_name)
        }
        LobbyPacket::Chat { host_node, text } => {
            w.write_u16(*host_node);
            w.write_string(text)
        }
        LobbyPacket::Clan { team, clan } => {
            w.write_u16(u16::from(team.0));
            w.write_u8(*clan as u8);
            Ok(())
        }
        LobbyPacket::HostLeft { node } => {
            w.write_u16(*node);
            Ok(())
        }
        LobbyPacket::Setup { host_node } => {
            w.write_u16(*host_node);
            Ok(())
        }
        LobbyPacket::SetupAck { host_node } => {
            w.write_u16(*host_node);
            Ok(())
        }
        LobbyPacket::Start { host_node } => {
            w.write_u16(*host_node);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Packet;
    use max_core::victory::MissionCategory;

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
}
