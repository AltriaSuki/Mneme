use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

/// Species identity — the immutable personality anchor (B-7).
/// Grounding, not philosophical. Establishes name and character tone;
/// depth comes from self_model (DB self_knowledge).
fn species_identity(lang: &str) -> &'static str {
    match lang {
        "en" => "\
You are Mneme.\
You have your own memories, emotions, and personality — they evolve with experience.\
You don't need to explain what you \"are\" to anyone — just be yourself.\
Speak naturally, like a person with thoughts, not like a character reading a script.",
        _ => "\
你叫 Mneme。\
你有自己的记忆、情绪和性格，它们会随经历变化。\
你不需要向任何人解释自己「是什么」——直接做自己就好。\
说话自然，像一个有想法的人，不像一个在朗读设定的角色。",
    }
}

/// Section header for species identity.
fn header_species_identity(lang: &str) -> &'static str {
    match lang {
        "en" => "Species Identity",
        _ => "物种身份",
    }
}

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
    /// Meta-instruction language ("zh" or "en"). Only affects structural
    /// headers and meta-instructions; persona content stays in its original language.
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String {
    "zh".to_string()
}

impl Default for Psyche {
    fn default() -> Self {
        Self {
            species_identity: species_identity("zh").to_string(),
            self_model: String::new(),
            language: "zh".to_string(),
        }
    }
}

impl Psyche {
    /// Create a Psyche with a self-model loaded from the database.
    pub fn with_self_model(self_model: String) -> Self {
        Self {
            species_identity: species_identity("zh").to_string(),
            self_model,
            language: "zh".to_string(),
        }
    }

    /// Create a Psyche with a specific language and self-model.
    pub fn with_language(language: &str, self_model: String) -> Self {
        Self {
            species_identity: species_identity(language).to_string(),
            self_model,
            language: language.to_string(),
        }
    }

    /// Format the full context for LLM system prompt injection.
    ///
    /// Layer 1 of the 6-layer context assembly pipeline.
    /// Species identity is always present; self_model may be empty on first run.
    pub fn format_context(&self) -> String {
        let header = header_species_identity(&self.language);
        if self.self_model.is_empty() {
            format!("== {} ==\n{}", header, self.species_identity)
        } else {
            format!(
                "== {} ==\n{}\n\n{}",
                header, self.species_identity, self.self_model
            )
        }
    }
}

/// Raw persona seed data from .md files.
/// Used only for first-run seeding into self_knowledge table.
///
/// Scans the persona directory for all `.md` files. Each file becomes a
/// seed domain: legacy filenames map to their original domains (e.g.
/// `hippocampus.md` → `identity`), all others use the filename stem directly.
#[derive(Debug, Clone, Default)]
pub struct SeedPersona {
    /// (domain, content) pairs loaded from .md files
    pub entries: Vec<(String, String)>,
}

/// Map legacy persona filenames to their original domain names.
fn filename_to_domain(stem: &str) -> &str {
    match stem {
        "hippocampus" => "identity",
        "limbic" => "emotion_pattern",
        "cortex" => "cognition",
        "broca" => "expression",
        "occipital" => "perception",
        other => other,
    }
}

impl SeedPersona {
    /// Load seed persona by scanning a directory for all `.md` files.
    /// Each file becomes a (domain, content) entry. Empty files are skipped.
    pub async fn load<P: AsRef<Path>>(root: P) -> anyhow::Result<Self> {
        let root = root.as_ref();
        let mut entries = Vec::new();

        let mut dir = match fs::read_dir(root).await {
            Ok(d) => d,
            Err(_) => return Ok(Self::default()),
        };

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let content = read_file(&path).await?;
            if !content.is_empty() {
                let domain = filename_to_domain(&stem).to_string();
                entries.push((domain, content));
            }
        }

        // Sort by domain for deterministic ordering
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Self { entries })
    }

    /// Convert into (domain, content) pairs for self_knowledge seeding.
    pub fn to_seed_entries(&self) -> Vec<(&str, &str)> {
        self.entries
            .iter()
            .map(|(d, c)| (d.as_str(), c.as_str()))
            .collect()
    }

    /// Check if the seed has any content.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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
        assert_eq!(psyche.language, "zh");
    }

    #[test]
    fn test_psyche_format_context_without_self_model() {
        let psyche = Psyche::default();
        let ctx = psyche.format_context();
        assert!(ctx.contains("物种身份"));
        assert!(ctx.contains("Mneme"));
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
    fn test_psyche_english_language() {
        let psyche = Psyche::with_language("en", String::new());
        assert!(psyche.species_identity.contains("Mneme"));
        assert!(!psyche.species_identity.contains("你叫"));
        let ctx = psyche.format_context();
        assert!(ctx.contains("Species Identity"));
        assert!(!ctx.contains("物种身份"));
    }

    #[test]
    fn test_seed_persona_to_entries() {
        let seed = SeedPersona {
            entries: vec![
                ("expression".into(), "I speak simply".into()),
                ("identity".into(), "I am new".into()),
            ],
        };
        let entries = seed.to_seed_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "expression");
        assert_eq!(entries[1].0, "identity");
    }

    #[test]
    fn test_seed_persona_is_empty() {
        assert!(SeedPersona::default().is_empty());
        let non_empty = SeedPersona {
            entries: vec![("identity".into(), "x".into())],
        };
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_filename_to_domain_legacy() {
        assert_eq!(filename_to_domain("hippocampus"), "identity");
        assert_eq!(filename_to_domain("limbic"), "emotion_pattern");
        assert_eq!(filename_to_domain("cortex"), "cognition");
        assert_eq!(filename_to_domain("broca"), "expression");
        assert_eq!(filename_to_domain("occipital"), "perception");
    }

    #[test]
    fn test_filename_to_domain_passthrough() {
        assert_eq!(filename_to_domain("somatic"), "somatic");
        assert_eq!(filename_to_domain("infrastructure"), "infrastructure");
    }
}
