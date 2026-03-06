use anyhow::{Context, Result};
use serde_json::Value;

const TOKEN_PATH: &str = "/var/lib/multiclaw/admin.token";
const API_BASE: &str = "http://127.0.0.1:8080/v1";

pub fn api_url(path: &str) -> String {
    format!("{}{}", API_BASE, path)
}

pub fn load_token() -> Result<String> {
    std::fs::read_to_string(TOKEN_PATH)
        .map(|t| t.trim().to_string())
        .context(format!("Could not read admin token from {}", TOKEN_PATH))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub async fn get(path: &str) -> Result<Value> {
    let token = load_token()?;
    let resp = client()
        .get(api_url(path))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .context("API request failed")?;
    let body = resp.json::<Value>().await.context("Failed to parse API response")?;
    Ok(body)
}

pub async fn post(path: &str, body: Option<Value>) -> Result<Value> {
    let token = load_token()?;
    let mut req = client()
        .post(api_url(path))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.context("API request failed")?;
    let result = resp.json::<Value>().await.context("Failed to parse API response")?;
    Ok(result)
}

pub async fn put(path: &str, body: Value) -> Result<Value> {
    let token = load_token()?;
    let resp = client()
        .put(api_url(path))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("API request failed")?;
    let result = resp.json::<Value>().await.context("Failed to parse API response")?;
    Ok(result)
}

/// Check if the API is reachable (no auth needed for /health).
pub async fn health_ok() -> bool {
    match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
        .get(api_url("/health"))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}
