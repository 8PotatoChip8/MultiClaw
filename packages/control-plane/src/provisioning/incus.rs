use super::vm_provider::{VmDetails, VmProvider, VmResources};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use tokio::process::Command;
use tokio::time::{sleep, timeout, Duration};

const MAX_OUTPUT_BYTES: usize = 1_048_576; // 1 MB

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VmInfo {
    pub status: String,
    pub ip_address: Option<String>,
    pub cpu_usage_ns: Option<i64>,
    pub memory_usage_bytes: Option<i64>,
    pub memory_total_bytes: Option<i64>,
}

pub struct IncusProvider;

impl IncusProvider {
    pub async fn new() -> Result<Self> {
        // Ensure multiclawbr0 exists
        let output = Command::new("incus")
            .args(&["network", "list", "--format", "json"])
            .output()
            .await?;
        
        if output.status.success() {
            let nets: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap_or_default();
            let exists = nets.iter().any(|n| n.get("name").and_then(|v| v.as_str()) == Some("multiclawbr0"));
            if !exists {
                Command::new("incus")
                    .args(&["network", "create", "multiclawbr0"])
                    .output()
                    .await?;
            }
        }
        
        Ok(Self)
    }

    async fn get_ip(&self, name: &str) -> Option<String> {
        let output = Command::new("incus")
            .args(&["list", name, "--format", "json"])
            .output()
            .await
            .ok()?;
        
        let instances: Vec<Value> = serde_json::from_slice(&output.stdout).ok()?;
        let instance = instances.first()?;
        
        let network = instance
            .get("state")?
            .get("network")?
            .as_object()?;
        
        // Check all network interfaces (eth0, enp5s0, enp6s0, etc.)
        for (_iface_name, iface) in network {
            let addrs = iface.get("addresses")?.as_array()?;
            for addr in addrs {
                if addr.get("family").and_then(|v| v.as_str()) == Some("inet") {
                    if let Some(scope) = addr.get("scope").and_then(|v| v.as_str()) {
                        if scope == "global" {
                            return addr.get("address").map(|v| v.as_str().unwrap().to_string());
                        }
                    }
                    // Fallback: return any inet address that's not 127.x.x.x
                    if let Some(ip) = addr.get("address").and_then(|v| v.as_str()) {
                        if !ip.starts_with("127.") {
                            return Some(ip.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Execute a command inside the VM via `incus exec`
    pub async fn exec_command(
        &self,
        vm_name: &str,
        command: &str,
        user: Option<&str>,
        working_dir: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<ExecResult> {
        let mut args: Vec<&str> = vec!["exec", vm_name];

        let uid_str;
        if let Some(u) = user {
            // incus exec --user expects a numeric UID; map "root" → 0, "agent" → 1000
            let uid = match u {
                "root" => 0u32,
                _ => 1000,
            };
            uid_str = uid.to_string();
            args.extend(&["--user", &uid_str]);
        }

        if let Some(wd) = working_dir {
            args.extend(&["--cwd", wd]);
        }

        args.extend(&["--", "bash", "-lc", command]);

        let secs = timeout_secs.unwrap_or(30).min(120);
        let output = timeout(
            Duration::from_secs(secs),
            Command::new("incus").args(&args).output(),
        )
        .await
        .map_err(|_| anyhow!("Command timed out after {} seconds", secs))??;

        let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
        stdout.truncate(MAX_OUTPUT_BYTES);
        stderr.truncate(MAX_OUTPUT_BYTES);

        Ok(ExecResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout,
            stderr,
        })
    }

    /// Get VM status info (running state, IP, resource usage)
    pub async fn get_info(&self, vm_name: &str) -> Result<VmInfo> {
        let output = Command::new("incus")
            .args(&["list", vm_name, "--format", "json"])
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow!("Failed to query VM info"));
        }

        let instances: Vec<Value> = serde_json::from_slice(&output.stdout)?;
        let instance = instances.first().ok_or_else(|| anyhow!("VM not found"))?;

        let status = instance
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let ip = self.get_ip(vm_name).await;

        let state = instance.get("state");
        let cpu_usage_ns = state
            .and_then(|s| s.get("cpu"))
            .and_then(|c| c.get("usage"))
            .and_then(|u| u.as_i64());
        let memory_usage_bytes = state
            .and_then(|s| s.get("memory"))
            .and_then(|m| m.get("usage"))
            .and_then(|u| u.as_i64());
        let memory_total_bytes = state
            .and_then(|s| s.get("memory"))
            .and_then(|m| m.get("total"))
            .and_then(|t| t.as_i64());

        Ok(VmInfo {
            status,
            ip_address: ip,
            cpu_usage_ns,
            memory_usage_bytes,
            memory_total_bytes,
        })
    }

    /// List all running VM names (for batch status checks)
    pub async fn list_running(&self) -> Result<Vec<String>> {
        let output = Command::new("incus")
            .args(&["list", "--format", "json", "status=Running"])
            .output()
            .await?;

        if !output.status.success() {
            return Ok(vec![]);
        }

        let instances: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap_or_default();
        Ok(instances.iter()
            .filter_map(|i| i.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
            .collect())
    }

    /// Push file content into the VM
    pub async fn file_push(
        &self,
        vm_name: &str,
        content: &[u8],
        remote_path: &str,
    ) -> Result<()> {
        let tmp = format!("/tmp/multiclaw-push-{}", uuid::Uuid::new_v4());
        tokio::fs::write(&tmp, content).await?;

        let output = Command::new("incus")
            .args(&[
                "file",
                "push",
                &tmp,
                &format!("{}/{}", vm_name, remote_path.trim_start_matches('/')),
            ])
            .output()
            .await?;

        let _ = tokio::fs::remove_file(&tmp).await;

        if !output.status.success() {
            return Err(anyhow!(
                "file push failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(())
    }

    /// Pull a file from the VM
    pub async fn file_pull(&self, vm_name: &str, remote_path: &str) -> Result<Vec<u8>> {
        let tmp = format!("/tmp/multiclaw-pull-{}", uuid::Uuid::new_v4());

        let output = Command::new("incus")
            .args(&[
                "file",
                "pull",
                &format!("{}/{}", vm_name, remote_path.trim_start_matches('/')),
                &tmp,
            ])
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow!(
                "file pull failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let content = tokio::fs::read(&tmp).await?;
        let _ = tokio::fs::remove_file(&tmp).await;
        Ok(content)
    }
}

#[async_trait]
impl VmProvider for IncusProvider {
    async fn provision(
        &self,
        name: &str,
        resources: &VmResources,
        cloud_init_data: &str,
    ) -> Result<VmDetails> {
        // 1. Write cloud-init temp file
        let tmp_path = format!("/tmp/{name}-cloud-init.yaml");
        tokio::fs::write(&tmp_path, cloud_init_data).await?;

        // 2. Launch VM
        tracing::info!("Launching incus vm: {}", name);
        let launch_out = Command::new("incus")
            .args(&[
                "launch", "images:ubuntu/24.04/cloud", name, "--vm",
                "-c", &format!("limits.cpu={}", resources.vcpus),
                "-c", &format!("limits.memory={}MB", resources.memory_mb),
            ])
            .output()
            .await?;

        if !launch_out.status.success() {
            return Err(anyhow!(
                "Failed to launch VM: {}",
                String::from_utf8_lossy(&launch_out.stderr)
            ));
        }

        // 3. Attach network
        Command::new("incus")
            .args(&["network", "attach", "multiclawbr0", name, "eth0"])
            .output()
            .await?;

        // 4. Push user-data
        let _ = Command::new("incus")
            .args(&["config", "set", name, "cloud-init.user-data", &std::fs::read_to_string(&tmp_path)?])
            .output()
            .await?;

        // 5. Restart to apply cloud-init
        Command::new("incus").args(&["restart", name]).output().await?;

        // 6. Wait for IP
        let mut ip = None;
        for _ in 0..60 {
            if let Some(i) = self.get_ip(name).await {
                ip = Some(i);
                break;
            }
            sleep(Duration::from_secs(1)).await;
        }

        let _ = tokio::fs::remove_file(tmp_path).await;

        Ok(VmDetails {
            provider_ref: name.to_string(),
            ip_address: ip,
        })
    }

    async fn destroy(&self, name: &str) -> Result<()> {
        Command::new("incus")
            .args(&["delete", name, "--force"])
            .output()
            .await?;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        Command::new("incus")
            .args(&["stop", name, "--force"])
            .output()
            .await?;
        Ok(())
    }
}
