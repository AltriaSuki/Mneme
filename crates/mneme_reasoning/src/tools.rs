use crate::api_types::{Tool, ToolInputSchema};
use crate::engine::{ToolErrorKind, ToolOutcome};
use crate::tool_registry::ToolHandler;
use mneme_memory::SqliteMemory;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

/// JSON schema for the shell tool.
pub fn shell_tool() -> Tool {
    Tool {
        name: "shell".to_string(),
        description: "Execute a shell command on the local OS. Use this to explore files, run programs, access the network (curl, wget), install packages, write scripts, and do anything a human could do in a terminal. You MUST provide the input as JSON with a \"command\" key, e.g. {\"command\": \"ls -la\"}".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: json!({
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                }
            }),
            required: vec!["command".to_string()],
        },
    }
}

/// The one hardcoded tool — her hands.
///
/// Shell is not a "tool" in the plugin sense. It is a body organ:
/// the fundamental ability to act on the world. Everything else
/// (browser, RSS, MCP servers) can be obtained through shell.
pub struct ShellToolHandler {
    timeout: Duration,
}

impl ShellToolHandler {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for ShellToolHandler {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command"
    }

    fn schema(&self) -> Tool {
        shell_tool()
    }

    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome {
        let command = match input.get("command").and_then(|v| v.as_str()) {
            Some(cmd) => cmd,
            None => {
                return ToolOutcome {
                    content: "Missing required parameter: \"command\"".to_string(),
                    is_error: true,
                    error_kind: Some(ToolErrorKind::Permanent),
                };
            }
        };

        let child = match Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                return ToolOutcome {
                    content: format!("Failed to spawn command: {e}"),
                    is_error: true,
                    error_kind: Some(ToolErrorKind::Transient),
                };
            }
        };

        let output = match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return ToolOutcome {
                    content: format!("Command execution error: {e}"),
                    is_error: true,
                    error_kind: Some(ToolErrorKind::Transient),
                };
            }
            Err(_) => {
                return ToolOutcome {
                    content: format!("Command timed out after {:?}", self.timeout),
                    is_error: true,
                    error_kind: Some(ToolErrorKind::Transient),
                };
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            let content = if stderr.is_empty() {
                stdout.to_string()
            } else {
                format!("{stdout}\n[stderr]: {stderr}")
            };
            ToolOutcome {
                content: if content.is_empty() {
                    "[no output]".to_string()
                } else {
                    content
                },
                is_error: false,
                error_kind: None,
            }
        } else {
            ToolOutcome {
                content: format!(
                    "Exit {}\n{stderr}\n{stdout}",
                    output.status.code().unwrap_or(-1)
                ),
                is_error: true,
                error_kind: Some(ToolErrorKind::Permanent),
            }
        }
    }
}

/// #84: Memory management tool — lets Mneme pin, unpin, forget, and list important memories.
pub struct MemoryToolHandler {
    db: Arc<SqliteMemory>,
}

impl MemoryToolHandler {
    pub fn new(db: Arc<SqliteMemory>) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl ToolHandler for MemoryToolHandler {
    fn name(&self) -> &str {
        "memory_manage"
    }

    fn description(&self) -> &str {
        "Manage your own memories: pin important ones, forget painful ones, list pinned memories"
    }

    fn schema(&self) -> Tool {
        Tool {
            name: "memory_manage".to_string(),
            description: "管理自己的记忆。action: pin/unpin/forget/list_pinned。pin/unpin/forget 需要 episode_id。".to_string(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: json!({
                    "action": {
                        "type": "string",
                        "enum": ["pin", "unpin", "forget", "list_pinned"],
                        "description": "The memory management action"
                    },
                    "episode_id": {
                        "type": "string",
                        "description": "Episode UUID (required for pin/unpin/forget)"
                    }
                }),
                required: vec!["action".to_string()],
            },
        }
    }

    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome {
        let action = match input.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return ToolOutcome::permanent_error("Missing required parameter: action".into()),
        };

        match action {
            "list_pinned" => match self.db.list_pinned_episodes(20).await {
                Ok(eps) if eps.is_empty() => ToolOutcome::ok("没有固定的记忆。".into()),
                Ok(eps) => {
                    let lines: Vec<String> = eps.iter()
                        .map(|(id, summary, s)| format!("[{}] (strength={:.2}) {}", &id[..8], s, summary))
                        .collect();
                    ToolOutcome::ok(lines.join("\n"))
                }
                Err(e) => ToolOutcome::transient_error(format!("DB error: {e}")),
            },
            "pin" | "unpin" | "forget" => {
                let id = match input.get("episode_id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => return ToolOutcome::permanent_error("episode_id required".into()),
                };
                let result = match action {
                    "pin" => self.db.pin_episode(id).await,
                    "unpin" => self.db.unpin_episode(id).await,
                    "forget" => self.db.forget_episode(id).await,
                    _ => unreachable!(),
                };
                match result {
                    Ok(true) => ToolOutcome::ok(format!("{action} 成功: {}", &id[..id.len().min(8)])),
                    Ok(false) => ToolOutcome::permanent_error(format!("Episode not found: {id}")),
                    Err(e) => ToolOutcome::transient_error(format!("DB error: {e}")),
                }
            }
            _ => ToolOutcome::permanent_error(format!("Unknown action: {action}")),
        }
    }
}
