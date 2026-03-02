use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod crypto;
mod db;
mod api;
mod agents;

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

    let _pool = db::init_db(&cfg.database_url).await?;
    
    // NOTE: This runs the actual API logic...
    Ok(())
}
