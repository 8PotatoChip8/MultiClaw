use anyhow::Result;
use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

pub async fn start_server() -> Result<()> {
    let app = Router::new().route("/health", get(health_handler));
    
    // Bind to loopback port reserved for sidecar control 
    let listener = tokio::net::TcpListener::bind("127.0.0.1:18790").await?;
    tracing::info!("Agentd local health server listening on 127.0.0.1:18790");
    axum::serve(listener, app).await?;
    
    Ok(())
}

async fn health_handler() -> Json<Value> {
    Json(json!({"status": "ok", "service": "multiclaw-agentd"}))
}
