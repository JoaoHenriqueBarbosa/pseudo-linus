// Mesclado das partes traduzidas de hash_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Elemento da tabela hash.
///
/// Cada elemento é um nó numa lista duplamente encadeada.
/// Todos os elementos estão numa única lista encadeada global.
pub struct HashElem {
    /// Próximo e anterior elementos na tabela
    pub next: Option<HashElemRef>,
    pub prev: Option<Weak<RefCell<HashElem>>>,
    /// Dados associados ao elemento
    pub data: Box<dyn std::any::Any>,
    /// Chave associada ao elemento
    pub p_key: Vec<u8>,
}

/// Tipo apelido para referência compartilhada a um HashElem
pub type HashElemRef = Rc<RefCell<HashElem>>;

/// Bucket da tabela hash interna
pub struct HtBucket {
    /// Número de entradas com este hash
    pub count: u32,
    /// Ponteiro para o primeiro elemento com este hash
    pub chain: Option<HashElemRef>,
}

/// Tabela hash genérica do SQLite.
///
/// A tabela completa é uma instância da seguinte estrutura.
/// Os internos desta estrutura devem ser opacos (cliente não deve acessar campos diretamente).
///
/// Todos os elementos da tabela hash estão numa única lista duplamente encadeada.
/// Hash.first aponta para o início desta lista.
///
/// Há Hash.ht_size buckets. Cada bucket aponta para um ponto na lista duplamente encadeada global.
/// Os conteúdos do bucket são o elemento apontado mais os próximos count-1 elementos da lista.
///
/// Hash.ht_size e Hash.ht podem ser zero. Neste caso, a busca é feita de forma linear na lista global.
/// Para tabelas pequenas, a tabela Hash.ht nunca é alocada porque é mais rápido fazer busca linear
/// que gerenciar a tabela hash.
pub struct Hash {
    /// Número de buckets na tabela hash
    pub ht_size: u32,
    /// Número de entradas nesta tabela
    pub count: u32,
    /// O primeiro elemento do array
    pub first: Option<HashElemRef>,
    /// A tabela hash
    pub ht: Option<Vec<HtBucket>>,
}

/// Retorna o primeiro elemento da tabela hash.
#[inline]
pub fn hash_first(h: &Hash) -> Option<HashElemRef> {
    h.first.clone()
}

/// Retorna o próximo elemento na iteração.
#[inline]
pub fn hash_next(e: &HashElem) -> Option<HashElemRef> {
    e.next.clone()
}

/// Retorna os dados associados ao elemento.
#[inline]
pub fn hash_data(e: &HashElem) -> &Box<dyn std::any::Any> {
    &e.data
}

/// Retorna o número de entradas na tabela hash.
#[inline]
pub fn hash_count(h: &Hash) -> u32 {
    h.count
}

