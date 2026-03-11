use sqlx::PgPool;
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

/// Acquire a per-agent turn lock (standalone version for tasks without AppState).
async fn acquire_agent_turn(
    locks: &tokio::sync::RwLock<std::collections::HashMap<Uuid, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    agent_id: Uuid,
) -> tokio::sync::OwnedMutexGuard<()> {
    let mutex = {
        let map = locks.read().await;
        if let Some(m) = map.get(&agent_id) {
            m.clone()
        } else {
            drop(map);
            let mut map = locks.write().await;
            map.entry(agent_id)
                .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        }
    };
    mutex.lock_owned().await
}

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
    };

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
    let openclaw_rp = openclaw_mgr.clone();
    let tx_rp = app_state.tx.clone();
    let agent_locks_rp = app_state.agent_message_locks.clone();
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

        tracing::info!("Starting post-restart recovery prompts for {} agents...", agents.len());

        // Group by tier and send cascade
        let tiers = ["MAIN", "CEO", "MANAGER", "WORKER"];
        for tier_name in &tiers {
            let tier_agents: Vec<&(Uuid, String, String)> = agents.iter()
                .filter(|a| a.2 == *tier_name)
                .collect();

            if tier_agents.is_empty() {
                continue;
            }

            tracing::info!(
                "Recovery prompts: sending to {} tier ({} agents)",
                tier_name, tier_agents.len()
            );

            // Send to all agents in this tier concurrently
            let mut handles = Vec::new();
            for (agent_id, agent_name, role) in &tier_agents {
                let pool = pool_rp.clone();
                let openclaw = openclaw_rp.clone();
                let tx = tx_rp.clone();
                let locks = agent_locks_rp.clone();
                let agent_id = *agent_id;
                let agent_name = (*agent_name).clone();
                let role = (*role).clone();
                let restart_ts = restart_ts.clone();

                handles.push(tokio::spawn(async move {
                    send_recovery_prompt(
                        &pool, &openclaw, &tx, &locks,
                        agent_id, &agent_name, &role, &restart_ts,
                    ).await;
                }));
            }

            // Wait for all agents in this tier to respond before moving to next
            for handle in handles {
                let _ = handle.await;
            }

            // Delay between tiers so agent actions can settle
            tracing::info!("Recovery prompts: {} tier complete, waiting 30s before next tier", tier_name);
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }

        tracing::info!("Post-restart recovery prompts complete");
    });

    // MainAgent heartbeat: periodic check-in loop
    let pool_hb = pool.clone();
    let openclaw_hb = openclaw_mgr.clone();
    let tx_hb = app_state.tx.clone();
    let agent_locks_hb = app_state.agent_message_locks.clone();
    let mut hb_rx = recovery_rx.clone();
    tokio::spawn(async move {
        // Wait for recovery + recovery prompts to finish before starting heartbeat.
        // The 300s settling delay gives recovery prompts (~5-7 min) time to complete.
        let _ = hb_rx.changed().await;
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        tracing::info!("MainAgent heartbeat loop started (post-recovery)");

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

            let _agent_guard = acquire_agent_turn(&agent_locks_hb, main_id).await;
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

/// Send a recovery prompt to a single agent after system restart.
/// Crafts a role-appropriate prompt, sends via OpenClaw, and handles the response.
/// Only MAIN agent substantive responses are posted to the user DM thread.
async fn send_recovery_prompt(
    db: &PgPool,
    openclaw: &OpenClawManager,
    tx: &Arc<broadcast::Sender<String>>,
    agent_locks: &tokio::sync::RwLock<std::collections::HashMap<Uuid, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    agent_id: Uuid,
    agent_name: &str,
    role: &str,
    restart_time: &str,
) {
    let prompt = match role {
        "MAIN" => format!(
            "SYSTEM RESTART NOTICE: The system was restarted at {}. \
             All agent containers have been recovered. Your in-memory context from before the restart is gone. \
             Review your current situation: \
             1. Check your org tree (companies and their CEOs) \
             2. Check your recent threads and messages for any in-progress work \
             3. Check your workspace memory for any saved state \
             4. Resume any interrupted work or verify everything is on track \
             If everything looks good and nothing needs immediate attention, respond with just: [RECOVERY_OK] \
             If there are issues or interrupted work to resume, briefly describe what you're doing about it.",
            restart_time
        ),
        "CEO" => format!(
            "SYSTEM RESTART NOTICE: The system was restarted at {}. \
             Your container has been recovered but your in-memory context is gone. \
             Review your situation: \
             1. Check your team (managers and workers under you) \
             2. Check your recent threads and DMs for any in-progress work \
             3. Check your workspace memory for saved state \
             4. Resume any interrupted work — do NOT re-hire people you already hired or re-brief people already briefed \
             If everything is on track, respond with: [RECOVERY_OK] \
             If you need to resume something, briefly act on it.",
            restart_time
        ),
        "MANAGER" => format!(
            "SYSTEM RESTART NOTICE: The system was restarted at {}. \
             Your container has been recovered but your in-memory context is gone. \
             Review your situation: \
             1. Check your team (workers under you) \
             2. Check your recent threads for any in-progress work \
             3. Check your workspace memory for saved state \
             4. Resume any interrupted work — do NOT re-hire or re-brief workers already in place \
             If everything is on track, respond with: [RECOVERY_OK] \
             If you need to resume something, briefly act on it.",
            restart_time
        ),
        _ => format!(
            "SYSTEM RESTART NOTICE: The system was restarted at {}. \
             Your container has been recovered but your in-memory context is gone. \
             Review your situation: \
             1. Check your workspace for any in-progress files or work \
             2. Check your recent threads for context on what you were working on \
             3. Check your workspace memory for saved state \
             4. Resume any interrupted work from where you left off \
             If everything is on track, respond with: [RECOVERY_OK] \
             If you have interrupted work to resume, briefly describe what you're picking back up.",
            restart_time
        ),
    };

    let instructions = "This is a system-generated restart notification. \
        Be concise. Review your state using available tools, then either resume \
        interrupted work or confirm everything is on track with [RECOVERY_OK]. \
        Do NOT narrate what you are about to do — just do it and respond with the result. \
        Do NOT repeat or summarize information you already know.";

    tracing::info!("Sending recovery prompt to {} ({})", agent_name, role);

    let _agent_guard = acquire_agent_turn(agent_locks, agent_id).await;
    match openclaw.send_message(agent_id, &prompt, Some(instructions)).await {
        Ok(response) => {
            let (cleaned, _) = api::routes::strip_agent_tags(&response);
            let has_recovery_ok = {
                let normalized: String = response.chars()
                    .filter(|c| !c.is_whitespace() && *c != '[' && *c != ']')
                    .collect();
                normalized.contains("RECOVERY_OK") || normalized.contains("RECOVERYOK")
            };

            if has_recovery_ok {
                tracing::info!("Recovery: {} ({}) reports all clear", agent_name, role);
            } else if cleaned.trim().is_empty() {
                tracing::warn!("Recovery: {} ({}) returned empty response", agent_name, role);
            } else {
                tracing::info!(
                    "Recovery: {} ({}) is resuming work ({} chars)",
                    agent_name, role, cleaned.len()
                );

                // For MAIN agent only: post substantive recovery reports to user DM thread
                // (mirrors heartbeat behavior so the operator sees what MAIN is doing)
                if role == "MAIN" {
                    let thread_id: Option<Uuid> = sqlx::query_scalar(
                        "SELECT tm.thread_id FROM thread_members tm \
                         JOIN threads t ON t.id = tm.thread_id \
                         JOIN thread_members tm2 ON t.id = tm2.thread_id \
                         WHERE tm.member_type = 'AGENT' AND tm.member_id = $1 \
                           AND tm2.member_type = 'USER' AND t.type = 'DM' LIMIT 1"
                    ).bind(agent_id).fetch_optional(db).await.ok().flatten();

                    if let Some(tid) = thread_id {
                        let msg_id = Uuid::new_v4();
                        let prefixed = format!("[Post-Restart Recovery] {}", cleaned);
                        let content = serde_json::json!({"text": prefixed});
                        let _ = sqlx::query(
                            "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) \
                             VALUES ($1,$2,'AGENT',$3,$4,0)"
                        ).bind(msg_id).bind(tid).bind(agent_id).bind(&content)
                        .execute(db).await;
                        let _ = tx.send(serde_json::json!({
                            "type": "new_message",
                            "message": {
                                "id": msg_id, "thread_id": tid,
                                "sender_type": "AGENT", "sender_id": agent_id,
                                "content": content
                            }
                        }).to_string());
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!("Recovery prompt failed for {} ({}): {}", agent_name, role, e);
        }
    }
}
