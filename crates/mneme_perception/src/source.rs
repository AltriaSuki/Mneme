use anyhow::Result;
use async_trait::async_trait;
use mneme_core::Content;
use std::sync::Arc;
use tokio::sync::Mutex;

#[async_trait]
pub trait Source: Send + Sync {
    /// unique identifier for the source (e.g., "rss:techcrunch")
    fn name(&self) -> &str;
    
    /// Polling interval in seconds. 0 means manual trigger only.
    fn interval(&self) -> u64;

    /// Fetch new content since the last check
    async fn fetch(&self) -> Result<Vec<Content>>;
}

pub struct SourceManager {
    sources: Mutex<Vec<Arc<dyn Source>>>,
}

impl SourceManager {
    pub fn new() -> Self {
        Self {
            sources: Mutex::new(Vec::new()),
        }
    }

    pub async fn add_source(&self, source: Arc<dyn Source>) {
        self.sources.lock().await.push(source);
    }

    pub async fn collect_all(&self) -> Vec<Content> {
        let sources = self.sources.lock().await;
        let mut all_content = Vec::new();

        for source in sources.iter() {
            match source.fetch().await {
                Ok(mut contents) => all_content.append(&mut contents),
                Err(e) => {
                    tracing::error!("Failed to fetch from source {}: {}", source.name(), e);
                }
            }
        }

        all_content
    }
}
