pub mod persona;
pub mod prelude;
pub mod affect;
pub mod state;
pub mod dynamics;
pub mod values;

pub use persona::{Psyche, SeedPersona};
pub use affect::Affect;
pub use state::{OrganismState, FastState, MediumState, SlowState, SensoryInput, AttachmentStyle, ValueNetwork};
pub use dynamics::{Dynamics, DefaultDynamics};
pub use values::{ValueJudge, RuleBasedJudge, Situation, JudgmentResult, ValueConflict, HierarchicalValueNetwork, ValueTier};


use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use uuid::Uuid;

/// Represents a normalized unit of information from any source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    pub id: Uuid,
    pub source: String,
    pub author: String,
    pub body: String,
    pub timestamp: i64, // Unix timestamp
    pub modality: Modality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
    Mixed,
}

/// Represents an event that triggers the reasoning loop
#[derive(Debug, Clone)]
pub enum Event {
    UserMessage(Content),
    SystemSignal(String),
    Heartbeat,
    /// Proactive trigger initiated by the system
    ProactiveTrigger(Trigger),
}

/// Trigger types that can initiate proactive reasoning
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Trigger {
    /// Scheduled time-based trigger (e.g., morning greeting, evening summary)
    Scheduled {
        name: String,
        /// Cron expression or simple schedule identifier
        schedule: String,
    },
    /// Content from a source matches user interests
    ContentRelevance {
        source: String,
        content_id: String,
        /// Relevance score (0.0 - 1.0)
        score: f32,
        /// Brief description of why it's relevant
        reason: String,
    },
    /// Topic not discussed in a while (memory decay)
    MemoryDecay {
        topic: String,
        /// Unix timestamp of last mention
        last_mentioned: i64,
    },
    /// Trending content on monitored platform
    Trending {
        platform: String,
        topic: String,
    },
    /// Internal state-driven rumination (mind-wandering, social longing, curiosity)
    Rumination {
        /// Kind: "mind_wandering", "social_longing", "curiosity_spike"
        kind: String,
        /// Human-readable context for the LLM
        context: String,
    },
}

#[async_trait]
pub trait Memory: Send + Sync {
    /// Recall relevant episodes via vector search.
    async fn recall(&self, query: &str) -> anyhow::Result<String>;
    /// Store a new content item.
    async fn memorize(&self, content: &Content) -> anyhow::Result<()>;
    /// Recall known facts formatted for prompt injection.
    /// Default: returns empty string (no facts store available).
    async fn recall_facts_formatted(&self, _query: &str) -> anyhow::Result<String> {
        Ok(String::new())
    }
    /// Store a semantic fact triple. Default: no-op.
    async fn store_fact(&self, _subject: &str, _predicate: &str, _object: &str, _confidence: f32) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait]
pub trait Perception: Send + Sync {
    async fn listen(&self) -> anyhow::Result<Event>;
}

#[async_trait]
pub trait Reasoning: Send + Sync {
    async fn think(&self, event: Event) -> anyhow::Result<ReasoningOutput>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub id: Uuid,
    pub name: String,
    /// Platform -> ID (e.g., "qq" -> "123456")
    pub aliases: std::collections::HashMap<String, String>,
}

#[async_trait]
pub trait SocialGraph: Send + Sync {
    /// Find a person by one of their platform aliases (e.g., "qq", "12345")
    async fn find_person(&self, platform: &str, platform_id: &str) -> anyhow::Result<Option<Person>>;
    
    /// Create or update a person
    async fn upsert_person(&self, person: &Person) -> anyhow::Result<()>;
    
    /// Record a relationship or interaction between two people
    async fn record_interaction(&self, from_person_id: Uuid, to_person_id: Uuid, context: &str) -> anyhow::Result<()>;
}

#[async_trait]
pub trait Expression: Send + Sync {
    async fn speak(&self, message: &str) -> anyhow::Result<()>;
}

/// Evaluates conditions and produces triggers for proactive behavior
#[async_trait]
pub trait TriggerEvaluator: Send + Sync {
    /// Evaluate if any triggers should fire now
    async fn evaluate(&self) -> anyhow::Result<Vec<Trigger>>;
    
    /// Get the name of this evaluator for logging
    fn name(&self) -> &'static str;
}

/// Emotional tone for voice synthesis (cross-cutting concern)
/// 
/// DEPRECATED: Use `Affect` for the new continuous emotion model.
/// This enum is kept for backward compatibility with TTS and existing code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Emotion {
    #[default]
    Neutral,
    Happy,
    Sad,
    Excited,
    Calm,
    Angry,
    Surprised,
}

impl Emotion {
    /// Get a descriptive name for the emotion
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Happy => "happy",
            Self::Sad => "sad",
            Self::Excited => "excited",
            Self::Calm => "calm",
            Self::Angry => "angry",
            Self::Surprised => "surprised",
        }
    }
    
    /// Parse from string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "neutral" => Some(Self::Neutral),
            "happy" => Some(Self::Happy),
            "sad" => Some(Self::Sad),
            "excited" => Some(Self::Excited),
            "calm" => Some(Self::Calm),
            "angry" => Some(Self::Angry),
            "surprised" => Some(Self::Surprised),
            _ => None,
        }
    }

    /// Convert from Affect (new model) to Emotion (legacy)
    pub fn from_affect(affect: &Affect) -> Self {
        match affect.to_discrete_label() {
            "happy" => Self::Happy,
            "sad" => Self::Sad,
            "excited" => Self::Excited,
            "calm" => Self::Calm,
            "angry" => Self::Angry,
            "anxious" => Self::Surprised, // Map anxious to surprised for TTS
            _ => Self::Neutral,
        }
    }
}

/// Modality hint for how reasoning output should be expressed
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum ResponseModality {
    #[default]
    Text,
    /// Voice output hint
    Voice,
    /// Platform-specific sticker/emoji
    Sticker(String),
}

/// Output from the reasoning engine with modality hints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningOutput {
    pub content: String,
    pub modality: ResponseModality,
    /// Legacy emotion for backward compatibility (TTS, etc.)
    pub emotion: Emotion,
    /// New continuous affect model
    #[serde(default)]
    pub affect: Affect,
}

impl ReasoningOutput {
    /// Create a new output with affect (emotion is derived automatically)
    pub fn with_affect(content: String, modality: ResponseModality, affect: Affect) -> Self {
        Self {
            content,
            modality,
            emotion: Emotion::from_affect(&affect),
            affect,
        }
    }
}
