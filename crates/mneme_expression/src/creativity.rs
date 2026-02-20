use anyhow::Result;
use mneme_core::{OrganismState, Trigger, TriggerEvaluator};
use mneme_limbic::BehaviorThresholds;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// ADR-007: Creativity evaluator — boredom + curiosity → autonomous creation.
///
/// When bored enough and curious about something specific, Mneme is prompted
/// to create something (write, explore, build) using her tools.
pub struct CreativityTriggerEvaluator {
    state: Arc<RwLock<OrganismState>>,
    thresholds: Arc<RwLock<BehaviorThresholds>>,
    last_fired: AtomicI64,
    cooldown_secs: i64,
}

impl CreativityTriggerEvaluator {
    pub fn new(
        state: Arc<RwLock<OrganismState>>,
        thresholds: Arc<RwLock<BehaviorThresholds>>,
    ) -> Self {
        Self {
            state,
            thresholds,
            last_fired: AtomicI64::new(0),
            cooldown_secs: 3 * 3600, // Once per 3 hours
        }
    }
}

#[async_trait::async_trait]
impl TriggerEvaluator for CreativityTriggerEvaluator {
    async fn evaluate(&self) -> Result<Vec<Trigger>> {
        let state = self.state.read().await;
        let thresholds = self.thresholds.read().await;

        // Need both boredom AND curiosity to create
        if state.fast.boredom < thresholds.rumination_boredom {
            return Ok(vec![]);
        }
        if state.fast.curiosity < thresholds.curiosity_interest {
            return Ok(vec![]);
        }

        // Energy gate
        if state.fast.energy < 0.3 {
            return Ok(vec![]);
        }

        // Get a topic of interest to channel creativity
        let top = state.fast.curiosity_vector.top_interests(1);
        let topic = top.first().map(|(t, _)| &**t).unwrap_or("something");

        // Cooldown gate
        let now = chrono::Utc::now().timestamp();
        let last = self.last_fired.load(Ordering::Relaxed);
        if now - last < self.cooldown_secs {
            return Ok(vec![]);
        }

        self.last_fired.store(now, Ordering::Relaxed);

        Ok(vec![Trigger::Rumination {
            kind: "creativity".to_string(),
            context: format!(
                "无聊({:.0}%)又好奇({:.0}%)，想创造点什么。\
                最近对「{}」感兴趣。用工具写点东西、探索一个想法、或做一个小实验。",
                state.fast.boredom * 100.0,
                state.fast.curiosity * 100.0,
                topic,
            ),
            route: None,
        }])
    }

    fn name(&self) -> &'static str {
        "CreativityTriggerEvaluator"
    }
}
