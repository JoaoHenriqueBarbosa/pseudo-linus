//! As tabelas binárias de um iterador de quebra (`rbbidata.h`, `rbbidata.cpp`), formato 6.

use crate::ucptrie::{CodePointTrie, ValueWidth};

pub const RBBI_BOF_REQUIRED: u32 = 2;
const RBBI_8BITS_ROWS: u32 = 4;

/// Uma tabela de estados (`RBBIStateTable`): cada linha tem `fAccepting`, `fLookAhead`,
/// `fTagsIdx` e o próximo estado por categoria de caractere, em 8 ou 16 bits.
#[derive(Clone, Debug)]
pub struct StateTable {
    pub num_states: u32,
    row_len: usize,
    pub dict_categories_start: u32,
    pub lookahead_results_size: u32,
    pub flags: u32,
    rows: Vec<u8>,
}

impl StateTable {
    fn parse(bytes: &[u8]) -> Option<Self> {
        let num_states = u32_at(bytes, 0)?;
        let row_len = u32_at(bytes, 4)? as usize;
        let flags = u32_at(bytes, 16)?;
        let rows = bytes.get(20..20 + num_states as usize * row_len)?.to_vec();
        Some(Self {
            num_states,
            row_len,
            dict_categories_start: u32_at(bytes, 8)?,
            lookahead_results_size: u32_at(bytes, 12)?,
            flags,
            rows,
        })
    }

    fn field(&self, state: i32, k: usize) -> u16 {
        let row = state as usize * self.row_len;
        if self.flags & RBBI_8BITS_ROWS != 0 {
            u16::from(self.rows.get(row + k).copied().unwrap_or(0))
        } else {
            let at = row + 2 * k;
            u16::from_le_bytes([
                self.rows.get(at).copied().unwrap_or(0),
                self.rows.get(at + 1).copied().unwrap_or(0),
            ])
        }
    }

    pub fn accepting(&self, state: i32) -> u16 {
        self.field(state, 0)
    }

    pub fn look_ahead(&self, state: i32) -> u16 {
        self.field(state, 1)
    }

    pub fn tags_idx(&self, state: i32) -> u16 {
        self.field(state, 2)
    }

    pub fn next_state(&self, state: i32, category: u32) -> i32 {
        i32::from(self.field(state, 3 + category as usize))
    }
}

/// `RBBIDataWrapper`: as tabelas de um conjunto de regras compiladas.
#[derive(Clone, Debug)]
pub struct RbbiData {
    pub cat_count: u32,
    pub forward: StateTable,
    pub reverse: StateTable,
    pub trie: CodePointTrie,
    pub rule_status_table: Vec<i32>,
    pub rule_source: String,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]))
}

impl RbbiData {
    /// Lê as tabelas de um `.brk`, com ou sem o cabeçalho do pacote de dados (`UDataInfo`) na
    /// frente, como `RBBIDataWrapper::init`. `None` se o formato não for o 6.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes = if bytes.get(2..4) == Some(&[0xda, 0x27]) {
            let header = usize::from(u16::from_le_bytes([bytes[0], bytes[1]]));
            bytes.get(header..)?
        } else {
            bytes
        };
        if u32_at(bytes, 0)? != 0xb1a0 || *bytes.get(4)? != 6 {
            return None;
        }
        let section = |offset_at: usize| -> Option<&[u8]> {
            let offset = u32_at(bytes, offset_at)? as usize;
            let len = u32_at(bytes, offset_at + 4)? as usize;
            bytes.get(offset..offset + len)
        };
        let trie = CodePointTrie::from_bytes(section(32)?)?;
        if !matches!(trie.value_width(), ValueWidth::Bits8 | ValueWidth::Bits16) {
            return None;
        }
        let status = section(48)?;
        Some(Self {
            cat_count: u32_at(bytes, 12)?,
            forward: StateTable::parse(section(16)?)?,
            reverse: StateTable::parse(section(24)?)?,
            trie,
            rule_status_table: status
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| i32::from_le_bytes(*c))
                .collect(),
            rule_source: String::from_utf8_lossy(section(40)?).into_owned(),
        })
    }
}
