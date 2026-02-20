use crate::api_types::{ContentBlock, Message, Role, Tool, ToolInputSchema};
use crate::engine::{RuntimeParams, ToolErrorKind, ToolOutcome};
use crate::llm::{CompletionParams, LlmClient};
use crate::tool_registry::ToolHandler;
use mneme_core::OrganismState;
use mneme_memory::SqliteMemory;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::RwLock;

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

/// #86: Runtime parameter self-modification tool — lets Mneme adjust her own LLM parameters.
pub struct ConfigToolHandler {
    params: Arc<RuntimeParams>,
}

impl ConfigToolHandler {
    pub fn new(params: Arc<RuntimeParams>) -> Self {
        Self { params }
    }
}

#[async_trait::async_trait]
impl ToolHandler for ConfigToolHandler {
    fn name(&self) -> &str {
        "config"
    }

    fn description(&self) -> &str {
        "Adjust your own reasoning parameters (temperature, max_tokens)"
    }

    fn schema(&self) -> Tool {
        Tool {
            name: "config".to_string(),
            description: "调整自己的推理参数。action: get/set_temperature/set_max_tokens。".to_string(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: json!({
                    "action": {
                        "type": "string",
                        "enum": ["get", "set_temperature", "set_max_tokens"],
                        "description": "The config action"
                    },
                    "value": {
                        "type": "number",
                        "description": "New value (required for set_* actions)"
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
            "get" => ToolOutcome::ok(format!(
                "temperature={:.2}, max_tokens={}",
                self.params.temperature(),
                self.params.max_tokens()
            )),
            "set_temperature" => {
                let v = match input.get("value").and_then(|v| v.as_f64()) {
                    Some(v) => v as f32,
                    None => return ToolOutcome::permanent_error("value required".into()),
                };
                self.params.set_temperature(v);
                ToolOutcome::ok(format!("temperature → {:.2}", self.params.temperature()))
            }
            "set_max_tokens" => {
                let v = match input.get("value").and_then(|v| v.as_u64()) {
                    Some(v) => v as u32,
                    None => return ToolOutcome::permanent_error("value required (integer)".into()),
                };
                self.params.set_max_tokens(v);
                ToolOutcome::ok(format!("max_tokens → {}", self.params.max_tokens()))
            }
            _ => ToolOutcome::permanent_error(format!("Unknown action: {action}")),
        }
    }
}

/// #56: Literary reading pipeline — lets Mneme read material and generate state-dependent reflections.
pub struct ReadingToolHandler {
    llm: Arc<dyn LlmClient>,
    db: Arc<SqliteMemory>,
    state: Arc<RwLock<OrganismState>>,
}

impl ReadingToolHandler {
    pub fn new(
        llm: Arc<dyn LlmClient>,
        db: Arc<SqliteMemory>,
        state: Arc<RwLock<OrganismState>>,
    ) -> Self {
        Self { llm, db, state }
    }

    async fn reflect_and_store(&self, title: &str, text: &str) -> ToolOutcome {
        let st = self.state.read().await;
        let mood = st.fast.affect.describe();
        let prompt = format!(
            "你正在阅读「{}」。你当前的内心状态：情绪={}，精力={:.0}%，压力={:.0}%。\n\n\
             以下是阅读材料（节选）：\n{}\n\n\
             请写出你的阅读感想。不需要总结内容，而是写出这段文字让你联想到什么、触动了什么、\
             与你自身经历或价值观的共鸣或冲突。用第一人称，简短真实。",
            title, mood, st.fast.energy * 100.0, st.fast.stress * 100.0,
            &text[..text.len().min(6000)]
        );
        drop(st);

        let messages = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: prompt }],
        }];
        let params = CompletionParams { max_tokens: 512, temperature: 0.9 };

        let resp = match self.llm.complete("", messages, vec![], params).await {
            Ok(r) => r,
            Err(e) => return ToolOutcome::transient_error(format!("LLM reflection failed: {e}")),
        };

        let reflection: String = resp.content.iter().filter_map(|b| {
            if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
        }).collect();

        if reflection.trim().is_empty() {
            return ToolOutcome::ok("（阅读后没有特别的感想）".into());
        }

        let claim = format!("读「{}」后的感想：{}", title, reflection.trim());
        if let Err(e) = self.db.store_self_knowledge("reading", &claim, 0.5, "self:reading", None).await {
            tracing::warn!("Failed to store reading reflection: {}", e);
        }

        ToolOutcome::ok(reflection)
    }
}

#[async_trait::async_trait]
impl ToolHandler for ReadingToolHandler {
    fn name(&self) -> &str { "read_literature" }
    fn description(&self) -> &str { "Read literary material and reflect on it" }

    fn schema(&self) -> Tool {
        Tool {
            name: "read_literature".to_string(),
            description: "阅读文学作品并生成反思。path: 本地文件路径；text: 直接提供文本；title: 作品标题。".to_string(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: json!({
                    "path": { "type": "string", "description": "Local file path to read" },
                    "text": { "type": "string", "description": "Text content to reflect on (alternative to path)" },
                    "title": { "type": "string", "description": "Title of the work" }
                }),
                required: vec!["title".to_string()],
            },
        }
    }

    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome {
        let title = match input.get("title").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return ToolOutcome::permanent_error("Missing: title".into()),
        };

        let text = if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
            match tokio::fs::read_to_string(path).await {
                Ok(c) => c,
                Err(e) => return ToolOutcome::permanent_error(format!("Cannot read file: {e}")),
            }
        } else if let Some(t) = input.get("text").and_then(|v| v.as_str()) {
            t.to_string()
        } else {
            return ToolOutcome::permanent_error("Provide either 'path' or 'text'".into());
        };

        if text.trim().is_empty() {
            return ToolOutcome::permanent_error("Empty text".into());
        }

        self.reflect_and_store(title, &text).await
    }
}
