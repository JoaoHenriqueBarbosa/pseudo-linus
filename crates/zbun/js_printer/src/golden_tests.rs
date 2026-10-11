//! Golden byte a byte: cada entrada de `wip/zbun/tests/golden` passa pelo parser e pelo printer do porte e o texto sai
//! comparado ao que o `bun build --no-bundle` do bun 1.4.2 imprime (`scripts/gen_golden.sh` gera o `.expected` no
//! oráculo). As opções do parser e do printer espelham as do `bun build`: TypeScript liga a remoção dos imports sem
//! uso, o printer roda com `bundling: false`, sem minificação, com as anotações de DCE.

use std::path::Path;

use bun_alloc::Arena;
use bun_ast::Source;
use bun_js_parser::defines::Define;
use bun_js_parser::parse::parse_entry::Options as ParseOptions;
use bun_js_parser::parser::{Parser, options};

use bun_sourcemap as SourceMap;
use crate::renamer::NoOpRenamer;
use crate::{BufferPrinter, BufferWriter, Options, PrintResult};

/// Divergências conhecidas: `(entrada, causa medida no oráculo)`. Uma entrada aqui continua rodando; se passar a
/// bater, o teste falha pedindo a remoção da linha. Vazia quando tudo bate.
const KNOWN_DIVERGENCES: &[(&str, &str)] = &[];


fn loader_for(name: &str) -> bun_ast::Loader {
    match Path::new(name).extension().and_then(|e| e.to_str()) {
        Some("ts") => bun_ast::Loader::Ts,
        Some("tsx") => bun_ast::Loader::Tsx,
        Some("jsx") => bun_ast::Loader::Jsx,
        _ => bun_ast::Loader::Js,
    }
}

/// As flags de minificação do `bun build --no-bundle` que o oráculo mede, com o sufixo do arquivo `.expected` de cada uma
/// (`""` é a execução sem flag).
struct Variant {
    suffix: &'static str,
    whitespace: bool,
    syntax: bool,
    identifiers: bool,
}

const VARIANTS: &[Variant] = &[
    Variant { suffix: "", whitespace: false, syntax: false, identifiers: false },
    Variant { suffix: ".minify-whitespace", whitespace: true, syntax: false, identifiers: false },
    Variant { suffix: ".minify-syntax", whitespace: false, syntax: true, identifiers: false },
    Variant { suffix: ".minify-identifiers", whitespace: false, syntax: false, identifiers: true },
    Variant { suffix: ".minify", whitespace: true, syntax: true, identifiers: true },
];

/// O texto que o `bun build --no-bundle` imprime para `text`, como o porte o imprime.
fn transpile(name: &str, text: &[u8], variant: &Variant) -> Result<Vec<u8>, String> {
    let loader = loader_for(name);
    // O oráculo monta a entrada em /w, e o nome do `jsxDEV` vem do caminho.
    // O `Source` pede fatias `'static`; num teste vazar os poucos bytes é o jeito mais simples.
    let path: &'static [u8] = Box::leak(format!("/w/{name}").into_bytes().into_boxed_slice());
    let text: &'static [u8] = Box::leak(text.to_vec().into_boxed_slice());
    let source = Source::init_path_string(path, text);
    let arena = Arena::default();
    let mut log = bun_ast::Log::default();
    let define = Define::default();
    let mut pragma = options::JSX::Pragma::default();
    pragma.parse = loader.is_jsx();
    let mut opts = ParseOptions::init(pragma, loader);
    // As opções que `Transpiler::parse` do bun 1.4.2 liga no caminho de `bun build --no-bundle` (target browser).
    opts.keep_names = true;
    opts.suppress_warnings_about_weird_code = true;
    opts.warn_about_unbundled_modules = true;
    // `bun build --no-bundle` (`transform_only`) liga `allow_runtime = false` (`build_command.rs`).
    opts.features.allow_runtime = false;
    opts.features.top_level_await = true;
    opts.features.auto_import_jsx = true;
    opts.features.lower_using = true;
    opts.features.trim_unused_imports = loader.is_typescript();
    opts.features.minify_syntax = variant.syntax;
    opts.features.minify_identifiers = variant.identifiers;
    opts.features.minify_whitespace = variant.whitespace;
    // Sem `experimentalDecorators` nem `emitDecoratorMetadata` os decorators do TypeScript são os do TC39.
    opts.features.standard_decorators = true;
    let parsed = {
        let parser = Parser::init(opts, &mut log, &source, &define, &arena).map_err(|e| format!("Parser::init: {e:?}"))?;
        parser.parse()
    };
    let messages = || {
        log.msgs.iter().map(|m| String::from_utf8_lossy(&m.data.text).into_owned()).collect::<Vec<_>>().join("; ")
    };
    let mut ast = match parsed {
        Ok(bun_js_parser::Result::Ast(ast)) => ast,
        Ok(_) => return Err("o parser não devolveu uma Ast".into()),
        Err(e) => return Err(format!("Parser::parse: {e:?} ({})", messages())),
    };
    if log.has_errors() {
        return Err(format!("o parser registrou erro: {}", messages()));
    }
    let symbols = bun_ast::symbol::Map::init_with_one_list(core::mem::take(&mut ast.symbols));
    let print_opts = Options {
        bundling: false,
        require_ref: Some(ast.require_ref),
        import_meta_ref: ast.import_meta_ref,
        // `emit_dce_annotations` é `!minify_whitespace` nas opções do `bun build`.
        print_dce_annotations: !variant.whitespace,
        hmr_ref: ast.wrapper_ref,
        mangled_props: None,
        minify_whitespace: variant.whitespace,
        minify_syntax: variant.syntax,
        minify_identifiers: variant.identifiers,
        ..Default::default()
    };
    let mut printer = BufferPrinter::init(BufferWriter::init());
    crate::print_ast::<_, false, false>(&mut printer, &arena, &ast, symbols, &source, print_opts)
        .map_err(|e| format!("print_ast: {e:?}"))?;
    Ok(printer.ctx.written().to_vec())
}

/// A primeira linha em que os dois textos diferem, com a vizinhança, para a mensagem de falha.
fn first_difference(expected: &str, actual: &str) -> String {
    let (e, a): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    let n = e.len().max(a.len());
    for i in 0..n {
        if e.get(i) != a.get(i) {
            let from = i.saturating_sub(2);
            let mut out = format!("linha {}:\n", i + 1);
            for j in from..(i + 3).min(n) {
                out.push_str(&format!("  esperado {:>3}| {}\n", j + 1, e.get(j).copied().unwrap_or("<fim>")));
                out.push_str(&format!("  obtido   {:>3}| {}\n", j + 1, a.get(j).copied().unwrap_or("<fim>")));
            }
            return out;
        }
    }
    "só o final do texto difere (quebra de linha?)".into()
}

#[test]
fn golden_matches_bun_build_no_bundle() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/golden");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("tests/golden")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with(".expected") && dir.join(n).is_file())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "o corpus de tests/golden está vazio");

    let mut failures = Vec::new();
    let (mut passed, mut total) = (0, 0);
    for name in &names {
        let source = std::fs::read(dir.join(name)).expect("entrada");
        for variant in VARIANTS {
            let case = format!("{name}{}", variant.suffix);
            let Ok(expected) = std::fs::read(dir.join(format!("{name}.expected{}", variant.suffix))) else {
                // O oráculo recusou a entrada com essa flag, ou ainda não foi gerada.
                continue;
            };
            total += 1;
            let known = KNOWN_DIVERGENCES.iter().find(|(n, _)| *n == case);
            let outcome = transpile(name, &source, variant);
            let matches = matches!(&outcome, Ok(actual) if *actual == expected);
            match (matches, known) {
                (true, None) => passed += 1,
                (true, Some((_, cause))) => failures.push(format!("{case}: passou a bater, tire de KNOWN_DIVERGENCES ({cause})")),
                (false, Some(_)) => {}
                (false, None) => failures.push(match outcome {
                    Ok(actual) => format!(
                        "{case}: {}",
                        first_difference(&String::from_utf8_lossy(&expected), &String::from_utf8_lossy(&actual))
                    ),
                    Err(e) => format!("{case}: {e}"),
                }),
            }
        }
    }
    assert!(failures.is_empty(), "{passed}/{total} casos batem; divergem:\n{}", failures.join("\n"));
}

/// O que o chunk de um bundle de um arquivo só do `bun build` contém: o código e o campo `mappings` (com o `;` da linha
/// do comentário de cabeçalho). O `--no-bundle` do bun 1.4.2 ignora `--sourcemap`, então o mapa de posições só se mede
/// no bundle; o `Builder` do mapa é o mesmo, e as entradas de `tests/golden/maps` não têm import, export nem colisão
/// de nome, então o chunk imprime as mesmas partes que o `--no-bundle` imprimiria.
fn bundle_chunk(name: &str, text: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let path: &'static [u8] = Box::leak(format!("/w/maps/{name}").into_bytes().into_boxed_slice());
    let text: &'static [u8] = Box::leak(text.to_vec().into_boxed_slice());
    let source = Source::init_path_string(path, text);
    let arena = Arena::default();
    let mut log = bun_ast::Log::default();
    let define = Define::default();
    let mut pragma = options::JSX::Pragma::default();
    pragma.parse = false;
    let mut opts = ParseOptions::init(pragma, bun_ast::Loader::Js);
    opts.bundle = true;
    opts.tree_shaking = true;
    opts.keep_names = true;
    opts.suppress_warnings_about_weird_code = true;
    opts.features.top_level_await = true;
    opts.features.auto_import_jsx = true;
    opts.features.lower_using = true;
    opts.features.auto_polyfill_require = true;
    let mut ast = {
        let parser = Parser::init(opts, &mut log, &source, &define, &arena).map_err(|e| format!("Parser::init: {e:?}"))?;
        match parser.parse().map_err(|e| format!("Parser::parse: {e:?}"))? {
            bun_js_parser::Result::Ast(ast) => ast,
            _ => return Err("o parser não devolveu uma Ast".into()),
        }
    };
    if log.has_errors() {
        return Err("o parser registrou erro".into());
    }
    let symbols = bun_ast::symbol::Map::init_with_one_list(core::mem::take(&mut ast.symbols));
    let no_op = NoOpRenamer::init(symbols, &source, &ast.store);
    // O bundler (caminho eager) entrega as tabelas de linha de cada fonte já prontas.
    let tables = SourceMap::line_offset_table::LineOffsetTable::generate(
        source.contents(),
        i32::try_from(ast.approximate_newline_count).expect("int cast"),
    )
    .expect("tabelas de linha");
    let print_opts = Options {
        line_offset_tables: Some(&tables),
        bundling: true,
        require_ref: Some(ast.require_ref),
        import_meta_ref: ast.import_meta_ref,
        print_dce_annotations: true,
        hmr_ref: ast.wrapper_ref,
        mangled_props: None,
        module_type: bun_options_types::bundle_enums::Format::Esm,
        target: bun_ast::Target::Browser,
        ..Default::default()
    };
    match crate::print::<true>(
        &arena,
        bun_ast::Target::Browser,
        &ast,
        &source,
        print_opts,
        ast.import_records.as_slice(),
        &ast.parts,
        no_op.to_renamer(),
    ) {
        PrintResult::Result(done) => {
            let map = done.source_map.ok_or("o printer não devolveu mapa")?;
            Ok((done.code.to_vec(), map.buffer.list.clone()))
        }
        PrintResult::Err(e) => Err(format!("print: {e:?}")),
    }
}

#[test]
fn source_map_mappings_match_bun_build() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/golden/maps");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("tests/golden/maps")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with("expected"))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "o corpus de tests/golden/maps está vazio");

    let mut failures = Vec::new();
    for name in &names {
        let source = std::fs::read(dir.join(name)).expect("entrada");
        let want_code = std::fs::read(dir.join(format!("{name}.bundle.expected"))).expect("rode scripts/gen_maps.sh");
        let want_map = std::fs::read(dir.join(format!("{name}.mappings.expected"))).expect("rode scripts/gen_maps.sh");
        match bundle_chunk(name, &source) {
            Err(e) => failures.push(format!("{name}: {e}")),
            Ok((code, mappings)) => {
                if code != want_code {
                    failures.push(format!(
                        "{name}: o código do chunk difere\n{}",
                        first_difference(&String::from_utf8_lossy(&want_code), &String::from_utf8_lossy(&code))
                    ));
                    continue;
                }
                // A linha do comentário de cabeçalho do chunk (`// ../w/maps/x.js`) não tem mapeamento.
                let mut got = b";".to_vec();
                got.extend_from_slice(&mappings);
                if got != want_map {
                    failures.push(format!(
                        "{name}: mappings\n  esperado: {}\n  obtido:   {}",
                        String::from_utf8_lossy(&want_map),
                        String::from_utf8_lossy(&got)
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "mapas que divergem:\n{}", failures.join("\n"));
}
