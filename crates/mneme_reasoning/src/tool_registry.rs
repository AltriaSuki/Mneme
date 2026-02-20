use crate::api_types::Tool;
use crate::engine::{ToolErrorKind, ToolOutcome};
use mneme_core::safety::CapabilityGuard;
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// ToolHandler trait
// ============================================================================

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

// ============================================================================
// ToolRegistry
// ============================================================================

pub struct ToolRegistry {
    handlers: HashMap<String, Box<dyn ToolHandler>>,
    guard: Option<Arc<CapabilityGuard>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            guard: None,
        }
    }

    pub fn with_guard(guard: Arc<CapabilityGuard>) -> Self {
        Self {
            handlers: HashMap::new(),
            guard: Some(guard),
        }
    }

    /// Register a tool handler. Overwrites any existing handler with the same name.
    pub fn register(&mut self, handler: Box<dyn ToolHandler>) {
        let name = handler.name().to_string();
        tracing::debug!("Registered tool: {}", name);
        self.handlers.insert(name, handler);
    }

    /// Get the list of Tool schemas for the LLM.
    pub fn available_tools(&self) -> Vec<Tool> {
        self.handlers.values().map(|h| h.schema()).collect()
    }

    /// Dispatch a tool call by name.
    pub async fn dispatch(&self, name: &str, input: &serde_json::Value) -> ToolOutcome {
        match self.handlers.get(name) {
            Some(handler) => handler.execute(input).await,
            None => ToolOutcome {
                content: format!("Unknown tool: {}", name),
                is_error: true,
                error_kind: Some(ToolErrorKind::Permanent),
            },
        }
    }

    /// Get a reference to the safety guard (if set).
    pub fn guard(&self) -> Option<&Arc<CapabilityGuard>> {
        self.guard.as_ref()
    }
}
