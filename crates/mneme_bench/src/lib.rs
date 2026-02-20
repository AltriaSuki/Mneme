//! mneme_bench — trajectory simulation tests for organism dynamics.
//!
//! Validates emergent behavior over long simulated time spans:
//! - 72h silent decay (homeostatic recovery)
//! - Trauma imprinting (stress spike + slow recovery)
//! - Species differentiation (different LearnableDynamics → different trajectories)

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
