//! `WeightTable` — взвешенный выбор типа юнита ИИ.
//!
//! Оракул: `max-cpp-port/src/weighttable.cpp/.hpp`. Порт 1:1: тот же порядок
//! записей (порядок вставки), тот же `RollUnitType` с одним вызовом
//! `Randomizer_Generate(total_weight)`, тот же `Disable` (swap-and-pop) и
//! `Compact` (удаление нулевых весов с сохранением порядка остальных).
//!
//! Детерминизм: итерация — только по `Vec` в порядке вставки; RNG — только
//! [`crate::rng::Randomizer`] (DESIGN §6.2/§6.4).

use crate::enums::UnitType;
use crate::rng::Randomizer;

/// Запись таблицы весов — C++ `struct UnitWeight`.
///
/// В C++ пустая запись кодируется `INVALID_ID`; в Rust — `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitWeight {
    /// Тип юнита (`ResourceID`); `None` — `INVALID_ID`.
    pub unit_type: Option<UnitType>,
    /// Вес выбора.
    pub weight: u16,
}

impl UnitWeight {
    /// Пустая запись (`UnitWeight()` — `INVALID_ID`, вес 0).
    pub const fn empty() -> Self {
        Self {
            unit_type: None,
            weight: 0,
        }
    }
}

/// Таблица весов — C++ `class WeightTable`.
///
/// `team` используется только для проверки строимости в C++ `Add()`
/// (`Builder_IsBuildable`). В max_core таблица строимости приходит из данных
/// юнитов (`SC_UNITS`), поэтому [`WeightTable::add_buildable`] принимает
/// результат проверки вызывающего слоя, а [`WeightTable::add`] добавляет запись
/// без проверки (как `PushBack`).
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
    /// Пустая таблица команды `team`.
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

    /// `WeightTable::Add` без проверки строимости (`PushBack(UnitWeight)`).
    pub fn add(&mut self, unit_type: UnitType, weight: u16) {
        self.entries.push(UnitWeight {
            unit_type: Some(unit_type),
            weight,
        });
    }

    /// `WeightTable::Add` с проверкой строимости (`Builder_IsBuildable`).
    ///
    /// Проверка — данные: вызывающий слой передаёт `is_buildable`.
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

    /// Записи в порядке вставки (C++ `operator[]`/`GetCount`).
    pub fn entries(&self) -> &[UnitWeight] {
        &self.entries
    }

    /// Мутабельный доступ к записям (C++ `operator[]` — `RebuildWeightTable`).
    pub fn entries_mut(&mut self) -> &mut [UnitWeight] {
        &mut self.entries
    }

    /// `WeightTable::Disable` — swap-and-pop (порядок остальных меняется, как в C++).
    pub fn disable(&mut self, index: usize) {
        if index < self.entries.len() {
            let last = self.entries.len() - 1;
            if index < last {
                self.entries.swap(index, last);
            }
            self.entries.pop();
        }
    }

    /// `WeightTable::Compact` — удаление записей с весом 0.
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

    /// Пустая ли таблица.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `WeightTable::RollUnitType` — взвешенный выбор через RNG.
    ///
    /// Возвращает `None` для `INVALID_ID` (нет валидных записей и/или сумма
    /// весов 0). Один вызов `Randomizer_Generate(total_weight)` — как в C++.
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

    /// `WeightTable::GetWeight` — вес типа (0, если нет записи).
    pub fn get_weight(&self, unit_type: UnitType) -> i32 {
        for entry in &self.entries {
            if entry.unit_type == Some(unit_type) {
                return i32::from(entry.weight);
            }
        }

        0
    }

    /// `WeightTable::operator+=` — сложение весов по типу, новые — в конец.
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

    /// `RebuildWeightTable` (`aiplayer.cpp:70`) — умножение веса типа.
    pub fn rebuild(&mut self, unit_type: UnitType, factor: u16) {
        for entry in &mut self.entries {
            if entry.unit_type == Some(unit_type) {
                entry.weight = entry.weight.wrapping_mul(factor);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roll_respects_weights_and_rng_order() {
        let mut table = WeightTable::new(0);
        table.add(UnitType::Tank, 1);
        table.add(UnitType::Scout, 3);
        // Сумма весов 4: rng.generate_i32(4) + 1 ∈ {1..4}.
        let mut rng = Randomizer::new(0);
        let first = table.roll_unit_type(&mut rng);
        let mut rng2 = Randomizer::new(0);
        let second = table.roll_unit_type(&mut rng2);
        assert_eq!(first, second);
        assert!(matches!(first, Some(UnitType::Tank) | Some(UnitType::Scout)));
    }

    #[test]
    fn empty_or_zero_weight_table_rolls_nothing() {
        let mut table = WeightTable::new(0);
        let mut rng = Randomizer::new(1);
        assert_eq!(table.roll_unit_type(&mut rng), None);
        table.add(UnitType::Tank, 0);
        assert_eq!(table.roll_unit_type(&mut rng), None);
        table.compact();
        assert!(table.is_empty());
    }

    #[test]
    fn disable_swaps_with_last_and_compact_keeps_order() {
        let mut table = WeightTable::new(0);
        table.add(UnitType::Scout, 1);
        table.add(UnitType::Tank, 2);
        table.add(UnitType::AssaultGun, 3);
        table.disable(0);
        assert_eq!(table.len(), 2);
        assert_eq!(table.entries()[0].unit_type, Some(UnitType::AssaultGun));
        assert_eq!(table.entries()[1].unit_type, Some(UnitType::Tank));

        let mut table = WeightTable::new(0);
        table.add(UnitType::Scout, 1);
        table.add(UnitType::Tank, 0);
        table.add(UnitType::AssaultGun, 3);
        table.compact();
        assert_eq!(table.len(), 2);
        assert_eq!(table.entries()[0].unit_type, Some(UnitType::Scout));
        assert_eq!(table.entries()[1].unit_type, Some(UnitType::AssaultGun));
    }

    #[test]
    fn merge_adds_weights_by_type() {
        let mut first = WeightTable::new(0);
        first.add(UnitType::Tank, 2);
        let mut second = WeightTable::new(1);
        second.add(UnitType::Tank, 3);
        second.add(UnitType::Scout, 1);
        first.merge(&second);
        assert_eq!(first.get_weight(UnitType::Tank), 5);
        assert_eq!(first.get_weight(UnitType::Scout), 1);
        assert_eq!(first.len(), 2);
    }

    #[test]
    fn rebuild_multiplies_weight() {
        let mut table = WeightTable::new(0);
        table.add(UnitType::Tank, 2);
        table.add(UnitType::Scout, 1);
        table.rebuild(UnitType::Tank, 10);
        assert_eq!(table.get_weight(UnitType::Tank), 20);
        assert_eq!(table.get_weight(UnitType::Scout), 1);
    }
}
