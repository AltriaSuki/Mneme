use crate::api_types::MessagesResponse;
use anyhow::Result;
use async_trait::async_trait;

/// Parameters for LLM completion, modulated by organism state
#[derive(Debug, Clone)]
pub struct CompletionParams {
    /// Maximum tokens to generate (will be clamped to provider limits)
    pub max_tokens: u32,
    /// Sampling temperature (0.0 - 2.0)
    pub temperature: f32,
}

impl Default for CompletionParams {
    fn default() -> Self {
        Self {
            max_tokens: 4096,
            temperature: 0.7,
        }
    }
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a chat completion request with tool definitions and modulated parameters.
    async fn complete(
        &self,
        system: &str,
        messages: Vec<crate::api_types::Message>,
        tools: Vec<crate::api_types::Tool>,
        params: CompletionParams,
    ) -> Result<MessagesResponse>;
}

// Providers available in crate::providers
