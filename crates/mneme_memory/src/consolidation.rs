//! Sleep Consolidation - Offline learning during system idle
//!
//! Inspired by biological sleep's role in memory consolidation:
//! - Transfers important patterns from feedback buffer to long-term state
//! - Weaves recent episodes into narrative chapters
//! - Updates slow state (values, rigidity) based on accumulated evidence
//!
//! This ensures System 1 learns gradually and stably, not from momentary noise.

use anyhow::Result;
use chrono::{DateTime, Utc, Timelike};
use std::sync::Arc;
use tokio::sync::RwLock;

use mneme_core::OrganismState;
use crate::feedback_buffer::{FeedbackBuffer, ConsolidationProcessor, StateUpdates};
use crate::narrative::{NarrativeWeaver, NarrativeChapter, EpisodeDigest, CrisisEvent};

/// Configuration for sleep consolidation
#[derive(Debug, Clone)]
pub struct SleepConfig {
    /// Hours when sleep consolidation can occur (e.g., 2-6 AM)
    pub sleep_start_hour: u32,
    pub sleep_end_hour: u32,
    
    /// Minimum interval between consolidation runs (in hours)
    pub min_consolidation_interval_hours: u32,
    
    /// Whether to allow manual consolidation trigger
    pub allow_manual_trigger: bool,
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            sleep_start_hour: 2,
            sleep_end_hour: 6,
            min_consolidation_interval_hours: 20,
            allow_manual_trigger: true,
        }
    }
}

/// The sleep consolidation system
pub struct SleepConsolidator {
    config: SleepConfig,
    feedback_buffer: Arc<RwLock<FeedbackBuffer>>,
    narrative_weaver: NarrativeWeaver,
    last_consolidation: RwLock<Option<DateTime<Utc>>>,
    next_chapter_id: RwLock<i64>,
}

impl SleepConsolidator {
    pub fn new(feedback_buffer: Arc<RwLock<FeedbackBuffer>>) -> Self {
        Self {
            config: SleepConfig::default(),
            feedback_buffer,
            narrative_weaver: NarrativeWeaver::new(),
            last_consolidation: RwLock::new(None),
            next_chapter_id: RwLock::new(1),
        }
    }

    pub fn with_config(feedback_buffer: Arc<RwLock<FeedbackBuffer>>, config: SleepConfig) -> Self {
        Self {
            config,
            feedback_buffer,
            narrative_weaver: NarrativeWeaver::new(),
            last_consolidation: RwLock::new(None),
            next_chapter_id: RwLock::new(1),
        }
    }

    /// Check if it's currently sleep time
    pub fn is_sleep_time(&self) -> bool {
        let hour = Utc::now().hour();
        hour >= self.config.sleep_start_hour && hour < self.config.sleep_end_hour
    }

    /// Check if consolidation is due
    pub async fn is_consolidation_due(&self) -> bool {
        let last = self.last_consolidation.read().await;
        
        if let Some(last_time) = *last {
            let hours_since = (Utc::now() - last_time).num_hours();
            hours_since >= self.config.min_consolidation_interval_hours as i64
        } else {
            true // Never consolidated before
        }
    }

    /// Perform sleep consolidation
    /// 
    /// This should be called during system idle time. It:
    /// 1. Consolidates feedback buffer patterns
    /// 2. Weaves narrative chapters
    /// 3. Returns state updates to apply
    pub async fn consolidate(
        &self,
        episodes: &[EpisodeDigest],
        current_state: &OrganismState,
    ) -> Result<ConsolidationResult> {
        // Check if we should consolidate
        if !self.is_sleep_time() && !self.config.allow_manual_trigger {
            return Ok(ConsolidationResult::skipped("Not sleep time"));
        }

        if !self.is_consolidation_due().await {
            return Ok(ConsolidationResult::skipped("Too soon since last consolidation"));
        }

        tracing::info!("Starting sleep consolidation...");

        // 1. Consolidate feedback buffer
        let patterns = {
            let mut buffer = self.feedback_buffer.write().await;
            buffer.consolidate()
        };
        
        let state_updates = ConsolidationProcessor::compute_state_updates(&patterns);
        tracing::debug!("Feedback consolidation produced {} patterns", patterns.len());

        // 2. Weave narrative chapter if enough episodes
        let chapter = if episodes.len() >= 10 {
            let mut chapter_id = self.next_chapter_id.write().await;
            let chapter = self.narrative_weaver.weave_chapter(episodes, *chapter_id);
            if chapter.is_some() {
                *chapter_id += 1;
            }
            chapter
        } else {
            None
        };

        // 3. Detect narrative crisis
        let crisis = self.narrative_weaver.detect_crisis(
            episodes,
            current_state.slow.narrative_bias,
        );

        // 4. Update last consolidation time
        {
            let mut last = self.last_consolidation.write().await;
            *last = Some(Utc::now());
        }

        tracing::info!(
            "Sleep consolidation complete: {} state updates, chapter={}, crisis={}",
            if state_updates.is_empty() { "no" } else { "has" },
            chapter.is_some(),
            crisis.is_some(),
        );

        Ok(ConsolidationResult {
            performed: true,
            state_updates,
            new_chapter: chapter,
            crisis,
            skip_reason: None,
        })
    }

    /// Apply state updates to the organism state
    pub fn apply_updates(state: &mut OrganismState, updates: &StateUpdates) {
        // Apply medium state updates
        state.medium.attachment.anxiety = 
            (state.medium.attachment.anxiety + updates.attachment_anxiety_delta).clamp(0.0, 1.0);
        state.medium.openness = 
            (state.medium.openness + updates.openness_delta).clamp(0.0, 1.0);
        
        // Curiosity is in fast state but we can adjust its baseline recovery target
        // (This would require dynamics modification - for now, just log)
        if updates.curiosity_delta.abs() > 0.01 {
            tracing::debug!("Curiosity delta: {:.3} (not directly applied)", updates.curiosity_delta);
        }

        // Apply slow state updates
        state.slow.narrative_bias = 
            (state.slow.narrative_bias + updates.narrative_bias_delta).clamp(-1.0, 1.0);

        // Apply value reinforcements
        for (value_name, delta) in &updates.value_reinforcements {
            if let Some(entry) = state.slow.values.values.get_mut(value_name) {
                // Reinforce both weight and rigidity
                entry.weight = (entry.weight + delta).clamp(0.0, 1.0);
                entry.rigidity = (entry.rigidity + delta * 0.5).clamp(0.0, 1.0);
                tracing::debug!("Value '{}' reinforced: weight={:.3}, rigidity={:.3}", 
                    value_name, entry.weight, entry.rigidity);
            }
        }
    }

    /// Handle a narrative crisis - may trigger slow state restructuring
    pub fn handle_crisis(state: &mut OrganismState, crisis: &CrisisEvent, dynamics: &mneme_core::DefaultDynamics) -> bool {
        tracing::warn!("Handling narrative crisis: {}", crisis.description);
        
        // Use dynamics to potentially trigger narrative collapse
        dynamics.step_slow_crisis(&mut state.slow, &state.medium, crisis.intensity)
    }
}

/// Result of a consolidation attempt
#[derive(Debug)]
pub struct ConsolidationResult {
    /// Whether consolidation was performed
    pub performed: bool,
    
    /// State updates to apply
    pub state_updates: StateUpdates,
    
    /// New narrative chapter (if created)
    pub new_chapter: Option<NarrativeChapter>,
    
    /// Crisis detected (if any)
    pub crisis: Option<CrisisEvent>,
    
    /// Reason for skipping (if not performed)
    pub skip_reason: Option<String>,
}

impl ConsolidationResult {
    fn skipped(reason: &str) -> Self {
        Self {
            performed: false,
            state_updates: StateUpdates::default(),
            new_chapter: None,
            crisis: None,
            skip_reason: Some(reason.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_consolidation_timing() {
        let buffer = Arc::new(RwLock::new(FeedbackBuffer::new()));
        let mut config = SleepConfig::default();
        config.allow_manual_trigger = true; // Allow testing
        
        let consolidator = SleepConsolidator::with_config(buffer, config);
        
        // Should be due since never consolidated
        assert!(consolidator.is_consolidation_due().await);
    }

    #[test]
    fn test_apply_updates() {
        let mut state = OrganismState::default();
        let initial_anxiety = state.medium.attachment.anxiety;
        
        let updates = StateUpdates {
            attachment_anxiety_delta: -0.1,
            openness_delta: 0.05,
            curiosity_delta: 0.0,
            narrative_bias_delta: 0.02,
            value_reinforcements: vec![("honesty".to_string(), 0.05)],
        };

        SleepConsolidator::apply_updates(&mut state, &updates);

        assert!(state.medium.attachment.anxiety < initial_anxiety);
        assert!(state.slow.values.values.get("honesty").unwrap().weight > 0.8);
    }
}
