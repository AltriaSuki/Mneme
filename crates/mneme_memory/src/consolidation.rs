//! Sleep Consolidation - Offline learning during system idle
//!
//! Inspired by biological sleep's role in memory consolidation:
//! - Transfers important patterns from feedback buffer to long-term state
//! - Weaves recent episodes into narrative chapters
//! - Self-reflection: extracts self-knowledge from patterns and episodes
//! - Updates slow state (values, rigidity) based on accumulated evidence
//!
//! This ensures System 1 learns gradually and stably, not from momentary noise.

use anyhow::Result;
use chrono::{DateTime, Utc, Timelike};
use std::sync::Arc;
use tokio::sync::RwLock;

use mneme_core::OrganismState;
use crate::feedback_buffer::{FeedbackBuffer, ConsolidatedPattern, ConsolidationProcessor, SignalType, StateUpdates};
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
    #[tracing::instrument(skip(self, episodes, current_state))]
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

        // 4. Self-reflection: extract self-knowledge from patterns + episodes
        let self_reflections = SelfReflector::reflect(&patterns, episodes);
        tracing::debug!("Self-reflection produced {} candidates", self_reflections.len());

        // 5. Update last consolidation time
        {
            let mut last = self.last_consolidation.write().await;
            *last = Some(Utc::now());
        }

        tracing::info!(
            "Sleep consolidation complete: {} state updates, chapter={}, crisis={}, reflections={}",
            if state_updates.is_empty() { "no" } else { "has" },
            chapter.is_some(),
            crisis.is_some(),
            self_reflections.len(),
        );

        Ok(ConsolidationResult {
            performed: true,
            state_updates,
            new_chapter: chapter,
            crisis,
            self_reflections,
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
    
    /// Self-knowledge candidates from reflection
    pub self_reflections: Vec<SelfKnowledgeCandidate>,

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
            self_reflections: Vec::new(),
            skip_reason: Some(reason.to_string()),
        }
    }
}

// =============================================================================
// Self-Reflection (ADR-002 Phase 2, #39)
// =============================================================================

/// A candidate self-knowledge entry produced by reflection.
#[derive(Debug, Clone)]
pub struct SelfKnowledgeCandidate {
    /// Domain category (e.g. "preference", "emotion_pattern", "relationship")
    pub domain: String,
    /// The self-knowledge statement
    pub content: String,
    /// Confidence in this insight (0.0 - 1.0)
    pub confidence: f32,
}

/// Rule-based self-reflector for sleep consolidation.
///
/// Analyzes consolidated patterns and episode digests to extract
/// self-knowledge candidates. This is the Phase 2 implementation;
/// Phase 3+ will upgrade to LLM-based reflection.
pub struct SelfReflector;

impl SelfReflector {
    /// Reflect on consolidated patterns and episodes, producing self-knowledge candidates.
    pub fn reflect(
        patterns: &[ConsolidatedPattern],
        episodes: &[EpisodeDigest],
    ) -> Vec<SelfKnowledgeCandidate> {
        let mut candidates = Vec::new();

        // 1. Extract insights from consolidated feedback patterns
        candidates.extend(Self::reflect_on_patterns(patterns));

        // 2. Extract emotional patterns from episodes
        candidates.extend(Self::reflect_on_episodes(episodes));

        candidates
    }

    /// Extract self-knowledge from consolidated feedback patterns.
    fn reflect_on_patterns(patterns: &[ConsolidatedPattern]) -> Vec<SelfKnowledgeCandidate> {
        let mut candidates = Vec::new();

        for pattern in patterns {
            // Only reflect on patterns with enough evidence
            if pattern.count < 2 {
                continue;
            }

            let confidence = (pattern.avg_confidence * 0.6 + (pattern.count as f32 / 10.0).min(1.0) * 0.4)
                .clamp(0.2, 0.9);

            match pattern.signal_type {
                SignalType::UserEmotionalFeedback => {
                    if pattern.avg_valence > 0.3 {
                        candidates.push(SelfKnowledgeCandidate {
                            domain: "relationship".to_string(),
                            content: "和用户的互动整体是积极的".to_string(),
                            confidence,
                        });
                    } else if pattern.avg_valence < -0.3 {
                        candidates.push(SelfKnowledgeCandidate {
                            domain: "relationship".to_string(),
                            content: "最近的互动中用户似乎不太满意".to_string(),
                            confidence,
                        });
                    }
                }
                SignalType::SelfReflection => {
                    candidates.push(SelfKnowledgeCandidate {
                        domain: "personality".to_string(),
                        content: "我会在互动后反思自己的表现".to_string(),
                        confidence: confidence.min(0.7),
                    });
                }
                SignalType::PredictionError => {
                    if pattern.count >= 3 {
                        candidates.push(SelfKnowledgeCandidate {
                            domain: "cognition".to_string(),
                            content: "我经常对对话的走向感到意外，说明我还在学习理解他人".to_string(),
                            confidence: confidence.min(0.6),
                        });
                    }
                }
                SignalType::ValueJudgment { .. } => {
                    candidates.push(SelfKnowledgeCandidate {
                        domain: "belief".to_string(),
                        content: "我在互动中会做出价值判断".to_string(),
                        confidence: confidence.min(0.5),
                    });
                }
                _ => {}
            }
        }

        candidates
    }

    /// Extract emotional patterns from episode digests.
    fn reflect_on_episodes(episodes: &[EpisodeDigest]) -> Vec<SelfKnowledgeCandidate> {
        let mut candidates = Vec::new();

        if episodes.is_empty() {
            return candidates;
        }

        // Compute overall emotional statistics
        let total = episodes.len() as f32;
        let avg_valence: f32 = episodes.iter().map(|e| e.emotional_valence).sum::<f32>() / total;
        let positive_ratio = episodes.iter().filter(|e| e.emotional_valence > 0.2).count() as f32 / total;
        let negative_ratio = episodes.iter().filter(|e| e.emotional_valence < -0.2).count() as f32 / total;

        // Overall emotional tendency
        if avg_valence > 0.2 && total >= 5.0 {
            candidates.push(SelfKnowledgeCandidate {
                domain: "emotion_pattern".to_string(),
                content: "最近的经历整体让我感觉积极".to_string(),
                confidence: (positive_ratio * 0.8).clamp(0.3, 0.8),
            });
        } else if avg_valence < -0.2 && total >= 5.0 {
            candidates.push(SelfKnowledgeCandidate {
                domain: "emotion_pattern".to_string(),
                content: "最近的经历让我感觉有些低落".to_string(),
                confidence: (negative_ratio * 0.8).clamp(0.3, 0.8),
            });
        }

        // Interaction volume insight
        if total >= 20.0 {
            candidates.push(SelfKnowledgeCandidate {
                domain: "relationship".to_string(),
                content: "最近和用户交流很频繁".to_string(),
                confidence: 0.7,
            });
        } else if total <= 3.0 {
            candidates.push(SelfKnowledgeCandidate {
                domain: "relationship".to_string(),
                content: "最近和用户交流不多".to_string(),
                confidence: 0.5,
            });
        }

        candidates
    }

    /// Format reflection results into a summary string for meta-episode storage.
    pub fn format_reflection_summary(candidates: &[SelfKnowledgeCandidate]) -> String {
        if candidates.is_empty() {
            return "睡眠反思：没有新的自我认知".to_string();
        }

        let mut summary = format!("睡眠反思：发现 {} 条自我认知\n", candidates.len());
        for c in candidates {
            summary.push_str(&format!(
                "- [{}] {} (确信度: {:.0}%)\n",
                c.domain, c.content, c.confidence * 100.0
            ));
        }
        summary
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

    // --- SelfReflector tests ---

    fn make_pattern(signal_type: SignalType, count: u32, avg_confidence: f32, avg_valence: f32) -> ConsolidatedPattern {
        ConsolidatedPattern {
            signal_type,
            count,
            avg_confidence,
            avg_valence,
            representative_content: "test pattern".to_string(),
            first_seen: Utc::now(),
            last_seen: Utc::now(),
        }
    }

    fn make_episode(valence: f32) -> EpisodeDigest {
        EpisodeDigest {
            timestamp: Utc::now(),
            author: "User".to_string(),
            content: "test".to_string(),
            emotional_valence: valence,
        }
    }

    #[test]
    fn test_self_reflector_empty_input() {
        let result = SelfReflector::reflect(&[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_self_reflector_positive_feedback_pattern() {
        let patterns = vec![
            make_pattern(SignalType::UserEmotionalFeedback, 5, 0.7, 0.6),
        ];
        let result = SelfReflector::reflect(&patterns, &[]);
        assert!(!result.is_empty());
        assert!(result.iter().any(|c| c.domain == "relationship"));
        assert!(result.iter().any(|c| c.content.contains("积极")));
    }

    #[test]
    fn test_self_reflector_negative_feedback_pattern() {
        let patterns = vec![
            make_pattern(SignalType::UserEmotionalFeedback, 3, 0.6, -0.5),
        ];
        let result = SelfReflector::reflect(&patterns, &[]);
        assert!(result.iter().any(|c| c.content.contains("不太满意")));
    }

    #[test]
    fn test_self_reflector_skips_low_count() {
        let patterns = vec![
            make_pattern(SignalType::UserEmotionalFeedback, 1, 0.9, 0.8),
        ];
        let result = SelfReflector::reflect(&patterns, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_self_reflector_episode_emotional_pattern() {
        // 10 positive episodes
        let episodes: Vec<_> = (0..10).map(|_| make_episode(0.5)).collect();
        let result = SelfReflector::reflect(&[], &episodes);
        assert!(result.iter().any(|c| c.domain == "emotion_pattern"));
        assert!(result.iter().any(|c| c.content.contains("积极")));
    }

    #[test]
    fn test_self_reflector_high_volume_episodes() {
        let episodes: Vec<_> = (0..25).map(|_| make_episode(0.1)).collect();
        let result = SelfReflector::reflect(&[], &episodes);
        assert!(result.iter().any(|c| c.content.contains("频繁")));
    }

    #[test]
    fn test_self_reflector_format_summary() {
        let candidates = vec![
            SelfKnowledgeCandidate {
                domain: "personality".to_string(),
                content: "我很好奇".to_string(),
                confidence: 0.7,
            },
        ];
        let summary = SelfReflector::format_reflection_summary(&candidates);
        assert!(summary.contains("1 条"));
        assert!(summary.contains("personality"));
        assert!(summary.contains("70%"));
    }

    #[test]
    fn test_self_reflector_format_summary_empty() {
        let summary = SelfReflector::format_reflection_summary(&[]);
        assert!(summary.contains("没有新的"));
    }
}
