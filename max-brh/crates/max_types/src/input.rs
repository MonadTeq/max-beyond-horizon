//! Command input model — protocol packets as pure data.
//!
//! Oracle: `max-cpp-port/src/remote.cpp` (handlers `Remote_ReceiveNetPacket_*`
//! and order processors `Remote_OrderProcessor{0..5}_Read`), `max-brh-docs/protocol.md`
//! §8.4 (wire field formats) and DESIGN.md §2.4.
//!
//! Network encoding (ENet, little-endian, `NetPacket`) is **not here**: that is
//! `max_net`/`max_app`. `max_core` receives already parsed commands and applies
//! them to `GameState` (`max_core::game`) in arrival order
//! (`max_core::events`). Therefore all structures of the module are plain data: they can be
//! built directly from tests, from max_net and from replay scripts (DESIGN §8.2).
//!
//! ## Packet 08 processors
//!
//! As in C++, the payload format is chosen by the unit's *new* order
//! (`Remote_OrderProcessors[unit->GetOrder()]`), the processors are nested:
//! `P3 ⊃ P2 ⊃ P1`, `P4 ⊃ P3`, `P5 ⊃ P3`; [`OrderPayload`] mirrors the nesting
//! with struct fields so that serialization on the network side follows the same byte
//! order as in C++ (`Remote_OrderProcessor2_Read` first calls
//! `Remote_OrderProcessor1_Read`, and so on).
//!
//! ## Divergences from the C++ port (recorded deliberately)
//!
//! * Packet **41** in the C++ port carries only `unit_id` and `mode` (see
//!   `Remote_SendNetPacket_41`), while `max-brh-docs/protocol.md` §8.4 also describes
//!   `grid_x/grid_y/speed/group_speed/shots/move_fraction`. In
//!   [`Packet41MoveFinished`] these fields exist (the protocol is a contract with max_net), but
//!   they are applied only if they differ from the already known state; see
//!   `max_core::events::apply_command`.
//! * Packet **38** in the C++ port does not carry `grid_x/grid_y`, while `max-brh-docs/protocol.md`
//!   §8.4 describes them; the field is kept in the structure for compatibility and
//!   is applied as "the current cell of the owner" (C++: `Remote_RelocateUnit`).
//! * Packet **53** (complex totals) is not implemented in the C++ port at all; in Rust there are
//!   data + application (DESIGN §2.4 listed it as part of the input model).

use crate::enums::{PlayMode, UnitOrder, UnitOrderState, UnitType};
use crate::geo::{PathStep, Point};
use crate::team::{ResearchTopicInfo, TeamId};

/// Size of the command array in network structures (`TRANSPORT_MAX_TEAM_COUNT`).
pub const MAX_TEAM_COUNT: usize = 4;

/// Number of attributes carried by upgrade packets 10/20 (13 `u16`
/// values in a fixed order, `max-brh-docs/protocol.md` §8.4).
pub const UPGRADE_VALUE_COUNT: usize = 13;

/// Processor level of the packet 08 payload (`Remote_OrderProcessor{0..5}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PayloadLevel {
    /// `ORDER_17` — empty (`Remote_OrderProcessor0_Read`).
    Empty = 0,
    /// `P1`: order/state/prior/disabled_reaction_fire.
    P1 = 1,
    /// `P2` = `P1` + parent/experience.
    P2 = 2,
    /// `P3` = `P2` + move/fire/enemy/transfer/stealth.
    P3 = 3,
    /// `P4` = `P3` + build fields.
    P4 = 4,
    /// `P5` = `P3` + mining fields.
    P5 = 5,
}

impl PayloadLevel {
    /// Processor level for an order — an exact copy of the `switch` from
    /// `Remote_Init` (`remote.cpp:661-724`).
    pub const fn for_order(order: UnitOrder) -> Self {
        use UnitOrder::*;
        match order {
            // ORDER_17: «unused order slots»
            Order17 => Self::Empty,
            // P1
            PowerOn | PowerOff | Explode | Sentry | Land | TakeOff | Load | Idle | HaltBuilding
            | AwaitScaling | AwaitTapePositioning | LayMine | HaltBuilding2 => Self::P1,
            // P4 (contains P3)
            Transform | Build | Clear | Disable => Self::P4,
            // P3
            Await | Move | Fire | Activate | Transfer | AwaitStealUnit | AwaitDisableUnit | MoveToUnit
            | MoveToAttack => Self::P3,
            // P5 (contains P3)
            NewAllocate => Self::P5,
            // P2
            Unload | Repair | Reload | Upgrade => Self::P2,
        }
    }
}

/// Common fields of all processors except `P0` — C++ `Remote_OrderProcessor1_Read`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadP1 {
    /// New order (`< ORDER_COUNT_MAX`, otherwise the packet is rejected).
    pub order: UnitOrder,
    /// New order state.
    pub state: UnitOrderState,
    /// Queued order (`< ORDER_COUNT_MAX`).
    pub prior_order: UnitOrder,
    /// Queued state.
    pub prior_state: UnitOrderState,
    /// Return fire disabled (`bool` on the wire).
    pub disabled_reaction_fire: bool,
}

/// `P2` — C++ `Remote_OrderProcessor2_Read`: `P1` + parent and experience.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadP2 {
    /// Fields of `P1`.
    pub p1: PayloadP1,
    /// `parent_unit_id` (`u32` on the wire, 0 = no parent); in the model —
    /// `Option<UnitId>`.
    pub parent_unit: Option<crate::unit::UnitId>,
    /// `experience i16`.
    pub experience: i16,
}

/// `P3` — C++ `Remote_OrderProcessor3_Read`: `P2` + targets and cargo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadP3 {
    /// Fields of `P2`.
    pub p2: PayloadP2,
    /// `move_to_grid_x/y` (C++ runtime fields, in V70 a single `target_grid`).
    pub move_to: Point,
    /// `fire_on_grid_x/y`.
    pub fire_on: Point,
    /// `enemy_unit_id` (0 = none).
    pub enemy_unit: Option<crate::unit::UnitId>,
    /// `transfer_cargo i16`.
    pub transfer_cargo: i16,
    /// `stealth_dice_roll u8`.
    pub stealth_dice_roll: u8,
}

/// `P4` — C++ `Remote_OrderProcessor4_Read`: `P3` + build fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadP4 {
    /// Fields of `P3`.
    pub p3: PayloadP3,
    /// `repeat_build` (`bool`).
    pub repeat_build: bool,
    /// `build_time u8`.
    pub build_time: u8,
    /// `build_rate u16`.
    pub build_rate: u16,
    /// Build queue (`unit_count u32` + `unit_count × ResourceID u16`).
    pub build_list: Vec<UnitType>,
}

/// `P5` — C++ `Remote_OrderProcessor5_Read`: `P3` + harvest rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadP5 {
    /// Fields of `P3`.
    pub p3: PayloadP3,
    /// `total_mining u8`.
    pub total_mining: u8,
    /// `raw_mining u8`.
    pub raw_mining: u8,
    /// `fuel_mining u8`.
    pub fuel_mining: u8,
    /// `gold_mining u8`.
    pub gold_mining: u8,
}

/// Payload of packet 08 (one of the six processors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderPayload {
    /// `P0`: `ORDER_17` — empty.
    Empty,
    /// `P1`.
    P1(PayloadP1),
    /// `P2`.
    P2(PayloadP2),
    /// `P3`.
    P3(PayloadP3),
    /// `P4`.
    P4(PayloadP4),
    /// `P5`.
    P5(PayloadP5),
}

impl OrderPayload {
    /// Processor level of this payload.
    pub const fn level(&self) -> PayloadLevel {
        match self {
            Self::Empty => PayloadLevel::Empty,
            Self::P1(_) => PayloadLevel::P1,
            Self::P2(_) => PayloadLevel::P2,
            Self::P3(_) => PayloadLevel::P3,
            Self::P4(_) => PayloadLevel::P4,
            Self::P5(_) => PayloadLevel::P5,
        }
    }

    /// Common fields of `P1`, if the processor has them.
    pub const fn p1(&self) -> Option<&PayloadP1> {
        match self {
            Self::Empty => None,
            Self::P1(p1) => Some(p1),
            Self::P2(p2) => Some(&p2.p1),
            Self::P3(p3) => Some(&p3.p2.p1),
            Self::P4(p4) => Some(&p4.p3.p2.p1),
            Self::P5(p5) => Some(&p5.p3.p2.p1),
        }
    }

    /// New order (if the payload carries one).
    pub fn order(&self) -> Option<UnitOrder> {
        self.p1().map(|p1| p1.order)
    }

    /// Check that "the processor matches the order" — as in C++: the payload format
    /// is chosen by the unit's new order (`Remote_ReceiveNetPacket_08`), so
    /// a mismatch between the level and the order is treated as a malformed packet.
    pub fn matches_order(&self, order: UnitOrder) -> bool {
        self.level() == PayloadLevel::for_order(order) && self.order() == Some(order)
    }
}

/// Packet **08** — order to a unit of order 08 (`C→S multicast`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet08Order {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
    /// Processor payload.
    pub payload: OrderPayload,
}

/// Path data in packet 38.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MovePathData {
    /// Ground path: `steps_count × PathStep { i8, i8 }`.
    Ground(Vec<PathStep>),
    /// Air path (exactly 1 step): a straight segment in pixels.
    Air {
        /// `end_x i16`.
        end_x: i16,
        /// `end_y i16`.
        end_y: i16,
        /// `distance_x i32`.
        distance_x: i32,
        /// `distance_y i32`.
        distance_y: i32,
        /// `euclidean_distance i16`.
        euclidean_distance: i16,
        /// `speed u16` (movement remaining at the moment of sending).
        speed: u16,
        /// `move_fraction u8`.
        move_fraction: u8,
        /// `max_velocity u8`.
        max_velocity: u8,
    },
}

/// Packet **38** — movement with a ready path (the path owner broadcasts it to peers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet38Move {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
    /// `order u8` (new order).
    pub order: UnitOrder,
    /// `order_state u8`.
    pub state: UnitOrderState,
    /// `grid_x/grid_y` — the owner's cell at the moment of sending (relocation
    /// of the unit at the receiver, `Remote_RelocateUnit`).
    pub grid: Point,
    /// `move_to_grid_x/y`.
    pub move_to: Point,
    /// `group_speed u16`.
    pub group_speed: u16,
    /// Path (`steps_count`; 0 — no path).
    pub path: Option<MovePathData>,
}

/// Packet **41** — movement finished (`BlockedOnPathRequest` at the receiver).
///
/// In the C++ port only `unit_id` + `mode` are transmitted; the other fields are from
/// `max-brh-docs/protocol.md` §8.4 (see the module header). They are `Option` so that the sender
/// can build the "port" (short) form of the packet: `None` = the field is not transmitted and
/// is not applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet41MoveFinished {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
    /// `mode u8(bool)` — "reactions after movement" (`MoveFinished(mode)`).
    pub mode: bool,
    /// `grid_x i16`.
    pub grid: Option<Point>,
    /// `speed u16`.
    pub speed: Option<u16>,
    /// `group_speed u16`.
    pub group_speed: Option<u16>,
    /// `shots u8`.
    pub shots: Option<u8>,
    /// `move_fraction u8`.
    pub move_fraction: Option<u8>,
}

impl Packet41MoveFinished {
    /// Short form of the C++ port: only `unit_id` and `mode`.
    pub const fn port_form(unit: crate::unit::UnitId, mode: bool) -> Self {
        Self {
            unit,
            mode,
            grid: None,
            speed: None,
            group_speed: None,
            shots: None,
            move_fraction: None,
        }
    }
}

/// Packet **50** — emergency unit stop (creates `UnitEventEmergencyStop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet50StopUnit {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
}

/// Packet **06** — the team has finished its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet06FinishedTurn {
    /// `team u16`.
    pub team: TeamId,
}

/// Packet **52** — end-of-turn barrier (the team's `turn index`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet52EndTurnBarrier {
    /// `team u16`.
    pub team: TeamId,
    /// `counter u8`.
    pub counter: u8,
}

/// Packet **45** — CRC barrier (number of the next turn + the team's CRC16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet45CrcBarrier {
    /// `team u16`.
    pub team: TeamId,
    /// `next_turn_index u8`.
    pub next_turn_index: u8,
    /// `crc16 u16`.
    pub crc16: u16,
}

/// Packet **00** — start-of-turn barrier (the team's frame counter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet00TurnBarrier {
    /// `team u16`.
    pub team: TeamId,
    /// `counter u8`.
    pub counter: u8,
}

/// Packet **01** — per-frame barrier (real-time pacing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet01FrameBarrier {
    /// `team u16`.
    pub team: TeamId,
    /// `counter u8` (6 bits).
    pub counter: u8,
}

/// Packet **18** — in-game chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet18Chat {
    /// `sender_team u16`.
    pub sender: TeamId,
    /// `message SmartString`.
    pub message: String,
}

/// Packet **11** — order to a complex (fields of `Complex::WritePacket`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet11ComplexOrder {
    /// `team u16`.
    pub team: TeamId,
    /// `complex_id u16`.
    pub complex_id: u16,
    /// `raw i16` (C++/wire: `material`).
    pub raw: i16,
    /// `fuel i16`.
    pub fuel: i16,
    /// `gold i16`.
    pub gold: i16,
    /// `power i16`.
    pub power: i16,
    /// `workers i16`.
    pub workers: i16,
    /// `buildings i16`.
    pub buildings: i16,
}

/// Totals of one complex in packet 53.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComplexSummaryEntry {
    /// `complex_id u16`.
    pub complex_id: u16,
    /// Six `i16` fields in packet 11/53 order
    /// (`raw, fuel, gold, power, workers, buildings`; C++ order
    /// `material, fuel, …`, `raw` ≡ `material`).
    pub values: [i16; 6],
}

/// Packet **53** — totals of the team's complexes at the start of the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet53ComplexSummary {
    /// `team u16`.
    pub team: TeamId,
    /// `turn i32`.
    pub turn: i32,
    /// `complex_count u16` + records.
    pub complexes: Vec<ComplexSummaryEntry>,
}

/// Packet **09** — team state: gold, upgrade spending, research allocation, name.
///
/// Oracle: `Remote_SendNetPacket_09` / `Remote_ReceiveNetPacket_09`
/// (`max-cpp-port/src/remote.cpp:2067/2088`). Wire layout (little endian, byte for
/// byte):
///
/// | Field | Type |
/// |---|---|
/// | type | `u8` (`0x59` = `TRANSPORT_APPL_PACKET_ID` + 9) |
/// | team | `u16` |
/// | gold | `u32` |
/// | gold_spent_on_upgrades | `u32` |
/// | research_topics | 96 B — 8 × { `i32` research_level, `i32` turns_to_complete, `i32` allocation } |
/// | name | `SmartString` — `u16` length (including the NUL) + bytes + NUL |
///
/// The oracle sends it as `REMOTE_MULTICAST` from `UpgradeMenu::CommitUpgradeChanges`
/// (`upgrademenu.cpp:97-101`), `ResearchMenu_OnClick_Done`
/// (`researchmenu.cpp:558-560`), `OptionsMenu` (`optionsmenu.cpp:558`) and the AI
/// (`aiplayer.cpp:370/2518`) — a *broadcast* packet, not a local one: gold and the
/// research allocation are part of the team state, so they travel the same channel
/// as the simulation commands.
///
/// Receive semantics (oracle): validate the team index, then assign `gold`,
/// `gold_spent_on_upgrades` and the whole `research_topics` array.
///
/// The port (`max_core::events::apply_packet_09`) performs those assignments and
/// additionally treats the `allocation` field as the *research menu target*: the
/// owner's own `Done` now travels through this packet instead of writing
/// `GameState`, so the receiver runs the same per-center loop C++ runs inline in
/// `ResearchMenu_OnClick_Done` (`GameState::set_research_allocation`). A packet that
/// repeats the current array (the gold-only upgrade commit) skips the loop. See the
/// apply function for the full rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet09TeamState {
    /// `team u16` (`Remote_IsValidTeamIndex` — 0..3).
    pub team: TeamId,
    /// `team_units->GetGold()` — the absolute gold value (not a delta).
    pub gold: u32,
    /// `stats_gold_spent_on_upgrades` — the absolute cumulative value (not a delta).
    pub gold_spent_on_upgrades: u32,
    /// 8 research topics in `RESEARCH_TOPIC_*` order (96 raw bytes on the wire).
    pub research: [ResearchTopicInfo; crate::enums::RESEARCH_TOPIC_COUNT],
    /// Team name (`SmartString` from `settings[menu_team_name_setting[team]]`).
    pub name: String,
}

/// Packet **10** — upgrade of a unit type of the team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet10UpgradeUnitType {
    /// `team u16`.
    pub team: TeamId,
    /// `unit_type u16` (`ResourceID`).
    pub unit_type: UnitType,
    /// 13 values in the order `turns, hits, armor, attack, speed, range, rounds,
    /// move_and_fire, scan, storage, ammo, attack_radius, agent_adjust`.
    pub values: [u16; UPGRADE_VALUE_COUNT],
}

/// Packet **20** — upgrade of a specific unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet20UpgradeUnit {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
    /// 13 values (order as in packet 10).
    pub values: [u16; UPGRADE_VALUE_COUNT],
}

/// Packet **43** — unit rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet43Rename {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
    /// `name SmartString`.
    pub name: String,
}

/// Packet **13** — RNG seed (the only seeding point of the simulation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet13Seed {
    /// `rng_seed u32`.
    pub seed: u32,
}

/// Packet **14** — unit landing (quick build / deployment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet14DeployUnit {
    /// `team u16`.
    pub team: TeamId,
    /// `unit_type u16`.
    pub unit_type: UnitType,
    /// `grid_x i32`.
    pub grid: Point,
}

/// Mission supply — record of packet 12.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissionSupplyEntry {
    /// `unit_type u16`.
    pub unit_type: UnitType,
    /// `cargo u16`.
    pub cargo: u16,
}

/// Packet **12** — landing point selection and mission supply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet12MissionSupply {
    /// `team u16`.
    pub team: TeamId,
    /// `round u8`.
    pub round: u8,
    /// `start_gold u16`.
    pub start_gold: u16,
    /// `team_gold u16`.
    pub team_gold: u16,
    /// `unit_count u32` + records.
    pub units: Vec<MissionSupplyEntry>,
    /// `gold_spent_on_upgrades u32`.
    pub gold_spent_on_upgrades: u32,
    /// `starting_position Point`.
    pub starting_position: Point,
    /// `proximity_alert_ack u8`.
    pub proximity_alert_ack: u8,
}

/// Packet type identifier (`max-brh-docs/protocol.md` §8.4, numbers after
/// `TRANSPORT_APPL_PACKET_ID`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum PacketKind {
    /// Start-of-turn barrier.
    BarrierTurn = 0x00,
    /// Per-frame barrier.
    BarrierFrame = 0x01,
    /// End of the team's turn.
    FinishedTurn = 0x06,
    /// Order to a unit.
    OrderUnit = 0x08,
    /// Team state (gold/upgrade spending/research allocation).
    TeamState = 0x09,
    /// Unit type upgrade.
    UpgradeUnitType = 0x0A,
    /// Order to a complex.
    ComplexOrder = 0x0B,
    /// Landing point and mission supply.
    MissionSupply = 0x0C,
    /// Seed RNG.
    Seed = 0x0D,
    /// Unit landing.
    DeployUnit = 0x0E,
    /// Chat.
    Chat = 0x12,
    /// Upgrade of a specific unit.
    UpgradeUnit = 0x14,
    /// Movement with a path.
    MoveWithPath = 0x26,
    /// Movement finished.
    MoveFinished = 0x29,
    /// Rename.
    Rename = 0x2B,
    /// CRC barrier.
    CrcBarrier = 0x2D,
    /// Emergency stop.
    StopUnit = 0x32,
    /// End-of-turn barrier.
    EndTurnBarrier = 0x34,
    /// Complex totals.
    ComplexSummary = 0x35,
}

/// One input command — the result of parsing a single APPL packet.
///
/// Application — `max_core::events::apply_command`, strictly in order of receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// 08.
    Order(Packet08Order),
    /// 38.
    Move(Packet38Move),
    /// 41.
    MoveFinished(Packet41MoveFinished),
    /// 50.
    Stop(Packet50StopUnit),
    /// 06.
    FinishedTurn(Packet06FinishedTurn),
    /// 52.
    EndTurnBarrier(Packet52EndTurnBarrier),
    /// 45.
    CrcBarrier(Packet45CrcBarrier),
    /// 00.
    TurnBarrier(Packet00TurnBarrier),
    /// 01.
    FrameBarrier(Packet01FrameBarrier),
    /// 18.
    Chat(Packet18Chat),
    /// 11.
    ComplexOrder(Packet11ComplexOrder),
    /// 53.
    ComplexSummary(Packet53ComplexSummary),
    /// 09.
    TeamState(Packet09TeamState),
    /// 10.
    UpgradeUnitType(Packet10UpgradeUnitType),
    /// 20.
    UpgradeUnit(Packet20UpgradeUnit),
    /// 43.
    Rename(Packet43Rename),
    /// 13.
    Seed(Packet13Seed),
    /// 14.
    Deploy(Packet14DeployUnit),
    /// 12.
    MissionSupply(Packet12MissionSupply),
}

impl Command {
    /// Command packet type.
    pub const fn kind(&self) -> PacketKind {
        match self {
            Self::Order(_) => PacketKind::OrderUnit,
            Self::Move(_) => PacketKind::MoveWithPath,
            Self::MoveFinished(_) => PacketKind::MoveFinished,
            Self::Stop(_) => PacketKind::StopUnit,
            Self::FinishedTurn(_) => PacketKind::FinishedTurn,
            Self::EndTurnBarrier(_) => PacketKind::EndTurnBarrier,
            Self::CrcBarrier(_) => PacketKind::CrcBarrier,
            Self::TurnBarrier(_) => PacketKind::BarrierTurn,
            Self::FrameBarrier(_) => PacketKind::BarrierFrame,
            Self::Chat(_) => PacketKind::Chat,
            Self::ComplexOrder(_) => PacketKind::ComplexOrder,
            Self::ComplexSummary(_) => PacketKind::ComplexSummary,
            Self::TeamState(_) => PacketKind::TeamState,
            Self::UpgradeUnitType(_) => PacketKind::UpgradeUnitType,
            Self::UpgradeUnit(_) => PacketKind::UpgradeUnit,
            Self::Rename(_) => PacketKind::Rename,
            Self::Seed(_) => PacketKind::Seed,
            Self::Deploy(_) => PacketKind::DeployUnit,
            Self::MissionSupply(_) => PacketKind::MissionSupply,
        }
    }

    /// Whether the command affects the game simulation (and not just barriers/UI).
    ///
    /// Used by determinism tests and diagnostics: barrier packets
    /// (00/01/45/52) are network facts for advancing phases, they do not change
    /// the state of units.
    pub const fn affects_simulation(&self) -> bool {
        !matches!(
            self,
            Self::EndTurnBarrier(_)
                | Self::CrcBarrier(_)
                | Self::TurnBarrier(_)
                | Self::FrameBarrier(_)
                | Self::Chat(_)
        )
    }
}

/// Check `Remote_IsValidTeamIndex` (`remote.cpp`: `entity_id <
/// PLAYER_TEAM_MAX`), but without the ALIEN slot: network packets are addressed only to
/// game teams 0..3.
pub fn is_valid_team_index(team: TeamId) -> bool {
    (team.0 as usize) < MAX_TEAM_COUNT
}

/// Order range check (`order < ORDER_COUNT_MAX`, including prior).
///
/// In C++ the order on the wire is a raw `u8`, and the receiver rejects values
/// `>= ORDER_COUNT_MAX` (`remote.cpp:394`). In the Rust model the order is a
/// `#[repr(u8)]` enum, so a value out of range cannot exist;
/// the function is kept as an explicit parity point with the C++ check.
pub fn is_valid_order(order: UnitOrder) -> bool {
    (order as u8) < crate::enums::ORDER_COUNT_MAX
}

/// Game mode in which the whole turn of one team is processed to the end
/// (`PLAY_MODE_TURN_BASED`).
pub const PLAY_MODE_TURN_BASED: PlayMode = PlayMode::TurnBased;

/// Simultaneous turns mode (`PLAY_MODE_SIMULTANEOUS_MOVES`).
pub const PLAY_MODE_SIMULTANEOUS_MOVES: PlayMode = PlayMode::SimultaneousMoves;

#[cfg(test)]
mod tests {
    use super::*;

    fn p1(order: UnitOrder) -> PayloadP1 {
        PayloadP1 {
            order,
            state: UnitOrderState::Init,
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
    fn processor_levels_match_cpp_init_table() {
        use UnitOrder::*;
        // Exact copy of the switch from Remote_Init (remote.cpp:661-724).
        assert_eq!(PayloadLevel::for_order(Order17), PayloadLevel::Empty);
        for order in [
            PowerOn,
            PowerOff,
            Explode,
            Sentry,
            Land,
            TakeOff,
            Load,
            Idle,
            HaltBuilding,
            AwaitScaling,
            AwaitTapePositioning,
            LayMine,
            HaltBuilding2,
        ] {
            assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P1, "{order:?}");
        }
        for order in [Transform, Build, Clear, Disable] {
            assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P4, "{order:?}");
        }
        for order in [
            Await,
            Move,
            Fire,
            Activate,
            Transfer,
            AwaitStealUnit,
            AwaitDisableUnit,
            MoveToUnit,
            MoveToAttack,
        ] {
            assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P3, "{order:?}");
        }
        assert_eq!(
            PayloadLevel::for_order(NewAllocate),
            PayloadLevel::P5
        );
        for order in [Unload, Repair, Reload, Upgrade] {
            assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P2, "{order:?}");
        }
        // All 32 orders are distributed (no forgotten variants).
        for order in UnitOrder::ALL {
            let _ = PayloadLevel::for_order(*order);
        }
    }

    #[test]
    fn payload_nesting_exposes_p1_fields() {
        let payload = OrderPayload::P4(PayloadP4 {
            p3: p3(UnitOrder::Build),
            repeat_build: true,
            build_time: 3,
            build_rate: 2,
            build_list: vec![UnitType::Tank],
        });
        assert_eq!(payload.level(), PayloadLevel::P4);
        assert_eq!(payload.order(), Some(UnitOrder::Build));
        assert!(payload.matches_order(UnitOrder::Build));
        assert!(!payload.matches_order(UnitOrder::Move));

        let empty = OrderPayload::Empty;
        assert_eq!(empty.level(), PayloadLevel::Empty);
        assert_eq!(empty.order(), None);
        assert!(!empty.matches_order(UnitOrder::Order17));
    }

    #[test]
    fn command_kinds_match_protocol_numbers() {
        let stop = Command::Stop(Packet50StopUnit {
            unit: crate::unit::UnitId(1),
        });
        assert_eq!(stop.kind(), PacketKind::StopUnit);
        assert_eq!(PacketKind::StopUnit as u8, 0x32);
        assert!(stop.affects_simulation());

        let chat = Command::Chat(Packet18Chat {
            sender: TeamId(0),
            message: "hello".into(),
        });
        assert!(!chat.affects_simulation());
        assert_eq!(PacketKind::Chat as u8, 0x12);

        // Packet 09 is a *simulation* command: gold and the research allocation
        // are part of the team state (`remote.cpp:2067-2107`).
        let team_state = Command::TeamState(Packet09TeamState {
            team: TeamId(0),
            gold: 1,
            gold_spent_on_upgrades: 2,
            research: [ResearchTopicInfo::default(); crate::enums::RESEARCH_TOPIC_COUNT],
            name: "RED".into(),
        });
        assert_eq!(team_state.kind(), PacketKind::TeamState);
        assert_eq!(PacketKind::TeamState as u8, 0x09);
        assert!(team_state.affects_simulation());
    }

    #[test]
    fn team_and_order_validation() {
        assert!(is_valid_team_index(TeamId(0)));
        assert!(is_valid_team_index(TeamId(3)));
        assert!(!is_valid_team_index(TeamId(4)));
        assert!(!is_valid_team_index(TeamId(5)));
        assert!(is_valid_order(UnitOrder::HaltBuilding2));
        assert_eq!(PLAY_MODE_TURN_BASED, PlayMode::TurnBased);
        assert_eq!(PLAY_MODE_SIMULTANEOUS_MOVES, PlayMode::SimultaneousMoves);
    }
}
