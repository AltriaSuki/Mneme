pub mod persona;
pub mod prelude;

pub use persona::Psyche;


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
}

#[async_trait]
pub trait Memory: Send + Sync {
    async fn recall(&self, query: &str) -> anyhow::Result<String>;
    async fn memorize(&self, content: &Content) -> anyhow::Result<()>;
}

#[async_trait]
pub trait Perception: Send + Sync {
    async fn listen(&self) -> anyhow::Result<Event>;
}

#[async_trait]
pub trait Reasoning: Send + Sync {
    async fn think(&self, event: Event) -> anyhow::Result<String>;
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
