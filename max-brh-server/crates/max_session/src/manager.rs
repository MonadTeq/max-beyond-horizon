//! The session manager: spawns, supervises and stops headless M.A.X. Port game sessions.
//!
//! Every session is one headless `max-port.exe` process in its own directory (a copy of the
//! manager's template), listening on a dedicated UDP port. A watchdog task per session restarts
//! the process after a crash, with exponential backoff, up to a configured limit.

use crate::config::{MaxConfig, SessionConfig};
use crate::ports::PortAllocator;
use crate::settings::write_session_settings;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, Notify};
use uuid::Uuid;

/// Lifecycle state of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// The process is being spawned (or a restart is pending).
    Starting,
    /// The process is running.
    Running,
    /// The process exited on its own and is not restarted any more; the code is the exit code
    /// (or -1 when it was killed by a signal).
    Exited(i32),
    /// The session was stopped through the manager.
    Stopped,
}

/// Snapshot of a session, safe to send to a client.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: Uuid,
    pub name: String,
    pub port: u16,
    pub status: SessionStatus,
    pub dir: PathBuf,
}

/// Name of the headless executable inside a session directory.
fn session_exe_name() -> &'static str {
    #[cfg(windows)]
    {
        "max-port.exe"
    }
    #[cfg(not(windows))]
    {
        "max-port"
    }
}

#[derive(Clone)]
struct SessionEntry {
    id: Uuid,
    config: SessionConfig,
    name: String,
    port: u16,
    dir: PathBuf,
    ticket: String,
    status: SessionStatus,
    requested_stop: Arc<AtomicBool>,
    restarts_left: Arc<Mutex<u32>>,
    stop_signal: Arc<Notify>,
    child: Arc<AsyncMutex<Option<Child>>>,
}

/// Process-lifetime tokio runtime for the watchdog tasks: the plugin handlers run on the host's
/// luminal runtime, so `tokio::spawn` at the call site has no reactor to attach to. A static
/// runtime is never dropped, which avoids tokio's "cannot drop a runtime in an async context"
/// panic when the manager is dropped from a plugin handler or an async test.
static SESSION_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

/// Manages the lifecycle of headless game sessions.
pub struct SessionManager {
    config: MaxConfig,
    ports: PortAllocator,
    sessions: Mutex<HashMap<Uuid, SessionEntry>>,
}

impl SessionManager {
    /// Creates a manager and validates its environment configuration.
    pub fn new(config: MaxConfig) -> anyhow::Result<Arc<Self>> {
        config.validate()?;

        SESSION_RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("failed to start the session runtime")
        });

        Ok(Arc::new(Self {
            ports: PortAllocator::new(config.port_range.clone()),
            sessions: Mutex::new(HashMap::new()),
            config,
        }))
    }

    /// Starts a new session: allocates a port, prepares the session directory and spawns the
    /// headless process under a watchdog.
    pub async fn start(self: &Arc<Self>, config: SessionConfig) -> anyhow::Result<SessionInfo> {
        let id = Uuid::new_v4();
        let port = self
            .ports
            .allocate()
            .ok_or_else(|| anyhow::anyhow!("no free UDP port left in the configured range"))?;

        let ticket = match (&config.ticket, config.password_protected) {
            (Some(ticket), _) => ticket.clone(),
            (None, true) => Uuid::new_v4().to_string(),
            (None, false) => String::new(),
        };

        let dir = self.config.session_root.join(id.to_string());
        let name = config
            .name
            .clone()
            .unwrap_or_else(|| id.to_string()[..8].to_string());

        // Prepare the directory before anything is registered, so a failure leaves no session
        // behind except the released port.
        if let Err(error) = self.prepare_session_dir(&dir, &config, port, &ticket) {
            self.ports.release(port);
            return Err(error);
        }

        let entry = SessionEntry {
            id,
            config: config.clone(),
            name,
            port,
            dir: dir.clone(),
            ticket,
            status: SessionStatus::Starting,
            requested_stop: Arc::new(AtomicBool::new(false)),
            restarts_left: Arc::new(Mutex::new(self.config.max_restarts)),
            stop_signal: Arc::new(Notify::new()),
            child: Arc::new(AsyncMutex::new(None)),
        };

        {
            let mut sessions = self.sessions.lock().expect("session registry poisoned");
            sessions.insert(id, entry);
        }

        self.spawn_watchdog(id);
        Ok(self.info(id).expect("session just registered"))
    }

    /// Stops a session and releases its port. The watchdog observes the stop request and
    /// terminates quietly.
    pub async fn stop(self: &Arc<Self>, id: Uuid) -> anyhow::Result<()> {
        let (requested_stop, stop_signal, child) = {
            let sessions = self.sessions.lock().expect("session registry poisoned");
            let entry = sessions
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("session {id} does not exist"))?;

            (Arc::clone(&entry.requested_stop), Arc::clone(&entry.stop_signal), Arc::clone(&entry.child))
        };

        requested_stop.store(true, Ordering::Release);
        stop_signal.notify_waiters();

        if let Some(child) = child.lock().await.as_mut() {
            let _ = child.kill().await;
        }

        let mut sessions = self.sessions.lock().expect("session registry poisoned");
        if let Some(entry) = sessions.remove(&id) {
            self.ports.release(entry.port);
            tracing::info!(session = %id, port = entry.port, "session stopped");
        }

        Ok(())
    }

    /// Stops a session and starts a fresh one with the same configuration (new id and port).
    pub async fn restart(self: &Arc<Self>, id: Uuid) -> anyhow::Result<SessionInfo> {
        let config = self
            .config_of(id)
            .ok_or_else(|| anyhow::anyhow!("session {id} does not exist"))?;

        self.stop(id).await?;
        self.start(config).await
    }

    /// Current status of a session.
    pub fn health(&self, id: Uuid) -> Option<SessionStatus> {
        self.info(id).map(|info| info.status)
    }

    /// Lists all sessions.
    pub fn list(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.lock().expect("session registry poisoned");
        sessions.values().map(Self::entry_info).collect()
    }

    /// Stops every session. Intended for graceful server shutdown.
    pub async fn shutdown_all(self: &Arc<Self>) {
        let ids: Vec<Uuid> = {
            let sessions = self.sessions.lock().expect("session registry poisoned");
            sessions.keys().copied().collect()
        };

        for id in ids {
            let _ = self.stop(id).await;
        }
    }

    fn config_of(&self, id: Uuid) -> Option<SessionConfig> {
        let sessions = self.sessions.lock().expect("session registry poisoned");
        sessions.get(&id).map(|entry| entry.config.clone())
    }

    fn info(&self, id: Uuid) -> Option<SessionInfo> {
        let sessions = self.sessions.lock().expect("session registry poisoned");
        sessions.get(&id).map(Self::entry_info)
    }

    fn entry_info(entry: &SessionEntry) -> SessionInfo {
        SessionInfo {
            id: entry.id,
            name: entry.name.clone(),
            port: entry.port,
            status: entry.status,
            dir: entry.dir.clone(),
        }
    }

    fn set_status(&self, id: Uuid, status: SessionStatus) {
        let mut sessions = self.sessions.lock().expect("session registry poisoned");
        if let Some(entry) = sessions.get_mut(&id) {
            entry.status = status;
        }
    }

    /// Copies the template directory into `dir`, writes the session settings and places the
    /// headless executable there.
    fn prepare_session_dir(&self, dir: &Path, config: &SessionConfig, port: u16, ticket: &str) -> anyhow::Result<()> {
        copy_dir_recursive(&self.config.template_dir, dir)
            .map_err(|error| anyhow::anyhow!("failed to prepare {}: {error}", dir.display()))?;

        write_session_settings(dir, config, port, ticket, &self.config.game_data)?;

        // The session runs the executable shipped with the manager; the template copy is only
        // a fallback for stale setups.
        let session_exe = dir.join(session_exe_name());
        if session_exe.exists() {
            let _ = std::fs::remove_file(&session_exe);
        }
        std::fs::copy(&self.config.executable, &session_exe).map_err(|error| {
            anyhow::anyhow!("failed to copy {} to {}: {error}", self.config.executable.display(), session_exe.display())
        })?;

        Ok(())
    }

    fn spawn_watchdog(self: &Arc<Self>, id: Uuid) {
        let manager = Arc::clone(self);

        SESSION_RUNTIME
            .get()
            .expect("session runtime initialized")
            .spawn(async move {
                manager.watchdog_loop(id).await;
            });
    }

    async fn spawn_process(&self, dir: &Path) -> anyhow::Result<Child> {
        let exe = dir.join(session_exe_name());

        let mut command = Command::new(&exe);
        command.arg("--server").current_dir(dir);
        command.kill_on_drop(true);

        let stdout = std::fs::File::create(dir.join("session_stdout.txt"))?;
        let stderr = std::fs::File::create(dir.join("session_stderr.txt"))?;
        command
            .stdout(std::process::Stdio::from(stdout))
            .stderr(std::process::Stdio::from(stderr));

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: the headless server runs without a console.
            command.creation_flags(0x0800_0000);
        }

        #[cfg(unix)]
        {
            // The session directory carries the shared libraries next to the executable
            // (the template ships them); the loader finds them via LD_LIBRARY_PATH.
            command.env("LD_LIBRARY_PATH", dir);
        }

        Ok(command.spawn()?)
    }

    async fn watchdog_loop(&self, id: Uuid) {
        let (requested_stop, restarts_left, stop_signal, child_slot, dir) = {
            let sessions = self.sessions.lock().expect("session registry poisoned");
            let Some(entry) = sessions.get(&id) else {
                return;
            };
            (
                Arc::clone(&entry.requested_stop),
                Arc::clone(&entry.restarts_left),
                Arc::clone(&entry.stop_signal),
                Arc::clone(&entry.child),
                entry.dir.clone(),
            )
        };

        let mut backoff = self.config.restart_backoff;

        loop {
            let child = match self.spawn_process(&dir).await {
                Ok(child) => child,
                Err(error) => {
                    tracing::error!(session = %id, %error, "failed to spawn the session process");
                    self.set_status(id, SessionStatus::Exited(-1));
                    return;
                }
            };

            {
                let mut slot = child_slot.lock().await;
                *slot = Some(child);
            }

            self.set_status(id, SessionStatus::Running);
            tracing::info!(session = %id, "session process started");

            // Wait for either a natural exit or a stop request; a stop request kills the
            // process, so the slot is only ever emptied here or by the manager.
            let exit_status = {
                let mut slot = child_slot.lock().await;
                let Some(child) = slot.as_mut() else {
                    // The manager took the child to kill it: the stop path is handling it.
                    self.set_status(id, SessionStatus::Stopped);
                    return;
                };

                tokio::select! {
                    status = child.wait() => Some(status),
                    _ = stop_signal.notified() => {
                        let _ = child.kill().await;
                        None
                    }
                }
            };

            if exit_status.is_none() || requested_stop.load(Ordering::Acquire) {
                self.set_status(id, SessionStatus::Stopped);
                tracing::info!(session = %id, "session process stopped on request");
                return;
            }

            let code = exit_status
                .map_or(-1, |status| status.ok().and_then(|status| status.code()).unwrap_or(-1));

            let restarts_remaining = {
                let mut restarts = restarts_left.lock().expect("restart counter poisoned");
                *restarts = restarts.saturating_sub(1);
                *restarts
            };

            if restarts_remaining == 0 {
                tracing::warn!(session = %id, exit_code = code, "session exited and the restart limit is reached");
                self.set_status(id, SessionStatus::Exited(code));
                return;
            }

            tracing::warn!(
                session = %id,
                exit_code = code,
                restarts_left = restarts_remaining,
                delay_ms = backoff.as_millis(),
                "session exited unexpectedly, restarting"
            );
            self.set_status(id, SessionStatus::Starting);
            tokio::time::sleep(backoff).await;
            backoff = backoff.saturating_mul(2);
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());

        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TeamConfig;
    use std::time::Duration;

    fn stub_max_config(template: &Path, root: &Path) -> MaxConfig {
        let exe = root.join(session_exe_name());
        std::fs::write(&exe, b"stub").unwrap();
        std::fs::write(template.join(".portable"), b"").unwrap();

        MaxConfig {
            executable: exe,
            template_dir: template.to_path_buf(),
            session_root: root.to_path_buf(),
            game_data: root.join("data").to_path_buf(),
            port_range: 25000..=25005,
            max_restarts: 1,
            restart_backoff: Duration::from_millis(10),
            session_secret: None,
            admin_secret: None,
        }
    }

    #[tokio::test]
    async fn start_prepares_an_isolated_session_directory() {
        let temp = tempfile::tempdir().unwrap();
        let template = temp.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        std::fs::write(template.join("settings.json"), serde_json::json!({"settings": {}}).to_string()).unwrap();

        let manager = SessionManager::new(stub_max_config(&template, temp.path())).unwrap();
        let session = manager.start(SessionConfig::default()).await.unwrap();

        assert!((25000..=25005).contains(&session.port));
        assert!(session.dir.join("settings.json").is_file());
        assert!(session.dir.join(".portable").is_file());
        assert!(session.dir.join(session_exe_name()).is_file());

        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(session.dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["settings"]["NETWORK_SETTINGS"]["host_port"], session.port);
        assert_eq!(settings["settings"]["DEBUG"]["server_mode"], 1);

        manager.shutdown_all().await;
    }

    #[tokio::test]
    async fn ticket_is_generated_for_protected_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let template = temp.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        std::fs::write(template.join("settings.json"), serde_json::json!({"settings": {}}).to_string()).unwrap();

        let manager = SessionManager::new(stub_max_config(&template, temp.path())).unwrap();

        let mut config = SessionConfig::default();
        config.password_protected = true;
        let session = manager.start(config).await.unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(session.dir.join("settings.json")).unwrap()).unwrap();
        let ticket = settings["settings"]["NETWORK_SETTINGS"]["server_session_ticket"].as_str().unwrap();
        assert!(!ticket.is_empty());

        manager.shutdown_all().await;
    }

    #[tokio::test]
    async fn stop_releases_the_port_and_removes_the_session() {
        let temp = tempfile::tempdir().unwrap();
        let template = temp.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        std::fs::write(template.join("settings.json"), serde_json::json!({"settings": {}}).to_string()).unwrap();

        let manager = SessionManager::new(stub_max_config(&template, temp.path())).unwrap();
        let session = manager.start(SessionConfig::default()).await.unwrap();
        let port = session.port;

        manager.stop(session.id).await.unwrap();

        assert!(manager.health(session.id).is_none());
        assert!(manager.list().is_empty());

        // The port is free again: a new session can take it.
        let next = manager.start(SessionConfig::default()).await.unwrap();
        assert_eq!(next.port, port);

        manager.shutdown_all().await;
    }

    #[tokio::test]
    async fn stop_unknown_session_reports_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let template = temp.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        std::fs::write(template.join("settings.json"), serde_json::json!({"settings": {}}).to_string()).unwrap();

        let manager = SessionManager::new(stub_max_config(&template, temp.path())).unwrap();
        assert!(manager.stop(Uuid::new_v4()).await.is_err());

        manager.shutdown_all().await;
    }

    #[tokio::test]
    async fn team_overrides_survive_into_settings() {
        let temp = tempfile::tempdir().unwrap();
        let template = temp.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        std::fs::write(template.join("settings.json"), serde_json::json!({"settings": {}}).to_string()).unwrap();

        let manager = SessionManager::new(stub_max_config(&template, temp.path())).unwrap();

        let mut config = SessionConfig::default();
        config.teams = [
            TeamConfig { name: "Alpha".into(), strategy: String::new(), clan: 7 },
            TeamConfig { name: "Beta".into(), strategy: "random".into(), clan: 1 },
            TeamConfig::default(),
            TeamConfig::default(),
        ];

        let session = manager.start(config).await.unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(session.dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["settings"]["TEAMS"]["red_team_name"], "Alpha");
        assert_eq!(settings["settings"]["TEAMS"]["green_team_name"], "Beta");

        manager.shutdown_all().await;
    }
}
