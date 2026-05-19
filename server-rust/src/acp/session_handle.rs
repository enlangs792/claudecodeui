//! Per-UI-session ACP state and child process lifecycle.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use agent_client_protocol::schema::ContentBlock;
use tokio::process::Child;
use tokio::sync::{broadcast, mpsc};

use crate::acp::permissions::PermissionStore;
use crate::shared::types::LlmProvider;

/// Commands sent to the session driver task.
pub enum SessionCommand {
    Prompt {
        blocks: Vec<ContentBlock>,
    },
    Abort,
}

pub struct ChildGuard {
    child: Mutex<Option<Child>>,
    aborted: AtomicBool,
}

impl ChildGuard {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            aborted: AtomicBool::new(false),
        }
    }

    pub fn set_child(&self, child: Child) {
        if let Ok(mut guard) = self.child.lock() {
            *guard = Some(child);
        }
    }

    pub async fn kill(&self) {
        if self.aborted.swap(true, Ordering::SeqCst) {
            return;
        }
        let child = self.child.lock().ok().and_then(|mut g| g.take());
        if let Some(mut child) = child {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.aborted.load(Ordering::SeqCst) {
            if let Ok(mut guard) = self.child.try_lock() {
                if let Some(mut child) = guard.take() {
                    let _ = child.start_kill();
                }
            }
        }
    }
}

pub struct AcpSessionHandle {
    pub ui_session_id: String,
    pub provider: LlmProvider,
    pub cwd: PathBuf,
    pub cmd_tx: mpsc::Sender<SessionCommand>,
    pub event_tx: broadcast::Sender<String>,
    pub child: Arc<ChildGuard>,
    pub permissions: PermissionStore,
    pub processing: Arc<AtomicBool>,
    /// Signaled once a WS forwarder is subscribed to `event_tx`.
    pub forwarder_ready: Arc<Notify>,
}

impl AcpSessionHandle {
    pub fn subscribe_events(&self) -> broadcast::Receiver<String> {
        self.event_tx.subscribe()
    }

    pub fn is_processing(&self) -> bool {
        self.processing.load(Ordering::SeqCst)
    }

    pub async fn send_prompt(&self, blocks: Vec<ContentBlock>) -> Result<(), String> {
        self.cmd_tx
            .send(SessionCommand::Prompt { blocks })
            .await
            .map_err(|_| "Session channel closed".to_string())
    }

    pub async fn abort(&self) {
        let _ = self.cmd_tx.send(SessionCommand::Abort).await;
        self.child.kill().await;
        self.permissions.cancel_all().await;
        self.processing.store(false, Ordering::SeqCst);
    }

    /// Begin ACP I/O after a forwarder has subscribed to session events.
    pub fn begin(&self) {
        self.forwarder_ready.notify_waiters();
    }
}
