use crate::source::Source;
use anyhow::{Context, Result};
use async_trait::async_trait;
use mneme_core::{Content, Modality};
use rss::Channel;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub struct RssSource {
    url: String,
    name: String,
}

impl RssSource {
    pub fn new(url: &str, name: &str) -> Self {
        Self {
            url: url.to_string(),
            name: name.to_string(),
        }
    }
}

#[async_trait]
impl Source for RssSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn interval(&self) -> u64 {
        3600 // Default 1 hour
    }

    async fn fetch(&self) -> Result<Vec<Content>> {
        let content = reqwest::get(&self.url)
            .await
            .context("Failed to fetch RSS feed")?
            .bytes()
            .await?;

        let channel = Channel::read_from(&content[..])
            .context("Failed to parse RSS feed")?;

        let mut items = Vec::new();
        // Just take top 3 for now to avoid spamming
        for item in channel.items().iter().take(3) {
            let title = item.title().unwrap_or("No Title");
            let description = item.description().unwrap_or("No Description");
            let link = item.link().unwrap_or("");
            
            let body = format!("Title: {}\nLink: {}\nSummary: {}", title, link, description);
            
            items.push(Content {
                id: Uuid::new_v4(),
                source: format!("rss:{}", self.name),
                author: channel.title().to_string(),
                body,
                timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64,
                modality: Modality::Text,
            });
        }

        Ok(items)
    }
}
