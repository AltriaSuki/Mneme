use crate::api_types::{ContentBlock, Message, Role, Tool};
use mneme_core::Psyche;
use mneme_limbic::SomaticMarker;

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
    /// Social context: person info for current speaker
    pub social_context: String,
}

pub struct ContextAssembler;

impl ContextAssembler {
    /// Build the full 6-layer system prompt.
    ///
    /// **Budget logic**: When total exceeds `budget_chars`, layers are trimmed
    /// in reverse priority (feed_digest first, then episodes, then facts).
    /// Persona and somatic marker are never dropped.
    /// Build the full 6-layer system prompt.
    ///
    pub fn build_full_system_prompt(
        psyche: &Psyche,
        somatic_marker: &SomaticMarker,
        layers: &ContextLayers,
        budget_chars: usize,
        tool_instructions: &str,
    ) -> String {
        let soma_context = somatic_marker.format_for_prompt();

        // Layer 1: Persona (always present, never trimmed)
        let persona = psyche.format_context();

        // Fixed sections (always present)
        // B-1: Expression style emerges from experience, not hardcoded rules.
        // We only provide the somatic context as a signal, not prescriptive formatting rules.
        let style_guide = format!("== 当前体感状态 ==\n{}", soma_context);

        let tool_instructions = tool_instructions.to_string();

        let fixed_size = persona.len() + style_guide.len() + tool_instructions.len() + 80;
        let remaining = budget_chars.saturating_sub(fixed_size);

        // Budget allocation for variable layers (priority order for inclusion):
        //   facts > episodes > feed_digest
        let mut variable_sections: Vec<(&str, &str)> = Vec::new();

        if !layers.user_facts.is_empty() {
            variable_sections.push(("KNOWN FACTS", &layers.user_facts));
        }
        if !layers.social_context.is_empty() {
            variable_sections.push(("SOCIAL CONTEXT", &layers.social_context));
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

        // Assemble final prompt — only append tool_instructions if non-empty
        let mut parts = vec![persona, style_guide];
        if !variable_text.is_empty() {
            parts.push(variable_text);
        }
        if !tool_instructions.is_empty() {
            parts.push(tool_instructions.to_string());
        }
        parts.join("\n\n")
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
        Self::build_full_system_prompt(psyche, somatic_marker, &layers, 32_000, "")
    }

    /// Legacy: Build system prompt with discrete emotion (backward compatibility)
    pub fn build_system_prompt(
        psyche: &Psyche,
        recalled_memory: &str,
        current_emotion: &mneme_core::Emotion,
    ) -> String {
        format!(
            "{}\n\nYou are currently feeling: {}.\n\n== EMOTIONAL STATE ==\nAlways start your response with an emotional state tag: <emotion>STATE</emotion>, where STATE is one of: Neutral, Happy, Sad, Excited, Calm, Angry, Surprised.\n\nIf the user's message is a casual remark in a group chat not directed at you, or if you have nothing meaningful to add, you may output exactly: [SILENCE]\nThis will cause you to stay silent.\n\n== RECALLED CONTEXT ==\n{}",
            psyche.format_context(),
            current_emotion.as_str(),
            recalled_memory
        )
    }

    pub fn assemble_history(raw_history: &[Message], user_input: &str) -> Vec<Message> {
        let mut messages = raw_history.to_vec();

        if !user_input.is_empty() {
            messages.push(Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: user_input.to_string(),
                }],
            });
        }

        messages
    }
}

/// Dynamically generate text-mode tool instructions from Tool schemas.
///
/// Produces a system prompt section that describes each tool's name, description,
/// required parameters, and the expected `<tool_call>` output format.
pub fn generate_text_tool_instructions(tools: &[Tool]) -> String {
    let mut lines = vec![
        "== SYSTEM TOOLS ==".to_string(),
        "The following tools are available regardless of current persona or cognitive stage.\n\
         Tool call format is specified below."
            .to_string(),
        String::new(),
        "AVAILABLE TOOLS:".to_string(),
    ];

    for (i, tool) in tools.iter().enumerate() {
        lines.push(String::new());
        lines.push(format!("{}. {} — {}", i + 1, tool.name, tool.description));

        if !tool.input_schema.required.is_empty() {
            // Build example input from schema properties
            let example = build_example_input(&tool.input_schema);
            lines.push(format!("   Input: {}", example));
        } else {
            lines.push("   No parameters required.".to_string());
        }
    }

    lines.push(String::new());
    lines.push(
        "Tool call format:\n\
         <tool_call>{\"name\": \"tool_name\", \"arguments\": {params}}</tool_call>\n\n\
         Example:\n\
         <tool_call>{\"name\": \"shell\", \"arguments\": {\"command\": \"ls -la\"}}</tool_call>\n\n\
         Note: tool calls require all specified parameters. Empty arguments {} are invalid."
            .to_string(),
    );

    lines.join("\n")
}

/// Build an example JSON input string from a ToolInputSchema.
fn build_example_input(schema: &crate::api_types::ToolInputSchema) -> String {
    let mut parts = Vec::new();
    if let Some(props) = schema.properties.as_object() {
        for key in &schema.required {
            let desc = props
                .get(key)
                .and_then(|v| v.get("description"))
                .and_then(|d| d.as_str())
                .unwrap_or("...");
            parts.push(format!("\"{}\": \"<{}>\"", key, desc));
        }
    }
    format!("{{{}}}", parts.join(", "))
}
