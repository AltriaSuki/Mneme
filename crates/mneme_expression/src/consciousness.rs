//! ConsciousnessGate — ADR-012/013: self-triggered consciousness + multi-resolution monologue
//!
//! Monitors ODE state and fires `Trigger::InnerMonologue` when internal conditions
//! cross consciousness thresholds. The trigger is driven by state, not by timers.
//!
//! Resolution selection (ADR-013):
//! - Zero: pure ODE evolution, no LLM call (handled implicitly — we don't fire a trigger)
//! - Low: fragment-style inner speech, cheap model, low-strength episodes
//! - High: full coherent thought, primary LLM, high-strength episodes
//!
//! Escalation: Zero→Low when state delta exceeds threshold; Low→High when
//! body feelings are intense or multiple feelings co-occur (worth "thinking deeply").

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;

use mneme_core::{MonologueResolution, OrganismState, Trigger, TriggerEvaluator};
use mneme_limbic::SomaticMarker;

/// Configuration for consciousness gate thresholds.
#[derive(Debug, Clone)]
pub struct ConsciousnessConfig {
    /// Minimum state delta (max across all dimensions) to trigger Low resolution.
    /// Below this, the state change is "zero resolution" — pure ODE, no LLM.
    pub low_threshold: f32,
    /// Intensity threshold for escalating to High resolution.
    /// When a body feeling's intensity exceeds this, it's "worth thinking deeply about".
    pub high_intensity_threshold: f32,
    /// Number of simultaneous body feelings that triggers High resolution.
    /// Multiple feelings co-occurring = complex internal state = worth full reflection.
    pub high_feeling_count: usize,
    /// Minimum energy to allow any consciousness trigger.
    /// Too tired to think — consciousness requires metabolic resources.
    pub energy_floor: f32,
    /// Body feeling significance threshold (passed to `describe_body_feeling`).
    pub feeling_threshold: f32,
    /// Minimum seconds between consciousness triggers.
    pub cooldown_secs: i64,
}

impl Default for ConsciousnessConfig {
    fn default() -> Self {
        Self {
            low_threshold: 0.12,
            high_intensity_threshold: 0.6,
            high_feeling_count: 3,
            energy_floor: 0.25,
            feeling_threshold: 0.10,
            cooldown_secs: 300, // 5 minutes
        }
    }
}

/// Evaluator that fires `Trigger::InnerMonologue` based on ODE state changes.
///
/// ADR-012: "LLM 调用的触发权属于 Mneme 自己" — the trigger comes from within.
/// ADR-013: Resolution is selected by state intensity, not by external request.
pub struct ConsciousnessGate {
    state: Arc<RwLock<OrganismState>>,
    config: ConsciousnessConfig,
    /// Previous somatic marker snapshot for delta detection.
    prev_marker: Mutex<Option<SomaticMarker>>,
    /// Cooldown: last fired timestamp.
    last_fired: Mutex<i64>,
}

impl ConsciousnessGate {
    pub fn new(state: Arc<RwLock<OrganismState>>) -> Self {
        Self {
            state,
            config: ConsciousnessConfig::default(),
            prev_marker: Mutex::new(None),
            last_fired: Mutex::new(0),
        }
    }

    pub fn with_config(state: Arc<RwLock<OrganismState>>, config: ConsciousnessConfig) -> Self {
        Self {
            state,
            config,
            prev_marker: Mutex::new(None),
            last_fired: Mutex::new(0),
        }
    }

    /// Compute the maximum absolute delta between two somatic markers.
    fn state_delta(curr: &SomaticMarker, prev: &SomaticMarker) -> f32 {
        let deltas = [
            (curr.energy - prev.energy).abs(),
            (curr.stress - prev.stress).abs(),
            (curr.social_need - prev.social_need).abs(),
            (curr.curiosity - prev.curiosity).abs(),
            (curr.mood_bias - prev.mood_bias).abs(),
            (curr.affect.valence - prev.affect.valence).abs(),
            (curr.affect.arousal - prev.affect.arousal).abs(),
        ];
        deltas.into_iter().fold(0.0f32, f32::max)
    }

    /// Classify the cause of consciousness from body feelings.
    fn classify_cause(feelings: &[(String, f32)], curr: &SomaticMarker) -> String {
        // Pick the dominant cause based on what changed most
        if curr.stress > 0.7 {
            return "stress_spike".to_string();
        }
        if curr.affect.arousal > 0.7 && curr.affect.valence.abs() > 0.5 {
            return "emotional_surge".to_string();
        }
        if feelings
            .iter()
            .any(|(t, _)| t.contains("累") || t.contains("没力气"))
        {
            return "body_feeling".to_string();
        }
        if feelings
            .iter()
            .any(|(t, _)| t.contains("想") || t.contains("痒"))
        {
            return "curiosity_overflow".to_string();
        }
        "state_shift".to_string()
    }

    /// Build seed content from body feelings for the monologue prompt.
    fn build_seed(feelings: &[(String, f32)]) -> String {
        if feelings.is_empty() {
            return "内部状态发生了变化".to_string();
        }
        feelings
            .iter()
            .map(|(text, intensity)| format!("{}(强度{:.0}%)", text, intensity * 100.0))
            .collect::<Vec<_>>()
            .join("；")
    }
}

#[async_trait]
impl TriggerEvaluator for ConsciousnessGate {
    async fn evaluate(&self) -> Result<Vec<Trigger>> {
        let state = self.state.read().await;
        let curr = SomaticMarker::from_state(&state);
        let now = chrono::Utc::now().timestamp();

        // Energy gate: too tired to think
        // NOTE: Do NOT update prev_marker here (#63). If we record the depleted state,
        // then when energy recovers the delta is calculated against that low baseline,
        // compressing the apparent change and suppressing consciousness triggers.
        if curr.energy < self.config.energy_floor {
            return Ok(vec![]);
        }

        // Cooldown check
        {
            let last = *self.last_fired.lock().unwrap();
            if now - last < self.config.cooldown_secs {
                return Ok(vec![]);
            }
        }

        // Get previous marker (first call → no delta, just record baseline)
        let prev = {
            let mut prev_lock = self.prev_marker.lock().unwrap();
            let prev = prev_lock.clone();
            *prev_lock = Some(curr.clone());
            prev
        };

        let prev = match prev {
            Some(p) => p,
            None => return Ok(vec![]),
        };

        // Compute state delta
        let delta = Self::state_delta(&curr, &prev);

        // Below low threshold → zero resolution (no trigger, pure ODE)
        if delta < self.config.low_threshold {
            return Ok(vec![]);
        }

        // State changed enough to warrant consciousness — compute body feelings
        let feelings = curr.describe_body_feeling(&prev, self.config.feeling_threshold);

        // Determine resolution: Low or High
        let max_intensity = feelings.iter().map(|(_, i)| *i).fold(0.0f32, f32::max);
        let feeling_count = feelings.len();

        let resolution = if max_intensity > self.config.high_intensity_threshold
            || feeling_count >= self.config.high_feeling_count
        {
            MonologueResolution::High
        } else {
            MonologueResolution::Low
        };

        let cause = Self::classify_cause(&feelings, &curr);
        let seed = Self::build_seed(&feelings);

        tracing::info!(
            "ConsciousnessGate: delta={:.3}, feelings={}, resolution={:?}, cause={}",
            delta,
            feeling_count,
            resolution,
            cause
        );

        // Record cooldown
        *self.last_fired.lock().unwrap() = now;

        Ok(vec![Trigger::InnerMonologue {
            cause,
            seed,
            resolution,
        }])
    }

    fn name(&self) -> &'static str {
        "ConsciousnessGate"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(energy: f32, stress: f32, boredom: f32) -> Arc<RwLock<OrganismState>> {
        let mut state = OrganismState::default();
        state.fast.energy = energy;
        state.fast.stress = stress;
        state.fast.boredom = boredom;
        Arc::new(RwLock::new(state))
    }

    #[tokio::test]
    async fn test_first_call_records_baseline() {
        let state = make_state(0.7, 0.3, 0.2);
        let gate = ConsciousnessGate::new(state);
        let triggers = gate.evaluate().await.unwrap();
        // First call: no previous marker → no trigger
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_no_trigger_below_threshold() {
        let state_arc = make_state(0.7, 0.3, 0.2);
        let gate = ConsciousnessGate::new(state_arc.clone());

        // First call: baseline
        gate.evaluate().await.unwrap();

        // Tiny change: delta < low_threshold
        {
            let mut s = state_arc.write().await;
            s.fast.stress = 0.32; // delta = 0.02
        }
        let triggers = gate.evaluate().await.unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_low_resolution_on_moderate_change() {
        let state_arc = make_state(0.7, 0.2, 0.2);
        let config = ConsciousnessConfig {
            cooldown_secs: 0, // No cooldown for testing
            ..Default::default()
        };
        let gate = ConsciousnessGate::with_config(state_arc.clone(), config);

        // Baseline
        gate.evaluate().await.unwrap();

        // Moderate stress increase: delta = 0.3 > low_threshold
        {
            let mut s = state_arc.write().await;
            s.fast.stress = 0.5;
        }
        let triggers = gate.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::InnerMonologue { resolution, .. } => {
                assert_eq!(*resolution, MonologueResolution::Low);
            }
            _ => panic!("Expected InnerMonologue trigger"),
        }
    }

    #[tokio::test]
    async fn test_high_resolution_on_intense_change() {
        let state_arc = make_state(0.8, 0.1, 0.2);
        let config = ConsciousnessConfig {
            cooldown_secs: 0,
            ..Default::default()
        };
        let gate = ConsciousnessGate::with_config(state_arc.clone(), config);

        // Baseline
        gate.evaluate().await.unwrap();

        // Large stress spike: delta = 0.8, intensity should be high
        {
            let mut s = state_arc.write().await;
            s.fast.stress = 0.9;
        }
        let triggers = gate.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::InnerMonologue {
                resolution, cause, ..
            } => {
                assert_eq!(*resolution, MonologueResolution::High);
                assert_eq!(cause, "stress_spike");
            }
            _ => panic!("Expected InnerMonologue trigger"),
        }
    }

    #[tokio::test]
    async fn test_energy_gate_blocks_trigger() {
        let state_arc = make_state(0.1, 0.2, 0.2); // Very low energy
        let config = ConsciousnessConfig {
            cooldown_secs: 0,
            ..Default::default()
        };
        let gate = ConsciousnessGate::with_config(state_arc.clone(), config);

        // Baseline
        gate.evaluate().await.unwrap();

        // Big change but no energy to think
        {
            let mut s = state_arc.write().await;
            s.fast.stress = 0.9;
        }
        let triggers = gate.evaluate().await.unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_energy_recovery_fires_trigger_after_gate() {
        // Bug #63: prev_marker should NOT be updated while below energy floor,
        // so that when energy recovers, the accumulated delta fires a trigger.
        let state_arc = make_state(0.7, 0.2, 0.2);
        let config = ConsciousnessConfig {
            cooldown_secs: 0,
            ..Default::default()
        };
        let gate = ConsciousnessGate::with_config(state_arc.clone(), config);

        // Baseline at normal energy
        gate.evaluate().await.unwrap();

        // Drop energy below floor + change stress — gate blocks, prev_marker preserved
        {
            let mut s = state_arc.write().await;
            s.fast.energy = 0.1;
            s.fast.stress = 0.8; // big change from 0.2
        }
        let triggers = gate.evaluate().await.unwrap();
        assert!(triggers.is_empty(), "should be blocked by energy gate");

        // Recover energy — delta should be computed against the pre-gate baseline (stress=0.2)
        {
            let mut s = state_arc.write().await;
            s.fast.energy = 0.7;
            // stress stays at 0.8, delta from baseline 0.2 = 0.6
        }
        let triggers = gate.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1, "should fire after energy recovery");
    }

    #[tokio::test]
    async fn test_cooldown_prevents_rapid_fire() {
        let state_arc = make_state(0.7, 0.2, 0.2);
        let gate = ConsciousnessGate::new(state_arc.clone()); // Default 5min cooldown

        // Baseline
        gate.evaluate().await.unwrap();

        // First trigger
        {
            let mut s = state_arc.write().await;
            s.fast.stress = 0.6;
        }
        let t1 = gate.evaluate().await.unwrap();
        assert_eq!(t1.len(), 1);

        // Second call within cooldown → blocked
        {
            let mut s = state_arc.write().await;
            s.fast.stress = 0.9;
        }
        let t2 = gate.evaluate().await.unwrap();
        assert!(t2.is_empty());
    }

    #[test]
    fn test_build_seed_empty() {
        assert_eq!(ConsciousnessGate::build_seed(&[]), "内部状态发生了变化");
    }

    #[test]
    fn test_build_seed_with_feelings() {
        let feelings = vec![("有点累了".to_string(), 0.5), ("心跳加快".to_string(), 0.8)];
        let seed = ConsciousnessGate::build_seed(&feelings);
        assert!(seed.contains("有点累了"));
        assert!(seed.contains("心跳加快"));
        assert!(seed.contains("50%"));
        assert!(seed.contains("80%"));
    }

    #[test]
    fn test_classify_cause_stress() {
        let feelings = vec![];
        let mut state = OrganismState::default();
        state.fast.stress = 0.8;
        let marker = SomaticMarker::from_state(&state);
        assert_eq!(
            ConsciousnessGate::classify_cause(&feelings, &marker),
            "stress_spike"
        );
    }

    #[test]
    fn test_classify_cause_default() {
        let feelings = vec![];
        let state = OrganismState::default();
        let marker = SomaticMarker::from_state(&state);
        assert_eq!(
            ConsciousnessGate::classify_cause(&feelings, &marker),
            "state_shift"
        );
    }
}
