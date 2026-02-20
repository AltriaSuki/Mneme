use anyhow::Result;
use mneme_core::{OrganismState, Trigger, TriggerEvaluator};
use mneme_limbic::BehaviorThresholds;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// B-20: Meaning-seeking evaluator — existential reflection when contemplative.
///
/// Fires when Mneme has enough energy, low stress, and sufficient time has
/// passed since the last reflection. Uses Rumination trigger with kind
/// "meaning_seeking" so the engine processes it as an internal thought.
pub struct MeaningSeekingEvaluator {
    state: Arc<RwLock<OrganismState>>,
    thresholds: Arc<RwLock<BehaviorThresholds>>,
    last_fired: AtomicI64,
    cooldown_secs: i64,
}

impl MeaningSeekingEvaluator {
    pub fn new(
        state: Arc<RwLock<OrganismState>>,
        thresholds: Arc<RwLock<BehaviorThresholds>>,
    ) -> Self {
        Self {
            state,
            thresholds,
            last_fired: AtomicI64::new(0),
            cooldown_secs: 6 * 3600, // Once per 6 hours
        }
    }
}

#[async_trait::async_trait]
impl TriggerEvaluator for MeaningSeekingEvaluator {
    async fn evaluate(&self) -> Result<Vec<Trigger>> {
        let state = self.state.read().await;
        let thresholds = self.thresholds.read().await;

        // Energy gate: need enough energy for deep reflection
        if state.fast.energy < thresholds.meaning_energy_min {
            return Ok(vec![]);
        }

        // Stress gate: too stressed to contemplate
        if state.fast.stress > 0.4 {
            return Ok(vec![]);
        }

        // Cooldown gate
        let now = chrono::Utc::now().timestamp();
        let last = self.last_fired.load(Ordering::Relaxed);
        if now - last < self.cooldown_secs {
            return Ok(vec![]);
        }

        self.last_fired.store(now, Ordering::Relaxed);

        Ok(vec![Trigger::Rumination {
            kind: "meaning_seeking".to_string(),
            context: format!(
                "心境平静(stress={:.0}%, energy={:.0}%)，适合进行存在性反思。\
                回顾最近的经历和记忆，思考它们对你意味着什么。\
                你可以思考：自己的成长、与人的关系、对世界的理解、或任何让你觉得有意义的事。",
                state.fast.stress * 100.0,
                state.fast.energy * 100.0,
            ),
            route: None,
        }])
    }

    fn name(&self) -> &'static str {
        "MeaningSeekingEvaluator"
    }
}
