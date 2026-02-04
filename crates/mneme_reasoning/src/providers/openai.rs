use crate::llm::LlmClient;
use crate::api_types::{Message, Tool, MessagesResponse, ContentBlock, Role};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Serialize, Deserialize};
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
    async fn complete(
        &self,
        system: &str,
        messages: Vec<Message>,
        tools: Vec<Tool>,
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
            "temperature": 0.7,
        });
        
        if !openai_tools.is_empty() {
            payload["tools"] = json!(openai_tools);
        }

        let url = format!("{}/chat/completions", self.base_url);
        
        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&payload)
            .send()
            .await
            .context("Failed to send request to OpenAI")?;

        if !response.status().is_success() {
             let error_text = response.text().await.unwrap_or_default();
             anyhow::bail!("OpenAI API Error: {}", error_text);
        }
        
        // Parse Response
        let resp_json: Value = response.json().await?;
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
                let args_str = func["arguments"].as_str().unwrap_or("{}");
                
                let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                
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
