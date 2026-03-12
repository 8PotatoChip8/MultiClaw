use anyhow::Result;
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub master_key_path: String,
    pub port: u16,
    pub ollama_url: String,
    pub host_ip: String,
    /// Maximum concurrent Ollama requests (semaphore permits). Default: 4.
    /// Set to 1 to restore serial behavior. Should match OLLAMA_NUM_PARALLEL on the server.
    pub max_concurrent_ollama: usize,
    /// Maximum concurrency the adaptive probe will attempt to discover. Default: 32.
    pub probe_ceiling: usize,
    /// Seconds between periodic concurrency re-probes. Default: 300 (5 min).
    /// Also readable at runtime from system_meta key 'concurrency_probe_interval_secs'.
    pub probe_interval_secs: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://multiclaw:multiclaw_pass@localhost:5432/multiclaw".to_string());
        let master_key_path = env::var("MASTER_KEY_PATH").unwrap_or_else(|_| "/tmp/master.key".to_string());
        let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string()).parse()?;
        let ollama_url = env::var("OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
        let host_ip = env::var("HOST_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
        let max_concurrent_ollama = env::var("MULTICLAW_MAX_CONCURRENT_OLLAMA")
            .unwrap_or_else(|_| "4".to_string())
            .parse()
            .unwrap_or(4);
        let probe_ceiling = env::var("MULTICLAW_PROBE_CEILING")
            .unwrap_or_else(|_| "32".to_string())
            .parse()
            .unwrap_or(32);
        let probe_interval_secs = env::var("MULTICLAW_PROBE_INTERVAL_SECS")
            .unwrap_or_else(|_| "300".to_string())
            .parse()
            .unwrap_or(300);

        Ok(Self {
            database_url,
            master_key_path,
            port,
            ollama_url,
            host_ip,
            max_concurrent_ollama,
            probe_ceiling,
            probe_interval_secs,
        })
    }
}
