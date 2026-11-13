//! UDP-канал к headless-серверу сессии.
//!
//! Контракт канала (по PLAN.md v2): хендшейк «версия + тикет + data-hash»,
//! вверх — приказы (`max_core::Order`) и события лобби-фазы (выбор команды, точки
//! высадки, конец хода), вниз — настройки, снапшоты/дельты состояния, кадровая
//! синхронизация. Реализация канала — фаза R2.

use max_core::order::Order;

/// Параметры подключения к сессии.
#[derive(Debug, Clone)]
pub struct SessionParams {
    pub host: String,
    pub port: u16,
    pub ticket: String,
}

/// Исходящее сообщение клиента.
#[derive(Debug, Clone)]
pub enum ClientMessage {
    Order(Order),
    SelectTeam(u8),
    SelectSite(i16, i16),
    EndTurn,
    Chat(String),
}

/// Входящее сообщение сервера (заглушка контракта; наполняется в фазе R2).
#[derive(Debug, Clone)]
pub enum ServerMessage {
    Settings(serde_json::Value),
    Snapshot(serde_json::Value),
    Delta(serde_json::Value),
    FrameSync(u32),
}

/// Сессия-заглушка: канал будет реализован на tokio::net::UdpSocket.
#[derive(Debug)]
pub struct Session {
    pub params: SessionParams,
}

impl Session {
    pub fn new(params: SessionParams) -> Self {
        Self { params }
    }
}
