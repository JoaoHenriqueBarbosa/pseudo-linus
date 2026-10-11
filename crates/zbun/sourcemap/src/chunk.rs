//! `Chunk` e o `Builder` que o printer alimenta a cada token (`Chunk.rs` do upstream), só com o formato VLQ.

use bun_ast::Loc;
use bun_core::{MutableString, strings};

use crate::{LineOffsetTable, SourceMapState, append_mapping_to_buffer, line_offset_table};

#[derive(Clone)]
pub struct Chunk {
    pub buffer: MutableString,

    /// Este estado final reescreve o começo do trecho seguinte, para preservar os VLQ em delta.
    pub end_state: SourceMapState,

    /// Provavelmente não há mapeamento no fim do arquivo, mas, se outro trecho vier depois, é preciso saber quantos
    /// caracteres tinha a última linha gerada.
    pub final_generated_column: i32,

    /// Trechos vazios são ignorados.
    pub should_ignore: bool,
}

impl Chunk {
    pub fn init_empty() -> Chunk {
        Chunk {
            buffer: MutableString::init_empty(),
            end_state: SourceMapState::default(),
            final_generated_column: 0,
            should_ignore: true,
        }
    }
}

/// Os métodos que o `SourceMapFormat<T>` repassa ao `ctx`.
pub trait SourceMapFormatCtx: Sized {
    fn init() -> Self;
    fn append_line_separator(&mut self) -> Result<(), crate::Error>;
    fn append(&mut self, current_state: SourceMapState, prev_state: SourceMapState) -> Result<(), crate::Error>;
    fn should_ignore(&self) -> bool;
    fn get_buffer(&mut self) -> &mut MutableString;
    fn take_buffer(&mut self) -> MutableString;
    fn get_count(&self) -> usize;
}

pub struct SourceMapFormat<T: SourceMapFormatCtx> {
    pub ctx: T,
}

impl<T: SourceMapFormatCtx> SourceMapFormat<T> {
    pub fn init() -> Self {
        Self { ctx: T::init() }
    }

    pub(crate) fn append_line_separator(&mut self) -> Result<(), crate::Error> {
        self.ctx.append_line_separator()
    }

    pub(crate) fn append(&mut self, current_state: SourceMapState, prev_state: SourceMapState) -> Result<(), crate::Error> {
        self.ctx.append(current_state, prev_state)
    }

    pub fn should_ignore(&self) -> bool {
        self.ctx.should_ignore()
    }

    pub(crate) fn get_buffer(&mut self) -> &mut MutableString {
        self.ctx.get_buffer()
    }

    pub(crate) fn take_buffer(&mut self) -> MutableString {
        self.ctx.take_buffer()
    }

    pub(crate) fn get_count(&self) -> usize {
        self.ctx.get_count()
    }
}

#[derive(Default)]
pub struct VLQSourceMap {
    pub data: MutableString,
    pub(crate) count: usize,
    pub(crate) offset: usize,
}

impl SourceMapFormatCtx for VLQSourceMap {
    fn init() -> VLQSourceMap {
        VLQSourceMap::default()
    }

    fn append_line_separator(&mut self) -> Result<(), crate::Error> {
        self.data.append_char(b';')?;
        Ok(())
    }

    fn append(&mut self, current_state: SourceMapState, prev_state: SourceMapState) -> Result<(), crate::Error> {
        let last_byte: u8 = if self.data.list.len() > self.offset { self.data.list[self.data.list.len() - 1] } else { 0 };
        append_mapping_to_buffer(&mut self.data, last_byte, prev_state, current_state);
        self.count += 1;
        Ok(())
    }

    fn should_ignore(&self) -> bool {
        self.count == 0
    }

    fn get_buffer(&mut self) -> &mut MutableString {
        &mut self.data
    }

    fn take_buffer(&mut self) -> MutableString {
        core::mem::replace(&mut self.data, MutableString::init_empty())
    }

    fn get_count(&self) -> usize {
        self.count
    }
}

/// A tabela de linhas por onde o `add_source_mapping` resolve posições, e quem é dono dela.
pub enum LineOffsetTables<'a> {
    /// Sem tabela: todo mapeamento é descartado (mapas desligados, ou nenhuma tabela para esta fonte).
    None,
    /// Tabela emprestada de quem a mantém (uma fonte imprime em vários trechos).
    Borrowed(&'a line_offset_table::List),
    /// A primeira chamada de `add_source_mapping` gera a tabela a partir de `contents` e troca esta variante por
    /// `Owned`, para módulos que não emitem mapeamento nunca pagarem a varredura da fonte inteira.
    Deferred { contents: &'a [u8], approximate_line_count: i32 },
    /// Gerada a partir de `Deferred`.
    Owned(line_offset_table::List),
}

impl LineOffsetTables<'_> {
    fn list(&self) -> Option<&line_offset_table::List> {
        match self {
            Self::Borrowed(list) => Some(list),
            Self::Owned(list) => Some(list),
            Self::None | Self::Deferred { .. } => None,
        }
    }

    fn len(&self) -> usize {
        self.list().map_or(0, |list| list.len())
    }
}

pub struct NewBuilder<'a, T: SourceMapFormatCtx> {
    pub source_map: SourceMapFormat<T>,
    pub line_offset_tables: LineOffsetTables<'a>,

    pub prev_state: SourceMapState,
    pub last_generated_update: u32,
    pub generated_column: i32,
    pub prev_loc: Loc,
    pub has_prev_state: bool,

    // Contorno de um bug da biblioteca "source-map" (mozilla/source-map#261): ela às vezes devolve nulo numa consulta
    // a menos que toda linha comece com um mapeamento na coluna zero. Repete-se o mapeamento anterior quando uma
    // linha acaba sem começar com um, e só quando preciso.
    pub line_starts_with_mapping: bool,
    pub cover_lines_without_mappings: bool,

    pub approximate_input_line_count: usize,
}

impl<T: SourceMapFormatCtx + Default> Default for NewBuilder<'_, T> {
    /// O que `get_source_map_builder` devolve com mapas desligados: inerte, nunca lido.
    fn default() -> Self {
        Self {
            source_map: SourceMapFormat { ctx: T::default() },
            line_offset_tables: LineOffsetTables::None,
            prev_state: SourceMapState::default(),
            last_generated_update: 0,
            generated_column: 0,
            prev_loc: Loc::EMPTY,
            has_prev_state: false,
            line_starts_with_mapping: false,
            cover_lines_without_mappings: false,
            approximate_input_line_count: 0,
        }
    }
}

impl NewBuilder<'_, VLQSourceMap> {
    pub fn generate_chunk(&mut self, output: &[u8]) -> Chunk {
        self.update_generated_line_and_column(output);
        Chunk {
            buffer: self.source_map.take_buffer(),
            end_state: self.prev_state,
            final_generated_column: self.generated_column,
            should_ignore: self.source_map.should_ignore(),
        }
    }

    /// Varre o texto impresso desde o último mapeamento e atualiza a linha e a coluna geradas.
    #[inline]
    pub(crate) fn update_generated_line_and_column(&mut self, output: &[u8]) {
        let slice = &output[self.last_generated_update as usize..];
        if strings::index_of_newline_or_non_ascii(slice, 0).is_none() {
            debug_assert!(slice.len() <= i32::MAX as usize);
            self.generated_column += slice.len() as i32;
            self.last_generated_update = output.len() as u32;
            return;
        }
        self.update_generated_line_and_column_slow(output);
    }

    fn update_generated_line_and_column_slow(&mut self, output: &[u8]) {
        let slice = &output[self.last_generated_update as usize..];

        let mut needs_mapping = self.cover_lines_without_mappings && !self.line_starts_with_mapping && self.has_prev_state;

        let mut i: usize = 0;
        let n: usize = slice.len();
        while i < n {
            let len = strings::wtf8_byte_sequence_length_with_invalid(slice[i]);
            let mut cp_bytes = [0u8; 4];
            let take = (len as usize).min(n - i);
            cp_bytes[..take].copy_from_slice(&slice[i..i + take]);
            let c = strings::decode_wtf8_rune_t::<i32>(cp_bytes, len, strings::UNICODE_REPLACEMENT as i32);
            i += len as usize;

            match c {
                14..=127 => {
                    if let Some(j) = strings::index_of_newline_or_non_ascii(slice, i as u32) {
                        let advance = (j as usize - i) + 1;
                        self.generated_column += advance as i32;
                        i = j as usize;
                        continue;
                    } else {
                        let remaining = slice[i..].len();
                        self.generated_column += remaining as i32 + 1;
                        break;
                    }
                }
                // '\r', '\n', U+2028, U+2029
                0x0D | 0x0A | 0x2028 | 0x2029 => {
                    // Quebra do Windows.
                    if c == 0x0D {
                        let newline_check = self.last_generated_update as usize + i + 1;
                        if newline_check < output.len() && output[newline_check] == b'\n' {
                            continue;
                        }
                    }

                    // Se vamos para a próxima linha e a anterior ficou sem mapeamento, mapeia o começo dela.
                    if needs_mapping {
                        self.append_mapping_without_remapping(SourceMapState {
                            generated_line: self.prev_state.generated_line,
                            generated_column: 0,
                            source_index: self.prev_state.source_index,
                            original_line: self.prev_state.original_line,
                            original_column: self.prev_state.original_column,
                        });
                    }

                    self.prev_state.generated_line += 1;
                    self.prev_state.generated_column = 0;
                    self.generated_column = 0;
                    self.source_map.append_line_separator().expect("unreachable");

                    // A linha nova ainda não tem mapeamento.
                    self.line_starts_with_mapping = false;

                    needs_mapping = self.cover_lines_without_mappings && !self.line_starts_with_mapping && self.has_prev_state;
                }

                // A "source-map" da Mozilla conta colunas em unidades UTF-16.
                _ => self.generated_column += (c > 0xFFFF) as i32 + 1,
            }
        }

        self.last_generated_update = output.len() as u32;
    }

    pub(crate) fn append_mapping(&mut self, current_state: SourceMapState) {
        self.append_mapping_without_remapping(current_state);
    }

    pub(crate) fn append_mapping_without_remapping(&mut self, current_state: SourceMapState) {
        self.source_map.append(current_state, self.prev_state).expect("unreachable");
        self.prev_state = current_state;
        self.has_prev_state = true;
    }

    pub fn add_source_mapping(&mut self, loc: Loc, output: &[u8]) {
        // Não repete o mapeamento da mesma posição, nem mapeia código gerado.
        if self.prev_loc.eql(loc) || loc.start == Loc::EMPTY.start {
            return;
        }

        self.prev_loc = loc;

        if let LineOffsetTables::Deferred { contents, approximate_line_count } = self.line_offset_tables {
            self.line_offset_tables =
                LineOffsetTables::Owned(LineOffsetTable::generate(contents, approximate_line_count).unwrap_or_default());
        }

        // Sem mapeamentos: acontece, por exemplo, ao importar um recurso que não tem mapa (png, jpg).
        let Some(list) = self.line_offset_tables.list() else {
            return;
        };
        if self.line_offset_tables.len() == 0 {
            return;
        }
        let byte_offsets = list.items_byte_offset_to_start_of_line();

        // O printer emite quase em ordem da fonte: a linha original anterior é a resposta, ou uma ou duas antes.
        let original_line = LineOffsetTable::find_line_with_hint(byte_offsets, loc, self.prev_state.original_line as u32);
        let idx = original_line.max(0) as usize;

        let mut original_column = loc.start - byte_offsets[idx] as i32;
        // `first_non_ascii` vale `i32::MAX` em linha só ASCII, então a comparação falha e a coluna não é lida.
        let first_non_ascii = list.items_byte_offset_to_first_non_ascii()[idx];
        if original_column >= first_non_ascii as i32 {
            let cols = list.columns_for_non_ascii(idx);
            if !cols.is_empty() {
                original_column = cols[(original_column as u32 - first_non_ascii) as usize];
            }
        }

        self.update_generated_line_and_column(output);

        // Se a linha não começa com mapeamento e vamos pôr um fora do começo, insere antes um que a abra.
        if self.cover_lines_without_mappings && !self.line_starts_with_mapping && self.generated_column > 0 && self.has_prev_state {
            self.append_mapping_without_remapping(SourceMapState {
                generated_line: self.prev_state.generated_line,
                generated_column: 0,
                source_index: self.prev_state.source_index,
                original_line: self.prev_state.original_line,
                original_column: self.prev_state.original_column,
            });
        }

        self.append_mapping(SourceMapState {
            generated_line: self.prev_state.generated_line,
            generated_column: self.generated_column.max(0),
            source_index: self.prev_state.source_index,
            original_line: original_line.max(0),
            original_column: original_column.max(0),
        });

        // A linha agora tem mapeamento, não insere outro.
        self.line_starts_with_mapping = true;
    }
}

pub type Builder<'a> = NewBuilder<'a, VLQSourceMap>;
