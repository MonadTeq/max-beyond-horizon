//! Horizon lobby JSON events (`max_lobby:*`), shared by the WS channel.

use serde::{Deserialize, Serialize};

/// Client request wrapper: routed as `namespace:event`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientEvent<T> {
    pub namespace: String,
    pub event: String,
    pub data: T,
}

/// Lobby response: `{"event": "...", ...}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LobbyEvent {
    GameList {
        games: Vec<GameListing>,
    },
    GameCreated {
        game: GameListing,
        ticket: String,
    },
    Error {
        error: String,
    },
    #[serde(other)]
    Unknown,
}

/// A game in the lobby list.
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

/// Request to join a game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRequest {
    pub game_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
