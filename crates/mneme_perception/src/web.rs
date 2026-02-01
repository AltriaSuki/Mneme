// TODO: Implement web extraction using scraper or readability
// For now this is a placeholder module to allow crate compilation

use crate::source::Source;
use anyhow::{Context, Result};
use async_trait::async_trait;
use mneme_core::{Content, Modality};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use std::io::Cursor;

pub struct WebSource {
    url: String,
}

impl WebSource {
    pub fn new(url: &str) -> Self {
        Self { url: url.to_string() }
    }
}

#[async_trait]
impl Source for WebSource {
    fn name(&self) -> &str {
        "web"
    }
    
    fn interval(&self) -> u64 { 0 }

    async fn fetch(&self) -> Result<Vec<Content>> {
         let html = reqwest::get(&self.url)
            .await
            .context("Failed to fetch page")?
            .text()
            .await?;

        let mut cursor = Cursor::new(html);
        let extractor = readability::extractor::extract(&mut cursor, &reqwest::Url::parse(&self.url)?)
            .context("Failed to extract content")?;

        let body = format!("Title: {}\nLink: {}\nContent: {}", extractor.title, self.url, extractor.text);

        Ok(vec![Content {
            id: Uuid::new_v4(),
            source: "web".to_string(),
            author: "Unknown".to_string(),
            body,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64,
            modality: Modality::Text,
        }])
    }
}

