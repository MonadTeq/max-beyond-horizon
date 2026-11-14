// Живой smoke: gRPC-проба Horizon (hello → list → create → join → тикет).
// Запуск: cargo run -p max_app --example horizon_probe -- <host> <port> [name]
use std::env;

use max_net::horizon::{CreateGameParams, HorizonClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1".to_string());
    let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8082);
    let player = args.get(3).cloned().unwrap_or_else(|| "probe".to_string());
    let _player = player; // имя игрока задаётся через create_game/join_game при необходимости

    let mut client = HorizonClient::connect(&host, port).await?;
    println!("[1] connected to {host}:{port}");

    let hello = client.hello(1, "horizon_probe").await?;
    println!(
        "[2] hello: protocol={} format={} compression={}",
        hello.protocol_version, hello.format, hello.compression
    );

    let games = client.list_games().await?;
    println!("[3] list_games: {} games", games.len());

    let name = format!("probe-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?.as_secs());
    let created = client
        .create_game(CreateGameParams { name: Some(name.clone()), ..Default::default() })
        .await?;
    println!("[4] create_game: id={} name={}", created.game.id, created.game.name);

    let joined = client.join_game(&created.game.id, None).await?;
    println!(
        "[5] join_game: host={} port={} ticket={} player={:?}",
        joined.host, joined.port, joined.ticket.value, joined.ticket.player_id
    );

    let pong = client.ping(None).await?;
    println!("[6] ping: {} ({})", pong.message, pong.timestamp);

    Ok(())
}
