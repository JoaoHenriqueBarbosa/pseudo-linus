//! O comparador `idb_cmp1` do leveldb do IndexedDB (`Compare` em `indexed_db_leveldb_coding.cc`): as chaves se
//! ordenam pelo prefixo (banco, repositório, índice) em número e depois pelo que o tipo do prefixo guarda, com as
//! chaves do IndexedDB comparadas decodificadas (número, data, texto, binário, array), não pelos bytes.
//!
//! Chave que não decodifica cai no bytewise, o que mantém uma ordem total para dado inválido (o Chromium faz o mesmo
//! com `ok = false` mais um `DCHECK`).

use std::cmp::Ordering;

use ul_leveldb::coding::Input;
use ul_leveldb::comparator::Comparator;

use super::coding::{decode_int, read_string_bytes};

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

/// Uma fatia que se consome da frente, como o `StringPiece` do Chromium (o `Input` do leveldb, sem os erros).
struct Cursor<'a>(Input<'a>);

impl<'a> Cursor<'a> {
    fn byte(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        self.0.take(length).ok()
    }

    fn varint(&mut self) -> Option<u64> {
        self.0.varint64().ok()
    }

    fn rest(&self) -> &'a [u8] {
        self.0.remaining()
    }

    fn string_with_length(&mut self) -> Option<&'a [u8]> {
        read_string_bytes(&mut self.0)
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

fn decode_prefix(cursor: &mut Cursor) -> Option<Prefix> {
    let first = cursor.byte()?;
    let widths = [((first >> 5) & 7) as usize + 1, ((first >> 2) & 7) as usize + 1, (first & 3) as usize + 1];
    let database = decode_int(cursor.take(widths[0])?);
    let store = decode_int(cursor.take(widths[1])?);
    let index = decode_int(cursor.take(widths[2])?);
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

/// As duas chaves lado a lado: cada leitura consome o mesmo campo de ambas e devolve a ordem entre elas.
struct Pair<'a> {
    a: Cursor<'a>,
    b: Cursor<'a>,
}

impl Pair<'_> {
    fn bytes(&mut self) -> Option<(u8, u8)> {
        Some((self.a.byte()?, self.b.byte()?))
    }

    fn varint(&mut self) -> Option<(u64, u64)> {
        Some((self.a.varint()?, self.b.varint()?))
    }

    /// `count` inteiros variáveis seguidos, na ordem lexicográfica.
    fn varints(&mut self, count: usize) -> Option<Ordering> {
        for _ in 0..count {
            let (a, b) = self.varint()?;
            if a != b {
                return Some(a.cmp(&b));
            }
        }
        Some(Ordering::Equal)
    }

    fn byte(&mut self) -> Option<Ordering> {
        let (a, b) = self.bytes()?;
        Some(a.cmp(&b))
    }

    fn string(&mut self) -> Option<Ordering> {
        Some(self.a.string_with_length()?.cmp(self.b.string_with_length()?))
    }

    fn rest(&self) -> Ordering {
        self.a.rest().cmp(self.b.rest())
    }

    fn field(&mut self, field: Field) -> Option<Ordering> {
        match field {
            Field::Varints(count) => self.varints(count),
            Field::Byte => self.byte(),
            Field::String => self.string(),
            Field::Rest => Some(self.rest()),
        }
    }

    fn rest_is_empty(&self) -> (bool, bool) {
        (self.a.rest().is_empty(), self.b.rest().is_empty())
    }
}

/// O que decide o campo seguinte: se a ordem já saiu do que foi lido, ela vale e o resto nem é lido (como o Chromium
/// faz, e uma chave malformada depois do ponto que decide não derruba a comparação); senão, `next`.
fn chain(first: Option<Ordering>, next: impl FnOnce() -> Option<Ordering>) -> Option<Ordering> {
    match first? {
        Ordering::Equal => next(),
        decided => Some(decided),
    }
}

fn compare_idb_keys(pair: &mut Pair) -> Option<Ordering> {
    let (type_a, type_b) = pair.bytes()?;
    let ordering = type_rank(type_a)?.cmp(&type_rank(type_b)?);
    if ordering != Ordering::Equal {
        return Some(ordering);
    }
    match type_a {
        0 | 5 => Some(Ordering::Equal),
        4 => {
            let (length_a, length_b) = pair.varint()?;
            for _ in 0..length_a.min(length_b) {
                let item = compare_idb_keys(pair)?;
                if item != Ordering::Equal {
                    return Some(item);
                }
            }
            Some(length_a.cmp(&length_b))
        }
        6 => {
            let (size_a, size_b) = pair.varint()?;
            Some(pair.a.take(size_a as usize)?.cmp(pair.b.take(size_b as usize)?))
        }
        1 => pair.string(),
        2 | 3 => pair.a.double()?.partial_cmp(&pair.b.double()?),
        _ => None,
    }
}

/// O corpo de uma chave de dados: a chave IDB (e, no índice, o número de sequência e a chave primária).
fn compare_data(prefix_index: u64, pair: &mut Pair) -> Option<Ordering> {
    let (empty_a, empty_b) = pair.rest_is_empty();
    if empty_a || empty_b {
        return Some(pair.a.rest().len().cmp(&pair.b.rest().len()));
    }
    let keys = compare_idb_keys(pair)?;
    if keys != Ordering::Equal || prefix_index < MINIMUM_INDEX_ID {
        return Some(keys);
    }
    chain(pair.varints(1), || {
        Some(match pair.rest_is_empty() {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => compare_idb_keys(pair)?,
        })
    })
}

/// Um campo do que segue o tipo de uma chave de metadados.
#[derive(Clone, Copy)]
enum Field {
    /// Esta quantidade de inteiros variáveis, na ordem lexicográfica.
    Varints(usize),
    Byte,
    /// Texto com o tamanho na frente (comparado por unidade UTF-16, não pelo tamanho).
    String,
    /// Tudo o que sobra, em bytes.
    Rest,
}

/// Um grupo de chaves que abre com um tipo (as globais e as de banco): tipos diferentes decidem pela ordem entre eles,
/// os abaixo de `simple_below` não têm mais nada (iguais quando o tipo é o mesmo) e os demais têm os campos de
/// `layouts`, comparados em ordem até um decidir. Tipo que não consta nem é simples não decodifica.
struct Section {
    simple_below: u8,
    layouts: &'static [(u8, &'static [Field])],
}

const GLOBAL: Section = Section {
    simple_below: MAX_SIMPLE_GLOBAL_TYPE,
    layouts: &[(SCOPES_PREFIX, &[Field::Rest]), (DATABASE_FREE_LIST, &[Field::Varints(1)]), (DATABASE_NAME, &[Field::String, Field::String])],
};

const DATABASE: Section = Section {
    simple_below: MAX_SIMPLE_DATABASE_TYPE,
    layouts: &[
        (OBJECT_STORE_META, &[Field::Varints(1), Field::Byte]),
        (INDEX_META, &[Field::Varints(2), Field::Byte]),
        (OBJECT_STORE_FREE_LIST, &[Field::Varints(1)]),
        (INDEX_FREE_LIST, &[Field::Varints(2)]),
        (OBJECT_STORE_NAMES, &[Field::String]),
        (INDEX_NAMES, &[Field::Varints(1), Field::String]),
    ],
};

fn compare_section(pair: &mut Pair, section: &Section) -> Option<Ordering> {
    let (type_a, type_b) = pair.bytes()?;
    if type_a != type_b {
        return Some(type_a.cmp(&type_b));
    }
    if type_a < section.simple_below {
        return Some(Ordering::Equal);
    }
    let (_, fields) = section.layouts.iter().find(|(kind, _)| *kind == type_a)?;
    for field in *fields {
        let order = pair.field(*field)?;
        if order != Ordering::Equal {
            return Some(order);
        }
    }
    Some(Ordering::Equal)
}

fn compare_decoded(a: &[u8], b: &[u8]) -> Option<Ordering> {
    let mut pair = Pair { a: Cursor(Input::new(a)), b: Cursor(Input::new(b)) };
    let (prefix_a, prefix_b) = (decode_prefix(&mut pair.a)?, decode_prefix(&mut pair.b)?);
    let prefixes = (prefix_a.database, prefix_a.store, prefix_a.index).cmp(&(prefix_b.database, prefix_b.store, prefix_b.index));
    if prefixes != Ordering::Equal {
        return Some(prefixes);
    }
    if prefix_a.database == 0 {
        return compare_section(&mut pair, &GLOBAL);
    }
    if prefix_a.store == 0 {
        return compare_section(&mut pair, &DATABASE);
    }
    match prefix_a.index {
        1..=3 => compare_data(0, &mut pair),
        index if index >= MINIMUM_INDEX_ID => compare_data(index, &mut pair),
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
    use super::super::coding::{index_data_key, record_index, record_key, units, IdbKey};
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
        let key = |value: IdbKey| record_key(1, 1, record_index::OBJECT_STORE_DATA, &value);
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
