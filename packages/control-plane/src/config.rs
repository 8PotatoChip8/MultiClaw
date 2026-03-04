use anyhow::Result;
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub admin_token: String,
    pub master_key_path: String,
    pub port: u16,
    pub ollama_url: String,
    pub host_ip: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://multiclaw:multiclaw_pass@localhost:5432/multiclaw".to_string());
        let admin_token = env::var("ADMIN_TOKEN").unwrap_or_else(|_| "dev_token_dummy".to_string());
        let master_key_path = env::var("MASTER_KEY_PATH").unwrap_or_else(|_| "/tmp/master.key".to_string());
        let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string()).parse()?;
        let ollama_url = env::var("OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
        let host_ip = env::var("HOST_IP").unwrap_or_else(|_| "127.0.0.1".to_string());

        Ok(Self {
            database_url,
            admin_token,
            master_key_path,
            port,
            ollama_url,
            host_ip,
        })
    }
}
