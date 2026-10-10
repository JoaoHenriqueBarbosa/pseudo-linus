//! O `VersionEdit` do leveldb (`db/version_edit.cc`): cada registro do `MANIFEST` é uma mudança de versão, codificada
//! como uma sequência de campos `tag varint` + valor, na ordem em que o `EncodeTo` os escreve.

use crate::coding::{put_length_prefixed, put_varint32, put_varint64, Input};
use crate::{Error, Result};

const COMPARATOR: u32 = 1;
const LOG_NUMBER: u32 = 2;
const NEXT_FILE_NUMBER: u32 = 3;
const LAST_SEQUENCE: u32 = 4;
const COMPACT_POINTER: u32 = 5;
const DELETED_FILE: u32 = 6;
const NEW_FILE: u32 = 7;
const PREV_LOG_NUMBER: u32 = 9;

/// Uma tabela (`.ldb`) que a versão passa a ter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub number: u64,
    pub file_size: u64,
    /// Menor e maior chave interna (chave do usuário + 8 bytes de sequência e tipo).
    pub smallest: Vec<u8>,
    pub largest: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionEdit {
    pub comparator: Option<Vec<u8>>,
    pub log_number: Option<u64>,
    pub prev_log_number: Option<u64>,
    pub next_file_number: Option<u64>,
    pub last_sequence: Option<u64>,
    pub compact_pointers: Vec<(u32, Vec<u8>)>,
    pub deleted_files: Vec<(u32, u64)>,
    pub new_files: Vec<(u32, FileMeta)>,
}

impl VersionEdit {
    /// O registro do `MANIFEST`, com os campos na ordem do `EncodeTo` do leveldb.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(name) = &self.comparator {
            put_varint32(&mut out, COMPARATOR);
            put_length_prefixed(&mut out, name);
        }
        if let Some(number) = self.log_number {
            put_varint32(&mut out, LOG_NUMBER);
            put_varint64(&mut out, number);
        }
        if let Some(number) = self.prev_log_number {
            put_varint32(&mut out, PREV_LOG_NUMBER);
            put_varint64(&mut out, number);
        }
        if let Some(number) = self.next_file_number {
            put_varint32(&mut out, NEXT_FILE_NUMBER);
            put_varint64(&mut out, number);
        }
        if let Some(sequence) = self.last_sequence {
            put_varint32(&mut out, LAST_SEQUENCE);
            put_varint64(&mut out, sequence);
        }
        for (level, key) in &self.compact_pointers {
            put_varint32(&mut out, COMPACT_POINTER);
            put_varint32(&mut out, *level);
            put_length_prefixed(&mut out, key);
        }
        // As tabelas apagadas são um `std::set<pair<int, uint64>>` no leveldb: saem ordenadas por (nível, número).
        let mut deleted = self.deleted_files.clone();
        deleted.sort_unstable();
        deleted.dedup();
        for (level, number) in &deleted {
            put_varint32(&mut out, DELETED_FILE);
            put_varint32(&mut out, *level);
            put_varint64(&mut out, *number);
        }
        for (level, file) in &self.new_files {
            put_varint32(&mut out, NEW_FILE);
            put_varint32(&mut out, *level);
            put_varint64(&mut out, file.number);
            put_varint64(&mut out, file.file_size);
            put_length_prefixed(&mut out, &file.smallest);
            put_length_prefixed(&mut out, &file.largest);
        }
        out
    }

    pub fn decode(data: &[u8]) -> Result<VersionEdit> {
        let mut edit = VersionEdit::default();
        let mut input = Input::new(data);
        while !input.is_empty() {
            match input.varint32()? {
                COMPARATOR => edit.comparator = Some(input.length_prefixed()?.to_vec()),
                LOG_NUMBER => edit.log_number = Some(input.varint64()?),
                PREV_LOG_NUMBER => edit.prev_log_number = Some(input.varint64()?),
                NEXT_FILE_NUMBER => edit.next_file_number = Some(input.varint64()?),
                LAST_SEQUENCE => edit.last_sequence = Some(input.varint64()?),
                COMPACT_POINTER => {
                    let level = input.varint32()?;
                    edit.compact_pointers.push((level, input.length_prefixed()?.to_vec()));
                }
                DELETED_FILE => {
                    let level = input.varint32()?;
                    edit.deleted_files.push((level, input.varint64()?));
                }
                NEW_FILE => {
                    let level = input.varint32()?;
                    let number = input.varint64()?;
                    let file_size = input.varint64()?;
                    let smallest = input.length_prefixed()?.to_vec();
                    let largest = input.length_prefixed()?.to_vec();
                    edit.new_files.push((level, FileMeta { number, file_size, smallest, largest }));
                }
                tag => return Err(Error::Corruption(format!("tag {tag} desconhecida no VersionEdit"))),
            }
        }
        Ok(edit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O primeiro registro do `MANIFEST-000001` de um banco novo (o do Local Storage do oráculo).
    #[test]
    fn the_new_database_edit_matches_the_oracle() {
        let edit = VersionEdit { comparator: Some(b"leveldb.BytewiseComparator".to_vec()), log_number: Some(0), next_file_number: Some(2), last_sequence: Some(0), ..VersionEdit::default() };
        let mut expected = vec![1u8, 26];
        expected.extend_from_slice(b"leveldb.BytewiseComparator");
        expected.extend_from_slice(&[2, 0, 3, 2, 4, 0]);
        assert_eq!(edit.encode(), expected);
        assert_eq!(VersionEdit::decode(&expected).unwrap(), edit);
    }

    #[test]
    fn files_and_pointers_round_trip() {
        let edit = VersionEdit {
            log_number: Some(9),
            prev_log_number: Some(0),
            next_file_number: Some(12),
            last_sequence: Some(4000),
            compact_pointers: vec![(1, b"abc\x01\0\0\0\0\0\0\0".to_vec())],
            deleted_files: vec![(0, 5), (1, 6)],
            new_files: vec![(0, FileMeta { number: 11, file_size: 70_000, smallest: b"a".to_vec(), largest: b"z".to_vec() })],
            ..VersionEdit::default()
        };
        assert_eq!(VersionEdit::decode(&edit.encode()).unwrap(), edit);
    }

    #[test]
    fn an_unknown_tag_is_corruption() {
        assert!(VersionEdit::decode(&[8, 1]).is_err());
    }
}
