use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::config::Config;
use crate::crypto::CryptoMaster;
use crate::agents::main_agent::MainAgent;
use crate::agents::sub_agent::SubAgent;
use crate::openclaw::OpenClawManager;
use crate::provisioning::incus::IncusProvider;

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
    /// Tracks when the last DM conversation completed between any agent pair.
    /// Key is (min(id_a, id_b), max(id_a, id_b)) to normalize direction.
    pub dm_cooldowns: Arc<RwLock<HashMap<(Uuid, Uuid), tokio::time::Instant>>>,
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
