use anyhow::{Context, Result};
use reqwest::Client;
use std::env;

#[derive(Debug, Clone)]
pub struct AnthropicClient {
    client: Client,
    api_key: String,
    model: String,
}

use crate::llm::{LlmClient, CompletionParams};

#[async_trait::async_trait]
impl LlmClient for AnthropicClient {
    async fn complete(
        &self,
        system: &str,
        messages: Vec<crate::api_types::Message>,
        tools: Vec<crate::api_types::Tool>,
        params: CompletionParams,
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

        // Check if we should use legacy system format (system as first user message)
        let use_legacy = env::var("ANTHROPIC_LEGACY_SYSTEM")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let (system_field, final_messages) = if use_legacy && !system.is_empty() {
            // Legacy mode: prepend system as a user message
            use crate::api_types::{Message, Role, ContentBlock};
            let mut msgs = vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: format!("[System]\n{}", system) }],
            }];
            // Add an assistant acknowledgment if first real message is from user
            if messages.first().map(|m| matches!(m.role, Role::User)).unwrap_or(false) {
                msgs.push(Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text { text: "Understood.".to_string() }],
                });
            }
            msgs.extend(messages);
            (None, msgs)
        } else {
            (Some(system.to_string()), messages)
        };

        let request_body = MessagesRequest {
            model: self.model.clone(),
            system: system_field,
            messages: final_messages,
            max_tokens: params.max_tokens,
            temperature: Some(params.temperature),
            tools,
        };

        // Debug: log the request body
        if env::var("DEBUG_PAYLOAD").map(|v| v == "true").unwrap_or(false) {
            tracing::info!("Anthropic request: {}", serde_json::to_string_pretty(&request_body).unwrap_or_default());
        }
        
        tracing::debug!(
            "LLM params: max_tokens={}, temperature={:.2}",
            params.max_tokens, params.temperature
        );

        let retry_config = crate::retry::RetryConfig::default();
        let client = &self.client;
        let api_key = &self.api_key;

        let response = crate::retry::with_retry(&retry_config, "Anthropic", || async {
            let resp = client
                .post(&url)
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&request_body)
                .send()
                .await
                .context("Failed to send request to Anthropic")?;
            Ok(resp)
        }).await?;

        // Log raw response for debugging tool-use issues (visible with RUST_LOG=debug)
        let resp_text = response.text().await?;
        tracing::debug!(
            "Anthropic raw response (first 2000 chars): {}",
            &resp_text[..resp_text.len().min(2000)]
        );
        let api_response: MessagesResponse = serde_json::from_str(&resp_text)
            .context("Failed to parse Anthropic response")?;
        Ok(api_response)
    }
}

impl AnthropicClient {
    pub fn new(model: &str) -> Result<Self> {
        let api_key = env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| "mock".to_string());
        
        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()?,
            api_key,
            model: model.to_string(),
        })
    }
}
