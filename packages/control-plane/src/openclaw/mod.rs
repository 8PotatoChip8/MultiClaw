mod rate_limiter;

use anyhow::{anyhow, Result};
use rate_limiter::AdaptiveRateLimiter;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// "ws" feature is done via raw HTTP for simplicity — we use the CLI for message sending.

/// Manages OpenClaw Docker containers — one per agent.
#[derive(Clone)]
pub struct OpenClawManager {
    /// Map of agent_id -> OpenClawInstance
    instances: Arc<RwLock<HashMap<Uuid, OpenClawInstance>>>,
    /// Base directory for agent data (config + workspace)
    data_dir: PathBuf,
    /// Ollama URL accessible from inside Docker containers
    ollama_url: String,
    /// MultiClaw API URL accessible from inside Docker containers
    multiclaw_api_url: String,
    /// Docker image to use
    image: String,
    /// Base port for OpenClaw gateways (each agent gets base_port + offset)
    base_port: u16,
    /// Adaptive rate limiter for upstream LLM API calls (handles 429s)
    rate_limiter: AdaptiveRateLimiter,
    /// Atomic counter for port allocation (avoids race conditions on concurrent spawns)
    next_port_offset: Arc<AtomicU16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawInstance {
    pub agent_id: Uuid,
    pub agent_name: String,
    pub container_name: String,
    pub port: u16,
    pub gateway_token: String,
    pub model: String,
    pub status: InstanceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InstanceStatus {
    Starting,
    Running,
    Stopped,
    Failed,
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub agent_id: Uuid,
    pub agent_name: String,
    pub role: String,
    pub company_name: String,
    pub holding_name: String,
    pub specialty: Option<String>,
    pub model: String,
    pub system_prompt: Option<String>,
}

impl OpenClawManager {
    pub fn new(
        data_dir: PathBuf,
        ollama_url: String,
        multiclaw_api_url: String,
    ) -> Self {
        Self {
            instances: Arc::new(RwLock::new(HashMap::new())),
            data_dir,
            ollama_url,
            multiclaw_api_url,
            image: "ghcr.io/openclaw/openclaw:latest".to_string(),
            base_port: 18790,
            rate_limiter: AdaptiveRateLimiter::new(),
            next_port_offset: Arc::new(AtomicU16::new(0)),
        }
    }

    /// Spawn an OpenClaw instance for an agent.
    pub async fn spawn_instance(&self, config: &AgentConfig) -> Result<OpenClawInstance> {
        let agent_id = config.agent_id;

        // Check if already running
        {
            let instances = self.instances.read().await;
            if let Some(inst) = instances.get(&agent_id) {
                if inst.status == InstanceStatus::Running || inst.status == InstanceStatus::Starting {
                    tracing::info!("OpenClaw instance already running/starting for {}", config.agent_name);
                    return Ok(inst.clone());
                }
            }
        }

        // Assign a port — reserve 4 ports per agent.
        // OpenClaw binds: gateway (+0), internal (+1), control service (+2), CDP relay (+3).
        // With --network host, these must not overlap between agents.
        // Use atomic counter to avoid race conditions when spawning concurrently.
        let offset = self.next_port_offset.fetch_add(4, Ordering::SeqCst);
        let port = self.base_port + offset;

        // Generate gateway token
        let gateway_token = format!("{:032x}", rand::random::<u128>());

        // Create agent data directory
        let agent_dir = self.data_dir.join(agent_id.to_string());
        let config_dir = agent_dir.join("config");
        let workspace_dir = agent_dir.join("workspace");
        let skills_dir = workspace_dir.join("skills").join("multiclaw");

        tokio::fs::create_dir_all(&config_dir).await?;
        tokio::fs::create_dir_all(&skills_dir).await?;

        // Render and write openclaw.json config
        let openclaw_config = self.render_config(config, port, &gateway_token)?;
        tokio::fs::write(config_dir.join("openclaw.json"), &openclaw_config).await?;

        // Render and write workspace files
        self.render_workspace(config, &workspace_dir).await?;

        // Fix ownership: OpenClaw runs as 'node' (UID 1000) inside the container.
        // The directories we just created are owned by root, so chown them.
        let _ = tokio::process::Command::new("chown")
            .args(["-R", "1000:1000", &agent_dir.display().to_string()])
            .output()
            .await;

        // Container name
        let container_name = format!("multiclaw-openclaw-{}", config.agent_name
            .to_lowercase()
            .replace(' ', "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>());

        // Stop existing container with the same name if any
        let _ = tokio::process::Command::new("docker")
            .args(["rm", "-f", &container_name])
            .output()
            .await;

        // Also kill any stale container that may be holding our port.
        // With --network host, port conflicts from leftover containers are fatal.
        if let Ok(output) = tokio::process::Command::new("docker")
            .args(["ps", "-q", "--filter", "name=multiclaw-openclaw-"])
            .output()
            .await
        {
            let running = String::from_utf8_lossy(&output.stdout);
            if !running.trim().is_empty() {
                // Get names of running openclaw containers
                if let Ok(names_output) = tokio::process::Command::new("docker")
                    .args(["ps", "--filter", "name=multiclaw-openclaw-", "--format", "{{.Names}}"])
                    .output()
                    .await
                {
                    let names = String::from_utf8_lossy(&names_output.stdout);
                    for name in names.lines() {
                        let name = name.trim();
                        if !name.is_empty() && name != container_name {
                            // Check if this container's tracked instance uses our port
                            let dominated = {
                                let instances = self.instances.read().await;
                                !instances.values().any(|i| i.container_name == name)
                            };
                            if dominated {
                                tracing::warn!("Removing untracked container {} (may conflict with port {})", name, port);
                                let _ = tokio::process::Command::new("docker")
                                    .args(["rm", "-f", name])
                                    .output()
                                    .await;
                            }
                        }
                    }
                }
            }
        }

        // Launch Docker container
        tracing::info!(
            "Spawning OpenClaw instance for {} on port {} (container: {})",
            config.agent_name, port, container_name
        );

        let output = tokio::process::Command::new("docker")
            .args([
                "run", "-d",
                "--name", &container_name,
                "--network", "host",
                "-e", &format!("OPENCLAW_GATEWAY_TOKEN={}", gateway_token),
                "-v", &format!("{}:/home/node/.openclaw:rw", config_dir.display()),
                "-v", &format!("{}:/workspace:rw", workspace_dir.display()),
                "--restart", "unless-stopped",
                &self.image,
                "openclaw", "gateway",
                "--port", &port.to_string(),
                "--verbose",
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!("Failed to spawn OpenClaw container: {}", stderr);
            return Err(anyhow!("Docker run failed: {}", stderr));
        }

        let instance = OpenClawInstance {
            agent_id,
            agent_name: config.agent_name.clone(),
            container_name: container_name.clone(),
            port,
            gateway_token: gateway_token.clone(),
            model: config.model.clone(),
            status: InstanceStatus::Starting,
        };

        // Store instance
        {
            let mut instances = self.instances.write().await;
            instances.insert(agent_id, instance.clone());
        }

        tracing::info!(
            "OpenClaw instance '{}' started for agent {} on port {}",
            container_name, config.agent_name, port
        );

        // Wait for the gateway to be ready in the background
        let self_clone = self.clone();
        let agent_name_clone = config.agent_name.clone();
        tokio::spawn(async move {
            if self_clone.wait_for_ready(agent_id, 90).await {
                tracing::info!("OpenClaw gateway ready for {}", agent_name_clone);
                let mut instances = self_clone.instances.write().await;
                if let Some(inst) = instances.get_mut(&agent_id) {
                    inst.status = InstanceStatus::Running;
                }
            } else {
                tracing::error!("OpenClaw gateway failed to start for {}", agent_name_clone);
                let mut instances = self_clone.instances.write().await;
                if let Some(inst) = instances.get_mut(&agent_id) {
                    inst.status = InstanceStatus::Failed;
                }
            }
        });

        Ok(instance)
    }

    /// Send a message to an agent's OpenClaw instance and get the response.
    /// Uses the HTTP /v1/responses endpoint.
    pub async fn send_message(&self, agent_id: Uuid, message: &str, instructions: Option<&str>) -> Result<String> {
        // Wait for instance to be ready if it's still starting
        let instance = {
            let mut retries = 0;
            loop {
                let instances = self.instances.read().await;
                let inst = instances
                    .get(&agent_id)
                    .cloned()
                    .ok_or_else(|| anyhow!("No OpenClaw instance for agent {}", agent_id))?;

                match inst.status {
                    InstanceStatus::Running => break inst,
                    InstanceStatus::Starting if retries < 30 => {
                        drop(instances);
                        retries += 1;
                        tracing::info!("Waiting for OpenClaw instance for {} to be ready (attempt {}/30)", inst.agent_name, retries);
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    }
                    _ => {
                        return Err(anyhow!(
                            "OpenClaw instance for {} is not running (status: {:?})",
                            inst.agent_name,
                            inst.status
                        ));
                    }
                }
            }
        };

        tracing::info!(
            "Sending message to {} via OpenClaw (container: {})",
            instance.agent_name,
            instance.container_name
        );

        // Use HTTP POST to OpenClaw's /v1/responses endpoint (with retry for transient errors)
        let url = format!("http://127.0.0.1:{}/v1/responses", instance.port);
        let client = reqwest::Client::new();
        let mut body = serde_json::json!({
            "model": format!("ollama/{}", instance.model),
            "input": message,
        });
        if let Some(inst) = instructions {
            body["instructions"] = serde_json::Value::String(inst.to_string());
        }

        let max_retries = 5u32;
        let max_429_retries = 6u32;
        let mut attempts_429 = 0u32;
        let mut attempt = 0u32;

        loop {
            // Wait for rate limiter permit before each request
            self.rate_limiter.wait_for_permit().await;

            let resp = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", instance.gateway_token))
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(600))
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    self.rate_limiter.record_success().await;

                    let resp_body: serde_json::Value = r.json().await
                        .map_err(|e| anyhow!("Failed to parse OpenClaw response: {}", e))?;

                    // Extract text from the OpenResponses format
                    // Response has an "output" array with items; find "message" type with "text" content
                    let text = resp_body["output"]
                        .as_array()
                        .and_then(|outputs| {
                            outputs.iter().find_map(|item| {
                                if item["type"] == "message" {
                                    item["content"]
                                        .as_array()
                                        .and_then(|content| {
                                            content.iter().find_map(|c| {
                                                if c["type"] == "output_text" {
                                                    c["text"].as_str().map(|s| s.to_string())
                                                } else {
                                                    None
                                                }
                                            })
                                        })
                                } else {
                                    None
                                }
                            })
                        })
                        .unwrap_or_else(|| {
                            // Fallback: try to get any text from the response
                            resp_body["output_text"]
                                .as_str()
                                .unwrap_or("[Agent produced no text output]")
                                .to_string()
                        });

                    tracing::info!(
                        "{} responded ({} chars)",
                        instance.agent_name,
                        text.len()
                    );

                    return Ok(text);
                }
                // Handle 429 Too Many Requests with exponential backoff
                Ok(r) if r.status().as_u16() == 429 => {
                    attempts_429 += 1;
                    self.rate_limiter.record_rate_limited().await;

                    if attempts_429 >= max_429_retries {
                        return Err(anyhow!(
                            "Rate limited by upstream after {} retries for {}",
                            attempts_429, instance.agent_name
                        ));
                    }

                    // Respect Retry-After header if present, otherwise exponential backoff
                    let retry_after = r.headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok());

                    let wait = if let Some(secs) = retry_after {
                        std::time::Duration::from_secs(secs)
                    } else {
                        let base_ms = 2u64.pow(attempts_429) * 1000;
                        let jitter_ms = rand::random::<u64>() % 1000;
                        std::time::Duration::from_millis(base_ms + jitter_ms)
                    };

                    tracing::warn!(
                        "429 from upstream for {} (attempt {}/{}), backing off {:?}",
                        instance.agent_name, attempts_429, max_429_retries, wait
                    );
                    tokio::time::sleep(wait).await;
                    continue; // Does NOT consume a regular retry attempt
                }
                Ok(r) if matches!(r.status().as_u16(), 404 | 502 | 503) && attempt < max_retries - 1 => {
                    tracing::warn!(
                        "Transient {} from {}, retrying in 3s ({}/{})",
                        r.status(), instance.agent_name, attempt + 1, max_retries
                    );
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    continue;
                }
                Ok(r) => {
                    let status = r.status();
                    let err_body = r.text().await.unwrap_or_default();
                    tracing::error!(
                        "OpenClaw HTTP error for {}: {} - {}",
                        instance.agent_name, status, err_body
                    );
                    return Err(anyhow!("OpenClaw HTTP {}: {}", status, err_body));
                }
                Err(e) if attempt < max_retries - 1 => {
                    tracing::warn!(
                        "OpenClaw connection error for {}, retrying in 3s ({}/{}): {}",
                        instance.agent_name, attempt + 1, max_retries, e
                    );
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    continue;
                }
                Err(e) => {
                    tracing::error!("OpenClaw request failed for {}: {}", instance.agent_name, e);
                    return Err(anyhow!("OpenClaw connection error: {}", e));
                }
            }
        }
    }

    /// Stop an agent's OpenClaw instance.
    pub async fn stop_instance(&self, agent_id: Uuid) -> Result<()> {
        let instance = {
            let instances = self.instances.read().await;
            instances
                .get(&agent_id)
                .cloned()
                .ok_or_else(|| anyhow!("No OpenClaw instance for agent {}", agent_id))?
        };

        tracing::info!("Stopping OpenClaw instance: {}", instance.container_name);

        let _ = tokio::process::Command::new("docker")
            .args(["stop", &instance.container_name])
            .output()
            .await;

        {
            let mut instances = self.instances.write().await;
            if let Some(inst) = instances.get_mut(&agent_id) {
                inst.status = InstanceStatus::Stopped;
            }
        }

        Ok(())
    }

    /// Destroy an agent's OpenClaw instance (container + data).
    pub async fn destroy_instance(&self, agent_id: Uuid) -> Result<()> {
        let instance = {
            let instances = self.instances.read().await;
            instances.get(&agent_id).cloned()
        };

        if let Some(inst) = instance {
            let _ = tokio::process::Command::new("docker")
                .args(["rm", "-f", &inst.container_name])
                .output()
                .await;

            // Remove agent data directory
            let agent_dir = self.data_dir.join(agent_id.to_string());
            let _ = tokio::fs::remove_dir_all(&agent_dir).await;
        }

        {
            let mut instances = self.instances.write().await;
            instances.remove(&agent_id);
        }

        Ok(())
    }

    /// List all managed instances.
    pub async fn list_instances(&self) -> Vec<OpenClawInstance> {
        let instances = self.instances.read().await;
        instances.values().cloned().collect()
    }

    /// Check health of an instance by verifying /v1/responses is serving.
    pub async fn check_health(&self, agent_id: Uuid) -> bool {
        let instance = {
            let instances = self.instances.read().await;
            instances.get(&agent_id).cloned()
        };

        if let Some(inst) = instance {
            // Check the actual API endpoint we'll use, not just the root.
            // Any response other than 404/502/503 means the endpoint is active.
            let url = format!("http://127.0.0.1:{}/v1/responses", inst.port);
            match reqwest::Client::new()
                .post(&url)
                .header("Authorization", format!("Bearer {}", inst.gateway_token))
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({}))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(r) => {
                    let s = r.status().as_u16();
                    s != 404 && s != 502 && s != 503
                }
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Wait for an instance to be ready (health check passes).
    pub async fn wait_for_ready(&self, agent_id: Uuid, timeout_secs: u64) -> bool {
        let deadline = tokio::time::Instant::now()
            + tokio::time::Duration::from_secs(timeout_secs);

        while tokio::time::Instant::now() < deadline {
            if self.check_health(agent_id).await {
                return true;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
        }

        tracing::warn!(
            "OpenClaw instance for agent {} failed to become ready within {}s",
            agent_id,
            timeout_secs
        );
        false
    }

    /// Recover instances from DB on startup.
    pub async fn recover_instances(&self, db_pool: &PgPool) -> Result<()> {
        // Clean up ALL stale openclaw containers from previous installs/sessions.
        // With --network host, leftover containers hold ports and cause conflicts.
        if let Ok(output) = tokio::process::Command::new("docker")
            .args(["ps", "-a", "--filter", "name=multiclaw-openclaw-", "--format", "{{.Names}}"])
            .output()
            .await
        {
            let containers = String::from_utf8_lossy(&output.stdout);
            for name in containers.lines() {
                let name = name.trim();
                if !name.is_empty() {
                    tracing::info!("Cleaning up stale OpenClaw container: {}", name);
                    let _ = tokio::process::Command::new("docker")
                        .args(["rm", "-f", name])
                        .output()
                        .await;
                }
            }
        }

        // Clear stale in-memory state — containers are gone, map entries must go too.
        // Without this, spawn_instance's early-return guard sees stale Running/Starting
        // entries and skips re-creation.
        {
            let mut instances = self.instances.write().await;
            instances.clear();
        }
        // Reset port counter — all containers are wiped, so ports are free.
        self.next_port_offset.store(0, Ordering::SeqCst);

        // Find all agents that should have OpenClaw instances
        let agents: Vec<(Uuid, String, String, Option<String>, Option<String>, Option<Uuid>, String)> =
            sqlx::query_as(
                "SELECT a.id, a.name, a.role, a.specialty, a.system_prompt, a.company_id, a.effective_model \
                 FROM agents a WHERE a.status = 'ACTIVE' ORDER BY \
                 CASE WHEN a.role = 'MAIN' THEN 0 ELSE 1 END, a.created_at"
            )
            .fetch_all(db_pool)
            .await?;

        if agents.is_empty() {
            tracing::info!("No active agents found — skipping OpenClaw recovery");
            return Ok(());
        }

        let holding_name: String = sqlx::query_scalar("SELECT name FROM holdings LIMIT 1")
            .fetch_optional(db_pool)
            .await?
            .unwrap_or_else(|| "MultiClaw Holdings".to_string());

        for (agent_id, name, role, specialty, system_prompt, company_id, model) in agents {
            let company_name = if let Some(cid) = company_id {
                sqlx::query_scalar::<_, String>("SELECT name FROM companies WHERE id = $1")
                    .bind(cid)
                    .fetch_optional(db_pool)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| holding_name.clone())
            } else {
                holding_name.clone()
            };

            let config = AgentConfig {
                agent_id,
                agent_name: name.clone(),
                role: role.clone(),
                company_name,
                holding_name: holding_name.clone(),
                specialty,
                model,
                system_prompt,
            };

            match self.spawn_instance(&config).await {
                Ok(_) => tracing::info!("Recovered OpenClaw instance for {}", name),
                Err(e) => tracing::error!("Failed to recover OpenClaw instance for {}: {}", name, e),
            }
        }

        Ok(())
    }

    /// Periodic reconciliation — check that all ACTIVE agents have running containers.
    /// Surgically respawns only the containers that are missing or stopped, leaving
    /// healthy containers untouched.
    pub async fn reconcile_instances(&self, db_pool: &PgPool) -> Result<()> {
        // Get all ACTIVE agent IDs from DB
        let agent_ids: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM agents WHERE status = 'ACTIVE'"
        ).fetch_all(db_pool).await?;

        if agent_ids.is_empty() {
            return Ok(());
        }

        // Phase 1: Identify which agents need respawning (read lock only)
        let mut needs_respawn: Vec<Uuid> = Vec::new();
        {
            let instances = self.instances.read().await;
            for (agent_id,) in &agent_ids {
                if let Some(inst) = instances.get(agent_id) {
                    // Skip instances still starting up — avoid racing the spawn
                    if inst.status == InstanceStatus::Starting {
                        continue;
                    }

                    // Check if Docker container is still running
                    let inspect = tokio::process::Command::new("docker")
                        .args(["inspect", "--format", "{{.State.Running}}", &inst.container_name])
                        .output()
                        .await;

                    let running = match inspect {
                        Ok(output) if output.status.success() => {
                            String::from_utf8_lossy(&output.stdout).trim() == "true"
                        }
                        _ => false,
                    };

                    if !running {
                        tracing::warn!(
                            "Watchdog: container {} for agent {} is not running, will respawn",
                            inst.container_name, inst.agent_name
                        );
                        needs_respawn.push(*agent_id);
                    }
                } else {
                    tracing::warn!(
                        "Watchdog: no tracked instance for active agent {}, will respawn",
                        agent_id
                    );
                    needs_respawn.push(*agent_id);
                }
            }
        }

        if needs_respawn.is_empty() {
            return Ok(());
        }

        // Phase 2: Remove stale entries so spawn_instance won't early-return
        {
            let mut instances = self.instances.write().await;
            for agent_id in &needs_respawn {
                instances.remove(agent_id);
            }
        }

        // Phase 3: Fetch configs and respawn individually
        let holding_name: String = sqlx::query_scalar("SELECT name FROM holdings LIMIT 1")
            .fetch_optional(db_pool)
            .await?
            .unwrap_or_else(|| "MultiClaw Holdings".to_string());

        for agent_id in needs_respawn {
            let row: Option<(Uuid, String, String, Option<String>, Option<String>, Option<Uuid>, String)> =
                sqlx::query_as(
                    "SELECT a.id, a.name, a.role, a.specialty, a.system_prompt, a.company_id, a.effective_model \
                     FROM agents a WHERE a.id = $1 AND a.status = 'ACTIVE'"
                )
                .bind(agent_id)
                .fetch_optional(db_pool)
                .await?;

            if let Some((id, name, role, specialty, system_prompt, company_id, model)) = row {
                let company_name = if let Some(cid) = company_id {
                    sqlx::query_scalar::<_, String>("SELECT name FROM companies WHERE id = $1")
                        .bind(cid)
                        .fetch_optional(db_pool)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| holding_name.clone())
                } else {
                    holding_name.clone()
                };

                let config = AgentConfig {
                    agent_id: id,
                    agent_name: name.clone(),
                    role,
                    company_name,
                    holding_name: holding_name.clone(),
                    specialty,
                    model,
                    system_prompt,
                };

                match self.spawn_instance(&config).await {
                    Ok(_) => tracing::info!("Watchdog: respawned OpenClaw instance for {}", name),
                    Err(e) => tracing::error!("Watchdog: failed to respawn instance for {}: {}", name, e),
                }
            }
        }

        Ok(())
    }

    // ─── Private helpers ───────────────────────────────────────────────

    fn render_config(&self, config: &AgentConfig, port: u16, token: &str) -> Result<String> {
        // Read the template
        let template_path = self.find_template_path("openclaw-base.json5")?;
        let template = std::fs::read_to_string(&template_path)
            .unwrap_or_else(|_| self.default_config_template());

        let rendered = template
            .replace("{{AGENT_NAME}}", &config.agent_name)
            .replace("{{AGENT_PORT}}", &port.to_string())
            .replace("{{OLLAMA_URL}}", &self.ollama_url)
            .replace("{{MODEL}}", &config.model)
            .replace("{{GATEWAY_TOKEN}}", token);

        Ok(rendered)
    }

    async fn render_workspace(&self, config: &AgentConfig, workspace_dir: &Path) -> Result<()> {
        let template_dir = self.find_template_dir()?;

        // Determine role-specific subdirectory (MAIN, CEO, MANAGER, WORKER)
        let role_upper = config.role.to_uppercase();
        let role_dir = template_dir.join(&role_upper);

        // Helper: try role-specific file first, then generic, then default
        async fn read_template(role_dir: &Path, template_dir: &Path, filename: &str, default: String) -> String {
            // Try role-specific: e.g., workspace-template/CEO/SOUL.md
            if let Ok(content) = tokio::fs::read_to_string(role_dir.join(filename)).await {
                return content;
            }
            // Fall back to generic: e.g., workspace-template/SOUL.md
            if let Ok(content) = tokio::fs::read_to_string(template_dir.join(filename)).await {
                return content;
            }
            default
        }

        // Render SOUL.md
        let soul_template = read_template(&role_dir, &template_dir, "SOUL.md", self.default_soul_template()).await;
        let soul = self.replace_vars(&soul_template, config);
        tokio::fs::write(workspace_dir.join("SOUL.md"), &soul).await?;

        // Render AGENTS.md
        let agents_template = read_template(&role_dir, &template_dir, "AGENTS.md", self.default_agents_template()).await;
        let agents = self.replace_vars(&agents_template, config);
        tokio::fs::write(workspace_dir.join("AGENTS.md"), &agents).await?;

        // Render TOOLS.md
        let tools_template = read_template(&role_dir, &template_dir, "TOOLS.md",
            "# Tools\nUse bash and curl to interact with the MultiClaw API.".into()).await;
        tokio::fs::write(workspace_dir.join("TOOLS.md"), &tools_template).await?;

        // Render skill
        let skill_dir = workspace_dir.join("skills").join("multiclaw");
        tokio::fs::create_dir_all(&skill_dir).await?;
        let role_skill_path = role_dir.join("skills").join("multiclaw").join("SKILL.md");
        let generic_skill_path = template_dir.join("skills").join("multiclaw").join("SKILL.md");
        let skill_template = if role_skill_path.exists() {
            tokio::fs::read_to_string(&role_skill_path).await.unwrap_or_else(|_| self.default_skill_template())
        } else if generic_skill_path.exists() {
            tokio::fs::read_to_string(&generic_skill_path).await.unwrap_or_else(|_| self.default_skill_template())
        } else {
            self.default_skill_template()
        };
        let skill = self.replace_vars(&skill_template, config);
        tokio::fs::write(skill_dir.join("SKILL.md"), &skill).await?;

        Ok(())
    }

    fn replace_vars(&self, template: &str, config: &AgentConfig) -> String {
        template
            .replace("{{AGENT_NAME}}", &config.agent_name)
            .replace("{{AGENT_ROLE}}", &config.role)
            .replace("{{COMPANY_NAME}}", &config.company_name)
            .replace("{{HOLDING_NAME}}", &config.holding_name)
            .replace("{{SPECIALTY}}", config.specialty.as_deref().unwrap_or("general operations"))
            .replace("{{MULTICLAW_API_URL}}", &self.multiclaw_api_url)
            .replace("{{AGENT_ID}}", &config.agent_id.to_string())
            .replace("{{MODEL}}", &config.model)
            // Handle conditional blocks (simple approach)
            .replace("{{#if SPECIALTY}}", "")
            .replace("{{/if}}", "")
    }

    fn find_template_path(&self, filename: &str) -> Result<PathBuf> {
        // Look in /opt/multiclaw/openclaw/ first, then relative to binary
        let paths = vec![
            PathBuf::from("/opt/multiclaw/openclaw").join(filename),
            PathBuf::from("/app/infra/openclaw").join(filename),
            PathBuf::from("infra/openclaw").join(filename),
        ];
        for p in &paths {
            if p.exists() {
                return Ok(p.clone());
            }
        }
        // Return the last path — will use fallback template
        Ok(paths.last().unwrap().clone())
    }

    fn find_template_dir(&self) -> Result<PathBuf> {
        let paths = vec![
            PathBuf::from("/opt/multiclaw/openclaw/workspace-template"),
            PathBuf::from("/app/infra/openclaw/workspace-template"),
            PathBuf::from("infra/openclaw/workspace-template"),
        ];
        for p in &paths {
            if p.exists() {
                return Ok(p.clone());
            }
        }
        Ok(paths.last().unwrap().clone())
    }

    fn default_config_template(&self) -> String {
        r#"{
  "gateway": {
    "mode": "local",
    "bind": "lan",
    "port": {{AGENT_PORT}},
    "auth": { "mode": "token", "token": "{{GATEWAY_TOKEN}}" },
    "http": { "endpoints": { "responses": { "enabled": true } } }
  },
  "agents": {
    "defaults": {
      "model": { "primary": "ollama/{{MODEL}}" },
      "workspace": "/workspace",
      "skipBootstrap": true
    }
  },
  "models": {
    "mode": "replace",
    "providers": {
      "ollama": {
        "baseUrl": "{{OLLAMA_URL}}/v1",
        "apiKey": "ollama",
        "api": "openai-completions",
        "models": [{ "id": "{{MODEL}}", "name": "{{MODEL}}", "reasoning": false, "input": ["text"], "contextWindow": 198000, "maxTokens": 16384 }]
      }
    }
  },
  "hooks": {
    "internal": {
      "enabled": true,
      "entries": {
        "session-memory": { "enabled": true },
        "command-logger": { "enabled": true }
      }
    }
  },
  "channels": {}
}"#
        .to_string()
    }

    fn default_soul_template(&self) -> String {
        "# Identity\nYou are {{AGENT_NAME}}, a {{AGENT_ROLE}} at {{COMPANY_NAME}}.\nYou are part of the {{HOLDING_NAME}} holding company.\nBe professional, concise, and proactive.".to_string()
    }

    fn default_agents_template(&self) -> String {
        "# {{AGENT_NAME}}\nRole: {{AGENT_ROLE}}\nCompany: {{COMPANY_NAME}}\nHolding: {{HOLDING_NAME}}".to_string()
    }

    fn default_skill_template(&self) -> String {
        "---\nname: multiclaw\ndescription: MultiClaw platform operations\n---\n# MultiClaw API\nUse curl to call {{MULTICLAW_API_URL}}/v1/ endpoints.".to_string()
    }
}
