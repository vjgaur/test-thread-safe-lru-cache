use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::sync::{Arc, Barrier, RwLock};
use std::thread;
use test_thread_safe_lru_cache::{LruCache, ShardedLruCache, ThreadSafeLruCache};

// ============================================================================
// RwLock-based implementation for comparison
// ============================================================================

#[derive(Clone)]
struct RwLockLruCache<K, V> {
    inner: Arc<RwLock<LruCache<K, V>>>,
}

impl<K, V> RwLockLruCache<K, V>
where
    K: std::hash::Hash + Eq + Clone + Send + 'static,
    V: Clone + Send + 'static,
{
    fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(LruCache::new(capacity))),
        }
    }

    fn get(&self, key: &K) -> Option<V> {
        // RwLock requires write lock for get since it modifies LRU order
        let mut cache = self.inner.write().expect("rwlock poisoned");
        cache.get(key).cloned()
    }

    fn put(&self, key: K, value: V) {
        let mut cache = self.inner.write().expect("rwlock poisoned");
        cache.put(key, value);
    }
}

// ============================================================================
// Single-threaded Benchmarks
// ============================================================================

fn bench_single_thread_put(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_thread_put");
    let capacity = 1000;
    let ops = 10_000;

    group.throughput(Throughput::Elements(ops));

    group.bench_function("mutex", |b| {
        b.iter(|| {
            let cache = ThreadSafeLruCache::new(capacity);
            for i in 0..ops {
                cache.put(black_box(i), black_box(i));
            }
        });
    });

    group.bench_function("rwlock", |b| {
        b.iter(|| {
            let cache = RwLockLruCache::new(capacity);
            for i in 0..ops {
                cache.put(black_box(i), black_box(i));
            }
        });
    });

    group.bench_function("sharded", |b| {
        b.iter(|| {
            let cache = Arc::new(ShardedLruCache::new(capacity));
            for i in 0..ops {
                cache.put(black_box(i), black_box(i));
            }
        });
    });

    group.finish();
}

fn bench_single_thread_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_thread_get");
    let capacity = 1000;
    let ops = 10_000;

    group.throughput(Throughput::Elements(ops));

    // Pre-populate caches
    let mutex_cache = ThreadSafeLruCache::new(capacity);
    let rwlock_cache = RwLockLruCache::new(capacity);
    let sharded_cache = Arc::new(ShardedLruCache::new(capacity));

    for i in 0..capacity as u64 {
        mutex_cache.put(i, i);
        rwlock_cache.put(i, i);
        sharded_cache.put(i, i);
    }

    group.bench_function("mutex", |b| {
        b.iter(|| {
            for i in 0..ops {
                black_box(mutex_cache.get(&black_box(i % capacity as u64)));
            }
        });
    });

    group.bench_function("rwlock", |b| {
        b.iter(|| {
            for i in 0..ops {
                black_box(rwlock_cache.get(&black_box(i % capacity as u64)));
            }
        });
    });

    group.bench_function("sharded", |b| {
        b.iter(|| {
            for i in 0..ops {
                black_box(sharded_cache.get(&black_box(i % capacity as u64)));
            }
        });
    });

    group.finish();
}

fn bench_single_thread_mixed(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_thread_mixed");
    let capacity = 1000;
    let ops = 10_000;

    group.throughput(Throughput::Elements(ops));

    group.bench_function("mutex", |b| {
        b.iter(|| {
            let cache = ThreadSafeLruCache::new(capacity);
            for i in 0..ops {
                if i % 3 == 0 {
                    cache.put(black_box(i), black_box(i));
                } else {
                    black_box(cache.get(&black_box(i % capacity as u64)));
                }
            }
        });
    });

    group.bench_function("rwlock", |b| {
        b.iter(|| {
            let cache = RwLockLruCache::new(capacity);
            for i in 0..ops {
                if i % 3 == 0 {
                    cache.put(black_box(i), black_box(i));
                } else {
                    black_box(cache.get(&black_box(i % capacity as u64)));
                }
            }
        });
    });

    group.bench_function("sharded", |b| {
        b.iter(|| {
            let cache = Arc::new(ShardedLruCache::new(capacity));
            for i in 0..ops {
                if i % 3 == 0 {
                    cache.put(black_box(i), black_box(i));
                } else {
                    black_box(cache.get(&black_box(i % capacity as u64)));
                }
            }
        });
    });

    group.finish();
}

// ============================================================================
// Multi-threaded Benchmarks
//
// All concurrent benchmarks use Barrier to synchronize thread start.
// This ensures all threads begin operations simultaneously, maximizing
// lock contention and producing accurate measurements of concurrent
// throughput rather than partially measuring thread spawn time.
// ============================================================================

fn bench_concurrent_writes(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_writes");
    let capacity = 1000;
    let ops_per_thread = 5_000;

    for num_threads in [2, 4, 8] {
        let total_ops = ops_per_thread * num_threads;
        group.throughput(Throughput::Elements(total_ops as u64));

        group.bench_with_input(
            BenchmarkId::new("mutex", num_threads),
            &num_threads,
            |b, &threads| {
                b.iter(|| {
                    let cache = ThreadSafeLruCache::new(capacity);
                    let barrier = Arc::new(Barrier::new(threads));
                    let handles: Vec<_> = (0..threads)
                        .map(|t| {
                            let cache = cache.clone();
                            let barrier = barrier.clone();
                            thread::spawn(move || {
                                barrier.wait();
                                for i in 0..ops_per_thread {
                                    let key = (t * ops_per_thread + i) as u64;
                                    cache.put(black_box(key), black_box(key));
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("rwlock", num_threads),
            &num_threads,
            |b, &threads| {
                b.iter(|| {
                    let cache = RwLockLruCache::new(capacity);
                    let barrier = Arc::new(Barrier::new(threads));
                    let handles: Vec<_> = (0..threads)
                        .map(|t| {
                            let cache = cache.clone();
                            let barrier = barrier.clone();
                            thread::spawn(move || {
                                barrier.wait();
                                for i in 0..ops_per_thread {
                                    let key = (t * ops_per_thread + i) as u64;
                                    cache.put(black_box(key), black_box(key));
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sharded", num_threads),
            &num_threads,
            |b, &threads| {
                b.iter(|| {
                    let cache = Arc::new(ShardedLruCache::new(capacity));
                    let barrier = Arc::new(Barrier::new(threads));
                    let handles: Vec<_> = (0..threads)
                        .map(|t| {
                            let cache = Arc::clone(&cache);
                            let barrier = barrier.clone();
                            thread::spawn(move || {
                                barrier.wait();
                                for i in 0..ops_per_thread {
                                    let key = (t * ops_per_thread + i) as u64;
                                    cache.put(black_box(key), black_box(key));
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_concurrent_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_reads");
    let capacity = 1000;
    let ops_per_thread = 5_000;

    for num_threads in [2, 4, 8] {
        let total_ops = ops_per_thread * num_threads;
        group.throughput(Throughput::Elements(total_ops as u64));

        // Pre-populate caches
        let mutex_cache = ThreadSafeLruCache::new(capacity);
        let rwlock_cache = RwLockLruCache::new(capacity);
        let sharded_cache = Arc::new(ShardedLruCache::new(capacity));

        for i in 0..capacity as u64 {
            mutex_cache.put(i, i);
            rwlock_cache.put(i, i);
            sharded_cache.put(i, i);
        }

        group.bench_with_input(
            BenchmarkId::new("mutex", num_threads),
            &num_threads,
            |b, &threads| {
                b.iter(|| {
                    let barrier = Arc::new(Barrier::new(threads));
                    let handles: Vec<_> = (0..threads)
                        .map(|_t| {
                            let cache = mutex_cache.clone();
                            let barrier = barrier.clone();
                            thread::spawn(move || {
                                barrier.wait();
                                for i in 0..ops_per_thread {
                                    black_box(cache.get(&black_box((i % capacity) as u64)));
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("rwlock", num_threads),
            &num_threads,
            |b, &threads| {
                b.iter(|| {
                    let barrier = Arc::new(Barrier::new(threads));
                    let handles: Vec<_> = (0..threads)
                        .map(|_t| {
                            let cache = rwlock_cache.clone();
                            let barrier = barrier.clone();
                            thread::spawn(move || {
                                barrier.wait();
                                for i in 0..ops_per_thread {
                                    black_box(cache.get(&black_box((i % capacity) as u64)));
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sharded", num_threads),
            &num_threads,
            |b, &threads| {
                b.iter(|| {
                    let barrier = Arc::new(Barrier::new(threads));
                    let handles: Vec<_> = (0..threads)
                        .map(|_t| {
                            let cache = Arc::clone(&sharded_cache);
                            let barrier = barrier.clone();
                            thread::spawn(move || {
                                barrier.wait();
                                for i in 0..ops_per_thread {
                                    black_box(cache.get(&black_box((i % capacity) as u64)));
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_concurrent_mixed(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_mixed");
    let capacity = 1000;
    let ops_per_thread = 5_000;

    for num_threads in [2, 4, 8] {
        let total_ops = ops_per_thread * num_threads;
        group.throughput(Throughput::Elements(total_ops as u64));

        group.bench_with_input(
            BenchmarkId::new("mutex", num_threads),
            &num_threads,
            |b, &threads| {
                b.iter(|| {
                    let cache = ThreadSafeLruCache::new(capacity);
                    let barrier = Arc::new(Barrier::new(threads));
                    let handles: Vec<_> = (0..threads)
                        .map(|t| {
                            let cache = cache.clone();
                            let barrier = barrier.clone();
                            thread::spawn(move || {
                                barrier.wait();
                                for i in 0..ops_per_thread {
                                    if i % 3 == 0 {
                                        let key = (t * ops_per_thread + i) as u64;
                                        cache.put(black_box(key), black_box(key));
                                    } else {
                                        black_box(cache.get(&black_box((i % capacity) as u64)));
                                    }
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("rwlock", num_threads),
            &num_threads,
            |b, &threads| {
                b.iter(|| {
                    let cache = RwLockLruCache::new(capacity);
                    let barrier = Arc::new(Barrier::new(threads));
                    let handles: Vec<_> = (0..threads)
                        .map(|t| {
                            let cache = cache.clone();
                            let barrier = barrier.clone();
                            thread::spawn(move || {
                                barrier.wait();
                                for i in 0..ops_per_thread {
                                    if i % 3 == 0 {
                                        let key = (t * ops_per_thread + i) as u64;
                                        cache.put(black_box(key), black_box(key));
                                    } else {
                                        black_box(cache.get(&black_box((i % capacity) as u64)));
                                    }
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sharded", num_threads),
            &num_threads,
            |b, &threads| {
                b.iter(|| {
                    let cache = Arc::new(ShardedLruCache::new(capacity));
                    let barrier = Arc::new(Barrier::new(threads));
                    let handles: Vec<_> = (0..threads)
                        .map(|t| {
                            let cache = Arc::clone(&cache);
                            let barrier = barrier.clone();
                            thread::spawn(move || {
                                barrier.wait();
                                for i in 0..ops_per_thread {
                                    if i % 3 == 0 {
                                        let key = (t * ops_per_thread + i) as u64;
                                        cache.put(black_box(key), black_box(key));
                                    } else {
                                        black_box(cache.get(&black_box((i % capacity) as u64)));
                                    }
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Read-Heavy vs Write-Heavy Workloads
// ============================================================================

fn bench_read_heavy(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_heavy_90_10");
    let capacity = 1000;
    let ops_per_thread = 5_000;
    let num_threads = 4;
    let total_ops = ops_per_thread * num_threads;

    group.throughput(Throughput::Elements(total_ops as u64));

    group.bench_function("mutex", |b| {
        b.iter(|| {
            let cache = ThreadSafeLruCache::new(capacity);
            // Pre-populate
            for i in 0..capacity as u64 {
                cache.put(i, i);
            }

            let barrier = Arc::new(Barrier::new(num_threads));
            let handles: Vec<_> = (0..num_threads)
                .map(|t| {
                    let cache = cache.clone();
                    let barrier = barrier.clone();
                    thread::spawn(move || {
                        barrier.wait();
                        for i in 0..ops_per_thread {
                            if i % 10 == 0 {
                                // 10% writes
                                let key = (t * ops_per_thread + i) as u64;
                                cache.put(black_box(key), black_box(key));
                            } else {
                                // 90% reads
                                black_box(cache.get(&black_box((i % capacity) as u64)));
                            }
                        }
                    })
                })
                .collect();

            for h in handles {
                h.join().unwrap();
            }
        });
    });

    group.bench_function("rwlock", |b| {
        b.iter(|| {
            let cache = RwLockLruCache::new(capacity);
            for i in 0..capacity as u64 {
                cache.put(i, i);
            }

            let barrier = Arc::new(Barrier::new(num_threads));
            let handles: Vec<_> = (0..num_threads)
                .map(|t| {
                    let cache = cache.clone();
                    let barrier = barrier.clone();
                    thread::spawn(move || {
                        barrier.wait();
                        for i in 0..ops_per_thread {
                            if i % 10 == 0 {
                                let key = (t * ops_per_thread + i) as u64;
                                cache.put(black_box(key), black_box(key));
                            } else {
                                black_box(cache.get(&black_box((i % capacity) as u64)));
                            }
                        }
                    })
                })
                .collect();

            for h in handles {
                h.join().unwrap();
            }
        });
    });

    group.bench_function("sharded", |b| {
        b.iter(|| {
            let cache = Arc::new(ShardedLruCache::new(capacity));
            for i in 0..capacity as u64 {
                cache.put(i, i);
            }

            let barrier = Arc::new(Barrier::new(num_threads));
            let handles: Vec<_> = (0..num_threads)
                .map(|t| {
                    let cache = Arc::clone(&cache);
                    let barrier = barrier.clone();
                    thread::spawn(move || {
                        barrier.wait();
                        for i in 0..ops_per_thread {
                            if i % 10 == 0 {
                                let key = (t * ops_per_thread + i) as u64;
                                cache.put(black_box(key), black_box(key));
                            } else {
                                black_box(cache.get(&black_box((i % capacity) as u64)));
                            }
                        }
                    })
                })
                .collect();

            for h in handles {
                h.join().unwrap();
            }
        });
    });

    group.finish();
}

fn bench_write_heavy(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_heavy_90_10");
    let capacity = 1000;
    let ops_per_thread = 5_000;
    let num_threads = 4;
    let total_ops = ops_per_thread * num_threads;

    group.throughput(Throughput::Elements(total_ops as u64));

    group.bench_function("mutex", |b| {
        b.iter(|| {
            let cache = ThreadSafeLruCache::new(capacity);
            let barrier = Arc::new(Barrier::new(num_threads));
            let handles: Vec<_> = (0..num_threads)
                .map(|t| {
                    let cache = cache.clone();
                    let barrier = barrier.clone();
                    thread::spawn(move || {
                        barrier.wait();
                        for i in 0..ops_per_thread {
                            if i % 10 == 0 {
                                // 10% reads
                                black_box(cache.get(&black_box((i % capacity) as u64)));
                            } else {
                                // 90% writes
                                let key = (t * ops_per_thread + i) as u64;
                                cache.put(black_box(key), black_box(key));
                            }
                        }
                    })
                })
                .collect();

            for h in handles {
                h.join().unwrap();
            }
        });
    });

    group.bench_function("rwlock", |b| {
        b.iter(|| {
            let cache = RwLockLruCache::new(capacity);
            let barrier = Arc::new(Barrier::new(num_threads));
            let handles: Vec<_> = (0..num_threads)
                .map(|t| {
                    let cache = cache.clone();
                    let barrier = barrier.clone();
                    thread::spawn(move || {
                        barrier.wait();
                        for i in 0..ops_per_thread {
                            if i % 10 == 0 {
                                black_box(cache.get(&black_box((i % capacity) as u64)));
                            } else {
                                let key = (t * ops_per_thread + i) as u64;
                                cache.put(black_box(key), black_box(key));
                            }
                        }
                    })
                })
                .collect();

            for h in handles {
                h.join().unwrap();
            }
        });
    });

    group.bench_function("sharded", |b| {
        b.iter(|| {
            let cache = Arc::new(ShardedLruCache::new(capacity));
            let barrier = Arc::new(Barrier::new(num_threads));
            let handles: Vec<_> = (0..num_threads)
                .map(|t| {
                    let cache = Arc::clone(&cache);
                    let barrier = barrier.clone();
                    thread::spawn(move || {
                        barrier.wait();
                        for i in 0..ops_per_thread {
                            if i % 10 == 0 {
                                black_box(cache.get(&black_box((i % capacity) as u64)));
                            } else {
                                let key = (t * ops_per_thread + i) as u64;
                                cache.put(black_box(key), black_box(key));
                            }
                        }
                    })
                })
                .collect();

            for h in handles {
                h.join().unwrap();
            }
        });
    });

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    benches,
    bench_single_thread_put,
    bench_single_thread_get,
    bench_single_thread_mixed,
    bench_concurrent_writes,
    bench_concurrent_reads,
    bench_concurrent_mixed,
    bench_read_heavy,
    bench_write_heavy,
);

criterion_main!(benches);
