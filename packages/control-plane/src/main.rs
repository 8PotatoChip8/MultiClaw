use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod crypto;
mod db;
mod api;
mod agents;

pub mod auth;
pub mod policy;
pub mod provisioning;
pub mod messaging;
pub mod services;
pub mod ledger;
pub mod observability;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,control_plane=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting MultiClaw Control Plane...");

    let cfg = config::Config::from_env()?;
    tracing::info!("Loaded config for {}", cfg.master_key_path);

    let pool = db::init_db(&cfg.database_url).await?;

    let (tx, _rx) = tokio::sync::broadcast::channel(100);
    let app_state = api::ws::AppState { 
        db: pool,
        tx: std::sync::Arc::new(tx),
    };
    let app = api::routes::app_router(app_state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
