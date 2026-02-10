//! Ollama LLM Provider (#12, v0.6.0)
//!
//! Ollama exposes an OpenAI-compatible API at localhost:11434/v1,
//! so we reuse the OpenAI SSE parsing logic.

use crate::llm::{LlmClient, CompletionParams};
use crate::api_types::{Message, Tool, MessagesResponse, ContentBlock, Role, StreamEvent};
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::env;
use std::time::Duration;
#[derive(Debug, Clone)]
pub struct OllamaClient {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaClient {
    pub fn new(model: &str) -> Result<Self> {
        let base_url = env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1".to_string())
            .trim_end_matches('/')
            .to_string();

        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()?,
            base_url,
            model: model.to_string(),
        })
    }
}

/// Convert messages to OpenAI-compatible format (shared with OpenAI provider).
fn build_openai_messages(system: &str, messages: Vec<Message>) -> Vec<Value> {
    let mut openai_messages = vec![json!({"role": "system", "content": system})];

    for msg in messages {
        match msg.role {
            Role::User => {
                let final_text = msg.content.iter().filter_map(|b| {
                    if let ContentBlock::Text { text } = b { Some(text.clone()) } else { None }
                }).collect::<Vec<_>>().join("\n");

                for block in &msg.content {
                    if let ContentBlock::ToolResult { tool_use_id, content, .. } = block {
                        openai_messages.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content
                        }));
                    }
                }
                if !final_text.is_empty() {
                    openai_messages.push(json!({"role": "user", "content": final_text}));
                }
            }
            Role::Assistant => {
                let mut text_parts = Vec::new();
                let mut tool_calls = Vec::new();
                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => text_parts.push(text.clone()),
                        ContentBlock::ToolUse { id, name, input } => {
                            tool_calls.push(json!({
                                "id": id, "type": "function",
                                "function": {"name": name, "arguments": input.to_string()}
                            }));
                        }
                        _ => {}
                    }
                }
                let mut msg_obj = json!({"role": "assistant"});
                if !text_parts.is_empty() {
                    msg_obj["content"] = json!(text_parts.join("\n"));
                }
                if !tool_calls.is_empty() {
                    msg_obj["tool_calls"] = json!(tool_calls);
                    if text_parts.is_empty() { msg_obj["content"] = Value::Null; }
                }
                openai_messages.push(msg_obj);
            }
        }
    }
    openai_messages
}

fn build_openai_tools(tools: &[Tool]) -> Vec<Value> {
    tools.iter().map(|t| {
        json!({
            "type": "function",
            "function": {
                "name": t.name,
                "description": t.description,
                "parameters": t.input_schema
            }
        })
    }).collect()
}

#[async_trait::async_trait]
impl LlmClient for OllamaClient {
    async fn complete(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Vec<Tool>,
        params: CompletionParams,
    ) -> Result<MessagesResponse> {
        let openai_messages = build_openai_messages(system, messages);
        let openai_tools = build_openai_tools(&tools);

        let mut payload = json!({
            "model": self.model,
            "messages": openai_messages,
            "temperature": params.temperature,
            "max_tokens": params.max_tokens,
        });
        if !openai_tools.is_empty() {
            payload["tools"] = json!(openai_tools);
        }

        let url = format!("{}/chat/completions", self.base_url);

        let response = self.client.post(&url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send request to Ollama")?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama error {}: {}", status, err_text);
        }

        let resp_json: Value = response.json().await?;
        parse_openai_response(&resp_json)
    }

    async fn stream_complete(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Vec<Tool>,
        params: CompletionParams,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamEvent>> {
        let openai_messages = build_openai_messages(system, messages);
        let openai_tools = build_openai_tools(&tools);

        let mut payload = json!({
            "model": self.model,
            "messages": openai_messages,
            "temperature": params.temperature,
            "max_tokens": params.max_tokens,
            "stream": true,
        });
        if !openai_tools.is_empty() {
            payload["tools"] = json!(openai_tools);
        }

        let url = format!("{}/chat/completions", self.base_url);

        let response = self.client.post(&url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send streaming request to Ollama")?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama streaming error {}: {}", status, err_text);
        }

        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let byte_stream = response.bytes_stream();

        tokio::spawn(async move {
            if let Err(e) = super::openai::parse_openai_sse(byte_stream, &tx).await {
                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
            }
        });

        Ok(rx)
    }
}

/// Parse a non-streaming OpenAI-compatible JSON response into MessagesResponse.
pub(crate) fn parse_openai_response(resp_json: &Value) -> Result<MessagesResponse> {
    let choice = &resp_json["choices"][0];
    let message = &choice["message"];
    let finish_reason = choice["finish_reason"].as_str().map(|s| s.to_string());

    let mut content_blocks = Vec::new();

    if let Some(content) = message["content"].as_str() {
        if !content.is_empty() {
            content_blocks.push(ContentBlock::Text { text: content.to_string() });
        }
    }

    if let Some(tool_calls) = message["tool_calls"].as_array() {
        for call in tool_calls {
            let id = call["id"].as_str().unwrap_or_default().to_string();
            let func = &call["function"];
            let name = func["name"].as_str().unwrap_or_default().to_string();
            let input: Value = if let Some(args_str) = func["arguments"].as_str() {
                serde_json::from_str(args_str).unwrap_or(json!({}))
            } else if func["arguments"].is_object() {
                func["arguments"].clone()
            } else {
                json!({})
            };
            content_blocks.push(ContentBlock::ToolUse { id, name, input });
        }
    }

    Ok(MessagesResponse {
        content: content_blocks,
        stop_reason: finish_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_types::{ContentBlock, Message, Role, Tool};
    use serde_json::json;

    #[test]
    fn test_ollama_client_creation() {
        let client = OllamaClient::new("llama3").unwrap();
        assert_eq!(client.model, "llama3");
        assert!(client.base_url.contains("11434"));
    }

    #[test]
    fn test_parse_text_response() {
        let resp = json!({
            "choices": [{
                "message": { "content": "Hello from Ollama!" },
                "finish_reason": "stop"
            }]
        });
        let result = parse_openai_response(&resp).unwrap();
        assert_eq!(result.content.len(), 1);
        if let ContentBlock::Text { text } = &result.content[0] {
            assert_eq!(text, "Hello from Ollama!");
        } else {
            panic!("Expected Text block");
        }
        assert_eq!(result.stop_reason, Some("stop".into()));
    }

    #[test]
    fn test_parse_tool_call_response() {
        let resp = json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {
                            "name": "shell",
                            "arguments": "{\"command\": \"ls\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });
        let result = parse_openai_response(&resp).unwrap();
        assert_eq!(result.content.len(), 1);
        if let ContentBlock::ToolUse { id, name, input } = &result.content[0] {
            assert_eq!(id, "call_1");
            assert_eq!(name, "shell");
            assert_eq!(input["command"], "ls");
        } else {
            panic!("Expected ToolUse block");
        }
    }

    #[test]
    fn test_parse_empty_content() {
        let resp = json!({
            "choices": [{
                "message": { "content": "" },
                "finish_reason": "stop"
            }]
        });
        let result = parse_openai_response(&resp).unwrap();
        assert!(result.content.is_empty());
    }

    #[test]
    fn test_build_messages_user_and_assistant() {
        let messages = vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "Hi".into() }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text { text: "Hello!".into() }],
            },
        ];
        let built = build_openai_messages("You are helpful.", messages);
        assert_eq!(built.len(), 3);
        assert_eq!(built[0]["role"], "system");
        assert_eq!(built[1]["role"], "user");
        assert_eq!(built[1]["content"], "Hi");
        assert_eq!(built[2]["role"], "assistant");
        assert_eq!(built[2]["content"], "Hello!");
    }

    #[test]
    fn test_build_tools() {
        use crate::api_types::ToolInputSchema;
        let tools = vec![Tool {
            name: "shell".into(),
            description: "Run a shell command".into(),
            input_schema: ToolInputSchema {
                schema_type: "object".into(),
                properties: json!({"command": {"type": "string"}}),
                required: vec!["command".into()],
            },
        }];
        let built = build_openai_tools(&tools);
        assert_eq!(built.len(), 1);
        assert_eq!(built[0]["type"], "function");
        assert_eq!(built[0]["function"]["name"], "shell");
    }
}
