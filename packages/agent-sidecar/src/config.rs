use anyhow::Result;
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub agent_id: String,
    pub agentd_token: String,
    pub ollama_token: String,
    pub multiclawd_url: String,         // Control plane url
    pub host_ollama_proxy_url: String,  // Ollama proxy on host
    pub openclaw_url: String,           // Local openclaw gateway
    pub openclaw_token: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            agent_id: env::var("AGENT_ID").unwrap_or_else(|_| "dev_agent".into()),
            agentd_token: env::var("AGENTD_TOKEN").unwrap_or_else(|_| "dev_token".into()),
            ollama_token: env::var("OLLAMA_TOKEN").unwrap_or_else(|_| "dev_token".into()),
            multiclawd_url: env::var("MULTICLAWD_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into()),
            host_ollama_proxy_url: env::var("HOST_OLLAMA_PROXY_URL").unwrap_or_else(|_| "http://127.0.0.1:11436".into()),
            openclaw_url: env::var("OPENCLAW_URL").unwrap_or_else(|_| "http://127.0.0.1:18789".into()),
            openclaw_token: env::var("OPENCLAW_GATEWAY_TOKEN").unwrap_or_else(|_| "dev_token".into()),
        })
    }
}
