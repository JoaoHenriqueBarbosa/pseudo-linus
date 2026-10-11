//! A tabela de deslocamentos de linha (`LineOffsetTable.rs` do upstream).
//!
//! A especificação de mapas de fontes não diz o que é uma coluna. A biblioteca "source-map" da Mozilla a lê como
//! contagem de unidades UTF-16, então geramos assim. A tabela acelera a conversão de deslocamento em bytes para essa
//! contagem; como quase todo JavaScript é ASCII e lá o mapa é 1 para 1, linhas só com ASCII não guardam colunas.

use bun_alloc::AllocError;
use bun_ast::Loc;
use bun_core::strings;

const NO_NON_ASCII: u32 = i32::MAX as u32;

/// Uma linha da tabela.
pub struct LineOffsetTable {
    pub columns_for_non_ascii: Box<[i32]>,
    /// Deslocamento do primeiro byte não ASCII da linha, ou `i32::MAX` se ela é só ASCII (o zero não serve: a linha
    /// pode começar com um byte não ASCII).
    pub byte_offset_to_first_non_ascii: u32,
    pub byte_offset_to_start_of_line: u32,
}

/// A tabela inteira em colunas (o `MultiArrayList` do upstream), à mão.
#[derive(Default)]
pub struct List {
    byte_offset_to_start_of_line: Vec<u32>,
    byte_offset_to_first_non_ascii: Vec<u32>,
    columns_for_non_ascii: Vec<Box<[i32]>>,
}

impl List {
    pub fn len(&self) -> usize {
        self.byte_offset_to_start_of_line.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn push(&mut self, row: LineOffsetTable) {
        self.byte_offset_to_start_of_line.push(row.byte_offset_to_start_of_line);
        self.byte_offset_to_first_non_ascii.push(row.byte_offset_to_first_non_ascii);
        self.columns_for_non_ascii.push(row.columns_for_non_ascii);
    }

    pub fn items_byte_offset_to_start_of_line(&self) -> &[u32] {
        &self.byte_offset_to_start_of_line
    }

    pub fn items_byte_offset_to_first_non_ascii(&self) -> &[u32] {
        &self.byte_offset_to_first_non_ascii
    }

    pub fn columns_for_non_ascii(&self, line: usize) -> &[i32] {
        &self.columns_for_non_ascii[line]
    }
}

impl LineOffsetTable {
    #[inline]
    pub(crate) fn find_line(byte_offsets_to_start_of_line: &[u32], loc: Loc) -> i32 {
        debug_assert!(loc.start > -1);
        let mut original_line: usize = 0;
        let loc_start = usize::try_from(loc.start).expect("int cast");
        let mut count = byte_offsets_to_start_of_line.len();
        while count > 0 {
            let step = count / 2;
            let i = original_line + step;
            if (byte_offsets_to_start_of_line[i] as usize) <= loc_start {
                original_line = i + 1;
                count = count - step - 1;
            } else {
                count = step;
            }
        }
        i32::try_from(original_line).expect("int cast") - 1
    }

    /// `find_line` com atalho para o acesso quase monotônico do printer: a resposta costuma ser `hint`, `hint + 1`
    /// ou `hint + 2`; fora disso cai na busca binária.
    #[inline]
    pub(crate) fn find_line_with_hint(offsets: &[u32], loc: Loc, hint: u32) -> i32 {
        debug_assert!(loc.start > -1);
        let loc_start = loc.start as u32;
        let len = offsets.len();
        let h = hint as usize;
        if h < len && offsets[h] <= loc_start {
            if h + 1 == len || loc_start < offsets[h + 1] {
                return hint as i32;
            }
            if h + 2 == len || loc_start < offsets[h + 2] {
                return hint as i32 + 1;
            }
            if h + 3 == len || loc_start < offsets[h + 3] {
                return hint as i32 + 2;
            }
        }
        Self::find_line(offsets, loc)
    }

    pub fn find_index(byte_offsets_to_start_of_line: &[u32], loc: Loc) -> Option<usize> {
        debug_assert!(loc.start > -1);
        let mut original_line: usize = 0;
        let loc_start = usize::try_from(loc.start).expect("int cast");
        let mut count = byte_offsets_to_start_of_line.len();
        while count > 0 {
            let step = count / 2;
            let i = original_line + step;
            let byte_offset = byte_offsets_to_start_of_line[i] as usize;
            if byte_offset == loc_start {
                return Some(i);
            }
            if i + 1 < byte_offsets_to_start_of_line.len() {
                let next_byte_offset = byte_offsets_to_start_of_line[i + 1] as usize;
                if byte_offset < loc_start && loc_start < next_byte_offset {
                    return Some(i);
                }
            }
            if byte_offset < loc_start {
                original_line = i + 1;
                count = count - step - 1;
            } else {
                count = step;
            }
        }
        None
    }

    pub fn generate(contents: &[u8], approximate_line_count: i32) -> Result<List, AllocError> {
        let mut list = List::default();
        let hint = approximate_line_count.max(1) as usize;
        list.byte_offset_to_start_of_line.try_reserve(hint).map_err(|_| AllocError)?;
        list.byte_offset_to_first_non_ascii.try_reserve(hint).map_err(|_| AllocError)?;
        list.columns_for_non_ascii.try_reserve(hint).map_err(|_| AllocError)?;

        let mut column: i32 = 0;
        let mut byte_offset_to_first_non_ascii: u32 = NO_NON_ASCII;
        let mut column_byte_offset: u32 = 0;
        let mut line_byte_offset: u32 = 0;
        let mut columns_for_non_ascii: Vec<i32> = Vec::new();

        let mut pos: usize = 0;
        while pos < contents.len() {
            let remaining = &contents[pos..];
            let b0 = remaining[0];
            let len_ = strings::wtf8_byte_sequence_length_with_invalid(b0);
            // `len_` é a largura declarada pelo byte inicial; uma fonte que termina em sequência cortada declara mais
            // do que resta, então toda fatia usa a largura limitada.
            let cp_len = (len_ as usize).min(remaining.len());
            let c: i32 = if len_ == 1 {
                b0 as i32
            } else {
                let mut cp_bytes = [0u8; 4];
                cp_bytes[..cp_len].copy_from_slice(&remaining[..cp_len]);
                strings::decode_wtf8_rune_t::<i32>(cp_bytes, len_, 0)
            };

            let offset = pos as u32;
            if column == 0 {
                line_byte_offset = offset;
            }

            // `byte_offset_to_first_non_ascii` também é a marca de "a linha tem não ASCII".
            if c > 0x7F && byte_offset_to_first_non_ascii == NO_NON_ASCII {
                column_byte_offset = offset - line_byte_offset;
                byte_offset_to_first_non_ascii = column_byte_offset;
            }

            if byte_offset_to_first_non_ascii != NO_NON_ASCII {
                let line_bytes_so_far = offset - line_byte_offset;
                let need = (line_bytes_so_far - column_byte_offset + 1) as usize;
                columns_for_non_ascii.extend(core::iter::repeat_n(column, need));
                column_byte_offset = line_bytes_so_far + 1;
            } else if let 14..=127 = c {
                // Pula até a próxima quebra de linha ou caractere não ASCII.
                match strings::index_of_newline_or_non_ascii_check_start::<false>(remaining, cp_len as u32) {
                    Some(j) => {
                        column += i32::try_from(j).expect("int cast");
                        pos += j as usize;
                    }
                    None => {
                        column += i32::try_from(remaining.len()).expect("int cast");
                        pos = contents.len();
                    }
                }
                continue;
            }

            match c {
                0x0D | 0x0A | 0x2028 | 0x2029 => {
                    // Quebra de linha do Windows.
                    if c == b'\r' as i32 && remaining.len() > 1 && remaining[1] == b'\n' {
                        column += 1;
                        pos += 1;
                        continue;
                    }
                    list.push(LineOffsetTable {
                        byte_offset_to_start_of_line: line_byte_offset,
                        byte_offset_to_first_non_ascii,
                        columns_for_non_ascii: core::mem::take(&mut columns_for_non_ascii).into_boxed_slice(),
                    });
                    column = 0;
                    byte_offset_to_first_non_ascii = NO_NON_ASCII;
                    column_byte_offset = 0;
                    line_byte_offset = 0;
                }
                // A "source-map" da Mozilla conta colunas em unidades UTF-16.
                _ => column += ((c > 0xFFFF) as i32) + 1,
            }
            pos += cp_len;
        }

        // Marca o começo da próxima linha.
        if column == 0 {
            line_byte_offset = contents.len() as u32;
        }
        if !columns_for_non_ascii.is_empty() {
            let line_bytes_so_far = contents.len() as u32 - line_byte_offset;
            let need = (line_bytes_so_far - column_byte_offset + 1) as usize;
            columns_for_non_ascii.extend(core::iter::repeat_n(column, need));
        }
        list.push(LineOffsetTable {
            byte_offset_to_start_of_line: line_byte_offset,
            byte_offset_to_first_non_ascii,
            columns_for_non_ascii: columns_for_non_ascii.into_boxed_slice(),
        });
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_lines() {
        let t = LineOffsetTable::generate(b"ab\ncd\r\nef", 3).unwrap();
        assert_eq!(t.items_byte_offset_to_start_of_line(), &[0, 3, 7]);
        assert!(t.items_byte_offset_to_first_non_ascii().iter().all(|&n| n == NO_NON_ASCII));
        assert_eq!(LineOffsetTable::find_line(t.items_byte_offset_to_start_of_line(), Loc { start: 4 }), 1);
    }

    #[test]
    fn non_ascii_columns_count_utf16_units() {
        // "é" ocupa 2 bytes e 1 unidade; "😀" 4 bytes e 2 unidades.
        let t = LineOffsetTable::generate("aé😀b".as_bytes(), 1).unwrap();
        assert_eq!(t.items_byte_offset_to_first_non_ascii(), &[1]);
        // Uma entrada por byte a partir do primeiro não ASCII, mais uma de sobra; os valores saem do porte fiel e são
        // conferidos contra o mapa do bun no teste de mapa do printer.
        assert_eq!(t.columns_for_non_ascii(0).len(), 8);
    }
}
