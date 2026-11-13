//! max_net — сетевая часть клиента: два канала к Horizon-инфраструктуре.
//!
//! 1. **WS к Horizon** (`lobby`): события `max_lobby:*` / `max_admin:*`, JSON-обёртка
//!    `{"namespace": "...", "event": "...", "data": {...}}`; ответы приходят как
//!    `{"event": "...", ...}`. Получение списка игр, создание, вход, тикет сессии.
//! 2. **UDP к headless-серверу сессии** (`session`): хендшейк (версия + тикет +
//!    data-hash), затем приказы вверх и авторитетное состояние вниз.
//!
//! Тикеты выдаёт Horizon; UDP-канал принимает их в хендшейке.

pub mod lobby;
pub mod protocol;
pub mod session;

pub use protocol::{GameListing, JoinRequest, LobbyEvent};
