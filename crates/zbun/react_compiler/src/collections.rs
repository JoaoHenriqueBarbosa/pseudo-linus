//! `IndexMap` / `IndexSet` do React Compiler: o `indexmap` com `FxBuildHasher`, em newtype só para ter `new()` e a
//! `remove` que preserva a ordem (a `remove` do `indexmap` 2.x é um alias de `swap_remove`).
//!
//! O resto da superfície (`get`, `insert`, `entry`, `iter`, `retain`, `shift_remove`...) vem por `Deref`.

use core::hash::Hash;
use core::ops::{Deref, DerefMut};

use rustc_hash::FxBuildHasher;

/// Mapa e conjunto sem ordem, de chaves pequenas e `Copy`: o SipHash do `std` é a escolha errada para ids densos
/// de `u32`.
#[allow(clippy::disallowed_types)]
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuildHasher>;
#[allow(clippy::disallowed_types)]
pub type FxHashSet<K> = std::collections::HashSet<K, FxBuildHasher>;

pub(crate) use indexmap::map::Entry;

// ──────────────────────────────────────────────────────────────────────────
// IndexMap
// ──────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct IndexMap<K, V>(indexmap::IndexMap<K, V, FxBuildHasher>);

impl<K, V> IndexMap<K, V> {
    pub(crate) fn new() -> Self {
        Self(indexmap::IndexMap::with_hasher(FxBuildHasher))
    }
}

impl<K: Hash + Eq, V> IndexMap<K, V> {
    /// Remoção que preserva a ordem, O(n).
    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        self.0.shift_remove(key)
    }
}

impl<K: Hash + Eq, V: PartialEq> PartialEq for IndexMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K: Hash + Eq, V: Eq> Eq for IndexMap<K, V> {}

impl<K, V> Deref for IndexMap<K, V> {
    type Target = indexmap::IndexMap<K, V, FxBuildHasher>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K, V> DerefMut for IndexMap<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<K, V> Default for IndexMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Hash + Eq, V> Extend<(K, V)> for IndexMap<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl<K: Hash + Eq, V> FromIterator<(K, V)> for IndexMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut m = Self::new();
        m.extend(iter);
        m
    }
}

impl<K: Hash + Eq, V, const N: usize> From<[(K, V); N]> for IndexMap<K, V> {
    fn from(arr: [(K, V); N]) -> Self {
        Self::from_iter(arr)
    }
}

impl<K, V> IntoIterator for IndexMap<K, V> {
    type Item = (K, V);
    type IntoIter = indexmap::map::IntoIter<K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a IndexMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = indexmap::map::Iter<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// IndexSet
// ──────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct IndexSet<K>(indexmap::IndexSet<K, FxBuildHasher>);

impl<K> IndexSet<K> {
    pub(crate) fn new() -> Self {
        Self(indexmap::IndexSet::with_hasher(FxBuildHasher))
    }
}

impl<K: Hash + Eq> PartialEq for IndexSet<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K: Hash + Eq> Eq for IndexSet<K> {}

impl<K> Deref for IndexSet<K> {
    type Target = indexmap::IndexSet<K, FxBuildHasher>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K> DerefMut for IndexSet<K> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<K> Default for IndexSet<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Hash + Eq> Extend<K> for IndexSet<K> {
    fn extend<I: IntoIterator<Item = K>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl<K: Hash + Eq> FromIterator<K> for IndexSet<K> {
    fn from_iter<I: IntoIterator<Item = K>>(iter: I) -> Self {
        let mut s = Self::new();
        s.extend(iter);
        s
    }
}

impl<K: Hash + Eq, const N: usize> From<[K; N]> for IndexSet<K> {
    fn from(arr: [K; N]) -> Self {
        Self::from_iter(arr)
    }
}

impl<K> IntoIterator for IndexSet<K> {
    type Item = K;
    type IntoIter = indexmap::set::IntoIter<K>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, K> IntoIterator for &'a IndexSet<K> {
    type Item = &'a K;
    type IntoIter = indexmap::set::Iter<'a, K>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// IdMap
// ──────────────────────────────────────────────────────────────────────────

/// Mapa em ordem de inserção de chave `u32` newtype. Guarda as chaves como `u32` cru, então todo newtype de id
/// compartilha uma monomorfização do mapa de baixo.
pub struct IdMap<K, V>(IndexMap<u32, V>, core::marker::PhantomData<K>);

impl<K: Copy + Into<u32>, V> IdMap<K, V> {
    pub(crate) fn new() -> Self {
        Self(IndexMap::new(), core::marker::PhantomData)
    }
    pub(crate) fn get(&self, k: K) -> Option<&V> {
        self.0.get(&k.into())
    }
    pub(crate) fn get_mut(&mut self, k: K) -> Option<&mut V> {
        self.0.get_mut(&k.into())
    }
    pub(crate) fn insert(&mut self, k: K, v: V) -> Option<V> {
        self.0.insert(k.into(), v)
    }
    pub(crate) fn contains_key(&self, k: K) -> bool {
        self.0.contains_key(&k.into())
    }
    pub(crate) fn entry(&mut self, k: K) -> Entry<'_, u32, V> {
        self.0.entry(k.into())
    }
    pub(crate) fn remove(&mut self, k: K) -> Option<V> {
        self.0.remove(&k.into())
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = (K, &V)>
    where
        K: From<u32>,
    {
        self.0.iter().map(|(k, v)| (K::from(*k), v))
    }
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(crate) fn values(&self) -> impl Iterator<Item = &V> {
        self.0.values()
    }
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.0.values_mut()
    }
}

impl<K, V> Default for IdMap<K, V> {
    fn default() -> Self {
        Self(IndexMap::new(), core::marker::PhantomData)
    }
}

impl<K, V: Clone> Clone for IdMap<K, V> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), core::marker::PhantomData)
    }
}
