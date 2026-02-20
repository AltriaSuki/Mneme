use mneme_expression::{ScheduleEntry, ScheduleHandle};
use mneme_reasoning::api_types::{Tool, ToolInputSchema};
use mneme_reasoning::engine::ToolOutcome;
use mneme_reasoning::tool_registry::ToolHandler;
use serde_json::json;

pub struct ScheduleToolHandler {
    handle: ScheduleHandle,
}

impl ScheduleToolHandler {
    pub fn new(handle: ScheduleHandle) -> Self {
        Self { handle }
    }
}

#[async_trait::async_trait]
impl ToolHandler for ScheduleToolHandler {
    fn name(&self) -> &str {
        "schedule"
    }

    fn description(&self) -> &str {
        "Manage your daily schedule (list/add/remove)"
    }

    fn schema(&self) -> Tool {
        Tool {
            name: "schedule".to_string(),
            description: "Manage your daily schedule. Actions: \"list\" (show all), \"add\" (create new entry), \"remove\" (delete by name).".to_string(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: json!({
                    "action": {
                        "type": "string",
                        "enum": ["list", "add", "remove"],
                        "description": "The action to perform"
                    },
                    "name": {
                        "type": "string",
                        "description": "Schedule entry name (required for add/remove)"
                    },
                    "hour": {
                        "type": "integer",
                        "description": "Hour 0-23 (required for add)"
                    },
                    "minute": {
                        "type": "integer",
                        "description": "Minute 0-59 (optional for add, default 0)"
                    },
                    "route": {
                        "type": "string",
                        "description": "Output route, e.g. 'onebot:group:12345' (optional for add)"
                    }
                }),
                required: vec!["action".to_string()],
            },
        }
    }

    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome {
        let action = match input.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return ToolOutcome::permanent_error("Missing required parameter: \"action\"".into()),
        };

        match action {
            "list" => ToolOutcome::ok(self.handle.list()),
            "add" => {
                let name = match input.get("name").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => return ToolOutcome::permanent_error("Missing \"name\" for add".into()),
                };
                let hour = match input.get("hour").and_then(|v| v.as_u64()) {
                    Some(h) => h as u32,
                    None => return ToolOutcome::permanent_error("Missing \"hour\" for add".into()),
                };
                let minute = input.get("minute").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let route = input.get("route").and_then(|v| v.as_str()).map(String::from);

                let mut entry = match ScheduleEntry::new(name, hour, minute) {
                    Ok(e) => e,
                    Err(e) => return ToolOutcome::permanent_error(format!("Invalid schedule: {e}")),
                };
                entry.route = route;

                match self.handle.add(entry) {
                    Ok(()) => ToolOutcome::ok(format!("Added schedule '{}' at {:02}:{:02}", name, hour, minute)),
                    Err(e) => ToolOutcome::permanent_error(e),
                }
            }
            "remove" => {
                let name = match input.get("name").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => return ToolOutcome::permanent_error("Missing \"name\" for remove".into()),
                };
                if self.handle.remove(name) {
                    ToolOutcome::ok(format!("Removed schedule '{}'", name))
                } else {
                    ToolOutcome::permanent_error(format!("Schedule '{}' not found", name))
                }
            }
            _ => ToolOutcome::permanent_error(format!("Unknown action: {action}")),
        }
    }
}
