use mneme_core::tools::{Tool, ToolHandler, ToolInputSchema, ToolErrorKind, ToolOutcome};
use rmcp::model::{CallToolRequestParams, CallToolResult, RawContent};
use rmcp::service::{Peer, RoleClient};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Bridges a single MCP tool to the ToolHandler trait.
///
/// Each discovered MCP tool gets one McpToolHandler instance registered
/// in the ToolRegistry. The shared Peer handle is lifecycle-aware:
/// set to None when the organism sleeps, restored on wake.
pub struct McpToolHandler {
    tool_name: String,
    tool_description: String,
    tool_schema: Tool,
    peer: Arc<RwLock<Option<Peer<RoleClient>>>>,
    server_name: String,
}

impl McpToolHandler {
    /// Create from an rmcp Tool discovered via list_tools().
    pub fn from_mcp_tool(
        mcp_tool: &rmcp::model::Tool,
        peer: Arc<RwLock<Option<Peer<RoleClient>>>>,
        server_name: &str,
    ) -> Self {
        let name = mcp_tool.name.to_string();
        let description = mcp_tool
            .description
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default();

        // Convert MCP tool schema → api_types::Tool schema
        let schema = convert_mcp_tool_schema(mcp_tool);

        Self {
            tool_name: name,
            tool_description: description,
            tool_schema: schema,
            peer,
            server_name: server_name.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl ToolHandler for McpToolHandler {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn schema(&self) -> Tool {
        self.tool_schema.clone()
    }

    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome {
        let peer_guard = self.peer.read().await;
        let Some(peer) = peer_guard.as_ref() else {
            return ToolOutcome {
                content: format!(
                    "MCP server '{}' is disconnected (organism may be sleeping)",
                    self.server_name
                ),
                is_error: true,
                error_kind: Some(ToolErrorKind::Transient),
            };
        };

        let arguments = input.as_object().cloned();

        let params = CallToolRequestParams {
            meta: None,
            name: self.tool_name.clone().into(),
            arguments,
            task: None,
        };

        match peer.call_tool(params).await {
            Ok(result) => convert_call_result(result),
            Err(e) => {
                let msg = e.to_string();
                let kind = if msg.contains("closed") || msg.contains("timeout") {
                    ToolErrorKind::Transient
                } else {
                    ToolErrorKind::Permanent
                };
                ToolOutcome {
                    content: format!("MCP tool '{}' failed: {}", self.tool_name, msg),
                    is_error: true,
                    error_kind: Some(kind),
                }
            }
        }
    }
}

/// Convert MCP CallToolResult → ToolOutcome.
fn convert_call_result(result: CallToolResult) -> ToolOutcome {
    let is_error = result.is_error.unwrap_or(false);

    // Concatenate all text content blocks
    let content: String = result
        .content
        .iter()
        .filter_map(|c| match &c.raw {
            RawContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    let content = if content.is_empty() {
        "[no output]".to_string()
    } else {
        content
    };

    ToolOutcome {
        content,
        is_error,
        error_kind: if is_error {
            Some(ToolErrorKind::Permanent)
        } else {
            None
        },
    }
}

/// Convert rmcp::model::Tool → api_types::Tool for the LLM.
fn convert_mcp_tool_schema(mcp_tool: &rmcp::model::Tool) -> Tool {
    let input_schema = &mcp_tool.input_schema;

    // Extract properties and required from the JSON schema object
    let properties = input_schema
        .get("properties")
        .cloned()
        .unwrap_or(json!({}));

    let required = input_schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Tool {
        name: mcp_tool.name.to_string(),
        description: mcp_tool
            .description
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties,
            required,
        },
    }
}
