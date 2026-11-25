//! `WeightTable` — weighted AI unit-type selection.
//!
//! Oracle: `max-cpp-port/src/weighttable.cpp/.hpp`. A 1:1 port: the same order
//! of entries (insertion order), the same `RollUnitType` with one
//! `Randomizer_Generate(total_weight)` call, the same `Disable` (swap-and-pop) and
//! `Compact` (removal of zero weights while preserving the order of the rest).
//!
//! Determinism: iteration is only over a `Vec` in insertion order; RNG — only
//! [`crate::rng::Randomizer`] (DESIGN §6.2/§6.4).

use crate::enums::UnitType;
use crate::rng::Randomizer;

/// A weight-table entry — C++ `struct UnitWeight`.
///
/// In C++ an empty entry is encoded as `INVALID_ID`; in Rust — `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitWeight {
    /// Unit type (`ResourceID`); `None` — `INVALID_ID`.
    pub unit_type: Option<UnitType>,
    /// Selection weight.
    pub weight: u16,
}

impl UnitWeight {
    /// Empty entry (`UnitWeight()` — `INVALID_ID`, weight 0).
    pub const fn empty() -> Self {
        Self {
            unit_type: None,
            weight: 0,
        }
    }
}

/// Weight table — C++ `class WeightTable`.
///
/// `team` is used only for the buildability check in C++ `Add()`
/// (`Builder_IsBuildable`). In max_core the buildability table comes from unit
/// data (`SC_UNITS`), so [`WeightTable::add_buildable`] accepts
/// the result of the caller-layer check, while [`WeightTable::add`] adds an entry
/// without a check (like `PushBack`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightTable {
    entries: Vec<UnitWeight>,
    team: u8,
}

impl Default for WeightTable {
    fn default() -> Self {
        Self::new(0)
    }
}

impl WeightTable {
    /// An empty table for team `team`.
    pub fn new(team: u8) -> Self {
        Self {
            entries: Vec::new(),
            team,
        }
    }

    /// `WeightTable::SetTeam`.
    pub fn set_team(&mut self, team: u8) {
        self.team = team;
    }

    /// `WeightTable::GetTeam`.
    pub const fn team(&self) -> u8 {
        self.team
    }

    /// `WeightTable::Add` without the buildability check (`PushBack(UnitWeight)`).
    pub fn add(&mut self, unit_type: UnitType, weight: u16) {
        self.entries.push(UnitWeight {
            unit_type: Some(unit_type),
            weight,
        });
    }

    /// `WeightTable::Add` with the buildability check (`Builder_IsBuildable`).
    ///
    /// The check is data: the caller layer passes `is_buildable`.
    pub fn add_buildable(&mut self, unit_type: UnitType, weight: u16, is_buildable: bool) -> bool {
        if is_buildable {
            self.add(unit_type, weight);
        }
        is_buildable
    }

    /// `WeightTable::PushBack`.
    pub fn push_back(&mut self, entry: UnitWeight) {
        self.entries.push(entry);
    }

    /// Entries in insertion order (C++ `operator[]`/`GetCount`).
    pub fn entries(&self) -> &[UnitWeight] {
        &self.entries
    }

    /// Mutable access to the entries (C++ `operator[]` — `RebuildWeightTable`).
    pub fn entries_mut(&mut self) -> &mut [UnitWeight] {
        &mut self.entries
    }

    /// `WeightTable::Disable` — swap-and-pop (the order of the rest changes, as in C++).
    pub fn disable(&mut self, index: usize) {
        if index < self.entries.len() {
            let last = self.entries.len() - 1;
            if index < last {
                self.entries.swap(index, last);
            }
            self.entries.pop();
        }
    }

    /// `WeightTable::Compact` — removal of entries with weight 0.
    pub fn compact(&mut self) {
        self.entries.retain(|entry| entry.weight != 0);
    }

    /// `WeightTable::Clear`.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// `WeightTable::GetCount`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `WeightTable::RollUnitType` — weighted selection via RNG.
    ///
    /// Returns `None` for `INVALID_ID` (no valid entries and/or the sum of
    /// weights is 0). One `Randomizer_Generate(total_weight)` call — as in C++.
    pub fn roll_unit_type(&self, rng: &mut Randomizer) -> Option<UnitType> {
        let mut total_weight: i32 = 0;

        for entry in &self.entries {
            total_weight += i32::from(entry.weight);
        }

        if total_weight > 0 {
            let roll = rng.generate_i32(total_weight) + 1;
            let mut roll = roll;

            for entry in &self.entries {
                roll -= i32::from(entry.weight);

                if roll <= 0 {
                    return entry.unit_type;
                }
            }
        }

        None
    }

    /// `WeightTable::GetWeight` — weight of a type (0 if there is no entry).
    pub fn get_weight(&self, unit_type: UnitType) -> i32 {
        for entry in &self.entries {
            if entry.unit_type == Some(unit_type) {
                return i32::from(entry.weight);
            }
        }

        0
    }

    /// `WeightTable::operator+=` — add weights by type, new ones at the end.
    pub fn merge(&mut self, other: &WeightTable) {
        for entry in &other.entries {
            match self
                .entries
                .iter_mut()
                .find(|candidate| candidate.unit_type == entry.unit_type)
            {
                Some(existing) => existing.weight = existing.weight.wrapping_add(entry.weight),
                None => self.entries.push(*entry),
            }
        }
    }

    /// `RebuildWeightTable` (`aiplayer.cpp:70`) — multiplication of a type's weight.
    pub fn rebuild(&mut self, unit_type: UnitType, factor: u16) {
        for entry in &mut self.entries {
            if entry.unit_type == Some(unit_type) {
                entry.weight = entry.weight.wrapping_mul(factor);
            }
        }
    }
}

#[cfg(test)]
#[path = "weight_tests.rs"]
mod tests;
