use mneme_core::safety::CapabilityGuard;
use mneme_core::tools::{Tool, ToolErrorKind, ToolOutcome};
use std::collections::HashMap;
use std::sync::Arc;

// ToolHandler trait lives in mneme_core::tools (v2.0.0 Phase 5a decoupling).
pub use mneme_core::tools::ToolHandler;

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

    /// Dispatch a tool call by name. All calls are audit-logged.
    pub async fn dispatch(&self, name: &str, input: &serde_json::Value) -> ToolOutcome {
        // #458: Destructive operation confirmation gate
        if name == "shell" {
            if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                if let Some(guard) = &self.guard {
                    if guard.needs_confirmation(cmd) {
                        tracing::warn!(tool = name, command = cmd, "AUDIT destructive_blocked — needs confirmation");
                        return ToolOutcome::permanent_error(format!(
                            "⚠ 破坏性操作需要确认：`{}`。请先向用户确认后再执行。",
                            cmd
                        ));
                    }
                }
            }
        }

        let start = std::time::Instant::now();
        let outcome = match self.handlers.get(name) {
            Some(handler) => handler.execute(input).await,
            None => ToolOutcome {
                content: format!("Unknown tool: {}", name),
                is_error: true,
                error_kind: Some(ToolErrorKind::Permanent),
            },
        };
        let elapsed_ms = start.elapsed().as_millis();
        let input_summary: String = input.to_string().chars().take(200).collect();
        if outcome.is_error {
            tracing::warn!(
                tool = name,
                elapsed_ms = elapsed_ms,
                input = %input_summary,
                error_kind = ?outcome.error_kind,
                "AUDIT tool_call FAILED"
            );
        } else {
            tracing::info!(
                tool = name,
                elapsed_ms = elapsed_ms,
                input = %input_summary,
                "AUDIT tool_call OK"
            );
        }
        outcome
    }

    /// Get a reference to the safety guard (if set).
    pub fn guard(&self) -> Option<&Arc<CapabilityGuard>> {
        self.guard.as_ref()
    }
}
