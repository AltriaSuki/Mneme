//! Tool abstraction types — shared between reasoning engine and MCP bridge.
//!
//! Moved here from mneme_reasoning to break the reverse dependency
//! mneme_mcp → mneme_reasoning (v2.0.0 Phase 5a).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON tool definition sent to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: ToolInputSchema,
}

/// JSON Schema for tool input parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: Value,
    pub required: Vec<String>,
}

/// Classification of tool execution errors.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolErrorKind {
    /// Transient: timeout, connection reset — worth retrying.
    Transient,
    /// Permanent: missing param, unknown tool — retrying won't help.
    Permanent,
}

/// Structured result from a tool execution.
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub content: String,
    pub is_error: bool,
    pub error_kind: Option<ToolErrorKind>,
}

impl ToolOutcome {
    pub fn ok(content: String) -> Self {
        Self { content, is_error: false, error_kind: None }
    }

    pub fn transient_error(msg: String) -> Self {
        Self { content: msg, is_error: true, error_kind: Some(ToolErrorKind::Transient) }
    }

    pub fn permanent_error(msg: String) -> Self {
        Self { content: msg, is_error: true, error_kind: Some(ToolErrorKind::Permanent) }
    }
}

/// Trait for tool handlers that can be registered and dispatched.
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    /// Unique name used for dispatch (must match the tool name in schema).
    fn name(&self) -> &str;
    /// Human-readable description for logging.
    fn description(&self) -> &str;
    /// JSON schema sent to the LLM so it knows how to call this tool.
    fn schema(&self) -> Tool;
    /// Execute the tool with the given JSON input.
    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome;
}
