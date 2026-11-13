//! JSON-события лобби Horizon (`max_lobby:*`), общие для WS-канала.

use serde::{Deserialize, Serialize};

/// Обёртка запроса клиента: маршрутизируется как `namespace:event`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientEvent<T> {
    pub namespace: String,
    pub event: String,
    pub data: T,
}

/// Ответ лобби: `{"event": "...", ...}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LobbyEvent {
    GameList { games: Vec<GameListing> },
    GameCreated { game: GameListing, ticket: String },
    Error { error: String },
    #[serde(other)]
    Unknown,
}

/// Игра в списке лобби.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameListing {
    pub id: String,
    pub name: String,
    pub world: u8,
    pub status: String,
    pub port: u16,
    pub players: u32,
    pub min_players: u32,
    pub password_protected: bool,
}

/// Запрос входа в игру.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRequest {
    pub game_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lobby_event_parses_game_list() {
        let json = r#"{"event":"game_list","games":[{"id":"abc","name":"smoke-one","world":1,
            "status":"running","port":31554,"players":1,"min_players":2,"password_protected":false}]}"#;
        let event: LobbyEvent = serde_json::from_str(json).unwrap();
        match event {
            LobbyEvent::GameList { games } => assert_eq!(games[0].port, 31554),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn lobby_event_parses_error() {
        let event: LobbyEvent = serde_json::from_str(r#"{"event":"error","error":"bad"}"#).unwrap();
        assert!(matches!(event, LobbyEvent::Error { error } if error == "bad"));
    }
}
