use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::config::Config;
use crate::crypto::CryptoMaster;
use crate::agents::main_agent::MainAgent;
use crate::agents::sub_agent::SubAgent;
use crate::openclaw::OpenClawManager;
use crate::provisioning::incus::IncusProvider;

/// Tracks an agent's current work status for the 3D world visualization.
#[derive(Clone, Debug, serde::Serialize)]
pub struct AgentActivityState {
    pub status: String,
    pub task: Option<String>,
    pub since: String,
    /// Number of concurrent requests in flight (when 0, agent is idle).
    pub pending_requests: u32,
}

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub tx: Arc<broadcast::Sender<String>>,
    pub config: Config,
    pub main_agent: Arc<MainAgent>,
    pub sub_agent: Arc<SubAgent>,
    pub openclaw: Arc<OpenClawManager>,
    pub vm_provider: Option<Arc<IncusProvider>>,
    pub crypto: Option<Arc<CryptoMaster>>,
    /// Anti-spam cooldown: tracks when the last DM conversation completed between any agent pair.
    /// Key is (min(id_a, id_b), max(id_a, id_b)) to normalize direction.
    /// Prevents rapid re-initiation (10s cooldown), not for rate limiting (handled by OpenClawManager).
    pub dm_cooldowns: Arc<RwLock<HashMap<(Uuid, Uuid), tokio::time::Instant>>>,
    /// Tracks agent pairs with an active DM conversation in progress.
    /// Prevents concurrent DM initiation between the same pair.
    pub active_dm_pairs: Arc<RwLock<HashSet<(Uuid, Uuid)>>>,
    /// In-memory agent activity tracker for the world view.
    /// Maps agent_id → current activity state. Resets on server restart.
    pub agent_activities: Arc<RwLock<Option<HashMap<Uuid, AgentActivityState>>>>,
}

impl AppState {
    /// Mark an agent as WORKING before calling send_message. Increments pending_requests.
    pub async fn mark_agent_working(&self, agent_id: Uuid, task: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        let mut guard = self.agent_activities.write().await;
        if let Some(ref mut map) = *guard {
            let entry = map.entry(agent_id).or_insert_with(|| AgentActivityState {
                status: "IDLE".to_string(),
                task: None,
                since: now.clone(),
                pending_requests: 0,
            });
            entry.pending_requests += 1;
            entry.status = "WORKING".to_string();
            entry.task = Some(task.to_string());
            entry.since = now;
        }
        drop(guard);
        let _ = self.tx.send(serde_json::json!({
            "type": "agent_activity_changed",
            "agent_id": agent_id,
            "status": "WORKING",
            "task": task,
        }).to_string());
    }

    /// Mark an agent as done with a request. Decrements pending_requests; if 0, sets IDLE.
    pub async fn mark_agent_done(&self, agent_id: Uuid) {
        let mut guard = self.agent_activities.write().await;
        let should_idle = if let Some(ref mut map) = *guard {
            if let Some(entry) = map.get_mut(&agent_id) {
                entry.pending_requests = entry.pending_requests.saturating_sub(1);
                if entry.pending_requests == 0 {
                    entry.status = "IDLE".to_string();
                    entry.task = None;
                    entry.since = chrono::Utc::now().to_rfc3339();
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        drop(guard);
        if should_idle {
            let _ = self.tx.send(serde_json::json!({
                "type": "agent_activity_changed",
                "agent_id": agent_id,
                "status": "IDLE",
            }).to_string());
        }
    }
}

/// Handler for the centralized event stream (used by the Next.js UI)
pub async fn events_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(|socket| handle_ui_socket(socket, state))
}

async fn handle_ui_socket(socket: WebSocket, state: AppState) {
    let (mut sender, _receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // Send initial heartbeat
    let _ = sender.send(Message::Text(
        serde_json::json!({"type": "connected", "message": "MultiClaw event stream connected"}).to_string()
    )).await;

    // Forward all broadcast messages to the WebSocket
    tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });
}
