//! Dream Generation (ADR-008) — 做梦机制
//!
//! During sleep consolidation, the rumination mechanism runs:
//! recall 2-3 episodes weighted by strength, weave them into a dream narrative.
//! Dreams have normal strength and can be recalled — she might say
//! "我昨晚做了个奇怪的梦……" during conversation.
//!
//! Phase 1: Rule-based template stitching (no LLM dependency).
//! Phase 2: LLM-generated dream narratives via DreamNarrator trait.

use anyhow::Result;
use async_trait::async_trait;
use crate::sqlite::DreamSeed;
use mneme_core::OrganismState;

/// Trait for LLM-based dream narrative generation (Phase 2).
#[async_trait]
pub trait DreamNarrator: Send + Sync {
    async fn narrate_dream(&self, seeds: &[DreamSeed], state: &OrganismState) -> Result<String>;
}

/// A generated dream episode ready for storage.
#[derive(Debug, Clone)]
pub struct DreamEpisode {
    /// The dream narrative text
    pub narrative: String,
    /// Source episode IDs that seeded this dream
    pub source_ids: Vec<String>,
    /// Emotional tone of the dream (-1.0 to 1.0)
    pub emotional_tone: f32,
}

/// Rule-based dream generator for sleep consolidation.
///
/// Selects a narrative template based on current mood_bias,
/// extracts key fragments from each seed, and weaves them
/// into a dream narrative in Chinese.
pub struct DreamGenerator;

impl DreamGenerator {
    /// Generate a dream from memory seeds and current organism state.
    ///
    /// Returns `None` if fewer than 2 seeds are provided.
    pub fn generate(seeds: &[DreamSeed], state: &OrganismState) -> Option<DreamEpisode> {
        if seeds.len() < 2 {
            return None;
        }

        let mood = state.medium.mood_bias;

        // Extract key fragments from each seed (truncate to ~80 chars)
        let fragments: Vec<String> = seeds
            .iter()
            .map(|s| Self::extract_fragment(&s.body))
            .collect();

        // Select template category based on mood_bias
        let narrative = Self::build_narrative(mood, &fragments);

        // Compute emotional tone: weighted average of seed strengths as proxy
        // for emotional intensity, shifted by mood_bias
        let emotional_tone = Self::compute_emotional_tone(seeds, mood);

        let source_ids = seeds.iter().map(|s| s.id.clone()).collect();

        Some(DreamEpisode {
            narrative,
            source_ids,
            emotional_tone,
        })
    }

    /// Extract a key fragment from an episode body.
    /// Truncates to roughly 80 characters at a natural boundary.
    fn extract_fragment(body: &str) -> String {
        let trimmed = body.trim();
        let chars: Vec<char> = trimmed.chars().collect();
        if chars.len() <= 80 {
            return trimmed.to_string();
        }
        // Take first 80 chars, try to break at punctuation
        let truncated = &chars[..80];
        // Find last punctuation char index for a clean break
        let punct = |c: &char| "。，！？、；….,".contains(*c);
        if let Some(last_punct) = truncated.iter().rposition(punct) {
            truncated[..=last_punct].iter().collect()
        } else {
            let s: String = truncated.iter().collect();
            format!("{}…", s)
        }
    }

    /// Build dream narrative from mood and fragments.
    fn build_narrative(mood: f32, fragments: &[String]) -> String {
        let f1 = fragments.first().map(|s| s.as_str()).unwrap_or("");
        let f2 = fragments.get(1).map(|s| s.as_str()).unwrap_or("");
        let f3 = fragments.get(2).map(|s| s.as_str()).unwrap_or("");

        if mood > 0.3 {
            // Positive dream
            Self::positive_template(f1, f2, f3)
        } else if mood < -0.3 {
            // Negative dream
            Self::negative_template(f1, f2, f3)
        } else if fragments.len() >= 3 {
            // Chaotic/mixed dream (when mood is neutral and enough fragments)
            Self::chaotic_template(f1, f2, f3)
        } else {
            // Neutral dream
            Self::neutral_template(f1, f2)
        }
    }

    fn positive_template(f1: &str, f2: &str, f3: &str) -> String {
        if f3.is_empty() {
            format!("梦见了一些温暖的画面……{f1}……然后场景变了，{f2}……醒来时心里暖暖的。")
        } else {
            format!(
                "梦见了一些温暖的画面……{f1}……然后场景变了，{f2}……最后隐约看到{f3}……醒来时心里暖暖的。"
            )
        }
    }

    fn negative_template(f1: &str, f2: &str, f3: &str) -> String {
        if f3.is_empty() {
            format!("做了一个不太舒服的梦……{f1}……突然，{f2}……醒来后还有点不安。")
        } else {
            format!("做了一个不太舒服的梦……{f1}……突然，{f2}……然后{f3}……醒来后还有点不安。")
        }
    }

    fn chaotic_template(f1: &str, f2: &str, f3: &str) -> String {
        format!("梦很碎，{f1}和{f2}混在一起，分不清先后……还有{f3}……醒来后只记得一些片段。")
    }

    fn neutral_template(f1: &str, f2: &str) -> String {
        format!("做了一个梦……好像是关于{f1}……后来又变成了{f2}……细节记不太清了。")
    }

    /// Compute emotional tone from seeds and mood bias.
    ///
    /// Uses seed strength as a proxy for emotional intensity (stronger memories
    /// tend to be more emotionally charged). The mood_bias shifts the tone.
    pub fn compute_emotional_tone(seeds: &[DreamSeed], mood_bias: f32) -> f32 {
        if seeds.is_empty() {
            return mood_bias.clamp(-1.0, 1.0);
        }
        let total_strength: f32 = seeds.iter().map(|s| s.strength).sum();
        let avg_strength = total_strength / seeds.len() as f32;
        // Map avg_strength (0.1..1.0) to a base tone (-0.2..0.4),
        // then shift by mood_bias
        let base_tone = (avg_strength - 0.5) * 0.8;
        (base_tone + mood_bias * 0.5).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_seed(id: &str, body: &str, strength: f32) -> DreamSeed {
        DreamSeed {
            id: id.to_string(),
            author: "User".to_string(),
            body: body.to_string(),
            timestamp: 1000,
            strength,
        }
    }

    fn make_state(mood_bias: f32) -> OrganismState {
        let mut state = OrganismState::default();
        state.medium.mood_bias = mood_bias;
        state
    }

    #[test]
    fn test_dream_from_positive_seeds() {
        let seeds = vec![
            make_seed("a", "和朋友一起吃了很好吃的蛋糕", 0.7),
            make_seed("b", "在公园里散步看到了樱花", 0.6),
        ];
        let state = make_state(0.5);
        let dream = DreamGenerator::generate(&seeds, &state).unwrap();

        assert!(dream.narrative.contains("温暖"));
        assert_eq!(dream.source_ids, vec!["a", "b"]);
        assert!(dream.emotional_tone > 0.0);
    }

    #[test]
    fn test_dream_from_negative_seeds() {
        let seeds = vec![
            make_seed("a", "被批评了感觉很难过", 0.5),
            make_seed("b", "下雨天忘带伞淋湿了", 0.4),
        ];
        let state = make_state(-0.5);
        let dream = DreamGenerator::generate(&seeds, &state).unwrap();

        assert!(dream.narrative.contains("不太舒服"));
        assert!(dream.emotional_tone < 0.0);
    }

    #[test]
    fn test_dream_insufficient_seeds() {
        let seeds = vec![make_seed("a", "只有一条记忆", 0.5)];
        let state = make_state(0.0);
        assert!(DreamGenerator::generate(&seeds, &state).is_none());
    }

    #[test]
    fn test_dream_empty_seeds() {
        let state = make_state(0.0);
        assert!(DreamGenerator::generate(&[], &state).is_none());
    }

    #[test]
    fn test_dream_emotional_tone_calculation() {
        // High strength seeds + positive mood → positive tone
        let seeds = vec![make_seed("a", "记忆一", 0.9), make_seed("b", "记忆二", 0.8)];
        let tone_positive = DreamGenerator::compute_emotional_tone(&seeds, 0.5);
        let tone_negative = DreamGenerator::compute_emotional_tone(&seeds, -0.5);

        assert!(tone_positive > tone_negative);
    }

    #[test]
    fn test_dream_mood_bias_influence() {
        let seeds = vec![
            make_seed("a", "普通的一天", 0.5),
            make_seed("b", "吃了午饭", 0.5),
        ];
        let positive_state = make_state(0.6);
        let negative_state = make_state(-0.6);

        let dream_pos = DreamGenerator::generate(&seeds, &positive_state).unwrap();
        let dream_neg = DreamGenerator::generate(&seeds, &negative_state).unwrap();

        // Positive mood → warm template; negative mood → uncomfortable template
        assert!(dream_pos.narrative.contains("温暖"));
        assert!(dream_neg.narrative.contains("不太舒服"));
        assert!(dream_pos.emotional_tone > dream_neg.emotional_tone);
    }

    #[test]
    fn test_dream_chaotic_with_three_seeds() {
        let seeds = vec![
            make_seed("a", "在学校上课", 0.5),
            make_seed("b", "去超市买东西", 0.5),
            make_seed("c", "和猫玩耍", 0.5),
        ];
        // Neutral mood + 3 seeds → chaotic template
        let state = make_state(0.0);
        let dream = DreamGenerator::generate(&seeds, &state).unwrap();

        assert!(dream.narrative.contains("碎"));
        assert_eq!(dream.source_ids.len(), 3);
    }

    #[test]
    fn test_extract_fragment_short() {
        let frag = DreamGenerator::extract_fragment("短文本");
        assert_eq!(frag, "短文本");
    }

    #[test]
    fn test_extract_fragment_long() {
        let long = "这是一段很长的文本，".repeat(10);
        let frag = DreamGenerator::extract_fragment(&long);
        assert!(frag.chars().count() <= 81); // 80 + possible trailing punctuation
    }

    #[test]
    fn test_compute_emotional_tone_pub() {
        // Verify public accessor works
        let seeds = vec![
            make_seed("a", "记忆一", 0.9),
            make_seed("b", "记忆二", 0.8),
        ];
        let tone = DreamGenerator::compute_emotional_tone(&seeds, 0.5);
        assert!(tone > 0.0);
        assert!(tone <= 1.0);
    }
}
