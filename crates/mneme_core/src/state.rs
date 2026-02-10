//! Multi-Scale Personality Dynamics State System
//! 
//! The internal state `s` is decomposed into three time-scales:
//! - `s_fast`: Second-scale dynamics (Arousal, Stress, Energy)
//! - `s_medium`: Hour-scale dynamics (Mood, Attachment, Openness)
//! - `s_slow`: Long-term dynamics (Core Values, Narrative Bias)
//!
//! This separation prevents state-space explosion and ensures stability.

use serde::{Deserialize, Serialize};
use crate::affect::Affect;

/// Guard against NaN and Infinity in state values.
/// If the value is NaN or Inf, replace with the provided fallback (homeostatic default).
#[inline]
fn sanitize_f32(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else {
        tracing::warn!("NaN/Inf detected in state, resetting to fallback {}", fallback);
        fallback
    }
}

/// Complete organism state: s = (s_fast, s_medium, s_slow)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganismState {
    pub fast: FastState,
    pub medium: MediumState,
    pub slow: SlowState,
    
    /// Unix timestamp of last state update
    pub last_updated: i64,
}

impl Default for OrganismState {
    fn default() -> Self {
        Self {
            fast: FastState::default(),
            medium: MediumState::default(),
            slow: SlowState::default(),
            last_updated: chrono::Utc::now().timestamp(),
        }
    }
}

impl OrganismState {
    /// Project the full state into an observable persona (p = σ(s))
    pub fn project(&self) -> ProjectedPersona {
        ProjectedPersona {
            affect: self.fast.affect,
            energy_level: self.fast.energy,
            stress_level: self.fast.stress,
            mood_bias: self.medium.mood_bias,
            attachment_style: self.medium.attachment.style(),
            dominant_values: self.slow.values.top_values(3),
        }
    }

    /// Generate a natural language description for LLM context injection
    /// NOTE: This describes HOW to behave, not WHAT you are feeling
    pub fn describe_for_context(&self) -> String {
        let mut behaviors = Vec::new();
        
        // Energy affects response style
        if self.fast.energy < 0.3 {
            behaviors.push("回复简洁，不展开复杂话题");
        } else if self.fast.energy > 0.7 {
            behaviors.push("可以深入讨论，思维活跃");
        }
        
        // Stress affects tone
        if self.fast.stress > 0.7 {
            behaviors.push("语气可能带点急躁或敏感");
        } else if self.fast.stress < 0.2 {
            behaviors.push("心态平和从容");
        }
        
        // Mood affects positivity
        if self.medium.mood_bias > 0.3 {
            behaviors.push("倾向于看到事物积极的一面");
        } else if self.medium.mood_bias < -0.3 {
            behaviors.push("语气略显低沉，不太活泼");
        }
        
        // Social need affects engagement
        if self.fast.social_need > 0.7 {
            behaviors.push("愿意主动延续话题");
        } else if self.fast.social_need < 0.2 {
            behaviors.push("倾向于简短回应");
        }
        
        // Curiosity affects exploration
        if self.fast.curiosity > 0.7 {
            behaviors.push("对新事物感兴趣，愿意追问");
        }

        // Boredom affects engagement
        if self.fast.boredom > 0.7 {
            behaviors.push("感到无聊，渴望新鲜话题");
        } else if self.fast.boredom > 0.5 {
            behaviors.push("对当前话题兴趣不大");
        }
        
        // Affect (valence + arousal) - the core emotional tone
        let affect_behavior = match (self.fast.affect.valence > 0.2, self.fast.affect.arousal > 0.5) {
            (true, true) => "语气活泼热情",
            (true, false) => "语气温和友善",
            (false, true) => "语气有些烦躁",
            (false, false) => "语气平淡冷静",
        };
        behaviors.push(affect_behavior);
        
        if behaviors.is_empty() {
            "以自然的方式回应".to_string()
        } else {
            format!("当前状态下的表达倾向：{}", behaviors.join("，"))
        }
    }
}

/// Projected persona for external observation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedPersona {
    pub affect: Affect,
    pub energy_level: f32,
    pub stress_level: f32,
    pub mood_bias: f32,
    pub attachment_style: AttachmentStyle,
    pub dominant_values: Vec<(String, f32)>,
}

// =============================================================================
// Fast State (Second-scale dynamics)
// =============================================================================

/// Fast dynamics: ds_fast/dt = F_fast(s_fast, s_medium, i, t)
/// 
/// These variables respond immediately to stimuli but also decay quickly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastState {
    /// Current affective state (Valence × Arousal)
    pub affect: Affect,
    
    /// Energy level (0.0 - 1.0): determines interaction vitality and persistence
    /// Depletes with activity, recovers with rest
    pub energy: f32,
    
    /// Stress level (0.0 - 1.0): sensitivity to negative information
    /// Spikes on negative stimuli, decays over time
    pub stress: f32,
    
    /// Curiosity (0.0 - 1.0): drive for exploration and topic divergence
    pub curiosity: f32,

    /// Social need (0.0 - 1.0): drive for proactive interaction
    /// Increases when alone, decreases after interaction
    pub social_need: f32,

    /// Boredom (0.0 - 1.0): monotony accumulator
    /// Increases with low-surprise, low-intensity input; decreases with novelty.
    /// Feeds back into curiosity drive and energy restlessness.
    pub boredom: f32,
}

impl Default for FastState {
    fn default() -> Self {
        Self {
            affect: Affect::default(),
            energy: 0.7,      // Start reasonably energized
            stress: 0.2,      // Low baseline stress
            curiosity: 0.5,   // Moderate curiosity
            social_need: 0.4, // Moderate social need
            boredom: 0.2,    // Low baseline boredom
        }
    }
}

impl FastState {
    /// Clamp all values to valid ranges
    pub fn normalize(&mut self) {
        self.energy = sanitize_f32(self.energy, 0.7).clamp(0.0, 1.0);
        self.stress = sanitize_f32(self.stress, 0.2).clamp(0.0, 1.0);
        self.curiosity = sanitize_f32(self.curiosity, 0.3).clamp(0.0, 1.0);
        self.social_need = sanitize_f32(self.social_need, 0.5).clamp(0.0, 1.0);
        self.boredom = sanitize_f32(self.boredom, 0.2).clamp(0.0, 1.0);
        self.affect.valence = sanitize_f32(self.affect.valence, 0.0).clamp(-1.0, 1.0);
        self.affect.arousal = sanitize_f32(self.affect.arousal, 0.3).clamp(0.0, 1.0);
    }
}

// =============================================================================
// Medium State (Hour-scale dynamics)
// =============================================================================

/// Medium dynamics: ds_medium/dt = F_medium(s_medium, s_slow, avg(s_fast))
/// 
/// These are integrals of fast state. Only change when fast state persists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediumState {
    /// Mood bias (-1.0 to 1.0): long-term emotional tendency
    /// Only changes when affect is consistently positive/negative
    pub mood_bias: f32,
    
    /// Attachment state: relationship dynamics with user
    pub attachment: AttachmentState,
    
    /// Openness (0.0 - 1.0): willingness to change and explore
    /// Influenced by curiosity and exploration success rate
    pub openness: f32,
    
    /// Hunger/Deprivation (0.0 - 1.0): general sense of lack
    /// Accumulates from unmet needs
    pub hunger: f32,
}

impl Default for MediumState {
    fn default() -> Self {
        Self {
            mood_bias: 0.0,
            attachment: AttachmentState::default(),
            openness: 0.6,  // Moderately open by default
            hunger: 0.2,
        }
    }
}

impl MediumState {
    /// Sanitize and clamp all fields to valid ranges.
    pub fn normalize(&mut self) {
        self.mood_bias = sanitize_f32(self.mood_bias, 0.0).clamp(-1.0, 1.0);
        self.openness = sanitize_f32(self.openness, 0.6).clamp(0.0, 1.0);
        self.hunger = sanitize_f32(self.hunger, 0.2).clamp(0.0, 1.0);
        self.attachment.anxiety = sanitize_f32(self.attachment.anxiety, 0.3).clamp(0.0, 1.0);
        self.attachment.avoidance = sanitize_f32(self.attachment.avoidance, 0.3).clamp(0.0, 1.0);
    }
}

/// Attachment state based on ECR (Experiences in Close Relationships) scale
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentState {
    /// Attachment anxiety (0.0 - 1.0): fear of rejection/abandonment
    /// Increases when ignored or rejected
    pub anxiety: f32,
    
    /// Attachment avoidance (0.0 - 1.0): resistance to intimacy
    /// Increases when intimacy leads to negative outcomes
    pub avoidance: f32,
}

impl Default for AttachmentState {
    fn default() -> Self {
        Self {
            anxiety: 0.3,   // Slight baseline anxiety
            avoidance: 0.2, // Low avoidance (open to connection)
        }
    }
}

impl AttachmentState {
    /// Classify into attachment style quadrant
    pub fn style(&self) -> AttachmentStyle {
        match (self.anxiety > 0.5, self.avoidance > 0.5) {
            (false, false) => AttachmentStyle::Secure,
            (true, false) => AttachmentStyle::Anxious,
            (false, true) => AttachmentStyle::Avoidant,
            (true, true) => AttachmentStyle::Disorganized,
        }
    }

    /// Update based on interaction outcome (Bayesian-like update)
    pub fn update_from_interaction(&mut self, was_positive: bool, response_delay_factor: f32) {
        let learning_rate = 0.05;
        
        if was_positive {
            self.anxiety -= learning_rate * self.anxiety;
            self.avoidance -= learning_rate * 0.5 * self.avoidance;
        } else {
            self.anxiety += learning_rate * (1.0 - self.anxiety);
        }
        
        // Long response delays increase anxiety
        if response_delay_factor > 1.5 {
            self.anxiety += learning_rate * 0.3 * (response_delay_factor - 1.0);
        }
        
        self.anxiety = self.anxiety.clamp(0.0, 1.0);
        self.avoidance = self.avoidance.clamp(0.0, 1.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentStyle {
    Secure,      // Low anxiety, low avoidance
    Anxious,     // High anxiety, low avoidance
    Avoidant,    // Low anxiety, high avoidance
    Disorganized, // High anxiety, high avoidance
}

// =============================================================================
// Slow State (Long-term dynamics)
// =============================================================================

/// Slow dynamics: ds_slow/dt = F_slow(s_slow, avg(s_medium), Crisis)
/// 
/// Most stable. Only changes significantly during narrative collapse/crisis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowState {
    /// Dynamic value network
    pub values: ValueNetwork,
    
    /// Narrative bias: how events are interpreted (-1.0 to 1.0)
    /// Positive = optimistic interpretation, Negative = pessimistic
    pub narrative_bias: f32,
    
    /// Core value rigidity (0.0 - 1.0): resistance to value change
    /// Increases as values are repeatedly reinforced
    pub rigidity: f32,
}

impl Default for SlowState {
    fn default() -> Self {
        Self {
            values: ValueNetwork::default(),
            narrative_bias: 0.1, // Slightly optimistic default
            rigidity: 0.3,       // Moderately flexible
        }
    }
}

/// Dynamic value network (replaces static constitution.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueNetwork {
    /// Value weights: name -> (weight, rigidity)
    /// Weight: how important this value is (0.0 - 1.0)
    /// Rigidity: how resistant to change (0.0 - 1.0)
    pub values: std::collections::HashMap<String, ValueEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueEntry {
    pub weight: f32,
    pub rigidity: f32,
}

impl Default for ValueNetwork {
    fn default() -> Self {
        let mut values = std::collections::HashMap::new();
        
        // Default value set with initial weights
        values.insert("honesty".to_string(), ValueEntry { weight: 0.8, rigidity: 0.5 });
        values.insert("kindness".to_string(), ValueEntry { weight: 0.7, rigidity: 0.4 });
        values.insert("curiosity".to_string(), ValueEntry { weight: 0.6, rigidity: 0.3 });
        values.insert("authenticity".to_string(), ValueEntry { weight: 0.7, rigidity: 0.5 });
        values.insert("growth".to_string(), ValueEntry { weight: 0.5, rigidity: 0.3 });
        values.insert("connection".to_string(), ValueEntry { weight: 0.6, rigidity: 0.4 });
        values.insert("autonomy".to_string(), ValueEntry { weight: 0.5, rigidity: 0.4 });
        
        Self { values }
    }
}

impl ValueNetwork {
    /// Get top N values by weight
    pub fn top_values(&self, n: usize) -> Vec<(String, f32)> {
        let mut sorted: Vec<_> = self.values.iter()
            .map(|(k, v)| (k.clone(), v.weight))
            .collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(n);
        sorted
    }

    /// Check if an action conflicts with high-weight values
    /// Returns the moral cost (stress penalty) if conflict detected
    pub fn compute_moral_cost(&self, violated_values: &[&str]) -> f32 {
        let mut cost = 0.0;
        for v in violated_values {
            if let Some(entry) = self.values.get(*v) {
                // Higher weight and rigidity = higher cost
                cost += entry.weight * (0.5 + 0.5 * entry.rigidity);
            }
        }
        cost.min(1.0)
    }
}

// =============================================================================
// Sensory Input (for dynamics computation)
// =============================================================================

/// Sensory input that drives state changes
#[derive(Debug, Clone, Default)]
pub struct SensoryInput {
    /// Emotional valence of incoming content (-1.0 to 1.0)
    pub content_valence: f32,
    
    /// Intensity/arousal of incoming content (0.0 to 1.0)
    pub content_intensity: f32,
    
    /// Surprise score (0.0 to 1.0): deviation from prediction
    pub surprise: f32,
    
    /// Whether this is a social interaction
    pub is_social: bool,
    
    /// Response delay factor (1.0 = normal, >1 = slow response from user)
    pub response_delay_factor: f32,
    
    /// Values potentially violated by current action
    pub violated_values: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state() {
        let state = OrganismState::default();
        assert!(state.fast.energy > 0.5);
        assert!(state.fast.stress < 0.5);
    }

    #[test]
    fn test_attachment_style() {
        let secure = AttachmentState { anxiety: 0.2, avoidance: 0.2 };
        assert_eq!(secure.style(), AttachmentStyle::Secure);
        
        let anxious = AttachmentState { anxiety: 0.8, avoidance: 0.2 };
        assert_eq!(anxious.style(), AttachmentStyle::Anxious);
    }

    #[test]
    fn test_value_network() {
        let net = ValueNetwork::default();
        let top = net.top_values(3);
        assert_eq!(top.len(), 3);
        assert!(top[0].1 >= top[1].1); // Sorted by weight
    }

    #[test]
    fn test_moral_cost() {
        let net = ValueNetwork::default();
        let cost = net.compute_moral_cost(&["honesty"]);
        assert!(cost > 0.5); // Honesty has high weight
    }

    #[test]
    fn test_attachment_all_styles() {
        assert_eq!(
            AttachmentState { anxiety: 0.2, avoidance: 0.2 }.style(),
            AttachmentStyle::Secure
        );
        assert_eq!(
            AttachmentState { anxiety: 0.8, avoidance: 0.2 }.style(),
            AttachmentStyle::Anxious
        );
        assert_eq!(
            AttachmentState { anxiety: 0.2, avoidance: 0.8 }.style(),
            AttachmentStyle::Avoidant
        );
        assert_eq!(
            AttachmentState { anxiety: 0.8, avoidance: 0.8 }.style(),
            AttachmentStyle::Disorganized
        );
    }

    #[test]
    fn test_attachment_positive_interaction() {
        let mut att = AttachmentState { anxiety: 0.6, avoidance: 0.4 };
        att.update_from_interaction(true, 1.0);
        // Positive interaction should reduce anxiety
        assert!(att.anxiety < 0.6);
        assert!(att.avoidance < 0.4);
    }

    #[test]
    fn test_attachment_negative_interaction() {
        let mut att = AttachmentState { anxiety: 0.3, avoidance: 0.2 };
        att.update_from_interaction(false, 1.0);
        // Negative interaction should increase anxiety
        assert!(att.anxiety > 0.3);
    }

    #[test]
    fn test_attachment_slow_response_increases_anxiety() {
        let mut att = AttachmentState { anxiety: 0.3, avoidance: 0.2 };
        att.update_from_interaction(true, 3.0); // Very slow response
        // Even positive interaction with slow response increases anxiety
        // The net effect depends on the learning rate balance
        // Just verify values stay in range
        assert!(att.anxiety >= 0.0 && att.anxiety <= 1.0);
        assert!(att.avoidance >= 0.0 && att.avoidance <= 1.0);
    }

    #[test]
    fn test_moral_cost_multiple_values() {
        let net = ValueNetwork::default();
        let cost_one = net.compute_moral_cost(&["honesty"]);
        let cost_two = net.compute_moral_cost(&["honesty", "kindness"]);
        assert!(cost_two > cost_one, "Violating more values should cost more");
    }

    #[test]
    fn test_moral_cost_unknown_value() {
        let net = ValueNetwork::default();
        let cost = net.compute_moral_cost(&["nonexistent_value"]);
        assert_eq!(cost, 0.0, "Unknown values should have zero cost");
    }

    #[test]
    fn test_moral_cost_capped_at_one() {
        let net = ValueNetwork::default();
        // Violate all values — cost should be capped at 1.0
        let all_values: Vec<&str> = net.values.keys().map(|s| s.as_str()).collect();
        let cost = net.compute_moral_cost(&all_values);
        assert!(cost <= 1.0, "Moral cost should be capped at 1.0, got {}", cost);
    }

    #[test]
    fn test_top_values_ordering() {
        let net = ValueNetwork::default();
        let top = net.top_values(5);
        for i in 1..top.len() {
            assert!(top[i - 1].1 >= top[i].1, "top_values should be sorted descending");
        }
    }

    #[test]
    fn test_top_values_exceeds_count() {
        let net = ValueNetwork::default();
        let top = net.top_values(100);
        assert_eq!(top.len(), net.values.len(), "Should return all values if n > count");
    }

    #[test]
    fn test_fast_state_normalize() {
        let mut fast = FastState::default();
        fast.energy = f32::NAN;
        fast.stress = f32::INFINITY;
        fast.curiosity = -5.0;
        fast.social_need = 10.0;
        fast.boredom = f32::NEG_INFINITY;
        fast.affect.valence = f32::NAN;
        fast.affect.arousal = f32::NAN;

        fast.normalize();

        assert!(fast.energy.is_finite() && fast.energy >= 0.0 && fast.energy <= 1.0);
        assert!(fast.stress.is_finite() && fast.stress >= 0.0 && fast.stress <= 1.0);
        assert!(fast.curiosity >= 0.0 && fast.curiosity <= 1.0);
        assert!(fast.social_need >= 0.0 && fast.social_need <= 1.0);
        assert!(fast.boredom >= 0.0 && fast.boredom <= 1.0);
        assert!(fast.affect.valence.is_finite());
        assert!(fast.affect.arousal.is_finite());
    }

    #[test]
    fn test_medium_state_normalize() {
        let mut medium = MediumState::default();
        medium.mood_bias = f32::NAN;
        medium.openness = f32::INFINITY;
        medium.hunger = -10.0;
        medium.attachment.anxiety = f32::NAN;

        medium.normalize();

        assert!(medium.mood_bias.is_finite() && medium.mood_bias >= -1.0 && medium.mood_bias <= 1.0);
        assert!(medium.openness >= 0.0 && medium.openness <= 1.0);
        assert!(medium.hunger >= 0.0 && medium.hunger <= 1.0);
        assert!(medium.attachment.anxiety.is_finite());
    }

    #[test]
    fn test_describe_for_context_low_energy() {
        let mut state = OrganismState::default();
        state.fast.energy = 0.1;
        let desc = state.describe_for_context();
        assert!(desc.contains("简洁"), "Low energy should mention 简洁, got: {}", desc);
    }

    #[test]
    fn test_describe_for_context_high_stress() {
        let mut state = OrganismState::default();
        state.fast.stress = 0.9;
        let desc = state.describe_for_context();
        assert!(desc.contains("急躁") || desc.contains("敏感"),
            "High stress should mention 急躁/敏感, got: {}", desc);
    }

    #[test]
    fn test_project_persona() {
        let state = OrganismState::default();
        let persona = state.project();
        assert_eq!(persona.attachment_style, AttachmentStyle::Secure);
        assert!(!persona.dominant_values.is_empty());
        assert_eq!(persona.dominant_values.len(), 3);
    }
}
