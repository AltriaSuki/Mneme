//! Mock LLM Provider — deterministic responses for testing without API keys.

use crate::api_types::{ContentBlock, Message, MessagesResponse, StreamEvent, Tool};
use crate::llm::{CompletionParams, LlmClient};
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct MockProvider {
    model: String,
}

impl MockProvider {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl LlmClient for MockProvider {
    async fn complete(
        &self,
        _system: &str,
        _messages: Vec<Message>,
        _tools: Vec<Tool>,
        _params: CompletionParams,
    ) -> Result<MessagesResponse> {
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        Ok(MessagesResponse {
            content: vec![ContentBlock::Text {
                text: format!("(Mock {} Response) I received your prompt.", self.model),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        })
    }

    async fn stream_complete(
        &self,
        _system: &str,
        _messages: Vec<Message>,
        _tools: Vec<Tool>,
        _params: CompletionParams,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let model = self.model.clone();
        tokio::spawn(async move {
            let _ = tx
                .send(StreamEvent::TextDelta(format!(
                    "(Mock {} Response) I received your prompt.",
                    model
                )))
                .await;
            let _ = tx
                .send(StreamEvent::Done {
                    stop_reason: Some("end_turn".into()),
                })
                .await;
        });
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_complete() {
        let provider = MockProvider::new("test-model");
        let resp = provider
            .complete("system", vec![], vec![], CompletionParams::default())
            .await
            .unwrap();
        assert_eq!(resp.content.len(), 1);
        if let ContentBlock::Text { text } = &resp.content[0] {
            assert!(text.contains("Mock"));
            assert!(text.contains("test-model"));
        } else {
            panic!("Expected Text block");
        }
    }

    #[tokio::test]
    async fn test_mock_stream() {
        let provider = MockProvider::new("test-model");
        let mut rx = provider
            .stream_complete("system", vec![], vec![], CompletionParams::default())
            .await
            .unwrap();
        let mut got_text = false;
        let mut got_done = false;
        while let Some(ev) = rx.recv().await {
            match ev {
                StreamEvent::TextDelta(t) => {
                    assert!(t.contains("Mock"));
                    got_text = true;
                }
                StreamEvent::Done { .. } => got_done = true,
                _ => {}
            }
        }
        assert!(got_text);
        assert!(got_done);
    }
}
