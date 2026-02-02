use anyhow::Result;
use async_trait::async_trait;
use mneme_core::{Perception, Event};

pub struct OneBot {
    api_url: String,
    access_token: Option<String>,
    client: reqwest::Client,
}

impl OneBot {
    pub fn new(api_url: &str, access_token: Option<String>) -> Self {
        Self {
            api_url: api_url.to_string(),
            access_token,
            client: reqwest::Client::new(),
        }
    }
    
    /// Send a private message
    pub async fn send_private_msg(&self, user_id: i64, message: &str) -> Result<()> {
        let url = format!("{}/send_private_msg", self.api_url);
        let json = serde_json::json!({
            "user_id": user_id,
            "message": message,
        });
        
        if let Some(_token) = &self.access_token {
            // OneBot implementations often accept token in Authorization header or query param. 
            // We'll assume header here or handle it in client builder if needed.
        }

        self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token.clone().unwrap_or_default()))
            .json(&json)
            .send()
            .await?;
            
        Ok(())
    }
}

#[async_trait]
impl Perception for OneBot {
    async fn listen(&self) -> Result<Event> {
        // TODO: Implement WebSocket listener
        // For now, this is a placeholder
        std::future::pending().await
    }
}
