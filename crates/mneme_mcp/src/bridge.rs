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

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{Annotated, RawTextContent};
    use std::borrow::Cow;

    /// Helper: build an rmcp Tool for testing schema conversion.
    fn make_rmcp_tool(name: &str, desc: &str, schema: serde_json::Value) -> rmcp::model::Tool {
        let map = schema.as_object().cloned().unwrap_or_default();
        rmcp::model::Tool {
            name: Cow::Owned(name.to_string()),
            title: None,
            description: Some(Cow::Owned(desc.to_string())),
            input_schema: Arc::new(map),
            output_schema: None,
            annotations: None,
            execution: None,
            icons: None,
            meta: None,
        }
    }

    /// Helper: build a text Content block for CallToolResult.
    fn text_content(s: &str) -> rmcp::model::Content {
        Annotated {
            raw: RawContent::Text(RawTextContent {
                text: s.to_string(),
                meta: None,
            }),
            annotations: None,
        }
    }

    // --- Schema conversion tests ---

    #[test]
    fn test_schema_conversion_basic() {
        let schema = json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"}
            },
            "required": ["query"]
        });
        let rmcp_tool = make_rmcp_tool("search", "Search the web", schema);
        let tool = convert_mcp_tool_schema(&rmcp_tool);

        assert_eq!(tool.name, "search");
        assert_eq!(tool.description, "Search the web");
        assert_eq!(tool.input_schema.schema_type, "object");
        assert!(tool.input_schema.properties["query"].is_object());
        assert_eq!(tool.input_schema.required, vec!["query"]);
    }

    #[test]
    fn test_schema_conversion_no_required() {
        let schema = json!({"type": "object", "properties": {}});
        let rmcp_tool = make_rmcp_tool("noop", "Do nothing", schema);
        let tool = convert_mcp_tool_schema(&rmcp_tool);

        assert!(tool.input_schema.required.is_empty());
    }

    #[test]
    fn test_schema_conversion_no_properties() {
        let schema = json!({"type": "object"});
        let rmcp_tool = make_rmcp_tool("bare", "Bare tool", schema);
        let tool = convert_mcp_tool_schema(&rmcp_tool);

        assert_eq!(tool.input_schema.properties, json!({}));
    }

    // --- convert_call_result tests ---

    #[test]
    fn test_call_result_success_text() {
        let result = CallToolResult::success(vec![text_content("hello world")]);
        let outcome = convert_call_result(result);

        assert!(!outcome.is_error);
        assert_eq!(outcome.content, "hello world");
        assert!(outcome.error_kind.is_none());
    }

    #[test]
    fn test_call_result_multiple_text_blocks() {
        let result = CallToolResult::success(vec![
            text_content("line 1"),
            text_content("line 2"),
        ]);
        let outcome = convert_call_result(result);

        assert_eq!(outcome.content, "line 1\nline 2");
    }

    #[test]
    fn test_call_result_empty_content() {
        let result = CallToolResult::success(vec![]);
        let outcome = convert_call_result(result);

        assert_eq!(outcome.content, "[no output]");
    }

    #[test]
    fn test_call_result_error() {
        let result = CallToolResult::error(vec![text_content("bad input")]);
        let outcome = convert_call_result(result);

        assert!(outcome.is_error);
        assert_eq!(outcome.content, "bad input");
        assert_eq!(outcome.error_kind, Some(ToolErrorKind::Permanent));
    }

    // --- Disconnected peer tests ---

    #[tokio::test]
    async fn test_execute_disconnected_peer() {
        let schema = json!({"type": "object", "properties": {}});
        let rmcp_tool = make_rmcp_tool("test_tool", "A test", schema);
        let peer = Arc::new(RwLock::new(None)); // disconnected
        let handler = McpToolHandler::from_mcp_tool(&rmcp_tool, peer, "test-server");

        let outcome = handler.execute(&json!({})).await;

        assert!(outcome.is_error);
        assert!(outcome.content.contains("disconnected"));
        assert!(outcome.content.contains("test-server"));
        assert_eq!(outcome.error_kind, Some(ToolErrorKind::Transient));
    }

    #[tokio::test]
    async fn test_from_mcp_tool_accessors() {
        let schema = json!({
            "type": "object",
            "properties": {"x": {"type": "number"}}
        });
        let rmcp_tool = make_rmcp_tool("calc", "Calculator", schema);
        let peer = Arc::new(RwLock::new(None));
        let handler = McpToolHandler::from_mcp_tool(&rmcp_tool, peer, "math-server");

        assert_eq!(handler.name(), "calc");
        assert_eq!(handler.description(), "Calculator");
        let s = handler.schema();
        assert_eq!(s.name, "calc");
    }
}
