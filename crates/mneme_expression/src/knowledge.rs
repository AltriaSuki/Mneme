//! #1284: Proactive knowledge organization.
//!
//! Periodically fires a Rumination trigger prompting the organism to
//! review, organize, and summarize accumulated knowledge.

use async_trait::async_trait;
use mneme_core::{Trigger, TriggerEvaluator};
use std::sync::atomic::{AtomicI64, Ordering};

pub struct KnowledgeMaintenanceEvaluator {
    /// Cooldown: 6 hours between maintenance runs.
    cooldown_secs: i64,
    last_fired: AtomicI64,
    episode_count: std::sync::Arc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = u64> + Send>> + Send + Sync>,
}

impl KnowledgeMaintenanceEvaluator {
    pub fn new<F, Fut>(episode_count_fn: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = u64> + Send + 'static,
    {
        let f = std::sync::Arc::new(move || -> std::pin::Pin<Box<dyn std::future::Future<Output = u64> + Send>> {
            Box::pin(episode_count_fn())
        });
        Self {
            cooldown_secs: 21600,
            last_fired: AtomicI64::new(0),
            episode_count: f,
        }
    }
}

#[async_trait]
impl TriggerEvaluator for KnowledgeMaintenanceEvaluator {
    async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>> {
        let now = chrono::Utc::now().timestamp();
        let last = self.last_fired.load(Ordering::Relaxed);
        if now - last < self.cooldown_secs {
            return Ok(Vec::new());
        }

        let count = (self.episode_count)().await;
        // Only trigger if there's meaningful content to organize (50+ episodes)
        if count < 50 {
            return Ok(Vec::new());
        }

        self.last_fired.store(now, Ordering::Relaxed);

        Ok(vec![Trigger::Rumination {
            kind: "knowledge_maintenance".to_string(),
            context: format!(
                "记忆库中有{}条记录。用 memory_manage search 回顾最近的知识，整理要点，固定重要记忆。",
                count
            ),
            route: None,
        }])
    }

    fn name(&self) -> &'static str {
        "KnowledgeMaintenanceEvaluator"
    }
}
