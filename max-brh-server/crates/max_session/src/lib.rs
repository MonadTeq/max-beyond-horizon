//! # max_session
//!
//! Process manager for headless [M.A.X. Port] game sessions.
//!
//! The dedicated server architecture keeps the game simulation in the native C++ engine
//! (`max-port.exe --server`, a MAX_HEADLESS build) and puts the *control plane* in the Horizon
//! server. This crate is the missing piece between them: it spawns one headless process per
//! game session, supervises it and stops it cleanly.
//!
//! Every session gets:
//!
//! * a dedicated directory under the configured session root (a copy of a prepared template:
//!   executable, `.portable`, game assets and a `settings.json`), which isolates its
//!   logs (`stdout.txt`, `ai_log.txt`) and saves;
//! * a dedicated UDP port from the configured range;
//! * a `settings.json` rewritten with the session rules (map, timers, teams), the port and the
//!   session ticket clients must present in the transport handshake;
//! * a watchdog task that restarts the process after a crash, with exponential backoff, up to
//!   a configured limit.
//!
//! ## Example
//!
//! ```no_run
//! use max_session::{MaxConfig, SessionConfig, SessionManager};
//! use std::time::Duration;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let manager = SessionManager::new(MaxConfig {
//!     executable: "max-port-server/max-port.exe".into(),
//!     template_dir: "max-port-server/template".into(),
//!     session_root: "sessions".into(),
//!     game_data: "game-data/MAX".into(),
//!     port_range: 32000..=32100,
//!     max_restarts: 3,
//!     restart_backoff: Duration::from_secs(2),
//!     session_secret: None,
//!     admin_secret: None,
//! })?;
//!
//! let session = manager.start(SessionConfig {
//!     name: Some("Friday 2v2".into()),
//!     password_protected: true,
//!     min_players: 2,
//!     world: 0,
//!     ..Default::default()
//! }).await?;
//!
//! println!("session {} on port {}", session.id, session.port);
//! # manager.shutdown_all().await;
//! # Ok(())
//! # }
//! ```
//!
//! A `max_lobby` plugin (see the Horizon plugin examples) is the intended consumer: it maps the
//! `create_game`/`join_game` client events onto [`SessionManager`] calls and hands the session
//! ticket and port back to the joining player.
//!
//! [M.A.X. Port]: https://github.com/klei1984/max

pub mod config;
pub mod manager;
pub mod ports;
pub mod settings;

pub use config::{load_max_toml_section, parse_port_range, MaxConfig, MaxTomlConfig, SessionConfig, TeamConfig};
pub use manager::{SessionInfo, SessionManager, SessionStatus};
pub use ports::PortAllocator;

/// Gated end-to-end smoke test: spawns the real headless executable, waits for the NetLog file
/// and stops the session. Requires `MAX_SESSION_TEST_EXE` (path to a MAX_HEADLESS max-port.exe)
/// and `MAX_SESSION_TEST_DATA` (game data folder); skipped otherwise.
#[cfg(test)]
mod integration {
    use super::*;
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_executable_smoke_test() {
        let Ok(exe) = std::env::var("MAX_SESSION_TEST_EXE") else {
            eprintln!("MAX_SESSION_TEST_EXE not set, skipping the smoke test");
            return;
        };
        let game_data = std::env::var("MAX_SESSION_TEST_DATA").unwrap_or_default();

        let temp = tempfile::tempdir().expect("temp dir");
        let template = temp.path().join("template");
        std::fs::create_dir_all(&template).expect("template dir");

        for file in [".portable", "settings.json", "PATCHES.RES", "language.json", "help.json", "maxint.json", "LICENSE"] {
            let source = std::path::Path::new(&exe).parent().unwrap().join(file);
            if source.exists() {
                std::fs::copy(&source, template.join(file)).expect("copy template asset");
            }
        }
        if !template.join("settings.json").exists() {
            std::fs::write(template.join("settings.json"), serde_json::json!({"settings": {}}).to_string()).unwrap();
        }

        let manager = SessionManager::new(MaxConfig {
            executable: exe.into(),
            template_dir: template.clone(),
            session_root: temp.path().join("sessions"),
            game_data: game_data.into(),
            port_range: 33000..=33010,
            max_restarts: 0,
            restart_backoff: Duration::from_secs(1),
            session_secret: None,
            admin_secret: None,
        })
        .expect("manager config");

        let session = manager.start(SessionConfig::default()).await.expect("session starts");

        // The headless server writes ai_log.txt into its pref directory (.portable) once the
        // network lobby is up; wait for it up to 60 seconds.
        let netlog = session.dir.join("ai_log.txt");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        let mut up = false;

        while tokio::time::Instant::now() < deadline {
            if netlog.is_file() {
                up = true;
                break;
            }
            assert_eq!(manager.health(session.id), Some(SessionStatus::Running), "server died before the lobby came up");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        assert!(up, "ai_log.txt did not appear within 60 seconds");

        manager.stop(session.id).await.expect("session stops");
        assert!(manager.health(session.id).is_none());
    }
}
