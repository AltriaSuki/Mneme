// TODO: Implement web extraction using scraper or readability
// For now this is a placeholder module to allow crate compilation

use crate::source::Source;
use anyhow::{Context, Result};
use async_trait::async_trait;
use mneme_core::{Content, Modality};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use std::io::Cursor;
use url::Url;

pub struct WebSource {
    url: String,
    name: String,
}

impl WebSource {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = Url::parse(url).context("Invalid URL")?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            anyhow::bail!("Only HTTP/HTTPS schemes are allowed");
        }
        
        // Simple name derivation from domain
        let domain = parsed.host_str().unwrap_or("unknown");
        let name = format!("web:{}", domain);

        Ok(Self { 
            url: url.to_string(),
            name,
        })
    }
}

#[async_trait]
impl Source for WebSource {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn interval(&self) -> u64 { 0 }

    async fn fetch(&self) -> Result<Vec<Content>> {
         let html = reqwest::get(&self.url)
            .await
            .context("Failed to fetch page")?
            .text()
            .await?;

        let mut cursor = Cursor::new(html);
        let extractor = readability::extractor::extract(&mut cursor, &Url::parse(&self.url)?)
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

