//! State Dynamics: The differential equations governing state evolution
//! 
//! ds/dt = F(s, i, t) where:
//! - s = (s_fast, s_medium, s_slow) is the organism state
//! - i = sensory input
//! - t = time
//!
//! The dynamics are separated by time-scale to ensure stability.

use std::time::Duration;
use crate::state::{OrganismState, FastState, MediumState, SlowState, SensoryInput};

/// Trait for implementing state dynamics
pub trait Dynamics: Send + Sync {
    /// Advance the state by dt given sensory input
    fn step(&self, state: &mut OrganismState, input: &SensoryInput, dt: Duration);
}

/// Default ODE-based dynamics implementation
/// 
/// Uses simple exponential decay/growth models. Can be replaced with
/// neural network (Burn/Candle) for learned dynamics.
#[derive(Debug, Clone)]
pub struct DefaultDynamics {
    /// Homeostatic targets
    pub energy_target: f32,
    pub stress_target: f32,
    pub social_need_target: f32,
    
    /// Decay/recovery rates (per second)
    pub energy_recovery_rate: f32,
    pub stress_decay_rate: f32,
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
            energy_target: 0.7,
            stress_target: 0.2,
            social_need_target: 0.5,
            
            energy_recovery_rate: 0.001,   // ~0.06/min
            stress_decay_rate: 0.002,      // ~0.12/min
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
    pub fn step_fast(&self, fast: &mut FastState, medium: &MediumState, input: &SensoryInput, dt: f32) {
        // === Energy dynamics ===
        // dE/dt = recovery_rate * (target - E) - activity_cost
        let activity_cost = if input.is_social { 0.01 } else { 0.002 };
        let d_energy = self.energy_recovery_rate * (self.energy_target - fast.energy) - activity_cost;
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
        
        let d_stress = -self.stress_decay_rate * (fast.stress - self.stress_target)
            + self.stress_sensitivity * (negative_stimulus + surprise_stress + moral_stress);
        fast.stress += d_stress * dt;
        
        // === Affect dynamics ===
        // Affect moves toward stimulus-induced target
        let target_valence = input.content_valence * self.affect_sensitivity 
            + medium.mood_bias * 0.3; // Biased by mood
        let target_arousal = input.content_intensity * 0.5 + input.surprise * 0.3 + 0.2;
        
        let affect_rate = 0.1; // How quickly affect changes
        fast.affect.valence += affect_rate * (target_valence - fast.affect.valence) * dt;
        fast.affect.arousal += affect_rate * (target_arousal - fast.affect.arousal) * dt;
        
        // Stress pulls valence down
        fast.affect.valence -= fast.stress * 0.1 * dt;
        
        // === Curiosity dynamics ===
        // Curiosity increases with positive surprise, decreases with stress
        let d_curiosity = input.surprise * 0.1 * input.content_valence.max(0.0) 
            - fast.stress * 0.05
            + medium.openness * 0.02;
        fast.curiosity += d_curiosity * dt;
        
        // === Social need dynamics ===
        // Increases when alone, decreases after social interaction
        let d_social = if input.is_social {
            -0.1 * fast.social_need // Satisfied by interaction
        } else {
            self.social_need_growth_rate * (self.social_need_target - fast.social_need)
        };
        fast.social_need += d_social * dt;
        
        // Normalize
        fast.normalize();
    }

    /// Medium dynamics: ds_medium/dt = F_medium(s_medium, s_slow, avg(s_fast))
    pub fn step_medium(&self, medium: &mut MediumState, fast: &FastState, _slow: &SlowState, input: &SensoryInput, dt: f32) {
        // Medium dynamics are much slower
        let dt_hours = dt / 3600.0;
        let tau = self.mood_time_constant;
        
        // === Mood bias ===
        // Integrates affect valence over time
        let d_mood = (fast.affect.valence - medium.mood_bias) / tau;
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
            medium.attachment.update_from_interaction(was_positive, input.response_delay_factor);
        }
    }

    /// Slow dynamics: only called on crisis events
    /// 
    /// Returns true if narrative collapse occurred
    pub fn step_slow_crisis(&self, slow: &mut SlowState, medium: &MediumState, crisis_intensity: f32) -> bool {
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
    let e_err = (state.fast.energy - dynamics.energy_target).abs();
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
}
