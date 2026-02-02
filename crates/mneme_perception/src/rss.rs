use crate::source::{Source, validate_url};
use anyhow::{Context, Result};
use async_trait::async_trait;
use mneme_core::{Content, Modality};
use rss::Channel;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use uuid::Uuid;
use chrono::DateTime;

const MAX_ITEMS: usize = 3;

pub struct RssSource {
    url: String,
    name: String,
}

impl RssSource {
    pub fn new(url: &str, name: &str) -> Result<Self> {
        validate_url(url)?;

        Ok(Self {
            url: url.to_string(),
            name: name.to_string(),
        })
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;

        let content = client.get(&self.url)
            .send()
            .await
            .context("Failed to fetch RSS feed")?
            .bytes()
            .await?;

        let channel = Channel::read_from(&content[..])
            .context("Failed to parse RSS feed")?;

        let mut items = Vec::new();
        // Limit items to avoid spamming
        for item in channel.items().iter().take(MAX_ITEMS) {
            let title = item.title().unwrap_or("No Title");
            let description = item.description().unwrap_or("No Description");
            let link = item.link().unwrap_or("");
            
            let body = format!("Title: {}\nLink: {}\nSummary: {}", title, link, description);

            // Use deterministic UUID based on link for deduplication
            let id = if !link.is_empty() {
                Uuid::new_v5(&Uuid::NAMESPACE_URL, link.as_bytes())
            } else {
                Uuid::new_v4()
            };

            // Parse timestamp from pub_date if available
            let timestamp = item.pub_date()
                .and_then(|d| DateTime::parse_from_rfc2822(d).ok())
                .map(|dt| dt.timestamp())
                .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64);
            
            items.push(Content {
                id,
                source: format!("rss:{}", self.name),
                author: channel.title().to_string(),
                body,
                timestamp,
                modality: Modality::Text,
            });
        }

        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method};

    #[tokio::test]
    async fn test_rss_fetch() {
        let mock_server = MockServer::start().await;

        let rss_body = r#"
        <?xml version="1.0" encoding="UTF-8" ?>
        <rss version="2.0">
        <channel>
            <title>Test Feed</title>
            <item>
                <title>Test Item 1</title>
                <link>http://example.com/1</link>
                <description>Description 1</description>
            </item>
        </channel>
        </rss>
        "#;

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(rss_body))
            .mount(&mock_server)
            .await;

        // Use the mock server URL
        let source = RssSource::new(&mock_server.uri(), "test-rss").expect("Valid URL");
        let content = source.fetch().await.expect("Failed to fetch");

        assert_eq!(content.len(), 1);
        assert_eq!(content[0].author, "Test Feed");
        assert!(content[0].body.contains("Test Item 1"));
    }

    #[test]
    fn test_rss_dedup() {
        // Same link should produce same UUID
        let link = "http://example.com/article1";
        let uuid1 = Uuid::new_v5(&Uuid::NAMESPACE_URL, link.as_bytes());
        let uuid2 = Uuid::new_v5(&Uuid::NAMESPACE_URL, link.as_bytes());
        assert_eq!(uuid1, uuid2);

        // Different link should produce different UUID
        let link2 = "http://example.com/article2";
        let uuid3 = Uuid::new_v5(&Uuid::NAMESPACE_URL, link2.as_bytes());
        assert_ne!(uuid1, uuid3);
    }
}
