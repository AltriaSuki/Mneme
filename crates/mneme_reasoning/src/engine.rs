use mneme_core::{Event, Trigger, TriggerEvaluator, Reasoning, ReasoningOutput, ResponseModality, Psyche, Memory, Emotion};
use mneme_limbic::LimbicSystem;
use mneme_memory::{OrganismCoordinator, LifecycleState, SignalType};
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
    evaluators: Vec<Box<dyn TriggerEvaluator>>,
    emotion_regex: Regex,
    executor: Arc<dyn Executor>,
    
    // System 1: Limbic System (new organic architecture)
    limbic: Arc<LimbicSystem>,
    
    // Organism Coordinator (integrates all subsystems)
    coordinator: Arc<OrganismCoordinator>,
    
    // Phase 3: Web Automation
    browser_session: tokio::sync::Mutex<Option<mneme_browser::BrowserClient>>,
}

impl ReasoningEngine {
    pub fn new(psyche: Psyche, memory: Arc<dyn Memory>, client: Box<dyn LlmClient>, executor: Arc<dyn Executor>) -> Self {
        let limbic = Arc::new(LimbicSystem::new());
        let coordinator = Arc::new(OrganismCoordinator::new(limbic.clone()));
        
        Self {
            psyche,
            memory,
            client,
            history: tokio::sync::Mutex::new(Vec::new()),
            evaluators: Vec::new(),
            emotion_regex: Regex::new(r"(?i)<emotion>(.*?)</emotion>").expect("Invalid regex"),
            executor,
            limbic,
            coordinator,
            browser_session: tokio::sync::Mutex::new(None), 
        }
    }

    /// Create with custom limbic system and coordinator
    pub fn with_coordinator(
        psyche: Psyche, 
        memory: Arc<dyn Memory>, 
        client: Box<dyn LlmClient>, 
        executor: Arc<dyn Executor>,
        coordinator: Arc<OrganismCoordinator>,
    ) -> Self {
        let limbic = coordinator.limbic().clone();
        Self {
            psyche,
            memory,
            client,
            history: tokio::sync::Mutex::new(Vec::new()),
            evaluators: Vec::new(),
            emotion_regex: Regex::new(r"(?i)<emotion>(.*?)</emotion>").expect("Invalid regex"),
            executor,
            limbic,
            coordinator,
            browser_session: tokio::sync::Mutex::new(None), 
        }
    }

    /// Get reference to the limbic system
    pub fn limbic(&self) -> &Arc<LimbicSystem> {
        &self.limbic
    }
    
    /// Get reference to the organism coordinator
    pub fn coordinator(&self) -> &Arc<OrganismCoordinator> {
        &self.coordinator
    }

    /// Register a trigger evaluator
    pub fn add_evaluator(&mut self, evaluator: Box<dyn TriggerEvaluator>) {
        self.evaluators.push(evaluator);
    }

    /// Check if proactive messaging should be triggered based on limbic state
    pub async fn should_initiate_contact(&self) -> bool {
        let marker = self.limbic.get_somatic_marker().await;
        marker.proactivity_urgency() > 0.6
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

    /// Analyze input for emotional content (simplified sentiment analysis)
    /// Reserved for future use when direct stimulus creation is needed
    #[allow(dead_code)]
    fn analyze_input_sentiment(&self, text: &str) -> (f32, f32) {
        let lower = text.to_lowercase();
        
        // Simple keyword-based sentiment (in production, use ML model)
        let positive_words = ["开心", "高兴", "喜欢", "爱", "棒", "好", "谢谢", "感谢", "哈哈", "😊", "❤️", "👍"];
        let negative_words = ["难过", "伤心", "讨厌", "恨", "糟糕", "差", "烦", "气", "怒", "😢", "😡", "💔"];
        let intense_words = ["非常", "特别", "超级", "极其", "太", "!", "！", "?!", "？！"];
        
        let pos_count = positive_words.iter().filter(|w| lower.contains(*w)).count() as f32;
        let neg_count = negative_words.iter().filter(|w| lower.contains(*w)).count() as f32;
        let intense_count = intense_words.iter().filter(|w| lower.contains(*w)).count() as f32;
        
        let total = pos_count + neg_count;
        let valence = if total > 0.0 {
            (pos_count - neg_count) / total
        } else {
            0.0
        };
        
        let intensity = ((total + intense_count) / 5.0).min(1.0);
        
        (valence * 0.8, intensity.max(0.2)) // Moderate the valence, ensure some intensity
    }

    async fn process_thought_loop(&self, input_text: &str, is_user_message: bool) -> Result<(String, Emotion, mneme_core::Affect)> {
        use crate::api_types::{Message, Role, ContentBlock};
        
        // Check lifecycle state - if sleeping, don't process
        if self.coordinator.lifecycle_state().await == LifecycleState::Sleeping {
            tracing::debug!("Organism is sleeping, deferring interaction");
            return Ok(("[正在休息中...]".to_string(), Emotion::Calm, mneme_core::Affect::default()));
        }
        
        // === Process through OrganismCoordinator ===
        // This handles System 1 (limbic) and state updates
        let interaction_result = if is_user_message {
            self.coordinator.process_interaction(
                "user",
                input_text,
                1.0, // Normal response delay
            ).await?
        } else {
            // For system events, just get current somatic marker
            mneme_memory::InteractionResult {
                somatic_marker: self.limbic.get_somatic_marker().await,
                state_snapshot: self.coordinator.state().read().await.clone(),
                lifecycle: self.coordinator.lifecycle_state().await,
            }
        };
        
        let somatic_marker = interaction_result.somatic_marker;
        
        // 1. Recall
        let context = self.memory.recall(input_text).await?;
        
        // 2. Prepare Tools
        let tools = vec![
            crate::tools::shell_tool(), 
            crate::tools::browser_goto_tool(),
            crate::tools::browser_click_tool(),
            crate::tools::browser_type_tool(),
            crate::tools::browser_screenshot_tool(),
            crate::tools::browser_get_html_tool(),
        ];
        
        // 3. Prepare System Prompt with Somatic Marker (System 1 → System 2)
        let system_prompt = ContextAssembler::build_system_prompt_with_soma(
            &self.psyche, 
            &context, 
            &somatic_marker
        );

        // Current messages serves as the "Scratchpad" for the ReAct loop
        let mut scratchpad_messages = {
            let history_lock = self.history.lock().await;
            ContextAssembler::assemble_history(&*history_lock, input_text)
        };
        
        let mut final_content = String::new();
        let mut final_emotion = Emotion::from_affect(&somatic_marker.affect);
        
        // --- React Loop (Max 5 turns) ---
        for _iteration in 0..5 {
            final_content.clear();
            
            let response = self.client.complete(
                &system_prompt,
                scratchpad_messages.clone(),
                tools.clone()
            ).await?;

            let assistant_msg = Message {
                role: Role::Assistant,
                content: response.content.clone(),
            };
            scratchpad_messages.push(assistant_msg);

            let mut tool_results = Vec::new();

            for block in &response.content {
                match block {
                    ContentBlock::Text { text } => {
                        tracing::debug!("LLM Text: {}", text);
                        
                        // Parse Emotion from text (legacy regex support, but prefer Affect)
                        if let Some(caps) = self.emotion_regex.captures(text) {
                            let emotion_str = caps.get(1).map_or("", |m| m.as_str());
                            final_emotion = Emotion::from_str(emotion_str).unwrap_or(final_emotion);
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

            if !tool_results.is_empty() {
                let tool_msg = Message {
                    role: Role::User,
                    content: tool_results,
                };
                scratchpad_messages.push(tool_msg);
                continue;
            } else {
                break;
            }
        }
        
        // Silence Check
        if final_content.trim() == "[SILENCE]" {
            final_content.clear();
        }

        // Save history
        {
            let mut history = self.history.lock().await;
            
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
            
            if !final_content.is_empty() {
                history.push(Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text { text: final_content.clone() }]
                });
            }
            
            // Prune
            if history.len() > 20 {
                let overflow = history.len() - 20;
                history.drain(0..overflow);
            }
            
            while !history.is_empty() {
                if matches!(history[0].role, Role::Assistant) {
                    history.remove(0);
                } else {
                    break;
                }
            }
        }

        // Get final affect from limbic system
        let final_affect = self.limbic.get_affect().await;

        // === Record feedback for later consolidation ===
        // Only record if we actually produced a response
        if !final_content.is_empty() && is_user_message {
            // Record self-reflection about our response
            self.coordinator.record_feedback(
                SignalType::SituationInterpretation,
                format!("对「{}」的回应：{}", 
                    input_text.chars().take(50).collect::<String>(),
                    final_content.chars().take(100).collect::<String>()),
                0.7, // Moderate confidence
                final_affect.valence,
            ).await;
        }

        Ok((final_content.trim().to_string(), final_emotion, final_affect))
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
                let (response_text, emotion, affect) = self.process_thought_loop(&content.body, true).await?;

                // Memorize
                self.memory.memorize(&content).await?;

                Ok(ReasoningOutput {
                    content: response_text,
                    modality: ResponseModality::Text,
                    emotion,
                    affect,
                })
            }
            Event::ProactiveTrigger(trigger) => {
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

                let (response_text, emotion, affect) = self.process_thought_loop(&prompt_text, false).await?;

                Ok(ReasoningOutput {
                    content: response_text,
                    modality: ResponseModality::Text,
                    emotion,
                    affect,
                })
            }
            _ => {
                let affect = self.limbic.get_affect().await;
                Ok(ReasoningOutput {
                    content: "Event not handled yet".to_string(),
                    modality: ResponseModality::Text,
                    emotion: Emotion::from_affect(&affect),
                    affect,
                })
            }
        }
    }
}
