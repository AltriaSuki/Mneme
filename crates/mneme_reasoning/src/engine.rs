use mneme_core::{Event, Trigger, TriggerEvaluator, Reasoning, ReasoningOutput, ResponseModality, Psyche, Memory, Emotion};
use crate::{prompts::ContextAssembler, llm::LlmClient};
use anyhow::Result;
use std::sync::Arc;
use regex::Regex;

use mneme_os::Executor;

pub struct ReasoningEngine {
    psyche: Psyche,
    memory: Arc<dyn Memory>,
    client: Box<dyn LlmClient>, // Dynamic dispatch
    history: tokio::sync::Mutex<Vec<crate::api_types::Message>>,
    current_emotion: tokio::sync::Mutex<Emotion>,
    evaluators: Vec<Box<dyn TriggerEvaluator>>,
    emotion_regex: Regex,
    executor: Arc<dyn Executor>,
    
    // Phase 3: Web Automation
    browser_session: tokio::sync::Mutex<Option<mneme_browser::BrowserClient>>,
}

impl ReasoningEngine {
    pub fn new(psyche: Psyche, memory: Arc<dyn Memory>, client: Box<dyn LlmClient>, executor: Arc<dyn Executor>) -> Result<Self> {
        Ok(Self {
            psyche,
            memory,
            client,
            history: tokio::sync::Mutex::new(Vec::new()),
            current_emotion: tokio::sync::Mutex::new(Emotion::Neutral),
            evaluators: Vec::new(),
            emotion_regex: Regex::new(r"(?i)<emotion>(.*?)</emotion>").expect("Invalid regex"),
            executor,
            browser_session: tokio::sync::Mutex::new(None), 
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

    async fn process_thought_loop(&self, input_text: &str, is_user_message: bool) -> Result<(String, Emotion)> {
        use crate::api_types::{Message, Role, ContentBlock};
        
        // 1. Recall
        let context = self.memory.recall(input_text).await?;
        
        // 2. Prepare Tools
        let tools = vec![
            crate::tools::shell_tool(), 
            // We reference separate tools now, assuming tools.rs is updated
            crate::tools::browser_goto_tool(),
            crate::tools::browser_click_tool(),
            crate::tools::browser_type_tool(),
            crate::tools::browser_screenshot_tool(),
            crate::tools::browser_get_html_tool(),
        ];
        
        // 3. Prepare System Prompt & History
        let emotion_lock = self.current_emotion.lock().await;
        let system_prompt = ContextAssembler::build_system_prompt(&self.psyche, &context, &*emotion_lock);
        drop(emotion_lock);

        // Current messages serves as the "Scratchpad" for the ReAct loop
        let mut scratchpad_messages = {
            let history_lock = self.history.lock().await;
            ContextAssembler::assemble_history(&*history_lock, input_text)
        };
        
        let mut final_content = String::new();
        let mut final_emotion = Emotion::Neutral;
        
        // --- React Loop (Max 5 turns) ---
        for _iteration in 0..5 {
            // Reset final content for this turn - we only want the last turn's text for the user
            final_content.clear();
            
            // Call API
            let response = self.client.complete(
                &system_prompt,
                scratchpad_messages.clone(),
                tools.clone()
            ).await?;

            // Append Assistant Response to scratchpad
            let assistant_msg = Message {
                role: Role::Assistant,
                content: response.content.clone(),
            };
            scratchpad_messages.push(assistant_msg);

            // Process Content Blocks
            let mut tool_results = Vec::new();

            for block in &response.content {
                match block {
                    ContentBlock::Text { text } => {
                        tracing::debug!("LLM Text: {}", text);
                        
                        // Parse Emotion from text (legacy regex support)
                         if let Some(caps) = self.emotion_regex.captures(text) {
                            let emotion_str = caps.get(1).map_or("", |m| m.as_str());
                            final_emotion = Emotion::from_str(emotion_str).unwrap_or(Emotion::Neutral);
                            let clean_content = self.emotion_regex.replace(text, "").trim().to_string();
                            final_content.push_str(&clean_content);
                        } else {
                            final_content.push_str(text);
                        }
                    },
                    ContentBlock::ToolUse { id, name, input } => {
                        tracing::info!("Tool Use: {} input: {:?}", name, input);
                        let result_content = self.execute_tool(name, input).await;
                        
                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: result_content,
                            is_error: None, 
                        });
                    },
                    _ => {}
                }
            }
            
            // Update Emotion State
            *self.current_emotion.lock().await = final_emotion.clone();

            // Check if we need to continue loop (if tools were used)
            if !tool_results.is_empty() {
                // Return tool results as a user message
                let tool_msg = Message {
                    role: Role::User,
                    content: tool_results,
                };
                scratchpad_messages.push(tool_msg);
                continue; // Loop again
            } else {
                // No tools used, we are done
                break;
            }
        }
        
        // Silence Check
        if final_content.trim() == "[SILENCE]" {
            final_content.clear();
        }

        // Save history (Compressed: Logic is History + UserInput + FinalResponse)
        // We drop intermediate tool steps.
        {
            let mut history = self.history.lock().await;
            
            // Add Input (User or System)
            if !input_text.is_empty() {
                let content = if is_user_message {
                    input_text.to_string()
                } else {
                    format!("[System Event]: {}", input_text)
                };
                
                history.push(Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text { text: content }]
                });
            }
            
            // Add Assistant Response (Only if not silent)
            if !final_content.is_empty() {
                history.push(Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text { text: final_content.clone() }]
                });
            }
            
            // Prune: Keep max 20, AND ensure history starts with User
            if history.len() > 20 {
                let overflow = history.len() - 20;
                history.drain(0..overflow);
            }
            
            // Validate start with User (Anthropic Requirement)
            while !history.is_empty() {
                if matches!(history[0].role, Role::Assistant) {
                    history.remove(0);
                } else {
                    break;
                }
            }
        }

        Ok((final_content.trim().to_string(), final_emotion))
    }
    
    async fn execute_tool(&self, name: &str, input: &serde_json::Value) -> String {
        match name {
            "shell" => {
                if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                     match self.executor.execute(cmd).await {
                        Ok(out) => out,
                        Err(e) => format!("Error: {}", e),
                    }
                } else {
                    "Missing 'command' parameter".to_string()
                }
            },
            "browser_goto" | "browser_click" | "browser_type" | "browser_screenshot" | "browser_get_html" => {
                 use mneme_browser::{BrowserClient, BrowserAction};
                 
                 // Map tool usage to BrowserAction
                 let action_result = match name {
                     "browser_goto" => input.get("url").and_then(|u| u.as_str())
                         .map(|url| BrowserAction::Goto { url: url.to_string() })
                         .ok_or("Missing 'url'"),
                     "browser_click" => input.get("selector").and_then(|s| s.as_str())
                         .map(|sel| BrowserAction::Click { selector: sel.to_string() })
                         .ok_or("Missing 'selector'"),
                     "browser_type" => {
                         let sel = input.get("selector").and_then(|s| s.as_str());
                         let txt = input.get("text").and_then(|t| t.as_str());
                         match (sel, txt) {
                             (Some(s), Some(t)) => Ok(BrowserAction::Type { selector: s.to_string(), text: t.to_string() }),
                             _ => Err("Missing 'selector' or 'text'")
                         }
                     },
                     "browser_screenshot" => Ok(BrowserAction::Screenshot),
                     "browser_get_html" => Ok(BrowserAction::GetHtml),
                     _ => Err("Unreachable"),
                 };
                 
                 match action_result {
                     Ok(action) => {
                         // Get or Init Session
                        let mut session_lock = self.browser_session.lock().await;
                        if session_lock.is_none() {
                             match BrowserClient::new(true) {
                                 Ok(mut client) => {
                                     if let Err(e) = client.launch() {
                                         return format!("Failed to launch browser: {}", e);
                                     }
                                     *session_lock = Some(client);
                                 },
                                 Err(e) => return format!("Failed to init browser: {}", e),
                             }
                        }
                        
                        if let Some(client) = session_lock.as_mut() {
                            match client.execute_action(action) {
                                Ok(out) => out,
                                Err(e) => format!("Browser Action Failed: {}", e),
                            }
                        } else {
                            "Browser Session Lost".to_string()
                        }
                     },
                     Err(e) => format!("Invalid Input for {}: {}", name, e),
                 }
            },
            _ => format!("Unknown Tool: {}", name),
        }
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
