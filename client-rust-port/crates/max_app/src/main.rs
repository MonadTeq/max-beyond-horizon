//! max_app — точка входа клиента: лобби → сессия → игровой цикл.

use anyhow::Result;
use max_net::lobby;
use max_net::protocol::{ClientEvent, LobbyEvent};

const HORIZON_DEFAULT: &str = "127.0.0.1:8081";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let host_port = std::env::args()
        .nth(1)
        .unwrap_or_else(|| HORIZON_DEFAULT.to_string());
    let (host, port) = host_port
        .split_once(':')
        .map(|(h, p)| (h.to_string(), p.parse().unwrap_or(8081)))
        .unwrap_or((HORIZON_DEFAULT.to_string(), 8081));

    let request = ClientEvent {
        namespace: "max_lobby".to_string(),
        event: "list_games".to_string(),
        data: serde_json::json!({}),
    };

    let reply: LobbyEvent = lobby::query(&host, port, &request, std::time::Duration::from_secs(5)).await?;
    match reply {
        LobbyEvent::GameList { games } => {
            tracing::info!("{} game(s) in the lobby", games.len());
            for game in games {
                tracing::info!("  {} at {}:{}", game.name, host, game.port);
            }
        }
        other => tracing::info!("lobby replied: {other:?}"),
    }

    Ok(())
}
