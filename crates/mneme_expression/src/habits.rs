//! B-21: Habit Formation — detect repeated behavioral patterns and trigger rumination.
//!
//! The HabitDetector periodically scans self_knowledge for repeated entries
//! (same domain + content appearing multiple times). When patterns are found,
//! it fires a Rumination trigger so the reasoning engine can reflect on them.

use async_trait::async_trait;
use mneme_core::{Memory, Trigger, TriggerEvaluator};
use std::sync::Arc;

/// Configuration for habit detection.
#[derive(Debug, Clone)]
pub struct HabitConfig {
    /// Minimum number of occurrences to consider a pattern (default: 3).
    pub min_count: usize,
    /// Maximum number of patterns to report per evaluation (default: 3).
    pub max_patterns: usize,
}

impl Default for HabitConfig {
    fn default() -> Self {
        Self {
            min_count: 3,
            max_patterns: 3,
        }
    }
}

/// Evaluator that detects repeated behavioral patterns in self_knowledge.
///
/// When patterns are found, fires Rumination triggers with habit context
/// so the reasoning engine can reflect on and potentially consolidate them.
pub struct HabitDetector {
    memory: Arc<dyn Memory>,
    config: HabitConfig,
}

impl HabitDetector {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self {
            memory,
            config: HabitConfig::default(),
        }
    }

    pub fn with_config(memory: Arc<dyn Memory>, config: HabitConfig) -> Self {
        Self { memory, config }
    }
}

#[async_trait]
impl TriggerEvaluator for HabitDetector {
    async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>> {
        let patterns = self
            .memory
            .detect_repeated_patterns(self.config.min_count)
            .await?;

        let triggers: Vec<Trigger> = patterns
            .into_iter()
            .take(self.config.max_patterns)
            .map(|(pattern, count)| Trigger::Rumination {
                kind: "habit_detected".to_string(),
                context: format!(
                    "反复出现的行为模式 ({}次): {}。这是否已经成为一种习惯？值得反思。",
                    count, pattern
                ),
                route: None,
            })
            .collect();

        if !triggers.is_empty() {
            tracing::info!("HabitDetector: found {} repeated patterns", triggers.len());
        }

        Ok(triggers)
    }

    fn name(&self) -> &'static str {
        "HabitDetector"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mneme_core::Content;

    /// Mock memory that returns predefined patterns.
    struct MockMemory {
        patterns: Vec<(String, usize)>,
    }

    #[async_trait]
    impl Memory for MockMemory {
        async fn recall(&self, _query: &str) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn memorize(&self, _content: &Content) -> anyhow::Result<()> {
            Ok(())
        }
        async fn detect_repeated_patterns(
            &self,
            min_count: usize,
        ) -> anyhow::Result<Vec<(String, usize)>> {
            Ok(self
                .patterns
                .iter()
                .filter(|(_, c)| *c >= min_count)
                .cloned()
                .collect())
        }
    }

    #[tokio::test]
    async fn test_habit_detector_fires_triggers() {
        let memory = Arc::new(MockMemory {
            patterns: vec![
                ("[behavior] 回避社交".to_string(), 5),
                ("[emotion] 焦虑感".to_string(), 3),
            ],
        });
        let detector = HabitDetector::new(memory);
        let triggers = detector.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 2);
        match &triggers[0] {
            Trigger::Rumination { kind, context, .. } => {
                assert_eq!(kind, "habit_detected");
                assert!(context.contains("回避社交"));
                assert!(context.contains("5次"));
            }
            _ => panic!("Expected Rumination trigger"),
        }
    }

    #[tokio::test]
    async fn test_habit_detector_respects_min_count() {
        let memory = Arc::new(MockMemory {
            patterns: vec![("[behavior] 偶尔出现".to_string(), 2)],
        });
        let detector = HabitDetector::new(memory); // default min_count = 3
        let triggers = detector.evaluate().await.unwrap();
        assert!(triggers.is_empty(), "Should not fire for count < min_count");
    }

    #[tokio::test]
    async fn test_habit_detector_caps_at_max_patterns() {
        let memory = Arc::new(MockMemory {
            patterns: vec![
                ("[a] p1".to_string(), 10),
                ("[a] p2".to_string(), 8),
                ("[a] p3".to_string(), 6),
                ("[a] p4".to_string(), 4),
                ("[a] p5".to_string(), 3),
            ],
        });
        let config = HabitConfig {
            min_count: 3,
            max_patterns: 2,
        };
        let detector = HabitDetector::with_config(memory, config);
        let triggers = detector.evaluate().await.unwrap();
        assert_eq!(triggers.len(), 2, "Should cap at max_patterns");
    }
}
