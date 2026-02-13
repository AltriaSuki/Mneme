use mneme_core::{Content, Modality};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Inbound message from any platform via the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    /// Platform identifier: "telegram", "web", "onebot", etc.
    pub platform: String,
    /// Channel within the platform: "group:123", "private:456", etc.
    #[serde(default)]
    pub channel: Option<String>,
    /// Author name or ID.
    pub author: String,
    /// Message body text.
    pub body: String,
    /// Optional external message ID for deduplication.
    #[serde(default)]
    pub message_id: Option<String>,
}

impl GatewayMessage {
    /// Convert to mneme_core Content with a gateway-prefixed source.
    pub fn into_content(self) -> Content {
        let source = match self.channel {
            Some(ref ch) => format!("gateway:{}:{}", self.platform, ch),
            None => format!("gateway:{}", self.platform),
        };
        Content {
            id: Uuid::new_v4(),
            source,
            author: self.author,
            body: self.body,
            timestamp: chrono::Utc::now().timestamp(),
            modality: Modality::Text,
        }
    }
}

/// Outbound response sent back through the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayResponse {
    /// The request ID this response corresponds to.
    pub request_id: Uuid,
    /// Response text from Mneme.
    pub content: String,
    /// Detected emotion label (if any).
    #[serde(default)]
    pub emotion: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_message_into_content_with_channel() {
        let msg = GatewayMessage {
            platform: "telegram".into(),
            channel: Some("group:123".into()),
            author: "alice".into(),
            body: "hello".into(),
            message_id: None,
        };
        let content = msg.into_content();
        assert_eq!(content.source, "gateway:telegram:group:123");
        assert_eq!(content.author, "alice");
        assert_eq!(content.body, "hello");
    }

    #[test]
    fn test_gateway_message_into_content_without_channel() {
        let msg = GatewayMessage {
            platform: "web".into(),
            channel: None,
            author: "bob".into(),
            body: "hi".into(),
            message_id: None,
        };
        let content = msg.into_content();
        assert_eq!(content.source, "gateway:web");
    }

    #[test]
    fn test_gateway_message_json_roundtrip() {
        let json = r#"{"platform":"web","author":"user","body":"test"}"#;
        let msg: GatewayMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.platform, "web");
        assert!(msg.channel.is_none());
        assert!(msg.message_id.is_none());
    }
}
