use anyhow::Result;
use async_trait::async_trait;
use chrono::{Local, Timelike};
use mneme_core::config::ScheduleEntryConfig;
use mneme_core::{Trigger, TriggerEvaluator};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
    /// Optional output route
    pub route: Option<String>,
}

impl ScheduleEntry {
    /// Create a new schedule entry
    pub fn new(name: &str, hour: u32, minute: u32) -> Result<Self> {
        if hour >= 24 {
            anyhow::bail!("Invalid hour: {}", hour);
        }
        if minute >= 60 {
            anyhow::bail!("Invalid minute: {}", minute);
        }

        Ok(Self {
            name: name.to_string(),
            hour,
            minute,
            tolerance_minutes: 5,
            route: None,
        })
    }

    /// Create from config entry
    pub fn from_config(cfg: &ScheduleEntryConfig) -> Result<Self> {
        if cfg.hour >= 24 {
            anyhow::bail!("Invalid hour: {}", cfg.hour);
        }
        if cfg.minute >= 60 {
            anyhow::bail!("Invalid minute: {}", cfg.minute);
        }
        Ok(Self {
            name: cfg.name.clone(),
            hour: cfg.hour,
            minute: cfg.minute,
            tolerance_minutes: cfg.tolerance_minutes,
            route: cfg.route.clone(),
        })
    }

    /// Check if specific time matches this entry (for testing)
    pub fn matches_at(&self, time: &impl Timelike) -> bool {
        let current_seconds = time.num_seconds_from_midnight();
        let target_seconds = (self.hour * 3600 + self.minute * 60) as i32;

        let diff_seconds = (current_seconds as i32 - target_seconds).abs();
        let tolerance_seconds = (self.tolerance_minutes * 60) as i32;

        diff_seconds <= tolerance_seconds
    }
}

/// Handle for hot-reloading schedules from outside the evaluator.
#[derive(Clone)]
pub struct ScheduleHandle(Arc<Mutex<Vec<ScheduleEntry>>>);

impl ScheduleHandle {
    /// Replace schedules with new config. Falls back to defaults if empty.
    pub fn reload(&self, configs: &[ScheduleEntryConfig]) {
        let new = if configs.is_empty() {
            vec![
                ScheduleEntry::new("morning_greeting", 8, 0).unwrap(),
                ScheduleEntry::new("evening_summary", 21, 0).unwrap(),
            ]
        } else {
            configs
                .iter()
                .filter_map(|c| ScheduleEntry::from_config(c).ok())
                .collect()
        };
        *self.0.lock().unwrap() = new;
    }

    /// List all current schedules as formatted text.
    pub fn list(&self) -> String {
        let schedules = self.0.lock().unwrap();
        if schedules.is_empty() {
            return "No schedules configured.".to_string();
        }
        schedules
            .iter()
            .map(|e| {
                let route = e.route.as_deref().unwrap_or("cli");
                format!(
                    "- {} @ {:02}:{:02} (±{}min) → {}",
                    e.name, e.hour, e.minute, e.tolerance_minutes, route
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Add a new schedule entry. Returns error if name already exists.
    pub fn add(&self, entry: ScheduleEntry) -> Result<(), String> {
        let mut schedules = self.0.lock().unwrap();
        if schedules.iter().any(|e| e.name == entry.name) {
            return Err(format!("Schedule '{}' already exists", entry.name));
        }
        schedules.push(entry);
        Ok(())
    }

    /// Remove a schedule by name. Returns true if found and removed.
    pub fn remove(&self, name: &str) -> bool {
        let mut schedules = self.0.lock().unwrap();
        let before = schedules.len();
        schedules.retain(|e| e.name != name);
        schedules.len() < before
    }
}

/// Evaluator for scheduled time-based triggers
pub struct ScheduledTriggerEvaluator {
    schedules: Arc<Mutex<Vec<ScheduleEntry>>>,
    /// Track which schedules have fired recently to avoid duplicates
    last_fired: Mutex<HashMap<String, i64>>,
}

impl ScheduledTriggerEvaluator {
    /// Create a new evaluator with default schedules (morning/evening)
    pub fn new() -> Self {
        Self {
            schedules: Arc::new(Mutex::new(vec![
                ScheduleEntry::new("morning_greeting", 8, 0).unwrap(),
                ScheduleEntry::new("evening_summary", 21, 0).unwrap(),
            ])),
            last_fired: Mutex::new(HashMap::new()),
        }
    }

    /// Create from config entries. Falls back to defaults if empty.
    pub fn from_config(configs: &[ScheduleEntryConfig]) -> Self {
        if configs.is_empty() {
            return Self::new();
        }
        let schedules: Vec<ScheduleEntry> = configs
            .iter()
            .filter_map(|c| match ScheduleEntry::from_config(c) {
                Ok(e) => Some(e),
                Err(e) => {
                    tracing::warn!("Invalid schedule entry '{}': {}", c.name, e);
                    None
                }
            })
            .collect();
        Self {
            schedules: Arc::new(Mutex::new(schedules)),
            last_fired: Mutex::new(HashMap::new()),
        }
    }

    /// Get a handle for hot-reloading schedules.
    pub fn schedule_handle(&self) -> ScheduleHandle {
        ScheduleHandle(self.schedules.clone())
    }

    /// Create with custom schedules
    pub fn with_schedules(schedules: Vec<ScheduleEntry>) -> Self {
        Self {
            schedules: Arc::new(Mutex::new(schedules)),
            last_fired: Mutex::new(HashMap::new()),
        }
    }

    /// Helper to evaluate at a specific time (for testing)
    pub fn evaluate_at(
        &self,
        now_timestamp: i64,
        time_struct: impl Timelike,
    ) -> Result<Vec<Trigger>> {
        let mut triggers = Vec::new();
        let mut last_fired = self.last_fired.lock().unwrap();
        let schedules = self.schedules.lock().unwrap();

        for entry in schedules.iter() {
            // Skip if already fired within the last hour
            if let Some(&last) = last_fired.get(&entry.name) {
                if now_timestamp - last < 3600 {
                    continue;
                }
            }

            if entry.matches_at(&time_struct) {
                last_fired.insert(entry.name.clone(), now_timestamp);

                triggers.push(Trigger::Scheduled {
                    name: entry.name.clone(),
                    schedule: format!("{}:{:02}", entry.hour, entry.minute),
                    route: entry.route.clone(),
                });
            }
        }

        Ok(triggers)
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
        let now = Local::now();
        self.evaluate_at(now.timestamp(), now.time())
    }

    fn name(&self) -> &'static str {
        "ScheduledTriggerEvaluator"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    #[test]
    fn test_schedule_entry_validation() {
        assert!(ScheduleEntry::new("valid", 23, 59).is_ok());
        assert!(ScheduleEntry::new("invalid_hour", 24, 0).is_err());
        assert!(ScheduleEntry::new("invalid_min", 12, 60).is_err());
    }

    #[test]
    fn test_matching_logic() {
        let entry = ScheduleEntry::new("test", 9, 0).unwrap();

        // Exact match
        assert!(entry.matches_at(&NaiveTime::from_hms_opt(9, 0, 0).unwrap()));

        // Within 5 min tolerance
        assert!(entry.matches_at(&NaiveTime::from_hms_opt(9, 4, 59).unwrap()));

        // Outside tolerance
        assert!(!entry.matches_at(&NaiveTime::from_hms_opt(9, 6, 0).unwrap()));
    }

    #[test]
    fn test_deduplication() {
        let entry = ScheduleEntry::new("test", 9, 0).unwrap();
        let evaluator = ScheduledTriggerEvaluator::with_schedules(vec![entry]);
        let time = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        let now_ts = 100000;

        // First fire
        let triggers = evaluator.evaluate_at(now_ts, time).unwrap();
        assert_eq!(triggers.len(), 1);

        // Immediate re-fire (should be deduplicated)
        let triggers_again = evaluator.evaluate_at(now_ts + 60, time).unwrap();
        assert_eq!(triggers_again.len(), 0); // Should be empty

        // Fire after 1 hour (should fire again)
        let triggers_later = evaluator.evaluate_at(now_ts + 3700, time).unwrap();
        assert_eq!(triggers_later.len(), 1);
    }
}
