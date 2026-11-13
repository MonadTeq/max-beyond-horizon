//! Configuration types for the M.A.X. Port session manager.
//!
//! Two layers of configuration exist:
//!
//! * [`MaxConfig`] describes the environment the manager itself runs in: where the headless
//!   executable and the session template live, where session directories are created, which
//!   UDP port range sessions may use, and the HMAC secrets for player tickets and admin
//!   tokens.
//! * [`SessionConfig`] describes a single game: map, timers, teams, the session ticket players
//!   must present, and the player limit.
//!
//! [`MaxConfig`] is assembled from three layers, in increasing priority order:
//!
//! 1. the historic defaults ([`MaxConfig::default`]);
//! 2. the `[max]` section of the Horizon `config.toml` ([`load_max_toml_section`] +
//!    [`MaxConfig::apply_toml`]);
//! 3. `MAX_*` environment variables ([`MaxConfig::apply_env`]) — the highest priority, so
//!    existing deployments that configure the manager through the environment keep working
//!    unchanged.
//!
//! [`MaxConfig::from_env_and_toml`] applies all three layers at once.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// Default of `session_root` when neither `[max]` nor `MAX_SESSION_ROOT` sets one.
const DEFAULT_SESSION_ROOT: &str = "sessions";

/// Default inclusive UDP port range (`MAX_PORT_RANGE`).
const DEFAULT_PORT_RANGE: (u16, u16) = (32000, 32100);

/// Default watchdog restart limit (`MAX_RESTARTS`).
const DEFAULT_MAX_RESTARTS: u32 = 3;

/// Default initial restart backoff (`MAX_RESTART_BACKOFF_MS`).
const DEFAULT_RESTART_BACKOFF_MS: u64 = 2000;

/// Environment configuration of the session manager.
#[derive(Clone, PartialEq)]
pub struct MaxConfig {
    /// Path to the headless `max-port.exe` (MAX_HEADLESS build).
    ///
    /// The executable is copied into every session directory, so the session pref-path
    /// (`.portable`) is isolated per session and its logs/saves never collide.
    pub executable: PathBuf,

    /// A prepared run directory to copy as the starting point of every session.
    ///
    /// It must contain everything the headless server needs next to its executable: `.portable`,
    /// `PATCHES.RES`, `settings.json`, `language.json`, `help.json`, `maxint.json`, `LICENSE`.
    /// The manager only rewrites `settings.json`; all other files are copied verbatim.
    pub template_dir: PathBuf,

    /// Root directory session directories are created under. One subdirectory per session.
    pub session_root: PathBuf,

    /// Game data folder (MAX.RES/PATCHES.RES) written into `SETUP.game_data` of the session.
    pub game_data: PathBuf,

    /// UDP port range (inclusive) sessions are allocated from.
    pub port_range: std::ops::RangeInclusive<u16>,

    /// How many times a crashed session is restarted before it is left in the `Exited` state.
    /// Zero disables automatic restarts.
    pub max_restarts: u32,

    /// Initial delay between a crash and the restart attempt; doubled after every restart
    /// (exponential backoff).
    pub restart_backoff: Duration,

    /// Shared secret for signed per-player session tickets (`max_auth`), minted by the lobby
    /// when a player joins a game. `None` keeps the legacy ticket behavior. Never logged.
    pub session_secret: Option<String>,

    /// Secret for admin tokens: `hex(HMAC-SHA256(admin_secret, "max-admin"))`. `None` makes
    /// `max_admin` reject every admin event (fail-closed). Never logged.
    pub admin_secret: Option<String>,
}

impl Default for MaxConfig {
    /// The historic defaults of the `MAX_*` environment variables. `executable` and
    /// `template_dir` start empty and must be filled in by the `[max]` section or the
    /// environment; [`MaxConfig::validate`] rejects an empty executable path.
    fn default() -> Self {
        Self {
            executable: PathBuf::new(),
            template_dir: PathBuf::new(),
            session_root: PathBuf::from(DEFAULT_SESSION_ROOT),
            game_data: PathBuf::new(),
            port_range: DEFAULT_PORT_RANGE.0..=DEFAULT_PORT_RANGE.1,
            max_restarts: DEFAULT_MAX_RESTARTS,
            restart_backoff: Duration::from_millis(DEFAULT_RESTART_BACKOFF_MS),
            session_secret: None,
            admin_secret: None,
        }
    }
}

impl fmt::Debug for MaxConfig {
    /// Debug output with the secrets redacted, so a config can never leak `session_secret`
    /// or `admin_secret` into logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MaxConfig")
            .field("executable", &self.executable)
            .field("template_dir", &self.template_dir)
            .field("session_root", &self.session_root)
            .field("game_data", &self.game_data)
            .field("port_range", &self.port_range)
            .field("max_restarts", &self.max_restarts)
            .field("restart_backoff", &self.restart_backoff)
            .field(
                "session_secret",
                &self.session_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("admin_secret", &self.admin_secret.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl MaxConfig {
    /// Validates the configuration: executable and template exist, the port range is sane.
    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.executable.is_file() {
            anyhow::bail!("headless executable not found: {}", self.executable.display());
        }
        if !self.template_dir.join("settings.json").is_file() {
            anyhow::bail!("session template has no settings.json: {}", self.template_dir.display());
        }
        if self.port_range.is_empty() {
            anyhow::bail!("empty port range");
        }
        Ok(())
    }

    /// Applies a parsed `[max]` TOML section on top of the current values.
    ///
    /// Keys missing from the section keep their current values, so `apply_toml` can be layered
    /// freely. Unknown keys and an inverted port range are errors.
    pub fn apply_toml(&mut self, section: &toml::Table) -> anyhow::Result<()> {
        let parsed = MaxTomlConfig::deserialize(section.clone())
            .map_err(|error| anyhow::anyhow!("invalid [max] section: {error}"))?;

        if let Some(value) = parsed.executable {
            self.executable = PathBuf::from(value);
        }
        if let Some(value) = parsed.template_dir {
            self.template_dir = PathBuf::from(value);
        }
        if let Some(value) = parsed.session_root {
            self.session_root = PathBuf::from(value);
        }
        if let Some(value) = parsed.game_data {
            self.game_data = PathBuf::from(value);
        }

        // A partial range keeps the other side of the current range.
        match (parsed.port_range_start, parsed.port_range_end) {
            (None, None) => {}
            (start, end) => {
                let start = start.unwrap_or(*self.port_range.start());
                let end = end.unwrap_or(*self.port_range.end());
                if start > end {
                    anyhow::bail!(
                        "invalid [max] port range: port_range_start {start} > port_range_end {end}"
                    );
                }
                self.port_range = start..=end;
            }
        }

        if let Some(value) = parsed.max_restarts {
            self.max_restarts = value;
        }
        if let Some(value) = parsed.restart_backoff_seconds {
            self.restart_backoff = Duration::from_secs(value);
        }
        if let Some(value) = parsed.session_secret {
            self.session_secret = non_empty_secret(value);
        }
        if let Some(value) = parsed.admin_secret {
            self.admin_secret = non_empty_secret(value);
        }

        Ok(())
    }

    /// Builds a [`MaxConfig`] from a parsed `[max]` TOML section, over the historic defaults.
    ///
    /// An empty section (a `config.toml` without `[max]`) yields the default configuration and
    /// never fails.
    pub fn from_toml(section: &toml::Table) -> anyhow::Result<MaxConfig> {
        let mut config = MaxConfig::default();
        config.apply_toml(section)?;
        Ok(config)
    }

    /// Applies the `MAX_*` environment variables on top of the current values.
    ///
    /// This is the highest-priority layer: an environment variable that is present (and, for
    /// numeric keys, parseable) always wins. Unset variables keep the current values.
    pub fn apply_env(&mut self) {
        if let Ok(value) = std::env::var("MAX_SERVER_EXE") {
            self.executable = PathBuf::from(value);
        }
        if let Ok(value) = std::env::var("MAX_TEMPLATE_DIR") {
            self.template_dir = PathBuf::from(value);
        }
        if let Ok(value) = std::env::var("MAX_SESSION_ROOT") {
            self.session_root = PathBuf::from(value);
        }
        if let Ok(value) = std::env::var("MAX_GAME_DATA") {
            self.game_data = PathBuf::from(value);
        }
        if let Some(range) = std::env::var("MAX_PORT_RANGE")
            .ok()
            .and_then(|value| parse_port_range(&value))
        {
            self.port_range = range;
        }
        if let Ok(value) = std::env::var("MAX_RESTARTS") {
            if let Ok(parsed) = value.parse() {
                self.max_restarts = parsed;
            }
        }
        if let Ok(value) = std::env::var("MAX_RESTART_BACKOFF_MS") {
            if let Ok(millis) = value.parse() {
                self.restart_backoff = Duration::from_millis(millis);
            }
        }
        if let Ok(value) = std::env::var("MAX_SESSION_SECRET") {
            self.session_secret = non_empty_secret(value);
        }
        if let Ok(value) = std::env::var("MAX_ADMIN_SECRET") {
            self.admin_secret = non_empty_secret(value);
        }
    }

    /// Full layered load: defaults, then the `[max]` TOML section (when given), then the
    /// `MAX_*` environment variables. The environment has the highest priority.
    pub fn from_env_and_toml(section: Option<&toml::Table>) -> anyhow::Result<MaxConfig> {
        let mut config = MaxConfig::default();
        if let Some(section) = section {
            config.apply_toml(section)?;
        }
        config.apply_env();
        Ok(config)
    }
}

/// Parsed form of the `[max]` section of the Horizon `config.toml`.
///
/// Every key is optional (the absent keys keep their defaults or the values of lower-priority
/// layers), and unknown keys are rejected so typos surface instead of being silently ignored.
/// See [`MaxConfig::apply_toml`] for how the keys are applied.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaxTomlConfig {
    /// `[max].executable` — path to the headless `max-port.exe` (`MAX_SERVER_EXE`).
    pub executable: Option<String>,

    /// `[max].template_dir` — prepared run directory copied per session (`MAX_TEMPLATE_DIR`).
    pub template_dir: Option<String>,

    /// `[max].session_root` — root directory session folders are created under
    /// (`MAX_SESSION_ROOT`).
    pub session_root: Option<String>,

    /// `[max].game_data` — game data folder written into `SETUP.game_data` (`MAX_GAME_DATA`).
    pub game_data: Option<String>,

    /// `[max].port_range_start` — first UDP port sessions may use (`MAX_PORT_RANGE`
    /// "start-end").
    pub port_range_start: Option<u16>,

    /// `[max].port_range_end` — last UDP port sessions may use (`MAX_PORT_RANGE` "start-end").
    pub port_range_end: Option<u16>,

    /// `[max].max_restarts` — watchdog restart limit (`MAX_RESTARTS`).
    pub max_restarts: Option<u32>,

    /// `[max].restart_backoff_seconds` — initial crash-restart delay in seconds. The
    /// environment equivalent `MAX_RESTART_BACKOFF_MS` is in milliseconds.
    pub restart_backoff_seconds: Option<u64>,

    /// `[max].session_secret` — HMAC secret for signed player tickets (`MAX_SESSION_SECRET`).
    /// An empty string means "not set".
    pub session_secret: Option<String>,

    /// `[max].admin_secret` — HMAC secret for admin tokens (`MAX_ADMIN_SECRET`). An empty
    /// string means "not set".
    pub admin_secret: Option<String>,
}

/// Loads the `[max]` section of a Horizon `config.toml`.
///
/// Reads and parses the file and returns the value of its `max` key. A missing `[max]`
/// section yields an empty table, so an unconfigured file never breaks the manager. An
/// unreadable file, invalid TOML or a non-table `max` value is an error.
pub fn load_max_toml_section(path: &Path) -> anyhow::Result<toml::Table> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| anyhow::anyhow!("failed to read {}: {error}", path.display()))?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|error| anyhow::anyhow!("{} is not valid TOML: {error}", path.display()))?;
    let toml::Value::Table(mut root) = value else {
        anyhow::bail!("{} does not contain a TOML table", path.display());
    };
    match root.remove("max") {
        Some(toml::Value::Table(section)) => Ok(section),
        Some(_) => anyhow::bail!("the [max] section of {} is not a table", path.display()),
        None => Ok(toml::Table::new()),
    }
}

/// Parses `"32000-32100"` into an inclusive port range; `None` on garbage or an inverted
/// range. This is the format of the `MAX_PORT_RANGE` environment variable.
pub fn parse_port_range(value: &str) -> Option<std::ops::RangeInclusive<u16>> {
    let (start, end) = value.split_once('-')?;
    let start = start.trim().parse::<u16>().ok()?;
    let end = end.trim().parse::<u16>().ok()?;
    if start > end {
        return None;
    }
    Some(start..=end)
}

/// Normalizes a secret from config/env: an empty string means "not set", so consumers can
/// fail closed on `None` even when a deployment explicitly sets `secret = ""`.
fn non_empty_secret(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Team slot description for the game settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamConfig {
    /// Team display name (`red_team_name` etc.). Empty = the default name.
    pub name: String,
    /// Team strategy for computer players (`red_team_strategy` etc., "random" by default).
    pub strategy: String,
    /// Clan id (`red_team_clan` etc.).
    pub clan: i64,
}

/// A single game session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Human readable game name, shown in the lobby list. Defaults to the session id.
    pub name: Option<String>,

    /// Session ticket clients must present in the transport handshake. An empty string leaves
    /// the session open; the manager generates a ticket when the game is password protected.
    pub ticket: Option<String>,

    /// Password protection flag (the ticket is generated when set).
    pub password_protected: bool,

    /// Maximum remote players accepted by the lobby (`server_min_players` is the number the
    /// headless server waits for before starting).
    pub min_players: u32,

    /// Bind address of the session process, e.g. "0.0.0.0".
    pub host_address: String,

    /// World index (0..=23 in the original data).
    pub world: i64,

    /// Turn timer in seconds.
    pub timer: i64,

    /// End-turn timer in seconds.
    pub endturn: i64,

    /// 0 = simultaneous, 1 = turn based.
    pub play_mode: i64,

    /// Starting credits.
    pub start_gold: i64,

    /// Victory type (0 = turns, 1 = score, 2 = conquest...).
    pub victory_type: i64,

    /// Victory limit (turns or score depending on the type).
    pub victory_limit: i64,

    /// Raw resource abundance (0-2).
    pub raw_resource: i64,
    /// Fuel resource abundance (0-2).
    pub fuel_resource: i64,
    /// Gold resource abundance (0-2).
    pub gold_resource: i64,

    /// Alien derelicts enabled.
    pub alien_derelicts: i64,

    /// Per-team overrides; empty entries keep the template values.
    pub teams: [TeamConfig; 4],
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            name: None,
            ticket: None,
            password_protected: false,
            min_players: 2,
            host_address: "0.0.0.0".to_string(),
            world: 0,
            timer: 30,
            endturn: 15,
            play_mode: 1,
            start_gold: 200,
            victory_type: 0,
            victory_limit: 100,
            raw_resource: 1,
            fuel_resource: 1,
            gold_resource: 1,
            alien_derelicts: 0,
            teams: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that mutate process environment variables: the `MAX_*` namespace is
    /// global, so env-manipulating tests must not race each other.
    static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Sets/removes `vars` for the duration of `body`, then restores the previous values.
    fn with_env_vars(vars: &[(&str, Option<&str>)], body: impl FnOnce()) {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let saved: Vec<(&str, Option<String>)> = vars
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect();
        for (key, value) in vars {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        body();
        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, &value),
                None => std::env::remove_var(key),
            }
        }
    }

    #[test]
    fn default_session_config_is_sane() {
        let config = SessionConfig::default();

        assert_eq!(config.min_players, 2);
        assert_eq!(config.timer, 30);
        assert_eq!(config.endturn, 15);
        assert_eq!(config.play_mode, 1);
        assert_eq!(config.teams, [TeamConfig::default(), TeamConfig::default(), TeamConfig::default(), TeamConfig::default()]);
    }

    #[test]
    fn max_config_validation_rejects_missing_executable() {
        let _config = MaxConfig {
            executable: PathBuf::from("definitely/missing/max-port.exe"),
            template_dir: PathBuf::from("."),
            session_root: PathBuf::from("."),
            game_data: PathBuf::from("."),
            port_range: 20000..=20010,
            max_restarts: 3,
            restart_backoff: std::time::Duration::from_secs(1),
            session_secret: None,
            admin_secret: None,
        };

        assert!(_config.validate().is_err());
    }

    #[test]
    fn max_config_validation_rejects_empty_port_range() {
        let config = MaxConfig {
            executable: PathBuf::from("."),
            template_dir: PathBuf::from("."),
            session_root: PathBuf::from("."),
            game_data: PathBuf::from("."),
            port_range: 20010..=20000,
            max_restarts: 0,
            restart_backoff: std::time::Duration::from_secs(1),
            session_secret: None,
            admin_secret: None,
        };

        // Executable check fails first; the port range is validated after it, so build a config
        // that passes the first two checks to reach the range check.
        let temp = tempfile::tempdir().unwrap();
        let exe = temp.path().join("max-port.exe");
        std::fs::write(&exe, b"stub").unwrap();
        let template = temp.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        std::fs::write(template.join("settings.json"), b"{}").unwrap();

        let config = MaxConfig {
            executable: exe,
            template_dir: template,
            session_root: temp.path().to_path_buf(),
            game_data: temp.path().to_path_buf(),
            port_range: 20010..=20000,
            max_restarts: 0,
            restart_backoff: std::time::Duration::from_secs(1),
            session_secret: None,
            admin_secret: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn toml_section_round_trips_into_config() {
        let section: toml::Table = toml::from_str(
            r#"
            executable = "max-port.exe"
            template_dir = "server-run"
            session_root = "sessions"
            game_data = "MAX"
            port_range_start = 31554
            port_range_end = 31600
            max_restarts = 3
            restart_backoff_seconds = 1
            session_secret = "change-me"
            admin_secret = "change-me-admin"
            "#,
        )
        .unwrap();

        let config = MaxConfig::from_toml(&section).unwrap();

        assert_eq!(config.executable, PathBuf::from("max-port.exe"));
        assert_eq!(config.template_dir, PathBuf::from("server-run"));
        assert_eq!(config.session_root, PathBuf::from("sessions"));
        assert_eq!(config.game_data, PathBuf::from("MAX"));
        assert_eq!(config.port_range, 31554..=31600);
        assert_eq!(config.max_restarts, 3);
        assert_eq!(config.restart_backoff, Duration::from_secs(1));
        assert_eq!(config.session_secret.as_deref(), Some("change-me"));
        assert_eq!(config.admin_secret.as_deref(), Some("change-me-admin"));
    }

    #[test]
    fn absent_or_empty_toml_section_keeps_defaults() {
        let empty = toml::Table::new();
        assert_eq!(MaxConfig::from_toml(&empty).unwrap(), MaxConfig::default());

        let partial: toml::Table = toml::from_str("max_restarts = 7\n").unwrap();
        let config = MaxConfig::from_toml(&partial).unwrap();
        assert_eq!(config.max_restarts, 7);
        assert_eq!(config.port_range, 32000..=32100);
        assert_eq!(config.session_root, PathBuf::from("sessions"));
        assert_eq!(config.session_secret, None);
        assert_eq!(config.admin_secret, None);
    }

    #[test]
    fn load_max_toml_section_returns_an_empty_table_without_a_max_section() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "[server]\nbind_address = \"0.0.0.0:8081\"\n").unwrap();

        let section = load_max_toml_section(&path).unwrap();
        assert!(section.is_empty());
        assert_eq!(MaxConfig::from_toml(&section).unwrap(), MaxConfig::default());
    }

    #[test]
    fn load_max_toml_section_reports_errors() {
        let temp = tempfile::tempdir().unwrap();

        let missing = temp.path().join("missing.toml");
        assert!(load_max_toml_section(&missing).is_err());

        let invalid = temp.path().join("invalid.toml");
        std::fs::write(&invalid, "not [valid toml").unwrap();
        assert!(load_max_toml_section(&invalid).is_err());

        let non_table = temp.path().join("non-table.toml");
        std::fs::write(&non_table, "max = 42\n").unwrap();
        assert!(load_max_toml_section(&non_table).is_err());
    }

    #[test]
    fn shipped_config_toml_max_section_parses() {
        // The repository root config.toml is the shipped source of the `[max]` defaults; this
        // test keeps that file in sync with the loader. Skipped when the file is absent
        // (e.g. a standalone crate checkout).
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config.toml");
        let Ok(section) = load_max_toml_section(&path) else {
            eprintln!("skipping: {} not found", path.display());
            return;
        };
        let config = MaxConfig::from_toml(&section).expect("shipped [max] section must parse");
        assert_eq!(config.executable, PathBuf::from("max-port.exe"));
        assert_eq!(config.port_range, 31554..=31600);
        assert_eq!(config.max_restarts, 3);
    }

    #[test]
    fn toml_section_rejects_unknown_fields() {
        let section: toml::Table = toml::from_str("executable = \"x\"\nunknown_key = 1\n").unwrap();
        assert!(MaxConfig::from_toml(&section).is_err());
    }

    #[test]
    fn toml_section_rejects_inverted_port_ranges() {
        let section: toml::Table =
            toml::from_str("port_range_start = 100\nport_range_end = 50\n").unwrap();
        assert!(MaxConfig::from_toml(&section).is_err());
    }

    #[test]
    fn partial_toml_port_range_keeps_the_other_side() {
        let section: toml::Table = toml::from_str("port_range_start = 32050\n").unwrap();
        assert_eq!(MaxConfig::from_toml(&section).unwrap().port_range, 32050..=32100);

        let section: toml::Table = toml::from_str("port_range_end = 32200\n").unwrap();
        assert_eq!(MaxConfig::from_toml(&section).unwrap().port_range, 32000..=32200);
    }

    #[test]
    fn env_variables_override_the_toml_section() {
        let section: toml::Table = toml::from_str(
            r#"
            executable = "toml-exe"
            template_dir = "toml-template"
            port_range_start = 10
            port_range_end = 20
            max_restarts = 9
            restart_backoff_seconds = 7
            session_secret = "toml-secret"
            admin_secret = "toml-admin"
            "#,
        )
        .unwrap();

        with_env_vars(
            &[
                ("MAX_SERVER_EXE", Some("env-exe")),
                ("MAX_PORT_RANGE", Some("100-110")),
                ("MAX_RESTARTS", Some("2")),
                ("MAX_RESTART_BACKOFF_MS", Some("500")),
                ("MAX_SESSION_SECRET", Some("env-secret")),
                ("MAX_TEMPLATE_DIR", None),
                ("MAX_ADMIN_SECRET", None),
            ],
            || {
                let config = MaxConfig::from_env_and_toml(Some(&section)).unwrap();

                assert_eq!(config.executable, PathBuf::from("env-exe"));
                assert_eq!(config.template_dir, PathBuf::from("toml-template"));
                assert_eq!(config.port_range, 100..=110);
                assert_eq!(config.max_restarts, 2);
                assert_eq!(config.restart_backoff, Duration::from_millis(500));
                assert_eq!(config.session_secret.as_deref(), Some("env-secret"));
                assert_eq!(config.admin_secret.as_deref(), Some("toml-admin"));
            },
        );
    }

    #[test]
    fn env_variables_alone_reproduce_the_legacy_load() {
        with_env_vars(
            &[
                ("MAX_SERVER_EXE", Some("legacy-exe")),
                ("MAX_TEMPLATE_DIR", Some("legacy-template")),
                ("MAX_SESSION_ROOT", Some("legacy-sessions")),
                ("MAX_GAME_DATA", Some("legacy-data")),
                ("MAX_RESTARTS", Some("4")),
                ("MAX_RESTART_BACKOFF_MS", Some("250")),
                ("MAX_PORT_RANGE", Some("33000-33010")),
                ("MAX_SESSION_SECRET", None),
                ("MAX_ADMIN_SECRET", None),
            ],
            || {
                let config = MaxConfig::from_env_and_toml(None).unwrap();
                assert_eq!(config.executable, PathBuf::from("legacy-exe"));
                assert_eq!(config.template_dir, PathBuf::from("legacy-template"));
                assert_eq!(config.session_root, PathBuf::from("legacy-sessions"));
                assert_eq!(config.game_data, PathBuf::from("legacy-data"));
                assert_eq!(config.port_range, 33000..=33010);
                assert_eq!(config.max_restarts, 4);
                assert_eq!(config.restart_backoff, Duration::from_millis(250));
                assert_eq!(config.session_secret, None);
                assert_eq!(config.admin_secret, None);
            },
        );
    }

    #[test]
    fn empty_secrets_are_treated_as_unset() {
        let section: toml::Table =
            toml::from_str("session_secret = \"\"\nadmin_secret = \"\"\n").unwrap();
        let config = MaxConfig::from_toml(&section).unwrap();
        assert_eq!(config.session_secret, None);
        assert_eq!(config.admin_secret, None);
    }

    #[test]
    fn debug_does_not_leak_secrets() {
        let mut config = MaxConfig::default();
        config.session_secret = Some("super-secret-session".into());
        config.admin_secret = Some("super-secret-admin".into());

        let rendered = format!("{config:?}");
        assert!(!rendered.contains("super-secret-session"));
        assert!(!rendered.contains("super-secret-admin"));
        assert!(rendered.contains("<redacted>"));
    }
}
