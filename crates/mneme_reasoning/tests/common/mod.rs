//! Shared test utilities for mneme_reasoning integration tests.

use anyhow::Result;
use async_trait::async_trait;
use mneme_core::{Content, Event, Memory, Modality, Psyche};
use mneme_reasoning::api_types::{ContentBlock, MessagesResponse};
use mneme_reasoning::llm::{CompletionParams, LlmClient};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

// ============================================================================
// Mock LLM Client
// ============================================================================

/// Queue-based mock LLM client. Each `complete()` call pops the next response.
pub struct MockLlmClient {
    responses: Mutex<Vec<MessagesResponse>>,
    call_count: AtomicUsize,
}

impl MockLlmClient {
    pub fn new(responses: Vec<MessagesResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            call_count: AtomicUsize::new(0),
        }
    }

    /// Client that returns a text response followed by empty extraction result.
    pub fn with_text(text: &str) -> Self {
        Self::new(vec![
            text_response(text),
            text_response(r#"{"facts": []}"#),
        ])
    }

    #[allow(dead_code)]
    pub fn calls(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn complete(
        &self,
        _system: &str,
        _messages: Vec<mneme_reasoning::api_types::Message>,
        _tools: Vec<mneme_reasoning::api_types::Tool>,
        _params: CompletionParams,
    ) -> Result<MessagesResponse> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let mut queue = self.responses.lock().await;
        if queue.is_empty() {
            Ok(text_response(""))
        } else {
            Ok(queue.remove(0))
        }
    }
}

// ============================================================================
// Mock Memory
// ============================================================================

/// In-memory mock that records memorized content and stored facts.
pub struct MockMemory {
    pub memorized: Mutex<Vec<Content>>,
    pub stored_facts: Mutex<Vec<(String, String, String, f32)>>,
}

impl MockMemory {
    pub fn new() -> Self {
        Self {
            memorized: Mutex::new(Vec::new()),
            stored_facts: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Memory for MockMemory {
    async fn recall(&self, _query: &str) -> Result<String> {
        Ok("No relevant memories found.".to_string())
    }

    async fn memorize(&self, content: &Content) -> Result<()> {
        self.memorized.lock().await.push(content.clone());
        Ok(())
    }

    async fn recall_facts_formatted(&self, _query: &str) -> Result<String> {
        Ok(String::new())
    }

    async fn store_fact(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        confidence: f32,
    ) -> Result<()> {
        self.stored_facts.lock().await.push((
            subject.to_string(),
            predicate.to_string(),
            object.to_string(),
            confidence,
        ));
        Ok(())
    }
}

// ============================================================================
// Mock Tool Handler
// ============================================================================

use mneme_reasoning::tool_registry::ToolHandler;
use mneme_reasoning::ToolOutcome;

/// Fixed-output tool handler for testing.
pub struct MockToolHandler {
    tool_name: String,
    output: String,
}

impl MockToolHandler {
    pub fn shell(output: &str) -> Self {
        Self {
            tool_name: "shell".to_string(),
            output: output.to_string(),
        }
    }
}

#[async_trait]
impl ToolHandler for MockToolHandler {
    fn name(&self) -> &str {
        &self.tool_name
    }
    fn description(&self) -> &str {
        "Mock tool for testing"
    }
    fn schema(&self) -> mneme_reasoning::api_types::Tool {
        mneme_reasoning::tools::shell_tool()
    }
    async fn execute(&self, _input: &serde_json::Value) -> ToolOutcome {
        ToolOutcome::ok(self.output.clone())
    }
}

/// Tool handler that fails N times before succeeding.
pub struct FailingToolHandler {
    fail_count: AtomicUsize,
    error_msg: String,
    success_output: String,
    transient: bool,
}

impl FailingToolHandler {
    pub fn always_fail(msg: &str, transient: bool) -> Self {
        Self {
            fail_count: AtomicUsize::new(usize::MAX),
            error_msg: msg.to_string(),
            success_output: String::new(),
            transient,
        }
    }

    pub fn fail_then_succeed(n: usize, msg: &str, output: &str, transient: bool) -> Self {
        Self {
            fail_count: AtomicUsize::new(n),
            error_msg: msg.to_string(),
            success_output: output.to_string(),
            transient,
        }
    }
}

#[async_trait]
impl ToolHandler for FailingToolHandler {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "Failing mock tool for testing"
    }
    fn schema(&self) -> mneme_reasoning::api_types::Tool {
        mneme_reasoning::tools::shell_tool()
    }
    async fn execute(&self, _input: &serde_json::Value) -> ToolOutcome {
        let remaining = self.fail_count.load(Ordering::SeqCst);
        if remaining > 0 {
            self.fail_count.fetch_sub(1, Ordering::SeqCst);
            if self.transient {
                ToolOutcome::transient_error(self.error_msg.clone())
            } else {
                ToolOutcome::permanent_error(self.error_msg.clone())
            }
        } else {
            ToolOutcome::ok(self.success_output.clone())
        }
    }
}

// ============================================================================
// Response builders
// ============================================================================

pub fn text_response(text: &str) -> MessagesResponse {
    MessagesResponse {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        stop_reason: Some("end_turn".to_string()),
        usage: None,
    }
}

pub fn tool_use_response(name: &str, input: serde_json::Value) -> MessagesResponse {
    MessagesResponse {
        content: vec![ContentBlock::ToolUse {
            id: format!("tool_{}", Uuid::new_v4()),
            name: name.to_string(),
            input,
        }],
        stop_reason: Some("tool_use".to_string()),
        usage: None,
    }
}

#[allow(dead_code)]
pub fn text_with_tool_use(prose: &str, name: &str, input: serde_json::Value) -> MessagesResponse {
    MessagesResponse {
        content: vec![
            ContentBlock::Text {
                text: prose.to_string(),
            },
            ContentBlock::ToolUse {
                id: format!("tool_{}", Uuid::new_v4()),
                name: name.to_string(),
                input,
            },
        ],
        stop_reason: Some("tool_use".to_string()),
        usage: None,
    }
}

// ============================================================================
// Engine builders
// ============================================================================

pub fn test_psyche() -> Psyche {
    Psyche::with_self_model("Test self model for unit tests.".into())
}

pub fn user_event(text: &str) -> Event {
    Event::UserMessage(Content {
        id: Uuid::new_v4(),
        source: "test".into(),
        author: "user".into(),
        body: text.into(),
        timestamp: 0,
        modality: Modality::Text,
        ..Default::default()
    })
}

pub fn build_engine(client: MockLlmClient) -> mneme_reasoning::engine::ReasoningEngine {
    let memory: Arc<dyn Memory> = Arc::new(MockMemory::new());
    let mut engine = mneme_reasoning::engine::ReasoningEngine::new(test_psyche(), memory, Box::new(client));
    engine.set_exploration_nudge(false);
    engine
}

pub fn build_engine_with_tool(
    client: MockLlmClient,
    memory: Arc<MockMemory>,
    tool_handler: Box<dyn ToolHandler>,
) -> mneme_reasoning::engine::ReasoningEngine {
    let mut engine = mneme_reasoning::engine::ReasoningEngine::new(
        test_psyche(),
        memory as Arc<dyn Memory>,
        Box::new(client),
    );
    let mut registry = mneme_reasoning::ToolRegistry::new();
    registry.register(tool_handler);
    engine.set_registry(Arc::new(tokio::sync::RwLock::new(registry)));
    engine.set_exploration_nudge(false);
    engine
}
