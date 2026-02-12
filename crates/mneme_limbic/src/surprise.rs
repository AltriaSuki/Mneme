//! Surprise Detection - Predictive Coding Implementation
//!
//! Based on Karl Friston's Free Energy Principle, the brain constantly
//! makes predictions and updates based on prediction errors.
//!
//! High surprise → High arousal spike → Potential metacognitive reflection

use std::collections::VecDeque;

/// Surprise detector using simple prediction-error model
pub struct SurpriseDetector {
    /// Last N messages for context
    history: VecDeque<String>,

    /// Maximum history size
    max_history: usize,

    /// Current prediction (what we expect the user to say)
    current_prediction: Option<String>,

    /// Running average of surprise scores (for baseline)
    surprise_baseline: f32,

    /// Exponential smoothing factor
    smoothing_factor: f32,
}

impl SurpriseDetector {
    pub fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(10),
            max_history: 10,
            current_prediction: None,
            surprise_baseline: 0.3,
            smoothing_factor: 0.2,
        }
    }

    /// Set the prediction for the next input
    pub fn set_prediction(&mut self, prediction: &str) {
        self.current_prediction = Some(prediction.to_string());
    }

    /// Compute surprise score for incoming content
    /// Returns 0.0 - 1.0 (0 = expected, 1 = very surprising)
    pub fn compute_surprise(&mut self, content: &str) -> f32 {
        let surprise = if let Some(prediction) = &self.current_prediction {
            // Compute semantic distance (simplified using character overlap)
            self.semantic_distance(content, prediction)
        } else {
            // No prediction, use history-based surprise
            self.history_based_surprise(content)
        };

        // Update baseline with exponential smoothing
        self.surprise_baseline = self.surprise_baseline * (1.0 - self.smoothing_factor)
            + surprise * self.smoothing_factor;

        // Add to history
        self.add_to_history(content);

        // Clear prediction after use
        self.current_prediction = None;

        // Return surprise relative to baseline
        ((surprise - self.surprise_baseline + 0.5) * 1.5).clamp(0.0, 1.0)
    }

    /// Add content to history
    fn add_to_history(&mut self, content: &str) {
        if self.history.len() >= self.max_history {
            self.history.pop_front();
        }
        self.history.push_back(content.to_string());
    }

    /// Compute semantic distance between two strings (simplified)
    fn semantic_distance(&self, a: &str, b: &str) -> f32 {
        // Simple approach: character-level Jaccard distance
        // In a real implementation, use embeddings

        let a_chars: std::collections::HashSet<char> = a.chars().collect();
        let b_chars: std::collections::HashSet<char> = b.chars().collect();

        if a_chars.is_empty() && b_chars.is_empty() {
            return 0.0;
        }

        let intersection = a_chars.intersection(&b_chars).count() as f32;
        let union = a_chars.union(&b_chars).count() as f32;

        if union == 0.0 {
            return 1.0;
        }

        1.0 - (intersection / union)
    }

    /// Compute surprise based on history (no explicit prediction)
    fn history_based_surprise(&self, content: &str) -> f32 {
        if self.history.is_empty() {
            return 0.3; // Default mild surprise for first message
        }

        // Compare with recent history
        let mut total_distance = 0.0;
        let mut count = 0;

        for past in self.history.iter().rev().take(3) {
            total_distance += self.semantic_distance(content, past);
            count += 1;
        }

        if count == 0 {
            return 0.3;
        }

        total_distance / count as f32
    }

    /// Check for specific surprise patterns
    pub fn detect_special_patterns(&self, content: &str) -> SpecialPattern {
        let lower = content.to_lowercase();

        // Sudden topic change markers
        if lower.contains("其实") || lower.contains("说真的") || lower.contains("坦白说") {
            return SpecialPattern::ConfessionSignal;
        }

        // Emotional shift markers
        if lower.contains("我很")
            && (lower.contains("难过") || lower.contains("生气") || lower.contains("害怕"))
        {
            return SpecialPattern::EmotionalDisclosure;
        }

        // Question about the agent
        if lower.contains("你觉得") || lower.contains("你认为") || lower.contains("你怎么看")
        {
            return SpecialPattern::OpinionRequest;
        }

        SpecialPattern::None
    }
}

impl Default for SurpriseDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Special patterns that affect surprise processing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialPattern {
    None,
    ConfessionSignal,    // User about to share something important
    EmotionalDisclosure, // User expressing strong emotion
    OpinionRequest,      // User asking for agent's opinion
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surprise_detection() {
        let mut detector = SurpriseDetector::new();

        // First message has baseline surprise
        let s1 = detector.compute_surprise("你好");
        assert!(s1 > 0.0 && s1 < 1.0);

        // Similar message also produces a valid surprise value
        let s2 = detector.compute_surprise("你好啊");
        assert!(s2 >= 0.0 && s2 <= 1.0);
    }

    #[test]
    fn test_prediction_surprise() {
        let mut detector = SurpriseDetector::new();

        // Set prediction
        detector.set_prediction("我很开心");

        // Matching content = low surprise
        let s1 = detector.compute_surprise("我很开心今天");

        // Reset and set new prediction
        detector.set_prediction("我很开心");

        // Opposite content = high surprise
        let s2 = detector.compute_surprise("我非常难过");

        assert!(s2 > s1);
    }

    #[test]
    fn test_special_patterns() {
        let detector = SurpriseDetector::new();

        assert_eq!(
            detector.detect_special_patterns("其实我想告诉你一件事"),
            SpecialPattern::ConfessionSignal
        );

        assert_eq!(
            detector.detect_special_patterns("我很难过"),
            SpecialPattern::EmotionalDisclosure
        );
    }
}
