mod config;
mod openclaw_client;
mod ollama_bridge;
mod control_ws;
mod health;
mod logging;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init();

    let cfg = config::Config::from_env()?;
    tracing::info!("Starting Agent Sidecar for agent {}", cfg.agent_id);

    // Start Ollama bridge
    let bridge_cfg = cfg.clone();
    tokio::spawn(async move {
        if let Err(e) = ollama_bridge::start(bridge_cfg).await {
            tracing::error!("Ollama bridge error: {e}");
        }
    });

    // Start Control WS loop
    let ws_cfg = cfg.clone();
    tokio::spawn(async move {
        if let Err(e) = control_ws::start_loop(ws_cfg).await {
            tracing::error!("Control WS loop ended: {e}");
        }
    });

    // Start local health server
    health::start_server().await?;

    Ok(())
}
