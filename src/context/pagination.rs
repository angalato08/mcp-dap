use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A cached pre-truncation value for pagination.
pub struct CacheEntry {
    /// The full JSON value before truncation.
    pub value: serde_json::Value,
    /// The original expression that produced this value.
    pub expression: String,
    /// The type name from the debug adapter.
    pub type_name: String,
    /// Total number of items (array length or object key count).
    pub total_count: usize,
    /// Whether this is an array (true) or object (false).
    pub is_array: bool,
    /// When this entry was created.
    pub created_at: SystemTime,
}

/// In-memory cache for paginating large debug results.
pub struct PaginationCache {
    entries: HashMap<String, CacheEntry>,
    max_entries: usize,
    ttl: Duration,
}

impl PaginationCache {
    pub fn new(max_entries: usize, ttl_secs: u64) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries,
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Insert a cache entry and return its pagination token.
    /// Evicts the oldest entry if at capacity.
    pub fn insert(&mut self, entry: CacheEntry) -> String {
        if self.entries.len() >= self.max_entries {
            self.evict_oldest();
        }
        let token = generate_token();
        self.entries.insert(token.clone(), entry);
        token
    }

    /// Look up an entry by token. Returns `None` if not found or expired.
    /// Lazily prunes the expired entry on access.
    pub fn get(&mut self, token: &str) -> Option<&CacheEntry> {
        // Check expiry first, remove if expired.
        let expired = self
            .entries
            .get(token)
            .is_some_and(|e| e.created_at.elapsed().unwrap_or_default() > self.ttl);
        if expired {
            self.entries.remove(token);
            return None;
        }
        self.entries.get(token)
    }

    /// Drop all cached entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Evict the oldest entry by creation time.
    fn evict_oldest(&mut self) {
        if let Some(oldest_key) = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.created_at)
            .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest_key);
        }
    }
}

/// Generate a unique pagination token: `pg_` + timestamp-nanos hex + counter hex.
fn generate_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("pg_{ts:x}{count:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_entry(expr: &str) -> CacheEntry {
        CacheEntry {
            value: json!([1, 2, 3]),
            expression: expr.into(),
            type_name: "list".into(),
            total_count: 3,
            is_array: true,
            created_at: SystemTime::now(),
        }
    }

    #[test]
    fn insert_and_get() {
        let mut cache = PaginationCache::new(10, 300);
        let token = cache.insert(make_entry("x"));
        assert!(token.starts_with("pg_"));
        let entry = cache.get(&token).unwrap();
        assert_eq!(entry.expression, "x");
        assert_eq!(entry.total_count, 3);
    }

    #[test]
    fn missing_token_returns_none() {
        let mut cache = PaginationCache::new(10, 300);
        assert!(cache.get("pg_nonexistent").is_none());
    }

    #[test]
    fn ttl_expiry() {
        let mut cache = PaginationCache::new(10, 0); // 0s TTL
        let entry = CacheEntry {
            value: json!([1]),
            expression: "y".into(),
            type_name: "list".into(),
            total_count: 1,
            is_array: true,
            created_at: SystemTime::now() - Duration::from_secs(1),
        };
        let token = cache.insert(entry);
        assert!(cache.get(&token).is_none());
    }

    #[test]
    fn capacity_eviction() {
        let mut cache = PaginationCache::new(2, 300);
        let t1 = cache.insert(CacheEntry {
            value: json!([1]),
            expression: "first".into(),
            type_name: "list".into(),
            total_count: 1,
            is_array: true,
            created_at: SystemTime::now() - Duration::from_secs(10),
        });
        let _t2 = cache.insert(make_entry("second"));
        // At capacity — next insert evicts oldest (t1).
        let _t3 = cache.insert(make_entry("third"));
        assert!(cache.get(&t1).is_none());
    }

    #[test]
    fn clear_drops_all() {
        let mut cache = PaginationCache::new(10, 300);
        cache.insert(make_entry("a"));
        cache.insert(make_entry("b"));
        cache.clear();
        assert_eq!(cache.entries.len(), 0);
    }

    #[test]
    fn unique_tokens() {
        let t1 = generate_token();
        let t2 = generate_token();
        assert_ne!(t1, t2);
    }
}
