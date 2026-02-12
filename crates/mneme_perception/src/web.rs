use crate::source::{validate_url, Source};
use anyhow::{Context, Result};
use async_trait::async_trait;
use mneme_core::{Content, Modality};
use std::io::Cursor;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;
use uuid::Uuid;

pub struct WebSource {
    url: String,
    name: String,
    client: reqwest::Client,
}

impl WebSource {
    pub fn new(url: &str) -> Result<Self> {
        validate_url(url)?;

        let parsed = Url::parse(url)?;
        // Simple name derivation from domain
        let domain = parsed.host_str().unwrap_or("unknown");
        let name = format!("web:{}", domain);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;

        Ok(Self {
            url: url.to_string(),
            name,
            client,
        })
    }
}

#[async_trait]
impl Source for WebSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn interval(&self) -> u64 {
        0
    }

    async fn fetch(&self) -> Result<Vec<Content>> {
        let html = self
            .client
            .get(&self.url)
            .send()
            .await
            .context("Failed to fetch page")?
            .text()
            .await?;

        let mut cursor = Cursor::new(html);
        let extractor = readability::extractor::extract(&mut cursor, &Url::parse(&self.url)?)
            .context("Failed to extract content")?;

        let body = format!(
            "Title: {}\nLink: {}\nContent: {}",
            extractor.title, self.url, extractor.text
        );

        // Deterministic UUID for web pages based on URL
        let id = Uuid::new_v5(&Uuid::NAMESPACE_URL, self.url.as_bytes());

        Ok(vec![Content {
            id,
            source: "web".to_string(),
            author: "Unknown".to_string(),
            body,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64,
            modality: Modality::Text,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_web_fetch() {
        let mock_server = MockServer::start().await;

        let html_body = r#"
        <html>
            <head><title>Test Page</title></head>
            <body>
                <h1>Test Header</h1>
                <p>This is the test content.</p>
            </body>
        </html>
        "#;

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(html_body))
            .mount(&mock_server)
            .await;

        let source = WebSource::new(&mock_server.uri()).expect("Valid URL");
        let content = source.fetch().await.expect("Failed to fetch");

        assert_eq!(content.len(), 1);
        assert!(content[0].body.contains("Test Page"));
        assert!(content[0].body.contains("test content"));
    }

    #[test]
    fn test_web_ssrf_prevention() {
        // Localhost is allowed in tests via validate_url's cfg!(test) check
        assert!(WebSource::new("http://localhost/admin").is_ok());
        assert!(WebSource::new("http://127.0.0.1/admin").is_ok());

        // Private networks should be blocked
        assert!(WebSource::new("http://169.254.169.254/meta").is_err());
        assert!(WebSource::new("http://192.168.1.1/router").is_err());

        // Public valid
        assert!(WebSource::new("http://google.com").is_ok());
    }
}
