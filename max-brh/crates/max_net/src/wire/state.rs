//! Audit/synchronisation and auxiliary in-game packets (`max-brh-docs/protocol.md`
//! §8.3–§8.6).
//!
//! These packets either carry whole-unit snapshots (23/54 — the closest thing
//! to a "snapshot" in a lockstep protocol), team-level bookkeeping (09/17), or
//! session control facts (07/24/39/40/42/46/48/49/51). None of them is a
//! [`max_types::input::Command`], so their DTOs live here; the numbers are kept
//! raw where the simulation has no typed counterpart (the client only forwards
//! or logs them).

use max_core::unit::UnitInfo;
use max_types::enums::TeamClan;
use max_types::team::{ResearchTopicInfo, TeamId};
use max_types::unit::{UnitId, UnitListKind};

use super::reader::{Reader, WireError, WireResult, Writer, MAX_STRING_LEN, MAX_VECTOR_LEN};

/// `RESEARCH_TOPIC_COUNT` — research slots per team.
pub const RESEARCH_TOPIC_COUNT: usize = 8;
/// `UNIT_END` — number of unit counters in the packet 54 tail.
pub const UNIT_END: usize = 93;
/// Number of `i32` fields in packet 17.
pub const GAME_OPTION_COUNT: usize = 38;
/// `NetPacket::MaximumArrayCount` used by packet 54 (`count` is `u16`).
pub const MAX_SYNC_UNITS: usize = MAX_VECTOR_LEN;
/// How many units C++ puts into one packet 54 (`Remote_SendUnitStates`,
/// `remote.cpp:4334`: `units_per_packet = 32`).
///
/// Rust cuts chunks smaller: [`super::MAX_PACKET_SIZE`] (1440) is hard-checked
/// by the codec, and 32 units are `7 + 32×47 = 1511` bytes without the tail and `1989` with it.
/// The C++ sender hands such a packet to ENet for fragmentation; the Rust decoder
/// would reject it as oversize, so [`state_sync_for`] uses
/// [`MAX_UNITS_PER_CHUNK`]/[`MAX_UNITS_LAST_CHUNK`]. Format 54 does not depend
/// on chunk size: the receiver accepts any `count <= MAX_SYNC_UNITS`.
pub const STATE_SYNC_UNITS_PER_PACKET: usize = 32;
/// Maximum units in a non-final chunk: `(1440 - 7) / 47 = 30`.
const MAX_UNITS_PER_CHUNK: usize = 30;
/// Maximum units in the last chunk: the tail is 478 bytes,
/// `(1440 - 7 - 478) / 47 = 20`.
const MAX_UNITS_LAST_CHUNK: usize = 20;

/// One research topic in the packet-54 tail: `i32 level, i32 turns, i32 allocation`.
///
/// Packet 09 carries the same 96-byte array, but as a simulation command
/// (`Command::TeamState` → `wire::command`), so it does not use this DTO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResearchTopicWire {
    /// Research level.
    pub research_level: i32,
    /// Turns left to complete the current level.
    pub turns_to_complete: i32,
    /// Allocated resources.
    pub allocation: i32,
}

/// Canonical unit state used by audit packet 23 and state-sync packet 54
/// (`Packet23Data` in `remote.cpp` / `max-brh-docs/protocol.md` §8.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnitState {
    /// Current order (`UnitOrderType`, raw byte).
    pub orders: u8,
    /// Current order state (`UnitOrderStateType`, raw byte).
    pub state: u8,
    /// Deferred order.
    pub prior_orders: u8,
    /// Deferred order state.
    pub prior_state: u8,
    /// Reaction fire disabled.
    pub disabled_reaction_fire: bool,
    /// Parent unit id (`0` = none).
    pub parent_unit_id: u16,
    /// Experience.
    pub experience: i16,
    /// Move target grid X.
    pub move_to_grid_x: i16,
    /// Move target grid Y.
    pub move_to_grid_y: i16,
    /// Fire target grid X.
    pub fire_on_grid_x: i16,
    /// Fire target grid Y.
    pub fire_on_grid_y: i16,
    /// Enemy unit id (`0` = none).
    pub enemy_unit_id: u16,
    /// Transfer cargo.
    pub transfer_cargo: i16,
    /// Stealth dice roll.
    pub stealth_dice_roll: u8,
    /// Mining counters.
    pub total_mining: u8,
    /// Raw mining.
    pub raw_mining: u8,
    /// Fuel mining.
    pub fuel_mining: u8,
    /// Gold mining.
    pub gold_mining: u8,
    /// Build timer.
    pub build_time: u8,
    /// Build rate.
    pub build_rate: u16,
    /// Unit type (`ResourceID`).
    pub unit_type: u16,
    /// `UnitInfo::unit_id` (object index, one byte on the wire).
    pub unit_id: u8,
    /// Grid X.
    pub grid_x: i16,
    /// Grid Y.
    pub grid_y: i16,
    /// Team index (one byte).
    pub team: u8,
    /// Hit points.
    pub hits: u16,
    /// Speed.
    pub speed: u16,
    /// Shots.
    pub shots: u8,
    /// Storage.
    pub storage: i16,
    /// Ammo.
    pub ammo: u8,
}

impl UnitState {
    fn read(r: &mut Reader<'_>) -> WireResult<Self> {
        Ok(Self {
            orders: r.read_u8()?,
            state: r.read_u8()?,
            prior_orders: r.read_u8()?,
            prior_state: r.read_u8()?,
            disabled_reaction_fire: r.read_bool()?,
            parent_unit_id: r.read_u16()?,
            experience: r.read_i16()?,
            move_to_grid_x: r.read_i16()?,
            move_to_grid_y: r.read_i16()?,
            fire_on_grid_x: r.read_i16()?,
            fire_on_grid_y: r.read_i16()?,
            enemy_unit_id: r.read_u16()?,
            transfer_cargo: r.read_i16()?,
            stealth_dice_roll: r.read_u8()?,
            total_mining: r.read_u8()?,
            raw_mining: r.read_u8()?,
            fuel_mining: r.read_u8()?,
            gold_mining: r.read_u8()?,
            build_time: r.read_u8()?,
            build_rate: r.read_u16()?,
            unit_type: r.read_u16()?,
            unit_id: r.read_u8()?,
            grid_x: r.read_i16()?,
            grid_y: r.read_i16()?,
            team: r.read_u8()?,
            hits: r.read_u16()?,
            speed: r.read_u16()?,
            shots: r.read_u8()?,
            storage: r.read_i16()?,
            ammo: r.read_u8()?,
        })
    }

    fn write(&self, w: &mut Writer) {
        w.write_u8(self.orders);
        w.write_u8(self.state);
        w.write_u8(self.prior_orders);
        w.write_u8(self.prior_state);
        w.write_bool(self.disabled_reaction_fire);
        w.write_u16(self.parent_unit_id);
        w.write_i16(self.experience);
        w.write_i16(self.move_to_grid_x);
        w.write_i16(self.move_to_grid_y);
        w.write_i16(self.fire_on_grid_x);
        w.write_i16(self.fire_on_grid_y);
        w.write_u16(self.enemy_unit_id);
        w.write_i16(self.transfer_cargo);
        w.write_u8(self.stealth_dice_roll);
        w.write_u8(self.total_mining);
        w.write_u8(self.raw_mining);
        w.write_u8(self.fuel_mining);
        w.write_u8(self.gold_mining);
        w.write_u8(self.build_time);
        w.write_u16(self.build_rate);
        w.write_u16(self.unit_type);
        w.write_u8(self.unit_id);
        w.write_i16(self.grid_x);
        w.write_i16(self.grid_y);
        w.write_u8(self.team);
        w.write_u16(self.hits);
        w.write_u16(self.speed);
        w.write_u8(self.shots);
        w.write_i16(self.storage);
        w.write_u8(self.ammo);
    }
}

/// Trailing team statistics of the last chunk of packet 54.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSyncTail {
    /// Team gold.
    pub gold: u32,
    /// Gold spent on upgrades.
    pub gold_spent_on_upgrades: u32,
    /// Eight research topics (96 bytes).
    pub research: [ResearchTopicWire; RESEARCH_TOPIC_COUNT],
    /// Objects created counter.
    pub number_of_objects_created: u16,
    /// 93 unit-id counters (372 bytes).
    pub unit_counters: [u32; UNIT_END],
}

impl Default for StateSyncTail {
    fn default() -> Self {
        Self {
            gold: 0,
            gold_spent_on_upgrades: 0,
            research: [ResearchTopicWire::default(); RESEARCH_TOPIC_COUNT],
            number_of_objects_created: 0,
            unit_counters: [0; UNIT_END],
        }
    }
}

/// Packet 54 — authoritative state of one team's units (chunked).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSync {
    /// Team whose units are described.
    pub team: TeamId,
    /// Check index (distinguishes rounds).
    pub sync_index: u8,
    /// Last chunk of the team.
    pub is_last: bool,
    /// Chunk entries: `unit_id` + [`UnitState`].
    pub units: Vec<(UnitId, UnitState)>,
    /// Present only when `is_last`.
    pub tail: Option<StateSyncTail>,
}

/// Packet 17 — full game options (`GameManager_GameState` + 38 × i32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameOptions {
    /// Team field (the server sends 0).
    pub team: TeamId,
    /// `GameManager_GameState`.
    pub game_state: u8,
    /// The 38 `i32` option values in protocol order.
    pub values: [i32; GAME_OPTION_COUNT],
}

/// Packet 16 — save request (`file`, `title` are `SmartString`s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    /// Team field (0 from the client).
    pub team: TeamId,
    /// File name, must match `save<1-3 digits>.<ext>`.
    pub file: String,
    /// Save title.
    pub title: String,
}

// ---------------------------------------------------------------------------
// Decoders
// ---------------------------------------------------------------------------

/// Decodes audit packet 23: `unit_id` + [`UnitState`].
pub fn decode_audit(r: &mut Reader<'_>) -> WireResult<(UnitId, UnitState)> {
    let unit = UnitId(r.read_u16()?);
    let state = UnitState::read(r)?;
    Ok((unit, state))
}

/// Encodes audit packet 23 body.
pub fn encode_audit(unit: UnitId, state: &UnitState, w: &mut Writer) {
    w.write_u16(unit.0);
    state.write(w);
}

/// Decodes packet 54.
pub fn decode_state_sync(r: &mut Reader<'_>) -> WireResult<StateSync> {
    let team = TeamId(r.read_u16()? as u8);
    let sync_index = r.read_u8()?;
    let is_last = r.read_bool()?;
    let count = r.read_u16()? as usize;
    if count > MAX_SYNC_UNITS {
        return Err(WireError::VectorTooLong {
            len: count,
            max: MAX_SYNC_UNITS,
        });
    }
    let mut units = Vec::with_capacity(count);
    for _ in 0..count {
        let unit = UnitId(r.read_u16()?);
        let state = UnitState::read(r)?;
        units.push((unit, state));
    }

    let tail = if is_last {
        // Wire order: gold, gold_spent, research[8], objects_created,
        // unit_counters[93] (`remote.cpp` `Remote_ReceiveNetPacket_54`).
        let gold = r.read_u32()?;
        let gold_spent_on_upgrades = r.read_u32()?;
        let mut research = [ResearchTopicWire::default(); RESEARCH_TOPIC_COUNT];
        for topic in &mut research {
            *topic = ResearchTopicWire {
                research_level: r.read_i32()?,
                turns_to_complete: r.read_i32()?,
                allocation: r.read_i32()?,
            };
        }
        let number_of_objects_created = r.read_u16()?;
        let mut unit_counters = [0u32; UNIT_END];
        for counter in &mut unit_counters {
            *counter = r.read_u32()?;
        }
        Some(StateSyncTail {
            gold,
            gold_spent_on_upgrades,
            research,
            number_of_objects_created,
            unit_counters,
        })
    } else {
        None
    };

    Ok(StateSync {
        team,
        sync_index,
        is_last,
        units,
        tail,
    })
}

/// Decodes packet 17.
pub fn decode_game_options(r: &mut Reader<'_>) -> WireResult<GameOptions> {
    let team = TeamId(r.read_u16()? as u8);
    let game_state = r.read_u8()?;
    let mut values = [0i32; GAME_OPTION_COUNT];
    for value in &mut values {
        *value = r.read_i32()?;
    }
    Ok(GameOptions {
        team,
        game_state,
        values,
    })
}

/// Decodes packet 16.
pub fn decode_save_request(r: &mut Reader<'_>) -> WireResult<SaveRequest> {
    let team = TeamId(r.read_u16()? as u8);
    let file = r.read_smart_string()?;
    let title = r.read_smart_string()?;
    Ok(SaveRequest { team, file, title })
}

// ---------------------------------------------------------------------------
// Encoders
// ---------------------------------------------------------------------------

/// Encodes packet 54.
pub fn encode_state_sync(packet: &StateSync, w: &mut Writer) -> WireResult<()> {
    if packet.units.len() > MAX_SYNC_UNITS || packet.units.len() > u16::MAX as usize {
        return Err(WireError::VectorTooLong {
            len: packet.units.len(),
            max: MAX_SYNC_UNITS,
        });
    }
    w.write_u16(u16::from(packet.team.0));
    w.write_u8(packet.sync_index);
    w.write_bool(packet.is_last);
    w.write_u16(packet.units.len() as u16);
    for (unit, state) in &packet.units {
        w.write_u16(unit.0);
        state.write(w);
    }
    if packet.is_last {
        let tail = packet.tail.as_ref().ok_or(WireError::PartialFields {
            packet: "54 (is_last without tail)",
        })?;
        w.write_u32(tail.gold);
        w.write_u32(tail.gold_spent_on_upgrades);
        for topic in &tail.research {
            w.write_i32(topic.research_level);
            w.write_i32(topic.turns_to_complete);
            w.write_i32(topic.allocation);
        }
        w.write_u16(tail.number_of_objects_created);
        for counter in &tail.unit_counters {
            w.write_u32(*counter);
        }
    } else if packet.tail.is_some() {
        return Err(WireError::PartialFields {
            packet: "54 (tail on a non-last chunk)",
        });
    }
    Ok(())
}

/// Encodes packet 17.
pub fn encode_game_options(packet: &GameOptions, w: &mut Writer) {
    w.write_u16(u16::from(packet.team.0));
    w.write_u8(packet.game_state);
    for value in &packet.values {
        w.write_i32(*value);
    }
}

/// Encodes packet 16.
pub fn encode_save_request(packet: &SaveRequest, w: &mut Writer) -> WireResult<()> {
    w.write_u16(u16::from(packet.team.0));
    w.write_smart_string(&packet.file)?;
    w.write_smart_string(&packet.title)
}

// ---------------------------------------------------------------------------
// Bridge to max_core: applying 54 and assembling own state
// ---------------------------------------------------------------------------

impl UnitState {
    /// Read-only projection: sim → wire. Does not call apply_*, does not mutate GameState.
    ///
    /// Builds `Packet23Data` from the authoritative unit.
    ///
    /// The order of values follows `Remote_CreateNetPacket_23` (`remote.cpp`); the move and fire
    /// points are taken from `OrderData` (`UnitInfo::move_to_grid_*` in C++).
    pub fn from_unit(state: max_core::universe::StateRef<'_>, unit: &UnitInfo) -> Self {
        let order = state.order_data(unit.id);

        Self {
            orders: unit.orders as u8,
            state: unit.state as u8,
            prior_orders: unit.prior_orders as u8,
            prior_state: unit.prior_state as u8,
            disabled_reaction_fire: unit.disabled_reaction_fire,
            parent_unit_id: unit.parent_unit.map(|parent| parent.0).unwrap_or(0),
            experience: unit.experience,
            move_to_grid_x: order.move_to.x,
            move_to_grid_y: order.move_to.y,
            fire_on_grid_x: order.fire_on.x,
            fire_on_grid_y: order.fire_on.y,
            enemy_unit_id: unit.enemy_unit.map(|enemy| enemy.0).unwrap_or(0),
            transfer_cargo: unit.transfer_cargo,
            stealth_dice_roll: unit.stealth_dice_roll,
            total_mining: unit.total_mining,
            raw_mining: unit.raw_mining,
            fuel_mining: unit.fuel_mining,
            gold_mining: unit.gold_mining,
            build_time: unit.build_time,
            build_rate: unit.build_rate,
            unit_type: unit.unit_type as u16,
            unit_id: unit.unit_id,
            grid_x: unit.grid.x,
            grid_y: unit.grid.y,
            team: unit.team.0,
            hits: unit.hits,
            speed: unit.speed,
            shots: unit.shots,
            storage: unit.storage,
            ammo: unit.ammo,
        }
    }

    /// Wire form of a canonical unit state — the inverse of [`Self::to_core`].
    ///
    /// Used by the facade transport DTOs (`NetworkCommand`/`RemoteCommand`):
    /// the facade hands packet 54 as [`max_types::events::StateSyncPacket`], the
    /// wire codec needs this struct.
    pub fn from_core(packet: &max_types::events::UnitStatePacket) -> Self {
        Self {
            orders: packet.orders,
            state: packet.state,
            prior_orders: packet.prior_orders,
            prior_state: packet.prior_state,
            disabled_reaction_fire: packet.disabled_reaction_fire,
            parent_unit_id: packet.parent_unit_id,
            experience: packet.experience,
            move_to_grid_x: packet.move_to_grid_x,
            move_to_grid_y: packet.move_to_grid_y,
            fire_on_grid_x: packet.fire_on_grid_x,
            fire_on_grid_y: packet.fire_on_grid_y,
            enemy_unit_id: packet.enemy_unit_id,
            transfer_cargo: packet.transfer_cargo,
            stealth_dice_roll: packet.stealth_dice_roll,
            total_mining: packet.total_mining,
            raw_mining: packet.raw_mining,
            fuel_mining: packet.fuel_mining,
            gold_mining: packet.gold_mining,
            build_time: packet.build_time,
            build_rate: packet.build_rate,
            unit_type: packet.unit_type,
            unit_id: packet.unit_id,
            grid_x: packet.grid_x,
            grid_y: packet.grid_y,
            team: packet.team,
            hits: packet.hits,
            speed: packet.speed,
            shots: packet.shots,
            storage: packet.storage,
            ammo: packet.ammo,
        }
    }

    /// Canonical form for [`max_core::events::apply_unit_state`].
    pub fn to_core(&self) -> max_types::events::UnitStatePacket {
        max_types::events::UnitStatePacket {
            orders: self.orders,
            state: self.state,
            prior_orders: self.prior_orders,
            prior_state: self.prior_state,
            disabled_reaction_fire: self.disabled_reaction_fire,
            parent_unit_id: self.parent_unit_id,
            experience: self.experience,
            move_to_grid_x: self.move_to_grid_x,
            move_to_grid_y: self.move_to_grid_y,
            fire_on_grid_x: self.fire_on_grid_x,
            fire_on_grid_y: self.fire_on_grid_y,
            enemy_unit_id: self.enemy_unit_id,
            transfer_cargo: self.transfer_cargo,
            stealth_dice_roll: self.stealth_dice_roll,
            total_mining: self.total_mining,
            raw_mining: self.raw_mining,
            fuel_mining: self.fuel_mining,
            gold_mining: self.gold_mining,
            build_time: self.build_time,
            build_rate: self.build_rate,
            unit_type: self.unit_type,
            unit_id: self.unit_id,
            grid_x: self.grid_x,
            grid_y: self.grid_y,
            team: self.team,
            hits: self.hits,
            speed: self.speed,
            shots: self.shots,
            storage: self.storage,
            ammo: self.ammo,
        }
    }
}

impl StateSyncTail {
    /// Canonical form for [`max_core::events::apply_state_sync`].
    ///
    /// `i32` on the wire and `u32` in `max_core` are the same bits (C++ puts
    /// `int32_t` into `uint32_t`/`memcpy`), so the conversion is a type cast.
    pub fn to_core(&self) -> max_types::events::TeamStateSyncTail {
        let mut research = [ResearchTopicInfo::default(); RESEARCH_TOPIC_COUNT];
        for (destination, source) in research.iter_mut().zip(self.research.iter()) {
            *destination = ResearchTopicInfo {
                research_level: source.research_level as u32,
                turns_to_complete: source.turns_to_complete as u32,
                allocation: source.allocation as u32,
            };
        }

        let mut unit_counters = [0u32; max_types::UNIT_COUNT];
        for (destination, source) in unit_counters.iter_mut().zip(self.unit_counters.iter()) {
            *destination = *source;
        }

        max_types::events::TeamStateSyncTail {
            gold: self.gold,
            gold_spent_on_upgrades: self.gold_spent_on_upgrades,
            research,
            number_of_objects_created: self.number_of_objects_created,
            unit_counters,
        }
    }

    /// Wire form of a canonical tail — the inverse of [`Self::to_core`].
    ///
    /// `i32` on the wire and `u32` in `max_core` are the same bits (C++ puts
    /// `int32_t` into `uint32_t`/`memcpy`), so the conversion is a type cast.
    pub fn from_core(tail: &max_types::events::TeamStateSyncTail) -> Self {
        let mut research = [ResearchTopicWire::default(); RESEARCH_TOPIC_COUNT];
        for (destination, source) in research.iter_mut().zip(tail.research.iter()) {
            *destination = ResearchTopicWire {
                research_level: source.research_level as i32,
                turns_to_complete: source.turns_to_complete as i32,
                allocation: source.allocation as i32,
            };
        }

        let mut unit_counters = [0u32; UNIT_END];
        for (destination, source) in unit_counters.iter_mut().zip(tail.unit_counters.iter()) {
            *destination = *source;
        }

        Self {
            gold: tail.gold,
            gold_spent_on_upgrades: tail.gold_spent_on_upgrades,
            research,
            number_of_objects_created: tail.number_of_objects_created,
            unit_counters,
        }
    }
}

impl StateSync {
    /// Canonical form for [`max_core::events::apply_state_sync`].
    pub fn to_core(&self) -> max_types::events::StateSyncPacket {
        max_types::events::StateSyncPacket {
            team: self.team,
            sync_index: self.sync_index,
            is_last: self.is_last,
            units: self
                .units
                .iter()
                .map(|(unit, state)| (*unit, state.to_core()))
                .collect(),
            tail: self.tail.as_ref().map(StateSyncTail::to_core),
        }
    }

    /// Wire form of a canonical packet 54 — the inverse of [`Self::to_core`].
    ///
    /// The facade and the transport exchange [`max_types::events::StateSyncPacket`]
    /// (ADR §3); the encoder needs the wire layout, so the DTO is projected back
    /// here without any simulation access.
    pub fn from_core(packet: max_types::events::StateSyncPacket) -> Self {
        Self {
            team: packet.team,
            sync_index: packet.sync_index,
            is_last: packet.is_last,
            units: packet
                .units
                .iter()
                .map(|(unit, state)| (*unit, UnitState::from_core(state)))
                .collect(),
            tail: packet.tail.as_ref().map(StateSyncTail::from_core),
        }
    }
}

/// Read-only projection: sim → wire. Does not call apply_*, does not mutate GameState.
///
/// Builds packets 54 with the authoritative state of the team's units.
///
/// Port of `Remote_SendUnitStates` (`remote.cpp:4333-4381`): units in the order of the
/// four lists (`GroundCover → MobileLandSea → Stationary → MobileAir`),
/// at most [`STATE_SYNC_UNITS_PER_PACKET`] per packet, and the economy tail only
/// in the last chunk. For an empty team a single packet with `is_last` is returned.
pub fn state_sync_for(
    state: max_core::universe::StateRef<'_>,
    team: TeamId,
    sync_index: u8,
) -> Vec<StateSync> {
    let units: Vec<UnitId> = [
        UnitListKind::GroundCover,
        UnitListKind::MobileLandSea,
        UnitListKind::Stationary,
        UnitListKind::MobileAir,
    ]
    .into_iter()
    .flat_map(|kind| state.unit_list(kind).iter().copied())
    .filter(|id| {
        state.unit(*id).is_some_and(|unit| {
            unit.team == team
                && !unit.id.is_no_id()
                && unit.flags & max_types::enums::unit_flags::EXPLODING == 0
        })
    })
    .collect();

    let tail = state_sync_tail(state, team);
    let sizes = chunk_sizes(units.len());
    let mut packets = Vec::with_capacity(sizes.len());
    let mut start = 0;

    for (chunk, size) in sizes.iter().copied().enumerate() {
        let end = start + size;
        let is_last = chunk + 1 == sizes.len();
        let chunk_units = units[start..end]
            .iter()
            .filter_map(|id| {
                state
                    .unit(*id)
                    .map(|unit| (*id, UnitState::from_unit(state, unit)))
            })
            .collect();

        packets.push(StateSync {
            team,
            sync_index,
            is_last,
            units: chunk_units,
            tail: is_last.then(|| tail.clone()),
        });
        start = end;
    }

    packets
}

/// Splits units into chunks 54 within [`super::MAX_PACKET_SIZE`].
///
/// A non-final chunk holds up to 30 units, the last one up to 20 (it also carries the 478-byte tail).
/// If greedy slicing leaves the last chunk larger than 20, it is split into
/// `(n − 20, 20)` so that no frame exceeds the transport limit.
fn chunk_sizes(total: usize) -> Vec<usize> {
    if total == 0 {
        return vec![0];
    }

    let mut sizes = Vec::new();
    let mut remaining = total;
    while remaining > 0 {
        let take = remaining.min(MAX_UNITS_PER_CHUNK);
        sizes.push(take);
        remaining -= take;
    }

    if let Some(last) = sizes.last_mut() {
        if *last > MAX_UNITS_LAST_CHUNK {
            let overflow = *last - MAX_UNITS_LAST_CHUNK;
            *last = overflow;
            sizes.push(MAX_UNITS_LAST_CHUNK);
        }
    }

    sizes
}

/// Read-only projection: sim → wire. Does not call apply_*, does not mutate GameState.
///
/// The tail of packet 54 (team economy) from `TeamInfo`/`TeamUnits`.
fn state_sync_tail(state: max_core::universe::StateRef<'_>, team: TeamId) -> StateSyncTail {
    let info = &state.teams()[team.0 as usize];
    let team_units = &state.team_units()[team.0 as usize];
    let mut tail = StateSyncTail {
        gold: team_units.gold,
        gold_spent_on_upgrades: info.stats_gold_spent_on_upgrades,
        number_of_objects_created: info.number_of_objects_created,
        ..StateSyncTail::default()
    };

    for (destination, source) in tail.research.iter_mut().zip(info.research_topics.iter()) {
        *destination = ResearchTopicWire {
            research_level: source.research_level as i32,
            turns_to_complete: source.turns_to_complete as i32,
            allocation: source.allocation as i32,
        };
    }
    for (destination, source) in tail.unit_counters.iter_mut().zip(info.unit_counters.iter()) {
        *destination = *source;
    }

    tail
}

/// Encodes the `team,param` control packets (24/39/40/42/49/51).
pub fn encode_team_param(team: TeamId, param: u8, w: &mut Writer) {
    w.write_u16(u16::from(team.0));
    w.write_u8(param);
}

/// Decodes the `team,param` control packets.
pub fn decode_team(team_raw: u16) -> TeamId {
    TeamId(team_raw as u8)
}

/// Decodes a clan byte (`TeamClanType`).
pub fn decode_clan(raw: u8) -> WireResult<TeamClan> {
    TeamClan::try_from(raw).map_err(|_| WireError::InvalidValue {
        kind: "TeamClan",
        value: u32::from(raw),
    })
}

/// Maximum accepted `SmartString` length for the save file name.
pub const MAX_SAVE_FILE_LEN: usize = MAX_STRING_LEN;

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::universe::GameUniverse;
    use max_types::enums::{UnitOrder, UnitOrderState, UnitType};
    use max_types::geo::Point;
    use max_types::team::TeamKind;
    use max_types::UNIT_COUNT;

    fn state_with_green_units(count: usize) -> (GameUniverse, Vec<UnitId>) {
        let mut state = GameUniverse::default();
        state
            .team_mut(TeamId::GREEN)
            .expect("GREEN slot")
            .team_type = TeamKind::Remote;
        *state.team_units_mut(TeamId::GREEN).expect("GREEN economy") =
            max_core::team::TeamUnits::with_clan(TeamClan::Random);

        let mut ids = Vec::with_capacity(count);
        for index in 0..count {
            let id = state
                .spawn_unit(
                    UnitType::Tank,
                    TeamId::GREEN,
                    Point::new(10 + (index % 80) as i16, 10 + (index / 80) as i16),
                    0,
                )
                .expect("tank");
            ids.push(id);
        }
        // A unit of another team must not end up in GREEN's 54.
        state
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(50, 50), 0)
            .expect("red tank");
        (state, ids)
    }

    #[test]
    fn state_sync_chunks_fit_transport_and_put_the_tail_last() {
        let (mut state, ids) = state_with_green_units(40);
        state
            .team_units_mut(TeamId::GREEN)
            .expect("GREEN economy")
            .gold = 777;
        state
            .team_mut(TeamId::GREEN)
            .expect("GREEN slot")
            .stats_gold_spent_on_upgrades = 12;
        state
            .team_mut(TeamId::GREEN)
            .expect("GREEN slot")
            .number_of_objects_created = 5;
        state
            .team_mut(TeamId::GREEN)
            .expect("GREEN slot")
            .unit_counters[UnitType::Tank as usize] = 9;
        state
            .team_mut(TeamId::GREEN)
            .expect("GREEN slot")
            .research_topics[2] = ResearchTopicInfo {
            research_level: 4,
            turns_to_complete: 5,
            allocation: 6,
        };
        state.set_order(ids[39], UnitOrder::Sentry, UnitOrderState::ExecutingOrder);

        let packets = state_sync_for(state.snapshot(), TeamId::GREEN, 3);
        assert_eq!(packets.len(), 2, "40 юнитов → 30 + 10");
        assert_eq!(STATE_SYNC_UNITS_PER_PACKET, 32, "номинал C++");

        let first = &packets[0];
        assert_eq!(first.team, TeamId::GREEN);
        assert_eq!(first.sync_index, 3);
        assert!(!first.is_last);
        assert_eq!(first.units.len(), 30);
        assert!(first.tail.is_none());
        // `MobileLandSea` uses `PushFront`, so the order is the reverse of spawn order.
        assert_eq!(first.units[0].0, ids[39]);
        assert_eq!(first.units[0].1.orders, UnitOrder::Sentry as u8);
        assert_eq!(first.units[0].1.team, TeamId::GREEN.0);
        assert_eq!(first.units[29].0, ids[10]);

        let last = &packets[1];
        assert!(last.is_last);
        assert_eq!(last.units.len(), 10);
        assert_eq!(last.units[0].0, ids[9]);
        assert_eq!(last.units[9].0, ids[0]);
        let tail = last.tail.as_ref().expect("tail on last chunk");
        assert_eq!(tail.gold, 777);
        assert_eq!(tail.gold_spent_on_upgrades, 12);
        assert_eq!(tail.number_of_objects_created, 5);
        assert_eq!(tail.unit_counters[UnitType::Tank as usize], 9);
        assert_eq!(
            tail.research[2],
            ResearchTopicWire {
                research_level: 4,
                turns_to_complete: 5,
                allocation: 6,
            }
        );
        assert_eq!(tail.unit_counters.len(), UNIT_COUNT);
    }

    #[test]
    fn state_sync_for_an_empty_team_is_one_last_chunk() {
        let mut state = GameUniverse::default();
        state
            .team_mut(TeamId::BLUE)
            .expect("BLUE slot")
            .team_type = TeamKind::Remote;

        let packets = state_sync_for(state.snapshot(), TeamId::BLUE, 1);
        assert_eq!(packets.len(), 1);
        assert!(packets[0].is_last);
        assert!(packets[0].units.is_empty());
        assert!(packets[0].tail.is_some());

        // Round-trip: an empty last chunk encodes and decodes without loss.
        let bytes = super::super::Packet::StateSync(packets[0].clone())
            .encode()
            .expect("encode empty 54");
        assert_eq!(
            super::super::Packet::decode(&bytes).expect("decode empty 54"),
            super::super::Packet::StateSync(packets[0].clone())
        );
    }

    #[test]
    fn state_sync_packets_fit_the_transport_limit() {
        // 100 units: non-final chunks of 30, the last one carries the 478-byte tail.
        let (state, _) = state_with_green_units(100);
        let packets = state_sync_for(state.snapshot(), TeamId::GREEN, 1);

        assert!(packets.len() >= 4);
        for packet in packets {
            let frame = super::super::Packet::StateSync(packet)
                .encode()
                .expect("каждый 54 обязан влезать в MAX_PACKET_SIZE");
            assert!(
                frame.len() <= super::super::MAX_PACKET_SIZE,
                "54 длиной {} > {}",
                frame.len(),
                super::super::MAX_PACKET_SIZE
            );
        }
    }

    #[test]
    fn chunk_sizes_keep_the_tail_chunk_small() {
        assert_eq!(chunk_sizes(0), vec![0]);
        assert_eq!(chunk_sizes(20), vec![20]);
        assert_eq!(chunk_sizes(21), vec![1, 20]);
        assert_eq!(chunk_sizes(25), vec![5, 20]);
        assert_eq!(chunk_sizes(30), vec![10, 20]);
        assert_eq!(chunk_sizes(40), vec![30, 10]);
        assert_eq!(chunk_sizes(55), vec![30, 5, 20]);
        assert_eq!(chunk_sizes(60), vec![30, 10, 20]);
        assert_eq!(chunk_sizes(100), vec![30, 30, 30, 10]);
    }

    #[test]
    fn wire_unit_state_converts_to_core_packet() {
        let (state, ids) = state_with_green_units(1);
        let unit = state.snapshot().unit(ids[0]).expect("unit");
        let mut wire = UnitState::from_unit(state.snapshot(), unit);
        wire.experience = 17;
        wire.storage = -3;

        let core = wire.to_core();
        assert_eq!(core.experience, 17);
        assert_eq!(core.storage, -3);
        assert_eq!(core.unit_type, UnitType::Tank as u16);
        assert_eq!(core.grid_y, 10);
    }
}
