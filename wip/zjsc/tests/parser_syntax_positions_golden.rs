//! Golden da linha do SyntaxError do parser contra o bun (`scripts/gen-syntax-error-positions-golden.js`):
//! cada linha é `fonte (JSON) <TAB> linha <TAB> coluna`, com "-" nos trechos aceitos.
//!
//! O bun só expõe a linha (`addErrorInfo` grava `line` e zera a coluna), então a coluna do golden
//! não é comparada: `error.token().start_position` fica coberto apenas pela linha dele.

use std::rc::Rc;
use zjsc::parser::nodes::ProgramNode;
use zjsc::parser::parser::parse_root_node;
use zjsc::parser::parser_error::ParserError;
use zjsc::parser::parser_modes::{
    JSParserBuiltinMode, JSParserScriptMode, SourceParseMode, NO_LEXICALLY_SCOPED_FEATURES,
};
use zjsc::parser::source_code::make_source;
use zjsc::parser::source_provider::SourceProviderSourceType;
use zjsc::parser::source_tainted_origin::SourceTaintedOrigin;
use zjsc::runtime::constructor_kind::ConstructorKind;
use zjsc::runtime::implementation_visibility::ImplementationVisibility;
use zjsc::runtime::source_origin::SourceOrigin;
use zjsc::runtime::vm::VM;
use zjsc::wtf::text::text_position::TextPosition;
use zjsc::wtf::text::wtf_string::String as WtfString;

/// Desfaz o `JSON.stringify` do gerador (escapes `\n`, `\"`, `\\`, `\uXXXX`).
fn unquote(quoted: &str) -> Vec<u16> {
    let inner: Vec<u16> = quoted[1..quoted.len() - 1].encode_utf16().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < inner.len() {
        if inner[i] != b'\\' as u16 {
            out.push(inner[i]);
            i += 1;
            continue;
        }
        i += 1;
        match char::from_u32(inner[i] as u32).unwrap() {
            'n' => out.push(10),
            't' => out.push(9),
            'b' => out.push(8),
            'r' => out.push(13),
            'f' => out.push(12),
            'u' => {
                let hex: String = inner[i + 1..i + 5].iter().map(|&c| c as u8 as char).collect();
                out.push(u16::from_str_radix(&hex, 16).unwrap());
                i += 4;
            }
            other => out.push(other as u16),
        }
        i += 1;
    }
    out
}

/// Linha do erro como o bun a expõe, ou "-" quando o trecho é aceito.
fn error_line(vm: &Rc<VM>, source: &[u16]) -> String {
    let code = make_source(
        &WtfString::from_utf16(source),
        &SourceOrigin::default(),
        SourceTaintedOrigin::Untainted,
        WtfString::default(),
        TextPosition::default(),
        SourceProviderSourceType::Program,
    );
    let mut error = ParserError::new();
    let root = parse_root_node::<ProgramNode>(
        vm,
        &code,
        ImplementationVisibility::Public,
        JSParserBuiltinMode::NotBuiltin,
        NO_LEXICALLY_SCOPED_FEATURES,
        JSParserScriptMode::Classic,
        SourceParseMode::ProgramMode,
        &mut error,
        ConstructorKind::None,
        None,
        None,
    );
    match root {
        Some(_) => "-".to_string(),
        None => error.line().to_string(),
    }
}

#[test]
fn syntax_error_lines_match_bun() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/syntax-error-positions.tsv");
    let golden = std::fs::read_to_string(path).unwrap_or_else(|_| {
        panic!("golden ausente: gere com `bun scripts/gen-syntax-error-positions-golden.js > tests/golden/syntax-error-positions.tsv`")
    });
    let vm = Rc::new(VM::new());
    let mut failures = Vec::new();
    for (line_number, line) in golden.lines().enumerate() {
        let mut fields = line.split('\t');
        let quoted = fields.next().unwrap();
        let expected = fields.next().unwrap();
        let got = error_line(&vm, &unquote(quoted));
        if got != expected {
            failures.push(format!("linha {}: {}: esperado {:?}, obtido {:?}", line_number + 1, quoted, expected, got));
        }
    }
    assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
}
