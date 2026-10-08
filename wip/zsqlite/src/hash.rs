//! Tabelas hash genéricas do SQLite (hash.c e hash.h do 3.46.1).
//!
//! Tradução fiel para preservar a ORDEM DE ITERAÇÃO do C, que aparece na saída do SQL
//! (por exemplo na ordem das tabelas do esquema). Todos os elementos ficam numa única lista
//! duplamente encadeada (`first`); cada balde guarda um ponto dessa lista e quantos elementos
//! seguintes pertencem a ele. Os ponteiros viram índices num `Vec` de elementos (slab com lista
//! de livres). O valor `V` é do chamador; a chave é copiada como bytes (o C guarda o ponteiro,
//! e quem chama garante o tempo de vida).
//!
//! Chaves são cadeias C: valem até o primeiro byte 0x00 (se houver).

use crate::consts::SQLITE_MALLOC_SOFT_LIMIT;
use crate::util::str_icmp;

/// Tamanho de `struct _ht` no C (um `unsigned int` com padding mais um ponteiro).
const SIZEOF_HT: usize = 16;

/// `struct _ht`: um balde.
#[derive(Clone, Copy, Default)]
struct Bucket {
    /// Número de entradas com este hash.
    count: u32,
    /// Primeira entrada com este hash.
    chain: Option<usize>,
}

/// `HashElem`: um elemento da tabela. `data` nunca é "nulo": o C nunca guarda dado nulo
/// (inserir nulo remove).
pub struct HashElem<V> {
    next: Option<usize>,
    prev: Option<usize>,
    data: V,
    key: Vec<u8>,
}

/// `Hash`: uma tabela hash completa.
pub struct Hash<V> {
    /// Número de entradas da tabela.
    count: u32,
    /// Primeiro elemento da lista.
    first: Option<usize>,
    /// Os baldes. Vazio equivale a `ht == NULL` e `htsize == 0` no C (busca linear).
    ht: Vec<Bucket>,
    /// Elementos (slab). `None` é posição livre.
    elems: Vec<Option<HashElem<V>>>,
    /// Posições livres de `elems`.
    free: Vec<usize>,
}

impl<V> Default for Hash<V> {
    /// Tabela vazia (`sqlite3HashInit`), para os `#[derive(Default)]` de quem possui um `Hash`.
    fn default() -> Self {
        hash_init()
    }
}

/// `sqlite3HashInit`: uma tabela vazia.
pub fn hash_init<V>() -> Hash<V> {
    Hash { count: 0, first: None, ht: Vec::new(), elems: Vec::new(), free: Vec::new() }
}

/// `sqlite3HashClear`: remove todas as entradas e devolve a memória; deixa a tabela vazia.
pub fn hash_clear<V>(h: &mut Hash<V>) {
    h.first = None;
    h.ht = Vec::new();
    h.elems = Vec::new();
    h.free = Vec::new();
    h.count = 0;
}

/// A chave como cadeia C: corta no primeiro byte 0x00.
#[inline]
fn c_key(key: &[u8]) -> &[u8] {
    match key.iter().position(|&b| b == 0) {
        Some(n) => &key[..n],
        None => key,
    }
}

/// `strHash`: a função de hash (Knuth multiplicativo, Sorting & Searching, p. 510).
/// 0x9e3779b1 é 2654435761, o primo mais próximo de (2**32)*golden_ratio.
fn str_hash(z: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &c in z {
        // sqlite3UpperToLower[c] é a minúscula ASCII (identidade fora de A-Z).
        h = h.wrapping_add(c.to_ascii_lowercase() as u32);
        h = h.wrapping_mul(0x9e3779b1);
    }
    h
}

impl<V> Hash<V> {
    #[inline]
    fn elem(&self, i: usize) -> &HashElem<V> {
        self.elems[i].as_ref().expect("elemento de hash vivo")
    }

    #[inline]
    fn elem_mut(&mut self, i: usize) -> &mut HashElem<V> {
        self.elems[i].as_mut().expect("elemento de hash vivo")
    }

    /// `insertElement`: liga `new` na lista de `self`. Se `entry` for `Some`, também insere
    /// `new` nesse balde.
    fn insert_element(&mut self, entry: Option<usize>, new: usize) {
        let head = match entry {
            Some(e) => {
                let b = &mut self.ht[e];
                let head = if b.count > 0 { b.chain } else { None };
                b.count += 1;
                b.chain = Some(new);
                head
            }
            None => None,
        };
        if let Some(head) = head {
            let head_prev = self.elem(head).prev;
            self.elem_mut(new).next = Some(head);
            self.elem_mut(new).prev = head_prev;
            match head_prev {
                Some(p) => self.elem_mut(p).next = Some(new),
                None => self.first = Some(new),
            }
            self.elem_mut(head).prev = Some(new);
        } else {
            let first = self.first;
            self.elem_mut(new).next = first;
            if let Some(f) = first {
                self.elem_mut(f).prev = Some(new);
            }
            self.elem_mut(new).prev = None;
            self.first = Some(new);
        }
    }

    /// `rehash`: redimensiona para `new_size` baldes. Devolve verdadeiro se redimensionou.
    /// O limite brando de alocação (`SQLITE_MALLOC_SOFT_LIMIT`, 1024 bytes) limita a 64 baldes.
    fn rehash(&mut self, new_size: u32) -> bool {
        let mut new_size = new_size as usize;
        if new_size * SIZEOF_HT > SQLITE_MALLOC_SOFT_LIMIT {
            new_size = SQLITE_MALLOC_SOFT_LIMIT / SIZEOF_HT;
        }
        if new_size == self.ht.len() {
            return false;
        }
        self.ht = vec![Bucket::default(); new_size];
        let mut elem = self.first;
        self.first = None;
        while let Some(e) = elem {
            let h = str_hash(&self.elem(e).key) as usize % new_size;
            elem = self.elem(e).next;
            self.insert_element(Some(h), e);
        }
        true
    }

    /// `findElementWithHash`: localiza o elemento com a chave dada. Devolve o elemento (se
    /// existir) e o hash da chave (0 quando não há baldes).
    fn find_element_with_hash(&self, key: &[u8]) -> (Option<usize>, u32) {
        let (mut elem, mut count, h) = if !self.ht.is_empty() {
            let h = str_hash(key) % self.ht.len() as u32;
            let entry = &self.ht[h as usize];
            (entry.chain, entry.count, h)
        } else {
            (self.first, self.count, 0)
        };
        while count > 0 {
            let e = elem.expect("lista de hash consistente");
            if str_icmp(&self.elem(e).key, key) == 0 {
                return (Some(e), h);
            }
            elem = self.elem(e).next;
            count -= 1;
        }
        (None, h)
    }

    /// `removeElementGivenHash`: remove um elemento, dado o hash da sua chave. Devolve o dado.
    fn remove_element_given_hash(&mut self, elem: usize, h: u32) -> V {
        let (prev, next) = {
            let e = self.elem(elem);
            (e.prev, e.next)
        };
        match prev {
            Some(p) => self.elem_mut(p).next = next,
            None => self.first = next,
        }
        if let Some(n) = next {
            self.elem_mut(n).prev = prev;
        }
        if !self.ht.is_empty() {
            let entry = &mut self.ht[h as usize];
            if entry.chain == Some(elem) {
                entry.chain = next;
            }
            debug_assert!(entry.count > 0);
            entry.count -= 1;
        }
        let removed = self.elems[elem].take().expect("elemento de hash vivo");
        self.free.push(elem);
        self.count -= 1;
        if self.count == 0 {
            debug_assert!(self.first.is_none());
            hash_clear(self);
        }
        removed.data
    }

    /// Guarda um elemento novo no slab e devolve o índice.
    fn alloc_element(&mut self, key: Vec<u8>, data: V) -> usize {
        let elem = HashElem { next: None, prev: None, data, key };
        match self.free.pop() {
            Some(i) => {
                self.elems[i] = Some(elem);
                i
            }
            None => {
                self.elems.push(Some(elem));
                self.elems.len() - 1
            }
        }
    }
}

/// `sqlite3HashFind`: o dado do elemento cuja chave casa (sem diferenciar maiúsculas de
/// minúsculas ASCII), ou `None`.
pub fn hash_find<'a, V>(h: &'a Hash<V>, key: &[u8]) -> Option<&'a V> {
    let key = c_key(key);
    h.find_element_with_hash(key).0.map(|e| &h.elem(e).data)
}

/// Como `sqlite3HashFind`, mas devolve o dado para edição no lugar (o C devolve o ponteiro
/// do dado, que o chamador altera).
pub fn hash_find_mut<'a, V>(h: &'a mut Hash<V>, key: &[u8]) -> Option<&'a mut V> {
    let key = c_key(key);
    match h.find_element_with_hash(key).0 {
        Some(e) => Some(&mut h.elem_mut(e).data),
        None => None,
    }
}

/// `sqlite3HashInsert`: insere um elemento com a chave e o dado dados.
///
/// Se não existe elemento com a chave, cria um e devolve `None`. Se já existe, o dado novo
/// substitui o antigo (e a chave guardada passa a ser a nova) e o dado antigo é devolvido. Se
/// `data` for `None`, o elemento da chave é removido (devolvendo o dado removido).
pub fn hash_insert<V>(h: &mut Hash<V>, key: &[u8], data: Option<V>) -> Option<V> {
    let key = c_key(key);
    let (found, mut hv) = h.find_element_with_hash(key);
    if let Some(elem) = found {
        return match data {
            None => Some(h.remove_element_given_hash(elem, hv)),
            Some(d) => {
                let e = h.elem_mut(elem);
                e.key = key.to_vec();
                Some(std::mem::replace(&mut e.data, d))
            }
        };
    }
    let data = data?;
    let new_elem = h.alloc_element(key.to_vec(), data);
    h.count += 1;
    if h.count >= 10 && h.count > 2 * h.ht.len() as u32 {
        if h.rehash(h.count * 2) {
            debug_assert!(!h.ht.is_empty());
            hv = str_hash(key) % h.ht.len() as u32;
        }
    }
    let entry = if h.ht.is_empty() { None } else { Some(hv as usize) };
    h.insert_element(entry, new_elem);
    None
}

/// `sqliteHashFirst`: o primeiro elemento da lista (ponto de partida da iteração).
#[inline]
pub fn hash_first<V>(h: &Hash<V>) -> Option<usize> {
    h.first
}

/// `sqliteHashNext`: o elemento seguinte na lista.
#[inline]
pub fn hash_next<V>(h: &Hash<V>, elem: usize) -> Option<usize> {
    h.elem(elem).next
}

/// `sqliteHashData`: o dado de um elemento.
#[inline]
pub fn hash_data<V>(h: &Hash<V>, elem: usize) -> &V {
    &h.elem(elem).data
}

/// Como `sqliteHashData`, para edição no lugar.
#[inline]
pub fn hash_data_mut<V>(h: &mut Hash<V>, elem: usize) -> &mut V {
    &mut h.elem_mut(elem).data
}

/// A chave de um elemento (o C tem a macro `sqliteHashKey` comentada como não usada; a
/// iteração do esquema precisa do nome, então fica exposta).
#[inline]
pub fn hash_key<V>(h: &Hash<V>, elem: usize) -> &[u8] {
    &h.elem(elem).key
}

/// `sqliteHashCount`: número de entradas.
#[inline]
pub fn hash_count<V>(h: &Hash<V>) -> u32 {
    h.count
}

/// Iterador na mesma ordem de `for(p=sqliteHashFirst(&h); p; p=sqliteHashNext(p))`. Rende
/// `(chave, dado)`.
pub struct HashIter<'a, V> {
    h: &'a Hash<V>,
    cur: Option<usize>,
}

pub fn hash_iter<V>(h: &Hash<V>) -> HashIter<'_, V> {
    HashIter { h, cur: h.first }
}

impl<'a, V> Iterator for HashIter<'a, V> {
    type Item = (&'a [u8], &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        let e = self.h.elem(self.cur?);
        self.cur = e.next;
        Some((&e.key, &e.data))
    }
}
