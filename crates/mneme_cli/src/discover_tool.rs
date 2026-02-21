use mneme_reasoning::api_types::{Tool, ToolInputSchema};
use mneme_reasoning::engine::ToolOutcome;
use mneme_reasoning::tool_registry::{ToolHandler, ToolRegistry};
use mneme_core::config::McpServerConfig;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Known MCP server entry in the discovery catalog.
struct CatalogEntry {
    name: &'static str,
    description: &'static str,
    command: &'static str,
    args: &'static [&'static str],
    tags: &'static [&'static str],
}

/// Built-in catalog of discoverable MCP servers (ADR-014 Layer 3).
static CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        name: "filesystem",
        description: "Read/write local files and directories",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
        tags: &["file", "read", "write", "directory", "filesystem"],
    },
    CatalogEntry {
        name: "fetch",
        description: "Fetch web pages and convert to readable text",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-fetch"],
        tags: &["web", "http", "fetch", "url", "browse"],
    },
    CatalogEntry {
        name: "memory-kv",
        description: "Persistent key-value memory store",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-memory"],
        tags: &["memory", "store", "kv", "persist"],
    },
    CatalogEntry {
        name: "brave-search",
        description: "Web search via Brave Search API",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-brave-search"],
        tags: &["search", "web", "brave", "query"],
    },
    CatalogEntry {
        name: "github",
        description: "GitHub API: repos, issues, PRs, code search",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-github"],
        tags: &["github", "git", "code", "repo", "issue"],
    },
    CatalogEntry {
        name: "sqlite",
        description: "Query and manage SQLite databases",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-sqlite"],
        tags: &["sql", "sqlite", "database", "query"],
    },
];

/// Phase 5b-2: MCP Server Discovery tool (ADR-014 Layer 3).
///
/// Lets Mneme search a catalog of known MCP servers by keyword,
/// then autonomously connect the ones she finds useful.
pub struct DiscoverToolHandler {
    registry: Arc<RwLock<ToolRegistry>>,
}

impl DiscoverToolHandler {
    pub fn new(registry: Arc<RwLock<ToolRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl ToolHandler for DiscoverToolHandler {
    fn name(&self) -> &str {
        "discover"
    }

    fn description(&self) -> &str {
        "Search for and connect MCP tool servers by capability"
    }

    fn schema(&self) -> Tool {
        Tool {
            name: "discover".to_string(),
            description: "搜索可用的 MCP 工具服务器。用 action=search + query 搜索，用 action=connect + name 连接。".to_string(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: json!({
                    "action": {
                        "type": "string",
                        "enum": ["search", "connect"],
                        "description": "search: find servers by keyword; connect: connect a discovered server"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search keyword (for action=search)"
                    },
                    "name": {
                        "type": "string",
                        "description": "Server name from search results (for action=connect)"
                    }
                }),
                required: vec!["action".to_string()],
            },
        }
    }

    async fn execute(&self, input: &serde_json::Value) -> ToolOutcome {
        let action = match input.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return ToolOutcome::permanent_error("Missing: action".into()),
        };

        match action {
            "search" => {
                let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let query_lower = query.to_lowercase();
                let matches: Vec<_> = CATALOG
                    .iter()
                    .filter(|e| {
                        query.is_empty()
                            || e.name.contains(&query_lower)
                            || e.description.to_lowercase().contains(&query_lower)
                            || e.tags.iter().any(|t| t.contains(&query_lower))
                    })
                    .collect();

                if matches.is_empty() {
                    return ToolOutcome::ok(format!("没有找到匹配 '{}' 的服务器", query));
                }

                let mut result = format!("找到 {} 个匹配的 MCP 服务器:\n", matches.len());
                for entry in &matches {
                    result.push_str(&format!("- {}: {}\n", entry.name, entry.description));
                }
                result.push_str("\n用 action=connect + name 连接感兴趣的服务器。");
                ToolOutcome::ok(result)
            }
            "connect" => {
                let name = match input.get("name").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => return ToolOutcome::permanent_error("Missing: name".into()),
                };

                let entry = match CATALOG.iter().find(|e| e.name == name) {
                    Some(e) => e,
                    None => return ToolOutcome::permanent_error(
                        format!("未知服务器 '{}', 先用 search 查找", name),
                    ),
                };

                let cfg = McpServerConfig {
                    name: entry.name.to_string(),
                    command: entry.command.to_string(),
                    args: entry.args.iter().map(|s| s.to_string()).collect(),
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
                            "已连接 '{}', 获得 {} 个工具",
                            name, count
                        ))
                    }
                    Err(e) => ToolOutcome::transient_error(format!("连接失败: {e}")),
                }
            }
            other => ToolOutcome::permanent_error(format!("未知 action: '{}'", other)),
        }
    }
}
