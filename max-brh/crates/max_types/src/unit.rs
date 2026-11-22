//! Идентификаторы юнитов (Infrastructure).
//!
//! Здесь только [`UnitId`] (16-битный id оригинала, старшие биты — команда) и
//! [`UnitListKind`] — листовые типы без данных `max_core`. Сам юнит
//! (`UnitInfo`), его пути (`GroundPath`/`AirPath`/`BuilderPath`) и `Rect`
//! остаются в `max_core::unit`: они зависят от атрибутов и кадровых данных.
//!
//! Оракул: `max-cpp-port/src/unitinfo.hpp` (`UnitInfo::hash_id`),
//! `units_manager.cpp` (пять списков юнитов).

use serde::{Deserialize, Serialize};

use crate::team::TeamId;

/// Идентификатор юнита (16-битный id оригинала).
///
/// Старшие биты — команда, младшие 13 бит — индекс объекта в команде
/// (`(team << 13) | object_index`), см. DESIGN.md §3.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnitId(pub u16);

impl UnitId {
    /// `NO_ID` (0xFFFF) — отсутствие юнита.
    pub const NO_ID: Self = Self(0xFFFF);

    /// Собирает id из команды и индекса объекта.
    #[inline]
    pub const fn new(team: TeamId, object_index: u16) -> Self {
        Self(((team.0 as u16) << 13) | (object_index & 0x1FFF))
    }

    /// Команда из старших бит id.
    #[inline]
    pub const fn team(self) -> TeamId {
        TeamId((self.0 >> 13) as u8)
    }

    /// Индекс объекта в команде (младшие 13 бит).
    #[inline]
    pub const fn object_index(self) -> u16 {
        self.0 & 0x1FFF
    }

    /// Является ли id «пустым» (`NO_ID`).
    #[inline]
    pub const fn is_no_id(self) -> bool {
        self.0 == Self::NO_ID.0
    }

    /// Из `u16` с V70-семантикой: 0xFFFF = [`UnitId::NO_ID`].
    #[inline]
    pub const fn from_v70(raw: u16) -> Self {
        Self(raw)
    }

    /// В `u16` для записи в V70-файл.
    #[inline]
    pub const fn to_v70(self) -> u16 {
        self.0
    }
}
