//! O transpilador do zbun (`wip/zbun/transpile`) como `SourceTranspiler` do zjsc, para os testes que rodam o fonte
//! ORIGINAL como o bun roda: reimpresso pelo impressor dele, com o mapa de posições dele.
use std::rc::Rc;

use zjsc::api::source_transpiler::{install_source_transpiler, BuildDiagnostic, BuildPosition, SourceTranspiler, TranspiledSource};
use zjsc::parser::position_map::MapSegment;

pub struct ZbunTranspiler;

impl SourceTranspiler for ZbunTranspiler {
    fn transpile(&self, path: &str, source: &str, compile_body: bool) -> Option<Result<TranspiledSource, Vec<BuildDiagnostic>>> {
        bun_transpile::loader_for(path)?;
        let done = match bun_transpile::transpile_with(path, source.as_bytes(), bun_transpile::Config { compile_body, ..Default::default() }) {
            Ok(done) => done,
            Err(error) => return Some(Err(error.diagnostics.iter().map(diagnostic).collect())),
        };
        let text = match String::from_utf8(done.text) {
            Ok(text) => text,
            Err(error) => return Some(Err(vec![BuildDiagnostic::plain(error.to_string())])),
        };
        let segments = done
            .segments
            .iter()
            .map(|s| MapSegment {
                generated_line: s.generated_line,
                generated_column: s.generated_column,
                original_line: s.original_line,
                original_column: s.original_column,
            })
            .collect();
        Some(Ok(TranspiledSource { text, segments, is_commonjs: done.is_commonjs }))
    }
}

fn diagnostic(source: &bun_transpile::Diagnostic) -> BuildDiagnostic {
    BuildDiagnostic {
        message: source.message.clone(),
        position: source.position.as_ref().map(|p| BuildPosition {
            line_text: p.line_text.clone(),
            file: p.file.clone(),
            namespace: p.namespace.clone(),
            line: p.line,
            column: p.column,
            length: p.length,
            offset: p.offset,
        }),
        notes: source.notes.iter().map(diagnostic).collect(),
    }
}

/// Instala o transpilador do zbun na thread do teste.
pub fn install() {
    install_source_transpiler(Some(Rc::new(ZbunTranspiler)));
}
