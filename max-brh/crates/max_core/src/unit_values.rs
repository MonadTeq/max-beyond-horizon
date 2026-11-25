//! Unit-type attributes (`UnitValues`) and base-value tables for 93 types.
//!
//! Oracle: `max-cpp-port/src/unitvalues.hpp` / `unitvalues.cpp` (the
//! `UnitValues` class, `ATTRIB_*` indices), `teamunits.cpp` (`TeamUnits::Init` —
//! population from attributes), `attributes.cpp` (the `SC_ATTRI` table),
//! `clans.cpp` + `resource_manager.cpp` (`ResourceManager_InitClanUnitValues` —
//! clan modifiers of base values, `TeamUnits_UpgradeOffsetFactor`).
//!
//! Field layout is as in C++ (declaration order preserved, `fuel` is a
//! runtime field and is not serialized in V70, see `unitvalues.cpp::FileLoad`).
//!
//! ## Where the [`BASE_UNIT_VALUES`] table comes from
//!
//! The C++ port does not keep the table in code: values are read from the `SC_ATTRI`
//! resource (JSON, `assets/PATCHES.RES`, `Attributes::LoadResource`). The generator
//! `max-brh/tools/gen_unit_values.py` extracts and decodes this resource
//! (XOR table `ResourceManager_GenericTable`) and prints
//! `src/unit_values_tables.in`, which is included here via [`include!`].
//! Clan modifiers (`CLAN_TRADEOFFS`) come from the `SC_CLANS` resource
//! (`Clans::GetUnitTradeoff`). The table's validity is checked by the
//! [`tests::base_values_match_real_v70_save`] test on a real V70 save: the base
//! team values in the file equal `BASE + clan modifiers`.

use serde::{Deserialize, Serialize};

use crate::enums::{TeamClan, UnitType};

// The table size by unit type (`UNIT_END`) is a shared lower-layer constant
// (`max_types`, F5.2); here it is re-exported so that the path
// `max_core::unit_values::UNIT_COUNT` does not change.
pub use max_types::UNIT_COUNT;

// ---------------------------------------------------------------------------
// ATTRIB_* — attribute indices (unitvalues.hpp)
// ---------------------------------------------------------------------------

/// The "attack" attribute (`ATTRIB_ATTACK` = `RESEARCH_TOPIC_ATTACK`).
pub const ATTRIB_ATTACK: u8 = 0;
/// The "shots per attack" attribute (`ATTRIB_ROUNDS` = `RESEARCH_TOPIC_SHOTS`).
pub const ATTRIB_ROUNDS: u8 = 1;
/// The "range" attribute (`ATTRIB_RANGE` = `RESEARCH_TOPIC_RANGE`).
pub const ATTRIB_RANGE: u8 = 2;
/// The "armor" attribute (`ATTRIB_ARMOR` = `RESEARCH_TOPIC_ARMOR`).
pub const ATTRIB_ARMOR: u8 = 3;
/// The "hits" attribute (`ATTRIB_HITS` = `RESEARCH_TOPIC_HITS`).
pub const ATTRIB_HITS: u8 = 4;
/// The "speed" attribute (`ATTRIB_SPEED` = `RESEARCH_TOPIC_SPEED`).
pub const ATTRIB_SPEED: u8 = 5;
/// The "scan" attribute (`ATTRIB_SCAN` = `RESEARCH_TOPIC_SCAN`).
pub const ATTRIB_SCAN: u8 = 6;
/// The "turns to build" attribute (`ATTRIB_TURNS` = `RESEARCH_TOPIC_COST`).
pub const ATTRIB_TURNS: u8 = 7;
/// The "ammo" attribute.
pub const ATTRIB_AMMO: u8 = 8;
/// The "move and fire" attribute.
pub const ATTRIB_MOVE_AND_FIRE: u8 = 9;
/// The "fuel" attribute (not serialized, not changed by clans).
pub const ATTRIB_FUEL: u8 = 10;
/// The "cargo storage" attribute.
pub const ATTRIB_STORAGE: u8 = 11;
/// The "area attack radius" attribute.
pub const ATTRIB_ATTACK_RADIUS: u8 = 12;
/// The "agent bonus" attribute (`ATTRIB_AGENT_ADJUST`, `Experience` for clans).
pub const ATTRIB_AGENT_ADJUST: u8 = 13;
/// Number of attributes (`ATTRIB_COUNT`).
pub const ATTRIB_COUNT: u8 = 14;

/// Attribute name by `ATTRIB_*` index (for debug dumps/snapshots).
pub const ATTRIB_NAMES: [&str; ATTRIB_COUNT as usize] = [
    "attack",
    "rounds",
    "range",
    "armor",
    "hits",
    "speed",
    "scan",
    "turns",
    "ammo",
    "move_and_fire",
    "fuel",
    "storage",
    "attack_radius",
    "agent_adjust",
];

// ---------------------------------------------------------------------------
// UnitValues
// ---------------------------------------------------------------------------

/// Unit-type attributes — C++ `class UnitValues : FileObject`
/// (`unitvalues.hpp`).
///
/// Field order repeats C++ (it is also the V70 serialization order). `fuel` is
/// a runtime field: declared in C++, but not written to the file.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct UnitValues {
    /// Turns to build.
    pub turns: u16,
    /// Hits.
    pub hits: u16,
    /// Armor.
    pub armor: u16,
    /// Attack.
    pub attack: u16,
    /// Speed (movement points).
    pub speed: u16,
    /// Fuel (runtime, not serialized).
    pub fuel: u16,
    /// Attack range.
    pub range: u16,
    /// Shots per attack.
    pub rounds: u16,
    /// Move and fire (`ATTRIB_MOVE_AND_FIRE`, u8 as in C++).
    pub move_and_fire: u8,
    /// Scan radius.
    pub scan: u16,
    /// Cargo storage (for `COMMANDO`/`REGENERATING_UNIT` — experience, see `UnitInfo`).
    pub storage: u16,
    /// Ammo.
    pub ammo: u16,
    /// Area attack radius.
    pub attack_radius: u16,
    /// Agent (saboteur) bonus.
    pub agent_adjust: u16,
    /// "Mark" (upgrade level), starts at 1 (`UnitValues::UnitValues`).
    pub version: u16,
    /// Whether the values slot is occupied (`m_is_in_use`; in V70 — `units_built != 0`).
    pub in_use: bool,
}

impl Default for UnitValues {
    fn default() -> Self {
        Self::new()
    }
}

impl UnitValues {
    /// C++ constructor: all fields 0, `version = 1`, `m_is_in_use = false`.
    pub const fn new() -> Self {
        Self {
            turns: 0,
            hits: 0,
            armor: 0,
            attack: 0,
            speed: 0,
            fuel: 0,
            range: 0,
            rounds: 0,
            move_and_fire: 0,
            scan: 0,
            storage: 0,
            ammo: 0,
            attack_radius: 0,
            agent_adjust: 0,
            version: 1,
            in_use: false,
        }
    }

    /// `UnitValues::GetAttribute`: attribute value by `ATTRIB_*` index.
    ///
    /// As in C++, an invalid index is a caller bug (in release
    /// 0 is returned, like the original's `default` branch).
    pub fn get_attribute(&self, attribute: u8) -> i32 {
        match attribute {
            ATTRIB_TURNS => i32::from(self.turns),
            ATTRIB_HITS => i32::from(self.hits),
            ATTRIB_ARMOR => i32::from(self.armor),
            ATTRIB_ATTACK => i32::from(self.attack),
            ATTRIB_MOVE_AND_FIRE => i32::from(self.move_and_fire),
            ATTRIB_SPEED => i32::from(self.speed),
            ATTRIB_FUEL => i32::from(self.fuel),
            ATTRIB_RANGE => i32::from(self.range),
            ATTRIB_ROUNDS => i32::from(self.rounds),
            ATTRIB_SCAN => i32::from(self.scan),
            ATTRIB_STORAGE => i32::from(self.storage),
            ATTRIB_AMMO => i32::from(self.ammo),
            ATTRIB_ATTACK_RADIUS => i32::from(self.attack_radius),
            ATTRIB_AGENT_ADJUST => i32::from(self.agent_adjust),
            _ => {
                debug_assert!(
                    false,
                    "UnitValues::get_attribute: invalid index {attribute}"
                );
                0
            }
        }
    }

    /// `UnitValues::SetAttribute`: assignment with narrowing as in C++
    /// (`uint16_t = int32_t` — the low 16 bits).
    pub fn set_attribute(&mut self, attribute: u8, value: i32) {
        match attribute {
            ATTRIB_TURNS => self.turns = value as u16,
            ATTRIB_HITS => self.hits = value as u16,
            ATTRIB_ARMOR => self.armor = value as u16,
            ATTRIB_ATTACK => self.attack = value as u16,
            ATTRIB_MOVE_AND_FIRE => self.move_and_fire = value as u8,
            ATTRIB_SPEED => self.speed = value as u16,
            ATTRIB_FUEL => self.fuel = value as u16,
            ATTRIB_RANGE => self.range = value as u16,
            ATTRIB_ROUNDS => self.rounds = value as u16,
            ATTRIB_SCAN => self.scan = value as u16,
            ATTRIB_STORAGE => self.storage = value as u16,
            ATTRIB_AMMO => self.ammo = value as u16,
            ATTRIB_ATTACK_RADIUS => self.attack_radius = value as u16,
            ATTRIB_AGENT_ADJUST => self.agent_adjust = value as u16,
            _ => debug_assert!(
                false,
                "UnitValues::set_attribute: invalid index {attribute}"
            ),
        }
    }

    /// `UnitValues::AddAttribute`: `field += value` with wrapping to the field's
    /// width (C++ computes in `int`, then narrows on write).
    pub fn add_attribute(&mut self, attribute: u8, value: i32) {
        let current = self.get_attribute(attribute);
        self.set_attribute(attribute, current.wrapping_add(value));
    }

    /// `UnitValues::UpdateVersion`: increments the "mark" only for occupied slots.
    pub fn update_version(&mut self) {
        if self.in_use {
            self.version = self.version.wrapping_add(1);
        }
    }

    /// `UnitValues::MarkAsNotInUse`.
    pub fn mark_as_not_in_use(&mut self) {
        self.in_use = false;
    }

    /// `UnitValues::MarkAsInUse`.
    pub fn mark_as_in_use(&mut self) {
        self.in_use = true;
    }

    /// `UnitValues::GetAgentSkill` — agent level (the number of
    /// `ATTRIB_AGENT_ADJUST` upgrades above the base; C++: `agent_adjust` of the current values).
    pub fn agent_skill(&self) -> u16 {
        self.agent_adjust
    }
}

impl PartialEq for UnitValues {
    /// `UnitValues::operator==`: compares all fields **except** `fuel`
    /// (a runtime field does not participate in the C++ comparison).
    fn eq(&self, other: &Self) -> bool {
        self.turns == other.turns
            && self.hits == other.hits
            && self.armor == other.armor
            && self.attack == other.attack
            && self.speed == other.speed
            && self.range == other.range
            && self.rounds == other.rounds
            && self.move_and_fire == other.move_and_fire
            && self.scan == other.scan
            && self.storage == other.storage
            && self.ammo == other.ammo
            && self.attack_radius == other.attack_radius
            && self.version == other.version
            && self.in_use == other.in_use
            && self.agent_adjust == other.agent_adjust
    }
}

impl Eq for UnitValues {}

// ---------------------------------------------------------------------------
// Tables: base values of 93 types and clan modifiers
// ---------------------------------------------------------------------------

include!("unit_values_tables.in");

/// Base values of a unit type without clan modifiers
/// (`TeamUnits::Init` + `UnitValues()`: `version = 1`, `in_use = false`).
#[inline]
pub fn base_unit_values(unit_type: UnitType) -> UnitValues {
    BASE_UNIT_VALUES[unit_type as usize]
}

/// Base values by raw type index (0..93); `None` out of range.
#[inline]
pub fn base_unit_values_by_index(index: u16) -> Option<UnitValues> {
    BASE_UNIT_VALUES.get(index as usize).copied()
}

/// Clan modifier of an attribute — C++ `Clans::GetUnitTradeoff`.
///
/// Returns 0 if there is no modifier for the (clan, type) pair (C++ then
/// does not call `AddAttribute`).
pub fn clan_tradeoff(clan: TeamClan, unit_type: UnitType, attribute: u8) -> i16 {
    let unit = unit_type as u16;
    let clan = clan as u8;
    let mut index = 0;
    while index < CLAN_TRADEOFFS.len() {
        let (c, u, a, delta) = CLAN_TRADEOFFS[index];
        if c == clan && u == unit && a == attribute {
            return delta;
        }
        index += 1;
    }
    0
}

/// Applies all clan modifiers to a set of attributes —
/// C++ `ResourceManager_SetClanUpgradesFromClans` (skips `ATTRIB_FUEL`).
pub fn apply_clan_tradeoffs(values: &mut UnitValues, clan: TeamClan, unit_type: UnitType) {
    let unit = unit_type as u16;
    let clan_index = clan as u8;
    for &(c, u, attribute, delta) in CLAN_TRADEOFFS.iter() {
        if c == clan_index && u == unit && attribute != ATTRIB_FUEL {
            values.add_attribute(attribute, i32::from(delta));
        }
    }
}

/// Base and current team values with the clan applied —
/// C++ `ResourceManager_InitClanUnitValues` (base == current after initialization).
///
/// For `TeamClan::Random` (0), C++ replaces the clan with a random one; here a
/// table without modifiers is returned (clan selection is a separate setup step).
pub fn team_base_values(clan: TeamClan) -> [UnitValues; UNIT_COUNT] {
    let mut values = BASE_UNIT_VALUES;
    if clan != TeamClan::Random {
        for (index, item) in values.iter_mut().enumerate() {
            let unit_type = UnitType::try_from(index as u16).expect("index < UNIT_END");
            apply_clan_tradeoffs(item, clan, unit_type);
        }
    }
    values
}

/// `TeamUnits_UpgradeOffsetFactor`: the upgrade step of an attribute, depending on
/// the base value (1/2/5/10).
///
/// Note: the full upgrade cost (`TeamUnits_GetUpgradeCost`) is not
/// implemented here — it requires `ResearchMenu_CalculateFactor` and
/// `UpgradeControl_CalculateCost` with `std::pow(..., 7.5)` (DESIGN risk 1:
/// float formulas move to build-time tables in P4). All integer parts
/// (`base/current` values, offset step) are here.
pub fn upgrade_offset_factor(base: &UnitValues, attribute: u8) -> i32 {
    let value = base.get_attribute(attribute);
    if value < 10 {
        1
    } else if value < 25 {
        2
    } else if value < 50 {
        5
    } else {
        10
    }
}

#[cfg(test)]
#[path = "unit_values_tests.rs"]
mod tests;
