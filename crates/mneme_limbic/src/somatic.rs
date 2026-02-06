//! Somatic Markers - The bridge between System 1 and System 2
//!
//! Based on Antonio Damasio's somatic marker hypothesis, these markers
//! represent the "gut feelings" that bias rational decision-making.
//! They are injected into System 2's context to influence reasoning.

use serde::{Deserialize, Serialize};
use mneme_core::{OrganismState, Affect, AttachmentStyle};

/// Structural modulation vector — the "neuromodulatory signal" that physically
/// changes how the LLM processes information, rather than telling it how to feel.
///
/// This is the core of the "embodied" paradigm: instead of injecting text hints
/// like "语气可能略急", we adjust LLM parameters and context budget so that
/// the behavior emerges naturally from structural constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModulationVector {
    /// Factor applied to max_tokens (0.3 - 1.5). Low energy → shorter responses.
    pub max_tokens_factor: f32,
    
    /// Delta applied to temperature (-0.3 to +0.4). High stress/arousal → more unpredictable.
    pub temperature_delta: f32,
    
    /// Factor applied to context budget (0.4 - 1.2). Low energy → less context fed to LLM.
    pub context_budget_factor: f32,
    
    /// Mood bias for memory recall (-1.0 to 1.0). Negative mood → recall skews negative.
    pub recall_mood_bias: f32,
    
    /// Silence inclination (0.0 - 1.0). High value → more likely to output [SILENCE].
    pub silence_inclination: f32,
    
    /// Typing speed factor for humanizer (0.5 - 2.0). High arousal → faster typing.
    pub typing_speed_factor: f32,
}

impl Default for ModulationVector {
    fn default() -> Self {
        Self {
            max_tokens_factor: 1.0,
            temperature_delta: 0.0,
            context_budget_factor: 1.0,
            recall_mood_bias: 0.0,
            silence_inclination: 0.0,
            typing_speed_factor: 1.0,
        }
    }
}

/// A somatic marker - compressed state for System 2 injection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SomaticMarker {
    /// Current affect (valence × arousal)
    pub affect: Affect,
    
    /// Energy level (0.0 - 1.0)
    pub energy: f32,
    
    /// Stress level (0.0 - 1.0)
    pub stress: f32,
    
    /// Social need (0.0 - 1.0)
    pub social_need: f32,
    
    /// Curiosity (0.0 - 1.0)
    pub curiosity: f32,
    
    /// Mood bias (-1.0 to 1.0)
    pub mood_bias: f32,
    
    /// Attachment style
    pub attachment_style: AttachmentStyle,
    
    /// Openness to experience (0.0 - 1.0)
    pub openness: f32,
    
    /// Natural language description for LLM injection
    pub description: String,
    
    /// Behavioral hints based on current state
    pub hints: Vec<String>,
}

impl SomaticMarker {
    /// Create a somatic marker from the full organism state
    pub fn from_state(state: &OrganismState) -> Self {
        let description = state.describe_for_context();
        let hints = Self::generate_hints(state);
        
        Self {
            affect: state.fast.affect,
            energy: state.fast.energy,
            stress: state.fast.stress,
            social_need: state.fast.social_need,
            curiosity: state.fast.curiosity,
            mood_bias: state.medium.mood_bias,
            attachment_style: state.medium.attachment.style(),
            openness: state.medium.openness,
            description,
            hints,
        }
    }

    /// Generate behavioral hints based on state
    fn generate_hints(state: &OrganismState) -> Vec<String> {
        let mut hints = Vec::new();
        
        // Energy-based hints
        if state.fast.energy < 0.3 {
            hints.push("保持回复简短，避免复杂话题".to_string());
        } else if state.fast.energy > 0.8 {
            hints.push("可以进行深入讨论，精力充沛".to_string());
        }
        
        // Stress-based hints
        if state.fast.stress > 0.7 {
            hints.push("当前压力较大，可能反应更敏感".to_string());
        }
        
        // Social need hints
        if state.fast.social_need > 0.7 {
            hints.push("渴望交流，可以主动延续话题".to_string());
        } else if state.fast.social_need < 0.2 {
            hints.push("社交需求已满足，可以接受简短交流".to_string());
        }
        
        // Curiosity hints
        if state.fast.curiosity > 0.7 {
            hints.push("好奇心旺盛，愿意探索新话题".to_string());
        }
        
        // Mood hints
        if state.medium.mood_bias < -0.5 {
            hints.push("整体情绪低落，需要温和对待".to_string());
        } else if state.medium.mood_bias > 0.5 {
            hints.push("心情不错，可以分享积极的事物".to_string());
        }
        
        // Attachment style hints
        match state.medium.attachment.style() {
            AttachmentStyle::Anxious => {
                hints.push("可能需要更多确认和回应".to_string());
            }
            AttachmentStyle::Avoidant => {
                hints.push("保持适当距离，不要过于热情".to_string());
            }
            AttachmentStyle::Disorganized => {
                hints.push("情绪可能不稳定，需要耐心".to_string());
            }
            AttachmentStyle::Secure => {}
        }
        
        hints
    }

    /// Format for LLM system prompt injection
    /// Now uses minimal numeric state as an auxiliary signal — the primary
    /// mechanism is the ModulationVector which structurally constrains the LLM.
    pub fn format_for_prompt(&self) -> String {
        format!(
            "[内部状态: E={:.2} S={:.2} M={:.2} A={:.2}/{:.2}]",
            self.energy, self.stress, self.mood_bias,
            self.affect.valence, self.affect.arousal,
        )
    }
    
    /// Convert somatic marker to a structural modulation vector.
    /// 
    /// This is the "neuromodulatory" pathway: instead of telling the LLM
    /// "you're tired", we actually give it less context and limit its output length.
    /// The behavior emerges from the constraint, not from instruction.
    pub fn to_modulation_vector(&self) -> ModulationVector {
        // === max_tokens_factor: energy → response length capacity ===
        // Low energy = physically cannot produce long responses
        // Range: 0.3 (exhausted) to 1.2 (energetic, expansive)
        let max_tokens_factor = lerp(0.3, 1.2, self.energy);
        
        // === temperature_delta: stress + arousal → unpredictability ===
        // High stress or arousal = more erratic, impulsive responses
        // Range: -0.1 (calm, focused) to +0.4 (agitated)
        let stress_temp = self.stress * 0.3;
        let arousal_temp = self.affect.arousal * 0.15;
        let calm_bonus = if self.stress < 0.2 && self.affect.arousal < 0.3 { -0.1 } else { 0.0 };
        let temperature_delta = (stress_temp + arousal_temp + calm_bonus).clamp(-0.1, 0.4);
        
        // === context_budget_factor: energy + stress → working memory capacity ===
        // Tired or stressed = can't process as much information
        // Range: 0.4 (overwhelmed) to 1.2 (sharp, absorbing everything)
        let energy_context = lerp(0.5, 1.1, self.energy);
        let stress_penalty = self.stress * 0.3;
        let context_budget_factor = (energy_context - stress_penalty).clamp(0.4, 1.2);
        
        // === recall_mood_bias: mood → what memories surface ===
        // Negative mood = negative memories float up (mood-congruent recall)
        // Range: -1.0 to 1.0
        let recall_mood_bias = self.mood_bias.clamp(-1.0, 1.0);
        
        // === silence_inclination: low social need + low energy → don't want to talk ===
        // Range: 0.0 to 1.0
        let energy_silence = (1.0 - self.energy) * 0.3;
        let social_silence = (1.0 - self.social_need) * 0.4;
        let stress_silence = if self.stress > 0.8 { 0.2 } else { 0.0 }; // extreme stress → withdraw
        let silence_inclination = (energy_silence + social_silence + stress_silence).clamp(0.0, 1.0);
        
        // === typing_speed_factor: arousal → urgency of expression ===
        // Range: 0.5 (slow, deliberate) to 2.0 (rapid-fire)
        let typing_speed_factor = lerp(0.6, 1.8, self.affect.arousal);
        
        ModulationVector {
            max_tokens_factor,
            temperature_delta,
            context_budget_factor,
            recall_mood_bias,
            silence_inclination,
            typing_speed_factor,
        }
    }

    /// Check if the marker indicates a need for special handling
    pub fn needs_attention(&self) -> bool {
        self.stress > 0.7 || self.energy < 0.3 || self.social_need > 0.8
    }

    /// Get urgency level (0.0 - 1.0) for proactive messaging
    pub fn proactivity_urgency(&self) -> f32 {
        // Social need is the primary driver
        let social_factor = self.social_need * 0.6;
        
        // Curiosity adds some urgency
        let curiosity_factor = self.curiosity * 0.2;
        
        // Energy gates proactivity (low energy = less proactive)
        let energy_gate = self.energy.max(0.3);
        
        // Stress reduces proactivity (focus on recovery)
        let stress_penalty = self.stress * 0.3;
        
        ((social_factor + curiosity_factor) * energy_gate - stress_penalty).clamp(0.0, 1.0)
    }
}

/// Linear interpolation helper
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_somatic_marker_creation() {
        let state = OrganismState::default();
        let marker = SomaticMarker::from_state(&state);
        
        assert!(!marker.description.is_empty());
        assert!(marker.energy > 0.5);
    }

    #[test]
    fn test_hints_generation() {
        let mut state = OrganismState::default();
        state.fast.energy = 0.2; // Low energy
        state.fast.stress = 0.8; // High stress
        
        let marker = SomaticMarker::from_state(&state);
        
        assert!(marker.hints.iter().any(|h| h.contains("简短")));
        assert!(marker.hints.iter().any(|h| h.contains("压力")));
    }

    #[test]
    fn test_proactivity_urgency() {
        let mut state = OrganismState::default();
        
        // Low social need = low urgency
        state.fast.social_need = 0.2;
        state.fast.energy = 0.7;
        let marker = SomaticMarker::from_state(&state);
        assert!(marker.proactivity_urgency() < 0.3);
        
        // High social need + good energy = high urgency
        state.fast.social_need = 0.9;
        state.fast.energy = 0.8;
        state.fast.stress = 0.1;
        state.fast.curiosity = 0.6;
        let marker = SomaticMarker::from_state(&state);
        assert!(marker.proactivity_urgency() > 0.3);
    }

    #[test]
    fn test_modulation_vector_default_state() {
        // Default state should produce near-neutral modulation
        let state = OrganismState::default();
        let marker = SomaticMarker::from_state(&state);
        let mv = marker.to_modulation_vector();
        
        // Default energy ~0.7 → max_tokens_factor should be above 0.8
        assert!(mv.max_tokens_factor > 0.8, "max_tokens_factor={}", mv.max_tokens_factor);
        // Default low stress → temperature_delta should be small
        assert!(mv.temperature_delta < 0.2, "temperature_delta={}", mv.temperature_delta);
        // Context budget should be close to 1.0
        assert!(mv.context_budget_factor > 0.7, "context_budget_factor={}", mv.context_budget_factor);
        // Silence should be low
        assert!(mv.silence_inclination < 0.5, "silence_inclination={}", mv.silence_inclination);
    }
    
    #[test]
    fn test_modulation_vector_exhausted_state() {
        // Exhausted + stressed → short responses, high temp, low context
        let mut state = OrganismState::default();
        state.fast.energy = 0.05;
        state.fast.stress = 0.95;
        state.medium.mood_bias = -0.8;
        state.fast.social_need = 0.1;
        
        let marker = SomaticMarker::from_state(&state);
        let mv = marker.to_modulation_vector();
        
        // Very low energy → very short max_tokens
        assert!(mv.max_tokens_factor < 0.5, "max_tokens_factor={}", mv.max_tokens_factor);
        // High stress → elevated temperature
        assert!(mv.temperature_delta > 0.2, "temperature_delta={}", mv.temperature_delta);
        // Low energy + high stress → small context budget
        assert!(mv.context_budget_factor < 0.7, "context_budget_factor={}", mv.context_budget_factor);
        // Negative mood → negative recall bias
        assert!(mv.recall_mood_bias < -0.5, "recall_mood_bias={}", mv.recall_mood_bias);
        // Low energy + low social need → high silence inclination
        assert!(mv.silence_inclination > 0.5, "silence_inclination={}", mv.silence_inclination);
    }
    
    #[test]
    fn test_modulation_vector_energetic_state() {
        // Energetic + curious + happy → long responses, stable temp, full context
        let mut state = OrganismState::default();
        state.fast.energy = 0.95;
        state.fast.stress = 0.1;
        state.fast.curiosity = 0.9;
        state.fast.social_need = 0.8;
        state.medium.mood_bias = 0.7;
        state.fast.affect.arousal = 0.6;
        
        let marker = SomaticMarker::from_state(&state);
        let mv = marker.to_modulation_vector();
        
        // High energy → long responses
        assert!(mv.max_tokens_factor > 1.0, "max_tokens_factor={}", mv.max_tokens_factor);
        // Low stress → modest temperature
        assert!(mv.temperature_delta < 0.2, "temperature_delta={}", mv.temperature_delta);
        // High energy, low stress → big context budget
        assert!(mv.context_budget_factor > 0.9, "context_budget_factor={}", mv.context_budget_factor);
        // Positive mood → positive recall
        assert!(mv.recall_mood_bias > 0.5, "recall_mood_bias={}", mv.recall_mood_bias);
        // High social need → low silence
        assert!(mv.silence_inclination < 0.3, "silence_inclination={}", mv.silence_inclination);
        // Moderate arousal → typing speed above baseline
        assert!(mv.typing_speed_factor > 1.0, "typing_speed_factor={}", mv.typing_speed_factor);
    }
    
    #[test]
    fn test_modulation_vector_bounds() {
        // Extreme edge: all zeros
        let mut state = OrganismState::default();
        state.fast.energy = 0.0;
        state.fast.stress = 0.0;
        state.fast.curiosity = 0.0;
        state.fast.social_need = 0.0;
        state.fast.affect.arousal = 0.0;
        state.fast.affect.valence = 0.0;
        state.medium.mood_bias = 0.0;
        
        let marker = SomaticMarker::from_state(&state);
        let mv = marker.to_modulation_vector();
        
        assert!(mv.max_tokens_factor >= 0.3);
        assert!(mv.temperature_delta >= -0.1);
        assert!(mv.context_budget_factor >= 0.4);
        assert!(mv.silence_inclination >= 0.0 && mv.silence_inclination <= 1.0);
        assert!(mv.typing_speed_factor >= 0.5);
        
        // Extreme edge: all maxed out
        state.fast.energy = 1.0;
        state.fast.stress = 1.0;
        state.fast.social_need = 1.0;
        state.fast.affect.arousal = 1.0;
        state.medium.mood_bias = 1.0;
        
        let marker = SomaticMarker::from_state(&state);
        let mv = marker.to_modulation_vector();
        
        assert!(mv.max_tokens_factor <= 1.5);
        assert!(mv.temperature_delta <= 0.4);
        assert!(mv.context_budget_factor <= 1.2);
        assert!(mv.silence_inclination >= 0.0 && mv.silence_inclination <= 1.0);
        assert!(mv.typing_speed_factor <= 2.0);
    }
    
    #[test]
    fn test_format_for_prompt_minimal() {
        let state = OrganismState::default();
        let marker = SomaticMarker::from_state(&state);
        let prompt = marker.format_for_prompt();
        
        // Should be compact numeric format, not verbose Chinese text
        assert!(prompt.starts_with("[内部状态:"));
        assert!(prompt.contains("E="));
        assert!(prompt.contains("S="));
        assert!(prompt.contains("M="));
        assert!(prompt.contains("A="));
    }
}
