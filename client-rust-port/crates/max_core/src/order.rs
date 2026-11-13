//! Приказы — ввод клиента, который уходит на сервер.

use serde::{Deserialize, Serialize};

use crate::unit::UnitId;

/// Приказ юниту. Клиент лишь *предлагает* приказ; легальность решает сервер.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Order {
    /// Движение в точку (маршрут).
    Move { unit: UnitId, path: Vec<(i16, i16)> },
    /// Атака юнита/точки.
    Attack { unit: UnitId, target: Target },
    /// Постройка здания в точке.
    Build { unit: UnitId, building: u8, x: i16, y: i16 },
    /// Смена режима (огонь/патруль/...).
    Mode { unit: UnitId, mode: u8 },
    /// Конец хода.
    EndTurn,
}

/// Цель атаки.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Target {
    Unit(UnitId),
    Cell(i16, i16),
}

/// Подтверждение/отклонение приказа от сервера.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderResult {
    Accepted { seq: u64 },
    Rejected { seq: u64, reason: String },
}
