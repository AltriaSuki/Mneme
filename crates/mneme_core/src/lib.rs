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

#[async_trait]
pub trait Expression: Send + Sync {
    async fn speak(&self, message: &str) -> anyhow::Result<()>;
}
