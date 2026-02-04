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

pub fn browser_goto_tool() -> Tool {
    Tool {
        name: "browser_goto".to_string(),
        description: "Navigate the browser to a specific URL.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: json!({
                "url": {
                    "type": "string",
                    "description": "The URL to navigate to (e.g. https://google.com)"
                }
            }),
            required: vec!["url".to_string()],
        },
    }
}

pub fn browser_click_tool() -> Tool {
    Tool {
        name: "browser_click".to_string(),
        description: "Click an element on the current page specified by a CSS selector.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: json!({
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the element to click (e.g. #btn-submit)"
                }
            }),
            required: vec!["selector".to_string()],
        },
    }
}

pub fn browser_type_tool() -> Tool {
    Tool {
        name: "browser_type".to_string(),
        description: "Type text into an input field.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: json!({
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the input element"
                },
                "text": {
                    "type": "string",
                    "description": "The text to type"
                }
            }),
            required: vec!["selector".to_string(), "text".to_string()],
        },
    }
}

pub fn browser_screenshot_tool() -> Tool {
    Tool {
        name: "browser_screenshot".to_string(),
        description: "Capture a screenshot of the current viewport.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: json!({}),
            required: vec![],
        },
    }
}

pub fn browser_get_html_tool() -> Tool {
    Tool {
        name: "browser_get_html".to_string(),
        description: "Get the HTML content of the current page. Useful for getting context before taking actions.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: json!({}),
            required: vec![],
        },
    }
}
