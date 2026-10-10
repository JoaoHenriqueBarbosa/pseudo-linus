//! `src/collections/array_hash_map.rs` do upstream, parcial: o `StringArrayHashMap`, que o `loader.rs` e o
//! `TSNamespaceScope` pedem.
//!
//! No upstream o mapa é o `ArrayHashMap` do Zig portado (entradas num vetor na ordem de inserção, índice de hash
//! separado, 25 trechos `unsafe` no arquivo) com contexto e alocador genéricos. No porte as entradas ficam em dois
//! `Vec` (chaves e valores, na ordem de inserção, com a mesma remoção por troca com o último) e o índice é um
//! `HashMap` da biblioteca padrão da chave para a posição. A ordem observável (iteração, `keys`, `values`,
//! `swap_remove`) é a do upstream; só o hash interno muda, e ele não aparece na saída. O contexto sem diferença de
//! caixa (`CaseInsensitiveAsciiStringArrayHashMap`, o mapa de ambiente do Windows) fica fora, e os sinônimos de uma
//! linha do upstream (`contains_key`, `put_assume_capacity`) não entram, pela regra de repasse do repositório.

use std::collections::HashMap;

/// O erro de alocação que o upstream devolve por `Result`; aqui nunca acontece (a biblioteca padrão aborta), e o
/// tipo existe para as assinaturas ficarem as do upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocError;

/// `std.StringArrayHashMap(V)`.
pub struct StringArrayHashMap<V> {
    keys: Vec<Box<[u8]>>,
    values: Vec<V>,
    index: HashMap<Box<[u8]>, usize>,
}

impl<V> Default for StringArrayHashMap<V> {
    fn default() -> Self {
        Self {
            keys: Vec::new(),
            values: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<V: Clone> StringArrayHashMap<V> {
    /// Fallible (OOM) clone; kept as `Result` for API stability.
    pub fn clone(&self) -> Result<Self, AllocError> {
        Ok(Self {
            keys: self.keys.clone(),
            values: self.values.clone(),
            index: self.index.clone(),
        })
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

impl<V> StringArrayHashMap<V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(n: usize) -> Self {
        Self {
            keys: Vec::with_capacity(n),
            values: Vec::with_capacity(n),
            index: HashMap::with_capacity(n),
        }
    }

    pub fn count(&self) -> usize {
        self.keys.len()
    }

    pub fn get_index(&self, key: &[u8]) -> Option<usize> {
        self.index.get(key).copied()
    }

    pub fn contains(&self, key: &[u8]) -> bool {
        self.index.contains_key(key)
    }

    pub fn get(&self, key: &[u8]) -> Option<&V> {
        self.get_index(key).map(|i| &self.values[i])
    }

    pub fn get_mut(&mut self, key: &[u8]) -> Option<&mut V> {
        self.get_index(key).map(|i| &mut self.values[i])
    }

    /// Insere ou troca; devolve o valor anterior.
    pub fn insert(&mut self, key: &[u8], value: V) -> Option<V> {
        match self.get_index(key) {
            Some(i) => Some(core::mem::replace(&mut self.values[i], value)),
            None => {
                self.push_new(key, value);
                None
            }
        }
    }

    pub fn put(&mut self, key: &[u8], value: V) -> Result<(), AllocError> {
        self.insert(key, value);
        Ok(())
    }

    /// Remove trocando com o último, como o `swapRemove` do Zig: a última entrada passa para a posição removida.
    pub fn swap_remove(&mut self, key: &[u8]) -> bool {
        self.fetch_swap_remove(key).is_some()
    }

    pub fn fetch_swap_remove(&mut self, key: &[u8]) -> Option<KV<Box<[u8]>, V>> {
        let i = self.index.remove(key)?;
        let removed_key = self.keys.swap_remove(i);
        let value = self.values.swap_remove(i);
        if let Some(moved) = self.keys.get(i) {
            *self.index.get_mut(moved).expect("índice do StringArrayHashMap coerente") = i;
        }
        Some(KV { key: removed_key, value })
    }

    pub fn keys(&self) -> &[Box<[u8]>] {
        &self.keys
    }

    pub fn values(&self) -> &[V] {
        &self.values
    }

    pub fn values_mut(&mut self) -> &mut [V] {
        &mut self.values
    }

    /// As entradas na ordem do mapa.
    pub fn iterator(&self) -> impl Iterator<Item = (&[u8], &V)> {
        self.keys.iter().map(|k| &**k).zip(self.values.iter())
    }

    pub fn clear(&mut self) {
        self.keys.clear();
        self.values.clear();
        self.index.clear();
    }

    fn push_new(&mut self, key: &[u8], value: V) -> usize {
        let i = self.keys.len();
        self.keys.push(Box::from(key));
        self.values.push(value);
        self.index.insert(Box::from(key), i);
        i
    }
}

impl<V: Default> StringArrayHashMap<V> {
    /// See `ArrayHashMap::get_or_put`: a entrada nova nasce com `V::default()`.
    pub fn get_or_put(&mut self, key: &[u8]) -> Result<GetOrPutResult<'_, Box<[u8]>, V>, AllocError> {
        let (found_existing, index) = match self.get_index(key) {
            Some(i) => (true, i),
            None => (false, self.push_new(key, V::default())),
        };
        Ok(GetOrPutResult {
            found_existing,
            index,
            key_ptr: &mut self.keys[index],
            value_ptr: &mut self.values[index],
        })
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
        assert_eq!(map.insert(b"b", 20), Some(2));
        assert!(map.swap_remove(b"a"));
        // O último ("d") ocupa a posição removida, como no Zig.
        let order: Vec<(&[u8], i32)> = map.iterator().map(|(k, v)| (k, *v)).collect();
        assert_eq!(order, [(&b"d"[..], 4), (b"b", 20), (b"c", 3)]);
        assert_eq!(map.get(b"d"), Some(&4));
        assert_eq!(map.get_index(b"c"), Some(2));
        let gop = map.get_or_put(b"e").unwrap();
        assert!(!gop.found_existing);
        *gop.value_ptr = 5;
        assert_eq!(map.count(), 4);
        assert!(map.get_or_put(b"e").unwrap().found_existing);
    }
}
