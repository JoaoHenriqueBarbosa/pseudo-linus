//! O formato das chaves e dos nomes de arquivo (`db/dbformat.h`, `db/filename.cc`).

use std::cmp::Ordering;

use crate::coding::put_fixed64;
use crate::comparator;

pub const NUM_LEVELS: usize = 7;
pub const L0_COMPACTION_TRIGGER: usize = 4;
pub const L0_SLOWDOWN_WRITES_TRIGGER: usize = 8;
pub const L0_STOP_WRITES_TRIGGER: usize = 12;
pub const MAX_MEM_COMPACT_LEVEL: usize = 2;

pub const MAX_SEQUENCE_NUMBER: u64 = (1 << 56) - 1;

/// O tipo de um valor na chave interna.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Deletion = 0,
    Value = 1,
}

/// O tipo usado ao procurar por uma sequência (o maior, para cair antes de todas as entradas da chave).
pub const VALUE_TYPE_FOR_SEEK: ValueType = ValueType::Value;

pub fn pack_sequence_and_type(sequence: u64, value_type: ValueType) -> u64 {
    (sequence << 8) | value_type as u64
}

/// A chave interna: a chave do usuário e 8 bytes com a sequência e o tipo.
pub fn internal_key(user_key: &[u8], sequence: u64, value_type: ValueType) -> Vec<u8> {
    let mut key = Vec::with_capacity(user_key.len() + 8);
    key.extend_from_slice(user_key);
    put_fixed64(&mut key, pack_sequence_and_type(sequence, value_type));
    key
}

pub fn user_key(internal_key: &[u8]) -> &[u8] {
    &internal_key[..internal_key.len().saturating_sub(8)]
}

/// Sequência e tipo de uma chave interna (`None` se ela tem menos de 8 bytes ou o tipo é desconhecido).
pub fn parse_internal_key(internal_key: &[u8]) -> Option<(&[u8], u64, ValueType)> {
    let split = internal_key.len().checked_sub(8)?;
    let packed = u64::from_le_bytes(internal_key[split..].try_into().ok()?);
    let value_type = match packed & 0xff {
        0 => ValueType::Deletion,
        1 => ValueType::Value,
        _ => return None,
    };
    Some((&internal_key[..split], packed >> 8, value_type))
}

/// A ordem do `InternalKeyComparator` com o comparador de usuário vigente (ver `comparator`): chave do usuário crescente, e para a mesma
/// chave a sequência decrescente (a mais nova primeiro).
pub fn compare_internal(a: &[u8], b: &[u8]) -> Ordering {
    match comparator::compare(user_key(a), user_key(b)) {
        Ordering::Equal => {
            let pack = |key: &[u8]| u64::from_le_bytes(key[key.len() - 8..].try_into().unwrap_or([0; 8]));
            pack(b).cmp(&pack(a))
        }
        other => other,
    }
}

/// `InternalKeyComparator::FindShortestSeparator`.
pub fn internal_shortest_separator(start: &mut Vec<u8>, limit: &[u8]) {
    let mut tmp = user_key(start).to_vec();
    comparator::find_shortest_separator(&mut tmp, user_key(limit));
    if tmp.len() < user_key(start).len() && comparator::compare(user_key(start), &tmp) == Ordering::Less {
        put_fixed64(&mut tmp, pack_sequence_and_type(MAX_SEQUENCE_NUMBER, VALUE_TYPE_FOR_SEEK));
        *start = tmp;
    }
}

/// `InternalKeyComparator::FindShortSuccessor`.
pub fn internal_short_successor(key: &mut Vec<u8>) {
    let mut tmp = user_key(key).to_vec();
    comparator::find_short_successor(&mut tmp);
    if tmp.len() < user_key(key).len() && comparator::compare(user_key(key), &tmp) == Ordering::Less {
        put_fixed64(&mut tmp, pack_sequence_and_type(MAX_SEQUENCE_NUMBER, VALUE_TYPE_FOR_SEEK));
        *key = tmp;
    }
}

/// Os tipos de arquivo de um banco, na numeração do `FileType` do leveldb (usada na linha `Delete type=N` do `LOG`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Log = 0,
    DbLock = 1,
    Table = 2,
    Descriptor = 3,
    Current = 4,
    Temp = 5,
    InfoLog = 6,
}

pub fn log_file_name(dir: &str, number: u64) -> String {
    format!("{dir}/{number:06}.log")
}

pub fn table_file_name(dir: &str, number: u64) -> String {
    format!("{dir}/{number:06}.ldb")
}

pub fn descriptor_file_name(dir: &str, number: u64) -> String {
    format!("{dir}/MANIFEST-{number:06}")
}

pub fn temp_file_name(dir: &str, number: u64) -> String {
    format!("{dir}/{number:06}.dbtmp")
}

pub fn current_file_name(dir: &str) -> String {
    format!("{dir}/CURRENT")
}

pub fn lock_file_name(dir: &str) -> String {
    format!("{dir}/LOCK")
}

pub fn info_log_file_name(dir: &str) -> String {
    format!("{dir}/LOG")
}

pub fn old_info_log_file_name(dir: &str) -> String {
    format!("{dir}/LOG.old")
}

/// O número e o tipo de um nome de arquivo do banco (`ParseFileName`).
pub fn parse_file_name(name: &str) -> Option<(u64, FileType)> {
    match name {
        "CURRENT" => return Some((0, FileType::Current)),
        "LOCK" => return Some((0, FileType::DbLock)),
        "LOG" | "LOG.old" => return Some((0, FileType::InfoLog)),
        _ => {}
    }
    if let Some(rest) = name.strip_prefix("MANIFEST-") {
        return rest.parse::<u64>().ok().filter(|_| rest.bytes().all(|b| b.is_ascii_digit())).map(|n| (n, FileType::Descriptor));
    }
    let (digits, suffix) = name.split_once('.')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number = digits.parse::<u64>().ok()?;
    let file_type = match suffix {
        "log" => FileType::Log,
        "sst" | "ldb" => FileType::Table,
        "dbtmp" => FileType::Temp,
        _ => return None,
    };
    Some((number, file_type))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_sequences_sort_first_for_the_same_user_key() {
        let old = internal_key(b"k", 1, ValueType::Value);
        let new = internal_key(b"k", 2, ValueType::Value);
        let other = internal_key(b"l", 1, ValueType::Value);
        assert_eq!(compare_internal(&new, &old), Ordering::Less);
        assert_eq!(compare_internal(&old, &other), Ordering::Less);
        assert_eq!(parse_internal_key(&new), Some((&b"k"[..], 2, ValueType::Value)));
    }

    #[test]
    fn separators_and_successors_follow_the_bytewise_comparator() {
        let mut start = b"abcdefg".to_vec();
        comparator::find_shortest_separator(&mut start, b"abzzz");
        assert_eq!(start, b"abd");
        let mut same = b"abc".to_vec();
        comparator::find_shortest_separator(&mut same, b"abcd");
        assert_eq!(same, b"abc");
        let mut key = b"\xff\xffa".to_vec();
        comparator::find_short_successor(&mut key);
        assert_eq!(key, b"\xff\xffb");
    }

    #[test]
    fn file_names_parse_like_the_leveldb_ones() {
        assert_eq!(parse_file_name("000003.log"), Some((3, FileType::Log)));
        assert_eq!(parse_file_name("000012.ldb"), Some((12, FileType::Table)));
        assert_eq!(parse_file_name("MANIFEST-000001"), Some((1, FileType::Descriptor)));
        assert_eq!(parse_file_name("CURRENT"), Some((0, FileType::Current)));
        assert_eq!(parse_file_name("LOG.old"), Some((0, FileType::InfoLog)));
        assert_eq!(parse_file_name("junk"), None);
        assert_eq!(table_file_name("/db", 7), "/db/000007.ldb");
    }
}
