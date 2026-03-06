use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod crypto;
mod db;
mod api;
pub mod agents;
pub mod openclaw;

pub mod auth;
pub mod policy;
pub mod provisioning;
pub mod messaging;
pub mod services;
pub mod ledger;
pub mod observability;

use agents::main_agent::MainAgent;
use agents::sub_agent::SubAgent;
use openclaw::OpenClawManager;
use provisioning::incus::IncusProvider;

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

    // Seed deployed_commit from current git HEAD so the auto-updater can compare.
    // The /opt/multiclaw repo is volume-mounted into the container.
    let head_sha = tokio::process::Command::new("git")
        .args(["-C", "/opt/multiclaw", "rev-parse", "HEAD"])
        .output()
        .await
        .ok()
        .and_then(|o| if o.status.success() {
            String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
        } else { None });

    if let Some(sha) = &head_sha {
        sqlx::query("INSERT INTO system_meta (key, value) VALUES ('deployed_commit', $1) ON CONFLICT (key) DO UPDATE SET value = $1, updated_at = NOW()")
            .bind(sha)
            .execute(&pool)
            .await
            .ok();
        tracing::info!("Recorded deployed_commit={}", &sha[..7.min(sha.len())]);
    } else {
        tracing::warn!("Could not resolve git HEAD at /opt/multiclaw, deployed_commit not seeded");
    }

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

    // Initialize VM provider (Incus) — gracefully degrade if unavailable
    let vm_provider = match IncusProvider::new().await {
        Ok(p) => {
            tracing::info!("Incus VM provider initialized");
            Some(std::sync::Arc::new(p))
        },
        Err(e) => {
            tracing::warn!("Incus not available (VMs disabled): {}", e);
            None
        }
    };

    let sub_agent = SubAgent::new(cfg.ollama_url.clone());

    // Initialize OpenClaw manager
    let data_dir = std::path::PathBuf::from(
        std::env::var("MULTICLAW_OPENCLAW_DATA").unwrap_or_else(|_| "/opt/multiclaw/openclaw-data".into())
    );
    // Ollama URL from inside containers (host networking = same as host)
    let ollama_url_for_containers = cfg.ollama_url.clone();
    // MultiClaw API URL from inside containers (host networking = localhost:8080)
    let multiclaw_api_url = format!("http://127.0.0.1:{}", cfg.port);
    let openclaw_mgr = OpenClawManager::new(data_dir, ollama_url_for_containers, multiclaw_api_url);

    let (tx, _rx) = tokio::sync::broadcast::channel(256);
    let app_state = api::ws::AppState { 
        db: pool.clone(),
        tx: std::sync::Arc::new(tx),
        config: cfg.clone(),
        main_agent: std::sync::Arc::new(main_agent),
        sub_agent: std::sync::Arc::new(sub_agent),
        openclaw: std::sync::Arc::new(openclaw_mgr.clone()),
        vm_provider,
    };

    // Recover OpenClaw instances from DB in background
    let pool_clone = pool.clone();
    let openclaw_clone = openclaw_mgr.clone();
    tokio::spawn(async move {
        tracing::info!("Recovering OpenClaw instances from DB...");
        match openclaw_clone.recover_instances(&pool_clone).await {
            Ok(()) => tracing::info!("OpenClaw instance recovery complete"),
            Err(e) => tracing::error!("OpenClaw recovery failed: {}", e),
        }
    });

    // Watchdog: periodically reconcile OpenClaw instances (every 60s)
    let pool_wd = pool.clone();
    let openclaw_wd = openclaw_mgr.clone();
    tokio::spawn(async move {
        // Wait for initial recovery to finish before starting watchdog
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        tracing::info!("Watchdog reconciliation loop started");
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(e) = openclaw_wd.reconcile_instances(&pool_wd).await {
                tracing::error!("Watchdog reconciliation error: {}", e);
            }
        }
    });

    let app = api::routes::app_router(app_state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], cfg.port));
    tracing::info!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
