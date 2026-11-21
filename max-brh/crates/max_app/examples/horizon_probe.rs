// Живой smoke: gRPC-проба Horizon (hello → list → create → join → тикет).
// Запуск: cargo run -p max_app --example horizon_probe -- <host> <port> [name] [min_players]
// `min_players` нужен live-smoke-3.sh: третий клиент должен успеть занять BLUE
// до старта сессии (по умолчанию сервер стартует после двух игроков).
use std::env;

use max_net::horizon::{CreateGameParams, HorizonClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let host = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8082);
    let player = args.get(3).cloned().unwrap_or_else(|| "probe".to_string());
    let _player = player; // имя игрока задаётся через create_game/join_game при необходимости
    // `min_players` по умолчанию оставляем серверу (2). Третьему клиенту нужен
    // слот: без `--min-players 3` сессия стартует после второго игрока, а
    // третий уже не успевает занять BLUE до пакета 34.
    let min_players = args.get(4).and_then(|value| value.parse::<u32>().ok());

    let mut client = HorizonClient::connect(&host, port).await?;
    println!("[1] connected to {host}:{port}");

    let hello = client.hello(1, "horizon_probe").await?;
    println!(
        "[2] hello: protocol={} format={} compression={}",
        hello.protocol_version, hello.format, hello.compression
    );

    let games = client.list_games().await?;
    println!("[3] list_games: {} games", games.len());

    let name = format!(
        "probe-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
    );
    let created = client
        .create_game(CreateGameParams {
            name: Some(name.clone()),
            min_players,
            ..Default::default()
        })
        .await?;
    println!(
        "[4] create_game: id={} name={}",
        created.game.id, created.game.name
    );

    // JoinGame спавнит headless-сессию и первое время отвечает
    // `FailedPrecondition: game is not running (status: starting)`, поэтому
    // ждём готовности до 30 с — так probe годится для проверки деплоя (M5.3).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let joined = loop {
        match client.join_game(&created.game.id, None).await {
            Ok(joined) => break joined,
            Err(error) if std::time::Instant::now() < deadline => {
                eprintln!("[5] join_game: {error}; повтор через 1 с");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Err(error) => return Err(error.into()),
        }
    };
    println!(
        "[5] join_game: host={} port={} ticket={} player={:?}",
        joined.host, joined.port, joined.ticket.value, joined.ticket.player_id
    );

    let pong = client.ping(None).await?;
    println!("[6] ping: {} ({})", pong.message, pong.timestamp);

    Ok(())
}
