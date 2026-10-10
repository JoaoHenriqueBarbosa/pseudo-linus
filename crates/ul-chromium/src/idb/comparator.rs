//! O comparador `idb_cmp1` do leveldb do IndexedDB (`Compare` em `indexed_db_leveldb_coding.cc`): as chaves se
//! ordenam pelo prefixo (banco, repositório, índice) em número e depois pelo que o tipo do prefixo guarda, com as
//! chaves do IndexedDB comparadas decodificadas (número, data, texto, binário, array), não pelos bytes.
//!
//! Chave que não decodifica cai no bytewise, o que mantém uma ordem total para dado inválido (o Chromium faz o mesmo
//! com `ok = false` mais um `DCHECK`).

use std::cmp::Ordering;

use ul_leveldb::comparator::Comparator;

const MAX_SIMPLE_GLOBAL_TYPE: u8 = 7;
const SCOPES_PREFIX: u8 = 50;
const DATABASE_FREE_LIST: u8 = 100;
const DATABASE_NAME: u8 = 201;

const MAX_SIMPLE_DATABASE_TYPE: u8 = 6;
const OBJECT_STORE_META: u8 = 50;
const INDEX_META: u8 = 100;
const OBJECT_STORE_FREE_LIST: u8 = 150;
const INDEX_FREE_LIST: u8 = 151;
const OBJECT_STORE_NAMES: u8 = 200;
const INDEX_NAMES: u8 = 201;

const MINIMUM_INDEX_ID: u64 = 30;

#[derive(Debug)]
pub struct IdbComparator;

/// Uma fatia que se consome da frente, como o `StringPiece` do Chromium.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn byte(&mut self) -> Option<u8> {
        let (first, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(*first)
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        if self.0.len() < length {
            return None;
        }
        let (head, rest) = self.0.split_at(length);
        self.0 = rest;
        Some(head)
    }

    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    /// O texto com o tamanho em unidades UTF-16 na frente; os bytes ficam em UTF-16BE, que se ordena como memcmp.
    fn string_with_length(&mut self) -> Option<&'a [u8]> {
        let length = self.varint()? as usize;
        self.take(length.checked_mul(2)?)
    }

    fn double(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
}

struct Prefix {
    database: u64,
    store: u64,
    index: u64,
}

fn little_endian(bytes: &[u8]) -> u64 {
    bytes.iter().rev().fold(0, |value, byte| (value << 8) | u64::from(*byte))
}

fn decode_prefix(cursor: &mut Cursor) -> Option<Prefix> {
    let first = cursor.byte()?;
    let widths = [((first >> 5) & 7) as usize + 1, ((first >> 2) & 7) as usize + 1, (first & 3) as usize + 1];
    let database = little_endian(cursor.take(widths[0])?);
    let store = little_endian(cursor.take(widths[1])?);
    let index = little_endian(cursor.take(widths[2])?);
    Some(Prefix { database, store, index })
}

/// A posição do tipo na ordem das chaves (`CompareTypes`): número, data, texto, binário, array.
fn type_rank(type_byte: u8) -> Option<u8> {
    Some(match type_byte {
        0 | 5 => 0,
        3 => 1,
        2 => 2,
        1 => 3,
        6 => 4,
        4 => 5,
        _ => return None,
    })
}

fn compare_idb_keys(a: &mut Cursor, b: &mut Cursor) -> Option<Ordering> {
    let (type_a, type_b) = (a.byte()?, b.byte()?);
    let ordering = type_rank(type_a)?.cmp(&type_rank(type_b)?);
    if ordering != Ordering::Equal {
        return Some(ordering);
    }
    Some(match type_a {
        0 | 5 => Ordering::Equal,
        4 => {
            let (length_a, length_b) = (a.varint()?, b.varint()?);
            for _ in 0..length_a.min(length_b) {
                let item = compare_idb_keys(a, b)?;
                if item != Ordering::Equal {
                    return Some(item);
                }
            }
            length_a.cmp(&length_b)
        }
        6 => {
            let (size_a, size_b) = (a.varint()? as usize, b.varint()? as usize);
            a.take(size_a)?.cmp(b.take(size_b)?)
        }
        1 => a.string_with_length()?.cmp(b.string_with_length()?),
        2 | 3 => a.double()?.partial_cmp(&b.double()?)?,
        _ => return None,
    })
}

fn compare_remaining(a: &mut Cursor, b: &mut Cursor) -> Ordering {
    a.0.len().cmp(&b.0.len())
}

/// O corpo de uma chave de dados: a chave IDB (e, no índice, o número de sequência e a chave primária).
fn compare_data(prefix_index: u64, a: &mut Cursor, b: &mut Cursor) -> Option<Ordering> {
    if a.0.is_empty() || b.0.is_empty() {
        return Some(compare_remaining(a, b));
    }
    let keys = compare_idb_keys(a, b)?;
    if keys != Ordering::Equal || prefix_index < MINIMUM_INDEX_ID {
        return Some(keys);
    }
    let sequence = a.varint()?.cmp(&b.varint()?);
    if sequence != Ordering::Equal {
        return Some(sequence);
    }
    Some(match (a.0.is_empty(), b.0.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => compare_idb_keys(a, b)?,
    })
}

fn compare_global(a: &mut Cursor, b: &mut Cursor) -> Option<Ordering> {
    let (type_a, type_b) = (a.byte()?, b.byte()?);
    if type_a != type_b {
        return Some(type_a.cmp(&type_b));
    }
    if type_a < MAX_SIMPLE_GLOBAL_TYPE {
        return Some(Ordering::Equal);
    }
    Some(match type_a {
        SCOPES_PREFIX => a.0.cmp(b.0),
        DATABASE_FREE_LIST => a.varint()?.cmp(&b.varint()?),
        DATABASE_NAME => {
            let origin = a.string_with_length()?.cmp(b.string_with_length()?);
            if origin != Ordering::Equal {
                return Some(origin);
            }
            a.string_with_length()?.cmp(b.string_with_length()?)
        }
        _ => return None,
    })
}

fn compare_database(a: &mut Cursor, b: &mut Cursor) -> Option<Ordering> {
    let (type_a, type_b) = (a.byte()?, b.byte()?);
    if type_a != type_b {
        return Some(type_a.cmp(&type_b));
    }
    if type_a < MAX_SIMPLE_DATABASE_TYPE {
        return Some(Ordering::Equal);
    }
    let mut varints = |count: usize| -> Option<Ordering> {
        for _ in 0..count {
            let order = a.varint()?.cmp(&b.varint()?);
            if order != Ordering::Equal {
                return Some(order);
            }
        }
        Some(Ordering::Equal)
    };
    Some(match type_a {
        OBJECT_STORE_META => {
            let ids = varints(1)?;
            if ids != Ordering::Equal {
                return Some(ids);
            }
            a.byte()?.cmp(&b.byte()?)
        }
        INDEX_META => {
            let ids = varints(2)?;
            if ids != Ordering::Equal {
                return Some(ids);
            }
            a.byte()?.cmp(&b.byte()?)
        }
        OBJECT_STORE_FREE_LIST => varints(1)?,
        INDEX_FREE_LIST => varints(2)?,
        OBJECT_STORE_NAMES => a.string_with_length()?.cmp(b.string_with_length()?),
        INDEX_NAMES => {
            let store = varints(1)?;
            if store != Ordering::Equal {
                return Some(store);
            }
            a.string_with_length()?.cmp(b.string_with_length()?)
        }
        _ => return None,
    })
}

fn compare_decoded(a: &[u8], b: &[u8]) -> Option<Ordering> {
    let (mut cursor_a, mut cursor_b) = (Cursor(a), Cursor(b));
    let (prefix_a, prefix_b) = (decode_prefix(&mut cursor_a)?, decode_prefix(&mut cursor_b)?);
    let prefixes = (prefix_a.database, prefix_a.store, prefix_a.index).cmp(&(prefix_b.database, prefix_b.store, prefix_b.index));
    if prefixes != Ordering::Equal {
        return Some(prefixes);
    }
    if prefix_a.database == 0 {
        return compare_global(&mut cursor_a, &mut cursor_b);
    }
    if prefix_a.store == 0 {
        return compare_database(&mut cursor_a, &mut cursor_b);
    }
    match prefix_a.index {
        1..=3 => compare_data(0, &mut cursor_a, &mut cursor_b),
        index if index >= MINIMUM_INDEX_ID => compare_data(index, &mut cursor_a, &mut cursor_b),
        _ => None,
    }
}

impl Comparator for IdbComparator {
    fn name(&self) -> &'static [u8] {
        b"idb_cmp1"
    }

    fn compare(&self, a: &[u8], b: &[u8]) -> Ordering {
        compare_decoded(a, b).unwrap_or_else(|| a.cmp(b))
    }

    /// O `IndexedDBComparator` não encurta separadores nem sucessores.
    fn find_shortest_separator(&self, _start: &mut Vec<u8>, _limit: &[u8]) {}

    fn find_short_successor(&self, _key: &mut Vec<u8>) {}
}

#[cfg(test)]
mod tests {
    use super::super::coding::{index_data_key, object_store_data_key, units, IdbKey};
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
        digits.chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ascii"), 16).expect("hex")).collect()
    }

    fn order(a: &[u8], b: &[u8]) -> Ordering {
        IdbComparator.compare(a, b)
    }

    /// Lote seq=251 do oráculo: as entradas do índice 31 saem por a, b, bb, c (a ordem de texto), e não pela de bytes,
    /// que poria `bb` depois de `c` (o tamanho vem antes do texto).
    #[test]
    fn index_entries_follow_the_key_order_not_the_byte_order() {
        let one = IdbKey::Number(1.0);
        let entry = |text: &str| index_data_key(1, 1, 31, &IdbKey::String(units(text)), 0, &one);
        let (a, b, bb, c) = (entry("a"), entry("b"), entry("bb"), entry("c"));
        assert_eq!([&a, &b, &bb, &c].windows(2).map(|pair| order(pair[0], pair[1])).collect::<Vec<_>>(), vec![Ordering::Less; 3]);
        assert_eq!(c.cmp(&bb), Ordering::Less, "no bytewise o tamanho vem antes do texto: `c` (1) antes de `bb` (2)");
        assert_eq!(order(&c, &bb), Ordering::Greater);
    }

    #[test]
    fn numbers_sort_by_value_and_types_by_rank() {
        let key = |value: IdbKey| object_store_data_key(1, 1, &value);
        assert_eq!(order(&key(IdbKey::Number(-1.0)), &key(IdbKey::Number(1.0))), Ordering::Less);
        assert_eq!(order(&key(IdbKey::Number(2.0)), &key(IdbKey::Number(10.0))), Ordering::Less);
        assert_eq!(order(&key(IdbKey::Number(1e300)), &key(IdbKey::Date(0.0))), Ordering::Less);
        assert_eq!(order(&key(IdbKey::Date(5.0)), &key(IdbKey::String(units("")))), Ordering::Less);
        assert_eq!(order(&key(IdbKey::String(units("z"))), &key(IdbKey::Binary(vec![]))), Ordering::Less);
        assert_eq!(order(&key(IdbKey::Binary(vec![9])), &key(IdbKey::Array(vec![]))), Ordering::Less);
        let array = |items: Vec<IdbKey>| key(IdbKey::Array(items));
        assert_eq!(order(&array(vec![IdbKey::Number(1.0)]), &array(vec![IdbKey::Number(1.0), IdbKey::Number(0.0)])), Ordering::Less);
    }

    #[test]
    fn prefixes_compare_as_numbers_and_metadata_groups_by_type() {
        assert_eq!(order(&hex("00 01 00 00 04"), &hex("00 02 00 00 03")), Ordering::Less);
        assert_eq!(order(&hex("00 01 01 01 03 00 00 00 00 00 00 f0 3f"), &hex("00 01 01 02 03 00 00 00 00 00 00 f0 3f")), Ordering::Less);
        // Global: o tipo decide (0 versão do esquema, 3 diário, 50 escopos, 201 nome do banco).
        assert_eq!(order(&hex("00 00 00 00 00"), &hex("00 00 00 00 02")), Ordering::Less);
        assert_eq!(order(&hex("00 00 00 00 03"), &hex("00 00 00 00 32 01 00")), Ordering::Less);
        // Escopos: comparação de bytes do que vem depois do tipo.
        assert_eq!(order(&hex("00 00 00 00 32 02 00 00 7f ff ff ff ff ff ff f0"), &hex("00 00 00 00 32 02 00 00 7f ff ff ff ff ff ff f1")), Ordering::Less);
        // Metadados de repositório: o id do repositório antes do campo (50, id, campo).
        assert_eq!(order(&hex("00 01 00 00 32 01 07"), &hex("00 01 00 00 32 02 00")), Ordering::Less);
        assert_eq!(order(&hex("00 01 00 00 32 01 00"), &hex("00 01 00 00 32 01 01")), Ordering::Less);
        // Nome de repositório: texto em ordem de unidades UTF-16, não pelo tamanho que vem na frente.
        let name = |text: &str| {
            let mut key = hex("00 01 00 00 c8");
            super::super::coding::string_with_length(&mut key, &units(text));
            key
        };
        assert_eq!(order(&name("b"), &name("ab")), Ordering::Greater);
        assert_eq!(order(&name("a"), &name("ab")), Ordering::Less);
    }

    /// O nome que vai para o MANIFEST.
    #[test]
    fn the_name_is_the_one_the_chromium_writes() {
        assert_eq!(IdbComparator.name(), b"idb_cmp1");
    }
}
