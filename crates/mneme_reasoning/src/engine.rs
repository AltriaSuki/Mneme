use mneme_core::{Event, Trigger, TriggerEvaluator, Reasoning, ReasoningOutput, ResponseModality, Psyche, Memory, Emotion};
use crate::{prompts::ContextAssembler, anthropic::AnthropicClient};
use anyhow::Result;
use std::sync::Arc;
use regex::Regex;

use mneme_os::Executor;

pub struct ReasoningEngine {
    psyche: Psyche,
    memory: Arc<dyn Memory>,
    client: AnthropicClient,
    history: tokio::sync::Mutex<Vec<String>>,
    current_emotion: tokio::sync::Mutex<Emotion>,
    evaluators: Vec<Box<dyn TriggerEvaluator>>,
    emotion_regex: Regex,
    cmd_regex: Regex,
    executor: Arc<dyn Executor>,
}

impl ReasoningEngine {
    pub fn new(psyche: Psyche, memory: Arc<dyn Memory>, model: &str, executor: Arc<dyn Executor>) -> Result<Self> {
        let client = AnthropicClient::new(model)?;
        Ok(Self {
            psyche,
            memory,
            client,
            history: tokio::sync::Mutex::new(Vec::new()),
            current_emotion: tokio::sync::Mutex::new(Emotion::Neutral),
            evaluators: Vec::new(),
            emotion_regex: Regex::new(r"(?i)<emotion>(.*?)</emotion>").expect("Invalid regex"),
            cmd_regex: Regex::new(r"(?s)<cmd>(.*?)</cmd>").expect("Invalid regex"),
            executor,
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
        
        // Lock history to append initial input
        {
            let mut history = self.history.lock().await;

            // Add initial input to history
            if is_user_message {
                history.push(format!("User: {}", input_text));
            } else {
                // For system triggers, we treat them as "System Event" but let's just format as text
                history.push(format!("System Event: {}", input_text));
            }
        } // Drop history lock here

        let mut final_content = String::new();
        let mut final_emotion = Emotion::Neutral;
        
        // --- React Loop (Max 5 turns) ---
        for _iteration in 0..5 {
            let emotion_lock = self.current_emotion.lock().await;
            
            // Re-acquire history lock briefly to read
            let history_text = self.history.lock().await.join("\n");
            
            // Build prompt. Notice we pass empty string for input because it's already in history_text
            let prompt = ContextAssembler::build_prompt(
                &self.psyche,
                &context,
                &history_text,
                "", 
                &*emotion_lock
            );
            drop(emotion_lock); // Release emotion lock before network call

            // Generate
            let raw_response = self.client.complete(&prompt).await?;
            tracing::debug!("LLM Output: {}", raw_response);

            // Parse Emotion
            let (content, emotion) = if let Some(caps) = self.emotion_regex.captures(&raw_response) {
                let emotion_str = caps.get(1).map_or("", |m| m.as_str());
                let parsed_emotion = Emotion::from_str(emotion_str).unwrap_or(Emotion::Neutral);
                let clean_content = self.emotion_regex.replace(&raw_response, "").trim().to_string();
                (clean_content, parsed_emotion)
            } else {
                (raw_response.clone(), Emotion::Neutral)
            };
            
            // Update state immediately
            *self.current_emotion.lock().await = emotion.clone();
            final_content = content.clone();
            final_emotion = emotion;

            // Silence check
            if content.trim() == "[SILENCE]" {
                let mut history = self.history.lock().await;
                history.push("Assistant: [SILENCE]".to_string());
                return Ok((String::new(), final_emotion));
            }
            
            // Append assistant response to history
             {
                let mut history = self.history.lock().await;
                history.push(format!("Assistant: {}", content));
            }
            
            // TOOL CHECK: Check for <cmd>...<cmd>
            if let Some(caps) = self.cmd_regex.captures(&content) {
                let command = caps.get(1).map_or("", |m| m.as_str());
                tracing::info!("Agent detected command execution: {}", command);
                
                // Execute command
                let output = match self.executor.execute(command).await {
                    Ok(out) => out,
                    Err(e) => format!("Error: {}", e),
                };
                
                // Truncate output if too long to prevent context explosion
                let truncated_output = if output.len() > 1000 {
                    format!("{} ... (truncated)", &output[..1000])
                } else {
                    output
                };
                
                tracing::info!("Command Output: {}", truncated_output);
                {
                    let mut history = self.history.lock().await;
                    history.push(format!("System Output: {}", truncated_output));
                }
                
                // Continue loop to let LLM react to output
                continue;
            }

            // No tool used, break loop
            break;
        }
        
        // Prune history
        {
            let mut history = self.history.lock().await;
            if history.len() > 20 {
                let overflow = history.len() - 20;
                history.drain(0..overflow);
            }
        }

        Ok((final_content, final_emotion))
    }
}
// ABORTING REPLACEMENT to rethink the loop structure.
// The previous logic was too messy. I need to implement a clean ReAct loop.
// 1. Receive User Input.
// 2. Append to History.
// 3. Loop:
//    a. Construct Prompt (History so far).
//    b. LLM Complete.
//    c. Parse Output.
//    d. If <cmd>: Execute -> Append "System: Output" to History -> Continue Loop.
//    e. Else: Break and Return.

// I need to update `prompts.rs` first to handle "History Only" mode or just be cleaner.
// Currently `build_prompt` takes `user_input` and enforces `User: {user_input}` at the end.
// If I want to support a loop where the "User" (System) has just spoken, I need `build_prompt` to be flexible.

// Let's look at `prompts.rs` again.


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
                    emotion, // Explicit emotion field
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
                    emotion,
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
