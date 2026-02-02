use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::env;

#[derive(Debug, Clone)]
pub struct AnthropicClient {
    client: Client,
    api_key: String,
    model: String,
}

impl AnthropicClient {
    pub fn new(model: &str) -> Result<Self> {
        let api_key = env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| "mock".to_string());
        
        Ok(Self {
            client: Client::new(),
            api_key,
            model: model.to_string(),
        })
    }

    pub async fn complete(&self, prompt: &str) -> Result<String> {
        if self.api_key == "mock" {
            // Mock delay to simulate network
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            return Ok(format!("(Mock Response) I received your prompt of length {}.", prompt.len()));
        }

        let base_url = env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
        // Handle trailing slash just in case
        let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

        let response = self.client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": self.model,
                "max_tokens": 1024,
                "messages": [
                    {"role": "user", "content": prompt}
                ]
            }))
            .send()
            .await
            .context("Failed to send request to Anthropic")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API Error: {}", error_text);
        }

        let json: serde_json::Value = response.json().await?;
        let content = json["content"][0]["text"]
            .as_str()
            .context("Failed to parse response content")?
            .to_string();

        Ok(content)
    }
}
