use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub tx: Arc<broadcast::Sender<String>>,
    pub config: Config,
}

/// Handler for the centralized event stream (used by the Next.js UI)
pub async fn events_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(|socket| handle_ui_socket(socket, state))
}

async fn handle_ui_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut _receiver) = socket.split();
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
