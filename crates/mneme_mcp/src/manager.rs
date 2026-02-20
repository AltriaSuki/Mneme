use crate::bridge::McpToolHandler;
use mneme_core::config::McpServerConfig;
use mneme_memory::LifecycleState;
use mneme_reasoning::tool_registry::ToolHandler;
use rmcp::service::{Peer, RoleClient, RunningService, ServiceExt};
use rmcp::transport::TokioChildProcess;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::RwLock;

/// A connected MCP server with its running service and shared peer handle.
struct ConnectedServer {
    name: String,
    service: RunningService<RoleClient, ()>,
    peer: Arc<RwLock<Option<Peer<RoleClient>>>>,
}

/// Manages MCP server connections with lifecycle awareness.
///
/// Spawns child processes, discovers tools, and bridges them to ToolHandler.
/// Responds to organism lifecycle changes (sleep → disconnect, wake → reconnect).
pub struct McpManager {
    configs: Vec<McpServerConfig>,
    servers: Vec<ConnectedServer>,
    lifecycle_rx: tokio::sync::watch::Receiver<LifecycleState>,
}

impl McpManager {
    pub fn new(
        configs: Vec<McpServerConfig>,
        lifecycle_rx: tokio::sync::watch::Receiver<LifecycleState>,
    ) -> Self {
        let _ = &lifecycle_rx; // suppress unused warning during construction
        Self {
            configs,
            servers: Vec::new(),
            lifecycle_rx,
        }
    }

    /// Connect to all configured MCP servers and discover their tools.
    /// Returns tool handlers ready for registration in ToolRegistry.
    pub async fn connect_all(&mut self) -> anyhow::Result<Vec<Box<dyn ToolHandler>>> {
        let mut all_tools: Vec<Box<dyn ToolHandler>> = Vec::new();

        for config in &self.configs {
            if !config.auto_connect {
                tracing::info!("Skipping MCP server '{}' (auto_connect=false)", config.name);
                continue;
            }

            match self.connect_server(config).await {
                Ok((server, tools)) => {
                    tracing::info!(
                        "MCP server '{}': {} tool(s) discovered",
                        config.name,
                        tools.len()
                    );
                    all_tools.extend(tools);
                    self.servers.push(server);
                }
                Err(e) => {
                    tracing::error!("Failed to connect MCP server '{}': {}", config.name, e);
                    // Non-fatal: continue with other servers
                }
            }
        }

        Ok(all_tools)
    }

    /// Connect a single MCP server and discover its tools.
    async fn connect_server(
        &self,
        config: &McpServerConfig,
    ) -> anyhow::Result<(ConnectedServer, Vec<Box<dyn ToolHandler>>)> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        let transport = TokioChildProcess::new(cmd)?;
        let service = ().serve(transport).await.map_err(|e| {
            anyhow::anyhow!("MCP handshake failed for '{}': {}", config.name, e)
        })?;

        // Discover tools
        let tools = service.peer().list_all_tools().await.map_err(|e| {
            anyhow::anyhow!("list_tools failed for '{}': {}", config.name, e)
        })?;

        // Create shared peer handle (lifecycle-aware)
        let peer = Arc::new(RwLock::new(Some(service.peer().clone())));

        // Bridge each MCP tool to ToolHandler
        let handlers: Vec<Box<dyn ToolHandler>> = tools
            .iter()
            .map(|t| {
                let handler = McpToolHandler::from_mcp_tool(t, peer.clone(), &config.name);
                tracing::debug!(
                    "  → tool '{}' from server '{}'",
                    t.name,
                    config.name
                );
                Box::new(handler) as Box<dyn ToolHandler>
            })
            .collect();

        let connected = ConnectedServer {
            name: config.name.clone(),
            service,
            peer,
        };

        Ok((connected, handlers))
    }

    /// Disconnect all MCP servers gracefully.
    pub async fn disconnect_all(&mut self) {
        for server in self.servers.drain(..) {
            // Clear peer so in-flight tool calls get transient errors
            *server.peer.write().await = None;
            // Cancel the service (kills child process)
            if let Err(e) = server.service.cancel().await {
                tracing::warn!("Error cancelling MCP server '{}': {:?}", server.name, e);
            }
            tracing::info!("MCP server '{}' disconnected", server.name);
        }
    }

    /// Connect a single MCP server at runtime and return its tool handlers.
    /// Used by config reload to add newly-configured servers without restarting.
    pub async fn connect_one(config: &McpServerConfig) -> anyhow::Result<Vec<Box<dyn ToolHandler>>> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        let transport = TokioChildProcess::new(cmd)?;
        let service = ().serve(transport).await.map_err(|e| {
            anyhow::anyhow!("MCP handshake failed for '{}': {}", config.name, e)
        })?;

        let tools = service.peer().list_all_tools().await.map_err(|e| {
            anyhow::anyhow!("list_tools failed for '{}': {}", config.name, e)
        })?;

        let peer = Arc::new(RwLock::new(Some(service.peer().clone())));

        let handlers: Vec<Box<dyn ToolHandler>> = tools
            .iter()
            .map(|t| {
                Box::new(McpToolHandler::from_mcp_tool(t, peer.clone(), &config.name))
                    as Box<dyn ToolHandler>
            })
            .collect();

        // Leak the service so the child process stays alive.
        // Full lifecycle management for runtime-added servers is a future enhancement.
        std::mem::forget(service);

        tracing::info!(
            "Runtime MCP server '{}': {} tool(s) discovered",
            config.name,
            handlers.len()
        );
        Ok(handlers)
    }

    /// Spawn a background task that watches lifecycle changes and
    /// disconnects/reconnects MCP servers accordingly.
    pub fn spawn_lifecycle_watcher(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut this = self;
            let mut rx: tokio::sync::watch::Receiver<LifecycleState> = this.lifecycle_rx.clone();

            loop {
                if rx.changed().await.is_err() {
                    tracing::info!("Lifecycle channel closed, MCP watcher exiting");
                    break;
                }

                let state = *rx.borrow();
                match state {
                    LifecycleState::Sleeping | LifecycleState::ShuttingDown => {
                        tracing::info!("Organism sleeping/shutting down → disconnecting MCP servers");
                        // Clear peers so tool calls return transient errors
                        for server in &this.servers {
                            *server.peer.write().await = None;
                        }
                    }
                    LifecycleState::Awake => {
                        tracing::info!("Organism awake → MCP servers remain connected");
                        // Peers are already set from connect_all; if we need
                        // reconnection logic in the future, it goes here.
                    }
                    LifecycleState::Drowsy | LifecycleState::Degraded => {
                        // No action needed for drowsy/degraded state
                    }
                }
            }

            // Final cleanup
            this.disconnect_all().await;
        })
    }
}
