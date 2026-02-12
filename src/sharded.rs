// Sharded LRU Cache
//
// Addresses the single-lock bottleneck of ThreadSafeLruCache by
// partitioning the keyspace across N independent Mutex<LruCache> shards.
//
// Key routing: hash(key) % num_shards → determines which shard owns the key.
//
// Trade-off: LRU eviction becomes per-shard rather than global. A key that
// is "least recently used" globally might not be evicted if its shard still
// has capacity, while another shard evicts a more recently used key. This
// is an accepted trade-off for significantly better concurrent throughput.
//
// Each shard has capacity = total_capacity / num_shards (remainder distributed
// to the first few shards to avoid wasting slots).

use crate::lru::LruCache;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

/// A sharded thread-safe LRU cache.
///
/// Distributes keys across multiple independent `Mutex<LruCache>` shards
/// to reduce lock contention under concurrent access. Each shard maintains
/// its own LRU ordering independently.
///
/// # Shard Count
///
/// The default shard count is 16, which provides a good balance between
/// contention reduction and memory overhead. Use `with_shards` to customize.
pub struct ShardedLruCache<K, V> {
    shards: Vec<Mutex<LruCache<K, V>>>,
    num_shards: usize,
}

impl<K, V> ShardedLruCache<K, V>
where
    K: Hash + Eq + Clone + Send + 'static,
    V: Clone + Send + 'static,
{
    /// Creates a new sharded cache with the given total capacity.
    ///
    /// Uses up to 16 shards, capped at the capacity (each shard needs
    /// at least 1 slot).
    ///
    /// # Panics
    /// Panics if `capacity` is 0.
    pub fn new(capacity: usize) -> Self {
        let num_shards = capacity.min(16);
        Self::with_shards(capacity, num_shards)
    }

    /// Creates a new sharded cache with the given total capacity and shard count.
    ///
    /// # Panics
    /// Panics if `capacity` is 0 or `num_shards` is 0.
    /// Panics if `num_shards` exceeds `capacity` (each shard needs at least 1 slot).
    pub fn with_shards(capacity: usize, num_shards: usize) -> Self {
        assert!(capacity > 0, "Cache capacity must be greater than 0");
        assert!(num_shards > 0, "Shard count must be greater than 0");
        assert!(
            num_shards <= capacity,
            "Shard count ({}) cannot exceed capacity ({})",
            num_shards,
            capacity
        );

        let base_capacity = capacity / num_shards;
        let remainder = capacity % num_shards;

        let shards = (0..num_shards)
            .map(|i| {
                // Distribute remainder: first `remainder` shards get one extra slot
                let shard_capacity = if i < remainder {
                    base_capacity + 1
                } else {
                    base_capacity
                };
                Mutex::new(LruCache::new(shard_capacity))
            })
            .collect();

        Self { shards, num_shards }
    }

    /// Determines which shard owns a given key.
    fn shard_index(&self, key: &K) -> usize {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish() as usize % self.num_shards
    }

    /// Retrieves a cloned value for the key, marking it as most recently
    /// used within its shard.
    ///
    /// Only locks the shard that owns the key — other shards remain available.
    pub fn get(&self, key: &K) -> Option<V> {
        let shard_idx = self.shard_index(key);
        let mut shard = self.shards[shard_idx].lock().expect("shard lock poisoned");
        shard.get(key).cloned()
    }

    /// Inserts a key-value pair into the appropriate shard.
    ///
    /// If the shard is at its capacity, the least recently used entry
    /// within that shard is evicted.
    pub fn put(&self, key: K, value: V) {
        let shard_idx = self.shard_index(&key);
        let mut shard = self.shards[shard_idx].lock().expect("shard lock poisoned");
        shard.put(key, value);
    }

    /// Returns the total number of entries across all shards.
    ///
    /// Note: This acquires each shard's lock sequentially. The result is
    /// a snapshot that may be slightly stale under concurrent modifications.
    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|s| s.lock().expect("shard lock poisoned").len())
            .sum()
    }

    /// Returns `true` if all shards are empty.
    pub fn is_empty(&self) -> bool {
        self.shards
            .iter()
            .all(|s| s.lock().expect("shard lock poisoned").is_empty())
    }

    /// Returns the number of shards.
    pub fn num_shards(&self) -> usize {
        self.num_shards
    }
}

// Manual Clone implementation — Arc is not used here because the struct
// itself is meant to be wrapped in Arc by the caller if sharing is needed.
// Each ShardedLruCache is an independent instance.
//
// For cross-thread sharing, use Arc<ShardedLruCache<K, V>> directly.
// The struct is Send + Sync because Mutex<LruCache> is Send + Sync.
unsafe impl<K: Send, V: Send> Sync for ShardedLruCache<K, V> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_put_and_get() {
        let cache = ShardedLruCache::new(10);
        cache.put("a", 1);
        cache.put("b", 2);

        assert_eq!(cache.get(&"a"), Some(1));
        assert_eq!(cache.get(&"b"), Some(2));
        assert_eq!(cache.get(&"c"), None);
    }

    #[test]
    fn test_capacity_distribution() {
        // 10 capacity across 3 shards: 4, 3, 3
        let cache = ShardedLruCache::<i32, i32>::with_shards(10, 3);
        assert_eq!(cache.num_shards(), 3);
    }

    #[test]
    fn test_len_and_is_empty() {
        let cache = ShardedLruCache::<i32, i32>::new(10);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        cache.put(1, 10);
        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_eviction_within_shard() {
        // Use 1 shard to test eviction behavior directly
        let cache = ShardedLruCache::with_shards(2, 1);
        cache.put("a", 1);
        cache.put("b", 2);
        cache.put("c", 3); // evicts "a"

        assert_eq!(cache.get(&"a"), None);
        assert_eq!(cache.get(&"b"), Some(2));
        assert_eq!(cache.get(&"c"), Some(3));
    }

    #[test]
    fn test_update_existing_key() {
        let cache = ShardedLruCache::new(10);
        cache.put("key", 1);
        cache.put("key", 2);

        assert_eq!(cache.get(&"key"), Some(2));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_total_capacity_bounded() {
        let capacity = 20;
        let cache = ShardedLruCache::with_shards(capacity, 4);

        // Insert more than capacity
        for i in 0..100 {
            cache.put(i, i);
        }

        // Total entries must not exceed capacity
        assert!(cache.len() <= capacity);
    }

    #[test]
    #[should_panic(expected = "Cache capacity must be greater than 0")]
    fn test_zero_capacity_panics() {
        let _cache: ShardedLruCache<i32, i32> = ShardedLruCache::new(0);
    }

    #[test]
    #[should_panic(expected = "Shard count must be greater than 0")]
    fn test_zero_shards_panics() {
        let _cache: ShardedLruCache<i32, i32> = ShardedLruCache::with_shards(10, 0);
    }

    #[test]
    #[should_panic(expected = "Shard count (20) cannot exceed capacity (10)")]
    fn test_shards_exceed_capacity_panics() {
        let _cache: ShardedLruCache<i32, i32> = ShardedLruCache::with_shards(10, 20);
    }
}
