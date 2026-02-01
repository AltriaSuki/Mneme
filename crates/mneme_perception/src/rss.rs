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
    pub fn new(url: &str, name: &str) -> Result<Self> {
        let parsed = url::Url::parse(url).context("Invalid URL")?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            anyhow::bail!("Only HTTP/HTTPS schemes are allowed");
        }

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
}
