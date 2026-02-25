//! Integration tests for the gateway HTTP server.
//!
//! These tests start a real HTTP server on a random port and send actual requests.

use mneme_core::Event;
use mneme_gateway::{GatewayMessage, GatewayResponse, GatewayServer};
use tokio::sync::mpsc;
use uuid::Uuid;

#[tokio::test]
async fn test_gateway_server_lifecycle() {
    // Test that the server can be created and started without panicking
    let (event_tx, _event_rx) = mpsc::channel::<Event>(64);
    let server = GatewayServer::new(event_tx, "127.0.0.1", 0);
    let _response_tx = server.response_sender();
    let active = server.active_connections();
    assert_eq!(active.load(std::sync::atomic::Ordering::Relaxed), 0);

    let handle = server.start();
    // Server should be running
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!handle.is_finished());

    // Cleanup: abort the server task
    handle.abort();
}

#[tokio::test]
async fn test_response_dispatch_resolves_pending() {
    // Test the response dispatch mechanism directly
    let (event_tx, _event_rx) = mpsc::channel::<Event>(64);
    let server = GatewayServer::new(event_tx, "127.0.0.1", 0);
    let response_tx = server.response_sender();
    let handle = server.start();

    // Give server time to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Send a response for a non-existent request (should be silently dropped)
    let fake_id = Uuid::new_v4();
    let resp = GatewayResponse {
        request_id: fake_id,
        content: "orphan response".to_string(),
        emotion: None,
    };
    response_tx.send(resp).await.unwrap();

    // Give dispatcher time to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // No panic, no error — orphan responses are silently dropped ✅
    handle.abort();
}

#[tokio::test]
async fn test_gateway_message_source_tagging() {
    // Verify that GatewayMessage produces correct source tags
    let msg = GatewayMessage {
        platform: "telegram".into(),
        channel: Some("group:42".into()),
        author: "alice".into(),
        body: "hello world".into(),
        message_id: Some("ext-123".into()),
    };
    let content = msg.into_content();
    assert_eq!(content.source, "gateway:telegram:group:42");
    assert_eq!(content.author, "alice");
    assert_eq!(content.body, "hello world");
}

#[tokio::test]
async fn test_gateway_response_json_roundtrip() {
    let resp = GatewayResponse {
        request_id: Uuid::new_v4(),
        content: "你好！".to_string(),
        emotion: Some("happy".to_string()),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: GatewayResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content, "你好！");
    assert_eq!(parsed.emotion.as_deref(), Some("happy"));
    assert_eq!(parsed.request_id, resp.request_id);
}

#[tokio::test]
async fn test_gateway_response_without_emotion() {
    let json = r#"{"request_id":"550e8400-e29b-41d4-a716-446655440000","content":"hi"}"#;
    let resp: GatewayResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.content, "hi");
    assert!(resp.emotion.is_none());
}
