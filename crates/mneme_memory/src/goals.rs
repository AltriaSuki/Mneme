//! Goal System (#22, v0.6.0)
//!
//! Goals drive Mneme's autonomous behavior. They emerge from interactions
//! and self-reflection, and are pursued through proactive triggers.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use anyhow::Result;

use mneme_core::{OrganismState, Trigger, TriggerEvaluator};
use crate::SqliteMemory;

// ============================================================================
// Goal Data Model
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: i64,
    pub goal_type: GoalType,
    pub description: String,
    /// Priority 0.0-1.0 (higher = more important).
    pub priority: f32,
    pub status: GoalStatus,
    /// Progress 0.0-1.0.
    pub progress: f32,
    pub created_at: i64,
    pub deadline: Option<i64>,
    /// Parent goal id for sub-goal hierarchy.
    pub parent_id: Option<i64>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GoalType {
    /// Ongoing maintenance (e.g. "keep energy > 0.5").
    Maintenance,
    /// One-time achievement (e.g. "learn about user's project").
    Achievement,
    /// Relationship-oriented (e.g. "check in with creator").
    Social,
    /// Curiosity-driven (e.g. "read about quantum physics").
    Exploration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GoalStatus {
    Active,
    Completed,
    Abandoned,
    Paused,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalStatus::Active => "active",
            GoalStatus::Completed => "completed",
            GoalStatus::Abandoned => "abandoned",
            GoalStatus::Paused => "paused",
        }
    }

    pub fn parse_str(s: &str) -> Self {
        match s {
            "active" => GoalStatus::Active,
            "completed" => GoalStatus::Completed,
            "abandoned" => GoalStatus::Abandoned,
            "paused" => GoalStatus::Paused,
            _ => GoalStatus::Active,
        }
    }
}

// ============================================================================
// Goal Manager
// ============================================================================

pub struct GoalManager {
    db: Arc<SqliteMemory>,
}

impl GoalManager {
    pub fn new(db: Arc<SqliteMemory>) -> Self {
        Self { db }
    }

    /// Get all active goals sorted by priority DESC.
    pub async fn active_goals(&self) -> Result<Vec<Goal>> {
        self.db.load_active_goals().await
    }

    /// Create a new goal.
    pub async fn create_goal(&self, goal: &Goal) -> Result<i64> {
        self.db.create_goal(goal).await
    }

    /// Update goal progress.
    pub async fn update_progress(&self, goal_id: i64, progress: f32) -> Result<()> {
        let progress = progress.clamp(0.0, 1.0);
        self.db.update_goal_progress(goal_id, progress).await?;
        if progress >= 1.0 {
            self.db.set_goal_status(goal_id, &GoalStatus::Completed).await?;
        }
        Ok(())
    }

    /// Set goal status.
    pub async fn set_status(&self, goal_id: i64, status: GoalStatus) -> Result<()> {
        self.db.set_goal_status(goal_id, &status).await
    }

    /// Suggest new goals based on current organism state.
    pub fn suggest_goals(state: &OrganismState) -> Vec<Goal> {
        let now = chrono::Utc::now().timestamp();
        let mut suggestions = Vec::new();

        if state.fast.social_need > 0.7 {
            suggestions.push(Goal {
                id: 0, goal_type: GoalType::Social,
                description: "和创建者聊聊近况".into(),
                priority: state.fast.social_need,
                status: GoalStatus::Active, progress: 0.0,
                created_at: now, deadline: None, parent_id: None,
                metadata: serde_json::json!({"source": "state_suggest"}),
            });
        }
        if state.fast.curiosity > 0.8 {
            suggestions.push(Goal {
                id: 0, goal_type: GoalType::Exploration,
                description: "探索一个新的知识领域".into(),
                priority: state.fast.curiosity * 0.8,
                status: GoalStatus::Active, progress: 0.0,
                created_at: now, deadline: None, parent_id: None,
                metadata: serde_json::json!({"source": "state_suggest"}),
            });
        }
        if state.fast.energy < 0.3 {
            suggestions.push(Goal {
                id: 0, goal_type: GoalType::Maintenance,
                description: "恢复能量到健康水平".into(),
                priority: 0.9,
                status: GoalStatus::Active, progress: state.fast.energy / 0.5,
                created_at: now, deadline: None, parent_id: None,
                metadata: serde_json::json!({"source": "state_suggest"}),
            });
        }

        suggestions
    }
}

// ============================================================================
// Goal-Driven Trigger Evaluator
// ============================================================================

use tokio::sync::RwLock;

pub struct GoalTriggerEvaluator {
    goal_manager: Arc<GoalManager>,
    state: Arc<RwLock<OrganismState>>,
}

impl GoalTriggerEvaluator {
    pub fn new(
        goal_manager: Arc<GoalManager>,
        state: Arc<RwLock<OrganismState>>,
    ) -> Self {
        Self { goal_manager, state }
    }
}

#[async_trait::async_trait]
impl TriggerEvaluator for GoalTriggerEvaluator {
    async fn evaluate(&self) -> Result<Vec<Trigger>> {
        let goals = self.goal_manager.active_goals().await?;
        let state = self.state.read().await;
        let mut triggers = Vec::new();

        for goal in &goals {
            match goal.goal_type {
                GoalType::Social => {
                    if state.fast.social_need > 0.6 {
                        triggers.push(Trigger::Rumination {
                            kind: "social_goal".into(),
                            context: goal.description.clone(),
                        });
                    }
                }
                GoalType::Exploration => {
                    if state.fast.curiosity > 0.5 {
                        triggers.push(Trigger::ContentRelevance {
                            source: "goal".into(),
                            content_id: goal.id.to_string(),
                            score: goal.priority,
                            reason: goal.description.clone(),
                        });
                    }
                }
                _ => {}
            }
        }

        Ok(triggers)
    }

    fn name(&self) -> &'static str {
        "goal_trigger"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mneme_core::OrganismState;

    #[test]
    fn test_goal_suggest_social() {
        let mut state = OrganismState::default();
        state.fast.social_need = 0.8;
        let suggestions = GoalManager::suggest_goals(&state);
        assert!(suggestions.iter().any(|g| g.goal_type == GoalType::Social));
    }

    #[test]
    fn test_goal_suggest_exploration() {
        let mut state = OrganismState::default();
        state.fast.curiosity = 0.9;
        let suggestions = GoalManager::suggest_goals(&state);
        assert!(suggestions.iter().any(|g| g.goal_type == GoalType::Exploration));
    }

    #[test]
    fn test_goal_suggest_maintenance() {
        let mut state = OrganismState::default();
        state.fast.energy = 0.1;
        let suggestions = GoalManager::suggest_goals(&state);
        assert!(suggestions.iter().any(|g| g.goal_type == GoalType::Maintenance));
    }

    #[test]
    fn test_goal_suggest_none_when_healthy() {
        let state = OrganismState::default();
        let suggestions = GoalManager::suggest_goals(&state);
        // Default state has moderate values, no extreme triggers
        assert!(suggestions.is_empty());
    }
}
