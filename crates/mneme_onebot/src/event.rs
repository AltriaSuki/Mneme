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
