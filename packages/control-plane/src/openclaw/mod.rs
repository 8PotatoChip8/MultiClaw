mod rate_limiter;

use anyhow::{anyhow, Result};
use futures::future::join_all;
use rate_limiter::ConcurrentRateLimiter;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
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
    /// Concurrency-aware rate limiter for upstream LLM API calls (semaphore + 429 backoff)
    rate_limiter: ConcurrentRateLimiter,
    /// Atomic counter for port allocation (avoids race conditions on concurrent spawns)
    next_port_offset: Arc<AtomicU16>,
    /// Agent IDs with a spawn in flight — watchdog must skip these to avoid duplicate containers.
    pending_spawns: Arc<RwLock<HashSet<Uuid>>>,
    /// Comma-separated list of available models for SKILL.md template rendering.
    available_models_csv: Arc<std::sync::RwLock<String>>,
    /// In-memory pull status for each model (transient, not persisted).
    model_pull_status: Arc<std::sync::RwLock<HashMap<String, ModelPullStatus>>>,
    /// Docker --memory limit for containers (e.g. "4g").
    container_memory_limit: String,
    /// Docker --cpus limit for containers (e.g. "2.0").
    container_cpu_limit: String,
    /// Tracks respawn attempts per agent for exponential backoff.
    /// Maps agent_id → (attempt_count, last_attempt_time).
    respawn_attempts: Arc<RwLock<HashMap<Uuid, (u32, tokio::time::Instant)>>>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModelPullStatus {
    Pulling,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub agent_id: Uuid,
    pub agent_name: String,
    pub role: String,
    pub company_id: Option<Uuid>,
    pub company_name: String,
    pub company_type: Option<String>,  // "INTERNAL" or "EXTERNAL"
    pub company_description: Option<String>,
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
        max_concurrent_ollama: usize,
        container_memory_limit: String,
        container_cpu_limit: String,
    ) -> Self {
        Self {
            instances: Arc::new(RwLock::new(HashMap::new())),
            data_dir,
            ollama_url,
            multiclaw_api_url,
            image: "ghcr.io/openclaw/openclaw:latest".to_string(),
            base_port: 18790,
            rate_limiter: ConcurrentRateLimiter::new(max_concurrent_ollama),
            next_port_offset: Arc::new(AtomicU16::new(0)),
            pending_spawns: Arc::new(RwLock::new(HashSet::new())),
            available_models_csv: Arc::new(std::sync::RwLock::new(
                "nemotron-3-super:cloud, minimax-m2.5:cloud, minimax-m2:cloud, glm-5:cloud, kimi-k2-thinking:cloud, kimi-k2.5:cloud, qwen3-coder:480b-cloud, devstral-2:123b-cloud, deepseek-v3.2:cloud, minimax-m2.1:cloud, glm-4.7:cloud, qwen3.5:397b-cloud, qwen3-coder-next:cloud".to_string()
            )),
            model_pull_status: Arc::new(std::sync::RwLock::new(HashMap::new())),
            container_memory_limit,
            container_cpu_limit,
            respawn_attempts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Returns the base data directory for agent workspaces.
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    /// Returns a mutable write guard to the instances map (for restart endpoint).
    pub async fn instances_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, HashMap<Uuid, OpenClawInstance>> {
        self.instances.write().await
    }

    /// Fire `count` concurrent 1-token requests to Ollama and return the number of successes.
    async fn fire_probe_batch(&self, model: &str, count: usize) -> usize {
        let client = reqwest::Client::new();
        let mut handles = Vec::with_capacity(count);

        for i in 0..count {
            let client = client.clone();
            let url = format!("{}/api/chat", self.ollama_url);
            let model = model.to_string();
            handles.push(tokio::spawn(async move {
                let resp = client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .json(&serde_json::json!({
                        "model": model,
                        "messages": [{"role": "user", "content": "Hi"}],
                        "stream": false,
                        "options": {"num_predict": 1},
                    }))
                    .timeout(std::time::Duration::from_secs(60))
                    .send()
                    .await;
                match resp {
                    Ok(r) => {
                        let status = r.status().as_u16();
                        tracing::debug!("Concurrency probe #{}: HTTP {}", i, status);
                        r.status().is_success()
                    }
                    Err(e) => {
                        tracing::debug!("Concurrency probe #{}: error {}", i, e);
                        false
                    }
                }
            }));
        }

        let results = join_all(handles).await;
        results.iter().filter(|r| matches!(r, Ok(true))).count()
    }

    /// Startup-only probe: waits for model availability, then discovers the concurrency limit.
    ///
    /// Call BEFORE any agent traffic. Uses a 1-token response to minimise cost.
    pub async fn probe_concurrency(&self, model: &str, ceiling: usize) {
        let ollama_url = self.ollama_url.clone();

        // Wait for Ollama to be reachable and the model to be available.
        // On fresh install / cold boot, Ollama may not have loaded the model yet.
        tracing::info!("Waiting for Ollama model '{}' to be available at {}...", model, ollama_url);
        let client = reqwest::Client::new();
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(120);
        loop {
            match client.get(format!("{}/api/tags", ollama_url))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let found = body["models"].as_array()
                            .map(|models| models.iter().any(|m| {
                                m["name"].as_str()
                                    .map(|n| n == model || n.starts_with(&format!("{}:", model.split(':').next().unwrap_or(model))))
                                    .unwrap_or(false)
                            }))
                            .unwrap_or(false);
                        if found {
                            tracing::info!("Ollama model '{}' is available", model);
                            break;
                        }
                        let test = client.post(format!("{}/api/chat", ollama_url))
                            .json(&serde_json::json!({
                                "model": model,
                                "messages": [{"role": "user", "content": "Hi"}],
                                "stream": false,
                                "options": {"num_predict": 1},
                            }))
                            .timeout(std::time::Duration::from_secs(30))
                            .send()
                            .await;
                        match test {
                            Ok(r) if r.status().is_success() || r.status().as_u16() == 429 => {
                                tracing::info!("Ollama model '{}' responds (cloud/unlisted model)", model);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }

            if tokio::time::Instant::now() >= deadline {
                tracing::warn!(
                    "Timed out waiting for Ollama model '{}' after 120s — proceeding with probe anyway",
                    model
                );
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }

        // Use the adaptive probe for the initial measurement too
        self.adaptive_probe_concurrency(model, ceiling).await;
    }

    /// Adaptive concurrency probe: discovers the current Ollama concurrency limit
    /// by probing at the current level and expanding upward if all succeed.
    ///
    /// Safe to call while agent traffic is active — probes bypass the rate limiter
    /// and `set_max_concurrent()` swaps the semaphore Arc without disrupting in-flight requests.
    pub async fn adaptive_probe_concurrency(&self, model: &str, ceiling: usize) {
        let current = self.rate_limiter.get_max_concurrent().await;
        let probe_start = current.max(2); // probe at least 2 even if current is 1

        tracing::info!(
            "Adaptive concurrency probe: current={}, ceiling={}, starting at {}",
            current, ceiling, probe_start
        );

        // Phase 1: validate current limit
        let succeeded = self.fire_probe_batch(model, probe_start).await;

        if succeeded == 0 {
            tracing::warn!(
                "Concurrency probe: 0/{} succeeded — Ollama may not be ready or login expired. \
                 Keeping current limit of {}.",
                probe_start, current
            );
            return;
        }

        if succeeded < probe_start {
            // Capacity decreased
            tracing::info!(
                "Concurrency probe: {}/{} succeeded — capacity decreased, adjusting {} -> {}",
                succeeded, probe_start, current, succeeded
            );
            self.rate_limiter.set_max_concurrent(succeeded).await;
            return;
        }

        // Phase 2: all passed — probe upward (up to 3 expansion rounds)
        let mut confirmed = succeeded;
        for round in 0..3u8 {
            if confirmed >= ceiling {
                break;
            }
            let step = (confirmed / 2).max(2);
            let extra = step.min(ceiling - confirmed);
            if extra == 0 {
                break;
            }

            tracing::debug!(
                "Concurrency probe expansion round {}: testing {} extra (confirmed so far: {})",
                round + 1, extra, confirmed
            );
            let extra_ok = self.fire_probe_batch(model, extra).await;
            confirmed += extra_ok;

            if extra_ok < extra {
                // Found the ceiling
                break;
            }
        }

        if confirmed != current {
            tracing::info!(
                "Concurrency probe: adjusting limit {} -> {} (ceiling={})",
                current, confirmed, ceiling
            );
            self.rate_limiter.set_max_concurrent(confirmed).await;
        } else {
            tracing::info!(
                "Concurrency probe: limit stable at {} (ceiling={})",
                current, ceiling
            );
        }
    }

    /// Register an agent as having a spawn in flight.
    /// Call BEFORE tokio::spawn(spawn_instance()) to prevent watchdog races.
    pub async fn register_pending_spawn(&self, agent_id: Uuid) {
        self.pending_spawns.write().await.insert(agent_id);
    }

    /// Spawn an OpenClaw instance for an agent.
    /// Automatically clears the pending-spawn flag on all exit paths.
    pub async fn spawn_instance(&self, config: &AgentConfig) -> Result<OpenClawInstance> {
        let result = self.spawn_instance_inner(config).await;
        // Always clear pending flag — instance is now tracked (success) or failed
        self.pending_spawns.write().await.remove(&config.agent_id);
        result
    }

    async fn spawn_instance_inner(&self, config: &AgentConfig) -> Result<OpenClawInstance> {
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

        // Ensure the agent's model is pulled before launching the container.
        // For auto-pulled models this is a fast no-op; for bench models (e.g.
        // qwen3-coder-next:cloud) this triggers an on-demand pull so the agent
        // doesn't fail on first send_message.
        {
            let needs_pull = self.model_pull_status.read()
                .map(|g| !matches!(g.get(&config.model), Some(ModelPullStatus::Ready) | Some(ModelPullStatus::Pulling)))
                .unwrap_or(true);
            if needs_pull {
                tracing::info!("On-demand model pull for '{}' (agent {})", config.model, config.agent_name);
                self.pull_model(&config.model).await;
            }
        }

        // Launch Docker container
        tracing::info!(
            "Spawning OpenClaw instance for {} on port {} (container: {})",
            config.agent_name, port, container_name
        );

        let env_token = format!("OPENCLAW_GATEWAY_TOKEN={}", gateway_token);
        let vol_config = format!("{}:/home/node/.openclaw:rw", config_dir.display());
        let vol_workspace = format!("{}:/workspace:rw", workspace_dir.display());
        // OpenClaw looks for skills at /app/skills/ by default; map workspace skills there too
        let vol_skills = format!("{}:/app/skills:rw", workspace_dir.join("skills").display());
        let port_str = port.to_string();

        // Mount the shared embeddings model (pre-downloaded during install) into
        // every container so memory_search works immediately without downloading.
        let gguf_path = self.data_dir.join("shared/models/embeddinggemma-300m-qat-Q8_0.gguf");
        let vol_models = if gguf_path.exists() {
            Some(format!("{}:/opt/multiclaw/shared/models:ro",
                self.data_dir.join("shared/models").display()))
        } else {
            tracing::warn!(
                "Embeddings model not found at {}; agents will download on first memory_search",
                gguf_path.display()
            );
            None
        };

        let mut docker_args: Vec<&str> = vec![
            "run", "-d",
            "--name", &container_name,
            "--network", "host",
            "-e", &env_token,
            "-v", &vol_config,
            "-v", &vol_workspace,
            "-v", &vol_skills,
        ];
        if let Some(ref vol) = vol_models {
            docker_args.extend(["-v", vol.as_str()]);
        }
        docker_args.extend([
            "--memory", &self.container_memory_limit,
            "--cpus", &self.container_cpu_limit,
            "--restart", "unless-stopped",
            &self.image,
            "openclaw", "gateway",
            "--port", &port_str,
            "--verbose",
        ]);

        let output = tokio::process::Command::new("docker")
            .args(&docker_args)
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
    pub async fn send_message(&self, agent_id: Uuid, message: &str, instructions: Option<&str>, timeout_secs: Option<u64>) -> Result<String> {
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
                        // Poll faster for the first 5 attempts (1s), then back off to 3s.
                        // Containers typically become ready within 3-10s, so fast initial
                        // polling reduces first-response latency without meaningful overhead.
                        let wait = if retries <= 5 { 1 } else { 3 };
                        tracing::info!("Waiting for OpenClaw instance for {} to be ready (attempt {}/30)", inst.agent_name, retries);
                        tokio::time::sleep(tokio::time::Duration::from_secs(wait)).await;
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
        let mut empty_retries = 0u32;

        loop {
            // Acquire a concurrency permit (blocks if all slots are in use).
            // The permit is held until we get a response or decide to retry,
            // preventing more than max_concurrent simultaneous Ollama requests.
            let _permit = self.rate_limiter.acquire().await;

            let resp = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", instance.gateway_token))
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(timeout_secs.unwrap_or(600)))
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    self.rate_limiter.record_success().await;

                    let resp_body: serde_json::Value = r.json().await
                        .map_err(|e| anyhow!("Failed to parse OpenClaw response: {}", e))?;

                    // Extract text from the OpenResponses format
                    // Response has an "output" array with message items containing output_text chunks.
                    // Concatenate ALL chunks — OpenClaw may split a streamed response across multiple
                    // output_text entries, and taking only the first drops most of the content.
                    let text = {
                        let mut all_text = String::new();
                        if let Some(outputs) = resp_body["output"].as_array() {
                            for item in outputs {
                                if item["type"] == "message" {
                                    if let Some(content) = item["content"].as_array() {
                                        for c in content {
                                            if c["type"] == "output_text" {
                                                if let Some(chunk) = c["text"].as_str() {
                                                    all_text.push_str(chunk);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if all_text.is_empty() {
                            // Fallback: try to get any text from the response
                            resp_body["output_text"]
                                .as_str()
                                .unwrap_or("[Agent produced no text output]")
                                .to_string()
                        } else {
                            all_text
                        }
                    };

                    tracing::info!(
                        "{} responded ({} chars)",
                        instance.agent_name,
                        text.len()
                    );

                    // Retry once if model returned no text (cold-start transient)
                    // Also retry on OpenClaw internal timeout messages (agent ran out of time)
                    let is_empty = text.trim().is_empty()
                        || text == "[Agent produced no text output]"
                        || text.contains("Request timed out before a response was generated");
                    if is_empty && empty_retries == 0 {
                        empty_retries += 1;
                        tracing::warn!(
                            "{} returned empty response, retrying once in 2s (cold-start?)",
                            instance.agent_name
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }

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
                    // Timeout means the OpenClaw run is still executing inside the container.
                    // Retrying would create a duplicate ghost run. Fail immediately.
                    if e.is_timeout() {
                        tracing::warn!(
                            "OpenClaw request timed out for {} after {}s (run may still be active inside container): {}",
                            instance.agent_name, timeout_secs.unwrap_or(600), e
                        );
                        return Err(anyhow!("OpenClaw request timed out: {}", e));
                    }
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
        {
            let mut pending = self.pending_spawns.write().await;
            pending.clear();
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
            let (company_name, company_type, company_description) = if let Some(cid) = company_id {
                let row: Option<(String, String, Option<String>)> = sqlx::query_as(
                    "SELECT name, type, description FROM companies WHERE id = $1"
                ).bind(cid).fetch_optional(db_pool).await.ok().flatten();
                match row {
                    Some((n, t, d)) => (n, Some(t), d),
                    None => (holding_name.clone(), None, None),
                }
            } else {
                (holding_name.clone(), None, None)
            };

            let config = AgentConfig {
                agent_id,
                agent_name: name.clone(),
                role: role.clone(),
                company_id,
                company_name,
                company_type,
                company_description,
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
            let pending = self.pending_spawns.read().await;
            let instances = self.instances.read().await;
            for (agent_id,) in &agent_ids {
                // Skip agents with a spawn already in flight
                if pending.contains(agent_id) {
                    tracing::debug!("Watchdog: skipping agent {} (spawn pending)", agent_id);
                    continue;
                }
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
            // Backoff: check if we've failed to respawn this agent recently
            {
                let attempts = self.respawn_attempts.read().await;
                if let Some((count, last)) = attempts.get(&agent_id) {
                    let delay = match count {
                        0 => std::time::Duration::from_secs(0),
                        1 => std::time::Duration::from_secs(30),
                        2 => std::time::Duration::from_secs(120),
                        _ => {
                            tracing::error!(
                                "Watchdog: agent {} failed to respawn after {} attempts, giving up (manual restart needed)",
                                agent_id, count
                            );
                            continue;
                        }
                    };
                    if last.elapsed() < delay {
                        tracing::debug!(
                            "Watchdog: skipping agent {} respawn (backoff: attempt {}, {}s remaining)",
                            agent_id, count, delay.as_secs().saturating_sub(last.elapsed().as_secs())
                        );
                        continue;
                    }
                }
            }

            let row: Option<(Uuid, String, String, Option<String>, Option<String>, Option<Uuid>, String)> =
                sqlx::query_as(
                    "SELECT a.id, a.name, a.role, a.specialty, a.system_prompt, a.company_id, a.effective_model \
                     FROM agents a WHERE a.id = $1 AND a.status = 'ACTIVE'"
                )
                .bind(agent_id)
                .fetch_optional(db_pool)
                .await?;

            if let Some((id, name, role, specialty, system_prompt, company_id, model)) = row {
                let (company_name, company_type, company_description) = if let Some(cid) = company_id {
                    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
                        "SELECT name, type, description FROM companies WHERE id = $1"
                    ).bind(cid).fetch_optional(db_pool).await.ok().flatten();
                    match row {
                        Some((n, t, d)) => (n, Some(t), d),
                        None => (holding_name.clone(), None, None),
                    }
                } else {
                    (holding_name.clone(), None, None)
                };

                let config = AgentConfig {
                    agent_id: id,
                    agent_name: name.clone(),
                    role,
                    company_id,
                    company_name,
                    company_type,
                    company_description,
                    holding_name: holding_name.clone(),
                    specialty,
                    model,
                    system_prompt,
                };

                match self.spawn_instance(&config).await {
                    Ok(_) => {
                        tracing::info!("Watchdog: respawned OpenClaw instance for {}", name);
                        self.respawn_attempts.write().await.remove(&agent_id);
                    }
                    Err(e) => {
                        tracing::error!("Watchdog: failed to respawn instance for {}: {}", name, e);
                        let mut attempts = self.respawn_attempts.write().await;
                        let entry = attempts.entry(agent_id).or_insert((0, tokio::time::Instant::now()));
                        entry.0 += 1;
                        entry.1 = tokio::time::Instant::now();
                    }
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

        // If the pre-downloaded embeddings GGUF exists, inject a modelPath so
        // OpenClaw uses the local file instead of downloading 328 MB on first use.
        let gguf_path = self.data_dir.join("shared/models/embeddinggemma-300m-qat-Q8_0.gguf");
        let memory_search_local = if gguf_path.exists() {
            r#"local: { modelPath: "/opt/multiclaw/shared/models/embeddinggemma-300m-qat-Q8_0.gguf" },"#.to_string()
        } else {
            String::new()
        };

        let rendered = template
            .replace("{{AGENT_NAME}}", &config.agent_name)
            .replace("{{AGENT_PORT}}", &port.to_string())
            .replace("{{OLLAMA_URL}}", &self.ollama_url)
            .replace("{{MODEL}}", &config.model)
            .replace("{{GATEWAY_TOKEN}}", token)
            .replace("{{MEMORY_SEARCH_LOCAL}}", &memory_search_local);

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

        // Ensure memory/ directory exists for session-memory hook
        tokio::fs::create_dir_all(workspace_dir.join("memory")).await?;

        // Pre-create today's daily log so agents don't hit ENOENT on first read.
        // The session-memory hook populates these over time, but agents often try
        // to read today's file before the hook has fired.
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let today_log = workspace_dir.join("memory").join(format!("{}.md", today));
        if !today_log.exists() {
            tokio::fs::write(&today_log, format!("# Daily Log — {}\n", today)).await?;
        }

        // Render MEMORY.md only on first creation — preserve agent's edits on respawn
        let memory_path = workspace_dir.join("MEMORY.md");
        if !memory_path.exists() {
            let memory_template = read_template(&role_dir, &template_dir, "MEMORY.md",
                format!("# {} - Long-Term Memory\n\n## Identity\n- Role: {}\n",
                    config.agent_name, config.role)).await;
            let memory = self.replace_vars(&memory_template, config);
            tokio::fs::write(&memory_path, &memory).await?;
        }

        // Render HEARTBEAT.md (overwrite on respawn — checklist should match latest template)
        let heartbeat_template = read_template(&role_dir, &template_dir, "HEARTBEAT.md",
            "# Heartbeat Checklist\n\nRespond with [HEARTBEAT_OK] if nothing needs attention.\n".into()).await;
        let heartbeat = self.replace_vars(&heartbeat_template, config);
        tokio::fs::write(workspace_dir.join("HEARTBEAT.md"), &heartbeat).await?;

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

    /// Update the cached available models list from the database.
    pub async fn refresh_available_models(&self, db: &sqlx::PgPool) {
        let raw: Option<String> = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'available_models'")
            .fetch_optional(db).await.ok().flatten();
        if let Some(json_str) = raw {
            if let Ok(models) = serde_json::from_str::<Vec<String>>(&json_str) {
                let csv = models.join(", ");
                if let Ok(mut guard) = self.available_models_csv.write() {
                    *guard = csv;
                }
            }
        }
    }

    /// Pull a single model via Ollama HTTP API. Updates in-memory status.
    pub async fn pull_model(&self, model: &str) {
        // Skip if already pulling
        if let Ok(guard) = self.model_pull_status.read() {
            if guard.get(model) == Some(&ModelPullStatus::Pulling) {
                tracing::debug!("Model '{}' already pulling, skipping", model);
                return;
            }
        }

        if let Ok(mut guard) = self.model_pull_status.write() {
            guard.insert(model.to_string(), ModelPullStatus::Pulling);
        }

        tracing::info!("Pulling Ollama model '{}'...", model);
        let client = reqwest::Client::new();
        let result = client.post(format!("{}/api/pull", self.ollama_url))
            .json(&serde_json::json!({"name": model, "stream": false}))
            .timeout(std::time::Duration::from_secs(600))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("Model '{}' pulled successfully", model);
                if let Ok(mut guard) = self.model_pull_status.write() {
                    guard.insert(model.to_string(), ModelPullStatus::Ready);
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body_text = resp.text().await.unwrap_or_default();
                tracing::error!("Model '{}' pull failed: HTTP {} - {}", model, status, body_text);
                if let Ok(mut guard) = self.model_pull_status.write() {
                    guard.insert(model.to_string(), ModelPullStatus::Failed(
                        format!("HTTP {}: {}", status, body_text.chars().take(200).collect::<String>())
                    ));
                }
            }
            Err(e) => {
                tracing::error!("Model '{}' pull request failed: {}", model, e);
                if let Ok(mut guard) = self.model_pull_status.write() {
                    guard.insert(model.to_string(), ModelPullStatus::Failed(e.to_string()));
                }
            }
        }
    }

    /// Pull multiple models concurrently (up to 3 at a time).
    pub async fn pull_all_models(&self, models: Vec<String>) {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(3));
        let mut handles = Vec::new();

        for model in models {
            let this = self.clone();
            let sem = semaphore.clone();
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await;
                this.pull_model(&model).await;
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }
    }

    /// Get current pull status for all tracked models.
    pub fn get_pull_status(&self) -> HashMap<String, ModelPullStatus> {
        self.model_pull_status.read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn replace_vars(&self, template: &str, config: &AgentConfig) -> String {
        let models_csv = self.available_models_csv.read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "glm-5:cloud".to_string());

        // Build set of truthy condition names for {{#if COND}}...{{/if}} blocks
        let mut truthy: std::collections::HashSet<&str> = std::collections::HashSet::new();
        if config.specialty.is_some() { truthy.insert("SPECIALTY"); }
        if config.company_description.is_some() { truthy.insert("COMPANY_DESCRIPTION"); }
        let ct = config.company_type.as_deref().unwrap_or("INTERNAL");
        if ct == "INTERNAL" { truthy.insert("INTERNAL"); }
        if ct == "EXTERNAL" { truthy.insert("EXTERNAL"); }

        // Process conditional blocks: {{#if NAME}}...{{/if}}
        let mut result = template.to_string();
        loop {
            let Some(start) = result.find("{{#if ") else { break };
            let Some(tag_end) = result[start..].find("}}") else { break };
            let tag_end = start + tag_end + 2;
            let cond_name = result[start + 6..tag_end - 2].trim();
            let end_tag = "{{/if}}";
            let Some(block_end) = result[tag_end..].find(end_tag) else { break };
            let block_end = tag_end + block_end;
            let content = &result[tag_end..block_end];
            if truthy.contains(cond_name) {
                // Keep content, remove the tags
                let replacement = content.to_string();
                result = format!("{}{}{}", &result[..start], replacement, &result[block_end + end_tag.len()..]);
            } else {
                // Remove entire block including tags
                result = format!("{}{}", &result[..start], &result[block_end + end_tag.len()..]);
            }
        }

        // Simple variable substitution
        result
            .replace("{{AGENT_NAME}}", &config.agent_name)
            .replace("{{AGENT_ROLE}}", &config.role)
            .replace("{{COMPANY_ID}}", &config.company_id.map(|u| u.to_string()).unwrap_or_default())
            .replace("{{COMPANY_NAME}}", &config.company_name)
            .replace("{{COMPANY_TYPE}}", ct)
            .replace("{{HOLDING_NAME}}", &config.holding_name)
            .replace("{{SPECIALTY}}", config.specialty.as_deref().unwrap_or("general operations"))
            .replace("{{COMPANY_DESCRIPTION}}", config.company_description.as_deref().unwrap_or(""))
            .replace("{{MULTICLAW_API_URL}}", &self.multiclaw_api_url)
            .replace("{{AGENT_ID}}", &config.agent_id.to_string())
            .replace("{{MODEL}}", &config.model)
            .replace("{{AVAILABLE_MODELS}}", &models_csv)
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
        "# {{AGENT_NAME}}\nRole: {{AGENT_ROLE}}\nCompany: {{COMPANY_NAME}}\nHolding: {{HOLDING_NAME}}\nSpecialty: {{SPECIALTY}}".to_string()
    }

    fn default_skill_template(&self) -> String {
        "---\nname: multiclaw\ndescription: MultiClaw platform operations\n---\n# MultiClaw API\nUse curl to call {{MULTICLAW_API_URL}}/v1/ endpoints.".to_string()
    }
}
