use crate::types::{GatewayMessage, GatewayResponse};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use mneme_core::Event;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

/// Shared state for the gateway server.
#[derive(Clone)]
struct AppState {
    /// Send events into the organism's main event loop.
    event_tx: mpsc::Sender<Event>,
    /// Pending request→response mappings (UUID-keyed oneshot channels).
    pending: Arc<RwLock<HashMap<Uuid, oneshot::Sender<GatewayResponse>>>>,
    /// Number of active WebSocket connections.
    active_ws: Arc<AtomicUsize>,
}

/// The gateway HTTP + WebSocket server.
///
/// Bridges external platforms to Mneme's event loop via:
/// - `POST /message` — synchronous request/response
/// - `GET /ws` — WebSocket bidirectional stream
/// - `GET /health` — health check
pub struct GatewayServer {
    /// Channel for the main loop to send responses back.
    response_tx: mpsc::Sender<GatewayResponse>,
    /// Receiver the main loop reads to get inbound events.
    /// (Handed off during `start`)
    response_rx: Option<mpsc::Receiver<GatewayResponse>>,
    /// Shared pending map.
    pending: Arc<RwLock<HashMap<Uuid, oneshot::Sender<GatewayResponse>>>>,
    /// Active WebSocket connection count (shared with handlers).
    active_ws: Arc<AtomicUsize>,
    /// Event sender (cloned into axum state).
    event_tx: mpsc::Sender<Event>,
    /// Bind address.
    host: String,
    port: u16,
}

impl GatewayServer {
    /// Create a new gateway server.
    ///
    /// `event_tx` feeds into the organism's main event loop.
    pub fn new(event_tx: mpsc::Sender<Event>, host: &str, port: u16) -> Self {
        let (response_tx, response_rx) = mpsc::channel::<GatewayResponse>(256);
        let pending = Arc::new(RwLock::new(HashMap::new()));
        let active_ws = Arc::new(AtomicUsize::new(0));
        Self {
            response_tx,
            response_rx: Some(response_rx),
            pending,
            active_ws,
            event_tx,
            host: host.to_string(),
            port,
        }
    }

    /// Get a sender for delivering responses from the main loop.
    ///
    /// The main loop calls `response_tx.send(GatewayResponse { request_id, content, .. })`
    /// to resolve pending HTTP requests.
    pub fn response_sender(&self) -> mpsc::Sender<GatewayResponse> {
        self.response_tx.clone()
    }

    /// Number of active WebSocket connections.
    pub fn active_connections(&self) -> Arc<AtomicUsize> {
        self.active_ws.clone()
    }

    /// Start the server. This spawns a background task and returns the join handle.
    pub fn start(mut self) -> tokio::task::JoinHandle<()> {
        let response_rx = self
            .response_rx
            .take()
            .expect("GatewayServer::start called twice");

        let pending = self.pending.clone();
        let state = AppState {
            event_tx: self.event_tx.clone(),
            pending: pending.clone(),
            active_ws: self.active_ws.clone(),
        };

        let app = Router::new()
            .route("/health", get(health))
            .route("/message", post(handle_message))
            .route("/ws", get(ws_upgrade))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr = format!("{}:{}", self.host, self.port);

        // Spawn response dispatcher (routes responses to pending oneshot channels)
        let dispatch_pending = pending;
        tokio::spawn(async move {
            let mut rx = response_rx;
            while let Some(resp) = rx.recv().await {
                let mut map = dispatch_pending.write().await;
                if let Some(tx) = map.remove(&resp.request_id) {
                    let _ = tx.send(resp);
                } else {
                    tracing::debug!(
                        "Gateway response for unknown request_id {}",
                        resp.request_id
                    );
                }
            }
        });

        // Spawn HTTP server
        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(&addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("Gateway failed to bind {}: {}", addr, e);
                    return;
                }
            };
            tracing::info!("Gateway listening on {}", addr);
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("Gateway server error: {}", e);
            }
        })
    }
}

// ============================================================================
// Route handlers
// ============================================================================

async fn health() -> &'static str {
    "ok"
}

/// POST /message — synchronous request/response.
///
/// Accepts a GatewayMessage, converts to Event, sends to main loop,
/// waits for the response via a oneshot channel.
async fn handle_message(
    State(state): State<AppState>,
    Json(msg): Json<GatewayMessage>,
) -> Result<Json<GatewayResponse>, StatusCode> {
    let request_id = Uuid::new_v4();
    let content = msg.into_content();

    // Register pending response channel
    let (tx, rx) = oneshot::channel();
    {
        let mut pending = state.pending.write().await;
        pending.insert(request_id, tx);
    }

    // Attach request_id to content source for routing back
    let mut tagged_content = content;
    tagged_content.source = format!("{}|req:{}", tagged_content.source, request_id);

    // Send event to main loop
    if state
        .event_tx
        .send(Event::UserMessage(tagged_content))
        .await
        .is_err()
    {
        state.pending.write().await.remove(&request_id);
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    // Wait for response with timeout
    match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
        Ok(Ok(response)) => Ok(Json(response)),
        Ok(Err(_)) => {
            // Sender dropped (main loop shut down)
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
        Err(_) => {
            // Timeout
            state.pending.write().await.remove(&request_id);
            Err(StatusCode::GATEWAY_TIMEOUT)
        }
    }
}

/// GET /ws — WebSocket upgrade.
async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Handle a WebSocket connection.
///
/// Inbound JSON messages are parsed as GatewayMessage and forwarded.
/// Outbound responses are sent back as JSON.
async fn handle_ws(socket: WebSocket, state: AppState) {
    state.active_ws.fetch_add(1, Ordering::Relaxed);
    let (mut ws_tx, mut ws_rx) = socket.split();
    let pending = state.pending.clone();

    // Read loop: parse inbound messages
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                let gw_msg: GatewayMessage = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        let err = serde_json::json!({"error": format!("Invalid JSON: {}", e)});
                        let _ = ws_tx.send(Message::Text(err.to_string().into())).await;
                        continue;
                    }
                };

                let request_id = Uuid::new_v4();
                let content = gw_msg.into_content();

                // Register pending
                let (tx, rx) = oneshot::channel();
                {
                    let mut p = pending.write().await;
                    p.insert(request_id, tx);
                }

                let mut tagged_content = content;
                tagged_content.source =
                    format!("{}|req:{}", tagged_content.source, request_id);

                if state
                    .event_tx
                    .send(Event::UserMessage(tagged_content))
                    .await
                    .is_err()
                {
                    break;
                }

                // Wait for response and send back over WS
                match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
                    Ok(Ok(response)) => {
                        let json = serde_json::to_string(&response).unwrap_or_default();
                        if ws_tx.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(Err(_)) => break,
                    Err(_) => {
                        pending.write().await.remove(&request_id);
                        let err = serde_json::json!({"error": "timeout"});
                        let _ = ws_tx.send(Message::Text(err.to_string().into())).await;
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    state.active_ws.fetch_sub(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_endpoint() {
        let result = health().await;
        assert_eq!(result, "ok");
    }

    #[tokio::test]
    async fn test_gateway_server_creates() {
        let (tx, _rx) = mpsc::channel(10);
        let server = GatewayServer::new(tx, "127.0.0.1", 0);
        assert_eq!(server.host, "127.0.0.1");
        assert_eq!(server.port, 0);
    }
}
