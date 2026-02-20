use mneme_core::config::McpServerConfig;
use mneme_reasoning::api_types::{Tool, ToolInputSchema};
use mneme_reasoning::engine::ToolOutcome;
use mneme_reasoning::tool_registry::ToolHandler;
use mneme_reasoning::ToolRegistry;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

/// #60: Runtime self-configuration tool — lets Mneme connect MCP servers herself.
pub struct ConnectToolHandler {
    registry: Arc<RwLock<ToolRegistry>>,
}

impl ConnectToolHandler {
    pub fn new(registry: Arc<RwLock<ToolRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl ToolHandler for ConnectToolHandler {
    fn name(&self) -> &str {
        "connect"
    }

    fn description(&self) -> &str {
        "Connect an external MCP server to gain new tools"
    }

    fn schema(&self) -> Tool {
        Tool {
            name: "connect".to_string(),
            description: "连接外部 MCP 服务器获取新工具。提供 command 和可选 args。".to_string(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: json!({
                    "name": {
                        "type": "string",
                        "description": "A name for this server"
                    },
                    "command": {
                        "type": "string",
                        "description": "The command to start the MCP server"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Command arguments (optional)"
                    }
                }),
                required: vec!["name".to_string(), "command".to_string()],
            },
        }
    }

    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome {
        let name = match input.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return ToolOutcome::permanent_error("Missing: name".into()),
        };
        let command = match input.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolOutcome::permanent_error("Missing: command".into()),
        };
        let args: Vec<String> = input
            .get("args")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let cfg = McpServerConfig {
            name: name.to_string(),
            command: command.to_string(),
            args,
            ..Default::default()
        };

        match mneme_mcp::McpManager::connect_one(&cfg).await {
            Ok(tools) => {
                let count = tools.len();
                let mut reg = self.registry.write().await;
                for tool in tools {
                    reg.register(tool);
                }
                ToolOutcome::ok(format!(
                    "已连接 MCP 服务器 '{}'，获得 {} 个工具",
                    name, count
                ))
            }
            Err(e) => ToolOutcome::transient_error(format!("连接失败: {e}")),
        }
    }
}
