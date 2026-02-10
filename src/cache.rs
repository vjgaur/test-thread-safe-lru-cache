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

impl<K: Hash + Eq + Clone, V: Clone> ThreadSafeLruCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LruCache::new(capacity))),
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        todo!("Implement in next step")
    }

    pub fn put(&self, key: K, value: V) {
        todo!("Implement in next step")
    }

    pub fn len(&self) -> usize {
        todo!("Implement in next step")
    }

    pub fn is_empty(&self) -> bool {
        todo!("Implement in next step")
    }
}

