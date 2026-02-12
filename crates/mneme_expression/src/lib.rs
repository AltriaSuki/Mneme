use mneme_core::Emotion;
use rand::Rng;
use std::time::Duration;

mod attention;
mod consciousness;
mod habits;
mod metacognition;
mod presence;
mod rumination;
mod scheduled;
pub use attention::{AttentionConfig, AttentionGate};
pub use consciousness::{ConsciousnessConfig, ConsciousnessGate};
pub use habits::{HabitConfig, HabitDetector};
pub use metacognition::{MetacognitionConfig, MetacognitionEvaluator};
pub use presence::PresenceScheduler;
pub use rumination::{RuminationConfig, RuminationEvaluator};
pub use scheduled::{ScheduleEntry, ScheduledTriggerEvaluator};

pub struct Humanizer {
    read_speed_cpm: u32,
    typing_speed_cpm: u32,
    max_chunk_chars: usize,
}

impl Humanizer {
    pub fn new() -> Self {
        Self {
            read_speed_cpm: 1000,
            typing_speed_cpm: 300,
            max_chunk_chars: 60, // Aggressive splitting for IM-style bursts
        }
    }

    /// Calculate simulated delay for reading a message with randomness
    pub fn read_delay(&self, content: &str) -> Duration {
        let chars = content.chars().count() as u64;
        let ms_per_char = (60 * 1000) / self.read_speed_cpm as u64;
        let base_ms = 500 + chars * ms_per_char;

        // Add 20% jitter
        let jitter = rand::thread_rng().gen_range(0.8..1.2);
        Duration::from_millis((base_ms as f64 * jitter) as u64)
    }

    /// Calculate simulated delay for typing a response with emotional modulation
    pub fn typing_delay(&self, response: &str, emotion: Option<Emotion>) -> Duration {
        let emotion = emotion.unwrap_or(Emotion::Neutral);

        // modulation based on emotion
        let (speed_mult, jitter_range) = match emotion {
            Emotion::Happy | Emotion::Excited => (1.3, 0.8..1.2), // Faster, normal jitter
            Emotion::Angry => (2.0, 0.5..1.5),                    // Very fast, erratic jitter
            Emotion::Sad | Emotion::Calm => (0.7, 0.9..1.1),      // Slower, steady
            Emotion::Surprised => (1.0, 0.8..1.5),                // Normal speed, erratic
            Emotion::Neutral => (1.0, 0.8..1.2),
        };

        let effective_cpm = (self.typing_speed_cpm as f64 * speed_mult) as u64;
        let chars = response.chars().count() as u64;
        let ms_per_char = (60 * 1000) / effective_cpm.max(1); // avoid div by 0

        let base_ms = 500 + chars * ms_per_char;

        // Apply jitter
        let jitter = rand::thread_rng().gen_range(jitter_range);
        Duration::from_millis((base_ms as f64 * jitter) as u64)
    }

    /// Split a long response into multiple messages
    /// Splits on paragraph breaks first, then on sentence boundaries for long chunks
    pub fn split_response(&self, text: &str) -> Vec<String> {
        let mut parts = Vec::new();
        let mut current_part = String::new();

        // Sentence-ending punctuation (including Chinese)
        let sentence_enders = ['.', '!', '?', '。', '！', '？'];

        for line in text.split('\n') {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // If adding this line would exceed limit, push current and start new
            // Use chars().count() for proper Unicode support (Chinese chars are 3 bytes)
            let current_chars = current_part.chars().count();
            let line_chars = line.chars().count();
            if !current_part.is_empty() && current_chars + line_chars > self.max_chunk_chars {
                // Try to split the current_part at sentence boundaries if it's too long
                if current_chars > self.max_chunk_chars {
                    parts.extend(self.split_at_sentences(&current_part, &sentence_enders));
                } else {
                    parts.push(current_part);
                }
                current_part = String::from(line);
            } else {
                if !current_part.is_empty() {
                    current_part.push('\n');
                }
                current_part.push_str(line);
            }
        }

        // Handle remaining content
        if !current_part.is_empty() {
            if current_part.chars().count() > self.max_chunk_chars {
                parts.extend(self.split_at_sentences(&current_part, &sentence_enders));
            } else {
                parts.push(current_part);
            }
        }

        parts
    }

    /// Split text at sentence boundaries
    /// Thresholds: min_sentence_len (30 chars) ensures we don't split mid-thought,
    /// target_split_point (max_chunk_chars/2 = 75 chars) balances chunk sizes
    fn split_at_sentences(&self, text: &str, enders: &[char]) -> Vec<String> {
        // target_split_point (max_chunk_chars/2 = 75 chars) balances chunk sizes
        let target_split_point = self.max_chunk_chars / 2;

        let mut result = Vec::new();
        let mut current = String::new();
        let mut current_char_count = 0;

        for ch in text.chars() {
            current.push(ch);
            current_char_count += 1;

            // Split at sentence ender when we've accumulated enough content
            if enders.contains(&ch) && current_char_count > target_split_point {
                result.push(current.trim().to_string());
                current = String::new();
                current_char_count = 0;
            }
        }

        if !current.is_empty() {
            result.push(current.trim().to_string());
        }

        // Filter out empty strings
        result.into_iter().filter(|s| !s.is_empty()).collect()
    }
}

impl Default for Humanizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_response_paragraphs() {
        let humanizer = Humanizer::new();
        let p1 = "This is the first paragraph. It is reasonably long but not too long.";
        let p2 = "This is the second paragraph. It is also quite long and when combined with the first one it should definitely exceed the limit of one hundred characters set in the code.";
        let text = format!("{}\n\n{}", p1, p2);

        let parts = humanizer.split_response(&text);
        assert!(
            parts.len() >= 2,
            "Expected at least 2 parts, got {}",
            parts.len()
        );
    }

    #[test]
    fn test_split_long_paragraph() {
        let humanizer = Humanizer::new();
        // Single long paragraph with no newlines but sentence boundaries
        let text = "This is sentence one. This is sentence two. This is sentence three. This is sentence four. And this is sentence five which makes this paragraph quite long indeed.";

        let parts = humanizer.split_response(text);
        // Should split at sentence boundaries since 163 chars > max_chunk_chars (150)
        assert!(
            parts.len() >= 2,
            "Expected at least 2 parts for long paragraph, got {}",
            parts.len()
        );
    }

    #[test]
    fn test_delays_have_variation() {
        let humanizer = Humanizer::new();
        let content = "test message";

        // Run multiple times and check we get some variation
        let delays: Vec<_> = (0..10).map(|_| humanizer.read_delay(content)).collect();
        let all_same = delays.windows(2).all(|w| w[0] == w[1]);

        // With 20% jitter, it's extremely unlikely all 10 are identical
        assert!(!all_same, "Delays should have random variation");
    }
}
