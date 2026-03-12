use std::sync::Arc;
use tokio::sync::broadcast;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

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
use crypto::CryptoMaster;
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

    // Record restart timestamp so recovery prompts can reference it
    let restart_time = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO system_meta (key, value) VALUES ('last_restart_at', $1) \
         ON CONFLICT (key) DO UPDATE SET value = $1, updated_at = NOW()"
    )
    .bind(&restart_time)
    .execute(&pool)
    .await
    .ok();
    tracing::info!("Recorded last_restart_at={}", restart_time);

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

    let (tx, _rx) = tokio::sync::broadcast::channel(256);
    let tx_arc = std::sync::Arc::new(tx);

    tracing::info!("MainAgent: name={}, model={}", agent_name, agent_model);
    let probe_model = agent_model.clone();
    let main_agent = MainAgent::new(agent_name, agent_model, cfg.ollama_url.clone(), tx_arc.clone());

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

    // Initialize CryptoMaster for secrets management
    let crypto = match CryptoMaster::new(&cfg.master_key_path) {
        Ok(c) => {
            tracing::info!("CryptoMaster initialized (secrets enabled)");
            Some(std::sync::Arc::new(c))
        }
        Err(e) => {
            tracing::warn!("CryptoMaster not available (secrets disabled): {}", e);
            None
        }
    };

    // Initialize OpenClaw manager
    let data_dir = std::path::PathBuf::from(
        std::env::var("MULTICLAW_OPENCLAW_DATA").unwrap_or_else(|_| "/opt/multiclaw/openclaw-data".into())
    );
    // Ollama URL from inside containers (host networking = same as host)
    let ollama_url_for_containers = cfg.ollama_url.clone();
    // MultiClaw API URL from inside containers (host networking = localhost:8080)
    let multiclaw_api_url = format!("http://127.0.0.1:{}", cfg.port);
    tracing::info!("Ollama concurrency: max_concurrent_ollama={}", cfg.max_concurrent_ollama);
    let openclaw_mgr = OpenClawManager::new(data_dir, ollama_url_for_containers, multiclaw_api_url, cfg.max_concurrent_ollama);
    openclaw_mgr.refresh_available_models(&pool).await;

    // Background: ensure all available models are pulled in Ollama
    {
        let pool_pull = pool.clone();
        let openclaw_pull = openclaw_mgr.clone();
        tokio::spawn(async move {
            // Small delay to let Ollama finish starting if it's booting alongside us
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            let raw: Option<String> = sqlx::query_scalar(
                "SELECT value FROM system_meta WHERE key = 'available_models'"
            ).fetch_optional(&pool_pull).await.ok().flatten();

            let models: Vec<String> = raw.as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_else(|| vec!["glm-5:cloud".to_string()]);

            if models.is_empty() {
                tracing::info!("No models configured — skipping startup pull");
                return;
            }

            tracing::info!("Startup model pull: ensuring {} model(s) are available...", models.len());
            openclaw_pull.pull_all_models(models).await;
            tracing::info!("Startup model pull complete");
        });
    }

    let app_state = api::ws::AppState {
        db: pool.clone(),
        tx: tx_arc,
        config: cfg.clone(),
        main_agent: std::sync::Arc::new(main_agent),
        openclaw: std::sync::Arc::new(openclaw_mgr.clone()),
        vm_provider,
        crypto,
        dm_cooldowns: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        active_dm_pairs: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new())),
        agent_activities: std::sync::Arc::new(tokio::sync::RwLock::new(Some(std::collections::HashMap::new()))),
        responding_to_user: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        action_prompt_cooldowns: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        agent_message_locks: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        queue_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
    };

    // Spawn the durable message queue worker
    {
        let worker_state = app_state.clone();
        let worker_notify = app_state.queue_notify.clone();
        tokio::spawn(async move {
            messaging::queue_worker::run(worker_state, worker_notify).await;
        });
    }

    // Watch channel: signals when OpenClaw instance recovery is complete and all containers are ready.
    // The heartbeat, watchdog, and recovery prompt tasks all wait on this instead of fixed timers.
    let (recovery_tx, recovery_rx) = tokio::sync::watch::channel(false);

    // Probe Ollama concurrency limit, recover OpenClaw instances, signal when ready
    let pool_clone = pool.clone();
    let openclaw_clone = openclaw_mgr.clone();
    tokio::spawn(async move {
        // Only probe concurrency when there are active agents to recover.
        // On fresh install, no agents exist yet and the model hasn't been pulled —
        // the install script only runs `ollama pull` after the user completes init.
        // The configured default (MULTICLAW_MAX_CONCURRENT_OLLAMA) is correct for this case.
        let agent_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents WHERE status = 'ACTIVE'")
            .fetch_one(&pool_clone)
            .await
            .unwrap_or(0);

        if agent_count > 0 {
            openclaw_clone.probe_concurrency(&probe_model).await;
        } else {
            tracing::info!("No active agents — skipping concurrency probe (using configured default)");
        }

        tracing::info!("Recovering OpenClaw instances from DB...");
        match openclaw_clone.recover_instances(&pool_clone).await {
            Ok(()) => {
                tracing::info!("OpenClaw instance recovery complete, waiting for containers to be ready...");
                // Poll until all instances are Running or Failed (up to 120s)
                let mut attempts = 0u32;
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    attempts += 1;
                    let instances = openclaw_clone.list_instances().await;
                    let all_ready = instances.iter().all(|i| {
                        i.status == openclaw::InstanceStatus::Running
                            || i.status == openclaw::InstanceStatus::Failed
                    });
                    if instances.is_empty() || all_ready || attempts > 24 {
                        break;
                    }
                }
                tracing::info!("All OpenClaw instances ready (or timed out)");
            }
            Err(e) => tracing::error!("OpenClaw recovery failed: {}", e),
        }
        // Signal all waiting tasks that recovery is done
        let _ = recovery_tx.send(true);
    });

    // Watchdog: periodically reconcile OpenClaw instances (every 60s)
    let pool_wd = pool.clone();
    let openclaw_wd = openclaw_mgr.clone();
    let mut wd_rx = recovery_rx.clone();
    tokio::spawn(async move {
        // Wait for recovery to actually complete instead of guessing with a timer
        let _ = wd_rx.changed().await;
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        tracing::info!("Watchdog reconciliation loop started");
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(e) = openclaw_wd.reconcile_instances(&pool_wd).await {
                tracing::error!("Watchdog reconciliation error: {}", e);
            }
        }
    });

    // Cleanup stale DM cooldowns every 60 seconds
    let dm_cooldowns_clone = app_state.dm_cooldowns.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let mut cooldowns = dm_cooldowns_clone.write().await;
            cooldowns.retain(|_, instant| instant.elapsed() < std::time::Duration::from_secs(10));
        }
    });

    // Post-restart recovery prompts: cascade top-down so agents resume work
    let pool_rp = pool.clone();
    let app_state_rp = app_state.clone();
    let mut rp_rx = recovery_rx.clone();
    tokio::spawn(async move {
        // Wait for all containers to be ready
        let _ = rp_rx.changed().await;
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

        // Check if recovery prompts are enabled (default: true)
        let enabled: bool = sqlx::query_scalar::<_, String>(
            "SELECT value FROM system_meta WHERE key = 'recovery_prompts_enabled'"
        ).fetch_optional(&pool_rp).await.ok().flatten()
         .map(|v| v != "false" && v != "0")
         .unwrap_or(true);

        if !enabled {
            tracing::info!("Recovery prompts disabled via system_meta, skipping");
            return;
        }

        // Fetch the restart timestamp for the prompt
        let restart_ts: String = sqlx::query_scalar::<_, String>(
            "SELECT value FROM system_meta WHERE key = 'last_restart_at'"
        ).fetch_optional(&pool_rp).await.ok().flatten()
         .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        // Fetch all active agents ordered by role tier
        let agents: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT id, name, role FROM agents WHERE status = 'ACTIVE' \
             ORDER BY CASE role \
               WHEN 'MAIN' THEN 0 \
               WHEN 'CEO' THEN 1 \
               WHEN 'MANAGER' THEN 2 \
               WHEN 'WORKER' THEN 3 \
               ELSE 4 END, created_at"
        ).fetch_all(&pool_rp).await.unwrap_or_default();

        if agents.is_empty() {
            tracing::info!("No active agents — skipping recovery prompts");
            return;
        }

        tracing::info!("Enqueuing post-restart recovery prompts for {} agents...", agents.len());

        // Enqueue recovery prompts by tier with delays between tiers
        let tiers = ["MAIN", "CEO", "MANAGER", "WORKER"];
        for tier_name in &tiers {
            let tier_agents: Vec<&(Uuid, String, String)> = agents.iter()
                .filter(|a| a.2 == *tier_name)
                .collect();

            if tier_agents.is_empty() {
                continue;
            }

            tracing::info!(
                "Recovery prompts: enqueuing {} tier ({} agents)",
                tier_name, tier_agents.len()
            );

            for (agent_id, agent_name, role) in &tier_agents {
                let _ = app_state_rp.enqueue_message(
                    *agent_id, 4, "recovery_prompt",
                    serde_json::json!({
                        "agent_id": agent_id.to_string(),
                        "agent_name": agent_name,
                        "role": role,
                        "restart_time": restart_ts,
                    }),
                ).await;
            }

            // Wait between tiers so higher-tier agents process first.
            // The queue worker will pick up these items and process them.
            tracing::info!("Recovery prompts: {} tier enqueued, waiting 60s before next tier", tier_name);
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }

        tracing::info!("Post-restart recovery prompts all enqueued");
    });

    // MainAgent heartbeat: periodic check-in loop — enqueues heartbeat via message queue
    let pool_hb = pool.clone();
    let app_state_hb = app_state.clone();
    let mut hb_rx = recovery_rx.clone();
    tokio::spawn(async move {
        // Wait for recovery + recovery prompts to finish before starting heartbeat.
        let _ = hb_rx.changed().await;
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        tracing::info!("MainAgent heartbeat loop started (post-recovery)");

        loop {
            let interval_secs: u64 = sqlx::query_scalar::<_, String>(
                "SELECT value FROM system_meta WHERE key = 'heartbeat_interval_secs'"
            ).fetch_optional(&pool_hb).await.ok().flatten()
             .and_then(|v| v.parse().ok())
             .unwrap_or(600);

            if interval_secs == 0 {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                continue;
            }

            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;

            let main_agent: Option<(Uuid, String)> = sqlx::query_as(
                "SELECT id, name FROM agents WHERE role = 'MAIN' AND status != 'QUARANTINED' LIMIT 1"
            ).fetch_optional(&pool_hb).await.ok().flatten();

            let (main_id, main_name) = match main_agent {
                Some(a) => a,
                None => {
                    tracing::debug!("Heartbeat skipped: no active MAIN agent found");
                    continue;
                }
            };

            let heartbeat_prompt = "SYSTEM HEARTBEAT: Time for your periodic check-in. \
                Read HEARTBEAT.md and follow the checklist. \
                If everything is running smoothly, respond with just: [HEARTBEAT_OK]";

            let instructions = "This is an automated periodic check-in. Be extremely concise. \
                Follow the HEARTBEAT.md checklist. Do not narrate — just check and report. \
                If nothing needs attention, respond with exactly [HEARTBEAT_OK] and nothing else.";

            match app_state_hb.enqueue_message(
                main_id, 5, "heartbeat",
                serde_json::json!({
                    "agent_id": main_id.to_string(),
                    "prompt": heartbeat_prompt,
                    "instructions": instructions,
                }),
            ).await {
                Ok(queue_id) => tracing::debug!("Heartbeat enqueued for {} (queue_id={})", main_name, queue_id),
                Err(e) => tracing::warn!("Failed to enqueue heartbeat for {}: {}", main_name, e),
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

