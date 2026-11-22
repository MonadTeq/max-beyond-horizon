// Live smoke: Horizon gRPC probe (hello → list → create → join → ticket).
// Run: cargo run -p max_app --example horizon_probe -- <host> <port> [name] [min_players]
// `min_players` is needed by live-smoke-3.sh: the third client must manage to take BLUE
// before the session starts (by default the server starts after two players).
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
    let _player = player; // the player name is set via create_game/join_game when needed
    // We leave `min_players` at the server default (2). The third client needs a
    // slot: without `--min-players 3` the session starts after the second player, and
    // the third one no longer manages to take BLUE before packet 34.
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

    // JoinGame spawns a headless session and for a while answers
    // `FailedPrecondition: game is not running (status: starting)`, so
    // we wait for readiness up to 30 s — this makes the probe suitable for deploy checks (M5.3).
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
