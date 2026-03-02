use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmResources {
    pub vcpus: u32,
    pub memory_mb: u32,
    pub disk_gb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmDetails {
    pub provider_ref: String,
    pub ip_address: Option<String>,
}

#[async_trait]
pub trait VmProvider: Send + Sync {
    /// Provisions a new VM with the given parameters, and blocks until it acquires an IP
    async fn provision(&self, name: &str, resources: &VmResources, cloud_init_data: &str) -> Result<VmDetails>;
    
    /// Deletes a VM
    async fn destroy(&self, name: &str) -> Result<()>;
    
    /// Stops a VM
    async fn stop(&self, name: &str) -> Result<()>;
}
