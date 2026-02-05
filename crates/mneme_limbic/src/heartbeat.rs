//! Heartbeat configuration for the limbic system
//!
//! The heartbeat determines how frequently the state is updated
//! even without external stimuli (homeostatic regulation).

use std::time::Duration;

/// Configuration for the limbic heartbeat
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// How often to tick the state evolution (default: 100ms)
    pub interval: Duration,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(100),
        }
    }
}

impl HeartbeatConfig {
    /// Fast heartbeat for real-time applications
    pub fn fast() -> Self {
        Self {
            interval: Duration::from_millis(50),
        }
    }

    /// Slow heartbeat for resource-constrained environments
    pub fn slow() -> Self {
        Self {
            interval: Duration::from_millis(500),
        }
    }

    /// Very slow heartbeat for testing
    pub fn testing() -> Self {
        Self {
            interval: Duration::from_millis(10),
        }
    }
}
