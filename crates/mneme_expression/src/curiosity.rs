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
            // B-2: No state percentages. Topic is factual, not state.
            context: format!(
                "特别想了解「{}」。用工具搜索或探索这个话题，然后分享发现。",
                topic,
            ),
            route: None,
        }])
    }

    fn name(&self) -> &'static str {
        "CuriosityTriggerEvaluator"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(curiosity: f32, interests: Vec<(&str, f32)>) -> Arc<RwLock<OrganismState>> {
        let mut state = OrganismState::default();
        state.fast.curiosity = curiosity;
        for (topic, intensity) in interests {
            state.fast.curiosity_vector.tag_interest(topic, intensity);
        }
        Arc::new(RwLock::new(state))
    }

    fn default_thresholds() -> Arc<RwLock<BehaviorThresholds>> {
        Arc::new(RwLock::new(BehaviorThresholds::default()))
    }

    #[tokio::test]
    async fn test_no_trigger_at_low_curiosity() {
        let state = make_state(0.3, vec![("量子计算", 0.8)]);
        let eval = CuriosityTriggerEvaluator::new(state, default_thresholds());
        let triggers = eval.evaluate().await.unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_no_trigger_without_interests() {
        // High curiosity but no specific interests
        let state = make_state(0.9, vec![]);
        let eval = CuriosityTriggerEvaluator::new(state, default_thresholds());
        let triggers = eval.evaluate().await.unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_no_trigger_with_weak_interest() {
        // High curiosity but interest intensity below threshold (default 0.4)
        let state = make_state(0.9, vec![("天文学", 0.2)]);
        let eval = CuriosityTriggerEvaluator::new(state, default_thresholds());
        let triggers = eval.evaluate().await.unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_fires_with_high_curiosity_and_strong_interest() {
        let state = make_state(0.8, vec![("量子计算", 0.7)]);
        let eval = CuriosityTriggerEvaluator::new(state, default_thresholds());
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::Rumination { kind, context, route } => {
                assert_eq!(kind, "curiosity_exploration");
                assert!(context.contains("量子计算"));
                assert!(route.is_none());
            }
            _ => panic!("Expected Rumination trigger"),
        }
    }

    #[tokio::test]
    async fn test_cooldown_prevents_rapid_fire() {
        let state = make_state(0.8, vec![("量子计算", 0.7)]);
        let eval = CuriosityTriggerEvaluator::new(state, default_thresholds());

        let t1 = eval.evaluate().await.unwrap();
        assert_eq!(t1.len(), 1);

        // Second call within cooldown → blocked
        let t2 = eval.evaluate().await.unwrap();
        assert!(t2.is_empty());
    }

    #[tokio::test]
    async fn test_cooldown_zero_allows_repeat() {
        let state = make_state(0.8, vec![("量子计算", 0.7)]);
        let thresholds = default_thresholds();
        let mut eval = CuriosityTriggerEvaluator::new(state, thresholds);
        eval.cooldown_secs = 0;

        let t1 = eval.evaluate().await.unwrap();
        assert_eq!(t1.len(), 1);

        let t2 = eval.evaluate().await.unwrap();
        assert_eq!(t2.len(), 1);
    }

    #[tokio::test]
    async fn test_picks_top_interest() {
        // Multiple interests — should pick the strongest one
        let state = make_state(0.8, vec![("天文学", 0.5), ("量子计算", 0.9)]);
        let eval = CuriosityTriggerEvaluator::new(state, default_thresholds());
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::Rumination { context, .. } => {
                assert!(context.contains("量子计算"));
            }
            _ => panic!("Expected Rumination trigger"),
        }
    }
}
