use mneme_core::Psyche;

pub struct ContextAssembler;

impl ContextAssembler {
    pub fn build_prompt(
        psyche: &Psyche,
        recalled_memory: &str,
        conversation_history: &str,
        user_input: &str,
        current_emotion: &mneme_core::Emotion
    ) -> String {
        format!(
            "{}\n\nYou are currently feeling: {}.\n\n== CAPABILITIES ==\nYou have access to the local operating system via the Shell.\nTo execute a command, output it within <cmd> tags: <cmd>ls -la</cmd>\nThe system will execute it and provide the output.\nYou can use this to explore files, run git commands, check system status, etc.\n\n== EMOTIONAL STATE ==\nAlways start your response with an emotional state tag: <emotion>STATE</emotion>, where STATE is one of: Neutral, Happy, Sad, Excited, Calm, Angry, Surprised.\n\nIf the user's message is a casual remark in a group chat not directed at you, or if you have nothing meaningful to add, you may output exactly: [SILENCE]\nThis will cause you to stay silent.\n\n== RECALLED CONTEXT ==\n{}\n\n== CONVERSATION HISTORY ==\n{}\n\n{}",
            psyche.format_context(),
            current_emotion.as_str(),
            recalled_memory,
            conversation_history,
            if user_input.is_empty() { String::new() } else { format!("User: {}", user_input) }
        )
    }
}
