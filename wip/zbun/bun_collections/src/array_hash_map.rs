//! `src/collections/array_hash_map.rs` do upstream, parcial: o `ArrayHashMap` e o `StringArrayHashMap`, que o
//! `loader.rs`, o `TSNamespaceScope` e o `Part` pedem.
//!
//! No upstream o mapa é o `ArrayHashMap` do Zig portado (entradas num vetor na ordem de inserção, índice de hash
//! separado, 25 trechos `unsafe` no arquivo) com contexto e alocador genéricos. No porte as entradas ficam em dois
//! `Vec` (chaves e valores, na ordem de inserção, com a mesma remoção por troca com o último) e o índice é um
//! `HashMap` da biblioteca padrão da chave para a posição. A ordem observável (iteração, `keys`, `values`,
//! `swap_remove`) é a do upstream; só o hash interno muda, e ele não aparece na saída. O contexto sem diferença de
//! caixa (`CaseInsensitiveAsciiStringArrayHashMap`, o mapa de ambiente do Windows) fica fora, e os sinônimos de uma
//! linha do upstream (`contains_key`, `put_assume_capacity`) não entram, pela regra de repasse do repositório.

use core::borrow::Borrow;
use core::hash::Hash;
use std::collections::HashMap;

/// O erro de alocação que o upstream devolve por `Result`; aqui nunca acontece (a biblioteca padrão aborta), e o
/// tipo existe para as assinaturas ficarem as do upstream. É o mesmo do `bun_alloc`.
pub use bun_alloc::AllocError;

/// Como a chave guardada nasce da chave de busca: `Box<[u8]>` copia os bytes de `[u8]`; uma chave `Copy` (o `Ref`)
/// implementa para si mesma.
pub trait OwnKey<Q: ?Sized> {
    fn own(key: &Q) -> Self;
}

impl OwnKey<[u8]> for Box<[u8]> {
    fn own(key: &[u8]) -> Self {
        Box::from(key)
    }
}

/// Chave `Copy` (inteiro, `Loc`): a própria chave.
impl<T: Copy> OwnKey<T> for T {
    fn own(key: &T) -> Self {
        *key
    }
}

/// `std.ArrayHashMap(K, V)`: as entradas na ordem de inserção. O `Clone` do trait serve ao `derive` do `Store`; o
/// `clone` inerente, falível como o do upstream, continua sendo o da chamada por método.
#[derive(Clone)]
pub struct ArrayHashMap<K, V> {
    keys: Vec<K>,
    values: Vec<V>,
    index: HashMap<K, usize>,
}

/// `std.StringArrayHashMap(V)`.
pub type StringArrayHashMap<V> = ArrayHashMap<Box<[u8]>, V>;

impl<K, V> Default for ArrayHashMap<K, V> {
    fn default() -> Self {
        Self { keys: Vec::new(), values: Vec::new(), index: HashMap::new() }
    }
}

impl<K: Clone, V: Clone> ArrayHashMap<K, V> {
    /// Fallible (OOM) clone; kept as `Result` for API stability.
    pub fn clone(&self) -> Result<Self, AllocError> {
        Ok(Self { keys: self.keys.clone(), values: self.values.clone(), index: self.index.clone() })
    }
}

pub struct GetOrPutResult<'a, K, V> {
    pub found_existing: bool,
    pub index: usize,
    pub key_ptr: &'a mut K,
    pub value_ptr: &'a mut V,
}

/// Owned key/value pair returned by the `fetch_*_remove` methods.
pub struct KV<K, V> {
    pub key: K,
    pub value: V,
}

impl<K: Hash + Eq + Clone, V> ArrayHashMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(n: usize) -> Self {
        Self { keys: Vec::with_capacity(n), values: Vec::with_capacity(n), index: HashMap::with_capacity(n) }
    }

    pub fn count(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn get_index<Q: ?Sized + Hash + Eq>(&self, key: &Q) -> Option<usize>
    where
        K: Borrow<Q>,
    {
        self.index.get(key).copied()
    }

    pub fn contains<Q: ?Sized + Hash + Eq>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.index.contains_key(key)
    }

    pub fn get<Q: ?Sized + Hash + Eq>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.get_index(key).map(|i| &self.values[i])
    }

    pub fn get_mut<Q: ?Sized + Hash + Eq>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
    {
        self.get_index(key).map(|i| &mut self.values[i])
    }

    /// Insere ou troca; devolve o valor anterior.
    pub fn insert<Q: ?Sized + Hash + Eq>(&mut self, key: &Q, value: V) -> Option<V>
    where
        K: Borrow<Q> + OwnKey<Q>,
    {
        match self.get_index(key) {
            Some(i) => Some(core::mem::replace(&mut self.values[i], value)),
            None => {
                self.push_new(K::own(key), value);
                None
            }
        }
    }

    pub fn put<Q: ?Sized + Hash + Eq>(&mut self, key: &Q, value: V) -> Result<(), AllocError>
    where
        K: Borrow<Q> + OwnKey<Q>,
    {
        self.insert(key, value);
        Ok(())
    }

    /// Remove trocando com o último, como o `swapRemove` do Zig: a última entrada passa para a posição removida.
    pub fn swap_remove<Q: ?Sized + Hash + Eq>(&mut self, key: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.fetch_swap_remove(key).is_some()
    }

    /// Tira a última entrada inserida (`pop` do upstream).
    pub fn pop(&mut self) -> Option<KV<K, V>>
    where
        K: Hash + Eq,
    {
        let key = self.keys.pop()?;
        let value = self.values.pop().expect("chaves e valores do ArrayHashMap do mesmo tamanho");
        self.index.remove(&key);
        Some(KV { key, value })
    }

    pub fn fetch_swap_remove<Q: ?Sized + Hash + Eq>(&mut self, key: &Q) -> Option<KV<K, V>>
    where
        K: Borrow<Q>,
    {
        let i = self.index.remove(key)?;
        let removed_key = self.keys.swap_remove(i);
        let value = self.values.swap_remove(i);
        if let Some(moved) = self.keys.get(i) {
            *self.index.get_mut::<K>(moved).expect("índice do ArrayHashMap coerente") = i;
        }
        Some(KV { key: removed_key, value })
    }

    pub fn keys(&self) -> &[K] {
        &self.keys
    }

    pub fn values(&self) -> &[V] {
        &self.values
    }

    pub fn values_mut(&mut self) -> &mut [V] {
        &mut self.values
    }

    /// As entradas na ordem do mapa.
    pub fn iterator(&self) -> impl Iterator<Item = (&K, &V)> {
        self.keys.iter().zip(self.values.iter())
    }

    pub fn clear(&mut self) {
        self.keys.clear();
        self.values.clear();
        self.index.clear();
    }

    /// Garante espaço para `n` entradas no total, sem realocar até lá.
    pub fn ensure_total_capacity(&mut self, n: usize) -> Result<(), AllocError> {
        let extra = n.saturating_sub(self.keys.len());
        self.ensure_unused_capacity(extra)
    }

    /// Garante espaço para mais `n` entradas além das que já existem.
    pub fn ensure_unused_capacity(&mut self, n: usize) -> Result<(), AllocError> {
        self.keys.reserve(n);
        self.values.reserve(n);
        self.index.reserve(n);
        Ok(())
    }

    /// Fica só com as `n` primeiras entradas (na ordem de inserção) e solta a capacidade que sobra.
    pub fn shrink_and_free(&mut self, n: usize) {
        if n >= self.keys.len() {
            return;
        }
        for key in self.keys.drain(n..) {
            self.index.remove(&key);
        }
        self.values.truncate(n);
        self.keys.shrink_to_fit();
        self.values.shrink_to_fit();
        self.index.shrink_to_fit();
    }

    /// Como o `get_or_put`, mas a entrada nova nasce com `value` no lugar de `V::default()`.
    pub fn get_or_put_value<Q: ?Sized + Hash + Eq>(
        &mut self,
        key: &Q,
        value: V,
    ) -> Result<GetOrPutResult<'_, K, V>, AllocError>
    where
        K: Borrow<Q> + OwnKey<Q>,
    {
        let (found_existing, index) = match self.get_index(key) {
            Some(i) => (true, i),
            None => (false, self.push_new(K::own(key), value)),
        };
        Ok(GetOrPutResult { found_existing, index, key_ptr: &mut self.keys[index], value_ptr: &mut self.values[index] })
    }

    fn push_new(&mut self, key: K, value: V) -> usize {
        let i = self.keys.len();
        self.index.insert(key.clone(), i);
        self.keys.push(key);
        self.values.push(value);
        i
    }
}

impl<K: Hash + Eq + Clone, V: Default> ArrayHashMap<K, V> {
    /// A entrada nova nasce com `V::default()`.
    pub fn get_or_put<Q: ?Sized + Hash + Eq>(&mut self, key: &Q) -> Result<GetOrPutResult<'_, K, V>, AllocError>
    where
        K: Borrow<Q> + OwnKey<Q>,
    {
        self.get_or_put_value(key, V::default())
    }
}

// ──────────────────────────────────────────────────────────────────────────
// StringHashMap<V>: unordered `[]const u8`-keyed map
// ──────────────────────────────────────────────────────────────────────────

/// Newtype over `hashbrown::HashMap` that adds the `get_or_put` / `put` entry points while keeping the
/// `hashbrown` surface reachable via `Deref`. Hashed with seed-0 wyhash.
///
/// No upstream a chave é o `StringHashMapKey` (ponteiro e comprimento empacotados, dono ou emprestado
/// `'static`) e o alocador é genérico. Aqui a chave é um `Box<[u8]>`: o `Hash` dela é o de `[u8]`, o mesmo que a
/// do upstream usa, e o `hashbrown` é a mesma versão, então a ordem de iteração é a dele. O `put_static_key`, que
/// só evita a cópia da chave, fica fora.
pub struct StringHashMap<V> {
    inner: hashbrown::HashMap<Box<[u8]>, V, bun_wyhash::BuildHasher>,
}

impl<V> Default for StringHashMap<V> {
    fn default() -> Self {
        Self { inner: hashbrown::HashMap::default() }
    }
}

impl<V: Clone> Clone for StringHashMap<V> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<V> core::ops::Deref for StringHashMap<V> {
    type Target = hashbrown::HashMap<Box<[u8]>, V, bun_wyhash::BuildHasher>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<V> core::ops::DerefMut for StringHashMap<V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<V> StringHashMap<V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.inner.len()
    }

    pub fn ensure_total_capacity(&mut self, n: usize) -> Result<(), AllocError> {
        let need = n.saturating_sub(self.inner.len());
        self.inner.reserve(need);
        Ok(())
    }

    /// Copia a chave só quando ela falta.
    pub fn put(&mut self, key: &[u8], value: V) -> Result<(), AllocError> {
        self.inner.entry_ref(key).insert(value);
        Ok(())
    }

    pub fn get_or_put_value(&mut self, key: &[u8], value: V) -> Result<&mut V, AllocError> {
        Ok(self.inner.entry_ref(key).or_insert(value))
    }
}

/// `StringHashMap::get_or_put` result: `hashbrown` cannot hand out `&mut K`, so this result omits `key_ptr`.
pub use crate::hash_map::GetOrPutResult as StringHashMapGetOrPut;

impl<V: Default> StringHashMap<V> {
    pub fn get_or_put(&mut self, key: &[u8]) -> Result<StringHashMapGetOrPut<'_, V>, AllocError> {
        Ok(match self.inner.entry_ref(key) {
            hashbrown::hash_map::EntryRef::Occupied(o) => {
                StringHashMapGetOrPut { found_existing: true, value_ptr: o.into_mut() }
            }
            hashbrown::hash_map::EntryRef::Vacant(v) => {
                StringHashMapGetOrPut { found_existing: false, value_ptr: v.insert(V::default()) }
            }
        })
    }
}

// ──────────────────────────────────────────────────────────────────────────
// StringSet: `StringArrayHashMap<()>` with key-duping insert
// ──────────────────────────────────────────────────────────────────────────

/// `bun.StringSet`: insertion-ordered set of owned byte-string keys.
#[derive(Default)]
pub struct StringSet {
    pub map: StringArrayHashMap<()>,
}

impl StringSet {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clone(&self) -> Result<Self, AllocError> {
        Ok(Self { map: self.map.clone()? })
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.count() == 0
    }

    #[inline]
    pub fn keys(&self) -> &[Box<[u8]>] {
        self.map.keys()
    }

    /// Insert `key`, duping it on miss. Returns `Ok(())` whether or not the key
    /// was already present.
    pub fn insert(&mut self, key: &[u8]) -> Result<(), AllocError> {
        let _ = self.map.get_or_put(key)?;
        Ok(())
    }

    #[inline]
    pub fn contains(&self, key: &[u8]) -> bool {
        self.map.contains(key)
    }

    #[inline]
    pub fn swap_remove(&mut self, key: &[u8]) -> bool {
        self.map.swap_remove(key)
    }
}

#[cfg(test)]
mod tests {
    use super::StringArrayHashMap;

    #[test]
    fn insertion_order_and_swap_remove() {
        let mut map = StringArrayHashMap::new();
        for (k, v) in [(&b"a"[..], 1), (b"b", 2), (b"c", 3), (b"d", 4)] {
            map.put(k, v).unwrap();
        }
        assert_eq!(map.insert(&b"b"[..], 20), Some(2));
        assert!(map.swap_remove(&b"a"[..]));
        // O último ("d") ocupa a posição removida, como no Zig.
        let order: Vec<(&[u8], i32)> = map.iterator().map(|(k, v)| (&**k, *v)).collect();
        assert_eq!(order, [(&b"d"[..], 4), (b"b", 20), (b"c", 3)]);
        assert_eq!(map.get(&b"d"[..]), Some(&4));
        assert_eq!(map.get_index(&b"c"[..]), Some(2));
        let gop = map.get_or_put(&b"e"[..]).unwrap();
        assert!(!gop.found_existing);
        *gop.value_ptr = 5;
        assert_eq!(map.count(), 4);
        assert!(map.get_or_put(&b"e"[..]).unwrap().found_existing);
    }

    #[test]
    fn string_hash_map_and_set() {
        let mut map = super::StringHashMap::<u32>::new();
        map.put(b"x", 1).unwrap();
        map.put(b"x", 2).unwrap();
        assert_eq!(map.get(&b"x"[..]), Some(&2));
        let gop = map.get_or_put(b"y").unwrap();
        assert!(!gop.found_existing);
        *gop.value_ptr = 9;
        assert_eq!(*map.get_or_put_value(b"y", 0).unwrap(), 9);
        assert_eq!(map.count(), 2);
        let mut set = super::StringSet::new();
        set.insert(b"b").unwrap();
        set.insert(b"a").unwrap();
        set.insert(b"b").unwrap();
        assert_eq!(set.keys(), [Box::from(&b"b"[..]), Box::from(&b"a"[..])]);
        assert!(set.contains(b"a") && !set.is_empty());
    }
}
