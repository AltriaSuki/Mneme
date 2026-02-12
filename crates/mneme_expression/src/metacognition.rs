//! MetacognitionEvaluator — periodic self-reflection trigger (#24)
//!
//! Monitors interaction count and energy to fire `Trigger::Metacognition`
//! when enough new experience has accumulated for meaningful reflection.
//! Follows the same gating pattern as `ConsciousnessGate`.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;

use mneme_core::{OrganismState, Trigger, TriggerEvaluator};

/// Configuration for metacognition trigger thresholds.
#[derive(Debug, Clone)]
pub struct MetacognitionConfig {
    /// Hours between reflections.
    pub cooldown_hours: u32,
    /// Minimum energy to allow reflection (reflection is expensive).
    pub energy_floor: f32,
    /// Minimum new interactions since last reflection.
    pub min_interactions: u32,
}

impl Default for MetacognitionConfig {
    fn default() -> Self {
        Self {
            cooldown_hours: 3,
            energy_floor: 0.35,
            min_interactions: 10,
        }
    }
}

/// Evaluator that fires `Trigger::Metacognition` when enough experience
/// has accumulated and the organism has energy to reflect.
pub struct MetacognitionEvaluator {
    state: Arc<RwLock<OrganismState>>,
    config: MetacognitionConfig,
    /// Cooldown: last fired timestamp (unix seconds).
    last_fired: Mutex<i64>,
    /// Shared interaction count from coordinator.
    interaction_count: Arc<RwLock<u32>>,
    /// Interaction count at last fire, to compute delta.
    interactions_at_last_fire: Mutex<u32>,
}

impl MetacognitionEvaluator {
    pub fn new(state: Arc<RwLock<OrganismState>>, interaction_count: Arc<RwLock<u32>>) -> Self {
        Self {
            state,
            config: MetacognitionConfig::default(),
            last_fired: Mutex::new(0),
            interaction_count,
            interactions_at_last_fire: Mutex::new(0),
        }
    }

    pub fn with_config(
        state: Arc<RwLock<OrganismState>>,
        interaction_count: Arc<RwLock<u32>>,
        config: MetacognitionConfig,
    ) -> Self {
        Self {
            state,
            config,
            last_fired: Mutex::new(0),
            interaction_count,
            interactions_at_last_fire: Mutex::new(0),
        }
    }
}

#[async_trait]
impl TriggerEvaluator for MetacognitionEvaluator {
    async fn evaluate(&self) -> Result<Vec<Trigger>> {
        let state = self.state.read().await;
        let now = chrono::Utc::now().timestamp();

        // 1. Energy gate: too tired to reflect
        if state.fast.energy < self.config.energy_floor {
            return Ok(vec![]);
        }

        // 2. Cooldown gate
        {
            let last = *self.last_fired.lock().unwrap();
            if now - last < (self.config.cooldown_hours as i64) * 3600 {
                return Ok(vec![]);
            }
        }

        // 3. Interaction gate: need enough new data to reflect on
        let current_count = *self.interaction_count.read().await;
        let last_fire_count = *self.interactions_at_last_fire.lock().unwrap();
        if current_count.saturating_sub(last_fire_count) < self.config.min_interactions {
            return Ok(vec![]);
        }

        // All gates passed — fire metacognition trigger
        let context_summary = format!(
            "energy={:.2}, stress={:.2}, mood_bias={:.2}, interactions_since_last={}",
            state.fast.energy,
            state.fast.stress,
            state.medium.mood_bias,
            current_count.saturating_sub(last_fire_count),
        );

        tracing::info!(
            "MetacognitionEvaluator: firing (interactions_delta={}, {})",
            current_count.saturating_sub(last_fire_count),
            context_summary,
        );

        // Record cooldown
        *self.last_fired.lock().unwrap() = now;
        *self.interactions_at_last_fire.lock().unwrap() = current_count;

        Ok(vec![Trigger::Metacognition {
            trigger_reason: "periodic".to_string(),
            context_summary,
        }])
    }

    fn name(&self) -> &'static str {
        "MetacognitionEvaluator"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(energy: f32, stress: f32) -> Arc<RwLock<OrganismState>> {
        let mut state = OrganismState::default();
        state.fast.energy = energy;
        state.fast.stress = stress;
        Arc::new(RwLock::new(state))
    }

    fn make_count(n: u32) -> Arc<RwLock<u32>> {
        Arc::new(RwLock::new(n))
    }

    #[tokio::test]
    async fn test_no_trigger_at_baseline() {
        // Zero interactions → should not fire
        let state = make_state(0.8, 0.2);
        let count = make_count(0);
        let eval = MetacognitionEvaluator::new(state, count);
        let triggers = eval.evaluate().await.unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_energy_gate_blocks() {
        let state = make_state(0.1, 0.2); // Very low energy
        let count = make_count(20);
        let eval = MetacognitionEvaluator::new(state, count);
        let triggers = eval.evaluate().await.unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_cooldown_prevents_rapid_fire() {
        let state = make_state(0.8, 0.2);
        let count = make_count(20);
        let config = MetacognitionConfig {
            cooldown_hours: 1, // 1 hour cooldown
            ..Default::default()
        };
        let eval = MetacognitionEvaluator::with_config(state, count.clone(), config);

        // First call should fire
        let t1 = eval.evaluate().await.unwrap();
        assert_eq!(t1.len(), 1);

        // Bump interactions for second attempt
        *count.write().await = 40;

        // Second call within cooldown → blocked
        let t2 = eval.evaluate().await.unwrap();
        assert!(t2.is_empty());
    }

    #[tokio::test]
    async fn test_fires_after_sufficient_interactions() {
        let state = make_state(0.8, 0.2);
        let count = make_count(15);
        let config = MetacognitionConfig {
            cooldown_hours: 0, // No cooldown for testing
            min_interactions: 10,
            ..Default::default()
        };
        let eval = MetacognitionEvaluator::with_config(state, count, config);
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::Metacognition { trigger_reason, .. } => {
                assert_eq!(trigger_reason, "periodic");
            }
            _ => panic!("Expected Metacognition trigger"),
        }
    }

    #[tokio::test]
    async fn test_trigger_reason_periodic() {
        let state = make_state(0.7, 0.3);
        let count = make_count(12);
        let config = MetacognitionConfig {
            cooldown_hours: 0,
            min_interactions: 10,
            energy_floor: 0.3,
        };
        let eval = MetacognitionEvaluator::with_config(state, count, config);
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::Metacognition {
                trigger_reason,
                context_summary,
            } => {
                assert_eq!(trigger_reason, "periodic");
                assert!(context_summary.contains("energy="));
                assert!(context_summary.contains("interactions_since_last="));
            }
            _ => panic!("Expected Metacognition trigger"),
        }
    }
}
