use serde_json::json;
use crate::api_types::{Tool, ToolInputSchema};

/// Tool descriptions embed parameter specs directly in the text as a fallback.
/// Some API proxies strip `input_schema`, so the model must be able to infer
/// required parameters from the description alone.

pub fn shell_tool() -> Tool {
    Tool {
        name: "shell".to_string(),
        description: "Execute a shell command on the local OS. Use this to explore files, run git commands, check system status, etc. You MUST provide the input as JSON with a \"command\" key, e.g. {\"command\": \"ls -la\"}".to_string(),
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
        description: "Navigate the browser to a specific URL. You MUST provide the input as JSON with a \"url\" key, e.g. {\"url\": \"https://google.com\"}".to_string(),
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
        description: "Click an element on the current page by CSS selector. You MUST provide the input as JSON with a \"selector\" key, e.g. {\"selector\": \"#submit-btn\"}".to_string(),
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
        description: "Type text into an input field. You MUST provide the input as JSON with \"selector\" and \"text\" keys, e.g. {\"selector\": \"#search\", \"text\": \"hello\"}".to_string(),
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
        description: "Capture a screenshot of the current viewport. No parameters needed.".to_string(),
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
        description: "Get the HTML content of the current page. Useful for getting context before taking actions. No parameters needed.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: json!({}),
            required: vec![],
        },
    }
}
