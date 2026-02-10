use anyhow::{Context, Result};
use reqwest::Client;
use std::env;
use futures_util::StreamExt;

#[derive(Debug, Clone)]
pub struct AnthropicClient {
    client: Client,
    api_key: String,
    model: String,
}

use crate::llm::{LlmClient, CompletionParams};

#[async_trait::async_trait]
impl LlmClient for AnthropicClient {
    #[tracing::instrument(skip(self, system, messages, tools, params), fields(model = %self.model))]
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

        // Debug: log the request body (always at debug level; full dump with DEBUG_PAYLOAD=true)
        if env::var("DEBUG_PAYLOAD").map(|v| v == "true").unwrap_or(false) {
            tracing::info!("Anthropic request: {}", serde_json::to_string_pretty(&request_body).unwrap_or_default());
        } else if tracing::enabled!(tracing::Level::DEBUG) {
            // At least log tool definitions so we can diagnose schema issues
            let tools_json = serde_json::to_string(&request_body.tools).unwrap_or_default();
            tracing::debug!("Anthropic tools payload ({}): {}", request_body.tools.len(), tools_json);
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

    async fn stream_complete(
        &self,
        system: &str,
        messages: Vec<crate::api_types::Message>,
        tools: Vec<crate::api_types::Tool>,
        params: CompletionParams,
    ) -> Result<tokio::sync::mpsc::Receiver<crate::api_types::StreamEvent>> {
        use crate::api_types::StreamEvent;

        if self.api_key == "mock" {
            let (tx, rx) = tokio::sync::mpsc::channel(32);
            tokio::spawn(async move {
                let _ = tx.send(StreamEvent::TextDelta("(Mock Response) I received your prompt.".into())).await;
                let _ = tx.send(StreamEvent::Done { stop_reason: Some("end_turn".into()) }).await;
            });
            return Ok(rx);
        }

        let base_url = env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
        let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

        let use_legacy = env::var("ANTHROPIC_LEGACY_SYSTEM")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let (system_field, final_messages) = if use_legacy && !system.is_empty() {
            use crate::api_types::{Message, Role, ContentBlock};
            let mut msgs = vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: format!("[System]\n{}", system) }],
            }];
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

        // Build streaming request body (add "stream": true)
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": params.max_tokens,
            "stream": true,
        });
        if let Some(sys) = system_field {
            body["system"] = serde_json::Value::String(sys);
        }
        body["messages"] = serde_json::to_value(&final_messages)?;
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(&tools)?;
        }
        body["temperature"] = serde_json::json!(params.temperature);

        let response = self.client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .context("Failed to send streaming request to Anthropic")?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic streaming error {}: {}", status, err_text);
        }

        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let byte_stream = response.bytes_stream();

        tokio::spawn(async move {
            if let Err(e) = parse_anthropic_sse(byte_stream, &tx).await {
                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
            }
        });

        Ok(rx)
    }
}

impl AnthropicClient {
    pub fn new(model: &str) -> Result<Self> {
        let api_key = env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| "mock".to_string());

        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()?,
            api_key,
            model: model.to_string(),
        })
    }
}

/// Parse Anthropic SSE byte stream into StreamEvents.
///
/// Anthropic SSE event types:
/// - `content_block_start` with type=text → (ignored, text comes via deltas)
/// - `content_block_start` with type=tool_use → `ToolUseStart`
/// - `content_block_delta` with type=text_delta → `TextDelta`
/// - `content_block_delta` with type=input_json_delta → `ToolInputDelta`
/// - `message_stop` → `Done`
/// - `message_delta` with stop_reason → captures stop_reason for Done
pub(crate) async fn parse_anthropic_sse<S>(
    byte_stream: S,
    tx: &tokio::sync::mpsc::Sender<crate::api_types::StreamEvent>,
) -> Result<()>
where
    S: futures_util::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Unpin + Send,
{
    use crate::api_types::StreamEvent;

    let mut stream = byte_stream;
    let mut buffer = String::new();
    let mut stop_reason: Option<String> = None;

    while let Some(chunk_result) = stream.next().await {
        let chunk: bytes::Bytes = chunk_result.context("Error reading SSE chunk")?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete SSE lines
        while let Some(pos) = buffer.find("\n\n") {
            let event_block = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            let mut event_type = String::new();
            let mut event_data = String::new();

            for line in event_block.lines() {
                if let Some(t) = line.strip_prefix("event: ") {
                    event_type = t.trim().to_string();
                } else if let Some(d) = line.strip_prefix("data: ") {
                    event_data = d.to_string();
                }
            }

            if event_data.is_empty() {
                continue;
            }

            match event_type.as_str() {
                "content_block_start" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&event_data) {
                        if let Some(cb) = v.get("content_block") {
                            if cb.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                let id = cb.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let name = cb.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let _ = tx.send(StreamEvent::ToolUseStart { id, name }).await;
                            }
                        }
                    }
                }
                "content_block_delta" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&event_data) {
                        if let Some(delta) = v.get("delta") {
                            let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match delta_type {
                                "text_delta" => {
                                    if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                        let _ = tx.send(StreamEvent::TextDelta(text.to_string())).await;
                                    }
                                }
                                "input_json_delta" => {
                                    if let Some(json) = delta.get("partial_json").and_then(|t| t.as_str()) {
                                        let _ = tx.send(StreamEvent::ToolInputDelta(json.to_string())).await;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "message_delta" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&event_data) {
                        if let Some(d) = v.get("delta") {
                            if let Some(sr) = d.get("stop_reason").and_then(|s| s.as_str()) {
                                stop_reason = Some(sr.to_string());
                            }
                        }
                    }
                }
                "message_stop" => {
                    let _ = tx.send(StreamEvent::Done { stop_reason: stop_reason.take() }).await;
                    return Ok(());
                }
                "error" => {
                    let _ = tx.send(StreamEvent::Error(event_data)).await;
                    return Ok(());
                }
                _ => {} // ping, message_start, content_block_stop, etc.
            }
        }
    }

    // Stream ended without message_stop — send Done anyway
    let _ = tx.send(StreamEvent::Done { stop_reason }).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_types::StreamEvent;

    /// Helper: create a fake byte stream from raw SSE text
    fn fake_stream(data: &str) -> impl futures_util::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Unpin + Send {
        let bytes = bytes::Bytes::from(data.to_string());
        futures_util::stream::iter(vec![Ok(bytes)])
    }

    #[tokio::test]
    async fn test_anthropic_sse_text_delta() {
        let sse = "event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\nevent: message_stop\ndata: {}\n\n";
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        parse_anthropic_sse(fake_stream(sse), &tx).await.unwrap();
        drop(tx);

        let mut texts = Vec::new();
        let mut done = false;
        while let Some(ev) = rx.recv().await {
            match ev {
                StreamEvent::TextDelta(t) => texts.push(t),
                StreamEvent::Done { .. } => { done = true; }
                _ => {}
            }
        }
        assert!(done);
        assert_eq!(texts, vec!["Hello"]);
    }

    #[tokio::test]
    async fn test_anthropic_sse_tool_use() {
        let sse = concat!(
            "event: content_block_start\ndata: {\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"shell\"}}\n\n",
            "event: content_block_delta\ndata: {\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\": \\\"ls\\\"}\"}}\n\n",
            "event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
            "event: message_stop\ndata: {}\n\n",
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        parse_anthropic_sse(fake_stream(sse), &tx).await.unwrap();
        drop(tx);

        let mut got_start = false;
        let mut input_parts = Vec::new();
        let mut stop = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                StreamEvent::ToolUseStart { id, name } => {
                    assert_eq!(id, "t1");
                    assert_eq!(name, "shell");
                    got_start = true;
                }
                StreamEvent::ToolInputDelta(j) => input_parts.push(j),
                StreamEvent::Done { stop_reason } => { stop = stop_reason; }
                _ => {}
            }
        }
        assert!(got_start);
        assert_eq!(input_parts.len(), 2);
        assert_eq!(stop, Some("tool_use".into()));
    }

    #[tokio::test]
    async fn test_anthropic_sse_error_event() {
        let sse = "event: error\ndata: {\"type\":\"overloaded_error\"}\n\n";
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        parse_anthropic_sse(fake_stream(sse), &tx).await.unwrap();
        drop(tx);

        let mut got_error = false;
        while let Some(ev) = rx.recv().await {
            if let StreamEvent::Error(e) = ev {
                assert!(e.contains("overloaded"));
                got_error = true;
            }
        }
        assert!(got_error);
    }
}
