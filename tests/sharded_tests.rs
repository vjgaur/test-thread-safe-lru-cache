// Concurrency tests for ShardedLruCache
//
// These mirror the ThreadSafeLruCache concurrency tests to verify
// that the sharded implementation provides the same correctness
// guarantees while allowing higher concurrent throughput.

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use test_thread_safe_lru_cache::ShardedLruCache;

const NUM_THREADS: usize = 8;
const OPS_PER_THREAD: usize = 1_000;

#[test]
fn sharded_concurrent_writes_respect_capacity() {
    let capacity = 50;
    let cache = Arc::new(ShardedLruCache::with_shards(capacity, 8));
    let barrier = Arc::new(Barrier::new(NUM_THREADS));

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|t| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for i in 0..OPS_PER_THREAD {
                    let key = t * OPS_PER_THREAD + i;
                    cache.put(key, key);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    assert!(cache.len() <= capacity);
}

#[test]
fn sharded_concurrent_reads_and_writes() {
    let cache = Arc::new(ShardedLruCache::with_shards(100, 8));

    // Pre-fill
    for i in 0..100 {
        cache.put(i, i * 10);
    }

    let barrier = Arc::new(Barrier::new(NUM_THREADS));
    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|t| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for i in 0..OPS_PER_THREAD {
                    if t % 2 == 0 {
                        let key = 100 + t * OPS_PER_THREAD + i;
                        cache.put(key, key);
                    } else {
                        let key = i % 100;
                        let _ = cache.get(&key);
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    assert!(cache.len() <= 100);
    assert!(!cache.is_empty());
}

#[test]
fn sharded_concurrent_updates_same_keys() {
    let cache = Arc::new(ShardedLruCache::with_shards(10, 4));
    let barrier = Arc::new(Barrier::new(NUM_THREADS));

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|t| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for i in 0..OPS_PER_THREAD {
                    let key = i % 10;
                    let value = t * OPS_PER_THREAD + i;
                    cache.put(key, value);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    // With sharding, keys are distributed across shards. Under heavy
    // concurrent writes, some keys in the 0-9 range may be evicted from
    // their shard even though the total capacity is 10. This is the
    // per-shard eviction trade-off.
    assert!(cache.len() <= 10);
    assert!(!cache.is_empty());

    // Values that are present must still be consistent
    for key in 0..10 {
        if let Some(value) = cache.get(&key) {
            assert_eq!(value % 10, key % 10);
        }
    }
}

#[test]
fn sharded_no_deadlock_under_contention() {
    let cache = Arc::new(ShardedLruCache::with_shards(20, 4));
    let barrier = Arc::new(Barrier::new(NUM_THREADS));

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|t| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for i in 0..OPS_PER_THREAD {
                    cache.put(t * 100 + i, i);
                    let _ = cache.get(&(t * 100 + i));
                    // Cross-shard access to stress multiple locks
                    let other_key = ((t + 1) % NUM_THREADS) * 100 + i;
                    let _ = cache.get(&other_key);
                }
            })
        })
        .collect();

    let (tx, rx) = std::sync::mpsc::channel();

    let watchdog = thread::spawn(move || {
        for h in handles {
            h.join().expect("thread panicked");
        }
        tx.send(()).ok();
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => {}
        Err(_) => panic!("deadlock detected: threads did not complete within 10 seconds"),
    }

    watchdog.join().expect("watchdog panicked");
}

#[test]
fn sharded_get_returns_consistent_values() {
    let cache = Arc::new(ShardedLruCache::with_shards(50, 8));
    let barrier = Arc::new(Barrier::new(NUM_THREADS));

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|t| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for i in 0..OPS_PER_THREAD {
                    if t < NUM_THREADS / 2 {
                        let key = i % 50;
                        cache.put(key, key * 100);
                    } else {
                        let key = i % 50;
                        if let Some(value) = cache.get(&key) {
                            assert_eq!(
                                value,
                                key * 100,
                                "inconsistent value for key {}: expected {}, got {}",
                                key,
                                key * 100,
                                value
                            );
                        }
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }
}

#[test]
fn sharded_stress_test_high_volume() {
    let cache = Arc::new(ShardedLruCache::with_shards(100, 16));
    let num_threads = 16;
    let ops = 5_000;
    let barrier = Arc::new(Barrier::new(num_threads));

    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for i in 0..ops {
                    match i % 3 {
                        0 => {
                            cache.put(t * ops + i, i);
                        }
                        1 => {
                            let _ = cache.get(&(t * ops + i));
                        }
                        _ => {
                            let key = i % 100;
                            if cache.get(&key).is_none() {
                                cache.put(key, i);
                            }
                        }
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    assert!(cache.len() <= 100);
    assert!(!cache.is_empty());
}

// -------------------------------------------------------------------
// Sharded-specific tests
// -------------------------------------------------------------------

#[test]
fn sharded_different_shard_counts() {
    // Verify correctness with various shard configurations
    for num_shards in [1, 2, 4, 8, 16] {
        let cache = Arc::new(ShardedLruCache::with_shards(32, num_shards));
        let barrier = Arc::new(Barrier::new(4));

        let handles: Vec<_> = (0..4)
            .map(|t| {
                let cache = cache.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    for i in 0..500 {
                        cache.put(t * 500 + i, i);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }

        assert!(
            cache.len() <= 32,
            "shard_count={}: len {} exceeds capacity 32",
            num_shards,
            cache.len()
        );
    }
}

#[test]
fn sharded_single_shard_behaves_like_mutex_cache() {
    // With 1 shard, behavior should be identical to ThreadSafeLruCache
    let cache = ShardedLruCache::with_shards(3, 1);

    cache.put(1, "one");
    cache.put(2, "two");
    cache.put(3, "three");

    // Access key 1 to make it most recent
    assert_eq!(cache.get(&1), Some("one"));

    // Insert key 4 — should evict key 2 (LRU)
    cache.put(4, "four");

    assert_eq!(cache.get(&2), None);
    assert_eq!(cache.get(&1), Some("one"));
    assert_eq!(cache.get(&3), Some("three"));
    assert_eq!(cache.get(&4), Some("four"));
}
