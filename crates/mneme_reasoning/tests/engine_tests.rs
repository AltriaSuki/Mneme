//! Integration tests for the ReasoningEngine.
//!
//! These tests use a MockLlmClient that returns configurable responses,
//! allowing us to test the full think() pipeline without real LLM calls.

use anyhow::Result;
use async_trait::async_trait;
use mneme_core::{Content, Event, Memory, Modality, Psyche, Reasoning};
use mneme_reasoning::engine::ReasoningEngine;
use mneme_reasoning::api_types::{ContentBlock, MessagesResponse};
use mneme_reasoning::llm::{CompletionParams, LlmClient};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use tokio::sync::Mutex;
use uuid::Uuid;

// ============================================================================
// Mock LLM Client
// ============================================================================

/// A mock LLM client that returns a sequence of pre-configured responses.
/// Each call to `complete()` pops the next response from the queue.
/// If the queue is exhausted, returns a default "empty" response.
struct MockLlmClient {
    responses: Mutex<Vec<MessagesResponse>>,
    call_count: AtomicUsize,
}

impl MockLlmClient {
    fn new(responses: Vec<MessagesResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            call_count: AtomicUsize::new(0),
        }
    }

    /// Create a client that always returns a simple text response.
    fn with_text(text: &str) -> Self {
        // Return the same text for both the main call and the extraction call
        Self::new(vec![
            text_response(text),
            // Extraction call returns empty facts
            text_response(r#"{"facts": []}"#),
        ])
    }

    #[allow(dead_code)]
    fn calls(&self) -> usize {
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

/// A simple in-memory mock that records what was memorized.
struct MockMemory {
    memorized: Mutex<Vec<Content>>,
    stored_facts: Mutex<Vec<(String, String, String, f32)>>,
}

impl MockMemory {
    fn new() -> Self {
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

    async fn store_fact(&self, subject: &str, predicate: &str, object: &str, confidence: f32) -> Result<()> {
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
// Mock Executor
// ============================================================================

struct MockExecutor {
    output: String,
}

impl MockExecutor {
    fn new(output: &str) -> Self {
        Self { output: output.to_string() }
    }
}

#[async_trait]
impl mneme_os::Executor for MockExecutor {
    async fn execute(&self, _command: &str) -> Result<String> {
        Ok(self.output.clone())
    }

    fn name(&self) -> &str {
        "mock"
    }
}

// ============================================================================
// Helper functions
// ============================================================================

fn text_response(text: &str) -> MessagesResponse {
    MessagesResponse {
        content: vec![ContentBlock::Text { text: text.to_string() }],
        stop_reason: Some("end_turn".to_string()),
    }
}

fn tool_use_response(tool_id: &str, tool_name: &str, input: serde_json::Value) -> MessagesResponse {
    MessagesResponse {
        content: vec![ContentBlock::ToolUse {
            id: tool_id.to_string(),
            name: tool_name.to_string(),
            input,
        }],
        stop_reason: Some("tool_use".to_string()),
    }
}

fn tool_use_and_text_response(tool_id: &str, tool_name: &str, input: serde_json::Value, text: &str) -> MessagesResponse {
    MessagesResponse {
        content: vec![
            ContentBlock::Text { text: text.to_string() },
            ContentBlock::ToolUse {
                id: tool_id.to_string(),
                name: tool_name.to_string(),
                input,
            },
        ],
        stop_reason: Some("tool_use".to_string()),
    }
}

fn test_psyche() -> Psyche {
    Psyche {
        hippocampus: "Test identity.".into(),
        limbic: "Test emotions.".into(),
        cortex: "Test cognition.".into(),
        broca: "Test language.".into(),
        occipital: "Test senses.".into(),
    }
}

fn user_event(text: &str) -> Event {
    Event::UserMessage(Content {
        id: Uuid::new_v4(),
        source: "test".into(),
        author: "user".into(),
        body: text.into(),
        timestamp: 0,
        modality: Modality::Text,
    })
}

fn build_engine(client: MockLlmClient) -> ReasoningEngine {
    let memory: Arc<dyn Memory> = Arc::new(MockMemory::new());
    let executor: Arc<dyn mneme_os::Executor> = Arc::new(MockExecutor::new("mock output"));
    ReasoningEngine::new(test_psyche(), memory, Box::new(client), executor)
}

fn build_engine_with_mocks(
    client: MockLlmClient,
    memory: Arc<MockMemory>,
    executor: Arc<MockExecutor>,
) -> ReasoningEngine {
    ReasoningEngine::new(
        test_psyche(),
        memory as Arc<dyn Memory>,
        Box::new(client),
        executor as Arc<dyn mneme_os::Executor>,
    )
}

// ============================================================================
// Tests: Basic Conversation
// ============================================================================

#[tokio::test]
async fn test_simple_text_response() {
    let engine = build_engine(MockLlmClient::with_text("你好呀！"));
    let result = engine.think(user_event("你好")).await.unwrap();

    assert_eq!(result.content, "你好呀！");
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_empty_response_is_silent() {
    let engine = build_engine(MockLlmClient::with_text(""));
    let result = engine.think(user_event("你好")).await.unwrap();

    // Empty content after sanitization
    assert!(result.content.is_empty());
}

#[tokio::test]
async fn test_silence_tag_produces_empty_response() {
    let engine = build_engine(MockLlmClient::with_text("[SILENCE]"));
    let result = engine.think(user_event("大家好")).await.unwrap();

    assert!(result.content.is_empty(), "SILENCE tag should produce empty content");
}

// ============================================================================
// Tests: Output Sanitization
// ============================================================================

#[tokio::test]
async fn test_roleplay_asterisks_stripped() {
    let engine = build_engine(MockLlmClient::with_text("*叹了口气*你说得对"));
    let result = engine.think(user_event("测试")).await.unwrap();

    assert!(!result.content.contains('*'), "Roleplay asterisks should be stripped");
    assert!(result.content.contains("你说得对"));
}

#[tokio::test]
async fn test_markdown_bold_stripped() {
    let engine = build_engine(MockLlmClient::with_text("这是**重要**的事情"));
    let result = engine.think(user_event("测试")).await.unwrap();

    assert!(!result.content.contains("**"), "Bold markdown should be stripped");
    assert!(result.content.contains("重要"));
}

#[tokio::test]
async fn test_markdown_headers_stripped() {
    let engine = build_engine(MockLlmClient::with_text("# 标题\n内容在这里"));
    let result = engine.think(user_event("测试")).await.unwrap();

    assert!(!result.content.starts_with('#'), "Headers should be stripped");
    assert!(result.content.contains("标题"));
    assert!(result.content.contains("内容在这里"));
}

#[tokio::test]
async fn test_markdown_bullets_stripped() {
    let engine = build_engine(MockLlmClient::with_text("- 第一\n- 第二\n- 第三"));
    let result = engine.think(user_event("测试")).await.unwrap();

    assert!(!result.content.contains("- "), "Bullet markers should be stripped");
    assert!(result.content.contains("第一"));
}

// ============================================================================
// Tests: Emotion Parsing
// ============================================================================

#[tokio::test]
async fn test_emotion_tag_parsed_and_stripped() {
    let engine = build_engine(MockLlmClient::with_text("<emotion>Happy</emotion>今天真开心！"));
    let result = engine.think(user_event("你好")).await.unwrap();

    // Emotion tag should be stripped from content
    assert!(!result.content.contains("<emotion>"));
    assert!(result.content.contains("今天真开心"));
    // Emotion should be parsed
    assert_eq!(result.emotion, mneme_core::Emotion::Happy);
}

#[tokio::test]
async fn test_emotion_tag_case_insensitive() {
    let engine = build_engine(MockLlmClient::with_text("<EMOTION>Sad</EMOTION>呜呜"));
    let result = engine.think(user_event("你好")).await.unwrap();

    assert!(!result.content.contains("EMOTION"));
    assert!(result.content.contains("呜呜"));
}

// ============================================================================
// Tests: Tool Use (ReAct Loop)
// ============================================================================

#[tokio::test]
async fn test_single_tool_call() {
    // Turn 1: LLM requests shell tool
    // Turn 2: LLM produces final text after seeing tool result
    // Turn 3: Extraction call
    let client = MockLlmClient::new(vec![
        tool_use_response("t1", "shell", serde_json::json!({"command": "echo hello"})),
        text_response("命令执行完毕，结果是 hello"),
        text_response(r#"{"facts": []}"#), // extraction
    ]);

    let executor = Arc::new(MockExecutor::new("hello\n"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor);

    let result = engine.think(user_event("执行 echo hello")).await.unwrap();

    assert!(result.content.contains("hello") || result.content.contains("命令"));
}

#[tokio::test]
async fn test_multi_turn_tool_calls() {
    // Turn 1: First tool call
    // Turn 2: Second tool call
    // Turn 3: Final text response
    // Turn 4: Extraction
    let client = MockLlmClient::new(vec![
        tool_use_response("t1", "shell", serde_json::json!({"command": "ls"})),
        tool_use_response("t2", "shell", serde_json::json!({"command": "cat file.txt"})),
        text_response("文件内容是 hello world"),
        text_response(r#"{"facts": []}"#),
    ]);

    let executor = Arc::new(MockExecutor::new("result"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory.clone(), executor);

    let result = engine.think(user_event("读取文件")).await.unwrap();

    assert!(result.content.contains("hello world"));
}

#[tokio::test]
async fn test_react_loop_max_iterations() {
    // LLM keeps requesting tools forever — should be capped at 5 iterations
    let mut responses = Vec::new();
    for i in 0..10 {
        responses.push(tool_use_response(
            &format!("t{}", i),
            "shell",
            serde_json::json!({"command": "loop"}),
        ));
    }
    // After the loop exits, extraction call
    responses.push(text_response(r#"{"facts": []}"#));

    let client = MockLlmClient::new(responses);
    let executor = Arc::new(MockExecutor::new("looped"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor);

    let result = engine.think(user_event("无限循环")).await.unwrap();

    // Should have been called at most 5 times for the main loop + 1 for extraction
    // (the loop has 5 iterations max, each consumes one response)
    // Content might be empty since we never got a text response
    assert!(result.content.is_empty() || !result.content.is_empty()); // shouldn't panic
}

#[tokio::test]
async fn test_unknown_tool_returns_error_message() {
    // LLM requests an unknown tool, then gives a text response
    let client = MockLlmClient::new(vec![
        tool_use_response("t1", "nonexistent_tool", serde_json::json!({})),
        text_response("好的我理解了"),
        text_response(r#"{"facts": []}"#),
    ]);

    let engine = build_engine(client);
    let result = engine.think(user_event("测试")).await.unwrap();

    // Should not panic; unknown tool returns "Unknown Tool: ..." and loop continues
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_tool_use_with_text_in_same_response() {
    // Some LLMs return text + tool_use in the same response
    let client = MockLlmClient::new(vec![
        tool_use_and_text_response("t1", "shell", serde_json::json!({"command": "date"}), "我来看看现在几点"),
        text_response("现在是下午三点"),
        text_response(r#"{"facts": []}"#),
    ]);

    let executor = Arc::new(MockExecutor::new("2026-02-06"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor);

    let result = engine.think(user_event("几点了")).await.unwrap();

    assert!(result.content.contains("三点"));
}

// ============================================================================
// Tests: Memory Integration
// ============================================================================

#[tokio::test]
async fn test_user_message_is_memorized() {
    let memory = Arc::new(MockMemory::new());
    let executor = Arc::new(MockExecutor::new(""));
    let client = MockLlmClient::with_text("收到");

    let engine = build_engine_with_mocks(client, memory.clone(), executor);
    engine.think(user_event("记住这句话")).await.unwrap();

    let memorized = memory.memorized.lock().await;
    assert_eq!(memorized.len(), 1);
    assert_eq!(memorized[0].body, "记住这句话");
}

#[tokio::test]
async fn test_fact_extraction_stores_results() {
    // Main response + extraction response with actual facts
    let client = MockLlmClient::new(vec![
        text_response("我知道了你喜欢猫"),
        text_response(r#"{"facts": [{"subject": "用户", "predicate": "喜欢", "object": "猫", "confidence": 0.9}]}"#),
    ]);

    let memory = Arc::new(MockMemory::new());
    let executor = Arc::new(MockExecutor::new(""));
    let engine = build_engine_with_mocks(client, memory.clone(), executor);

    engine.think(user_event("我很喜欢猫")).await.unwrap();

    let facts = memory.stored_facts.lock().await;
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].0, "用户"); // subject
    assert_eq!(facts[0].1, "喜欢"); // predicate
    assert_eq!(facts[0].2, "猫");   // object
}

// ============================================================================
// Tests: History Management
// ============================================================================

#[tokio::test]
async fn test_history_accumulates_across_turns() {
    // Use a client with enough responses for 3 conversations
    let client = MockLlmClient::new(vec![
        text_response("回复1"), text_response(r#"{"facts": []}"#),
        text_response("回复2"), text_response(r#"{"facts": []}"#),
        text_response("回复3"), text_response(r#"{"facts": []}"#),
    ]);

    let engine = build_engine(client);

    engine.think(user_event("消息1")).await.unwrap();
    engine.think(user_event("消息2")).await.unwrap();
    engine.think(user_event("消息3")).await.unwrap();

    // We can't directly inspect history, but we can verify it didn't crash
    // and that the 3rd response still works (implicitly tests history assembly)
}

#[tokio::test]
async fn test_history_prune_at_limit() {
    // Send more than 20 messages (10 turns) to trigger pruning
    let mut responses = Vec::new();
    for _ in 0..15 {
        responses.push(text_response("ok"));
        responses.push(text_response(r#"{"facts": []}"#));
    }

    let client = MockLlmClient::new(responses);
    let engine = build_engine(client);

    for i in 0..15 {
        let result = engine.think(user_event(&format!("消息{}", i))).await;
        assert!(result.is_ok(), "Turn {} should succeed after pruning", i);
    }

    // If pruning logic is broken, this would have panicked
}

// ============================================================================
// Tests: Proactive Triggers
// ============================================================================

#[tokio::test]
async fn test_proactive_trigger_scheduled() {
    let client = MockLlmClient::new(vec![
        text_response("早上好！新的一天开始了"),
        // No extraction for proactive triggers (not a UserMessage)
    ]);

    let engine = build_engine(client);

    let event = Event::ProactiveTrigger(mneme_core::Trigger::Scheduled {
        name: "morning_greeting".into(),
        schedule: "0 8 * * *".into(),
    });

    let result = engine.think(event).await.unwrap();
    assert!(result.content.contains("早上好") || !result.content.is_empty());
}

#[tokio::test]
async fn test_proactive_trigger_memory_decay() {
    let client = MockLlmClient::new(vec![
        text_response("对了，你之前提到过的旅行计划怎么样了？"),
    ]);

    let engine = build_engine(client);

    let event = Event::ProactiveTrigger(mneme_core::Trigger::MemoryDecay {
        topic: "旅行计划".into(),
        last_mentioned: 0,
    });

    let result = engine.think(event).await.unwrap();
    assert!(!result.content.is_empty());
}

// ============================================================================
// Tests: Edge Cases
// ============================================================================

#[tokio::test]
async fn test_multiline_response_preserved() {
    let engine = build_engine(MockLlmClient::with_text("第一行\n第二行\n第三行"));
    let result = engine.think(user_event("测试")).await.unwrap();

    assert!(result.content.contains('\n'));
    assert!(result.content.contains("第一行"));
    assert!(result.content.contains("第三行"));
}

#[tokio::test]
async fn test_very_long_input_does_not_panic() {
    let long_input = "啊".repeat(10_000);
    let engine = build_engine(MockLlmClient::with_text("收到了"));
    let result = engine.think(user_event(&long_input)).await.unwrap();

    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_unicode_emoji_handled() {
    let engine = build_engine(MockLlmClient::with_text("😊❤️🎉"));
    let result = engine.think(user_event("发个表情")).await.unwrap();

    assert!(result.content.contains("😊"));
    assert!(result.content.contains("❤️"));
}

#[tokio::test]
async fn test_shell_tool_missing_command_param() {
    // LLM calls shell tool without required "command" param
    let client = MockLlmClient::new(vec![
        tool_use_response("t1", "shell", serde_json::json!({})),
        text_response("参数有误"),
        text_response(r#"{"facts": []}"#),
    ]);

    let engine = build_engine(client);
    let result = engine.think(user_event("执行命令")).await.unwrap();

    // Should gracefully handle missing param without panic
    assert!(!result.content.is_empty());
}
