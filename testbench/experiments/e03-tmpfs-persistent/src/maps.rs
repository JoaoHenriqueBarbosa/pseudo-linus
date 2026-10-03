//! Abstrações de mapa persistente e as implementações candidatas.
//!
//! - [`IntMap`]: tabela de inodes (`ino -> V`), via [`TableFamily`] porque o valor depende do flavor.
//! - [`NameMap`]: diretório (`nome -> ino`).
//!
//! Toda implementação tem semântica de valor: `clone()` é o snapshot e as operações `&mut self`
//! copiam só o que está compartilhado (caminho na árvore, ou o mapa inteiro no caso do `Arc<BTreeMap>`).

use std::collections::BTreeMap;
use std::hash::RandomState;
use std::marker::PhantomData;
use std::sync::Arc;

use archery::SharedPointerKind;

use crate::content::Content;
use crate::radix::RadixMap;

pub type Ino = u64;
pub type Name = Arc<[u8]>;

pub trait IntMap<V>: Clone + Default + Send + Sync + 'static {
    fn get(&self, key: u64) -> Option<&V>;
    fn get_mut(&mut self, key: u64) -> Option<&mut V>;
    fn insert(&mut self, key: u64, value: V);
    fn remove(&mut self, key: u64) -> bool;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn for_each(&self, f: &mut dyn FnMut(u64, &V));
}

/// Família de mapas pra tabela de inodes (o tipo do valor é decidido pelo flavor).
pub trait TableFamily: Send + Sync + 'static {
    const LABEL: &'static str;
    type Map<V: Clone + Send + Sync + 'static>: IntMap<V>;
}

pub trait NameMap: Clone + Default + Send + Sync + 'static {
    const LABEL: &'static str;
    fn get(&self, name: &[u8]) -> Option<Ino>;
    fn insert(&mut self, name: Name, ino: Ino);
    fn remove(&mut self, name: &[u8]) -> bool;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn for_each(&self, f: &mut dyn FnMut(&Name, Ino));
}

/// Um candidato completo: estrutura da tabela, dos diretórios e do conteúdo.
pub trait Flavor: Send + Sync + Sized + 'static {
    type Table: TableFamily;
    type Dir: NameMap;
    type Content: Content;

    fn label() -> String {
        format!(
            "tabela {} / diretório {} / conteúdo {}",
            <Self::Table as TableFamily>::LABEL,
            <Self::Dir as NameMap>::LABEL,
            <Self::Content as Content>::LABEL
        )
    }
}

/// Marcador de tipo sem posse (o flavor nunca guarda valores das peças).
type Pieces<T, D, C> = fn() -> (T, D, C);

/// Flavor montado a partir das três peças.
pub struct Stack<T, D, C>(PhantomData<Pieces<T, D, C>>);

impl<T: TableFamily, D: NameMap, C: Content> Flavor for Stack<T, D, C> {
    type Table = T;
    type Dir = D;
    type Content = C;
}

// ---------------------------------------------------------------------------------------------
// imbl 7: HashMap (HAMT) e OrdMap (B-tree), com o ponteiro escolhido via archery (ArcK = std Arc,
// ArcTK = triomphe::Arc, sem contador fraco).

impl<V, P> IntMap<V> for imbl::GenericHashMap<u64, V, RandomState, P>
where
    V: Clone + Send + Sync + 'static,
    P: SharedPointerKind + Send + Sync + 'static,
{
    fn get(&self, key: u64) -> Option<&V> {
        imbl::GenericHashMap::get(self, &key)
    }
    fn get_mut(&mut self, key: u64) -> Option<&mut V> {
        imbl::GenericHashMap::get_mut(self, &key)
    }
    fn insert(&mut self, key: u64, value: V) {
        imbl::GenericHashMap::insert(self, key, value);
    }
    fn remove(&mut self, key: u64) -> bool {
        imbl::GenericHashMap::remove(self, &key).is_some()
    }
    fn len(&self) -> usize {
        imbl::GenericHashMap::len(self)
    }
    fn for_each(&self, f: &mut dyn FnMut(u64, &V)) {
        for (k, v) in self.iter() {
            f(*k, v);
        }
    }
}

impl<P> NameMap for imbl::GenericHashMap<Name, Ino, RandomState, P>
where
    P: SharedPointerKind + Send + Sync + 'static + PointerLabel,
{
    const LABEL: &'static str = P::HASH_LABEL;
    fn get(&self, name: &[u8]) -> Option<Ino> {
        imbl::GenericHashMap::get(self, name).copied()
    }
    fn insert(&mut self, name: Name, ino: Ino) {
        imbl::GenericHashMap::insert(self, name, ino);
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        imbl::GenericHashMap::remove(self, name).is_some()
    }
    fn len(&self) -> usize {
        imbl::GenericHashMap::len(self)
    }
    fn for_each(&self, f: &mut dyn FnMut(&Name, Ino)) {
        for (k, v) in self.iter() {
            f(k, *v);
        }
    }
}

impl<V, P> IntMap<V> for imbl::GenericOrdMap<u64, V, P>
where
    V: Clone + Send + Sync + 'static,
    P: SharedPointerKind + Send + Sync + 'static,
{
    fn get(&self, key: u64) -> Option<&V> {
        imbl::GenericOrdMap::get(self, &key)
    }
    fn get_mut(&mut self, key: u64) -> Option<&mut V> {
        imbl::GenericOrdMap::get_mut(self, &key)
    }
    fn insert(&mut self, key: u64, value: V) {
        imbl::GenericOrdMap::insert(self, key, value);
    }
    fn remove(&mut self, key: u64) -> bool {
        imbl::GenericOrdMap::remove(self, &key).is_some()
    }
    fn len(&self) -> usize {
        imbl::GenericOrdMap::len(self)
    }
    fn for_each(&self, f: &mut dyn FnMut(u64, &V)) {
        for (k, v) in self.iter() {
            f(*k, v);
        }
    }
}

impl<P> NameMap for imbl::GenericOrdMap<Name, Ino, P>
where
    P: SharedPointerKind + Send + Sync + 'static + PointerLabel,
{
    const LABEL: &'static str = P::ORD_LABEL;
    fn get(&self, name: &[u8]) -> Option<Ino> {
        imbl::GenericOrdMap::get(self, name).copied()
    }
    fn insert(&mut self, name: Name, ino: Ino) {
        imbl::GenericOrdMap::insert(self, name, ino);
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        imbl::GenericOrdMap::remove(self, name).is_some()
    }
    fn len(&self) -> usize {
        imbl::GenericOrdMap::len(self)
    }
    fn for_each(&self, f: &mut dyn FnMut(&Name, Ino)) {
        for (k, v) in self.iter() {
            f(k, *v);
        }
    }
}

/// Rótulos legíveis por tipo de ponteiro do archery.
pub trait PointerLabel {
    const HASH_LABEL: &'static str;
    const ORD_LABEL: &'static str;
    const TABLE_HASH_LABEL: &'static str;
    const TABLE_ORD_LABEL: &'static str;
    const VECTOR_LABEL: &'static str;
}

impl PointerLabel for archery::ArcK {
    const HASH_LABEL: &'static str = "imbl::HashMap<Arc>";
    const ORD_LABEL: &'static str = "imbl::OrdMap<Arc>";
    const TABLE_HASH_LABEL: &'static str = "imbl::HashMap<Arc>";
    const TABLE_ORD_LABEL: &'static str = "imbl::OrdMap<Arc>";
    const VECTOR_LABEL: &'static str = "imbl::Vector<Arc<bloco>>";
}

impl PointerLabel for archery::ArcTK {
    const HASH_LABEL: &'static str = "imbl::HashMap<triomphe>";
    const ORD_LABEL: &'static str = "imbl::OrdMap<triomphe>";
    const TABLE_HASH_LABEL: &'static str = "imbl::HashMap<triomphe>";
    const TABLE_ORD_LABEL: &'static str = "imbl::OrdMap<triomphe>";
    const VECTOR_LABEL: &'static str = "imbl::Vector<triomphe, Arc<bloco>>";
}

pub struct ImblHashTable<P>(PhantomData<P>);
impl<P: SharedPointerKind + Send + Sync + 'static + PointerLabel> TableFamily for ImblHashTable<P> {
    const LABEL: &'static str = P::TABLE_HASH_LABEL;
    type Map<V: Clone + Send + Sync + 'static> = imbl::GenericHashMap<u64, V, RandomState, P>;
}

pub struct ImblOrdTable<P>(PhantomData<P>);
impl<P: SharedPointerKind + Send + Sync + 'static + PointerLabel> TableFamily for ImblOrdTable<P> {
    const LABEL: &'static str = P::TABLE_ORD_LABEL;
    type Map<V: Clone + Send + Sync + 'static> = imbl::GenericOrdMap<u64, V, P>;
}

pub type ImblHashDir<P> = imbl::GenericHashMap<Name, Ino, RandomState, P>;
pub type ImblOrdDir<P> = imbl::GenericOrdMap<Name, Ino, P>;

// ---------------------------------------------------------------------------------------------
// im 15 (o projeto de onde o imbl foi bifurcado; sem manutenção).

impl<V> IntMap<V> for im::HashMap<u64, V>
where
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, key: u64) -> Option<&V> {
        im::HashMap::get(self, &key)
    }
    fn get_mut(&mut self, key: u64) -> Option<&mut V> {
        im::HashMap::get_mut(self, &key)
    }
    fn insert(&mut self, key: u64, value: V) {
        im::HashMap::insert(self, key, value);
    }
    fn remove(&mut self, key: u64) -> bool {
        im::HashMap::remove(self, &key).is_some()
    }
    fn len(&self) -> usize {
        im::HashMap::len(self)
    }
    fn for_each(&self, f: &mut dyn FnMut(u64, &V)) {
        for (k, v) in self.iter() {
            f(*k, v);
        }
    }
}

impl NameMap for im::HashMap<Name, Ino> {
    const LABEL: &'static str = "im::HashMap";
    fn get(&self, name: &[u8]) -> Option<Ino> {
        im::HashMap::get(self, name).copied()
    }
    fn insert(&mut self, name: Name, ino: Ino) {
        im::HashMap::insert(self, name, ino);
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        im::HashMap::remove(self, name).is_some()
    }
    fn len(&self) -> usize {
        im::HashMap::len(self)
    }
    fn for_each(&self, f: &mut dyn FnMut(&Name, Ino)) {
        for (k, v) in self.iter() {
            f(k, *v);
        }
    }
}

pub struct ImHashTable;
impl TableFamily for ImHashTable {
    const LABEL: &'static str = "im::HashMap";
    type Map<V: Clone + Send + Sync + 'static> = im::HashMap<u64, V>;
}

pub type ImHashDir = im::HashMap<Name, Ino>;

// ---------------------------------------------------------------------------------------------
// rpds 1: HashTrieMapSync (HAMT) e RedBlackTreeMapSync, ambos com triomphe::Arc.

impl<V> IntMap<V> for rpds::HashTrieMapSync<u64, V>
where
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, key: u64) -> Option<&V> {
        rpds::HashTrieMap::get(self, &key)
    }
    fn get_mut(&mut self, key: u64) -> Option<&mut V> {
        // O get_mut do rpds copia o caminho mesmo quando a chave não existe; o get antes evita isso.
        rpds::HashTrieMap::get(self, &key)?;
        rpds::HashTrieMap::get_mut(self, &key)
    }
    fn insert(&mut self, key: u64, value: V) {
        self.insert_mut(key, value);
    }
    fn remove(&mut self, key: u64) -> bool {
        self.remove_mut(&key)
    }
    fn len(&self) -> usize {
        self.size()
    }
    fn for_each(&self, f: &mut dyn FnMut(u64, &V)) {
        for (k, v) in self.iter() {
            f(*k, v);
        }
    }
}

impl NameMap for rpds::HashTrieMapSync<Name, Ino> {
    const LABEL: &'static str = "rpds::HashTrieMapSync";
    fn get(&self, name: &[u8]) -> Option<Ino> {
        rpds::HashTrieMap::get(self, name).copied()
    }
    fn insert(&mut self, name: Name, ino: Ino) {
        self.insert_mut(name, ino);
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        self.remove_mut(name)
    }
    fn len(&self) -> usize {
        self.size()
    }
    fn for_each(&self, f: &mut dyn FnMut(&Name, Ino)) {
        for (k, v) in self.iter() {
            f(k, *v);
        }
    }
}

impl<V> IntMap<V> for rpds::RedBlackTreeMapSync<u64, V>
where
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, key: u64) -> Option<&V> {
        rpds::RedBlackTreeMap::get(self, &key)
    }
    fn get_mut(&mut self, key: u64) -> Option<&mut V> {
        rpds::RedBlackTreeMap::get(self, &key)?;
        rpds::RedBlackTreeMap::get_mut(self, &key)
    }
    fn insert(&mut self, key: u64, value: V) {
        self.insert_mut(key, value);
    }
    fn remove(&mut self, key: u64) -> bool {
        self.remove_mut(&key)
    }
    fn len(&self) -> usize {
        self.size()
    }
    fn for_each(&self, f: &mut dyn FnMut(u64, &V)) {
        for (k, v) in self.iter() {
            f(*k, v);
        }
    }
}

impl NameMap for rpds::RedBlackTreeMapSync<Name, Ino> {
    const LABEL: &'static str = "rpds::RedBlackTreeMapSync";
    fn get(&self, name: &[u8]) -> Option<Ino> {
        rpds::RedBlackTreeMap::get(self, name).copied()
    }
    fn insert(&mut self, name: Name, ino: Ino) {
        self.insert_mut(name, ino);
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        self.remove_mut(name)
    }
    fn len(&self) -> usize {
        self.size()
    }
    fn for_each(&self, f: &mut dyn FnMut(&Name, Ino)) {
        for (k, v) in self.iter() {
            f(k, *v);
        }
    }
}

pub struct RpdsHashTable;
impl TableFamily for RpdsHashTable {
    const LABEL: &'static str = "rpds::HashTrieMapSync";
    type Map<V: Clone + Send + Sync + 'static> = rpds::HashTrieMapSync<u64, V>;
}

pub struct RpdsRbtTable;
impl TableFamily for RpdsRbtTable {
    const LABEL: &'static str = "rpds::RedBlackTreeMapSync";
    type Map<V: Clone + Send + Sync + 'static> = rpds::RedBlackTreeMapSync<u64, V>;
}

pub type RpdsHashDir = rpds::HashTrieMapSync<Name, Ino>;
pub type RpdsRbtDir = rpds::RedBlackTreeMapSync<Name, Ino>;

// ---------------------------------------------------------------------------------------------
// immutable-chunkmap 2: árvore AVL de blocos ordenados (até 512 entradas por bloco no MapM).

impl<V> IntMap<V> for immutable_chunkmap::map::MapM<u64, V>
where
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, key: u64) -> Option<&V> {
        immutable_chunkmap::map::Map::get(self, &key)
    }
    fn get_mut(&mut self, key: u64) -> Option<&mut V> {
        immutable_chunkmap::map::Map::get(self, &key)?;
        self.get_mut_cow(&key)
    }
    fn insert(&mut self, key: u64, value: V) {
        self.insert_cow(key, value);
    }
    fn remove(&mut self, key: u64) -> bool {
        self.remove_cow(&key).is_some()
    }
    fn len(&self) -> usize {
        immutable_chunkmap::map::Map::len(self)
    }
    fn for_each(&self, f: &mut dyn FnMut(u64, &V)) {
        for (k, v) in self {
            f(*k, v);
        }
    }
}

impl NameMap for immutable_chunkmap::map::MapM<Name, Ino> {
    const LABEL: &'static str = "immutable_chunkmap::MapM";
    fn get(&self, name: &[u8]) -> Option<Ino> {
        immutable_chunkmap::map::Map::get(self, name).copied()
    }
    fn insert(&mut self, name: Name, ino: Ino) {
        self.insert_cow(name, ino);
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        // `remove_cow` exige chave `Sized`; a chave guardada é reaproveitada (só um incremento de Arc).
        let Some(key) = self.get_key(name).cloned() else {
            return false;
        };
        self.remove_cow(&key).is_some()
    }
    fn len(&self) -> usize {
        immutable_chunkmap::map::Map::len(self)
    }
    fn for_each(&self, f: &mut dyn FnMut(&Name, Ino)) {
        for (k, v) in self {
            f(k, *v);
        }
    }
}

pub struct ChunkTable;
impl TableFamily for ChunkTable {
    const LABEL: &'static str = "immutable_chunkmap::MapM";
    type Map<V: Clone + Send + Sync + 'static> = immutable_chunkmap::map::MapM<u64, V>;
}

pub type ChunkDir = immutable_chunkmap::map::MapM<Name, Ino>;

// ---------------------------------------------------------------------------------------------
// À mão: trie de raiz 64 pra tabela, e `Arc<BTreeMap>` com `Arc::make_mut` (cópia do mapa inteiro
// na primeira escrita depois do snapshot).

impl<V> IntMap<V> for RadixMap<V>
where
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, key: u64) -> Option<&V> {
        RadixMap::get(self, key)
    }
    fn get_mut(&mut self, key: u64) -> Option<&mut V> {
        RadixMap::get_mut(self, key)
    }
    fn insert(&mut self, key: u64, value: V) {
        RadixMap::insert(self, key, value);
    }
    fn remove(&mut self, key: u64) -> bool {
        RadixMap::remove(self, key).is_some()
    }
    fn len(&self) -> usize {
        RadixMap::len(self)
    }
    fn for_each(&self, f: &mut dyn FnMut(u64, &V)) {
        RadixMap::for_each(self, f)
    }
}

pub struct RadixTable;
impl TableFamily for RadixTable {
    const LABEL: &'static str = "trie de raiz 64 à mão";
    type Map<V: Clone + Send + Sync + 'static> = RadixMap<V>;
}

/// `Arc<BTreeMap>` com `Arc::make_mut`: o "Arc à mão" mais direto.
pub struct ArcBTree<K, V>(Arc<BTreeMap<K, V>>);

impl<K, V> Clone for ArcBTree<K, V> {
    fn clone(&self) -> Self {
        ArcBTree(Arc::clone(&self.0))
    }
}

impl<K, V> Default for ArcBTree<K, V> {
    fn default() -> Self {
        ArcBTree(Arc::new(BTreeMap::new()))
    }
}

impl<V> IntMap<V> for ArcBTree<u64, V>
where
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, key: u64) -> Option<&V> {
        self.0.get(&key)
    }
    fn get_mut(&mut self, key: u64) -> Option<&mut V> {
        if !self.0.contains_key(&key) {
            return None;
        }
        Arc::make_mut(&mut self.0).get_mut(&key)
    }
    fn insert(&mut self, key: u64, value: V) {
        Arc::make_mut(&mut self.0).insert(key, value);
    }
    fn remove(&mut self, key: u64) -> bool {
        if !self.0.contains_key(&key) {
            return false;
        }
        Arc::make_mut(&mut self.0).remove(&key).is_some()
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn for_each(&self, f: &mut dyn FnMut(u64, &V)) {
        for (k, v) in self.0.iter() {
            f(*k, v);
        }
    }
}

impl NameMap for ArcBTree<Name, Ino> {
    const LABEL: &'static str = "Arc<BTreeMap> à mão";
    fn get(&self, name: &[u8]) -> Option<Ino> {
        self.0.get(name).copied()
    }
    fn insert(&mut self, name: Name, ino: Ino) {
        Arc::make_mut(&mut self.0).insert(name, ino);
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        if !self.0.contains_key(name) {
            return false;
        }
        Arc::make_mut(&mut self.0).remove(name).is_some()
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn for_each(&self, f: &mut dyn FnMut(&Name, Ino)) {
        for (k, v) in self.0.iter() {
            f(k, *v);
        }
    }
}

/// Tabela inteira num `Arc<BTreeMap>`: controle negativo (snapshot O(1), mas a primeira escrita
/// depois dele copia a tabela toda).
pub struct FlatTable;
impl TableFamily for FlatTable {
    const LABEL: &'static str = "Arc<BTreeMap> inteiro à mão";
    type Map<V: Clone + Send + Sync + 'static> = ArcBTree<u64, V>;
}

pub type BTreeDir = ArcBTree<Name, Ino>;
