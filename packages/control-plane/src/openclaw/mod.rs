use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawInstance {
    pub agent_id: Uuid,
    pub agent_name: String,
    pub container_name: String,
    pub port: u16,
    pub gateway_token: String,
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
        }
    }

    /// Spawn an OpenClaw instance for an agent.
    pub async fn spawn_instance(&self, config: &AgentConfig) -> Result<OpenClawInstance> {
        let agent_id = config.agent_id;

        // Check if already running
        {
            let instances = self.instances.read().await;
            if let Some(inst) = instances.get(&agent_id) {
                if inst.status == InstanceStatus::Running {
                    tracing::info!("OpenClaw instance already running for {}", config.agent_name);
                    return Ok(inst.clone());
                }
            }
        }

        // Assign a port
        let port = {
            let instances = self.instances.read().await;
            self.base_port + instances.len() as u16
        };

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

        // Stop existing container if any
        let _ = tokio::process::Command::new("docker")
            .args(["rm", "-f", &container_name])
            .output()
            .await;

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
            status: InstanceStatus::Running,
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

        Ok(instance)
    }

    /// Send a message to an agent's OpenClaw instance and get the response.
    /// Uses the CLI in the Docker container to send a message.
    pub async fn send_message(&self, agent_id: Uuid, message: &str) -> Result<String> {
        let instance = {
            let instances = self.instances.read().await;
            instances
                .get(&agent_id)
                .cloned()
                .ok_or_else(|| anyhow!("No OpenClaw instance for agent {}", agent_id))?
        };

        if instance.status != InstanceStatus::Running {
            return Err(anyhow!(
                "OpenClaw instance for {} is not running (status: {:?})",
                instance.agent_name,
                instance.status
            ));
        }

        tracing::info!(
            "Sending message to {} via OpenClaw (container: {})",
            instance.agent_name,
            instance.container_name
        );

        // Use docker exec to run openclaw agent --message in the container
        let output = tokio::process::Command::new("docker")
            .args([
                "exec", &instance.container_name,
                "openclaw", "agent",
                "--message", message,
                "--no-stream",
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!(
                "OpenClaw message failed for {}: {}",
                instance.agent_name,
                stderr
            );

            // If the container is dead, try to restart it
            if stderr.contains("is not running") || stderr.contains("No such container") {
                let mut instances = self.instances.write().await;
                if let Some(inst) = instances.get_mut(&agent_id) {
                    inst.status = InstanceStatus::Failed;
                }
            }

            return Err(anyhow!("OpenClaw agent error: {}", stderr));
        }

        let response = String::from_utf8_lossy(&output.stdout).trim().to_string();
        tracing::info!(
            "{} responded: {}",
            instance.agent_name,
            &response[..response.len().min(200)]
        );

        Ok(response)
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

    /// Check health of an instance by pinging its gateway.
    pub async fn check_health(&self, agent_id: Uuid) -> bool {
        let instance = {
            let instances = self.instances.read().await;
            instances.get(&agent_id).cloned()
        };

        if let Some(inst) = instance {
            let url = format!("http://127.0.0.1:{}/", inst.port);
            match reqwest::Client::new()
                .get(&url)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(r) => r.status().is_success() || r.status().as_u16() == 401,
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

        // Render SOUL.md
        let soul_template = tokio::fs::read_to_string(template_dir.join("SOUL.md"))
            .await
            .unwrap_or_else(|_| self.default_soul_template());
        let soul = self.replace_vars(&soul_template, config);
        tokio::fs::write(workspace_dir.join("SOUL.md"), &soul).await?;

        // Render AGENTS.md
        let agents_template = tokio::fs::read_to_string(template_dir.join("AGENTS.md"))
            .await
            .unwrap_or_else(|_| self.default_agents_template());
        let agents = self.replace_vars(&agents_template, config);
        tokio::fs::write(workspace_dir.join("AGENTS.md"), &agents).await?;

        // Render TOOLS.md
        let tools_template = tokio::fs::read_to_string(template_dir.join("TOOLS.md"))
            .await
            .unwrap_or_else(|_| "# Tools\nUse bash and curl to interact with the MultiClaw API.".into());
        tokio::fs::write(workspace_dir.join("TOOLS.md"), &tools_template).await?;

        // Render skill
        let skill_dir = workspace_dir.join("skills").join("multiclaw");
        tokio::fs::create_dir_all(&skill_dir).await?;
        let skill_template = tokio::fs::read_to_string(
            template_dir.join("skills").join("multiclaw").join("SKILL.md"),
        )
        .await
        .unwrap_or_else(|_| self.default_skill_template());
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
    "auth": { "mode": "token", "token": "{{GATEWAY_TOKEN}}" }
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
        "models": [{ "id": "{{MODEL}}", "name": "{{MODEL}}", "reasoning": false, "input": ["text"], "contextWindow": 8192, "maxTokens": 4096 }]
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
