//! # Thread-Safe LRU Cache
//!
//! A concurrent Least Recently Used (LRU) cache implementation
//! using a HashMap + arena based doubly-linked list for O(1) operations,
//! wrapped in a Mutex for thread safety.

mod cache;
mod lru;

pub use cache::ThreadSafeLruCache;
pub use lru::LruCache;