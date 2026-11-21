//! Test doubles for the session service.
//!
//! [`MockSessions`] implements [`SessionControl`](crate::backend::SessionControl)
//! without touching the filesystem or spawning a single process. It is what the
//! lobby/admin unit tests and the gRPC integration test
//! (`tests/grpc_roundtrip.rs`) run against, so the *whole* adapter — registry,
//! password checks, ticket minting, admin authorization, protobuf conversion —
//! is exercised while the expensive `max-port` part is faked.
//!
//! Sessions start in [`SessionStatus::Starting`], because that is what a real
//! manager reports right after spawn; tests flip them to
//! [`SessionStatus::Running`] with [`MockSessions::set_status`] to allow a join.
//!
//! This module is tiny, dependency-free test support and is compiled into the
//! library (not behind `#[cfg(test)]`) so integration tests — which link the
//! library as an external crate — can use it too.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use max_session::{SessionConfig, SessionInfo, SessionStatus};
use uuid::Uuid;

use crate::backend::SessionControl;

/// In-memory [`SessionControl`] used by tests.
#[derive(Default)]
pub struct MockSessions {
    state: Mutex<MockState>,
}

#[derive(Default)]
struct MockState {
    sessions: HashMap<Uuid, SessionInfo>,
    next_port: u16,
    start_error: Option<String>,
    stop_error: Option<String>,
    restart_error: Option<String>,
}

impl MockSessions {
    /// Creates an empty fake whose sessions are allocated ports from 32000.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(MockState {
                next_port: 32000,
                ..MockState::default()
            }),
        })
    }

    /// Creates an empty fake whose first session gets `port`.
    pub fn with_next_port(port: u16) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(MockState {
                next_port: port,
                ..MockState::default()
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MockState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Makes the next `start` fail with `message` (or restores success).
    pub fn set_start_error(&self, message: Option<String>) {
        self.lock().start_error = message;
    }

    /// Makes every `stop` fail with `message` (or restores success).
    pub fn set_stop_error(&self, message: Option<String>) {
        self.lock().stop_error = message;
    }

    /// Makes every `restart` fail with `message` (or restores success).
    pub fn set_restart_error(&self, message: Option<String>) {
        self.lock().restart_error = message;
    }

    /// Sets the lifecycle state of a known session; returns false for unknown ids.
    pub fn set_status(&self, id: Uuid, status: SessionStatus) -> bool {
        let mut state = self.lock();
        match state.sessions.get_mut(&id) {
            Some(session) => {
                session.status = status;
                true
            }
            None => false,
        }
    }

    /// Whether the fake tracks `id`.
    pub fn contains(&self, id: Uuid) -> bool {
        self.lock().sessions.contains_key(&id)
    }

    /// Number of tracked sessions.
    pub fn len(&self) -> usize {
        self.lock().sessions.len()
    }

    /// Whether no sessions are tracked.
    pub fn is_empty(&self) -> bool {
        self.lock().sessions.is_empty()
    }

    /// Creates a session directly, bypassing `start` (for arranging a test).
    pub fn register(&self, config: &SessionConfig) -> SessionInfo {
        let mut state = self.lock();
        let id = Uuid::new_v4();
        let port = state.next_port;
        state.next_port = state.next_port.wrapping_add(1);
        let name = config
            .name
            .clone()
            .unwrap_or_else(|| id.to_string()[..8].to_string());
        let info = SessionInfo {
            id,
            name,
            port,
            status: SessionStatus::Starting,
            dir: PathBuf::from(format!("sessions/{id}")),
        };
        state.sessions.insert(id, info.clone());
        info
    }
}

#[async_trait]
impl SessionControl for MockSessions {
    async fn start(&self, config: SessionConfig) -> Result<SessionInfo, String> {
        {
            let state = self.lock();
            if let Some(error) = &state.start_error {
                return Err(error.clone());
            }
        }
        Ok(self.register(&config))
    }

    fn health(&self, id: Uuid) -> Option<SessionStatus> {
        self.lock().sessions.get(&id).map(|info| info.status)
    }

    fn list(&self) -> Vec<SessionInfo> {
        self.lock().sessions.values().cloned().collect()
    }

    async fn stop(&self, id: Uuid) -> Result<(), String> {
        let mut state = self.lock();
        if let Some(error) = &state.stop_error {
            return Err(error.clone());
        }
        match state.sessions.remove(&id) {
            Some(_) => Ok(()),
            None => Err(format!("session {id} does not exist")),
        }
    }

    async fn restart(&self, id: Uuid) -> Result<SessionInfo, String> {
        let name = {
            let mut state = self.lock();
            if let Some(error) = &state.restart_error {
                return Err(error.clone());
            }
            let Some(session) = state.sessions.remove(&id) else {
                return Err(format!("session {id} does not exist"));
            };
            session.name
        };

        // A restart starts a fresh session with the same configuration: the id
        // and port change, the name is kept.
        let new_id = Uuid::new_v4();
        let mut state = self.lock();
        let port = state.next_port;
        state.next_port = state.next_port.wrapping_add(1);
        let info = SessionInfo {
            id: new_id,
            name,
            port,
            status: SessionStatus::Starting,
            dir: PathBuf::from(format!("sessions/{new_id}")),
        };
        state.sessions.insert(new_id, info.clone());
        Ok(info)
    }

    async fn shutdown_all(&self) {
        self.lock().sessions.clear();
    }
}

impl std::fmt::Debug for MockSessions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MockSessions")
            .field("sessions", &self.lock().sessions.len())
            .finish()
    }
}
