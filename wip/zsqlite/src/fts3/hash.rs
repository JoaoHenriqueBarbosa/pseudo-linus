//! `fts3_hash.c` e `fts3_hash.h`: a tabela hash genérica do FTS3 (é a do SQLite, ligeiramente
//! modificada para ser independente). Guarda o registro de tokenizadores e as tabelas de termos
//! pendentes de cada índice.
//!
//! Modelo v2:
//!
//! * `Fts3Hash<T>` guarda o dado `T` por valor no lugar do `void *data`. O `NULL` do C que apaga
//!   uma entrada na inserção é `None` em [`Fts3Hash::insert`]; o `NULL` de "não achou" é `None`.
//! * Os elementos ficam num arena (`Vec<Option<Fts3HashElem<T>>>`) e a lista duplamente
//!   encadeada (`next`, `prev`) e as cadeias dos baldes (`chain`) são índices do arena, com a
//!   mesma ordem de inserção e de iteração do C (que aparece na ordem em que os termos pendentes
//!   são gravados no segmento). [`HashElemId`] é o `Fts3HashElem *` do C.
//! * A chave é sempre copiada (o `copyKey` do C só evita a cópia quando o chamador garante a vida
//!   do ponteiro; em Rust a posse é da tabela e o parâmetro some). O comprimento da chave é o da
//!   fatia (`nKey`).
//! * Falta de memória não existe: `Insert` nunca devolve o dado novo por falha de alocação.

/// `FTS3_HASH_STRING`: a chave é uma cadeia de `nKey` bytes (com o terminador, se houver);
/// maiúsculas e minúsculas são diferentes.
pub const FTS3_HASH_STRING: u8 = 1;
/// `FTS3_HASH_BINARY`: a chave é binária e a comparação é `memcmp`.
pub const FTS3_HASH_BINARY: u8 = 2;

/// O `Fts3HashElem *` do C: índice de um elemento no arena da tabela. Vale até o elemento ser
/// removido ou a tabela ser limpa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashElemId(pub usize);

/// `struct _fts3ht`: um balde.
#[derive(Debug, Clone, Copy, Default)]
struct Fts3Ht {
    /// Número de entradas com este hash.
    count: i32,
    /// A primeira entrada deste balde (índice no arena).
    chain: Option<usize>,
}

/// `Fts3HashElem`.
#[derive(Debug)]
struct Fts3HashElem<T> {
    /// Próximo e anterior na lista única de todos os elementos.
    next: Option<usize>,
    prev: Option<usize>,
    /// O dado associado.
    data: T,
    /// A chave (`pKey`, `nKey` é o tamanho).
    key: Vec<u8>,
}

/// `Fts3Hash`.
#[derive(Debug)]
pub struct Fts3Hash<T> {
    /// `keyClass`: `FTS3_HASH_STRING` ou `FTS3_HASH_BINARY`.
    key_class: u8,
    /// Número de entradas.
    count: i32,
    /// O primeiro elemento da lista.
    first: Option<usize>,
    /// Número de baldes.
    htsize: i32,
    /// Os baldes (vazio enquanto `htsize` é zero).
    ht: Vec<Fts3Ht>,
    /// O arena de elementos.
    elems: Vec<Option<Fts3HashElem<T>>>,
    /// Vagas livres do arena.
    free_slots: Vec<usize>,
}

/// `fts3StrHash`. O C sinaliza `nKey<=0` com `strlen`; aqui o comprimento é o da fatia. O `char` do
/// C tem sinal no x86-64: bytes 0x80 a 0xFF entram no hash estendidos com sinal.
fn str_hash(key: &[u8]) -> i32 {
    let mut h: u32 = 0;
    for &c in key {
        h = (h << 3) ^ h ^ (c as i8 as i32 as u32);
    }
    (h & 0x7fff_ffff) as i32
}

/// `fts3StrCompare`: `strncmp` de `n1` bytes (para no primeiro NUL) depois de conferir os
/// tamanhos.
fn str_compare(key1: &[u8], key2: &[u8]) -> i32 {
    if key1.len() != key2.len() {
        return 1;
    }
    for i in 0..key1.len() {
        let (a, b) = (key1[i], key2[i]);
        if a != b {
            return a as i32 - b as i32;
        }
        if a == 0 {
            return 0;
        }
    }
    0
}

/// `fts3BinHash`: o `h` do C é `int` com sinal e `*(z++)` um `char` com sinal; o deslocamento
/// descarta os bits altos como no x86-64.
fn bin_hash(key: &[u8]) -> i32 {
    let mut h: i32 = 0;
    for &c in key {
        h = (h << 3) ^ h ^ (c as i8 as i32);
    }
    h & 0x7fff_ffff
}

/// `fts3BinCompare`.
fn bin_compare(key1: &[u8], key2: &[u8]) -> i32 {
    if key1.len() != key2.len() {
        return 1;
    }
    match key1.cmp(key2) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `ftsHashFunction`: aplica a função de hash da classe de chave.
fn hash_key(key_class: u8, key: &[u8]) -> i32 {
    if key_class == FTS3_HASH_STRING {
        str_hash(key)
    } else {
        debug_assert!(key_class == FTS3_HASH_BINARY);
        bin_hash(key)
    }
}

/// `ftsCompareFunction`: aplica a comparação da classe de chave.
fn compare_keys(key_class: u8, key1: &[u8], key2: &[u8]) -> i32 {
    if key_class == FTS3_HASH_STRING {
        str_compare(key1, key2)
    } else {
        debug_assert!(key_class == FTS3_HASH_BINARY);
        bin_compare(key1, key2)
    }
}

impl<T> Fts3Hash<T> {
    /// `sqlite3Fts3HashInit`: uma tabela vazia da classe de chave dada.
    pub fn new(key_class: u8) -> Self {
        debug_assert!((FTS3_HASH_STRING..=FTS3_HASH_BINARY).contains(&key_class));
        Fts3Hash {
            key_class,
            count: 0,
            first: None,
            htsize: 0,
            ht: Vec::new(),
            elems: Vec::new(),
            free_slots: Vec::new(),
        }
    }

    /// `fts3HashCount`: número de entradas.
    #[inline]
    pub fn count(&self) -> i32 {
        self.count
    }

    /// `sqlite3Fts3HashClear`: remove todas as entradas e devolve a memória. Os dados `T` são
    /// soltos (no C o chamador os libera antes).
    pub fn clear(&mut self) {
        self.first = None;
        self.ht = Vec::new();
        self.htsize = 0;
        self.elems = Vec::new();
        self.free_slots = Vec::new();
        self.count = 0;
    }

    /// `fts3HashFirst`: o primeiro elemento da lista.
    #[inline]
    pub fn first(&self) -> Option<HashElemId> {
        self.first.map(HashElemId)
    }

    /// `fts3HashNext`: o elemento que segue `e`.
    #[inline]
    pub fn next(&self, e: HashElemId) -> Option<HashElemId> {
        self.elem(e.0).next.map(HashElemId)
    }

    /// `fts3HashData`.
    #[inline]
    pub fn data(&self, e: HashElemId) -> &T {
        &self.elem(e.0).data
    }

    /// `fts3HashData` para alterar o dado no lugar.
    #[inline]
    pub fn data_mut(&mut self, e: HashElemId) -> &mut T {
        &mut self.elem_mut(e.0).data
    }

    /// `fts3HashKey` e `fts3HashKeysize` (o tamanho é o da fatia).
    #[inline]
    pub fn key(&self, e: HashElemId) -> &[u8] {
        &self.elem(e.0).key
    }

    #[inline]
    fn elem(&self, i: usize) -> &Fts3HashElem<T> {
        self.elems[i].as_ref().expect("fts3 hash: elemento livre")
    }

    #[inline]
    fn elem_mut(&mut self, i: usize) -> &mut Fts3HashElem<T> {
        self.elems[i].as_mut().expect("fts3 hash: elemento livre")
    }

    /// `fts3HashInsertElement`: liga o elemento `new` ao balde `h`.
    fn insert_element(&mut self, h: usize, new: usize) {
        let p_head = self.ht[h].chain;
        if let Some(head) = p_head {
            let head_prev = self.elem(head).prev;
            {
                let n = self.elem_mut(new);
                n.next = Some(head);
                n.prev = head_prev;
            }
            if let Some(hp) = head_prev {
                self.elem_mut(hp).next = Some(new);
            } else {
                self.first = Some(new);
            }
            self.elem_mut(head).prev = Some(new);
        } else {
            let first = self.first;
            {
                let n = self.elem_mut(new);
                n.next = first;
                n.prev = None;
            }
            if let Some(f) = first {
                self.elem_mut(f).prev = Some(new);
            }
            self.first = Some(new);
        }
        self.ht[h].count += 1;
        self.ht[h].chain = Some(new);
    }

    /// `fts3Rehash`: redimensiona a tabela para `new_size` baldes (potência de 2).
    fn rehash(&mut self, new_size: i32) {
        debug_assert!(new_size & (new_size - 1) == 0);
        self.ht = vec![Fts3Ht::default(); new_size as usize];
        self.htsize = new_size;
        let mut elem = self.first;
        self.first = None;
        while let Some(e) = elem {
            let h = hash_key(self.key_class, &self.elem(e).key) & (new_size - 1);
            let next_elem = self.elem(e).next;
            self.insert_element(h as usize, e);
            elem = next_elem;
        }
    }

    /// `fts3FindElementByHash`: procura a chave no balde `h` (o hash já mascarado).
    fn find_element_by_hash(&self, key: &[u8], h: i32) -> Option<usize> {
        if self.ht.is_empty() {
            return None;
        }
        let entry = self.ht[h as usize];
        let mut elem = entry.chain;
        let mut count = entry.count;
        while count > 0 {
            count -= 1;
            let Some(e) = elem else {
                break;
            };
            let el = self.elem(e);
            if compare_keys(self.key_class, &el.key, key) == 0 {
                return Some(e);
            }
            elem = el.next;
        }
        None
    }

    /// `fts3RemoveElementByHash`: tira `e` da tabela e devolve o dado.
    fn remove_element_by_hash(&mut self, e: usize, h: i32) -> T {
        let el = self.elems[e].take().expect("fts3 hash: elemento livre");
        self.free_slots.push(e);
        if let Some(p) = el.prev {
            self.elem_mut(p).next = el.next;
        } else {
            self.first = el.next;
        }
        if let Some(n) = el.next {
            self.elem_mut(n).prev = el.prev;
        }
        let entry = &mut self.ht[h as usize];
        if entry.chain == Some(e) {
            entry.chain = el.next;
        }
        entry.count -= 1;
        if entry.count <= 0 {
            entry.chain = None;
        }
        self.count -= 1;
        if self.count <= 0 {
            debug_assert!(self.first.is_none());
            debug_assert!(self.count == 0);
            self.clear();
        }
        el.data
    }

    /// `sqlite3Fts3HashFindElem`.
    pub fn find_elem(&self, key: &[u8]) -> Option<HashElemId> {
        if self.ht.is_empty() {
            return None;
        }
        let h = hash_key(self.key_class, key);
        debug_assert!(self.htsize & (self.htsize - 1) == 0);
        self.find_element_by_hash(key, h & (self.htsize - 1)).map(HashElemId)
    }

    /// `sqlite3Fts3HashFind`: o dado da entrada com a chave dada, se existe.
    pub fn find(&self, key: &[u8]) -> Option<&T> {
        self.find_elem(key).map(|e| self.data(e))
    }

    /// `sqlite3Fts3HashFind` para alterar o dado no lugar.
    pub fn find_mut(&mut self, key: &[u8]) -> Option<&mut T> {
        match self.find_elem(key) {
            Some(e) => Some(self.data_mut(e)),
            None => None,
        }
    }

    /// `sqlite3Fts3HashInsert`: insere `data` na chave. Sem entrada anterior, cria uma e devolve
    /// `None`. Com entrada anterior, o dado novo a substitui e o antigo é devolvido; `data` `None`
    /// remove a entrada (e devolve o dado que ela tinha).
    pub fn insert(&mut self, key: &[u8], data: Option<T>) -> Option<T> {
        let hraw = hash_key(self.key_class, key);
        debug_assert!(self.htsize & (self.htsize - 1) == 0);
        let mut h = hraw & (self.htsize - 1);
        if let Some(e) = self.find_element_by_hash(key, h) {
            return match data {
                None => Some(self.remove_element_by_hash(e, h)),
                Some(d) => Some(std::mem::replace(&mut self.elem_mut(e).data, d)),
            };
        }
        let data = data?;
        if self.htsize == 0 {
            self.rehash(8);
        }
        if self.count >= self.htsize {
            self.rehash(self.htsize * 2);
        }
        debug_assert!(self.htsize > 0);
        let new_elem = Fts3HashElem { next: None, prev: None, data, key: key.to_vec() };
        let new = match self.free_slots.pop() {
            Some(i) => {
                self.elems[i] = Some(new_elem);
                i
            }
            None => {
                self.elems.push(Some(new_elem));
                self.elems.len() - 1
            }
        };
        self.count += 1;
        debug_assert!(self.htsize & (self.htsize - 1) == 0);
        h = hraw & (self.htsize - 1);
        self.insert_element(h as usize, new);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_find_remove() {
        let mut h: Fts3Hash<i32> = Fts3Hash::new(FTS3_HASH_STRING);
        assert!(h.find(b"simple\0").is_none());
        for i in 0..40 {
            let k = format!("term{}\0", i);
            assert!(h.insert(k.as_bytes(), Some(i)).is_none());
        }
        assert_eq!(h.count(), 40);
        assert_eq!(h.find(b"term7\0"), Some(&7));
        assert_eq!(h.insert(b"term7\0", Some(70)), Some(7));
        assert_eq!(h.find(b"term7\0"), Some(&70));
        assert_eq!(h.insert(b"term7\0", None), Some(70));
        assert!(h.find(b"term7\0").is_none());
        assert_eq!(h.count(), 39);
        let mut n = 0;
        let mut e = h.first();
        while let Some(id) = e {
            n += 1;
            e = h.next(id);
        }
        assert_eq!(n, 39);
        for i in 0..40 {
            let k = format!("term{}\0", i);
            h.insert(k.as_bytes(), None);
        }
        assert_eq!(h.count(), 0);
        assert!(h.first().is_none());
    }

    #[test]
    fn string_keys_stop_at_nul() {
        let mut h: Fts3Hash<i32> = Fts3Hash::new(FTS3_HASH_STRING);
        h.insert(b"a\0b", Some(1));
        assert_eq!(h.find(b"a\0b"), Some(&1));
        assert_eq!(str_compare(b"a\0b", b"a\0c"), 0);
        assert_eq!(str_compare(b"ab", b"abc"), 1);
    }
}
