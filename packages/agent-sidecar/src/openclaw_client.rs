use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use futures::StreamExt;

pub struct OpenClawClient {
    client: Client,
    url: String,
    token: String,
}

impl OpenClawClient {
    pub fn new(url: String, token: String) -> Self {
        Self {
            client: Client::new(),
            url,
            token,
        }
    }

    /// Sends a prompt to OpenClaw and streams the response deltas to the provided callback
    pub async fn chat_completion<F>(&self, prompt: &str, thread_id: &str, mut on_delta: F) -> Result<String>
    where
        F: FnMut(String) + Send + Sync,
    {
        let body = serde_json::json!({
            "model": "openclaw:main",
            "user": format!("multiclaw-thread:{}", thread_id),
            "stream": true,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let mut res = self.client.post(&format!("{}/v1/chat/completions", self.url))
            .bearer_auth(&self.token)
            .header("x-openclaw-agent-id", "main")
            .json(&body)
            .send()
            .await?;
        
        if !res.status().is_success() {
            let err = res.text().await?;
            return Err(anyhow::anyhow!("OpenClaw error: {}", err));
        }

        let mut final_content = String::new();
        let mut stream = res.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            let text = String::from_utf8_lossy(&bytes);
            
            for line in text.lines() {
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        continue;
                    }
                    if let Ok(json) = serde_json::from_str::<Value>(data) {
                        if let Some(delta) = json["choices"][0]["delta"]["content"].as_str() {
                            final_content.push_str(delta);
                            on_delta(delta.to_string());
                        }
                    }
                }
            }
        }

        Ok(final_content)
    }
}
