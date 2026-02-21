//! Standalone OneBot ↔ Gateway bridge.
//!
//! Connects to a OneBot server (WebSocket) and forwards messages to
//! Mneme's Gateway HTTP endpoint, routing responses back.

use anyhow::Result;
use clap::Parser;
use mneme_gateway::types::{GatewayMessage, GatewayResponse};
use mneme_onebot::OneBotClient;
use tracing::info;

#[derive(Parser)]
#[command(name = "onebot-bridge", about = "Bridge OneBot ↔ Mneme Gateway")]
struct Args {
    /// OneBot WebSocket URL (e.g. ws://127.0.0.1:8080)
    #[arg(long, env = "ONEBOT_WS_URL", default_value = "ws://127.0.0.1:8080")]
    onebot_url: String,

    /// Gateway HTTP base URL (e.g. http://127.0.0.1:3000)
    #[arg(long, env = "GATEWAY_URL", default_value = "http://127.0.0.1:3000")]
    gateway_url: String,

    /// OneBot access token (optional)
    #[arg(long, env = "ONEBOT_TOKEN")]
    token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();
    info!("Starting OneBot bridge: {} ↔ {}", args.onebot_url, args.gateway_url);

    let (client, mut content_rx) =
        OneBotClient::new(&args.onebot_url, args.token.as_deref())?;

    let http = reqwest::Client::new();
    let gateway_msg_url = format!("{}/message", args.gateway_url);

    while let Some(content) = content_rx.recv().await {
        let source = content.source.clone();
        let author = content.author.clone();

        let gw_msg = GatewayMessage {
            platform: "onebot".into(),
            channel: Some(source.clone()),
            author: content.author,
            body: content.body,
            message_id: None,
        };

        let resp = match http.post(&gateway_msg_url).json(&gw_msg).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Gateway request failed: {e}");
                continue;
            }
        };

        if let Ok(gw_resp) = resp.json::<GatewayResponse>().await {
            if let Err(e) = client.route_message(&source, &author, &gw_resp.content).await {
                tracing::error!("Failed to route response back to OneBot: {e}");
            }
        }
    }

    Ok(())
}
