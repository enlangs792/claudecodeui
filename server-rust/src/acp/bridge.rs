//! ACP bridge — session registry and WS dispatch.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent_client_protocol::schema::{
    CancelNotification, ContentBlock, InitializeRequest, PromptRequest, ProtocolVersion,
    RequestPermissionOutcome, RequestPermissionRequest,
    SessionNotification,
};
use agent_client_protocol::util::MatchDispatch;
use agent_client_protocol::{Agent, Client, ConnectionTo, Error as AcpError, Result as AcpResult, SessionMessage};
use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc};

use crate::acp::mapper::ws_inbound::{build_content_blocks, parse_permission_response, ProviderCommand};
use crate::acp::mapper::ws_outbound::{
    self, complete, error_message, permission_request, session_created,
};
use crate::acp::permissions::{auto_approve_permissions, build_permission_response, PermissionStore};
use crate::acp::registry::ProviderRegistry;
use crate::acp::session_handle::{AcpSessionHandle, ChildGuard, SessionCommand};
use crate::shared::types::LlmProvider;

pub struct AcpBridge {
    sessions: DashMap<String, Arc<AcpSessionHandle>>,
}

impl Default for AcpBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl AcpBridge {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    pub fn is_active(&self, provider: &str, ui_session_id: &str) -> bool {
        let key = session_key(provider, ui_session_id);
        self.sessions
            .get(&key)
            .map(|h| h.is_processing())
            .unwrap_or(false)
    }

    pub fn abort_session(&self, provider: &str, ui_session_id: &str) -> bool {
        let key = session_key(provider, ui_session_id);
        if let Some((_, handle)) = self.sessions.remove(&key) {
            let handle = handle;
            tokio::spawn(async move {
                handle.abort().await;
            });
            true
        } else {
            false
        }
    }

    pub fn get_active_sessions(&self) -> serde_json::Value {
        let mut by_provider: HashMap<&str, Vec<String>> = HashMap::new();
        for entry in self.sessions.iter() {
            if entry.value().is_processing() {
                by_provider
                    .entry(entry.value().provider.as_str())
                    .or_default()
                    .push(entry.value().ui_session_id.clone());
            }
        }
        serde_json::json!({
            "claude": by_provider.get("claude").cloned().unwrap_or_default(),
            "cursor": by_provider.get("cursor").cloned().unwrap_or_default(),
            "codex": by_provider.get("codex").cloned().unwrap_or_default(),
            "gemini": by_provider.get("gemini").cloned().unwrap_or_default(),
        })
    }

    pub fn reconnect_writer(
        &self,
        provider: &str,
        ui_session_id: &str,
        ws_tx: mpsc::Sender<String>,
    ) -> bool {
        let key = session_key(provider, ui_session_id);
        let Some(handle) = self.sessions.get(&key) else {
            return false;
        };
        if !handle.is_processing() {
            return false;
        }
        let mut rx = handle.subscribe_events();
        tokio::spawn(async move {
            while let Ok(msg) = rx.recv().await {
                if ws_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });
        true
    }

    pub async fn handle_permission_response(
        &self,
        request_id: &str,
        decision: crate::acp::permissions::PermissionDecision,
    ) -> bool {
        for entry in self.sessions.iter() {
            if entry
                .permissions
                .resolve(request_id, decision.clone())
                .await
            {
                return true;
            }
        }
        false
    }

    pub async fn get_pending_permissions(&self, session_id: &str) -> Vec<serde_json::Value> {
        for entry in self.sessions.iter() {
            if entry.value().ui_session_id == session_id {
                return entry
                    .permissions
                    .list_pending_for_session(session_id)
                    .await;
            }
        }
        vec![]
    }

    pub async fn handle_provider_command(
        &self,
        cmd: ProviderCommand,
        ws_tx: mpsc::Sender<String>,
    ) {
        let key = session_key(cmd.provider.as_str(), &cmd.ui_session_id);

        if let Some(handle) = self.sessions.get(&key) {
            if cmd.resume || handle.is_processing() {
                let blocks = build_content_blocks(&cmd);
                Self::forward_events(handle.clone(), ws_tx.clone(), cmd.provider);
                if let Err(e) = handle.send_prompt(blocks).await {
                    let _ = ws_tx
                        .send(error_message(
                            &cmd.ui_session_id,
                            cmd.provider,
                            &e,
                        ))
                        .await;
                }
                return;
            }
        }

        if let Err(e) = ProviderRegistry::check_binary_in_path(cmd.provider) {
            let _ = ws_tx
                .send(error_message(&cmd.ui_session_id, cmd.provider, &e))
                .await;
            return;
        }

        let _ = ws_tx
            .send(session_created(&cmd.ui_session_id, cmd.provider))
            .await;

        match self.spawn_session(cmd.clone()).await {
            Ok(handle) => {
                self.sessions.insert(key.clone(), handle.clone());
                let blocks = build_content_blocks(&cmd);
                Self::forward_events(handle.clone(), ws_tx.clone(), cmd.provider);
                if let Err(e) = handle.send_prompt(blocks).await {
                    let _ = ws_tx
                        .send(error_message(
                            &cmd.ui_session_id,
                            cmd.provider,
                            &e,
                        ))
                        .await;
                }
            }
            Err(e) => {
                let _ = ws_tx
                    .send(error_message(&cmd.ui_session_id, cmd.provider, &e))
                    .await;
            }
        }
    }

    async fn spawn_session(
        &self,
        cmd: ProviderCommand,
    ) -> Result<Arc<AcpSessionHandle>, String> {
        let agent = ProviderRegistry::resolve_agent(cmd.provider)?;
        let (event_tx, _) = broadcast::channel::<String>(256);
        let (cmd_tx, cmd_rx) = mpsc::channel::<SessionCommand>(32);
        let child = Arc::new(ChildGuard::new());
        let permissions = PermissionStore::new();
        let processing = Arc::new(AtomicBool::new(true));

        let ui_session_id = cmd.ui_session_id.clone();
        let provider = cmd.provider;
        let cwd = cmd.cwd.clone();
        let auto_approve = auto_approve_permissions(
            cmd.permission_mode.as_deref(),
            cmd.skip_permissions,
        );
        let model = cmd.model.clone();
        let sandbox_mode = cmd.sandbox_mode.clone();

        let event_tx_driver = event_tx.clone();
        let child_driver = child.clone();
        let permissions_driver = permissions.clone();
        let processing_driver = processing.clone();
        let forwarder_ready = Arc::new(tokio::sync::Notify::new());
        let forwarder_ready_driver = forwarder_ready.clone();

        let ui_session_id_err = ui_session_id.clone();
        tokio::spawn(async move {
            forwarder_ready_driver.notified().await;
            if let Err(e) = run_session_driver(
                agent,
                cmd_rx,
                event_tx_driver.clone(),
                child_driver,
                permissions_driver,
                processing_driver,
                ui_session_id,
                provider,
                cwd,
                auto_approve,
                model,
                sandbox_mode,
            )
            .await
            {
                let _ = event_tx_driver.send(error_message(
                    &ui_session_id_err,
                    provider,
                    &e,
                ));
            }
        });

        Ok(Arc::new(AcpSessionHandle {
            ui_session_id: cmd.ui_session_id,
            provider: cmd.provider,
            cwd: cmd.cwd,
            cmd_tx,
            event_tx,
            child,
            permissions,
            processing,
            forwarder_ready,
        }))
    }

    fn forward_events(
        handle: Arc<AcpSessionHandle>,
        ws_tx: mpsc::Sender<String>,
        provider: LlmProvider,
    ) {
        tokio::spawn(async move {
            let mut rx = handle.subscribe_events();
            handle.begin();
            let mut sent_complete = false;
            while let Ok(msg) = rx.recv().await {
                sent_complete = msg.contains("\"kind\":\"complete\"");
                if ws_tx.send(msg).await.is_err() {
                    break;
                }
                if sent_complete {
                    handle.processing.store(false, Ordering::SeqCst);
                    break;
                }
            }
            if !sent_complete {
                handle.processing.store(false, Ordering::SeqCst);
                let _ = ws_tx
                    .send(complete(&handle.ui_session_id, provider))
                    .await;
            }
        });
    }
}

fn session_key(provider: &str, ui_session_id: &str) -> String {
    format!("{provider}:{ui_session_id}")
}

async fn run_session_driver(
    agent: agent_client_protocol::AcpAgent,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
    event_tx: broadcast::Sender<String>,
    child_guard: Arc<ChildGuard>,
    permissions: PermissionStore,
    processing: Arc<AtomicBool>,
    ui_session_id: String,
    provider: LlmProvider,
    cwd: PathBuf,
    auto_approve: bool,
    model: Option<String>,
    sandbox_mode: Option<String>,
) -> Result<(), String> {
    let ui_sid = ui_session_id.clone();
    let prov = provider;
    let event_tx_notif = event_tx.clone();
    let event_tx_perm = event_tx.clone();
    let permissions_perm = permissions.clone();
    let ui_sid_perm = ui_session_id.clone();
    let prov_perm = provider;

    let processing_outer = processing.clone();
    let connect_result = Client
        .builder()
        .name("cloudcli-server")
        .on_receive_notification(
            {
                let ui_sid = ui_session_id.clone();
                async move |notification: SessionNotification, _cx| {
                    if let Some(json) =
                        ws_outbound::map_session_notification(&notification, &ui_sid, prov)
                    {
                        let _ = event_tx_notif.send(json);
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                async move |request: RequestPermissionRequest, responder, _connection| {
                    if auto_approve {
                        if let Some(id) = request.options.first() {
                            responder.respond(build_permission_response(
                                RequestPermissionOutcome::Selected(
                                    agent_client_protocol::schema::SelectedPermissionOutcome::new(
                                        id.option_id.clone(),
                                    ),
                                ),
                            ));
                        } else {
                            responder.respond(build_permission_response(
                                RequestPermissionOutcome::Cancelled,
                            ));
                        }
                        return Ok(());
                    }

                    let tool_name = request
                        .tool_call
                        .fields
                        .title
                        .clone()
                        .unwrap_or_else(|| "tool".to_string());
                    let input = request
                        .tool_call
                        .fields
                        .raw_input
                        .clone()
                        .unwrap_or(serde_json::json!({}));

                    let (request_id, rx) = permissions_perm
                        .register_wait(request.options.clone())
                        .await;

                    let _ = event_tx_perm.send(permission_request(
                        &ui_sid_perm,
                        prov_perm,
                        &request_id,
                        &tool_name,
                        input,
                    ));

                    tokio::spawn(async move {
                        match rx.await {
                            Ok(outcome) => {
                                responder.respond(build_permission_response(outcome));
                            }
                            Err(_) => {
                                responder.respond(build_permission_response(
                                    RequestPermissionOutcome::Cancelled,
                                ));
                            }
                        }
                    });
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |connection: ConnectionTo<Agent>| {
            let cwd = cwd.clone();
            let ui_sid = ui_sid.clone();
            let event_tx = event_tx.clone();
            let processing = processing.clone();
            let model = model.clone();
            let sandbox_mode = sandbox_mode.clone();

            async move {
                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;

                if let Some(model) = model {
                    std::env::set_var("CLOUDCLI_MODEL", model);
                }
                if let Some(sandbox) = sandbox_mode {
                    std::env::set_var("CLOUDCLI_SANDBOX_MODE", sandbox);
                }

                let abs_cwd = std::path::absolute(&cwd).unwrap_or(cwd);
                let mut session = connection
                    .build_session(&abs_cwd)
                    .block_task()
                    .start_session()
                    .await?;

                loop {
                    let cmd = cmd_rx.recv().await.ok_or_else(|| {
                        AcpError::internal_error().data("command channel closed")
                    })?;

                    match cmd {
                        SessionCommand::Prompt { blocks } => {
                            // Session-scoped updates are routed to `session.read_update()` by
                            // ActiveSessionHandler, not the global `on_receive_notification`
                            // callback. Pump read_update while the prompt RPC is in flight so
                            // stream_delta/thinking reach the WS forwarder during the turn.
                            send_prompt_and_stream_updates(
                                &mut session,
                                blocks,
                                &event_tx,
                                &ui_sid,
                                prov,
                            )
                            .await?;
                            let _ = event_tx.send(complete(&ui_sid, prov));
                        }
                        SessionCommand::Abort => {
                            let _ = connection.send_notification(CancelNotification::new(
                                session.session_id().clone(),
                            ));
                            break;
                        }
                    }
                }

                processing.store(false, Ordering::SeqCst);
                Ok(())
            }
        })
        .await;

    child_guard.kill().await;
    processing_outer.store(false, Ordering::SeqCst);

    connect_result.map_err(|e| format!("ACP connection error: {e}"))
}

async fn send_prompt_and_stream_updates(
    session: &mut agent_client_protocol::ActiveSession<'static, Agent>,
    blocks: Vec<ContentBlock>,
    event_tx: &broadcast::Sender<String>,
    ui_session_id: &str,
    provider: LlmProvider,
) -> AcpResult<()> {
    let session_id = session.session_id().clone();
    let conn = session.connection();
    let prompt = conn
        .send_request_to(Agent, PromptRequest::new(session_id, blocks))
        .block_task();
    tokio::pin!(prompt);

    loop {
        tokio::select! {
            prompt_result = prompt.as_mut() => {
                let _ = prompt_result?;
                break;
            }
            update_result = session.read_update() => {
                let stop = forward_session_update(
                    update_result?,
                    event_tx,
                    ui_session_id,
                    provider,
                )
                .await?;
                if stop {
                    break;
                }
            }
        }
    }

    // Stragglers after prompt RPC returns (StopReason may not be pumped with raw send_request_to).
    drain_session_updates(session, event_tx, ui_session_id, provider).await
}

async fn forward_session_update(
    update: SessionMessage,
    event_tx: &broadcast::Sender<String>,
    ui_session_id: &str,
    provider: LlmProvider,
) -> AcpResult<bool> {
    match update {
        SessionMessage::SessionMessage(dispatch) => {
            MatchDispatch::new(dispatch)
                .if_notification(async move |notif: SessionNotification| {
                    if let Some(json) = ws_outbound::map_session_notification(
                        &notif,
                        ui_session_id,
                        provider,
                    ) {
                        let _ = event_tx.send(json);
                    }
                    Ok(())
                })
                .await
                .otherwise_ignore()?;
            Ok(false)
        }
        SessionMessage::StopReason(_) => Ok(true),
        #[allow(unreachable_patterns)]
        _ => Ok(false),
    }
}

async fn drain_session_updates(
    session: &mut agent_client_protocol::ActiveSession<'static, Agent>,
    event_tx: &broadcast::Sender<String>,
    ui_session_id: &str,
    provider: LlmProvider,
) -> AcpResult<()> {
    use tokio::time::{timeout, Duration};

    loop {
        let read = timeout(Duration::from_millis(500), session.read_update());
        let update = match read.await {
            Err(_) => break,
            Ok(Err(e)) => return Err(e),
            Ok(Ok(update)) => update,
        };

        if forward_session_update(update, event_tx, ui_session_id, provider)
            .await?
        {
            break;
        }
    }
    Ok(())
}

/// Resolve WS `claude-permission-response` messages.
pub async fn handle_ws_permission_response(
    bridge: &AcpBridge,
    parsed: &serde_json::Value,
) -> bool {
    if let Some((request_id, decision)) = parse_permission_response(parsed) {
        return bridge.handle_permission_response(&request_id, decision).await;
    }
    false
}
