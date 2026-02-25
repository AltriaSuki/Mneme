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

#[cfg(test)]
mod tests {
    use super::*;
    use mneme_core::{Person, PersonContext};
    use std::collections::HashMap;

    /// Mock SocialGraph that only implements list_recent_contacts.
    /// Uses the default trait methods for everything else.
    struct MockSocialGraph {
        contacts: Vec<PersonContext>,
    }

    #[async_trait]
    impl SocialGraph for MockSocialGraph {
        async fn find_person(&self, _: &str, _: &str) -> anyhow::Result<Option<Person>> {
            Ok(None)
        }
        async fn upsert_person(&self, _: &Person) -> anyhow::Result<()> {
            Ok(())
        }
        async fn record_interaction(
            &self,
            _from: uuid::Uuid,
            _to: uuid::Uuid,
            _ctx: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn get_person_context(
            &self,
            _id: uuid::Uuid,
        ) -> anyhow::Result<Option<PersonContext>> {
            Ok(None)
        }
        async fn list_recent_contacts(&self, _limit: usize) -> anyhow::Result<Vec<PersonContext>> {
            Ok(self.contacts.clone())
        }
    }

    fn make_state(social_need: f32) -> Arc<RwLock<OrganismState>> {
        let mut state = OrganismState::default();
        state.fast.social_need = social_need;
        Arc::new(RwLock::new(state))
    }

    fn default_thresholds() -> Arc<RwLock<BehaviorThresholds>> {
        Arc::new(RwLock::new(BehaviorThresholds::default()))
    }

    fn make_contact(name: &str, qq_id: Option<&str>, last_ts: Option<i64>) -> PersonContext {
        let mut aliases = HashMap::new();
        if let Some(id) = qq_id {
            aliases.insert("qq".to_string(), id.to_string());
        }
        PersonContext {
            person: Person {
                id: uuid::Uuid::new_v4(),
                name: name.to_string(),
                aliases,
            },
            interaction_count: 5,
            last_interaction_ts: last_ts,
            relationship_notes: "好朋友".to_string(),
        }
    }

    #[tokio::test]
    async fn test_no_trigger_at_low_social_need() {
        let state = make_state(0.3);
        let graph = Arc::new(MockSocialGraph {
            contacts: vec![make_contact("小明", Some("12345"), Some(100))],
        });
        let eval = SocialTriggerEvaluator::new(state, graph, default_thresholds());
        let triggers = eval.evaluate().await.unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_no_trigger_with_empty_contacts() {
        let state = make_state(0.9);
        let graph = Arc::new(MockSocialGraph { contacts: vec![] });
        let eval = SocialTriggerEvaluator::new(state, graph, default_thresholds());
        let triggers = eval.evaluate().await.unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_fires_with_high_social_need_and_contacts() {
        let now = chrono::Utc::now().timestamp();
        let state = make_state(0.9);
        let graph = Arc::new(MockSocialGraph {
            contacts: vec![make_contact("小明", Some("12345"), Some(now - 7200))],
        });
        let eval = SocialTriggerEvaluator::new(state, graph, default_thresholds());
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::Rumination { kind, context, route } => {
                assert_eq!(kind, "social_outreach");
                assert!(context.contains("小明"));
                assert!(context.contains("好朋友"));
                assert_eq!(route.as_deref(), Some("onebot:private:12345"));
            }
            _ => panic!("Expected Rumination trigger"),
        }
    }

    #[tokio::test]
    async fn test_no_route_without_qq_alias() {
        let state = make_state(0.9);
        let graph = Arc::new(MockSocialGraph {
            contacts: vec![make_contact("小红", None, Some(100))],
        });
        let eval = SocialTriggerEvaluator::new(state, graph, default_thresholds());
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::Rumination { route, .. } => {
                assert!(route.is_none());
            }
            _ => panic!("Expected Rumination trigger"),
        }
    }

    #[tokio::test]
    async fn test_cooldown_prevents_rapid_fire() {
        let state = make_state(0.9);
        let graph = Arc::new(MockSocialGraph {
            contacts: vec![make_contact("小明", Some("12345"), Some(100))],
        });
        let eval = SocialTriggerEvaluator::new(state, graph, default_thresholds());

        let t1 = eval.evaluate().await.unwrap();
        assert_eq!(t1.len(), 1);

        let t2 = eval.evaluate().await.unwrap();
        assert!(t2.is_empty());
    }

    #[tokio::test]
    async fn test_last_interaction_days_ago_format() {
        let now = chrono::Utc::now().timestamp();
        let state = make_state(0.9);
        let graph = Arc::new(MockSocialGraph {
            contacts: vec![make_contact("小明", None, Some(now - 3 * 86400))],
        });
        let eval = SocialTriggerEvaluator::new(state, graph, default_thresholds());
        let triggers = eval.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            Trigger::Rumination { context, .. } => {
                assert!(context.contains("3天前"));
            }
            _ => panic!("Expected Rumination trigger"),
        }
    }
}
