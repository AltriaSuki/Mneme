use anyhow::{Context, Result};
use reqwest::Client;
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

    pub async fn complete_with_tools(
        &self,
        system: &str,
        messages: Vec<crate::api_types::Message>,
        tools: Vec<crate::api_types::Tool>,
    ) -> Result<crate::api_types::MessagesResponse> {
        use crate::api_types::{MessagesRequest, ContentBlock, MessagesResponse};

        if self.api_key == "mock" {
            // Mock delay to simulate network
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            return Ok(MessagesResponse {
                content: vec![ContentBlock::Text { 
                    text: "(Mock Response) I received your prompt.".to_string() 
                }],
                stop_reason: Some("end_turn".to_string()),
            });
        }

        let base_url = env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
        // Handle trailing slash just in case
        let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

        let request_body = MessagesRequest {
            model: self.model.clone(),
            system: system.to_string(),
            messages,
            max_tokens: 4096, // Increased as requested
            tools,
        };

        let response = self.client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to Anthropic")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API Error: {}", error_text);
        }

        let api_response: MessagesResponse = response.json().await?;
        Ok(api_response)
    }
}
