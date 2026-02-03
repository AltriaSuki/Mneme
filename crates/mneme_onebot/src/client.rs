use anyhow::{Result, Context};
use futures::{SinkExt, StreamExt};
use mneme_core::{Content, Modality};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream};
use url::Url;
use uuid::Uuid;

use crate::event::{OneBotEvent, SendMessageAction, SendMessageParams};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct OneBotClient {
    pub(crate) _ws_url: Url,
    tx: mpsc::Sender<String>, // Channel to send outgoing messages to WS task
}

impl OneBotClient {
    pub fn new(url: &str) -> Result<(Self, mpsc::Receiver<Content>)> {
        let ws_url = Url::parse(url).context("Invalid OneBot WS URL")?;
        let (tx, mut rx) = mpsc::channel::<String>(32);
        let (content_tx, content_rx) = mpsc::channel::<Content>(32);

        let client = Self { _ws_url: ws_url.clone(), tx };

        // Spawn the WebSocket handler task
        tokio::spawn(async move {
            let mut retry_count = 0;
            loop {
                tracing::info!("Connecting to OneBot at {}...", ws_url);
                match connect_async(&ws_url).await {
                    Ok((ws_stream, _)) => {
                        tracing::info!("Connected to OneBot!");
                        retry_count = 0; // Reset retry count on success
                        if let Err(e) = Self::handle_connection(ws_stream, &mut rx, &content_tx).await {
                            tracing::error!("OneBot connection error: {}", e);
                        }
                    }
                    Err(e) => {
                        let wait_secs = 5u64.min(2u64.pow(retry_count));
                        tracing::error!("Failed to connect to OneBot: {}. Retrying in {}s...", e, wait_secs);
                        tokio::time::sleep(tokio::time::Duration::from_secs(wait_secs)).await;
                        if retry_count < 6 { retry_count += 1; }
                    }
                }
                // If handle_connection returns, it means connection lost. Wait before reconnect.
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        });

        Ok((client, content_rx))
    }

    async fn handle_connection(
        stream: WsStream,
        rx: &mut mpsc::Receiver<String>,
        content_tx: &mpsc::Sender<Content>,
    ) -> Result<()> {
        let (mut write, mut read) = stream.split();

        loop {
            tokio::select! {
                // Incoming messages from OneBot
                Some(msg) = read.next() => {
                    let msg = msg?;
                    if let Message::Text(text) = msg {
                        // Parse event
                        match serde_json::from_str::<OneBotEvent>(&text) {
                             Ok(event) => {
                                 if let OneBotEvent::Message(msg_event) = event {
                                     // Determine source type (private vs group)
                                     let source = if let Some(group_id) = msg_event.group_id {
                                         format!("onebot:group:{}", group_id)
                                     } else {
                                         "onebot:private".to_string()
                                     };

                                     // Convert to mneme_core::Content
                                     let content = Content {
                                         id: Uuid::new_v4(),
                                         source,
                                         author: msg_event.user_id.to_string(),
                                         body: msg_event.raw_message,
                                         timestamp: msg_event.time,
                                         modality: Modality::Text,
                                     };
                                     
                                     // Send to reasoning engine
                                     let _ = content_tx.send(content).await;
                                 }
                             }
                             Err(_) => {
                                 // Simple debug log for unparseable events (heartbeats usually don't match OneBotEvent if filtered strict, or just noise)
                                 // To avoid spamming logs with Heartbeats if they fail parsing, we check if it is a heartbeat meta event type first or just ignore.
                                 // For now, minimal logging.
                                 tracing::debug!("Ignored non-message event or parse error.");
                             }
                        }
                    }
                }
                
                // Outgoing messages to OneBot (from Client::send)
                Some(json_payload) = rx.recv() => {
                    write.send(Message::Text(json_payload)).await?;
                }
            }
        }
    }

    pub async fn send_private_message(&self, user_id: i64, message: &str) -> Result<()> {
        let payload = SendMessageAction {
            action: "send_private_msg".to_string(),
            params: SendMessageParams {
                message_type: "private".to_string(),
                user_id: Some(user_id),
                group_id: None,
                message: message.to_string(),
            },
        };
        let json = serde_json::to_string(&payload)?;
        self.tx.send(json).await.map_err(|_| anyhow::anyhow!("WS task dropped"))?;
        Ok(())
    }

    pub async fn send_group_message(&self, group_id: i64, message: &str) -> Result<()> {
        let payload = SendMessageAction {
            action: "send_group_msg".to_string(),
            params: SendMessageParams {
                message_type: "group".to_string(),
                user_id: None,
                group_id: Some(group_id),
                message: message.to_string(),
            },
        };
        let json = serde_json::to_string(&payload)?;
        self.tx.send(json).await.map_err(|_| anyhow::anyhow!("WS task dropped"))?;
        Ok(())
    }
    
    // Extensibility: send_group_message, etc.
}
