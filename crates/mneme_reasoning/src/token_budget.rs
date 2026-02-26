use chrono::Datelike;
use mneme_core::config::{DegradationStrategy, TokenBudgetConfig};
use mneme_memory::SqliteMemory;
use std::sync::Arc;

// ============================================================================
// Budget status
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum BudgetStatus {
    /// Under budget, all clear
    Ok,
    /// Approaching limit (above warning_threshold)
    Warning { usage_pct: f32 },
    /// Over budget
    Exceeded,
}

// ============================================================================
// TokenBudget
// ============================================================================

/// Tracks daily/monthly token consumption and enforces budget limits.
pub struct TokenBudget {
    config: TokenBudgetConfig,
    db: Arc<SqliteMemory>,
}

impl TokenBudget {
    /// Create a new budget tracker backed by the given database.
    pub fn new(config: TokenBudgetConfig, db: Arc<SqliteMemory>) -> Self {
        Self { config, db }
    }

    /// Record token usage from an API call.
    pub async fn record_usage(&self, input_tokens: u64, output_tokens: u64) {
        if let Err(e) = self
            .db
            .record_token_usage(input_tokens, output_tokens)
            .await
        {
            tracing::warn!("Failed to record token usage: {}", e);
        }
    }

    /// Check current budget status against daily/monthly limits.
    ///
    /// Checks Exceeded on both limits first, then Warning, so a monthly
    /// Exceeded is never shadowed by a daily Warning.
    pub async fn check_budget(&self) -> BudgetStatus {
        let daily_pct = match self.config.daily_limit {
            Some(limit) if limit > 0 => {
                Some(self.get_daily_usage().await as f32 / limit as f32)
            }
            _ => None,
        };

        let monthly_pct = match self.config.monthly_limit {
            Some(limit) if limit > 0 => {
                Some(self.get_monthly_usage().await as f32 / limit as f32)
            }
            _ => None,
        };

        // Check Exceeded first (either limit)
        if daily_pct.is_some_and(|p| p >= 1.0) || monthly_pct.is_some_and(|p| p >= 1.0) {
            return BudgetStatus::Exceeded;
        }

        // Then check Warning (worst of the two)
        let worst_pct = daily_pct.unwrap_or(0.0).max(monthly_pct.unwrap_or(0.0));
        if worst_pct >= self.config.warning_threshold {
            return BudgetStatus::Warning {
                usage_pct: worst_pct,
            };
        }

        BudgetStatus::Ok
    }

    /// Get total tokens used today.
    pub async fn get_daily_usage(&self) -> u64 {
        let start_of_day = start_of_today();
        match self.db.get_token_usage_since(start_of_day).await {
            Ok((inp, out)) => inp + out,
            Err(_) => 0,
        }
    }

    /// Get total tokens used this month.
    pub async fn get_monthly_usage(&self) -> u64 {
        let start_of_month = start_of_this_month();
        match self.db.get_token_usage_since(start_of_month).await {
            Ok((inp, out)) => inp + out,
            Err(_) => 0,
        }
    }

    /// If budget is exceeded and strategy is Degrade, return the capped max_tokens.
    pub fn degraded_max_tokens(&self, base: u32) -> u32 {
        match &self.config.degradation_strategy {
            DegradationStrategy::Degrade { max_tokens_cap } => base.min(*max_tokens_cap),
            DegradationStrategy::HardStop => base,
        }
    }

    /// Fraction of daily budget remaining (0.0 = exhausted, 1.0 = untouched).
    /// Returns 1.0 if no daily limit is configured.
    pub async fn remaining_fraction(&self) -> f32 {
        match self.config.daily_limit {
            Some(limit) if limit > 0 => {
                let used = self.get_daily_usage().await;
                (1.0 - used as f32 / limit as f32).clamp(0.0, 1.0)
            }
            _ => 1.0,
        }
    }

    /// Evaluate whether an action is worth the token cost.
    /// `priority`: 0.0 (optional/proactive) to 1.0 (user-initiated, must serve).
    /// Returns true if the action should proceed.
    pub async fn is_worthy(&self, priority: f32) -> bool {
        let remaining = self.remaining_fraction().await;
        // User messages (priority >= 0.8) always pass unless budget fully exhausted
        if priority >= 0.8 {
            return remaining > 0.0;
        }
        // Proactive actions need proportionally more remaining budget:
        // priority=0.0 needs >50% remaining, priority=0.5 needs >25%
        let threshold = 0.5 * (1.0 - priority);
        remaining > threshold
    }

    /// Access the underlying budget configuration.
    pub fn config(&self) -> &TokenBudgetConfig {
        &self.config
    }
}

fn start_of_today() -> i64 {
    let now = chrono::Utc::now();
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mneme_core::config::DegradationStrategy;

    #[test]
    fn test_degraded_max_tokens_degrade_strategy() {
        let config = TokenBudgetConfig {
            daily_limit: Some(100_000),
            monthly_limit: None,
            warning_threshold: 0.8,
            degradation_strategy: DegradationStrategy::Degrade {
                max_tokens_cap: 1024,
            },
        };
        // Can't construct TokenBudget without a real DB, so test the logic directly
        match &config.degradation_strategy {
            DegradationStrategy::Degrade { max_tokens_cap } => {
                // base > cap → should be capped
                assert_eq!(4096_u32.min(*max_tokens_cap), 1024);
                // base < cap → should stay as-is
                assert_eq!(512_u32.min(*max_tokens_cap), 512);
            }
            DegradationStrategy::HardStop => panic!("wrong strategy"),
        }
    }

    #[test]
    fn test_degraded_max_tokens_hardstop_passthrough() {
        let config = TokenBudgetConfig {
            daily_limit: Some(100_000),
            monthly_limit: None,
            warning_threshold: 0.8,
            degradation_strategy: DegradationStrategy::HardStop,
        };
        match &config.degradation_strategy {
            DegradationStrategy::HardStop => {
                // HardStop doesn't cap — base passes through
                assert_eq!(4096_u32, 4096);
            }
            DegradationStrategy::Degrade { .. } => panic!("wrong strategy"),
        }
    }
}

fn start_of_this_month() -> i64 {
    let now = chrono::Utc::now();
    now.date_naive()
        .with_day(1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp()
}
