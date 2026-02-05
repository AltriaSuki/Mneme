//! Somatic Markers - The bridge between System 1 and System 2
//!
//! Based on Antonio Damasio's somatic marker hypothesis, these markers
//! represent the "gut feelings" that bias rational decision-making.
//! They are injected into System 2's context to influence reasoning.

use serde::{Deserialize, Serialize};
use mneme_core::{OrganismState, Affect, AttachmentStyle};

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
    /// Focus on behavioral guidance, not state description
    pub fn format_for_prompt(&self) -> String {
        let mut guidance = Vec::new();
        
        // Energy -> response length/depth
        if self.energy < 0.3 {
            guidance.push("简洁回复");
        }
        
        // Stress -> tone sensitivity
        if self.stress > 0.7 {
            guidance.push("语气可能略急");
        }
        
        // Social need -> engagement level
        if self.social_need > 0.7 {
            guidance.push("愿意多聊");
        }
        
        // Curiosity -> exploration
        if self.curiosity > 0.7 {
            guidance.push("对新话题感兴趣");
        }
        
        // Mood -> positivity
        if self.mood_bias < -0.3 {
            guidance.push("语气偏淡");
        } else if self.mood_bias > 0.3 {
            guidance.push("语气偏积极");
        }
        
        if guidance.is_empty() {
            "以自然方式回应".to_string()
        } else {
            guidance.join("，")
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
}
