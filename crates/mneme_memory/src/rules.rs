//! Declarative Behavior Rule Engine (ADR-004)
//!
//! Key behavior decisions are made by database-driven rules, not LLM calls.
//! Only content generation goes through the LLM. Rules follow a
//! trigger → condition → action three-stage pattern.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use anyhow::Result;

use mneme_core::OrganismState;
use crate::coordinator::LifecycleState;
use crate::SqliteMemory;

// ============================================================================
// Rule Data Model
// ============================================================================

/// A declarative behavior rule: trigger → condition → action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorRule {
    pub id: i64,
    pub name: String,
    /// Higher priority rules are evaluated first.
    pub priority: i32,
    pub enabled: bool,
    pub trigger: RuleTrigger,
    pub condition: RuleCondition,
    pub action: RuleAction,
    /// Minimum seconds between firings (None = no cooldown).
    pub cooldown_secs: Option<i64>,
    /// Unix timestamp of last firing (None = never fired).
    pub last_fired: Option<i64>,
}

/// When to evaluate this rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum RuleTrigger {
    /// Evaluate on every incoming user message.
    OnMessage,
    /// Evaluate on periodic tick.
    OnTick,
    /// Evaluate when a state field crosses a threshold.
    OnStateChange { field: String, threshold: f32 },
    /// Evaluate on a cron-like schedule.
    OnSchedule { cron: String },
}

/// Condition clause — must be satisfied for the rule to fire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum RuleCondition {
    /// Always true.
    Always,
    /// All sub-conditions must be true.
    All { conditions: Vec<RuleCondition> },
    /// Any sub-condition must be true.
    Any { conditions: Vec<RuleCondition> },
    /// State field > value.
    StateGt { field: String, value: f32 },
    /// State field < value.
    StateLt { field: String, value: f32 },
    /// Current hour is between start and end (24h clock).
    TimeBetween { start_hour: u32, end_hour: u32 },
    /// Cooldown has elapsed since last firing.
    CooldownElapsed { secs: i64 },
    /// Minimum interaction count reached.
    InteractionCount { min: u32 },
    /// Lifecycle is in a specific state.
    LifecycleIs { state: String },
}

/// Action to execute when a rule fires.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum RuleAction {
    /// Set the decision level for routing.
    SetDecisionLevel { level: String },
    /// Emit a proactive trigger.
    EmitTrigger { trigger_json: String },
    /// Modify a state field by a delta.
    ModifyState { field: String, delta: f32 },
    /// Transition lifecycle state.
    TransitionLifecycle { target: String },
    /// Execute a tool autonomously.
    ExecuteTool { name: String, input: serde_json::Value },
    /// Composite: execute multiple actions.
    Composite { actions: Vec<RuleAction> },
}

// ============================================================================
// Rule Context — snapshot of state for evaluation
// ============================================================================

/// Context provided to the rule engine for condition evaluation.
#[derive(Debug, Clone)]
pub struct RuleContext {
    pub trigger_type: RuleTrigger,
    pub state: OrganismState,
    pub current_hour: u32,
    pub interaction_count: u32,
    pub lifecycle: LifecycleState,
    pub now: i64,
    /// Optional: the user message text (for OnMessage rules).
    pub message_text: Option<String>,
}

// ============================================================================
// Rule Engine
// ============================================================================

/// The rule engine loads rules from SQLite and evaluates them against context.
pub struct RuleEngine {
    rules: Vec<BehaviorRule>,
    db: Option<Arc<SqliteMemory>>,
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleEngine {
    /// Create an empty rule engine (no DB persistence).
    pub fn new() -> Self {
        Self { rules: Vec::new(), db: None }
    }

    /// Load all enabled rules from the database.
    pub async fn load(db: Arc<SqliteMemory>) -> Result<Self> {
        let rules = db.load_behavior_rules().await?;
        tracing::info!("Loaded {} behavior rules", rules.len());
        Ok(Self { rules, db: Some(db) })
    }

    /// Add a rule programmatically (for testing or seed rules).
    pub fn add_rule(&mut self, rule: BehaviorRule) {
        self.rules.push(rule);
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Get all loaded rules.
    pub fn rules(&self) -> &[BehaviorRule] {
        &self.rules
    }

    /// Evaluate all rules against the given context.
    /// Returns matched (rule_id, action) pairs. First-match-wins for same priority.
    pub async fn evaluate(&mut self, ctx: &RuleContext) -> Vec<(i64, RuleAction)> {
        let mut results = Vec::new();

        for rule in &mut self.rules {
            if !rule.enabled {
                continue;
            }
            // Check trigger type matches
            if !trigger_matches(&rule.trigger, &ctx.trigger_type) {
                continue;
            }
            // Check cooldown
            if let Some(cooldown) = rule.cooldown_secs {
                if let Some(last) = rule.last_fired {
                    if ctx.now - last < cooldown {
                        continue;
                    }
                }
            }
            // Check condition
            if !evaluate_condition(&rule.condition, ctx, rule.last_fired) {
                continue;
            }
            // Rule matched — record firing time
            rule.last_fired = Some(ctx.now);
            results.push((rule.id, rule.action.clone()));
        }

        // Persist updated last_fired timestamps
        if let Some(ref db) = self.db {
            for (id, _) in &results {
                if let Err(e) = db.update_rule_last_fired(*id, ctx.now).await {
                    tracing::warn!("Failed to update rule last_fired: {}", e);
                }
            }
        }

        results
    }
}

// ============================================================================
// Trigger & Condition Evaluation Helpers
// ============================================================================

fn trigger_matches(rule_trigger: &RuleTrigger, ctx_trigger: &RuleTrigger) -> bool {
    match (rule_trigger, ctx_trigger) {
        (RuleTrigger::OnMessage, RuleTrigger::OnMessage) => true,
        (RuleTrigger::OnTick, RuleTrigger::OnTick) => true,
        (
            RuleTrigger::OnStateChange { field: rf, threshold: rt },
            RuleTrigger::OnStateChange { field: cf, threshold: ct },
        ) => rf == cf && (rt - ct).abs() < f32::EPSILON,
        (
            RuleTrigger::OnSchedule { cron: rc },
            RuleTrigger::OnSchedule { cron: cc },
        ) => rc == cc,
        _ => false,
    }
}

fn get_state_field(state: &OrganismState, field: &str) -> Option<f32> {
    match field {
        "energy" => Some(state.fast.energy),
        "stress" => Some(state.fast.stress),
        "valence" => Some(state.fast.affect.valence),
        "arousal" => Some(state.fast.affect.arousal),
        "social_need" => Some(state.fast.social_need),
        "curiosity" => Some(state.fast.curiosity),
        "boredom" => Some(state.fast.boredom),
        "mood_bias" => Some(state.medium.mood_bias),
        "openness" => Some(state.medium.openness),
        _ => None,
    }
}

fn evaluate_condition(cond: &RuleCondition, ctx: &RuleContext, last_fired: Option<i64>) -> bool {
    match cond {
        RuleCondition::Always => true,
        RuleCondition::All { conditions } => {
            conditions.iter().all(|c| evaluate_condition(c, ctx, last_fired))
        }
        RuleCondition::Any { conditions } => {
            conditions.iter().any(|c| evaluate_condition(c, ctx, last_fired))
        }
        RuleCondition::StateGt { field, value } => {
            get_state_field(&ctx.state, field).is_some_and(|v| v > *value)
        }
        RuleCondition::StateLt { field, value } => {
            get_state_field(&ctx.state, field).is_some_and(|v| v < *value)
        }
        RuleCondition::TimeBetween { start_hour, end_hour } => {
            if start_hour <= end_hour {
                ctx.current_hour >= *start_hour && ctx.current_hour < *end_hour
            } else {
                // Wraps midnight: e.g. 22..6
                ctx.current_hour >= *start_hour || ctx.current_hour < *end_hour
            }
        }
        RuleCondition::CooldownElapsed { secs } => {
            match last_fired {
                None => true,
                Some(last) => ctx.now - last >= *secs,
            }
        }
        RuleCondition::InteractionCount { min } => ctx.interaction_count >= *min,
        RuleCondition::LifecycleIs { state } => {
            let current = format!("{:?}", ctx.lifecycle);
            current.eq_ignore_ascii_case(state)
        }
    }
}

// ============================================================================
// Seed Rules — default rules that replace hardcoded behavior
// ============================================================================

/// Create the default seed rules for a fresh Mneme instance.
pub fn seed_rules() -> Vec<BehaviorRule> {
    vec![
        BehaviorRule {
            id: 0, name: "low_energy_silence".into(), priority: 50, enabled: true,
            trigger: RuleTrigger::OnTick,
            condition: RuleCondition::StateLt { field: "energy".into(), value: 0.2 },
            action: RuleAction::ModifyState { field: "boredom".into(), delta: 0.3 },
            cooldown_secs: Some(300), last_fired: None,
        },
        BehaviorRule {
            id: 0, name: "night_drowsy".into(), priority: 40, enabled: true,
            trigger: RuleTrigger::OnTick,
            condition: RuleCondition::All { conditions: vec![
                RuleCondition::TimeBetween { start_hour: 2, end_hour: 6 },
                RuleCondition::InteractionCount { min: 10 },
            ]},
            action: RuleAction::TransitionLifecycle { target: "drowsy".into() },
            cooldown_secs: Some(3600), last_fired: None,
        },
        BehaviorRule {
            id: 0, name: "greeting_quick".into(), priority: 60, enabled: true,
            trigger: RuleTrigger::OnMessage,
            condition: RuleCondition::Always,
            action: RuleAction::SetDecisionLevel { level: "quick".into() },
            cooldown_secs: None, last_fired: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mneme_core::OrganismState;

    fn test_ctx(trigger: RuleTrigger) -> RuleContext {
        RuleContext {
            trigger_type: trigger,
            state: OrganismState::default(),
            current_hour: 14,
            interaction_count: 5,
            lifecycle: LifecycleState::Awake,
            now: 1000000,
            message_text: None,
        }
    }

    #[test]
    fn test_rule_condition_all_logic() {
        let cond = RuleCondition::All {
            conditions: vec![
                RuleCondition::StateGt { field: "energy".into(), value: 0.3 },
                RuleCondition::InteractionCount { min: 3 },
            ],
        };
        let ctx = test_ctx(RuleTrigger::OnTick);
        // Default energy is 0.7, interaction_count is 5 → both true
        assert!(evaluate_condition(&cond, &ctx, None));
    }

    #[test]
    fn test_rule_condition_any_logic() {
        let cond = RuleCondition::Any {
            conditions: vec![
                RuleCondition::StateLt { field: "energy".into(), value: 0.1 },
                RuleCondition::InteractionCount { min: 3 },
            ],
        };
        let ctx = test_ctx(RuleTrigger::OnTick);
        // energy 0.7 > 0.1 (false), but interaction 5 >= 3 (true)
        assert!(evaluate_condition(&cond, &ctx, None));
    }

    #[tokio::test]
    async fn test_rule_priority_ordering() {
        let mut engine = RuleEngine::new();
        engine.add_rule(BehaviorRule {
            id: 1, name: "low_pri".into(), priority: 10, enabled: true,
            trigger: RuleTrigger::OnTick,
            condition: RuleCondition::Always,
            action: RuleAction::ModifyState { field: "energy".into(), delta: 0.1 },
            cooldown_secs: None, last_fired: None,
        });
        engine.add_rule(BehaviorRule {
            id: 2, name: "high_pri".into(), priority: 100, enabled: true,
            trigger: RuleTrigger::OnTick,
            condition: RuleCondition::Always,
            action: RuleAction::SetDecisionLevel { level: "quick".into() },
            cooldown_secs: None, last_fired: None,
        });

        let ctx = test_ctx(RuleTrigger::OnTick);
        let results = engine.evaluate(&ctx).await;
        assert_eq!(results.len(), 2);
        // High priority rule should be first
        assert_eq!(results[0].0, 2);
        assert_eq!(results[1].0, 1);
    }

    #[tokio::test]
    async fn test_rule_cooldown_respected() {
        let mut engine = RuleEngine::new();
        engine.add_rule(BehaviorRule {
            id: 1, name: "cooldown_rule".into(), priority: 50, enabled: true,
            trigger: RuleTrigger::OnTick,
            condition: RuleCondition::Always,
            action: RuleAction::ModifyState { field: "energy".into(), delta: 0.1 },
            cooldown_secs: Some(600),
            last_fired: Some(999500), // fired 500s ago
        });

        let ctx = test_ctx(RuleTrigger::OnTick); // now = 1000000
        let results = engine.evaluate(&ctx).await;
        // 1000000 - 999500 = 500 < 600 cooldown → should NOT fire
        assert!(results.is_empty());
    }

    #[test]
    fn test_seed_rules_loaded() {
        let rules = seed_rules();
        assert_eq!(rules.len(), 3);
        assert!(rules.iter().any(|r| r.name == "low_energy_silence"));
        assert!(rules.iter().any(|r| r.name == "night_drowsy"));
        assert!(rules.iter().any(|r| r.name == "greeting_quick"));
    }

    #[test]
    fn test_trigger_matches_compares_inner_data() {
        // OnMessage/OnTick: variant-only match
        assert!(trigger_matches(&RuleTrigger::OnMessage, &RuleTrigger::OnMessage));
        assert!(trigger_matches(&RuleTrigger::OnTick, &RuleTrigger::OnTick));
        assert!(!trigger_matches(&RuleTrigger::OnMessage, &RuleTrigger::OnTick));

        // OnStateChange: must match field AND threshold
        let energy_rule = RuleTrigger::OnStateChange { field: "energy".into(), threshold: 0.3 };
        let energy_ctx = RuleTrigger::OnStateChange { field: "energy".into(), threshold: 0.3 };
        let stress_ctx = RuleTrigger::OnStateChange { field: "stress".into(), threshold: 0.3 };
        let energy_diff = RuleTrigger::OnStateChange { field: "energy".into(), threshold: 0.9 };
        assert!(trigger_matches(&energy_rule, &energy_ctx));
        assert!(!trigger_matches(&energy_rule, &stress_ctx), "different field should not match");
        assert!(!trigger_matches(&energy_rule, &energy_diff), "different threshold should not match");

        // OnSchedule: must match cron expression
        let cron_a = RuleTrigger::OnSchedule { cron: "0 * * * *".into() };
        let cron_b = RuleTrigger::OnSchedule { cron: "0 * * * *".into() };
        let cron_c = RuleTrigger::OnSchedule { cron: "30 * * * *".into() };
        assert!(trigger_matches(&cron_a, &cron_b));
        assert!(!trigger_matches(&cron_a, &cron_c), "different cron should not match");

        // Cross-variant never matches
        assert!(!trigger_matches(&energy_rule, &RuleTrigger::OnTick));
        assert!(!trigger_matches(&cron_a, &RuleTrigger::OnMessage));
    }

    #[test]
    fn test_time_between_wraps_midnight() {
        let cond = RuleCondition::TimeBetween { start_hour: 22, end_hour: 6 };
        let mut ctx = test_ctx(RuleTrigger::OnTick);

        ctx.current_hour = 23;
        assert!(evaluate_condition(&cond, &ctx, None));

        ctx.current_hour = 3;
        assert!(evaluate_condition(&cond, &ctx, None));

        ctx.current_hour = 14;
        assert!(!evaluate_condition(&cond, &ctx, None));
    }
}
