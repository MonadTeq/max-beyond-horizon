//! Команды и их типы.

use serde::{Deserialize, Serialize};

/// Идентификатор команды (0-3 у оригинала, 4 = пришельцы).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TeamId(pub u8);

/// Кем управляется команда.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeamKind {
    /// Локальный игрок.
    Player,
    /// Удалённый игрок (другой клиент) — с точки зрения сервера любой клиент удалённый.
    Remote,
    /// Компьютер (ИИ симулируется сервером).
    Computer,
    /// Пустой слот.
    None,
    /// Выбыла.
    Eliminated,
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
