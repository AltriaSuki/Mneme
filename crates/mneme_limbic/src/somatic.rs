//! Somatic Markers - The bridge between System 1 and System 2
//!
//! Based on Antonio Damasio's somatic marker hypothesis, these markers
//! represent the "gut feelings" that bias rational decision-making.
//! They are injected into System 2's context to influence reasoning.

use mneme_core::{Affect, AttachmentStyle, OrganismState};
use serde::{Deserialize, Serialize};

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

impl ModulationVector {
    /// Linearly interpolate between self and other.
    /// `t` = 0.0 → returns self (no change), `t` = 1.0 → returns other (instant jump).
    /// A low `t` (e.g. 0.15) gives heavy inertia; a high `t` (e.g. 0.8) gives fast response.
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            max_tokens_factor: mix(self.max_tokens_factor, other.max_tokens_factor),
            temperature_delta: mix(self.temperature_delta, other.temperature_delta),
            context_budget_factor: mix(self.context_budget_factor, other.context_budget_factor),
            recall_mood_bias: mix(self.recall_mood_bias, other.recall_mood_bias),
            silence_inclination: mix(self.silence_inclination, other.silence_inclination),
            typing_speed_factor: mix(self.typing_speed_factor, other.typing_speed_factor),
        }
    }

    /// Compute the maximum absolute difference across all fields.
    /// Used to detect "surprise jumps" that should bypass smoothing.
    pub fn max_delta(&self, other: &Self) -> f32 {
        let deltas = [
            (self.max_tokens_factor - other.max_tokens_factor).abs(),
            (self.temperature_delta - other.temperature_delta).abs(),
            (self.context_budget_factor - other.context_budget_factor).abs(),
            (self.recall_mood_bias - other.recall_mood_bias).abs(),
            (self.silence_inclination - other.silence_inclination).abs(),
            (self.typing_speed_factor - other.typing_speed_factor).abs(),
        ];
        deltas.into_iter().fold(0.0f32, f32::max)
    }
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

/// Learnable parameters for state → ModulationVector mapping.
/// Each curve defines how a state dimension maps to a modulation parameter
/// via a linear range (low_output, high_output).
/// Default values match current hardcoded behavior exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModulationCurves {
    /// energy → max_tokens_factor: (low_energy_output, high_energy_output)
    pub energy_to_max_tokens: (f32, f32),
    /// stress → temperature delta: (low_stress_output, high_stress_output)
    pub stress_to_temperature: (f32, f32),
    /// energy → context budget: (low_energy_output, high_energy_output)
    pub energy_to_context: (f32, f32),
    /// mood → recall bias: (low_mood_output, high_mood_output)
    pub mood_to_recall_bias: (f32, f32),
    /// social_need → silence: (high_social_output, low_social_output)
    /// Note: high social need → low silence, so the mapping is inverted
    pub social_to_silence: (f32, f32),
    /// arousal → typing speed: (low_arousal_output, high_arousal_output)
    pub arousal_to_typing: (f32, f32),
}

impl Default for ModulationCurves {
    /// Default values match the original hardcoded behavior exactly.
    fn default() -> Self {
        Self {
            energy_to_max_tokens: (0.3, 1.2),  // lerp(0.3, 1.2, energy)
            stress_to_temperature: (0.0, 0.3), // stress * 0.3
            energy_to_context: (0.5, 1.1),     // lerp(0.5, 1.1, energy)
            mood_to_recall_bias: (-1.0, 1.0),  // direct passthrough of mood_bias
            social_to_silence: (0.4, 0.0),     // (1-social) * 0.4
            arousal_to_typing: (0.6, 1.8),     // lerp(0.6, 1.8, arousal)
        }
    }
}

/// Learnable behavior thresholds — replaces hardcoded magic numbers in somatic logic.
/// Each threshold controls when a specific behavior triggers.
/// Default values match the original hardcoded behavior exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorThresholds {
    /// Stress level above which `needs_attention()` fires
    pub attention_stress: f32,
    /// Energy level below which `needs_attention()` fires
    pub attention_energy: f32,
    /// Social need above which `needs_attention()` fires
    pub attention_social: f32,
    /// Energy level below which body feeling text changes to "exhausted"
    pub energy_critical: f32,
    /// Stress level above which body feeling text changes to "heart racing"
    pub stress_critical: f32,
    /// Social need above which "want to talk" feeling triggers
    pub social_need_high: f32,
    /// Curiosity above which "brain itching" feeling triggers
    pub curiosity_high: f32,
    /// Minimum energy gate for proactivity (clamped floor)
    pub energy_gate_min: f32,
    /// Stress below this + low arousal → calm bonus on temperature
    pub calm_stress_max: f32,
    /// Arousal below this + low stress → calm bonus on temperature
    pub calm_arousal_max: f32,
    /// Stress above this → extra silence inclination
    pub stress_silence_min: f32,
}

impl Default for BehaviorThresholds {
    fn default() -> Self {
        Self {
            attention_stress: 0.7,
            attention_energy: 0.3,
            attention_social: 0.8,
            energy_critical: 0.2,
            stress_critical: 0.8,
            social_need_high: 0.7,
            curiosity_high: 0.7,
            energy_gate_min: 0.3,
            calm_stress_max: 0.2,
            calm_arousal_max: 0.3,
            stress_silence_min: 0.8,
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

    /// Top curiosity interests (ADR-007: curiosity vectorization)
    #[serde(default)]
    pub curiosity_interests: Vec<(String, f32)>,
}

impl SomaticMarker {
    /// Create a somatic marker from the full organism state
    pub fn from_state(state: &OrganismState) -> Self {
        let interests: Vec<(String, f32)> = state
            .fast
            .curiosity_vector
            .top_interests(3)
            .into_iter()
            .map(|(t, i)| (t.to_string(), i))
            .collect();
        Self {
            affect: state.fast.affect,
            energy: state.fast.energy,
            stress: state.fast.stress,
            social_need: state.fast.social_need,
            curiosity: state.fast.curiosity,
            mood_bias: state.medium.mood_bias,
            attachment_style: state.medium.attachment.style(),
            openness: state.medium.openness,
            curiosity_interests: interests,
        }
    }

    /// Format for LLM system prompt injection.
    /// Combines minimal numeric state with a human-readable affect description
    /// so the LLM can both "feel" (via ModulationVector) and "know" its emotional state.
    /// Uses Chinese labels by default; call `format_for_prompt_lang` for other languages.
    pub fn format_for_prompt(&self) -> String {
        self.format_for_prompt_lang("zh")
    }

    /// Format for LLM system prompt injection with language selection.
    pub fn format_for_prompt_lang(&self, lang: &str) -> String {
        let affect_desc = self.affect.describe();
        let (state_label, emotion_label, curiosity_label) = match lang {
            "en" => ("Internal State", "Current Emotion", "Current Curiosities"),
            _ => ("内部状态", "当前情绪", "当前好奇方向"),
        };
        let mut s = format!(
            "[{}: E={:.2} S={:.2} M={:.2} A={:.2}/{:.2}]\n[{}: {}]",
            state_label,
            self.energy, self.stress, self.mood_bias, self.affect.valence, self.affect.arousal,
            emotion_label, affect_desc,
        );
        // ADR-007: Inject curiosity direction
        if !self.curiosity_interests.is_empty() {
            let interests: Vec<String> = self
                .curiosity_interests
                .iter()
                .map(|(t, i)| format!("{}({:.0}%)", t, i * 100.0))
                .collect();
            s.push_str(&format!("\n[{}: {}]", curiosity_label, interests.join(", ")));
        }
        s
    }

    /// Convert somatic marker to a structural modulation vector.
    /// Uses default curves (matches original hardcoded behavior).
    pub fn to_modulation_vector(&self) -> ModulationVector {
        self.to_modulation_vector_with_curves(&ModulationCurves::default())
    }

    /// Convert somatic marker to a structural modulation vector using learnable curves.
    ///
    /// This is the "neuromodulatory" pathway: instead of telling the LLM
    /// "you're tired", we actually give it less context and limit its output length.
    /// The behavior emerges from the constraint, not from instruction.
    pub fn to_modulation_vector_with_curves(&self, curves: &ModulationCurves) -> ModulationVector {
        self.to_modulation_vector_full(curves, &BehaviorThresholds::default())
    }

    /// Convert with both learnable curves and learnable thresholds.
    pub fn to_modulation_vector_full(
        &self,
        curves: &ModulationCurves,
        t: &BehaviorThresholds,
    ) -> ModulationVector {
        let max_tokens_factor = lerp(
            curves.energy_to_max_tokens.0,
            curves.energy_to_max_tokens.1,
            self.energy,
        );

        let stress_temp = lerp(
            curves.stress_to_temperature.0,
            curves.stress_to_temperature.1,
            self.stress,
        );
        let arousal_temp = self.affect.arousal * 0.15;
        let calm_bonus =
            if self.stress < t.calm_stress_max && self.affect.arousal < t.calm_arousal_max {
                -0.1
            } else {
                0.0
            };
        let temperature_delta = (stress_temp + arousal_temp + calm_bonus).clamp(-0.1, 0.4);

        let energy_context = lerp(
            curves.energy_to_context.0,
            curves.energy_to_context.1,
            self.energy,
        );
        let stress_penalty = self.stress * 0.3;
        let context_budget_factor = (energy_context - stress_penalty).clamp(0.4, 1.2);

        let mood_t = (self.mood_bias + 1.0) / 2.0;
        let recall_mood_bias = lerp(
            curves.mood_to_recall_bias.0,
            curves.mood_to_recall_bias.1,
            mood_t,
        )
        .clamp(-1.0, 1.0);

        let social_silence = lerp(
            curves.social_to_silence.0,
            curves.social_to_silence.1,
            self.social_need,
        );
        let energy_silence = (1.0 - self.energy) * 0.3;
        let stress_silence = if self.stress > t.stress_silence_min {
            0.2
        } else {
            0.0
        };
        let silence_inclination =
            (energy_silence + social_silence + stress_silence).clamp(0.0, 1.0);

        let typing_speed_factor = lerp(
            curves.arousal_to_typing.0,
            curves.arousal_to_typing.1,
            self.affect.arousal,
        );

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
        self.needs_attention_with(&BehaviorThresholds::default())
    }

    /// Check with learnable thresholds
    pub fn needs_attention_with(&self, t: &BehaviorThresholds) -> bool {
        self.stress > t.attention_stress
            || self.energy < t.attention_energy
            || self.social_need > t.attention_social
    }

    /// Get urgency level (0.0 - 1.0) for proactive messaging
    pub fn proactivity_urgency(&self) -> f32 {
        self.proactivity_urgency_with(&BehaviorThresholds::default())
    }

    /// Get urgency with learnable thresholds
    pub fn proactivity_urgency_with(&self, t: &BehaviorThresholds) -> f32 {
        let social_factor = self.social_need * 0.6;
        let curiosity_factor = self.curiosity * 0.2;
        let energy_gate = self.energy.max(t.energy_gate_min);
        let stress_penalty = self.stress * 0.3;
        ((social_factor + curiosity_factor) * energy_gate - stress_penalty).clamp(0.0, 1.0)
    }

    /// Describe subjective body feelings based on state changes (Damasio somatic marker hypothesis).
    ///
    /// Compares current state against a previous snapshot. Returns feeling descriptions
    /// only when changes exceed the significance threshold — not every tick produces a feeling.
    /// Each returned tuple is (feeling_text, intensity) where intensity ∈ [0, 1].
    pub fn describe_body_feeling(
        &self,
        prev: &SomaticMarker,
        threshold: f32,
    ) -> Vec<(String, f32)> {
        self.describe_body_feeling_with(prev, threshold, &BehaviorThresholds::default())
    }

    /// Describe body feelings with learnable thresholds.
    pub fn describe_body_feeling_with(
        &self,
        prev: &SomaticMarker,
        threshold: f32,
        t: &BehaviorThresholds,
    ) -> Vec<(String, f32)> {
        let mut feelings = Vec::new();

        let energy_delta = self.energy - prev.energy;
        let stress_delta = self.stress - prev.stress;
        let mood_delta = self.mood_bias - prev.mood_bias;
        let social_delta = self.social_need - prev.social_need;
        let curiosity_delta = self.curiosity - prev.curiosity;

        // Energy drop
        if energy_delta < -threshold {
            let intensity = (-energy_delta).min(1.0);
            let text = if self.energy < t.energy_critical {
                "感觉好像突然没力气了，整个人沉沉的".to_string()
            } else {
                "有点累了，想休息一下".to_string()
            };
            feelings.push((text, intensity));
        }

        // Energy surge
        if energy_delta > threshold {
            let intensity = energy_delta.min(1.0);
            feelings.push(("突然觉得精神好了很多！".to_string(), intensity));
        }

        // Stress spike
        if stress_delta > threshold {
            let intensity = stress_delta.min(1.0);
            let text = if self.stress > t.stress_critical {
                "心跳加快，有点紧张的感觉".to_string()
            } else {
                "感觉有一点点紧绷".to_string()
            };
            feelings.push((text, intensity));
        }

        // Stress relief
        if stress_delta < -threshold {
            let intensity = (-stress_delta).min(1.0);
            feelings.push(("松了一口气，放松下来了".to_string(), intensity));
        }

        // Mood drop
        if mood_delta < -threshold {
            let intensity = (-mood_delta).min(1.0);
            feelings.push(("心里有点闷闷的".to_string(), intensity));
        }

        // Mood lift
        if mood_delta > threshold {
            let intensity = mood_delta.min(1.0);
            feelings.push(("心情变好了，暖暖的".to_string(), intensity));
        }

        // Social need surge
        if social_delta > threshold && self.social_need > t.social_need_high {
            let intensity = social_delta.min(1.0);
            feelings.push(("好想和人说说话".to_string(), intensity));
        }

        // Curiosity spike
        if curiosity_delta > threshold && self.curiosity > t.curiosity_high {
            let intensity = curiosity_delta.min(1.0);
            feelings.push(("脑子里痒痒的，想知道更多".to_string(), intensity));
        }

        feelings
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

        assert!(marker.energy > 0.5);
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
        assert!(
            mv.max_tokens_factor > 0.8,
            "max_tokens_factor={}",
            mv.max_tokens_factor
        );
        // Default low stress → temperature_delta should be small
        assert!(
            mv.temperature_delta < 0.2,
            "temperature_delta={}",
            mv.temperature_delta
        );
        // Context budget should be close to 1.0
        assert!(
            mv.context_budget_factor > 0.7,
            "context_budget_factor={}",
            mv.context_budget_factor
        );
        // Silence should be low
        assert!(
            mv.silence_inclination < 0.5,
            "silence_inclination={}",
            mv.silence_inclination
        );
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
        assert!(
            mv.max_tokens_factor < 0.5,
            "max_tokens_factor={}",
            mv.max_tokens_factor
        );
        // High stress → elevated temperature
        assert!(
            mv.temperature_delta > 0.2,
            "temperature_delta={}",
            mv.temperature_delta
        );
        // Low energy + high stress → small context budget
        assert!(
            mv.context_budget_factor < 0.7,
            "context_budget_factor={}",
            mv.context_budget_factor
        );
        // Negative mood → negative recall bias
        assert!(
            mv.recall_mood_bias < -0.5,
            "recall_mood_bias={}",
            mv.recall_mood_bias
        );
        // Low energy + low social need → high silence inclination
        assert!(
            mv.silence_inclination > 0.5,
            "silence_inclination={}",
            mv.silence_inclination
        );
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
        assert!(
            mv.max_tokens_factor > 1.0,
            "max_tokens_factor={}",
            mv.max_tokens_factor
        );
        // Low stress → modest temperature
        assert!(
            mv.temperature_delta < 0.2,
            "temperature_delta={}",
            mv.temperature_delta
        );
        // High energy, low stress → big context budget
        assert!(
            mv.context_budget_factor > 0.9,
            "context_budget_factor={}",
            mv.context_budget_factor
        );
        // Positive mood → positive recall
        assert!(
            mv.recall_mood_bias > 0.5,
            "recall_mood_bias={}",
            mv.recall_mood_bias
        );
        // High social need → low silence
        assert!(
            mv.silence_inclination < 0.3,
            "silence_inclination={}",
            mv.silence_inclination
        );
        // Moderate arousal → typing speed above baseline
        assert!(
            mv.typing_speed_factor > 1.0,
            "typing_speed_factor={}",
            mv.typing_speed_factor
        );
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
    fn test_modulation_vector_lerp_midpoint() {
        let a = ModulationVector {
            max_tokens_factor: 0.4,
            temperature_delta: -0.1,
            context_budget_factor: 0.5,
            recall_mood_bias: -1.0,
            silence_inclination: 0.0,
            typing_speed_factor: 0.6,
        };
        let b = ModulationVector {
            max_tokens_factor: 1.2,
            temperature_delta: 0.3,
            context_budget_factor: 1.1,
            recall_mood_bias: 0.8,
            silence_inclination: 0.8,
            typing_speed_factor: 1.8,
        };
        let mid = a.lerp(&b, 0.5);
        assert!((mid.max_tokens_factor - 0.8).abs() < 0.01);
        assert!((mid.temperature_delta - 0.1).abs() < 0.01);
        assert!((mid.recall_mood_bias - (-0.1)).abs() < 0.01);
    }

    #[test]
    fn test_modulation_vector_lerp_extremes() {
        let a = ModulationVector::default();
        let b = ModulationVector {
            max_tokens_factor: 0.3,
            temperature_delta: 0.4,
            ..Default::default()
        };
        // t=0 → returns a
        let r0 = a.lerp(&b, 0.0);
        assert!((r0.max_tokens_factor - 1.0).abs() < 0.001);
        // t=1 → returns b
        let r1 = a.lerp(&b, 1.0);
        assert!((r1.max_tokens_factor - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_modulation_vector_max_delta() {
        let a = ModulationVector::default();
        let b = ModulationVector {
            max_tokens_factor: 0.3, // delta = 0.7
            temperature_delta: 0.4, // delta = 0.4
            ..Default::default()
        };
        let delta = a.max_delta(&b);
        assert!((delta - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_body_feeling_energy_drop() {
        let mut state = OrganismState::default();
        state.fast.energy = 0.7;
        let prev = SomaticMarker::from_state(&state);

        state.fast.energy = 0.3;
        let curr = SomaticMarker::from_state(&state);

        let feelings = curr.describe_body_feeling(&prev, 0.15);
        assert!(!feelings.is_empty());
        assert!(feelings.iter().any(|(t, _)| t.contains("累")));
    }

    #[test]
    fn test_body_feeling_stress_spike() {
        let mut state = OrganismState::default();
        state.fast.stress = 0.2;
        let prev = SomaticMarker::from_state(&state);

        state.fast.stress = 0.9;
        let curr = SomaticMarker::from_state(&state);

        let feelings = curr.describe_body_feeling(&prev, 0.15);
        assert!(feelings.iter().any(|(t, _)| t.contains("紧")));
    }

    #[test]
    fn test_body_feeling_no_change() {
        let state = OrganismState::default();
        let marker = SomaticMarker::from_state(&state);
        // Same state → no feelings
        let feelings = marker.describe_body_feeling(&marker, 0.15);
        assert!(feelings.is_empty());
    }

    #[test]
    fn test_body_feeling_mood_lift() {
        let mut state = OrganismState::default();
        state.medium.mood_bias = -0.3;
        let prev = SomaticMarker::from_state(&state);

        state.medium.mood_bias = 0.3;
        let curr = SomaticMarker::from_state(&state);

        let feelings = curr.describe_body_feeling(&prev, 0.15);
        assert!(feelings.iter().any(|(t, _)| t.contains("暖")));
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

    #[test]
    fn test_modulation_curves_default_matches_hardcoded() {
        // Default curves must produce identical results to the old hardcoded behavior
        let state = OrganismState::default();
        let marker = SomaticMarker::from_state(&state);

        let mv_default = marker.to_modulation_vector();
        let mv_curves = marker.to_modulation_vector_with_curves(&ModulationCurves::default());

        assert!((mv_default.max_tokens_factor - mv_curves.max_tokens_factor).abs() < 0.001);
        assert!((mv_default.temperature_delta - mv_curves.temperature_delta).abs() < 0.001);
        assert!((mv_default.context_budget_factor - mv_curves.context_budget_factor).abs() < 0.001);
        assert!((mv_default.recall_mood_bias - mv_curves.recall_mood_bias).abs() < 0.001);
        assert!((mv_default.silence_inclination - mv_curves.silence_inclination).abs() < 0.001);
        assert!((mv_default.typing_speed_factor - mv_curves.typing_speed_factor).abs() < 0.001);
    }

    #[test]
    fn test_modulation_curves_different_curves_different_output() {
        let mut state = OrganismState::default();
        state.fast.energy = 0.5;
        state.fast.stress = 0.5;
        let marker = SomaticMarker::from_state(&state);

        let default_mv = marker.to_modulation_vector();

        // "Sensitive" personality: small state changes → big modulation swings
        let sensitive = ModulationCurves {
            energy_to_max_tokens: (0.1, 1.5),
            stress_to_temperature: (0.0, 0.5),
            ..Default::default()
        };
        let sensitive_mv = marker.to_modulation_vector_with_curves(&sensitive);

        // Sensitive should have lower max_tokens at same energy (wider range, lower floor)
        assert!(
            (sensitive_mv.max_tokens_factor - default_mv.max_tokens_factor).abs() > 0.01,
            "Different curves should produce different max_tokens"
        );
    }

    #[test]
    fn test_modulation_curves_serialization_roundtrip() {
        let curves = ModulationCurves::default();
        let json = serde_json::to_string(&curves).unwrap();
        let deserialized: ModulationCurves = serde_json::from_str(&json).unwrap();

        assert!(
            (curves.energy_to_max_tokens.0 - deserialized.energy_to_max_tokens.0).abs() < 0.001
        );
        assert!(
            (curves.stress_to_temperature.1 - deserialized.stress_to_temperature.1).abs() < 0.001
        );
    }

    #[test]
    fn test_behavior_thresholds_default_matches_hardcoded() {
        let t = BehaviorThresholds::default();
        assert!((t.attention_stress - 0.7).abs() < 1e-6);
        assert!((t.attention_energy - 0.3).abs() < 1e-6);
        assert!((t.attention_social - 0.8).abs() < 1e-6);
        assert!((t.energy_critical - 0.2).abs() < 1e-6);
        assert!((t.stress_critical - 0.8).abs() < 1e-6);
        assert!((t.social_need_high - 0.7).abs() < 1e-6);
        assert!((t.curiosity_high - 0.7).abs() < 1e-6);
        assert!((t.energy_gate_min - 0.3).abs() < 1e-6);
        assert!((t.calm_stress_max - 0.2).abs() < 1e-6);
        assert!((t.calm_arousal_max - 0.3).abs() < 1e-6);
        assert!((t.stress_silence_min - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_behavior_thresholds_default_behavior_unchanged() {
        // Default thresholds must produce identical results to no-arg methods
        let state = OrganismState::default();
        let marker = SomaticMarker::from_state(&state);
        let t = BehaviorThresholds::default();

        assert_eq!(marker.needs_attention(), marker.needs_attention_with(&t));
        assert!((marker.proactivity_urgency() - marker.proactivity_urgency_with(&t)).abs() < 1e-6);

        let mv_default = marker.to_modulation_vector();
        let mv_full = marker.to_modulation_vector_full(&ModulationCurves::default(), &t);
        assert!((mv_default.silence_inclination - mv_full.silence_inclination).abs() < 1e-6);
        assert!((mv_default.temperature_delta - mv_full.temperature_delta).abs() < 1e-6);
    }

    #[test]
    fn test_custom_thresholds_change_behavior() {
        let mut state = OrganismState::default();
        state.fast.stress = 0.6;
        state.fast.energy = 0.35;
        state.fast.social_need = 0.75;
        let marker = SomaticMarker::from_state(&state);

        // Default thresholds: stress 0.6 < 0.7, energy 0.35 > 0.3, social 0.75 < 0.8
        assert!(!marker.needs_attention());

        // Custom: lower thresholds → now triggers
        let sensitive = BehaviorThresholds {
            attention_stress: 0.5,
            attention_energy: 0.4,
            attention_social: 0.7,
            ..Default::default()
        };
        assert!(marker.needs_attention_with(&sensitive));
    }

    #[test]
    fn test_thresholds_serialization_roundtrip() {
        let t = BehaviorThresholds::default();
        let json = serde_json::to_string(&t).unwrap();
        let restored: BehaviorThresholds = serde_json::from_str(&json).unwrap();
        assert!((t.attention_stress - restored.attention_stress).abs() < 1e-6);
        assert!((t.stress_silence_min - restored.stress_silence_min).abs() < 1e-6);
    }
}
