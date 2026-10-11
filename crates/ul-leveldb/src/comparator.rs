//! O comparador de chaves de usuário (`leveldb::Comparator`): quem decide a ordem das chaves no banco, no `MANIFEST`
//! (o nome) e nas tabelas (os separadores). O Local Storage usa o bytewise; o IndexedDB do Chromium usa o `idb_cmp1`,
//! que ordena pela chave decodificada, não pelos bytes (ver `ul-chromium::idb::comparator`).
//!
//! As funções de ordenação do leveldb (`compare_internal`, os separadores das tabelas, as faixas das tabelas)
//! consultam o comparador do banco que está executando: cada chamada pública do [`Db`](crate::Db) abre um
//! [`Scope`] com o dele, que o restaura ao sair. Fora de um escopo vale o bytewise, o que mantém os testes dos módulos
//! de baixo nível como estavam.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::fmt::Debug;
use std::sync::Arc;

pub trait Comparator: Send + Sync + Debug {
    /// O nome gravado no `MANIFEST`; abrir um banco com outro nome é corrupção.
    fn name(&self) -> &'static [u8];
    fn compare(&self, a: &[u8], b: &[u8]) -> Ordering;
    /// `FindShortestSeparator`: encurta `start` para algo em `[start, limit)`, ou o deixa como está.
    fn find_shortest_separator(&self, start: &mut Vec<u8>, limit: &[u8]);
    /// `FindShortSuccessor`: a menor chave maior ou igual a `key` que o comparador sabe fazer, ou `key` mesma.
    fn find_short_successor(&self, key: &mut Vec<u8>);
}

#[derive(Debug)]
pub struct Bytewise;

impl Comparator for Bytewise {
    fn name(&self) -> &'static [u8] {
        b"leveldb.BytewiseComparator"
    }

    fn compare(&self, a: &[u8], b: &[u8]) -> Ordering {
        a.cmp(b)
    }

    fn find_shortest_separator(&self, start: &mut Vec<u8>, limit: &[u8]) {
        let min_length = start.len().min(limit.len());
        let mut diff_index = 0;
        while diff_index < min_length && start[diff_index] == limit[diff_index] {
            diff_index += 1;
        }
        if diff_index >= min_length {
            return;
        }
        let diff_byte = start[diff_index];
        if diff_byte < 0xff && diff_byte + 1 < limit[diff_index] {
            start[diff_index] += 1;
            start.truncate(diff_index + 1);
        }
    }

    fn find_short_successor(&self, key: &mut Vec<u8>) {
        for index in 0..key.len() {
            if key[index] != 0xff {
                key[index] += 1;
                key.truncate(index + 1);
                return;
            }
        }
    }
}

pub type Shared = Arc<dyn Comparator>;

pub fn bytewise() -> Shared {
    Arc::new(Bytewise)
}

thread_local! {
    static CURRENT: RefCell<Shared> = RefCell::new(bytewise());
}

/// O comparador vigente volta ao anterior quando o escopo termina.
pub struct Scope(Shared);

impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = self.0.clone());
    }
}

/// Faz de `comparator` o vigente até o escopo devolvido sair de cena.
pub fn scoped(comparator: &Shared) -> Scope {
    Scope(CURRENT.with(|current| current.replace(comparator.clone())))
}

fn current() -> Shared {
    CURRENT.with(|current| current.borrow().clone())
}

pub fn name() -> &'static [u8] {
    current().name()
}

pub fn compare(a: &[u8], b: &[u8]) -> Ordering {
    current().compare(a, b)
}

pub fn find_shortest_separator(start: &mut Vec<u8>, limit: &[u8]) {
    current().find_shortest_separator(start, limit);
}

pub fn find_short_successor(key: &mut Vec<u8>) {
    current().find_short_successor(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Reverse;

    impl Comparator for Reverse {
        fn name(&self) -> &'static [u8] {
            b"reverse"
        }
        fn compare(&self, a: &[u8], b: &[u8]) -> Ordering {
            b.cmp(a)
        }
        fn find_shortest_separator(&self, _: &mut Vec<u8>, _: &[u8]) {}
        fn find_short_successor(&self, _: &mut Vec<u8>) {}
    }

    #[test]
    fn the_scope_swaps_the_comparator_and_restores_it() {
        assert_eq!(compare(b"a", b"b"), Ordering::Less);
        {
            let _scope = scoped(&(Arc::new(Reverse) as Shared));
            assert_eq!(compare(b"a", b"b"), Ordering::Greater);
            assert_eq!(name(), b"reverse");
            {
                let _inner = scoped(&bytewise());
                assert_eq!(compare(b"a", b"b"), Ordering::Less);
            }
            assert_eq!(compare(b"a", b"b"), Ordering::Greater);
        }
        assert_eq!(compare(b"a", b"b"), Ordering::Less);
        assert_eq!(name(), b"leveldb.BytewiseComparator");
    }
}
