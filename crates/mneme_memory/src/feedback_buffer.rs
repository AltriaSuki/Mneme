//! Feedback Buffer - Prevents cognitive bias reinforcement
//!
//! Based on temp.md's "Top-Down: Buffered Feedback" design:
//! - Uncertainty Discounting: low confidence interpretations have no effect
//! - Temporal Smoothing: only repeated patterns can modify s_medium/s_slow
//! - Sleep Consolidation: real updates happen during offline processing
//!
//! This module collects feedback signals during the day and processes them
//! during "sleep" (system idle) to prevent System 1 from being corrupted
//! by System 2's occasional hallucinations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};

/// A feedback signal from System 2 to be buffered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackSignal {
    /// Unique ID
    pub id: i64,
    
    /// Timestamp when the signal was generated
    pub timestamp: DateTime<Utc>,
    
    /// Type of feedback
    pub signal_type: SignalType,
    
    /// The interpretation or conclusion
    pub content: String,
    
    /// System 2's confidence in this interpretation (0.0 - 1.0)
    pub confidence: f32,
    
    /// Emotional valence of the context (-1.0 to 1.0)
    pub emotional_context: f32,
    
    /// Has this been processed during sleep consolidation?
    pub consolidated: bool,
}

/// Types of feedback signals
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SignalType {
    /// User expressed emotion toward agent
    UserEmotionalFeedback,
    /// Agent's interpretation of a situation
    SituationInterpretation,
    /// Value judgment made during interaction
    ValueJudgment { value: String },
    /// Self-reflection about own behavior
    SelfReflection,
    /// Prediction error (surprise) signal
    PredictionError,
}

/// Aggregated pattern from multiple similar signals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidatedPattern {
    /// Signal type being aggregated
    pub signal_type: SignalType,
    
    /// Number of similar signals seen
    pub count: u32,
    
    /// Average confidence across signals
    pub avg_confidence: f32,
    
    /// Average emotional valence
    pub avg_valence: f32,
    
    /// Representative content (most frequent or most confident)
    pub representative_content: String,
    
    /// First seen
    pub first_seen: DateTime<Utc>,
    
    /// Last seen
    pub last_seen: DateTime<Utc>,
}

/// The feedback buffer that accumulates signals during waking hours
#[derive(Debug, Clone, Default)]
pub struct FeedbackBuffer {
    /// Pending signals waiting for consolidation
    signals: Vec<FeedbackSignal>,
    
    /// Minimum confidence threshold for a signal to be considered
    confidence_threshold: f32,
    
    /// Minimum occurrences needed for a pattern to affect state
    pattern_threshold: u32,
    
    /// Next signal ID
    next_id: i64,
}

impl FeedbackBuffer {
    pub fn new() -> Self {
        Self {
            signals: Vec::new(),
            confidence_threshold: 0.6,
            pattern_threshold: 3,
            next_id: 1,
        }
    }

    /// Add a feedback signal to the buffer
    pub fn add_signal(&mut self, signal_type: SignalType, content: String, confidence: f32, emotional_context: f32) {
        // Apply uncertainty discounting immediately
        if confidence < self.confidence_threshold {
            tracing::debug!(
                "Signal discounted due to low confidence: {:.2} < {:.2}",
                confidence, self.confidence_threshold
            );
            return;
        }

        let signal = FeedbackSignal {
            id: self.next_id,
            timestamp: Utc::now(),
            signal_type,
            content,
            confidence,
            emotional_context,
            consolidated: false,
        };

        self.next_id += 1;
        self.signals.push(signal);
    }

    /// Get the number of pending signals
    pub fn pending_count(&self) -> usize {
        self.signals.iter().filter(|s| !s.consolidated).count()
    }

    /// Perform sleep consolidation - aggregate patterns and return actionable updates
    /// 
    /// This should be called during system idle time (e.g., night hours)
    pub fn consolidate(&mut self) -> Vec<ConsolidatedPattern> {
        let mut patterns: HashMap<SignalType, Vec<&FeedbackSignal>> = HashMap::new();

        // Group signals by type
        for signal in self.signals.iter().filter(|s| !s.consolidated) {
            patterns.entry(signal.signal_type.clone())
                .or_default()
                .push(signal);
        }

        let mut consolidated = Vec::new();

        // Aggregate each group
        for (signal_type, signals) in patterns {
            let count = signals.len() as u32;
            
            // Only patterns that appear multiple times can affect state
            if count < self.pattern_threshold {
                tracing::debug!(
                    "Pattern {:?} has {} occurrences, below threshold {}",
                    signal_type, count, self.pattern_threshold
                );
                continue;
            }

            let avg_confidence = signals.iter().map(|s| s.confidence).sum::<f32>() / count as f32;
            let avg_valence = signals.iter().map(|s| s.emotional_context).sum::<f32>() / count as f32;

            // Find most confident signal as representative
            let representative = signals.iter()
                .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
                .unwrap();

            let first_seen = signals.iter()
                .map(|s| s.timestamp)
                .min()
                .unwrap();
            let last_seen = signals.iter()
                .map(|s| s.timestamp)
                .max()
                .unwrap();

            consolidated.push(ConsolidatedPattern {
                signal_type,
                count,
                avg_confidence,
                avg_valence,
                representative_content: representative.content.clone(),
                first_seen,
                last_seen,
            });
        }

        // Mark all signals as consolidated
        for signal in &mut self.signals {
            signal.consolidated = true;
        }

        // Clean up old consolidated signals (keep last 1000)
        if self.signals.len() > 1000 {
            let drain_count = self.signals.len() - 1000;
            self.signals.drain(0..drain_count);
        }

        consolidated
    }

    /// Clear all unconsolidated signals (e.g., after a crisis event)
    pub fn clear_pending(&mut self) {
        self.signals.retain(|s| s.consolidated);
    }

    /// Get recent signals for debugging/inspection
    pub fn recent_signals(&self, count: usize) -> Vec<&FeedbackSignal> {
        self.signals.iter()
            .rev()
            .take(count)
            .collect()
    }
}

/// Determines what state updates should happen based on consolidated patterns
pub struct ConsolidationProcessor;

impl ConsolidationProcessor {
    /// Compute state deltas from consolidated patterns
    pub fn compute_state_updates(patterns: &[ConsolidatedPattern]) -> StateUpdates {
        let mut updates = StateUpdates::default();

        for pattern in patterns {
            match &pattern.signal_type {
                SignalType::UserEmotionalFeedback => {
                    // Positive feedback reduces attachment anxiety
                    if pattern.avg_valence > 0.3 {
                        updates.attachment_anxiety_delta -= 0.02 * pattern.avg_confidence;
                    } else if pattern.avg_valence < -0.3 {
                        updates.attachment_anxiety_delta += 0.03 * pattern.avg_confidence;
                    }
                }
                SignalType::ValueJudgment { value } => {
                    // Repeated value judgments reinforce that value
                    updates.value_reinforcements.push((
                        value.clone(),
                        0.01 * pattern.count as f32 * pattern.avg_confidence,
                    ));
                }
                SignalType::SelfReflection => {
                    // Self-reflection affects openness
                    if pattern.avg_valence > 0.0 {
                        updates.openness_delta += 0.01 * pattern.avg_confidence;
                    }
                }
                SignalType::PredictionError => {
                    // High prediction errors increase curiosity
                    updates.curiosity_delta += 0.02 * pattern.count as f32 * pattern.avg_confidence;
                }
                SignalType::SituationInterpretation => {
                    // Affects narrative bias
                    updates.narrative_bias_delta += pattern.avg_valence * 0.01 * pattern.avg_confidence;
                }
            }
        }

        updates
    }
}

/// State updates computed from consolidation
#[derive(Debug, Clone, Default)]
pub struct StateUpdates {
    /// Delta for attachment anxiety
    pub attachment_anxiety_delta: f32,
    
    /// Delta for openness
    pub openness_delta: f32,
    
    /// Delta for curiosity baseline
    pub curiosity_delta: f32,
    
    /// Delta for narrative bias
    pub narrative_bias_delta: f32,
    
    /// Value reinforcements: (value_name, delta)
    pub value_reinforcements: Vec<(String, f32)>,
}

impl StateUpdates {
    pub fn is_empty(&self) -> bool {
        self.attachment_anxiety_delta.abs() < 0.001
            && self.openness_delta.abs() < 0.001
            && self.curiosity_delta.abs() < 0.001
            && self.narrative_bias_delta.abs() < 0.001
            && self.value_reinforcements.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uncertainty_discounting() {
        let mut buffer = FeedbackBuffer::new();
        
        // Low confidence signal should be discounted
        buffer.add_signal(
            SignalType::SelfReflection,
            "I think I was wrong".to_string(),
            0.3, // Below threshold
            0.0,
        );
        
        assert_eq!(buffer.pending_count(), 0);
        
        // High confidence signal should be kept
        buffer.add_signal(
            SignalType::SelfReflection,
            "I am certain I was wrong".to_string(),
            0.8,
            0.0,
        );
        
        assert_eq!(buffer.pending_count(), 1);
    }

    #[test]
    fn test_temporal_smoothing() {
        let mut buffer = FeedbackBuffer::new();
        
        // Add only 2 signals (below threshold of 3)
        for _ in 0..2 {
            buffer.add_signal(
                SignalType::UserEmotionalFeedback,
                "User seemed happy".to_string(),
                0.8,
                0.5,
            );
        }
        
        let patterns = buffer.consolidate();
        assert!(patterns.is_empty()); // Not enough occurrences
        
        // Now add 3 more signals (fresh batch)
        for _ in 0..3 {
            buffer.add_signal(
                SignalType::UserEmotionalFeedback,
                "User seemed happy again".to_string(),
                0.9,
                0.6,
            );
        }
        
        let patterns = buffer.consolidate();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].count, 3);
    }

    #[test]
    fn test_state_updates_computation() {
        let patterns = vec![
            ConsolidatedPattern {
                signal_type: SignalType::UserEmotionalFeedback,
                count: 5,
                avg_confidence: 0.8,
                avg_valence: 0.6,
                representative_content: "User seemed happy".to_string(),
                first_seen: Utc::now(),
                last_seen: Utc::now(),
            },
        ];

        let updates = ConsolidationProcessor::compute_state_updates(&patterns);
        
        // Positive feedback should reduce attachment anxiety
        assert!(updates.attachment_anxiety_delta < 0.0);
    }
}
