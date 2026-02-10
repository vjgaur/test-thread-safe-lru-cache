// Core LRU Cache implementation
//
// Data structure: HashMap<K, usize> + Vec<Node<K,V>> arena-based doubly-linked list
//
// The linked list maintains access recency:
//   head (most recent) <-> ... <-> tail (least recent)
//
// Arena approach: Nodes live in a Vec, referenced by index. This avoids
// unsafe pointer manipulation while keeping O(1) linked list operations.
// Evicted slots are recycled via a free list.

use std::collections::HashMap;
use std::hash::Hash;

//A Node in the arena-baed doubly-linked list stored in a Vec-based arena
struct Node<K,V> {
    key: K,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

/// Single-threaded LRU cache
/// Not safe for concurrent use 
///Check `ThreadSafeLruCache` for the thread-safe wrapper
pub struct LruCache<K,V> {
    capacity: usize,
    map: HashMap<K,usize>, // key -> index in nodes arena
    nodes: Vec<Node<K,V>>, // arena for linked list nodes
    head: Option<usize>, // most recently used
    tail: Option<usize>, // least recently used
    free_list: Vec<usize>, // reusable slots from evicted nodes
}

impl<K: Hash + Eq + Clone, V> LruCache<K,V> {
    /// Creates a new LRU cache with the given maximum capacity.
    ///
    /// # Panics
    /// Panics if `capacity` is 0.
    pub fn new(capacity: usize)-> Self {
        assert!(capacity > 0, "Cache capacity must be greater than 0");

        Self {
            capacity,
            map: HashMap::with_capacity(capacity),
            nodes: Vec::with_capacity(capacity),
            head: None,
            tail: None,
            free_list:Vec::new(),
        }
    }


// ---------------------------------------------------------------
    // Linked list helpers (private)
    // ---------------------------------------------------------------

    /// Unlinks a node from its current position in the doubly-linked list.
    /// Does NOT remove it from the map or free the slot.

fn unlink(&mut self, index:usize){

    let prev = self.nodes[index].prev;
    let next = self.nodes[index].next;

    //fix the previous node's next pointer (or update head)
    match prev  {
        Some(p) => self.nodes[p].next = next,
        None => self.head = next, //node was head
    }

    //Fix the previous node's prev pointer (or update tail)
    match next {
        Some(n) => self.nodes[n].prev = prev,
        None => self.tail = prev,
    }
    //Clear the node's own pointers
    self.nodes[index].prev = None;
    self.nodes[index].next = None;

}
/// Inserts a node at the head of the list (most recently used position).
/// Assumes the node is already unlinked.
fn push_front(&mut self, index: usize){
    
    self.nodes[index].prev = None;
    self.nodes[index].next = self.head;

    if let Some(old_head) = self.head {
        self.nodes[old_head].prev = Some(index);
    }
    self.head = Some(index);

    // if list was empty this node is also the tail 
    if self.tail.is_none(){
        self.tail = Some(index);
    }
}

/// Evicts the tail node (least recently used).
/// Returns the evicted key so the caller can remove it from the map.
/// The slot is added to the free list for reuse.
fn evict_tail(&mut self) -> Option<K> {
    let tail_index = self.tail?;
    self.unlink(tail_index);

    //Clone the key before recycling the slot 
    let evicated_key = self.nodes[tail_index].key.clone();

    //Add the slot to the free list for reuse 
    self.free_list.push(tail_index);

    Some(evicated_key)
}

/// Allocates a slot in the arena for a new node.
/// Reuses a free slot if available, otherwise pushes to the Vec.
fn allocate_node(&mut self, key: K, value: V) -> usize {
    if let Some(index) = self.free_list.pop() {
        // Reuse an evicted slot
        self.nodes[index] = Node {
            key,
            value,
            prev: None,
            next: None,
        };
        index
    } else {
        // Allocate a new slot at the end of the arena
        let index = self.nodes.len();
        self.nodes.push(Node {
            key,
            value,
            prev: None,
            next: None,
        });
        index
    }
}

// ---------------------------------------------------------------
// Public API
// ---------------------------------------------------------------

/// Retrieves the value for a key, marking it as most recently used.
///
/// Returns `None` if the key is not present.

pub fn get(&mut self, key: &K)-> Option<&V> {
    // Look up the index; we need to separate the borrow of self.map
    // from the mutable borrows in unlink/push_front.
    let index = *self.map.get(key)?;

    // Move to front (mark as most recently used)
    self.unlink(index);
    self.push_front(index);

    Some(&self.nodes[index].value)
}

/// Inserts a key-value pair into the cache.
///
/// If the key already exists, its value is updated and it becomes
/// the most recently used. If the cache is at capacity, the least
/// recently used entry is evicted.
pub fn put(&mut self, key: K, value: V) {
    if let Some(&index) = self.map.get(&key) {
            // Key exists: update value in place, move to front
            self.nodes[index].value = value;
            self.unlink(index);
            self.push_front(index);
        } else {
            // Key is new: evict if at capacity
            if self.map.len() >= self.capacity {
                if let Some(evicted_key) = self.evict_tail() {
                    self.map.remove(&evicted_key);
                }
            }

            // Allocate and insert new node at head
            let index = self.allocate_node(key.clone(), value);
            self.push_front(index);
            self.map.insert(key, index);
        }
}
pub fn len(&self) -> usize{
    self.map.len()
}
pub fn is_empty(&self)->bool {
    self.map.is_empty()
}
}

// ---------------------------------------------------------------
// Tests
// ---------------------------------------------------------------


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_put_and_get() {
        let mut cache = LruCache::new(3);
        cache.put("a", 1);
        cache.put("b", 2);
        cache.put("c", 3);

        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get(&"b"), Some(&2));
        assert_eq!(cache.get(&"c"), Some(&3));
    }

    #[test]
    fn test_get_nonexistent_key() {
        let mut cache: LruCache<&str, i32> = LruCache::new(2);
        assert_eq!(cache.get(&"missing"), None);
    }

    #[test]
    fn test_eviction_removes_lru() {
        let mut cache = LruCache::new(2);
        cache.put("a", 1);
        cache.put("b", 2);
        // Cache: head=[b] -> [a]=tail

        cache.put("c", 3); // evicts "a" (least recently used)
        // Cache: head=[c] -> [b]=tail

        assert_eq!(cache.get(&"a"), None); // evicted
        assert_eq!(cache.get(&"b"), Some(&2));
        assert_eq!(cache.get(&"c"), Some(&3));
    }

    #[test]
    fn test_get_updates_recency() {
        let mut cache = LruCache::new(2);
        cache.put("a", 1);
        cache.put("b", 2);
        // Cache: head=[b] -> [a]=tail

        cache.get(&"a"); // "a" is now most recent
        // Cache: head=[a] -> [b]=tail

        cache.put("c", 3); // evicts "b" (now least recent)
        // Cache: head=[c] -> [a]=tail

        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get(&"b"), None); // evicted
        assert_eq!(cache.get(&"c"), Some(&3));
    }

    #[test]
    fn test_put_updates_existing_key() {
        let mut cache = LruCache::new(2);
        cache.put("a", 1);
        cache.put("b", 2);

        cache.put("a", 10); // update existing key

        assert_eq!(cache.get(&"a"), Some(&10));
        assert_eq!(cache.len(), 2); // no extra entry
    }

    #[test]
    fn test_capacity_one() {
        let mut cache = LruCache::new(1);
        cache.put("a", 1);
        assert_eq!(cache.get(&"a"), Some(&1));

        cache.put("b", 2); // evicts "a"
        assert_eq!(cache.get(&"a"), None);
        assert_eq!(cache.get(&"b"), Some(&2));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_len_and_is_empty() {
        let mut cache: LruCache<&str, i32> = LruCache::new(3);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        cache.put("a", 1);
        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);

        cache.put("b", 2);
        cache.put("c", 3);
        assert_eq!(cache.len(), 3);

        cache.put("d", 4); // evicts one
        assert_eq!(cache.len(), 3); // still at capacity
    }

    #[test]
    fn test_eviction_order_sequential() {
        let mut cache = LruCache::new(3);
        cache.put(1, "a");
        cache.put(2, "b");
        cache.put(3, "c");
        // Order: head=[3] -> [2] -> [1]=tail

        cache.put(4, "d"); // evicts 1
        assert_eq!(cache.get(&1), None);

        cache.put(5, "e"); // evicts 2
        assert_eq!(cache.get(&2), None);

        cache.put(6, "f"); // evicts 3
        assert_eq!(cache.get(&3), None);

        // Only 4, 5, 6 remain
        assert_eq!(cache.get(&4), Some(&"d"));
        assert_eq!(cache.get(&5), Some(&"e"));
        assert_eq!(cache.get(&6), Some(&"f"));
    }

    #[test]
    fn test_update_existing_preserves_capacity() {
        let mut cache = LruCache::new(2);
        cache.put("a", 1);
        cache.put("b", 2);

        // Repeatedly update same keys — should never evict
        for i in 0..100 {
            cache.put("a", i);
            cache.put("b", i * 2);
        }

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&"a"), Some(&99));
        assert_eq!(cache.get(&"b"), Some(&198));
    }

    #[test]
    #[should_panic(expected = "Cache capacity must be greater than 0")]
    fn test_zero_capacity_panics() {
        let _cache: LruCache<&str, i32> = LruCache::new(0);
    }
}