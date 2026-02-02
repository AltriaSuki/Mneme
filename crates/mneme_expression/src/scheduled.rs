//! Scheduled trigger evaluator
//!
//! Evaluates time-based triggers for proactive messaging.

use async_trait::async_trait;
use chrono::{Local, NaiveTime, Timelike};
use mneme_core::{Trigger, TriggerEvaluator};
use anyhow::Result;

/// Entry in the schedule
#[derive(Debug, Clone)]
pub struct ScheduleEntry {
    /// Name of this scheduled event
    pub name: String,
    /// Hour to trigger (0-23)
    pub hour: u32,
    /// Minute to trigger (0-59)
    pub minute: u32,
    /// Tolerance window in minutes (triggers if within this window)
    pub tolerance_minutes: u32,
}

impl ScheduleEntry {
    /// Create a new schedule entry
    pub fn new(name: &str, hour: u32, minute: u32) -> Self {
        Self {
            name: name.to_string(),
            hour,
            minute,
            tolerance_minutes: 5, // Default 5 minute window
        }
    }
    
    /// Check if current time matches this entry
    pub fn matches_now(&self) -> bool {
        let now = Local::now();
        let current_time = now.time();
        
        let target = NaiveTime::from_hms_opt(self.hour, self.minute, 0).unwrap();
        let diff_seconds = (current_time.num_seconds_from_midnight() as i32
            - target.num_seconds_from_midnight() as i32)
            .abs();
        let tolerance_seconds = (self.tolerance_minutes * 60) as i32;
        
        diff_seconds <= tolerance_seconds
    }
}

/// Evaluator for scheduled time-based triggers
pub struct ScheduledTriggerEvaluator {
    schedules: Vec<ScheduleEntry>,
    /// Track which schedules have fired recently to avoid duplicates
    last_fired: std::collections::HashMap<String, i64>,
}

impl ScheduledTriggerEvaluator {
    /// Create a new evaluator with default schedules (morning/evening)
    pub fn new() -> Self {
        Self {
            schedules: vec![
                ScheduleEntry::new("morning_greeting", 8, 0),
                ScheduleEntry::new("evening_summary", 21, 0),
            ],
            last_fired: std::collections::HashMap::new(),
        }
    }
    
    /// Create with custom schedules
    pub fn with_schedules(schedules: Vec<ScheduleEntry>) -> Self {
        Self {
            schedules,
            last_fired: std::collections::HashMap::new(),
        }
    }
    
    /// Add a schedule entry
    pub fn add_schedule(&mut self, entry: ScheduleEntry) {
        self.schedules.push(entry);
    }
}

impl Default for ScheduledTriggerEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TriggerEvaluator for ScheduledTriggerEvaluator {
    async fn evaluate(&self) -> Result<Vec<Trigger>> {
        let now = Local::now().timestamp();
        let mut triggers = Vec::new();
        
        for entry in &self.schedules {
            // Skip if already fired within the last hour
            if let Some(&last) = self.last_fired.get(&entry.name) {
                if now - last < 3600 {
                    continue;
                }
            }
            
            if entry.matches_now() {
                triggers.push(Trigger::Scheduled {
                    name: entry.name.clone(),
                    schedule: format!("{}:{:02}", entry.hour, entry.minute),
                });
            }
        }
        
        Ok(triggers)
    }
    
    fn name(&self) -> &'static str {
        "ScheduledTriggerEvaluator"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_schedule_entry_creation() {
        let entry = ScheduleEntry::new("test", 9, 30);
        assert_eq!(entry.name, "test");
        assert_eq!(entry.hour, 9);
        assert_eq!(entry.minute, 30);
        assert_eq!(entry.tolerance_minutes, 5);
    }
    
    #[test]
    fn test_default_evaluator_has_schedules() {
        let evaluator = ScheduledTriggerEvaluator::new();
        assert_eq!(evaluator.schedules.len(), 2);
    }
}
