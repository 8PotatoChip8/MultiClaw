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
use agents::sub_agent::SubAgent;
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

    let sub_agent = SubAgent::new(cfg.ollama_url.clone());

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
    let openclaw_mgr = OpenClawManager::new(data_dir, ollama_url_for_containers, multiclaw_api_url);

    let app_state = api::ws::AppState {
        db: pool.clone(),
        tx: tx_arc,
        config: cfg.clone(),
        main_agent: std::sync::Arc::new(main_agent),
        sub_agent: std::sync::Arc::new(sub_agent),
        openclaw: std::sync::Arc::new(openclaw_mgr.clone()),
        vm_provider,
        crypto,
        dm_cooldowns: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        active_dm_pairs: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new())),
        agent_activities: std::sync::Arc::new(tokio::sync::RwLock::new(Some(std::collections::HashMap::new()))),
        responding_to_user: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        action_prompt_cooldowns: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
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

    // Cleanup stale DM cooldowns every 60 seconds
    let dm_cooldowns_clone = app_state.dm_cooldowns.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let mut cooldowns = dm_cooldowns_clone.write().await;
            cooldowns.retain(|_, instant| instant.elapsed() < std::time::Duration::from_secs(10));
        }
    });

    // MainAgent heartbeat: periodic check-in loop
    let pool_hb = pool.clone();
    let openclaw_hb = openclaw_mgr.clone();
    let tx_hb = app_state.tx.clone();
    tokio::spawn(async move {
        // Wait for OpenClaw instances to recover before starting heartbeat
        tokio::time::sleep(std::time::Duration::from_secs(180)).await;
        tracing::info!("MainAgent heartbeat loop started");

        loop {
            // Read interval from system_meta (default 600s = 10 minutes)
            let interval_secs: u64 = sqlx::query_scalar::<_, String>(
                "SELECT value FROM system_meta WHERE key = 'heartbeat_interval_secs'"
            ).fetch_optional(&pool_hb).await.ok().flatten()
             .and_then(|v| v.parse().ok())
             .unwrap_or(600);

            // 0 = disabled
            if interval_secs == 0 {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                continue;
            }

            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;

            // Find the MAIN agent
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

            // Send heartbeat prompt via OpenClaw
            let heartbeat_prompt = "SYSTEM HEARTBEAT: Time for your periodic check-in. \
                Review the current state of things — check for any pending approvals, \
                see if your CEOs need anything, and note any issues. \
                If everything is running smoothly and there is nothing to report, \
                respond with just: [HEARTBEAT_OK] \
                If there IS something to report or act on, handle it and then \
                briefly summarize what you did.";

            let instructions = "This is an automated periodic check-in. Be extremely concise. \
                Only take action or report if something actually needs attention. \
                If nothing needs attention, respond with exactly [HEARTBEAT_OK] and nothing else. \
                Do not generate filler or repeat known information.";

            match openclaw_hb.send_message(main_id, heartbeat_prompt, Some(instructions)).await {
                Ok(response) => {
                    let trimmed = response.trim();
                    // Content-aware heartbeat detection: strip the heartbeat tag and
                    // narration filler, then check if any substantive content remains.
                    // This handles cases like "Let me check...\n[HEARTBEAT_OK]" where
                    // the model narrates before the tag, pushing length past any fixed guard.
                    let (cleaned, _) = api::routes::strip_agent_tags(trimmed);
                    let has_heartbeat_tag = {
                        let normalized: String = trimmed.chars()
                            .filter(|c| !c.is_whitespace() && *c != '[' && *c != ']')
                            .collect();
                        normalized.contains("HEARTBEAT_OK") || normalized.contains("HEARTBEATOK")
                    };
                    if has_heartbeat_tag {
                        // Response contained HEARTBEAT_OK — all clear, discard any
                        // surrounding narration (e.g. "I'll check... [HEARTBEAT_OK]")
                        tracing::debug!("Heartbeat: {} reports all clear", main_name);
                    } else if cleaned.is_empty() {
                        tracing::debug!("Heartbeat: {} response was empty after cleaning", main_name);
                    } else {
                        tracing::info!("Heartbeat: {} has a report ({}B)", main_name, cleaned.len());
                        // Store the cleaned response in the MainAgent's human-operator DM thread
                        let thread_id: Option<Uuid> = sqlx::query_scalar(
                            "SELECT tm.thread_id FROM thread_members tm \
                             JOIN threads t ON t.id = tm.thread_id \
                             JOIN thread_members tm2 ON t.id = tm2.thread_id \
                             WHERE tm.member_type = 'AGENT' AND tm.member_id = $1 \
                               AND tm2.member_type = 'USER' AND t.type = 'DM' LIMIT 1"
                        ).bind(main_id).fetch_optional(&pool_hb).await.ok().flatten();

                        if let Some(tid) = thread_id {
                            let msg_id = Uuid::new_v4();
                            let content = serde_json::json!({"text": cleaned});
                            let _ = sqlx::query(
                                "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) \
                                 VALUES ($1,$2,'AGENT',$3,$4,0)"
                            ).bind(msg_id).bind(tid).bind(main_id).bind(&content)
                            .execute(&pool_hb).await;
                            let _ = tx_hb.send(serde_json::json!({
                                "type":"new_message",
                                "message": {"id": msg_id, "thread_id": tid, "sender_type": "AGENT", "sender_id": main_id, "content": content}
                            }).to_string());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Heartbeat failed for {}: {}", main_name, e);
                }
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
