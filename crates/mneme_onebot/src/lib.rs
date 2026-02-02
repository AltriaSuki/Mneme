use std::time::Duration;
use anyhow::{Result, Context};
use async_trait::async_trait;
use mneme_core::{Perception, Event};

pub struct OneBot {
    api_url: String,
    access_token: Option<String>,
    client: reqwest::Client,
}

impl OneBot {
    pub fn new(api_url: &str, access_token: Option<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;
            
        Ok(Self {
            api_url: api_url.to_string(),
            access_token,
            client,
        })
    }
    
    /// Send a private message
    pub async fn send_private_msg(&self, user_id: i64, message: &str) -> Result<()> {
        let url = format!("{}/send_private_msg", self.api_url);
        let json = serde_json::json!({
            "user_id": user_id,
            "message": message,
        });
        
        let mut req = self.client.post(&url).json(&json);
        
        if let Some(token) = &self.access_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let response = req.send().await.context("Failed to send message")?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OneBot API error: {} - {}", status, body);
        }
            
        Ok(())
    }
}

#[async_trait]
impl Perception for OneBot {
    async fn listen(&self) -> Result<Event> {
        // WebSocket listener not yet implemented
        anyhow::bail!("OneBot listen not yet implemented - use WebSocket integration")
    }
}
