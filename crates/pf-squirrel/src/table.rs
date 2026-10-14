//! Script tables, laid out as the game's (launcher.dll) so that `foreach`
//! visits keys in the same order the game does: scripts can see that order
//! (docs/port/scripts.md, "Tables"; checked against oracle run 2).

use crate::value::{Ref, Value};

#[derive(Clone, Default)]
struct Node {
    /// Null for an empty node.
    key: Value,
    val: Value,
    /// The next node in this slot's chain.
    next: Option<usize>,
}

pub struct Table {
    /// Always a power of two long.
    nodes: Vec<Node>,
    /// The "free" cursor: new keys that collide go here. Starts at the last
    /// node and only moves down.
    free: usize,
    count: usize,
    /// The table's delegate (`delegate d : t`, `setdelegate`).
    pub delegate: Option<Ref<Table>>,
}

impl Default for Table {
    fn default() -> Table {
        Table::with_capacity(0)
    }
}

impl Table {
    pub fn new() -> Table {
        Table::default()
    }

    /// A table sized for `n` keys: 4, or the next power of two up.
    pub fn with_capacity(n: usize) -> Table {
        let mut size = 4;
        while size < n {
            size *= 2;
        }
        let mut t = Table {
            nodes: Vec::new(),
            free: 0,
            count: 0,
            delegate: None,
        };
        t.alloc(size);
        t
    }

    fn alloc(&mut self, size: usize) {
        self.nodes = vec![Node::default(); size];
        self.free = size - 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn main_slot(&self, key: &Value) -> usize {
        (key.key_hash() & (self.nodes.len() as u64 - 1)) as usize
    }

    fn find(&self, key: &Value) -> Option<usize> {
        let mut i = Some(self.main_slot(key));
        while let Some(n) = i {
            let node = &self.nodes[n];
            if !node.key.is_null() && node.key.raw_eq(key) {
                return Some(n);
            }
            i = node.next;
        }
        None
    }

    /// The value for `key`, without delegates.
    pub fn get(&self, key: &Value) -> Option<&Value> {
        self.find(key).map(|n| &self.nodes[n].val)
    }

    pub fn contains(&self, key: &Value) -> bool {
        self.find(key).is_some()
    }

    /// Change an existing key's value. False if the key isn't there.
    pub fn set(&mut self, key: &Value, val: Value) -> bool {
        match self.find(key) {
            Some(n) => {
                self.nodes[n].val = val;
                true
            }
            None => false,
        }
    }

    /// Add `key` or change its value (`<-`). The key must not be null.
    pub fn insert(&mut self, key: Value, val: Value) {
        debug_assert!(!key.is_null());
        loop {
            if self.set(&key, val.clone()) {
                return;
            }
            let main = self.main_slot(&key);
            let mut slot = main;
            if !self.nodes[main].key.is_null() {
                let free = self.free;
                let other_main = self.main_slot(&self.nodes[main].key);
                if free < main && other_main != main {
                    // The key sitting here belongs elsewhere: move it to the
                    // free node and take its place.
                    let mut prev = other_main;
                    while self.nodes[prev].next != Some(main) {
                        prev = self.nodes[prev].next.expect("broken table chain");
                    }
                    self.nodes[prev].next = Some(free);
                    self.nodes[free] = std::mem::take(&mut self.nodes[main]);
                } else {
                    // Otherwise the new key goes in the free node, chained
                    // right after its main slot.
                    self.nodes[free].next = self.nodes[main].next;
                    self.nodes[main].next = Some(free);
                    slot = free;
                }
            }
            self.nodes[slot].key = key.clone();
            // Walk the free cursor down to a node with no key and no link.
            loop {
                let f = &self.nodes[self.free];
                if f.key.is_null() && f.next.is_none() {
                    self.nodes[slot].val = val;
                    self.count += 1;
                    return;
                }
                if self.free == 0 {
                    break;
                }
                self.free -= 1;
            }
            // Full: resize (the half-placed key is carried over) and retry.
            self.resize(true);
        }
    }

    /// Remove `key`, returning its value.
    pub fn remove(&mut self, key: &Value) -> Option<Value> {
        let n = self.find(key)?;
        // The node keeps its chain link, as in the game.
        self.nodes[n].key = Value::Null;
        let old = std::mem::take(&mut self.nodes[n].val);
        self.count -= 1;
        self.resize(false);
        Some(old)
    }

    /// Grow when three quarters full, shrink when a quarter full; `force`
    /// rebuilds at the same size otherwise (a full insert).
    fn resize(&mut self, force: bool) {
        let size = self.nodes.len().max(4);
        let quarter = size / 4;
        let new_size = if self.count >= size - quarter {
            size * 2
        } else if quarter < self.count || size < 5 {
            if !force {
                return;
            }
            size
        } else {
            size / 2
        };
        let old = std::mem::take(&mut self.nodes);
        self.alloc(new_size);
        self.count = 0;
        // Re-inserted in node order, first to last.
        for node in old {
            if !node.key.is_null() {
                self.insert(node.key, node.val);
            }
        }
    }

    /// The first key at node `from` or after: (its node, key, value). This is
    /// `foreach`'s order.
    pub fn next(&self, from: usize) -> Option<(usize, Value, Value)> {
        self.nodes
            .iter()
            .enumerate()
            .skip(from)
            .find(|(_, n)| !n.key.is_null())
            .map(|(i, n)| (i, n.key.clone(), n.val.clone()))
    }

    /// Every (key, value), in `foreach` order.
    pub fn iter(&self) -> impl Iterator<Item = (&Value, &Value)> {
        self.nodes
            .iter()
            .filter(|n| !n.key.is_null())
            .map(|n| (&n.key, &n.val))
    }

    /// `clone`: a shallow copy, same size, keys re-inserted in node order.
    pub fn shallow_clone(&self) -> Table {
        let mut t = Table::with_capacity(self.nodes.len());
        for (k, v) in self.iter() {
            t.insert(k.clone(), v.clone());
        }
        t.delegate = self.delegate.clone();
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(t: &Table) -> Vec<String> {
        t.iter()
            .map(|(k, _)| match k {
                Value::Str(s) => s.to_string_lossy(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    // The key orders the real game printed (oracle run 2, O001–O006).

    #[test]
    fn insert_order() {
        let mut t = Table::new();
        for c in ["a", "b", "c", "d", "e", "f", "g", "h"] {
            t.insert(Value::str(c), Value::Int(1));
        }
        assert_eq!(keys(&t).concat(), "acbedgfh");
    }

    #[test]
    fn numbered_string_keys() {
        let mut t = Table::new();
        for i in 0..20 {
            t.insert(Value::str(&format!("key{i}")), Value::Int(i));
        }
        let order: Vec<i32> = t
            .iter()
            .map(|(_, v)| match v {
                Value::Int(i) => *i,
                _ => -1,
            })
            .collect();
        assert_eq!(
            order,
            [
                1, 0, 3, 2, 5, 4, 7, 6, 9, 8, 10, 11, 15, 14, 13, 12, 19, 18, 17, 16
            ]
        );
    }

    #[test]
    fn integer_keys() {
        let mut t = Table::new();
        for i in 0..40 {
            t.insert(Value::Int(i * 7), Value::Int(i));
        }
        let order: Vec<String> = keys(&t);
        assert_eq!(
            order[..12].join(","),
            "0,259,196,133,70,7,266,203,140,77,14,273"
        );
        assert_eq!(order.len(), 40);
    }

    #[test]
    fn literal_sized_by_entry_count() {
        let mut t = Table::with_capacity(5);
        for k in ["zebra", "apple", "mango", "kiwi", "banana"] {
            t.insert(Value::str(k), Value::Int(1));
        }
        assert_eq!(keys(&t), ["banana", "apple", "kiwi", "zebra", "mango"]);
    }

    #[test]
    fn deletes_then_insert() {
        let mut t = Table::new();
        for i in 0..10 {
            t.insert(Value::str(&format!("k{i}")), Value::Int(i));
        }
        for i in (0..10).step_by(2) {
            assert!(t.remove(&Value::str(&format!("k{i}"))).is_some());
        }
        t.insert(Value::str("extra"), Value::Int(99));
        assert_eq!(keys(&t), ["k1", "k3", "k5", "k7", "k9", "extra"]);
        assert_eq!(t.len(), 6);
    }
}
