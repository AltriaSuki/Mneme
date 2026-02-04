use mneme_core::Psyche;
use crate::api_types::{Message, Role, ContentBlock};

pub struct ContextAssembler;

impl ContextAssembler {
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
