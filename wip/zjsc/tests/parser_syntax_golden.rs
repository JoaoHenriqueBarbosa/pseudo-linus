//! Golden de SyntaxError do parser contra o bun 1.4.2 (`scripts/gen-syntax-errors-golden.js`):
//! cada linha é `fonte (JSON) <TAB> "ok" ou a mensagem do SyntaxError`.

use zjsc::parser::nodes::ProgramNode;
use zjsc::parser::parser::parse_root_node;
use zjsc::parser::parser_error::ParserError;
use zjsc::parser::parser_modes::{
    JSParserBuiltinMode, JSParserScriptMode, SourceParseMode, NO_LEXICALLY_SCOPED_FEATURES,
};
use zjsc::parser::source_code::make_source;
use zjsc::parser::source_tainted_origin::SourceTaintedOrigin;
use zjsc::runtime::constructor_kind::ConstructorKind;
use zjsc::runtime::implementation_visibility::ImplementationVisibility;
use zjsc::runtime::source_origin::SourceOrigin;
use zjsc::runtime::vm::VM;
use zjsc::wtf::text::text_position::TextPosition;
use zjsc::wtf::text::conversion_mode::ConversionMode;
use zjsc::wtf::text::wtf_string::String as WtfString;
use zjsc::parser::source_provider::SourceProviderSourceType;
use std::rc::Rc;

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

fn parse(vm: &Rc<VM>, source: &[u16]) -> String {
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
        Some(_) => "ok".to_string(),
        None => String::from_utf8(error.message().utf8(ConversionMode::LenientConversion)).unwrap(),
    }
}

#[test]
fn syntax_errors_match_bun() {
    let vm = Rc::new(VM::new());
    // Depuração: PARSER_SRC='a in' mostra o resultado de um trecho avulso.
    if let Some(src) = std::env::var_os("PARSER_SRC") {
        let units: Vec<u16> = src.to_str().unwrap().encode_utf16().collect();
        eprintln!("RESULT {:?}", parse(&vm, &units));
    }
    let golden = include_str!("golden/syntax-errors.tsv");
    let mut failures = Vec::new();
    for (line_number, line) in golden.lines().enumerate() {
        let (quoted, expected) = line.split_once('\t').unwrap();
        if let Some(only) = std::env::var_os("PARSER_ONLY") {
            if only.to_str() != Some(&(line_number + 1).to_string()) {
                continue;
            }
        }
        if std::env::var_os("PARSER_TRACE").is_some() {
            eprintln!("{}", line_number + 1);
        }
        let got = parse(&vm, &unquote(quoted));
        if got != expected {
            failures.push(format!("linha {}: {}: esperado {:?}, obtido {:?}", line_number + 1, quoted, expected, got));
        }
    }
    assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
}
