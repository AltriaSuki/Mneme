use crate::api_types::{Tool, ToolInputSchema};
use crate::engine::{ToolErrorKind, ToolOutcome};
use crate::tool_registry::ToolHandler;
use mneme_core::safety::CapabilityGuard;
use mneme_os::Executor;
use serde_json::json;
use std::sync::Arc;

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
        description: "Capture a screenshot of the current viewport. No parameters needed."
            .to_string(),
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

// ============================================================================
// ToolHandler implementations
// ============================================================================

/// Shell command execution handler with safety guard.
pub struct ShellToolHandler {
    pub executor: Arc<dyn Executor>,
    pub guard: Option<Arc<CapabilityGuard>>,
}

#[async_trait::async_trait]
impl ToolHandler for ShellToolHandler {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "Execute a shell command on the local OS"
    }
    fn schema(&self) -> Tool {
        shell_tool()
    }

    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome {
        // Lenient parsing: try multiple patterns for the command string.
        // Some models send {"command": "ls"}, others {"cmd": "ls"},
        // others just "ls" as a bare string, or put it in an unexpected key.
        let cmd = input
            .get("command")
            .and_then(|v| v.as_str())
            .or_else(|| input.get("cmd").and_then(|v| v.as_str()))
            .or_else(|| input.as_str())
            .or_else(|| {
                // Last resort: if the object has exactly one string value, use it
                input
                    .as_object()
                    .filter(|obj| obj.len() == 1)
                    .and_then(|obj| obj.values().next())
                    .and_then(|v| v.as_str())
            });

        let cmd = match cmd {
            Some(c) if !c.is_empty() => c,
            _ => {
                return ToolOutcome {
                    content: format!(
                        "ERROR: You called the shell tool but did not provide a command. \
                     You MUST provide input as: {{\"command\": \"<shell command>\"}}. \
                     For example, to list files: {{\"command\": \"ls -la\"}}. \
                     You sent: {}",
                        input
                    ),
                    is_error: true,
                    error_kind: Some(ToolErrorKind::Permanent),
                }
            }
        };

        // Safety guard check
        if let Some(ref guard) = self.guard {
            if let Err(denied) = guard.check_command(cmd) {
                return ToolOutcome {
                    content: format!("Safety: {}", denied),
                    is_error: true,
                    error_kind: Some(ToolErrorKind::Permanent),
                };
            }
        }

        match self.executor.execute(cmd).await {
            Ok(out) => ToolOutcome {
                content: out,
                is_error: false,
                error_kind: None,
            },
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("timed out") || msg.contains("spawn") {
                    ToolOutcome {
                        content: format!("Shell command failed (transient): {}", msg),
                        is_error: true,
                        error_kind: Some(ToolErrorKind::Transient),
                    }
                } else {
                    ToolOutcome {
                        content: format!("Shell command failed: {}", msg),
                        is_error: true,
                        error_kind: Some(ToolErrorKind::Permanent),
                    }
                }
            }
        }
    }
}

/// Generic browser tool handler that wraps a shared browser session.
pub struct BrowserToolHandler {
    pub session: Arc<tokio::sync::Mutex<Option<mneme_browser::BrowserClient>>>,
    tool_name: String,
    tool_schema: Tool,
}

impl BrowserToolHandler {
    pub fn goto(session: Arc<tokio::sync::Mutex<Option<mneme_browser::BrowserClient>>>) -> Self {
        Self {
            session,
            tool_name: "browser_goto".into(),
            tool_schema: browser_goto_tool(),
        }
    }
    pub fn click(session: Arc<tokio::sync::Mutex<Option<mneme_browser::BrowserClient>>>) -> Self {
        Self {
            session,
            tool_name: "browser_click".into(),
            tool_schema: browser_click_tool(),
        }
    }
    pub fn type_text(
        session: Arc<tokio::sync::Mutex<Option<mneme_browser::BrowserClient>>>,
    ) -> Self {
        Self {
            session,
            tool_name: "browser_type".into(),
            tool_schema: browser_type_tool(),
        }
    }
    pub fn screenshot(
        session: Arc<tokio::sync::Mutex<Option<mneme_browser::BrowserClient>>>,
    ) -> Self {
        Self {
            session,
            tool_name: "browser_screenshot".into(),
            tool_schema: browser_screenshot_tool(),
        }
    }
    pub fn get_html(
        session: Arc<tokio::sync::Mutex<Option<mneme_browser::BrowserClient>>>,
    ) -> Self {
        Self {
            session,
            tool_name: "browser_get_html".into(),
            tool_schema: browser_get_html_tool(),
        }
    }

    fn parse_action(
        &self,
        input: &serde_json::Value,
    ) -> Result<mneme_browser::BrowserAction, String> {
        use mneme_browser::BrowserAction;
        match self.tool_name.as_str() {
            "browser_goto" => input
                .get("url")
                .and_then(|u| u.as_str())
                .map(|url| BrowserAction::Goto {
                    url: url.to_string(),
                })
                .ok_or_else(|| "Missing 'url' parameter".to_string()),
            "browser_click" => input
                .get("selector")
                .and_then(|s| s.as_str())
                .map(|sel| BrowserAction::Click {
                    selector: sel.to_string(),
                })
                .ok_or_else(|| "Missing 'selector' parameter".to_string()),
            "browser_type" => {
                let sel = input.get("selector").and_then(|s| s.as_str());
                let txt = input.get("text").and_then(|t| t.as_str());
                match (sel, txt) {
                    (Some(s), Some(t)) => Ok(BrowserAction::Type {
                        selector: s.to_string(),
                        text: t.to_string(),
                    }),
                    _ => Err("Missing 'selector' or 'text'".to_string()),
                }
            }
            "browser_screenshot" => Ok(BrowserAction::Screenshot),
            "browser_get_html" => Ok(BrowserAction::GetHtml),
            _ => Err(format!("Unknown browser tool: {}", self.tool_name)),
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for BrowserToolHandler {
    fn name(&self) -> &str {
        &self.tool_name
    }
    fn description(&self) -> &str {
        "Browser automation tool"
    }
    fn schema(&self) -> Tool {
        self.tool_schema.clone()
    }

    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome {
        let action = match self.parse_action(input) {
            Ok(a) => a,
            Err(msg) => {
                return ToolOutcome {
                    content: msg,
                    is_error: true,
                    error_kind: Some(ToolErrorKind::Permanent),
                }
            }
        };

        let mut session = self.session.lock().await;

        // Health check (blocking CDP call → spawn_blocking)
        if let Some(client) = session.take() {
            let alive = tokio::task::spawn_blocking(move || {
                let alive = client.is_alive();
                (client, alive)
            })
            .await
            .unwrap();
            if alive.1 {
                *session = Some(alive.0);
            }
            // else: dead session dropped
        }

        // Ensure session exists (blocking Browser::new → spawn_blocking)
        if session.is_none() {
            match tokio::task::spawn_blocking(create_browser_session)
                .await
                .unwrap()
            {
                Ok(c) => {
                    *session = Some(c);
                }
                Err(e) => {
                    return ToolOutcome {
                        content: format!("Failed to launch browser: {}", e),
                        is_error: true,
                        error_kind: Some(ToolErrorKind::Transient),
                    }
                }
            }
        }

        // Execute action (blocking CDP call → spawn_blocking)
        if let Some(mut client) = session.take() {
            let act = action.clone();
            let (client, result) = tokio::task::spawn_blocking(move || {
                let r = client.execute_action(act);
                (client, r)
            })
            .await
            .unwrap();

            match result {
                Ok(out) => {
                    *session = Some(client);
                    return ToolOutcome {
                        content: out,
                        is_error: false,
                        error_kind: None,
                    };
                }
                Err(e) => {
                    tracing::warn!("Browser action failed, recovering: {}", e);
                    // Drop dead client, fall through to recovery
                }
            }
        }

        // Recovery attempt (blocking → spawn_blocking)
        let recovery = tokio::task::spawn_blocking(move || {
            let mut client = create_browser_session()?;
            let result = client.execute_action(action);
            Ok::<_, anyhow::Error>((client, result))
        })
        .await
        .unwrap();

        match recovery {
            Ok((client, result)) => {
                *session = Some(client);
                match result {
                    Ok(out) => ToolOutcome {
                        content: out,
                        is_error: false,
                        error_kind: None,
                    },
                    Err(e) => ToolOutcome {
                        content: format!("Browser failed after recovery: {}", e),
                        is_error: true,
                        error_kind: Some(ToolErrorKind::Transient),
                    },
                }
            }
            Err(e) => ToolOutcome {
                content: format!("Browser recovery failed: {}", e),
                is_error: true,
                error_kind: Some(ToolErrorKind::Transient),
            },
        }
    }
}

fn create_browser_session() -> anyhow::Result<mneme_browser::BrowserClient> {
    let mut client = mneme_browser::BrowserClient::new(true)?;
    client.launch()?;
    Ok(client)
}

/// Ping an existing browser session to prevent idle timeout.
///
/// Call this periodically (e.g. on each organism tick) to keep Chrome's
/// DevTools connection alive. No-op if no session exists.
pub async fn browser_keepalive(session: &tokio::sync::Mutex<Option<mneme_browser::BrowserClient>>) {
    let mut guard = session.lock().await;
    if let Some(client) = guard.take() {
        let (client, alive) = tokio::task::spawn_blocking(move || {
            let alive = client.keepalive();
            (client, alive)
        })
        .await
        .unwrap_or_else(|_| {
            // JoinError — treat as dead
            // We can't recover the client here, so return a dummy
            // This shouldn't happen in practice
            panic!("browser keepalive spawn_blocking panicked");
        });
        if alive {
            *guard = Some(client);
        } else {
            tracing::debug!("Browser session died during keepalive, will recreate on next use");
        }
    }
}
