//! Audit/synchronisation and auxiliary in-game packets (`docs/protocol.md`
//! §8.3–§8.6).
//!
//! These packets either carry whole-unit snapshots (23/54 — the closest thing
//! to a "snapshot" in a lockstep protocol), team-level bookkeeping (09/17), or
//! session control facts (07/24/39/40/42/46/48/49/51). None of them is a
//! [`max_core::input::Command`], so their DTOs live here; the numbers are kept
//! raw where the simulation has no typed counterpart (the client only forwards
//! or logs them).

use max_core::enums::TeamClan;
use max_core::team::TeamId;
use max_core::unit::UnitId;

use super::reader::{Reader, WireError, WireResult, Writer, MAX_STRING_LEN, MAX_VECTOR_LEN};

/// `RESEARCH_TOPIC_COUNT` — research slots per team.
pub const RESEARCH_TOPIC_COUNT: usize = 8;
/// `UNIT_END` — number of unit counters in the packet 54 tail.
pub const UNIT_END: usize = 93;
/// Number of `i32` fields in packet 17.
pub const GAME_OPTION_COUNT: usize = 38;
/// `NetPacket::MaximumArrayCount` used by packet 54 (`count` is `u16`).
pub const MAX_SYNC_UNITS: usize = MAX_VECTOR_LEN;

/// One research topic in packet 09/54: `i32 level, i32 turns, i32 allocation`.
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
/// (`Packet23Data` in `remote.cpp` / `docs/protocol.md` §8.5).
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

/// Packet 09 — team state (gold, research, name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamState {
    /// Team index.
    pub team: TeamId,
    /// Gold.
    pub gold: u32,
    /// Gold spent on upgrades.
    pub gold_spent_on_upgrades: u32,
    /// Research topics (8 × 12 bytes).
    pub research: [ResearchTopicWire; RESEARCH_TOPIC_COUNT],
    /// Team name (`SmartString`).
    pub name: String,
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

/// Decodes packet 09.
pub fn decode_team_state(r: &mut Reader<'_>) -> WireResult<TeamState> {
    let team = TeamId(r.read_u16()? as u8);
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
    let name = r.read_smart_string()?;
    Ok(TeamState {
        team,
        gold,
        gold_spent_on_upgrades,
        research,
        name,
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

/// Encodes packet 09.
pub fn encode_team_state(packet: &TeamState, w: &mut Writer) -> WireResult<()> {
    w.write_u16(u16::from(packet.team.0));
    w.write_u32(packet.gold);
    w.write_u32(packet.gold_spent_on_upgrades);
    for topic in &packet.research {
        w.write_i32(topic.research_level);
        w.write_i32(topic.turns_to_complete);
        w.write_i32(topic.allocation);
    }
    w.write_smart_string(&packet.name)
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
