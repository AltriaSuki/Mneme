use crate::llm::LlmClient;
use mneme_core::TaskType;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Task-based model router (Phase 5b-1, B-8 Level 2).
///
/// Holds multiple named LLM clients and routes requests based on task type.
/// Routing preferences can be overridden at runtime via self_knowledge
/// (e.g. "用 deepseek 写代码更快" → prefer deepseek for Creation tasks).
pub struct ModelRouter {
    /// Primary client (fallback for any unrouted task).
    primary: Arc<dyn LlmClient>,
    /// Named clients keyed by profile name.
    clients: HashMap<String, Arc<dyn LlmClient>>,
    /// Task → profile name mapping (config-based defaults + runtime overrides).
    routing: RwLock<HashMap<TaskType, String>>,
}

impl ModelRouter {
    pub fn new(primary: Arc<dyn LlmClient>) -> Self {
        Self {
            primary,
            clients: HashMap::new(),
            routing: RwLock::new(HashMap::new()),
        }
    }

    /// Register a named client with optional task preferences.
    pub fn register(&mut self, name: String, client: Arc<dyn LlmClient>, tasks: &[TaskType]) {
        self.clients.insert(name.clone(), client);
        let routing = self.routing.get_mut();
        for &task in tasks {
            routing.insert(task, name.clone());
        }
    }

    /// Get the client for a given task type.
    pub async fn client_for(&self, task: TaskType) -> Arc<dyn LlmClient> {
        let routing = self.routing.read().await;
        if let Some(name) = routing.get(&task) {
            if let Some(client) = self.clients.get(name) {
                return client.clone();
            }
        }
        self.primary.clone()
    }

    /// Override routing at runtime (called when Mneme forms model preferences).
    pub async fn set_preference(&self, task: TaskType, profile_name: String) {
        if self.clients.contains_key(&profile_name) {
            self.routing.write().await.insert(task, profile_name);
        }
    }

    /// Get current routing table (for introspection / self_knowledge storage).
    pub async fn current_routing(&self) -> HashMap<TaskType, String> {
        self.routing.read().await.clone()
    }

    /// List available profile names.
    pub fn available_profiles(&self) -> Vec<&str> {
        self.clients.keys().map(|s| s.as_str()).collect()
    }

    /// Get the primary (fallback) client.
    pub fn primary(&self) -> &Arc<dyn LlmClient> {
        &self.primary
    }
}
