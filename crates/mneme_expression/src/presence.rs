//! Presence scheduling for proactive messaging
//!
//! Filters triggers based on appropriate timing (active hours, days of week).

use chrono::{Datelike, Local, NaiveTime, Weekday};
use mneme_core::Trigger;

/// Presence scheduler that filters triggers by appropriate timing
pub struct PresenceScheduler {
    /// Start of active hours (e.g., 08:00)
    pub active_start: NaiveTime,
    /// End of active hours (e.g., 23:00)
    pub active_end: NaiveTime,
    /// Days when proactive messaging is allowed
    pub active_days: Vec<Weekday>,
}

impl PresenceScheduler {
    /// Create a default scheduler (8:00-23:00, all days)
    pub fn new() -> Self {
        Self {
            active_start: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            active_end: NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
            active_days: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
                Weekday::Sat,
                Weekday::Sun,
            ],
        }
    }
    
    /// Create a scheduler with custom hours
    pub fn with_hours(start_hour: u32, end_hour: u32) -> Self {
        Self {
            active_start: NaiveTime::from_hms_opt(start_hour, 0, 0).unwrap(),
            active_end: NaiveTime::from_hms_opt(end_hour, 0, 0).unwrap(),
            ..Self::new()
        }
    }
    
    /// Check if the current time is within active hours
    pub fn is_appropriate_time(&self) -> bool {
        let now = Local::now();
        let current_time = now.time();
        let current_day = now.weekday();
        
        // Check day of week
        if !self.active_days.contains(&current_day) {
            return false;
        }
        
        // Check time of day (handles overnight ranges like 22:00-06:00)
        if self.active_start <= self.active_end {
            // Normal range (e.g., 08:00-23:00)
            current_time >= self.active_start && current_time <= self.active_end
        } else {
            // Overnight range (e.g., 22:00-06:00)
            current_time >= self.active_start || current_time <= self.active_end
        }
    }
    
    /// Filter triggers, keeping only those appropriate for current time
    pub fn filter_triggers(&self, triggers: Vec<Trigger>) -> Vec<Trigger> {
        if self.is_appropriate_time() {
            triggers
        } else {
            // Outside active hours, suppress all proactive triggers
            Vec::new()
        }
    }
}

impl Default for PresenceScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_scheduler_creation() {
        let scheduler = PresenceScheduler::default();
        assert_eq!(scheduler.active_start, NaiveTime::from_hms_opt(8, 0, 0).unwrap());
        assert_eq!(scheduler.active_end, NaiveTime::from_hms_opt(23, 0, 0).unwrap());
        assert_eq!(scheduler.active_days.len(), 7);
    }
    
    #[test]
    fn test_custom_hours() {
        let scheduler = PresenceScheduler::with_hours(9, 21);
        assert_eq!(scheduler.active_start, NaiveTime::from_hms_opt(9, 0, 0).unwrap());
        assert_eq!(scheduler.active_end, NaiveTime::from_hms_opt(21, 0, 0).unwrap());
    }
    
    #[test]
    fn test_empty_triggers_on_filter() {
        let scheduler = PresenceScheduler::new();
        let empty: Vec<Trigger> = Vec::new();
        let result = scheduler.filter_triggers(empty);
        assert!(result.is_empty());
    }
}
