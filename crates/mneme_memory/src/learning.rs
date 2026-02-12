//! Offline Learning Pipeline — CurveLearner
//!
//! Records (state, modulation, feedback) triples during interaction,
//! then adjusts ModulationCurves during sleep using reward-weighted nudging.

use mneme_limbic::{ModulationCurves, ModulationVector};
use serde::{Deserialize, Serialize};

/// A recorded triple of (organism_state, modulation_output, user_feedback).
/// Collected during interaction, consumed during sleep by CurveLearner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModulationSample {
    pub id: i64,
    pub energy: f32,
    pub stress: f32,
    pub arousal: f32,
    pub mood_bias: f32,
    pub social_need: f32,
    pub modulation: ModulationVector,
    pub feedback_valence: f32,
    pub timestamp: i64,
}

/// Gradient-free curve optimizer.
///
/// Uses reward-weighted nudging: positive feedback pulls curves toward
/// the modulation parameters that produced it; negative feedback pushes away.
pub struct CurveLearner {
    /// Step size for curve adjustment (default 0.02)
    pub learning_rate: f32,
    /// Minimum samples required before learning (default 5)
    pub min_samples: usize,
}

impl CurveLearner {
    pub fn new() -> Self {
        Self {
            learning_rate: 0.02,
            min_samples: 5,
        }
    }

    /// Attempt to learn improved curves from collected samples.
    ///
    /// Returns `None` if there are insufficient samples.
    /// Returns `Some(new_curves)` with nudged parameters otherwise.
    pub fn learn(
        &self,
        current: &ModulationCurves,
        samples: &[ModulationSample],
    ) -> Option<ModulationCurves> {
        if samples.len() < self.min_samples {
            return None;
        }

        let positive: Vec<&ModulationSample> = samples
            .iter()
            .filter(|s| s.feedback_valence > 0.2)
            .collect();
        let negative: Vec<&ModulationSample> = samples
            .iter()
            .filter(|s| s.feedback_valence < -0.2)
            .collect();

        // Need at least one side to learn from
        if positive.is_empty() && negative.is_empty() {
            return None;
        }

        let avg_pos = Self::avg_modulation(&positive);
        let avg_neg = Self::avg_modulation(&negative);

        let mut curves = current.clone();
        self.nudge_curves(&mut curves, &avg_pos, &avg_neg);
        Some(curves)
    }

    /// Compute average modulation vector from a set of samples.
    /// Returns default if the slice is empty.
    fn avg_modulation(samples: &[&ModulationSample]) -> ModulationVector {
        if samples.is_empty() {
            return ModulationVector::default();
        }
        let n = samples.len() as f32;
        let mut sum = ModulationVector {
            max_tokens_factor: 0.0,
            temperature_delta: 0.0,
            context_budget_factor: 0.0,
            recall_mood_bias: 0.0,
            silence_inclination: 0.0,
            typing_speed_factor: 0.0,
        };
        for s in samples {
            sum.max_tokens_factor += s.modulation.max_tokens_factor;
            sum.temperature_delta += s.modulation.temperature_delta;
            sum.context_budget_factor += s.modulation.context_budget_factor;
            sum.recall_mood_bias += s.modulation.recall_mood_bias;
            sum.silence_inclination += s.modulation.silence_inclination;
            sum.typing_speed_factor += s.modulation.typing_speed_factor;
        }
        ModulationVector {
            max_tokens_factor: sum.max_tokens_factor / n,
            temperature_delta: sum.temperature_delta / n,
            context_budget_factor: sum.context_budget_factor / n,
            recall_mood_bias: sum.recall_mood_bias / n,
            silence_inclination: sum.silence_inclination / n,
            typing_speed_factor: sum.typing_speed_factor / n,
        }
    }

    /// Nudge curve high_output endpoints toward positive and away from negative.
    fn nudge_curves(
        &self,
        curves: &mut ModulationCurves,
        avg_pos: &ModulationVector,
        avg_neg: &ModulationVector,
    ) {
        let lr = self.learning_rate;

        // energy_to_max_tokens: nudge high-end toward positive max_tokens_factor
        let delta_mt = (avg_pos.max_tokens_factor - avg_neg.max_tokens_factor) * lr;
        curves.energy_to_max_tokens.1 = (curves.energy_to_max_tokens.1 + delta_mt).clamp(0.3, 1.5);

        // stress_to_temperature: nudge high-end
        let delta_temp = (avg_pos.temperature_delta - avg_neg.temperature_delta) * lr;
        curves.stress_to_temperature.1 =
            (curves.stress_to_temperature.1 + delta_temp).clamp(0.0, 0.5);

        // energy_to_context: nudge high-end
        let delta_ctx = (avg_pos.context_budget_factor - avg_neg.context_budget_factor) * lr;
        curves.energy_to_context.1 = (curves.energy_to_context.1 + delta_ctx).clamp(0.5, 1.5);

        // mood_to_recall_bias: nudge high-end
        let delta_rb = (avg_pos.recall_mood_bias - avg_neg.recall_mood_bias) * lr;
        curves.mood_to_recall_bias.1 = (curves.mood_to_recall_bias.1 + delta_rb).clamp(-1.0, 1.0);

        // social_to_silence: nudge low-end (inverted mapping)
        let delta_sil = (avg_pos.silence_inclination - avg_neg.silence_inclination) * lr;
        curves.social_to_silence.0 = (curves.social_to_silence.0 + delta_sil).clamp(0.0, 1.0);

        // arousal_to_typing: nudge high-end
        let delta_typ = (avg_pos.typing_speed_factor - avg_neg.typing_speed_factor) * lr;
        curves.arousal_to_typing.1 = (curves.arousal_to_typing.1 + delta_typ).clamp(0.5, 2.5);
    }
}

impl Default for CurveLearner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sample(valence: f32, mv: ModulationVector) -> ModulationSample {
        ModulationSample {
            id: 0,
            energy: 0.5,
            stress: 0.3,
            arousal: 0.4,
            mood_bias: 0.0,
            social_need: 0.5,
            modulation: mv,
            feedback_valence: valence,
            timestamp: 0,
        }
    }

    #[test]
    fn test_learner_no_samples() {
        let learner = CurveLearner::new();
        let curves = ModulationCurves::default();
        // Too few samples → None
        let samples: Vec<ModulationSample> = (0..3)
            .map(|_| make_sample(0.5, ModulationVector::default()))
            .collect();
        assert!(learner.learn(&curves, &samples).is_none());
    }

    #[test]
    fn test_learner_positive_nudge() {
        let learner = CurveLearner::new();
        let curves = ModulationCurves::default();
        let original_high = curves.energy_to_max_tokens.1;

        // All positive feedback with high max_tokens_factor
        let high_mv = ModulationVector {
            max_tokens_factor: 1.4,
            ..Default::default()
        };
        let samples: Vec<ModulationSample> =
            (0..6).map(|_| make_sample(0.8, high_mv.clone())).collect();

        let new_curves = learner.learn(&curves, &samples).unwrap();
        // high_output should have increased (positive nudge, no negative to counteract)
        assert!(
            new_curves.energy_to_max_tokens.1 > original_high,
            "Expected nudge up: {} > {}",
            new_curves.energy_to_max_tokens.1,
            original_high
        );
    }

    #[test]
    fn test_learner_negative_nudge() {
        let learner = CurveLearner::new();
        let curves = ModulationCurves::default();
        let original_high = curves.energy_to_max_tokens.1;

        // All negative feedback with high max_tokens_factor
        let high_mv = ModulationVector {
            max_tokens_factor: 1.4,
            ..Default::default()
        };
        let samples: Vec<ModulationSample> =
            (0..6).map(|_| make_sample(-0.8, high_mv.clone())).collect();

        let new_curves = learner.learn(&curves, &samples).unwrap();
        // high_output should have decreased (negative nudge pushes away)
        assert!(
            new_curves.energy_to_max_tokens.1 < original_high,
            "Expected nudge down: {} < {}",
            new_curves.energy_to_max_tokens.1,
            original_high
        );
    }

    #[test]
    fn test_learner_bounds() {
        let mut learner = CurveLearner::new();
        learner.learning_rate = 0.5; // Aggressive rate for testing bounds

        let mut curves = ModulationCurves::default();

        // Extreme positive samples pushing max_tokens very high
        let extreme_mv = ModulationVector {
            max_tokens_factor: 10.0,
            temperature_delta: 10.0,
            context_budget_factor: 10.0,
            recall_mood_bias: 10.0,
            silence_inclination: 10.0,
            typing_speed_factor: 10.0,
        };
        let samples: Vec<ModulationSample> = (0..10)
            .map(|_| make_sample(1.0, extreme_mv.clone()))
            .collect();

        // Apply learning multiple rounds
        for _ in 0..20 {
            if let Some(new) = learner.learn(&curves, &samples) {
                curves = new;
            }
        }

        // All values should be within safe bounds
        assert!(curves.energy_to_max_tokens.1 <= 1.5);
        assert!(curves.stress_to_temperature.1 <= 0.5);
        assert!(curves.energy_to_context.1 <= 1.5);
        assert!(curves.mood_to_recall_bias.1 <= 1.0);
        assert!(curves.social_to_silence.0 <= 1.0);
        assert!(curves.arousal_to_typing.1 <= 2.5);
    }
}
