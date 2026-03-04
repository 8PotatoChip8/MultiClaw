use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod crypto;
mod db;
mod api;
pub mod agents;

pub mod auth;
pub mod policy;
pub mod provisioning;
pub mod messaging;
pub mod services;
pub mod ledger;
pub mod observability;

use agents::main_agent::MainAgent;

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
    tracing::info!("Loaded config, port={}, ollama_url={}", cfg.port, cfg.ollama_url);

    let pool = db::init_db(&cfg.database_url).await?;

    // Try to load MainAgent config from DB (name + model)
    let (agent_name, agent_model) = {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT name, effective_model FROM agents WHERE role = 'MAIN' LIMIT 1"
        )
        .fetch_optional(&pool)
        .await
        .unwrap_or(None);

        match row {
            Some((name, model)) => (name, model),
            None => ("MainAgent".to_string(), "glm-5:cloud".to_string()),
        }
    };

    tracing::info!("MainAgent: name={}, model={}", agent_name, agent_model);
    let main_agent = MainAgent::new(agent_name, agent_model, cfg.ollama_url.clone());

    let (tx, _rx) = tokio::sync::broadcast::channel(256);
    let app_state = api::ws::AppState { 
        db: pool,
        tx: std::sync::Arc::new(tx),
        config: cfg.clone(),
        main_agent: std::sync::Arc::new(main_agent),
    };
    let app = api::routes::app_router(app_state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], cfg.port));
    tracing::info!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
