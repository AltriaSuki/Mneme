//! Presence scheduling for proactive messaging
//!
//! Filters triggers based on appropriate timing (active hours, days of week).

use chrono::{Datelike, Local, NaiveTime, Weekday};
use mneme_core::Trigger;
use anyhow::{Context, Result};

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
    ///
    /// # Arguments
    /// * `start_hour` - Hour to start (0-23)
    /// * `end_hour` - Hour to end (0-23)
    pub fn with_hours(start_hour: u32, end_hour: u32) -> Result<Self> {
        Ok(Self {
            active_start: NaiveTime::from_hms_opt(start_hour, 0, 0)
                .context("Invalid start hour")?,
            active_end: NaiveTime::from_hms_opt(end_hour, 0, 0)
                .context("Invalid end hour")?,
            ..Self::new()
        })
    }
    
    /// Check if the current time is within active hours
    pub fn is_appropriate_time(&self) -> bool {
        self.is_appropriate_at(Local::now())
    }

    /// Check if a specific time is within active hours (for testing)
    pub fn is_appropriate_at<T: Datelike + chrono::Timelike>(&self, time: T) -> bool {
        let current_time = NaiveTime::from_hms_opt(time.hour(), time.minute(), time.second()).unwrap();
        let current_day = time.weekday();
        
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
    use chrono::NaiveDate;

    #[test]
    fn test_custom_hours_validation() {
        assert!(PresenceScheduler::with_hours(9, 21).is_ok());
        assert!(PresenceScheduler::with_hours(24, 0).is_err());
        assert!(PresenceScheduler::with_hours(0, 24).is_err());
    }

    #[test]
    fn test_behavior_normal_range() {
        let scheduler = PresenceScheduler::with_hours(9, 17).unwrap();
        // Day is Monday (2023-01-02)
        let base_date = NaiveDate::from_ymd_opt(2023, 1, 2).unwrap();
        
        // 12:00 (inside)
        let noon = base_date.and_hms_opt(12, 0, 0).unwrap();
        assert!(scheduler.is_appropriate_at(noon));
        
        // 08:00 (outside)
        let morning = base_date.and_hms_opt(8, 0, 0).unwrap();
        assert!(!scheduler.is_appropriate_at(morning));
        
        // 18:00 (outside)
        let evening = base_date.and_hms_opt(18, 0, 0).unwrap();
        assert!(!scheduler.is_appropriate_at(evening));
    }

    #[test]
    fn test_behavior_overnight_range() {
        let scheduler = PresenceScheduler::with_hours(22, 6).unwrap();
        let base_date = NaiveDate::from_ymd_opt(2023, 1, 2).unwrap();
        
        // 23:00 (inside)
        let late_night = base_date.and_hms_opt(23, 0, 0).unwrap();
        assert!(scheduler.is_appropriate_at(late_night));
        
        // 05:00 (inside)
        let early_morning = base_date.and_hms_opt(5, 0, 0).unwrap();
        assert!(scheduler.is_appropriate_at(early_morning));
        
        // 12:00 (outside)
        let noon = base_date.and_hms_opt(12, 0, 0).unwrap();
        assert!(!scheduler.is_appropriate_at(noon));
    }
}
