mod auth;
mod ratelimit;
mod forward;
mod models_allowlist;
mod access_log;

use axum::{
    body::Body,
    extract::State,
    http::Request,
    response::Response,
    routing::any,
    Router,
};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
pub struct AppState {
    pub client: reqwest::Client,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,ollama_proxy=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Host Ollama Proxy...");

    let state = AppState {
        client: reqwest::Client::new(),
    };

    let app = Router::new()
        .route("/*path", any(forward::proxy_handler))
        .layer(axum::middleware::from_fn(auth::auth_middleware))
        .with_state(state);

    // Bind to 0.0.0.0:11436. In production this should be locked down to the Incus bridge subnet
    // via iptables/firewall rules or specific interface binding.
    let listener = tokio::net::TcpListener::bind("0.0.0.0:11436").await?;
    tracing::info!("Host Ollama proxy listening on 0.0.0.0:11436");
    axum::serve(listener, app).await?;

    Ok(())
}
