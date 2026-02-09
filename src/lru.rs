
use std::collections:Hash;
use std::hash::Hash;

//A Node in the doubly-linked list stored in a Vec-based arena
struct Node<K,V> {
    key: K,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

/// Single-threaded LRU cache
pub struct LruCache<K,V> {
    capacity: usize,
    map: HashMap<K,usize>,
    nodes: Vec<Node<K,V>>,
    head: Option<usize>,
    tail: Option<usize>,
    free_list: Vec<usize>,
}

pub fn get(&mut self, key: &K)-> Option<&V> {
    todo!()
}

pub fn put(&mut self, key:K, value: V) {
    todo!()
}
pub fn len(&self) -> usize{
    self.map.len()
}
pub fn is_empty(&self)->bool {
    self.map.is_empty()
}