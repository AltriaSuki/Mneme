//! #83: Proactive Social Triggers — reach out to known contacts when social need is high.
//!
//! When social_need exceeds a threshold, queries the SocialGraph for recent
//! contacts and fires a Rumination trigger routed to a specific person.

use async_trait::async_trait;
use mneme_core::{OrganismState, SocialGraph, Trigger, TriggerEvaluator};
use mneme_limbic::BehaviorThresholds;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for social trigger evaluation.
#[derive(Debug, Clone)]
pub struct SocialTriggerConfig {
    /// Cooldown between social triggers in seconds (default: 3600 = 1 hour).
    pub cooldown_secs: i64,
}

impl Default for SocialTriggerConfig {
    fn default() -> Self {
        Self {
            cooldown_secs: 3600,
        }
    }
}

/// Evaluator that fires social outreach triggers when social need is high.
pub struct SocialTriggerEvaluator {
    state: Arc<RwLock<OrganismState>>,
    social_graph: Arc<dyn SocialGraph>,
    thresholds: Arc<RwLock<BehaviorThresholds>>,
    config: SocialTriggerConfig,
    last_fired: AtomicI64,
}

impl SocialTriggerEvaluator {
    pub fn new(state: Arc<RwLock<OrganismState>>, social_graph: Arc<dyn SocialGraph>, thresholds: Arc<RwLock<BehaviorThresholds>>) -> Self {
        Self {
            state,
            social_graph,
            thresholds,
            config: SocialTriggerConfig::default(),
            last_fired: AtomicI64::new(0),
        }
    }
}

#[async_trait]
impl TriggerEvaluator for SocialTriggerEvaluator {
    async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>> {
        let state = self.state.read().await;
        let thresholds = self.thresholds.read().await;
        if state.fast.social_need < thresholds.social_trigger {
            return Ok(Vec::new());
        }

        let now = chrono::Utc::now().timestamp();
        let last = self.last_fired.load(Ordering::Relaxed);
        if now - last < self.config.cooldown_secs {
            return Ok(Vec::new());
        }

        let contacts = self.social_graph.list_recent_contacts(5).await?;
        let Some(contact) = contacts.first() else {
            return Ok(Vec::new());
        };

        // Find a QQ alias for routing, fall back to CLI
        let route = contact
            .person
            .aliases
            .get("qq")
            .map(|id| format!("onebot:private:{}", id));

        self.last_fired.store(now, Ordering::Relaxed);

        tracing::info!(
            "SocialTrigger: social_need={:.2}, reaching out to {}",
            state.fast.social_need,
            contact.person.name
        );

        Ok(vec![Trigger::Rumination {
            kind: "social_outreach".to_string(),
            context: format!(
                "社交需求很高({:.0}%)，想和{}聊聊。上次互动: {}。{}",
                state.fast.social_need * 100.0,
                contact.person.name,
                contact
                    .last_interaction_ts
                    .map(|ts| {
                        let ago = now - ts;
                        if ago > 86400 {
                            format!("{}天前", ago / 86400)
                        } else {
                            format!("{}小时前", ago / 3600)
                        }
                    })
                    .unwrap_or_else(|| "未知".to_string()),
                contact.relationship_notes,
            ),
            route,
        }])
    }

    fn name(&self) -> &'static str {
        "SocialTriggerEvaluator"
    }
}
