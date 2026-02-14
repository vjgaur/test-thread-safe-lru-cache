// Thread-safe wrapper around LruCache
// Uses Arc<Mutex<LruCache>> for safe concurrent access
//
// TODO: Implement in next step

use crate::lru::LruCache;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

/// A thread-safe LRU cache, cloneable across threads.
///
/// All operations acquire a mutex lock. Since `get` must also
/// update recency (move-to-front).

#[derive(Clone)]
pub struct ThreadSafeLruCache<K, V> {
    inner: Arc<Mutex<LruCache<K, V>>>,
}

impl<K, V> ThreadSafeLruCache<K, V>
where
    K: Hash + Eq + Clone + Send + 'static,
    V: Clone + Send + 'static,
{
    /// Creates a new thread-safe LRU cache with the given capacity.
    ///
    /// # Panics
    /// Panics if `capacity` is 0.
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LruCache::new(capacity))),
        }
    }

    /// Retrieves a cloned value for the key, marking it as most recently used.
    ///
    /// Returns `None` if the key is not present.
    /// The lock is held only for the duration of the lookup and clone.
    pub fn get(&self, key: &K) -> Option<V> {
        let mut cache = self.inner.lock().expect("lock poisoned");
        cache.get(key).cloned()
    }
    /// Inserts a key-value pair into the cache.
    ///
    /// If the key exists, its value is updated. If the cache is at capacity,
    /// the least recently used entry is evicted.
    pub fn put(&self, key: K, value: V) {
        let mut cache = self.inner.lock().expect("lock poisoned");
        cache.put(key, value);
    }

    /// Returns the number of entries currently in the cache.
    pub fn len(&self) -> usize {
        let cache = self.inner.lock().expect("lock poisoned");
        cache.len()
    }
    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        let cache = self.inner.lock().expect("lock poisoned");
        cache.is_empty()
    }
}
