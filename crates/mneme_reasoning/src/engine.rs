use mneme_core::{Event, Trigger, TriggerEvaluator, Reasoning, ReasoningOutput, ResponseModality, Psyche, Memory, Emotion};
use mneme_limbic::LimbicSystem;
use mneme_memory::{OrganismCoordinator, LifecycleState, SignalType};
use crate::{prompts::{ContextAssembler, ContextLayers}, llm::{LlmClient, CompletionParams}};
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

    // Layer 3: Shared feed digest cache (written by CLI sync, read during think)
    feed_cache: Arc<tokio::sync::RwLock<String>>,
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
            feed_cache: Arc::new(tokio::sync::RwLock::new(String::new())),
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
            feed_cache: Arc::new(tokio::sync::RwLock::new(String::new())),
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

    /// Update the feed digest cache (called by CLI after sync).
    /// `items` are formatted into a concise digest string.
    pub async fn update_feed_digest(&self, items: &[mneme_core::Content]) {
        let digest = format_feed_digest(items);
        *self.feed_cache.write().await = digest;
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
        
        // === Compute Modulation Vector (the "neuromodulatory" signal) ===
        let modulation = somatic_marker.to_modulation_vector();
        
        tracing::info!(
            "Modulation: max_tokens×{:.2}, temp_delta={:+.2}, context×{:.2}, silence={:.2}",
            modulation.max_tokens_factor,
            modulation.temperature_delta,
            modulation.context_budget_factor,
            modulation.silence_inclination,
        );
        
        // Apply modulation to LLM parameters
        let base_max_tokens: u32 = 4096;
        let base_temperature: f32 = 0.7;
        let completion_params = CompletionParams {
            max_tokens: ((base_max_tokens as f32 * modulation.max_tokens_factor) as u32).max(256),
            temperature: (base_temperature + modulation.temperature_delta).clamp(0.0, 2.0),
        };
        
        // 1. Recall episodes + facts in parallel
        let (episodes, user_facts) = tokio::join!(
            self.memory.recall(input_text),
            self.memory.recall_facts_formatted(input_text),
        );
        let episodes = episodes?;
        let user_facts = user_facts.unwrap_or_default();
        
        // 2. Prepare Tools
        let tools = vec![
            crate::tools::shell_tool(), 
            crate::tools::browser_goto_tool(),
            crate::tools::browser_click_tool(),
            crate::tools::browser_type_tool(),
            crate::tools::browser_screenshot_tool(),
            crate::tools::browser_get_html_tool(),
        ];
        
        // 3. Assemble 6-layer context with modulated budget
        let base_budget: usize = 32_000; // ~8k tokens worth of chars
        let context_budget = (base_budget as f32 * modulation.context_budget_factor) as usize;
        
        let context_layers = ContextLayers {
            user_facts,
            recalled_episodes: episodes,
            feed_digest: self.feed_cache.read().await.clone(),
        };
        
        let system_prompt = ContextAssembler::build_full_system_prompt(
            &self.psyche,
            &somatic_marker,
            &context_layers,
            context_budget,
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
                tools.clone(),
                completion_params.clone(),
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

        // Sanitize output: strip roleplay asterisks and casual markdown
        // This is a code-level defense — we don't rely on the LLM to follow formatting rules.
        if !final_content.is_empty() {
            final_content = sanitize_chat_output(&final_content);
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

                // Memorize the episode
                self.memory.memorize(&content).await?;

                // Background fact extraction: spawn a task so it doesn't delay the response.
                // We extract facts from the exchange and store them in semantic memory.
                {
                    let user_text = content.body.clone();
                    let reply_text = response_text.clone();
                    let memory = self.memory.clone();
                    // We need to call the LLM client, but it's not Arc-shareable.
                    // Instead, extract facts inline (fast: ~500ms with low max_tokens).
                    let facts = crate::extraction::extract_facts(
                        self.client.as_ref(), &user_text, &reply_text
                    ).await;
                    for fact in facts {
                        if let Err(e) = memory.store_fact(
                            &fact.subject, &fact.predicate, &fact.object, fact.confidence
                        ).await {
                            tracing::warn!("Failed to store extracted fact: {}", e);
                        }
                    }
                }

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

/// Post-process LLM output to strip non-human formatting artifacts.
///
/// This is a structural defense: instead of telling the LLM "don't use markdown",
/// we just strip it. Same principle as ModulationVector — constrain structurally,
/// not with instructions.
fn sanitize_chat_output(text: &str) -> String {
    let mut result = text.to_string();
    
    // 1. Strip markdown bold first: **text** → text
    let bold_re = Regex::new(r"\*\*([^*]+)\*\*").unwrap();
    result = bold_re.replace_all(&result, "$1").to_string();
    
    // 2. Strip roleplay asterisks: *action* or *心理描写* → text
    //    Safe to run after bold is already removed
    let roleplay_re = Regex::new(r"\*([^*\n]+)\*").unwrap();
    result = roleplay_re.replace_all(&result, "$1").to_string();
    
    // 3. Strip markdown headers: # text, ## text, etc.
    let header_re = Regex::new(r"(?m)^#{1,6}\s+").unwrap();
    result = header_re.replace_all(&result, "").to_string();
    
    // 4. Strip markdown bullet lists: - text or * text (at line start)
    let bullet_re = Regex::new(r"(?m)^[\-\*]\s+").unwrap();
    result = bullet_re.replace_all(&result, "").to_string();
    
    // 5. Clean up excess whitespace from stripping
    let multi_newline = Regex::new(r"\n{3,}").unwrap();
    result = multi_newline.replace_all(&result, "\n\n").to_string();
    
    result.trim().to_string()
}

/// Format a list of Content items into a concise feed digest for the LLM context.
/// Each item is condensed to one line: "[source] first-line-of-body".
/// Caps at 10 items to stay within budget.
fn format_feed_digest(items: &[mneme_core::Content]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = items.iter().take(10).map(|item| {
        let headline = item.body.lines().next().unwrap_or("(empty)");
        format!("[{}] {}", item.source, headline)
    }).collect();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mneme_core::{Content, Modality};

    fn test_content(source: &str, body: &str) -> Content {
        Content {
            id: uuid::Uuid::nil(),
            source: source.into(),
            author: "Feed".into(),
            body: body.into(),
            timestamp: 0,
            modality: Modality::Text,
        }
    }

    #[test]
    fn test_format_feed_digest_empty() {
        assert_eq!(format_feed_digest(&[]), "");
    }

    #[test]
    fn test_format_feed_digest_basic() {
        let items = vec![
            test_content("rss:tech", "Title: Rust 2024\nLink: http://example.com\nSummary: Great year"),
            test_content("rss:news", "Title: Weather Update"),
        ];
        let digest = format_feed_digest(&items);
        assert_eq!(digest, "[rss:tech] Title: Rust 2024\n[rss:news] Title: Weather Update");
    }

    #[test]
    fn test_format_feed_digest_caps_at_10() {
        let items: Vec<Content> = (0..15).map(|i| {
            test_content(&format!("rss:feed{}", i), &format!("Item {}", i))
        }).collect();
        let digest = format_feed_digest(&items);
        assert_eq!(digest.lines().count(), 10);
    }

    #[test]
    fn test_sanitize_chat_output() {
        assert_eq!(sanitize_chat_output("*叹气*你好"), "叹气你好");
        assert_eq!(sanitize_chat_output("**重要**的事"), "重要的事");
        assert_eq!(sanitize_chat_output("# 标题\n内容"), "标题\n内容");
        assert_eq!(sanitize_chat_output("- 项目一\n- 项目二"), "项目一\n项目二");
    }
}
