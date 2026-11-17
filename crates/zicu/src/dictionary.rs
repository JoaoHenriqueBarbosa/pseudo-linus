//! Os dicionários de palavras (`dictionarydata.cpp`): o formato `.dict` e o casamento de prefixos
//! num `BytesTrie`, com a transformação por deslocamento que põe um script num byte.

use crate::bytes_trie::{BytesTrie, TrieResult};
use crate::utext::Utf16Text;

const TRIE_TYPE_MASK: i32 = 7;
const TRIE_TYPE_BYTES: i32 = 0;
const TRANSFORM_TYPE_OFFSET: i32 = 0x0100_0000;
const TRANSFORM_TYPE_MASK: i32 = 0x7f00_0000;
const TRANSFORM_OFFSET_MASK: i32 = 0x001f_ffff;

/// `BytesDictionaryMatcher`. Os dicionários de trie de `char16_t` (o `cjdict`) não estão
/// traduzidos.
#[derive(Clone, Debug)]
pub struct DictionaryMatcher {
    trie: Vec<u8>,
    transform_constant: i32,
}

/// O resultado de [`DictionaryMatcher::matches`]: o comprimento de cada palavra achada, em
/// unidades e em pontos de código, e o prefixo mais longo casado, em pontos de código.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Matches {
    pub cu_lengths: Vec<i32>,
    pub cp_lengths: Vec<i32>,
    pub prefix: i32,
}

fn i32_at(bytes: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]))
}

impl DictionaryMatcher {
    /// Lê um `.dict`, com ou sem o cabeçalho do pacote de dados. `None` se não for um dicionário
    /// de trie de bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes = if bytes.get(2..4) == Some(&[0xda, 0x27]) {
            let header = usize::from(u16::from_le_bytes([bytes[0], bytes[1]]));
            bytes.get(header..)?
        } else {
            bytes
        };
        let trie_offset = i32_at(bytes, 0)? as usize;
        let reserved1 = i32_at(bytes, 4)? as usize;
        let trie_type = i32_at(bytes, 16)?;
        if trie_type & TRIE_TYPE_MASK != TRIE_TYPE_BYTES {
            return None;
        }
        Some(Self {
            trie: bytes.get(trie_offset..reserved1)?.to_vec(),
            transform_constant: i32_at(bytes, 20)?,
        })
    }

    /// `BytesDictionaryMatcher::transform`.
    fn transform(&self, c: i32) -> i32 {
        if self.transform_constant & TRANSFORM_TYPE_MASK == TRANSFORM_TYPE_OFFSET {
            if c == 0x200d {
                return 0xff;
            } else if c == 0x200c {
                return 0xfe;
            }
            let delta = c - (self.transform_constant & TRANSFORM_OFFSET_MASK);
            if !(0..=0xfd).contains(&delta) {
                return -1;
            }
            return delta;
        }
        c
    }

    /// `matches`: as palavras do dicionário que começam na posição do texto, até `max_length`
    /// unidades e no máximo `limit` palavras. Deixa o texto depois do prefixo mais longo.
    pub fn matches(&self, text: &mut Utf16Text, max_length: i32, limit: usize) -> Matches {
        let mut trie = BytesTrie::new(&self.trie);
        let start = text.index();
        let mut found = Matches::default();
        let mut cp_matched = 0;
        loop {
            let c = text.next32();
            if c < 0 {
                break;
            }
            let result = if cp_matched == 0 {
                trie.first(self.transform(c))
            } else {
                trie.next(self.transform(c))
            };
            let length = text.index() - start;
            cp_matched += 1;
            if result.has_value() {
                if found.cu_lengths.len() < limit {
                    found.cu_lengths.push(length);
                    found.cp_lengths.push(cp_matched);
                }
                if result == TrieResult::FinalValue {
                    break;
                }
            } else if result == TrieResult::NoMatch {
                break;
            }
            if length >= max_length {
                break;
            }
        }
        found.prefix = cp_matched;
        found
    }
}
