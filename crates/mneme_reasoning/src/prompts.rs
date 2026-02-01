use mneme_core::Psyche;

pub struct ContextAssembler;

impl ContextAssembler {
    pub fn build_prompt(
        psyche: &Psyche,
        recalled_memory: &str,
        conversation_history: &str,
        user_input: &str
    ) -> String {
        format!(
            "{}\n\n== RECALLED CONTEXT ==\n{}\n\n== CONVERSATION HISTORY ==\n{}\n\nUser: {}",
            psyche.format_context(),
            recalled_memory,
            conversation_history,
            user_input
        )
    }
}
