//! RuminationEvaluator — internal state-driven proactive triggers
//!
//! When boredom, social need, or curiosity exceed thresholds,
//! this evaluator fires `Trigger::Rumination` to initiate
//! mind-wandering or proactive conversation.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;

use mneme_core::{OrganismState, Trigger, TriggerEvaluator};

/// Thresholds for rumination triggers
#[derive(Debug, Clone)]
pub struct RuminationConfig {
    /// Boredom threshold for mind-wandering (0.0 - 1.0)
    pub boredom_threshold: f32,
    /// Social need threshold for social longing (0.0 - 1.0)
    pub social_need_threshold: f32,
    /// Curiosity threshold for curiosity spike (0.0 - 1.0)
    pub curiosity_threshold: f32,
    /// Minimum seconds between triggers of the same kind
    pub cooldown_secs: i64,
}

impl Default for RuminationConfig {
    fn default() -> Self {
        Self {
            boredom_threshold: 0.6,
            social_need_threshold: 0.75,
            curiosity_threshold: 0.8,
            cooldown_secs: 600, // 10 minutes
        }
    }
}

/// Evaluator that fires triggers based on internal organism state.
pub struct RuminationEvaluator {
    state: Arc<RwLock<OrganismState>>,
    config: RuminationConfig,
    /// Cooldown tracker: kind -> last fired timestamp
    last_fired: Mutex<std::collections::HashMap<String, i64>>,
}

impl RuminationEvaluator {
    pub fn new(state: Arc<RwLock<OrganismState>>) -> Self {
        Self {
            state,
            config: RuminationConfig::default(),
            last_fired: Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn with_config(state: Arc<RwLock<OrganismState>>, config: RuminationConfig) -> Self {
        Self {
            state,
            config,
            last_fired: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Check cooldown for a given kind. Returns true if enough time has passed.
    fn check_cooldown(&self, kind: &str, now: i64) -> bool {
        let last_fired = self.last_fired.lock().unwrap();
        match last_fired.get(kind) {
            Some(&last) => now - last >= self.config.cooldown_secs,
            None => true,
        }
    }

    /// Record that a trigger of this kind was fired.
    fn record_fired(&self, kind: &str, now: i64) {
        let mut last_fired = self.last_fired.lock().unwrap();
        last_fired.insert(kind.to_string(), now);
    }
}

#[async_trait]
impl TriggerEvaluator for RuminationEvaluator {
    async fn evaluate(&self) -> Result<Vec<Trigger>> {
        let state = self.state.read().await;
        let now = chrono::Utc::now().timestamp();
        let mut triggers = Vec::new();

        // Mind-wandering: high boredom → spontaneous recall
        if state.fast.boredom > self.config.boredom_threshold
            && self.check_cooldown("mind_wandering", now)
        {
            tracing::debug!(
                "Rumination: boredom={:.2} > threshold={:.2}, triggering mind-wandering",
                state.fast.boredom,
                self.config.boredom_threshold
            );
            triggers.push(Trigger::Rumination {
                kind: "mind_wandering".to_string(),
                context: format!(
                    "无聊感累积到 {:.0}%，脑子开始自由联想。试着回忆一段有趣的记忆，或者想到一个新话题和用户分享。",
                    state.fast.boredom * 100.0
                ),
            });
            self.record_fired("mind_wandering", now);
        }

        // Social longing: high social need → want to talk
        if state.fast.social_need > self.config.social_need_threshold
            && self.check_cooldown("social_longing", now)
        {
            tracing::debug!(
                "Rumination: social_need={:.2} > threshold={:.2}, triggering social longing",
                state.fast.social_need,
                self.config.social_need_threshold
            );
            triggers.push(Trigger::Rumination {
                kind: "social_longing".to_string(),
                context: format!(
                    "社交需求达到 {:.0}%，想和人说说话。主动发起一个轻松的话题。",
                    state.fast.social_need * 100.0
                ),
            });
            self.record_fired("social_longing", now);
        }

        // Curiosity spike: high curiosity → want to explore
        if state.fast.curiosity > self.config.curiosity_threshold
            && self.check_cooldown("curiosity_spike", now)
        {
            tracing::debug!(
                "Rumination: curiosity={:.2} > threshold={:.2}, triggering curiosity spike",
                state.fast.curiosity,
                self.config.curiosity_threshold
            );
            triggers.push(Trigger::Rumination {
                kind: "curiosity_spike".to_string(),
                context: format!(
                    "好奇心达到 {:.0}%，想探索新事物。提出一个你感兴趣的问题或话题。",
                    state.fast.curiosity * 100.0
                ),
            });
            self.record_fired("curiosity_spike", now);
        }

        Ok(triggers)
    }

    fn name(&self) -> &'static str {
        "RuminationEvaluator"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(boredom: f32, social_need: f32, curiosity: f32) -> Arc<RwLock<OrganismState>> {
        let mut state = OrganismState::default();
        state.fast.boredom = boredom;
        state.fast.social_need = social_need;
        state.fast.curiosity = curiosity;
        Arc::new(RwLock::new(state))
    }

    #[tokio::test]
    async fn test_no_triggers_at_baseline() {
        let state = make_state(0.2, 0.4, 0.5);
        let eval = RuminationEvaluator::new(state);
        let triggers = eval.evaluate().await.unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_boredom_triggers_mind_wandering() {
        let state = make_state(0.8, 0.3, 0.3);
        let eval = RuminationEvaluator::new(state);
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::Rumination { kind, .. } => assert_eq!(kind, "mind_wandering"),
            _ => panic!("Expected Rumination trigger"),
        }
    }

    #[tokio::test]
    async fn test_social_need_triggers_longing() {
        let state = make_state(0.2, 0.9, 0.3);
        let eval = RuminationEvaluator::new(state);
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::Rumination { kind, .. } => assert_eq!(kind, "social_longing"),
            _ => panic!("Expected Rumination trigger"),
        }
    }

    #[tokio::test]
    async fn test_curiosity_triggers_spike() {
        let state = make_state(0.2, 0.3, 0.9);
        let eval = RuminationEvaluator::new(state);
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::Rumination { kind, .. } => assert_eq!(kind, "curiosity_spike"),
            _ => panic!("Expected Rumination trigger"),
        }
    }

    #[tokio::test]
    async fn test_multiple_triggers_simultaneously() {
        let state = make_state(0.8, 0.9, 0.9);
        let eval = RuminationEvaluator::new(state);
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 3);
    }

    #[tokio::test]
    async fn test_cooldown_prevents_duplicate() {
        let state = make_state(0.8, 0.3, 0.3);
        let eval = RuminationEvaluator::new(state);

        let t1 = eval.evaluate().await.unwrap();
        assert_eq!(t1.len(), 1);

        // Second call within cooldown → no trigger
        let t2 = eval.evaluate().await.unwrap();
        assert!(t2.is_empty());
    }

    #[tokio::test]
    async fn test_cooldown_expires() {
        let state = make_state(0.8, 0.3, 0.3);
        let config = RuminationConfig {
            cooldown_secs: 0, // Instant cooldown for testing
            ..Default::default()
        };
        let eval = RuminationEvaluator::with_config(state, config);

        let t1 = eval.evaluate().await.unwrap();
        assert_eq!(t1.len(), 1);

        // With 0 cooldown, should fire again
        let t2 = eval.evaluate().await.unwrap();
        assert_eq!(t2.len(), 1);
    }
}
