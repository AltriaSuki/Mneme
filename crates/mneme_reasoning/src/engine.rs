use mneme_core::{Event, Trigger, TriggerEvaluator, Reasoning, ReasoningOutput, ResponseModality, Psyche, Memory, Emotion};
use crate::{prompts::ContextAssembler, anthropic::AnthropicClient};
use anyhow::Result;
use std::sync::Arc;
use regex::Regex;

pub struct ReasoningEngine {
    psyche: Psyche,
    memory: Arc<dyn Memory>,
    client: AnthropicClient,
    history: tokio::sync::Mutex<Vec<String>>, // Simple in-memory history for now
    current_emotion: tokio::sync::Mutex<Emotion>,
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
            current_emotion: tokio::sync::Mutex::new(Emotion::Neutral),
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

    /// Helper to process thought loop for any input text, returns (content, emotion)
    async fn process_thought_loop(&self, input_text: &str, is_user_message: bool) -> Result<(String, Emotion)> {
        // 1. Recall
        let context = self.memory.recall(input_text).await?;
        
        // 2. Assemble
        let mut history = self.history.lock().await;
        let mut emotion_lock = self.current_emotion.lock().await;
        
        let history_text = history.join("\n");
        
        // Update prompt builder to use current emotion
        let prompt = ContextAssembler::build_prompt(
            &self.psyche,
            &context,
            &history_text,
            input_text,
            &*emotion_lock
        );

        // 3. Generate
        let raw_response = self.client.complete(&prompt).await?;

        // 4. Parse Emotion from Response
        // 4. Parse Emotion from Response using Regex
        // Format: <emotion>Happy</emotion> Content...
        // Regex is safer for flexible whitespace or malformed tags
        let re = Regex::new(r"(?i)<emotion>(.*?)</emotion>").unwrap();
        
        let (content, emotion) = if let Some(caps) = re.captures(&raw_response) {
            let emotion_str = caps.get(1).map_or("", |m| m.as_str());
            let parsed_emotion = Emotion::from_str(emotion_str).unwrap_or(Emotion::Neutral);
            
            // Strip the tag from the content
            let clean_content = re.replace(&raw_response, "").trim().to_string();
            (clean_content, parsed_emotion)
        } else {
            (raw_response.clone(), Emotion::Neutral)
        };

        // Update state
        *emotion_lock = emotion;

        // 5. Update History (store the raw response with emotion tag internally or just content? 
        // Let's store content to keep history clean for context, but maybe we lose emotional context in history. 
        // For now, let's just store the content to avoid confusing the model with its own tags if we re-feed it unsystematically)
        if is_user_message {
            history.push(format!("User: {}", input_text));
        } else {
            history.push(format!("System Event: {}", input_text));
        }
        history.push(format!("Assistant: {}", content));
        
        if history.len() > 20 {
            let overflow = history.len() - 20;
            history.drain(0..overflow);
        }

        Ok((content, emotion))
    }
}

#[async_trait::async_trait]
impl Reasoning for ReasoningEngine {
    async fn think(&self, event: Event) -> Result<ReasoningOutput> {
        match event {
            Event::UserMessage(content) => {

                // Refactored to use shared helper
                let (response_text, emotion) = self.process_thought_loop(&content.body, true).await?;

                // 5. Memorize
                self.memory.memorize(&content).await?;

                Ok(ReasoningOutput {
                    content: response_text,
                    modality: ResponseModality::Text, // Standard text output
                    emotion: emotion, // Explicit emotion field
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

                let (response_text, emotion) = self.process_thought_loop(&prompt_text, false).await?;

                Ok(ReasoningOutput {
                    content: response_text,
                    modality: ResponseModality::Text,
                    emotion: emotion,
                })
            }
            _ => Ok(ReasoningOutput {
                content: "Event not handled yet".to_string(),
                modality: ResponseModality::Text,
                emotion: Emotion::Neutral,
            }),
        }
    }
}
