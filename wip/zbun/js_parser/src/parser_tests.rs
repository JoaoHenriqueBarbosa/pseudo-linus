//! O parser inteiro (`Parser::init` e `Parser::parse`) sobre a primeira entrada do plano
//! (`wip/notes/bun-transpiler-t0.md`): `const x: number = 1` em TypeScript. Confere que a análise termina sem mensagem
//! de erro e que a `Ast` devolvida carrega o `Store` com o statement, a declaração e o valor que o parser do upstream
//! produz (a anotação de tipo sai na visita do TypeScript).

use bun_alloc::Arena;
use bun_ast::{ExprData, Source, StmtData};

use crate::defines::Define;
use crate::parse::parse_entry::Options;
use crate::parser::{Parser, options};

/// Roda o parser sobre `text` com o `loader` e devolve a `Ast`; qualquer erro registrado no `Log` falha o teste.
fn parse(text: &'static str, name: &'static str, loader: options::Loader) -> Box<bun_ast::Ast> {
    let source = Source::init_path_string(name.as_bytes(), text.as_bytes());
    let arena = Arena::default();
    let mut log = bun_ast::Log::default();
    let define = Define::default();
    let mut opts = Options::init(options::JSX::Pragma::default(), loader);
    // O `bun build` liga a remoção dos imports sem uso nos arquivos TypeScript; o default do `RuntimeFeatures` não.
    opts.features.trim_unused_imports = loader.is_typescript();
    let ast = {
        let parser = Parser::init(opts, &mut log, &source, &define, &arena).expect("Parser::init");
        match parser.parse().expect("Parser::parse") {
            crate::Result::Ast(ast) => ast,
            _ => panic!("o parser não devolveu uma Ast"),
        }
    };
    assert!(!log.has_errors(), "o parser registrou erro em {text:?}");
    ast
}

#[test]
fn first_case() {
    let ast = parse("const x: number = 1", "caso.ts", options::Loader::Ts);
    let stmts: Vec<_> = ast.parts.iter().flat_map(|part| ast.store[part.stmts].iter().copied()).collect();
    assert_eq!(stmts.len(), 1, "um statement no arquivo");
    let StmtData::SLocal(local) = stmts[0].data else { panic!("o statement não é uma declaração") };
    assert!(ast.store[local].kind == bun_ast::S::Kind::KConst, "a declaração não é const");
    let decls = &ast.store[ast.store[local].decls];
    assert_eq!(decls.len(), 1);
    let value = decls[0].value.expect("o valor inicial");
    let ExprData::ENumber(number) = value.data else { panic!("o valor não é número") };
    assert_eq!(number.value(), 1.0);
}

/// As etiquetas dos statements de topo da `Ast`, na ordem, como os `tag()` do upstream as escrevem.
fn top_level_tags(ast: &bun_ast::Ast) -> Vec<&'static str> {
    ast.parts
        .iter()
        .flat_map(|part| ast.store[part.stmts].iter().copied())
        .map(|stmt| <&'static str>::from(stmt.data.tag()))
        .collect()
}

#[test]
fn base_case() {
    let ast = parse("const x = 1 + 2;", "caso.ts", options::Loader::Ts);
    assert_eq!(top_level_tags(&ast), ["s_local"]);
}

#[test]
fn enum_case() {
    // O enum do TypeScript vira `var Color;` e a chamada que o preenche.
    let ast = parse("enum Color { Red, Green = 4, Blue }", "caso.ts", options::Loader::Ts);
    assert_eq!(top_level_tags(&ast), ["s_local", "s_expr"]);
}

#[test]
fn namespace_case() {
    let ast = parse("namespace NS { export const a = 1; }", "caso.ts", options::Loader::Ts);
    assert_eq!(top_level_tags(&ast), ["s_local", "s_expr"]);
}

#[test]
fn import_type_case() {
    // O `import type` e o import que o TypeScript descobre sem uso saem da `Ast`.
    let ast = parse(
        "import type { T } from \"./t\"; import { v } from \"./v\";",
        "caso.ts",
        options::Loader::Ts,
    );
    assert_eq!(top_level_tags(&ast), Vec::<&str>::new());
}

#[test]
fn satisfies_case() {
    let ast = parse("const cfg = { port: 1 } satisfies Config;", "caso.ts", options::Loader::Ts);
    assert_eq!(top_level_tags(&ast), ["s_local"]);
    let StmtData::SLocal(local) = ast.store[ast.parts[0].stmts][0].data else { panic!("não é declaração") };
    let value = ast.store[ast.store[local].decls][0].value.expect("o valor inicial");
    assert!(matches!(value.data, ExprData::EObject(_)), "o `satisfies` sai e fica o objeto");
}

#[test]
fn decorators_case() {
    // Só confere que a análise e a baixa dos decorators terminam sem erro e deixam statements.
    let ast = parse(
        "@sealed class Greeter { @log greet() { return \"hi\"; } }",
        "caso.ts",
        options::Loader::Ts,
    );
    assert!(!top_level_tags(&ast).is_empty());
}

#[test]
fn jsx_case() {
    let ast = parse("export default () => <div className=\"a\">hi</div>", "caso.tsx", options::Loader::Tsx);
    assert_eq!(top_level_tags(&ast), ["s_export_default"]);
}
