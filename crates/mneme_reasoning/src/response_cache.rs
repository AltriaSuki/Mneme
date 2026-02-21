//! Simple response cache for reusing answers to similar questions (#771).
//!
//! Uses normalized query text as key with TTL-based expiry.

use std::collections::HashMap;
use std::time::{Duration, Instant};

const MAX_ENTRIES: usize = 64;
const DEFAULT_TTL_SECS: u64 = 300; // 5 minutes

struct CacheEntry {
    response: String,
    created: Instant,
    hits: u32,
}

pub struct ResponseCache {
    entries: HashMap<String, CacheEntry>,
    ttl: Duration,
}

impl ResponseCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(DEFAULT_TTL_SECS),
        }
    }

    /// Normalize a query for cache lookup: lowercase, trim, collapse whitespace.
    fn normalize(query: &str) -> String {
        query.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
    }

    /// Look up a cached response. Returns None if miss or expired.
    pub fn get(&mut self, query: &str) -> Option<String> {
        let key = Self::normalize(query);
        let now = Instant::now();

        if let Some(entry) = self.entries.get_mut(&key) {
            if now.duration_since(entry.created) < self.ttl {
                entry.hits += 1;
                return Some(entry.response.clone());
            }
        }
        // Remove expired entry outside the borrow
        self.entries.remove(&key);
        None
    }

    /// Store a response in the cache.
    pub fn put(&mut self, query: &str, response: String) {
        // Evict expired entries if at capacity
        if self.entries.len() >= MAX_ENTRIES {
            let now = Instant::now();
            self.entries.retain(|_, e| now.duration_since(e.created) < self.ttl);
        }
        // If still full, evict oldest
        if self.entries.len() >= MAX_ENTRIES {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.created)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest_key);
            }
        }

        let key = Self::normalize(query);
        self.entries.insert(
            key,
            CacheEntry {
                response,
                created: Instant::now(),
                hits: 0,
            },
        );
    }

    /// Number of cached entries (for diagnostics).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total cache hits across all entries.
    pub fn total_hits(&self) -> u32 {
        self.entries.values().map(|e| e.hits).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_put_get() {
        let mut cache = ResponseCache::new();
        cache.put("Hello world", "response1".into());
        assert_eq!(cache.get("Hello world").as_deref(), Some("response1"));
        assert_eq!(cache.get("hello  world").as_deref(), Some("response1")); // normalized
    }

    #[test]
    fn test_cache_miss() {
        let mut cache = ResponseCache::new();
        assert_eq!(cache.get("nonexistent"), None);
    }

    #[test]
    fn test_cache_eviction() {
        let mut cache = ResponseCache::new();
        for i in 0..MAX_ENTRIES + 5 {
            cache.put(&format!("query {}", i), format!("resp {}", i));
        }
        assert!(cache.len() <= MAX_ENTRIES);
    }
}
