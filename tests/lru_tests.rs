// Integration tests for LRU cache
//
// These test the public API as an external consumer would.
// Organized into two sections:
//   1. LruCache (single-threaded) — eviction behavior, edge cases
//   2. ThreadSafeLruCache — same semantics through the thread-safe wrapper

use test_thread_safe_lru_cache::{LruCache, ThreadSafeLruCache};

// ===================================================================
// LruCache (single-threaded)
// ===================================================================

#[test]
fn lru_basic_operations() {
    let mut cache = LruCache::new(2);

    cache.put("x", 10);
    cache.put("y", 20);

    assert_eq!(cache.get(&"x"), Some(&10));
    assert_eq!(cache.get(&"y"), Some(&20));
    assert_eq!(cache.get(&"z"), None);
}

#[test]
fn lru_evicts_least_recently_used() {
    let mut cache = LruCache::new(3);
    cache.put(1, "one");
    cache.put(2, "two");
    cache.put(3, "three");

    // Access key 1 to make it most recent
    // Order before: head=[3] -> [2] -> [1]=tail
    // Order after:  head=[1] -> [3] -> [2]=tail
    assert_eq!(cache.get(&1), Some(&"one"));

    // Insert 4, should evict 2 (now least recent)
    cache.put(4, "four");

    assert_eq!(cache.get(&2), None); // evicted
    assert_eq!(cache.get(&1), Some(&"one"));
    assert_eq!(cache.get(&3), Some(&"three"));
    assert_eq!(cache.get(&4), Some(&"four"));
}

#[test]
fn lru_put_existing_key_updates_value_and_recency() {
    let mut cache = LruCache::new(2);
    cache.put("a", 1);
    cache.put("b", 2);
    // head=[b] -> [a]=tail

    // Update "a" — becomes most recent with new value
    cache.put("a", 100);
    // head=[a] -> [b]=tail

    assert_eq!(cache.get(&"a"), Some(&100));

    // Insert "c" — should evict "b" (now least recent)
    cache.put("c", 3);
    assert_eq!(cache.get(&"b"), None);
    assert_eq!(cache.get(&"a"), Some(&100));
    assert_eq!(cache.get(&"c"), Some(&3));
}

#[test]
fn lru_capacity_one_always_holds_latest() {
    let mut cache = LruCache::new(1);

    for i in 0..50 {
        cache.put("only", i);
        assert_eq!(cache.get(&"only"), Some(&i));
        assert_eq!(cache.len(), 1);
    }
}

#[test]
fn lru_large_capacity_fill_and_evict() {
    let cap = 100;
    let mut cache = LruCache::new(cap);

    // Fill to capacity
    for i in 0..cap {
        cache.put(i, i * 10);
    }
    assert_eq!(cache.len(), cap);

    // All keys present
    for i in 0..cap {
        assert_eq!(cache.get(&i), Some(&(i * 10)));
    }

    // Insert one more — key 0 was accessed most recently (by the get loop),
    // but key 1 was accessed just after key 0, so the eviction depends on
    // the get loop order. After the loop, order is:
    //   head=[99] -> [98] -> ... -> [0]=tail
    // Wait — the loop accessed 0 first, making 99 the most recent.
    // Actually each get moves the key to front, so after the loop:
    //   head=[99] -> [98] -> ... -> [0]=tail
    // NO — get(0) moves 0 to front, then get(1) moves 1 to front, etc.
    // Final order: head=[99] -> [98] -> ... -> [0]=tail
    // So inserting 100 evicts key 0.
    cache.put(cap, 999);
    assert_eq!(cache.get(&0), None); // evicted
    assert_eq!(cache.get(&cap), Some(&999));
    assert_eq!(cache.len(), cap);
}

#[test]
fn lru_repeated_access_prevents_eviction() {
    let mut cache = LruCache::new(3);
    cache.put(0, 0);
    cache.put(1, 1);
    cache.put(2, 2);

    // Repeatedly access key 0 while cycling new keys through the cache
    for i in 3..20 {
        cache.get(&0); // refresh recency — key 0 stays at head
        cache.put(i, i); // evicts the oldest non-0 entry
    }

    // Key 0 should survive all evictions
    assert_eq!(cache.get(&0), Some(&0));
    assert_eq!(cache.len(), 3);
}

#[test]
#[should_panic(expected = "Cache capacity must be greater than 0")]
fn lru_zero_capacity_panics() {
    let _cache: LruCache<i32, i32> = LruCache::new(0);
}

// ===================================================================
// ThreadSafeLruCache
// ===================================================================

#[test]
fn thread_safe_basic_operations() {
    let cache = ThreadSafeLruCache::new(2);

    cache.put("x", 10);
    cache.put("y", 20);

    // Note: returns owned V, not &V
    assert_eq!(cache.get(&"x"), Some(10));
    assert_eq!(cache.get(&"y"), Some(20));
    assert_eq!(cache.get(&"z"), None);
}

#[test]
fn thread_safe_eviction() {
    let cache = ThreadSafeLruCache::new(2);
    cache.put(1, "one");
    cache.put(2, "two");
    cache.put(3, "three"); // evicts 1

    assert_eq!(cache.get(&1), None);
    assert_eq!(cache.get(&2), Some("two"));
    assert_eq!(cache.get(&3), Some("three"));
}

#[test]
fn thread_safe_get_updates_recency() {
    let cache = ThreadSafeLruCache::new(2);
    cache.put("a", 1);
    cache.put("b", 2);

    cache.get(&"a"); // refresh "a"

    cache.put("c", 3); // evicts "b"

    assert_eq!(cache.get(&"a"), Some(1));
    assert_eq!(cache.get(&"b"), None);
    assert_eq!(cache.get(&"c"), Some(3));
}

#[test]
fn thread_safe_update_existing() {
    let cache = ThreadSafeLruCache::new(2);
    cache.put("key", 1);
    cache.put("key", 2);

    assert_eq!(cache.get(&"key"), Some(2));
    assert_eq!(cache.len(), 1);
}

#[test]
fn thread_safe_len_and_is_empty() {
    let cache: ThreadSafeLruCache<i32, i32> = ThreadSafeLruCache::new(3);
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);

    cache.put(1, 10);
    assert!(!cache.is_empty());
    assert_eq!(cache.len(), 1);

    cache.put(2, 20);
    cache.put(3, 30);
    cache.put(4, 40); // evicts 1

    assert_eq!(cache.len(), 3);
}

#[test]
#[should_panic(expected = "Cache capacity must be greater than 0")]
fn thread_safe_zero_capacity_panics() {
    let _cache: ThreadSafeLruCache<i32, i32> = ThreadSafeLruCache::new(0);
}
