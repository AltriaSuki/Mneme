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

// === Learnable dynamics bounds ===
const LEARNING_RATE: f32 = 0.005;
const ENERGY_TARGET_MIN: f32 = 0.3;
const ENERGY_TARGET_MAX: f32 = 0.9;
const STRESS_DECAY_MIN: f32 = 0.001;
const STRESS_DECAY_MAX: f32 = 0.02;

// === Fast dynamics: activity costs ===
const SOCIAL_ACTIVITY_COST: f32 = 0.01;
const STIMULUS_ACTIVITY_COST: f32 = 0.002;

// === Fast dynamics: stress ===
const SURPRISE_STRESS_FACTOR: f32 = 0.3;
const MORAL_COST_BASE: f32 = 0.5;

// === Fast dynamics: affect ===
const MOOD_INFLUENCE_FACTOR: f32 = 0.3;
const AROUSAL_INTENSITY_WEIGHT: f32 = 0.5;
const AROUSAL_SURPRISE_WEIGHT: f32 = 0.3;
const AROUSAL_BASELINE: f32 = 0.2;
const AFFECT_RATE: f32 = 0.1;
const STRESS_VALENCE_COUPLING: f32 = 0.1;

// === Fast dynamics: curiosity ===
const CURIOSITY_DECAY_RATE: f32 = 0.001;

// === Fast dynamics: social ===
const SOCIAL_SATISFACTION_FACTOR: f32 = 0.1;

// === Fast dynamics: boredom ===
const BOREDOM_MONOTONY_RATE: f32 = 0.01;
const BOREDOM_NOVELTY_SUPPRESSION: f32 = 0.15;
const BOREDOM_STRESS_SUPPRESSION: f32 = 0.01;
const NOVELTY_INTENSITY_WEIGHT: f32 = 0.3;

// === Medium dynamics ===
const IDLE_TAU_MULTIPLIER: f32 = 0.3;

// === Slow dynamics ===
const COLLAPSE_BASE_THRESHOLD: f32 = 0.5;
const COLLAPSE_RIGIDITY_WEIGHT: f32 = 0.4;
const RIGIDITY_COLLAPSE_FACTOR: f32 = 0.7;
const RIGIDITY_GROWTH_RATE: f32 = 0.001;

// === Moral cost distribution ===
const MORAL_STRESS_FACTOR: f32 = 0.5;
const MORAL_ENERGY_FACTOR: f32 = 0.3;
const MORAL_VALENCE_FACTOR: f32 = 0.2;

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
        let lr = LEARNING_RATE;
        let n = samples.len() as f32;

        // Energy target: move toward energy levels that got positive feedback
        let energy_signal: f32 = samples
            .iter()
            .map(|(e, _, fv)| fv * (e - self.energy_target))
            .sum::<f32>()
            / n;
        self.energy_target = (self.energy_target + lr * energy_signal).clamp(ENERGY_TARGET_MIN, ENERGY_TARGET_MAX);

        // Stress decay: increase if high stress correlates with negative feedback
        let stress_signal: f32 = samples.iter().map(|(_, s, fv)| -fv * s).sum::<f32>() / n;
        self.stress_decay_rate =
            (self.stress_decay_rate + lr * 0.001 * stress_signal).clamp(STRESS_DECAY_MIN, STRESS_DECAY_MAX);

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
            SOCIAL_ACTIVITY_COST
        } else if has_stimulus {
            STIMULUS_ACTIVITY_COST
        } else {
            0.0
        };
        // === Energy dynamics ===
        // Exponential recovery toward target (unconditionally dt-stable).
        // Activity cost is an impulse (one-time event per interaction, not scaled by dt).
        let energy_blend = 1.0 - (-self.energy_recovery_rate * dt).exp();
        fast.energy += energy_blend * (self.learnable.energy_target - fast.energy);
        fast.energy -= activity_cost; // impulse, not *= dt

        // === Stress dynamics ===
        // Exponential decay toward target (dt-stable) + impulse from stimulus.
        // Euler `d_stress * dt` caused stress spikes when dt > 1 (e.g. dt=15 for
        // interaction attention window).
        let negative_stimulus = (-input.content_valence).max(0.0) * input.content_intensity;
        let surprise_stress = input.surprise * SURPRISE_STRESS_FACTOR;

        // Moral cost from value violations creates direct stress
        let moral_stress = if !input.violated_values.is_empty() {
            MORAL_COST_BASE // Base moral cost, refined in state.rs
        } else {
            0.0
        };

        let stress_blend = 1.0 - (-self.learnable.stress_decay_rate * dt).exp();
        fast.stress += stress_blend * (self.stress_target - fast.stress);
        // Stimulus as impulse (not scaled by dt)
        fast.stress += self.stress_sensitivity * (negative_stimulus + surprise_stress + moral_stress);

        // === Affect dynamics ===
        // Affect moves toward stimulus-induced target.
        // During idle (no stimulus), affect decays toward neutral — mood_bias
        // should NOT pull valence negative when nothing is happening, otherwise
        // a negative mood creates a feedback loop: negative affect → stress ↑ →
        // more negative affect → stress ↑↑ (death spiral).
        let mood_influence = if has_stimulus {
            medium.mood_bias * MOOD_INFLUENCE_FACTOR
        } else {
            0.0
        };
        let target_valence = input.content_valence * self.affect_sensitivity + mood_influence;
        let target_arousal = input.content_intensity * AROUSAL_INTENSITY_WEIGHT + input.surprise * AROUSAL_SURPRISE_WEIGHT + AROUSAL_BASELINE;

        // Exact exponential blend — unconditionally stable for any dt.
        // Euler `rate * dt` overshoots when rate*dt > 1 (e.g. dt=60).
        let affect_blend = 1.0 - (-AFFECT_RATE * dt).exp();
        fast.affect.valence += affect_blend * (target_valence - fast.affect.valence);
        fast.affect.arousal += affect_blend * (target_arousal - fast.affect.arousal);

        // Stress pulls valence down — but only during active interaction.
        // During idle, this coupling creates a positive feedback loop that
        // prevents homeostatic recovery.
        if has_stimulus {
            // Cap dt contribution to avoid overshoot with large dt
            fast.affect.valence -= fast.stress * STRESS_VALENCE_COUPLING * dt.min(2.0);
        }

        // === Curiosity dynamics ===
        // Uses exponential approach toward a stimulus-driven target (dt-stable).
        // Stress is folded into the target to avoid linear drag exploding with large dt.
        let curiosity_target = (input.surprise * input.content_valence.max(0.0)
            + medium.openness * 0.4
            + fast.boredom * 0.5
            - fast.stress * 0.3)
            .clamp(0.0, 1.0);
        let curiosity_blend = 1.0 - (-0.02 * dt).exp();
        let d_curiosity = curiosity_target - fast.curiosity; // sign for topic tagging
        fast.curiosity += curiosity_blend * d_curiosity;

        // ADR-007: Curiosity vectorization — tag topic on any meaningful stimulus.
        // Decoupled from d_curiosity sign: even if overall curiosity is falling,
        // a surprising topic should still register as an interest.
        if input.surprise > 0.1 {
            if let Some(ref topic) = input.topic_hint {
                let boost = (input.surprise * 0.3).min(0.3);
                fast.curiosity_vector.tag_interest(topic, boost);
            }
        }
        // Decay existing interests slowly (exact exponential for dt-stability)
        fast.curiosity_vector.decay((-CURIOSITY_DECAY_RATE * dt).exp());

        // === Social need dynamics ===
        // Exponential blend for dt-stability. Social interaction is an impulse.
        if input.is_social {
            fast.social_need -= SOCIAL_SATISFACTION_FACTOR * fast.social_need; // impulse
        } else {
            let social_blend = 1.0 - (-self.social_need_growth_rate * dt).exp();
            fast.social_need += social_blend * (self.social_need_target - fast.social_need);
        }

        // === Boredom dynamics ===
        // Uses exact exponential blend toward a target (unconditionally dt-stable).
        let novelty = input.surprise * AROUSAL_INTENSITY_WEIGHT + input.content_intensity * NOVELTY_INTENSITY_WEIGHT;
        let boredom_target = (1.0 - novelty * 2.0).max(0.0);
        let boredom_blend = 1.0 - (-BOREDOM_MONOTONY_RATE * dt).exp();
        fast.boredom += boredom_blend * (boredom_target - fast.boredom)
            - novelty * BOREDOM_NOVELTY_SUPPRESSION * dt.min(2.0)
            - fast.stress * BOREDOM_STRESS_SUPPRESSION * dt.min(2.0);

        // ADR-019: Propagate environment metrics into fast state
        fast.env = input.env.clone();

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
        // Uses exponential blend (dt-stable) instead of Euler, which overshoots
        // when dt_hours is large (e.g. user returns after 24h).
        let has_stimulus = input.is_social || input.content_intensity > 0.01;
        let effective_tau = if has_stimulus { tau } else { tau * IDLE_TAU_MULTIPLIER }; // 3x faster idle recovery
        let mood_blend = 1.0 - (-dt_hours / effective_tau).exp();
        medium.mood_bias += mood_blend * (fast.affect.valence - medium.mood_bias);
        medium.mood_bias = medium.mood_bias.clamp(-1.0, 1.0);

        // === Openness ===
        // Influenced by curiosity and recent exploration outcomes.
        // Rate 0.1/hour → exponential blend for dt-stability.
        let openness_blend = 1.0 - (-0.1 * dt_hours).exp();
        medium.openness += openness_blend * (fast.curiosity * 0.5 - medium.openness);
        medium.openness = medium.openness.clamp(0.0, 1.0);

        // === Hunger/Deprivation ===
        // Accumulates from unmet social needs.
        // Rate 0.1/hour → exponential blend for dt-stability.
        let hunger_target = (fast.social_need - 0.5).max(0.0);
        let hunger_blend = 1.0 - (-0.1 * dt_hours).exp();
        medium.hunger += hunger_blend * (hunger_target - medium.hunger);
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
        let collapse_threshold = COLLAPSE_BASE_THRESHOLD + slow.rigidity * COLLAPSE_RIGIDITY_WEIGHT;

        if crisis_intensity > collapse_threshold {
            // Narrative collapse! Major personality shift possible

            // Reduce rigidity temporarily (plasticity window)
            slow.rigidity *= RIGIDITY_COLLAPSE_FACTOR;

            // Narrative bias shifts based on crisis nature
            // (In real implementation, this would be determined by the crisis content)
            slow.narrative_bias = medium.mood_bias * 0.5;

            return true;
        }

        // Normal slow update: rigidity increases over time (belief solidification)
        slow.rigidity += RIGIDITY_GROWTH_RATE * (1.0 - slow.rigidity);
        slow.rigidity = slow.rigidity.clamp(0.0, 1.0);
        slow.narrative_bias = slow.narrative_bias.clamp(-1.0, 1.0);

        false
    }

    /// Apply moral cost from value violation
    pub fn apply_moral_cost(&self, fast: &mut FastState, cost: f32) {
        // Moral violations create immediate stress and energy depletion
        fast.stress += cost * MORAL_STRESS_FACTOR;
        fast.energy -= cost * MORAL_ENERGY_FACTOR;

        // Also affects valence (guilt)
        fast.affect.valence -= cost * MORAL_VALENCE_FACTOR;

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
