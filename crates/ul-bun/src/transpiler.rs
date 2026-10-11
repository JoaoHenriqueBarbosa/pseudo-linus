//! O transpilador do bun (`bun_transpile`, o porte do `js_parser` e do `js_printer`) como `SourceTranspiler` do motor:
//! o `bun run a.ts`, o `import './b.tsx'` e o `bun -e` com TypeScript passam o fonte por ele antes de o zjsc o rodar, e
//! o motor lê o mapa de posições dele para as pilhas de erro.

use zjsc::api::source_transpiler::{BuildDiagnostic, BuildPosition, SourceTranspiler, TranspiledSource};
use zjsc::parser::position_map::MapSegment;

/// O `SourceTranspiler` do bun: o transpilador do runtime (`bun run`), alvo `bun`.
#[derive(Debug, Default)]
pub struct BunTranspiler;

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

impl SourceTranspiler for BunTranspiler {
    fn transpile(&self, path: &str, source: &str, compile_body: bool) -> Option<Result<TranspiledSource, Vec<BuildDiagnostic>>> {
        // `bun -e` roda o código como `[eval]` com o loader `tsx` (medido: TypeScript e JSX valem).
        let loader = path.ends_with("/[eval]").then_some(bun_transpile::Loader::Tsx);
        if loader.is_none() {
            bun_transpile::loader_for(path)?;
        }
        let done = match bun_transpile::transpile_with(path, source.as_bytes(), bun_transpile::Config { compile_body, loader, ..Default::default() }) {
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
