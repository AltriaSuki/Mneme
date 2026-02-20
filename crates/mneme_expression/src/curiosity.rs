//! #81: Curiosity-driven autonomous exploration.
//!
//! When curiosity is high AND CuriosityVector has specific interests,
//! fires a Rumination trigger with the top interest topic so the LLM
//! can autonomously search/explore it using tools.

use async_trait::async_trait;
use mneme_core::{OrganismState, Trigger, TriggerEvaluator};
use mneme_limbic::BehaviorThresholds;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct CuriosityTriggerEvaluator {
    state: Arc<RwLock<OrganismState>>,
    thresholds: Arc<RwLock<BehaviorThresholds>>,
    /// Cooldown in seconds (default: 1800 = 30 min).
    cooldown_secs: i64,
    last_fired: AtomicI64,
}

impl CuriosityTriggerEvaluator {
    pub fn new(state: Arc<RwLock<OrganismState>>, thresholds: Arc<RwLock<BehaviorThresholds>>) -> Self {
        Self {
            state,
            thresholds,
            cooldown_secs: 1800,
            last_fired: AtomicI64::new(0),
        }
    }
}

#[async_trait]
impl TriggerEvaluator for CuriosityTriggerEvaluator {
    async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>> {
        let state = self.state.read().await;
        let thresholds = self.thresholds.read().await;

        if state.fast.curiosity < thresholds.curiosity_trigger {
            return Ok(Vec::new());
        }

        let top = state.fast.curiosity_vector.top_interests(1);
        let Some(&(topic, intensity)) = top.first() else {
            return Ok(Vec::new());
        };
        if intensity < thresholds.curiosity_interest {
            return Ok(Vec::new());
        }

        let now = chrono::Utc::now().timestamp();
        let last = self.last_fired.load(Ordering::Relaxed);
        if now - last < self.cooldown_secs {
            return Ok(Vec::new());
        }

        self.last_fired.store(now, Ordering::Relaxed);

        tracing::info!(
            "CuriosityTrigger: curiosity={:.2}, topic='{}' intensity={:.2}",
            state.fast.curiosity,
            topic,
            intensity
        );

        Ok(vec![Trigger::Rumination {
            kind: "curiosity_exploration".to_string(),
            context: format!(
                "好奇心很强({:.0}%)，特别想了解「{}」(兴趣强度{:.0}%)。用工具搜索或探索这个话题，然后分享发现。",
                state.fast.curiosity * 100.0,
                topic,
                intensity * 100.0,
            ),
            route: None,
        }])
    }

    fn name(&self) -> &'static str {
        "CuriosityTriggerEvaluator"
    }
}
