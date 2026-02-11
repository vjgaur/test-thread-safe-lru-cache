# LRU Cache — Implementation Guide

A thread-safe Least Recently Used (LRU) cache implementation in Rust, built
with safe Rust only and zero external dependencies.

## Quick Start

### Prerequisites

- Rust 1.75.0 or later
- Cargo (included with Rust)

### Build

```bash
cargo build
```

### Run Tests

```bash
# Run all tests (unit + integration + concurrency)
cargo test

# Run only unit tests
cargo test --lib

# Run only integration tests
cargo test --test lru_tests

# Run only concurrency tests
cargo test --test concurrency_tests

# Run a specific test by name
cargo test test_eviction_removes_lru
```

### Lint and Format

```bash
cargo fmt
cargo clippy -- -D warnings
cargo clippy --tests -- -D warnings
```

## Project Structure

```
├── Cargo.toml
├── README.md                  Assignment specification
├── DESIGN.md                  Architecture and design decisions
├── LRUReadme.md               This file
├── src/
│   ├── lib.rs                 Public API re-exports
│   ├── lru.rs                 Core LRU logic (single-threaded)
│   └── cache.rs               Thread-safe wrapper (Arc<Mutex>)
└── tests/
    ├── lru_tests.rs           Integration tests for public API
    └── concurrency_tests.rs   Multi-threaded correctness tests
```

## API Reference

### `LruCache<K, V>` — Single-Threaded

For use within a single thread or when you manage your own synchronization.

```rust
use lru_cache::LruCache;

let mut cache = LruCache::new(3);

cache.put("alice", 100);
cache.put("bob", 200);
cache.put("carol", 300);

assert_eq!(cache.get(&"alice"), Some(&100));

// Inserting a 4th key evicts the least recently used
cache.put("dave", 400);
assert_eq!(cache.get(&"bob"), None); // evicted
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `fn new(capacity: usize) -> Self` | Creates a cache with the given max capacity. Panics if capacity is 0. |
| `get` | `fn get(&mut self, key: &K) -> Option<&V>` | Retrieves a value by key, marking it as most recently used. |
| `put` | `fn put(&mut self, key: K, value: V)` | Inserts or updates a key-value pair. Evicts LRU entry if at capacity. |
| `len` | `fn len(&self) -> usize` | Returns the number of entries in the cache. |
| `is_empty` | `fn is_empty(&self) -> bool` | Returns true if the cache has no entries. |

**Type constraints:** `K: Hash + Eq + Clone`, `V` has no constraints.

**Note:** `get` takes `&mut self` because accessing a key updates its recency
in the internal linked list.

### `ThreadSafeLruCache<K, V>` — Multi-Threaded

Safe for concurrent access. Cloneable — each clone shares the same underlying
cache via `Arc`.

```rust
use lru_cache::ThreadSafeLruCache;
use std::thread;

let cache = ThreadSafeLruCache::new(100);

let handles: Vec<_> = (0..4)
    .map(|t| {
        let cache = cache.clone();
        thread::spawn(move || {
            for i in 0..1000 {
                cache.put(t * 1000 + i, i);
            }
        })
    })
    .collect();

for h in handles {
    h.join().unwrap();
}

assert!(cache.len() <= 100);
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `fn new(capacity: usize) -> Self` | Creates a thread-safe cache. Panics if capacity is 0. |
| `get` | `fn get(&self, key: &K) -> Option<V>` | Returns a **cloned** value. Takes `&self` (not `&mut`). |
| `put` | `fn put(&self, key: K, value: V)` | Inserts or updates. Takes `&self`. |
| `len` | `fn len(&self) -> usize` | Current entry count. |
| `is_empty` | `fn is_empty(&self) -> bool` | True if empty. |

**Type constraints:** `K: Hash + Eq + Clone + Send + 'static`,
`V: Clone + Send + 'static`

**Key difference from `LruCache`:** `get` returns `Option<V>` (owned clone)
instead of `Option<&V>` (reference). This is because the value is behind a
Mutex — returning a reference would require holding the lock across the
caller's use of that reference, which would block all other threads.

**Tip:** For large values, use `ThreadSafeLruCache<K, Arc<V>>` to make
cloning cheap (reference count increment instead of deep copy).

## Test Summary

The project contains 31 tests across three test suites:

### Unit Tests (10 tests in `src/lru.rs`)

Test the core LRU logic directly, including private internals.

- Basic put and get
- Get nonexistent key
- Eviction removes LRU item
- Get updates recency
- Put updates existing key
- Capacity-one edge case
- Len and is_empty
- Sequential eviction order
- Repeated updates preserve capacity
- Zero capacity panics

### Integration Tests (13 tests in `tests/lru_tests.rs`)

Test both `LruCache` and `ThreadSafeLruCache` through the public API.

- Basic operations
- Eviction of least recently used
- Put updates value and recency
- Capacity-one always holds latest
- Large capacity fill and evict
- Repeated access prevents eviction
- Zero capacity panics
- Thread-safe: basic operations, eviction, recency, update, len/is_empty, panic

### Concurrency Tests (8 tests in `tests/concurrency_tests.rs`)

Validate thread safety under contention using barriers for synchronized starts.

- Concurrent writes respect capacity (8 threads × 1,000 ops)
- Concurrent reads and writes
- Concurrent updates to same keys with value consistency checks
- Deadlock detection via watchdog timeout (10 second limit)
- Get returns consistent values (no torn reads)
- Concurrent capacity-one edge case
- Cloned handles share state across threads
- High-volume stress test (16 threads × 5,000 ops)

## Design Decisions

For the full design rationale, architecture diagrams, and trade-off analysis,
see [DESIGN.md](DESIGN.md).

Key decisions summarized:

- **HashMap + arena-based doubly-linked list** for O(1) operations in safe Rust
- **Mutex over RwLock** because `get` mutates internal state (move-to-front)
- **Clone on get** to avoid leaking lock scope to callers
- **Free list slot recycling** for bounded, predictable memory usage
- **No external dependencies** — built entirely on `std`

## Assumptions

- Cache capacity is always at least 1 (enforced by panic on 0).
- Keys and values are cheaply cloneable, or wrapped in `Arc` if expensive.
- The cache is used in a synchronous context. For async, the Mutex would need
  to be replaced with an async-aware equivalent.
- Lock poisoning indicates an unrecoverable error (we panic rather than recover).

## Dependencies

None. This project uses only the Rust standard library.