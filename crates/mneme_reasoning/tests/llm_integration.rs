//! Real LLM integration tests.
//!
//! These tests make actual API calls and are marked `#[ignore]` so they
//! don't run in CI. Run manually with:
//!
//!   cargo test -p mneme_reasoning --test llm_integration -- --ignored
//!
//! Requires a `.env` file (or env vars) with:
//!   ANTHROPIC_API_KEY, ANTHROPIC_BASE_URL (optional), ANTHROPIC_MODEL (optional)

use mneme_reasoning::api_types::{ContentBlock, Message, Role, StreamEvent};
use mneme_reasoning::llm::{CompletionParams, LlmClient};
use mneme_reasoning::providers::anthropic::AnthropicClient;

/// Load .env and build a real AnthropicClient from environment.
fn real_client() -> AnthropicClient {
    dotenv::dotenv().ok();
    let model = std::env::var("ANTHROPIC_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-5-20250929".to_string());
    let base_url = std::env::var("ANTHROPIC_BASE_URL").ok();
    AnthropicClient::new(&model, 30, base_url.as_deref())
        .expect("Failed to create AnthropicClient")
}

fn simple_user_message(text: &str) -> Vec<Message> {
    vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
    }]
}

// ============================================================================
// Non-streaming completion
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_real_anthropic_complete() {
    let client = real_client();
    let params = CompletionParams {
        max_tokens: 128,
        temperature: 0.0,
        tool_choice: None,
    };

    let response = client
        .complete(
            "You are a helpful assistant. Reply in one short sentence.",
            simple_user_message("What is 2+2?"),
            vec![],
            params,
        )
        .await
        .expect("API call failed");

    // Should have at least one text block containing "4"
    let text = response
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    assert!(!text.is_empty(), "Response should not be empty");
    assert!(text.contains('4'), "Response should mention 4, got: {text}");
    assert!(
        response.stop_reason.is_some(),
        "Should have a stop reason"
    );
}

// ============================================================================
// Streaming completion
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_real_anthropic_stream() {
    let client = real_client();
    let params = CompletionParams {
        max_tokens: 128,
        temperature: 0.0,
        tool_choice: None,
    };

    let mut rx = client
        .stream_complete(
            "You are a helpful assistant. Reply in one short sentence.",
            simple_user_message("What is 3+3?"),
            vec![],
            params,
        )
        .await
        .expect("Stream request failed");

    let mut full_text = String::new();
    let mut got_done = false;

    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::TextDelta(t) => full_text.push_str(&t),
            StreamEvent::Done { .. } => {
                got_done = true;
                break;
            }
            StreamEvent::Error(e) => panic!("Stream error: {e}"),
            _ => {}
        }
    }

    assert!(got_done, "Should receive Done event");
    assert!(!full_text.is_empty(), "Streamed text should not be empty");
    assert!(
        full_text.contains('6'),
        "Response should mention 6, got: {full_text}"
    );
}
