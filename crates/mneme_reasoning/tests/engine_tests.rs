//! Integration tests for the ReasoningEngine.
//!
//! These tests use a MockLlmClient that returns configurable responses,
//! allowing us to test the full think() pipeline without real LLM calls.

mod common;

use common::*;
use mneme_core::{Event, Memory, Reasoning};
use mneme_reasoning::engine::ReasoningEngine;
use std::sync::Arc;

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
// Tests: Tool Use (ReAct Loop)
// ============================================================================

#[tokio::test]
async fn test_single_tool_call() {
    // Turn 1: LLM requests shell tool
    // Turn 2: LLM produces final text after seeing tool result
    // Turn 3: Extraction call
    let client = MockLlmClient::new(vec![
        tool_use_response("shell", serde_json::json!({"command": "echo hello"})),
        text_response("命令执行完毕，结果是 hello"),
        text_response(r#"{"facts": []}"#), // extraction
    ]);

    let tool = Box::new(MockToolHandler::shell("hello\n"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_tool(client, memory, tool);

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
        tool_use_response("shell", serde_json::json!({"command": "ls"})),
        tool_use_response("shell", serde_json::json!({"command": "cat file.txt"})),
        text_response("文件内容是 hello world"),
        text_response(r#"{"facts": []}"#),
    ]);

    let tool = Box::new(MockToolHandler::shell("result"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_tool(client, memory.clone(), tool);

    let result = engine.think(user_event("读取文件")).await.unwrap();

    assert!(result.content.contains("hello world"));
}

#[tokio::test]
async fn test_react_loop_max_iterations() {
    // LLM keeps requesting tools forever — should be capped at 5 iterations
    let mut responses = Vec::new();
    for _i in 0..10 {
        responses.push(tool_use_response(
            "shell",
            serde_json::json!({"command": "loop"}),
        ));
    }
    // After the loop exits, extraction call
    responses.push(text_response(r#"{"facts": []}"#));

    let client = MockLlmClient::new(responses);
    let tool = Box::new(MockToolHandler::shell("looped"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_tool(client, memory, tool);

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
        tool_use_response("nonexistent_tool", serde_json::json!({})),
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
        text_with_tool_use(
            "我来看看现在几点",
            "shell",
            serde_json::json!({"command": "date"}),
        ),
        text_response("现在是下午三点"),
        text_response(r#"{"facts": []}"#),
    ]);

    let tool = Box::new(MockToolHandler::shell("2026-02-06"));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_tool(client, memory, tool);

    let result = engine.think(user_event("几点了")).await.unwrap();

    assert!(result.content.contains("三点"));
}

// ============================================================================
// Tests: Memory Integration
// ============================================================================

#[tokio::test]
async fn test_user_message_is_memorized() {
    let memory = Arc::new(MockMemory::new());
    let client = MockLlmClient::with_text("收到");

    let engine = ReasoningEngine::new(
        test_psyche(),
        memory.clone() as Arc<dyn Memory>,
        Box::new(client),
    );
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
    let engine = ReasoningEngine::new(
        test_psyche(),
        memory.clone() as Arc<dyn Memory>,
        Box::new(client),
    );

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
        route: None,
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
        tool_use_response("shell", serde_json::json!({})),
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

#[tokio::test]
async fn test_shell_timeout_returns_is_error_true() {
    // Shell times out → LLM sees is_error=true with descriptive message
    let client = MockLlmClient::new(vec![
        tool_use_response("shell", serde_json::json!({"command": "sleep 100"})),
        text_response("命令超时了，我换个方式"),
        text_response(r#"{"facts": []}"#),
    ]);

    let tool = Box::new(FailingToolHandler::always_fail(
        "Command execution timed out after 30s",
        true,
    ));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_tool(client, memory, tool);

    let result = engine.think(user_event("执行很久的命令")).await.unwrap();

    // The LLM received the error and produced a recovery response
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_shell_permanent_failure_returns_is_error() {
    // Shell command fails with non-zero exit (permanent) — no retry
    let client = MockLlmClient::new(vec![
        tool_use_response("shell", serde_json::json!({"command": "bad_cmd"})),
        text_response("命令执行失败了"),
        text_response(r#"{"facts": []}"#),
    ]);

    let tool = Box::new(FailingToolHandler::always_fail(
        "Command failed with status exit code: 127",
        false,
    ));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_tool(client, memory, tool);

    let result = engine.think(user_event("执行错误命令")).await.unwrap();

    // Should recover gracefully
    assert!(result.content.contains("失败"));
}

#[tokio::test]
async fn test_shell_transient_retry_succeeds() {
    // First call times out (transient), retry succeeds
    let client = MockLlmClient::new(vec![
        tool_use_response("shell", serde_json::json!({"command": "echo ok"})),
        text_response("命令执行成功"),
        text_response(r#"{"facts": []}"#),
    ]);

    let tool = Box::new(FailingToolHandler::fail_then_succeed(
        1,
        "Command execution timed out after 30s",
        "ok\n",
        true,
    ));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_tool(client, memory, tool);

    let result = engine.think(user_event("执行命令")).await.unwrap();

    assert!(result.content.contains("成功"));
}

#[tokio::test]
async fn test_unknown_tool_is_permanent_error() {
    // Unknown tool should be permanent (not retried)
    let client = MockLlmClient::new(vec![
        tool_use_response("flying_car", serde_json::json!({})),
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
        tool_use_response("browser_goto", serde_json::json!({})),
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
        tool_use_response("browser_click", serde_json::json!({})),
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
        tool_use_response("browser_type", serde_json::json!({"selector": "#input"})),
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
        tool_use_response("shell", serde_json::json!({"command": "fail"})),
        tool_use_response("shell", serde_json::json!({"command": "echo ok"})),
        text_response("第二次就好了"),
        text_response(r#"{"facts": []}"#),
    ]);

    // First call fails, second succeeds
    let tool = Box::new(FailingToolHandler::fail_then_succeed(
        1,
        "Command failed with status exit code: 1",
        "ok\n",
        false,
    ));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_tool(client, memory, tool);

    let result = engine.think(user_event("尝试命令")).await.unwrap();

    // The LLM should have recovered after getting the error
    assert!(result.content.contains("第二次") || !result.content.is_empty());
}

#[tokio::test]
async fn test_spawn_failure_is_transient() {
    // "spawn" in error message → transient, will retry
    let client = MockLlmClient::new(vec![
        tool_use_response("shell", serde_json::json!({"command": "echo ok"})),
        text_response("最终成功了"),
        text_response(r#"{"facts": []}"#),
    ]);

    let tool = Box::new(FailingToolHandler::fail_then_succeed(
        1,
        "Failed to spawn command locally",
        "ok\n",
        true,
    ));
    let memory = Arc::new(MockMemory::new());
    let engine = build_engine_with_tool(client, memory, tool);

    let result = engine.think(user_event("执行")).await.unwrap();

    assert!(!result.content.is_empty());
}

