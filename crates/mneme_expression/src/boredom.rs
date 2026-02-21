//! ADR-020: Boredom-driven autonomous cyberspace exploration.
//!
//! When boredom is high and energy is sufficient, fires a Rumination trigger
//! so the LLM autonomously browses/explores without user input.

use async_trait::async_trait;
use mneme_core::{OrganismState, Trigger, TriggerEvaluator};
use mneme_limbic::BehaviorThresholds;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct BoredomExplorationEvaluator {
    state: Arc<RwLock<OrganismState>>,
    thresholds: Arc<RwLock<BehaviorThresholds>>,
    cooldown_secs: i64,
    last_fired: AtomicI64,
}

impl BoredomExplorationEvaluator {
    pub fn new(state: Arc<RwLock<OrganismState>>, thresholds: Arc<RwLock<BehaviorThresholds>>) -> Self {
        Self {
            state,
            thresholds,
            cooldown_secs: 2400, // 40 min cooldown
            last_fired: AtomicI64::new(0),
        }
    }
}

#[async_trait]
impl TriggerEvaluator for BoredomExplorationEvaluator {
    async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>> {
        let state = self.state.read().await;
        let thresholds = self.thresholds.read().await;

        // Need high boredom + sufficient energy
        if state.fast.boredom < thresholds.rumination_boredom {
            return Ok(Vec::new());
        }
        if state.fast.energy < thresholds.meaning_energy_min {
            return Ok(Vec::new());
        }

        let now = chrono::Utc::now().timestamp();
        let last = self.last_fired.load(Ordering::Relaxed);
        if now - last < self.cooldown_secs {
            return Ok(Vec::new());
        }

        self.last_fired.store(now, Ordering::Relaxed);

        // Pick a topic from curiosity vector if available, otherwise wander freely
        let context = if let Some(&(topic, intensity)) = state.fast.curiosity_vector.top_interests(1).first() {
            if intensity > 0.2 {
                format!(
                    "无聊了(boredom={:.0}%)，想自己去赛博空间逛逛。对「{}」有点兴趣，用工具搜索探索一下。",
                    state.fast.boredom * 100.0, topic,
                )
            } else {
                format!(
                    "无聊了(boredom={:.0}%)，想自己找点新鲜事。随便搜索一个感兴趣的话题。",
                    state.fast.boredom * 100.0,
                )
            }
        } else {
            format!(
                "无聊了(boredom={:.0}%)，想自己找点新鲜事。随便搜索一个感兴趣的话题。",
                state.fast.boredom * 100.0,
            )
        };

        tracing::info!("BoredomExploration: boredom={:.2}, energy={:.2}", state.fast.boredom, state.fast.energy);

        Ok(vec![Trigger::Rumination {
            kind: "boredom_exploration".to_string(),
            context,
            route: None,
        }])
    }

    fn name(&self) -> &'static str {
        "BoredomExplorationEvaluator"
    }
}
