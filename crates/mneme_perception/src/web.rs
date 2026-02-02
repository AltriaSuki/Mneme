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
        
        if let Some(host) = parsed.host_str() {
            let is_test = cfg!(test);
            if !is_test && (host == "localhost" || host == "127.0.0.1" || host == "::1") {
                anyhow::bail!("Localhost is not allowed");
            }
            if host.starts_with("192.168.") || host.starts_with("10.") || host.starts_with("169.254.") {
                anyhow::bail!("Private network addresses are not allowed");
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method};

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
        // Localhost is allowed in tests for wiremock
        assert!(WebSource::new("http://localhost/admin").is_ok());
        assert!(WebSource::new("http://127.0.0.1/admin").is_ok());
        
        // Private networks should still be blocked
        assert!(WebSource::new("http://169.254.169.254/meta").is_err());
        assert!(WebSource::new("http://192.168.1.1/router").is_err());
        assert!(WebSource::new("http://google.com").is_ok());
    }
}

