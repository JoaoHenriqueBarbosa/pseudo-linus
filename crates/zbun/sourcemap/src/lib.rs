//! Porte para Rust seguro do subconjunto de `bun_sourcemap` (`src/sourcemap` do bun 1.4.2) que o printer escreve:
//! o VLQ, a tabela de deslocamentos de linha e o `Chunk::Builder` que gera o campo `mappings` do mapa. Sem `unsafe`,
//! sem ponteiro cru. O `InternalSourceMap` (formato privado em processo do modo preguiçoso do runtime do bun, que
//! nada fora do bun lê), o `Mapping`/`ParsedSourceMap` (a leitura de mapas) e o `SavedSourceMap` não entram: o README
//! de `wip/zbun` registra o que falta.

pub use bun_core::{CrateError as Error, CrateResult as Result};

pub mod chunk;
pub mod line_offset_table;
pub mod vlq;

pub use chunk::Chunk;
pub use line_offset_table::LineOffsetTable;
pub use vlq::VLQ;

/// Coordenadas num mapa de fontes são deltas, por tamanho. Ao juntar trechos gerados em paralelo, o primeiro segmento
/// de cada um precisa ser refeito em relação ao fim do anterior.
#[derive(Default, Clone, Copy)]
pub struct SourceMapState {
    /// Não vai no mapa: só o bundler a usa para juntar os trechos direito.
    pub generated_line: i32,
    /// Estas vão no mapa, em VLQ.
    pub generated_column: i32,
    pub source_index: i32,
    pub original_line: i32,
    pub original_column: i32,
}

/// Anexa ao buffer o mapeamento `current_state` como quatro VLQ, com delta em relação a `prev_state` e vírgula antes
/// se o último byte já escrito não é um separador.
pub(crate) fn append_mapping_to_buffer(
    buffer: &mut bun_core::MutableString,
    last_byte: u8,
    prev_state: SourceMapState,
    current_state: SourceMapState,
) {
    let needs_comma = last_byte != 0 && last_byte != b';' && last_byte != b'"';
    if needs_comma {
        buffer.list.push(b',');
    }
    let deltas = [
        // A coluna gerada (a linha vai com ';' em outro lugar).
        current_state.generated_column.saturating_sub(prev_state.generated_column),
        // O arquivo de origem.
        current_state.source_index.saturating_sub(prev_state.source_index),
        // A linha original.
        current_state.original_line.saturating_sub(prev_state.original_line),
        // A coluna original.
        current_state.original_column.saturating_sub(prev_state.original_column),
    ];
    for delta in deltas {
        buffer.list.extend_from_slice(VLQ::encode(delta).slice());
    }
}
