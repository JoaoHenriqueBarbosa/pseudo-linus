//! Trie de raiz 64 persistente, feita à mão só com `Arc` e `Arc::make_mut`.
//!
//! Chave `u64` densa (inos alocados em sequência, índices de bloco de um arquivo), 6 bits por nível,
//! como o xarray do kernel. Clonar o mapa é clonar o `Arc` da raiz; alterar uma chave depois do clone
//! copia só os nós do caminho (no máximo 11 níveis pra `u64`, 3 níveis até 262144 chaves).
//! Nós vazios são podados na remoção.

use std::sync::Arc;

pub const BITS: u32 = 6;
pub const FANOUT: usize = 1 << BITS;
const MASK: u64 = (FANOUT as u64) - 1;

// As duas variantes têm 64 ponteiros (512 bytes pra `V` do tamanho de um ponteiro); o clippy só não
// enxerga o tamanho de `V` genérico. Encaixotar o vetor poria uma indireção a mais em cada nível.
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
enum Slots<V> {
    Leaf([Option<V>; FANOUT]),
    Inner([Option<Arc<Node<V>>>; FANOUT]),
}

#[derive(Clone)]
struct Node<V> {
    count: u32,
    slots: Slots<V>,
}

impl<V> Node<V> {
    fn leaf() -> Self {
        Node { count: 0, slots: Slots::Leaf(std::array::from_fn(|_| None)) }
    }

    fn inner() -> Self {
        Node { count: 0, slots: Slots::Inner(std::array::from_fn(|_| None)) }
    }

    fn for_level(level: u32) -> Self {
        if level == 0 { Node::leaf() } else { Node::inner() }
    }
}

pub struct RadixMap<V> {
    root: Option<Arc<Node<V>>>,
    /// Número de níveis; a raiz fica no nível `height - 1` e as folhas no nível 0.
    height: u32,
    len: usize,
}

impl<V> Clone for RadixMap<V> {
    fn clone(&self) -> Self {
        RadixMap { root: self.root.clone(), height: self.height, len: self.len }
    }
}

impl<V> Default for RadixMap<V> {
    fn default() -> Self {
        RadixMap { root: None, height: 0, len: 0 }
    }
}

fn capacity(height: u32) -> u128 {
    1u128 << (BITS * height)
}

fn index(key: u64, level: u32) -> usize {
    ((key >> (BITS * level)) & MASK) as usize
}

impl<V: Clone> RadixMap<V> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, key: u64) -> Option<&V> {
        let mut node = self.root.as_deref()?;
        if u128::from(key) >= capacity(self.height) {
            return None;
        }
        let mut level = self.height - 1;
        loop {
            match &node.slots {
                Slots::Leaf(s) => return s[index(key, 0)].as_ref(),
                Slots::Inner(c) => {
                    node = c[index(key, level)].as_deref()?;
                    level -= 1;
                }
            }
        }
    }

    /// Referência mutável ao valor, copiando o caminho se ele estiver compartilhado. Não copia nada
    /// quando a chave não existe.
    pub fn get_mut(&mut self, key: u64) -> Option<&mut V> {
        self.get(key)?;
        let level = self.height - 1;
        get_mut_rec(Arc::make_mut(self.root.as_mut()?), key, level)
    }

    pub fn insert(&mut self, key: u64, value: V) -> Option<V> {
        if self.root.is_none() {
            self.root = Some(Arc::new(Node::leaf()));
            self.height = 1;
        }
        while u128::from(key) >= capacity(self.height) {
            let old = self.root.take().expect("raiz");
            let mut node = Node::inner();
            if let Slots::Inner(c) = &mut node.slots {
                c[0] = Some(old);
            }
            node.count = 1;
            self.root = Some(Arc::new(node));
            self.height += 1;
        }
        let level = self.height - 1;
        let root = Arc::make_mut(self.root.as_mut().expect("raiz"));
        let prev = insert_rec(root, key, level, value);
        if prev.is_none() {
            self.len += 1;
        }
        prev
    }

    pub fn remove(&mut self, key: u64) -> Option<V> {
        self.get(key)?;
        let level = self.height - 1;
        let root = Arc::make_mut(self.root.as_mut()?);
        let value = remove_rec(root, key, level);
        let empty = root.count == 0;
        if value.is_some() {
            self.len -= 1;
        }
        if empty {
            self.root = None;
            self.height = 0;
        }
        value
    }

    pub fn for_each(&self, f: &mut dyn FnMut(u64, &V)) {
        if let Some(root) = &self.root {
            walk(root, self.height - 1, 0, f);
        }
    }

    /// Remove todas as chaves `>= from` (usado pra truncar arquivo).
    pub fn truncate_from(&mut self, from: u64) {
        let mut keys = Vec::new();
        self.for_each(&mut |k, _| {
            if k >= from {
                keys.push(k);
            }
        });
        for k in keys {
            self.remove(k);
        }
    }
}

fn get_mut_rec<V: Clone>(node: &mut Node<V>, key: u64, level: u32) -> Option<&mut V> {
    match &mut node.slots {
        Slots::Leaf(s) => s[index(key, 0)].as_mut(),
        Slots::Inner(c) => get_mut_rec(Arc::make_mut(c[index(key, level)].as_mut()?), key, level - 1),
    }
}

fn insert_rec<V: Clone>(node: &mut Node<V>, key: u64, level: u32, value: V) -> Option<V> {
    match &mut node.slots {
        Slots::Leaf(s) => {
            let prev = s[index(key, 0)].replace(value);
            if prev.is_none() {
                node.count += 1;
            }
            prev
        }
        Slots::Inner(c) => {
            let slot = &mut c[index(key, level)];
            if slot.is_none() {
                *slot = Some(Arc::new(Node::for_level(level - 1)));
                node.count += 1;
            }
            let child = Arc::make_mut(slot.as_mut().expect("filho"));
            insert_rec(child, key, level - 1, value)
        }
    }
}

fn remove_rec<V: Clone>(node: &mut Node<V>, key: u64, level: u32) -> Option<V> {
    match &mut node.slots {
        Slots::Leaf(s) => {
            let value = s[index(key, 0)].take();
            if value.is_some() {
                node.count -= 1;
            }
            value
        }
        Slots::Inner(c) => {
            let slot = &mut c[index(key, level)];
            let child = Arc::make_mut(slot.as_mut()?);
            let value = remove_rec(child, key, level - 1);
            if child.count == 0 {
                *slot = None;
                node.count -= 1;
            }
            value
        }
    }
}

fn walk<V>(node: &Node<V>, level: u32, prefix: u64, f: &mut dyn FnMut(u64, &V)) {
    match &node.slots {
        Slots::Leaf(s) => {
            for (i, v) in s.iter().enumerate() {
                if let Some(v) = v {
                    f(prefix | i as u64, v);
                }
            }
        }
        Slots::Inner(c) => {
            for (i, child) in c.iter().enumerate() {
                if let Some(child) = child {
                    walk(child, level - 1, prefix | ((i as u64) << (BITS * level)), f);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn matches_btreemap_and_shares_structure() {
        let mut m = RadixMap::default();
        let mut r = BTreeMap::new();
        let mut x: u64 = 0x9e3779b97f4a7c15;
        for i in 0..20_000u64 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let k = if i % 3 == 0 { x % 300_000 } else { i };
            if x.is_multiple_of(5) {
                assert_eq!(m.remove(k), r.remove(&k));
            } else {
                assert_eq!(m.insert(k, i), r.insert(k, i));
            }
            assert_eq!(m.len(), r.len());
        }
        let snap = m.clone();
        let snap_ref = r.clone();
        for (k, v) in r.iter_mut() {
            *v += 1;
            *m.get_mut(*k).expect("chave") += 1;
        }
        let mut seen = BTreeMap::new();
        m.for_each(&mut |k, v| {
            seen.insert(k, *v);
        });
        assert_eq!(seen, r);
        let mut seen = BTreeMap::new();
        snap.for_each(&mut |k, v| {
            seen.insert(k, *v);
        });
        assert_eq!(seen, snap_ref);
        m.truncate_from(1000);
        assert!(m.len() <= 1000);
        assert_eq!(m.get(999), r.get(&999));
        assert_eq!(m.get(1000), None);
    }

    #[test]
    fn large_keys() {
        let mut m = RadixMap::default();
        m.insert(u64::MAX, 1);
        m.insert(0, 2);
        assert_eq!(m.get(u64::MAX), Some(&1));
        assert_eq!(m.get(0), Some(&2));
        assert_eq!(m.remove(u64::MAX), Some(1));
        assert_eq!(m.len(), 1);
    }
}
