use mneme_core::{Event, Trigger, TriggerEvaluator, Reasoning, ReasoningOutput, ResponseModality, Psyche, Memory};
use crate::{prompts::ContextAssembler, anthropic::AnthropicClient};
use anyhow::Result;
use std::sync::Arc;

pub struct ReasoningEngine {
    psyche: Psyche,
    memory: Arc<dyn Memory>,
    client: AnthropicClient,
    history: tokio::sync::Mutex<Vec<String>>, // Simple in-memory history for now
    evaluators: Vec<Box<dyn TriggerEvaluator>>,
}

impl ReasoningEngine {
    pub fn new(psyche: Psyche, memory: Arc<dyn Memory>, model: &str) -> Result<Self> {
        let client = AnthropicClient::new(model)?;
        Ok(Self {
            psyche,
            memory,
            client,
            history: tokio::sync::Mutex::new(Vec::new()),
            evaluators: Vec::new(),
        })
    }

    /// Register a trigger evaluator
    pub fn add_evaluator(&mut self, evaluator: Box<dyn TriggerEvaluator>) {
        self.evaluators.push(evaluator);
    }

    /// Evaluate all registered trigger sources (resilient to individual failures)
    pub async fn evaluate_triggers(&self) -> Result<Vec<Trigger>> {
        let mut triggers = Vec::new();
        for evaluator in &self.evaluators {
            match evaluator.evaluate().await {
                Ok(found) => triggers.extend(found),
                Err(e) => tracing::error!("Evaluator {} failed: {}", evaluator.name(), e),
            }
        }
        Ok(triggers)
    }

    /// Helper to process thought loop for any input text
    async fn process_thought_loop(&self, input_text: &str, is_user_message: bool) -> Result<String> {
        // 1. Recall
        let context = self.memory.recall(input_text).await?;
        
        // 2. Assemble
        let mut history = self.history.lock().await;
        let history_text = history.join("\n");
        
        let prompt = ContextAssembler::build_prompt(
            &self.psyche,
            &context,
            &history_text,
            input_text
        );

        // 3. Generate
        let response = self.client.complete(&prompt).await?;

        // 4. Update History
        if is_user_message {
            history.push(format!("User: {}", input_text));
        } else {
            // For system/proactive events, record them differently
            history.push(format!("System Event: {}", input_text));
        }
        history.push(format!("Assistant: {}", response));
        
        if history.len() > 20 {
            let overflow = history.len() - 20;
            history.drain(0..overflow);
        }

        Ok(response)
    }
}

#[async_trait::async_trait]
impl Reasoning for ReasoningEngine {
    async fn think(&self, event: Event) -> Result<ReasoningOutput> {
        match event {
            Event::UserMessage(content) => {
                // Refactored to use shared helper
                let response = self.process_thought_loop(&content.body, true).await?;

                // 5. Memorize
                self.memory.memorize(&content).await?;

                Ok(ReasoningOutput {
                    content: response,
                    modality: ResponseModality::Text,
                })
            }
            Event::ProactiveTrigger(trigger) => {
                // Synthesize a prompt for the agent based on the trigger
                let prompt_text = match trigger {
                    Trigger::Scheduled { name, .. } => 
                        format!("It is time for the {}. Please initiate this interaction.", name),
                    Trigger::ContentRelevance { reason, .. } =>
                        format!("Relevant content found: {}. Please share this with the user.", reason),
                    Trigger::MemoryDecay { topic, .. } =>
                        format!("You haven't discussed '{}' in a while. Bring it up naturally.", topic),
                    Trigger::Trending { topic, .. } =>
                        format!("'{}' is trending. Mention it if relevant.", topic),
                };

                let response = self.process_thought_loop(&prompt_text, false).await?;

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
