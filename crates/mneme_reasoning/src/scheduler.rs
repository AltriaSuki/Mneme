//! Presence Scheduler (#11, v0.6.0)
//!
//! Dynamically adjusts tick and trigger intervals based on organism state,
//! lifecycle, active goals, and time of day.

use chrono::Timelike;
use mneme_core::OrganismState;
use mneme_memory::LifecycleState;
use std::time::Duration;

/// State-aware scheduler that computes dynamic intervals.
pub struct PresenceScheduler {
    pub base_tick: Duration,
    pub base_trigger: Duration,
}

impl PresenceScheduler {
    pub fn new(base_tick: Duration, base_trigger: Duration) -> Self {
        Self {
            base_tick,
            base_trigger,
        }
    }

    /// Compute the next tick interval based on state and lifecycle.
    pub fn next_tick_interval(&self, state: &OrganismState, lifecycle: LifecycleState) -> Duration {
        let base = self.base_tick.as_secs_f64();

        let factor = match lifecycle {
            LifecycleState::Sleeping => 10.0,
            LifecycleState::Drowsy => 3.0,
            LifecycleState::ShuttingDown => 1.0,
            LifecycleState::Degraded => 2.0,
            LifecycleState::Awake => {
                if state.fast.energy > 0.7 {
                    0.5
                } else {
                    1.0
                }
            }
        };

        let secs = (base * factor).max(1.0);
        Duration::from_secs_f64(secs)
    }

    /// Compute the next trigger evaluation interval.
    pub fn next_trigger_interval(&self, state: &OrganismState, active_goals: usize) -> Duration {
        let base = self.base_trigger.as_secs_f64();
        let mut factor = 1.0;

        // More active goals → shorter interval (more responsive)
        if active_goals > 3 {
            factor *= 0.5;
        } else if active_goals > 0 {
            factor *= 0.75;
        }

        // Night mode: 0-6h → longer interval
        let hour = chrono::Local::now().hour();
        if hour < 6 {
            factor *= 2.0;
        }

        // Low energy → longer interval
        if state.fast.energy < 0.3 {
            factor *= 1.5;
        }

        let secs = (base * factor).max(5.0);
        Duration::from_secs_f64(secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_scheduler() -> PresenceScheduler {
        PresenceScheduler::new(Duration::from_secs(10), Duration::from_secs(120))
    }

    #[test]
    fn test_scheduler_drowsy_longer_interval() {
        let s = base_scheduler();
        let state = OrganismState::default();
        let interval = s.next_tick_interval(&state, LifecycleState::Drowsy);
        // Drowsy = base * 3.0 = 30s
        assert_eq!(interval, Duration::from_secs(30));
    }

    #[test]
    fn test_scheduler_sleeping_much_longer() {
        let s = base_scheduler();
        let state = OrganismState::default();
        let interval = s.next_tick_interval(&state, LifecycleState::Sleeping);
        // Sleeping = base * 10.0 = 100s
        assert_eq!(interval, Duration::from_secs(100));
    }

    #[test]
    fn test_scheduler_high_energy_shorter() {
        let s = base_scheduler();
        let mut state = OrganismState::default();
        state.fast.energy = 0.9;
        let interval = s.next_tick_interval(&state, LifecycleState::Awake);
        // High energy = base * 0.5 = 5s
        assert_eq!(interval, Duration::from_secs(5));
    }

    #[test]
    fn test_scheduler_goals_affect_trigger() {
        let s = base_scheduler();
        let state = OrganismState::default();

        let no_goals = s.next_trigger_interval(&state, 0);
        let many_goals = s.next_trigger_interval(&state, 5);

        // More goals → shorter interval
        assert!(many_goals < no_goals);
    }
}
