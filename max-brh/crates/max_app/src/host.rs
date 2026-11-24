//! Server-side lobby authority and host player of the P2P host mode
//! (`PLAN-host-mode.md` §5 H2–H4).
//!
//! The host is the session authority of the star topology: guests connect to its
//! ENet server ([`max_net::host`], H1) and the server lobby below is ported from
//! the Klei oracle. The transport knows nothing about the lobby; every game
//! decision here (`entity_id` assignment, team slots, READY/start, settings) is
//! application policy and lives in `max_app`, not in `max_net`.
//!
//! # Sequence (`remote.cpp:2930-3014`, `:3528-3554`, `:996-1024`)
//!
//! 1. **28 → 44.** A guest entering the lobby is answered with the host
//!    announcement (`Remote_ReceiveNetPacket_28` → `Remote_SendNetPacket_44`):
//!    host node id, game version and host name, unicast to the newcomer.
//! 2. **29 → 47 + 30.** The guest registers at the host node; the host assigns
//!    the next free `entity_id` in entry order (`Remote_GenerateEntityId`, but
//!    deterministic instead of `Randomizer(31991)+10`), announces the node to
//!    everybody (47) and sends the full welcome (30): the guest's node id,
//!    `host_node`, `team_nodes`, `team_names`, the roster and the session
//!    settings (`Remote_WriteGameSettings`).
//! 3. **36/35/26.** Player name, team slot + READY flag and clan update the
//!    roster (`Remote_ReceiveNetPacket_35`: the source node leaves its previous
//!    jar, the new slot records `team_nodes`/`team_names`/`team_jar_in_use`).
//! 4. **READY ≥ `--min-players` → 32.** The C++ `IsAllowedToStartGame`
//!    (`networkmenu.cpp:875-887`) needs at least two players in jars and every
//!    occupied jar in use; the host then broadcasts `32` and waits for the
//!    setup acknowledgements (`Remote_SetupConnection`, `remote.cpp:996-1024`).
//! 5. **05 → 34.** One `05` per guest (the wait is `remote_player_count ==
//!    remote players` in C++, a pending-ack set here) and the host broadcasts
//!    `34`, which completes the lobby on both sides.
//!
//! # Boundaries
//!
//! * no `max_net::host` changes: the H1 transport exposes opaque peer ids and
//!   no UDP addresses, so the roster's [`NetNode`] entries carry a placeholder
//!   [`NetAddress`] — no current consumer reads it (the Rust guest ignores the
//!   node list; addressing uses the ENet peer id);
//! * the lobby is a pure state machine ([`HostLobby`], unit-tested without a
//!   socket) plus a thin async pump ([`run_host_lobby`]);
//! * [`SessionPolicy`] is parsed here and enforced by
//!   [`crate::net::p2p::P2pTransport`] (H4): `Pause` is the default,
//!   `EliminateAfter { ticks }` counts numbered heartbeat ticks and emits the
//!   deterministic `PlayerEliminated { team, sequence }` event. The heartbeat
//!   carries the policy to the guests, so no packet-30 field changes.
//!
//! # Gate H2
//!
//! Two headless processes (`--host` and `--connect <host:port>`) must both reach
//! packet 34 and log the same [`LobbyInfo::summary`] line
//! (`host_node`/`team_nodes`/`rng_seed`/settings).
//!
//! # Host as a player (H3, `PLAN-host-mode.md` §5 H3)
//!
//! After `34` the host does not stay a lobby-only authority: it builds its own
//! `App<NetworkDispatcher>` from the same [`LobbyInfo`] the guests received in
//! packet 30 and plays its team through the **generic** lockstep driver
//! (`App::run_game_loop`, `I: HostIo`), exactly like a guest. The
//! host's [`HostIo`] implementation broadcasts every driver packet to the
//! verified guests; an incoming `HostEventKind::Packet` has already been
//! copy-forwarded to the other guests by the H1 transport, so the driver only
//! submits it to its own simulation. The relay does not re-encode or reorder a
//! frame (`PLAN-host-mode.md` §2).
//!
//! # Gate H3
//!
//! * (a) host + guest, 10 turns: `0` desyncs, `0` barrier timeouts, the
//!   cross-client digest equal and the packet-45 CRC matching every turn;
//! * (b) host + 2 guests (teams 0/1/2), 10 turns: the barrier-packet histograms
//!   (`00`/`45`/`52`/`53`/`54`) are identical on all three clients.

#![forbid(unsafe_code)]

use std::fmt;
use std::net::Ipv4Addr;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use max_core::enums::{Planet, PlayMode, TeamClan};
use max_core::game::PLAYER_TEAM_COUNT;
use max_core::team::TeamId;
use max_core::victory::MissionCategory;
use max_net::{
    HostConfig, HostError, HostEvent, HostEventKind, HostHandle, LobbyGameSettings, LobbyPacket,
    LobbyWelcome, NetAddress, NetNode, Packet, Recipient, WireName, DEFAULT_GAME_VERSION,
    DEFAULT_HEARTBEAT_PERIOD, DEFAULT_HOST_PORT, DEFAULT_MAX_PEERS,
};

use crate::assets::world_asset_name;
use crate::net::{
    handshake_data_hashes, local_team_of, play_mode_from_settings, DisconnectOutcome, FrameSink,
    HostIo, IoEvent, LobbyInfo, NetworkDispatcher, NetworkOptions, PacketHistogram, SharedDigest,
};
use crate::{App, AppConfig, AppDigest};

/// Node id of the host (`NetworkMenu::player_node`, `Randomizer(31991)+10` in C++).
///
/// The deterministic base keeps the H2 gate reproducible; the C++ range starts at
/// 10 as well, and node ids `<= 4` are "no client" (`Remote_SetupPlayers`).
pub const HOST_NODE_ID: u16 = 10;
/// First node id assigned to a guest; ids follow the acceptance/registration order.
pub const FIRST_CLIENT_NODE: u16 = 11;
/// Players needed for `32` by default (the host plus one guest).
///
/// C++ `IsAllowedToStartGame` rejects fewer than two human selections
/// (`remote_player_count < 2`, `networkmenu.cpp:876`).
pub const DEFAULT_MIN_PLAYERS: usize = 2;
/// Maximum number of human players (the four team slots of the original).
pub const MAX_PLAYERS: usize = PLAYER_TEAM_COUNT;
/// How long [`run_host_lobby`] waits for the `28 → … → 34` sequence.
pub const DEFAULT_HOST_LOBBY_TIMEOUT: Duration = Duration::from_secs(30);
/// Event wait per pump iteration (the ENet service task runs at 10 ms).
pub const HOST_LOBBY_EVENT_WAIT: Duration = Duration::from_millis(50);
/// Time the host keeps the session alive after `34`, so the reliable frame
/// reaches every guest before the ENet server closes.
pub const HOST_START_LINGER: Duration = Duration::from_millis(500);
/// `EliminateAfter` tick count used by the `--host-policy eliminate` shorthand.
///
/// The plan makes elimination tick-based (never seconds); the timer is enforced
/// by [`crate::net::p2p::P2pTransport`] on numbered heartbeat ticks.
pub const DEFAULT_ELIMINATION_TICKS: u32 = 30;

/// Session policy of a host game (`PLAN-host-mode.md` §4, §6.1).
///
/// `Pause` (default) freezes the match while a guest is silent; `EliminateAfter`
/// removes the team after a deterministic number of **session ticks**
/// (`ticks_without_heartbeat`, carried by the numbered host heartbeat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionPolicy {
    /// Wait for the silent guest (deterministic default, like a dedicated server).
    #[default]
    Pause,
    /// Eliminate the team after `ticks` ticks without a heartbeat.
    EliminateAfter {
        /// Tick budget before elimination.
        ticks: u32,
    },
}

impl SessionPolicy {
    /// Parses the `--host-policy` value: `pause` or `eliminate`.
    ///
    /// `eliminate` maps to [`DEFAULT_ELIMINATION_TICKS`]; H4 adds an explicit
    /// tick-count flag if needed.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pause" => Some(Self::Pause),
            "eliminate" => Some(Self::EliminateAfter {
                ticks: DEFAULT_ELIMINATION_TICKS,
            }),
            _ => None,
        }
    }

    /// CLI name (`pause`/`eliminate`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::EliminateAfter { .. } => "eliminate",
        }
    }
}

impl fmt::Display for SessionPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Session settings the host advertises in packet 30 (a subset of the C++ INI).
///
/// H2 has no host settings UI: the values come from the CLI (`--world`, `--seed`,
/// `--raw`/`--fuel`/`--gold`/`--derelicts`). The `ini_*` wire block is the same
/// one the guest's `initialize_game` reads (`remote.cpp:247-292`).
#[derive(Debug, Clone)]
pub struct HostSessionSettings {
    /// Planet (`ini_world_index`/`world_name`).
    pub planet: Planet,
    /// World RNG seed (`rng_seed`).
    pub seed: u32,
    /// Raw material level 0..=2.
    pub raw: u8,
    /// Fuel level 0..=2.
    pub fuel: u8,
    /// Gold level 0..=2.
    pub gold: u8,
    /// Alien derelict level 0..=2.
    pub derelicts: u8,
    /// Starting gold (`ini_start_gold`).
    pub start_gold: u32,
}

impl Default for HostSessionSettings {
    fn default() -> Self {
        let reference = max_core::setup::ResourceLevel::NORMAL;

        Self {
            planet: Planet::Snowcrab,
            seed: 0,
            raw: reference.value(),
            fuel: reference.value(),
            gold: reference.value(),
            derelicts: max_core::setup::ResourceLevel::LOW.value(),
            start_gold: max_core::deploy::DEFAULT_START_GOLD,
        }
    }
}

impl HostSessionSettings {
    /// Builds the `LobbyGameSettings` block of packet 30/37.
    ///
    /// Multiplayer custom game (`MISSION_CATEGORY_MULTI`), turn-based, no map
    /// modification: exactly the fields `Remote_WriteGameSettings` writes.
    pub fn lobby_settings(&self) -> LobbyGameSettings {
        LobbyGameSettings {
            mission_category: MissionCategory::Multiplayer,
            multi_scenario_id: Some(1),
            mission_hashes: Vec::new(),
            world_name: WireName::from(world_asset_name(self.planet)),
            ini_world_index: i32::from(self.planet as u8),
            ini_play_mode: 0,
            ini_timer: 3,
            ini_endturn: 0,
            ini_opponent: 1,
            ini_start_gold: self.start_gold.min(i32::MAX as u32) as i32,
            ini_raw_resource: i32::from(self.raw),
            ini_fuel_resource: i32::from(self.fuel),
            ini_gold_resource: i32::from(self.gold),
            ini_alien_derelicts: i32::from(self.derelicts),
            ini_victory_type: 0,
            ini_victory_limit: 0,
            is_map_changed: false,
            is_multi_scenario: false,
            default_team_names: [WireName::EMPTY; 4],
            rng_seed: self.seed,
        }
    }
}

/// Configuration of the lobby state machine.
#[derive(Debug, Clone)]
pub struct HostLobbyConfig {
    /// Host player name (packet 44 `player_name`, roster `NetNode.name`).
    pub host_name: String,
    /// Host team slot 0..=3 (`NetworkMenu::player_team`; clamped).
    pub host_team: u8,
    /// READY threshold including the host (`--min-players`).
    pub min_players: usize,
    /// Session policy (recorded by the lobby; enforced by the H4 transport).
    pub policy: SessionPolicy,
    /// Settings block advertised in packet 30.
    pub settings: LobbyGameSettings,
}

/// Where a lobby packet produced by [`HostLobby`] must go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LobbyTarget {
    /// Unicast to the peer whose packet triggered the action (44/30 replies).
    Source,
    /// Multicast to every verified peer (47/32/34).
    Broadcast,
}

/// One packet the host must send as a reaction to an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyOutgoing {
    /// Destination.
    pub target: LobbyTarget,
    /// Lobby packet (wrapped into `Packet::Lobby` by the pump).
    pub packet: LobbyPacket,
}

/// One registered lobby client (`NetNode` plus the READY state).
#[derive(Debug, Clone)]
struct LobbyPeer {
    /// H1 transport peer id (stable per connection).
    id: u32,
    /// Assigned `entity_id` (`0` — not registered yet via packet 29).
    node: u16,
    /// Player name from packet 36.
    name: WireName,
    /// Team slot from packet 35.
    team_slot: Option<u8>,
    /// READY flag from packet 35 (`team_jar_in_use`).
    ready: bool,
}

/// Read-only snapshot of the roster for logs and the final report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HostLobbyStatus {
    /// Verified peers (H1 `Verified`).
    pub peers: usize,
    /// Peers that registered and got a node id (29).
    pub remote_players: usize,
    /// READY players including the host.
    pub ready_players: usize,
    /// `32` has been broadcast.
    pub setup_sent: bool,
    /// `34` has been broadcast.
    pub start_sent: bool,
    /// Guests that received `32` but have not sent `05` yet.
    pub pending_acks: usize,
}

/// Server-side lobby authority (`PLAN-host-mode.md` §5 H2).
///
/// The state machine is transport-free: [`HostLobby::on_verified`],
/// [`HostLobby::on_packet`] and [`HostLobby::on_disconnect`] return the packets to
/// send (or nothing), which [`run_host_lobby`] forwards through
/// [`HostHandle::send`]. This keeps the port of the Klei lobby unit-testable.
#[derive(Debug, Clone)]
pub struct HostLobby {
    host_node: u16,
    host_name: String,
    host_team: usize,
    min_players: usize,
    policy: SessionPolicy,
    settings: LobbyGameSettings,
    team_nodes: [u16; MAX_PLAYERS],
    team_names: [WireName; MAX_PLAYERS],
    team_clans: [Option<TeamClan>; MAX_PLAYERS],
    peers: Vec<LobbyPeer>,
    next_node: u16,
    setup_sent: bool,
    pending_acks: Vec<u32>,
    start_sent: bool,
}

impl HostLobby {
    /// Creates an empty host lobby with the host already in its team slot.
    pub fn new(config: HostLobbyConfig) -> Self {
        let host_team = usize::from(config.host_team.min((MAX_PLAYERS - 1) as u8));
        let mut team_nodes = [0u16; MAX_PLAYERS];
        let mut team_names = [WireName::EMPTY; MAX_PLAYERS];
        team_nodes[host_team] = HOST_NODE_ID;
        team_names[host_team] = WireName::from(config.host_name.as_str());

        Self {
            host_node: HOST_NODE_ID,
            host_name: config.host_name,
            host_team,
            min_players: config.min_players.clamp(1, MAX_PLAYERS),
            policy: config.policy,
            settings: config.settings,
            team_nodes,
            team_names,
            team_clans: [None; MAX_PLAYERS],
            peers: Vec::new(),
            next_node: FIRST_CLIENT_NODE,
            setup_sent: false,
            pending_acks: Vec::new(),
            start_sent: false,
        }
    }

    /// Host node id (`Remote_NetworkMenu->player_node`).
    pub const fn host_node(&self) -> u16 {
        self.host_node
    }

    /// Host team slot.
    pub const fn host_team(&self) -> usize {
        self.host_team
    }

    /// Session policy the lobby was created with.
    pub const fn policy(&self) -> SessionPolicy {
        self.policy
    }

    /// Settings block the host advertises.
    pub const fn settings(&self) -> &LobbyGameSettings {
        &self.settings
    }

    /// READY threshold.
    pub const fn min_players(&self) -> usize {
        self.min_players
    }

    /// Current team slot of a clan selection (packet 26), if one arrived.
    pub fn team_clan(&self, slot: usize) -> Option<TeamClan> {
        self.team_clans.get(slot).copied().flatten()
    }

    /// Snapshot for logs and the final report.
    pub fn status(&self) -> HostLobbyStatus {
        HostLobbyStatus {
            peers: self.peers.len(),
            remote_players: self.peers.iter().filter(|peer| peer.node != 0).count(),
            ready_players: self.ready_players(),
            setup_sent: self.setup_sent,
            start_sent: self.start_sent,
            pending_acks: self.pending_acks.len(),
        }
    }

    /// `true` after `34` (`connection_state = 1` in C++).
    pub const fn is_complete(&self) -> bool {
        self.start_sent
    }

    /// `LobbyInfo` of this session: the same type the guest's [`crate::net`]
    /// lobby returns, so the host can initialize its own world in H3.
    pub fn lobby_info(&self) -> LobbyInfo {
        LobbyInfo {
            handshake: true,
            player_node: self.host_node,
            host_node: self.host_node,
            team_nodes: self.team_nodes,
            settings: Some(self.settings.clone()),
            ..LobbyInfo::default()
        }
    }

    /// Registers a verified transport peer (H1 `HostEventKind::Verified`).
    ///
    /// No packet is produced: the guest starts the sequence with packet 28.
    pub fn on_verified(&mut self, peer: u32) {
        if self.peers.iter().any(|entry| entry.id == peer) {
            return;
        }

        self.peers.push(LobbyPeer {
            id: peer,
            node: 0,
            name: WireName::EMPTY,
            team_slot: None,
            ready: false,
        });

        tracing::info!("host-lobby: #{peer} подтвердил TP 00 (верифицирован)");
    }

    /// Processes an APPL packet of one verified peer.
    pub fn on_packet(&mut self, peer: u32, packet: &Packet) -> Vec<LobbyOutgoing> {
        let Packet::Lobby(lobby) = packet else {
            tracing::trace!("host-lobby: APPL #{peer} — не лобби-пакет");
            return Vec::new();
        };

        if self.peer_index(peer).is_none() {
            tracing::warn!("host-lobby: лобби-пакет от неверифицированного #{peer} проигнорирован");
            return Vec::new();
        }

        match lobby {
            LobbyPacket::EnterLobby { .. } => self.answer_entry(peer),
            LobbyPacket::Register { host_node } => self.register(peer, *host_node),
            LobbyPacket::PlayerName {
                player_node,
                player_name,
            } => {
                self.note_player_name(*player_node, player_name);
                Vec::new()
            }
            LobbyPacket::TeamSelect {
                host_node,
                player_node,
                team_slot,
                ready,
                team_name,
            } => self.note_team_select(
                peer,
                *host_node,
                *player_node,
                *team_slot,
                *ready,
                team_name,
            ),
            LobbyPacket::Clan { team, clan } => {
                self.note_clan(*team, *clan);
                Vec::new()
            }
            LobbyPacket::SetupAck { host_node } => self.note_setup_ack(peer, *host_node),
            LobbyPacket::HostLeft { node } => self.note_host_left(peer, *node),
            LobbyPacket::Chat { text, .. } => {
                tracing::info!("host-lobby: чат: {text}");
                Vec::new()
            }
            other => {
                tracing::debug!("host-lobby: пакет {other:?} проигнорирован");
                Vec::new()
            }
        }
    }

    /// Removes a peer that left, clearing its team slot and pending ack.
    pub fn on_disconnect(&mut self, peer: u32) -> Vec<LobbyOutgoing> {
        let Some(index) = self.peer_index(peer) else {
            return Vec::new();
        };
        let entry = self.peers.remove(index);

        if entry.node != 0 {
            self.clear_slot(entry.node);
            tracing::warn!("host-lobby: #{peer} (node {}) покинул лобби", entry.node);
        }

        self.pending_acks.retain(|id| *id != peer);

        if self.setup_sent && !self.start_sent && self.pending_acks.is_empty() {
            return self.start();
        }

        if !self.setup_sent {
            return self.maybe_setup().into_iter().collect();
        }

        Vec::new()
    }

    // -- packet handlers --------------------------------------------------

    /// 28 → 44 (`Remote_ReceiveNetPacket_28`): announce the host to the newcomer.
    fn answer_entry(&mut self, peer: u32) -> Vec<LobbyOutgoing> {
        let Some(node) = self.assign_node(peer) else {
            return Vec::new();
        };
        let announce = LobbyPacket::HostAnnounce {
            player_node: self.host_node,
            game_version: DEFAULT_GAME_VERSION,
            player_name: self.host_name.clone(),
        };

        tracing::info!(
            "host-lobby: 28 от #{peer} (node {node}) → 44 (host_node={})",
            self.host_node
        );

        vec![LobbyOutgoing {
            target: LobbyTarget::Source,
            packet: announce,
        }]
    }

    /// 29 → 47 + 30 (`Remote_ReceiveNetPacket_29`): assign the node id and send
    /// the roster/settings snapshot.
    fn register(&mut self, peer: u32, host_node: u16) -> Vec<LobbyOutgoing> {
        if host_node != self.host_node {
            tracing::warn!(
                "host-lobby: 29 с чужим host_node={host_node} (свой {})",
                self.host_node
            );
            return Vec::new();
        }

        let Some(node) = self.assign_node(peer) else {
            return Vec::new();
        };
        let name = self
            .peers
            .iter()
            .find(|entry| entry.id == peer)
            .map_or(WireName::EMPTY, |entry| entry.name);
        let registered = LobbyPacket::NodeRegistered {
            player_node: self.host_node,
            node: NetNode {
                entity_id: node,
                address: NetAddress::default(),
                name,
                is_host: false,
            },
        };
        let welcome = LobbyPacket::Welcome(Box::new(self.welcome(node)));

        tracing::info!("host-lobby: 29 #{peer} → node {node}; 47 (multicast) + 30 (unicast)");

        vec![
            LobbyOutgoing {
                target: LobbyTarget::Broadcast,
                packet: registered,
            },
            LobbyOutgoing {
                target: LobbyTarget::Source,
                packet: welcome,
            },
        ]
    }

    /// 36: a player name (the roster and any already assigned team name).
    fn note_player_name(&mut self, player_node: u16, player_name: &str) {
        let name = WireName::from(player_name);
        let mut matched = false;

        for peer in &mut self.peers {
            if peer.node == player_node {
                peer.name = name;
                matched = true;
            }
        }

        for slot in 0..MAX_PLAYERS {
            if self.team_nodes[slot] == player_node {
                self.team_names[slot] = name;
            }
        }

        if matched {
            tracing::info!("host-lobby: 36 node={player_node} имя={player_name}");
        }
    }

    /// 35: the source node leaves its previous jar and takes `team_slot`.
    fn note_team_select(
        &mut self,
        peer: u32,
        host_node: u16,
        player_node: u16,
        team_slot: u8,
        ready: bool,
        team_name: &str,
    ) -> Vec<LobbyOutgoing> {
        if host_node != self.host_node {
            tracing::warn!(
                "host-lobby: 35 с чужим host_node={host_node} (свой {})",
                self.host_node
            );
            return Vec::new();
        }

        let slot = usize::from(team_slot);
        if slot >= MAX_PLAYERS {
            tracing::warn!("host-lobby: 35 с недопустимым слотом {team_slot}");
            return Vec::new();
        }

        let Some(index) = self.peer_index(peer) else {
            return Vec::new();
        };
        let node = self.peers[index].node;
        if node == 0 || player_node != node {
            tracing::warn!(
                "host-lobby: 35 #{peer} node={player_node} не совпадает с назначенным {node}"
            );
            return Vec::new();
        }

        if self.setup_sent {
            tracing::warn!("host-lobby: 35 node={node} после 32 проигнорирован");
            return Vec::new();
        }

        // A slot held by another node is not stolen: C++ keeps `team_nodes`
        // consistent because clients pick a free jar (`NetworkMenu::NetSync`).
        if self.team_nodes[slot] != 0 && self.team_nodes[slot] != node {
            tracing::warn!(
                "host-lobby: 35 node={node} слот {slot} занят node {} — отклонено",
                self.team_nodes[slot]
            );
            return Vec::new();
        }

        if let Some(previous) = self.peers[index].team_slot {
            if usize::from(previous) != slot {
                self.clear_slot(node);
            }
        }

        self.team_nodes[slot] = node;
        self.team_names[slot] = WireName::from(team_name);
        self.peers[index].team_slot = Some(team_slot);
        self.peers[index].ready = ready;

        tracing::info!("host-lobby: 35 node={node} → слот {slot} ready={ready} имя={team_name}");

        self.maybe_setup().into_iter().collect()
    }

    /// 26: `Remote_ReceiveNetPacket_26` stores the clan of a team slot.
    fn note_clan(&mut self, team: TeamId, clan: TeamClan) {
        let slot = usize::from(team.0);
        if slot >= MAX_PLAYERS {
            tracing::warn!("host-lobby: 26 с недопустимым слотом {slot}");
            return;
        }

        self.team_clans[slot] = Some(clan);
        tracing::info!("host-lobby: 26 слот {slot} клан {clan:?}");
    }

    /// 05: a guest acknowledged `32`; the last one completes the lobby with `34`.
    fn note_setup_ack(&mut self, peer: u32, host_node: u16) -> Vec<LobbyOutgoing> {
        if !self.setup_sent || host_node != self.host_node {
            return Vec::new();
        }

        let Some(index) = self.pending_acks.iter().position(|id| *id == peer) else {
            tracing::debug!("host-lobby: повторный 05 от #{peer}");
            return Vec::new();
        };
        self.pending_acks.remove(index);

        tracing::info!(
            "host-lobby: 05 #{peer}; ожидающих подтверждений {}",
            self.pending_acks.len()
        );

        if self.pending_acks.is_empty() {
            self.start()
        } else {
            Vec::new()
        }
    }

    /// 31: the human host left (C++ `Remote_SendNetPacket_31`); a guest sending
    /// it is treated as a leave request for its own slot.
    fn note_host_left(&mut self, peer: u32, node: u16) -> Vec<LobbyOutgoing> {
        let Some(index) = self.peer_index(peer) else {
            return Vec::new();
        };

        if self.peers[index].node != node {
            tracing::warn!("host-lobby: 31 node={node} от #{peer} не совпадает");
            return Vec::new();
        }

        self.on_disconnect(peer)
    }

    // -- state transitions -------------------------------------------------

    /// `IsAllowedToStartGame` (`networkmenu.cpp:875-887`): every occupied jar is
    /// in use and `--min-players` READY players are present.
    fn is_allowed_to_start(&self) -> bool {
        if self.ready_players() < self.min_players {
            return false;
        }

        for &node in &self.team_nodes {
            if node == 0 || node == self.host_node {
                continue;
            }

            match self.peers.iter().find(|peer| peer.node == node) {
                Some(peer) if peer.ready => {}
                _ => return false,
            }
        }

        true
    }

    /// READY players including the host.
    fn ready_players(&self) -> usize {
        1 + self
            .peers
            .iter()
            .filter(|peer| peer.node != 0 && peer.ready)
            .count()
    }

    /// Broadcasts `32` once the READY condition holds.
    fn maybe_setup(&mut self) -> Option<LobbyOutgoing> {
        if self.setup_sent || self.start_sent || !self.is_allowed_to_start() {
            return None;
        }

        self.setup_sent = true;
        // Only registered guests (29) are expected to acknowledge `32`: a peer
        // that is still at packet 28 has no `host_node` yet and would ignore the
        // broadcast, which must not block the lobby (C++ counts remote teams).
        self.pending_acks = self
            .peers
            .iter()
            .filter(|peer| peer.node != 0)
            .map(|peer| peer.id)
            .collect();

        tracing::info!(
            "host-lobby: READY {}/{}, игроков {} → 32 setup",
            self.ready_players(),
            self.min_players,
            self.peers.len() + 1
        );

        Some(LobbyOutgoing {
            target: LobbyTarget::Broadcast,
            packet: LobbyPacket::Setup {
                host_node: self.host_node,
            },
        })
    }

    /// Broadcasts `34` after every guest acknowledged `32`.
    fn start(&mut self) -> Vec<LobbyOutgoing> {
        if self.start_sent {
            return Vec::new();
        }
        self.start_sent = true;

        tracing::info!("host-lobby: 34 start (host_node={})", self.host_node);

        vec![LobbyOutgoing {
            target: LobbyTarget::Broadcast,
            packet: LobbyPacket::Start {
                host_node: self.host_node,
            },
        }]
    }

    // -- helpers -----------------------------------------------------------

    fn peer_index(&self, peer: u32) -> Option<usize> {
        self.peers.iter().position(|entry| entry.id == peer)
    }

    /// Assigns the next node id to a peer (idempotent).
    fn assign_node(&mut self, peer: u32) -> Option<u16> {
        let index = self.peer_index(peer)?;
        if self.peers[index].node != 0 {
            return Some(self.peers[index].node);
        }

        let node = self.next_node;
        self.next_node = self.next_node.wrapping_add(1);
        self.peers[index].node = node;

        Some(node)
    }

    /// Frees every team slot held by `node`.
    fn clear_slot(&mut self, node: u16) {
        for slot in 0..MAX_PLAYERS {
            if self.team_nodes[slot] == node {
                self.team_nodes[slot] = 0;
                self.team_names[slot] = WireName::EMPTY;
                self.team_clans[slot] = None;
            }
        }
    }

    /// Packet 30 body (`Remote_SendNetPacket_30`).
    fn welcome(&self, player_node: u16) -> LobbyWelcome {
        let mut nodes = Vec::with_capacity(self.peers.len() + 1);
        nodes.push(NetNode {
            entity_id: self.host_node,
            address: NetAddress::default(),
            name: WireName::from(self.host_name.as_str()),
            is_host: true,
        });
        for peer in &self.peers {
            if peer.node == 0 {
                continue;
            }
            nodes.push(NetNode {
                entity_id: peer.node,
                address: NetAddress::default(),
                name: peer.name,
                is_host: false,
            });
        }

        LobbyWelcome {
            player_node,
            host_node: self.host_node,
            team_nodes: self.team_nodes,
            team_names: self.team_names,
            nodes,
            settings: self.settings.clone(),
        }
    }
}

/// Host side of [`HostIo`] (`PLAN-host-mode.md` §5 H3): the host is a player.
///
/// Every packet the driver sends is a broadcast to the verified guests. The
/// host's own copy is applied locally by the facade (a barrier step, `submit`
/// or `apply_immediate`), so it is never looped back through the relay.
///
/// An incoming [`HostEventKind::Packet`] arrives **after** the H1 transport has
/// copy-forwarded the original bytes to the other guests
/// (`max_net::host::HostSession::process_appl`), so the driver only feeds it
/// into its own simulation — the relay is byte-exact by construction.
///
/// A guest that disconnects is not fatal here: H3 keeps the match waiting for
/// it (`DisconnectOutcome::Ignored`); the H4 session policy owns the
/// pause/elimination decision.
#[allow(async_fn_in_trait)]
impl HostIo for HostHandle {
    async fn send_packet(&mut self, packet: Packet) -> Result<()> {
        HostHandle::send(self, Recipient::Broadcast, packet)
            .await
            .context("host: отправка пакета")
    }

    async fn send_packet_untracked(&mut self, packet: Packet) -> Result<()> {
        // The host transport has no barrier tracker (the driver retries the
        // barriers itself), so both send paths are the same broadcast.
        HostHandle::send(self, Recipient::Broadcast, packet)
            .await
            .context("host: отправка пакета")
    }

    async fn clear_barriers(&mut self) -> Result<()> {
        // The host's barrier retransmission lives in the FSM
        // (`BarrierPending`), not in the transport: nothing to clear.
        Ok(())
    }

    async fn next_io_event(&mut self, wait: Duration) -> Result<IoEvent> {
        match self.next_event_timeout(wait).await {
            Ok(Some(event)) => Ok(match event.kind {
                HostEventKind::Packet(packet) => IoEvent::Packet(packet),
                // Lobby-phase/roster bookkeeping: no game meaning for the driver.
                HostEventKind::Connected | HostEventKind::Verified => IoEvent::Control,
                HostEventKind::Disconnected => IoEvent::Disconnected,
            }),
            Ok(None) => Ok(IoEvent::Closed),
            Err(HostError::Timeout { .. }) => Ok(IoEvent::Idle),
            Err(error) => Err(anyhow::Error::from(error)),
        }
    }

    async fn on_disconnect(&mut self) -> Result<DisconnectOutcome> {
        Ok(DisconnectOutcome::Ignored)
    }

    fn is_authority(&self) -> bool {
        true
    }
}

/// Parameters of the host process (`--host` + the lobby-related flags).
#[derive(Debug, Clone)]
pub struct HostRunOptions {
    /// Address the ENet server binds (`--listen`, default `0.0.0.0`).
    pub bind_ip: Ipv4Addr,
    /// UDP port (`--host-port`, default [`DEFAULT_HOST_PORT`] = 31554).
    pub port: u16,
    /// Host player name (`--player`).
    pub host_name: String,
    /// Host team slot (`--team`, clamped to 0..=3).
    pub host_team: u8,
    /// READY threshold including the host (`--min-players`, default 2).
    pub min_players: usize,
    /// Session policy (`--host-policy`, default `Pause`).
    pub policy: SessionPolicy,
    /// Session settings advertised to guests (`--world`/`--seed`/resource levels).
    pub session: HostSessionSettings,
    /// How long the lobby may take before the run fails.
    pub lobby_timeout: Duration,
    /// How many turns the host plays; `0` — until the match ends (`--turns`).
    pub max_turns: u32,
    /// How long a guest may hear nothing from the host before it declares the
    /// host failed (H4 migration, `PLAN-host-mode.md` §4, §6.2).
    ///
    /// Validated in `5..=300 s` and advertised to every guest in the heartbeat.
    pub host_timeout: Duration,
    /// Host heartbeat publishing period (numbered transport heartbeats).
    pub heartbeat: Duration,
}

impl Default for HostRunOptions {
    fn default() -> Self {
        Self {
            bind_ip: Ipv4Addr::UNSPECIFIED,
            port: DEFAULT_HOST_PORT,
            host_name: "host".to_string(),
            host_team: 0,
            min_players: DEFAULT_MIN_PLAYERS,
            policy: SessionPolicy::default(),
            session: HostSessionSettings::default(),
            lobby_timeout: DEFAULT_HOST_LOBBY_TIMEOUT,
            max_turns: 0,
            host_timeout: max_net::DEFAULT_HOST_TIMEOUT,
            heartbeat: DEFAULT_HEARTBEAT_PERIOD,
        }
    }
}

/// The host's own match (H3): the same generic lockstep driver as a guest.
#[derive(Debug, Clone)]
pub struct HostGameReport {
    /// The team the host plays.
    pub local_team: TeamId,
    /// Simulation ticks run by the host.
    pub ticks: u64,
    /// Commands the host applied.
    pub commands_applied: u64,
    /// Turns closed by barrier 52.
    pub turns_finished: u32,
    /// Times a barrier did not wait for the guests.
    pub barrier_timeouts: u32,
    /// Turns whose packet-45 CRC did not match every guest (`0` — no desync).
    pub desyncs: u32,
    /// H4 `host_timeout` detections (a guest fell silent for the whole window).
    pub host_timeouts: u32,
    /// H4 host migrations that completed a takeover (the host is normally the
    /// initial authority; a non-zero value means this client once took over).
    pub migrations: u32,
    /// Host-authoritative team eliminations applied (`TeamEliminated`).
    pub eliminations: u32,
    /// Barrier packets the host FSM emitted (gate (b)).
    pub sent: PacketHistogram,
    /// Barrier packets the transport delivered to the host (gate (b)).
    pub received: PacketHistogram,
    /// H5: desync audit rounds (23/24/49) of this run — empty when the match
    /// never diverged.
    pub audits: Vec<crate::net::DesyncAudit>,
    /// Final state digest (per view: it carries the host's `player_team`).
    pub digest: AppDigest,
    /// The same state projected onto the values every client shares.
    pub shared: SharedDigest,
}

/// Result of one host run: the lobby (`34` reached) plus the host's match.
#[derive(Debug, Clone)]
pub struct HostRunReport {
    /// Lobby data in the same shape the guest returns.
    pub lobby: LobbyInfo,
    /// Bound address.
    pub bind_ip: Ipv4Addr,
    /// Bound UDP port.
    pub port: u16,
    /// Configured session policy.
    pub policy: SessionPolicy,
    /// Final roster counters.
    pub status: HostLobbyStatus,
    /// The host's own match (H3).
    pub game: HostGameReport,
}

/// Result of [`run_host_lobby`] (state machine + final snapshot).
#[derive(Debug, Clone)]
pub struct HostLobbyReport {
    /// Lobby data in the same shape the guest returns.
    pub lobby: LobbyInfo,
    /// Final roster counters.
    pub status: HostLobbyStatus,
}

/// Binds the H1 ENet server, runs the server lobby until packet `34`, then
/// plays the host's own team through the generic lockstep driver (H3).
///
/// The host is a full player of its session: after the lobby it builds the same
/// `App<NetworkDispatcher>` a guest does ([`run_host_game`]) and drives it over
/// [`HostHandle`]. Every packet the driver produces is broadcast to the verified
/// guests; every guest packet the transport delivers has already been relayed to
/// the other guests (H1 copy-forward) and is submitted to the host's own
/// simulation.
pub async fn run_host(config: &AppConfig, options: &HostRunOptions) -> Result<HostRunReport> {
    max_net::validate_host_timeout(options.host_timeout)
        .context("host: host_timeout должен быть в диапазоне 5..=300 с")?;
    if options.heartbeat.is_zero() {
        bail!("host: период heartbeat должен быть больше нуля");
    }

    let hashes = handshake_data_hashes(config).context("host: data_hash для TP 00")?;
    let data_hash = *hashes
        .first()
        .context("host: нет ни одного варианта data_hash")?;

    let host_config = HostConfig {
        bind_ip: options.bind_ip,
        port: options.port,
        game_version: DEFAULT_GAME_VERSION,
        data_hash,
        max_peers: DEFAULT_MAX_PEERS,
        // H4 numbers every heartbeat itself (session_tick), so the ENet
        // transport must not repeat a stale frame.
        heartbeat: Duration::ZERO,
        host_timeout: options.host_timeout,
    };
    let mut host = HostHandle::bind(host_config).with_context(|| {
        format!(
            "host: не удалось занять UDP {}:{}",
            options.bind_ip, options.port
        )
    })?;

    tracing::info!(
        "host: ENet-сервер слушает {}:{} (min_players={}, policy={})",
        options.bind_ip,
        options.port,
        options.min_players,
        options.policy
    );

    let lobby_config = HostLobbyConfig {
        host_name: options.host_name.clone(),
        host_team: options.host_team,
        min_players: options.min_players,
        policy: options.policy,
        settings: options.session.lobby_settings(),
    };

    let result = run_host_lobby(&mut host, &lobby_config, options.lobby_timeout).await;
    let report = match result {
        Ok(report) => report,
        Err(error) => {
            host.shutdown().await;
            return Err(error);
        }
    };

    tracing::info!("{}", report.lobby.summary());

    // H3/H4: the host is a player too — the same world, the same barrier FSM,
    // the same packets as a guest; the transport is the H4 authority wrapper
    // (heartbeat 55, quorum gate, `TeamEliminated`).
    let game = run_host_game(host, config, &report.lobby, options).await?;

    Ok(HostRunReport {
        lobby: report.lobby,
        bind_ip: options.bind_ip,
        port: options.port,
        policy: options.policy,
        status: report.status,
        game,
    })
}

/// Plays the host's own team through the generic lockstep driver (H3/H4).
///
/// The world is initialized from the same [`LobbyInfo`] the guests receive in
/// packet 30 ([`App::initialize_game`]), so the host's simulation starts from
/// the identical seed/map/team set. The host's team is the slot it holds in
/// `team_nodes`; every other occupied slot is a `Remote` team the barrier FSM
/// waits for.
async fn run_host_game(
    host: HostHandle,
    config: &AppConfig,
    lobby: &LobbyInfo,
    options: &HostRunOptions,
) -> Result<HostGameReport> {
    let local_team = local_team_of(lobby, options.host_team);
    let play_mode = lobby
        .settings
        .as_ref()
        .map(play_mode_from_settings)
        .unwrap_or(PlayMode::TurnBased);

    let mut game = App::<NetworkDispatcher>::new_network(config.clone(), local_team, play_mode);
    game.initialize_game(lobby);

    let network = NetworkOptions {
        max_turns: options.max_turns,
        host_timeout: options.host_timeout,
        // H5: `--host` writes `desync-turn<turn>.txt` next to the process.
        audit_dir: Some(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))),
        ..NetworkOptions::default()
    };
    // The host is headless (H3): no SDL window, so no frame sink.
    let mut sink: Option<&mut dyn FrameSink> = None;

    let roster =
        crate::net::p2p::P2pRoster::new(lobby.player_node, lobby.host_node, lobby.team_nodes);
    let mut io = crate::net::p2p::P2pTransport::host(
        host,
        roster,
        local_team,
        options.policy,
        options.host_timeout,
        options.heartbeat,
    );

    tracing::info!(
        "host: игровой цикл за команду {} (turns={}, пиров={}, host_timeout={:?}, policy={})",
        local_team.0,
        options.max_turns,
        game.universe
            .dispatcher()
            .remote_teams(&game.universe)
            .count(),
        options.host_timeout,
        options.policy
    );

    let stats = game.run_game_loop(&mut io, &network, &mut sink).await?;
    io.shutdown().await;
    let digest = game.digest();

    Ok(HostGameReport {
        local_team,
        ticks: game.ticks(),
        commands_applied: game.applied_commands(),
        turns_finished: stats.turns_finished,
        barrier_timeouts: stats.barrier_timeouts,
        desyncs: stats.desyncs,
        host_timeouts: stats.host_timeouts,
        migrations: stats.migrations,
        eliminations: stats.eliminations,
        sent: stats.sent,
        received: stats.received,
        audits: stats.audits,
        digest,
        shared: SharedDigest::capture(digest),
    })
}

/// Pumps H1 host events through [`HostLobby`] until `34` is broadcast.
pub async fn run_host_lobby(
    host: &mut HostHandle,
    config: &HostLobbyConfig,
    timeout: Duration,
) -> Result<HostLobbyReport> {
    let mut lobby = HostLobby::new(config.clone());
    let deadline = tokio::time::Instant::now() + timeout;

    while !lobby.is_complete() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            let status = lobby.status();
            bail!(
                "лобби хоста не завершилось за {timeout:?}: пиров={}, игроков={}, \
                 готовы={}, 32={}, ожидающих 05={}",
                status.peers,
                status.remote_players,
                status.ready_players,
                status.setup_sent,
                status.pending_acks
            );
        }

        let wait = remaining.min(HOST_LOBBY_EVENT_WAIT);
        let event = match host.next_event_timeout(wait).await {
            Ok(Some(event)) => event,
            Ok(None) => bail!("ENet-сессия хоста закрылась во время лобби"),
            Err(HostError::Timeout { .. }) => continue,
            Err(error) => return Err(error).context("host-lobby: ожидание события хоста"),
        };

        for action in dispatch_host_event(&mut lobby, &event) {
            let recipient = match action.target {
                LobbyTarget::Source => Recipient::Peer(event.peer),
                LobbyTarget::Broadcast => Recipient::Broadcast,
            };
            host.send(recipient, Packet::Lobby(action.packet))
                .await
                .context("host-lobby: отправка пакета")?;
        }
    }

    let status = lobby.status();
    tracing::info!("host-lobby: 34 отправлен, держим сессию {HOST_START_LINGER:?} для доставки");
    tokio::time::sleep(HOST_START_LINGER).await;

    Ok(HostLobbyReport {
        lobby: lobby.lobby_info(),
        status,
    })
}

/// Feeds one H1 event into the state machine and returns the reaction packets.
fn dispatch_host_event(lobby: &mut HostLobby, event: &HostEvent) -> Vec<LobbyOutgoing> {
    match &event.kind {
        HostEventKind::Connected => {
            tracing::debug!("host-lobby: ENet-соединение {} (ждём TP 00)", event.peer);
            Vec::new()
        }
        HostEventKind::Verified => {
            lobby.on_verified(event.peer.value());
            Vec::new()
        }
        HostEventKind::Packet(packet) => lobby.on_packet(event.peer.value(), packet),
        HostEventKind::Disconnected => lobby.on_disconnect(event.peer.value()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::input::{Command, Packet18Chat};
    use max_net::DataHash;

    fn config(min_players: usize) -> HostLobbyConfig {
        HostLobbyConfig {
            host_name: "Host".to_string(),
            host_team: 0,
            min_players,
            policy: SessionPolicy::Pause,
            settings: HostSessionSettings {
                seed: 0x0102_0304,
                ..HostSessionSettings::default()
            }
            .lobby_settings(),
        }
    }

    fn lobby(min_players: usize) -> HostLobby {
        HostLobby::new(config(min_players))
    }

    fn packet(packet: LobbyPacket) -> Packet {
        Packet::Lobby(packet)
    }

    /// 28 of a verified peer (`Remote_ReceiveNetPacket_28`).
    fn enter(lobby: &mut HostLobby, peer: u32) -> Vec<LobbyOutgoing> {
        lobby.on_verified(peer);
        lobby.on_packet(peer, &packet(LobbyPacket::EnterLobby { node: 0 }))
    }

    /// 29 of a peer from packet 44's `host_node`.
    fn register(lobby: &mut HostLobby, peer: u32) -> Vec<LobbyOutgoing> {
        lobby.on_packet(
            peer,
            &packet(LobbyPacket::Register {
                host_node: HOST_NODE_ID,
            }),
        )
    }

    /// The node id packet 30 assigned to the peer.
    fn welcome_node(actions: &[LobbyOutgoing]) -> u16 {
        actions
            .iter()
            .find_map(|action| match &action.packet {
                LobbyPacket::Welcome(welcome) => Some(welcome.player_node),
                _ => None,
            })
            .expect("29 must produce a welcome")
    }

    fn team_select(
        lobby: &mut HostLobby,
        peer: u32,
        node: u16,
        slot: u8,
        ready: bool,
    ) -> Vec<LobbyOutgoing> {
        lobby.on_packet(
            peer,
            &packet(LobbyPacket::TeamSelect {
                host_node: HOST_NODE_ID,
                player_node: node,
                team_slot: slot,
                ready,
                team_name: format!("Guest#{slot}"),
            }),
        )
    }

    /// 28 → 44 then 29 → 47 + 30, with node ids following the entry order.
    #[test]
    fn enter_and_register_assign_nodes_in_entry_order() {
        let mut session = lobby(2);

        let actions = enter(&mut session, 7);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].target, LobbyTarget::Source);
        match &actions[0].packet {
            LobbyPacket::HostAnnounce {
                player_node,
                game_version,
                player_name,
            } => {
                assert_eq!(*player_node, HOST_NODE_ID);
                assert_eq!(*game_version, DEFAULT_GAME_VERSION);
                assert_eq!(player_name, "Host");
            }
            other => panic!("28 must be answered with 44, got {other:?}"),
        }

        let actions = register(&mut session, 7);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].target, LobbyTarget::Broadcast);
        match &actions[0].packet {
            LobbyPacket::NodeRegistered { player_node, node } => {
                assert_eq!(*player_node, HOST_NODE_ID);
                assert_eq!(node.entity_id, FIRST_CLIENT_NODE);
                assert!(!node.is_host);
            }
            other => panic!("29 must produce 47, got {other:?}"),
        }

        let LobbyPacket::Welcome(welcome) = &actions[1].packet else {
            panic!("29 must produce 30");
        };
        assert_eq!(actions[1].target, LobbyTarget::Source);
        assert_eq!(welcome.player_node, FIRST_CLIENT_NODE);
        assert_eq!(welcome.host_node, HOST_NODE_ID);
        assert_eq!(welcome.team_nodes, [HOST_NODE_ID, 0, 0, 0]);
        assert_eq!(welcome.team_names[0].as_str(), "Host");
        assert_eq!(welcome.nodes.len(), 2);
        assert!(welcome.nodes[0].is_host);
        assert_eq!(welcome.nodes[0].entity_id, HOST_NODE_ID);
        assert_eq!(welcome.nodes[1].entity_id, FIRST_CLIENT_NODE);
        assert_eq!(welcome.settings, config(2).settings);

        // The second guest gets the next node id, not the same one.
        enter(&mut session, 9);
        let actions = register(&mut session, 9);
        let LobbyPacket::Welcome(second) = &actions[1].packet else {
            panic!("29 must produce 30");
        };
        assert_eq!(second.player_node, FIRST_CLIENT_NODE + 1);
        assert_eq!(second.nodes.len(), 3);
    }

    /// READY ≥ `min_players` broadcasts 32; the guest's 05 broadcasts 34.
    #[test]
    fn ready_players_reach_32_and_the_acknowledgement_reaches_34() {
        let mut session = lobby(2);
        enter(&mut session, 7);
        let actions = register(&mut session, 7);
        let node = welcome_node(&actions);

        let actions = team_select(&mut session, 7, node, 1, true);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].target, LobbyTarget::Broadcast);
        assert!(matches!(
            &actions[0].packet,
            LobbyPacket::Setup { host_node } if *host_node == HOST_NODE_ID
        ));
        assert!(session.status().setup_sent);
        assert_eq!(session.status().pending_acks, 1);

        let actions = session.on_packet(
            7,
            &packet(LobbyPacket::SetupAck {
                host_node: HOST_NODE_ID,
            }),
        );
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].target, LobbyTarget::Broadcast);
        assert!(matches!(
            &actions[0].packet,
            LobbyPacket::Start { host_node } if *host_node == HOST_NODE_ID
        ));
        assert!(session.is_complete());

        let info = session.lobby_info();
        assert_eq!(info.player_node, HOST_NODE_ID);
        assert_eq!(info.host_node, HOST_NODE_ID);
        assert_eq!(info.team_nodes, [HOST_NODE_ID, node, 0, 0]);
        assert_eq!(
            info.settings.as_ref().expect("settings").rng_seed,
            0x0102_0304
        );
        assert!(info.summary().contains("team_nodes=[10, 11, 0, 0]"));
    }

    /// The host slot cannot be stolen, and a node mismatch is rejected.
    #[test]
    fn team_select_rejects_the_host_slot_and_a_foreign_node() {
        let mut session = lobby(2);
        enter(&mut session, 7);
        let actions = register(&mut session, 7);
        let node = welcome_node(&actions);

        let actions = team_select(&mut session, 7, node, 0, true);
        assert!(actions.is_empty());
        assert_eq!(session.lobby_info().team_nodes[0], HOST_NODE_ID);
        assert_eq!(session.status().ready_players, 1);

        let actions = team_select(&mut session, 7, node + 5, 1, true);
        assert!(actions.is_empty());
        assert!(!session.status().setup_sent);
    }

    /// One READY guest is not enough when `--min-players` is 3.
    #[test]
    fn the_ready_threshold_is_inclusive_of_the_host() {
        let mut session = lobby(3);
        enter(&mut session, 7);
        let actions = register(&mut session, 7);
        let node = welcome_node(&actions);

        let actions = team_select(&mut session, 7, node, 1, true);
        assert!(actions.is_empty(), "2 of 3 players must not start");
        assert_eq!(session.status().ready_players, 2);
        assert!(!session.status().setup_sent);
    }

    /// A disconnect frees the jar; the remaining host slot is not overwritten.
    #[test]
    fn a_disconnect_clears_the_team_slot() {
        let mut session = lobby(3);
        enter(&mut session, 7);
        let actions = register(&mut session, 7);
        let node = welcome_node(&actions);
        session.on_packet(
            7,
            &packet(LobbyPacket::TeamSelect {
                host_node: HOST_NODE_ID,
                player_node: node,
                team_slot: 2,
                ready: true,
                team_name: "Guest".to_string(),
            }),
        );

        let actions = session.on_disconnect(7);
        assert!(actions.is_empty());
        assert_eq!(session.lobby_info().team_nodes, [HOST_NODE_ID, 0, 0, 0]);
        assert_eq!(session.status().peers, 0);
    }

    /// Packet 26 stores the clan by team slot (`Remote_ReceiveNetPacket_26`).
    #[test]
    fn clan_packets_are_stored_per_team_slot() {
        let mut session = lobby(2);
        session.on_verified(7);

        let actions = session.on_packet(
            7,
            &packet(LobbyPacket::Clan {
                team: TeamId(2),
                clan: TeamClan::Musashi,
            }),
        );

        assert!(actions.is_empty());
        assert_eq!(session.team_clan(2), Some(TeamClan::Musashi));
        assert_eq!(session.team_clan(0), None);
    }

    /// `--host-policy` names.
    #[test]
    fn session_policy_parses_the_cli_values() {
        assert_eq!(SessionPolicy::parse("pause"), Some(SessionPolicy::Pause));
        assert_eq!(SessionPolicy::parse("PAUSE"), Some(SessionPolicy::Pause));
        assert_eq!(
            SessionPolicy::parse("eliminate"),
            Some(SessionPolicy::EliminateAfter {
                ticks: DEFAULT_ELIMINATION_TICKS
            })
        );
        assert_eq!(SessionPolicy::parse("other"), None);
        assert_eq!(SessionPolicy::default().to_string(), "pause");
        assert_eq!(
            SessionPolicy::EliminateAfter { ticks: 1 }.to_string(),
            "eliminate"
        );
    }

    /// Gate H2 invariant: the host's `LobbyInfo` and the guest's `Welcome`
    /// (with its own slot patched locally, as `run_lobby` does) have the same
    /// identity line.
    #[test]
    fn host_and_guest_lobby_identity_agree() {
        let mut session = lobby(2);
        enter(&mut session, 7);
        let actions = register(&mut session, 7);
        let node = welcome_node(&actions);
        let welcome = actions
            .iter()
            .find_map(|action| match &action.packet {
                LobbyPacket::Welcome(welcome) => Some((**welcome).clone()),
                _ => None,
            })
            .expect("welcome");

        team_select(&mut session, 7, node, 1, true);

        // Guest side of `run_lobby`: `Welcome`, own team slot patched locally.
        let mut guest_team_nodes = welcome.team_nodes;
        guest_team_nodes[1] = node;
        let guest_info = LobbyInfo {
            handshake: true,
            player_node: node,
            host_node: welcome.host_node,
            team_nodes: guest_team_nodes,
            settings: Some(welcome.settings.clone()),
            ..LobbyInfo::default()
        };

        assert_eq!(session.lobby_info().summary(), guest_info.summary());
    }

    /// H3: the host's `HostIo` adapter maps transport events and keeps the match
    /// alive when a guest is lost (`DisconnectOutcome::Ignored`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_io_adapter_is_idle_and_ignores_a_lost_guest() {
        let socket = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe socket");
        let port = socket.local_addr().expect("probe addr").port();
        drop(socket);

        let config = HostConfig::new(
            Ipv4Addr::LOCALHOST,
            port,
            DataHash::from_bytes(b"max", b"patches"),
        );
        let mut host = HostHandle::bind(config).expect("bind host");

        assert!(
            matches!(
                host.next_io_event(Duration::from_millis(50))
                    .await
                    .expect("host event"),
                IoEvent::Idle
            ),
            "an empty server has no game event"
        );
        host.clear_barriers().await.expect("clear barriers");
        // The host is not a peer: broadcasting with an empty roster is a no-op.
        host.send_packet(Packet::Command(Command::Chat(Packet18Chat {
            sender: TeamId::RED,
            message: "no guests yet".to_string(),
        })))
        .await
        .expect("broadcast with no verified peers");
        assert_eq!(
            host.on_disconnect().await.expect("disconnect outcome"),
            DisconnectOutcome::Ignored,
            "H3 keeps waiting for a lost guest; H4 owns the session policy"
        );

        host.shutdown().await;
    }
}
