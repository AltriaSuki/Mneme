//! Context assembly — extracts context-gathering logic from ReasoningEngine.
//!
//! `ContextBuilder` gathers all layers (recall, social, self-knowledge, resources,
//! tools) and delegates to `ContextAssembler` for the final 6-layer system prompt.

use crate::api_types::Tool;
use crate::prompts::{ContextAssembler, ContextLayers};
use anyhow::Result;
use mneme_core::{Memory, Psyche, SocialGraph};
use mneme_limbic::{ModulationVector, SomaticMarker};
use std::sync::Arc;

/// Result of context assembly — everything the LLM call needs.
pub struct AssembledContext {
    pub system_prompt: String,
    pub api_tools: Vec<Tool>,
}

/// Gathers all context layers and builds the system prompt.
///
/// Extracted from `ReasoningEngine` to separate context assembly from
/// the ReAct loop and LLM orchestration.
pub struct ContextBuilder<'a> {
    psyche: &'a Psyche,
    memory: &'a Arc<dyn Memory>,
    feed_cache: &'a Arc<tokio::sync::RwLock<String>>,
    social_graph: &'a Option<Arc<dyn SocialGraph>>,
    token_budget: &'a Option<Arc<crate::token_budget::TokenBudget>>,
    registry: &'a Option<Arc<tokio::sync::RwLock<crate::tool_registry::ToolRegistry>>>,
    context_budget_chars: usize,
    start_time: std::time::Instant,
}

impl<'a> ContextBuilder<'a> {
    pub fn new(
        psyche: &'a Psyche,
        memory: &'a Arc<dyn Memory>,
        feed_cache: &'a Arc<tokio::sync::RwLock<String>>,
        social_graph: &'a Option<Arc<dyn SocialGraph>>,
        token_budget: &'a Option<Arc<crate::token_budget::TokenBudget>>,
        registry: &'a Option<Arc<tokio::sync::RwLock<crate::tool_registry::ToolRegistry>>>,
        context_budget_chars: usize,
        start_time: std::time::Instant,
    ) -> Self {
        Self {
            psyche,
            memory,
            feed_cache,
            social_graph,
            token_budget,
            registry,
            context_budget_chars,
            start_time,
        }
    }

    /// Build the full assembled context for an LLM call.
    ///
    /// Gathers recalled episodes, facts, social context, self-knowledge,
    /// resource status, and tool definitions, then assembles the 6-layer
    /// system prompt with modulated budget.
    pub async fn build(
        &self,
        input_text: &str,
        speaker: Option<(&str, &str)>,
        somatic_marker: &SomaticMarker,
        modulation: &ModulationVector,
        top_interests: &[(&str, f32)],
        is_user_message: bool,
    ) -> Result<AssembledContext> {
        // Curiosity context for prompt
        let curiosity_context = if top_interests.is_empty() {
            String::new()
        } else {
            top_interests
                .iter()
                .map(|(topic, intensity)| format!("- {} ({:.0}%)", topic, intensity * 100.0))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // 1. Blended recall: episodes + facts
        //    ADR-007: Augment query with top curiosity interests to bias retrieval
        let stress = somatic_marker.stress;
        let recall_query = if top_interests.is_empty() {
            input_text.to_string()
        } else {
            let curiosity_suffix: String = top_interests
                .iter()
                .take(2)
                .map(|(t, _)| *t)
                .collect::<Vec<_>>()
                .join(" ");
            format!("{} {}", input_text, curiosity_suffix)
        };
        let recalled_episodes = self
            .memory
            .recall_reconstructed(&recall_query, modulation.recall_mood_bias, stress)
            .await?;
        let facts = self
            .memory
            .recall_facts_formatted(input_text)
            .await
            .unwrap_or_default();

        // 1b. Social graph: look up person context for the current speaker
        let social_context = if let Some((source, author)) = speaker {
            self.lookup_social_context(source, author).await
        } else {
            String::new()
        };

        // 1c. Self-knowledge for internal thoughts (#71, #72, #77)
        let self_knowledge = if !is_user_message {
            self.recall_self_knowledge_for_prompt().await
        } else {
            String::new()
        };

        // 1d. Resource status (#80)
        let resource_status = self.build_resource_status().await;

        // 2. Tool definitions — passed via API native tool_use (ADR-014)
        let api_tools = if let Some(ref registry) = self.registry {
            registry.read().await.available_tools()
        } else {
            vec![]
        };
        // Tool output honesty guard — only injected when tools are available
        let tool_instructions = if !api_tools.is_empty() {
            format_tool_honesty_guard(self.psyche.language.as_str())
        } else {
            String::new()
        };

        // 3. Assemble 6-layer context with modulated budget
        let context_budget =
            (self.context_budget_chars as f32 * modulation.context_budget_factor) as usize;

        let context_layers = ContextLayers {
            user_facts: facts,
            recalled_episodes,
            feed_digest: self.feed_cache.read().await.clone(),
            social_context,
            self_knowledge,
            resource_status,
            curiosity_context,
        };

        let system_prompt = ContextAssembler::build_full_system_prompt(
            self.psyche,
            somatic_marker,
            &context_layers,
            context_budget,
            &tool_instructions,
        );

        Ok(AssembledContext {
            system_prompt,
            api_tools,
        })
    }

    /// Look up social context for the current speaker.
    async fn lookup_social_context(&self, source: &str, author: &str) -> String {
        let graph = match self.social_graph {
            Some(g) => g,
            None => return String::new(),
        };

        let platform = source.split(':').next().unwrap_or(source);

        let person = match graph.find_person(platform, author).await {
            Ok(Some(p)) => p,
            Ok(None) => return String::new(),
            Err(e) => {
                tracing::debug!("Social graph lookup failed: {}", e);
                return String::new();
            }
        };

        let person_id = person.id;
        match graph.get_person_context(person_id).await {
            Ok(Some(ctx)) => {
                let mut parts = vec![format!("说话人: {}", ctx.person.name)];
                if ctx.interaction_count > 0 {
                    parts.push(format!("互动次数: {}", ctx.interaction_count));
                }
                if !ctx.relationship_notes.is_empty() {
                    parts.push(format!("关系备注: {}", ctx.relationship_notes));
                }
                parts.join("\n")
            }
            Ok(None) => String::new(),
            Err(e) => {
                tracing::debug!("Failed to get person context: {}", e);
                String::new()
            }
        }
    }

    /// Recall self-knowledge entries formatted for system prompt injection.
    async fn recall_self_knowledge_for_prompt(&self) -> String {
        let domains = [
            "behavior",
            "emotion",
            "social",
            "expression",
            "body_feeling",
            "infrastructure",
        ];
        let mut lines = Vec::new();
        for domain in &domains {
            let entries = self
                .memory
                .recall_self_knowledge_by_domain(domain)
                .await
                .unwrap_or_default();
            for (content, confidence) in entries.iter().take(3) {
                lines.push(format!(
                    "[{}] {} ({:.0}%)",
                    domain,
                    content,
                    confidence * 100.0
                ));
            }
        }
        lines.join("\n")
    }

    /// Build resource status string for prompt injection (#80).
    async fn build_resource_status(&self) -> String {
        let mut parts = Vec::new();

        // Uptime
        let uptime = self.start_time.elapsed();
        let hours = uptime.as_secs() / 3600;
        let mins = (uptime.as_secs() % 3600) / 60;
        if hours > 0 {
            parts.push(format!("运行时间: {}小时{}分钟", hours, mins));
        } else {
            parts.push(format!("运行时间: {}分钟", mins));
        }

        // Episode count
        if let Ok(count) = self.memory.episode_count().await {
            parts.push(format!("记忆片段数: {}", count));
        }

        // Token budget
        if let Some(ref budget) = self.token_budget {
            let daily = budget.get_daily_usage().await;
            let monthly = budget.get_monthly_usage().await;
            let mut token_line = format!("今日token: {}", daily);
            if monthly > daily {
                token_line.push_str(&format!(", 本月: {}", monthly));
            }
            if let Some(daily_limit) = budget.config().daily_limit {
                token_line.push_str(&format!(" / 日限额{}", daily_limit));
            }
            parts.push(token_line);
        }

        parts.join("\n")
    }
}

/// Tool output honesty guard — prevents LLM from fabricating tool result details.
fn format_tool_honesty_guard(lang: &str) -> String {
    match lang {
        "en" => "== Tool Output Honesty ==\n\
            When using tools: only state information that actually appears in the tool result.\n\
            If a result is truncated, empty, or unclear, say so honestly. Never fabricate details."
            .to_string(),
        _ => "== 工具输出诚实性 ==\n\
            使用工具时：只陈述工具结果中实际出现的信息。\n\
            如果结果被截断、为空或不明确，请如实说明。绝不捏造细节。"
            .to_string(),
    }
}
