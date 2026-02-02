use mneme_core::{Event, Reasoning, ReasoningOutput, ResponseModality, Psyche, Memory};
use crate::{prompts::ContextAssembler, anthropic::AnthropicClient};
use anyhow::Result;
use std::sync::Arc;

pub struct ReasoningEngine {
    psyche: Psyche,
    memory: Arc<dyn Memory>,
    client: AnthropicClient,
    history: tokio::sync::Mutex<Vec<String>>, // Simple in-memory history for now
}

impl ReasoningEngine {
    pub fn new(psyche: Psyche, memory: Arc<dyn Memory>, model: &str) -> Result<Self> {
        let client = AnthropicClient::new(model)?;
        Ok(Self {
            psyche,
            memory,
            client,
            history: tokio::sync::Mutex::new(Vec::new()),
        })
    }
}

#[async_trait::async_trait]
impl Reasoning for ReasoningEngine {
    async fn think(&self, event: Event) -> Result<ReasoningOutput> {
        match event {
            Event::UserMessage(content) => {
                // 1. Recall
                let context = self.memory.recall(&content.body).await?;
                
                // 2. Assemble
                let mut history = self.history.lock().await;
                let history_text = history.join("\n");
                
                let prompt = ContextAssembler::build_prompt(
                    &self.psyche,
                    &context,
                    &history_text,
                    &content.body
                );

                // 3. Generate
                let response = self.client.complete(&prompt).await?;

                // 4. Update History (Naive with cap)
                history.push(format!("User: {}", content.body));
                history.push(format!("Assistant: {}", response));
                
                // Keep only last 20 messages to prevent unbounded growth
                if history.len() > 20 {
                    let overflow = history.len() - 20;
                    history.drain(0..overflow);
                }

                // 5. Memorize (Fire and forget, or await)
                self.memory.memorize(&content).await?;
                // In a real system, we'd also extract facts here.

                Ok(ReasoningOutput {
                    content: response,
                    modality: ResponseModality::Text,
                })
            }
            _ => Ok(ReasoningOutput {
                content: "Event not handled yet".to_string(),
                modality: ResponseModality::Text,
            }),
        }
    }
}
