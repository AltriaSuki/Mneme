use crate::scheduler::PresenceScheduler;
use mneme_core::{Trigger, TriggerEvaluator};
use mneme_memory::OrganismCoordinator;
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// AgentAction
// ============================================================================

#[derive(Debug, Clone)]
pub enum AgentAction {
    /// A proactive trigger fired — caller should process via engine.think()
    ProactiveTrigger(Trigger),
    /// Organism state tick completed
    StateUpdate,
    /// Autonomous tool use triggered by rule engine (#23, v0.6.0)
    AutonomousToolUse {
        tool_name: String,
        input: serde_json::Value,
        goal_id: Option<i64>,
    },
}

// ============================================================================
// AgentLoop
// ============================================================================

pub struct AgentLoop {
    coordinator: Arc<OrganismCoordinator>,
    evaluators: Vec<Box<dyn TriggerEvaluator>>,
    action_tx: tokio::sync::mpsc::Sender<AgentAction>,
    tick_interval: Duration,
    trigger_interval: Duration,
    scheduler: Option<PresenceScheduler>,
}

impl AgentLoop {
    /// Create a new AgentLoop.
    ///
    /// Returns `(AgentLoop, Receiver)` — the receiver yields `AgentAction`s
    /// that the caller should handle (e.g. feed into `engine.think()`).
    pub fn new(
        coordinator: Arc<OrganismCoordinator>,
        evaluators: Vec<Box<dyn TriggerEvaluator>>,
        tick_interval: Duration,
        trigger_interval: Duration,
    ) -> (Self, tokio::sync::mpsc::Receiver<AgentAction>) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let scheduler = PresenceScheduler::new(tick_interval, trigger_interval);
        let agent = Self {
            coordinator,
            evaluators,
            action_tx: tx,
            tick_interval,
            trigger_interval,
            scheduler: Some(scheduler),
        };
        (agent, rx)
    }

    /// Evaluate all registered triggers (resilient to individual failures).
    ///
    /// Each evaluator is given a 10s timeout to prevent a hung evaluator
    /// from blocking the entire trigger cycle.
    async fn evaluate_triggers(&self) -> Vec<Trigger> {
        let mut triggers = Vec::new();
        for evaluator in &self.evaluators {
            match tokio::time::timeout(Duration::from_secs(10), evaluator.evaluate()).await {
                Ok(Ok(found)) => triggers.extend(found),
                Ok(Err(e)) => {
                    tracing::error!("AgentLoop: evaluator '{}' failed: {}", evaluator.name(), e)
                }
                Err(_) => tracing::error!(
                    "AgentLoop: evaluator '{}' timed out (10s)",
                    evaluator.name(),
                ),
            }
        }
        triggers
    }

    /// Spawn the background loop. Runs until the channel is closed (receiver dropped).
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick_sleep = self.tick_interval;
            let mut trigger_sleep = self.trigger_interval;

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(tick_sleep) => {
                        if let Err(e) = self.coordinator.tick().await {
                            tracing::warn!("AgentLoop tick error: {}", e);
                        }
                        match self.action_tx.try_send(AgentAction::StateUpdate) {
                            Ok(()) => {}
                            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                tracing::warn!("AgentLoop: action channel full, dropping StateUpdate");
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                tracing::info!("AgentLoop: receiver dropped, shutting down");
                                return;
                            }
                        }

                        // Evaluate OnTick rules and emit autonomous actions
                        let rule_actions = self.coordinator.evaluate_rules(
                            mneme_memory::RuleTrigger::OnTick,
                        ).await;
                        for (_id, action) in rule_actions {
                            if let mneme_memory::RuleAction::ExecuteTool { name, input } = action {
                                match self.action_tx.try_send(AgentAction::AutonomousToolUse {
                                    tool_name: name.clone(),
                                    input,
                                    goal_id: None,
                                }) {
                                    Ok(()) => {}
                                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                        tracing::warn!(
                                            "AgentLoop: action channel full, dropping AutonomousToolUse({})",
                                            name
                                        );
                                    }
                                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                        tracing::info!("AgentLoop: receiver dropped, shutting down");
                                        return;
                                    }
                                }
                            }
                        }

                        // Dynamic scheduling: recompute tick interval
                        if let Some(ref sched) = self.scheduler {
                            let state = self.coordinator.state().read().await.clone();
                            let lifecycle = self.coordinator.lifecycle_state().await;
                            tick_sleep = sched.next_tick_interval(&state, lifecycle);
                        }
                    }
                    _ = tokio::time::sleep(trigger_sleep) => {
                        let triggers = self.evaluate_triggers().await;
                        for trigger in triggers {
                            tracing::info!("AgentLoop trigger fired: {:?}", trigger);
                            if self.action_tx.send(AgentAction::ProactiveTrigger(trigger)).await.is_err() {
                                tracing::info!("AgentLoop: receiver dropped, shutting down");
                                return;
                            }
                        }

                        // Dynamic scheduling: recompute trigger interval
                        if let Some(ref sched) = self.scheduler {
                            let state = self.coordinator.state().read().await.clone();
                            let goal_count = if let Some(gm) = self.coordinator.goal_manager() {
                                gm.active_goals().await.map(|g| g.len()).unwrap_or(0)
                            } else {
                                0
                            };
                            trigger_sleep = sched.next_trigger_interval(&state, goal_count);
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mneme_core::Trigger;
    use mneme_limbic::LimbicSystem;

    /// A test evaluator that always returns a fixed set of triggers.
    struct FixedEvaluator {
        triggers: Vec<Trigger>,
    }

    #[async_trait::async_trait]
    impl TriggerEvaluator for FixedEvaluator {
        async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>> {
            Ok(self.triggers.clone())
        }
        fn name(&self) -> &'static str {
            "fixed_test"
        }
    }

    /// A test evaluator that always fails.
    struct FailingEvaluator;

    #[async_trait::async_trait]
    impl TriggerEvaluator for FailingEvaluator {
        async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>> {
            Err(anyhow::anyhow!("test failure"))
        }
        fn name(&self) -> &'static str {
            "failing_test"
        }
    }

    /// A test evaluator that hangs forever (for timeout testing).
    struct SlowEvaluator;

    #[async_trait::async_trait]
    impl TriggerEvaluator for SlowEvaluator {
        async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>> {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(vec![])
        }
        fn name(&self) -> &'static str {
            "slow_test"
        }
    }

    fn test_coordinator() -> Arc<OrganismCoordinator> {
        let limbic = Arc::new(LimbicSystem::new());
        Arc::new(OrganismCoordinator::new(limbic))
    }

    #[tokio::test]
    async fn test_evaluate_triggers_empty() {
        let (agent, _rx) = AgentLoop::new(
            test_coordinator(),
            vec![],
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let triggers = agent.evaluate_triggers().await;
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_evaluate_triggers_collects_all() {
        let eval = FixedEvaluator {
            triggers: vec![
                Trigger::Scheduled {
                    name: "morning".into(),
                    schedule: "08:00".into(),
                    route: None,
                },
                Trigger::Scheduled {
                    name: "evening".into(),
                    schedule: "22:00".into(),
                    route: None,
                },
            ],
        };
        let (agent, _rx) = AgentLoop::new(
            test_coordinator(),
            vec![Box::new(eval)],
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let triggers = agent.evaluate_triggers().await;
        assert_eq!(triggers.len(), 2);
    }

    #[tokio::test]
    async fn test_evaluate_triggers_resilient_to_failure() {
        let good = FixedEvaluator {
            triggers: vec![Trigger::Scheduled {
                name: "ok".into(),
                schedule: "08:00".into(),
                route: None,
            }],
        };
        let (agent, _rx) = AgentLoop::new(
            test_coordinator(),
            vec![Box::new(FailingEvaluator), Box::new(good)],
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        // Failing evaluator is skipped, good evaluator still works
        let triggers = agent.evaluate_triggers().await;
        assert_eq!(triggers.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn test_evaluate_triggers_timeout() {
        let good = FixedEvaluator {
            triggers: vec![Trigger::Scheduled {
                name: "ok".into(),
                schedule: "08:00".into(),
                route: None,
            }],
        };
        let (agent, _rx) = AgentLoop::new(
            test_coordinator(),
            vec![Box::new(SlowEvaluator), Box::new(good)],
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        // SlowEvaluator times out (10s in prod, but test uses tokio::time::pause)
        // Good evaluator still returns its trigger
        let triggers = agent.evaluate_triggers().await;
        assert_eq!(triggers.len(), 1);
    }

    #[tokio::test]
    async fn test_spawn_sends_state_update() {
        let (agent, mut rx) = AgentLoop::new(
            test_coordinator(),
            vec![],
            Duration::from_millis(50), // Fast tick
            Duration::from_secs(600),  // Slow trigger (won't fire in test)
        );
        let handle = agent.spawn();

        // Should receive a StateUpdate within a reasonable time
        let action = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
        assert!(action.is_ok());
        assert!(matches!(action.unwrap(), Some(AgentAction::StateUpdate)));

        handle.abort();
    }

    #[tokio::test]
    async fn test_spawn_sends_proactive_trigger() {
        let eval = FixedEvaluator {
            triggers: vec![Trigger::Scheduled {
                name: "test".into(),
                schedule: "08:00".into(),
                route: None,
            }],
        };
        let (agent, mut rx) = AgentLoop::new(
            test_coordinator(),
            vec![Box::new(eval)],
            Duration::from_secs(600),  // Slow tick
            Duration::from_millis(50), // Fast trigger
        );
        let handle = agent.spawn();

        // Collect actions until we see a ProactiveTrigger
        let mut found_trigger = false;
        for _ in 0..10 {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(AgentAction::ProactiveTrigger(_))) => {
                    found_trigger = true;
                    break;
                }
                Ok(Some(_)) => continue,
                _ => break,
            }
        }
        assert!(found_trigger, "Expected a ProactiveTrigger action");

        handle.abort();
    }

    #[tokio::test]
    async fn test_spawn_stops_when_receiver_dropped() {
        let eval = FixedEvaluator {
            triggers: vec![Trigger::Scheduled {
                name: "test".into(),
                schedule: "08:00".into(),
                route: None,
            }],
        };
        let (agent, rx) = AgentLoop::new(
            test_coordinator(),
            vec![Box::new(eval)],
            Duration::from_secs(600),
            Duration::from_millis(50),
        );
        let handle = agent.spawn();

        // Drop the receiver
        drop(rx);

        // The loop should exit gracefully
        let result = tokio::time::timeout(Duration::from_millis(500), handle).await;
        assert!(
            result.is_ok(),
            "AgentLoop should stop when receiver is dropped"
        );
    }
}
