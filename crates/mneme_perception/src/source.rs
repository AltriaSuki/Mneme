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

        // Check for private IPs
        if let Ok(ip) = host_str.parse::<IpAddr>() {
            if is_private_ip(ip) {
                anyhow::bail!("Private network addresses are not allowed");
            }
        } else {
            // It's a domain name. 
            // Ideally we'd resolve it to check the IP, but for now we block known localhost strings.
            // DNS resolution is complex to do synchronously without side effects.
            // Production systems should resolve and check IP.
            if host_str == "localhost" {
                anyhow::bail!("Localhost is not allowed");
            }
        }
    }

    Ok(())
}

fn is_private_ip(ip: IpAddr) -> bool {
    // 10.0.0.0/8
    // 172.16.0.0/12
    // 192.168.0.0/16
    // 169.254.0.0/16
    // fc00::/7 (Unique Local)
    // Loopback
    // Unspecified
    
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }

    match ip {
        IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();
            // 10.0.0.0/8
            if octets[0] == 10 { return true; }
            // 172.16.0.0/12
            if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31 { return true; }
            // 192.168.0.0/16
            if octets[0] == 192 && octets[1] == 168 { return true; }
            // 169.254.0.0/16
            if octets[0] == 169 && octets[1] == 254 { return true; }
            false
        }
        IpAddr::V6(ipv6) => {
            // fc00::/7
            let octets = ipv6.octets();
             // fc00::/7 includes fc00:: to fdff::
             // fc = 1111 1100, fd = 1111 1101. So first 7 bits are 1111 110.
            if (octets[0] & 0xFE) == 0xFC { return true; }
            false
        }
    }
}
