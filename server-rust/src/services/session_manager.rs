//! Session manager — mirrors server/sessionManager.js
//!
//! Tracks active CLI sessions (Claude, Codex, Gemini, Cursor) with
//! their child process handles for lifecycle management.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::process::Child;
use tokio::sync::oneshot;

/// A managed CLI session
pub struct ManagedSession {
    pub session_id: String,
    pub provider: String,
    pub project_path: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub kill_tx: Option<oneshot::Sender<()>>,
}

/// Shared session manager state
pub struct SessionManager {
    sessions: HashMap<String, ManagedSession>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { sessions: HashMap::new() }
    }

    pub fn add(&mut self, session: ManagedSession) {
        self.sessions.insert(session.session_id.clone(), session);
    }

    pub fn remove(&mut self, session_id: &str) -> Option<ManagedSession> {
        self.sessions.remove(session_id)
    }

    pub fn get(&self, session_id: &str) -> Option<&ManagedSession> {
        self.sessions.get(session_id)
    }

    pub fn list(&self) -> Vec<&ManagedSession> {
        self.sessions.values().collect()
    }

    pub fn list_by_provider(&self, provider: &str) -> Vec<&ManagedSession> {
        self.sessions.values().filter(|s| s.provider == provider).collect()
    }

    pub fn is_active(&self, session_id: &str) -> bool {
        self.sessions.contains_key(session_id)
    }
}

pub type SharedSessionManager = Arc<Mutex<SessionManager>>;

pub fn new_shared() -> SharedSessionManager {
    Arc::new(Mutex::new(SessionManager::new()))
}
