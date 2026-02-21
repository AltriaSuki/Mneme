use mneme_reasoning::api_types::{Tool, ToolInputSchema};
use mneme_reasoning::engine::ToolOutcome;
use mneme_reasoning::tool_registry::{ToolHandler, ToolRegistry};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Phase 5b-4: Tool Composition (B-8 Level 3).
///
/// Lets Mneme chain multiple tools in a pipeline. Each step's input
/// can reference `{{prev}}` to inject the previous step's output.
pub struct ComposeToolHandler {
    registry: Arc<RwLock<ToolRegistry>>,
}

impl ComposeToolHandler {
    pub fn new(registry: Arc<RwLock<ToolRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl ToolHandler for ComposeToolHandler {
    fn name(&self) -> &str {
        "compose"
    }

    fn description(&self) -> &str {
        "Chain multiple tools in a pipeline, passing results between steps"
    }

    fn schema(&self) -> Tool {
        Tool {
            name: "compose".to_string(),
            description: "组合多个工具形成管道。steps 数组中每个元素有 tool 和 input，input 中的 {{prev}} 会被替换为上一步的输出。".to_string(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: json!({
                    "steps": {
                        "type": "array",
                        "description": "Pipeline steps. Each has 'tool' (name) and 'input' (JSON object). Use {{prev}} in string values to reference previous output.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "tool": { "type": "string" },
                                "input": { "type": "object" }
                            },
                            "required": ["tool", "input"]
                        }
                    }
                }),
                required: vec!["steps".to_string()],
            },
        }
    }

    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome {
        let steps = match input.get("steps").and_then(|v| v.as_array()) {
            Some(s) if !s.is_empty() => s,
            _ => return ToolOutcome::permanent_error("Missing or empty: steps".into()),
        };

        if steps.len() > 5 {
            return ToolOutcome::permanent_error("Maximum 5 steps per pipeline".into());
        }

        let mut prev_output = String::new();
        let registry = self.registry.read().await;

        for (i, step) in steps.iter().enumerate() {
            let tool_name = match step.get("tool").and_then(|v| v.as_str()) {
                Some(n) => n,
                None => return ToolOutcome::permanent_error(
                    format!("Step {}: missing 'tool' name", i),
                ),
            };

            // Don't allow recursive compose calls
            if tool_name == "compose" {
                return ToolOutcome::permanent_error("Recursive compose not allowed".into());
            }

            let step_input = match step.get("input") {
                Some(v) => substitute_prev(v, &prev_output),
                None => json!({}),
            };

            let outcome = registry.dispatch(tool_name, &step_input).await;
            if outcome.is_error {
                return ToolOutcome::permanent_error(
                    format!("Step {} ({}) failed: {}", i, tool_name, outcome.content),
                );
            }
            prev_output = outcome.content;
        }

        ToolOutcome::ok(prev_output)
    }
}

/// Replace `{{prev}}` in all string values within a JSON value.
fn substitute_prev(value: &serde_json::Value, prev: &str) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            serde_json::Value::String(s.replace("{{prev}}", prev))
        }
        serde_json::Value::Object(map) => {
            let new_map: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), substitute_prev(v, prev)))
                .collect();
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| substitute_prev(v, prev)).collect())
        }
        other => other.clone(),
    }
}
