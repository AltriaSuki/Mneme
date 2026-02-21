//! Reasoning engine crate — LLM integration, tool dispatch, and autonomous agent loop.

/// Background agent loop: periodic ticks and proactive trigger evaluation.
pub mod agent_loop;
/// Wire-format types for LLM API requests/responses.
pub mod api_types;
/// Context assembly for LLM prompts (memory retrieval, budget trimming).
pub mod context;
/// Rule-based decision routing (fast-path vs LLM).
pub mod decision;
/// Core reasoning engine orchestrating LLM calls, tool use, and memory.
pub mod engine;
/// Structured extraction of facts, goals, and emotions from conversation.
pub mod extraction;
/// LLM client trait and shared types.
pub mod llm;
/// Metacognitive self-monitoring (confidence calibration, strategy reflection).
pub mod metacognition;
/// System prompt templates and persona injection.
pub mod prompts;
/// LLM provider implementations (Anthropic, OpenAI, Ollama, Mock).
pub mod providers;
/// Retry logic with exponential backoff for transient LLM errors.
pub mod retry;
/// Multi-model task-based routing.
pub mod router;
/// Presence scheduling (active hours, dynamic tick/trigger intervals).
pub mod scheduler;
/// LRU+TTL response cache for deduplicating repeated queries.
pub mod response_cache;
/// Daily/monthly token budget tracking and enforcement.
pub mod token_budget;
/// Tool registry mapping names to handlers with safety gating.
pub mod tool_registry;
/// Built-in tool handler implementations (shell, memory, config, reading).
pub mod tools;

pub use decision::{DecisionLevel, DecisionRouter, DecisionRule};
pub use engine::ReasoningEngine;
pub use engine::{is_silence_response, sanitize_tool_result};
pub use engine::{ToolErrorKind, ToolOutcome};
pub use engine::LlmDreamNarrator;
pub use scheduler::PresenceScheduler;
pub use tool_registry::{ToolHandler, ToolRegistry};
pub use engine::RuntimeParams;
pub use router::ModelRouter;
pub use tools::{ConfigToolHandler, MemoryToolHandler, ReadingToolHandler, ShellToolHandler};
