use serde_json::json;
use crate::api_types::{Tool, ToolInputSchema};

pub fn shell_tool() -> Tool {
    Tool {
        name: "shell".to_string(),
        description: "Execute a shell command on the local OS. Use this to explore files, run git commands, check system status, etc.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: json!({
                "command": {
                    "type": "string",
                    "description": "The command line to execute (e.g., 'ls -la', 'git status')"
                }
            }),
            required: vec!["command".to_string()],
        },
    }
}

pub fn browser_action_tool() -> Tool {
    Tool {
        name: "browser_action".to_string(),
        description: "Control a web browser to navigate, click, type, and inspect pages.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: json!({
                "action": {
                    "type": "string",
                    "enum": ["goto", "click", "type", "screenshot", "get_html"],
                    "description": "The action to perform"
                },
                "url": {
                    "type": "string",
                    "description": "URL for 'goto' action"
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector for 'click' or 'type' actions"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type for 'type' action"
                }
            }),
            required: vec!["action".to_string()],
        },
    }
}
