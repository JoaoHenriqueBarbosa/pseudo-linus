//! O transpilador do runtime do bun 1.4.2 (`bun run arquivo.ts`): parse, visita e impressão com as opções que o
//! `ModuleLoader` liga para o alvo `bun`, e o mapa de posições do texto impresso para o fonte original.
//!
//! As opções espelham `Transpiler::parse` e `print_ast_esm_ascii::<_, IS_BUN = true>` do upstream com o `Transpiler`
//! padrão de `bun run`: alvo `bun` (o que liga `tree_shaking`, `inlining` e `minify_syntax`), sem `minify_whitespace`
//! nem `minify_identifiers`, sem anotações de DCE (`emit_dce_annotations` é falso), impressão ASCII (`EsmAscii`) e o
//! formato do módulo pelo tipo de exportação que o parser achou.
//!
//! O mapa sai do VLQ do `Builder` do printer, um segmento por token mapeado: o JSC do bun remapeia a pilha pelo
//! segmento que cobre a posição (`SavedSourceMap`), e o zjsc consome os mesmos segmentos.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use bun_alloc::Arena;
use bun_ast::Source;
use bun_js_parser::defines::Define;
use bun_js_parser::parse::parse_entry::Options as ParseOptions;
use bun_js_parser::parser::{Parser, options};
use bun_js_printer::{BufferPrinter, BufferWriter, Options, OnSourceMapChunk, SourceMapHandler};
use bun_sourcemap::vlq;

/// Uma entrada do mapa: a posição `(generated_line, generated_column)` do texto impresso é a `(original_line,
/// original_column)` do fonte. Todas de base 0, como no campo `mappings` do source map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub generated_line: u32,
    pub generated_column: u32,
    pub original_line: u32,
    pub original_column: u32,
}

/// O resultado de uma transpilação.
#[derive(Debug)]
pub struct Transpiled {
    /// O texto JavaScript que o motor recebe (UTF-8, ASCII-only nos identificadores e literais, como o `EsmAscii`).
    pub text: Vec<u8>,
    /// Os segmentos do mapa, ordenados por posição gerada.
    pub segments: Vec<Segment>,
    /// O parser classificou o arquivo como CommonJS: o texto impresso é o corpo que o wrapper do runtime envolve.
    pub is_commonjs: bool,
}

/// O que impediu a transpilação: o texto das mensagens do parser, na ordem em que ele as registrou, e o `report`, o
/// mesmo que o bun imprime no stderr (o trecho do fonte, o circunflexo, `error: texto` e `at arquivo:linha:coluna`,
/// com as notas, um bloco por mensagem). Sem posição (falha que não veio do parser), o `report` é `error: texto`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub messages: Vec<String>,
    pub report: Vec<u8>,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.messages.join("; "))
    }
}

impl std::error::Error for Error {}

fn fail(text: impl Into<String>) -> Error {
    let text = text.into();
    Error { report: format!("error: {text}\n").into_bytes(), messages: vec![text] }
}

/// O loader que o bun escolhe pela extensão; `None` para o que o transpilador de JavaScript não trata (`.json`,
/// `.toml`, `.css`...).
pub fn loader_for(path: &str) -> Option<bun_ast::Loader> {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("ts" | "mts" | "cts") => Some(bun_ast::Loader::Ts),
        Some("tsx") => Some(bun_ast::Loader::Tsx),
        Some("jsx") => Some(bun_ast::Loader::Jsx),
        Some("js" | "mjs" | "cjs") => Some(bun_ast::Loader::Js),
        _ => None,
    }
}

/// O `Source` pede o caminho `'static`: cada caminho distinto é vazado uma vez só.
fn interned_path(path: &str) -> &'static [u8] {
    static PATHS: OnceLock<Mutex<HashSet<&'static [u8]>>> = OnceLock::new();
    let mut paths = PATHS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = paths.get(path.as_bytes()) {
        return existing;
    }
    let leaked: &'static [u8] = Box::leak(path.as_bytes().to_vec().into_boxed_slice());
    paths.insert(leaked);
    leaked
}

/// Guarda o `Chunk` que o printer entrega ao final da impressão.
struct ChunkSink<'a>(&'a mut Option<Vec<u8>>);

impl OnSourceMapChunk for ChunkSink<'_> {
    fn on_source_map_chunk(&mut self, chunk: bun_sourcemap::Chunk, _source: &Source) -> bun_js_printer::Result<()> {
        *self.0 = Some(chunk.buffer.list.clone());
        Ok(())
    }
}

/// As diferenças entre o runtime e o `bun build --no-bundle --target=bun --minify-syntax`, que é o oráculo do texto: o
/// `build` imprime as anotações `/* @__PURE__ */`, não trata o arquivo como CommonJS em tempo de execução e imprime
/// sempre em ESM; o runtime faz o contrário nas três. O `inlining` está ligado nos dois (a medição do oráculo inlina
/// `const` com `--target=bun --minify-syntax`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub like_build: bool,
    /// O texto é o corpo de `module._compile`: o parser não o embrulha na função de wrapper (o runtime do bun liga
    /// `remove_cjs_module_wrapper`) e o impressor o imprime como programa comum; quem chama põe o cabeçalho.
    pub compile_body: bool,
    /// O `bun build` sem `--minify-syntax`: sem a dobra de constante (o `if (false)` perde o corpo mesmo assim, medido).
    pub no_minify_syntax: bool,
    /// O `bun build` sem `--target=bun`: o inlining dos `const` é do alvo `bun`, não do `--minify-syntax` (medido).
    pub no_inlining: bool,
    /// O `bun build --no-bundle` sem `--target=bun`: sem `tree_shaking` o topo é uma parte só, e o `mangle_stmts` junta as
    /// declarações adjacentes (`const a = f(), b = g();`); com ele cada instrução de topo é visitada sozinha (medido).
    pub no_tree_shaking: bool,
}

/// Transpila `source` (o arquivo `path`) como o runtime do bun faz antes de entregá-lo ao JSC.
pub fn transpile(path: &str, source: &[u8]) -> Result<Transpiled, Error> {
    transpile_with(path, source, Config::default())
}

/// [`transpile`] com a [`Config`] escolhida.
pub fn transpile_with(path: &str, source: &[u8], config: Config) -> Result<Transpiled, Error> {
    let loader = loader_for(path).ok_or_else(|| fail(format!("sem loader de JavaScript para {path}")))?;
    let source = Source::init_path_string(interned_path(path), source.to_vec());
    let arena = Arena::default();
    let mut log = bun_ast::Log::default();
    let define = Define::with_pure_globals();
    let mut pragma = options::JSX::Pragma::default();
    pragma.parse = loader.is_jsx();
    let mut opts = ParseOptions::init(pragma, loader);
    opts.keep_names = true;
    opts.suppress_warnings_about_weird_code = true;
    opts.tree_shaking = !config.no_tree_shaking;
    // `warn_about_unbundled_modules` é `!target.is_bun()`.
    opts.warn_about_unbundled_modules = false;
    opts.features.allow_runtime = true;
    opts.features.top_level_await = true;
    opts.features.remove_cjs_module_wrapper = config.compile_body;
    opts.features.auto_import_jsx = true;
    // O JSC implementa `using` e `await using`: com o alvo `bun` nada se rebaixa.
    opts.features.lower_using = false;
    opts.features.trim_unused_imports = loader.is_typescript();
    opts.features.inlining = !config.no_inlining;
    opts.features.minify_syntax = !config.no_minify_syntax;
    opts.features.dead_code_elimination = true;
    opts.features.commonjs_at_runtime = !config.like_build;
    // Sem `experimentalDecorators` nem `emitDecoratorMetadata` os decorators do TypeScript são os do TC39.
    opts.features.standard_decorators = true;
    // O que o parser recusou: as mensagens de erro do log (e o relatório que o bun imprime), ou `fallback` quando ele
    // não registrou nenhuma.
    let refused = |log: &bun_ast::Log, fallback: String| {
        let messages: Vec<String> = log
            .msgs
            .iter()
            .filter(|m| m.kind == bun_ast::Kind::Err)
            .map(|m| String::from_utf8_lossy(&m.data.text).into_owned())
            .collect();
        if messages.is_empty() {
            return fail(fallback);
        }
        let mut report = Vec::new();
        let _ = log.print(&mut report);
        Error { messages, report }
    };
    let parser = match Parser::init(opts, &mut log, &source, &define, &arena) {
        Ok(parser) => parser,
        Err(e) => return Err(refused(&log, format!("{e:?}"))),
    };
    let parsed = parser.parse();
    let mut ast = match parsed {
        Ok(bun_js_parser::Result::Ast(ast)) => ast,
        Ok(_) => return Err(fail("o parser não devolveu uma Ast")),
        Err(e) => return Err(refused(&log, format!("{e:?}"))),
    };
    if log.has_errors() {
        return Err(refused(&log, String::new()));
    }
    let is_commonjs = ast.exports_kind == bun_ast::ExportsKind::Cjs;
    let symbols = bun_ast::symbol::Map::init_with_one_list(core::mem::take(&mut ast.symbols));
    let mut mappings: Option<Vec<u8>> = None;
    let print_opts = Options {
        bundling: false,
        require_ref: Some(ast.require_ref),
        source_map_handler: Some(SourceMapHandler::for_(ChunkSink(&mut mappings))),
        import_meta_ref: ast.import_meta_ref,
        print_dce_annotations: config.like_build,
        minify_syntax: !config.no_minify_syntax,
        hmr_ref: ast.wrapper_ref,
        mangled_props: None,
        module_type: if is_commonjs && !config.like_build && !config.compile_body {
            bun_options_types::bundle_enums::Format::Cjs
        } else {
            bun_options_types::bundle_enums::Format::Esm
        },
        target: bun_ast::Target::Bun,
        ..Default::default()
    };
    let mut printer = BufferPrinter::init(BufferWriter::init());
    bun_js_printer::print_ast::<_, true, true>(&mut printer, &arena, &ast, symbols, &source, print_opts)
        .map_err(|e| fail(format!("print_ast: {e:?}")))?;
    let text = printer.ctx.written().to_vec();
    Ok(Transpiled { text, segments: decode_mappings(mappings.as_deref().unwrap_or(b"")), is_commonjs })
}

/// Decodifica o campo `mappings` (segmentos de 4 ou 5 VLQ, linhas separadas por `;`) em segmentos absolutos.
pub fn decode_mappings(mappings: &[u8]) -> Vec<Segment> {
    let mut out = Vec::new();
    let (mut generated_line, mut original_line, mut original_column) = (0u32, 0i64, 0i64);
    let mut generated_column = 0i64;
    let mut at = 0;
    while at < mappings.len() {
        match mappings[at] {
            b';' => {
                generated_line += 1;
                generated_column = 0;
                at += 1;
                continue;
            }
            b',' => {
                at += 1;
                continue;
            }
            _ => {}
        }
        let mut fields = [0i32; 5];
        let mut count = 0;
        while count < 5 && at < mappings.len() && !matches!(mappings[at], b',' | b';') {
            let decoded = vlq::decode(mappings, at);
            if decoded.start == at {
                return out;
            }
            fields[count] = decoded.value;
            at = decoded.start;
            count += 1;
        }
        if count < 4 {
            continue;
        }
        generated_column += i64::from(fields[0]);
        original_line += i64::from(fields[2]);
        original_column += i64::from(fields[3]);
        let to_u32 = |value: i64| u32::try_from(value.max(0)).unwrap_or(u32::MAX);
        out.push(Segment {
            generated_line,
            generated_column: to_u32(generated_column),
            original_line: to_u32(original_line),
            original_column: to_u32(original_column),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(path: &str, source: &str) -> String {
        String::from_utf8(transpile(path, source.as_bytes()).expect("transpila").text).expect("utf-8")
    }

    #[test]
    fn runtime_options_fold_what_minify_syntax_folds() {
        let text = run("/w/a.js", "function f(x) {\n  const dead = 1;\n  return x + 1;\n}\nnew a[\"class\"]();\n");
        assert!(text.contains("function f(x) {\n  return x + 1;\n}"), "{text}");
        assert!(text.contains("new a.class"), "{text}");
    }

    #[test]
    fn typescript_types_are_erased() {
        let text = run("/w/a.ts", "const x: number = 1 as number;\nexport { x };\n");
        assert!(text.contains("const x = 1"), "{text}");
        assert!(!text.contains("number"), "{text}");
    }

    #[test]
    fn commonjs_is_detected() {
        let done = transpile("/w/a.js", b"module.exports = 1;\n").expect("transpila");
        assert!(done.is_commonjs);
    }

    #[test]
    fn syntax_error_reports_messages() {
        let err = transpile("/w/a.js", b"let = = ;").expect_err("sintaxe inválida");
        assert!(!err.messages.is_empty());
    }

    #[test]
    fn mappings_decode_to_absolute_positions() {
        // `AAAA` é (0,0,0,0); `QACA` é coluna +8, linha +1; `;` troca a linha gerada.
        let segments = decode_mappings(b"AAAA,QACA;AACA");
        assert_eq!(
            segments,
            vec![
                Segment { generated_line: 0, generated_column: 0, original_line: 0, original_column: 0 },
                Segment { generated_line: 0, generated_column: 8, original_line: 1, original_column: 0 },
                Segment { generated_line: 1, generated_column: 0, original_line: 2, original_column: 0 },
            ]
        );
    }
}
