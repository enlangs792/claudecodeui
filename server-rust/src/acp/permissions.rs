//! Pending permission requests and WS-driven resolution.

use std::collections::HashMap;
use std::sync::Arc;

use agent_client_protocol::schema::{
    PermissionOption, RequestPermissionOutcome, RequestPermissionResponse, SelectedPermissionOutcome,
};
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PermissionDecision {
    pub allow: bool,
    pub updated_input: Option<serde_json::Value>,
    pub message: Option<String>,
}

struct PendingEntry {
    options: Vec<PermissionOption>,
    respond_tx: oneshot::Sender<RequestPermissionOutcome>,
}

#[derive(Clone, Default)]
pub struct PermissionStore {
    inner: Arc<Mutex<HashMap<String, PendingEntry>>>,
}

impl PermissionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_wait(
        &self,
        options: Vec<PermissionOption>,
    ) -> (String, oneshot::Receiver<RequestPermissionOutcome>) {
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.inner.lock().await.insert(
            request_id.clone(),
            PendingEntry {
                options,
                respond_tx: tx,
            },
        );
        (request_id, rx)
    }

    pub async fn resolve(&self, request_id: &str, decision: PermissionDecision) -> bool {
        let Some(entry) = self.inner.lock().await.remove(request_id) else {
            return false;
        };

        let outcome = if decision.allow {
            let option_id = entry
                .options
                .iter()
                .find(|o| {
                    matches!(
                        o.kind,
                        agent_client_protocol::schema::PermissionOptionKind::AllowOnce
                            | agent_client_protocol::schema::PermissionOptionKind::AllowAlways
                    )
                })
                .or_else(|| entry.options.first())
                .map(|o| o.option_id.clone());

            match option_id {
                Some(id) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)),
                None => RequestPermissionOutcome::Cancelled,
            }
        } else {
            let reject_id = entry
                .options
                .iter()
                .find(|o| {
                    matches!(
                        o.kind,
                        agent_client_protocol::schema::PermissionOptionKind::RejectOnce
                            | agent_client_protocol::schema::PermissionOptionKind::RejectAlways
                    )
                })
                .map(|o| o.option_id.clone());

            if let Some(id) = reject_id {
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id))
            } else {
                RequestPermissionOutcome::Cancelled
            }
        };

        entry.respond_tx.send(outcome).is_ok()
    }

    pub async fn list_pending_for_session(&self, _session_id: &str) -> Vec<serde_json::Value> {
        let pending = self.inner.lock().await;
        pending
            .keys()
            .map(|id| serde_json::json!({ "requestId": id }))
            .collect()
    }

    pub async fn cancel_all(&self) {
        self.inner.lock().await.clear();
    }
}

pub fn auto_approve_permissions(
    permission_mode: Option<&str>,
    skip_permissions: bool,
) -> bool {
    if skip_permissions {
        return true;
    }
    match permission_mode {
        Some("bypassPermissions") | Some("bypass") => true,
        Some("plan") => false,
        _ => false,
    }
}

pub fn build_permission_response(outcome: RequestPermissionOutcome) -> RequestPermissionResponse {
    RequestPermissionResponse::new(outcome)
}
