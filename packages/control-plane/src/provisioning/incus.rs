use super::vm_provider::{VmDetails, VmProvider, VmResources};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::process::Command as StdCommand; // Renamed to avoid conflict with tokio::process::Command
use tokio::process::Command;
use tokio::time::{sleep, Duration};

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
        
        let addrs = instance
            .get("state")?
            .get("network")?
            .get("eth0")?
            .get("addresses")?
            .as_array()?;
            
        for addr in addrs {
            if addr.get("family").and_then(|v| v.as_str()) == Some("inet") {
                return addr.get("address").map(|v| v.as_str().unwrap().to_string());
            }
        }
        None
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
                &format!("-c limits.cpu={}", resources.vcpus),
                &format!("-c limits.memory={}MB", resources.memory_mb),
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
