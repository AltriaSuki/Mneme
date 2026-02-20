//! B-17: Single-threaded Attention — priority competition among triggers.
//!
//! The AttentionGate wraps multiple TriggerEvaluators and enforces a
//! single-focus model: only the highest-priority trigger wins per evaluation
//! cycle. Priority order: external > high-urgency internal > low-urgency.
//! An engagement-modulated interrupt threshold prevents low-priority triggers
//! from breaking focus during active interaction.

use async_trait::async_trait;
use mneme_core::{Trigger, TriggerEvaluator};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Priority tier for trigger competition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Priority {
    /// Low-urgency internal (rumination, low-res monologue)
    Low = 0,
    /// Medium-urgency internal (habit detection, metacognition)
    Medium = 1,
    /// High-urgency internal (high-res monologue, stress spike)
    High = 2,
    /// External triggers (scheduled, content relevance, trending)
    External = 3,
}

/// Configuration for the attention gate.
#[derive(Debug, Clone)]
pub struct AttentionConfig {
    /// Base interrupt threshold (0.0-1.0). Triggers with normalized priority
    /// below this are suppressed. Default: 0.3.
    pub base_threshold: f32,
    /// How much engagement raises the threshold. At max engagement (1.0),
    /// effective threshold = base_threshold + engagement_boost. Default: 0.4.
    pub engagement_boost: f32,
    /// Maximum number of triggers to emit per cycle. Default: 1.
    pub max_triggers: usize,
}

impl Default for AttentionConfig {
    fn default() -> Self {
        Self {
            base_threshold: 0.3,
            engagement_boost: 0.4,
            max_triggers: 1,
        }
    }
}

/// Shared handle for updating engagement level from outside the AttentionGate.
///
/// Obtain via `AttentionGate::engagement_handle()` before moving the gate
/// into an evaluator collection. The handle and gate share the same atomic,
/// so updates are lock-free and immediate.
#[derive(Clone)]
pub struct EngagementHandle {
    inner: Arc<AtomicU64>,
}

impl EngagementHandle {
    /// Set engagement level (clamped to [0.0, 1.0]).
    pub fn set(&self, level: f32) {
        let clamped = level.clamp(0.0, 1.0) as f64;
        self.inner.store(clamped.to_bits(), Ordering::Relaxed);
    }

    /// Get current engagement level.
    pub fn get(&self) -> f32 {
        f64::from_bits(self.inner.load(Ordering::Relaxed)) as f32
    }

    /// Multiplicative decay (e.g. 0.85 per tick). Useful for gradual cooldown.
    pub fn decay(&self, factor: f32) {
        let current = self.get();
        self.set(current * factor);
    }
}

/// Wraps multiple evaluators and enforces single-focus attention.
pub struct AttentionGate {
    evaluators: Vec<Box<dyn TriggerEvaluator>>,
    config: AttentionConfig,
    /// Shared engagement level (lock-free via AtomicU64).
    engagement: Arc<AtomicU64>,
}

impl AttentionGate {
    pub fn new(evaluators: Vec<Box<dyn TriggerEvaluator>>) -> Self {
        Self {
            evaluators,
            config: AttentionConfig::default(),
            engagement: Arc::new(AtomicU64::new(0.0f64.to_bits())),
        }
    }

    pub fn with_config(
        evaluators: Vec<Box<dyn TriggerEvaluator>>,
        config: AttentionConfig,
    ) -> Self {
        Self {
            evaluators,
            config,
            engagement: Arc::new(AtomicU64::new(0.0f64.to_bits())),
        }
    }

    /// Get a shared handle for updating engagement from outside.
    /// Call this before moving the gate into an evaluator collection.
    pub fn engagement_handle(&self) -> EngagementHandle {
        EngagementHandle {
            inner: self.engagement.clone(),
        }
    }

    /// Update engagement level (called after user interaction).
    /// Value is clamped to [0.0, 1.0].
    pub fn set_engagement(&self, level: f32) {
        let clamped = level.clamp(0.0, 1.0) as f64;
        self.engagement.store(clamped.to_bits(), Ordering::Relaxed);
    }

    /// Get current engagement level.
    pub fn engagement(&self) -> f32 {
        f64::from_bits(self.engagement.load(Ordering::Relaxed)) as f32
    }

    /// Compute effective interrupt threshold based on engagement.
    fn effective_threshold(&self) -> f32 {
        let engagement = self.engagement();
        (self.config.base_threshold + self.config.engagement_boost * engagement).clamp(0.0, 1.0)
    }
}

/// Classify a trigger into a priority tier.
fn classify_trigger(trigger: &Trigger) -> Priority {
    match trigger {
        // External triggers get highest priority
        Trigger::Scheduled { .. } | Trigger::ContentRelevance { .. } | Trigger::Trending { .. } => {
            Priority::External
        }

        // Memory decay is external but lower urgency
        Trigger::MemoryDecay { .. } => Priority::High,

        // High-resolution inner monologue = high urgency
        Trigger::InnerMonologue {
            resolution: mneme_core::MonologueResolution::High,
            ..
        } => Priority::High,

        // Metacognition = medium urgency
        Trigger::Metacognition { .. } => Priority::Medium,

        // Low-resolution monologue, rumination, habits = low urgency
        Trigger::InnerMonologue { .. } => Priority::Low,
        Trigger::Rumination { .. } => Priority::Low,
    }
}

/// Normalize priority to [0.0, 1.0] for threshold comparison.
fn priority_score(priority: Priority) -> f32 {
    match priority {
        Priority::Low => 0.2,
        Priority::Medium => 0.5,
        Priority::High => 0.75,
        Priority::External => 1.0,
    }
}

#[async_trait]
impl TriggerEvaluator for AttentionGate {
    async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>> {
        // Collect all triggers from wrapped evaluators (resilient to failures)
        let mut candidates: Vec<(Trigger, Priority)> = Vec::new();
        for evaluator in &self.evaluators {
            match evaluator.evaluate().await {
                Ok(triggers) => {
                    for t in triggers {
                        let priority = classify_trigger(&t);
                        candidates.push((t, priority));
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "AttentionGate: evaluator {} failed: {}",
                        evaluator.name(),
                        e
                    );
                }
            }
        }

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        // Sort by priority (highest first)
        candidates.sort_by(|a, b| b.1.cmp(&a.1));

        // Apply interrupt threshold
        let threshold = self.effective_threshold();
        let filtered: Vec<Trigger> = candidates
            .into_iter()
            .filter(|(_, priority)| priority_score(*priority) >= threshold)
            .take(self.config.max_triggers)
            .map(|(trigger, _)| trigger)
            .collect();

        if !filtered.is_empty() {
            tracing::info!(
                "AttentionGate: {} trigger(s) passed (threshold={:.2}, engagement={:.2})",
                filtered.len(),
                threshold,
                self.engagement(),
            );
        }

        Ok(filtered)
    }

    fn name(&self) -> &'static str {
        "AttentionGate"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mneme_core::MonologueResolution;

    /// Mock evaluator that returns predefined triggers.
    struct MockEvaluator {
        triggers: Vec<Trigger>,
        label: &'static str,
    }

    #[async_trait]
    impl TriggerEvaluator for MockEvaluator {
        async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>> {
            Ok(self.triggers.clone())
        }
        fn name(&self) -> &'static str {
            self.label
        }
    }

    fn scheduled_trigger() -> Trigger {
        Trigger::Scheduled {
            name: "morning".to_string(),
            schedule: "0 8 * * *".to_string(),
            route: None,
        }
    }

    fn rumination_trigger() -> Trigger {
        Trigger::Rumination {
            kind: "mind_wandering".to_string(),
            context: "想起了什么".to_string(),
        }
    }

    fn high_monologue_trigger() -> Trigger {
        Trigger::InnerMonologue {
            cause: "stress_spike".to_string(),
            seed: "压力很大".to_string(),
            resolution: MonologueResolution::High,
        }
    }

    fn metacognition_trigger() -> Trigger {
        Trigger::Metacognition {
            trigger_reason: "periodic".to_string(),
            context_summary: "状态正常".to_string(),
        }
    }

    #[tokio::test]
    async fn test_priority_competition_external_wins() {
        let evaluators: Vec<Box<dyn TriggerEvaluator>> = vec![
            Box::new(MockEvaluator {
                triggers: vec![rumination_trigger()],
                label: "rumination",
            }),
            Box::new(MockEvaluator {
                triggers: vec![scheduled_trigger()],
                label: "scheduled",
            }),
        ];
        let gate = AttentionGate::new(evaluators);
        let result = gate.evaluate().await.unwrap();
        // Only 1 trigger (max_triggers=1), and it should be the scheduled (external) one
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], Trigger::Scheduled { .. }));
    }

    #[tokio::test]
    async fn test_high_engagement_suppresses_low_priority() {
        let evaluators: Vec<Box<dyn TriggerEvaluator>> = vec![Box::new(MockEvaluator {
            triggers: vec![rumination_trigger()],
            label: "rumination",
        })];
        let gate = AttentionGate::new(evaluators);
        // High engagement: threshold = 0.3 + 0.4*1.0 = 0.7
        // Rumination priority score = 0.2 < 0.7 → suppressed
        gate.set_engagement(1.0);
        let result = gate.evaluate().await.unwrap();
        assert!(
            result.is_empty(),
            "Low-priority trigger should be suppressed during high engagement"
        );
    }

    #[tokio::test]
    async fn test_external_survives_high_engagement() {
        let evaluators: Vec<Box<dyn TriggerEvaluator>> = vec![Box::new(MockEvaluator {
            triggers: vec![scheduled_trigger()],
            label: "scheduled",
        })];
        let gate = AttentionGate::new(evaluators);
        gate.set_engagement(1.0);
        let result = gate.evaluate().await.unwrap();
        // External priority score = 1.0 >= threshold 0.7 → passes
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn test_idle_allows_low_priority() {
        // Default threshold 0.3 would still suppress rumination (0.2),
        // so use a lower base threshold to simulate a relaxed idle state.
        let gate = AttentionGate::with_config(
            vec![Box::new(MockEvaluator {
                triggers: vec![rumination_trigger()],
                label: "rumination",
            })],
            AttentionConfig {
                base_threshold: 0.1,
                engagement_boost: 0.4,
                max_triggers: 1,
            },
        );
        let result = gate.evaluate().await.unwrap();
        assert_eq!(
            result.len(),
            1,
            "Low-priority trigger should pass when idle with low threshold"
        );
    }

    #[tokio::test]
    async fn test_focus_persistence_medium_vs_high() {
        let evaluators: Vec<Box<dyn TriggerEvaluator>> = vec![
            Box::new(MockEvaluator {
                triggers: vec![metacognition_trigger()],
                label: "metacognition",
            }),
            Box::new(MockEvaluator {
                triggers: vec![high_monologue_trigger()],
                label: "monologue",
            }),
        ];
        let gate = AttentionGate::new(evaluators);
        let result = gate.evaluate().await.unwrap();
        // High monologue (High=0.75) beats metacognition (Medium=0.5)
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], Trigger::InnerMonologue { .. }));
    }

    #[tokio::test]
    async fn test_empty_evaluators() {
        let gate = AttentionGate::new(Vec::new());
        let result = gate.evaluate().await.unwrap();
        assert!(result.is_empty());
    }
}
