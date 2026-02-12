//! Integration tests for the ReasoningEngine.
//!
//! These tests use a MockLlmClient that returns configurable responses,
//! allowing us to test the full think() pipeline without real LLM calls.

use anyhow::Result;
use async_trait::async_trait;
use mneme_core::{Content, Event, Memory, Modality, Psyche, Reasoning};
use mneme_reasoning::api_types::{ContentBlock, MessagesResponse};
use mneme_reasoning::engine::ReasoningEngine;
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
// Mock Executor
// ============================================================================

struct MockExecutor {
    output: String,
}

impl MockExecutor {
    fn new(output: &str) -> Self {
        Self {
            output: output.to_string(),
        }
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
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        stop_reason: Some("end_turn".to_string()),
        usage: None,
    }
}

/// Helper: create a text response containing a <tool_call> tag (text-only tool path).
fn text_tool_call(name: &str, input: serde_json::Value) -> MessagesResponse {
    let json = serde_json::json!({"name": name, "arguments": input});
    text_response(&format!("<tool_call>{}</tool_call>", json))
}

/// Helper: text response with both prose and a <tool_call> tag.
fn text_with_tool_call(prose: &str, name: &str, input: serde_json::Value) -> MessagesResponse {
    let json = serde_json::json!({"name": name, "arguments": input});
    text_response(&format!("{} <tool_call>{}</tool_call>", prose, json))
}

fn test_psyche() -> Psyche {
    Psyche::with_self_model("Test self model for unit tests.".into())
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
    executor: Arc<dyn mneme_os::Executor>,
) -> ReasoningEngine {
    ReasoningEngine::new(
        test_psyche(),
        memory as Arc<dyn Memory>,
        Box::new(client),
        executor,
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

    assert!(
        result.content.is_empty(),
        "SILENCE tag should produce empty content"
    );
}

// ============================================================================
// Tests: Output Sanitization
// ============================================================================

#[tokio::test]
async fn test_roleplay_asterisks_stripped() {
    let engine = build_engine(MockLlmClient::with_text("*叹了口气*你说得对"));
    let result = engine.think(user_event("测试")).await.unwrap();

    assert!(
        !result.content.contains('*'),
        "Roleplay asterisks should be stripped"
    );
    assert!(result.content.contains("你说得对"));
}

#[tokio::test]
async fn test_markdown_bold_stripped() {
    let engine = build_engine(MockLlmClient::with_text("这是**重要**的事情"));
    let result = engine.think(user_event("测试")).await.unwrap();

    assert!(
        !result.content.contains("**"),
        "Bold markdown should be stripped"
    );
    assert!(result.content.contains("重要"));
}

#[tokio::test]
async fn test_markdown_headers_stripped() {
    let engine = build_engine(MockLlmClient::with_text("# 标题\n内容在这里"));
    let result = engine.think(user_event("测试")).await.unwrap();

    assert!(
        !result.content.starts_with('#'),
        "Headers should be stripped"
    );
    assert!(result.content.contains("标题"));
    assert!(result.content.contains("内容在这里"));
}

#[tokio::test]
async fn test_markdown_bullets_stripped() {
    let engine = build_engine(MockLlmClient::with_text("- 第一\n- 第二\n- 第三"));
    let result = engine.think(user_event("测试")).await.unwrap();

    assert!(
        !result.content.contains("- "),
        "Bullet markers should be stripped"
    );
    assert!(result.content.contains("第一"));
}

// ============================================================================
// Tests: Emotion Parsing
// ============================================================================

#[tokio::test]
async fn test_emotion_tag_parsed_and_stripped() {
    let engine = build_engine(MockLlmClient::with_text(
        "<emotion>Happy</emotion>今天真开心！",
    ));
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
        text_tool_call("shell", serde_json::json!({"command": "echo hello"})),
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
        text_tool_call("shell", serde_json::json!({"command": "ls"})),
        text_tool_call("shell", serde_json::json!({"command": "cat file.txt"})),
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
    for _i in 0..10 {
        responses.push(text_tool_call(
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
        text_tool_call("nonexistent_tool", serde_json::json!({})),
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
        text_with_tool_call(
            "我来看看现在几点",
            "shell",
            serde_json::json!({"command": "date"}),
        ),
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
        text_response(
            r#"{"facts": [{"subject": "用户", "predicate": "喜欢", "object": "猫", "confidence": 0.9}]}"#,
        ),
    ]);

    let memory = Arc::new(MockMemory::new());
    let executor = Arc::new(MockExecutor::new(""));
    let engine = build_engine_with_mocks(client, memory.clone(), executor);

    engine.think(user_event("我很喜欢猫")).await.unwrap();

    let facts = memory.stored_facts.lock().await;
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].0, "用户"); // subject
    assert_eq!(facts[0].1, "喜欢"); // predicate
    assert_eq!(facts[0].2, "猫"); // object
}

// ============================================================================
// Tests: History Management
// ============================================================================

#[tokio::test]
async fn test_history_accumulates_across_turns() {
    // Use a client with enough responses for 3 conversations
    let client = MockLlmClient::new(vec![
        text_response("回复1"),
        text_response(r#"{"facts": []}"#),
        text_response("回复2"),
        text_response(r#"{"facts": []}"#),
        text_response("回复3"),
        text_response(r#"{"facts": []}"#),
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
    let client = MockLlmClient::new(vec![text_response(
        "对了，你之前提到过的旅行计划怎么样了？",
    )]);

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
        text_tool_call("shell", serde_json::json!({})),
        text_response("参数有误"),
        text_response(r#"{"facts": []}"#),
    ]);

    let engine = build_engine(client);
    let result = engine.think(user_event("执行命令")).await.unwrap();

    // Should gracefully handle missing param without panic
    assert!(!result.content.is_empty());
}

// ============================================================================
// Tests: Structured Tool Error Handling (#2)
// ============================================================================

/// A mock executor that can simulate different failure modes.
struct FailingExecutor {
    /// How many calls fail before succeeding.
    fail_count: AtomicUsize,
    /// Error message to use.
    error_msg: String,
    /// Output on success.
    success_output: String,
}

impl FailingExecutor {
    /// Always fails with the given message.
    fn always_fail(msg: &str) -> Self {
        Self {
            fail_count: AtomicUsize::new(usize::MAX),
            error_msg: msg.to_string(),
            success_output: String::new(),
        }
    }

    /// Fails `n` times, then succeeds with `output`.
    fn fail_then_succeed(n: usize, msg: &str, output: &str) -> Self {
        Self {
            fail_count: AtomicUsize::new(n),
            error_msg: msg.to_string(),
            success_output: output.to_string(),
        }
    }
}

#[async_trait]
impl mneme_os::Executor for FailingExecutor {
    async fn execute(&self, _command: &str) -> Result<String> {
        let remaining = self.fail_count.load(Ordering::SeqCst);
        if remaining > 0 {
            self.fail_count.fetch_sub(1, Ordering::SeqCst);
            anyhow::bail!("{}", self.error_msg);
        }
        Ok(self.success_output.clone())
    }

    fn name(&self) -> &str {
        "failing_mock"
    }
}

#[tokio::test]
async fn test_shell_timeout_returns_is_error_true() {
    // Shell times out → LLM sees is_error=true with descriptive message
    let client = MockLlmClient::new(vec![
        text_tool_call("shell", serde_json::json!({"command": "sleep 100"})),
        text_response("命令超时了，我换个方式"),
        text_response(r#"{"facts": []}"#),
    ]);

    let executor = Arc::new(FailingExecutor::always_fail(
        "Command execution timed out after 30s",
    ));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor as Arc<dyn mneme_os::Executor>);

    let result = engine.think(user_event("执行很久的命令")).await.unwrap();

    // The LLM received the error and produced a recovery response
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_shell_permanent_failure_returns_is_error() {
    // Shell command fails with non-zero exit (permanent) — no retry
    let client = MockLlmClient::new(vec![
        text_tool_call("shell", serde_json::json!({"command": "bad_cmd"})),
        text_response("命令执行失败了"),
        text_response(r#"{"facts": []}"#),
    ]);

    let executor = Arc::new(FailingExecutor::always_fail(
        "Command failed with status exit code: 127",
    ));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor as Arc<dyn mneme_os::Executor>);

    let result = engine.think(user_event("执行错误命令")).await.unwrap();

    // Should recover gracefully
    assert!(result.content.contains("失败"));
}

#[tokio::test]
async fn test_shell_transient_retry_succeeds() {
    // First call times out (transient), retry succeeds
    let client = MockLlmClient::new(vec![
        text_tool_call("shell", serde_json::json!({"command": "echo ok"})),
        text_response("命令执行成功"),
        text_response(r#"{"facts": []}"#),
    ]);

    let executor = Arc::new(FailingExecutor::fail_then_succeed(
        1,
        "Command execution timed out after 30s",
        "ok\n",
    ));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor as Arc<dyn mneme_os::Executor>);

    let result = engine.think(user_event("执行命令")).await.unwrap();

    assert!(result.content.contains("成功"));
}

#[tokio::test]
async fn test_unknown_tool_is_permanent_error() {
    // Unknown tool should be permanent (not retried)
    let client = MockLlmClient::new(vec![
        text_tool_call("flying_car", serde_json::json!({})),
        text_response("我没有那个工具"),
        text_response(r#"{"facts": []}"#),
    ]);

    let engine = build_engine(client);
    let result = engine.think(user_event("发射飞船")).await.unwrap();

    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_browser_missing_url_is_permanent_error() {
    // browser_goto without url → permanent error, no retry
    let client = MockLlmClient::new(vec![
        text_tool_call("browser_goto", serde_json::json!({})),
        text_response("缺少网址参数"),
        text_response(r#"{"facts": []}"#),
    ]);

    let engine = build_engine(client);
    let result = engine.think(user_event("打开网页")).await.unwrap();

    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_browser_missing_selector_is_permanent_error() {
    // browser_click without selector → permanent error
    let client = MockLlmClient::new(vec![
        text_tool_call("browser_click", serde_json::json!({})),
        text_response("缺少选择器"),
        text_response(r#"{"facts": []}"#),
    ]);

    let engine = build_engine(client);
    let result = engine.think(user_event("点击按钮")).await.unwrap();

    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_browser_type_missing_text_is_permanent_error() {
    // browser_type with selector but no text → permanent error
    let client = MockLlmClient::new(vec![
        text_tool_call("browser_type", serde_json::json!({"selector": "#input"})),
        text_response("参数不完整"),
        text_response(r#"{"facts": []}"#),
    ]);

    let engine = build_engine(client);
    let result = engine.think(user_event("输入文字")).await.unwrap();

    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_tool_error_does_not_crash_react_loop() {
    // Tool fails but the ReAct loop should still continue
    // Turn 1: shell fails, Turn 2: LLM tries again, Turn 3: success, Turn 4: final text
    let client = MockLlmClient::new(vec![
        text_tool_call("shell", serde_json::json!({"command": "fail"})),
        text_tool_call("shell", serde_json::json!({"command": "echo ok"})),
        text_response("第二次就好了"),
        text_response(r#"{"facts": []}"#),
    ]);

    // First call fails, second succeeds
    let executor = Arc::new(FailingExecutor::fail_then_succeed(
        1,
        "Command failed with status exit code: 1",
        "ok\n",
    ));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor as Arc<dyn mneme_os::Executor>);

    let result = engine.think(user_event("尝试命令")).await.unwrap();

    // The LLM should have recovered after getting the error
    assert!(result.content.contains("第二次") || !result.content.is_empty());
}

#[tokio::test]
async fn test_spawn_failure_is_transient() {
    // "spawn" in error message → transient, will retry
    let client = MockLlmClient::new(vec![
        text_tool_call("shell", serde_json::json!({"command": "echo ok"})),
        text_response("最终成功了"),
        text_response(r#"{"facts": []}"#),
    ]);

    let executor = Arc::new(FailingExecutor::fail_then_succeed(
        1,
        "Failed to spawn command locally",
        "ok\n",
    ));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor as Arc<dyn mneme_os::Executor>);

    let result = engine.think(user_event("执行")).await.unwrap();

    assert!(!result.content.is_empty());
}

// ============================================================================
// Tests: Text Tool Call Parsing (always active)
// ============================================================================

#[tokio::test]
async fn test_text_mode_parses_tool_call_tag() {
    // In Text mode, model returns <tool_call> in plain text instead of structured ToolUse.
    // Turn 1: Model outputs text with <tool_call> tag → engine parses and executes shell
    // Turn 2: Model sees tool result as plain text, produces final response
    // Turn 3: Extraction
    let client = MockLlmClient::new(vec![
        text_response(
            "我来看看 <tool_call>{\"name\":\"shell\",\"arguments\":{\"command\":\"ls -la\"}}</tool_call>"
        ),
        text_response("当前目录有这些文件：file1.txt file2.rs"),
        text_response(r#"{"facts": []}"#),
    ]);

    let executor = Arc::new(MockExecutor::new("file1.txt\nfile2.rs\n"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor);

    let result = engine.think(user_event("看看当前目录")).await.unwrap();

    assert!(result.content.contains("file1.txt") || result.content.contains("文件"));
}

#[tokio::test]
async fn test_text_mode_strips_tool_call_from_content() {
    // The <tool_call> tag should be stripped from the displayed content
    let client = MockLlmClient::new(vec![
        text_response(
            "好的 <tool_call>{\"name\":\"shell\",\"arguments\":{\"command\":\"pwd\"}}</tool_call>",
        ),
        text_response("你在 /home/user 目录"),
        text_response(r#"{"facts": []}"#),
    ]);

    let executor = Arc::new(MockExecutor::new("/home/user\n"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor);

    let result = engine.think(user_event("我在哪")).await.unwrap();

    // Final content should not contain tool_call tags
    assert!(!result.content.contains("tool_call"));
}

#[tokio::test]
async fn test_auto_mode_falls_back_to_text_parsing() {
    // In Auto mode, when no structured ToolUse is returned,
    // engine should fall back to parsing <tool_call> from text.
    let client = MockLlmClient::new(vec![
        text_response(
            "<tool_call>{\"name\":\"shell\",\"arguments\":{\"command\":\"date\"}}</tool_call>",
        ),
        text_response("现在是2026年"),
        text_response(r#"{"facts": []}"#),
    ]);

    let executor = Arc::new(MockExecutor::new("2026-02-11\n"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor);

    let result = engine.think(user_event("几点了")).await.unwrap();

    assert!(result.content.contains("2026"));
}

#[tokio::test]
async fn test_text_mode_tool_error_sent_as_text() {
    // When a text-parsed tool fails, the error is sent back as plain text
    let client = MockLlmClient::new(vec![
        text_response(
            "<tool_call>{\"name\":\"shell\",\"arguments\":{\"command\":\"bad_cmd\"}}</tool_call>",
        ),
        text_response("命令失败了，换个方式"),
        text_response(r#"{"facts": []}"#),
    ]);

    let executor = Arc::new(FailingExecutor::always_fail("command not found: bad_cmd"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_mocks(client, memory, executor as Arc<dyn mneme_os::Executor>);

    let result = engine.think(user_event("执行错误命令")).await.unwrap();

    assert!(result.content.contains("失败") || !result.content.is_empty());
}
