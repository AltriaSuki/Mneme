//! Retry logic with exponential backoff for HTTP API calls.
//!
//! Retries on transient errors (429 rate limit, 5xx server errors, network timeouts).
//! Does NOT retry on client errors (400, 401, 403, 404).

use anyhow::Result;
use reqwest::{Response, StatusCode};
use std::time::Duration;

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of attempts (including the first).
    pub max_attempts: u32,
    /// Initial delay before the first retry.
    pub initial_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Multiplier for each subsequent delay.
    pub backoff_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
        }
    }
}

/// Determine if a status code is retryable.
fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS         // 429
        || status == StatusCode::INTERNAL_SERVER_ERROR // 500
        || status == StatusCode::BAD_GATEWAY           // 502
        || status == StatusCode::SERVICE_UNAVAILABLE   // 503
        || status == StatusCode::GATEWAY_TIMEOUT       // 504
        || status == StatusCode::REQUEST_TIMEOUT // 408
}

/// Execute an async HTTP operation with retry logic.
///
/// The `operation` closure is called repeatedly until it succeeds, returns a
/// non-retryable error, or `max_attempts` is exhausted.
///
/// Returns the successful `Response`, or the last error if all retries failed.
pub async fn with_retry<F, Fut>(
    config: &RetryConfig,
    provider_name: &str,
    operation: F,
) -> Result<Response>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Response>>,
{
    let mut delay = config.initial_delay;
    let mut last_error = None;

    for attempt in 1..=config.max_attempts {
        match operation().await {
            Ok(response) => {
                let status = response.status();

                if status.is_success() {
                    if attempt > 1 {
                        tracing::info!("{} succeeded on attempt {}", provider_name, attempt);
                    }
                    return Ok(response);
                }

                if !is_retryable_status(status) {
                    // Non-retryable error (400, 401, 403, etc.) — fail immediately
                    let error_text = response.text().await.unwrap_or_default();
                    anyhow::bail!("{} API Error ({}): {}", provider_name, status, error_text);
                }

                // Retryable error
                let error_text = response.text().await.unwrap_or_default();
                tracing::warn!(
                    "{} returned {} on attempt {}/{}: {}",
                    provider_name,
                    status,
                    attempt,
                    config.max_attempts,
                    error_text.chars().take(200).collect::<String>()
                );
                last_error = Some(format!("{} ({}): {}", provider_name, status, error_text));
            }
            Err(e) => {
                // Network error (timeout, DNS failure, connection refused)
                tracing::warn!(
                    "{} network error on attempt {}/{}: {}",
                    provider_name,
                    attempt,
                    config.max_attempts,
                    e
                );
                last_error = Some(format!("{}: {}", provider_name, e));
            }
        }

        if attempt < config.max_attempts {
            // Extract Retry-After header if available (429 responses)
            // For simplicity, use exponential backoff with jitter
            let jitter = Duration::from_millis(rand_jitter());
            let sleep_time = delay + jitter;

            tracing::info!(
                "{} retrying in {:.1}s (attempt {}/{})",
                provider_name,
                sleep_time.as_secs_f64(),
                attempt + 1,
                config.max_attempts
            );

            tokio::time::sleep(sleep_time).await;

            // Increase delay for next attempt
            delay = Duration::from_secs_f64(
                (delay.as_secs_f64() * config.backoff_factor).min(config.max_delay.as_secs_f64()),
            );
        }
    }

    anyhow::bail!(
        "All {} retry attempts exhausted. Last error: {}",
        config.max_attempts,
        last_error.unwrap_or_else(|| "unknown".to_string())
    )
}

/// Simple jitter: random 0-500ms using timestamp as poor-man's random.
fn rand_jitter() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 500) as u64
}
