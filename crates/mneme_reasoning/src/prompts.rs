use mneme_core::Psyche;
use mneme_limbic::SomaticMarker;
use crate::api_types::{Message, Role, ContentBlock};

/// Context layers for the 6-layer assembly pipeline.
/// Priority order (1 = highest, never dropped):
///   1. Persona (identity) — always present, never truncated
///   2. User facts (semantic memory) — known facts about the user
///   3. Social feed digest — summarized perception data (TODO)
///   4. Relevant episodes — recalled memories
///   5. Conversation history — sliding window (in messages, not system prompt)
///   6. Triggering event — the current input (in messages, not system prompt)
///   *  Somatic marker — auxiliary numeric state signal (always present)
#[derive(Debug, Default)]
pub struct ContextLayers {
    /// Layer 2: Known facts formatted for prompt
    pub user_facts: String,
    /// Layer 3: Feed digest (not yet implemented, placeholder)
    pub feed_digest: String,
    /// Layer 4: Recalled episodes from vector search
    pub recalled_episodes: String,
}

pub struct ContextAssembler;

impl ContextAssembler {
    /// Build the full 6-layer system prompt.
    ///
    /// **Budget logic**: When total exceeds `budget_chars`, layers are trimmed
    /// in reverse priority (feed_digest first, then episodes, then facts).
    /// Persona and somatic marker are never dropped.
    pub fn build_full_system_prompt(
        psyche: &Psyche,
        somatic_marker: &SomaticMarker,
        layers: &ContextLayers,
        budget_chars: usize,
    ) -> String {
        let soma_context = somatic_marker.format_for_prompt();
        
        // Layer 1: Persona (always present, never trimmed)
        let persona = psyche.format_context();
        
        // Fixed sections (always present)
        let style_guide = format!(
            "== 表达风格指引 ==\n{}\n\n\
             重要：不要在回复中直接描述或提及你的情绪状态、精力水平或心情。\
             让这些自然地体现在你的语气、回复长度和热情程度中，而不是用语言说出来。\n\n\
             == SILENCE RULES ==\n\
             If the user's message is a casual remark in a group chat not directed at you, \
             or if you have nothing meaningful to add, you may output exactly: [SILENCE]",
            soma_context
        );

        let fixed_size = persona.len() + style_guide.len() + 50; // 50 for separators
        let remaining = budget_chars.saturating_sub(fixed_size);

        // Budget allocation for variable layers (priority order for inclusion):
        //   facts > episodes > feed_digest
        let mut variable_sections: Vec<(&str, &str)> = Vec::new();

        if !layers.user_facts.is_empty() {
            variable_sections.push(("KNOWN FACTS", &layers.user_facts));
        }
        if !layers.recalled_episodes.is_empty() {
            variable_sections.push(("RECALLED MEMORIES", &layers.recalled_episodes));
        }
        if !layers.feed_digest.is_empty() {
            variable_sections.push(("SOCIAL FEED DIGEST", &layers.feed_digest));
        }

        // Fit variable sections within remaining budget
        let mut variable_parts = Vec::new();
        let mut used = 0;
        for (label, content) in &variable_sections {
            let section = format!("== {} ==\n{}", label, content);
            if used + section.len() <= remaining {
                used += section.len();
                variable_parts.push(section);
            } else {
                // Try to fit a truncated version
                let avail = remaining.saturating_sub(used);
                if avail > 80 {
                    let truncated: String = content.chars().take(avail - 40).collect();
                    variable_parts.push(format!("== {} (truncated) ==\n{}…", label, truncated));
                }
                break; // No more budget
            }
        }

        let variable_text = variable_parts.join("\n\n");

        if variable_text.is_empty() {
            format!("{}\n\n{}", persona, style_guide)
        } else {
            format!("{}\n\n{}\n\n{}", persona, style_guide, variable_text)
        }
    }

    /// Build system prompt with somatic marker injection (new System 1 integration)
    /// Legacy API — delegates to the full pipeline with no facts/feed.
    pub fn build_system_prompt_with_soma(
        psyche: &Psyche,
        recalled_memory: &str,
        somatic_marker: &SomaticMarker,
    ) -> String {
        let layers = ContextLayers {
            recalled_episodes: recalled_memory.to_string(),
            ..Default::default()
        };
        Self::build_full_system_prompt(psyche, somatic_marker, &layers, 32_000)
    }

    /// Legacy: Build system prompt with discrete emotion (backward compatibility)
    pub fn build_system_prompt(
        psyche: &Psyche,
        recalled_memory: &str,
        current_emotion: &mneme_core::Emotion
    ) -> String {
        format!(
            "{}\n\nYou are currently feeling: {}.\n\n== EMOTIONAL STATE ==\nAlways start your response with an emotional state tag: <emotion>STATE</emotion>, where STATE is one of: Neutral, Happy, Sad, Excited, Calm, Angry, Surprised.\n\nIf the user's message is a casual remark in a group chat not directed at you, or if you have nothing meaningful to add, you may output exactly: [SILENCE]\nThis will cause you to stay silent.\n\n== RECALLED CONTEXT ==\n{}",
            psyche.format_context(),
            current_emotion.as_str(),
            recalled_memory
        )
    }

    pub fn assemble_history(
        raw_history: &[Message],
        user_input: &str
    ) -> Vec<Message> {
        let mut messages = raw_history.to_vec();
        
        if !user_input.is_empty() {
            messages.push(Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: user_input.to_string() }]
            });
        }
        
        messages
    }
}
