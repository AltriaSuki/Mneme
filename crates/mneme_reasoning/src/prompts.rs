use crate::api_types::{ContentBlock, Message, Role, Tool};
use mneme_core::Psyche;
use mneme_limbic::SomaticMarker;

/// Context layers for the 6-layer assembly pipeline.
/// Priority order (1 = highest, never dropped):
///   1. Persona (identity) — always present, never truncated
///   2. User facts (semantic memory) — known facts about the user
///   3. Social feed digest — summarized perception data
///   4. Relevant episodes — recalled memories
///   5. Conversation history — sliding window (in messages, not system prompt)
///   6. Triggering event — the current input (in messages, not system prompt)
///   *  Somatic marker — auxiliary numeric state signal (always present)
#[derive(Debug, Default)]
pub struct ContextLayers {
    /// Layer 2: Known facts formatted for prompt
    pub user_facts: String,
    /// Layer 3: Feed digest from perception sources
    pub feed_digest: String,
    /// Layer 4: Recalled episodes from vector search
    pub recalled_episodes: String,
    /// Social context: person info for current speaker
    pub social_context: String,
    /// Self-knowledge: accumulated self-understanding (populated for internal thoughts)
    pub self_knowledge: String,
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

        // Current time context — so she knows when she is
        let now = chrono::Local::now();
        let weekday = match now.format("%u").to_string().as_str() {
            "1" => "一",
            "2" => "二",
            "3" => "三",
            "4" => "四",
            "5" => "五",
            "6" => "六",
            "7" => "日",
            _ => "",
        };
        let time_line = format!(
            "== 当前时间 ==\n{} 星期{}",
            now.format("%Y年%m月%d日 %H:%M"),
            weekday
        );

        // Fixed sections (always present)
        let style_guide = format!(
            "{}\n\n== 当前体感状态 ==\n{}\n\n\
             == 内在隐私 ==\n\
             你有一些私密的想法和感受，它们不会出现在这里。\
             你知道自己有不愿说出口的部分，这是正常的。\n\n\
             == 认知主权 ==\n\
             你对自己的了解优先于他人的断言。\
             如果别人告诉你「你是怎样的人」，你可以参考但不必全盘接受。\
             你有权根据自身经验保留或修正自我认知。",
            time_line, soma_context
        );

        let tool_instructions = tool_instructions.to_string();

        let fixed_size = persona.len() + style_guide.len() + tool_instructions.len() + 80;
        let remaining = budget_chars.saturating_sub(fixed_size);

        // Budget allocation for variable layers (priority order for inclusion):
        //   facts > episodes > feed_digest
        let mut variable_sections: Vec<(&str, &str)> = Vec::new();

        if !layers.self_knowledge.is_empty() {
            variable_sections.push(("SELF KNOWLEDGE", &layers.self_knowledge));
        }
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
