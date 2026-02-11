# Design Document: Thread-Safe LRU Cache

## 1. Problem Statement

Design and implement a thread-safe Least Recently Used (LRU) cache in Rust that
supports concurrent access from multiple threads while maintaining O(1) time
complexity for both `get` and `put` operations, bounded memory usage, and correct
eviction behavior under contention.

## 2. Architecture Overview

The implementation is split into two layers with clear separation of concerns:

```
┌─────────────────────────────────────────────────────┐
│                  Public API Layer                     │
│            ThreadSafeLruCache<K, V>                  │
│         (Arc<Mutex<LruCache<K, V>>>)                 │
│                                                      │
│   get(&self, key) -> Option<V>    (returns clone)    │
│   put(&self, key, value)                             │
│   len(&self) -> usize                                │
│   is_empty(&self) -> bool                            │
├─────────────────────────────────────────────────────┤
│                Synchronization Layer                  │
│              std::sync::Mutex                        │
│                                                      │
│   • Single lock per cache instance                   │
│   • Acquired and released within each method call    │
│   • O(1) critical sections — no extended blocking    │
├─────────────────────────────────────────────────────┤
│                  Core Logic Layer                     │
│              LruCache<K, V>                          │
│                                                      │
│   HashMap<K, usize>   +   Vec<Node<K,V>> Arena      │
│   (key → arena index)     (doubly-linked list)       │
│                                                      │
│   head ←→ ... ←→ tail                                │
│   (MRU)          (LRU)                               │
└─────────────────────────────────────────────────────┘
```

### Module Structure

```
src/
├── lib.rs          Public API re-exports
├── lru.rs          Core single-threaded LRU logic + unit tests
└── cache.rs        Thread-safe Mutex wrapper

tests/
├── lru_tests.rs              Integration tests (public API)
└── concurrency_tests.rs      Multi-threaded correctness tests
```

**Why this separation?**

The core LRU logic in `lru.rs` is completely unaware of threading. It is a
plain `&mut self` API — simple to reason about, test, and debug. The thread-safe
wrapper in `cache.rs` is a thin layer that adds `Mutex` synchronization. This
means:

- The eviction algorithm can be tested independently without thread complexity.
- The synchronization layer can be swapped (e.g., to `tokio::sync::Mutex` for
  async) without touching the core logic.
- Each layer has a single, clear responsibility.

## 3. Data Structures

### 3.1 Arena-Based Doubly-Linked List

The LRU cache requires a data structure that supports O(1) insertion, removal,
and move-to-front. A doubly-linked list provides this, but Rust's ownership
model makes traditional pointer-based linked lists difficult without `unsafe`.

We use an **arena-based** approach: nodes are stored in a `Vec<Node<K, V>>` and
referenced by index (`usize`) instead of pointers.

```
Arena (Vec<Node<K, V>>):
┌───────┬───────┬───────┬───────┬───────┐
│ idx 0 │ idx 1 │ idx 2 │ idx 3 │ idx 4 │
│ key=C │ key=A │ FREE  │ key=D │ key=B │
│ prev=3│ prev=4│       │ prev=─│ prev=0│
│ next=─│ next=─│       │ next=0│ next=1│
└───────┴───────┴───────┴───────┴───────┘
                    ↑
               free_list: [2]

Linked list order (by following next pointers from head):
  head=3(D) → 0(C) → 4(B) → 1(A)=tail
  (MRU)                       (LRU)
```

**Node structure:**

```rust
struct Node<K, V> {
    key: K,
    value: V,
    prev: Option<usize>,    // index of previous node (toward head)
    next: Option<usize>,    // index of next node (toward tail)
}
```

**Why arena-based instead of pointers?**

| Approach              | Safe Rust | Cache-friendly | O(1) ops | Complexity |
|-----------------------|-----------|----------------|----------|------------|
| Raw pointer list      | No        | No             | Yes      | High       |
| `Rc<RefCell<Node>>`   | Yes       | No             | Yes      | Medium     |
| `LinkedList` (std)    | Yes       | No             | No (O(n))| Low        |
| **Arena (Vec-indexed)** | **Yes** | **Yes**        | **Yes**  | **Medium** |

The arena approach gives us all three requirements: safe Rust, O(1) operations,
and cache-friendly memory layout (nodes are contiguous in memory).

### 3.2 HashMap for O(1) Key Lookup

```rust
map: HashMap<K, usize>    // key → index in the arena Vec
```

The HashMap provides O(1) average lookup from a key to its arena index. Combined
with O(1) index-based access into the Vec, both `get` and `put` achieve O(1)
overall.

### 3.3 Free List for Slot Recycling

```rust
free_list: Vec<usize>    // indices of evicted/reusable slots
```

When a node is evicted, its arena slot is not deallocated (the Vec doesn't
shrink). Instead, the index is pushed onto the free list. When a new node is
needed, we pop from the free list before growing the Vec. This ensures:

- The arena never grows beyond `capacity` entries.
- No allocations occur after the cache is warmed up.
- Memory usage remains bounded and predictable.

## 4. Core Operations

### 4.1 Internal Helpers

Four private methods form the foundation of all public operations:

**`unlink(index)`** — Removes a node from its current linked list position by
patching its neighbors' `prev`/`next` pointers. Handles edge cases: node is
head, node is tail, node is both (single element list). Does not remove the
node from the HashMap or free its slot.

**`push_front(index)`** — Inserts a node at the head of the list (most recently
used position). Updates the old head's `prev` pointer and handles the empty
list case (sets both head and tail).

**`evict_tail()`** — Removes the tail node (least recently used). Clones the
evicted key (needed for HashMap removal by the caller) and pushes the slot
onto the free list for reuse.

**`allocate_node(key, value)`** — Returns an arena index for a new node. Reuses
a free list slot if available, otherwise appends to the Vec.

### 4.2 Public API

**`get(key) -> Option<&V>`**

```
HashMap lookup → found? → unlink node → push_front → return &value
                → not found? → return None
```

Every `get` is a mutation: the accessed node moves to the head. This is why
`get` takes `&mut self` on the inner `LruCache`.

**`put(key, value)`**

```
Key exists?
  → Yes: update value in place → unlink → push_front
  → No:  at capacity?
           → Yes: evict_tail → remove evicted key from HashMap
         allocate_node → push_front → insert in HashMap
```

### 4.3 Operation Complexity

| Operation      | HashMap | Linked List | Overall |
|----------------|---------|-------------|---------|
| `get`          | O(1)    | O(1)        | O(1)    |
| `put` (new)    | O(1)    | O(1)        | O(1)    |
| `put` (update) | O(1)    | O(1)        | O(1)    |
| `put` (evict)  | O(1)    | O(1)        | O(1)    |

## 5. Synchronization Strategy

### 5.1 Why Mutex, Not RwLock

The intuitive choice for a cache is `RwLock` — allow concurrent readers, only
block for writers. However, **LRU's `get` is not a read-only operation**. Every
`get` must move the accessed node to the head of the linked list, which mutates
internal state.

With `RwLock`, we would face two bad options:

1. **Acquire write lock for every `get`** — negates all benefits of RwLock,
   adds overhead from RwLock's more complex implementation.
2. **Acquire read lock, then upgrade to write lock** — Rust's `std::sync::RwLock`
   does not support lock upgrading. Attempting to acquire a write lock while
   holding a read lock causes deadlock.

A `Mutex` is the honest choice: every operation mutates, so every operation
takes an exclusive lock. It is simpler, has lower overhead than RwLock, and
has zero risk of deadlock with a single lock.

### 5.2 Lock Scope and Critical Section Duration

```rust
pub fn get(&self, key: &K) -> Option<V> {
    let mut cache = self.inner.lock().expect("lock poisoned");
    cache.get(key).cloned()
}   // ← MutexGuard dropped here, lock released
```

Each public method follows the same pattern:

1. Acquire lock
2. Perform O(1) operation on in-memory data structures
3. Lock is released when `MutexGuard` is dropped (end of scope)

Critical sections contain only HashMap lookups and pointer index manipulation —
no I/O, no allocations (after warmup), no user callbacks. This keeps lock hold
times minimal and predictable.

### 5.3 Clone on Get

```
ThreadSafeLruCache::get → returns Option<V>    (owned, cloned)
LruCache::get           → returns Option<&V>   (borrowed)
```

The thread-safe wrapper returns a **cloned** value instead of a reference. This
is necessary because returning `&V` would require the caller to hold the
`MutexGuard` alive for the reference's lifetime, effectively holding the lock
across arbitrary user code. Cloning inside the critical section keeps the lock
scope tight.

The trade-off is the `V: Clone` bound. For large values, users can wrap them
in `Arc<V>` to make cloning cheap (only incrementing a reference count).

### 5.4 Thread Safety Bounds

```rust
impl<K, V> ThreadSafeLruCache<K, V>
where
    K: Hash + Eq + Clone + Send + 'static,
    V: Clone + Send + 'static,
```

- `Send` — required because values cross thread boundaries through `Arc<Mutex<>>`.
- `'static` — required for values stored in a shared heap-allocated structure
  that may outlive any particular thread.
- `Clone` — required for the clone-on-get pattern described above.

### 5.5 Poison Handling

```rust
self.inner.lock().expect("lock poisoned")
```

A `Mutex` becomes poisoned if a thread panics while holding the lock. We use
`expect()` to propagate the panic, because a poisoned lock means the internal
data structure may be in an inconsistent state (e.g., a node was unlinked but
not yet pushed to front). Operating on corrupt state would be worse than
panicking.

## 6. Concurrency Correctness

### 6.1 Why Single Mutex is Sufficient

The LRU cache has a key invariant: **the HashMap and the linked list must
always be consistent** — every key in the HashMap must point to a valid node
in the arena, and every node in the linked list must have a corresponding
HashMap entry.

A single Mutex ensures this invariant by making all operations atomic with
respect to each other. There is no window where one thread can observe a
partially-updated state.

### 6.2 Deadlock Freedom

Deadlocks require a cycle in lock acquisition order. With a single lock per
cache instance, cycles are impossible. Our implementation:

- Never holds two locks simultaneously
- Never calls user code while holding the lock
- Never performs I/O while holding the lock

### 6.3 What We Tested

| Test | Threads | Ops | What It Validates |
|------|---------|-----|-------------------|
| `concurrent_writes_respect_capacity` | 8 | 8,000 | Capacity bound holds under concurrent inserts |
| `concurrent_reads_and_writes` | 8 | 8,000 | Interleaved get/put doesn't corrupt state |
| `concurrent_updates_same_keys` | 8 | 8,000 | Competing writes to same keys produce valid values |
| `no_deadlock_under_contention` | 8 | 8,000 | Watchdog timeout detects deadlock (10s limit) |
| `get_returns_consistent_values` | 8 | 8,000 | No torn reads or value corruption |
| `concurrent_capacity_one` | 8 | 8,000 | Edge case: capacity-1 under contention |
| `cloned_handles_share_state` | 2 | 1 | Arc clone semantics work correctly |
| `stress_test_high_volume` | 16 | 80,000 | Mixed read/write patterns under heavy load |

All concurrency tests use `Barrier` to force threads to start simultaneously,
maximizing lock contention and the chance of exposing race conditions.

## 7. Memory Layout and Bounds

```
Total memory ≈ HashMap(capacity entries) + Vec(capacity nodes) + Vec(free_list ≤ capacity)

Per entry:
  HashMap: K + usize + overhead (~64 bytes typical for small keys)
  Node:    K + V + 2×Option<usize> (16 bytes for prev/next)
  
For a cache with capacity=1000 and (u64, u64) key-value pairs:
  ≈ 1000 × (64 + 32) bytes ≈ ~94 KB
```

Memory is bounded because:

- The HashMap never exceeds `capacity` entries.
- The arena Vec never grows beyond `capacity` slots (free list recycles).
- The free list itself is bounded by `capacity`.
- No allocations occur after the cache is fully warmed up.

## 8. Trade-Offs

| Decision | What We Gain | What We Give Up |
|---|---|---|
| Single Mutex | Simplicity, correctness, deadlock-free | Operations serialize under contention |
| Arena-based list | Safe Rust, cache-friendly, O(1) | Evicted slots aren't freed (recycled instead) |
| Clone on `get` | Short critical sections, no lock leakage | Clone overhead; requires `V: Clone` |
| No sharding | Simple implementation, easy to reason about | Single contention point under high load |
| `K: Clone` bound | Needed for eviction (clone key for HashMap removal) | Excludes non-Clone key types |
| `expect()` on poison | Fail-fast on corrupted state | Panic instead of graceful degradation |

## 9. Known Limitations

1. **Single lock bottleneck**: Under extremely high contention from many
   threads, all operations serialize through one Mutex. A sharded approach
   (partitioning the keyspace across N independent `Mutex<LruCache>` instances)
   would improve throughput linearly with shard count.

2. **Clone requirement**: Both `K` and `V` must implement `Clone`. For large
   values, this can be mitigated by storing `Arc<V>` (clone is just a refcount
   increment).

3. **No async support**: Uses `std::sync::Mutex` which blocks the OS thread.
   In async contexts (tokio, async-std), this could block the executor. An
   async version would use `tokio::sync::Mutex`.

4. **No TTL/expiration**: Items are only evicted by the LRU policy, not by
   time. Time-based expiration would require a separate mechanism (e.g., a
   timestamp per node checked on access).

5. **No cache statistics**: Hit rate, miss rate, and eviction count are not
   tracked. Could be added with `AtomicU64` counters outside the Mutex for
   zero-contention metrics.

6. **Arena fragmentation**: The free list recycles slots but doesn't compact
   the Vec. In practice this doesn't matter since the Vec is bounded by
   capacity, but the index-to-physical-position mapping becomes non-sequential
   over time.

## 10. Potential Improvements

- **Sharded cache**: Partition keyspace across N `Mutex<LruCache>` instances
  using `hash(key) % N`. Reduces contention linearly with shard count.
  Trade-off: LRU ordering becomes approximate (per-shard rather than global).

- **Async support**: Replace `std::sync::Mutex` with `tokio::sync::Mutex` and
  add `.await` to lock acquisition. The core `LruCache` logic remains unchanged.

- **Configurable eviction**: Abstract the eviction policy behind a trait,
  supporting LFU (Least Frequently Used), FIFO, or TTL-based policies.

- **Lock-free reads**: Use a concurrent HashMap (e.g., `dashmap`) with
  per-bucket locking. Reads that don't require recency updates could avoid
  global locking. Trade-off: significantly more complex implementation.

- **Metrics**: Add `AtomicU64` counters for hits, misses, and evictions
  outside the Mutex. Zero additional contention.