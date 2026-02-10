use crate::api_types::{MessagesResponse, StreamEvent, ContentBlock};
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

    /// Stream a completion, returning a channel receiver of StreamEvents.
    /// Default implementation falls back to non-streaming complete().
    async fn stream_complete(
        &self,
        system: &str,
        messages: Vec<crate::api_types::Message>,
        tools: Vec<crate::api_types::Tool>,
        params: CompletionParams,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamEvent>> {
        let response = self.complete(system, messages, tools, params).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            for block in response.content {
                match block {
                    ContentBlock::Text { text } => {
                        let _ = tx.send(StreamEvent::TextDelta(text)).await;
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        let _ = tx.send(StreamEvent::ToolUseStart {
                            id,
                            name,
                        }).await;
                        let _ = tx.send(StreamEvent::ToolInputDelta(
                            input.to_string(),
                        )).await;
                    }
                    _ => {}
                }
            }
            let _ = tx.send(StreamEvent::Done {
                stop_reason: response.stop_reason,
            }).await;
        });
        Ok(rx)
    }
}

// Providers available in crate::providers
