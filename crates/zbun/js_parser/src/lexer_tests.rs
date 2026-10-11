//! O lexer sobre os oito casos do plano (`wip/notes/bun-transpiler-t0.md`): cada entrada é tokenizada como o parser
//! do upstream a começa (`step()` e `next()`), e a sequência de tokens, com o texto cru de cada um, é a que o lexer
//! do bun produz. O caso de JSX também passa pelos modos de elemento e de filho, que é como o parser o lê.

use bun_alloc::Arena;
use bun_ast::lexer_tables::T;
use bun_ast::{Log, Source};

use crate::lexer::Lexer;

fn name(t: T) -> &'static str {
    t.into()
}

/// Os tokens de `text` até o fim, como `nome texto-cru`.
fn tokens(text: &'static str) -> Vec<String> {
    let source = Source::init_path_string(b"caso.ts".as_slice(), text.as_bytes());
    let arena = Arena::default();
    let mut log = Log::default();
    let mut out = Vec::new();
    {
        let mut lexer = Lexer::init_without_reading(&mut log, &source, &arena);
        lexer.step();
        lexer.next().expect("next");
        while lexer.token != T::TEndOfFile {
            out.push(format!("{} {}", name(lexer.token), String::from_utf8_lossy(lexer.raw())));
            lexer.next().expect("next");
        }
    }
    assert!(!log.has_any(), "o lexer registrou mensagens");
    out
}

fn assert_tokens(text: &'static str, expected: &[&str]) {
    assert_eq!(tokens(text), expected, "tokens de {text:?}");
}

#[test]
fn first_case() {
    assert_tokens(
        "const x: number = 1",
        &["TConst const", "TIdentifier x", "TColon :", "TIdentifier number", "TEquals =", "TNumericLiteral 1"],
    );
}

#[test]
fn base_case() {
    assert_tokens(
        "const x = 1 + 2;",
        &["TConst const", "TIdentifier x", "TEquals =", "TNumericLiteral 1", "TPlus +", "TNumericLiteral 2", "TSemicolon ;"],
    );
}

#[test]
fn enum_case() {
    assert_tokens(
        "enum Color { Red, Green = 4, Blue }",
        &[
            "TEnum enum",
            "TIdentifier Color",
            "TOpenBrace {",
            "TIdentifier Red",
            "TComma ,",
            "TIdentifier Green",
            "TEquals =",
            "TNumericLiteral 4",
            "TComma ,",
            "TIdentifier Blue",
            "TCloseBrace }",
        ],
    );
}

#[test]
fn namespace_case() {
    assert_tokens(
        "namespace NS { export const a = 1; }",
        &[
            "TIdentifier namespace",
            "TIdentifier NS",
            "TOpenBrace {",
            "TExport export",
            "TConst const",
            "TIdentifier a",
            "TEquals =",
            "TNumericLiteral 1",
            "TSemicolon ;",
            "TCloseBrace }",
        ],
    );
}

#[test]
fn import_type_case() {
    assert_tokens(
        "import type { T } from \"./t\"; import { v } from \"./v\";",
        &[
            "TImport import",
            "TIdentifier type",
            "TOpenBrace {",
            "TIdentifier T",
            "TCloseBrace }",
            "TIdentifier from",
            "TStringLiteral \"./t\"",
            "TSemicolon ;",
            "TImport import",
            "TOpenBrace {",
            "TIdentifier v",
            "TCloseBrace }",
            "TIdentifier from",
            "TStringLiteral \"./v\"",
            "TSemicolon ;",
        ],
    );
}

#[test]
fn satisfies_case() {
    assert_tokens(
        "const cfg = { port: 1 } satisfies Config;",
        &[
            "TConst const",
            "TIdentifier cfg",
            "TEquals =",
            "TOpenBrace {",
            "TIdentifier port",
            "TColon :",
            "TNumericLiteral 1",
            "TCloseBrace }",
            "TIdentifier satisfies",
            "TIdentifier Config",
            "TSemicolon ;",
        ],
    );
}

#[test]
fn decorators_case() {
    assert_tokens(
        "@sealed class Greeter { @log greet() { return \"hi\"; } }",
        &[
            "TAt @",
            "TIdentifier sealed",
            "TClass class",
            "TIdentifier Greeter",
            "TOpenBrace {",
            "TAt @",
            "TIdentifier log",
            "TIdentifier greet",
            "TOpenParen (",
            "TCloseParen )",
            "TOpenBrace {",
            "TReturn return",
            "TStringLiteral \"hi\"",
            "TSemicolon ;",
            "TCloseBrace }",
            "TCloseBrace }",
        ],
    );
}

const JSX: &str = "export default () => <div className=\"a\">hi</div>";

/// Os dois casos de JSX (automático e com `jsxImportSource` no tsconfig) têm o mesmo fonte; o tsconfig não chega
/// ao lexer.
#[test]
fn jsx_case_plain_tokens() {
    assert_tokens(
        JSX,
        &[
            "TExport export",
            "TDefault default",
            "TOpenParen (",
            "TCloseParen )",
            "TEqualsGreaterThan =>",
            "TLessThan <",
            "TIdentifier div",
            "TIdentifier className",
            "TEquals =",
            "TStringLiteral \"a\"",
            "TGreaterThan >",
            "TIdentifier hi",
            "TLessThan <",
            "TSlash /",
            "TIdentifier div",
            "TGreaterThan >",
        ],
    );
}

/// O caminho que o parser usa no JSX: depois do `<`, o modo de elemento; depois do `>`, o modo de filho.
#[test]
fn jsx_case_element_modes() {
    let source = Source::init_path_string(b"caso.tsx".as_slice(), JSX.as_bytes());
    let arena = Arena::default();
    let mut log = Log::default();
    let mut seen = Vec::new();
    {
        let mut lexer = Lexer::init_without_reading(&mut log, &source, &arena);
        lexer.step();
        lexer.next().unwrap();
        while lexer.token != T::TLessThan {
            lexer.next().unwrap();
        }
        let mut push = |lexer: &Lexer<'_>| {
            seen.push(format!("{} {}", name(lexer.token), String::from_utf8_lossy(lexer.raw())))
        };
        for _ in 0..5 {
            lexer.next_inside_jsx_element().unwrap();
            push(&lexer);
        }
        lexer.next_jsx_element_child().unwrap();
        push(&lexer);
        lexer.next_jsx_element_child().unwrap();
        push(&lexer);
        for _ in 0..3 {
            lexer.next_inside_jsx_element().unwrap();
            push(&lexer);
        }
        lexer.next().unwrap();
        push(&lexer);
    }
    assert!(!log.has_any(), "o lexer registrou mensagens");
    assert_eq!(
        seen,
        [
            "TIdentifier div",
            "TIdentifier className",
            "TEquals =",
            "TStringLiteral \"a\"",
            "TGreaterThan >",
            "TStringLiteral hi",
            "TLessThan <",
            "TSlash /",
            "TIdentifier div",
            "TGreaterThan >",
            "TEndOfFile ",
        ]
    );
}

/// Pragmas de JSX e o `sourceMappingURL` viram faixas do fonte (a `StoreStr` do argumento é o deslocamento dele).
#[test]
fn pragma_args_are_source_ranges() {
    let text = "/** @jsxImportSource preact */\nlet a = 1;\n//# sourceMappingURL=a.js.map\n";
    let source = Source::init_path_string(b"caso.tsx".as_slice(), text.as_bytes());
    let arena = Arena::default();
    let mut log = Log::default();
    let mut lexer = Lexer::init_without_reading(&mut log, &source, &arena);
    lexer.step();
    lexer.next().unwrap();
    while lexer.token != T::TEndOfFile {
        lexer.next().unwrap();
    }
    let span = lexer.jsx_pragma._jsx_import_source;
    let r = span.text;
    let at = text.find("preact").unwrap();
    assert_eq!(r, bun_ast::StoreStr::from_range(at as u32, 6));
    // O `range` do argumento segue a conta do upstream (`self.start + offset_for_errors + ...`), que não é o
    // deslocamento do texto; só serve às mensagens de erro, e por isso o texto não sai dele.
    let url = lexer.source_mapping_url.expect("sourceMappingURL");
    let at = text.find("a.js.map").unwrap();
    assert_eq!(url.text, bun_ast::StoreStr::from_range(at as u32, 8));
}

#[test]
fn numbers_and_strings() {
    let source = Source::init_path_string(b"caso.ts".as_slice(), b"0x1F 1_000 .5e1 'a\\u00e9' 10n".as_slice());
    let arena = Arena::default();
    let mut log = Log::default();
    let mut store = bun_ast::Store::default();
    let mut lexer = Lexer::init_without_reading(&mut log, &source, &arena);
    lexer.step();
    lexer.next().unwrap();
    assert_eq!(lexer.number, 31.0);
    lexer.next().unwrap();
    assert_eq!(lexer.number, 1000.0);
    lexer.next().unwrap();
    assert_eq!(lexer.number, 5.0);
    lexer.next().unwrap();
    assert_eq!(lexer.token, T::TStringLiteral);
    let s = lexer.to_utf8_e_string(&mut store).unwrap();
    assert_eq!(bun_ast::NodeStore::str(&store, s.data), "aé".as_bytes());
    lexer.next().unwrap();
    assert_eq!(lexer.token, T::TBigIntegerLiteral);
    assert_eq!(lexer.identifier, b"10");
}
