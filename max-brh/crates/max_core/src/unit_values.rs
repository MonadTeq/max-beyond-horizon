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
mod tests {
    use super::*;

    /// The base table covers exactly 93 types (`UNIT_END`).
    #[test]
    fn table_covers_all_unit_types() {
        assert_eq!(BASE_UNIT_VALUES.len(), UNIT_COUNT);
        assert_eq!(UNIT_COUNT, 93);
        assert_eq!(UnitType::COUNT, UNIT_COUNT);
        for (index, values) in BASE_UNIT_VALUES.iter().enumerate() {
            let expected = UnitType::try_from(index as u16).expect("valid unit type");
            assert_eq!(base_unit_values(expected), *values);
            // C++ `UnitValues()`: version = 1, m_is_in_use = false.
            assert_eq!(values.version, 1, "type {index}");
            assert!(!values.in_use, "type {index}");
            assert_eq!(values.fuel, 0, "type {index}");
        }
    }

    /// Spot check against the `SC_ATTRI` table (see `gen_unit_values.py`).
    #[test]
    fn representative_base_values_match_cpp_table() {
        // COMMTWR / POWERSTN: 12 turns, 56 hp, 8 armor, scan 3.
        let commtower = base_unit_values(UnitType::GoldRefinery);
        assert_eq!(
            (commtower.turns, commtower.hits, commtower.armor),
            (12, 56, 8)
        );
        assert_eq!(commtower.scan, 3);
        // POWGEN: 4 turns, 14 hp.
        let powgen = base_unit_values(UnitType::PowerGenerator);
        assert_eq!((powgen.turns, powgen.hits), (4, 14));
        // TANK (`TANK`, index 51): 4 turns, 24 hp, 10 armor, 16 attack,
        // 6 speed, 4 range, 2 shots, 4 scan, 14 ammo.
        let tank = base_unit_values(UnitType::Tank);
        assert_eq!(
            (
                tank.turns,
                tank.hits,
                tank.armor,
                tank.attack,
                tank.speed,
                tank.range,
                tank.rounds,
                tank.scan,
                tank.ammo,
                tank.attack_radius,
            ),
            (4, 24, 10, 16, 6, 4, 2, 4, 14, 0)
        );
        // SCOUT (`SCOUT`, 50): 3 turns, 16 hp, 12 attack, 12 speed, scan 9.
        let scout = base_unit_values(UnitType::Scout);
        assert_eq!(
            (
                scout.turns,
                scout.hits,
                scout.attack,
                scout.speed,
                scout.scan
            ),
            (3, 16, 12, 12, 9)
        );
        // GUNTURRT (34): stationary, 4 turns, 24 hp, 12 armor, scan 4.
        let turret = base_unit_values(UnitType::GunTurret);
        assert_eq!(
            (turret.turns, turret.hits, turret.armor, turret.speed),
            (4, 24, 12, 0)
        );
        // WALDO (`WALDO`, 92) — an easter egg without attributes.
        let waldo = base_unit_values(UnitType::DeadWaldo);
        assert_eq!(waldo.hits, 0);
        assert_eq!(waldo.turns, 0);
    }

    /// Attribute access: `ATTRIB_*` indices and field order as in C++.
    #[test]
    fn attribute_access_matches_cpp_indices() {
        let mut values = UnitValues::new();
        for attribute in 0..ATTRIB_COUNT {
            values.set_attribute(attribute, i32::from(attribute) + 1);
            assert_eq!(values.get_attribute(attribute), i32::from(attribute) + 1);
        }
        assert_eq!(values.attack, 1);
        assert_eq!(values.rounds, 2);
        assert_eq!(values.range, 3);
        assert_eq!(values.armor, 4);
        assert_eq!(values.hits, 5);
        assert_eq!(values.speed, 6);
        assert_eq!(values.scan, 7);
        assert_eq!(values.turns, 8);
        assert_eq!(values.ammo, 9);
        assert_eq!(values.move_and_fire, 10);
        assert_eq!(values.fuel, 11);
        assert_eq!(values.storage, 12);
        assert_eq!(values.attack_radius, 13);
        assert_eq!(values.agent_adjust, 14);

        // `AddAttribute` narrows to the field width (C++: int -> uint16_t).
        values.set_attribute(ATTRIB_HITS, 65_535);
        values.add_attribute(ATTRIB_HITS, 1);
        assert_eq!(values.hits, 0);
        values.set_attribute(ATTRIB_MOVE_AND_FIRE, 255);
        values.add_attribute(ATTRIB_MOVE_AND_FIRE, 1);
        assert_eq!(values.move_and_fire, 0);
    }

    /// `operator==` ignores `fuel` (as in C++).
    #[test]
    fn equality_ignores_fuel() {
        let mut left = UnitValues::new();
        let mut right = UnitValues::new();
        assert_eq!(left, right);
        right.fuel = 7;
        assert_eq!(left, right);
        left.hits = 1;
        assert_ne!(left, right);
    }

    /// `UpdateVersion` increments the version only for an occupied slot.
    #[test]
    fn version_updates_only_in_use() {
        let mut values = UnitValues::new();
        values.update_version();
        assert_eq!(values.version, 1);
        values.mark_as_in_use();
        values.update_version();
        assert_eq!(values.version, 2);
        values.mark_as_not_in_use();
        values.update_version();
        assert_eq!(values.version, 2);
    }

    /// Clan modifiers: `Clan C` (Von Griffin) gives Scout +2 attack/speed
    /// and +2 scan, Radar +6 scan, Infiltrator -8 build turns
    /// (`SC_CLANS`, `Clans::GetUnitTradeoff`).
    #[test]
    fn clan_tradeoffs_apply() {
        let scout = UnitType::Scout;
        assert_eq!(clan_tradeoff(TeamClan::VonGriffin, scout, ATTRIB_ATTACK), 2);
        assert_eq!(clan_tradeoff(TeamClan::VonGriffin, scout, ATTRIB_SPEED), 2);
        assert_eq!(clan_tradeoff(TeamClan::VonGriffin, scout, ATTRIB_SCAN), 2);
        assert_eq!(
            clan_tradeoff(TeamClan::VonGriffin, UnitType::Radar, ATTRIB_SCAN),
            6
        );
        assert_eq!(clan_tradeoff(TeamClan::Random, scout, ATTRIB_ATTACK), 0);
        // Another clan of the same unit — a different modifier.
        assert_eq!(
            clan_tradeoff(TeamClan::SevenKnights, scout, ATTRIB_ATTACK),
            0
        );

        let plain = base_unit_values(scout);
        let clan_values = team_base_values(TeamClan::VonGriffin);
        let modified = clan_values[scout as usize];
        assert_eq!(modified.attack, plain.attack + 2);
        assert_eq!(modified.speed, plain.speed + 2);
        assert_eq!(modified.scan, plain.scan + 2);
        // Other types do not change.
        assert_eq!(
            clan_values[UnitType::Tank as usize],
            base_unit_values(UnitType::Tank)
        );
        // `Random` — a table without modifiers.
        assert_eq!(team_base_values(TeamClan::Random), BASE_UNIT_VALUES);
    }

    /// `TeamUnits_UpgradeOffsetFactor` — upgrade step boundaries.
    #[test]
    fn upgrade_offset_factor_boundaries() {
        let mut values = UnitValues::new();
        for (base, expected) in [
            (0, 1),
            (9, 1),
            (10, 2),
            (24, 2),
            (25, 5),
            (49, 5),
            (50, 10),
            (999, 10),
        ] {
            values.set_attribute(ATTRIB_HITS, base);
            assert_eq!(
                upgrade_offset_factor(&values, ATTRIB_HITS),
                expected,
                "base {base}"
            );
        }
    }

    /// `ATTRIB_*` indices match `ResearchTopic` (the first 8).
    #[test]
    fn research_topics_match_attribute_order() {
        use crate::enums::ResearchTopic;
        assert_eq!(ATTRIB_ATTACK, ResearchTopic::Attack as u8);
        assert_eq!(ATTRIB_ROUNDS, ResearchTopic::Shots as u8);
        assert_eq!(ATTRIB_RANGE, ResearchTopic::Range as u8);
        assert_eq!(ATTRIB_ARMOR, ResearchTopic::Armor as u8);
        assert_eq!(ATTRIB_HITS, ResearchTopic::Hits as u8);
        assert_eq!(ATTRIB_SPEED, ResearchTopic::Speed as u8);
        assert_eq!(ATTRIB_SCAN, ResearchTopic::Scan as u8);
        assert_eq!(ATTRIB_TURNS, ResearchTopic::Cost as u8);
        assert_eq!(ATTRIB_COUNT as usize, ATTRIB_NAMES.len());
    }
}
