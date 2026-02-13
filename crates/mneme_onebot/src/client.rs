use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use mneme_core::{Content, Modality};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;
use uuid::Uuid;

use crate::event::{OneBotEvent, SendMessageAction, SendMessageParams};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Maximum number of pending messages buffered during disconnect.
const PENDING_QUEUE_CAPACITY: usize = 256;

/// Bounded message queue for buffering outgoing messages during WebSocket disconnect.
/// When full, the oldest message is dropped to make room.
#[derive(Clone)]
pub struct PendingMessageQueue {
    inner: Arc<Mutex<VecDeque<String>>>,
}

impl Default for PendingMessageQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingMessageQueue {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Push a message. If at capacity, drops the oldest message first.
    pub fn push(&self, msg: String) {
        let mut q = self.inner.lock().unwrap();
        if q.len() >= PENDING_QUEUE_CAPACITY {
            let dropped = q.pop_front();
            tracing::warn!(
                "Pending queue full ({}), dropping oldest message: {:?}",
                PENDING_QUEUE_CAPACITY,
                dropped.as_deref().map(|s| &s[..s.len().min(80)])
            );
        }
        q.push_back(msg);
    }

    /// Drain all pending messages (FIFO order).
    pub fn drain_all(&self) -> Vec<String> {
        let mut q = self.inner.lock().unwrap();
        q.drain(..).collect()
    }

    /// Number of pending messages.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct OneBotClient {
    pub(crate) _ws_url: Url,
    tx: mpsc::Sender<String>,
    pending: PendingMessageQueue,
    connected: Arc<AtomicBool>,
}

impl OneBotClient {
    pub fn new(url: &str, access_token: Option<&str>) -> Result<(Self, mpsc::Receiver<Content>)> {
        let mut ws_url = Url::parse(url).context("Invalid OneBot WS URL")?;

        // Append access token as query parameter if provided
        if let Some(token) = access_token {
            ws_url.query_pairs_mut().append_pair("access_token", token);
        }

        let (tx, mut rx) = mpsc::channel::<String>(32);
        let (content_tx, content_rx) = mpsc::channel::<Content>(32);

        let pending = PendingMessageQueue::new();
        let connected = Arc::new(AtomicBool::new(false));
        let client = Self {
            _ws_url: ws_url.clone(),
            tx,
            pending: pending.clone(),
            connected: connected.clone(),
        };

        // Spawn the WebSocket handler task
        tokio::spawn(async move {
            let mut retry_count: u32 = 0;
            const MAX_RETRIES: u32 = 10;
            loop {
                tracing::info!(
                    "Connecting to OneBot at {}...",
                    ws_url.as_str().split('?').next().unwrap_or(ws_url.as_str())
                );
                match connect_async(&ws_url).await {
                    Ok((ws_stream, _)) => {
                        tracing::info!("Connected to OneBot!");
                        connected.store(true, Ordering::Relaxed);
                        retry_count = 0; // Reset on successful connection
                        if let Err(e) =
                            Self::handle_connection(ws_stream, &mut rx, &content_tx, &pending).await
                        {
                            tracing::error!("OneBot connection error: {}", e);
                        }
                        connected.store(false, Ordering::Relaxed);
                        // Connection lost, wait before reconnecting
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                    Err(e) => {
                        retry_count += 1;
                        if retry_count > MAX_RETRIES {
                            tracing::error!(
                                "OneBot: giving up after {} failed connection attempts. Last error: {}",
                                MAX_RETRIES, e
                            );
                            connected.store(false, Ordering::Relaxed);
                            return; // Circuit breaker: stop the task
                        }
                        let wait_secs = 60u64.min(2u64.pow(retry_count.min(6) + 1)); // 4s, 8s, 16s, 32s, 64→60s
                        tracing::error!(
                            "Failed to connect (attempt {}/{}): {}. Retrying in {}s...",
                            retry_count,
                            MAX_RETRIES,
                            e,
                            wait_secs
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(wait_secs)).await;
                    }
                }
            }
        });

        Ok((client, content_rx))
    }

    async fn handle_connection(
        stream: WsStream,
        rx: &mut mpsc::Receiver<String>,
        content_tx: &mpsc::Sender<Content>,
        pending: &PendingMessageQueue,
    ) -> Result<()> {
        let (mut write, mut read) = stream.split();

        // Drain pending messages buffered during disconnect
        let buffered = pending.drain_all();
        if !buffered.is_empty() {
            tracing::info!(
                "Draining {} pending messages after reconnect",
                buffered.len()
            );
            for msg in buffered {
                write.send(Message::Text(msg)).await?;
            }
        }

        loop {
            tokio::select! {
                // Incoming messages from OneBot
                Some(msg) = read.next() => {
                    let msg = msg?;
                    if let Message::Text(text) = msg {
                        // Parse event
                        // Parse event or response
                        if let Ok(event) = serde_json::from_str::<OneBotEvent>(&text) {
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
                             } else {
                                 tracing::debug!("Ignored non-message event: {:?}", event);
                             }
                        } else if let Ok(response) = serde_json::from_str::<super::event::OneBotResponse>(&text) {
                            if response.status == "ok" {
                                tracing::debug!("OneBot Action Success: {:?}", response);
                            } else {
                                tracing::warn!("OneBot Action Failed: {:?}", response);
                            }
                        } else {
                             // Downgrade to debug (#66): heartbeat messages with non-standard
                             // fields fail to parse as OneBotEvent/OneBotResponse, flooding
                             // logs with warn-level noise that drowns real protocol errors.
                             tracing::debug!("Unrecognized OneBot message: {}", text);
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
        self.try_send_or_queue(json);
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
        self.try_send_or_queue(json);
        Ok(())
    }

    /// Try to send via the mpsc channel; if full or closed, buffer in the pending queue.
    fn try_send_or_queue(&self, json: String) {
        match self.tx.try_send(json) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(msg)) => {
                tracing::warn!("OneBot send channel full, buffering message");
                self.pending.push(msg);
            }
            Err(mpsc::error::TrySendError::Closed(msg)) => {
                tracing::warn!("OneBot WS task closed, buffering message");
                self.pending.push(msg);
            }
        }
    }

    /// Whether the WebSocket connection is currently active.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Number of messages buffered while disconnected.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pending_queue_push_and_drain() {
        let q = PendingMessageQueue::new();
        assert!(q.is_empty());

        q.push("msg1".to_string());
        q.push("msg2".to_string());
        assert_eq!(q.len(), 2);

        let drained = q.drain_all();
        assert_eq!(drained, vec!["msg1", "msg2"]);
        assert!(q.is_empty());
    }

    #[test]
    fn test_pending_queue_overflow_drops_oldest() {
        let q = PendingMessageQueue::new();
        for i in 0..PENDING_QUEUE_CAPACITY {
            q.push(format!("msg{}", i));
        }
        assert_eq!(q.len(), PENDING_QUEUE_CAPACITY);

        // Push one more — should drop msg0
        q.push("overflow".to_string());
        assert_eq!(q.len(), PENDING_QUEUE_CAPACITY);

        let drained = q.drain_all();
        assert_eq!(drained[0], "msg1"); // msg0 was dropped
        assert_eq!(drained.last().unwrap(), "overflow");
    }

    #[test]
    fn test_pending_queue_empty_drain() {
        let q = PendingMessageQueue::new();
        let drained = q.drain_all();
        assert!(drained.is_empty());
    }
}
