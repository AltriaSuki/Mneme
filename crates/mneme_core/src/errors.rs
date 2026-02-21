//! Domain error types for mneme_core.
//!
//! Provides structured errors instead of opaque `anyhow::Error` at public API
//! boundaries. Existing code can gradually migrate from `anyhow::Result<T>`
//! to `Result<T, MnemeError>`.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MnemeError {
    #[error("memory operation failed: {0}")]
    Memory(#[from] MemoryError),

    #[error("reasoning failed: {0}")]
    Reasoning(#[from] ReasoningError),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("safety guard blocked: {capability}")]
    SafetyBlocked { capability: String },

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("database error: {0}")]
    Database(String),

    #[error("embedding failed: {0}")]
    Embedding(String),

    #[error("consolidation failed: {0}")]
    Consolidation(String),

    #[error("episode not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Error)]
pub enum ReasoningError {
    #[error("LLM provider error: {0}")]
    Provider(String),

    #[error("token budget exhausted (used {used}, limit {limit})")]
    BudgetExhausted { used: u64, limit: u64 },

    #[error("tool execution failed: {tool}: {reason}")]
    ToolExecution { tool: String, reason: String },

    #[error("context assembly failed: {0}")]
    ContextAssembly(String),
}
