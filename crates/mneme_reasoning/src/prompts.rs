use mneme_core::Psyche;
use mneme_limbic::SomaticMarker;
use crate::api_types::{Message, Role, ContentBlock};

pub struct ContextAssembler;

impl ContextAssembler {
    /// Build system prompt with somatic marker injection (new System 1 integration)
    pub fn build_system_prompt_with_soma(
        psyche: &Psyche,
        recalled_memory: &str,
        somatic_marker: &SomaticMarker,
    ) -> String {
        let soma_context = somatic_marker.format_for_prompt();
        
        format!(
            "{}\n\n== 表达风格指引 ==\n{}\n\n重要：不要在回复中直接描述或提及你的情绪状态、精力水平或心情。让这些自然地体现在你的语气、回复长度和热情程度中，而不是用语言说出来。\n\n== SILENCE RULES ==\nIf the user's message is a casual remark in a group chat not directed at you, or if you have nothing meaningful to add, you may output exactly: [SILENCE]\n\n== RECALLED CONTEXT ==\n{}",
            psyche.format_context(),
            soma_context,
            recalled_memory
        )
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
