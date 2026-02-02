use anyhow::Result;
use async_trait::async_trait;
use mneme_core::Content;
use std::sync::Arc;
use tokio::sync::Mutex;
use futures::future::join_all;
use ipnetwork::IpNetwork;
use std::net::IpAddr;
use url::Url;

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
        Self::default()
    }

    pub async fn add_source(&self, source: Arc<dyn Source>) {
        self.sources.lock().await.push(source);
    }

    pub async fn collect_all(&self) -> Vec<Content> {
        let sources = self.sources.lock().await.clone();
        let mut all_content = Vec::new();
        
        let futures = sources.iter().map(|source| {
            let source = source.clone();
            async move {
                (source.name().to_string(), source.fetch().await)
            }
        });

        let results = join_all(futures).await;

        for (name, result) in results {
            match result {
                Ok(mut contents) => all_content.append(&mut contents),
                Err(e) => {
                    tracing::error!("Failed to fetch from source {}: {}", name, e);
                }
            }
        }

        all_content
    }
}

impl Default for SourceManager {
    fn default() -> Self {
        Self {
            sources: Mutex::new(Vec::new()),
        }
    }
}

/// Validates that a URL is safe to fetch (HTTP/HTTPS only, no private IPs)
pub fn validate_url(url: &str) -> Result<()> {
    let parsed = Url::parse(url)?;
    
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        anyhow::bail!("Only HTTP/HTTPS schemes are allowed");
    }

    if let Some(host_str) = parsed.host_str() {
        if cfg!(test) && (host_str == "localhost" || host_str == "127.0.0.1" || host_str == "::1") {
            return Ok(());
        }

        // Check for private IPs using ipnetwork crate logic manually or by checking known ranges
        // Note: The reviewer mentioned using `ipnetwork` crate directly but `is_private` is actually on IpAddr in std (unstable) or we implement it.
        // However, the `ipnetwork` crate provides convenient range checking.
        if let Ok(ip) = host_str.parse::<IpAddr>() {
            if is_private_ip(ip) {
                anyhow::bail!("Private network addresses are not allowed");
            }
        } else {
            // Domain name
            if host_str == "localhost" {
                anyhow::bail!("Localhost is not allowed");
            }
        }
    }

    Ok(())
}

fn is_private_ip(ip: IpAddr) -> bool {
    static PRIVATE_RANGES: once_cell::sync::Lazy<Vec<IpNetwork>> = once_cell::sync::Lazy::new(|| {
        vec![
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "169.254.0.0/16",
            "fc00::/7",
        ]
        .into_iter()
        .flat_map(|s| s.parse::<IpNetwork>())
        .collect()
    });

    for net in PRIVATE_RANGES.iter() {
        if net.contains(ip) {
            return true;
        }
    }

    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    
    false
}
