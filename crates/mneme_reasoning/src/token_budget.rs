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

pub struct TokenBudget {
    config: TokenBudgetConfig,
    db: Arc<SqliteMemory>,
}

impl TokenBudget {
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
    pub async fn check_budget(&self) -> BudgetStatus {
        // Check daily limit
        if let Some(daily_limit) = self.config.daily_limit {
            let daily = self.get_daily_usage().await;
            let pct = daily as f32 / daily_limit as f32;
            if pct >= 1.0 {
                return BudgetStatus::Exceeded;
            }
            if pct >= self.config.warning_threshold {
                return BudgetStatus::Warning { usage_pct: pct };
            }
        }

        // Check monthly limit
        if let Some(monthly_limit) = self.config.monthly_limit {
            let monthly = self.get_monthly_usage().await;
            let pct = monthly as f32 / monthly_limit as f32;
            if pct >= 1.0 {
                return BudgetStatus::Exceeded;
            }
            if pct >= self.config.warning_threshold {
                return BudgetStatus::Warning { usage_pct: pct };
            }
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
