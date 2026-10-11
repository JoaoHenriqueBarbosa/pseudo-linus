//! A memtable (`db/memtable.cc`, `db/skiplist.h`, `util/arena.cc`). Os dados ficam numa `BTreeMap` ordenada pela chave
//! interna; o que o leveldb tem de observável aqui é QUANDO a memtable passa do `write_buffer_size` (o arquivo `.ldb`
//! aparece, o log é trocado), e isso depende do `ApproximateMemoryUsage` do arena, que cresce com o tamanho de cada
//! entrada, o alinhamento dos nós da skip list e a altura sorteada de cada nó. Este módulo reproduz essa contabilidade
//! byte a byte: o mesmo sorteio (`Random(0xdeadbeef)`, ramificação 4, altura máxima 12) e o mesmo arena.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::coding::{put_fixed64, varint_length};
use crate::dbformat::{compare_internal, pack_sequence_and_type, ValueType, MAX_SEQUENCE_NUMBER, VALUE_TYPE_FOR_SEEK};

const ARENA_BLOCK_SIZE: usize = 4096;
const POINTER_SIZE: usize = 8;
const MAX_HEIGHT: usize = 12;
const BRANCHING: u32 = 4;
/// `sizeof(Node)` da skip list: o ponteiro da chave e o primeiro `next_`.
const NODE_BASE_SIZE: usize = 16;

/// O gerador do leveldb (`util/random.h`): Lehmer de 31 bits.
#[derive(Debug)]
struct Random {
    seed: u32,
}

impl Random {
    fn new(seed: u32) -> Random {
        let seed = seed & 0x7fff_ffff;
        Random { seed: if seed == 0 || seed == 2_147_483_647 { 1 } else { seed } }
    }

    fn next(&mut self) -> u32 {
        const M: u64 = 2_147_483_647;
        const A: u64 = 16_807;
        let product = u64::from(self.seed) * A;
        let mut seed = (product >> 31) + (product & M);
        if seed > M {
            seed -= M;
        }
        self.seed = seed as u32;
        self.seed
    }

    fn one_in(&mut self, n: u32) -> bool {
        self.next() % n == 0
    }
}

/// A contabilidade do `Arena`: quantos bytes ele pediu ao sistema.
#[derive(Debug)]
struct Arena {
    /// Bytes que sobram no bloco corrente e quantos já foram usados nele.
    remaining: usize,
    used: usize,
    usage: usize,
}

impl Arena {
    fn new() -> Arena {
        Arena { remaining: 0, used: 0, usage: 0 }
    }

    fn allocate(&mut self, bytes: usize) {
        if bytes <= self.remaining {
            self.used += bytes;
            self.remaining -= bytes;
        } else {
            self.fallback(bytes);
        }
    }

    fn allocate_aligned(&mut self, bytes: usize) {
        let current_mod = self.used & 7;
        let slop = if current_mod == 0 { 0 } else { 8 - current_mod };
        let needed = bytes + slop;
        if needed <= self.remaining {
            self.used += needed;
            self.remaining -= needed;
        } else {
            self.fallback(bytes);
        }
    }

    fn fallback(&mut self, bytes: usize) {
        if bytes > ARENA_BLOCK_SIZE / 4 {
            // Um objeto grande ganha o bloco só para ele; o bloco corrente não muda.
            self.usage += bytes + POINTER_SIZE;
            return;
        }
        self.usage += ARENA_BLOCK_SIZE + POINTER_SIZE;
        self.used = bytes;
        self.remaining = ARENA_BLOCK_SIZE - bytes;
    }
}

/// A chave de uma entrada da memtable, ordenada como o `InternalKeyComparator`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key(Vec<u8>);

impl Ord for Key {
    fn cmp(&self, other: &Key) -> Ordering {
        compare_internal(&self.0, &other.0)
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Key) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// O resultado de procurar uma chave na memtable.
#[derive(Debug, PartialEq, Eq)]
pub enum Lookup {
    Found(Vec<u8>),
    Deleted,
    Missing,
}

#[derive(Debug)]
pub struct MemTable {
    entries: BTreeMap<Key, Vec<u8>>,
    arena: Arena,
    random: Random,
}

impl MemTable {
    pub fn new() -> MemTable {
        let mut arena = Arena::new();
        // O nó cabeça da skip list, de altura máxima.
        arena.allocate_aligned(NODE_BASE_SIZE + POINTER_SIZE * (MAX_HEIGHT - 1));
        MemTable { entries: BTreeMap::new(), arena, random: Random::new(0xdead_beef) }
    }

    pub fn approximate_memory_usage(&self) -> usize {
        self.arena.usage
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn random_height(&mut self) -> usize {
        let mut height = 1;
        while height < MAX_HEIGHT && self.random.one_in(BRANCHING) {
            height += 1;
        }
        height
    }

    pub fn add(&mut self, sequence: u64, value_type: ValueType, user_key: &[u8], value: &[u8]) {
        let mut internal = Vec::with_capacity(user_key.len() + 8);
        internal.extend_from_slice(user_key);
        put_fixed64(&mut internal, pack_sequence_and_type(sequence, value_type));
        let encoded_len = varint_length(internal.len() as u64) + internal.len() + varint_length(value.len() as u64) + value.len();
        self.arena.allocate(encoded_len);
        let height = self.random_height();
        self.arena.allocate_aligned(NODE_BASE_SIZE + POINTER_SIZE * (height - 1));
        self.entries.insert(Key(internal), value.to_vec());
    }

    /// `MemTable::Get`: a entrada mais nova de `user_key` com sequência até `sequence`.
    pub fn get(&self, user_key: &[u8], sequence: u64) -> Lookup {
        let mut seek = Vec::with_capacity(user_key.len() + 8);
        seek.extend_from_slice(user_key);
        put_fixed64(&mut seek, pack_sequence_and_type(sequence.min(MAX_SEQUENCE_NUMBER), VALUE_TYPE_FOR_SEEK));
        match self.entries.range(Key(seek)..).next() {
            Some((key, value)) if crate::dbformat::user_key(&key.0) == user_key => match crate::dbformat::parse_internal_key(&key.0) {
                Some((_, _, ValueType::Value)) => Lookup::Found(value.clone()),
                Some((_, _, ValueType::Deletion)) => Lookup::Deleted,
                None => Lookup::Missing,
            },
            _ => Lookup::Missing,
        }
    }

    /// As entradas em ordem de chave interna (chave interna, valor).
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.entries.iter().map(|(key, value)| (key.0.as_slice(), value.as_slice()))
    }
}

impl Default for MemTable {
    fn default() -> MemTable {
        MemTable::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_random_generator_is_the_leveldb_one() {
        // Os primeiros valores de `Random(0xdeadbeef)`: 16807 * seed mod (2^31 - 1).
        let mut random = Random::new(0xdead_beef);
        let seed = 0xdead_beef_u64 & 0x7fff_ffff;
        assert_eq!(u64::from(random.next()), (seed * 16807) % 2_147_483_647);
    }

    #[test]
    fn an_empty_memtable_has_only_the_head_node_block() {
        assert_eq!(MemTable::new().approximate_memory_usage(), ARENA_BLOCK_SIZE + POINTER_SIZE);
    }

    #[test]
    fn entries_that_fit_the_current_block_cost_nothing_and_large_ones_get_their_own() {
        let mut table = MemTable::new();
        let before = table.approximate_memory_usage();
        // 1 + 11 + 2 + 2000 = 2014 bytes cabem nos 3992 que sobram do primeiro bloco: o arena não pede memória nova.
        table.add(1, ValueType::Value, b"key", &vec![b'v'; 2000]);
        assert_eq!(table.approximate_memory_usage(), before);
        // 1 + 11 + 2 + 5000 = 5014 bytes não cabem no que sobra e passam de 1024: ganham um bloco exato, mais o
        // ponteiro do bloco.
        table.add(2, ValueType::Value, b"key", &vec![b'v'; 5000]);
        assert_eq!(table.approximate_memory_usage(), before + 5014 + POINTER_SIZE);
    }

    #[test]
    fn get_sees_the_newest_entry_at_or_before_the_sequence() {
        let mut table = MemTable::new();
        table.add(1, ValueType::Value, b"k", b"one");
        table.add(2, ValueType::Value, b"k", b"two");
        table.add(3, ValueType::Deletion, b"k", b"");
        assert_eq!(table.get(b"k", 1), Lookup::Found(b"one".to_vec()));
        assert_eq!(table.get(b"k", 2), Lookup::Found(b"two".to_vec()));
        assert_eq!(table.get(b"k", 3), Lookup::Deleted);
        assert_eq!(table.get(b"other", 3), Lookup::Missing);
        let keys: Vec<u64> = table.iter().filter_map(|(key, _)| crate::dbformat::parse_internal_key(key).map(|(_, sequence, _)| sequence)).collect();
        assert_eq!(keys, vec![3, 2, 1]);
    }
}
