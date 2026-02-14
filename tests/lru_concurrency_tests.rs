// Concurrency tests for ThreadSafeLruCache
//
// These tests validate:
//   - Thread safety: no panics, no data races under concurrent access
//   - Deadlock freedom: all threads complete within a reasonable time
//   - Correctness: capacity bounds hold, values are consistent
//   - Contention handling: cache remains correct under high contention
//
// Strategy: We use Barriers to force threads to start simultaneously,
// maximizing the chance of lock contention and exposing race conditions.

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use test_thread_safe_lru_cache::ThreadSafeLruCache;

/// Number of threads for contention tests.
const NUM_THREADS: usize = 8;
/// Operations per thread for stress tests.
const OPS_PER_THREAD: usize = 1_000;

// -------------------------------------------------------------------
// Test 1: Concurrent writes don't exceed capacity
// -------------------------------------------------------------------
// Multiple threads insert unique keys simultaneously. After all threads
// finish, the cache size must not exceed its configured capacity.
#[test]
fn concurrent_writes_respect_capacity() {
    let capacity = 50;
    let cache = ThreadSafeLruCache::new(capacity);
    let barrier = Arc::new(Barrier::new(NUM_THREADS));

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|t| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait(); // synchronize start
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

    // Total unique keys inserted: NUM_THREADS * OPS_PER_THREAD = 8000
    // Cache capacity is 50, so len must be exactly 50
    assert_eq!(cache.len(), capacity);
}

// -------------------------------------------------------------------
// Test 2: Concurrent reads and writes
// -------------------------------------------------------------------
// Half the threads write, half read. No panics, no poisoned locks.
// Validates that interleaved get/put doesn't corrupt state.
#[test]
fn concurrent_reads_and_writes() {
    let cache = ThreadSafeLruCache::new(100);

    // Pre-fill with some data
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
                        // Writer threads: insert new keys
                        let key = 100 + t * OPS_PER_THREAD + i;
                        cache.put(key, key);
                    } else {
                        // Reader threads: read existing keys
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

// -------------------------------------------------------------------
// Test 3: Concurrent updates to same keys
// -------------------------------------------------------------------
// All threads compete to update the same set of keys. After completion,
// each key must hold a value that was actually written by some thread
// (no garbage or partial writes).
#[test]
fn concurrent_updates_same_keys() {
    let cache = ThreadSafeLruCache::new(10);
    let barrier = Arc::new(Barrier::new(NUM_THREADS));

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|t| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for i in 0..OPS_PER_THREAD {
                    let key = i % 10; // all threads fight over keys 0-9
                    let value = t * OPS_PER_THREAD + i; // unique per thread+iteration
                    cache.put(key, value);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    // All 10 keys should still be present (capacity is 10)
    assert_eq!(cache.len(), 10);

    // Every value must be a valid value that some thread wrote
    for key in 0..10 {
        let value = cache.get(&key).expect("key should exist");
        // Value should be of the form t * OPS_PER_THREAD + i where i % 10 == key
        assert_eq!(value % 10, key % 10);
    }
}

// -------------------------------------------------------------------
// Test 4: Deadlock freedom under high contention
// -------------------------------------------------------------------
// Runs a stress test with a timeout. If threads deadlock, the test
// will fail due to timeout rather than hanging forever.
#[test]
fn no_deadlock_under_contention() {
    let cache = ThreadSafeLruCache::new(10);
    let barrier = Arc::new(Barrier::new(NUM_THREADS));

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|t| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                // Interleave gets and puts on overlapping keys
                for i in 0..OPS_PER_THREAD {
                    cache.put(t * 100 + i, i);
                    let _ = cache.get(&(t * 100 + i));
                    // Also access other threads' keys to create cross-contention
                    let other_key = ((t + 1) % NUM_THREADS) * 100 + i;
                    let _ = cache.get(&other_key);
                }
            })
        })
        .collect();

    // If this completes, there's no deadlock.
    // We add a secondary check: spawn a watchdog thread.
    let (tx, rx) = std::sync::mpsc::channel();

    let watchdog = thread::spawn(move || {
        for h in handles {
            h.join().expect("thread panicked");
        }
        tx.send(()).ok();
    });

    // 10 seconds is extremely generous for in-memory O(1) operations
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => {} // all threads completed
        Err(_) => panic!("deadlock detected: threads did not complete within 10 seconds"),
    }

    watchdog.join().expect("watchdog panicked");
}

// -------------------------------------------------------------------
// Test 5: Get returns consistent values
// -------------------------------------------------------------------
// Writers write (key, key * 100). Readers verify that if a value is
// returned, it equals key * 100. This catches torn reads or value
// corruption.
#[test]
fn get_returns_consistent_values() {
    let cache = ThreadSafeLruCache::new(50);
    let barrier = Arc::new(Barrier::new(NUM_THREADS));

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|t| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for i in 0..OPS_PER_THREAD {
                    if t < NUM_THREADS / 2 {
                        // Writers: value is always key * 100
                        let key = i % 50;
                        cache.put(key, key * 100);
                    } else {
                        // Readers: verify consistency
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

// -------------------------------------------------------------------
// Test 6: Cache with capacity 1 under concurrency
// -------------------------------------------------------------------
// Edge case: capacity-1 cache with multiple threads. Only one entry
// should ever exist.
#[test]
fn concurrent_capacity_one() {
    let cache = ThreadSafeLruCache::new(1);
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

                    // After every put, len must be exactly 1
                    assert_eq!(cache.len(), 1);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    assert_eq!(cache.len(), 1);
}

// -------------------------------------------------------------------
// Test 7: Clone shares the same cache
// -------------------------------------------------------------------
// Validates that cloned handles see each other's writes.
#[test]
fn cloned_handles_share_state() {
    let cache = ThreadSafeLruCache::new(10);
    let cache2 = cache.clone();

    let handle = thread::spawn(move || {
        cache2.put("from_thread", 42);
    });

    handle.join().expect("thread panicked");

    assert_eq!(cache.get(&"from_thread"), Some(42));
}

// -------------------------------------------------------------------
// Test 8: Stress test — high volume of operations
// -------------------------------------------------------------------
// Hammers the cache with many threads and operations to surface any
// subtle race conditions that only manifest under load.
#[test]
fn stress_test_high_volume() {
    let cache = ThreadSafeLruCache::new(100);
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
                            // Read-then-write pattern
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
