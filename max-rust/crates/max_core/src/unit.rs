//! Юниты: типы, состояние, идентификаторы.

use serde::{Deserialize, Serialize};

use crate::team::TeamId;

/// Идентификатор юнита (16-битный id оригинала).
///
/// Старшие биты — команда, младшие 13 бит — индекс объекта в команде
/// (`(team << 13) | object_index`), см. DESIGN.md §3.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
}

/// Тип юнита (список уточняется по данным оригинала; пока — имя-строка).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnitKind {
    Surveyor,
    Engineer,
    Constructor,
    Scout,
    Tank,
    AssaultGun,
    MissileCrawler,
    MineLayer,
    Scanner,
    Repair,
    Transport,
    MiningStation,
    PowerGenerator,
    Storage,
    Other(u8),
}

/// Состояние юнита — подмножество полей, которое сервер передаёт в снапшотах/дельтах.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub kind: UnitKind,
    pub team: TeamId,
    pub x: i16,
    pub y: i16,
    pub hits: i16,
    pub max_hits: i16,
    pub speed: i16,
    pub shots: i16,
    pub storage: i16,
    pub ammo: i16,
}
