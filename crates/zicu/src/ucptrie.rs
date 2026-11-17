//! `UCPTrie` (`ucptrie.cpp`, `ucptrie_impl.h`): só a leitura de uma trie serializada e a consulta
//! de um ponto de código, que é o que o iterador de quebra usa.

const SIG: u32 = 0x5472_6933;
const FAST_SHIFT: u32 = 6;
const FAST_DATA_MASK: u32 = (1 << FAST_SHIFT) - 1;
const SMALL_MAX: u32 = 0xfff;
const ERROR_VALUE_NEG_DATA_OFFSET: usize = 1;
const HIGH_VALUE_NEG_DATA_OFFSET: usize = 2;
const BMP_INDEX_LENGTH: u32 = 0x10000 >> FAST_SHIFT;
const SMALL_LIMIT: u32 = 0x1000;
const SMALL_INDEX_LENGTH: u32 = SMALL_LIMIT >> FAST_SHIFT;
const SHIFT_3: u32 = 4;
const SHIFT_2: u32 = 5 + SHIFT_3;
const SHIFT_1: u32 = 5 + SHIFT_2;
const OMITTED_BMP_INDEX_1_LENGTH: u32 = 0x10000 >> SHIFT_1;
const INDEX_2_MASK: u32 = (1 << (SHIFT_1 - SHIFT_2)) - 1;
const INDEX_3_MASK: u32 = (1 << (SHIFT_2 - SHIFT_3)) - 1;
const SMALL_DATA_MASK: u32 = (1 << SHIFT_3) - 1;

/// Os valores da trie, na largura em que foram gravados.
#[derive(Clone, Debug)]
enum Data {
    Bits16(Vec<u16>),
    Bits32(Vec<u32>),
    Bits8(Vec<u8>),
}

/// A largura dos valores (`UCPTrieValueWidth`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueWidth {
    Bits16,
    Bits32,
    Bits8,
}

#[derive(Clone, Debug)]
pub struct CodePointTrie {
    index: Vec<u16>,
    data: Data,
    data_length: usize,
    high_start: u32,
    is_fast: bool,
    null_value: u32,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]))
}

impl CodePointTrie {
    /// `ucptrie_openFromBinary` com tipo e largura quaisquer, em little-endian.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if u32_at(bytes, 0)? != SIG {
            return None;
        }
        let options = u32::from(u16_at(bytes, 4)?);
        let index_length = usize::from(u16_at(bytes, 6)?);
        let data_length = (((options & 0xf000) << 4) | u32::from(u16_at(bytes, 8)?)) as usize;
        let data_null_offset = (((options & 0xf00) << 8) | u32::from(u16_at(bytes, 12)?)) as usize;
        let high_start = u32::from(u16_at(bytes, 14)?) << SHIFT_2;
        let type_int = (options >> 6) & 3;
        let width = match options & 7 {
            0 => ValueWidth::Bits16,
            1 => ValueWidth::Bits32,
            2 => ValueWidth::Bits8,
            _ => return None,
        };
        if type_int > 1 || options & 0x38 != 0 {
            return None;
        }
        let header = 16;
        let index = (0..index_length)
            .map(|i| u16_at(bytes, header + 2 * i))
            .collect::<Option<Vec<_>>>()?;
        let at = header + 2 * index_length;
        let data = match width {
            ValueWidth::Bits16 => Data::Bits16(
                (0..data_length)
                    .map(|i| u16_at(bytes, at + 2 * i))
                    .collect::<Option<_>>()?,
            ),
            ValueWidth::Bits32 => Data::Bits32(
                (0..data_length)
                    .map(|i| u32_at(bytes, at + 4 * i))
                    .collect::<Option<_>>()?,
            ),
            ValueWidth::Bits8 => Data::Bits8(bytes.get(at..at + data_length)?.to_vec()),
        };
        let mut trie = Self {
            index,
            data,
            data_length,
            high_start,
            is_fast: type_int == 0,
            null_value: 0,
        };
        let null_offset = if data_null_offset >= data_length {
            data_length - HIGH_VALUE_NEG_DATA_OFFSET
        } else {
            data_null_offset
        };
        trie.null_value = trie.value(null_offset);
        Some(trie)
    }

    pub fn value_width(&self) -> ValueWidth {
        match self.data {
            Data::Bits16(_) => ValueWidth::Bits16,
            Data::Bits32(_) => ValueWidth::Bits32,
            Data::Bits8(_) => ValueWidth::Bits8,
        }
    }

    pub fn null_value(&self) -> u32 {
        self.null_value
    }

    fn value(&self, i: usize) -> u32 {
        match &self.data {
            Data::Bits16(d) => d.get(i).map_or(0xffff_ffff, |v| u32::from(*v)),
            Data::Bits32(d) => d.get(i).copied().unwrap_or(0xffff_ffff),
            Data::Bits8(d) => d.get(i).map_or(0xffff_ffff, |v| u32::from(*v)),
        }
    }

    fn ix(&self, i: u32) -> u32 {
        u32::from(self.index.get(i as usize).copied().unwrap_or(0))
    }

    /// `ucptrie_internalSmallIndex`.
    fn small_index(&self, c: u32) -> usize {
        let mut i1 = c >> SHIFT_1;
        if self.is_fast {
            i1 += BMP_INDEX_LENGTH - OMITTED_BMP_INDEX_1_LENGTH;
        } else {
            i1 += SMALL_INDEX_LENGTH;
        }
        let mut i3_block = self.ix(self.ix(i1) + ((c >> SHIFT_2) & INDEX_2_MASK));
        let mut i3 = (c >> SHIFT_3) & INDEX_3_MASK;
        let data_block = if i3_block & 0x8000 == 0 {
            self.ix(i3_block + i3)
        } else {
            // Índices de 18 bits, em grupos de 9 entradas para cada 8 índices.
            i3_block = (i3_block & 0x7fff) + (i3 & !7) + (i3 >> 3);
            i3 &= 7;
            let high = (self.ix(i3_block) << (2 + 2 * i3)) & 0x30000;
            high | self.ix(i3_block + 1 + i3)
        };
        (data_block + (c & SMALL_DATA_MASK)) as usize
    }

    /// `_UCPTRIE_CP_INDEX`.
    fn cp_index(&self, fast_max: u32, c: u32) -> usize {
        if c <= fast_max {
            (self.ix(c >> FAST_SHIFT) + (c & FAST_DATA_MASK)) as usize
        } else if c <= 0x10ffff {
            if c >= self.high_start {
                self.data_length - HIGH_VALUE_NEG_DATA_OFFSET
            } else {
                self.small_index(c)
            }
        } else {
            self.data_length - ERROR_VALUE_NEG_DATA_OFFSET
        }
    }

    /// `UCPTRIE_FAST_GET`, que o iterador de quebra usa: `c` negativo conta como fora do Unicode.
    pub fn fast_get(&self, c: i32) -> u32 {
        self.value(self.cp_index(0xffff, c as u32))
    }

    /// `ucptrie_get`.
    pub fn get(&self, c: i32) -> u32 {
        let c = c as u32;
        if c <= 0x7f {
            return self.value(c as usize);
        }
        let fast_max = if self.is_fast { 0xffff } else { SMALL_MAX };
        self.value(self.cp_index(fast_max, c))
    }
}
