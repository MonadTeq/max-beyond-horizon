//! Команды и их типы.

use serde::{Deserialize, Serialize};

/// Идентификатор команды (0-3 у оригинала, 4 = пришельцы).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TeamId(pub u8);

/// Кем управляется команда (C++ `PlayerType`, save.md `TeamType`).
///
/// Значения зафиксированы как в C++/V70: `None=0`, `Player=1` (HUMAN),
/// `Computer=2`, `Remote=3`, `Eliminated=4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum TeamKind {
    /// Пустой слот.
    None = 0,
    /// Локальный игрок (`TEAM_TYPE_HUMAN`).
    Player = 1,
    /// Компьютер (ИИ симулируется сервером).
    Computer = 2,
    /// Удалённый игрок (другой клиент).
    Remote = 3,
    /// Выбыла.
    Eliminated = 4,
}

/// Команда.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: TeamId,
    pub kind: TeamKind,
    pub name: String,
    pub clan: u8,
    /// Золото и стартовые ресурсы.
    pub gold: i32,
    pub start_gold: i32,
}

impl Team {
    pub fn new(id: TeamId, kind: TeamKind) -> Self {
        Self {
            id,
            kind,
            name: String::new(),
            clan: 0,
            gold: 0,
            start_gold: 0,
        }
    }
}
