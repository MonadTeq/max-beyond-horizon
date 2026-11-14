//! Юниты: типы, состояние, идентификаторы.

use serde::{Deserialize, Serialize};

use crate::team::TeamId;

/// Идентификатор юнита (16-битный id оригинала).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnitId(pub u16);

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
