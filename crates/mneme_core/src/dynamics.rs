//! State Dynamics: The differential equations governing state evolution
//!
//! ds/dt = F(s, i, t) where:
//! - s = (s_fast, s_medium, s_slow) is the organism state
//! - i = sensory input
//! - t = time
//!
//! The dynamics are separated by time-scale to ensure stability.

use crate::state::{FastState, MediumState, OrganismState, SensoryInput, SlowState};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Trait for implementing state dynamics
pub trait Dynamics: Send + Sync {
    /// Advance the state by dt given sensory input
    fn step(&self, state: &mut OrganismState, input: &SensoryInput, dt: Duration);
}

/// Learnable dynamics parameters — individualized through feedback.
///
/// These start at sensible defaults but drift per-instance based on
/// interaction feedback during sleep consolidation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnableDynamics {
    pub energy_target: f32,
    pub stress_decay_rate: f32,
}

impl Default for LearnableDynamics {
    fn default() -> Self {
        Self {
            energy_target: 0.7,
            stress_decay_rate: 0.005,
        }
    }
}

impl LearnableDynamics {
    /// Adjust parameters from interaction samples: (energy, stress, feedback_valence).
    /// Returns true if an update was applied.
    pub fn learn_from_samples(&mut self, samples: &[(f32, f32, f32)]) -> bool {
        if samples.len() < 3 {
            return false;
        }
        let lr = 0.005;
        let n = samples.len() as f32;

        // Energy target: move toward energy levels that got positive feedback
        let energy_signal: f32 = samples
            .iter()
            .map(|(e, _, fv)| fv * (e - self.energy_target))
            .sum::<f32>()
            / n;
        self.energy_target = (self.energy_target + lr * energy_signal).clamp(0.3, 0.9);

        // Stress decay: increase if high stress correlates with negative feedback
        let stress_signal: f32 = samples.iter().map(|(_, s, fv)| -fv * s).sum::<f32>() / n;
        self.stress_decay_rate =
            (self.stress_decay_rate + lr * 0.001 * stress_signal).clamp(0.001, 0.02);

        true
    }
}

/// Default ODE-based dynamics implementation
///
/// Uses simple exponential decay/growth models. Can be replaced with
/// neural network (Burn/Candle) for learned dynamics.
#[derive(Debug, Clone)]
pub struct DefaultDynamics {
    /// Learnable parameters (persisted per-instance)
    pub learnable: LearnableDynamics,

    /// Homeostatic targets (fixed)
    pub stress_target: f32,
    pub social_need_target: f32,

    /// Decay/recovery rates (per second)
    pub energy_recovery_rate: f32,
    pub social_need_growth_rate: f32,

    /// Stimulus sensitivity
    pub stress_sensitivity: f32,
    pub affect_sensitivity: f32,

    /// Medium dynamics time constant (hours)
    pub mood_time_constant: f32,
}

impl Default for DefaultDynamics {
    fn default() -> Self {
        Self {
            learnable: LearnableDynamics::default(),

            stress_target: 0.2,
            social_need_target: 0.5,

            energy_recovery_rate: 0.003, // ~0.18/min — recovers from 0→0.7 in ~5 min
            social_need_growth_rate: 0.0001, // Slow growth when alone

            stress_sensitivity: 0.5,
            affect_sensitivity: 0.3,

            mood_time_constant: 2.0, // 2 hours
        }
    }
}

impl Dynamics for DefaultDynamics {
    fn step(&self, state: &mut OrganismState, input: &SensoryInput, dt: Duration) {
        let dt_secs = dt.as_secs_f32();

        // Fast dynamics (always computed)
        self.step_fast(&mut state.fast, &state.medium, input, dt_secs);

        // Medium dynamics (computed less frequently, here we just integrate)
        self.step_medium(&mut state.medium, &state.fast, &state.slow, input, dt_secs);

        // Slow dynamics (only on crisis events, handled separately)
        // self.step_slow() is called explicitly when needed

        state.last_updated = chrono::Utc::now().timestamp();
    }
}

impl DefaultDynamics {
    /// Fast dynamics: ds_fast/dt = F_fast(s_fast, s_medium, i, t)
    pub fn step_fast(
        &self,
        fast: &mut FastState,
        medium: &MediumState,
        input: &SensoryInput,
        dt: f32,
    ) {
        // === Energy dynamics ===
        // dE/dt = recovery_rate * (target - E) - activity_cost
        // Only apply activity cost when there's actual stimulus (social interaction
        // or meaningful input). Idle heartbeat ticks should not drain energy —
        // otherwise energy drains to 0 and can never recover (death spiral).
        let has_stimulus = input.is_social || input.content_intensity > 0.01;
        let activity_cost = if input.is_social {
            0.01
        } else if has_stimulus {
            0.002
        } else {
            0.0
        };
        let d_energy =
            self.energy_recovery_rate * (self.learnable.energy_target - fast.energy) - activity_cost;
        fast.energy += d_energy * dt;

        // === Stress dynamics ===
        // dS/dt = -decay_rate * (S - target) + sensitivity * negative_stimulus
        let negative_stimulus = (-input.content_valence).max(0.0) * input.content_intensity;
        let surprise_stress = input.surprise * 0.3;

        // Moral cost from value violations creates direct stress
        let moral_stress = if !input.violated_values.is_empty() {
            0.5 // Base moral cost, refined in state.rs
        } else {
            0.0
        };

        let d_stress = -self.learnable.stress_decay_rate * (fast.stress - self.stress_target)
            + self.stress_sensitivity * (negative_stimulus + surprise_stress + moral_stress);
        fast.stress += d_stress * dt;

        // === Affect dynamics ===
        // Affect moves toward stimulus-induced target.
        // During idle (no stimulus), affect decays toward neutral — mood_bias
        // should NOT pull valence negative when nothing is happening, otherwise
        // a negative mood creates a feedback loop: negative affect → stress ↑ →
        // more negative affect → stress ↑↑ (death spiral).
        let mood_influence = if has_stimulus {
            medium.mood_bias * 0.3
        } else {
            0.0
        };
        let target_valence = input.content_valence * self.affect_sensitivity + mood_influence;
        let target_arousal = input.content_intensity * 0.5 + input.surprise * 0.3 + 0.2;

        let affect_rate = 0.1; // How quickly affect changes
        fast.affect.valence += affect_rate * (target_valence - fast.affect.valence) * dt;
        fast.affect.arousal += affect_rate * (target_arousal - fast.affect.arousal) * dt;

        // Stress pulls valence down — but only during active interaction.
        // During idle, this coupling creates a positive feedback loop that
        // prevents homeostatic recovery.
        if has_stimulus {
            fast.affect.valence -= fast.stress * 0.1 * dt;
        }

        // === Curiosity dynamics ===
        // Curiosity increases with positive surprise, decreases with stress
        // Boredom also drives curiosity up (seeking novelty)
        let d_curiosity = input.surprise * 0.1 * input.content_valence.max(0.0)
            - fast.stress * 0.05
            + medium.openness * 0.02
            + fast.boredom * 0.03;
        fast.curiosity += d_curiosity * dt;

        // ADR-007: Curiosity vectorization — tag with topic when curiosity rises
        if d_curiosity > 0.0 {
            if let Some(ref topic) = input.topic_hint {
                let boost = (d_curiosity * dt).min(0.3);
                fast.curiosity_vector.tag_interest(topic, boost);
            }
        }
        // Decay existing interests slowly
        fast.curiosity_vector.decay(1.0 - 0.001 * dt);

        // === Social need dynamics ===
        // Increases when alone, decreases after social interaction
        let d_social = if input.is_social {
            -0.1 * fast.social_need // Satisfied by interaction
        } else {
            self.social_need_growth_rate * (self.social_need_target - fast.social_need)
        };
        fast.social_need += d_social * dt;

        // === Boredom dynamics ===
        // Increases with low-surprise, low-intensity input (monotony).
        // Decreases sharply with novel/surprising stimuli.
        let novelty = input.surprise * 0.5 + input.content_intensity * 0.3;
        let d_boredom = 0.01 * (1.0 - novelty)  // Monotony accumulation
            - novelty * 0.15                      // Novelty suppression
            - fast.stress * 0.01; // Stress also suppresses boredom
        fast.boredom += d_boredom * dt;

        // Normalize
        fast.normalize();
    }

    /// Medium dynamics: ds_medium/dt = F_medium(s_medium, s_slow, avg(s_fast))
    pub fn step_medium(
        &self,
        medium: &mut MediumState,
        fast: &FastState,
        _slow: &SlowState,
        input: &SensoryInput,
        dt: f32,
    ) {
        // Medium dynamics are much slower
        let dt_hours = dt / 3600.0;
        let tau = self.mood_time_constant;

        // === Mood bias ===
        // Integrates affect valence over time.
        // During idle (no stimulus), mood decays faster toward neutral — the
        // organism shouldn't stay sad for hours just because of one bad interaction.
        let has_stimulus = input.is_social || input.content_intensity > 0.01;
        let effective_tau = if has_stimulus { tau } else { tau * 0.3 }; // 3x faster idle recovery
        let d_mood = (fast.affect.valence - medium.mood_bias) / effective_tau;
        medium.mood_bias += d_mood * dt_hours;
        medium.mood_bias = medium.mood_bias.clamp(-1.0, 1.0);

        // === Openness ===
        // Influenced by curiosity and recent exploration outcomes
        let d_openness = (fast.curiosity * 0.5 - medium.openness) * 0.1;
        medium.openness += d_openness * dt_hours;
        medium.openness = medium.openness.clamp(0.0, 1.0);

        // === Hunger/Deprivation ===
        // Accumulates from unmet social needs
        let d_hunger = (fast.social_need - 0.5).max(0.0) * 0.1;
        medium.hunger += d_hunger * dt_hours;
        medium.hunger = medium.hunger.clamp(0.0, 1.0);

        // === Attachment ===
        // Updated based on interaction outcomes
        if input.is_social {
            let was_positive = input.content_valence > 0.0;
            medium
                .attachment
                .update_from_interaction(was_positive, input.response_delay_factor);
        }

        // Sanitize all medium state values (NaN/Inf guard)
        medium.normalize();
    }

    /// Slow dynamics: only called on crisis events
    ///
    /// Returns true if narrative collapse occurred
    pub fn step_slow_crisis(
        &self,
        slow: &mut SlowState,
        medium: &MediumState,
        crisis_intensity: f32,
    ) -> bool {
        // Narrative collapse threshold depends on rigidity
        let collapse_threshold = 0.5 + slow.rigidity * 0.4;

        if crisis_intensity > collapse_threshold {
            // Narrative collapse! Major personality shift possible

            // Reduce rigidity temporarily (plasticity window)
            slow.rigidity *= 0.7;

            // Narrative bias shifts based on crisis nature
            // (In real implementation, this would be determined by the crisis content)
            slow.narrative_bias = medium.mood_bias * 0.5;

            return true;
        }

        // Normal slow update: rigidity increases over time (belief solidification)
        slow.rigidity += 0.001 * (1.0 - slow.rigidity);
        slow.rigidity = slow.rigidity.clamp(0.0, 1.0);
        slow.narrative_bias = slow.narrative_bias.clamp(-1.0, 1.0);

        false
    }

    /// Apply moral cost from value violation
    pub fn apply_moral_cost(&self, fast: &mut FastState, cost: f32) {
        // Moral violations create immediate stress and energy depletion
        fast.stress += cost * 0.5;
        fast.energy -= cost * 0.3;

        // Also affects valence (guilt)
        fast.affect.valence -= cost * 0.2;

        fast.normalize();
    }
}

/// Compute homeostatic error (how far from equilibrium)
pub fn homeostatic_error(state: &OrganismState, dynamics: &DefaultDynamics) -> f32 {
    let e_err = (state.fast.energy - dynamics.learnable.energy_target).abs();
    let s_err = (state.fast.stress - dynamics.stress_target).abs();
    let sn_err = (state.fast.social_need - dynamics.social_need_target).abs();

    (e_err + s_err + sn_err) / 3.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stress_decay() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();
        state.fast.stress = 0.8; // High initial stress

        let input = SensoryInput::default();

        // Let stress decay
        for _ in 0..100 {
            dynamics.step(&mut state, &input, Duration::from_secs(60));
        }

        // Stress should have decayed toward target
        assert!(state.fast.stress < 0.5);
    }

    #[test]
    fn test_social_interaction_reduces_social_need() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();
        state.fast.social_need = 0.8;

        let input = SensoryInput {
            is_social: true,
            content_valence: 0.5,
            ..Default::default()
        };

        dynamics.step(&mut state, &input, Duration::from_secs(1));

        assert!(state.fast.social_need < 0.8);
    }

    #[test]
    fn test_negative_input_increases_stress() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();
        let initial_stress = state.fast.stress;

        let input = SensoryInput {
            content_valence: -0.8,
            content_intensity: 0.9,
            ..Default::default()
        };

        dynamics.step(&mut state, &input, Duration::from_secs(1));

        assert!(state.fast.stress > initial_stress);
    }

    #[test]
    fn test_nan_resistance() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();

        // Inject NaN into various state fields
        state.fast.energy = f32::NAN;
        state.fast.stress = f32::INFINITY;
        state.fast.curiosity = f32::NEG_INFINITY;
        state.fast.affect.valence = f32::NAN;
        state.medium.mood_bias = f32::NAN;
        state.medium.openness = f32::INFINITY;

        let input = SensoryInput::default();

        // Step should not panic, and should recover all values to finite
        dynamics.step(&mut state, &input, Duration::from_secs(1));

        assert!(
            state.fast.energy.is_finite(),
            "energy should be finite, got {}",
            state.fast.energy
        );
        assert!(
            state.fast.stress.is_finite(),
            "stress should be finite, got {}",
            state.fast.stress
        );
        assert!(
            state.fast.curiosity.is_finite(),
            "curiosity should be finite, got {}",
            state.fast.curiosity
        );
        assert!(
            state.fast.affect.valence.is_finite(),
            "valence should be finite, got {}",
            state.fast.affect.valence
        );
        assert!(
            state.fast.affect.arousal.is_finite(),
            "arousal should be finite, got {}",
            state.fast.affect.arousal
        );
        assert!(
            state.medium.mood_bias.is_finite(),
            "mood_bias should be finite, got {}",
            state.medium.mood_bias
        );
        assert!(
            state.medium.openness.is_finite(),
            "openness should be finite, got {}",
            state.medium.openness
        );

        // All should be in valid ranges
        assert!(state.fast.energy >= 0.0 && state.fast.energy <= 1.0);
        assert!(state.fast.stress >= 0.0 && state.fast.stress <= 1.0);
        assert!(state.medium.mood_bias >= -1.0 && state.medium.mood_bias <= 1.0);
    }

    #[test]
    fn test_extreme_dt_stability() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();

        // Very large dt (simulating long pause)
        let input = SensoryInput {
            content_valence: -1.0,
            content_intensity: 1.0,
            surprise: 1.0,
            ..Default::default()
        };

        // 24 hours in one step — shouldn't produce NaN or out-of-range
        dynamics.step(&mut state, &input, Duration::from_secs(86400));

        assert!(
            state.fast.energy.is_finite() && state.fast.energy >= 0.0 && state.fast.energy <= 1.0
        );
        assert!(
            state.fast.stress.is_finite() && state.fast.stress >= 0.0 && state.fast.stress <= 1.0
        );
        assert!(
            state.medium.mood_bias.is_finite()
                && state.medium.mood_bias >= -1.0
                && state.medium.mood_bias <= 1.0
        );
    }

    #[test]
    fn test_boredom_increases_with_monotony() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();
        let initial_boredom = state.fast.boredom;

        // Low surprise, low intensity = monotony
        let input = SensoryInput {
            content_valence: 0.0,
            content_intensity: 0.1,
            surprise: 0.0,
            ..Default::default()
        };

        for _ in 0..100 {
            dynamics.step(&mut state, &input, Duration::from_secs(1));
        }

        assert!(
            state.fast.boredom > initial_boredom,
            "Boredom should increase with monotony: {} > {}",
            state.fast.boredom,
            initial_boredom
        );
    }

    #[test]
    fn test_boredom_decreases_with_novelty() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();
        state.fast.boredom = 0.8; // Start bored

        // High surprise = novelty
        let input = SensoryInput {
            content_valence: 0.5,
            content_intensity: 0.8,
            surprise: 0.9,
            ..Default::default()
        };

        for _ in 0..50 {
            dynamics.step(&mut state, &input, Duration::from_secs(1));
        }

        assert!(
            state.fast.boredom < 0.5,
            "Boredom should decrease with novelty: {}",
            state.fast.boredom
        );
    }

    #[test]
    fn test_slow_crisis_no_collapse() {
        let dynamics = DefaultDynamics::default();
        let mut slow = SlowState::default();
        let medium = MediumState::default();
        let initial_rigidity = slow.rigidity;

        // Low intensity crisis — should NOT collapse
        let collapsed = dynamics.step_slow_crisis(&mut slow, &medium, 0.2);

        assert!(!collapsed);
        // Rigidity should increase slightly (belief solidification)
        assert!(slow.rigidity > initial_rigidity);
    }

    #[test]
    fn test_slow_crisis_collapse() {
        let dynamics = DefaultDynamics::default();
        let mut slow = SlowState::default();
        let medium = MediumState::default();

        // Very high intensity crisis — should collapse
        let collapsed = dynamics.step_slow_crisis(&mut slow, &medium, 1.0);

        assert!(collapsed);
        // Rigidity should decrease (plasticity window)
        assert!(slow.rigidity < SlowState::default().rigidity);
    }

    #[test]
    fn test_slow_crisis_rigidity_affects_threshold() {
        let dynamics = DefaultDynamics::default();

        // High rigidity → higher collapse threshold
        let mut slow_rigid = SlowState::default();
        slow_rigid.rigidity = 0.9;
        let medium = MediumState::default();
        let collapsed_rigid = dynamics.step_slow_crisis(&mut slow_rigid, &medium, 0.8);

        // Low rigidity → lower collapse threshold
        let mut slow_flexible = SlowState::default();
        slow_flexible.rigidity = 0.1;
        let collapsed_flexible = dynamics.step_slow_crisis(&mut slow_flexible, &medium, 0.8);

        // Same crisis intensity: flexible personality collapses, rigid one doesn't
        assert!(
            !collapsed_rigid,
            "High rigidity should resist collapse at 0.8"
        );
        assert!(collapsed_flexible, "Low rigidity should collapse at 0.8");
    }

    #[test]
    fn test_apply_moral_cost() {
        let dynamics = DefaultDynamics::default();
        let mut fast = FastState::default();
        let initial_stress = fast.stress;
        let initial_energy = fast.energy;
        let initial_valence = fast.affect.valence;

        dynamics.apply_moral_cost(&mut fast, 0.6);

        assert!(
            fast.stress > initial_stress,
            "Moral cost should increase stress"
        );
        assert!(
            fast.energy < initial_energy,
            "Moral cost should decrease energy"
        );
        assert!(
            fast.affect.valence < initial_valence,
            "Moral cost should decrease valence (guilt)"
        );
        // Values should remain in valid range
        assert!(fast.stress >= 0.0 && fast.stress <= 1.0);
        assert!(fast.energy >= 0.0 && fast.energy <= 1.0);
    }

    #[test]
    fn test_homeostatic_error() {
        let dynamics = DefaultDynamics::default();

        // At equilibrium, error should be near zero
        let mut state = OrganismState::default();
        state.fast.energy = dynamics.learnable.energy_target;
        state.fast.stress = dynamics.stress_target;
        state.fast.social_need = dynamics.social_need_target;
        let err = homeostatic_error(&state, &dynamics);
        assert!(
            err < 0.01,
            "Error at equilibrium should be near zero, got {}",
            err
        );

        // Far from equilibrium, error should be high
        state.fast.energy = 0.0;
        state.fast.stress = 1.0;
        state.fast.social_need = 1.0;
        let err = homeostatic_error(&state, &dynamics);
        assert!(
            err > 0.3,
            "Error far from equilibrium should be high, got {}",
            err
        );
    }

    /// Regression test: organism with negative mood should converge toward
    /// homeostasis during idle, NOT diverge into a stress death spiral.
    #[test]
    fn test_idle_convergence_with_negative_mood() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();

        // Start with the problematic state the user reported:
        // negative mood, high stress, low energy
        state.fast.energy = 0.56;
        state.fast.stress = 0.67;
        state.medium.mood_bias = -0.35;
        state.fast.affect.valence = -0.2;

        let input = SensoryInput::default(); // Idle — no stimulus

        // Simulate 10 minutes of idle ticks (600 x 1-second steps)
        for _ in 0..600 {
            dynamics.step(&mut state, &input, Duration::from_secs(1));
        }

        // After 10 minutes idle, stress should have DECREASED toward target (0.2)
        assert!(
            state.fast.stress < 0.5,
            "Stress should decrease during idle, got {:.3}",
            state.fast.stress
        );

        // Energy should have INCREASED toward target (0.7)
        assert!(
            state.fast.energy > 0.6,
            "Energy should recover during idle, got {:.3}",
            state.fast.energy
        );

        // Mood bias should have moved toward neutral
        assert!(
            state.medium.mood_bias > -0.3,
            "Mood bias should recover toward neutral during idle, got {:.3}",
            state.medium.mood_bias
        );
    }

    #[test]
    fn test_curiosity_vector_topic_tagging() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();

        // Use step_fast directly to isolate curiosity vectorization logic
        let input = SensoryInput {
            content_valence: 0.8,
            content_intensity: 0.5,
            surprise: 0.9,
            topic_hint: Some("量子计算".to_string()),
            ..Default::default()
        };

        let medium = state.medium.clone();
        for _ in 0..10 {
            dynamics.step_fast(&mut state.fast, &medium, &input, 1.0);
        }

        let top = state.fast.curiosity_vector.top_interests(3);
        assert!(
            !top.is_empty(),
            "Should have tagged curiosity interest from topic_hint"
        );
        assert_eq!(top[0].0, "量子计算");
    }

    #[test]
    fn test_curiosity_vector_decay() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();

        // Pre-seed an interest
        state.fast.curiosity_vector.tag_interest("音乐", 0.8);

        // Run idle ticks — interests should decay
        let input = SensoryInput::default();
        for _ in 0..500 {
            dynamics.step(&mut state, &input, Duration::from_secs(1));
        }

        let top = state.fast.curiosity_vector.top_interests(3);
        if !top.is_empty() {
            assert!(
                top[0].1 < 0.8,
                "Interest should have decayed, got {}",
                top[0].1
            );
        }
    }

    #[test]
    fn test_curiosity_vector_interest_ranking() {
        use crate::state::CuriosityVector;

        let mut cv = CuriosityVector::default();
        cv.tag_interest("哲学", 0.3);
        cv.tag_interest("编程", 0.9);
        cv.tag_interest("音乐", 0.6);

        let top = cv.top_interests(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "编程");
        assert_eq!(top[1].0, "音乐");
    }
}
