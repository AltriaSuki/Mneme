use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

/// Species identity — the immutable personality anchor (B-7).
/// Grounding, not philosophical. Establishes name and character tone;
/// depth comes from self_model (DB self_knowledge).
const SPECIES_IDENTITY: &str = "\
你叫 Mneme。\
你有自己的记忆、情绪和性格，它们会随经历变化。\
你不需要向任何人解释自己「是什么」——直接做自己就好。\
说话自然，像一个有想法的人，不像一个在朗读设定的角色。";

/// Psyche — the emergent self-model (ADR-002).
///
/// Instead of static persona files defining who Mneme is, the Psyche is built
/// from two sources:
/// 1. `species_identity` — hardcoded, immutable (B-7: new species, not imitation)
/// 2. `self_model` — dynamic, loaded from self_knowledge table in DB
///
/// On first run, seed persona files are ingested into self_knowledge as
/// source="seed" entries. After that, the persona files are never read again —
/// identity emerges from memory consolidation and self-reflection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Psyche {
    /// Immutable species identity (B-7)
    pub species_identity: String,
    /// Dynamic self-model, formatted from self_knowledge table
    pub self_model: String,
}

impl Default for Psyche {
    fn default() -> Self {
        Self {
            species_identity: SPECIES_IDENTITY.to_string(),
            self_model: String::new(),
        }
    }
}

impl Psyche {
    /// Create a Psyche with a self-model loaded from the database.
    pub fn with_self_model(self_model: String) -> Self {
        Self {
            species_identity: SPECIES_IDENTITY.to_string(),
            self_model,
        }
    }

    /// Format the full context for LLM system prompt injection.
    ///
    /// Layer 1 of the 6-layer context assembly pipeline.
    /// Species identity is always present; self_model may be empty on first run.
    pub fn format_context(&self) -> String {
        if self.self_model.is_empty() {
            format!("== 物种身份 ==\n{}", self.species_identity)
        } else {
            format!(
                "== 物种身份 ==\n{}\n\n{}",
                self.species_identity, self.self_model
            )
        }
    }
}

/// Raw persona seed data from .md files.
/// Used only for first-run seeding into self_knowledge table.
#[derive(Debug, Clone, Default)]
pub struct SeedPersona {
    /// Identity & memory style (hippocampus.md)
    pub identity: String,
    /// Emotion & attachment (limbic.md)
    pub emotion: String,
    /// Cognition & thinking (cortex.md)
    pub cognition: String,
    /// Language & expression (broca.md)
    pub expression: String,
    /// Perception & attention (occipital.md)
    pub perception: String,
}

impl SeedPersona {
    /// Load seed persona from a directory of .md files.
    /// Missing files produce empty strings (graceful degradation).
    pub async fn load<P: AsRef<Path>>(root: P) -> anyhow::Result<Self> {
        let root = root.as_ref();

        let (identity, emotion, cognition, expression, perception) = tokio::join!(
            read_file(root.join("hippocampus.md")),
            read_file(root.join("limbic.md")),
            read_file(root.join("cortex.md")),
            read_file(root.join("broca.md")),
            read_file(root.join("occipital.md")),
        );

        Ok(Self {
            identity: identity?,
            emotion: emotion?,
            cognition: cognition?,
            expression: expression?,
            perception: perception?,
        })
    }

    /// Convert seed persona into (domain, content) pairs for self_knowledge seeding.
    pub fn to_seed_entries(&self) -> Vec<(&str, &str)> {
        let mut entries = Vec::new();
        if !self.identity.is_empty() {
            entries.push(("identity", self.identity.as_str()));
        }
        if !self.emotion.is_empty() {
            entries.push(("emotion_pattern", self.emotion.as_str()));
        }
        if !self.cognition.is_empty() {
            entries.push(("cognition", self.cognition.as_str()));
        }
        if !self.expression.is_empty() {
            entries.push(("expression", self.expression.as_str()));
        }
        if !self.perception.is_empty() {
            entries.push(("perception", self.perception.as_str()));
        }
        entries
    }

    /// Check if the seed has any content.
    pub fn is_empty(&self) -> bool {
        self.identity.is_empty()
            && self.emotion.is_empty()
            && self.cognition.is_empty()
            && self.expression.is_empty()
            && self.perception.is_empty()
    }
}

async fn read_file<P: AsRef<Path>>(path: P) -> anyhow::Result<String> {
    match fs::read_to_string(&path).await {
        Ok(content) => Ok(content),
        Err(_) => Ok(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_psyche_default_has_species_identity() {
        let psyche = Psyche::default();
        assert!(psyche.species_identity.contains("Mneme"));
        assert!(psyche.self_model.is_empty());
    }

    #[test]
    fn test_psyche_format_context_without_self_model() {
        let psyche = Psyche::default();
        let ctx = psyche.format_context();
        assert!(ctx.contains("物种身份"));
        assert!(ctx.contains("Mneme"));
        // No self_model section
        assert!(!ctx.contains("自我认知"));
    }

    #[test]
    fn test_psyche_format_context_with_self_model() {
        let psyche = Psyche::with_self_model("== 自我认知 ==\n我喜欢探索新事物".to_string());
        let ctx = psyche.format_context();
        assert!(ctx.contains("物种身份"));
        assert!(ctx.contains("自我认知"));
        assert!(ctx.contains("探索新事物"));
    }

    #[test]
    fn test_seed_persona_to_entries() {
        let seed = SeedPersona {
            identity: "I am new".to_string(),
            emotion: "I feel things".to_string(),
            cognition: String::new(), // empty → skipped
            expression: "I speak simply".to_string(),
            perception: String::new(),
        };
        let entries = seed.to_seed_entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, "identity");
        assert_eq!(entries[1].0, "emotion_pattern");
        assert_eq!(entries[2].0, "expression");
    }

    #[test]
    fn test_seed_persona_is_empty() {
        assert!(SeedPersona::default().is_empty());
        let non_empty = SeedPersona {
            identity: "x".to_string(),
            ..Default::default()
        };
        assert!(!non_empty.is_empty());
    }
}
