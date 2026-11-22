//! DTO of authoritative team state (packet 54) — pure data.
//!
//! Only protocol structures that do not read simulation data:
//! [`UnitStatePacket`], [`TeamStateSyncTail`], [`StateSyncPacket`]. Their
//! application (`apply_unit_state`, `apply_state_sync`) stays in
//! `max_core::events`, because it works with `GameState`.
//!
//! Oracle: `max-cpp-port/src/remote.cpp` (`Remote_SendUnitStates`,
//! `Remote_ReceiveNetPacket_54`, `Remote_ApplyUnitState`),
//! `max-brh-docs/protocol.md` §8.5.

use crate::enums::RESEARCH_TOPIC_COUNT;
use crate::team::{ResearchTopicInfo, TeamId};
use crate::unit::UnitId;
use crate::UNIT_COUNT;

// ---------------------------------------------------------------------------
// 54 — authoritative team state (P23 record + economy tail)
// ---------------------------------------------------------------------------

/// Canonical unit state — `struct Packet23Data` (`max-brh-docs/protocol.md`
/// §8.5); used by packets 23 and 54.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnitStatePacket {
    /// Current order (`UnitOrderType`, raw `u8`).
    pub orders: u8,
    /// Order state (`UnitOrderStateType`, raw `u8`).
    pub state: u8,
    /// Queued order.
    pub prior_orders: u8,
    /// Queued order state.
    pub prior_state: u8,
    /// Return fire is forbidden.
    pub disabled_reaction_fire: bool,
    /// Parent unit (`0` — none).
    pub parent_unit_id: u16,
    /// Experience.
    pub experience: i16,
    /// Move point X.
    pub move_to_grid_x: i16,
    /// Move point Y.
    pub move_to_grid_y: i16,
    /// Fire target X.
    pub fire_on_grid_x: i16,
    /// Fire target Y.
    pub fire_on_grid_y: i16,
    /// Target unit (`0` — none).
    pub enemy_unit_id: u16,
    /// Reloadable cargo.
    pub transfer_cargo: i16,
    /// Camouflage roll.
    pub stealth_dice_roll: u8,
    /// Harvest: total.
    pub total_mining: u8,
    /// Materials harvested.
    pub raw_mining: u8,
    /// Fuel harvested.
    pub fuel_mining: u8,
    /// Gold harvested.
    pub gold_mining: u8,
    /// Build timer.
    pub build_time: u8,
    /// Build rate.
    pub build_rate: u16,
    /// Unit type (`ResourceID`).
    pub unit_type: u16,
    /// `UnitInfo::unit_id` (index within the team, one byte).
    pub unit_id: u8,
    /// Cell X.
    pub grid_x: i16,
    /// Cell Y.
    pub grid_y: i16,
    /// Team.
    pub team: u8,
    /// Hits.
    pub hits: u16,
    /// Speed.
    pub speed: u16,
    /// Shots.
    pub shots: u8,
    /// Cargo in storage.
    pub storage: i16,
    /// Ammo.
    pub ammo: u8,
}

/// Tail of the last chunk of packet 54 — team economy at the moment of the check.
///
/// The fields mirror `Remote_SendUnitStates`/`Remote_ReceiveNetPacket_54`
/// (`remote.cpp:4368-4373`, `4601-4648`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamStateSyncTail {
    /// Team credits (`TeamUnits::gold`).
    pub gold: u32,
    /// Gold spent on upgrades.
    pub gold_spent_on_upgrades: u32,
    /// Progress of the eight research topics.
    pub research: [ResearchTopicInfo; RESEARCH_TOPIC_COUNT],
    /// Counter of created objects.
    pub number_of_objects_created: u16,
    /// Name counters by type (`uint32_t[UNIT_END]`).
    pub unit_counters: [u32; UNIT_COUNT],
}

impl Default for TeamStateSyncTail {
    fn default() -> Self {
        Self {
            gold: 0,
            gold_spent_on_upgrades: 0,
            research: [ResearchTopicInfo::default(); RESEARCH_TOPIC_COUNT],
            number_of_objects_created: 0,
            unit_counters: [0; UNIT_COUNT],
        }
    }
}

/// Decoded packet 54 in model terms (`max_core`, without a `max_net` dependency).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSyncPacket {
    /// Team that owns the state.
    pub team: TeamId,
    /// Check number (distinguishes rounds).
    pub sync_index: u8,
    /// Last chunk of the team (carries [`TeamStateSyncTail`]).
    pub is_last: bool,
    /// Units of the chunk in owner order.
    pub units: Vec<(UnitId, UnitStatePacket)>,
    /// Economy tail; required when `is_last`.
    pub tail: Option<TeamStateSyncTail>,
}
