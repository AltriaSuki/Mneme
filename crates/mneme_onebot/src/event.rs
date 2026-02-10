use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "post_type")]
pub enum OneBotEvent {
    #[serde(rename = "message")]
    Message(MessageEvent),
    #[serde(rename = "meta_event")]
    Meta(MetaEvent),
    #[serde(rename = "notice")]
    Notice(serde_json::Value),
    #[serde(rename = "request")]
    Request(serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEvent {
    pub message_type: String, // "private" or "group"
    pub sub_type: Option<String>,
    pub message_id: i32,
    pub user_id: i64,
    pub group_id: Option<i64>,
    pub raw_message: String,
    pub font: i32,
    pub sender: Sender,
    pub time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sender {
    pub user_id: Option<i64>,
    pub nickname: Option<String>,
    pub card: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "meta_event_type")]
pub enum MetaEvent {
    #[serde(rename = "heartbeat")]
    Heartbeat {
        time: i64,
        status: serde_json::Value,
        interval: i64,
    },
    #[serde(rename = "lifecycle")]
    Lifecycle {
        time: i64,
        sub_type: String,
    },
}

#[derive(Debug, Serialize)]
pub struct SendMessageAction {
    pub action: String,
    pub params: SendMessageParams,
}

#[derive(Debug, Serialize)]
pub struct SendMessageParams {
    pub message_type: String,
    pub user_id: Option<i64>,
    pub group_id: Option<i64>,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneBotResponse {
    pub status: String,
    pub retcode: i32,
    pub data: Option<serde_json::Value>,
    pub message: String,
    pub wording: String,
    pub echo: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_message_event() {
        let json = r#"{
            "post_type": "message",
            "message_type": "group",
            "sub_type": "normal",
            "message_id": 12345,
            "user_id": 100001,
            "group_id": 200001,
            "raw_message": "hello world",
            "font": 0,
            "sender": {
                "user_id": 100001,
                "nickname": "TestUser",
                "card": "TestCard",
                "role": "member"
            },
            "time": 1700000000
        }"#;

        let event: OneBotEvent = serde_json::from_str(json).unwrap();
        match event {
            OneBotEvent::Message(msg) => {
                assert_eq!(msg.message_type, "group");
                assert_eq!(msg.user_id, 100001);
                assert_eq!(msg.group_id, Some(200001));
                assert_eq!(msg.raw_message, "hello world");
                assert_eq!(msg.sender.nickname, Some("TestUser".to_string()));
            }
            _ => panic!("Expected Message event"),
        }
    }

    #[test]
    fn test_parse_meta_heartbeat() {
        let json = r#"{
            "post_type": "meta_event",
            "meta_event_type": "heartbeat",
            "time": 1700000000,
            "status": {"online": true},
            "interval": 5000
        }"#;

        let event: OneBotEvent = serde_json::from_str(json).unwrap();
        match event {
            OneBotEvent::Meta(MetaEvent::Heartbeat { time, interval, .. }) => {
                assert_eq!(time, 1700000000);
                assert_eq!(interval, 5000);
            }
            _ => panic!("Expected Heartbeat meta event"),
        }
    }

    #[test]
    fn test_parse_meta_lifecycle() {
        let json = r#"{
            "post_type": "meta_event",
            "meta_event_type": "lifecycle",
            "time": 1700000000,
            "sub_type": "connect"
        }"#;

        let event: OneBotEvent = serde_json::from_str(json).unwrap();
        match event {
            OneBotEvent::Meta(MetaEvent::Lifecycle { time, sub_type }) => {
                assert_eq!(time, 1700000000);
                assert_eq!(sub_type, "connect");
            }
            _ => panic!("Expected Lifecycle meta event"),
        }
    }

    #[test]
    fn test_parse_unknown_event() {
        // Notice and Request variants accept arbitrary JSON via serde_json::Value
        let json = r#"{
            "post_type": "notice",
            "notice_type": "group_increase",
            "time": 1700000000
        }"#;

        let event: OneBotEvent = serde_json::from_str(json).unwrap();
        match event {
            OneBotEvent::Notice(val) => {
                assert_eq!(val["notice_type"], "group_increase");
            }
            _ => panic!("Expected Notice event"),
        }
    }

    #[test]
    fn test_send_message_action_serialize() {
        let action = SendMessageAction {
            action: "send_msg".to_string(),
            params: SendMessageParams {
                message_type: "group".to_string(),
                user_id: None,
                group_id: Some(200001),
                message: "hello".to_string(),
            },
        };

        let json = serde_json::to_value(&action).unwrap();
        assert_eq!(json["action"], "send_msg");
        assert_eq!(json["params"]["message_type"], "group");
        assert_eq!(json["params"]["group_id"], 200001);
        assert!(json["params"]["user_id"].is_null());
        assert_eq!(json["params"]["message"], "hello");
    }

    #[test]
    fn test_onebot_response_parse() {
        let json = r#"{
            "status": "ok",
            "retcode": 0,
            "data": {"message_id": 99},
            "message": "",
            "wording": "",
            "echo": "test-echo-id"
        }"#;

        let resp: OneBotResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.retcode, 0);
        assert!(resp.data.is_some());
        assert_eq!(resp.data.unwrap()["message_id"], 99);
        assert_eq!(resp.echo, Some("test-echo-id".to_string()));
    }
}