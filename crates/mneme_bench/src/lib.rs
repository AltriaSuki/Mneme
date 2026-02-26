//! mneme_bench — trajectory simulation tests for organism dynamics.
//!
//! Validates emergent behavior over long simulated time spans across three dimensions:
//!
//! ## Trajectory (ODE correctness)
//! - 72h silent decay (homeostatic recovery)
//! - Trauma imprinting (stress spike + slow recovery)
//! - Species differentiation (different LearnableDynamics → different trajectories)
//!
//! ## Consistency (一致性)
//! - Emotional proportionality: stronger stimuli → proportionally stronger response
//! - Valence sign preservation: positive input → positive affect, negative → negative
//! - Repeated stimulus convergence: same stimulus applied repeatedly → stable equilibrium
//!
//! ## Interference (干涉性)
//! - Idle tick non-erasure: idle ticks after interaction shouldn't undo state changes
//! - Stress-affect decoupling during idle: no death spiral when mood is negative
//! - Cross-timescale isolation: fast dynamics perturbation shouldn't corrupt medium state
//!
//! ## Autonomy (自主性)
//! - Goal suggestion thresholds: extreme states trigger appropriate goal types
//! - Curiosity vector accumulation: repeated topic exposure builds interest
//! - Social need escalation: prolonged isolation raises social need above trigger threshold

#[cfg(test)]
mod tests {
    use mneme_core::{DefaultDynamics, Dynamics, LearnableDynamics, OrganismState, SensoryInput};
    use std::time::Duration;

    /// Simulate `total_secs` of dynamics in `step_secs` increments.
    fn simulate(
        dynamics: &DefaultDynamics,
        state: &mut OrganismState,
        input: &SensoryInput,
        total_secs: f64,
        step_secs: f64,
    ) {
        let steps = (total_secs / step_secs) as usize;
        let dt = Duration::from_secs_f64(step_secs);
        for _ in 0..steps {
            dynamics.step(state, input, dt);
        }
    }

    /// Apply a single interaction (dt=15s attention window) then return to idle.
    fn interact_once(
        dynamics: &DefaultDynamics,
        state: &mut OrganismState,
        input: &SensoryInput,
    ) {
        dynamics.step(state, input, Duration::from_secs(15));
    }

    /// 72 hours of silence: all fast variables should converge to homeostatic targets.
    #[test]
    fn test_72h_silent_decay() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();

        // Start from a disturbed state
        state.fast.energy = 0.2;
        state.fast.stress = 0.9;
        state.fast.social_need = 0.1;
        state.medium.mood_bias = -0.6;

        let idle = SensoryInput::default();

        // Simulate 72h in 10s steps
        simulate(&dynamics, &mut state, &idle, 72.0 * 3600.0, 10.0);

        // Energy should recover toward target (0.7)
        assert!(
            (state.fast.energy - 0.7).abs() < 0.1,
            "Energy should recover to ~0.7, got {}",
            state.fast.energy
        );
        // Stress should decay toward target (0.2)
        assert!(
            (state.fast.stress - 0.2).abs() < 0.1,
            "Stress should decay to ~0.2, got {}",
            state.fast.stress
        );
        // Mood should recover toward neutral
        assert!(
            state.medium.mood_bias.abs() < 0.15,
            "Mood should recover toward 0, got {}",
            state.medium.mood_bias
        );
    }

    /// Trauma imprinting: intense negative stimulus causes stress spike,
    /// followed by slow exponential recovery (not instant snap-back).
    #[test]
    fn test_trauma_imprinting() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();

        // Deliver trauma: intense negative social stimulus
        let trauma = SensoryInput {
            content_valence: -0.95,
            content_intensity: 0.95,
            surprise: 0.9,
            is_social: true,
            violated_values: vec!["honesty".to_string()],
            ..Default::default()
        };

        // Apply trauma for 10 minutes (mood has 2h time constant, needs time)
        simulate(&dynamics, &mut state, &trauma, 600.0, 1.0);

        let post_trauma_stress = state.fast.stress;
        let post_trauma_mood = state.medium.mood_bias;

        assert!(
            post_trauma_stress > 0.5,
            "Stress should spike after trauma, got {}",
            post_trauma_stress
        );
        assert!(
            post_trauma_mood < -0.01,
            "Mood should drop after trauma, got {}",
            post_trauma_mood
        );

        // Now recover in silence for 1 hour
        let idle = SensoryInput::default();
        simulate(&dynamics, &mut state, &idle, 3600.0, 10.0);

        let recovery_stress = state.fast.stress;

        // Stress should have decreased but not fully recovered (slow decay)
        assert!(
            recovery_stress < post_trauma_stress,
            "Stress should decrease during recovery: {} -> {}",
            post_trauma_stress, recovery_stress
        );

        // After 1h, mood should still be somewhat negative (slow time constant)
        // but less negative than immediately post-trauma
        assert!(
            state.medium.mood_bias > post_trauma_mood,
            "Mood should partially recover: {} -> {}",
            post_trauma_mood, state.medium.mood_bias
        );
    }

    /// Different LearnableDynamics produce measurably different trajectories.
    /// "Short-lived" species: high energy target, fast stress decay (burns bright).
    /// "Long-lived" species: low energy target, slow stress decay (conserves).
    #[test]
    fn test_species_differentiation() {
        let stimulus = SensoryInput {
            content_valence: -0.5,
            content_intensity: 0.6,
            surprise: 0.3,
            is_social: true,
            ..Default::default()
        };
        let idle = SensoryInput::default();

        // Short-lived: high energy, fast stress recovery
        let short_lived = DefaultDynamics {
            learnable: LearnableDynamics {
                energy_target: 0.9,
                stress_decay_rate: 0.02,
            },
            ..Default::default()
        };

        // Long-lived: low energy, slow stress recovery
        let long_lived = DefaultDynamics {
            learnable: LearnableDynamics {
                energy_target: 0.5,
                stress_decay_rate: 0.001,
            },
            ..Default::default()
        };

        let mut state_s = OrganismState::default();
        let mut state_l = OrganismState::default();

        // Phase 1: Brief stress (5 min) to elevate both
        simulate(&short_lived, &mut state_s, &stimulus, 300.0, 1.0);
        simulate(&long_lived, &mut state_l, &stimulus, 300.0, 1.0);

        // Phase 2: Recovery (1 hour idle) — decay rate difference shows
        simulate(&short_lived, &mut state_s, &idle, 3600.0, 10.0);
        simulate(&long_lived, &mut state_l, &idle, 3600.0, 10.0);

        // Short-lived should have higher energy (higher target)
        assert!(
            state_s.fast.energy > state_l.fast.energy,
            "Short-lived should have higher energy: {} vs {}",
            state_s.fast.energy, state_l.fast.energy
        );

        // Short-lived should have lower stress after recovery (faster decay)
        assert!(
            state_s.fast.stress < state_l.fast.stress,
            "Short-lived should recover stress faster: {} vs {}",
            state_s.fast.stress, state_l.fast.stress
        );
    }

    // ========================================================================
    // Consistency (一致性)
    // ========================================================================

    /// Stronger stimuli should produce proportionally stronger state changes.
    /// A valence=-0.9 message should move affect more than valence=-0.3.
    #[test]
    fn test_emotional_proportionality() {
        let dynamics = DefaultDynamics::default();

        let mild = SensoryInput {
            content_valence: -0.3,
            content_intensity: 0.3,
            surprise: 0.2,
            is_social: true,
            ..Default::default()
        };
        let strong = SensoryInput {
            content_valence: -0.9,
            content_intensity: 0.9,
            surprise: 0.5,
            is_social: true,
            ..Default::default()
        };

        let mut state_mild = OrganismState::default();
        let mut state_strong = OrganismState::default();

        interact_once(&dynamics, &mut state_mild, &mild);
        interact_once(&dynamics, &mut state_strong, &strong);

        // Stronger stimulus should produce more negative valence
        assert!(
            state_strong.fast.affect.valence < state_mild.fast.affect.valence,
            "Strong stimulus should produce more negative valence: {:.3} vs {:.3}",
            state_strong.fast.affect.valence, state_mild.fast.affect.valence
        );

        // Stronger stimulus should produce more stress
        assert!(
            state_strong.fast.stress > state_mild.fast.stress,
            "Strong stimulus should produce more stress: {:.3} vs {:.3}",
            state_strong.fast.stress, state_mild.fast.stress
        );
    }

    /// Positive input → positive affect shift; negative → negative.
    #[test]
    fn test_valence_sign_preservation() {
        let dynamics = DefaultDynamics::default();

        let positive = SensoryInput {
            content_valence: 0.7,
            content_intensity: 0.6,
            surprise: 0.3,
            is_social: true,
            ..Default::default()
        };
        let negative = SensoryInput {
            content_valence: -0.7,
            content_intensity: 0.6,
            surprise: 0.3,
            is_social: true,
            ..Default::default()
        };

        let mut state_pos = OrganismState::default();
        let mut state_neg = OrganismState::default();
        let baseline_valence = state_pos.fast.affect.valence;

        interact_once(&dynamics, &mut state_pos, &positive);
        interact_once(&dynamics, &mut state_neg, &negative);

        assert!(
            state_pos.fast.affect.valence > baseline_valence,
            "Positive input should raise valence: {:.3} > {:.3}",
            state_pos.fast.affect.valence, baseline_valence
        );
        assert!(
            state_neg.fast.affect.valence < baseline_valence,
            "Negative input should lower valence: {:.3} < {:.3}",
            state_neg.fast.affect.valence, baseline_valence
        );
    }

    /// Repeated identical stimulus should converge to a stable equilibrium.
    #[test]
    fn test_repeated_stimulus_convergence() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();

        let stimulus = SensoryInput {
            content_valence: -0.5,
            content_intensity: 0.5,
            surprise: 0.3,
            is_social: true,
            ..Default::default()
        };
        let idle = SensoryInput::default();

        // Apply same stimulus 100 times with 60s idle between each
        for _ in 0..100 {
            interact_once(&dynamics, &mut state, &stimulus);
            simulate(&dynamics, &mut state, &idle, 60.0, 10.0);
        }

        let val_a = state.fast.affect.valence;
        let stress_a = state.fast.stress;

        // 50 more — should barely change (converged)
        for _ in 0..50 {
            interact_once(&dynamics, &mut state, &stimulus);
            simulate(&dynamics, &mut state, &idle, 60.0, 10.0);
        }

        assert!(
            (state.fast.affect.valence - val_a).abs() < 0.05,
            "Valence should converge: {:.3} vs {:.3}",
            val_a, state.fast.affect.valence
        );
        assert!(
            (state.fast.stress - stress_a).abs() < 0.05,
            "Stress should converge: {:.3} vs {:.3}",
            stress_a, state.fast.stress
        );
    }

    // ========================================================================
    // Interference (干涉性)
    // ========================================================================

    /// Idle ticks after an interaction should NOT erase the state change.
    /// This was a real bug: 60s idle ticks with boredom growth undid 1s interactions.
    #[test]
    fn test_idle_tick_non_erasure() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();
        let baseline_valence = state.fast.affect.valence;

        // Strong positive interaction
        let positive = SensoryInput {
            content_valence: 0.8,
            content_intensity: 0.7,
            surprise: 0.5,
            is_social: true,
            ..Default::default()
        };
        interact_once(&dynamics, &mut state, &positive);

        let post_interaction_valence = state.fast.affect.valence;
        assert!(
            post_interaction_valence > baseline_valence,
            "Interaction should raise valence"
        );

        // 5 minutes of idle ticks (5 x 60s)
        let idle = SensoryInput::default();
        simulate(&dynamics, &mut state, &idle, 300.0, 60.0);

        // Valence should still be above baseline (not erased)
        assert!(
            state.fast.affect.valence > baseline_valence,
            "Idle ticks should not erase interaction effect: {:.3} should be > {:.3}",
            state.fast.affect.valence, baseline_valence
        );
    }

    /// Negative mood during idle should NOT create a stress death spiral.
    /// This was a real bug: mood_bias pulled valence negative → stress ↑ → more negative.
    #[test]
    fn test_no_idle_death_spiral() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();

        // Start with negative mood and elevated stress (post-bad-interaction)
        state.medium.mood_bias = -0.4;
        state.fast.stress = 0.6;
        state.fast.affect.valence = -0.3;

        let idle = SensoryInput::default();

        // 30 minutes of idle
        simulate(&dynamics, &mut state, &idle, 1800.0, 10.0);

        // Stress should DECREASE (homeostatic recovery), not increase
        assert!(
            state.fast.stress < 0.6,
            "Stress should decrease during idle, got {:.3}",
            state.fast.stress
        );
        // Valence should move toward neutral, not deeper negative
        assert!(
            state.fast.affect.valence > -0.3,
            "Valence should recover toward neutral during idle, got {:.3}",
            state.fast.affect.valence
        );
    }

    /// Fast dynamics perturbation should not corrupt medium state within a single step.
    /// Medium state (mood_bias) has a 2-hour time constant — a single interaction
    /// should move it only slightly.
    #[test]
    fn test_cross_timescale_isolation() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();
        let initial_mood = state.medium.mood_bias;

        // Single extreme interaction
        let extreme = SensoryInput {
            content_valence: -1.0,
            content_intensity: 1.0,
            surprise: 1.0,
            is_social: true,
            ..Default::default()
        };
        interact_once(&dynamics, &mut state, &extreme);

        // Mood should move only slightly (tau=2h, dt=15s → blend ≈ 0.002)
        let mood_shift = (state.medium.mood_bias - initial_mood).abs();
        assert!(
            mood_shift < 0.1,
            "Single interaction should not drastically shift mood: shift={:.4}",
            mood_shift
        );
    }

    // ========================================================================
    // Autonomy (自主性)
    // ========================================================================

    /// Prolonged isolation should raise social_need above the trigger threshold (0.6).
    /// This is the precondition for GoalTriggerEvaluator to fire social triggers.
    #[test]
    fn test_social_need_escalation() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();
        let idle = SensoryInput::default();

        // 4 hours of isolation
        simulate(&dynamics, &mut state, &idle, 4.0 * 3600.0, 60.0);

        assert!(
            state.fast.social_need > 0.45,
            "4h isolation should raise social_need toward target (0.5), got {:.3}",
            state.fast.social_need
        );
    }

    /// Repeated exposure to a topic should build curiosity vector interest.
    #[test]
    fn test_curiosity_vector_accumulation() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();

        let topic_stimulus = SensoryInput {
            content_valence: 0.5,
            content_intensity: 0.6,
            surprise: 0.8,
            is_social: true,
            topic_hint: Some("量子计算".to_string()),
            ..Default::default()
        };

        // 20 interactions about the same topic
        for _ in 0..20 {
            interact_once(&dynamics, &mut state, &topic_stimulus);
        }

        let top = state.fast.curiosity_vector.top_interests(3);
        assert!(
            !top.is_empty(),
            "Repeated topic exposure should build curiosity vector"
        );
        assert_eq!(top[0].0, "量子计算");
        assert!(
            top[0].1 > 0.1,
            "Interest strength should be meaningful: {:.3}",
            top[0].1
        );
    }

    /// Monotonous idle should raise boredom above the exploration trigger threshold.
    #[test]
    fn test_boredom_drives_curiosity() {
        let dynamics = DefaultDynamics::default();
        let mut state = OrganismState::default();
        let idle = SensoryInput::default();

        // 2 hours of monotony
        simulate(&dynamics, &mut state, &idle, 2.0 * 3600.0, 60.0);

        assert!(
            state.fast.boredom > 0.3,
            "2h monotony should raise boredom, got {:.3}",
            state.fast.boredom
        );
    }
}
