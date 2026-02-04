use crate::api_types::{Message, Tool, MessagesResponse};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a chat completion request with tool definitions.
    async fn complete(
        &self,
        system: &str,
        messages: Vec<crate::api_types::Message>,
        tools: Vec<crate::api_types::Tool>,
    ) -> Result<MessagesResponse>;
}

// Providers availalbe in crate::providers
