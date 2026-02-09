use crate::llm::{LlmClient, CompletionParams};
use crate::api_types::{Message, Tool, MessagesResponse, ContentBlock, Role};
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::env;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct OpenAiClient {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl OpenAiClient {
    pub fn new(model: &str) -> Result<Self> {
        let api_key = env::var("OPENAI_API_KEY").unwrap_or_else(|_| "mock".to_string());
        let base_url = env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string())
            .trim_end_matches('/')
            .to_string();

        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()?,
            api_key,
            base_url,
            model: model.to_string(),
        })
    }
}

#[async_trait::async_trait]
impl LlmClient for OpenAiClient {
    #[tracing::instrument(skip(self, system, messages, tools, params), fields(model = %self.model))]
    async fn complete(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Vec<Tool>,
        params: CompletionParams,
    ) -> Result<MessagesResponse> {
        if self.api_key == "mock" {
            tokio::time::sleep(Duration::from_millis(500)).await;
            return Ok(MessagesResponse {
                content: vec![ContentBlock::Text { 
                    text: "(Mock OpenAI Response) I received your prompt.".to_string() 
                }],
                stop_reason: Some("stop".to_string()),
            });
        }

        // Convert Tools to OpenAI format
        let openai_tools: Vec<Value> = tools.iter().map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema
                }
            })
        }).collect();

        // Convert Messages to OpenAI format
        // OpenAI puts System prompt as the first message with role "system"
        let mut openai_messages = Vec::new();
        openai_messages.push(json!({
            "role": "system",
            "content": system
        }));

        for msg in messages {
            match msg.role {
                Role::User => {
                    // Extract text
                    let final_text = msg.content.iter().filter_map(|b| {
                        match b {
                            ContentBlock::Text { text } => Some(text.clone()),
                            _ => None // OpenAI user msg is text based mostly (for now)
                        }
                    }).collect::<Vec<_>>().join("\n");
                    
                    // Also check for ToolResults (which are role: tool in OpenAI)
                    // NOTE: This logic assumes ToolResults might be mixed in User messages (Mneme internal model).
                    // In strict OpenAI flow, tool results are standalone messages. 
                    // This converter extracts them and ensures they are emitted as distinct named messages.
                    for block in msg.content {
                        match block {
                            ContentBlock::ToolResult { tool_use_id, content, .. } => {
                                openai_messages.push(json!({
                                    "role": "tool",
                                    "tool_call_id": tool_use_id,
                                    "content": content
                                }));
                            },
                            _ => {}
                        }
                    }

                    if !final_text.is_empty() {
                         openai_messages.push(json!({
                            "role": "user",
                            "content": final_text
                        }));
                    }
                },
                Role::Assistant => {
                    // OpenAI Assistant message can have content AND tool_calls
                    let mut text_parts = Vec::new();
                    let mut tool_calls = Vec::new();

                    for block in msg.content {
                        match block {
                            ContentBlock::Text { text } => text_parts.push(text),
                            ContentBlock::ToolUse { id, name, input } => {
                                tool_calls.push(json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": input.to_string() // OpenAI expects stringified JSON
                                    }
                                }));
                            },
                            _ => {}
                        }
                    }
                    
                    let mut msg_obj = json!({"role": "assistant"});
                    if !text_parts.is_empty() {
                        msg_obj["content"] = json!(text_parts.join("\n"));
                    }
                    if !tool_calls.is_empty() {
                        msg_obj["tool_calls"] = json!(tool_calls);
                         // If tools are present, content can be null in strict strict OpenAI, but usually optional
                         if text_parts.is_empty() {
                             msg_obj["content"] = serde_json::Value::Null;
                         }
                    }
                    openai_messages.push(msg_obj);
                }
            }
        }
        
        // Request payload
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
        
        tracing::debug!(
            "LLM params: max_tokens={}, temperature={:.2}",
            params.max_tokens, params.temperature
        );
        
        let retry_config = crate::retry::RetryConfig::default();
        let client = &self.client;
        let api_key = &self.api_key;

        let response = crate::retry::with_retry(&retry_config, "OpenAI", || async {
            let resp = client.post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&payload)
                .send()
                .await
                .context("Failed to send request to OpenAI")?;
            Ok(resp)
        }).await?;
        
        // Parse Response — log raw for debugging tool-use issues
        let resp_json: Value = response.json().await?;
        if tracing::enabled!(tracing::Level::DEBUG) {
            let raw = serde_json::to_string(&resp_json).unwrap_or_default();
            tracing::debug!("OpenAI raw response (first 2000 chars): {}", &raw[..raw.len().min(2000)]);
        }
        let choice = &resp_json["choices"][0];
        let message = &choice["message"];
        let finish_reason = choice["finish_reason"].as_str().map(|s| s.to_string());

        let mut content_blocks = Vec::new();

        // 1. Text Content
        if let Some(content) = message["content"].as_str() {
            if !content.is_empty() {
                content_blocks.push(ContentBlock::Text { text: content.to_string() });
            }
        }

        // 2. Tool Calls
        if let Some(tool_calls) = message["tool_calls"].as_array() {
            for call in tool_calls {
                let id = call["id"].as_str().unwrap_or_default().to_string();
                let func = &call["function"];
                let name = func["name"].as_str().unwrap_or_default().to_string();

                // OpenAI returns arguments as a JSON string, but some compatible APIs
                // (DeepSeek, local models, etc.) may return a parsed JSON object directly.
                let input: Value = if let Some(args_str) = func["arguments"].as_str() {
                    // Standard OpenAI: arguments is a JSON-encoded string
                    serde_json::from_str(args_str).unwrap_or_else(|e| {
                        tracing::warn!("Failed to parse tool arguments string: {e}. Raw: {args_str}");
                        json!({})
                    })
                } else if func["arguments"].is_object() {
                    // Compatible API: arguments is already a JSON object
                    func["arguments"].clone()
                } else {
                    tracing::warn!("Unexpected arguments type for tool '{}': {:?}", name, func["arguments"]);
                    json!({})
                };

                tracing::debug!("Parsed tool_call: name={}, input={}", name, input);
                content_blocks.push(ContentBlock::ToolUse {
                    id,
                    name,
                    input
                });
            }
        }

        Ok(MessagesResponse {
            content: content_blocks,
            stop_reason: finish_reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: simulate parsing tool_calls from a raw OpenAI-style response JSON.
    fn parse_tool_calls_from(resp_json: &Value) -> Vec<ContentBlock> {
        let message = &resp_json["choices"][0]["message"];
        let mut blocks = Vec::new();
        if let Some(tool_calls) = message["tool_calls"].as_array() {
            for call in tool_calls {
                let func = &call["function"];
                let name = func["name"].as_str().unwrap_or_default().to_string();
                let input: Value = if let Some(args_str) = func["arguments"].as_str() {
                    serde_json::from_str(args_str).unwrap_or(json!({}))
                } else if func["arguments"].is_object() {
                    func["arguments"].clone()
                } else {
                    json!({})
                };
                blocks.push(ContentBlock::ToolUse {
                    id: call["id"].as_str().unwrap_or_default().to_string(),
                    name,
                    input,
                });
            }
        }
        blocks
    }

    #[test]
    fn test_parse_arguments_string() {
        // Standard OpenAI: arguments is a JSON string
        let resp = json!({
            "choices": [{ "message": {
                "tool_calls": [{ "id": "tc1", "function": {
                    "name": "shell",
                    "arguments": "{\"command\": \"ls -la\"}"
                }}]
            }}]
        });
        let blocks = parse_tool_calls_from(&resp);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::ToolUse { name, input, .. } = &blocks[0] {
            assert_eq!(name, "shell");
            assert_eq!(input["command"], "ls -la");
        } else {
            panic!("Expected ToolUse");
        }
    }

    #[test]
    fn test_parse_arguments_object() {
        // Compatible API (DeepSeek etc.): arguments is already a JSON object
        let resp = json!({
            "choices": [{ "message": {
                "tool_calls": [{ "id": "tc2", "function": {
                    "name": "shell",
                    "arguments": { "command": "git status" }
                }}]
            }}]
        });
        let blocks = parse_tool_calls_from(&resp);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::ToolUse { name, input, .. } = &blocks[0] {
            assert_eq!(name, "shell");
            assert_eq!(input["command"], "git status");
        } else {
            panic!("Expected ToolUse");
        }
    }

    #[test]
    fn test_parse_arguments_malformed_string() {
        // Malformed JSON string → fallback to {}
        let resp = json!({
            "choices": [{ "message": {
                "tool_calls": [{ "id": "tc3", "function": {
                    "name": "shell",
                    "arguments": "{not valid json"
                }}]
            }}]
        });
        let blocks = parse_tool_calls_from(&resp);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::ToolUse { input, .. } = &blocks[0] {
            assert!(input.as_object().unwrap().is_empty());
        } else {
            panic!("Expected ToolUse");
        }
    }

    #[test]
    fn test_parse_arguments_null() {
        // Arguments is null → fallback to {}
        let resp = json!({
            "choices": [{ "message": {
                "tool_calls": [{ "id": "tc4", "function": {
                    "name": "shell",
                    "arguments": null
                }}]
            }}]
        });
        let blocks = parse_tool_calls_from(&resp);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::ToolUse { input, .. } = &blocks[0] {
            assert!(input.as_object().unwrap().is_empty());
        } else {
            panic!("Expected ToolUse");
        }
    }
}